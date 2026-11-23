//! Framework forms of the small dialogs: Alert, Unsaved Changes (Close Document), Delete Pages, Update Link, New
//! Workspace, Rectangle (frame size), Go to Row, Go to Page, Insert Pages, Pages Panel Options and Parent Options.
//!
//! Each form edits the same `Dialog` draft its native body edits, and OK and Cancel go through the host's `confirm` and
//! `cancel`, so the commands, the document and its undo steps are the ones the native windows produced. The native
//! bodies stay in `dialogs.rs` for the browser build.

use super::Form;
use crate::DesignApp;
use crate::dialogs::Dialog;
use craft_ui_panels::sections::GroupBox;
use craft_ui_widgets::buttons::Button;
use craft_ui_widgets::choice::{Checkbox, Radio, RadioGroup};
use craft_ui_widgets::contract::{Bind, Entry, Props as _, Scroll, Show as _, ShowContainer as _, Sizing};
use craft_ui_widgets::display::{Shade, Text};
use craft_ui_widgets::form::{Field, FieldGroup};
use craft_ui_widgets::layout_components::{Row, Stack};
use craft_ui_widgets::pickers::Select;
use craft_ui_widgets::style::TypeRole;
use craft_ui_widgets::text::{NumberField, TextField};
use egui::Ui;
use serde_json::{Value, json};

/// Alert: one OK; import warnings list a Show button beside each line that names a frame.
pub(crate) const ALERT: Form = Form::new(alert).width(440.0);
/// Unsaved Changes: Save, Cancel and Don't Save, which accepts with `discard` set.
pub(crate) const CLOSE_DOCUMENT: Form = Form::new(close_document).width(380.0).extra(&[("Don't Save", "discard")]);
/// Delete Pages: the warning for pages that hold objects.
pub(crate) const DELETE_PAGES: Form = Form::new(delete_pages).width(380.0);
/// Update Link: the warning for a link edited in the layout.
pub(crate) const UPDATE_LINK: Form = Form::new(update_link).width(380.0);
/// New Workspace: a name and what the workspace captures.
pub(crate) const NEW_WORKSPACE: Form = Form::new(new_workspace).width(380.0);
/// Rectangle: the size of the frame a frame tool click makes.
pub(crate) const FRAME_SIZE: Form = Form::new(frame_size);
/// Go to Row: the table section and the row.
pub(crate) const GO_TO_ROW: Form = Form::new(go_to_row);
/// Go to Page: a page name or number.
pub(crate) const GO_TO_PAGE: Form = Form::new(go_to_page);
/// Insert Pages: how many, where and with which parent.
pub(crate) const INSERT_PAGES: Form = Form::new(insert_pages).width(440.0);
/// Panel Options of the Pages panel.
pub(crate) const PAGES_PANEL_OPTIONS: Form = Form::new(pages_panel_options).width(380.0);
/// Parent Options: prefix, name and Based on Parent.
pub(crate) const PARENT_OPTIONS: Form = Form::new(parent_options).width(380.0);

/// `s` in the interface language.
fn tr(app: &DesignApp, s: &str) -> String {
    crate::i18n::tr(&app.ui.language, s).to_owned()
}

/// A check box over the bool draft field `key`.
fn check(ui: &mut Ui, d: &mut Dialog, key: &str, label: String, enabled: bool) {
    let mut on = d.b(key);
    let out = Checkbox::new(Bind::given(on)).label(label).id_salt(("simple_check", key)).enabled(enabled).show(ui);
    for event in out.events {
        event.apply(&mut on);
    }
    if on != d.b(key) {
        d.fields.insert(key.into(), json!(on));
    }
}

/// A text field over the string draft field `key`, named `label`.
fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, label: String) {
    let mut text = d.s(key);
    let out = TextField::new(label, Bind::given(text.clone())).id_salt(("simple_text", key)).limit(None::<usize>).show(ui);
    for event in out.events {
        event.apply(&mut text);
    }
    if text != d.s(key) {
        d.fields.insert(key.into(), Value::String(text));
    }
}

/// A menu over the string draft field `key`: `entries` are the values and their captions.
fn choice(ui: &mut Ui, d: &mut Dialog, key: &str, label: String, entries: &[(String, String)], enabled: bool) {
    let current = d.s(key);
    let entries: Vec<Entry<String>> = entries.iter().map(|(value, caption)| Entry::new(value.clone(), caption.clone())).collect();
    let out = Select::new(label, Bind::given(current.clone()), &entries).id_salt(("simple_select", key)).enabled(enabled).show(ui);
    if let Some(next) = out.events.committed().filter(|next| **next != current) {
        d.fields.insert(key.into(), json!(next));
    }
}

/// The text a dialog shows for its status line, if it has one.
fn status(d: &Dialog) -> Option<String> {
    d.fields.get("status").and_then(Value::as_str).map(str::to_owned)
}

