use super::common::{bool_s, pkg_root};
use super::spread::margin_el;
use super::{El, Ex, PARA_BUILTINS, document, escape_id, names, num, pt, st};
use craft_idml::model::{CompositeFontDefinition, CompositeFontEntryDefinition, FontDefinition, FontFamilyDefinition, FontsPart};

impl<'a> Ex<'a> {
    // ---------- Fonts.xml ----------

    pub(super) fn note_font(&mut self, family: &str, style: &str) {
        self.fonts.entry(family.to_string()).or_default().insert(style.to_string());
    }

    pub(super) fn fonts_part(&mut self) -> Vec<u8> {
        let mut fonts_part = FontsPart::default();
        for f in &self.d.styles.composite_fonts {
            let mut entries = Vec::new();
            for (i, e) in f.entries.iter().enumerate() {
                self.note_font(&e.family, &e.style);
                entries.push(CompositeFontEntryDefinition {
                    self_id: format!("dcComposite{}_{i}", escape_id(&f.name)),
                    name: e.name.clone(),
                    characters: e.characters.clone(),
                    font_style: e.style.clone(),
                    relative_size: num(e.relative_size * 100.0),
                    horizontal_scale: num(e.horizontal_scale * 100.0),
                    vertical_scale: num(e.vertical_scale * 100.0),
                    baseline_shift: num(e.baseline_shift * 100.0),
                    scale_option: e.scale_option,
                    applied_font: e.family.clone(),
                });
            }
            fonts_part.composites.push(CompositeFontDefinition {
                self_id: format!("CompositeFont/{}", escape_id(&f.name)),
                name: f.name.clone(),
                entries,
            });
        }
        let fonts = std::mem::take(&mut self.fonts);
        for (fam, styles) in &fonts {
            let fid = format!("di{:x}", {
                self.next += 1;
                self.next
            });
            let mut faces = Vec::new();
            for s in styles {
                let name = format!("{fam} {s}");
                faces.push(FontDefinition {
                    self_id: format!("{fid}Fontn{fam} {s}"),
                    family: fam.clone(),
                    name,
                    postscript_name: format!("$ID/{}-{}", fam.replace(' ', ""), s.replace(' ', "")),
                    style: s.clone(),
                });
            }
            fonts_part.families.push(FontFamilyDefinition { self_id: fid, name: fam.clone(), fonts: faces });
        }
        self.fonts = fonts;
        craft_idml::part_xml(&fonts_part.to_el())
    }

    // ---------- Preferences.xml ----------

    pub(super) fn prefs_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let s = &d.settings;
        let mut root = pkg_root("Preferences");
        let document_preferences = craft_idml::model::DocumentPreferences {
            page_width: Some(s.page_width),
            page_height: Some(s.page_height),
            facing_pages: Some(s.facing_pages),
            page_binding: Some(if s.right_to_left_binding { "RightToLeft" } else { "LeftToRight" }.into()),
            bleed: s.bleed.map(Some),
            slug: s.slug.map(Some),
            overprint_black: Some(s.overprint_black),
            intent: Some(
                match s.intent {
                    designcraft_doc::Intent::Print => "PrintIntent",
                    designcraft_doc::Intent::Web => "WebIntent",
                    designcraft_doc::Intent::Mobile => "MobileIntent",
                }
                .into(),
            ),
            primary_text_frame: Some(s.primary_text_frame),
            column_guides_locked: Some(s.column_guides_locked),
        };
        let mut document_preference = El::new("DocumentPreference");
        for (key, value) in document_preferences.to_export_attrs() {
            document_preference.set(&key, value);
        }
        root.push(document_preference);
        let page = d.parents.first().and_then(|p| p.pages.first()).or_else(|| d.page(0));
        if let Some(pg) = page {
            root.push(margin_el(pg));
        }
        root.push(
            El::new("ViewPreference")
                .attr("HorizontalMeasurementUnits", names::unit_out(s.horizontal_units))
                .attr("VerticalMeasurementUnits", names::unit_out(s.vertical_units))
                .attr(
                    "RulerOrigin",
                    match s.ruler_origin {
                        designcraft_doc::RulerOrigin::Spread => "SpreadOrigin",
                        designcraft_doc::RulerOrigin::Page => "PageOrigin",
                        designcraft_doc::RulerOrigin::Spine => "SpineOrigin",
                    },
                )
                .attr("CursorKeyIncrement", num(s.keyboard_increment))
                .attr("PointsPerInch", "72"),
        );
        let bg = &s.baseline_grid;
        let grid_preferences = craft_idml::model::GridPreferences {
            horizontal_division: Some(s.grid.horizontal),
            vertical_division: Some(s.grid.vertical),
            horizontal_subdivision: Some(f64::from(s.grid.subdivisions)),
            vertical_subdivision: Some(f64::from(s.grid.subdivisions)),
            grids_in_back: Some(s.grid.in_back),
            baseline_start: Some(bg.start),
            baseline_division: Some(bg.increment),
            baseline_view_threshold: Some(bg.view_threshold * 100.0),
            baseline_relative_option: Some(
                match bg.relative_to {
                    designcraft_doc::GridRelative::TopOfPage => "TopOfPageOfBaselineGridRelativeOption",
                    designcraft_doc::GridRelative::TopMargin => "TopOfMarginOfBaselineGridRelativeOption",
                }
                .into(),
            ),
        };
        let mut grid_preference = El::new("GridPreference");
        for (key, value) in grid_preferences.to_attrs() {
            grid_preference.set(&key, value);
        }
        root.push(grid_preference);
        root.push(El::new("PasteboardPreference").attr("PasteboardMargins", pt(s.pasteboard.0, s.pasteboard.1)));
        root.push(self.footnote_option_el());
        root.push(El::new("TransparencyPreference").attr(
            "BlendingSpace",
            match s.blend_space {
                designcraft_doc::BlendSpace::Cmyk => "CMYK",
                designcraft_doc::BlendSpace::Rgb => "RGB",
            },
        ));
        // Split Document: the style is a reference, a built-in one by its `$ID/` name; an empty
        // one splits on the styles' export tags.
        let (break_doc, split_style) = match &s.epub_split {
            designcraft_doc::EpubSplit::Off => (false, names::style_name_out(PARA_BUILTINS, st::BASIC_PARAGRAPH)),
            designcraft_doc::EpubSplit::ExportTags => (true, String::new()),
            designcraft_doc::EpubSplit::Style(n) if PARA_BUILTINS.iter().any(|(o, _)| o == n) => (true, names::style_name_out(PARA_BUILTINS, n)),
            designcraft_doc::EpubSplit::Style(n) => (true, names::style_self("ParagraphStyle", PARA_BUILTINS, n)),
        };
        root.push(El::new("EPubExportPreference").attr("BreakDocument", bool_s(break_doc)).attr("ParagraphStyleName", split_style));
        let a = &s.advanced_type;
        root.push(
            El::new("TextPreference")
                .attr("SuperscriptSize", num(a.superscript_size))
                .attr("SuperscriptPosition", num(a.superscript_position))
                .attr("SubscriptSize", num(a.subscript_size))
                .attr("SubscriptPosition", num(a.subscript_position)),
        );
        // InDesign always writes the document's default story preference; readers such as Okapi's IDML
        // filter take the spread reading order from it and fail without it.
        root.push(
            El::new("StoryPreference")
                .attr("OpticalMarginAlignment", "false")
                .attr("OpticalMarginSize", "12")
                .attr("FrameType", "TextFrameType")
                .attr("StoryOrientation", "Horizontal")
                .attr("StoryDirection", "LeftToRightDirection"),
        );
        document(&root)
    }
}
