//! A tiny XML DOM on top of `quick-xml`: enough for IDML parts (elements, attributes, text,
//! CDATA and processing instructions such as `<?ACE 18?>`).

use quick_xml::events::Event;

const MAX_XML_DEPTH: usize = 256;
const MAX_ATTRIBUTES_PER_ELEMENT: usize = 4_096;

#[derive(Clone, Copy)]
struct XmlLimits {
    input_bytes: usize,
    events: usize,
    nodes: usize,
    attributes: usize,
    text_bytes: usize,
    allocated_bytes: usize,
    structural_bytes: usize,
}

const IMPORT_XML_LIMITS: XmlLimits = XmlLimits {
    input_bytes: 128 * 1024 * 1024,
    events: 1_000_000,
    nodes: 500_000,
    attributes: 1_000_000,
    text_bytes: 64 * 1024 * 1024,
    allocated_bytes: 128 * 1024 * 1024,
    structural_bytes: 64 * 1024 * 1024,
};

/// Shared accounting across every XML part in one IDML import.
pub(crate) struct XmlBudget {
    limits: XmlLimits,
    input_bytes: usize,
    events: usize,
    nodes: usize,
    attributes: usize,
    text_bytes: usize,
    allocated_bytes: usize,
    structural_bytes: usize,
}

impl Default for XmlBudget {
    fn default() -> Self {
        Self { limits: IMPORT_XML_LIMITS, input_bytes: 0, events: 0, nodes: 0, attributes: 0, text_bytes: 0, allocated_bytes: 0, structural_bytes: 0 }
    }
}

impl XmlBudget {
    fn charge(total: &mut usize, amount: usize, limit: usize, label: &str) -> Result<(), String> {
        *total = total.checked_add(amount).ok_or_else(|| format!("XML {label} count overflow"))?;
        if *total > limit {
            return Err(format!("XML {label} limit exceeded ({limit})"));
        }
        Ok(())
    }

    fn input(&mut self, amount: usize) -> Result<(), String> {
        Self::charge(&mut self.input_bytes, amount, self.limits.input_bytes, "input byte")
    }

    fn event(&mut self) -> Result<(), String> {
        Self::charge(&mut self.events, 1, self.limits.events, "event")
    }

    fn node(&mut self) -> Result<(), String> {
        Self::charge(&mut self.nodes, 1, self.limits.nodes, "node")?;
        self.structure(std::mem::size_of::<Node>())
    }

    fn element(&mut self) -> Result<(), String> {
        self.node()?;
        self.structure(std::mem::size_of::<El>())
    }

    fn attribute(&mut self) -> Result<(), String> {
        Self::charge(&mut self.attributes, 1, self.limits.attributes, "attribute")?;
        self.structure(std::mem::size_of::<(String, String)>())
    }

    fn text(&mut self, amount: usize) -> Result<(), String> {
        Self::charge(&mut self.text_bytes, amount, self.limits.text_bytes, "text byte")
    }

    fn allocation(&mut self, amount: usize) -> Result<(), String> {
        Self::charge(&mut self.allocated_bytes, amount, self.limits.allocated_bytes, "allocation byte")
    }

    fn structure(&mut self, amount: usize) -> Result<(), String> {
        Self::charge(&mut self.structural_bytes, amount, self.limits.structural_bytes, "structural byte")
    }

    fn ensure_allocation(&self, amount: usize) -> Result<(), String> {
        let total = self.allocated_bytes.checked_add(amount).ok_or("XML allocation byte count overflow")?;
        if total > self.limits.allocated_bytes {
            return Err(format!("XML allocation byte limit exceeded ({})", self.limits.allocated_bytes));
        }
        Ok(())
    }

