//! DesignCraft's half of Open and Save against cloud storage.
//!
//! The pages themselves are `craft_ui::cloud`. Every Craft app offers the same
//! ones. What is here is what only DesignCraft can answer. A document is a
//! layout. These extensions and these content types are what that means.
//! Bytes become one through `file.openBytes` and go back through
//! `designcraft_format::save`. The words are the ones `i18n` has.
//!
//! DesignCraft holds no provider credential and speaks no provider protocol.
//! Every call goes to the CollabCraft service, which is the single place a
//! provider is configured and permissions are decided. Replacing Nextcloud
//! with something else changes nothing here.
//!
//! Nothing in this file opens a socket either. The transport half is a
//! [`CloudBackend`] the host application supplies (see `apps/designcraft/cloud`).
//! Every CollabCraft call blocks. A blocking read of a forty megabyte layout
//! inside a frame would freeze the window for seconds. So the pages state a
//! [`Job`], the host runs it on a worker thread, and the answer arrives on a
//! channel that [`pump`] drains on a later frame.
//!
//! DesignCraft has no Backstage view. The pages are therefore drawn in two
//! places of its own. The start screen is its Open page. [`open_section`] puts
//! the Cloud section there. [`modal`] is the same two pages over the window,
//! for a File menu that has to reach them with a layout already open.

use std::collections::HashMap;
use std::sync::Arc;

// Re-exported so a host application, and a test, can name these types without
// depending on the crates they come from.
pub use craft_cloud::{Capability, Entry, EntryKind};
pub use craft_ui::cloud::{
    Account, Busy, CloudApp, CloudBackend, CloudFiles, Conflict, CopyFormat, DocumentKind, Event, Fault, Folder, Job, Link, Origin, Post, Source,
    UNAVAILABLE, available, browse, can_converse, conflict_modal, destination_name, has_target, link, load_shared, open_entry, open_page, origin,
    parent_of, prefers_cloud, progress, pump, reload_active, save, save_as, save_page, share_button, shares, sign_in, sign_out, start, who,
    work_together,
};
use designcraft_doc::Document;
use egui::{Color32, Painter, Rect, Ui};
use serde_json::{Value as Json, json};

use crate::DesignApp;
use crate::theme::Tokens;

/// Extensions DesignCraft opens. The same set the desktop file picker offers
/// (see the document filters in `apps/designcraft/src/main.rs`).
const OPENS: [&str; 2] = ["designcraft", "idml"];

/// Content types DesignCraft opens, matched against the start of what the
/// provider said. Read beside the extension because neither is reliable
/// alone. A WebDAV server hands back `application/octet-stream` for a
/// perfectly good layout, and a file exported by a web application often has
/// a correct content type and no extension at all.
const MIMES: [&str; 2] = ["application/vnd.designcraft+zip", "application/vnd.adobe.indesign-idml-package"];

/// What a DesignCraft document is, as a cloud file list needs to know.
pub const LAYOUT: DocumentKind = DocumentKind { extensions: &OPENS, mimes: &MIMES, noun: "layout", default_extension: "designcraft" };

/// The Save a Copy formats the Share menu offers under the share itself.
///
/// The identifier is the UI command the chosen format runs. Each one writes a
/// copy and leaves the open layout as it is.
const COPIES: [(&str, &str); 4] = [
    ("app.saveCopyDialog", "Save a Copy as DesignCraft Layout (.designcraft)…"),
    ("app.exportPdf", "Export as PDF…"),
    ("app.exportIdml", "Export IDML…"),
    ("app.exportEpub", "Export EPUB…"),
];

/// The layout snapshot each save in flight encoded.
///
/// Only DesignCraft knows what a version of a layout is. The pages hold none
/// of these. A save that succeeds clears the dirty flag for exactly the
/// version that reached the server. An edit made while the bytes were on the
/// wire stays unsaved.
pub type Sent = HashMap<u64, Arc<Document>>;

/// Which cloud page the window is showing over the layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// The account's own files and the ones other people shared.
    Open,
    /// A folder and a name to write under.
    Save,
}

/// Whether DesignCraft can open this entry.
pub fn opens(entry: &Entry) -> bool {
    craft_ui::cloud::opens(entry, LAYOUT)
}

/// A typed name as a layout file name. Any folder separators are dropped and
/// a missing extension becomes `.designcraft`.
pub fn layout_name(typed: &str) -> String {
    craft_ui::cloud::document_name(typed, LAYOUT)
}

/// The whole of the cloud state, for the control channel and for tests.
pub fn status(app: &DesignApp) -> Json {
    craft_ui::cloud::report(app)
}

/// The Share menu's export half, as the control it is drawn in wants it.
pub fn copies() -> Vec<CopyFormat> {
    COPIES.iter().map(|(id, label)| CopyFormat::new(*id, *label)).collect()
}

