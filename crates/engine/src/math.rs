//! Math expressions: a LaTeX subset typeset into boxes and written as SVG (text in the bundled
//! serif, rules and radicals as paths). Covers fractions, super/subscripts, square and n-th roots,
//! big operators with limits, Greek letters, common symbols, `\text{}`, `\mathrm{}`, spacing
//! commands and `\left( … \right)`. `cases`, `aligned` (`align`, `align*`, `gathered`, `split`)
//! and `array` environments set as grids: rows split at `\\`, cells at `&`; matrix environments
//! (`matrix`, `pmatrix`, `bmatrix`, `Bmatrix`, `vmatrix`, `Vmatrix`, `smallmatrix`) centre their
//! columns between drawn delimiters.

use std::fmt::Write as _;

use designcraft_fonts::{FontDb, FontFace};

/// A positioned piece of the formula (baseline-relative: y grows downwards).
#[derive(Clone, Debug, PartialEq)]
enum Elem {
    Text {
        x: f64,
        y: f64,
        s: String,
        size: f64,
        italic: bool,
    },
    Rule {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// A polyline (radical sign), stroke width `w`.
    Line {
        pts: Vec<(f64, f64)>,
        w: f64,
    },
}

/// A laid-out box: width, height above and depth below the baseline.
#[derive(Clone, Debug, Default)]
struct MBox {
    w: f64,
    asc: f64,
    desc: f64,
    els: Vec<Elem>,
}

impl MBox {
    fn shifted(mut self, dx: f64, dy: f64) -> Vec<Elem> {
        for e in &mut self.els {
            match e {
                Elem::Text { x, y, .. } | Elem::Rule { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                Elem::Line { pts, .. } => {
                    for p in pts {
                        p.0 += dx;
                        p.1 += dy;
                    }
                }
            }
        }
        self.els
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    /// A character run: identifiers italic, numbers and symbols upright.
    Sym(String, Class),
    Group(Vec<Node>),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Option<Box<Node>>, Box<Node>),
    Scripts {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    Text(String),
    Space(f64),
    /// A `\begin{env} … \end{env}` grid: rows of cells.
    Grid {
        env: String,
        rows: Vec<Vec<Node>>,
    },
    /// A big operator (∑ ∏ ∫): drawn larger, limits as scripts.
    Big(String),
    /// `\left( … \right)`: delimiters sized to the content.
    Fenced(String, Box<Node>, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Class {
    Ident,
    Number,
    /// Binary operator (+ − × ·): medium space both sides.
    Bin,
    /// Relation (= < ≤ →): thick space both sides.
    Rel,
    Punct,
    Plain,
}

fn symbol(name: &str) -> Option<(&'static str, Class)> {
    use Class::*;
    Some(match name {
        "alpha" => ("α", Ident),
        "beta" => ("β", Ident),
        "gamma" => ("γ", Ident),
        "delta" => ("δ", Ident),
        "epsilon" | "varepsilon" => ("ε", Ident),
        "zeta" => ("ζ", Ident),
        "eta" => ("η", Ident),
        "theta" => ("θ", Ident),
        "iota" => ("ι", Ident),
        "kappa" => ("κ", Ident),
        "lambda" => ("λ", Ident),
        "mu" => ("μ", Ident),
        "nu" => ("ν", Ident),
        "xi" => ("ξ", Ident),
        "pi" => ("π", Ident),
        "rho" => ("ρ", Ident),
        "sigma" => ("σ", Ident),
        "tau" => ("τ", Ident),
        "phi" | "varphi" => ("φ", Ident),
        "chi" => ("χ", Ident),
        "psi" => ("ψ", Ident),
        "omega" => ("ω", Ident),
        "Gamma" => ("Γ", Plain),
        "Delta" => ("Δ", Plain),
        "Theta" => ("Θ", Plain),
        "Lambda" => ("Λ", Plain),
        "Pi" => ("Π", Plain),
        "Sigma" => ("Σ", Plain),
        "Phi" => ("Φ", Plain),
        "Psi" => ("Ψ", Plain),
        "Omega" => ("Ω", Plain),
        "times" => ("×", Bin),
        "cdot" => ("·", Bin),
        "pm" => ("±", Bin),
        "mp" => ("∓", Bin),
        "div" => ("÷", Bin),
        "le" | "leq" => ("≤", Rel),
        "ge" | "geq" => ("≥", Rel),
        "neq" | "ne" => ("≠", Rel),
        "approx" => ("≈", Rel),
        "equiv" => ("≡", Rel),
        "to" | "rightarrow" => ("→", Rel),
        "leftarrow" => ("←", Rel),
        "Rightarrow" => ("⇒", Rel),
        "in" => ("∈", Rel),
        "infty" => ("∞", Plain),
        "partial" => ("∂", Plain),
        "nabla" => ("∇", Plain),
        "ldots" | "dots" => ("…", Plain),
        "cdots" => ("⋯", Plain),
        "prime" => ("′", Plain),
        "degree" => ("°", Plain),
        _ => return None,
    })
}

fn big_op(name: &str) -> Option<&'static str> {
    Some(match name {
        "sum" => "∑",
        "prod" => "∏",
        "int" => "∫",
        "oint" => "∮",
        _ => return None,
    })
}

/// Deepest expression nesting parsed. Recursive descent must not overflow the stack on hostile
/// input (`\frac\frac…`, `{{{{…`, `\left(\left(…`); past the cap groups stop descending.
const MAX_NESTING: usize = 256;

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    /// Enclosing group depth, at most [`MAX_NESTING`].
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }
    fn command(&mut self) -> String {
        let mut name = String::new();
        if let Some(c) = self.peek()
            && !c.is_ascii_alphabetic()
        {
            self.i += 1;
            return c.to_string();
        }
        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
            name.push(c);
            self.i += 1;
        }
        name
    }
    /// Raw text of a `{…}` argument.
    fn raw_group(&mut self) -> String {
        self.skip_ws();
        if self.peek() != Some('{') {
            return self
                .peek()
                .map(|c| {
                    self.i += 1;
                    c.to_string()
                })
                .unwrap_or_default();
        }
        self.i += 1;
        let mut depth = 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            out.push(c);
        }
        out
    }
    fn arg(&mut self) -> Node {
        if self.depth >= MAX_NESTING {
            return Node::Group(Vec::new());
        }
        self.depth += 1;
        self.skip_ws();
        let n = if self.peek() == Some('{') {
            self.i += 1;
            Node::Group(self.list(Some('}')))
        } else {
            self.atom().unwrap_or(Node::Group(vec![]))
        };
        self.depth -= 1;
        n
    }
    /// Raw source up to the matching `\end{env}`, nested environments kept whole; the rest of
    /// the input when it never closes.
    fn env_body(&mut self, env: &str) -> String {
        let open: Vec<char> = "\\begin{".chars().collect();
        let close: Vec<char> = format!("\\end{{{env}}}").chars().collect();
        let close_any: Vec<char> = "\\end{".chars().collect();
        let mut depth = 0usize;
        let start = self.i;
        while let Some(rest) = self.s.get(self.i..).filter(|r| !r.is_empty()) {
            if rest.starts_with(&open) {
                depth += 1;
            } else if depth == 0 && rest.starts_with(&close) {
                let body: String = self.s.get(start..self.i).unwrap_or_default().iter().collect();
                self.i += close.len();
                return body;
            } else if depth > 0 && rest.starts_with(&close_any) {
                depth -= 1;
            }
            self.i += 1;
        }
        self.s.get(start..).unwrap_or_default().iter().collect()
    }
    fn delimiter(&mut self) -> String {
        self.skip_ws();
        match self.peek() {
            Some('\\') => {
                self.i += 1;
                match self.command().as_str() {
                    "{" => "{".into(),
                    "}" => "}".into(),
                    "|" => "‖".into(),
                    "langle" => "⟨".into(),
                    "rangle" => "⟩".into(),
                    _ => String::new(),
                }
            }
            Some('.') => {
                self.i += 1;
                String::new()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }
    fn atom(&mut self) -> Option<Node> {
        self.skip_ws();
        let c = self.peek()?;
        self.i += 1;
        Some(match c {
            '{' => Node::Group(self.list(Some('}'))),
            '\\' => {
                let name = self.command();
                match name.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        let a = self.arg();
                        let b = self.arg();
                        Node::Frac(Box::new(a), Box::new(b))
                    }
                    "sqrt" => {
                        self.skip_ws();
                        let idx = (self.peek() == Some('[')).then(|| {
                            self.i += 1;
                            let mut s = String::new();
                            while let Some(c) = self.peek() {
                                self.i += 1;
                                if c == ']' {
                                    break;
                                }
                                s.push(c);
                            }
                            Box::new(parse_at(&s, self.depth + 1))
                        });
                        let a = self.arg();
                        Node::Sqrt(idx, Box::new(a))
                    }
                    "text" | "mathrm" | "textrm" | "operatorname" => Node::Text(self.raw_group()),
                    "begin" => {
                        let env = self.raw_group();
                        // `array`'s column spec (`{lcr}`) sets nothing here.
                        if env == "array" {
                            let _ = self.raw_group();
                        }
                        let body = self.env_body(&env);
                        if self.depth >= MAX_NESTING {
                            return Some(Node::Group(Vec::new()));
                        }
                        // Each open environment keeps its own copy of the rest of the input while
                        // its cells are parsed, so a grid uses up a 16th of the nesting budget.
                        let depth = self.depth + MAX_NESTING / 16;
                        let align = matches!(env.as_str(), "aligned" | "align" | "align*" | "split");
                        let rows = split_grid(&body)
                            .into_iter()
                            .map(|r| {
                                r.into_iter()
                                    .enumerate()
                                    // As in amsmath, the right cell of an aligned pair starts with an
                                    // empty `{}`, so a leading relation keeps its space on the left.
                                    .map(|(k, c)| if align && k % 2 == 1 { parse_at(&format!("{{}}{c}"), depth) } else { parse_at(&c, depth) })
                                    .collect()
                            })
                            .collect();
                        Node::Grid { env, rows }
                    }
                    // A stray `\end{…}` (its `\begin` is missing) sets nothing.
                    "end" => {
                        let _ = self.raw_group();
                        Node::Space(0.0)
                    }
                    "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "lim" | "max" | "min" => Node::Text(name),
                    "," => Node::Space(3.0 / 18.0),
                    ":" | ">" => Node::Space(4.0 / 18.0),
                    ";" => Node::Space(5.0 / 18.0),
                    "quad" => Node::Space(1.0),
                    "qquad" => Node::Space(2.0),
                    "!" => Node::Space(-3.0 / 18.0),
                    " " => Node::Space(0.25),
                    "left" => {
                        let open = self.delimiter();
                        let body = self.list_until_right();
                        let close = self.delimiter();
                        Node::Fenced(open, Box::new(Node::Group(body)), close)
                    }
                    "{" => Node::Sym("{".into(), Class::Punct),
                    "}" => Node::Sym("}".into(), Class::Punct),
                    "%" | "$" | "#" | "&" | "_" => Node::Sym(name, Class::Plain),
                    n => match (big_op(n), symbol(n)) {
                        (Some(op), _) => Node::Big(op.into()),
                        (None, Some((s, cl))) => Node::Sym(s.into(), cl),
                        _ => Node::Text(format!("\\{n}")),
                    },
                }
            }
            c if c.is_ascii_digit() || c == '.' => {
                let mut s = c.to_string();
                while let Some(d) = self.peek().filter(|d| d.is_ascii_digit() || *d == '.') {
                    s.push(d);
                    self.i += 1;
                }
                Node::Sym(s, Class::Number)
            }
            c if c.is_alphabetic() => Node::Sym(c.to_string(), Class::Ident),
            '+' => Node::Sym("+".into(), Class::Bin),
            '-' => Node::Sym("−".into(), Class::Bin),
            '*' => Node::Sym("∗".into(), Class::Bin),
            '=' | '<' | '>' => Node::Sym(c.to_string(), Class::Rel),
            ',' | ';' => Node::Sym(c.to_string(), Class::Punct),
            '\'' => Node::Sym("′".into(), Class::Plain),
            c => Node::Sym(c.to_string(), Class::Plain),
        })
    }
    fn list(&mut self, end: Option<char>) -> Vec<Node> {
        if self.depth >= MAX_NESTING {
            return Vec::new();
        }
        self.depth += 1;
        let mut out: Vec<Node> = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => break,
                Some(c) if Some(c) == end => {
                    self.i += 1;
                    break;
                }
                Some('^') | Some('_') => {
                    let up = self.peek() == Some('^');
                    self.i += 1;
                    let a = self.arg();
                    let base = out.pop().unwrap_or(Node::Group(vec![]));
                    let node = match base {
                        Node::Scripts { base, sup, sub } => {
                            if up {
                                Node::Scripts { base, sup: Some(Box::new(a)), sub }
                            } else {
                                Node::Scripts { base, sup, sub: Some(Box::new(a)) }
                            }
                        }
                        b => Node::Scripts { base: Box::new(b), sup: up.then(|| Box::new(a.clone())), sub: (!up).then(|| Box::new(a)) },
                    };
                    out.push(node);
                }
                Some(_) => {
                    if let Some(a) = self.atom() {
                        out.push(a);
                    }
                }
            }
        }
        self.depth -= 1;
        out
    }
    fn list_until_right(&mut self) -> Vec<Node> {
        if self.depth >= MAX_NESTING {
            return Vec::new();
        }
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                break;
            }
            if self.s[self.i..].starts_with(&['\\', 'r', 'i', 'g', 'h', 't']) && !self.s.get(self.i + 6).is_some_and(|c| c.is_ascii_alphabetic()) {
                self.i += 6;
                break;
            }
            let mut p = Parser { s: self.s, i: self.i, depth: self.depth + 1 };
            let one = p.list_one();
            self.i = p.i;
            out.extend(one);
        }
        out
    }
    /// One atom plus any scripts on it.
    fn list_one(&mut self) -> Vec<Node> {
        let mut out = Vec::new();
        if let Some(a) = self.atom() {
            out.push(a);
        }
        loop {
            self.skip_ws();
            match self.peek() {
                Some('^') | Some('_') => {
                    let up = self.peek() == Some('^');
                    self.i += 1;
                    let a = self.arg();
                    let base = out.pop().unwrap_or(Node::Group(vec![]));
                    out.push(match base {
                        Node::Scripts { base, sub, .. } if up => Node::Scripts { base, sup: Some(Box::new(a)), sub },
                        Node::Scripts { base, sup, .. } => Node::Scripts { base, sup, sub: Some(Box::new(a)) },
                        b => Node::Scripts { base: Box::new(b), sup: up.then(|| Box::new(a.clone())), sub: (!up).then(|| Box::new(a)) },
                    });
                }
                _ => break,
            }
        }
        out
    }
}