    #[cfg(test)]
    fn with_limits(limits: XmlLimits) -> Self {
        Self { limits, input_bytes: 0, events: 0, nodes: 0, attributes: 0, text_bytes: 0, allocated_bytes: 0, structural_bytes: 0 }
    }
}

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
        self.prop_el(k).map(|e| e.text_content())
    }
    pub fn prop_el(&self, k: &str) -> Option<&El> {
        self.find("Properties").and_then(|p| p.find(k))
    }
    pub fn num(&self, k: &str) -> Option<f64> {
        self.prop(k).and_then(|v| v.trim().parse().ok())
    }
    pub fn boolean(&self, k: &str) -> Option<bool> {
        match self.prop(k)?.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    // ---------- writing ----------

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

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push('\t');
    }
}

/// XML-escape text. Control characters that XML 1.0 forbids are dropped; tab/CR/LF in
/// attributes become character references so they survive attribute-value normalisation.
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
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    root.write(&mut s, 0);
    s.into_bytes()
}

/// Serialize with extra processing instructions after the declaration (designmap's `<?aid …?>`).
pub fn document_with_pi(root: &El, pi: &str) -> Vec<u8> {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    s.push_str(pi);
    s.push('\n');
    root.write(&mut s, 0);
    s.into_bytes()
}

/// Parse a document into its root element.
#[cfg(test)]
pub fn parse(bytes: &[u8]) -> Result<El, String> {
    let mut budget = XmlBudget::default();
    parse_with_budget(bytes, &mut budget)
}

