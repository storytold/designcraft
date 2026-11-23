//! The framework form of the generic command dialogs (`cmd:<command>`): one field per parameter of the command's
//! documentation, as the native form lists them. A flag is a check box, a parameter with a list of values a picker (with
//! a blank first entry), and any other parameter a text field showing the documented value spec as its hint. The form edits
//! the same draft fields, and OK runs the command with them as the controller always has.

use super::framework::Form;
use super::text_forms::{Attrs, check, field, message, select_at, text_of};
use super::{CommandField, Dialog, command_fields, humanize};
use crate::DesignApp;
use craft_ui_widgets::contract::{Bind, Props as _, Show as _, ShowContainer as _};
use craft_ui_widgets::form::FieldGroup;
use craft_ui_widgets::layout_components::Stack;
use craft_ui_widgets::text::TextField;
use egui::Ui;
use serde_json::{Value, json};

/// Any command's generic dialog.
pub(crate) const COMMAND_FORM: Form = Form::new(command_form);

/// Whether the dialog `id` is the generic form of a command: a `cmd:` dialog with no form of its own.
pub(crate) fn is_generic(id: &str) -> bool {
    id.starts_with("cmd:") && ![super::STEP_AND_REPEAT, super::CORNER_OPTIONS, super::TOC].contains(&id)
}

fn command_form(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let lang = app.ui.language.clone();
    let lang = lang.as_str();
    let command = d.id.strip_prefix("cmd:").unwrap_or_default().to_owned();
    let fields = command_fields(designcraft_engine::find_command(&command).map_or("", |c| c.params));
    let none = Value::Null;
    let a = Attrs::new("", &none);
    let title = super::dialog_title(app, d);
    Stack::new().id_salt(("command_form", command.clone())).fill_width().show(ui, |ui, _| {
        FieldGroup::new(title.clone()).id_salt(("command_form_fields", command.clone())).show(ui, |ui, _| {
            for f in &fields {
                let label = format!("{}{}", crate::i18n::tr(lang, &humanize(&f.key)), if f.optional { "" } else { " *" });
                field(ui, &label, &f.key, |ui| parameter(ui, d, &a, lang, f, &label));
            }
        });
        if let Some(status) = d.fields.get("status").and_then(Value::as_str).map(str::to_owned) {
            message(ui, &status, false);
        }
    });
    None
}

/// The control of one parameter.
fn parameter(ui: &mut Ui, d: &mut Dialog, a: &Attrs, lang: &str, f: &CommandField, label: &str) {
    if f.boolean {
        check(ui, d, a, &f.key, label, true);
    } else if !f.choices.is_empty() {
        // A blank first entry (no value), then the documented values.
        let current = d.s(&f.key);
        let mut captions = vec!["—".to_owned()];
        captions.extend(f.choices.iter().map(|c| crate::i18n::tr(lang, &humanize(c)).to_owned()));
        let at = if current.is_empty() { Some(0) } else { f.choices.iter().position(|c| *c == current).map(|p| p + 1) };
        if let Some(picked) = select_at(ui, &f.key, label, &captions, at, Some(current), true) {
            let value = if picked == 0 { String::new() } else { f.choices[picked - 1].clone() };
            d.fields.insert(f.key.clone(), json!(value));
        }
    } else {
        let current = text_of(d.fields.get(&f.key).unwrap_or(&Value::Null));
        let mut field =
            TextField::new(label.to_owned(), Bind::given(current.clone())).id_salt(("command_form_text", f.key.clone())).limit(None::<usize>);
        if !f.hint.is_empty() {
            field = field.placeholder(f.hint.clone());
        }
        if let Some(next) = field.show(ui).events.landed().filter(|next| **next != current).cloned() {
            d.fields.insert(f.key.clone(), Value::String(next));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::super::table_layout_kit as kit;
    use super::*;
    use egui::accesskit::Role;

    /// Guards the generic command form: every documented parameter has a control, what is typed and checked stays in the
    /// draft until OK, and OK runs the command with the parameters the native form gave it. `facingPages` has no documented
    /// type, so it is a text field the command reads as JSON, as in the native form.
    #[test]
    fn command_form_edits_the_documented_parameters_and_ok_runs_the_command() {
        let fields = command_fields(designcraft_engine::find_command("file.new").unwrap().params);
        assert!(fields.iter().any(|f| f.key == "title") && fields.iter().any(|f| f.key == "primaryTextFrame" && f.boolean), "{fields:?}");
        let mut app = kit::app(1, false);
        app.ui.dialog = Some(Dialog::new("cmd:file.new", json!({})));
        let mut h = kit::harness(app);
        let documents = h.state().session.documents().len();
        kit::type_into(&mut h, "Title", 0, "Flyer");
        kit::type_into(&mut h, "Pages", 0, "3");
        kit::type_into(&mut h, "Facing pages", 0, "false");
        kit::click_first(&mut h, "Primary text frame");
        let d = kit::draft(&h);
        assert_eq!((d.s("title"), d.s("pages"), d.s("facingPages")), ("Flyer".to_owned(), "3".to_owned(), "false".to_owned()));
        assert!(d.b("primaryTextFrame"));
        assert_eq!(h.state().session.documents().len(), documents, "the fields only edit the draft");
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.is_none());
        let doc = &h.state().session.active().unwrap().doc;
        assert_eq!((doc.title.as_str(), doc.page_count(), doc.settings.facing_pages), ("Flyer", 3, false));
        assert!(doc.spreads.iter().any(|s| !s.items.is_empty()), "primaryTextFrame reaches the command");
    }

    /// Guards the refusal path of the generic form: a command that refuses its parameters keeps the dialog open with the
    /// reason shown, and Cancel closes it without running anything.
    #[test]
    fn command_form_keeps_the_dialog_open_when_the_command_refuses() {
        let mut app = kit::app(1, false);
        app.ui.dialog = Some(Dialog::new("cmd:file.new", json!({"width": 0})));
        let mut h = kit::harness(app);
        let documents = h.state().session.documents().len();
        kit::click(&mut h, &[Role::Button], "OK", 0);
        assert!(h.state().ui.dialog.as_ref().is_some_and(|d| d.s("status").contains("page size out of range")));
        kit::click(&mut h, &[Role::Button], "Cancel", 0);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.documents().len(), documents);
    }

    /// Guards the prefix rule of the framework registry: a `cmd:` dialog with a form of its own is not the generic one.
    #[test]
    fn only_commands_without_a_form_of_their_own_are_generic() {
        assert!(is_generic("cmd:file.new"));
        assert!(!is_generic(super::super::STEP_AND_REPEAT) && !is_generic(super::super::TOC) && !is_generic(super::super::CORNER_OPTIONS));
        assert!(!is_generic("insertTable"));
    }
}
