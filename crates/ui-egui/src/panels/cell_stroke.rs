//! Cell strokes: the edge proxy (a drawn 2 × 2 cell grid whose lines choose the cell edges a change
//! applies to) and the stroke fields shared by the Stroke panel and Cell Options › Strokes and Fills.
//!
//! The proxy's lines are the target range's top, bottom, left and right sides and the lines
//! between its rows (inner horizontal) and columns (inner vertical). A click toggles a line; a
//! double-click on a line chooses all outer or all inner lines; a triple-click chooses all lines,
//! or none when all were chosen.

use designcraft_doc::StrokeType;
use egui::{Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::DesignApp;
use crate::i18n::tr;
use crate::theme::Tokens;
use crate::widgets::{NumField, caption};

/// Edge names of the proxy lines, in proxy order; `table.setCell` takes them as `stroke.edges`.
pub const EDGE_NAMES: [&str; 6] = ["top", "bottom", "left", "right", "innerHorizontal", "innerVertical"];

/// Proxy line choice, in [`EDGE_NAMES`] order.
pub type Edges = [bool; 6];

pub const PROXY_SIZE: f32 = 64.0;
const PROXY_INSET: f32 = 8.0;

const WEIGHTS: &[f64] = &[0.0, 0.25, 0.5, 0.75, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 20.0, 30.0, 40.0, 50.0, 100.0];

/// Stroke types offered for cell edges (all of them draw on table edges).
const TYPES: &[&str] =
    &["solid", "dashed", "dotted", "thickThin", "thinThick", "thinThin", "thickThick", "thinThickThin", "thickThinThick", "wavy", "hashed"];

/// Whether the selection is table cells (or the text cursor is in a cell).
pub fn in_cells(app: &DesignApp) -> bool {
    app.session.active().is_some_and(|d| d.selection.cells.is_some() || d.selection.text.is_some_and(|t| t.cell.is_some()))
}

/// The chosen edges as the `edges` parameter.
pub fn edges_param(edges: &Edges) -> Value {
    json!(EDGE_NAMES.iter().zip(edges).filter(|(_, on)| **on).map(|(name, _)| *name).collect::<Vec<_>>())
}

/// Edges from an `edges` parameter (names); anything else chooses all.
pub fn edges_from(v: &Value) -> Edges {
    match v.as_array() {
        Some(names) => std::array::from_fn(|i| names.iter().any(|n| n.as_str() == EDGE_NAMES.get(i).copied())),
        None => [true; 6],
    }
}

const PROXY_HELP: &str =
    "Click a line to include or exclude it. Double-click selects all outer or all inner lines; triple-click selects or clears all.";

/// The proxy's lines (start, end) inside its rect, in [`EDGE_NAMES`] order.
pub fn proxy_lines(r: Rect) -> [(Pos2, Pos2); 6] {
    lines_in(r, PROXY_INSET)
}

/// The proxy's lines in `r`, the cell grid `inset` inside it.
fn lines_in(r: Rect, inset: f32) -> [(Pos2, Pos2); 6] {
    let i = r.shrink(inset);
    let c = i.center();
    [
        (i.left_top(), i.right_top()),
        (i.left_bottom(), i.right_bottom()),
        (i.left_top(), i.left_bottom()),
        (i.right_top(), i.right_bottom()),
        (pos2(i.left(), c.y), pos2(i.right(), c.y)),
        (pos2(c.x, i.top()), pos2(c.x, i.bottom())),
    ]
}

/// The line under `p`, if any is within reach.
fn line_at(r: Rect, p: Pos2) -> Option<usize> {
    proxy_lines(r)
        .iter()
        .enumerate()
        .map(|(i, (a, b))| {
            let t = if a == b { 0.0 } else { ((p - *a).dot(*b - *a) / (*b - *a).length_sq()).clamp(0.0, 1.0) };
            (i, p.distance(*a + (*b - *a) * t))
        })
        .filter(|(_, d)| *d <= PROXY_INSET - 1.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A click on the proxy.
struct ProxyClick {
    /// The choice before the first click of this click sequence.
    start: Edges,
    /// The line clicked, and the line of the click before.
    hit: Option<usize>,
    last: Option<usize>,
    double: bool,
    triple: bool,
}

/// Apply a proxy click to `edges`. Returns whether the choice changed.
fn proxy_click(edges: &mut Edges, click: ProxyClick) -> bool {
    if click.triple {
        let all = click.start.iter().all(|on| *on);
        *edges = [!all; 6];
        return true;
    }
    let Some(h) = click.hit else { return false };
    if click.double && click.last == Some(h) {
        let outer = h < 4;
        for (i, on) in edges.iter_mut().enumerate() {
            *on = (i < 4) == outer;
        }
    } else if let Some(on) = edges.get_mut(h) {
        *on = !*on;
    }
    true
}

/// The edge proxy. Returns whether the choice changed.
pub fn edge_proxy(ui: &mut Ui, id: &str, edges: &mut Edges, lang: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(PROXY_SIZE, PROXY_SIZE), Sense::click());
    let label = tr(lang, "Cell edges").to_string();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
    let resp = resp.on_hover_text(tr(lang, PROXY_HELP));
    let hit = resp.interact_pointer_pos().or_else(|| resp.hover_pos()).and_then(|p| line_at(r, p));
    let mut changed = false;
    if resp.clicked() {
        let (double, triple) = (resp.double_clicked(), resp.triple_clicked());
        // A double-click counts only on the line of the click before it; a triple-click
        // decides from the choice before the sequence's first click.
        let last_key = ui.scope_id().with((id, "proxy_last"));
        let start_key = ui.scope_id().with((id, "proxy_start"));
        let last: Option<usize> = ui.data(|d| d.get_temp(last_key)).flatten();
        if !double && !triple {
            ui.data_mut(|d| d.insert_temp(start_key, *edges));
        }
        let start: Edges = ui.data(|d| d.get_temp(start_key)).unwrap_or(*edges);
        changed = proxy_click(edges, ProxyClick { start, hit, last, double, triple });
        ui.data_mut(|d| d.insert_temp(last_key, hit));
    }
    let p = ui.painter();
    p.rect_filled(r, 0.0, t.input);
    p.rect_filled(r.shrink(PROXY_INSET), 0.0, egui::Color32::from_gray(0xf2));
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.field_border), StrokeKind::Inside);
    for (i, (a, b)) in proxy_lines(r).into_iter().enumerate() {
        let on = edges.get(i).copied().unwrap_or(false);
        let w = if hit == Some(i) {
            3.0
        } else if on {
            2.5
        } else {
            1.0
        };
        p.line_segment([a, b], Stroke::new(w, if on { t.accent } else { egui::Color32::from_gray(0x9a) }));
    }
    changed
}

/// A built-in type by its `type` parameter name.
fn builtin(kind: &str) -> StrokeType {
    match kind {
        "dashed" => StrokeType::Dashed { pattern: Vec::new() },
        "dotted" => StrokeType::Dotted,
        "thickThin" => StrokeType::ThickThin,
        "thinThick" => StrokeType::ThinThick,
        "thinThin" => StrokeType::ThinThin,
        "thickThick" => StrokeType::ThickThick,
        "thinThickThin" => StrokeType::ThinThickThin,
        "thickThinThick" => StrokeType::ThickThinThick,
        "wavy" => StrokeType::Wavy,
        "hashed" => StrokeType::Hashed,
        _ => StrokeType::Solid,
    }
}

/// The type's menu label (named styles by name).
fn type_label(lang: &str, v: &Value) -> String {
    match serde_json::from_value::<StrokeType>(v.clone()) {
        Ok(StrokeType::Style { name }) => name,
        Ok(k) => tr(lang, k.label()).to_string(),
        Err(_) => String::new(),
    }
}

// CELL_STROKE_TYPE_APPLICATION_BEGIN
fn stroke_type_choice(app: &DesignApp, ui: &mut Ui, id: &str, v: &Value, custom: &[String], lang: &str) -> Option<Value> {
    #[cfg(not(target_arch = "wasm32"))]
    if app.framework_installed {
        let (rect, _) = ui.allocate_exact_size(vec2(140.0, 20.0), Sense::hover());
        return type_select(app, ui, id, v, custom, lang, Some(rect));
    }
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    let mut picked = None;
    let shown = type_label(lang, &v["type"]);
    let combo = craft_ui_legacy::ComboBox::from_id_salt((id, "type")).selected_text(crate::rtl::widget(ui, &shown)).width(140.0).show_ui(ui, |ui| {
        for kind in TYPES {
            let text = tr(&lang, builtin(kind).label());
            if ui.selectable_label(shown == text, crate::rtl::widget(ui, text)).clicked() {
                picked = Some(json!(kind));
            }
        }
        if !custom.is_empty() {
            ui.separator();
        }
        for n in custom {
            if ui.selectable_label(shown == *n, n).clicked() {
                picked = Some(json!({"kind": "style", "name": n}));
            }
        }
    });
    let _ = crate::widgets::name_combo(combo, tr(&lang, "Type"));
    picked
}
// CELL_STROKE_TYPE_APPLICATION_END

