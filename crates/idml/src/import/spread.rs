use super::frame::{fit_columns, margins_of};
use super::values::{nums, parse_affine};
use super::{
    Affine, Arc, Columns, El, Guide, Importer, ItemId, Margins, Orientation, Page, PageId, PageSide, ParentInfo, Point, Rect, Spread, SpreadId, names,
};

/// View › Rotate Spread from a `Spread` element's `ItemTransform`: quarter turns clockwise on
/// screen (IDML y points down, so `0 1 -1 0` is one clockwise turn). The spread's items and
/// pages stay in unrotated spread coordinates, so only the rotation part is read. Anything other
/// than an exact quarter turn (identity, scale, other angles, malformed numbers) is no rotation.
fn spread_view_rotation(s: Option<&str>) -> u8 {
    let parsed: std::result::Result<Vec<f64>, _> = s.unwrap_or("").split_whitespace().take(7).map(str::parse::<f64>).collect();
    let Some([a, b, c, d, _, _]) = parsed.ok().and_then(|v| <[f64; 6]>::try_from(v).ok()) else { return 0 };
    let is = |x: f64, y: f64| (x - y).abs() < 1e-6;
    let quarter = |m: [f64; 4]| is(a, m[0]) && is(b, m[1]) && is(c, m[2]) && is(d, m[3]);
    if quarter([0.0, 1.0, -1.0, 0.0]) {
        1
    } else if quarter([-1.0, 0.0, 0.0, -1.0]) {
        2
    } else if quarter([0.0, -1.0, 1.0, 0.0]) {
        3
    } else {
        0
    }
}

impl<'r> Importer<'r> {
    // ---------- spreads and items ----------

    /// Ruler guides of a `<Page>` (its `<Guide>` children), `page` its rectangle in our spread
    /// coordinates. A guide's Location is measured from the ruler origin: the spread's top left, the
    /// page's, or the spine. Fit to Page off: the guide crosses the spread.
    fn guides_of(&self, p: &El, page: Rect, spine: f64) -> Vec<Guide> {
        p.find_all("Guide")
            .filter_map(|g| {
                let at = g.num("Location").filter(|v| v.is_finite() && v.abs() < 1e7)?;
                let orientation = if g.get("Orientation") == Some("Horizontal") { Orientation::Horizontal } else { Orientation::Vertical };
                let position = match (orientation, self.settings.ruler_origin) {
                    (Orientation::Horizontal, designcraft_doc::RulerOrigin::Page) => page.y0 + at,
                    (Orientation::Horizontal, _) => at,
                    (Orientation::Vertical, designcraft_doc::RulerOrigin::Page) => page.x0 + at,
                    (Orientation::Vertical, designcraft_doc::RulerOrigin::Spine) => spine + at,
                    (Orientation::Vertical, designcraft_doc::RulerOrigin::Spread) => at,
                };
                Some(Guide {
                    orientation,
                    position,
                    spread: g.get("FitToPage") == Some("false"),
                    locked: g.get("Locked") == Some("true"),
                    layer: g.get("ItemLayer").and_then(|l| self.layer_ids.get(l)).copied(),
                    liquid: g.get("GuideType") == Some("Liquid"),
                })
            })
            .collect()
    }

