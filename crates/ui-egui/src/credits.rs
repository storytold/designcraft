//! The About window's credits: every contributor (GitHub login always; display and real names only
//! with recorded consent) and the AI models that helped. The tables are baked into the binary by
//! build.rs from contributors/contributors.json; nothing is
//! read at run time.

use std::cmp::Ordering;

use crate::i18n::{tr, tr_context};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub login: &'static str,
    pub display_name: Option<&'static str>,
    pub real_name: Option<&'static str>,
    pub prs: u64,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
    pub binary_added: u64,
    pub binary_deleted: u64,
    /// ISO 8601 UTC; empty for someone credited only through a merged PR.
    pub first_commit: &'static str,
    pub last_commit: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub company: &'static str,
    pub model: &'static str,
    pub version: &'static str,
    pub commits: u64,
    pub lines_added: u64,
    pub lines_deleted: u64,
}

include!(concat!(env!("OUT_DIR"), "/credits.rs"));

/// Which name to show. Display and real names fall back to the `@username` when not given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NameMode {
    #[default]
    Username,
    DisplayName,
    RealName,
}

impl NameMode {
    pub const ALL: [NameMode; 3] = [NameMode::Username, NameMode::DisplayName, NameMode::RealName];

    pub fn label(self) -> &'static str {
        match self {
            NameMode::Username => "Username",
            NameMode::DisplayName => "Display name",
            NameMode::RealName => "Real name",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Prs,
    Commits,
    LinesAdded,
    LinesDeleted,
    LinesDelta,
    BinaryAdded,
    BinaryDeleted,
    #[default]
    FirstCommit,
    LastCommit,
}

impl SortKey {
    pub const ALL: [SortKey; 10] = [
        SortKey::Name,
        SortKey::Prs,
        SortKey::Commits,
        SortKey::LinesAdded,
        SortKey::LinesDeleted,
        SortKey::LinesDelta,
        SortKey::BinaryAdded,
        SortKey::BinaryDeleted,
        SortKey::FirstCommit,
        SortKey::LastCommit,
    ];

    /// Menu label and table header.
    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            SortKey::Name => ("Name (A–Z)", "Name"),
            SortKey::Prs => ("Merged PRs", "PRs"),
            SortKey::Commits => ("Commits", "Commits"),
            SortKey::LinesAdded => ("Lines added", "+LOC"),
            SortKey::LinesDeleted => ("Lines deleted", "−LOC"),
            SortKey::LinesDelta => ("Line delta", "ΔLOC"),
            SortKey::BinaryAdded => ("Binary assets added", "+Bin"),
            SortKey::BinaryDeleted => ("Binary assets removed", "−Bin"),
            SortKey::FirstCommit => ("First commit", "First"),
            SortKey::LastCommit => ("Last commit", "Last"),
        }
    }

    /// Names and dates read naturally oldest/A first; counts read biggest first.
    pub fn default_ascending(self) -> bool {
        matches!(self, SortKey::Name | SortKey::FirstCommit | SortKey::LastCommit)
    }
}

impl Contributor {
    pub fn name(&self, mode: NameMode) -> String {
        let chosen = match mode {
            NameMode::Username => None,
            NameMode::DisplayName => self.display_name,
            NameMode::RealName => self.real_name,
        };
        chosen.map_or_else(|| format!("@{}", self.login), str::to_string)
    }

    pub fn lines_delta(&self) -> i128 {
        i128::from(self.lines_added) - i128::from(self.lines_deleted)
    }

    /// One line with everything we know, for tooltips.
    pub fn summary(&self) -> String {
        format!(
            "@{}: {} PRs, {} commits, +{} / −{} lines (Δ {}), +{} / −{} binary assets, {} – {}",
            self.login,
            self.prs,
            self.commits,
            group(self.lines_added),
            group(self.lines_deleted),
            signed(self.lines_delta()),
            self.binary_added,
            self.binary_deleted,
            day(self.first_commit),
            day(self.last_commit),
        )
    }
}