/// The framework's Type select over the stroke attributes `v`: at `at` in a native parent (the Stroke panel), else in the
/// flow of a framework form. Returns the `type` payload of a committed pick.
#[cfg(not(target_arch = "wasm32"))]
fn type_select(app: &DesignApp, ui: &mut Ui, id: &str, v: &Value, custom: &[String], lang: &str, at: Option<Rect>) -> Option<Value> {
    use craft_ui_widgets::contract::{Bind, Entry, Props as _, Show as _};
    // Values are serialized type identities, never translated captions or row ordinals.
    // A built-in and a named style may legitimately have the same displayed name.
    let choices: Vec<(String, String, Value)> = TYPES
        .iter()
        .map(|kind| (json!(builtin(kind)).to_string(), tr(lang, builtin(kind).label()).to_owned(), json!(kind)))
        .chain(custom.iter().map(|name| {
            let payload = json!({"kind": "style", "name": name});
            (payload.to_string(), name.clone(), payload)
        }))
        .collect();
    let mut entries: Vec<_> = choices[..TYPES.len()].iter().map(|(key, label, _)| Entry::new(key.clone(), label.clone())).collect();
    if !custom.is_empty() {
        entries.push(Entry::separator());
        entries.extend(choices[TYPES.len()..].iter().map(|(key, label, _)| Entry::new(key.clone(), label.clone())));
    }
    let target = app.session.active().and_then(|st| {
        let (story, table, range) = if let Some(cells) = st.selection.cells {
            (cells.story, cells.table, cells.range)
        } else {
            let text = st.selection.text?;
            let cell = text.cell?;
            (text.story, cell.table, designcraft_doc::CellRange::cell(cell.row, cell.col))
        };
        let grid = st.doc.story(story)?.tables.get(&table)?;
        (range.r0 <= range.r1 && range.c0 <= range.c1 && range.r1 < grid.rows.len() && range.c1 < grid.columns.len())
            .then_some((story.0, table, range.r0, range.r1, range.c0, range.c1))
    });
    let edges = if id == "csp" {
        ui.data(|data| data.get_temp::<Edges>(egui::Id::unique("cell_stroke_edges"))).unwrap_or([true; 6])
    } else {
        edges_from(&v["edges"])
    };
    let valid = target.is_some() && edges.iter().any(|on| *on);
    let owner = app.session.active().map(|st| (st.uid, st.identity_epoch, st.revision));
    // Native Type selects by builtin kind even when a dashed value carries an explicit
    // pattern. Activating that same popup row still commits the original scalar payload,
    // so the native command can restore its weight-scaled dash pattern.
    let current =
        v["type"]["kind"].as_str().filter(|kind| TYPES.contains(kind)).map_or_else(|| v["type"].to_string(), |kind| json!(builtin(kind)).to_string());
    let identity = ui.make_persistent_id(("cell-stroke-type", id, owner, target, edges, custom, &current, lang));
    let binding = if v["type"].is_null() { craft_behavior::Mixed::Mixed } else { craft_behavior::Mixed::Same(current) };
    let select = craft_ui_widgets::pickers::Select::new(tr(lang, "Type").to_owned(), Bind::given(binding), &entries)
        .id(identity)
        .enabled(valid)
        // An orphan is shown verbatim but is never synthesized as an enabled catalog row.
        .placeholder(type_label(lang, &v["type"]));
    let shown = match at {
        Some(rect) => select.size(craft_theme::ControlSize::Sm).show_at(ui, rect),
        None => select.show(ui),
    };
    shown.events.committed().and_then(|key| choices.iter().find(|(actual, _, _)| actual == key).map(|(_, _, payload)| payload.clone()))
}

/// Weight, type, colour, tint, gap colour and gap tint for the stroke attributes in `v`
/// (`table.getCellStroke` keys; null shows as mixed). Returns the attribute the user changed.
pub fn stroke_fields(app: &mut DesignApp, ui: &mut Ui, id: &str, v: &Value) -> Option<(&'static str, Value)> {
    let lang = app.ui.language.clone();
    let custom: Vec<String> = app.session.active().map(|d| d.doc.stroke_styles.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
    let mut out = None;
    let percent = |key: &str| v[key].as_f64().map(|t| (t * 100.0).round());
    egui::Grid::new((id, "cell_stroke")).num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        caption(ui, tr(&lang, "Weight:"));
        let wid = format!("{id}_weight");
        if let Some(w) = NumField::points(&wid, v["weight"].as_f64(), 3).width(86.0).spinner().presets(WEIGHTS).range(0.0, 1000.0).show(ui) {
            out = Some(("weight", json!(w)));
        }
        ui.end_row();
        caption(ui, tr(&lang, "Type:"));
        if let Some(value) = stroke_type_choice(app, ui, id, v, &custom, &lang) {
            out = Some(("type", value));
        }
        ui.end_row();
        for (label, key, tint_label, tint_key) in [("Color:", "color", "Tint:", "tint"), ("Gap Color:", "gapColor", "Gap Tint:", "gapTint")] {
            caption(ui, tr(&lang, label));
            let mut picked = None;
            super::swatch_picker(app, ui, &format!("{id}_{key}"), v[key].as_str().map(str::to_string), |_, n| picked = Some(n));
            if let Some(n) = picked {
                out = Some((key, json!(n)));
            }
            ui.end_row();
            caption(ui, tr(&lang, tint_label));
            let tid = format!("{id}_{tint_key}");
            if let Some(t) = NumField::number(&tid, percent(tint_key), "%", 0).width(60.0).range(0.0, 100.0).show(ui) {
                out = Some((tint_key, json!(t / 100.0)));
            }
            ui.end_row();
        }
    });
    out
}

/// The shown stroke attributes of `edges` of the target cells (null where they differ).
pub fn current(app: &mut DesignApp, edges: &Edges) -> Value {
    if !edges.iter().any(|on| *on) {
        return Value::Null;
    }
    app.session.execute("table.getCellStroke", &json!({"edges": edges_param(edges)})).unwrap_or(Value::Null)
}

/// The Stroke panel for table cells: the proxy and the stroke fields, applied at once.
pub fn panel(app: &mut DesignApp, ui: &mut Ui) {
    let key = egui::Id::unique("cell_stroke_edges");
    let mut edges: Edges = ui.data(|d| d.get_temp(key)).unwrap_or([true; 6]);
    let lang = app.ui.language.clone();
    if edge_proxy(ui, "stroke_panel", &mut edges, &lang) {
        ui.data_mut(|d| d.insert_temp(key, edges));
    }
    ui.add_space(6.0);
    let values = current(app, &edges);
    let any = edges.iter().any(|on| *on);
    let changed = ui.add_enabled_ui(any, |ui| stroke_fields(app, ui, "csp", &values)).inner;
    if let Some((k, v)) = changed
        && let Err(e) = app.run("table.setCell", json!({"stroke": {k: v, "edges": edges_param(&edges)}}))
    {
        app.status(format!("Stroke: {e}"));
    }
}

