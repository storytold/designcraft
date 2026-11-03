//! Native review over read-only spelling queries. Only explicit UI invocation opens this dialog.
use std::{collections::HashMap, sync::Arc};

use serde_json::{Value, json};

use crate::{DesignApp, dialogs::Dialog};

pub(crate) const ID: &str = "spellingReview";

/// Trusted snapshot: automation may edit dialog fields, but cannot change the reviewed range,
/// document identity, or the text against which that range was checked.
#[derive(Clone, Debug)]
pub(crate) struct Review {
    epoch: u64,
    revision: u64,
    issues: Vec<Value>,
    skipped: Vec<Value>,
    texts: HashMap<u64, Arc<str>>,
}

pub(crate) fn open(app: &mut DesignApp) -> Result<Value, String> {
    crate::dialogs::cancel(app);
    let epoch = app.session.active().ok_or("no document open")?.identity_epoch;
    let mut d = Dialog::new(ID, json!({"action": "change"}));
    d.spelling = Some(Review { epoch, revision: 0, issues: vec![], skipped: vec![], texts: HashMap::new() });
    refresh(app, &mut d)?;
    let result = serde_json::to_value(&d).map_err(|e| e.to_string())?;
    app.ui.dialog = Some(d);
    Ok(result)
}

fn same_document(app: &DesignApp, d: &Dialog) -> bool {
    d.spelling.as_ref().is_some_and(|r| app.session.active().is_some_and(|st| st.identity_epoch == r.epoch))
}

fn current(d: &Dialog) -> Option<&Value> {
    d.spelling.as_ref()?.issues.first()
}

fn same_issue(a: &Value, b: &Value) -> bool {
    ["story", "start", "end", "word", "kind"].iter().all(|k| a[k] == b[k])
}

fn refresh(app: &mut DesignApp, d: &mut Dialog) -> Result<(), String> {
    if !same_document(app, d) {
        return Err("The document changed. Close this review and run Check Spelling again.".into());
    }
    let result = app.run("spelling.check", json!({"suggestions": false}))?;
    let issues = result.as_array().ok_or("invalid spelling response")?;
    let status = if designcraft_engine::find_command("spelling.status").is_some() { app.run("spelling.status", json!({}))? } else { Value::Null };
    let st = app.session.active().ok_or("no document open")?;
    let has_khmer = st.doc.stories.values().any(|s| designcraft_engine::cmd::spelling::has_khmer(&s.text));
    let r = d.spelling.as_mut().ok_or("review was not initialized; run app.checkSpelling")?;
    // Text edits outside this review invalidate Skip's offsets too.
    if r.texts.iter().any(|(sid, text)| st.doc.story(designcraft_doc::StoryId(*sid)).is_none_or(|s| s.text.as_str() != text.as_ref())) {
        r.skipped.clear();
    }
    r.issues = issues.iter().filter(|i| !r.skipped.iter().any(|skip| same_issue(skip, i))).cloned().collect();
    r.texts = st.doc.stories.iter().map(|(sid, s)| (sid.0, Arc::from(s.text.as_str()))).collect();
    r.revision = st.revision;
    d.fields.insert("khmerPresent".into(), json!(has_khmer));
    d.fields.insert("khmerUnchecked".into(), json!(has_khmer && status["khmerAvailable"] != true));
    load_current(app, d)
}

/// Never compute edit-distance suggestions for the rest of a book's findings.
fn load_current(app: &mut DesignApp, d: &mut Dialog) -> Result<(), String> {
    sync_current(d)?;
    if let Some(issue) = current(d).cloned() {
        let fresh = app.run("spelling.suggest", json!({"story": issue["story"], "start": issue["start"], "end": issue["end"]}))?;
        if !same_issue(&issue, &fresh) || !fresh["suggestions"].is_array() {
            return Err("This issue is no longer current. Refresh the review.".into());
        }
        let r = d.spelling.as_mut().ok_or("review was not initialized")?;
        if let Some(issue) = r.issues.first_mut() {
            *issue = fresh;
        }
    }
    sync_current(d)
}

