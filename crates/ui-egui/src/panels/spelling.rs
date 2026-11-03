//! The Spelling panel and right-click suggestions: the shared review (`craft-proof-ui`) over this
//! document's stories and the read-only spelling queries.
use std::{borrow::Cow, sync::Arc};

use craft_proof_ui::{Action, ContextChoice, Host, Issue, IssueKind, Review, Text};
use serde_json::{Value, json};

use crate::{DesignApp, i18n::tr, theme::Tokens};

/// The session as the review sees it.
struct AppHost<'a>(&'a mut DesignApp);

fn issue_of(v: &Value) -> Option<Issue> {
    Some(Issue {
        story: v["story"].as_u64()?,
        start: usize::try_from(v["start"].as_u64()?).ok()?,
        end: usize::try_from(v["end"].as_u64()?).ok()?,
        word: v["word"].as_str()?.to_owned(),
        kind: if v["kind"] == "spelling" { IssueKind::Spelling } else { IssueKind::Grammar },
        message: v["message"].as_str().unwrap_or("").to_owned(),
        suggestions: v["suggestions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect(),
    })
}

impl Host for AppHost<'_> {
    fn check(&mut self) -> Result<Vec<Issue>, String> {
        let found = self.0.run("spelling.check", json!({"suggestions": false}))?;
        Ok(found.as_array().into_iter().flatten().filter_map(issue_of).collect())
    }

    fn suggestions(&mut self, issue: &Issue) -> Result<Vec<String>, String> {
        let fresh = self.0.run("spelling.suggest", json!({"story": issue.story, "start": issue.start, "end": issue.end}))?;
        let fresh = issue_of(&fresh).ok_or("invalid spelling response")?;
        if fresh.word != issue.word {
            return Err("the issue changed".into());
        }
        Ok(fresh.suggestions)
    }

    fn text(&self, story: u64) -> Option<&str> {
        Some(self.0.session.active()?.doc.story(designcraft_doc::StoryId(story))?.text.as_str())
    }

    /// A story is replaced on any edit, formatting too; that can only ask for a needless re-check.
    fn version(&self, story: u64) -> Option<u64> {
        let story = self.0.session.active()?.doc.stories.get(&designcraft_doc::StoryId(story))?;
        Some(Arc::as_ptr(story) as usize as u64)
    }

    fn document(&self) -> Option<u64> {
        Some(self.0.session.active()?.identity_epoch)
    }

    fn reveal(&mut self, issue: &Issue) {
        let _ = self.0.run("text.select", json!({"story": issue.story, "anchor": issue.start, "focus": issue.end}));
    }

    fn change(&mut self, issue: &Issue, to: &str) -> Result<(), String> {
        self.0.run("spelling.change", json!({"story": issue.story, "start": issue.start, "end": issue.end, "to": to})).map(|_| ())
    }

    fn add_word(&mut self, word: &str) -> Result<(), String> {
        self.0.run("spelling.addWord", json!({"word": word})).map(|_| ())
    }

    fn notice(&self) -> Option<String> {
        let st = self.0.session.active()?;
        let unchecked = !designcraft_engine::cmd::spelling::khmer_available()
            && st.doc.stories.values().any(|s| designcraft_engine::cmd::spelling::has_khmer(&s.text));
        unchecked.then(|| tr(&self.0.ui.language, "Khmer data is not included. Khmer text has not been checked.").to_owned())
    }
}

struct AppText(String);

impl Text for AppText {
    fn ui<'a>(&'a self, source: &'a str) -> Cow<'a, str> {
        Cow::Owned(tr(&self.0, source).to_owned())
    }
}

fn colors(t: &Tokens, ui: &egui::Ui) -> craft_proof_ui::Colors {
    craft_proof_ui::Colors {
        text: t.text,
        dim: t.text_dim,
        field: t.input,
        field_border: t.input_border,
        hover: t.hover,
        spelling: egui::Color32::from_rgb(230, 30, 30),
        grammar: t.accent,
        error: ui.visuals().error_fg_color,
    }
}

/// Opens the panel and checks the document from the top.
pub(crate) fn open(app: &mut DesignApp) -> Result<Value, String> {
    if app.session.active().is_none() {
        return Err("no document open".into());
    }
    app.run("window.panel", json!({"panel": "spelling"}))?;
    let mut review = Review::default();
    review.start(&mut AppHost(app));
    let status = status(&review);
    app.spelling = review;
    Ok(status)
}

fn status(review: &Review) -> Value {
    let issue = review.current().map(|i| json!({"story": i.story, "start": i.start, "end": i.end, "word": i.word, "suggestions": i.suggestions}));
    json!({"issue": issue, "remaining": review.remaining(), "to": review.replacement, "message": review.message()})
}

/// `app.spelling`: the panel's actions for automation.
pub(crate) fn action(app: &mut DesignApp, name: &str, to: Option<&str>) -> Result<Value, String> {
    let action = match name {
        "change" => Action::Change(to.map(str::to_owned)),
        "ignoreOnce" => Action::IgnoreOnce,
        "ignoreAll" => Action::IgnoreAll,
        "add" => Action::AddToDictionary,
        "check" => Action::Check,
        _ => return Err("unknown action (change, ignoreOnce, ignoreAll, add, check)".into()),
    };
    if !app.spelling.started() {
        return Err("run app.checkSpelling first".into());
    }
    let mut review = std::mem::take(&mut app.spelling);
    let result = review.act(&mut AppHost(app), action);
    let status = status(&review);
    app.spelling = review;
    result.map(|()| status)
}

pub fn show(app: &mut DesignApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let colors = colors(&t, ui);
    let text = AppText(app.ui.language.clone());
    let mut review = std::mem::take(&mut app.spelling);
    craft_proof_ui::panel(ui, &mut review, &mut AppHost(app), &text, &colors);
    app.spelling = review;
}

/// On a right-click in text: the issue under the caret, with its suggestions, for the menu.
/// Checked once here, not on every frame the menu is open.
pub(crate) fn prepare_context(app: &mut DesignApp) {
    app.spelling_context = None;
    let Some(text) = app.session.active().and_then(|st| st.selection.text) else { return };
    if let Ok(found) = app.run("spelling.at", json!({"story": text.story.0, "at": text.focus})) {
        app.spelling_context = issue_of(&found);
    }
}

/// The menu's first section: the suggestions, then what else can be done with the word.
pub(crate) fn context_items(app: &mut DesignApp, ui: &mut egui::Ui) {
    let Some(issue) = app.spelling_context.clone() else { return };
    let text = AppText(app.ui.language.clone());
    let Some(choice) = craft_proof_ui::context_section(ui, &issue, &text) else { return };
    let result = match choice {
        ContextChoice::Change(to) => AppHost(app).change(&issue, &to),
        ContextChoice::AddToDictionary => AppHost(app).add_word(&issue.word),
        ContextChoice::Review => open(app).map(|_| ()),
    };
    if let Err(e) = result {
        app.status(e);
    }
    app.spelling_context = None;
    ui.close();
}

#[cfg(test)]
mod tests;