/// The host part of the service address, for the account menu's first line.
///
/// Read from the environment and compiled in nowhere, because the service is
/// deployment-specific. Somebody with an account on two deployments needs to
/// know which one a layout is about to land in.
fn service_host() -> String {
    service_url().map(|value| value.trim_start_matches("https://").trim_start_matches("http://").to_owned()).unwrap_or_default()
}

/// Where the CollabCraft service is, as the environment gave it.
///
/// Public because the host application reads the same answer to decide whether
/// to start a worker at all. Two spellings of one setting would be a setting
/// that works in one half of the application.
pub fn service_url() -> Option<String> {
    ["DESIGNCRAFT_CLOUD_URL", "CRAFT_CLOUD_URL", "COLLABCRAFT_URL"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .find(|value| !value.is_empty())
}

impl CloudApp for DesignApp {
    fn cloud(&self) -> &CloudFiles {
        &self.cloud
    }

    fn cloud_mut(&mut self) -> &mut CloudFiles {
        &mut self.cloud
    }

    fn kind(&self) -> DocumentKind {
        LAYOUT
    }

    fn active(&self) -> Option<u64> {
        self.session.active().map(|d| d.uid)
    }

    fn title(&self, uid: u64) -> String {
        self.session.documents().iter().find(|d| d.uid == uid).map(|d| d.title()).unwrap_or_default()
    }

    fn open_bytes(&mut self, name: &str, bytes: Vec<u8>) -> Result<Option<u64>, String> {
        let encoded = designcraft_engine::cmd::base64_encode(&bytes);
        // A failure is handed back. The pages name the file that could not be
        // opened and say why. The warnings of one that did open are this
        // side's to report, the same way a layout opened from disk does.
        let result = self.run("file.openBytes", json!({"name": name, "base64": encoded}))?;
        self.cloud_page = None;
        self.show_open_warnings(name, &result);
        Ok(self.session.active().map(|d| d.uid))
    }

    fn encode(&mut self, uid: u64) -> Result<Vec<u8>, String> {
        let doc = self.session.documents().iter().find(|d| d.uid == uid).ok_or("no layout open")?;
        let snapshot = doc.doc.clone();
        // `designcraft_format::save` has a failure to hand back.
        // `cmd::to_bytes` turns one into empty bytes. Those would be written
        // over the stored layout as if they were a save.
        let bytes = designcraft_format::save(&snapshot).map_err(|e| e.to_string())?;
        self.cloud_sent.insert(uid, snapshot);
        Ok(bytes)
    }

    fn close(&mut self, uid: u64) {
        if let Some(index) = self.session.documents().iter().position(|d| d.uid == uid) {
            self.session.close_document(index);
        }
    }

    fn mark_saved(&mut self, uid: u64) {
        let Some(snapshot) = self.cloud_sent.remove(&uid) else { return };
        if let Some(state) = self.session.document_mut(uid) {
            state.saved_revision = state.revision;
            state.saved_doc = snapshot;
        }
    }

    fn drop_pending(&mut self, uid: u64) {
        self.cloud_sent.remove(&uid);
    }

    fn sharing(&mut self) -> &mut craft_ui::share::ShareDialog {
        &mut self.share
    }

    fn presence(&mut self) -> &mut craft_ui::presence::PresenceBar {
        &mut self.presence
    }

    fn paint_icon(&self, painter: &Painter, rect: Rect, kind: EntryKind, colour: Color32) {
        let icon = if kind == EntryKind::Folder { "folder" } else { "document" };
        crate::icons::paint(painter, rect, icon, colour);
    }

    fn open_url(&self, url: &str) {
        // A host that handed in its own opener gets it. A test is what wants
        // one. Otherwise egui's own, the route every other link in DesignCraft
        // already takes to a browser.
        if let Some(open) = &self.services.open_url {
            open(url);
        } else if let Some(ctx) = &self.egui_ctx {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
    }

    fn say(&mut self, text: String) {
        self.ui.dialog = Some(crate::dialogs::Dialog::new("alert", json!({"title": "DesignCraft", "message": text})));
    }

    fn toast(&mut self, text: String) {
        self.status(text);
    }

    fn go_to_save_as(&mut self) {
        self.cloud_page = Some(Page::Save);
        self.save_to_cloud = Some(true);
    }

    fn app_name(&self) -> &str {
        "DesignCraft"
    }

    fn tr(&self, text: &str) -> String {
        crate::i18n::tr(&self.ui.language, text).to_owned()
    }

    fn service(&self) -> String {
        service_host()
    }

    fn save_note(&self) -> Option<String> {
        Some(self.tr("The layout is saved as a DesignCraft layout (.designcraft) in the folder above."))
    }

    fn style(&self, ui: &Ui) -> craft_ui::cloud::CloudStyle {
        let t = Tokens::get(ui.ctx());
        craft_ui::cloud::CloudStyle {
            surface: t.panel,
            border: t.border,
            text: t.text,
            dim: t.text_dim,
            hover: t.hover,
            soft: t.row_selected,
            family: egui::FontFamily::Proportional,
            bold: egui::FontFamily::Name("semibold".into()),
        }
    }
}

// ---------------------------------------------------------------- where the pages are drawn

/// The Cloud section of the start screen, DesignCraft's Open page.
///
/// Beside the local picker and never in place of it. A build with no service
/// draws none of it. An account badge over an empty list reads to a user as a
/// broken window.
pub fn open_section(app: &mut DesignApp, ui: &mut Ui) {
    if !available(app) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.add_space(22.0);
    ui.label(crate::rtl::widget(
        ui,
        egui::RichText::new(crate::i18n::tr(&app.ui.language, "Cloud")).font(crate::theme::semibold(15.0)).color(t.text_strong),
    ));
    ui.add_space(8.0);
    open_page(app, ui);
}

/// The Open and Save pages over the window, for the File menu.
///
/// A modal. That is how DesignCraft asks for a file name everywhere else. The
/// page stands in for the window's own answer to the same question.
pub fn modal(app: &mut DesignApp, ctx: &egui::Context) {
    let Some(page) = app.cloud_page else { return };
    let lang = app.ui.language.clone();
    let title = match page {
        Page::Open => crate::i18n::tr(&lang, "Open from the Cloud"),
        Page::Save => crate::i18n::tr(&lang, "Save to the Cloud"),
    };
    let mut close = false;
    egui::Modal::new(egui::Id::new("cloud_page")).show(ctx, |ui| {
        let margin = egui::Frame::popup(ui.style()).total_margin().sum();
        let room = (ctx.content_rect().size() - margin - egui::vec2(16.0, 16.0)).max(egui::Vec2::splat(1.0));
        let width = 680.0_f32.min(room.x);
        ui.set_min_width(380.0_f32.min(width));
        ui.set_max_width(width);
        crate::rtl::label(ui, egui::RichText::new(title).font(crate::theme::semibold(16.0)));
        ui.add_space(10.0);
        let body = (room.y - ui.min_size().y - 48.0).max(1.0);
        egui::ScrollArea::vertical().id_salt(("cloud_page", page == Page::Save)).max_height(body).show(ui, |ui| match page {
            Page::Open => open_page(app, ui),
            Page::Save => close = save_body(app, ui),
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Close, not Cancel. The page's own buttons do the committing,
                // and a sign-in waiting for the browser offers a Cancel of its
                // own a few lines above this one.
                if ui.button(crate::rtl::widget(ui, crate::i18n::tr(&lang, "Close"))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
    });
    // Opening a layout clears the page itself, so this only has to handle the
    // ways the user leaves without one.
    if close {
        app.cloud_page = None;
    }
}

/// Save As: where to write, then the page for whichever was picked.
///
/// True once the layout has gone somewhere and the page is finished with.
fn save_body(app: &mut DesignApp, ui: &mut Ui) -> bool {
    let lang = app.ui.language.clone();
    let to_cloud = app.save_to_cloud.unwrap_or_else(|| prefers_cloud(app));
    // Named before anything is picked, so the destination reads as the user's
    // own account and not as a generic place.
    start(app);
    let account = destination_name(app);
    crate::rtl::label(ui, crate::i18n::tr(&lang, "Where"));
    ui.add_space(6.0);
    if place_row(ui, "home", crate::i18n::tr(&lang, "This Computer"), crate::i18n::tr(&lang, "A folder on this computer"), !to_cloud).clicked() {
        app.save_to_cloud = Some(false);
    }
    if place_row(ui, "share", &account, crate::i18n::tr(&lang, "Your cloud account"), to_cloud).clicked() {
        app.save_to_cloud = Some(true);
    }
    ui.add_space(18.0);
    if to_cloud {
        return save_page(app, ui);
    }
    // The local half is the system Save dialog this application already has.
    // The page is only the choice of where.
    let label = crate::i18n::tr(&lang, "Browse…").to_owned();
    if ui.button(crate::rtl::widget(ui, label)).clicked() {
        let _ = app.run("app.saveDialogLocal", json!({}));
        return true;
    }
    false
}

/// One destination to save into: an icon, a name and a second line.
fn place_row(ui: &mut Ui, icon: &str, name: &str, hint: &str, chosen: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, chosen, name));
    if chosen {
        ui.painter().rect_filled(rect, t.radius, t.row_selected);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let glyph = Rect::from_min_size(rect.min + egui::vec2(8.0, 10.0), egui::Vec2::splat(20.0));
    crate::icons::paint(ui.painter(), glyph, icon, t.icon);
    let x = rect.min.x + 36.0;
    ui.painter().text(egui::pos2(x, rect.center().y - 7.0), egui::Align2::LEFT_CENTER, name, crate::theme::semibold(12.5), t.text_strong);
    ui.painter().text(egui::pos2(x, rect.center().y + 8.0), egui::Align2::LEFT_CENTER, hint, egui::FontId::proportional(11.0), t.text_dim);
    response
}