/// Alphabetical key: case-insensitive, ignoring the `@` of usernames.
fn name_key(name: &str) -> String {
    name.trim_start_matches('@').to_lowercase()
}

fn compare(a: &Contributor, b: &Contributor, mode: NameMode, key: SortKey) -> Ordering {
    match key {
        SortKey::Name => name_key(&a.name(mode)).cmp(&name_key(&b.name(mode))),
        SortKey::Prs => a.prs.cmp(&b.prs),
        SortKey::Commits => a.commits.cmp(&b.commits),
        SortKey::LinesAdded => a.lines_added.cmp(&b.lines_added),
        SortKey::LinesDeleted => a.lines_deleted.cmp(&b.lines_deleted),
        SortKey::LinesDelta => a.lines_delta().cmp(&b.lines_delta()),
        SortKey::BinaryAdded => a.binary_added.cmp(&b.binary_added),
        SortKey::BinaryDeleted => a.binary_deleted.cmp(&b.binary_deleted),
        // A PR-only contributor (no commit date) sorts after everyone with a date.
        SortKey::FirstCommit => dated(a.first_commit).cmp(&dated(b.first_commit)),
        SortKey::LastCommit => dated(a.last_commit).cmp(&dated(b.last_commit)),
    }
}

fn dated(d: &str) -> (bool, &str) {
    (d.is_empty(), d)
}

