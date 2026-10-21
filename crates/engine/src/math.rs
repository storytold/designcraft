//! Math expressions: a LaTeX subset typeset into boxes and written as SVG (text in the bundled
//! serif, rules and radicals as paths). Covers fractions, super/subscripts, square and n-th roots,
//! big operators with limits, Greek letters, common symbols, `\text{}`, `\mathrm{}`, spacing
//! commands and `\left( … \right)`, set-builder and logic symbols, `\mathbb{}` and the usual
//! operator names. Symbols the bundled serif lacks come from an installed DejaVu Serif, then the
//! bundled sans, then the bundled monospace (which covers ℝ ∘ ∪ ⌊ ⟨ ∀ ∅ …); `⋯` `⋮` `⋱` are built
//! from the serif's own dots instead.

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
    /// An operator name (`sin`, `log`, `lim`, …), upright, followed by a thin space.
    OpName(String),
    Space(f64),
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
    /// A relation with an extra thick space each side (`\iff`, `\implies`, `\impliedby`), as
    /// TeX's `\;\Longleftrightarrow\;`.
    WideRel,
    Punct,
    Plain,
}

/// The character a symbol command (`\alpha`, `\le`, `\to`, …) sets and its spacing class;
/// `None` for a name that isn't one.
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
        "Leftarrow" => ("⇐", Rel),
        "Leftrightarrow" => ("⇔", Rel),
        "Longrightarrow" => ("⟹", Rel),
        "Longleftarrow" => ("⟸", Rel),
        "Longleftrightarrow" => ("⟺", Rel),
        "longrightarrow" => ("⟶", Rel),
        "longleftarrow" => ("⟵", Rel),
        "implies" => ("⟹", WideRel),
        "impliedby" => ("⟸", WideRel),
        "iff" => ("⟺", WideRel),
        "mapsto" => ("↦", Rel),
        "in" => ("∈", Rel),
        "notin" => ("∉", Rel),
        "mid" => ("|", Rel),
        "subset" => ("⊂", Rel),
        "subseteq" => ("⊆", Rel),
        "sim" => ("∼", Rel),
        "circ" => ("∘", Bin),
        "cap" => ("∩", Bin),
        "cup" => ("∪", Bin),
        "setminus" => ("∖", Bin),
        "ast" => ("∗", Bin),
        "emptyset" | "varnothing" => ("∅", Plain),
        "lfloor" => ("⌊", Plain),
        "rfloor" => ("⌋", Plain),
        "lceil" => ("⌈", Plain),
        "rceil" => ("⌉", Plain),
        "langle" => ("⟨", Plain),
        "rangle" => ("⟩", Plain),
        "vert" => ("|", Plain),
        "Vert" => ("‖", Plain),
        "forall" => ("∀", Plain),
        "exists" => ("∃", Plain),
        "neg" | "lnot" => ("¬", Plain),
        "angle" => ("∠", Plain),
        "triangle" => ("△", Plain),
        "ell" => ("ℓ", Ident),
        "vdots" => ("⋮", Plain),
        "ddots" => ("⋱", Plain),
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
    /// One argument: a `{…}` group or a single atom (an empty group past the nesting limit).
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
    /// The delimiter after `\left` or `\right`: a character, the one a command names
    /// (`\{`, `\langle`, …), or none for `.` and unknown commands.
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
                    "lfloor" => "⌊".into(),
                    "rfloor" => "⌋".into(),
                    "lceil" => "⌈".into(),
                    "rceil" => "⌉".into(),
                    "vert" => "|".into(),
                    "Vert" => "‖".into(),
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
    /// The next atom: a group, a command, an environment or a character; `None` at the end of
    /// the input. Scripts after it are left to the caller.
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
                    "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan" | "sinh" | "cosh" | "tanh" | "coth" | "log"
                    | "lg" | "ln" | "exp" | "det" | "deg" | "dim" | "gcd" | "ker" | "arg" | "lim" | "max" | "min" => Node::OpName(name),
                    "mathbb" => Node::Sym(self.raw_group().chars().map(double_struck).collect(), Class::Plain),
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