/// The import warnings of an alert: each line's text and the frame it names, if any.
fn alert_lines(d: &Dialog) -> Vec<(String, Option<u64>)> {
    d.fields
        .get("lines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|l| (l.get("text").and_then(Value::as_str).unwrap_or_default().to_string(), l.get("item").and_then(Value::as_u64)))
        .collect()
}

/// An alert's message. Import warnings list one per line, scrolling when there are many, with Show beside each that
/// names a frame: Show selects that frame in view and closes the alert.
fn alert(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let show = tr(app, "Show");
    let file = d.s("file");
    let message = d.s("message");
    let lines = alert_lines(d);
    let mut chosen = None;
    Stack::new().id_salt("simple_alert").fill_width().show(ui, |ui, _| {
        if !file.is_empty() {
            Text::new(crate::rtl::isolate(&file)).role(TypeRole::BodyStrong).show(ui);
        }
        if lines.iter().all(|(_, item)| item.is_none()) {
            Text::new(message.clone()).show(ui);
            return;
        }
        Stack::new().id_salt("simple_alert_lines").fill_width().scroll(Scroll::vertical()).room_height(360.0).show(ui, |ui, _| {
            for (at, (text, item)) in lines.iter().enumerate() {
                Row::new().id_salt(("simple_alert_line", at)).fill_width().show(ui, |ui, _| {
                    if let Some(item) = item
                        && Button::new(show.clone()).id_salt(("simple_alert_show", at)).size(craft_theme::ControlSize::Sm).show(ui).clicked()
                    {
                        chosen = Some(*item);
                    }
                    Text::new(text.clone()).sizing(Sizing::FillWidth).show(ui);
                });
            }
        });
    });
    let item = chosen?;
    if app.run("selection.set", json!({"ids": [item]})).is_ok() {
        let _ = app.run("view.fitSelection", json!({}));
    }
    Some(false)
}

fn close_document(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let text = tr(app, "“{}” has changes that are not saved. Save them before closing?").replace("{}", &d.s("title"));
    Text::new(text).show(ui);
    None
}

fn delete_pages(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let several = d.fields.get("pages").and_then(Value::as_array).is_some_and(|p| p.len() > 1);
    let message = tr(
        app,
        if several {
            "The affected pages contain objects. Delete the pages anyway?"
        } else {
            "The affected page contains objects. Delete the page anyway?"
        },
    );
    let again = tr(app, "Don't show again");
    Stack::new().id_salt("simple_delete_pages").fill_width().show(ui, |ui, _| {
        Text::new(message.clone()).show(ui);
        check(ui, d, "dontShowAgain", again.clone(), true);
    });
    None
}

fn update_link(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let text =
        tr(app, "“{}” has been edited in this layout. Updating replaces those edits with the text from the file.").replace("{}", &d.s("files"));
    Text::new(text).show(ui);
    None
}

fn new_workspace(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let (title, name, capture) = (tr(app, "New Workspace"), tr(app, "Name:"), tr(app, "Capture"));
    let (panels, menus) = (tr(app, "Panel Locations"), tr(app, "Menu Customization"));
    Stack::new().id_salt("simple_new_workspace").fill_width().show(ui, |ui, _| {
        FieldGroup::new(title.clone()).show(ui, |ui, _| {
            Field::new(name.clone()).id_salt("simple_workspace_name").show(ui, |ui, _| text_field(ui, d, "name", name.clone()));
        });
        GroupBox::new(capture.clone(), None).id_salt("simple_workspace_capture").show(ui, |ui, _| {
            check(ui, d, "panelLocations", panels.clone(), true);
            check(ui, d, "menuCustomization", menus.clone(), true);
        });
    });
    None
}

fn frame_size(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let (title, width, height) = (tr(app, "Rectangle"), tr(app, "Width"), tr(app, "Height"));
    FieldGroup::new(title).show(ui, |ui, _| {
        Field::new(width.clone()).id_salt("simple_frame_width").show(ui, |ui, _| text_field(ui, d, "width", width.clone()));
        Field::new(height.clone()).id_salt("simple_frame_height").show(ui, |ui, _| text_field(ui, d, "height", height.clone()));
    });
    None
}

/// The section menu shows when the table has header or footer rows.
fn go_to_row(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let (title, section, row) = (tr(app, "Go to Row"), tr(app, "Section"), tr(app, "Row"));
    let mut sections = Vec::new();
    if d.b("hasHeader") {
        sections.push(("header".to_owned(), tr(app, "Header Rows")));
    }
    sections.push(("body".to_owned(), tr(app, "Body Rows")));
    if d.b("hasFooter") {
        sections.push(("footer".to_owned(), tr(app, "Footer Rows")));
    }
    let with_sections = d.b("hasHeader") || d.b("hasFooter");
    Stack::new().id_salt("simple_go_to_row").fill_width().show(ui, |ui, _| {
        FieldGroup::new(title.clone()).show(ui, |ui, _| {
            if with_sections {
                Field::new(section.clone())
                    .id_salt("simple_row_section")
                    .show(ui, |ui, _| choice(ui, d, "section", section.clone(), &sections, true));
            }
            Field::new(row.clone()).id_salt("simple_row_number").show(ui, |ui, _| text_field(ui, d, "row", row.clone()));
        });
        if let Some(message) = status(d) {
            Text::new(message).shade(Shade::Muted).show(ui);
        }
    });
    None
}

fn go_to_page(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let (title, page) = (tr(app, "Go to Page"), tr(app, "Page"));
    FieldGroup::new(title).show(ui, |ui, _| {
        Field::new(page.clone()).id_salt("simple_go_to_page").show(ui, |ui, _| text_field(ui, d, "page", page.clone()));
    });
    None
}

/// Insert Pages, as InDesign's: how many, where (after or before a page, at the start or the end of the document) and
/// with which parent.
fn insert_pages(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let st = app.session.active()?;
    let pages: Vec<String> = (0..st.doc.page_count()).map(|i| app.session.page_label(i)).collect();
    let mut parents = vec![(String::new(), tr(app, "[None]"))];
    parents.extend(st.doc.parents.iter().map(|p| (p.id.0.to_string(), p.parent.as_ref().map(|i| i.label()).unwrap_or_default())));
    let wheres: Vec<(String, String)> =
        [("after", "After Page"), ("before", "Before Page"), ("start", "At Start of Document"), ("end", "At End of Document")]
            .into_iter()
            .map(|(value, caption)| (value.to_owned(), tr(app, caption)))
            .collect();
    let (title, count_label, insert, page_label, parent) =
        (tr(app, "Insert Pages"), tr(app, "Pages:"), tr(app, "Insert:"), tr(app, "Page"), tr(app, "Parent:"));
    let count = d.n("count").unwrap_or(1.0).clamp(1.0, 9999.0) as u32;
    FieldGroup::new(title).show(ui, |ui, _| {
        Field::new(count_label.clone()).id_salt("simple_insert_count").show(ui, |ui, _| {
            let out = NumberField::new(count_label.clone(), Bind::given(Some(f64::from(count))))
                .id_salt("simple_insert_count_field")
                .range(1.0..=9999.0)
                .step(1.0)
                .snap_typed(true)
                .fixed_decimals(0)
                .commit_decimals(0)
                .show(ui);
            if let Some(Some(next)) = out.edit.events.landed().filter(|next| next.is_some_and(f64::is_finite)) {
                let next = next.clamp(1.0, 9999.0) as u32;
                if next != count {
                    d.fields.insert("count".into(), json!(next));
                }
            }
        });
        Field::new(insert.clone()).id_salt("simple_insert_where").show(ui, |ui, _| {
            Row::new().id_salt("simple_insert_where_row").fill_width().show(ui, |ui, _| {
                choice(ui, d, "insert", insert.clone(), &wheres, true);
                // The page, by name: At Start and At End need none.
                let current = (d.n("page").unwrap_or(0.0).max(0.0) as usize).min(pages.len().saturating_sub(1));
                let entries: Vec<Entry<usize>> = pages.iter().enumerate().map(|(at, name)| Entry::new(at, name.clone())).collect();
                let out = Select::new(page_label.clone(), Bind::given(current), &entries)
                    .id_salt("simple_insert_page")
                    .enabled(matches!(d.s("insert").as_str(), "after" | "before"))
                    .show(ui);
                if let Some(next) = out.events.committed().filter(|next| **next != current) {
                    d.fields.insert("page".into(), json!(next));
                }
            });
        });
        Field::new(parent.clone()).id_salt("simple_insert_parent").show(ui, |ui, _| choice(ui, d, "parent", parent.clone(), &parents, true));
    });
    None
}

/// As InDesign's: Pages, Parents, Panel Layout.
fn pages_panel_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    use crate::panels::pages::{IconSize, PanelResize};
    let sizes: Vec<(String, String)> = IconSize::ALL.iter().map(|s| (s.key().to_owned(), tr(app, s.label()))).collect();
    let resizes: Vec<(String, String)> = PanelResize::ALL.iter().map(|r| (r.key().to_owned(), tr(app, r.label()))).collect();
    let (pages, parents, layout) = (tr(app, "Pages"), tr(app, "Parents"), tr(app, "Panel Layout"));
    let (size, thumbnails, vertically, resize) = (tr(app, "Size:"), tr(app, "Show Thumbnails"), tr(app, "Show Vertically"), tr(app, "Resize:"));
    let layouts = [("pagesOnTop", tr(app, "Pages on Top")), ("parentsOnTop", tr(app, "Parents on Top"))];
    Stack::new().id_salt("simple_pages_panel").fill_width().show(ui, |ui, _| {
        GroupBox::new(pages.clone(), None).id_salt("simple_pages_group").show(ui, |ui, _| {
            Field::new(size.clone()).id_salt("simple_page_size").show(ui, |ui, _| choice(ui, d, "pageSize", size.clone(), &sizes, true));
            // Extra Small pages have no room for a thumbnail.
            let room = d.s("pageSize") != "extraSmall";
            check(ui, d, "pageThumbnails", thumbnails.clone(), room);
        });
        GroupBox::new(parents.clone(), None).id_salt("simple_parents_group").show(ui, |ui, _| {
            Field::new(size.clone()).id_salt("simple_parent_size").show(ui, |ui, _| choice(ui, d, "parentSize", size.clone(), &sizes, true));
            let room = d.s("parentSize") != "extraSmall";
            check(ui, d, "parentThumbnails", thumbnails.clone(), room);
            check(ui, d, "parentsVertical", vertically.clone(), true);
        });
        GroupBox::new(layout.clone(), None).id_salt("simple_layout_group").show(ui, |ui, _| {
            let before = d.s("layout");
            let shown = RadioGroup::new(layout.clone(), Bind::given(before.clone())).id_salt("simple_layout").show(ui, |ui, _| {
                Stack::new().show(ui, |ui, _| {
                    for (value, caption) in &layouts {
                        Radio::new((*value).to_owned()).label(caption.clone()).show(ui);
                    }
                });
            });
            let mut next = before.clone();
            for event in shown.output.events {
                event.apply(&mut next);
            }
            if next != before {
                d.fields.insert("layout".into(), json!(next));
            }
            Field::new(resize.clone()).id_salt("simple_resize").show(ui, |ui, _| choice(ui, d, "resize", resize.clone(), &resizes, true));
        });
    });
    None
}