/// Contributors in display order. Ties fall back to the name (A–Z) so the order is stable.
pub fn sorted(list: &[Contributor], mode: NameMode, key: SortKey, ascending: bool) -> Vec<&Contributor> {
    let mut v: Vec<&Contributor> = list.iter().collect();
    v.sort_by(|a, b| {
        let o = compare(a, b, mode, key);
        let o = if ascending { o } else { o.reverse() };
        o.then_with(|| compare(a, b, mode, SortKey::Name)).then_with(|| a.login.cmp(b.login))
    });
    v
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn signed(n: i128) -> String {
    let g = group(u64::try_from(n.unsigned_abs()).unwrap_or(u64::MAX));
    if n < 0 { format!("−{g}") } else { format!("+{g}") }
}

fn day(iso: &str) -> &str {
    if iso.is_empty() { "—" } else { iso.get(..10).unwrap_or(iso) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct View {
    names: NameMode,
    key: SortKey,
    ascending: bool,
    table: bool,
}

impl Default for View {
    fn default() -> Self {
        View { names: NameMode::Username, key: SortKey::FirstCommit, ascending: true, table: false }
    }
}

/// The name toggle, sort and layout controls above the list, as framework components: toggle buttons, a select for the sort
/// key and the reverse button.
fn controls_framework(ui: &mut egui::Ui, v: &mut View, lang: &str) {
    use craft_ui_widgets::buttons::Button;
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
    use craft_ui_widgets::display::{Divider, Text};
    use craft_ui_widgets::layout_components::Row;
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::style::Variant;

    Row::new().id_salt("credits_controls").wrap(true).show(ui, |ui, _| {
        Text::new(tr(lang, "Show").to_owned()).show(ui);
        for m in NameMode::ALL {
            if Button::new(tr(lang, m.label()).to_owned())
                .id_salt(("credits_names", m.label()))
                .variant(Variant::GHOST)
                .selected(v.names == m)
                .show(ui)
                .clicked()
            {
                v.names = m;
            }
        }
        Divider::vertical().show(ui);
        Text::new(tr(lang, "Sort").to_owned()).show(ui);
        let entries: Vec<Entry<usize>> = SortKey::ALL.iter().enumerate().map(|(i, k)| Entry::new(i, tr(lang, k.label().0).to_owned())).collect();
        let at = SortKey::ALL.iter().position(|k| *k == v.key).unwrap_or(0);
        let label = tr(lang, "Sort").to_owned();
        let picked = Select::new(label, Bind::given(at), &entries).id_salt("credits_sort").show(ui).events.committed().copied();
        if let Some(k) = picked.and_then(|i| SortKey::ALL.get(i).copied()).filter(|k| *k != v.key) {
            v.key = k;
            v.ascending = k.default_ascending();
        }
        let reverse = tr(lang, "Reverse the order").to_owned();
        if Button::new(reverse.clone()).painted(if v.ascending { "▲" } else { "▼" }).tooltip(reverse).id_salt("credits_reverse").show(ui).clicked()
        {
            v.ascending = !v.ascending;
        }
        Divider::vertical().show(ui);
        if Button::new(tr(lang, "Grab bag").to_owned()).id_salt("credits_grab_bag").variant(Variant::GHOST).selected(!v.table).show(ui).clicked() {
            v.table = false;
        }
        if Button::new(tr(lang, "Table").to_owned()).id_salt("credits_table").variant(Variant::GHOST).selected(v.table).show(ui).clicked() {
            v.table = true;
        }
    });
}

/// One contributor's table cells after the name, in `SortKey::ALL` order.
fn cells(c: &Contributor) -> [String; 9] {
    [
        group(c.prs),
        group(c.commits),
        group(c.lines_added),
        group(c.lines_deleted),
        signed(c.lines_delta()),
        group(c.binary_added),
        group(c.binary_deleted),
        day(c.first_commit).to_string(),
        day(c.last_commit).to_string(),
    ]
}

/// The rows of a framework table or list over `count` items, named by their text.
struct Rows(Vec<String>);

impl craft_behavior::DisplayOrder<usize> for Rows {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn key(&self, at: usize) -> usize {
        at
    }

    fn index_of(&self, key: &usize) -> Option<usize> {
        (*key < self.0.len()).then_some(*key)
    }

    fn text(&self, at: usize) -> Option<std::borrow::Cow<'_, str>> {
        self.0.get(at).map(|name| std::borrow::Cow::Borrowed(name.as_str()))
    }
}

impl craft_behavior::ItemSource<usize> for Rows {}

/// About ▸ Contributors on the framework: the name toggle, sort and layout controls, then the list as a grab bag of links or
/// as a sortable table. The link that was activated (a click on a name, or Enter on a table row) comes back as the URL to open.
pub fn contributors_framework(ui: &mut egui::Ui, lang: &str) -> Option<String> {
    use craft_behavior::{ColumnKey, ColumnWidth, Selection};
    use craft_ui_widgets::buttons::Link;
    use craft_ui_widgets::contract::{
        CollectionEvent, ColumnEvent, ColumnSort, ColumnSpec, Props as _, Scroll, Show as _, ShowCollection as _, ShowContainer as _, Sizing,
    };
    use craft_ui_widgets::display::{Divider, Shade, Text};
    use craft_ui_widgets::layout_components::{Row, Stack};
    use craft_ui_widgets::list::ListRow;
    use craft_ui_widgets::style::TypeRole;
    use craft_ui_widgets::table::Table;

    let id = egui::Id::unique("credits_view");
    let mut v = ui.data_mut(|d| d.get_temp::<View>(id)).unwrap_or_default();
    controls_framework(ui, &mut v, lang);
    let list = sorted(CONTRIBUTORS, v.names, v.key, v.ascending);
    let totals = tr(lang, "{contributors} contributors · {commits} commits")
        .replace("{contributors}", &list.len().to_string())
        .replace("{commits}", &group(TOTAL_COMMITS));
    Text::new(totals).role(TypeRole::Caption).shade(Shade::Muted).sizing(Sizing::FillWidth).show(ui);
    Divider::horizontal().show(ui);
    let mut open: Option<String> = None;
    if list.is_empty() {
        Text::new(tr(lang, "No contributor data was built into this copy.").to_owned()).sizing(Sizing::FillWidth).show(ui);
    } else if v.table {
        let columns: Vec<ColumnSpec> = SortKey::ALL
            .iter()
            .enumerate()
            .map(|(i, &k)| {
                let width = if i == 0 { ColumnWidth::fill(1.0) } else { ColumnWidth::Content };
                let mut column = ColumnSpec::new(ColumnKey::index(i as u32), tr_context(lang, k.label().1, "contributors").to_owned(), width)
                    .sortable()
                    .resizable();
                if i > 0 {
                    column = column.numeric();
                }
                if !k.default_ascending() {
                    column = column.first_descending();
                }
                column
            })
            .collect();
        let rows: Vec<[String; 9]> = list.iter().map(|c| cells(c)).collect();
        let names = Rows(list.iter().map(|c| c.name(v.names)).collect());
        let sort_at = SortKey::ALL.iter().position(|k| *k == v.key).unwrap_or(0);
        let out = Table::<usize>::new(tr(lang, "Contributors").to_owned(), columns)
            .striped(true)
            .sort(ColumnSort::new(ColumnKey::index(sort_at as u32), !v.ascending))
            .max_rows(14)
            .selection(craft_ui_widgets::contract::Bind::given(Selection::default()))
            .show_items(ui, &names, &mut |at, icx| {
                let mut row = ListRow::new(list[*at].name(v.names)).cells(icx.cells()).striped(icx.row.striped);
                for (col, text) in rows[*at].iter().enumerate() {
                    row = row.cell(ColumnKey::index(col as u32 + 1), text.clone());
                }
                row
            });
        for event in out.events {
            match event {
                CollectionEvent::Column(ColumnEvent::Sort { key, descending, .. }) => {
                    let at = SortKey::ALL.iter().enumerate().find(|(i, _)| ColumnKey::index(*i as u32) == key).map(|(_, k)| *k);
                    if let Some(k) = at {
                        v.key = k;
                        v.ascending = !descending;
                    }
                }
                CollectionEvent::Activate(at) => open = list.get(at).map(|c| format!("https://github.com/{}", c.login)),
                _ => {}
            }
        }
    } else {
        Stack::new().id_salt("credits_bag").fill_width().scroll(Scroll::vertical()).room_height(320.0).show(ui, |ui, _| {
            Row::new().id_salt("credits_names").wrap(true).show(ui, |ui, _| {
                for (i, c) in list.iter().enumerate() {
                    if i > 0 {
                        Text::new("·".to_owned()).shade(Shade::Muted).show(ui);
                    }
                    let url = format!("https://github.com/{}", c.login);
                    if Link::new(c.name(v.names)).url(url.clone()).tooltip(c.summary()).id_salt(("credit", c.login)).show(ui).clicked() {
                        open = Some(url);
                    }
                }
            });
        });
    }
    ui.data_mut(|d| d.insert_temp(id, v));
    open
}

/// About ▸ Models on the framework: AI models credited in Co-Authored-By trailers, in a table.
pub fn models_framework(ui: &mut egui::Ui, lang: &str) {
    use craft_behavior::{ColumnKey, ColumnWidth, Selection};
    use craft_ui_widgets::contract::{ColumnSpec, Props as _, Show as _, ShowCollection as _, Sizing};
    use craft_ui_widgets::display::Text;
    use craft_ui_widgets::list::ListRow;
    use craft_ui_widgets::table::Table;

    if MODELS.is_empty() {
        Text::new(tr(lang, "No model credits were built into this copy.").to_owned()).sizing(Sizing::FillWidth).show(ui);
        return;
    }
    let assisted: u64 = MODELS.iter().map(|m| m.commits).max().unwrap_or(0).max(1);
    let titles = ["Company", "Model", "Version", "Commits", "% of all commits", "Lines +/−"];
    // Company, Model and Version read as text; the counts line up on the right.
    let right = [false, false, false, true, true, true];
    let grow = [1.0, 2.0, 0.0, 0.0, 0.0, 0.0];
    let columns: Vec<ColumnSpec> = titles
        .iter()
        .enumerate()
        .map(|(i, title)| {
            let width = if grow[i] > 0.0 { ColumnWidth::fill(grow[i]) } else { ColumnWidth::Content };
            let column = ColumnSpec::new(ColumnKey::index(i as u32), tr(lang, title).to_owned(), width).resizable();
            if right[i] { column.numeric() } else { column }
        })
        .collect();
    let rows: Vec<[String; 6]> = MODELS
        .iter()
        .map(|m| {
            [
                m.company.to_string(),
                m.model.to_string(),
                m.version.to_string(),
                group(m.commits),
                format!("{:.0}%", 100.0 * m.commits as f64 / TOTAL_COMMITS.max(assisted) as f64),
                format!("+{} / −{}", group(m.lines_added), group(m.lines_deleted)),
            ]
        })
        .collect();
    let names = Rows(MODELS.iter().map(|m| format!("{} {}", m.company, m.model)).collect());
    let _ = Table::<usize>::new(tr(lang, "Models").to_owned(), columns)
        .striped(true)
        .max_rows(14)
        .selection(craft_ui_widgets::contract::Bind::given(Selection::default()))
        .show_items(ui, &names, &mut |at, icx| {
            // The first column shows the row's own label: the company.
            let mut row = ListRow::new(rows[*at][0].clone()).cells(icx.cells()).striped(icx.row.striped);
            for (col, text) in rows[*at].iter().enumerate().skip(1) {
                row = row.cell(ColumnKey::index(col as u32), text.clone());
            }
            row
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(
        login: &'static str,
        display: Option<&'static str>,
        real: Option<&'static str>,
        added: u64,
        deleted: u64,
        first: &'static str,
    ) -> Contributor {
        Contributor {
            login,
            display_name: display,
            real_name: real,
            prs: 0,
            commits: 1,
            lines_added: added,
            lines_deleted: deleted,
            binary_added: 0,
            binary_deleted: 0,
            first_commit: first,
            last_commit: first,
        }
    }

    fn people() -> [Contributor; 4] {
        [
            c("zed", None, None, 5, 50, "2026-01-02T00:00:00Z"),
            c("Bob", Some("Ann"), None, 10, 0, "2026-01-01T00:00:00Z"),
            c("alice", None, None, 7, 1, ""),
            c("carol", None, Some("Dana"), 1, 0, "2026-01-03T00:00:00Z"),
        ]
    }

    fn logins(v: &[&Contributor]) -> Vec<&'static str> {
        v.iter().map(|c| c.login).collect()
    }

    #[test]
    fn names_fall_back_to_the_username() {
        let p = c("bob", Some("Bobby"), None, 0, 0, "");
        assert_eq!(p.name(NameMode::Username), "@bob");
        assert_eq!(p.name(NameMode::DisplayName), "Bobby");
        assert_eq!(p.name(NameMode::RealName), "@bob");
    }

    #[test]
    fn alphabetical_ignores_at_and_case() {
        let all = people();
        let v = sorted(&all, NameMode::Username, SortKey::Name, true);
        assert_eq!(logins(&v), ["alice", "Bob", "carol", "zed"]);
        let v = sorted(&all, NameMode::DisplayName, SortKey::Name, true);
        assert_eq!(logins(&v), ["alice", "Bob", "carol", "zed"]); // "Ann" sorts with the @usernames
        let v = sorted(&all, NameMode::RealName, SortKey::Name, false);
        assert_eq!(logins(&v), ["zed", "carol", "Bob", "alice"]); // "Dana" between bob and zed
    }

    #[test]
    fn delta_and_dates() {
        let all = people();
        let v = sorted(&all, NameMode::Username, SortKey::LinesDelta, false);
        assert_eq!(logins(&v), ["Bob", "alice", "carol", "zed"]);
        let v = sorted(&all, NameMode::Username, SortKey::FirstCommit, true);
        assert_eq!(logins(&v), ["Bob", "zed", "carol", "alice"]); // undated last
    }

    #[test]
    fn formatting() {
        assert_eq!(group(1234567), "1,234,567");
        assert_eq!(group(12), "12");
        assert_eq!(signed(-1000), "−1,000");
        assert_eq!(day("2026-10-07T18:45:23Z"), "2026-10-07");
        assert_eq!(day(""), "—");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod successor_tests;