fn sync_current(d: &mut Dialog) -> Result<(), String> {
    let r = d.spelling.as_ref().ok_or("review was not initialized")?;
    d.fields.insert("remaining".into(), json!(r.issues.len()));
    d.fields.insert("skipped".into(), json!(r.skipped.len()));
    d.fields.insert("issue".into(), r.issues.first().cloned().unwrap_or(Value::Null));
    d.fields.insert("status".into(), json!(""));
    seed_selection(d);
    Ok(())
}

fn seed_selection(d: &mut Dialog) {
    let to = current(d).and_then(|i| i["suggestions"].as_array()?.first()?.as_str()).unwrap_or("").to_owned();
    d.fields.insert("suggestion".into(), json!(0));
    d.fields.insert("to".into(), json!(to));
    d.fields.insert("action".into(), json!("change"));
}

pub(crate) fn select(d: &mut Dialog, index: &Value) {
    if let Some(to) = index.as_u64().and_then(|n| usize::try_from(n).ok()).and_then(|n| current(d)?["suggestions"].as_array()?.get(n)?.as_str()) {
        d.fields.insert("to".into(), json!(to));
    }
}

fn validate(app: &mut DesignApp, d: &Dialog, issue: &Value) -> Result<(), String> {
    if !same_document(app, d) {
        return Err("The document changed. Close this review and run Check Spelling again.".into());
    }
    let sid = issue["story"].as_u64().ok_or("invalid story")?;
    let start = issue["start"].as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("invalid start")?;
    let end = issue["end"].as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("invalid end")?;
    let text = &app.session.active().and_then(|s| s.doc.story(designcraft_doc::StoryId(sid))).ok_or("story was removed; refresh the review")?.text;
    let snapshot = d.spelling.as_ref().and_then(|r| r.texts.get(&sid).map(AsRef::as_ref));
    if snapshot != Some(text.as_str()) || text.get(start..end) != issue["word"].as_str() {
        return Err("The text changed. Refresh the review before applying a correction.".into());
    }
    // Also reject findings invalidated by language/style/dictionary edits with unchanged text.
    let fresh = app.run("spelling.check", json!({"story": sid, "suggestions": false}))?;
    if !fresh.as_array().is_some_and(|issues| issues.iter().any(|i| same_issue(issue, i))) {
        return Err("This issue is no longer current. Refresh the review.".into());
    }
    Ok(())
}

/// All buttons and ui.dialog.confirm share the same validation and engine edit path.
pub(crate) fn confirm(app: &mut DesignApp, mut d: Dialog) -> Result<Value, String> {
    let result = apply(app, &mut d);
    if let Err(e) = &result {
        d.fields.insert("status".into(), json!(e));
        app.status(e.clone());
    }
    d.fields.insert("action".into(), json!("change"));
    app.ui.dialog = Some(d);
    result
}