    /// The page rectangles of a spread element, in IDML spread coordinates.
    fn page_rects<'a>(&self, e: &'a El) -> Vec<(Rect, &'a El)> {
        let mut pages: Vec<(Rect, &'a El)> = Vec::new();
        for p in e.find_all("Page") {
            let gb = nums(p.get("GeometricBounds").unwrap_or(""));
            let (t, l, bt, r) =
                if gb.len() == 4 { (gb[0], gb[1], gb[2], gb[3]) } else { (0.0, 0.0, self.settings.page_height, self.settings.page_width) };
            let xf = parse_affine(p.get("ItemTransform"));
            let a = xf * Point::new(l, t);
            let c = xf * Point::new(r, bt);
            pages.push((Rect::from_points(a, c), p));
        }
        pages
    }

    /// A page's margins and columns, and whether they are local (differ from its parent page's).
    fn page_margins(&self, p: &El, r: &Rect, master: bool, parent: Option<SpreadId>, (side, index): (PageSide, usize)) -> (Margins, Columns, bool) {
        // A document page's margins are local when they differ from its parent page's; a
        // page without its own takes the parent page's.
        let from_parent = if master {
            None
        } else {
            parent
                .and_then(|id| self.parents.iter().find(|sp| sp.id == id))
                .and_then(|sp| sp.pages.get(sp.parent_page_at(side, index)?))
                .map(|q| (q.margins, q.columns.clone()))
        };
        let (margins, columns) = match (p.find("MarginPreference"), &from_parent) {
            (Some(m), _) => {
                let (margins, columns) = margins_of(m);
                (margins, fit_columns(columns, &margins, r.width()))
            }
            (None, Some(own)) => own.clone(),
            (None, None) => {
                let (margins, columns) = self.default_margins.clone().unwrap_or((Margins::uniform(36.0), Columns::default()));
                (margins, fit_columns(columns, &margins, r.width()))
            }
        };
        let local_margins = from_parent.is_some_and(|(m, c)| m != margins || c != columns);
        (margins, columns, local_margins)
    }

    pub(super) fn spread(&mut self, e: &El, id: SpreadId, master: bool) -> Spread {
        let facing = self.settings.facing_pages;
        // Page rectangles in IDML spread coordinates.
        let pages = self.page_rects(e);
        let min_x = pages.iter().map(|(r, _)| r.x0).fold(f64::INFINITY, f64::min);
        let min_y = pages.iter().map(|(r, _)| r.y0).fold(f64::INFINITY, f64::min);
        let (min_x, min_y) = if min_x.is_finite() { (min_x, min_y) } else { (0.0, 0.0) };
        let shift = Affine::translate((-min_x, -min_y));
        // The spine: between a left and a right page (else the spread's left edge).
        let spine = pages.iter().map(|(r, _)| r.x0 - min_x).filter(|x| *x > 1e-6).fold(f64::INFINITY, f64::min);
        let spine = if spine.is_finite() && facing { spine } else { 0.0 };
        let show = e.get("ShowMasterItems") != Some("false");
        let first_abs: usize = self.spreads.iter().map(|s| s.pages.len()).sum();
        let view_rotation = if master { 0 } else { spread_view_rotation(e.get("ItemTransform")) };
        let mut out_pages = Vec::new();
        for (i, (r, p)) in pages.iter().enumerate() {
            let pid = PageId(self.alloc());
            if let Some(s) = p.get("Self")
                && !master
            {
                self.page_index.insert(s.to_string(), first_abs + i);
            }
            let side = if !facing {
                PageSide::Single
            } else if r.x1 <= 1e-6 {
                PageSide::Left
            } else {
                PageSide::Right
            };
            let parent = p.get("AppliedMaster").and_then(|m| self.parent_ids.get(m)).copied();
            let (margins, columns, local_margins) = self.page_margins(p, r, master, parent, (side, i));
            out_pages.push(Page {
                id: pid,
                width: r.width(),
                height: r.height(),
                x: r.x0 - min_x,
                margins,
                columns,
                local_margins,
                parent,
                side,
                overridden: vec![],
                guides: self.guides_of(p, Rect::new(r.x0 - min_x, r.y0 - min_y, r.x1 - min_x, r.y1 - min_y), spine),
                show_parent_items: show,
                liquid: names::liquid_rule_in(p.get("LayoutRule").unwrap_or("Off")),
                transition: None,
                view_rotation: if i == 0 { view_rotation } else { 0 },
                show_parent_overlay: false,
            });
        }
        let overrides: Vec<Vec<String>> =
            pages.iter().map(|(_, p)| p.get("OverrideList").unwrap_or("").split_whitespace().map(str::to_string).collect()).collect();
        let mut items = Vec::new();
        for c in e.elements() {
            if let Some(it) = self.item(c, shift) {
                items.push(Arc::new(it));
            }
        }
        for (pg, ov) in out_pages.iter_mut().zip(overrides) {
            pg.overridden = ov.iter().filter_map(|s| self.item_ids.get(s)).copied().filter(|i| self.is_parent_item(*i)).collect();
        }
        let parent = if master {
            let prefix =
                e.get("NamePrefix").map(str::to_string).unwrap_or_else(|| e.get("Name").unwrap_or("A").split('-').next().unwrap_or("A").to_string());
            let name = e.get("BaseName").map(str::to_string).unwrap_or_else(|| {
                let n = e.get("Name").unwrap_or("Parent");
                n.split_once('-').map(|(_, b)| b.to_string()).unwrap_or_else(|| n.to_string())
            });
            // A parent spread has one base: its first page's. Pages that disagree lose theirs.
            let applied: Vec<Option<SpreadId>> =
                pages.iter().map(|(_, p)| p.get("AppliedMaster").and_then(|m| self.parent_ids.get(m)).copied()).collect();
            let label = format!("{prefix}-{name}");
            if applied.windows(2).any(|w| w[0] != w[1]) {
                self.warnings.push(format!("parent {label}: its pages are based on different parents; only the first page's is kept"));
            }
            let based_on = applied.first().copied().flatten();
            if applied.contains(&Some(id)) {
                self.warnings.push(format!("parent {label}: a page names its own parent as its base, which is ignored"));
            }
            let based_on = based_on.filter(|b| *b != id);
            Some(ParentInfo { prefix, name, based_on })
        } else {
            None
        };
        for p in &mut out_pages {
            if master {
                p.parent = None;
            }
        }
        Spread { id, pages: out_pages, items, parent, allow_shuffle: e.get("AllowPageShuffle") != Some("false") }
    }

    pub(super) fn is_parent_item(&self, id: ItemId) -> bool {
        self.parents.iter().any(|p| {
            p.items.iter().any(|it| {
                let mut found = false;
                it.walk(&mut |i| found |= i.id == id);
                found
            })
        })
    }
}
