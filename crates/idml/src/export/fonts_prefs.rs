use super::common::{bool_s, p, pct, pkg_root};
use super::spread::margin_el;
use super::{El, Ex, PARA_BUILTINS, document, escape_id, names, num, pt, st};

impl<'a> Ex<'a> {
    // ---------- Fonts.xml ----------

    pub(super) fn note_font(&mut self, family: &str, style: &str) {
        self.fonts.entry(family.to_string()).or_default().insert(style.to_string());
    }

    pub(super) fn fonts_part(&mut self) -> Vec<u8> {
        let mut root = pkg_root("Fonts");
        for f in &self.d.styles.composite_fonts {
            let mut el = El::new("CompositeFont").attr("Self", format!("CompositeFont/{}", escape_id(&f.name))).attr("Name", &f.name);
            for (i, e) in f.entries.iter().enumerate() {
                self.note_font(&e.family, &e.style);
                el.push(
                    El::new("CompositeFontEntry")
                        .attr("Self", format!("dcComposite{}_{i}", escape_id(&f.name)))
                        .attr("Name", &e.name)
                        .attr("CustomCharacters", &e.characters)
                        .attr("FontStyle", &e.style)
                        .attr("RelativeSize", num(e.relative_size * 100.0))
                        .attr("HorizontalScale", num(e.horizontal_scale * 100.0))
                        .attr("VerticalScale", num(e.vertical_scale * 100.0))
                        .attr("BaselineShift", num(e.baseline_shift * 100.0))
                        .attr("ScaleOption", bool_s(e.scale_option))
                        .child(El::new("Properties").child(p("AppliedFont", "string", &e.family))),
                );
            }
            root.push(el);
        }
        let fonts = std::mem::take(&mut self.fonts);
        for (fam, styles) in &fonts {
            let fid = format!("di{:x}", {
                self.next += 1;
                self.next
            });
            let mut fe = El::new("FontFamily").attr("Self", &fid).attr("Name", fam);
            for s in styles {
                fe.push(
                    El::new("Font")
                        .attr("Self", format!("{fid}Fontn{fam} {s}"))
                        .attr("FontFamily", fam)
                        .attr("Name", format!("{fam} {s}"))
                        .attr("PostScriptName", format!("$ID/{}-{}", fam.replace(' ', ""), s.replace(' ', "")))
                        .attr("Status", "NotAvailable")
                        .attr("FontStyleName", s)
                        .attr("FontType", "OpenTypeTT")
                        .attr("WritingScript", "0")
                        .attr("FullName", format!("{fam} {s}"))
                        .attr("FullNameNative", format!("{fam} {s}"))
                        .attr("FontStyleNameNative", s)
                        .attr("PlatformName", "$ID/")
                        .attr("Version", "$ID/")
                        .attr("TypekitID", "$ID/"),
                );
            }
            root.push(fe);
        }
        self.fonts = fonts;
        document(&root)
    }

    // ---------- Preferences.xml ----------

    pub(super) fn prefs_part(&mut self) -> Vec<u8> {
        let d = self.d;
        let s = &d.settings;
        let mut root = pkg_root("Preferences");
        root.push(
            El::new("DocumentPreference")
                .attr("PageHeight", num(s.page_height))
                .attr("PageWidth", num(s.page_width))
                // InDesign creates this many pages before reading the spreads and replaces only the first
                // spread, so (as InDesign itself writes) the value must be 1.
                .attr("PagesPerDocument", 1)
                .attr("FacingPages", bool_s(s.facing_pages))
                .attr("DocumentBleedTopOffset", num(s.bleed[0]))
                .attr("DocumentBleedBottomOffset", num(s.bleed[1]))
                .attr("DocumentBleedInsideOrLeftOffset", num(s.bleed[2]))
                .attr("DocumentBleedOutsideOrRightOffset", num(s.bleed[3]))
                .attr("DocumentBleedUniformSize", bool_s(s.bleed.iter().all(|b| *b == s.bleed[0])))
                .attr("SlugTopOffset", num(s.slug[0]))
                .attr("SlugBottomOffset", num(s.slug[1]))
                .attr("SlugInsideOrLeftOffset", num(s.slug[2]))
                .attr("SlugRightOrOutsideOffset", num(s.slug[3]))
                .attr("DocumentSlugUniformSize", bool_s(s.slug.iter().all(|b| *b == s.slug[0])))
                .attr("PreserveLayoutWhenShuffling", "true")
                .attr("AllowPageShuffle", "true")
                .attr("OverprintBlack", bool_s(s.overprint_black))
                .attr("PageBinding", if s.right_to_left_binding { "RightToLeft" } else { "LeftToRight" })
                .attr("ColumnDirection", "Horizontal")
                .attr(
                    "Intent",
                    match s.intent {
                        designcraft_doc::Intent::Print => "PrintIntent",
                        designcraft_doc::Intent::Web => "WebIntent",
                        designcraft_doc::Intent::Mobile => "MobileIntent",
                    },
                )
                .attr("CreatePrimaryTextFrame", bool_s(s.primary_text_frame))
                .attr("ColumnGuideLocked", bool_s(s.column_guides_locked))
                .attr("MasterTextFrame", bool_s(s.primary_text_frame))
                .attr("SnippetImportUsesOriginalLocation", "false"),
        );
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
        root.push(
            El::new("GridPreference")
                .attr("HorizontalGridlineDivision", num(s.grid.horizontal))
                .attr("VerticalGridlineDivision", num(s.grid.vertical))
                .attr("HorizontalGridSubdivision", s.grid.subdivisions)
                .attr("VerticalGridSubdivision", s.grid.subdivisions)
                .attr("GridsInBack", bool_s(s.grid.in_back))
                .attr("BaselineStart", num(bg.start))
                .attr("BaselineDivision", num(bg.increment))
                .attr("BaselineViewThreshold", pct(bg.view_threshold))
                .attr(
                    "BaselineGridRelativeOption",
                    match bg.relative_to {
                        designcraft_doc::GridRelative::TopOfPage => "TopOfPageOfBaselineGridRelativeOption",
                        designcraft_doc::GridRelative::TopMargin => "TopOfMarginOfBaselineGridRelativeOption",
                    },
                ),
        );
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