fn apply(app: &mut DesignApp, d: &mut Dialog) -> Result<Value, String> {
    let action = d.s("action");
    if action == "refresh" {
        if let Some(r) = &mut d.spelling {
            r.skipped.clear();
        }
        refresh(app, d)?;
        return Ok(Value::Null);
    }
    let issue = current(d).cloned().ok_or("no issue to review")?;
    validate(app, d, &issue)?;
    match action.as_str() {
        "change" => {
            let to = d.s("to");
            if to.is_empty() || issue["word"].as_str() == Some(to.as_str()) {
                return Err("Choose a suggestion or enter a different replacement.".into());
            }
            app.run("spelling.change", json!({"story": issue["story"], "start": issue["start"], "end": issue["end"], "to": to}))?;
            // Retain skipped issues across our own edit, translating later byte offsets.
            // External text edits still invalidate this pass on refresh.
            if let Some(r) = &mut d.spelling {
                let start = issue["start"].as_u64().ok_or("invalid start")?;
                let end = issue["end"].as_u64().ok_or("invalid end")?;
                r.skipped.retain_mut(|skip| {
                    if skip["story"] != issue["story"] {
                        return true;
                    }
                    let Some(a) = skip["start"].as_u64() else { return false };
                    let Some(b) = skip["end"].as_u64() else { return false };
                    if b <= start {
                        return true;
                    }
                    if a < end {
                        return false;
                    }
                    let shift = |n: u64| n.checked_sub(end.saturating_sub(start))?.checked_add(to.len() as u64);
                    let (Some(a), Some(b)) = (shift(a), shift(b)) else { return false };
                    skip["start"] = json!(a);
                    skip["end"] = json!(b);
                    true
                });
                if let Some(sid) = issue["story"].as_u64()
                    && let Some(story) = app.session.active().and_then(|st| st.doc.story(designcraft_doc::StoryId(sid)))
                {
                    r.texts.insert(sid, Arc::from(story.text.as_str()));
                }
            }
        }
        "skip" => {
            if let Some(r) = &mut d.spelling {
                r.skipped.push(issue);
                if !r.issues.is_empty() {
                    r.issues.remove(0);
                }
            }
            load_current(app, d)?;
            return Ok(json!({"remaining": d.fields.get("remaining")}));
        }
        "add" => {
            if issue["kind"].as_str().is_some_and(|kind| kind != "spelling") {
                return Err("Only spelling issues can be added to the dictionary.".into());
            }
            app.run("spelling.addWord", json!({"word": issue["word"]}))?;
        }
        _ => return Err("unknown review action (change, skip, add, refresh)".into()),
    }
    refresh(app, d)?;
    Ok(json!({"remaining": d.fields.get("remaining")}))
}