/// Parse `src` into a node tree, from the top level.
fn parse(src: &str) -> Node {
    parse_at(src, 0)
}

/// Parse `src` with `depth` levels of nesting already used (nested parses share
/// [`MAX_NESTING`]).
fn parse_at(src: &str, depth: usize) -> Node {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { s: &chars, i: 0, depth };
    Node::Group(p.list(None))
}

/// `\mathbb` letters: the letterlike ℝ ℕ ℤ ℚ ℂ ℙ ℍ, other capitals from the Mathematical
/// Alphanumeric Symbols block; anything else unchanged.
fn double_struck(c: char) -> char {
    match c {
        'R' => 'ℝ',
        'N' => 'ℕ',
        'Z' => 'ℤ',
        'Q' => 'ℚ',
        'C' => 'ℂ',
        'P' => 'ℙ',
        'H' => 'ℍ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32)).unwrap_or(c),
        c => c,
    }
}

/// Typesetting fonts and metrics.
struct Fonts {
    upright: std::sync::Arc<FontFace>,
    italic: std::sync::Arc<FontFace>,
    /// Faces tried in order for a character the serif lacks, with their family names.
    fallbacks: Vec<(&'static str, std::sync::Arc<FontFace>)>,
}

impl Fonts {
    /// The bundled serif (upright and italic) and the fallback faces `db` has, in order.
    fn new(db: &FontDb) -> Fonts {
        let fallbacks =
            FALLBACKS.iter().filter(|(fam, _, bundled)| *bundled || db.has_family(fam)).map(|&(fam, style, _)| (fam, db.face(fam, style))).collect();
        Fonts { upright: db.face(FAMILY, "Regular"), italic: db.face(FAMILY, "Italic"), fallbacks }
    }
    /// The face that draws `c` and its family name, and whether it is the italic serif. A
    /// character no face has stays with the serif (which draws its missing-glyph box).
    fn face(&self, c: char, italic: bool) -> (&str, &FontFace, bool) {
        let f = if italic { &self.italic } else { &self.upright };
        if f.glyph_for(c) != 0 {
            return (FAMILY, f, italic);
        }
        match self.fallbacks.iter().find(|(_, ff)| ff.glyph_for(c) != 0) {
            Some((fam, ff)) => (fam, ff, false),
            None => (FAMILY, f, italic),
        }
    }
    /// The advance of `s` at `size`, each character measured in the face that draws it.
    fn width(&self, s: &str, size: f64, italic: bool) -> f64 {
        s.chars()
            .map(|c| {
                let (_, f, _) = self.face(c, italic);
                f.advance(f.glyph_for(c)) / f.units_per_em().max(1.0) * size
            })
            .sum()
    }
}

const FAMILY: &str = designcraft_fonts::DEFAULT_FAMILY;
/// Fallback faces in order: (family, style, bundled). A serif that has the symbol comes first so
/// it matches the text (an installed DejaVu Serif), then the bundled sans, and the bundled
/// monospace only last: its fixed-width advances and shapes stand out in serif maths. Without
/// DejaVu Serif the output still uses only bundled faces (the browser too).
const FALLBACKS: [(&str, &str, bool); 3] = [("DejaVu Serif", "Book", false), ("Source Sans 3", "Regular", true), ("JetBrains Mono", "Regular", true)];

/// A box holding `s` at `size` (italic or upright), with the nominal ascent and descent.
fn text_box(f: &Fonts, s: &str, size: f64, italic: bool) -> MBox {
    let w = f.width(s, size, italic);
    MBox { w, asc: size * 0.72, desc: size * 0.22, els: vec![Elem::Text { x: 0.0, y: 0.0, s: s.to_string(), size, italic }] }
}

/// `parts` (each with the space before it) side by side on a common baseline.
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

/// An operator name, bare or with scripts (`\cos`, `\cos^2`, `\log_2`).
fn is_op(n: &Node) -> bool {
    match n {
        Node::OpName(_) => true,
        Node::Scripts { base, .. } => matches!(&**base, Node::OpName(_)),
        _ => false,
    }
}

/// Whether the item before `list[k]` leaves no room for an operator's thin space: an opening
/// delimiter, an explicit space (`\,\sin`, `\quad\lim`), or a bare `|` that opens a pair
/// because it starts the list or follows a relation, operator, punctuation, opening delimiter
/// or space (`|\sin x|`, `y=|\cos x|`). TeX would space the bar there; it reads as an opener.
fn opens_before(list: &[Node], k: usize) -> bool {
    let opener = |n: &Node| matches!(n, Node::Sym(s, _) if matches!(s.as_str(), "(" | "[" | "{" | "⟨" | "⌊" | "⌈"));
    let Some(prev) = k.checked_sub(1).and_then(|j| list.get(j)) else { return true };
    if opener(prev) || matches!(prev, Node::Space(_)) {
        return true;
    }
    if !matches!(prev, Node::Sym(s, _) if s == "|") {
        return false;
    }
    match k.checked_sub(2).and_then(|j| list.get(j)) {
        None => true,
        Some(n) => opener(n) || matches!(n, Node::Space(_) | Node::Sym(_, Class::Rel | Class::WideRel | Class::Bin | Class::Punct)),
    }
}

/// A long arrow (`⟹` `⟸` `⟺`) when some face draws it, else the short one (`⇒` `⇐` `⇔`), so
/// the bundled fonts alone never draw a missing-glyph box for `\iff`.
fn long_arrow_or_short(f: &Fonts, s: &str) -> String {
    let short = match s {
        "⟹" => '⇒',
        "⟸" => '⇐',
        "⟺" => '⇔',
        "⟶" => '→',
        "⟵" => '←',
        _ => return s.to_string(),
    };
    let drawn = s.chars().all(|c| {
        let (_, face, _) = f.face(c, false);
        face.glyph_for(c) != 0
    });
    if drawn { s.to_string() } else { short.to_string() }
}

/// `⋯` `⋮` `⋱` built from three dots of the upright serif, as TeX builds `\cdots`, `\vdots` and
/// `\ddots`, when the serif has no such glyph (a fallback face would set them in another design).
/// `None` for anything else, or when the serif has the glyph or no dot.
fn built_dots(f: &Fonts, s: &str, size: f64) -> Option<MBox> {
    let mut cs = s.chars();
    let (Some(c), None) = (cs.next(), cs.next()) else { return None };
    if !matches!(c, '⋯' | '⋮' | '⋱') || f.upright.glyph_for(c) != 0 {
        return None;
    }
    let dot = if c == '⋯' { '·' } else { '.' };
    if f.upright.glyph_for(dot) == 0 {
        return None;
    }
    let dw = f.upright.advance(f.upright.glyph_for(dot)) / f.upright.units_per_em().max(1.0) * size;
    let mu = size / 18.0;
    // (x, raise) of each dot: TeX's \cdotp\cdotp\cdotp with thin spaces; \vdots 0.4 em apart;
    // \ddots raised 7, 4 and 1 tenths of an em with 2 mu between.
    let (pos, w, asc): ([(f64, f64); 3], f64, f64) = match c {
        '⋯' => ([(0.0, 0.0), (dw + 3.0 * mu, 0.0), (2.0 * (dw + 3.0 * mu), 0.0)], 3.0 * dw + 6.0 * mu, size * 0.72),
        '⋮' => ([(0.0, 0.0), (0.0, size * 0.4), (0.0, size * 0.8)], dw, size * 0.92),
        _ => ([(mu, size * 0.7), (dw + 3.0 * mu, size * 0.4), (2.0 * dw + 5.0 * mu, size * 0.1)], 3.0 * dw + 6.0 * mu, size * 0.82),
    };
    let els = pos.iter().map(|&(x, r)| Elem::Text { x, y: -r, s: dot.to_string(), size, italic: false }).collect();
    Some(MBox { w, asc, desc: size * 0.22, els })
}

/// Lay out `n` at `size`: its box, elements placed from its left end on its baseline.
fn layout(f: &Fonts, n: &Node, size: f64) -> MBox {
    let axis = size * 0.25;
    match n {
        Node::Sym(s, cl) => built_dots(f, s, size).unwrap_or_else(|| text_box(f, &long_arrow_or_short(f, s), size, *cl == Class::Ident)),
        Node::Text(s) | Node::OpName(s) => text_box(f, s, size, false),
        Node::Space(em) => MBox { w: em * size, ..Default::default() },
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
                let after_op = k.checked_sub(1).and_then(|j| list.get(j)).is_some_and(|n| match n {
                    Node::OpName(_) => true,
                    Node::Scripts { base, .. } => matches!(&**base, Node::OpName(_)),
                    _ => false,
                });
                // A leading + or − (or after another operator, or an operator name) is a sign,
                // not a binary operator.
                let cl = if cl == Some(Class::Bin)
                    && (k == 0 || after_op || matches!(prev, Some(Class::Bin | Class::Rel | Class::WideRel | Class::Punct)))
                {
                    Some(Class::Plain)
                } else {
                    cl
                };
                let gap = |c: Option<Class>| match c {
                    Some(Class::Bin) => size * 4.0 / 18.0,
                    Some(Class::Rel) => size * 5.0 / 18.0,
                    Some(Class::WideRel) => size * 10.0 / 18.0,
                    _ => 0.0,
                };
                let mut before = gap(cl).max(gap(prev));
                if k == 0 {
                    before = 0.0;
                }
                if prev == Some(Class::Punct) {
                    before = before.max(size * 3.0 / 18.0);
                }
                // A thin space after an operator name (`\sin x`, `\log_2 n`), as TeX's Op–Ord, but
                // none before a relation, operator, punctuation or an opening delimiter (`\sin(x)`). A bare
                // `|` is an ordinary symbol in TeX, so `\log|x|` keeps the space (`\left|` does not).
                let opens =
                    matches!(item, Node::Fenced(..)) || matches!(item, Node::Sym(s, _) if matches!(s.as_str(), "(" | "[" | "{" | "⟨" | "⌊" | "⌈"));
                if after_op && !opens && !matches!(cl, Some(Class::Rel | Class::WideRel | Class::Bin | Class::Punct)) {
                    before = before.max(size * 3.0 / 18.0);
                }
                // And one before it after an ordinary atom, a closing delimiter or another operator
                // (TeX's Ord–Op: `2\cos x`, `c\lim`, `f(x)\sin x`). Not at the start, after a relation
                // or operator (which have their own space), an opening delimiter or an explicit space.
                if k > 0 && is_op(item) && !matches!(prev, Some(Class::Rel | Class::WideRel | Class::Bin | Class::Punct)) && !opens_before(list, k) {
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

/// Typeset `latex` at `size` points: SVG bytes and the size in points.
pub fn to_svg(latex: &str, size: f64) -> (Vec<u8>, (f64, f64)) {
    let f = Fonts::new(FontDb::global());
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
                    let (fam, _, it) = f.face(c, *italic);
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

    /// `\left`/`\right` take the floor, ceiling and bar commands as delimiters (they used to set
    /// nothing, dropping both brackets); an unknown command still sets none.
    #[test]
    fn floor_ceil_and_bars_as_fence_delimiters() {
        for (src, open, close) in [
            (r"\left\lfloor x \right\rfloor", "⌊", "⌋"),
            (r"\left\lceil \frac{n}{2} \right\rceil", "⌈", "⌉"),
            (r"\left\vert x \right\vert", "|", "|"),
            (r"\left\Vert x \right\Vert", "‖", "‖"),
            (r"\left\| x \right\|", "‖", "‖"),
            (r"\left\langle x \right\rangle", "⟨", "⟩"),
            (r"\left\lfloor x \right.", "⌊", ""),
            (r"\left\foo x \right\rceil", "", "⌉"),
        ] {
            let Node::Group(v) = parse(src) else { panic!() };
            assert!(matches!(&v[..], [Node::Fenced(o, _, c)] if o == open && c == close), "{src}: {v:?}");
        }
        let svg = String::from_utf8(to_svg(r"\left\lfloor \frac{x}{2} \right\rfloor", 12.0).0).unwrap();
        assert!(svg.contains(">⌊<") && svg.contains(">⌋<"), "{svg}");
    }

    /// Every glyph written for the new symbols comes from a font that has it: the bundled ones
    /// cover all but ∖ ∗ ∠; an installed DejaVu Serif, when there, is preferred to the bundled sans
    /// and monospace.
    #[test]
    fn symbols_and_double_struck_letters() {
        let src = r"\{x\in\mathbb{R}\mid x\notin\mathbb{Z}\} \cup A\cap B,\ f\circ g,\ \forall n\in\mathbb{N}\ \exists q\in\mathbb{Q}: \lfloor q\rfloor \le \lceil q\rceil,\ \langle u,v\rangle,\ A\subseteq B \iff \emptyset,\ \neg p \implies \vdots";
        let s = String::from_utf8(to_svg(src, 12.0).0).unwrap();
        for c in ["ℝ", "ℤ", "ℕ", "ℚ", "|", "∉", "∪", "∩", "∘", "∀", "∃", "⌊", "⌋", "⌈", "⌉", "⟨", "⟩", "⊆", "∅", "¬"]
        {
            assert!(s.contains(&format!(">{c}<")), "{c}: {s}");
        }
        // `\iff` and `\implies` are long arrows where a face has them, else the short ones.
        assert!(s.contains(">⟺<") || s.contains(">⇔<"), "{s}");
        assert!(s.contains(">⟹<") || s.contains(">⇒<"), "{s}");
        assert!(!s.contains(">\\<"), "an unknown command leaked: {s}");
        let db = FontDb::global();
        let f = Fonts::new(db);
        for c in "ℝℤℕℚℂ∉∪∩∘∀∃⌊⌋⌈⌉⟨⟩⊆⇔∅¬⇒⇐↦∼ℓ".chars() {
            let (fam, face, _) = f.face(c, false);
            assert!(face.glyph_for(c) != 0, "{c} from {fam}");
            // The monospace is the last resort: any serif or sans fallback that has the symbol wins.
            if fam == "JetBrains Mono" {
                assert!(f.fallbacks.iter().filter(|(n, _)| *n != "JetBrains Mono").all(|(_, ff)| ff.glyph_for(c) == 0), "{c} from the monospace");
            }
            // An installed DejaVu Serif draws what the text serif lacks.
            if db.has_family("DejaVu Serif") && f.upright.glyph_for(c) == 0 && db.face("DejaVu Serif", "Book").glyph_for(c) != 0 {
                assert_eq!(fam, "DejaVu Serif", "{c}");
            }
        }
        assert_eq!(double_struck('A'), '𝔸');
        assert_eq!(double_struck('x'), 'x');
    }

    /// Operator names are upright and followed by a thin space before an operand, not before a
    /// delimiter, relation or a sign.
    #[test]
    fn operator_names_take_a_thin_space() {
        let w = |src: &str| to_svg(src, 12.0).1.0;
        let thin = 12.0 * 3.0 / 18.0;
        for name in ["sin", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "det", "deg", "dim", "gcd", "log", "lim"] {
            assert!(matches!(&parse(&format!("\\{name} x")), Node::Group(v) if v.first() == Some(&Node::OpName(name.into()))), "{name}");
            let spaced = w(&format!("\\{name} x")) - w(&format!("\\{name}")) - w("x") + w("");
            assert!((spaced - thin).abs() < 0.05, "{name}: {spaced}");
        }
        assert!((w(r"\sin(x)") - w(r"\sin") - w("(x)") + w("")).abs() < 0.05);
        // A bare `|` is ordinary (thin space, as TeX's `\log|x|`); `\left|` opens (none).
        assert!((w(r"\log|x|") - w(r"\log") - w("|x|") + w("") - thin).abs() < 0.05);
        assert!((w(r"\log\left|x\right|") - w(r"\log") - w(r"\left|x\right|") + w("")).abs() < 0.05);
        assert!((w(r"\sin\left(x\right)") - w(r"\sin") - w(r"\left(x\right)") + w("")).abs() < 0.05);
        // `\sin -x`: the minus is a sign (no medium spaces around it).
        assert!((w(r"\sin -x") - w(r"\sin") - w("-x") + w("") - thin).abs() < 0.05);
        // `\text{}` is not an operator: no space after it.
        assert!((w(r"\text{if}x") - w(r"\text{if}") - w("x") + w("")).abs() < 0.05);
    }

    /// TeX's Ord–Op space: an operator name after a number, letter, closing delimiter, script or
    /// fraction gets a thin space before it too; not at the start, after an opening delimiter, a
    /// relation, an explicit space or an opening bar.
    #[test]
    fn operator_names_take_a_thin_space_before_them() {
        let w = |src: &str| to_svg(src, 12.0).1.0;
        let thin = 12.0 * 3.0 / 18.0;
        // Extra width of `a` followed by `b` over the two set apart.
        let gap = |a: &str, b: &str| w(&format!("{a}{b}")) - w(a) - w(b) + w("");
        for (a, b) in
            [("2", r"\cos x"), ("c", r"\lim_{x} f"), ("x^2", r"\sin x"), ("f(x)", r"\sin x"), (r"\frac{1}{2}", r"\log x"), ("3", r"\cos^2 x")]
        {
            assert!((gap(a, b) - thin).abs() < 0.05, "{a} {b}: {}", gap(a, b));
        }
        // `\sin\cos x`: one thin space between the names (after one, before the other).
        assert!((gap(r"\sin", r"\cos x") - thin).abs() < 0.05);
        for (a, b) in [("(", r"\sin x"), ("[", r"\cos x"), (r"2\,", r"\cos x"), (r"x\quad", r"\lim f"), ("|", r"\sin x|"), ("y=|", r"\cos x|")] {
            assert!(gap(a, b).abs() < 0.05, "{a} {b}: {}", gap(a, b));
        }
        // After a relation or operator only its own space: `=\sin x` as `= x`.
        let rel = 12.0 * 5.0 / 18.0;
        assert!((gap("y=", r"\sin x") - rel).abs() < 0.05, "{}", gap("y=", r"\sin x"));
        assert!((gap("a+", r"\sin x") - 12.0 * 4.0 / 18.0).abs() < 0.05, "{}", gap("a+", r"\sin x"));
        // The space after the name is unchanged: `2\cos x` is `2`, thin, `\cos`, thin, `x`.
        assert!((w(r"2\cos x") - w("2") - w(r"\cos") - w("x") + 2.0 * w("") - 2.0 * thin).abs() < 0.05);
    }

    /// `\iff`, `\implies` and `\impliedby` are long double arrows with TeX's extra thick space
    /// each side (`\;\Longleftrightarrow\;`); `\Leftrightarrow` stays short with a relation's
    /// space.
    #[test]
    fn iff_is_a_long_arrow_with_wide_spacing() {
        // Load the fallback faces first: a face another test thread is loading at the same
        // moment can come back as the default face once, which would change one width below.
        let _ = Fonts::new(FontDb::global());
        let w = |src: &str| to_svg(src, 12.0).1.0;
        let gap = |src: &str, op: &str| w(src) - w("a") - w(op) - w("b") + 2.0 * w("");
        let (thick, wide) = (12.0 * 5.0 / 18.0, 12.0 * 10.0 / 18.0);
        for op in [r"\iff", r"\implies", r"\impliedby"] {
            assert!((gap(&format!("a{op} b"), op) - 2.0 * wide).abs() < 0.05, "{op}");
        }
        assert!((gap(r"a\Leftrightarrow b", r"\Leftrightarrow") - 2.0 * thick).abs() < 0.05);
        assert!((gap(r"a\Longleftrightarrow b", r"\Longleftrightarrow") - 2.0 * thick).abs() < 0.05);
        let svg = |src: &str| String::from_utf8(to_svg(src, 12.0).0).unwrap();
        let f = Fonts::new(FontDb::global());
        for (src, long, short) in [(r"\iff", '⟺', '⇔'), (r"\implies", '⟹', '⇒'), (r"\impliedby", '⟸', '⇐')] {
            let (_, face, _) = f.face(long, false);
            let want = if face.glyph_for(long) != 0 { long } else { short };
            assert!(svg(src).contains(&format!(">{want}<")), "{src}");
        }
        // A sign after `\iff` is a sign, not a binary operator.
        assert!(w(r"a\iff -b") < w(r"a\iff c-b"));
    }

    /// `\cdots`, `\vdots` and `\ddots` are built from three dots of the serif when it lacks the
    /// glyph, instead of borrowing a glyph of another design.
    #[test]
    fn dots_are_built_from_the_serif() {
        let f = Fonts::new(FontDb::global());
        let svg = |src: &str| String::from_utf8(to_svg(src, 12.0).0).unwrap();
        let ys = |s: &str, dot: &str| -> Vec<(f64, f64)> {
            s.split("<text ")
                .filter(|t| t.contains(&format!(">{dot}<")))
                .filter_map(|t| {
                    let num = |k: &str| t.split(k).nth(1)?.split('"').next()?.parse::<f64>().ok();
                    Some((num("x=\"")?, num("y=\"")?))
                })
                .collect()
        };
        for (src, c, dot) in [(r"\cdots", '⋯', "·"), (r"\vdots", '⋮', "."), (r"\ddots", '⋱', ".")] {
            let s = svg(src);
            if f.upright.glyph_for(c) != 0 {
                assert!(s.contains(&format!(">{c}<")), "{src}");
                continue;
            }
            assert!(!s.contains(&format!(">{c}<")), "{src}: {s}");
            let p = ys(&s, dot);
            assert_eq!(p.len(), 3, "{src}: {s}");
            assert!(s.contains("Source Serif 4"), "{src}: {s}");
            let (dx, dy) = (p[1].0 - p[0].0, p[1].1 - p[0].1);
            match c {
                '⋯' => assert!(dx > 2.0 && dy.abs() < 1e-6 && (p[2].0 - p[1].0 - dx).abs() < 1e-3, "{p:?}"),
                '⋮' => assert!(dx.abs() < 1e-6 && (dy + 4.8).abs() < 1e-3 && (p[2].1 - p[1].1 - dy).abs() < 1e-3, "{p:?}"),
                _ => assert!(dx > 0.0 && (dy - 3.6).abs() < 1e-3 && p[2].0 > p[1].0, "{p:?}"),
            }
        }
        // Built dots take part in spacing like the glyph: `a+\cdots+b` keeps its binary spaces.
        let w = |src: &str| to_svg(src, 12.0).1.0;
        assert!(w(r"a+\cdots+b") > w(r"\cdots") + w("a+b") - w(""));
        // `\ldots` is the serif's own ellipsis.
        assert!(svg(r"\ldots").contains(">…<"));
    }

    /// Laid-out sizes are plausible (a fraction taller than a letter, limits taller still) and
    /// the SVG parses and rasterises.
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
        ] {
            let (svg, _) = to_svg(&latex, 12.0);
            assert!(!svg.is_empty());
        }
    }
}
