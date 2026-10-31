//! Shared synthetic Book fixtures and page-label width regressions.
use super::super::*;
use designcraft_doc::Margins;
use designcraft_doc::build::NewDocument;
use std::path::{Path, PathBuf};

pub(crate) type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn toc_document(width: f64, height: f64, primary: bool, separator: &str) -> Document {
    let mut d = Document::new(&NewDocument {
        width: width + 40.0,
        height: height + 40.0,
        facing_pages: false,
        primary_text_frame: true,
        margins: Margins::uniform(20.0),
        ..Default::default()
    });
    let story = d.settings.primary_story.unwrap_or(StoryId(0));
    let mut entry = TocEntry::new("Head", 1);
    entry.separator = Some(separator.into());
    d.toc = Some(Toc {
        scope: TocScope::EntireBook,
        story,
        title: String::new(),
        entries: vec![entry],
        page_numbers: true,
        title_style: None,
        create_bookmarks: false,
    });
    if !primary {
        d.settings.primary_story = None;
    }
    d
}

pub(crate) fn chapter(label: &str, count: usize, pages: usize) -> TestResult<Document> {
    let paras = (0..count).map(|i| (format!("{label} {:02}", i + 1), "Head")).collect::<Vec<_>>();
    let borrowed = paras.iter().map(|(t, s)| (t.as_str(), *s)).collect::<Vec<_>>();
    let (s, _) = super::super::tests::doc_with(&borrowed);
    let mut d = (*s.doc()?.doc).clone();
    if pages == 1 {
        d.delete_pages(&[1])?;
    } else if pages > 2 {
        d.insert_pages(None, pages - 2, None)?;
    }
    Ok(d)
}

pub(crate) struct BookFixture {
    pub(crate) dir: PathBuf,
    pub(crate) s: Session,
}
impl BookFixture {
    pub(crate) fn new(root: &Path, name: &str, docs: Vec<(&str, Document)>) -> TestResult<Self> {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir)?;
        let mut paths = Vec::new();
        for (name, d) in docs {
            let file = format!("{name}.designcraft");
            std::fs::write(dir.join(&file), designcraft_format::save(&d)?)?;
            paths.push(file);
        }
        let manifest = dir.join("test.dcbook");
        std::fs::write(&manifest, serde_json::to_vec_pretty(&json!({"documents":paths}))?)?;
        let mut s = Session::new();
        s.execute("book.open", &json!({"path":manifest}))?;
        s.execute("book.paginate", &json!({}))?;
        let toc = paths.iter().position(|p| p == "toc.designcraft").ok_or("TOC missing")?;
        s.execute("book.openDocument", &json!({"index":toc}))?;
        Ok(Self { dir, s })
    }
    pub(crate) fn mutate(&self, name: &str, change: impl FnOnce(&mut Document) -> TestResult) -> TestResult {
        let path = self.dir.join(format!("{name}.designcraft"));
        let mut d = designcraft_format::load(&std::fs::read(&path)?)?;
        change(&mut d)?;
        std::fs::write(path, designcraft_format::save(&d)?)?;
        Ok(())
    }
}

pub(crate) fn exact_headings(text: &str, count: usize) -> TestResult<Document> {
    let mut d = chapter("placeholder", count, 1)?;
    for story in d.stories.values_mut() {
        let len = story.len();
        std::sync::Arc::make_mut(story).replace(0..len, &vec![text; count].join("\n"));
    }
    Ok(d)
}

fn number_probe(text: &str, label: &str, width: f64, separator: &str) -> TestResult<Value> {
    let mut d = toc_document(width, 600.0, true, separator);
    let toc = d.toc.clone().ok_or("no TOC")?;
    let found = Found {
        document: Some("calibration".into()),
        source: (StoryId(1), 0),
        level: 1,
        text: text.into(),
        page: label.into(),
        entry: 0,
        key: (0, 0, 0),
    };
    fill(&mut d, &toc, Some(width), Some(vec![found]))?;
    let cs = compose_story(&d, toc.story, &Default::default());
    Ok(
        json!({"label":label,"separator":separator,"width":width,"lines":cs.line_count(),"height":cs.frames.first().map(|f|f.content_height),"text":d.story(toc.story).map(|s|s.text.clone())}),
    )
}

#[test]
fn digit_width_boundaries_change_existing_toc_composition() -> TestResult {
    for (old, new, width) in [("9", "10", 107.75), ("99", "100", 113.75)] {
        for separator in [" ", "\t"] {
            assert_eq!(number_probe("Measured heading", old, width, separator)?["lines"], 1);
            assert_eq!(number_probe("Measured heading", new, width, separator)?["lines"], 2);
        }
    }
    Ok(())
}

pub(crate) fn set_moving_text(d: &mut Document, body: &str) -> TestResult {
    let sid = *d.stories.keys().next().ok_or("story")?;
    let story = d.story_mut(sid).ok_or("story")?;
    story.replace(0..story.len(), &format!("{body}\nMovable 01"));
    story.format_paras(0..0, |p| *p = ParaFormat::default());
    let at = body.len() + 1;
    story.format_paras(at..at, |p| *p = ParaFormat { style: "Head".into(), ..Default::default() });
    Ok(())
}

pub(crate) fn moving_heading_document() -> TestResult<Document> {
    let mut d = chapter("Movable", 1, 2)?;
    let from = *d.stories.values().next().and_then(|s| s.frames.first()).ok_or("frame")?;
    let (to, _) = d.add_text_frame(SpreadRef::Doc(1), Rect::new(36.0, 36.0, 576.0, 756.0), d.default_layer(), "", ParaFormat::default())?;
    d.thread(from, to)?;
    set_moving_text(&mut d, "Short body")?;
    Ok(d)
}
