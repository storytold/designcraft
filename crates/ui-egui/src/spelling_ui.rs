//! Spelling uses one check at a time; unchanged stories keep their last valid underlines.
use crate::DesignApp;
use designcraft_doc::{Document, StoryId};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, mpsc};

type Key = (u64, u64);
#[derive(Default)]
struct State {
    #[cfg(not(target_arch = "wasm32"))]
    running: Option<(Key, mpsc::Receiver<Report>)>,
    #[cfg(any(target_arch = "wasm32", test))]
    pending: Option<(Key, IncrementalReport)>,
    cached: Option<(Key, Arc<Report>)>,
}
pub(crate) struct Report {
    pub doc: Arc<Document>,
    pub words: Vec<(StoryId, Range<usize>)>,
    pub error: Option<String>,
    by_story: BTreeMap<StoryId, Vec<Range<usize>>>,
}
impl Report {
    /// Create a report and index its misspellings by story.
    fn new(doc: Arc<Document>, words: Vec<(StoryId, Range<usize>)>, error: Option<String>) -> Self {
        let mut report = Self { doc, words, error, by_story: BTreeMap::new() };
        report.index_words();
        report
    }
    /// Rebuild the story index after collecting a complete scan.
    fn index_words(&mut self) {
        self.by_story.clear();
        for (sid, range) in &self.words {
            self.by_story.entry(*sid).or_default().push(range.clone());
        }
    }
    /// Reuse offsets only when the story, styles and user dictionary still match.
    pub(crate) fn story_words(&self, doc: &Document, sid: StoryId) -> Option<&[Range<usize>]> {
        if !Arc::ptr_eq(&self.doc.styles, &doc.styles) || self.doc.user_words != doc.user_words {
            return None;
        }
        let (old, current) = self.doc.stories.get(&sid).zip(doc.stories.get(&sid))?;
        Arc::ptr_eq(old, current).then(|| self.by_story.get(&sid).map(Vec::as_slice).unwrap_or(&[]))
    }
}
#[cfg(not(target_arch = "wasm32"))]
/// Check all stories off the UI thread, stopping with any dictionary-load error.
fn build(doc: Arc<Document>) -> Report {
    let mut words = Vec::new();
    for story in doc.stories.values() {
        if let Err(error) = designcraft_engine::cmd::spelling::ensure_story_dictionary(&doc, story) {
            return Report::new(doc, vec![], Some(error.into()));
        }
        words.extend(designcraft_engine::cmd::spelling::misspellings(&doc, story, &doc.user_words).into_iter().map(|r| (story.id, r)));
    }
    Report::new(doc, words, None)
}
// The browser has no native worker thread. Keep the scan resumable even inside
// one long story, and expose only complete results for the current revision.
#[cfg(any(target_arch = "wasm32", test))]
struct IncrementalReport {
    report: Report,
    story: Option<StoryId>,
    cursor: designcraft_engine::cmd::spelling::SpellingCursor,
    last_frame: Option<u64>,
    validated: bool,
}
#[cfg(any(target_arch = "wasm32", test))]
impl IncrementalReport {
    /// Start a resumable scan over an immutable document snapshot.
    fn new(doc: Arc<Document>) -> Self {
        let story = doc.stories.keys().next().copied();
        Self { report: Report::new(doc, vec![], None), story, cursor: Default::default(), last_frame: None, validated: false }
    }
    /// Spend one browser frame's bounded scan budget.
    fn advance(&mut self, frame: u64) -> bool {
        // web_time uses Performance.now() in browsers; std::time::Instant panics
        // on wasm32-unknown-unknown. A single lookup/initialization is indivisible.
        let start = web_time::Instant::now();
        self.advance_until(frame, || start.elapsed() >= std::time::Duration::from_millis(2))
    }
    /// Resume at most once per frame and yield when the time or work budget expires.
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
            if !self.validated {
                if let Some(error) = self
                    .report
                    .doc
                    .story(sid)
                    .and_then(|story| designcraft_engine::cmd::spelling::ensure_story_dictionary(&self.report.doc, story).err())
                {
                    self.report.error = Some(error.into());
                    self.report.words.clear();
                    self.story = None;
                    return true;
                }
                self.validated = true;
            }
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
                self.validated = false;
            }
        }
        if self.story.is_none() {
            self.report.index_words();
        }
        self.story.is_none()
    }
}
#[cfg(any(target_arch = "wasm32", test))]
impl State {
    /// Advance the pending snapshot and schedule another frame after stale completion.
    fn advance_browser(&mut self, key: Key, doc: Arc<Document>, ctx: &egui::Context) -> Option<Arc<Report>> {
        if self.pending.as_ref().is_none_or(|(pending_key, _)| pending_key.0 != key.0) {
            self.pending = Some((key, IncrementalReport::new(doc)));
        }
        if self.pending.as_mut().is_some_and(|(_, work)| work.advance(ctx.cumulative_frame_nr()))
            && let Some((finished_key, work)) = self.pending.take()
        {
            let result = Arc::new(work.report);
            self.cached = Some((finished_key, result.clone()));
            if finished_key == key {
                return Some(result);
            }
        }
        // A stale completion is not the current result. The dialog may be the
        // only caller, so it must get another frame to start the current scan.
        ctx.request_repaint();
        None
    }
}