/// As InDesign's: Prefix, Name, Based on Parent (Number of Pages is the parent spread's own).
fn parent_options(app: &mut DesignApp, ui: &mut Ui, d: &mut Dialog) -> Option<bool> {
    let (title, prefix, name, based) = (tr(app, "Parent Options"), tr(app, "Prefix:"), tr(app, "Name:"), tr(app, "Based on Parent:"));
    let mut bases = vec![(String::new(), tr(app, "[None]"))];
    bases.extend(
        d.fields
            .get("bases")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|b| Some((b.get(0)?.as_str()?.to_string(), b.get(1)?.as_str()?.to_string()))),
    );
    Stack::new().id_salt("simple_parent_options").fill_width().show(ui, |ui, _| {
        FieldGroup::new(title.clone()).show(ui, |ui, _| {
            Field::new(prefix.clone()).id_salt("simple_parent_prefix").show(ui, |ui, _| text_field(ui, d, "prefix", prefix.clone()));
            Field::new(name.clone()).id_salt("simple_parent_name").show(ui, |ui, _| text_field(ui, d, "name", name.clone()));
            Field::new(based.clone()).id_salt("simple_parent_based").show(ui, |ui, _| choice(ui, d, "basedOn", based.clone(), &bases, true));
        });
        if let Some(message) = status(d) {
            Text::new(message).show(ui);
        }
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_geom::{Unit, format_measure};
    use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role, vec2};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    type Window = Harness<'static, DesignApp>;

    fn app_of(pages: u32) -> DesignApp {
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.run("file.new", json!({"pages": pages, "facingPages": false})).unwrap();
        app
    }

    /// The app's dialog host with the framework installed, as the desktop shell runs it: the dialog in `app.ui.dialog`
    /// shows in the framework's modal.
    fn open(mut app: DesignApp) -> Window {
        let ctx = egui::Context::default();
        app.framework_transform = false;
        app.framework_opacity = true;
        app.install_framework(&ctx);
        app.raw_input_hook(&ctx, &mut egui::RawInput::default());
        assert!(app.framework_installed, "the framework forms need the installed runtime");
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(920.0, 1000.0)).build_ui_state(
            move |ui, app: &mut DesignApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &app.ui.language);
                    crate::theme::apply(ui.ctx(), &crate::theme::Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                crate::dialogs::show(app, ui.ctx());
            },
            app,
        );
        h.input_mut().max_texture_side = Some(8192);
        steps(&mut h, 3);
        h
    }

    fn event(h: &mut Window, event: Event) {
        let mut raw = std::mem::take(h.input_mut());
        raw.events.push(event);
        let ctx = h.ctx.clone();
        h.state_mut().raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }

    fn steps(h: &mut Window, count: usize) {
        for _ in 0..count {
            let mut raw = std::mem::take(h.input_mut());
            let ctx = h.ctx.clone();
            h.state_mut().raw_input_hook(&ctx, &mut raw);
            *h.input_mut() = raw;
            h.step();
        }
    }

    fn key(h: &mut Window, key: Key) {
        for pressed in [true, false] {
            event(h, Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        }
        steps(h, 3);
    }

    fn click_at(h: &mut Window, at: egui::Pos2) {
        event(h, Event::PointerMoved(at));
        for pressed in [true, false] {
            event(h, Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        }
        steps(h, 3);
    }

    /// Clicks the first control of `role` named `label` (the Pages panel's two Size menus share a name).
    fn click(h: &mut Window, role: Role, label: &str) {
        let at = h.query_all_by_role_and_label(role, label).next().unwrap_or_else(|| panic!("no {role:?} named {label}")).rect().center();
        click_at(h, at);
    }

    /// Chooses `option` in the menu named `label`: opens it, then clicks the listed entry.
    fn pick(h: &mut Window, label: &str, option: &str) {
        click(h, Role::ComboBox, label);
        let at = h.get_by_label(option).rect().center();
        click_at(h, at);
    }

    /// Types `text` over what the field named `label` of `role` holds, by real pointer and key events.
    fn type_into(h: &mut Window, role: Role, label: &str, text: &str) {
        click(h, role, label);
        assert!(h.get_by_role_and_label(role, label).is_focused(), "a click focuses {label}");
        for pressed in [true, false] {
            event(h, Event::Key { key: Key::A, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND });
        }
        event(h, Event::Text(text.into()));
        event(h, Event::ModifiersChanged(Modifiers::NONE));
        steps(h, 3);
    }

    fn dialog(h: &Window) -> &Dialog {
        h.state().ui.dialog.as_ref().expect("the dialog is open")
    }

    fn shows(h: &Window, label: &str) -> bool {
        h.query_all_by_label(label).next().is_some()
    }

    fn journal(h: &Window, command: &str) -> Vec<Value> {
        h.state().session.journal.iter().filter(|(id, _)| id == command).map(|(_, params)| params.clone()).collect()
    }

    fn pages(h: &Window) -> usize {
        h.state().session.active().unwrap().doc.page_count()
    }

    #[test]
    fn every_simple_dialog_has_a_framework_form() {
        for id in [
            "alert",
            "closeDocument",
            "deletePages",
            "updateLink",
            "newWorkspace",
            "frameSize",
            "goToRow",
            "goToPage",
            "insertPages",
            "pagesPanelOptions",
            "parentOptions",
        ] {
            assert!(super::super::form(id).is_some(), "{id} is registered with the host");
        }
        assert_eq!(super::super::form("closeDocument").unwrap().extra, &[("Don't Save", "discard")]);
    }

    #[test]
    fn an_alert_has_only_ok_and_shows_its_message() {
        let mut app = app_of(1);
        app.ui.dialog = Some(Dialog::new("alert", json!({"title": "Import Warnings", "file": "book.indd", "message": "One font was replaced."})));
        let mut h = open(app);
        assert!(shows(&h, "One font was replaced.") && shows(&h, "book.indd"));
        assert!(h.query_by_role_and_label(Role::Button, "Cancel").is_none(), "an alert only has OK");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
    }

    /// Successor, under the framework host, of `import_warning_show_selects_the_frame_it_is_about`.
    #[test]
    fn an_import_warning_offers_show_only_where_it_names_a_frame_and_show_selects_it() {
        let mut app = app_of(1);
        let f = app.run("frame.create", json!({"rect": [36, 36, 300, 200], "content": "text"})).unwrap();
        app.run("selection.set", json!({"ids": []})).unwrap();
        let lines = json!([
            {"text": "a table of 2 rows was read as 1", "item": null},
            {"text": "story 30892: text orientation of its frames is not known", "item": f["id"]},
        ]);
        app.ui.dialog = Some(Dialog::new("alert", json!({"title": "Import Warnings", "file": "book.indd", "message": "", "lines": lines})));
        let mut h = open(app);
        assert_eq!(h.query_all_by_role_and_label(Role::Button, "Show").count(), 1);
        assert!(shows(&h, "a table of 2 rows was read as 1"));
        click(&mut h, Role::Button, "Show");
        assert!(h.state().ui.dialog.is_none(), "Show closes the warnings");
        assert_eq!(h.state().session.doc().unwrap().selection.items, vec![designcraft_doc::ItemId(f["id"].as_u64().unwrap())]);
    }

    fn unsaved() -> DesignApp {
        let mut app = app_of(1);
        app.run("frame.create", json!({"rect": [36, 36, 136, 136]})).unwrap();
        assert!(app.session.documents()[0].is_dirty());
        crate::menus::close_document(&mut app, None);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("closeDocument"));
        app
    }

    #[test]
    fn unsaved_changes_dont_save_discards_the_document_and_cancel_keeps_it() {
        let mut h = open(unsaved());
        assert!(shows(&h, &format!("“{}” has changes that are not saved. Save them before closing?", dialog(&h).s("title"))));
        click(&mut h, Role::Button, "Don't Save");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().session.documents().is_empty(), "Don't Save closes the document without saving");

        let mut h = open(unsaved());
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.documents().len(), 1);
        assert!(h.state().session.documents()[0].is_dirty(), "Cancel changes nothing");

        // One Escape cancels, as the footer's Cancel does.
        let mut h = open(unsaved());
        key(&mut h, Key::Escape);
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.documents().len(), 1);
    }

    /// Save on a document that has no file asks for a path; with no picker answering, the save is cancelled and the
    /// document stays open (as `a_cancelled_save_keeps_the_document_open` has it for the native window).
    #[test]
    fn unsaved_changes_save_that_is_cancelled_keeps_the_document_open() {
        let mut h = open(unsaved());
        click(&mut h, Role::Button, "Save");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(h.state().session.documents().len(), 1);
        assert!(h.state().session.documents()[0].is_dirty());
    }

    fn with_object_on_page_two() -> DesignApp {
        let mut app = app_of(3);
        app.run("frame.create", json!({"spread": 1, "rect": [72, 72, 200, 200]})).unwrap();
        crate::dialogs::delete_pages(&mut app, vec![1]).unwrap();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("deletePages"));
        app
    }

    #[test]
    fn delete_pages_ok_deletes_once_and_remembers_dont_show_again() {
        let mut h = open(with_object_on_page_two());
        assert!(shows(&h, "The affected page contains objects. Delete the page anyway?"));
        click(&mut h, Role::CheckBox, "Don't show again");
        assert!(dialog(&h).b("dontShowAgain"));
        assert_eq!(pages(&h), 3, "the check box only edits the draft");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(pages(&h), 2);
        assert!(h.state().ui.skip_delete_pages_warning);
        assert_eq!(journal(&h, "layout.pages.delete"), vec![json!({"pages": [1]})]);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(pages(&h), 3, "one undo step brings the page back");
    }

    #[test]
    fn delete_pages_cancel_deletes_nothing_and_asks_again_next_time() {
        let mut h = open(with_object_on_page_two());
        click(&mut h, Role::CheckBox, "Don't show again");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(pages(&h), 3);
        assert!(!h.state().ui.skip_delete_pages_warning, "Cancel does not keep the check");
        assert!(journal(&h, "layout.pages.delete").is_empty());
    }

    fn linked_text(dir: &std::path::Path) -> (DesignApp, std::path::PathBuf, u64) {
        let path = dir.join("note.txt");
        std::fs::write(&path, "One").unwrap();
        let mut app = DesignApp::new_for_panel_tests(designcraft_engine::Session::new(), crate::Services::default());
        app.ui.language = "en".to_owned();
        app.session.execute("file.new", &json!({})).unwrap();
        let r = app.session.execute("file.place", &json!({"path": path.to_string_lossy(), "link": true, "rect": [72, 72, 300, 200]})).unwrap();
        let story = r["story"].as_u64().unwrap();
        // Edited in the layout: updating asks first.
        app.session.execute("story.setText", &json!({"story": story, "text": "Mine"})).unwrap();
        std::fs::write(&path, "Three").unwrap();
        crate::panels::properties::update_links(&mut app, json!({"story": story}));
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("updateLink"));
        (app, path, story)
    }

    #[test]
    fn update_link_update_replaces_the_edits_and_cancel_keeps_them() {
        let dir = tempfile::tempdir().unwrap();
        let text = |h: &Window, story: u64| h.state().session.doc().unwrap().doc.story(designcraft_doc::StoryId(story)).unwrap().text.clone();
        let (app, _, story) = linked_text(dir.path());
        let mut h = open(app);
        assert!(shows(&h, "“note.txt” has been edited in this layout. Updating replaces those edits with the text from the file."));
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(text(&h, story), "Mine");

        let (app, _, story) = linked_text(dir.path());
        let mut h = open(app);
        click(&mut h, Role::Button, "Update");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(text(&h, story), "Three");
    }

    fn workspace_dialog() -> DesignApp {
        let mut app = app_of(1);
        app.run("window.newWorkspace", json!({})).unwrap();
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("newWorkspace"));
        app
    }

    #[test]
    fn new_workspace_saves_the_name_and_what_it_captures() {
        let mut h = open(workspace_dialog());
        type_into(&mut h, Role::TextInput, "Name:", "Mine");
        assert_eq!(dialog(&h).s("name"), "Mine");
        click(&mut h, Role::CheckBox, "Menu Customization");
        assert!(!dialog(&h).b("menuCustomization") && dialog(&h).b("panelLocations"));
        assert!(h.state().ui.custom_workspaces.is_empty(), "the draft saves nothing until OK");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let saved = &h.state().ui.custom_workspaces;
        assert_eq!(saved.len(), 1);
        assert_eq!((saved[0].name.as_str(), saved[0].keep_panels, saved[0].hidden_menu_items.is_none()), ("Mine", false, true));
        assert_eq!(h.state().ui.workspace, "Mine");
    }

    #[test]
    fn new_workspace_without_a_name_asks_again_and_cancel_saves_nothing() {
        let mut h = open(workspace_dialog());
        click(&mut h, Role::Button, "OK");
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("newWorkspace"), "no name: the command asks again");
        assert!(h.state().ui.custom_workspaces.is_empty());
        type_into(&mut h, Role::TextInput, "Name:", "Never saved");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert!(h.state().ui.custom_workspaces.is_empty());
    }

    #[test]
    fn rectangle_reads_each_field_in_its_own_ruler_unit() {
        let mut app = app_of(1);
        app.run("document.preferences", json!({"horizontalUnits": "inches", "verticalUnits": "millimeters"})).unwrap();
        app.session.ui_requests.push(designcraft_engine::UiRequest::Dialog {
            id: "frameSize".into(),
            params: json!({"spread": 0, "shape": "rectangle", "content": "graphic", "x": 36.0, "y": 36.0, "width": 72.0, "height": 72.0}),
        });
        app.after_engine();
        let before = app.session.doc().unwrap().doc.clone();
        let mut h = open(app);
        assert_eq!(dialog(&h).s("width"), format_measure(72.0, Unit::Inches));
        type_into(&mut h, Role::TextInput, "Width", "2");
        type_into(&mut h, Role::TextInput, "Height", "50");
        assert_eq!((dialog(&h).s("width"), dialog(&h).s("height")), ("2".to_string(), "50".to_string()));
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let after = &h.state().session.doc().unwrap().doc;
        let added: Vec<_> = after.spreads[0].items.iter().filter(|i| !before.spreads[0].items.iter().any(|b| b.id == i.id)).collect();
        assert_eq!(added.len(), 1, "one frame");
        let b = added[0].bounds();
        assert!((b.width() - 144.0).abs() < 0.01 && (b.height() - 50.0 / 25.4 * 72.0).abs() < 0.01, "2 in by 50 mm: {b:?}");
    }

    fn table_app() -> DesignApp {
        let mut app = app_of(2);
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0)));
        let a = app.run("frame.create", json!({"rect": [72, 72, 400, 200], "content": "text", "text": ""})).unwrap();
        let b = app.run("frame.create", json!({"spread": 1, "rect": [72, 72, 400, 600], "content": "unassigned"})).unwrap();
        app.run("text.thread", json!({"from": a["id"], "to": b["id"]})).unwrap();
        app.run("text.select", json!({"story": a["story"], "anchor": 0, "focus": 0})).unwrap();
        app.run("table.insert", json!({"rows": 20, "cols": 2, "headerRows": 1})).unwrap();
        for row in 0..21 {
            app.run("table.setRowHeight", json!({"row": row, "height": 20, "mode": "exactly"})).unwrap();
        }
        app.run("table.select", json!({"rows": [2, 2]})).unwrap();
        crate::canvas::go_to_page(&mut app, 0);
        crate::menus::activate(&mut app, "table.goToRow", &Value::Null);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("goToRow"));
        app
    }

    /// Successor, under the framework host, of the native Go to Row checks: the dialog opens on the selected row, a
    /// row that is not a whole number keeps it open with the reason, and OK goes to the row and its page.
    #[test]
    fn go_to_row_keeps_a_refused_row_open_with_the_reason_and_then_goes_to_the_row() {
        let mut h = open(table_app());
        assert_eq!((dialog(&h).s("section"), dialog(&h).s("row")), ("body".to_string(), "2".to_string()));
        assert!(h.query_by_role_and_label(Role::ComboBox, "Section").is_some(), "a table with a header row offers the section menu");
        type_into(&mut h, Role::TextInput, "Row", "x");
        click(&mut h, Role::Button, "OK");
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("goToRow"));
        let reason = dialog(&h).s("status");
        assert!(!reason.is_empty());
        assert!(shows(&h, &reason), "the reason shows in the dialog");
        assert_eq!(crate::canvas::current_page(h.state()), Some(0), "the view stays");
        type_into(&mut h, Role::TextInput, "Row", "20");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let range = h.state().session.active().unwrap().selection.cells.unwrap().range;
        assert_eq!((range.r0, range.r1, range.c0, range.c1), (20, 20, 0, 1));
        assert_eq!(crate::canvas::current_page(h.state()), Some(1), "the view goes to the page that shows the row");
    }

    #[test]
    fn go_to_row_section_menu_picks_the_header_rows() {
        let mut h = open(table_app());
        pick(&mut h, "Section", "Header Rows");
        assert_eq!(dialog(&h).s("section"), "header");
        type_into(&mut h, Role::TextInput, "Row", "1");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let range = h.state().session.active().unwrap().selection.cells.unwrap().range;
        assert_eq!((range.r0, range.r1), (0, 0), "the first header row");
    }

    #[test]
    fn go_to_page_goes_to_the_typed_page_and_cancel_stays() {
        let mut app = app_of(4);
        app.canvas_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(1400.0, 900.0)));
        crate::canvas::go_to_page(&mut app, 0);
        app.ui.dialog = Some(Dialog::new("goToPage", json!({})));
        let mut h = open(app);
        assert_eq!(dialog(&h).s("page"), "1");
        type_into(&mut h, Role::TextInput, "Page", "3");
        click(&mut h, Role::Button, "Cancel");
        assert_eq!(crate::canvas::current_page(h.state()), Some(0), "Cancel does not go");
        h.state_mut().ui.dialog = Some(Dialog::new("goToPage", json!({})));
        steps(&mut h, 3);
        type_into(&mut h, Role::TextInput, "Page", "3");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(crate::canvas::current_page(h.state()), Some(2));
    }

    #[test]
    fn insert_pages_inserts_the_count_where_asked_in_one_undo_step() {
        let mut app = app_of(3);
        crate::dialogs::open_insert_pages(&mut app, 0);
        let mut h = open(app);
        assert!(!h.get_by_role_and_label(Role::ComboBox, "Page").accesskit_node().is_disabled(), "After Page takes a page");
        type_into(&mut h, Role::SpinButton, "Pages:", "2");
        key(&mut h, Key::Tab);
        assert_eq!(dialog(&h).n("count"), Some(2.0));
        pick(&mut h, "Insert:", "At End of Document");
        assert_eq!(dialog(&h).s("insert"), "end");
        assert!(h.get_by_role_and_label(Role::ComboBox, "Page").accesskit_node().is_disabled(), "At End needs no page");
        assert_eq!(pages(&h), 3, "the draft changes nothing until OK");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(pages(&h), 5);
        assert_eq!(journal(&h, "layout.pages.insert"), vec![json!({"count": 2, "after": 2, "parent": null})]);
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(pages(&h), 3);
    }

    #[test]
    fn insert_pages_cancel_inserts_nothing() {
        let mut app = app_of(3);
        crate::dialogs::open_insert_pages(&mut app, 1);
        let mut h = open(app);
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
        assert_eq!(pages(&h), 3);
        assert!(journal(&h, "layout.pages.insert").is_empty());
    }

    #[test]
    fn pages_panel_options_apply_every_choice_to_the_panel() {
        use crate::panels::pages::{IconSize, PanelResize};
        let mut app = app_of(2);
        crate::panels::pages::open_panel_options(&mut app);
        let was = app.ui.pages_panel.clone();
        let mut h = open(app);
        assert_eq!(h.query_all_by_role_and_label(Role::ComboBox, "Size:").count(), 2, "page and parent sizes");
        pick(&mut h, "Size:", "Jumbo");
        assert_eq!(dialog(&h).s("pageSize"), "jumbo");
        click(&mut h, Role::RadioButton, "Pages on Top");
        assert_eq!(dialog(&h).s("layout"), "pagesOnTop");
        click(&mut h, Role::CheckBox, "Show Vertically");
        assert_eq!(dialog(&h).b("parentsVertical"), !was.parents_vertical);
        pick(&mut h, "Resize:", "Proportional");
        assert_eq!(h.state().ui.pages_panel, was, "the draft changes nothing until OK");
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let o = &h.state().ui.pages_panel;
        assert_eq!(
            (o.page_size, o.pages_on_top, o.parents_vertical, o.resize),
            (IconSize::Jumbo, true, !was.parents_vertical, PanelResize::Proportional)
        );
        assert_eq!((o.parent_size, o.page_thumbnails, o.parent_thumbnails), (was.parent_size, was.page_thumbnails, was.parent_thumbnails));
    }

    #[test]
    fn pages_panel_options_extra_small_pages_have_no_thumbnail_choice() {
        let mut app = app_of(2);
        crate::panels::pages::open_panel_options(&mut app);
        let mut h = open(app);
        let thumbnails = |h: &Window| {
            h.query_all_by_role_and_label(Role::CheckBox, "Show Thumbnails").map(|n| n.accesskit_node().is_disabled()).collect::<Vec<_>>()
        };
        assert_eq!(thumbnails(&h), [false, false]);
        pick(&mut h, "Size:", "Extra Small");
        assert_eq!(dialog(&h).s("pageSize"), "extraSmall");
        assert_eq!(thumbnails(&h), [true, false], "only the pages' choice is out");
        click(&mut h, Role::Button, "Cancel");
        assert!(h.state().ui.dialog.is_none());
    }

    fn parent_dialog() -> (DesignApp, designcraft_doc::SpreadId, designcraft_doc::SpreadId) {
        let mut app = app_of(2);
        app.run("layout.parents.new", json!({"prefix": "B"})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        let (a, b) = (doc.parents[0].id, doc.parents[1].id);
        crate::panels::pages::open_parent_options(&mut app, &doc, b);
        assert_eq!(app.ui.dialog.as_ref().map(|d| d.id.as_str()), Some("parentOptions"));
        (app, a, b)
    }

    #[test]
    fn parent_options_rename_and_rebase_the_parent() {
        let (app, a, b) = parent_dialog();
        let mut h = open(app);
        assert_eq!((dialog(&h).s("prefix"), dialog(&h).s("name")), ("B".to_string(), "Parent".to_string()));
        type_into(&mut h, Role::TextInput, "Prefix:", "CV");
        type_into(&mut h, Role::TextInput, "Name:", "Cover");
        pick(&mut h, "Based on Parent:", "A-Parent");
        assert_eq!(dialog(&h).s("basedOn"), a.0.to_string());
        click(&mut h, Role::Button, "OK");
        assert!(h.state().ui.dialog.is_none());
        let doc = &h.state().session.doc().unwrap().doc;
        let info = doc.parents[1].parent.as_ref().unwrap();
        assert_eq!((info.label(), info.based_on), ("CV-Cover".into(), Some(a)));
        assert_eq!(journal(&h, "layout.parents.options"), vec![json!({"parent": b.0, "prefix": "CV", "name": "Cover", "basedOn": a.0})]);
    }

    #[test]
    fn parent_options_a_refused_prefix_keeps_the_dialog_with_what_was_typed_and_why() {
        let (app, _, _) = parent_dialog();
        let mut h = open(app);
        type_into(&mut h, Role::TextInput, "Prefix:", "A");
        click(&mut h, Role::Button, "OK");
        assert_eq!(h.state().ui.dialog.as_ref().map(|d| d.id.as_str()), Some("parentOptions"), "the dialog stays open to be fixed");
        assert_eq!(dialog(&h).s("prefix"), "A");
        let reason = dialog(&h).s("status");
        assert!(!reason.is_empty() && shows(&h, &reason), "the reason shows: {reason:?}");
        let doc = &h.state().session.doc().unwrap().doc;
        assert_eq!(doc.parents[1].parent.as_ref().unwrap().prefix, "B", "nothing changed");
    }

    /// One Escape cancels each of these dialogs with no command, as InDesign's do.
    #[test]
    fn escape_cancels_each_simple_dialog_without_a_command() {
        let opened: Vec<Box<dyn Fn() -> DesignApp>> = vec![
            Box::new(|| {
                let mut app = app_of(1);
                app.ui.dialog = Some(Dialog::new("alert", json!({"title": "Notice", "message": "Done."})));
                app
            }),
            Box::new(unsaved),
            Box::new(with_object_on_page_two),
            Box::new(workspace_dialog),
            Box::new(|| {
                let mut app = app_of(3);
                crate::dialogs::open_insert_pages(&mut app, 0);
                app
            }),
            Box::new(|| {
                let mut app = app_of(1);
                crate::panels::pages::open_panel_options(&mut app);
                app
            }),
            Box::new(|| parent_dialog().0),
        ];
        for open_dialog in opened {
            let app = open_dialog();
            let id = app.ui.dialog.as_ref().unwrap().id.clone();
            let (journal_before, pages_before) = (app.session.journal.len(), app.session.doc().map(|s| s.doc.page_count()));
            let mut h = open(app);
            key(&mut h, Key::Escape);
            assert!(h.state().ui.dialog.is_none(), "one Escape closes {id}");
            assert_eq!(h.state().session.journal.len(), journal_before, "{id}: Escape runs no command");
            assert_eq!(h.state().session.doc().map(|s| s.doc.page_count()).ok(), pages_before.ok(), "{id}: the document is unchanged");
        }
    }
}
