//! DesignCraft's IDML model adapter, using `craft-xml` for parsing and writing XML.

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    El(El),
    Text(String),
    /// Written as `<![CDATA[…]]>`; the parser returns CDATA as `Text`.
    CData(String),
    /// Processing instruction: (target, content).
    Pi(String, String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl El {
    pub fn new(name: &str) -> Self {
        El { name: name.into(), attrs: Vec::new(), children: Vec::new() }
    }

    // ---------- building ----------

    pub fn attr(mut self, k: &str, v: impl ToString) -> Self {
        self.set(k, v);
        self
    }
    pub fn set(&mut self, k: &str, v: impl ToString) {
        let v = v.to_string();
        match self.attrs.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v,
            None => self.attrs.push((k.into(), v)),
        }
    }
    pub fn child(mut self, c: El) -> Self {
        self.children.push(Node::El(c));
        self
    }
    pub fn push(&mut self, c: El) {
        self.children.push(Node::El(c));
    }
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.children.push(Node::Text(t.into()));
        self
    }

    // ---------- reading ----------

    /// Local name (namespace prefix stripped).
    pub fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            _ => None,
        })
    }
    pub fn find(&self, local: &str) -> Option<&El> {
        self.elements().find(|e| e.local() == local)
    }
    pub fn find_mut(&mut self, local: &str) -> Option<&mut El> {
        self.children.iter_mut().find_map(|n| match n {
            Node::El(e) if e.local() == local => Some(e),
            _ => None,
        })
    }
    pub fn find_all<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.elements().filter(move |e| e.local() == local)
    }
    /// Concatenated text content of direct text children.
    pub fn text_content(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) | Node::CData(t) => s.push_str(t),
                Node::El(e) => s.push_str(&e.text_content()),
                Node::Pi(..) => {}
            }
        }
        s
    }
    /// A property in either form: an attribute, or a `<Properties><Name …>value</Name>` child.
    pub fn prop(&self, k: &str) -> Option<String> {
        if let Some(v) = self.get(k) {
            return Some(v.to_string());
        }
        self.prop_el(k).map(El::text_content)
    }
    pub fn prop_el(&self, k: &str) -> Option<&El> {
        self.find("Properties").and_then(|p| p.find(k))
    }
    /// A finite numeric property; `NaN` and infinities count as absent.
    pub fn num(&self, k: &str) -> Option<f64> {
        self.prop(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
    pub fn boolean(&self, k: &str) -> Option<bool> {
        match self.prop(k)?.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    // ---------- writing ----------

    #[cfg(test)]
    pub fn write(&self, out: &mut String, depth: usize) {
        indent(out, depth);
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape_into(out, v, true);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>\n");
            return;
        }
        out.push('>');
        let inline = self.children.iter().all(|c| !matches!(c, Node::El(_)));
        if !inline {
            out.push('\n');
        }
        for c in &self.children {
            match c {
                Node::El(e) => e.write(out, depth + 1),
                Node::Text(t) => escape_into(out, t, false),
                Node::CData(t) => {
                    out.push_str("<![CDATA[");
                    out.push_str(&t.replace("]]>", "]]]]><![CDATA[>"));
                    out.push_str("]]>");
                }
                Node::Pi(t, c) => {
                    out.push_str("<?");
                    out.push_str(t);
                    if !c.is_empty() {
                        out.push(' ');
                        out.push_str(c);
                    }
                    out.push_str("?>");
                }
            }
        }
        if !inline {
            indent(out, depth);
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push_str(">\n");
    }
}

#[cfg(test)]
fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}

/// XML-escape text. Control characters that XML 1.0 forbids are dropped; tab/CR/LF in
/// attributes become character references so they survive attribute-value normalisation.
#[cfg(test)]
pub fn escape_into(out: &mut String, s: &str, attr: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\t' | '\n' | '\r' if attr => out.push_str(&format!("&#x{:x};", c as u32)),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
}

/// Serialize a part: XML declaration + root element.
pub fn document(root: &El) -> Vec<u8> {
    craft_idml::part_xml(&to_craft(root))
}

pub(crate) fn to_craft(el: &El) -> craft_xml::El {
    craft_xml::El {
        name: el.name.clone(),
        attrs: el.attrs.clone(),
        children: el
            .children
            .iter()
            .map(|node| match node {
                Node::El(child) => craft_xml::Node::El(to_craft(child)),
                Node::Text(text) => craft_xml::Node::Text(text.clone()),
                Node::CData(text) => craft_xml::Node::CData(text.clone()),
                Node::Pi(target, data) => craft_xml::Node::Pi { target: target.clone(), data: data.clone() },
            })
            .collect(),
    }
}

/// How deep elements may nest. InDesign's packages stay under a few dozen levels; the tree is
/// walked and dropped recursively afterwards, so unbounded nesting is refused here, not left to
/// run into the stack (#712).
#[cfg(test)]
pub const MAX_DEPTH: usize = 256;

/// Parse a document into its root element.
#[cfg(test)]
pub fn parse(bytes: &[u8]) -> Result<El, String> {
    let mut options = craft_xml::Options::default();
    options.limits.max_input = 512 << 20;
    options.pis = true;
    let parsed = options.parse(bytes).map_err(|e| match e {
        craft_xml::Error::TooDeep(_) => format!("XML elements nested deeper than {MAX_DEPTH} levels"),
        e => e.to_string(),
    })?;
    Ok(from_craft(parsed))
}

pub(crate) fn from_craft(el: craft_xml::El) -> El {
    El {
        name: el.name,
        attrs: el.attrs,
        children: el
            .children
            .into_iter()
            .filter_map(|n| match n {
                craft_xml::Node::El(e) => Some(Node::El(from_craft(e))),
                craft_xml::Node::Text(t) => Some(Node::Text(t)),
                craft_xml::Node::CData(t) => Some(Node::Text(t)),
                craft_xml::Node::Pi { target, data } => Some(Node::Pi(target, data.trim().to_string())),
                craft_xml::Node::Comment(_) => None,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #712: nesting past the budget is an error from the parser, not a stack overflow later.
    #[test]
    fn nesting_past_the_budget_is_refused() {
        let nested = |n: usize| format!("{}{}", "<A>".repeat(n), "</A>".repeat(n)).into_bytes();
        assert!(parse(&nested(MAX_DEPTH)).is_ok(), "the budget itself parses");
        let err = parse(&nested(MAX_DEPTH + 1)).expect_err("one level more is refused");
        assert!(err.contains("nested deeper"), "{err}");
        assert!(parse(&nested(100_000)).is_err());
    }

    #[test]
    fn roundtrip_text_pi_and_entities() {
        let root = El::new("Root").attr("A", "x\"<&\t").child(El::new("Content").text("a\tb & <c>"));
        let mut c = El::new("Content");
        c.children.push(Node::Text("Page ".into()));
        c.children.push(Node::Pi("ACE".into(), "18".into()));
        let root = root.child(c);
        let bytes = document(&root);
        let back = parse(&bytes).unwrap();
        assert_eq!(back.get("A"), Some("x\"<&\t"));
        let contents: Vec<&El> = back.find_all("Content").collect();
        assert_eq!(contents[0].text_content(), "a\tb & <c>");
        assert_eq!(contents[1].children[1], Node::Pi("ACE".into(), "18".into()));
    }

    #[test]
    fn properties_form() {
        let x = br#"<A PointSize="9"><Properties><Leading type="unit">14</Leading></Properties></A>"#;
        let e = parse(x).unwrap();
        assert_eq!(e.num("PointSize"), Some(9.0));
        assert_eq!(e.num("Leading"), Some(14.0));
        assert_eq!(e.prop("Missing"), None);
    }

    /// `NaN` and infinities parse as `f64` but are not usable numbers.
    #[test]
    fn non_finite_numbers_count_as_absent() {
        let x = br#"<A W="NaN" X="inf"><Properties><Y type="unit">-inf</Y><Z>2.5</Z></Properties></A>"#;
        let e = parse(x).unwrap();
        assert_eq!((e.num("W"), e.num("X"), e.num("Y"), e.num("Z")), (None, None, None, Some(2.5)));
    }
}
