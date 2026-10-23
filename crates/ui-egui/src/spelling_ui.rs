//! Spelling uses one background check at a time; stale document snapshots are never displayed.
use crate::DesignApp;
use designcraft_doc::{Document, StoryId};
use serde_json::{Value, json};
use std::ops::Range;
use std::sync::{Arc, Mutex, mpsc};

type Key = (u64, u64);
#[derive(Default)]
struct State {
    #[cfg(not(target_arch = "wasm32"))]
    running: Option<(Key, mpsc::Receiver<Report>)>,
    #[cfg(target_arch = "wasm32")]
    pending: Option<(Key, IncrementalReport)>,
    cached: Option<(Key, Arc<Report>)>,
}
pub(crate) struct Report {
    pub doc: Arc<Document>,
    pub words: Vec<(StoryId, Range<usize>)>,
    pub error: Option<String>,
}
#[cfg(not(target_arch = "wasm32"))]
fn build(doc: Arc<Document>) -> Report {
    let mut words = Vec::new();
    for story in doc.stories.values() {
        words.extend(designcraft_engine::cmd::spelling::misspellings(&doc, story, &doc.user_words).into_iter().map(|r| (story.id, r)));
    }
    Report { doc, words, error: None }
}
// The browser has no native worker thread. Keep the scan resumable even inside
// one long story, and expose only complete results for the current revision.
#[cfg(any(target_arch = "wasm32", test))]
struct IncrementalReport {
    report: Report,
    story: Option<StoryId>,
    cursor: designcraft_engine::cmd::spelling::SpellingCursor,
    last_frame: Option<u64>,
}
#[cfg(any(target_arch = "wasm32", test))]
impl IncrementalReport {
    fn new(doc: Arc<Document>) -> Self {
        let story = doc.stories.keys().next().copied();
        Self { report: Report { doc, words: vec![], error: None }, story, cursor: Default::default(), last_frame: None }
    }
    fn advance(&mut self, frame: u64) -> bool {
        // web_time uses Performance.now() in browsers; std::time::Instant panics
        // on wasm32-unknown-unknown. A single lookup/initialization is indivisible.
        let start = web_time::Instant::now();
        self.advance_until(frame, || start.elapsed() >= std::time::Duration::from_millis(2))
    }
    fn advance_until(&mut self, frame: u64, mut expired: impl FnMut() -> bool) -> bool {
        if self.story.is_none() {
            return true;
        }
        // The dialog and canvas can ask in the same frame: spend the budget once.
        if self.last_frame == Some(frame) {
            return false;
        }
        self.last_frame = Some(frame);
        if let Some(sid) = self.story {
            let mut words = Vec::new();
            let mut done = false;
            // At most 16,384 character/run steps and 1,024 completed words per
            // frame, even if the clock has low resolution. Check time after each
            // small chunk so slower devices yield without spending that cap.
            for _ in 0..128 {
                done = self.report.doc.story(sid).is_none_or(|story| self.cursor.advance(&self.report.doc, story, 128, 8, &mut words));
                if done || expired() {
                    break;
                }
            }
            self.report.words.extend(words.into_iter().map(|r| (sid, r)));
            if done {
                self.story = self.report.doc.stories.range((std::ops::Bound::Excluded(sid), std::ops::Bound::Unbounded)).next().map(|(id, _)| *id);
                self.cursor = Default::default();
            }
        }
        self.story.is_none()
    }
}
pub(crate) fn report(app: &DesignApp, ctx: &egui::Context) -> Option<Arc<Report>> {
    let active = app.session.active()?;
    let key = (active.uid, active.revision);
    let cell: Arc<Mutex<State>> = ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<State>>>(egui::Id::new("spelling_worker")).clone());
    let mut state = cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((job_key, rx)) = &state.running {
        match rx.try_recv() {
            Ok(result) => {
                state.cached = Some((*job_key, Arc::new(result)));
                state.running = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                state.running = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    if let Some((cached_key, result)) = &state.cached
        && *cached_key == key
    {
        return Some(result.clone());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if state.running.is_none() {
        let doc = active.doc.clone();
        let (tx, rx) = mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let repaint = ctx.clone();
            let failed_doc = doc.clone();
            let spawn = std::thread::Builder::new().name("spelling".into()).spawn(move || {
                let fallback = doc.clone();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(doc))).unwrap_or_else(|_| Report {
                    doc: fallback,
                    words: vec![],
                    error: Some("Spelling check failed".into()),
                });
                let _ = tx.send(result);
                repaint.request_repaint();
            });
            if let Err(e) = spawn {
                let result = Arc::new(Report { doc: failed_doc, words: vec![], error: Some(e.to_string()) });
                state.cached = Some((key, result.clone()));
                return Some(result);
            }
        }
        state.running = Some((key, rx));
    }
    #[cfg(target_arch = "wasm32")]
    {
        if state.pending.as_ref().is_none_or(|(pending_key, _)| *pending_key != key) {
            state.pending = Some((key, IncrementalReport::new(active.doc.clone())));
        }
        if state.pending.as_mut().is_some_and(|(_, work)| work.advance(ctx.cumulative_frame_nr()))
            && let Some((finished_key, work)) = state.pending.take()
        {
            let result = Arc::new(work.report);
            state.cached = Some((finished_key, result.clone()));
            return Some(result);
        }
        ctx.request_repaint();
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    None
}

type SuggestionKey = (u64, u64, u64, usize, usize);
#[derive(Default)]
struct SuggestionState {
    running: Option<(SuggestionKey, mpsc::Receiver<Vec<String>>)>,
    cached: Option<(SuggestionKey, Vec<String>)>,
}
fn suggestions_async(ctx: &egui::Context, key: SuggestionKey, doc: Arc<Document>, sid: StoryId, range: Range<usize>) -> Option<Vec<String>> {
    let cell: Arc<Mutex<SuggestionState>> =
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<SuggestionState>>>(egui::Id::new("spelling_suggestions")).clone());
    let mut state = cell.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((job_key, rx)) = &state.running {
        match rx.try_recv() {
            Ok(result) => {
                state.cached = Some((*job_key, result));
                state.running = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                state.running = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    if let Some((cached_key, result)) = &state.cached
        && *cached_key == key
    {
        return Some(result.clone());
    }
    if state.running.is_none() {
        let (tx, rx) = mpsc::channel();
        let work = move || doc.story(sid).map(|story| designcraft_engine::cmd::spelling::suggestions(&doc, story, range)).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let repaint = ctx.clone();
            if std::thread::Builder::new()
                .name("spelling-suggestions".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_default();
                    let _ = tx.send(result);
                    repaint.request_repaint();
                })
                .is_err()
            {
                return Some(vec![]);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = tx.send(work());
        }
        state.running = Some((key, rx));
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    None
}

pub(crate) fn open(app: &mut DesignApp) -> Result<Value, String> {
    let uid = app.session.active().ok_or("no document open")?.uid;
    app.ui.dialog = Some(crate::dialogs::Dialog::new("spelling", json!({"uid": uid, "index": 0})));
    Ok(Value::Null)
}

// Reports follow the document's BTreeMap story order. Keep skipped occurrences behind
// the cursor when this dialog changes a word; unrelated revisions restart the check.
fn resume_index(words: &[(StoryId, Range<usize>)], sid: StoryId, byte: usize) -> usize {
    words.iter().position(|(story, r)| *story > sid || (*story == sid && r.start >= byte)).unwrap_or(words.len())
}

pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.ui.dialog.clone() else { return };
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let mut open = true;
    let mut action = None;
    let result = report(app, ctx);
    egui::Window::new(tr("Check Spelling"))
        .id(egui::Id::new("spelling_dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| {
            if app.session.active().is_none_or(|s| Some(s.uid) != dialog.fields.get("uid").and_then(Value::as_u64)) {
                ui.label(tr("The document has changed. Reopen the spelling checker."));
                return;
            }
            let Some(result) = &result else {
                ui.spinner();
                ui.label(tr("Checking spelling…"));
                return;
            };
            if let Some(error) = &result.error {
                ui.label(error);
                return;
            }
            let revision = app.session.active().map(|s| s.revision);
            if dialog.fields.get("revision").and_then(Value::as_u64) != revision {
                dialog.fields.insert("revision".into(), json!(revision));
                let resume = dialog.fields.remove("resume");
                let index = resume
                    .as_ref()
                    .filter(|v| v["revision"].as_u64() == revision)
                    .and_then(|v| Some((StoryId(v["story"].as_u64()?), usize::try_from(v["byte"].as_u64()?).ok()?)))
                    .map_or(0, |(sid, byte)| resume_index(&result.words, sid, byte));
                dialog.fields.insert("index".into(), json!(index));
                dialog.fields.remove("suggestionKey");
            }
            let index = dialog.fields.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let Some((sid, range)) = result.words.get(index) else {
                ui.label(tr("Spelling check complete."));
                ui.label(tr("Only text set to English or Hungarian is checked."));
                return;
            };
            let Some(story) = result.doc.story(*sid) else { return };
            let Some(word) = story.text.get(range.clone()) else { return };
            ui.label(format!("{} / {}", index + 1, result.words.len()));
            ui.heading(word);
            let key = json!([sid.0, range.start, range.end]);
            if dialog.fields.get("suggestionKey") != Some(&key) {
                let uid = dialog.fields.get("uid").and_then(Value::as_u64).unwrap_or(0);
                let job_key = (uid, revision.unwrap_or(0), sid.0, range.start, range.end);
                let Some(list) = suggestions_async(ctx, job_key, result.doc.clone(), *sid, range.clone()) else {
                    ui.spinner();
                    ui.label(tr("Checking spelling…"));
                    return;
                };
                dialog.fields.insert("to".into(), json!(list.first().map(String::as_str).unwrap_or(word)));
                dialog.fields.insert("suggestions".into(), json!(list));
                dialog.fields.insert("suggestionKey".into(), key);
            }
            if let Some(list) = dialog.fields.get("suggestions").and_then(Value::as_array).cloned() {
                for item in list.iter().filter_map(Value::as_str) {
                    let selected = dialog.fields.get("to").and_then(Value::as_str) == Some(item);
                    if ui.selectable_label(selected, item).clicked() {
                        dialog.fields.insert("to".into(), json!(item));
                    }
                }
            }
            ui.label(tr("Change to:"));
            let mut to = dialog.fields.get("to").and_then(Value::as_str).unwrap_or(word).to_string();
            if ui.text_edit_singleline(&mut to).changed() {
                dialog.fields.insert("to".into(), json!(to));
            }
            ui.horizontal(|ui| {
                if ui.button(tr("Change")).clicked() {
                    action = Some((
                        "spelling.change",
                        json!({"story": sid.0, "start": range.start, "end": range.end, "to": to, "expected": word}),
                        *sid,
                        range.start.saturating_add(to.len()),
                    ));
                }
                if ui.button(tr("Add to Dictionary")).clicked() {
                    action = Some(("spelling.addWord", json!({"word": word}), *sid, range.end));
                }
                if ui.button(tr("Skip")).clicked() {
                    dialog.fields.insert("index".into(), json!(index + 1));
                }
            });
            if let Some(error) = dialog.fields.get("error").and_then(Value::as_str) {
                ui.colored_label(egui::Color32::RED, error);
            }
            ui.label(tr("The text language is set in the Character panel."));
        });
    if let Some((command, params, sid, byte)) = action {
        // Both the document and its revision must still match the checked snapshot.
        let fresh = app.session.active().is_some_and(|s| {
            Some(s.uid) == dialog.fields.get("uid").and_then(Value::as_u64)
                && Some(s.revision) == dialog.fields.get("revision").and_then(Value::as_u64)
        });
        if fresh {
            match app.run(command, params) {
                Ok(_) => {
                    dialog.fields.insert("resume".into(), json!({"story":sid.0,"byte":byte,"revision":app.session.active().map(|s|s.revision)}));
                    dialog.fields.remove("error");
                }
                Err(e) => {
                    dialog.fields.insert("error".into(), json!(e));
                }
            }
        } else {
            dialog.fields.insert("error".into(), json!(tr("The document has changed. Reopen the spelling checker.")));
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    app.ui.dialog = open.then_some(dialog);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn checked(app: &DesignApp, ctx: &egui::Context) -> Arc<Report> {
        let start = std::time::Instant::now();
        loop {
            if let Some(r) = report(app, ctx) {
                return r;
            }
            assert!(start.elapsed().as_secs() < 15, "background spelling timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[test]
    fn browser_scan_yields_once_per_frame_and_keeps_complete_results() {
        let mut app = DesignApp::new(SessionForTest::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect":[0,0,300,300],"content":"text","text":"qzxw ".repeat(2400)})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let expected = build(doc.clone()).words;
        let mut work = IncrementalReport::new(doc);
        assert!(!work.advance_until(0, || false));
        let first = work.report.words.len();
        assert_eq!(first, 1024, "fast scans should use the larger per-frame word budget");
        assert!(!work.advance_until(0, || false));
        assert_eq!(work.report.words.len(), first, "two callers must share the frame budget");
        let mut finished = false;
        for frame in 1..1000 {
            if work.advance(frame) {
                finished = true;
                break;
            }
        }
        assert!(finished);
        assert_eq!(work.report.words, expected);
    }
    #[test]
    fn browser_scan_deadline_yields_and_preserves_results() {
        let mut session = SessionForTest::new();
        session.execute("file.new", &json!({})).unwrap();
        let text = "hejesírás árvi\u{301}z qzxw 2026-ban ".repeat(100);
        let mut doc = (*session.doc().unwrap().doc).clone();
        let mut story = designcraft_doc::Story::with_text(StoryId(1), &text, Default::default());
        story.format_chars(0..text.len(), |f| f.over.language = Some("Hungarian".into()));
        doc.stories.insert(StoryId(1), story.into());
        let doc = Arc::new(doc);
        let expected = build(doc.clone()).words;
        let mut work = IncrementalReport::new(doc);
        let mut checks = 0;
        assert!(!work.advance_until(0, || {
            checks += 1;
            true
        }));
        assert_eq!(checks, 1, "an expired deadline must prevent a second chunk");
        let first = work.report.words.len();
        assert!(first > 0 && first <= 8);
        assert!(!work.advance_until(0, || panic!("same frame must not spend more time")));
        assert_eq!(work.report.words.len(), first);
        for frame in 1..1000 {
            if work.advance_until(frame, || true) {
                break;
            }
        }
        assert!(work.story.is_none());
        assert_eq!(work.report.words, expected);
    }
    #[test]
    fn browser_scan_work_cap_bounds_correct_words_and_punctuation() {
        for text in ["hello ".repeat(5000), ".".repeat(50_000), "a".repeat(50_000)] {
            let mut session = SessionForTest::new();
            session.execute("file.new", &json!({})).unwrap();
            let mut doc = (*session.doc().unwrap().doc).clone();
            doc.stories.insert(StoryId(1), designcraft_doc::Story::with_text(StoryId(1), &text, Default::default()).into());
            let doc = Arc::new(doc);
            let expected = build(doc.clone()).words;
            let mut work = IncrementalReport::new(doc);
            let mut chunks = 0;
            assert!(!work.advance_until(0, || {
                chunks += 1;
                false
            }));
            assert_eq!(chunks, 128, "work is capped even when the clock does not advance");
            for frame in 1..100 {
                if work.advance_until(frame, || false) {
                    break;
                }
            }
            assert!(work.story.is_none());
            assert_eq!(work.report.words, expected);
        }
    }
    /// Native timing of the browser's resumable scheduler, not a browser FPS test.
    #[test]
    #[ignore = "manual spelling scheduler timing; run with --nocapture"]
    fn browser_scan_timing() {
        for (name, language, sample, repeats) in [
            ("english-50k", "English", "hello world qzxw this is a test. ", 1600),
            ("hungarian-50k", "Hungarian", "Az asszony könnyen olvas hosszú szöveget. hejesírás 2026-ban. ", 850),
            ("hungarian-book", "Hungarian", "Az asszony könnyen olvas hosszú szöveget. hejesírás 2026-ban. ", 8500),
        ] {
            let mut session = SessionForTest::new();
            session.execute("file.new", &json!({})).unwrap();
            let text = sample.repeat(repeats);
            let mut doc = (*session.doc().unwrap().doc).clone();
            let mut story = designcraft_doc::Story::with_text(StoryId(1), &text, Default::default());
            story.format_chars(0..text.len(), |f| f.over.language = Some(language.into()));
            doc.stories.insert(StoryId(1), story.into());
            let doc = Arc::new(doc);
            let mut cold = IncrementalReport::new(doc.clone());
            let cold_start = std::time::Instant::now();
            cold.advance(0);
            let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;
            let expected = build(doc.clone()).words;
            let mut scan = IncrementalReport::new(doc);
            let mut times = Vec::new();
            for frame in 0..100_000 {
                let start = std::time::Instant::now();
                let done = scan.advance(frame);
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                if done {
                    break;
                }
            }
            assert!(scan.story.is_none());
            assert_eq!(scan.report.words, expected);
            times.sort_by(f64::total_cmp);
            println!(
                "SCAN_TIMING {}",
                json!({"case":name, "characters":text.chars().count(),
                "words":text.split_whitespace().count(), "frames":times.len(), "first_call_ms":cold_ms,
                "total_work_ms":times.iter().sum::<f64>(), "p95_frame_ms":times[times.len()*95/100],
                "max_frame_ms":times.last(), "misspellings":expected.len()})
            );
        }
    }
    #[test]
    fn background_cache_tracks_user_words_and_language_changes() {
        let mut app = DesignApp::new(SessionForTest::new(), crate::Services::default());
        let ctx = egui::Context::default();
        app.run("file.new", json!({})).unwrap();
        let r = app.run("frame.create", json!({"rect":[36,36,400,200],"content":"text","text":"hejesírás qzxw"})).unwrap();
        app.run("text.select", json!({"story":r["story"],"anchor":0,"focus":"hejesírás qzxw".len()})).unwrap();
        app.run("type.char", json!({"attrs":{"language":"Hungarian"}})).unwrap();
        assert_eq!(checked(&app, &ctx).words.len(), 2);
        app.run("spelling.setWords", json!({"words":["hejesírás"]})).unwrap();
        let first = checked(&app, &ctx);
        assert_eq!(first.words.len(), 1);
        app.run("spelling.setWords", json!({"words":["qzxw"]})).unwrap();
        let second = checked(&app, &ctx);
        assert_eq!(second.words.len(), 1);
        assert_ne!(first.words[0].1, second.words[0].1, "same-sized user dictionaries must invalidate the cache");
        app.run("type.char", json!({"attrs":{"language":"German"}})).unwrap();
        assert!(checked(&app, &ctx).words.is_empty());
    }
    #[test]
    fn spelling_cursor_does_not_revisit_skipped_occurrences_after_its_own_edit() {
        let words = vec![(StoryId(9), 0..5), (StoryId(9), 16..20), (StoryId(11), 0..4)];
        assert_eq!(resume_index(&words, StoryId(9), 15), 1);
        assert_eq!(resume_index(&words, StoryId(9), 25), 2);
        assert_eq!(resume_index(&words, StoryId(11), 4), 3);
    }
    use designcraft_engine::Session as SessionForTest;
}