fn parse(src: &str) -> Node {
    parse_at(src, 0)
}

fn parse_at(src: &str, depth: usize) -> Node {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { s: &chars, i: 0, depth };
    Node::Group(p.list(None))
}

/// The delimiters of a matrix environment (none for `matrix` and `smallmatrix`); `None` for
/// any other environment.
fn matrix_delimiters(env: &str) -> Option<(&'static str, &'static str)> {
    Some(match env {
        "matrix" | "smallmatrix" => ("", ""),
        "pmatrix" => ("(", ")"),
        "bmatrix" => ("[", "]"),
        "Bmatrix" => ("{", "}"),
        "vmatrix" => ("|", "|"),
        "Vmatrix" => ("‖", "‖"),
        _ => return None,
    })
}

/// Split an environment body into rows (`\\`) and cells (`&`) outside braces and nested
/// environments. A trailing `\\` adds no empty row.
fn split_grid(body: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = body.chars().collect();
    let (mut rows, mut row, mut cell) = (Vec::new(), Vec::new(), String::new());
    let (mut depth, mut env, mut i) = (0i32, 0i32, 0usize);
    let [row_end, begin, end]: [Vec<char>; 3] = ["\\\\", "\\begin{", "\\end{"].map(|w| w.chars().collect());
    let starts = |i: usize, w: &[char]| chars.get(i..).is_some_and(|r| r.starts_with(w));
    let mut prev = None;
    while let Some(&c) = chars.get(i) {
        let top = depth == 0 && env == 0;
        if top && c == '&' && prev != Some('\\') {
            row.push(std::mem::take(&mut cell));
            (i, prev) = (i + 1, None);
            continue;
        }
        if top && starts(i, &row_end) {
            row.push(std::mem::take(&mut cell));
            rows.push(std::mem::take(&mut row));
            (i, prev) = (i + 2, None);
            continue;
        }
        if c == '{' && prev != Some('\\') {
            depth += 1;
        } else if c == '}' && prev != Some('\\') {
            // A stray closer is ignored, as `env_body` ignores it, so later cells still split.
            depth = (depth - 1).max(0);
        } else if starts(i, &begin) {
            env += 1;
        } else if starts(i, &end) {
            env = (env - 1).max(0);
        }
        cell.push(c);
        // `\\` is spent: a backslash after it does not escape the next character.
        prev = if prev == Some('\\') && c == '\\' { None } else { Some(c) };
        i += 1;
    }
    if !cell.trim().is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// Typesetting fonts and metrics.
struct Fonts {
    upright: std::sync::Arc<FontFace>,
    italic: std::sync::Arc<FontFace>,
    fallback: std::sync::Arc<FontFace>,
}

impl Fonts {
    fn face(&self, c: char, italic: bool) -> &FontFace {
        let f = if italic { &self.italic } else { &self.upright };
        if f.glyph_for(c) != 0 { f } else { &self.fallback }
    }
    fn width(&self, s: &str, size: f64, italic: bool) -> f64 {
        s.chars()
            .map(|c| {
                let f = self.face(c, italic);
                f.advance(f.glyph_for(c)) / f.units_per_em().max(1.0) * size
            })
            .sum()
    }
}

const FAMILY: &str = designcraft_fonts::DEFAULT_FAMILY;
const FALLBACK: &str = "Source Sans 3";

fn text_box(f: &Fonts, s: &str, size: f64, italic: bool) -> MBox {
    let w = f.width(s, size, italic);
    MBox { w, asc: size * 0.72, desc: size * 0.22, els: vec![Elem::Text { x: 0.0, y: 0.0, s: s.to_string(), size, italic }] }
}

fn hbox(parts: Vec<(MBox, f64)>) -> MBox {
    // (box, space before) laid out along the baseline.
    let mut out = MBox::default();
    let mut x = 0.0;
    for (b, before) in parts {
        x += before;
        out.asc = out.asc.max(b.asc);
        out.desc = out.desc.max(b.desc);
        let w = b.w;
        out.els.extend(b.shifted(x, 0.0));
        x += w;
    }
    out.w = x.max(0.0);
    out
}

fn layout(f: &Fonts, n: &Node, size: f64) -> MBox {
    let axis = size * 0.25;
    match n {
        Node::Sym(s, cl) => text_box(f, s, size, *cl == Class::Ident),
        Node::Text(s) => text_box(f, s, size, false),
        Node::Space(em) => MBox { w: em * size, ..Default::default() },
        // `smallmatrix` sets its cells at script size, still centred on the text's math axis.
        Node::Grid { env, rows } if env == "smallmatrix" => {
            let s2 = size * 0.7;
            let b = grid(f, env, rows, s2);
            let dy = -(size - s2) * 0.25;
            let (w, asc, desc) = (b.w, b.asc - dy, b.desc + dy);
            MBox { w, asc, desc, els: b.shifted(0.0, dy) }
        }
        Node::Grid { env, rows } => grid(f, env, rows, size),
        Node::Group(list) => {
            let mut parts = Vec::new();
            let mut prev: Option<Class> = None;
            for (k, item) in list.iter().enumerate() {
                let cl = match item {
                    Node::Sym(_, c) => Some(*c),
                    Node::Scripts { base, .. } => match &**base {
                        Node::Sym(_, c) => Some(*c),
                        _ => None,
                    },
                    _ => None,
                };
                // A leading + or − (or after another operator) is a sign, not a binary operator.
                let cl = if cl == Some(Class::Bin) && (k == 0 || matches!(prev, Some(Class::Bin | Class::Rel | Class::Punct))) {
                    Some(Class::Plain)
                } else {
                    cl
                };
                let gap = |c: Option<Class>| match c {
                    Some(Class::Bin) => size * 4.0 / 18.0,
                    Some(Class::Rel) => size * 5.0 / 18.0,
                    _ => 0.0,
                };
                let mut before = gap(cl).max(gap(prev));
                if k == 0 {
                    before = 0.0;
                }
                if prev == Some(Class::Punct) {
                    before = before.max(size * 3.0 / 18.0);
                }
                parts.push((layout(f, item, size), before));
                prev = cl;
            }
            hbox(parts)
        }
        Node::Frac(a, b) => {
            let s2 = size * 0.85;
            let (na, nb) = (layout(f, a, s2), layout(f, b, s2));
            let w = na.w.max(nb.w) + size * 0.2;
            let t = (size * 0.06).max(0.4);
            let gap = size * 0.15;
            let num_base = -(axis + t / 2.0 + gap + na.desc);
            let den_base = -axis + t / 2.0 + gap + nb.asc;
            let mut out = MBox { w, asc: -num_base + na.asc, desc: den_base + nb.desc, els: Vec::new() };
            out.els.push(Elem::Rule { x: 0.0, y: -axis - t / 2.0, w, h: t });
            let (wa, wb) = (na.w, nb.w);
            out.els.extend(na.shifted((w - wa) / 2.0, num_base));
            out.els.extend(nb.shifted((w - wb) / 2.0, den_base));
            out
        }
        Node::Sqrt(idx, a) => {
            let inner = layout(f, a, size);
            let t = (size * 0.05).max(0.4);
            let gap = size * 0.12;
            let top = inner.asc + gap + t;
            let bottom = inner.desc;
            let hook = size * 0.55;
            let mut out = MBox { w: hook + inner.w + size * 0.1, asc: top, desc: bottom, els: Vec::new() };
            // The radical: a short rise, the long stroke down, then up to the bar.
            out.els.push(Elem::Line {
                pts: vec![(0.0, -axis * 0.6), (hook * 0.25, -axis), (hook * 0.55, bottom), (hook, -top + t / 2.0), (out.w, -top + t / 2.0)],
                w: t,
            });
            let iw = inner.w;
            let _ = iw;
            out.els.extend(inner.shifted(hook, 0.0));
            if let Some(i) = idx {
                let ib = layout(f, i, size * 0.55);
                let (ia, iw2) = (ib.asc, ib.w);
                out.els.extend(ib.shifted((hook * 0.3 - iw2).max(0.0) - 0.0, -axis - size * 0.1));
                out.asc = out.asc.max(axis + size * 0.1 + ia);
            }
            out
        }
        Node::Big(op) => {
            let mut b = text_box(f, op, size * 1.5, false);
            b.asc = size * 1.05;
            b.desc = size * 0.35;
            for e in &mut b.els {
                if let Elem::Text { y, .. } = e {
                    // Centre the glyph on the math axis.
                    *y = size * 0.32;
                }
            }
            b
        }
        Node::Scripts { base, sup, sub } => {
            let big = matches!(&**base, Node::Big(_));
            let b = layout(f, base, size);
            let s2 = size * 0.7;
            let (bw, basc, bdesc) = (b.w, b.asc, b.desc);
            let mut out = MBox { w: bw, asc: basc, desc: bdesc, els: b.els };
            if big {
                // Limits above and below.
                let mut w = bw;
                if let Some(sp) = sup {
                    let sb = layout(f, sp, s2);
                    let y = -basc - size * 0.1 - sb.desc;
                    w = w.max(sb.w);
                    let (sw, sa) = (sb.w, sb.asc);
                    out.els.extend(sb.shifted((bw - sw) / 2.0, y));
                    out.asc = out.asc.max(-y + sa);
                }
                if let Some(sbn) = sub {
                    let sb = layout(f, sbn, s2);
                    let y = bdesc + size * 0.1 + sb.asc;
                    w = w.max(sb.w);
                    let (sw, sd) = (sb.w, sb.desc);
                    out.els.extend(sb.shifted((bw - sw) / 2.0, y));
                    out.desc = out.desc.max(y + sd);
                }
                // Centre the operator over the widest limit.
                let dx = (w - bw) / 2.0;
                let els = std::mem::take(&mut out.els);
                out = MBox { w, asc: out.asc, desc: out.desc, els: MBox { els, ..Default::default() }.shifted(dx, 0.0) };
                return out;
            }
            let mut w = bw;
            if let Some(sp) = sup {
                let sb = layout(f, sp, s2);
                let y = -(size * 0.42).max(basc - sb.asc * 0.6);
                w = w.max(bw + sb.w + size * 0.05);
                let sa = sb.asc;
                out.els.extend(sb.shifted(bw + size * 0.05, y));
                out.asc = out.asc.max(-y + sa);
            }
            if let Some(sbn) = sub {
                let sb = layout(f, sbn, s2);
                let y = (size * 0.2).max(bdesc * 0.8);
                w = w.max(bw + sb.w + size * 0.05);
                let sd = sb.desc;
                out.els.extend(sb.shifted(bw + size * 0.05, y));
                out.desc = out.desc.max(y + sd);
            }
            out.w = w;
            out
        }
        Node::Fenced(open, body, close) => {
            let inner = layout(f, body, size);
            let h = (inner.asc + inner.desc).max(size);
            let k = h / size;
            let ds = size * k.max(1.0);
            let mk = |s: &str| -> MBox {
                if s.is_empty() {
                    return MBox::default();
                }
                let mut b = text_box(f, s, ds, false);
                // Scaled delimiters centred on the content.
                let mid = (inner.desc - inner.asc) / 2.0;
                for e in &mut b.els {
                    if let Elem::Text { y, .. } = e {
                        *y = mid + ds * 0.25;
                    }
                }
                b.asc = inner.asc.max(size * 0.72);
                b.desc = inner.desc.max(size * 0.22);
                b
            };
            let (l, r) = (mk(open), mk(close));
            hbox(vec![(l, 0.0), (inner, size * 0.05), (r, size * 0.05)])
        }
    }
}

/// An environment grid: columns as wide as their widest cell, rows on the text pitch, centred
/// on the math axis. `aligned`-like environments alternate right- and left-aligned columns with
/// no gap between a pair (`a &= b`); `cases` gets a drawn left brace spanning the rows; matrix
/// environments centre their columns.
fn grid(f: &Fonts, env: &str, rows: &[Vec<Node>], size: f64) -> MBox {
    let cells: Vec<Vec<MBox>> = rows.iter().map(|r| r.iter().map(|c| layout(f, c, size)).collect()).collect();
    let ncol = cells.iter().map(Vec::len).max().unwrap_or(0);
    let mut colw = vec![0.0f64; ncol];
    for r in &cells {
        for (w, c) in colw.iter_mut().zip(r) {
            *w = w.max(c.w);
        }
    }
    let align = matches!(env, "aligned" | "align" | "align*" | "split");
    let centred = env == "gathered" || matrix_delimiters(env).is_some();
    let gaps: Vec<f64> = (0..ncol)
        .map(|k| match k {
            0 => 0.0,
            _ if align && k % 2 == 1 => 0.0,
            _ if align => size * 2.0,
            _ => size,
        })
        .collect();
    let row_gap = size * 0.35;
    let (mut y, mut first_asc, mut prev_desc) = (0.0f64, 0.0f64, 0.0f64);
    let mut placed: Vec<(MBox, f64, f64)> = Vec::new();
    for (ri, r) in cells.into_iter().enumerate() {
        let asc = r.iter().map(|c| c.asc).fold(size * 0.72, f64::max);
        let desc = r.iter().map(|c| c.desc).fold(size * 0.22, f64::max);
        if ri == 0 {
            first_asc = asc;
        } else {
            y += prev_desc + row_gap + asc;
        }
        let mut x = 0.0;
        for (k, (c, (cw, g))) in r.into_iter().zip(colw.iter().zip(&gaps)).enumerate() {
            x += g;
            let dx = match env {
                _ if align && k % 2 == 0 => cw - c.w,
                _ if centred => (cw - c.w) / 2.0,
                _ => 0.0,
            };
            placed.push((c, x + dx, y));
            x += cw;
        }
        prev_desc = desc;
    }
    let total_w: f64 = colw.iter().sum::<f64>() + gaps.iter().sum::<f64>();
    let height = first_asc + y + prev_desc;
    let top = -(height / 2.0 + size * 0.25);
    let shift = top + first_asc;
    let brace = env == "cases";
    // A matrix's delimiters are drawn to the grid's height, like the `cases` brace.
    let (open, close) = matrix_delimiters(env).unwrap_or(("", ""));
    let (margin, dw, pad) = (size * 0.06, size * 0.32, size * 0.18);
    let side = |d: &str| if d.is_empty() { 0.0 } else { margin + dw + pad };
    let bw = if brace { size * 0.8 } else { side(open) };
    let mut out = MBox::default();
    for (c, x, y) in placed {
        out.els.extend(c.shifted(bw + x, shift + y));
    }
    let ext = if open.is_empty() && close.is_empty() { 0.0 } else { size * 0.08 };
    let (t, b) = (top - ext, top + height + ext);
    let lw = (size * 0.055).max(0.5);
    out.els.extend(delimiter(open, margin, dw, t, b, lw, size));
    out.els.extend(delimiter(close, bw + total_w + pad, -dw, t, b, lw, size));
    if brace {
        let (t, b) = (top, top + height);
        let m = (t + b) / 2.0;
        let w = size * 0.4;
        let lw = (size * 0.06).max(0.5);
        out.els.push(Elem::Line { pts: vec![(w, t), (w * 0.55, t + size * 0.12), (w * 0.5, m - size * 0.15), (0.0, m)], w: lw });
        out.els.push(Elem::Line { pts: vec![(0.0, m), (w * 0.5, m + size * 0.15), (w * 0.55, b - size * 0.12), (w, b)], w: lw });
    }
    out.w = bw + total_w + side(close);
    out.asc = -t;
    out.desc = b;
    out
}

/// A drawn delimiter from `t` to `b`, `w` wide from `x` (a negative `w` mirrors it, for the
/// closing side): `(` `[` `{` `|` `‖` and their closing pairs; nothing for an empty one.
fn delimiter(d: &str, x: f64, w: f64, t: f64, b: f64, lw: f64, size: f64) -> Vec<Elem> {
    // `at(u, v)`: `u` across from the outer edge (0) to the inner (1), `v` down from `t` (0) to `b` (1).
    let at = |u: f64, v: f64| (x + if w < 0.0 { -w * (1.0 - u) } else { w * u }, t + (b - t) * v);
    let line = |pts: Vec<(f64, f64)>| Elem::Line { pts, w: lw };
    let m = (t + b) / 2.0;
    let mv = |y: f64| (y - t) / (b - t).max(f64::EPSILON);
    match d {
        "(" | ")" => vec![line((0..=16).map(|i| f64::from(i) / 16.0).map(|v| at(1.0 - 0.85 * (std::f64::consts::PI * v).sin(), v)).collect())],
        "[" | "]" => vec![line(vec![at(1.0, 0.0), at(0.25, 0.0), at(0.25, 1.0), at(1.0, 1.0)])],
        "{" | "}" => {
            let k = size * 0.12;
            vec![line(vec![
                at(1.0, 0.0),
                at(0.55, mv(t + k)),
                at(0.5, mv(m - size * 0.15)),
                at(0.0, 0.5),
                at(0.5, mv(m + size * 0.15)),
                at(0.55, mv(b - k)),
                at(1.0, 1.0),
            ])]
        }
        "|" => vec![line(vec![at(0.5, 0.0), at(0.5, 1.0)])],
        "‖" => vec![line(vec![at(0.25, 0.0), at(0.25, 1.0)]), line(vec![at(0.75, 0.0), at(0.75, 1.0)])],
        _ => Vec::new(),
    }
}

/// Typeset `latex` at `size` points: SVG bytes and the size in points.
pub fn to_svg(latex: &str, size: f64) -> (Vec<u8>, (f64, f64)) {
    let db = FontDb::global();
    let f = Fonts { upright: db.face(FAMILY, "Regular"), italic: db.face(FAMILY, "Italic"), fallback: db.face(FALLBACK, "Regular") };
    let b = layout(&f, &parse(latex), size);
    let pad = size * 0.1;
    let (w, h) = (b.w + 2.0 * pad, b.asc + b.desc + 2.0 * pad);
    let base = pad + b.asc;
    let mut svg = String::new();
    let _ = write!(svg, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.2}pt\" height=\"{h:.2}pt\" viewBox=\"0 0 {w:.3} {h:.3}\">");
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    for e in &b.els {
        match e {
            Elem::Text { x, y, s, size, italic } => {
                // Per character, so each uses a face that has it.
                let mut cx = *x;
                for c in s.chars() {
                    let face = f.face(c, *italic);
                    let fam = if std::ptr::eq(face, &*f.fallback) { FALLBACK } else { FAMILY };
                    let it = *italic && !std::ptr::eq(face, &*f.fallback);
                    let _ = write!(
                        svg,
                        "<text x=\"{:.3}\" y=\"{:.3}\" font-family=\"'{fam}'\" font-size=\"{size:.3}\"{}>{}</text>",
                        pad + cx,
                        base + y,
                        if it { " font-style=\"italic\"" } else { "" },
                        esc(&c.to_string())
                    );
                    cx += f.width(&c.to_string(), *size, *italic);
                }
            }
            Elem::Rule { x, y, w: rw, h: rh } => {
                let _ = write!(svg, "<rect x=\"{:.3}\" y=\"{:.3}\" width=\"{rw:.3}\" height=\"{rh:.3}\"/>", pad + x, base + y);
            }
            Elem::Line { pts, w: lw } => {
                let d: Vec<String> = pts.iter().map(|(x, y)| format!("{:.3},{:.3}", pad + x, base + y)).collect();
                let _ = write!(
                    svg,
                    "<polyline points=\"{}\" fill=\"none\" stroke=\"black\" stroke-width=\"{lw:.3}\" stroke-linejoin=\"round\"/>",
                    d.join(" ")
                );
            }
        }
    }
    svg.push_str("</svg>");
    (svg.into_bytes(), (w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_grid_ignores_stray_closers() {
        let cells = |body: &str| split_grid(body).iter().map(Vec::len).collect::<Vec<_>>();
        assert_eq!(cells(r"a \end{foo} & b"), vec![2]);
        assert_eq!(cells(r"a } & b \\ c & d"), vec![2, 2]);
    }

    #[test]
    fn parses_the_common_constructs() {
        let n = parse(r"\frac{a^2+b}{\sqrt[3]{x}}");
        let Node::Group(v) = n else { panic!() };
        assert!(matches!(&v[0], Node::Frac(_, _)));
        let n = parse(r"\sum_{i=1}^{n} i^2 \le \alpha");
        let Node::Group(v) = n else { panic!() };
        assert!(matches!(&v[0], Node::Scripts { base, sup: Some(_), sub: Some(_) } if matches!(**base, Node::Big(_))));
        assert!(v.iter().any(|x| *x == Node::Sym("≤".into(), Class::Rel)));
        assert!(v.iter().any(|x| *x == Node::Sym("α".into(), Class::Ident)));
        let n = parse(r"\left( \frac{1}{2} \right) + \text{if } x");
        let Node::Group(v) = n else { panic!() };
        assert!(matches!(&v[0], Node::Fenced(o, _, c) if o == "(" && c == ")"));
        assert!(v.contains(&Node::Text("if ".into())));
    }

    /// A command that merely starts with "right" (`\rightarrow`) must not close a `\left` fence.
    #[test]
    fn right_prefixed_commands_do_not_close_a_fence() {
        let n = parse(r"\left( x \rightarrow y \right)");
        let Node::Group(v) = n else { panic!() };
        assert_eq!(v.len(), 1, "{v:?}");
        let Node::Fenced(o, body, c) = &v[0] else { panic!("{v:?}") };
        assert!(o == "(" && c == ")", "{v:?}");
        let Node::Group(b) = &**body else { panic!("{v:?}") };
        assert_eq!(*b, vec![Node::Sym("x".into(), Class::Ident), Node::Sym("→".into(), Class::Rel), Node::Sym("y".into(), Class::Ident)]);
        // An unknown `\right…` command stays inside the fence too.
        let Node::Group(v) = parse(r"\left[ \rightfoo \right]") else { panic!() };
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(matches!(&v[0], Node::Fenced(o, _, c) if o == "[" && c == "]"), "{v:?}");
        // Real closers still close it.
        for (src, close) in [(r"\left( x \right)", ")"), (r"\left( x \right.", ""), (r"\left( x \right)y", ")")] {
            let Node::Group(v) = parse(src) else { panic!() };
            assert!(matches!(&v[0], Node::Fenced(_, _, c) if c == close), "{src}: {v:?}");
        }
    }

    #[test]
    fn environments_set_as_grids() {
        let grid = |src: &str| match parse(src) {
            Node::Group(v) => match v.into_iter().find(|n| matches!(n, Node::Grid { .. })) {
                Some(Node::Grid { env, rows }) => Some((env, rows.iter().map(Vec::len).collect::<Vec<_>>())),
                _ => None,
            },
            _ => None,
        };
        assert_eq!(grid(r"f(x)=\begin{cases} 1 & x<0 \\ x^2 & x\ge 0 \end{cases}"), Some(("cases".into(), vec![2, 2])));
        assert_eq!(grid(r"\begin{aligned} a &= b+c \\ &= d \\ \end{aligned}"), Some(("aligned".into(), vec![2, 2])));
        assert_eq!(grid(r"\begin{array}{lcr} a & b & c \end{array}"), Some(("array".into(), vec![3])));
        // `&` and `\\` inside braces or a nested environment belong to the cell.
        assert_eq!(
            grid(r"\begin{cases} \text{a \& b} & {x \\ y} \\ \begin{aligned} p &= q \end{aligned} & z \end{cases}"),
            Some(("cases".into(), vec![2, 2]))
        );
        // Unclosed, empty or stray: no panic, nothing leaks as raw source.
        for src in [r"\begin{cases} 1 & 2", r"\begin{cases}\end{cases}", r"\end{cases} x", r"\begin{", r"\begin{array}"] {
            let s = String::from_utf8(to_svg(src, 12.0).0).unwrap();
            assert!(!s.contains(">\\<") && !s.contains(">&amp;<"), "{src}: {s}");
        }
        // The rows stack and `cases` draws its brace.
        let (svg, (_, h)) = to_svg(r"f(x)=\begin{cases} 1 & x<0 \\ x^2 & x\ge 0 \end{cases}", 12.0);
        let s = String::from_utf8(svg).unwrap();
        assert!(h > 2.0 * to_svg("x", 12.0).1.1 && s.contains("<polyline"), "{h}");
        // `aligned` lines up the relations: both `=` start at the same x.
        let s = String::from_utf8(to_svg(r"\begin{aligned} a &= b \\ xyz &= c \end{aligned}", 12.0).0).unwrap();
        let xs: Vec<&str> = s.split("<text x=\"").skip(1).filter(|t| t.contains(">=<")).filter_map(|t| t.split('"').next()).collect();
        assert!(xs.len() == 2 && xs[0] == xs[1], "{xs:?}");
        // ... and keeps the thick space before them, as `a = b` has inline.
        let eq_x = |latex: &str| {
            let s = String::from_utf8(to_svg(latex, 12.0).0).unwrap();
            s.split("<text x=\"").skip(1).find(|t| t.contains(">=<")).and_then(|t| t.split('"').next()).and_then(|x| x.parse::<f64>().ok()).unwrap()
        };
        let (grid_x, inline_x) = (eq_x(r"\begin{aligned} a &= b \end{aligned}"), eq_x(r"a = b"));
        assert!((grid_x - inline_x).abs() < 0.01, "{grid_x} {inline_x}");
    }

    #[test]
    fn matrix_environments_set_with_delimiters() {
        let shape = |src: &str| match parse(src) {
            Node::Group(v) => match v.into_iter().next() {
                Some(Node::Grid { env, rows }) => Some((env, rows.iter().map(Vec::len).collect::<Vec<_>>())),
                _ => None,
            },
            _ => None,
        };
        let svg = |src: &str| to_svg(src, 12.0);
        let lines = |src: &str| String::from_utf8(svg(src).0).unwrap().matches("<polyline").count();
        // Each environment: a 2 × 2 grid, with its delimiters drawn (`‖` is two strokes a side).
        for (env, strokes) in [("matrix", 0), ("pmatrix", 2), ("bmatrix", 2), ("Bmatrix", 2), ("vmatrix", 2), ("Vmatrix", 4), ("smallmatrix", 0)] {
            let src = format!(r"\begin{{{env}}} a & b \\ c & d \end{{{env}}}");
            assert_eq!(shape(&src), Some((env.to_string(), vec![2, 2])), "{env}");
            assert_eq!(lines(&src), strokes, "{env}");
            let s = String::from_utf8(svg(&src).0).unwrap();
            assert!(!s.contains("begin") && !s.contains(">\\<") && !s.contains(">&amp;<"), "{env}: {s}");
        }
        // Delimiters add width and span the grid: three rows are taller than two.
        let (_, (plain_w, _)) = svg(r"\begin{matrix} a & b \\ c & d \end{matrix}");
        let (_, (paren_w, two)) = svg(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}");
        let (_, (_, three)) = svg(r"\begin{pmatrix} a \\ b \\ c \end{pmatrix}");
        assert!(paren_w > plain_w + 6.0 && three > two + 8.0, "{plain_w} {paren_w} {two} {three}");
        // `smallmatrix` sets its cells smaller.
        let (_, (small_w, small_h)) = svg(r"\begin{smallmatrix} a & b \\ c & d \end{smallmatrix}");
        let (_, (_, plain_h)) = svg(r"\begin{matrix} a & b \\ c & d \end{matrix}");
        assert!(small_w < plain_w * 0.8 && small_h < plain_h, "{small_w} {plain_w} {small_h} {plain_h}");
        // Columns are centred: a narrow entry starts right of a wide one above it (an `array`
        // column, left-aligned, starts both at the same x).
        let xs = |src: &str| -> Vec<f64> {
            let s = String::from_utf8(svg(src).0).unwrap();
            s.split("<text x=\"").skip(1).filter_map(|t| t.split('"').next()).filter_map(|x| x.parse().ok()).collect()
        };
        let m = xs(r"\begin{pmatrix} 100 \\ 1 \end{pmatrix}");
        let a = xs(r"\begin{array}{l} 100 \\ 1 \end{array}");
        // (Each glyph is its own `<text>`: the last one is the second row's `1`.)
        let ends = |v: &[f64]| (v.first().copied().unwrap_or(0.0), v.last().copied().unwrap_or(0.0));
        assert!(m.len() > 2 && ends(&m).1 > ends(&m).0 + 2.0, "{m:?}");
        assert!(a.len() > 2 && (ends(&a).1 - ends(&a).0).abs() < 1e-6, "{a:?}");
        // Unclosed, empty and nested matrices: no panic, nothing leaks as raw source.
        for src in [
            r"\begin{pmatrix} 1 & 2",
            r"\begin{bmatrix}\end{bmatrix}",
            r"\begin{vmatrix} \begin{pmatrix} a \end{pmatrix} & b \end{vmatrix}",
            r"\end{pmatrix}",
        ] {
            let s = String::from_utf8(svg(src).0).unwrap();
            assert!(!s.contains("begin") && !s.contains("end") && !s.contains(">&amp;<"), "{src}: {s}");
        }
    }

    #[test]
    fn layout_sizes_and_svg() {
        let (svg, (w, h)) = to_svg(r"E = mc^2", 12.0);
        let s = String::from_utf8(svg).unwrap();
        assert!(s.starts_with("<svg") && s.contains("font-style=\"italic\">E<") && s.contains(">2<"));
        assert!(w > 30.0 && h > 10.0, "{w} {h}");
        // A fraction is taller than its numerator alone; a big operator with limits taller still.
        let (_, (_, h1)) = to_svg("x", 12.0);
        let (_, (_, h2)) = to_svg(r"\frac{x}{y}", 12.0);
        let (_, (_, h3)) = to_svg(r"\sum_{i=0}^{n}", 12.0);
        assert!(h2 > h1 * 1.4 && h3 > h2, "{h1} {h2} {h3}");
        // The SVG parses and rasterises.
        let (svg, _) = to_svg(r"\sqrt{a^2+b^2} \ne \pi", 14.0);
        assert!(designcraft_images::svg_tree(&svg).is_some());
    }

    /// Hostile input must not overflow the stack: nesting past the cap stops descending instead
    /// of recursing once per group (100k levels used to abort the process).
    #[test]
    fn an_over_deep_expression_is_bounded() {
        for latex in [
            format!("{}x{}", "{".repeat(100_000), "}".repeat(100_000)),
            format!("{}x", r"\frac".repeat(100_000)),
            format!("{}x", r"\left(".repeat(100_000)),
            format!("{}x", r"\begin{cases}".repeat(20_000)),
            format!("{}x", r"\begin{cases}{".repeat(20_000)),
            format!("{}x", r"\begin{pmatrix}".repeat(20_000)),
            format!("{}x", r"\begin{Vmatrix} & \\ \begin{smallmatrix}".repeat(10_000)),
            format!("{}x{}", r"\begin{bmatrix}{".repeat(10_000), r"}\end{bmatrix}".repeat(10_000)),
        ] {
            let (svg, _) = to_svg(&latex, 12.0);
            assert!(!svg.is_empty());
        }
        // Grids nest at most 16 deep, which bounds the copies of the input they hold.
        fn grid_depth(n: &Node) -> usize {
            match n {
                Node::Grid { rows, .. } => 1 + rows.iter().flatten().map(grid_depth).max().unwrap_or(0),
                Node::Group(v) => v.iter().map(grid_depth).max().unwrap_or(0),
                _ => 0,
            }
        }
        let d = grid_depth(&parse(&format!("{}x", r"\begin{cases}".repeat(100))));
        assert!((2..=16).contains(&d), "{d}");
    }
}