/// Parse one XML part, charging it to the import-wide budget.
pub(crate) fn parse_with_budget(bytes: &[u8], budget: &mut XmlBudget) -> Result<El, String> {
    budget.input(bytes.len())?;
    budget.structure(std::mem::size_of::<El>())?;
    let mut r = quick_xml::Reader::from_reader(bytes);
    r.config_mut().trim_text(false);
    let mut stack: Vec<El> = vec![El::new("#document")];
    let mut buf = Vec::new();
    let pos = |r: &quick_xml::Reader<&[u8]>| r.buffer_position();
    loop {
        let ev = r.read_event_into(&mut buf).map_err(|e| format!("XML error at {}: {e}", pos(&r)))?;
        budget.event()?;
        match ev {
            Event::Start(e) => {
                if stack.len() > MAX_XML_DEPTH {
                    return Err(format!("XML nesting depth limit exceeded ({MAX_XML_DEPTH})"));
                }
                budget.element()?;
                let el = start_el(&e, budget)?;
                stack.push(el);
            }
            Event::Empty(e) => {
                if stack.len() > MAX_XML_DEPTH {
                    return Err(format!("XML nesting depth limit exceeded ({MAX_XML_DEPTH})"));
                }
                budget.element()?;
                let el = start_el(&e, budget)?;
                stack.last_mut().ok_or("unbalanced")?.push(el);
            }
            Event::End(_) => {
                let el = stack.pop().ok_or("unbalanced end tag")?;
                if stack.is_empty() {
                    return Err("unbalanced end tag".into());
                }
                stack.last_mut().ok_or("unbalanced")?.push(el);
            }
            Event::Text(t) => {
                budget.ensure_allocation(t.len().checked_mul(3).ok_or("XML allocation byte count overflow")?)?;
                let s = t.decode().map_err(|e| e.to_string())?;
                push_text(stack.last_mut().ok_or("unbalanced")?, &s, budget)?;
            }
            Event::CData(t) => {
                budget.ensure_allocation(t.len().checked_mul(3).ok_or("XML allocation byte count overflow")?)?;
                let s = String::from_utf8_lossy(&t).to_string();
                push_text(stack.last_mut().ok_or("unbalanced")?, &s, budget)?;
            }
            Event::GeneralRef(g) => {
                let s = match g.resolve_char_ref().map_err(|e| e.to_string())? {
                    Some(c) => c.to_string(),
                    None => match g.decode().map_err(|e| e.to_string())?.as_ref() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        _ => String::new(),
                    },
                };
                push_text(stack.last_mut().ok_or("unbalanced")?, &s, budget)?;
            }
            Event::PI(p) => {
                let allocation = p.len().checked_mul(3).ok_or("XML allocation byte count overflow")?;
                budget.ensure_allocation(allocation)?;
                let target = String::from_utf8_lossy(p.target()).to_string();
                let content = String::from_utf8_lossy(p.content()).trim().to_string();
                if let Some(top) = stack.last_mut() {
                    budget.node()?;
                    budget.text(content.len())?;
                    budget.allocation(target.len().checked_add(content.len()).ok_or("XML allocation byte count overflow")?)?;
                    top.children.push(Node::Pi(target, content));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if stack.len() != 1 {
        return Err("unexpected end of document".into());
    }
    let doc = stack.pop().unwrap_or_default();
    doc.children
        .into_iter()
        .find_map(|n| match n {
            Node::El(e) => Some(e),
            _ => None,
        })
        .ok_or_else(|| "empty document".into())
}

fn push_text(el: &mut El, s: &str, budget: &mut XmlBudget) -> Result<(), String> {
    if s.is_empty() {
        return Ok(());
    }
    budget.node()?;
    budget.text(s.len())?;
    budget.allocation(s.len())?;
    if let Some(Node::Text(t)) = el.children.last_mut() {
        t.push_str(s);
    } else {
        el.children.push(Node::Text(s.to_string()));
    }
    Ok(())
}

fn start_el(e: &quick_xml::events::BytesStart, budget: &mut XmlBudget) -> Result<El, String> {
    budget.ensure_allocation(e.len().checked_mul(3).ok_or("XML allocation byte count overflow")?)?;
    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
    budget.allocation(name.len())?;
    let mut el = El { name, attrs: Vec::new(), children: Vec::new() };
    for (i, a) in e.attributes().with_checks(false).enumerate() {
        if i >= MAX_ATTRIBUTES_PER_ELEMENT {
            return Err(format!("XML attribute limit exceeded ({MAX_ATTRIBUTES_PER_ELEMENT})"));
        }
        let a = a.map_err(|e| e.to_string())?;
        budget.attribute()?;
        let k = String::from_utf8_lossy(a.key.as_ref()).to_string();
        let v = a.unescape_value().map(|v| v.to_string()).unwrap_or_else(|_| String::from_utf8_lossy(&a.value).to_string());
        budget.allocation(k.len().checked_add(v.len()).ok_or("XML allocation byte count overflow")?)?;
        el.attrs.push((k, v));
    }
    Ok(el)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn rejects_excessive_nesting() {
        let mut xml = "<R>".repeat(MAX_XML_DEPTH + 1);
        xml.push_str(&"</R>".repeat(MAX_XML_DEPTH + 1));
        assert!(parse(xml.as_bytes()).is_err_and(|e| e.contains("nesting depth limit")));
    }

    #[test]
    fn shares_budget_across_parse_calls() {
        let limits =
            XmlLimits { input_bytes: 64, events: 20, nodes: 20, attributes: 20, text_bytes: 5, allocated_bytes: 64, structural_bytes: 4_096 };
        let mut budget = XmlBudget::with_limits(limits);
        assert!(parse_with_budget(b"<R>abc</R>", &mut budget).is_ok());
        assert!(parse_with_budget(b"<R>de</R>", &mut budget).is_ok());
        assert!(parse_with_budget(b"<R>f</R>", &mut budget).is_err_and(|e| e.contains("text byte limit")));
    }

    #[test]
    fn charges_attributes_and_dom_structure() {
        let limits =
            XmlLimits { input_bytes: 64, events: 20, nodes: 20, attributes: 1, text_bytes: 64, allocated_bytes: 64, structural_bytes: 4_096 };
        let mut budget = XmlBudget::with_limits(limits);
        assert!(parse_with_budget(br#"<R A="1" B="2"/>"#, &mut budget).is_err_and(|e| e.contains("attribute limit")));

        let limits = XmlLimits { attributes: 20, structural_bytes: 1, ..limits };
        let mut budget = XmlBudget::with_limits(limits);
        assert!(parse_with_budget(b"<R/>", &mut budget).is_err_and(|e| e.contains("structural byte limit")));
    }
}