/// Return the current report or safe cached underlines while scheduling a fresh scan.
pub(crate) fn report(app: &DesignApp, ctx: &egui::Context) -> Option<Arc<Report>> {
    let active = app.session.active()?;
    let key = (active.uid, active.revision);
    let cell: Arc<Mutex<State>> = ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<State>>>(egui::Id::unique("spelling_worker")).clone());
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
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(doc)))
                    .unwrap_or_else(|_| Report::new(fallback, vec![], Some("Spelling check failed".into())));
                let _ = tx.send(result);
                repaint.request_repaint();
            });
            if let Err(e) = spawn {
                let result = Arc::new(Report::new(failed_doc, vec![], Some(e.to_string())));
                state.cached = Some((key, result.clone()));
                return Some(result);
            }
        }
        state.running = Some((key, rx));
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(result) = state.advance_browser(key, active.doc.clone(), ctx) {
        return Some(result);
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    state.cached.as_ref().filter(|(cached_key, _)| cached_key.0 == key.0).map(|(_, result)| result.clone())
}

type SuggestionKey = (u64, u64, u64, usize, usize);
#[derive(Default)]
struct SuggestionState {
    running: Option<(SuggestionKey, mpsc::Receiver<Vec<String>>)>,
    cached: Option<(SuggestionKey, Vec<String>)>,
}
/// Generate suggestions outside the native UI thread and cache only the matching request.
fn suggestions_async(ctx: &egui::Context, key: SuggestionKey, doc: Arc<Document>, sid: StoryId, range: Range<usize>) -> Option<Vec<String>> {
    let cell: Arc<Mutex<SuggestionState>> =
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Arc<Mutex<SuggestionState>>>(egui::Id::unique("spelling_suggestions")).clone());
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

/// Open spelling for the active document after releasing any previous dialog scope.
pub(crate) fn open(app: &mut DesignApp) -> Result<Value, String> {
    app.session.active().ok_or("no document open")?;
    crate::dialogs::cancel(app);
    let uid = app.session.active().ok_or("no document open")?.uid;
    app.ui.dialog = Some(crate::dialogs::Dialog::new("spelling", json!({"uid": uid, "index": 0})));
    Ok(Value::Null)
}

// Reports follow the document's BTreeMap story order. Keep skipped occurrences behind
// the cursor when this dialog changes a word; unrelated revisions restart the check.
/// Find the first occurrence at or after an accepted spelling replacement.
fn resume_index(words: &[(StoryId, Range<usize>)], sid: StoryId, byte: usize) -> usize {
    words.iter().position(|(story, r)| *story > sid || (*story == sid && r.start >= byte)).unwrap_or(words.len())
}