pub(crate) fn show(app: &mut DesignApp, ctx: &egui::Context) {
    // Taken, not cloned: a book's findings would otherwise be copied on every frame.
    let Some(mut d) = app.ui.dialog.take() else { return };
    let lang = app.ui.language.clone();
    let tr = |s: &str| crate::i18n::tr(&lang, s).to_owned();
    let mut action = None;
    let mut close = false;
    let popup_open = egui::Popup::is_any_open(ctx);
    let stale = d.spelling.as_ref().is_none_or(|r| app.session.active().is_none_or(|st| st.revision != r.revision));
    let host = egui::Id::unique("spelling_review");
    crate::modal_drag::modal(ctx, host, host).show(ctx, |ui| {
        let width = (ctx.content_rect().width() - 48.0).clamp(1.0, 560.0);
        ui.set_width(width);
        let title = crate::rtl::label(ui, egui::RichText::new(tr("Check Spelling…")).font(crate::theme::semibold(16.0)));
        crate::modal_drag::title_drag(ui, host, title.rect);
        ui.add_space(8.0);
        // Keep the footer stationary when advancing or completing a review.
        let body_height = (ctx.content_rect().height() - 150.0).clamp(1.0, 400.0);
        egui::ScrollArea::vertical().min_scrolled_height(body_height).max_height(body_height).show(ui, |ui| {
            if !same_document(app, &d) {
                crate::rtl::label(ui, tr("The document changed. Close this review and run Check Spelling again."));
            } else if stale {
                crate::rtl::label(ui, tr("The document was edited. Refresh the review to check the current text."));
            } else {
                if d.b("khmerUnchecked") {
                    crate::rtl::label(ui, tr("Khmer data is not included. Khmer text has not been checked."));
                    ui.separator();
                }
                if d.b("khmerPresent") && !d.b("khmerUnchecked") {
                    crate::rtl::label(ui, tr("Khmer review covers spelling, preferred forms and simple rules."));
                }
                if let Some(issue) = current(&d).cloned() {
                    crate::rtl::label(ui, format!("{}: {}", tr("Remaining"), d.fields.get("remaining").unwrap_or(&Value::Null)));
                    let kind = match issue["kind"].as_str().unwrap_or("spelling") {
                        "grammar" => "Preferred form / simple rule",
                        "spelling" => "Spelling",
                        other => other,
                    };
                    crate::rtl::label(ui, egui::RichText::new(tr(kind)).strong());
                    crate::rtl::label(ui, egui::RichText::new(issue["word"].as_str().unwrap_or("")).size(20.0));
                    if let Some(message) = issue["message"].as_str().filter(|s| !s.is_empty()) {
                        crate::rtl::label(ui, message);
                    }
                    if let Some(context) = context(&d, &issue) {
                        crate::rtl::label(ui, context);
                    }
                    ui.separator();
                    crate::rtl::label(ui, tr("Suggestions"));
                    if let Some(suggestions) = issue["suggestions"].as_array().filter(|s| !s.is_empty()) {
                        for (index, suggestion) in suggestions.iter().enumerate().filter_map(|(n, s)| Some((n, s.as_str()?))) {
                            if ui.selectable_label(d.fields.get("suggestion") == Some(&json!(index)), suggestion).clicked() {
                                d.set_field("suggestion", json!(index));
                            }
                        }
                    } else {
                        crate::rtl::label(ui, tr("No suggestions. Enter a replacement or skip this issue."));
                    }
                    let name = crate::rtl::label(ui, tr("Change to"));
                    let mut to = d.s("to");
                    if ui.add(egui::TextEdit::singleline(&mut to).desired_width(width)).labelled_by(name.id).changed() {
                        d.fields.insert("to".into(), json!(to));
                        d.fields.remove("suggestion");
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(!d.s("to").is_empty(), egui::Button::new(tr("Change"))).clicked() {
                            action = Some("change");
                        }
                        if ui.button(tr("Skip")).clicked() {
                            action = Some("skip");
                        }
                        if issue["kind"].as_str().is_none_or(|k| k == "spelling") && ui.button(tr("Add to Dictionary")).clicked() {
                            action = Some("add");
                        }
                    });
                } else {
                    crate::rtl::label(
                        ui,
                        tr(if d.fields.get("skipped").and_then(Value::as_u64).unwrap_or(0) > 0 {
                            "Review complete. Some issues were skipped."
                        } else if d.b("khmerUnchecked") {
                            "No issues found in checked text. Khmer text remains unchecked."
                        } else {
                            "No issues found."
                        }),
                    );
                }
            }
            if !d.s("status").is_empty() {
                ui.separator();
                crate::rtl::label(ui, d.s("status"));
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.add_enabled(same_document(app, &d), egui::Button::new(tr("Refresh"))).clicked() {
                if let Some(r) = &mut d.spelling {
                    r.skipped.clear();
                }
                action = Some("refresh");
            }
            if ui.button(tr("Done")).clicked() {
                close = true;
            }
        });
        if !popup_open
            && !stale
            && same_document(app, &d)
            && action.is_none()
            && current(&d).is_some()
            && !d.s("to").is_empty()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            action = Some("change");
        }
        if !popup_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    if let Some(action) = action.filter(|_| !close) {
        d.fields.insert("action".into(), json!(action));
    }
    app.ui.dialog = Some(d);
    if close {
        crate::dialogs::cancel(app);
    } else if action.is_some() {
        let _ = crate::dialogs::confirm(app);
    }
}

fn context(d: &Dialog, issue: &Value) -> Option<String> {
    let text = d.spelling.as_ref()?.texts.get(&issue["story"].as_u64()?)?;
    let start = usize::try_from(issue["start"].as_u64()?).ok()?;
    let end = usize::try_from(issue["end"].as_u64()?).ok()?;
    text.get(start..end)?;
    let left = text.get(..start)?.char_indices().rev().nth(45).map_or(0, |(i, _)| i);
    let right = text.get(end..)?.char_indices().nth(45).map_or(text.len(), |(i, _)| end + i);
    Some(format!("{}{}{}", if left > 0 { "…" } else { "" }, text.get(left..right)?, if right < text.len() { "…" } else { "" }))
}