/// Cell strokes on the craft framework, for the framework dialog forms (Cell Options › Strokes and Fills and
/// Diagonal Lines): the edge proxy as a framework component, the stroke fields and the swatch select.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod framework {
    use super::{EDGE_NAMES, Edges, PROXY_HELP, ProxyClick, WEIGHTS, lines_in, proxy_click};
    use crate::DesignApp;
    use crate::i18n::tr;
    use craft_behavior::{HitShape, Metrics, Mixed, NavShape, NumberCodec, PartKey, PartRef, PressPhase, Roving, Sense};
    use craft_ui_widgets::contract::{
        Access, Bind, BindMut, Common, Component, Cx, Edit as _, EditResponse, Entry, Input, MeasureCx, Name, PaintUi, PartsCx, Pointer as _,
        Props as _, Remember as _, Role, SetPosition, Show as _, ShowContainer as _, Styled as _,
    };
    use craft_ui_widgets::form::{Field, FieldGroup};
    use craft_ui_widgets::pickers::Select;
    use craft_ui_widgets::primitives::{ContentPainterCx, PaintShape, StrokeWidth};
    use craft_ui_widgets::runtime::env::{Env, Label, UnitKind};
    use craft_ui_widgets::runtime::focus::{ActivateKeys, KeyUse};
    use craft_ui_widgets::style::{ComponentStyle, Flags, FocusLook, Stateful, States, StyleColor, StyleCx, SurfaceKind, SurfaceLook};
    use craft_ui_widgets::text::{NumberField, NumberPick};
    use egui::{Rect, Ui, Vec2};
    use serde_json::{Value, json};

    const LINE: PartKey = PartKey::new("line");

    /// The proxy lines' names, in [`EDGE_NAMES`] order.
    const EDGE_LABELS: [&str; 6] = ["Top", "Bottom", "Left", "Right", "Inner Horizontal", "Inner Vertical"];

    /// The look of the edge proxy: a well around a cell grid, its lines in the accent colour while chosen.
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct EdgeProxyStyle {
        surface: SurfaceLook,
        focus: FocusLook,
        /// The side of one of the grid's 2 × 2 cells.
        cell: f32,
        /// Between the well's edge and the grid; a press this close to a line takes it.
        inset: f32,
        /// The cells inside the lines.
        paper: Stateful<StyleColor>,
        /// A chosen line.
        on: Stateful<StyleColor>,
        /// A line that is not chosen.
        off: Stateful<StyleColor>,
    }

    impl ComponentStyle for EdgeProxyStyle {
        fn surface(&self) -> Option<&SurfaceLook> {
            Some(&self.surface)
        }

        fn focus(&self) -> Option<&FocusLook> {
            Some(&self.focus)
        }

        fn metrics(&self) -> Metrics {
            Metrics::new()
        }
    }

    /// The click sequence the proxy is in: the choice before its first click and the line of the click before.
    #[derive(Clone, Default)]
    struct ClickRun {
        start: Edges,
        last: Option<usize>,
    }

    /// The edge proxy: a drawn 2 × 2 cell grid whose six lines choose the cell edges a change applies to, in
    /// [`EDGE_NAMES`] order. The lines are one Tab stop; the arrows move between them and Space includes or excludes
    /// the focused one, as on a check box, while Enter stays the dialog's. A click toggles a line, a double-click on a
    /// line chooses all outer or all inner lines, and a triple-click chooses all lines, or none when all were chosen.
    #[derive(craft_ui_widgets::contract::Component, craft_ui_widgets::contract::Props)]
    pub(crate) struct EdgeProxy<'a> {
        #[common]
        common: Common,
        value: Bind<'a, Edges>,
        names: [String; 6],
    }

    impl<'a> EdgeProxy<'a> {
        /// A proxy named `label` over `value`, its lines named `names`.
        pub(crate) fn new(label: impl Into<Label>, value: Bind<'a, Edges>, names: [String; 6]) -> Self {
            EdgeProxy { common: Common::new(), value, names }.label(label)
        }

        fn part(at: usize) -> PartRef {
            LINE.at(at as u64)
        }
    }

    impl Component for EdgeProxy<'_> {
        const ROLE: Role = Role::GROUP;
        const OVERLAYS: bool = false;
        const KEYS: KeyUse = KeyUse::of(Input::Pointer).with_activate(ActivateKeys::Space);
        type Style = EdgeProxyStyle;
        type Output = EditResponse<Edges>;

        fn style(cx: &StyleCx<'_>, _: EdgeProxyStyleKey) -> EdgeProxyStyle {
            let well = cx.surface(SurfaceKind::Well);
            let paper = cx.surface(SurfaceKind::Raised);
            let colors = &cx.tokens.color;
            let disabled = cx.color(colors.text.disabled);
            let look = |rest: StyleColor| Stateful::new(rest).off(States::new(rest, rest, rest, disabled));
            EdgeProxyStyle {
                surface: SurfaceLook::new(Stateful::new(StyleColor(well.fill)))
                    .kind(SurfaceKind::Well)
                    .radius(well.radius)
                    .border(Stateful::new(cx.color(colors.line.control_edge)), cx.tokens.stroke.thin),
                focus: cx.focus_look(),
                cell: cx.tokens.size.control.md,
                inset: cx.tokens.space.gap.item,
                paper: Stateful::new(StyleColor(paper.fill)),
                on: look(cx.color(colors.accent.text)),
                off: look(cx.color(colors.text.muted)),
            }
        }

        fn measure(&mut self, cx: &MeasureCx<'_, Self>) -> Vec2 {
            Vec2::splat(cx.style.cell * 2.0 + cx.style.inset * 2.0)
        }

        fn sense(&self) -> Sense {
            Sense::HOVER
        }

        fn parts(&mut self, rect: Rect, cx: &mut PartsCx<'_, Self>) {
            let current = cx.value().copied().unwrap_or([false; 6]);
            let inset = cx.style.inset;
            for (at, (a, b)) in lines_in(rect, inset).into_iter().enumerate() {
                let part =
                    cx.part(Self::part(at), HitShape::polyline_within(vec![a, b], false, inset)).sense(Sense::CLICK | Sense::FOCUS).focusable();
                if current[at] {
                    part.flags(Flags::CHECKED).selected();
                }
            }
            cx.roving(Roving::new(NavShape::Flat));
        }

        fn bind(&mut self) -> Option<BindMut<'_, Edges>> {
            Some(self.value.reborrow())
        }

        /// A click or Space on a line applies a proxy click to the choice: the press's click count makes it a
        /// double or triple click, which the run of clicks before it completes.
        fn update(&mut self, cx: &mut Cx<'_, Self>) {
            let Some(current) = cx.value().copied() else { return };
            let Some(at) = (0..EDGE_NAMES.len()).find(|at| cx.part_activated(Self::part(*at)).is_some()) else { return };
            let clicks = match cx.part(Self::part(at)).press.phase {
                PressPhase::Release { start, .. } => start.clicks,
                _ => 1,
            };
            let (double, triple) = (clicks == 2, clicks >= 3);
            let run = cx.memory_at::<ClickRun>("clicks").clone();
            let start = if double || triple { run.start } else { current };
            let mut edges = current;
            proxy_click(&mut edges, ProxyClick { start, hit: Some(at), last: run.last, double, triple });
            *cx.memory_at::<ClickRun>("clicks") = ClickRun { start, last: Some(at) };
            if edges != current {
                cx.set(edges);
            }
        }

        fn access(&self, access: &mut Access, _: &Cx<'_, Self>) {
            for (at, name) in self.names.iter().enumerate() {
                let node = access.node(Self::part(at), Role::CHECK_BOX);
                node.name = Name::Text(name.clone());
                node.position = Some(SetPosition::new(at).of(EDGE_NAMES.len()));
            }
        }

        fn paint(&mut self, ui: &mut PaintUi<'_>, cx: &Cx<'_, Self>) {
            let current = cx.value().copied().unwrap_or([false; 6]);
            let inset = cx.style.inset;
            let draw = ui.draw();
            draw.fill(&PaintShape::Rect(cx.rect.shrink(inset), egui::CornerRadius::ZERO), cx.resolve(&cx.style.paper));
            for (at, (a, b)) in lines_in(cx.rect, inset).into_iter().enumerate() {
                let ink = if current[at] { &cx.style.on } else { &cx.style.off };
                let width = if cx.part(Self::part(at)).hovered {
                    StrokeWidth::Heavy
                } else if current[at] {
                    StrokeWidth::Thick
                } else {
                    StrokeWidth::Thin
                };
                draw.stroke(&PaintShape::Path(vec![a, b]), width, cx.resolve(ink));
            }
        }

        fn finish(self, cx: Cx<'_, Self>) -> EditResponse<Edges> {
            cx.edit_response()
        }
    }

    /// The edge proxy over `edges`. Returns whether the choice changed.
    pub(crate) fn edge_proxy(ui: &mut Ui, id: &str, edges: &mut Edges, lang: &str) -> bool {
        let names = EDGE_LABELS.map(|label| tr(lang, label).to_owned());
        let shown = EdgeProxy::new(tr(lang, "Cell edges").to_owned(), Bind::given(*edges), names)
            .id_salt(("cell_edges", id))
            .tooltip(tr(lang, PROXY_HELP).to_owned())
            // The lines are positions in the table, not reading order: the left line stays on the left.
            .ltr_axis()
            .show(ui);
        match shown.events.committed() {
            Some(next) if *next != *edges => {
                *edges = *next;
                true
            }
            _ => false,
        }
    }

    /// A stroke weight in points, with the weight presets. Returns the weight committed.
    fn weight_field(ui: &mut Ui, id: &str, label: &str, weight: Option<f64>) -> Option<f64> {
        let units = Env::of(ui).units();
        let symbol = units.symbol(UnitKind::Points);
        let presets: Vec<Entry<NumberPick>> =
            WEIGHTS.iter().map(|w| Entry::new(NumberPick::Number(*w), format!("{} {symbol}", units.number_ungrouped(*w, 0..=2)))).collect();
        // Weights keep the native field's three decimals; typed text may name any length unit.
        let codec = NumberCodec::new(units.reader(Some(UnitKind::Points)), 3);
        NumberField::new(label.to_owned(), Bind::given(weight))
            .id_salt(("cell_stroke_weight", id))
            .unit_kind(UnitKind::Points)
            .codec(codec)
            .range(0.0..=1000.0)
            .presets(presets.as_slice())
            .show(ui)
            .edit
            .events
            .committed()
            .copied()
            .flatten()
    }

    /// A tint as whole percents over the ratio `tint` (0 to 1). Returns the percent committed.
    pub(crate) fn percent_field(ui: &mut Ui, id: &str, label: &str, tint: Option<f64>) -> Option<f64> {
        let percent = tint.map(|t| (t * 100.0).round());
        NumberField::new(label.to_owned(), Bind::given(percent))
            .id_salt(("cell_stroke_percent", id))
            .unit_kind(UnitKind::Percent)
            .range(0.0..=100.0)
            .fixed_decimals(0)
            .show(ui)
            .edit
            .events
            .committed()
            .copied()
            .flatten()
    }

    /// A select of the active document's swatches, its face the chip of `current` (mixed when `None`). Returns the
    /// swatch picked; shows nothing without a document.
    pub(crate) fn swatch_select(app: &DesignApp, ui: &mut Ui, id: &str, label: &str, current: Option<&str>) -> Option<String> {
        let doc = app.session.active().map(|st| st.doc.clone())?;
        let entries: Vec<Entry<String>> = doc.swatches.iter().map(|sw| Entry::new(sw.name.clone(), sw.name.clone())).collect();
        let value = current.map_or(Mixed::Mixed, |name| Mixed::Same(name.to_owned()));
        Select::new(label.to_owned(), Bind::given(value), &entries)
            .id_salt(("swatch_select", id))
            .picture(move |name: &String, cx: &ContentPainterCx, rect: Rect| {
                let (color, gradient) = crate::widgets::swatch_colors(&doc, name, 1.0);
                crate::widgets::paint_chip(&cx.painter, rect, color, gradient);
            })
            .picture_only()
            .show(ui)
            .events
            .committed()
            .cloned()
    }

    /// Weight, type, colour, tint, gap colour and gap tint for the stroke attributes in `v` (`table.getCellStroke`
    /// keys; null shows as mixed), as the native [`stroke_fields`](super::stroke_fields). Returns the attribute the
    /// user changed.
    pub(crate) fn stroke_fields(app: &DesignApp, ui: &mut Ui, id: &str, v: &Value, enabled: bool) -> Option<(&'static str, Value)> {
        let lang = app.ui.language.clone();
        let custom: Vec<String> = app.session.active().map(|d| d.doc.stroke_styles.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
        let mut out = None;
        FieldGroup::new(tr(&lang, "Stroke").to_owned()).id_salt((id, "stroke_fields")).enabled(enabled).show(ui, |ui, _| {
            Field::new(tr(&lang, "Weight:").to_owned()).id_salt((id, "weight_field")).show(ui, |ui, _| {
                if let Some(w) = weight_field(ui, id, tr(&lang, "Weight"), v["weight"].as_f64()) {
                    out = Some(("weight", json!(w)));
                }
            });
            Field::new(tr(&lang, "Type:").to_owned()).id_salt((id, "type_field")).show(ui, |ui, _| {
                if let Some(value) = super::type_select(app, ui, id, v, &custom, &lang, None) {
                    out = Some(("type", value));
                }
            });
            for (caption, name, key, tint_caption, tint_name, tint_key) in
                [("Color:", "Color", "color", "Tint:", "Tint", "tint"), ("Gap Color:", "Gap Color", "gapColor", "Gap Tint:", "Gap Tint", "gapTint")]
            {
                Field::new(tr(&lang, caption).to_owned()).id_salt((id, key)).show(ui, |ui, _| {
                    if let Some(swatch) = swatch_select(app, ui, &format!("{id}_{key}"), tr(&lang, name), v[key].as_str()) {
                        out = Some((key, json!(swatch)));
                    }
                });
                Field::new(tr(&lang, tint_caption).to_owned()).id_salt((id, tint_key)).show(ui, |ui, _| {
                    if let Some(t) = percent_field(ui, &format!("{id}_{tint_key}"), tr(&lang, tint_name), v[tint_key].as_f64()) {
                        out = Some((tint_key, json!(t / 100.0)));
                    }
                });
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::{Harness, kittest::Queryable};

    use super::*;

    #[test]
    fn proxy_triple_click_clears_all_lines_when_all_were_chosen_and_chooses_all_otherwise() {
        for (before, after) in [([true; 6], [false; 6]), ([true, false, true, false, false, false], [true; 6]), ([false; 6], [true; 6])] {
            let mut edges = before;
            let click = |hit, double, triple| ProxyClick { start: before, hit, last: Some(0), double, triple };
            proxy_click(&mut edges, click(Some(0), false, false));
            proxy_click(&mut edges, click(Some(0), true, false));
            proxy_click(&mut edges, click(Some(0), false, true));
            assert_eq!(edges, after, "triple-click from {before:?}");
        }
    }

    #[test]
    fn stroke_panel_proxy_limits_a_weight_change_to_the_chosen_cell_edges() {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        app.run("frame.create", json!({"rect": [72, 72, 400, 400], "content": "text", "caret": true})).unwrap();
        let tid = app.run("table.insert", json!({"rows": 2, "cols": 2})).unwrap()["table"].as_u64().unwrap();
        app.run("table.select", json!({"what": "table"})).unwrap();
        let mut h = Harness::builder().with_size(vec2(320.0, 600.0)).build_ui_state(
            |ui, app: &mut DesignApp| {
                let id = egui::Id::unique("test_fonts");
                if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                    crate::theme::install_fonts(ui.ctx(), "");
                    ui.data_mut(|d| d.insert_temp(id, true));
                    return;
                }
                crate::panels::properties::stroke_panel(app, ui);
            },
            app,
        );
        h.run_steps(3);
        // Leave only the inner horizontal line chosen: click the others off.
        let lines = proxy_lines(h.get_by_label("Cell edges").rect());
        for i in [0, 1, 2, 3, 5] {
            let (a, b) = lines[i];
            let at = a + (b - a) * 0.3;
            h.hover_at(at);
            h.drag_at(at);
            h.drop_at(at);
            h.run_steps(3);
        }
        let weight = h.get_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("1 pt")).rect();
        h.hover_at(weight.center());
        h.drag_at(weight.center());
        h.drop_at(weight.center());
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.event(egui::Event::Text("4".into()));
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        let st = h.state().session.active().unwrap();
        let t = st.doc.stories.values().find_map(|s| s.tables.get(&tid)).unwrap();
        let w = |r: usize, c: usize, side: usize| t.cell(r, c).unwrap().strokes[side].weight;
        assert_eq!([w(0, 0, 2), w(0, 1, 2), w(1, 0, 0), w(1, 1, 0)], [4.0; 4], "inner horizontal edges");
        assert_eq!([w(0, 0, 0), w(0, 0, 1), w(0, 0, 3), w(0, 1, 1), w(1, 1, 2), w(1, 1, 3)], [1.0; 6], "the other edges keep their weight");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod type_style_sdk_tests {
    use super::*;
    use designcraft_engine::{DocState, HistoryEntry};
    use egui::{Event, Key, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use std::sync::Arc;

    const STYLE: &str = "Cell rule Café–α with a complete, deliberately long catalog name";
    const REGION: &str = "cell_stroke_type_style_application_region";
    #[derive(Clone, Copy)]
    enum Parent {
        Stroke,
        Dialog,
    }
    struct State {
        app: DesignApp,
        parent: Parent,
        disabled: bool,
        visible: bool,
        sizing: bool,
    }
    struct Made {
        story: u64,
        table: u64,
        other_story: u64,
        other_table: u64,
    }
    fn select(app: &mut DesignApp, story: u64, table: u64, rows: [usize; 2], cols: [usize; 2]) {
        app.run("table.select", json!({"story": story, "table": table, "rows": rows, "cols": cols})).unwrap();
    }
    fn fixture() -> (DesignApp, Made) {
        let mut app = DesignApp::new(designcraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({})).unwrap();
        let mut ids = Vec::new();
        for y in [72, 420] {
            app.run("frame.create", json!({"rect": [72, y, 500, y + 280], "content": "text", "caret": true})).unwrap();
            let table = app.run("table.insert", json!({"rows": 4, "cols": 4})).unwrap();
            let story = table["story"].as_u64().unwrap();
            let tid = table["table"].as_u64().unwrap();
            select(&mut app, story, tid, [0, 3], [0, 3]);
            app.run(
                "table.setCell",
                json!({"text": "Independent cell text", "fill": "[Paper]", "stroke": {
                    "weight": 2.375, "tint": 0.625, "gapTint": 0.375,
                    "type": {"kind": "dashed", "pattern": [3.0, 1.0]}, "edges": "all"
                }}),
            )
            .unwrap();
            ids.push((story, tid));
        }
        app.run("strokeStyle.new", json!({"name": STYLE, "type": "dash", "pattern": [5, 2]})).unwrap();
        app.run("strokeStyle.new", json!({"name": "Solid", "type": "dot"})).unwrap();
        // A genuine inactive document is protected by every complete-domain oracle.
        app.run("file.new", json!({})).unwrap();
        app.session.set_active(0);
        let made = Made { story: ids[0].0, table: ids[0].1, other_story: ids[1].0, other_table: ids[1].1 };
        select(&mut app, made.story, made.table, [1, 2], [1, 2]);
        app.run("table.setCell", json!({"fill": "[None]"})).unwrap();
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(app.session.active().unwrap().history.redo.len(), 1);
        (app, made)
    }
    fn harness(mut app: DesignApp, installed: bool, parent: Parent, width: f32) -> Harness<'static, State> {
        let ctx = egui::Context::default();
        if installed {
            app.framework_opacity = true;
            app.install_framework(&ctx);
        }
        assert_eq!(app.framework_installed, installed);
        let mut ready = false;
        let mut h = Harness::builder().with_context(ctx).with_size(vec2(width, 720.0)).build_ui_state(
            move |ui, state: &mut State| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx(), &state.app.ui.language);
                    crate::theme::apply(ui.ctx(), &Tokens::for_brightness(Default::default()));
                    ready = true;
                    return;
                }
                let _ = craft_ui_widgets::runtime::inspect::record(ui, egui::Id::NULL.with(REGION), |ui| {
                    if !state.visible {
                        return;
                    }
                    match state.parent {
                        Parent::Dialog => crate::dialogs::show(&mut state.app, ui.ctx()),
                        Parent::Stroke => {
                            ui.add_enabled_ui(!state.disabled, |ui| {
                                let builder = egui::UiBuilder::new().id_salt("actual_stroke_parent");
                                let builder = if state.sizing { builder.sizing_pass() } else { builder };
                                ui.scope_builder(builder, |ui| {
                                    if state.sizing {
                                        assert!(ui.is_sizing_pass());
                                    }
                                    crate::panels::properties::stroke_panel(&mut state.app, ui);
                                });
                            });
                        }
                    }
                });
            },
            State { app, parent, disabled: false, visible: true, sizing: false },
        );
        h.input_mut().max_texture_side = Some(8192);
        idle(&mut h, 3);
        h
    }
    fn frame(h: &mut Harness<'_, State>, event: Option<Event>) {
        let mut raw = std::mem::take(h.input_mut());
        if let Some(event) = event {
            raw.events.push(event);
        }
        let ctx = h.ctx.clone();
        h.state_mut().app.raw_input_hook(&ctx, &mut raw);
        *h.input_mut() = raw;
        h.step();
    }
    fn idle(h: &mut Harness<'_, State>, count: usize) {
        for _ in 0..count {
            frame(h, None);
        }
    }
    fn key(h: &mut Harness<'_, State>, key: Key) {
        for pressed in [true, false] {
            frame(h, Some(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }));
        }
        idle(h, 2);
    }
    fn pointer(h: &mut Harness<'_, State>, at: Pos2, pressed: bool) {
        frame(h, Some(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }));
    }
    fn click_at(h: &mut Harness<'_, State>, at: Pos2) {
        frame(h, Some(Event::PointerMoved(at)));
        pointer(h, at, true);
        pointer(h, at, false);
        idle(h, 2);
    }
    fn click(h: &mut Harness<'_, State>, role: egui::accesskit::Role, label: &str) {
        let at = h.get_by_role_and_label(role, label).rect().center();
        click_at(h, at);
    }
    fn name(h: &Harness<'_, State>) -> String {
        tr(&h.state().app.ui.language, "Type").to_owned()
    }
    fn control(h: &Harness<'_, State>) -> craft_ui_widgets::runtime::inspect::Element {
        let name = name(h);
        let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
        let elements: Vec<_> = record
            .elements
            .iter()
            .filter(|e| e.component == "Select" && e.facts.as_ref().and_then(|f| f.name.as_deref()) == Some(name.as_str()))
            .collect();
        assert_eq!(elements.len(), 1, "one real SDK Select with the complete translated Type name");
        assert_eq!(h.get_all_by_role_and_label(egui::accesskit::Role::ComboBox, &name).count(), 1);
        elements[0].clone()
    }
    fn tab_to(h: &mut Harness<'_, State>) {
        let label = name(h);
        let count = h
            .query_all_by(|n| {
                matches!(
                    n.role(),
                    egui::accesskit::Role::Button
                        | egui::accesskit::Role::ComboBox
                        | egui::accesskit::Role::TextInput
                        | egui::accesskit::Role::CheckBox
                )
            })
            .count();
        for _ in 0..=count {
            if h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &label).is_focused() {
                let id = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &label).accesskit_node().id();
                let focused: Vec<_> = h.query_all_by(|n| n.is_focused()).map(|n| n.accesskit_node().id()).collect();
                assert_eq!(focused, vec![id]);
                return;
            }
            key(h, Key::Tab);
        }
        panic!("natural Tab did not reach the current Type owner");
    }
    fn assert_popup_physical_owner(h: &Harness<'_, State>) {
        let label = name(h);
        let owner = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &label).accesskit_node();
        let roots: Vec<_> = owner.controls().collect();
        assert_eq!(roots.len(), 1, "strict actual controls root; no portal exception for Work408");
        let list = h.get_by_role_and_label(egui::accesskit::Role::ListBox, &label).accesskit_node();
        // Consumer parent() is the literal physical edge, including otherwise-uninteresting Ui nodes.
        let mut cursor = Some(list);
        let mut seen = Vec::new();
        let mut found = false;
        while let Some(node) = cursor {
            assert!(!seen.contains(&node.id()), "actual physical tree must be acyclic");
            seen.push(node.id());
            if node.id() == roots[0].id() {
                found = true;
                break;
            }
            cursor = node.parent();
        }
        assert!(found, "the exact controls root must physically contain the unique ListBox, full IDs unchanged");
        assert!(list.bounding_box().is_some(), "the actual popup has real geometry");
    }
    fn open(h: &mut Harness<'_, State>) {
        let name = name(h);
        click(h, egui::accesskit::Role::ComboBox, &name);
        assert_popup_physical_owner(h);
    }
    #[derive(Clone, Debug)]
    struct PopupOption {
        position: usize,
        id: u128,
        label: String,
        rect: Rect,
    }
    fn popup_bound(h: &Harness<'_, State>) -> usize {
        TYPES.len() + h.state().app.session.active().unwrap().doc.stroke_styles.len() + 2
    }
    fn visible_popup_options(h: &Harness<'_, State>) -> Vec<PopupOption> {
        assert_popup_physical_owner(h);
        let label = name(h);
        let list = h.get_by_role_and_label(egui::accesskit::Role::ListBox, &label);
        let view = list.rect();
        let list_id = list.accesskit_node().id();
        assert!(view.is_finite() && view.is_positive());
        let mut rows = Vec::new();
        for row in h.query_all_by_role(egui::accesskit::Role::ListBoxOption) {
            let node = row.accesskit_node();
            let mut cursor = node.parent();
            let mut inside = false;
            while let Some(parent) = cursor {
                if parent.id() == list_id {
                    inside = true;
                    break;
                }
                cursor = parent.parent();
            }
            assert!(inside, "every actual option physically belongs to this unique real ListBox");
            let rect = row.rect();
            // A virtualized lead/anchor may be kept alive offscreen. It is not a clickable row.
            if view.contains(rect.center()) {
                assert!(rect.is_finite() && rect.is_positive());
                rows.push(PopupOption {
                    position: node.position_in_set().expect("real SDK option ordinal"),
                    id: u128::from(node.id()),
                    label: node.label().expect("complete real option name"),
                    rect,
                });
            }
        }
        rows.sort_by_key(|row| row.position);
        assert!(!rows.is_empty(), "the real popup must expose physical rows");
        assert!(rows.windows(2).all(|pair| pair[0].position < pair[1].position));
        rows
    }
    fn scroll_popup(h: &mut Harness<'_, State>, down: bool) {
        let before = quiet(&h.state().app);
        let owner = control(h).id;
        let label = name(h);
        let view = h.get_by_role_and_label(egui::accesskit::Role::ListBox, &label).rect();
        frame(h, Some(Event::PointerMoved(view.center())));
        // Half a real viewport preserves overlap. No guessed option coordinates, direct offset,
        // fabricated AX rows or provider visibility override is used.
        let delta = vec2(0.0, view.height() * if down { -0.5 } else { 0.5 });
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            frame(
                h,
                Some(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: if phase == egui::TouchPhase::Move { delta } else { egui::Vec2::ZERO },
                    phase,
                    modifiers: Modifiers::NONE,
                    source: egui::MouseWheelSource::Wheel,
                }),
            );
            assert_quiet(&h.state().app, &before);
        }
        idle(h, 2);
        assert_popup_physical_owner(h);
        assert_eq!(control(h).id, owner, "physical scrolling cannot replace the real Type owner");
        assert_quiet(&h.state().app, &before);
    }
    fn observe_popup_catalog(h: &mut Harness<'_, State>) -> Vec<PopupOption> {
        let count = popup_bound(h) - 2;
        let mut seen = std::collections::BTreeMap::<usize, PopupOption>::new();
        let mut previous = None;
        let mut down = false;
        for _ in 0..2 * popup_bound(h) {
            let rows = visible_popup_options(h);
            let signature: Vec<_> = rows.iter().map(|row| (row.position, row.id)).collect();
            for row in rows {
                if let Some(old) = seen.get(&row.position) {
                    assert_eq!((row.id, &row.label), (old.id, &old.label), "same real ordinal retains its distinct identity and full name");
                } else {
                    seen.insert(row.position, row);
                }
            }
            assert!(seen.len() <= count, "no extra enabled options may be silently discarded");
            if seen.len() == count {
                let catalog: Vec<_> = seen.into_values().collect();
                for (i, row) in catalog.iter().enumerate() {
                    assert!(catalog[..i].iter().all(|old| old.id != row.id), "distinct option ordinals need distinct real AX identities");
                }
                return catalog;
            }
            if previous.as_ref() == Some(&signature) {
                assert!(!down, "physical scrolling stopped before the complete catalog was observed");
                down = true;
            }
            previous = Some(signature);
            scroll_popup(h, down);
        }
        panic!("the finite real catalog was not exposed by physical scrolling");
    }
    fn reveal_popup_option(h: &mut Harness<'_, State>, label: &str, identity: Option<&PopupOption>) -> PopupOption {
        let mut previous = None;
        let mut down = false;
        for _ in 0..2 * popup_bound(h) {
            let rows = visible_popup_options(h);
            let matches: Vec<_> = rows.iter().filter(|row| row.label == label && identity.is_none_or(|wanted| row.id == wanted.id)).collect();
            match matches.as_slice() {
                [] => {}
                [row] => {
                    if let Some(wanted) = identity {
                        assert_eq!((row.position, row.id, &row.label), (wanted.position, wanted.id, &wanted.label));
                    }
                    return (**row).clone();
                }
                _ => panic!("never choose the first of ambiguous same-caption visible options"),
            }
            let signature: Vec<_> = rows.iter().map(|row| (row.position, row.id)).collect();
            if previous.as_ref() == Some(&signature) {
                assert!(identity.is_none() && !down, "physical scrolling cannot reveal the requested complete option");
                down = true;
            }
            previous = Some(signature);
            let direction = identity.map_or(down, |wanted| wanted.position > rows[0].position);
            scroll_popup(h, direction);
        }
        panic!("the exact real option was not physically reachable");
    }
    fn click_popup_option(h: &mut Harness<'_, State>, label: &str, identity: Option<&PopupOption>) {
        let row = reveal_popup_option(h, label, identity);
        click_at(h, row.rect.center());
    }
    fn pick(h: &mut Harness<'_, State>, label: &str) {
        open(h);
        click_popup_option(h, label, None);
    }
    /// The uninstalled route's legacy Type menu is a scroll area as tall as egui's combo height, and the named styles
    /// follow eleven built-in rows below it: the row is reached as a user reaches it, with the wheel over the open menu.
    fn reveal_native_row(h: &mut Harness<'_, State>, label: &str) -> Pos2 {
        let dotted = tr(&h.state().app.ui.language, builtin("dotted").label()).to_owned();
        let over = h.get_by_label(&dotted).rect().center();
        frame(h, Some(Event::PointerMoved(over)));
        for phase in [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End] {
            frame(
                h,
                Some(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: if phase == egui::TouchPhase::Move { vec2(0.0, -2000.0) } else { egui::Vec2::ZERO },
                    phase,
                    modifiers: Modifiers::NONE,
                    source: egui::MouseWheelSource::Wheel,
                }),
            );
        }
        idle(h, 20);
        h.get_by_label(label).rect().center()
    }
    #[derive(Clone)]
    struct Quiet {
        docs: Vec<DocState>,
        active: Option<usize>,
        prefs: Value,
        journal: Vec<(String, Value)>,
    }
    fn quiet(app: &DesignApp) -> Quiet {
        Quiet {
            docs: app.session.documents().to_vec(),
            active: app.session.active_index(),
            prefs: serde_json::to_value(&app.session.prefs).unwrap(),
            journal: app.session.journal.clone(),
        }
    }
    fn same_entries(now: &[HistoryEntry], before: &[HistoryEntry]) {
        assert_eq!(now.len(), before.len());
        for (now, before) in now.iter().zip(before) {
            assert_eq!(now.label, before.label);
            assert_eq!(now.selection, before.selection);
            assert_eq!(now.doc.as_ref(), before.doc.as_ref());
            assert!(Arc::ptr_eq(&now.doc, &before.doc));
        }
    }
    fn same_doc(now: &DocState, before: &DocState) {
        assert_eq!(format!("{now:?}"), format!("{before:?}"), "complete authoritative DocState, not just the type");
        assert!(Arc::ptr_eq(&now.doc, &before.doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.saved_doc));
        same_entries(&now.history.undo, &before.history.undo);
        same_entries(&now.history.redo, &before.history.redo);
        assert_eq!(now.is_dirty(), before.is_dirty());
    }
    fn assert_quiet(app: &DesignApp, before: &Quiet) {
        assert_eq!(app.session.active_index(), before.active);
        assert_eq!(app.session.documents().len(), before.docs.len());
        for (now, before) in app.session.documents().iter().zip(&before.docs) {
            same_doc(now, before);
        }
        assert_eq!(serde_json::to_value(&app.session.prefs).unwrap(), before.prefs);
        assert_eq!(app.session.journal, before.journal, "no added command journal entries, including after idle");
    }

    fn assert_edit(app: &DesignApp, before: &Quiet, payload: Value) {
        assert_eq!(app.session.active_index(), before.active);
        assert_eq!(app.session.documents().len(), before.docs.len());
        assert_eq!(serde_json::to_value(&app.session.prefs).unwrap(), before.prefs);
        assert_eq!(&app.session.journal[..before.journal.len()], before.journal.as_slice());
        assert_eq!(&app.session.journal[before.journal.len()..], &[("table.setCell".to_owned(), payload.clone())]);
        let active = before.active.unwrap();
        let mut oracle = designcraft_engine::Session::new();
        oracle.add_document(before.docs[active].clone());
        oracle.execute("table.setCell", &payload).unwrap();
        let mut expected = oracle.active().unwrap().clone();
        let now = app.session.active().unwrap();
        assert_ne!(expected.doc.as_ref(), before.docs[active].doc.as_ref(), "fixture must genuinely change a stroke");
        assert_eq!(
            now.doc.as_ref(),
            expected.doc.as_ref(),
            "complete document equals the independent authoritative native command, not a hand-picked field"
        );
        assert!(now.revision > before.docs[active].revision, "process-global revisions are not +1 counters");
        expected.revision = now.revision;
        assert_eq!(format!("{now:?}"), format!("{expected:?}"), "full DocState and exactly one normal Cell Options undo step");
        assert_eq!(now.history.undo.len(), before.docs[active].history.undo.len() + 1);
        assert_eq!(now.history.undo.last().unwrap().label, "Cell Options");
        same_entries(&now.history.undo[..before.docs[active].history.undo.len()], &before.docs[active].history.undo);
        assert!(Arc::ptr_eq(&now.history.undo.last().unwrap().doc, &before.docs[active].doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.docs[active].saved_doc));
        assert!(now.history.redo.is_empty());
        for (index, state) in app.session.documents().iter().enumerate() {
            if index != active {
                same_doc(state, &before.docs[index]);
            }
        }
    }
    fn payload(kind: Value, edges: Edges) -> Value {
        json!({"stroke": {"type": kind, "edges": edges_param(&edges)}})
    }
    fn undo_redo(h: &mut Harness<'_, State>, before: &Quiet) {
        let active = before.active.unwrap();
        let after = quiet(&h.state().app);
        h.state_mut().app.run("edit.undo", json!({})).unwrap();
        idle(h, 2);
        let now = h.state().app.session.active().unwrap();
        assert_eq!(now.doc.as_ref(), before.docs[active].doc.as_ref());
        assert!(Arc::ptr_eq(&now.doc, &before.docs[active].doc));
        assert_eq!(now.selection, before.docs[active].selection);
        same_entries(&now.history.undo, &before.docs[active].history.undo);
        assert_eq!(now.history.redo.len(), 1);
        assert!(Arc::ptr_eq(&now.history.redo[0].doc, &after.docs[active].doc));
        assert!(Arc::ptr_eq(&now.saved_doc, &before.docs[active].saved_doc));
        let mut expected_undo = before.docs[active].clone();
        expected_undo.revision = now.revision;
        expected_undo.history.redo =
            vec![HistoryEntry { label: "Cell Options".into(), doc: after.docs[active].doc.clone(), selection: after.docs[active].selection.clone() }];
        assert_eq!(format!("{now:?}"), format!("{expected_undo:?}"), "complete Undo state, including unrelated metadata and exact redo contents");
        h.state_mut().app.run("edit.redo", json!({})).unwrap();
        idle(h, 2);
        let now = h.state().app.session.active().unwrap();
        assert!(Arc::ptr_eq(&now.doc, &after.docs[active].doc));
        assert_eq!(now.doc.as_ref(), after.docs[active].doc.as_ref());
        assert_eq!(now.selection, after.docs[active].selection);
        same_entries(&now.history.undo, &after.docs[active].history.undo);
        assert!(now.history.redo.is_empty());
        let mut expected_redo = after.docs[active].clone();
        expected_redo.revision = now.revision;
        assert_eq!(format!("{now:?}"), format!("{expected_redo:?}"), "complete Redo state and preserved metadata");
        assert_eq!(h.state().app.session.active_index(), before.active);
        assert_eq!(serde_json::to_value(&h.state().app.session.prefs).unwrap(), before.prefs);
        assert_eq!(&h.state().app.session.journal[..after.journal.len()], after.journal.as_slice());
        assert_eq!(
            &h.state().app.session.journal[after.journal.len()..],
            &[("edit.undo".to_owned(), json!({})), ("edit.redo".to_owned(), json!({}))]
        );
        for (index, doc) in h.state().app.session.documents().iter().enumerate() {
            if index != active {
                same_doc(doc, &before.docs[index]);
            }
        }
        let stable = quiet(&h.state().app);
        idle(h, 3);
        assert_quiet(&h.state().app, &stable);
    }

    #[test]
    fn cell_stroke_all_eleven_builtin_pointer_choices_keep_exact_payload_domain_and_undo_from_the_real_stroke_parent() {
        for kind in TYPES {
            let (app, _) = fixture();
            let mut h = harness(app, true, Parent::Stroke, 400.0);
            assert!(!control(&h).facts.unwrap().disabled);
            let before = quiet(&h.state().app);
            let label = tr(&h.state().app.ui.language, builtin(kind).label()).to_owned();
            // Reselecting the current Dashed popup row must still dispatch the original
            // scalar grammar and restore its native weight-scaled pattern, not silently no-op.
            pick(&mut h, &label);
            assert_edit(&h.state().app, &before, payload(json!(kind), [true; 6]));
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn cell_stroke_custom_names_distinct_from_identical_builtin_captions_keep_object_payload_and_complete_catalog() {
        let (app, _) = fixture();
        let mut h = harness(app, true, Parent::Stroke, 400.0);
        let before = quiet(&h.state().app);
        open(&mut h);
        let catalog = observe_popup_catalog(&mut h);
        let labels: Vec<_> = catalog.iter().map(|row| row.label.clone()).collect();
        let mut expected: Vec<_> = TYPES.iter().map(|kind| tr("", builtin(kind).label()).to_owned()).collect();
        expected.extend([STYLE.to_owned(), "Solid".to_owned()]);
        assert_eq!(labels, expected, "all eleven builtins followed by the real ordered custom catalog, with no elided names");
        assert_quiet(&h.state().app, &before);
        let duplicates: Vec<_> = catalog.iter().filter(|row| row.label == "Solid").collect();
        assert_eq!(duplicates.len(), 2, "a legitimate named style may share a builtin's displayed caption");
        assert_ne!(duplicates[0].id, duplicates[1].id, "builtin Solid and the named Solid are distinct real choices");
        click_popup_option(&mut h, "Solid", Some(duplicates[1]));
        assert_edit(&h.state().app, &before, payload(json!({"kind": "style", "name": "Solid"}), [true; 6]));
        undo_redo(&mut h, &before);
        let before = quiet(&h.state().app);
        pick(&mut h, STYLE);
        assert_edit(&h.state().app, &before, payload(json!({"kind": "style", "name": STYLE}), [true; 6]));
        undo_redo(&mut h, &before);
    }

    #[test]
    fn cell_stroke_natural_tab_popup_preview_escape_and_enter_commit_only_one_authoritative_edit() {
        let (app, _) = fixture();
        let mut h = harness(app, true, Parent::Stroke, 400.0);
        let before = quiet(&h.state().app);
        let owner = control(&h).id;
        tab_to(&mut h);
        key(&mut h, Key::Enter);
        assert_popup_physical_owner(&h);
        key(&mut h, Key::End);
        assert_eq!(control(&h).id, owner, "keyboard proposals never become authoritative type changes");
        assert_quiet(&h.state().app, &before);
        key(&mut h, Key::Escape);
        assert_eq!(h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(), 0);
        assert_quiet(&h.state().app, &before);
        tab_to(&mut h);
        key(&mut h, Key::Enter);
        assert_popup_physical_owner(&h);
        key(&mut h, Key::End);
        assert_quiet(&h.state().app, &before);
        key(&mut h, Key::Enter);
        assert_edit(&h.state().app, &before, payload(json!({"kind": "style", "name": "Solid"}), [true; 6]));
        undo_redo(&mut h, &before);
    }

    #[test]
    fn cell_stroke_actual_mixed_and_imported_orphan_types_never_invent_solid_or_an_enabled_orphan_catalog_entry() {
        for orphan in [false, true] {
            let (mut app, made) = fixture();
            if orphan {
                // A genuine imported unresolved style is document data, not a forged SDK flag.
                let st = app.session.active_mut().unwrap();
                let doc = Arc::make_mut(&mut st.doc);
                let story = doc.story_mut(designcraft_doc::StoryId(made.story)).unwrap();
                let table = Arc::make_mut(story.tables.get_mut(&made.table).unwrap());
                for cell in &mut table.cells {
                    for stroke in &mut cell.strokes {
                        stroke.kind = StrokeType::Style { name: "Missing imported rule".into() };
                    }
                }
                story.rev = story.rev.wrapping_add(1);
                st.bump_revision();
            } else {
                app.run("table.setCell", json!({"row": 1, "col": 1, "stroke": {"type": "dotted", "edges": ["top"]}})).unwrap();
                assert!(current(&mut app, &[true; 6])["type"].is_null(), "genuine disagreement among the selected cell edges");
            }
            let mut h = harness(app, true, Parent::Stroke, 400.0);
            let before = quiet(&h.state().app);
            if orphan {
                let actual = current(&mut h.state_mut().app, &[true; 6]);
                assert_eq!(actual["type"], json!({"kind": "style", "name": "Missing imported rule"}));
                assert_eq!(type_label("", &actual["type"]), "Missing imported rule", "the complete orphan is the actual placeholder, never Solid");
                assert!(
                    h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &name(&h)).value().is_none_or(|v| v.is_empty()),
                    "public Select honestly reports no catalog ordinal/value for an orphan"
                );
            }
            open(&mut h);
            assert_eq!(h.query_all_by_role_and_label(egui::accesskit::Role::ListBoxOption, "Missing imported rule").count(), 0);
            assert_quiet(&h.state().app, &before);
            let label = tr(&h.state().app.ui.language, builtin("hashed").label()).to_owned();
            click(&mut h, egui::accesskit::Role::ListBoxOption, &label);
            assert_edit(&h.state().app, &before, payload(json!("hashed"), [true; 6]));
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn cell_stroke_unchanged_real_edge_proxy_limits_type_to_inner_boundaries_and_cell_caret_targets_only_its_cell() {
        for caret in [false, true] {
            let (mut app, _) = fixture();
            if caret {
                app.run("table.toggleSelection", json!({})).unwrap();
                assert!(app.session.active().unwrap().selection.cells.is_none());
                assert!(app.session.active().unwrap().selection.text.unwrap().cell.is_some());
            }
            let mut h = harness(app, true, Parent::Stroke, 400.0);
            let edges = if caret {
                [true; 6]
            } else {
                let lines = proxy_lines(h.get_by_label("Cell edges").rect());
                for index in [0, 1, 2, 3, 5] {
                    let (a, b) = lines[index];
                    click_at(&mut h, a + (b - a) * 0.3);
                }
                let chosen = h.ctx.data(|data| data.get_temp::<Edges>(egui::Id::unique("cell_stroke_edges"))).unwrap();
                assert_eq!(chosen, [false, false, false, false, true, false]);
                chosen
            };
            let before = quiet(&h.state().app);
            pick(&mut h, STYLE);
            assert_edit(&h.state().app, &before, payload(json!({"kind": "style", "name": STYLE}), edges));
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn cell_stroke_real_cell_options_drafts_cancel_without_domain_effect_then_confirm_one_exact_type_only_edit() {
        let (mut app, _) = fixture();
        crate::dialogs::open_cell_options(&mut app).unwrap();
        let mut h = harness(app, true, Parent::Dialog, 640.0);
        let before = quiet(&h.state().app);
        let original = h.state().app.ui.dialog.as_ref().unwrap().fields.clone();
        pick(&mut h, STYLE);
        assert_quiet(&h.state().app, &before);
        let draft = &h.state().app.ui.dialog.as_ref().unwrap().fields;
        let mut expected = original.clone();
        expected.insert("type".into(), json!({"kind": "style", "name": STYLE}));
        assert_eq!(draft, &expected, "the real Cell Options caller retains every other seed, edge, numeric and color field");
        click(&mut h, egui::accesskit::Role::Button, "Cancel");
        assert!(h.state().app.ui.dialog.is_none());
        assert_quiet(&h.state().app, &before);
        crate::dialogs::open_cell_options(&mut h.state_mut().app).unwrap();
        idle(&mut h, 3);
        pick(&mut h, "Dotted");
        assert_quiet(&h.state().app, &before);
        assert!(h.state().app.ui.dialog.is_some(), "picking a row is not dialog confirmation");
        click(&mut h, egui::accesskit::Role::Button, "OK");
        assert!(h.state().app.ui.dialog.is_none());
        assert_edit(&h.state().app, &before, payload(json!("dotted"), [true; 6]));
        undo_redo(&mut h, &before);
    }

    #[test]
    fn cell_stroke_real_cell_options_refusal_preserves_the_deleted_style_draft_and_all_document_history_until_recovery() {
        let (mut app, _) = fixture();
        crate::dialogs::open_cell_options(&mut app).unwrap();
        let mut h = harness(app, true, Parent::Dialog, 640.0);
        pick(&mut h, STYLE);
        h.state_mut().app.run("strokeStyle.delete", json!({"name": STYLE})).unwrap();
        idle(&mut h, 2);
        let before = quiet(&h.state().app);
        let draft = h.state().app.ui.dialog.as_ref().unwrap().fields.clone();
        click(&mut h, egui::accesskit::Role::Button, "OK");
        assert_quiet(&h.state().app, &before);
        let retained = h.state().app.ui.dialog.as_ref().unwrap();
        assert_eq!(retained.id, "cellOptions");
        assert_eq!(retained.fields, draft, "real rejected confirmation preserves the complete correction draft");
        pick(&mut h, "Dotted");
        click(&mut h, egui::accesskit::Role::Button, "OK");
        assert_edit(&h.state().app, &before, payload(json!("dotted"), [true; 6]));
        undo_redo(&mut h, &before);
    }

    #[test]
    fn cell_stroke_held_popup_choice_retires_catalog_range_table_document_epoch_edges_and_disabled_or_absent_owners() {
        for change in ["range", "table", "document", "epoch", "catalog", "definition", "edges", "parent-disabled", "hidden", "no-selection"] {
            let (app, made) = fixture();
            let mut h = harness(app, true, Parent::Stroke, 400.0);
            let old_owner = control(&h).id;
            open(&mut h);
            let held = reveal_popup_option(&mut h, STYLE, None).rect.center();
            frame(&mut h, Some(Event::PointerMoved(held)));
            pointer(&mut h, held, true);
            match change {
                "range" => select(&mut h.state_mut().app, made.story, made.table, [0, 0], [0, 1]),
                "table" => select(&mut h.state_mut().app, made.other_story, made.other_table, [1, 2], [1, 2]),
                "document" => {
                    h.state_mut().app.run("file.new", json!({})).unwrap();
                }
                "epoch" => {
                    let old = h.state().app.session.active().unwrap();
                    let mut replacement = DocState::new((*old.doc).clone(), old.path.clone());
                    replacement.uid = old.uid;
                    replacement.selection = old.selection.clone();
                    assert_ne!(replacement.identity_epoch, old.identity_epoch);
                    *h.state_mut().app.session.active_mut().unwrap() = replacement;
                }
                "catalog" => {
                    h.state_mut().app.run("strokeStyle.delete", json!({"name": STYLE})).unwrap();
                }
                "definition" => {
                    h.state_mut().app.run("strokeStyle.new", json!({"name": STYLE, "type": "dot"})).unwrap();
                }
                "edges" => {
                    h.ctx.data_mut(|data| data.insert_temp(egui::Id::unique("cell_stroke_edges"), [true, false, false, false, false, false]));
                }
                "parent-disabled" => h.state_mut().disabled = true,
                "hidden" => h.state_mut().visible = false,
                "no-selection" => {
                    h.state_mut().app.run("selection.set", json!({"ids": []})).unwrap();
                }
                _ => unreachable!(),
            }
            let retired = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            assert_eq!(
                h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(),
                0,
                "old popup retires before release or refocusing after {change}"
            );
            if !matches!(change, "document" | "hidden" | "no-selection" | "parent-disabled") {
                assert_ne!(control(&h).id, old_owner, "actual target/catalog/edge owner changes after {change}");
            }
            pointer(&mut h, held, false);
            key(&mut h, Key::Enter);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &retired);
            h.state_mut().visible = true;
            h.state_mut().disabled = false;
            h.state_mut().app.session.set_active(0);
            select(&mut h.state_mut().app, made.story, made.table, [1, 2], [1, 2]);
            h.ctx.data_mut(|data| data.insert_temp(egui::Id::unique("cell_stroke_edges"), [true; 6]));
            idle(&mut h, 2);
            let before = quiet(&h.state().app);
            tab_to(&mut h);
            key(&mut h, Key::Enter);
            assert_popup_physical_owner(&h);
            key(&mut h, Key::Home);
            assert_quiet(&h.state().app, &before);
            key(&mut h, Key::Enter);
            assert_edit(&h.state().app, &before, payload(json!("solid"), [true; 6]));
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn cell_stroke_genuine_missing_document_selection_and_disabled_parent_are_quiet_and_uninstalled_native_fallback_recovers() {
        for missing in ["document", "selection", "parent", "edges"] {
            let (mut app, _) = fixture();
            if missing == "document" {
                while !app.session.documents().is_empty() {
                    app.session.close_document(0);
                }
            }
            if missing == "selection" {
                app.run("selection.set", json!({"ids": []})).unwrap();
            }
            let mut h = harness(app, true, Parent::Stroke, 400.0);
            h.state_mut().disabled = missing == "parent";
            if missing == "edges" {
                h.ctx.data_mut(|data| data.insert_temp(egui::Id::unique("cell_stroke_edges"), [false; 6]));
            }
            idle(&mut h, 2);
            let before = quiet(&h.state().app);
            if matches!(missing, "parent" | "edges") {
                assert!(control(&h).facts.unwrap().disabled);
                let label = name(&h);
                click(&mut h, egui::accesskit::Role::ComboBox, &label);
            } else {
                let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
                assert!(!record.elements.iter().any(|e| e.component == "Select"));
            }
            key(&mut h, Key::Space);
            key(&mut h, Key::Enter);
            assert_eq!(h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(), 0);
            assert_quiet(&h.state().app, &before);
            if matches!(missing, "parent" | "edges") {
                h.state_mut().disabled = false;
                h.ctx.data_mut(|data| data.insert_temp(egui::Id::unique("cell_stroke_edges"), [true; 6]));
                idle(&mut h, 2);
                let before = quiet(&h.state().app);
                pick(&mut h, "Dotted");
                assert_edit(&h.state().app, &before, payload(json!("dotted"), [true; 6]));
                undo_redo(&mut h, &before);
            }
        }
        let (app, _) = fixture();
        let mut h = harness(app, false, Parent::Stroke, 400.0);
        assert!(!h.state().app.framework_installed, "genuinely uninstalled native route");
        let before = quiet(&h.state().app);
        click(&mut h, egui::accesskit::Role::ComboBox, "Type");
        let at = reveal_native_row(&mut h, STYLE);
        click_at(&mut h, at);
        assert_edit(&h.state().app, &before, payload(json!({"kind": "style", "name": STYLE}), [true; 6]));
        undo_redo(&mut h, &before);
        let ctx = h.ctx.clone();
        h.state_mut().app.framework_opacity = true;
        h.state_mut().app.install_framework(&ctx);
        assert!(h.state().app.framework_installed);
        idle(&mut h, 3);
        let before = quiet(&h.state().app);
        pick(&mut h, "Dotted");
        assert_edit(&h.state().app, &before, payload(json!("dotted"), [true; 6]));
        undo_redo(&mut h, &before);
    }

    #[test]
    fn cell_stroke_real_sizing_and_hidden_parent_register_no_live_owner_hits_focus_stop_or_delayed_edit() {
        for sizing in [false, true] {
            let (app, _) = fixture();
            let mut h = harness(app, true, Parent::Stroke, 400.0);
            let label = name(&h);
            let at = h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &label).rect().center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            pointer(&mut h, at, true);
            h.state_mut().sizing = sizing;
            h.state_mut().visible = sizing;
            let before = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_eq!(h.query_all_by(|n| n.role() == egui::accesskit::Role::ComboBox && n.label().as_deref() == Some(label.as_str())).count(), 0);
            let record = craft_ui_widgets::runtime::inspect::recorded(&h.ctx, egui::Id::NULL.with(REGION)).unwrap();
            assert!(!record.elements.iter().any(|e| e.component == "Select"));
            assert!(record.hits.is_empty());
            assert!(record.elements.iter().all(|e| e.stop.is_none()));
            pointer(&mut h, at, false);
            key(&mut h, Key::Enter);
            assert_quiet(&h.state().app, &before);
            h.state_mut().sizing = false;
            h.state_mut().visible = true;
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &before);
            pick(&mut h, "Dotted");
            assert_edit(&h.state().app, &before, payload(json!("dotted"), [true; 6]));
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn cell_stroke_authoritative_refresh_complete_localized_names_narrow_parent_and_native_numeric_fields_stay_honest() {
        for language in ["en", "de"] {
            let (mut app, _) = fixture();
            app.ui.language = language.to_owned();
            let mut h = harness(app, true, Parent::Stroke, 190.0);
            let before = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &before);
            assert!(control(&h).rect.is_finite());
            assert_eq!(control(&h).facts.unwrap().name.as_deref(), Some(tr(language, "Type")));
            assert_eq!(
                h.query_all_by(|n| n.role() == egui::accesskit::Role::TextInput && n.value().as_deref() == Some("2.375 pt")).count(),
                1,
                "the untouched real native point field retains its precision"
            );
            h.state_mut().app.run("table.setCell", json!({"stroke": {"type": "wavy", "edges": "all"}})).unwrap();
            let before = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_quiet(&h.state().app, &before);
            assert_eq!(h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &name(&h)).value().as_deref(), Some(tr(language, "Wavy")));
            pick(&mut h, STYLE);
            assert_edit(&h.state().app, &before, payload(json!({"kind": "style", "name": STYLE}), [true; 6]));
            assert_eq!(
                h.get_by_role_and_label(egui::accesskit::Role::ComboBox, &name(&h)).value().as_deref(),
                Some(STYLE),
                "full accessible value, not an elided display fragment"
            );
            undo_redo(&mut h, &before);
        }
    }

    #[test]
    fn cell_stroke_real_cell_options_keyboard_popup_cancel_commit_and_outer_escape_keep_the_actual_modal_transaction() {
        let (mut app, _) = fixture();
        crate::dialogs::open_cell_options(&mut app).unwrap();
        let mut h = harness(app, true, Parent::Dialog, 640.0);
        let before = quiet(&h.state().app);
        let original = h.state().app.ui.dialog.as_ref().unwrap().fields.clone();
        tab_to(&mut h);
        key(&mut h, Key::Enter);
        assert_popup_physical_owner(&h);
        assert_eq!(h.state().app.ui.dialog.as_ref().unwrap().fields, original);
        key(&mut h, Key::End);
        assert_quiet(&h.state().app, &before);
        assert_eq!(h.state().app.ui.dialog.as_ref().unwrap().fields, original, "popup proposal is not a dialog draft edit");
        key(&mut h, Key::Escape);
        assert_eq!(h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(), 0);
        assert_eq!(h.state().app.ui.dialog.as_ref().unwrap().fields, original, "inner Escape closes only the popup");
        assert_quiet(&h.state().app, &before);
        tab_to(&mut h);
        key(&mut h, Key::Enter);
        assert_popup_physical_owner(&h);
        key(&mut h, Key::End);
        key(&mut h, Key::Enter);
        let mut expected = original;
        expected.insert("type".into(), json!({"kind": "style", "name": "Solid"}));
        assert_eq!(h.state().app.ui.dialog.as_ref().unwrap().fields, expected, "Enter selects exactly one draft field without confirming the modal");
        assert_quiet(&h.state().app, &before);
        key(&mut h, Key::Escape);
        assert!(h.state().app.ui.dialog.is_none(), "outer Escape must still cancel the genuine Cell Options modal after an SDK child was used");
        assert_quiet(&h.state().app, &before);
    }

    #[test]
    fn cell_stroke_real_cell_options_held_choice_retires_target_catalog_and_document_before_touching_its_draft() {
        for change in ["range", "catalog", "document"] {
            let (mut app, made) = fixture();
            crate::dialogs::open_cell_options(&mut app).unwrap();
            let mut h = harness(app, true, Parent::Dialog, 640.0);
            let draft = h.state().app.ui.dialog.as_ref().unwrap().fields.clone();
            open(&mut h);
            let at = reveal_popup_option(&mut h, STYLE, None).rect.center();
            frame(&mut h, Some(Event::PointerMoved(at)));
            pointer(&mut h, at, true);
            match change {
                "range" => select(&mut h.state_mut().app, made.story, made.table, [0, 0], [0, 1]),
                "catalog" => {
                    h.state_mut().app.run("strokeStyle.delete", json!({"name": STYLE})).unwrap();
                }
                "document" => {
                    while !h.state().app.session.documents().is_empty() {
                        h.state_mut().app.session.close_document(0);
                    }
                }
                _ => unreachable!(),
            }
            let before = quiet(&h.state().app);
            idle(&mut h, 2);
            assert_eq!(
                h.query_all_by_role(egui::accesskit::Role::ListBoxOption).count(),
                0,
                "old popup disappears before input release after {change}"
            );
            pointer(&mut h, at, false);
            idle(&mut h, 2);
            assert_eq!(h.state().app.ui.dialog.as_ref().unwrap().fields, draft, "stale popup cannot write a new target's modal draft after {change}");
            assert_quiet(&h.state().app, &before);
            click(&mut h, egui::accesskit::Role::Button, "Cancel");
            assert!(h.state().app.ui.dialog.is_none());
            assert_quiet(&h.state().app, &before);
        }
    }

    #[test]
    fn cell_stroke_framework_cell_options_edge_proxy_reseeds_the_draft_and_limits_the_type_to_the_chosen_edges() {
        let (mut app, _) = fixture();
        crate::dialogs::open_cell_options(&mut app).unwrap();
        let mut h = harness(app, true, Parent::Dialog, 640.0);
        let before = quiet(&h.state().app);
        let lang = h.state().app.ui.language.clone();
        // The framework proxy's lines are its own accessible check boxes, not guessed coordinates.
        for line in ["Top", "Bottom", "Left", "Right", "Inner Vertical"] {
            click(&mut h, egui::accesskit::Role::CheckBox, tr(&lang, line));
        }
        let inner = [false, false, false, false, true, false];
        let fields = h.state().app.ui.dialog.as_ref().unwrap().fields.clone();
        assert_eq!(fields["edges"], edges_param(&inner), "only the inner horizontal lines are chosen");
        let shown = current(&mut h.state_mut().app, &inner);
        for key in ["weight", "type", "color", "tint", "gapColor", "gapTint"] {
            assert_eq!(fields["seed"][key], shown[key], "{key} reseeds from the chosen edges");
            assert_eq!(fields[key], shown[key], "untouched {key} shows the chosen edges' value");
        }
        assert_quiet(&h.state().app, &before);
        pick(&mut h, "Dotted");
        assert_quiet(&h.state().app, &before);
        click(&mut h, egui::accesskit::Role::Button, "OK");
        assert!(h.state().app.ui.dialog.is_none());
        assert_edit(&h.state().app, &before, payload(json!("dotted"), inner));
        undo_redo(&mut h, &before);
    }

    #[test]
    fn cell_stroke_framework_diagonal_lines_segment_and_draw_order_apply_as_one_cell_options_edit() {
        let (mut app, _) = fixture();
        crate::dialogs::open_diagonal_lines(&mut app).unwrap();
        let mut h = harness(app, true, Parent::Dialog, 640.0);
        let before = quiet(&h.state().app);
        let lang = h.state().app.ui.language.clone();
        click(&mut h, egui::accesskit::Role::RadioButton, tr(&lang, "Crossed Diagonal Lines"));
        click(&mut h, egui::accesskit::Role::ComboBox, tr(&lang, "Draw"));
        click(&mut h, egui::accesskit::Role::ListBoxOption, tr(&lang, "Content In Front"));
        assert_quiet(&h.state().app, &before);
        let fields = h.state().app.ui.dialog.as_ref().unwrap().fields.clone();
        assert_eq!((&fields["lines"], &fields["inFront"]), (&json!("crossed"), &json!(false)), "picks are draft edits only");
        // OK applies exactly the two changed values to the cells the dialog opened for.
        let mut expected = fields["cells"].clone();
        expected["diagonal"] = json!({"lines": "crossed", "inFront": false});
        click(&mut h, egui::accesskit::Role::Button, "OK");
        assert!(h.state().app.ui.dialog.is_none());
        assert_edit(&h.state().app, &before, expected);
        undo_redo(&mut h, &before);
    }
}