/// Render spelling controls only for the current snapshot and apply requested corrections.
pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.ui.dialog.clone() else { return };
    let lang = app.ui.language.clone();
    let tr = |s: &'static str| crate::i18n::tr(&lang, s);
    let mut open = true;
    let mut action = None;
    let result = report(app, ctx);
    egui::Window::new(tr("Check Spelling"))
        .id(egui::Id::unique("spelling_dialog"))
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
            if app.session.active().is_none_or(|active| !Arc::ptr_eq(&result.doc, &active.doc)) {
                ui.spinner();
                ui.label(tr("Checking spelling…"));
                return;
            }
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
    /// Require explicit selection of the verified Hungarian pack for these opt-in integration tests.
    fn require_hu_pack() {
        assert!(
            option_env!("CRAFT_PROOF_EMBED_PACKS").is_some_and(|locales| locales.split(',').any(|locale| locale.trim() == "hu")),
            "select the verified hu Language pack before explicitly running this test"
        );
    }

    /// Build a completed spelling snapshot for scanner and dialog regressions.
    fn checked(app: &DesignApp, ctx: &egui::Context) -> Arc<Report> {
        let start = std::time::Instant::now();
        loop {
            if let Some(r) = report(app, ctx)
                && app.session.active().is_some_and(|active| Arc::ptr_eq(&r.doc, &active.doc))
            {
                return r;
            }
            assert!(start.elapsed().as_secs() < 15, "background spelling timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    /// Regression: missing hungarian pack is an error in both scanners.
    #[test]
    fn missing_hungarian_pack_is_an_error_in_both_scanners() {
        let mut session = SessionForTest::new();
        session.execute("file.new", &json!({})).unwrap();
        let mut doc = (*session.doc().unwrap().doc).clone();
        doc.stories.insert(StoryId(1), designcraft_doc::Story::with_text(StoryId(1), "qzxw", Default::default()).into());
        let english = build(Arc::new(doc.clone()));
        assert!(english.error.is_none());
        assert!(!english.words.is_empty());
        let mut hu = designcraft_doc::Story::with_text(StoryId(2), "hejesírás", Default::default());
        hu.format_chars(0..hu.text.len(), |f| f.over.language = Some("Hungarian".into()));
        if designcraft_engine::cmd::spelling::ensure_story_dictionary(&doc, &hu).is_ok() {
            return; // Exercised by the explicit no-pack test configuration.
        }
        doc.stories.insert(StoryId(2), hu.into());
        let doc = Arc::new(doc);
        let native = build(doc.clone());
        assert!(native.error.is_some());
        assert!(native.words.is_empty());
        assert!(native.by_story.is_empty());
        let mut browser = IncrementalReport::new(doc);
        assert!(!browser.advance_until(0, || false));
        assert!(!browser.report.words.is_empty(), "positive English scan before the missing pack");
        assert!(browser.advance_until(1, || false));
        assert_eq!(browser.report.error, native.error);
        assert!(browser.report.words.is_empty());
        assert!(browser.report.by_story.is_empty());
    }

    /// Regression: cached underlines are reused only for unchanged stories and settings.
    #[test]
    fn cached_underlines_are_reused_only_for_unchanged_stories_and_settings() {
        let mut session = SessionForTest::new();
        session.execute("file.new", &json!({})).unwrap();
        let mut doc = (*session.doc().unwrap().doc).clone();
        for sid in [StoryId(1), StoryId(2)] {
            doc.stories.insert(sid, designcraft_doc::Story::with_text(sid, "qzxw", Default::default()).into());
        }
        let report = build(Arc::new(doc.clone()));
        assert_eq!(report.story_words(&doc, StoryId(1)).unwrap(), std::slice::from_ref(&(0..4)));
        doc.story_mut(StoryId(2)).unwrap().text = "hello".into();
        assert_eq!(report.story_words(&doc, StoryId(1)).unwrap(), std::slice::from_ref(&(0..4)));
        assert!(report.story_words(&doc, StoryId(2)).is_none());
        let mut changed = doc.clone();
        changed.user_words.push("qzxw".into());
        assert!(report.story_words(&changed, StoryId(1)).is_none());
        changed = doc.clone();
        Arc::make_mut(&mut changed.styles);
        assert!(report.story_words(&changed, StoryId(1)).is_none());
    }

    /// Regression: revision changes keep the last report but document switches do not.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn revision_changes_keep_the_last_report_but_document_switches_do_not() {
        let mut app = DesignApp::new(SessionForTest::new(), crate::Services::default());
        let ctx = egui::Context::default();
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect":[0,0,300,300],"content":"text","text":"qzxw"})).unwrap();
        let active = app.session.active().unwrap();
        let key = (active.uid, active.revision);
        let old = Arc::new(build(active.doc.clone()));
        let (_tx, rx) = mpsc::channel(); // Keep a deterministic pending job; no thread/timing dependency.
        let state = State { running: Some((key, rx)), cached: Some((key, old.clone())), ..Default::default() };
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("spelling_worker"), Arc::new(Mutex::new(state))));
        app.run("frame.create", json!({"rect":[0,0,300,300],"content":"text","text":"hello"})).unwrap();
        assert!(Arc::ptr_eq(&report(&app, &ctx).unwrap(), &old));
        assert!(!Arc::ptr_eq(&old.doc, &app.session.active().unwrap().doc), "dialog must wait for the current snapshot");
        app.run("file.new", json!({})).unwrap();
        assert!(report(&app, &ctx).is_none());
    }

    /// Regression: stale browser completion requests a frame for the current scan.
    #[test]
    fn stale_browser_completion_requests_a_frame_for_the_current_scan() {
        let ctx = egui::Context::default();
        for _ in 0..8 {
            ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        }
        assert!(!ctx.has_requested_repaint(), "the idle dialog has no scheduled frame");
        let mut session = SessionForTest::new();
        session.execute("file.new", &json!({})).unwrap();
        let doc = session.doc().unwrap().doc.clone();
        let old_key = (1, 1);
        let key = (1, 2);
        let mut state = State { pending: Some((old_key, IncrementalReport::new(doc.clone()))), ..Default::default() };
        assert!(state.advance_browser(key, doc.clone(), &ctx).is_none());
        assert!(ctx.has_requested_repaint(), "a stale result must not leave the dialog waiting indefinitely");
        assert_eq!(state.cached.as_ref().unwrap().0, old_key);
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let current = state.advance_browser(key, doc.clone(), &ctx).unwrap();
        assert!(Arc::ptr_eq(&current.doc, &doc));
        assert_eq!(state.cached.as_ref().unwrap().0, key);
    }
    /// Regression: browser scan yields once per frame and keeps complete results.
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
    /// Regression: browser scan deadline yields and preserves results.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn browser_scan_deadline_yields_and_preserves_results() {
        require_hu_pack();
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
    /// Regression: browser scan work cap bounds correct words and punctuation.
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
    #[ignore = "manual spelling scheduler timing; requires the verified hu Language pack; run with --nocapture"]
    fn browser_scan_timing() {
        require_hu_pack();
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
    /// Regression: background cache tracks user words and language changes.
    #[test]
    #[ignore = "requires the verified hu Language pack; see docs/hungarian-language-support.md"]
    fn background_cache_tracks_user_words_and_language_changes() {
        require_hu_pack();
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
    /// Regression: spelling cursor does not revisit skipped occurrences after its own edit.
    #[test]
    fn spelling_cursor_does_not_revisit_skipped_occurrences_after_its_own_edit() {
        let words = vec![(StoryId(9), 0..5), (StoryId(9), 16..20), (StoryId(11), 0..4)];
        assert_eq!(resume_index(&words, StoryId(9), 15), 1);
        assert_eq!(resume_index(&words, StoryId(9), 25), 2);
        assert_eq!(resume_index(&words, StoryId(11), 4), 3);
    }
    use designcraft_engine::Session as SessionForTest;

    /// Regression: parameterized spelling calls return reports without opening a dialog.
    #[test]
    fn parameterized_spelling_calls_return_reports_without_opening_a_dialog() {
        for control in [false, true] {
            let mut app = DesignApp::new(SessionForTest::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            app.run("frame.create", json!({"rect":[0,0,300,300],"content":"text","text":"qzxw"})).unwrap();
            let sid = *app.session.doc().unwrap().doc.stories.keys().next().unwrap();
            app.in_control = control;
            let result = app.run("spelling.check", json!({"story":sid.0,"suggestions":false})).unwrap();
            assert!(result.is_array());
            assert_eq!(result[0]["word"], "qzxw");
            assert_eq!(result[0]["suggestions"], json!([]));
            assert!(app.ui.dialog.is_none());
            app.in_control = false;
            crate::menus::activate(&mut app, "spelling.check", &json!({"story":sid.0,"suggestions":false}));
            assert!(app.ui.dialog.is_none());
            app.run("spelling.check", json!({})).unwrap();
            assert_eq!(app.ui.dialog.as_ref().unwrap().id, "spelling");
        }
    }

    /// Regression: opening spelling cancels existing dialog previews on both routes.
    #[test]
    fn opening_spelling_cancels_existing_dialog_previews_on_both_routes() {
        for menu in [false, true] {
            let mut app = DesignApp::new(SessionForTest::new(), crate::Services::default());
            app.run("file.new", json!({})).unwrap();
            app.run("frame.create", json!({"rect":[0,0,300,300],"content":"text","text":"before"})).unwrap();
            let sid = *app.session.doc().unwrap().doc.stories.keys().next().unwrap();
            app.session.execute("text.select", &json!({"story":sid.0,"anchor":0,"focus":6})).unwrap();
            let before = serde_json::to_value(app.session.doc().unwrap().doc.story(sid).unwrap()).unwrap();
            app.session.begin_interaction("style preview").unwrap();
            app.session.execute("type.char", &json!({"attrs":{"size":42.0}})).unwrap();
            assert_ne!(serde_json::to_value(app.session.doc().unwrap().doc.story(sid).unwrap()).unwrap(), before);
            app.ui.dialog = Some(crate::dialogs::Dialog::new("paragraphStyleOptions", json!({"_previewed":true})));
            if menu {
                crate::menus::activate(&mut app, "spelling.check", &Value::Null);
            } else {
                app.run("spelling.check", json!({})).unwrap();
            }
            assert_eq!(serde_json::to_value(app.session.doc().unwrap().doc.story(sid).unwrap()).unwrap(), before);
            let dialog = app.ui.dialog.as_ref().unwrap();
            assert_eq!(dialog.id, "spelling");
            assert_eq!(dialog.fields.get("uid"), Some(&json!(app.session.active().unwrap().uid)));
        }
    }

    /// Regression: opening spelling without a document preserves the existing dialog.
    #[test]
    fn opening_spelling_without_a_document_preserves_the_existing_dialog() {
        let mut app = DesignApp::new(SessionForTest::new(), crate::Services::default());
        app.ui.dialog = Some(crate::dialogs::Dialog::new("preferences", json!({})));
        assert!(open(&mut app).is_err());
        assert_eq!(app.ui.dialog.as_ref().unwrap().id, "preferences");
    }
}
