//! Math expressions: a LaTeX subset typeset into boxes and written as SVG (text in the bundled
//! serif, rules and radicals as paths). Covers fractions, super/subscripts, square and n-th roots,
//! big operators with limits, Greek letters, common symbols, `\text{}`, `\mathrm{}`, spacing
//! commands and `\left( … \right)`.

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

#[cfg_attr(test, derive(Debug, PartialEq))]
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
    /// A big operator (∑ ∏ ∫): drawn larger, limits as scripts.
    Big(String),
    /// `\left( … \right)`: delimiters sized to the content.
    Fenced(String, Box<Node>, String),
}

// Empty a node before field drop glue can follow its children recursively. This also covers
// replaced scripts and partially built parser continuations during unwinding.
impl Node {
    /// Move every child link into `pending`, draining `Group` children and replacing boxed children with empty groups, so field drop glue never walks a deep subtree.
    fn detach_children(&mut self, pending: &mut Vec<Node>) {
        /// Detach one child into `pending`, leaving an empty group in its slot.
        fn detach(child: &mut Node, pending: &mut Vec<Node>) {
            pending.push(std::mem::replace(child, Node::Group(Vec::new())));
        }
        match self {
            Self::Group(nodes) => pending.append(nodes),
            Self::Frac(a, b) => {
                detach(a, pending);
                detach(b, pending);
            }
            Self::Sqrt(index, body) => {
                if let Some(index) = index {
                    detach(index, pending);
                }
                detach(body, pending);
            }
            Self::Scripts { base, sup, sub } => {
                detach(base, pending);
                if let Some(sup) = sup {
                    detach(sup, pending);
                }
                if let Some(sub) = sub {
                    detach(sub, pending);
                }
            }
            Self::Fenced(_, body, _) => detach(body, pending),
            Self::Sym(_, _) | Self::Text(_) | Self::Space(_) | Self::Big(_) => {}
        }
    }
}

impl Drop for Node {
    /// Drain an explicit worklist, emptying each node before normal drop glue, so deep or replaced ASTs drop without stack overflow.
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.detach_children(&mut pending);
        while let Some(mut node) = pending.pop() {
            node.detach_children(&mut pending);
            // Its own Drop and boxed field glue now see only empty leaves, never this subtree.
        }
    }
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

/// Deepest logical expression nesting parsed. Heap continuations keep this full budget safe on
/// the default native thread stack; past the cap groups retain their existing empty result.
const MAX_NESTING: usize = 256;

struct Parser<'a> {
    s: &'a [char],
    i: usize,
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
}

/// Continuations carry parser state on the heap instead of in native call frames.
enum ParseWork<'a> {
    List { end: Option<char>, depth: usize },
    Sequence { kind: SequenceKind, depth: usize, out: Vec<Node> },
    Append { kind: SequenceKind, depth: usize, out: Vec<Node>, group: bool },
    Arg { depth: usize },
    Atom { depth: usize },
    Script { kind: SequenceKind, depth: usize, out: Vec<Node>, up: bool },
    FractionNumerator { depth: usize },
    FractionDenominator { numerator: Node },
    SqrtIndex { outer: Parser<'a>, depth: usize },
    SqrtArgument { index: Option<Box<Node>> },
    Fence { open: String },
    UntilRight { depth: usize },
    One { depth: usize },
    OneAtom { depth: usize },
}

#[derive(Clone, Copy)]
enum SequenceKind {
    List(Option<char>),
    Fence,
    One,
}

/// Repeated scripts replace only their own slot, preserving the base and the opposite slot.
fn attach_script(mut base: Node, argument: Node, up: bool) -> Node {
    if let Node::Scripts { sup, sub, .. } = &mut base {
        let slot = if up { sup } else { sub };
        *slot = Some(Box::new(argument));
        base
    } else if up {
        Node::Scripts { base: Box::new(base), sup: Some(Box::new(argument)), sub: None }
    } else {
        Node::Scripts { base: Box::new(base), sup: None, sub: Some(Box::new(argument)) }
    }
}

impl<'a> Parser<'a> {
    /// Run the parser as a heap worklist from a root `List`, so input nesting does not consume native stack.
    fn parse(&mut self, depth: usize) -> Node {
        let mut work = vec![ParseWork::List { end: None, depth }];
        let mut value = Node::Group(Vec::new());
        while let Some(next) = work.pop() {
            match next {
                ParseWork::List { end, depth } => Self::step_list(&mut work, &mut value, end, depth),
                ParseWork::Sequence { kind, depth, out } => self.step_sequence(&mut work, &mut value, kind, depth, out),
                ParseWork::Append { kind, depth, out, group } => Self::step_append(&mut work, &mut value, kind, depth, out, group),
                ParseWork::Arg { depth } => self.step_arg(&mut work, &mut value, depth),
                ParseWork::Atom { depth } => self.step_atom(&mut work, &mut value, depth),
                ParseWork::Script { kind, depth, out, up } => Self::step_script(&mut work, &mut value, kind, depth, out, up),
                ParseWork::FractionNumerator { depth } => Self::step_fraction_numerator(&mut work, &mut value, depth),
                ParseWork::FractionDenominator { numerator } => Self::step_fraction_denominator(&mut value, numerator),
                ParseWork::SqrtIndex { outer, depth } => self.step_sqrt_index(&mut work, &mut value, outer, depth),
                ParseWork::SqrtArgument { index } => Self::step_sqrt_argument(&mut value, index),
                ParseWork::Fence { open } => self.step_fence(&mut value, open),
                ParseWork::UntilRight { depth } => Self::step_until_right(&mut work, &mut value, depth),
                ParseWork::One { depth } => Self::step_one(&mut work, depth),
                ParseWork::OneAtom { depth } => Self::step_one_atom(&mut work, &mut value, depth),
            }
        }
        value
    }

    /// Start a list sequence, or yield an empty group when `depth` has reached `MAX_NESTING`.
    fn step_list(work: &mut Vec<ParseWork<'a>>, value: &mut Node, end: Option<char>, depth: usize) {
        if depth >= MAX_NESTING {
            *value = Node::Group(Vec::new());
        } else {
            work.push(ParseWork::Sequence { kind: SequenceKind::List(end), depth: depth + 1, out: Vec::new() });
        }
    }

    /// Consume list/fence terminators, route `^`/`_` to script attachment, append the next atom for list and fence sequences, or finish a `One` sequence after its single atom; fence sequences stop only at a real `\right`.
    fn step_sequence(&mut self, work: &mut Vec<ParseWork<'a>>, value: &mut Node, kind: SequenceKind, depth: usize, mut out: Vec<Node>) {
        self.skip_ws();
        let Some(c) = self.peek() else {
            *value = Node::Group(out);
            return;
        };
        if let SequenceKind::List(Some(end)) = kind
            && c == end
        {
            self.i += 1;
            *value = Node::Group(out);
            return;
        }
        if matches!(kind, SequenceKind::Fence)
            && self.s.get(self.i..).is_some_and(|s| s.starts_with(&['\\', 'r', 'i', 'g', 'h', 't']))
            && !self.s.get(self.i + 6).is_some_and(char::is_ascii_alphabetic)
        {
            self.i += 6;
            *value = Node::Group(out);
            return;
        }
        if (c == '^' || c == '_') && !matches!(kind, SequenceKind::Fence) {
            self.i += 1;
            work.push(ParseWork::Script { kind, depth, out, up: c == '^' });
            work.push(ParseWork::Arg { depth });
        } else if matches!(kind, SequenceKind::One) {
            *value = Node::Group(out);
        } else {
            let group = matches!(kind, SequenceKind::Fence);
            work.push(ParseWork::Append { kind, depth, out: std::mem::take(&mut out), group });
            work.push(if group { ParseWork::One { depth: depth + 1 } } else { ParseWork::Atom { depth } });
        }
    }

    /// Commit the completed atom or fence group into the pending sequence and resume it.
    fn step_append(work: &mut Vec<ParseWork<'a>>, value: &mut Node, kind: SequenceKind, depth: usize, mut out: Vec<Node>, group: bool) {
        if group {
            if let Node::Group(nodes) = &mut *value {
                out.append(nodes);
            }
        } else {
            out.push(std::mem::replace(value, Node::Group(Vec::new())));
        }
        work.push(ParseWork::Sequence { kind, depth, out });
    }

    /// Parse a braced list or one atom at `depth + 1`; a capped argument becomes an empty group.
    fn step_arg(&mut self, work: &mut Vec<ParseWork<'a>>, value: &mut Node, depth: usize) {
        if depth >= MAX_NESTING {
            *value = Node::Group(Vec::new());
            return;
        }
        self.skip_ws();
        if self.peek() == Some('{') {
            self.i += 1;
            work.push(ParseWork::List { end: Some('}'), depth: depth + 1 });
        } else {
            work.push(ParseWork::Atom { depth: depth + 1 });
        }
    }

    /// Attach the completed argument as superscript or subscript, preserving the opposite slot of an existing script node.
    fn step_script(work: &mut Vec<ParseWork<'a>>, value: &mut Node, kind: SequenceKind, depth: usize, mut out: Vec<Node>, up: bool) {
        let argument = std::mem::replace(value, Node::Group(Vec::new()));
        let base = out.pop().unwrap_or(Node::Group(Vec::new()));
        out.push(attach_script(base, argument, up));
        work.push(ParseWork::Sequence { kind, depth, out });
    }

    /// Save the parsed numerator and queue the denominator argument.
    fn step_fraction_numerator(work: &mut Vec<ParseWork<'a>>, value: &mut Node, depth: usize) {
        let numerator = std::mem::replace(value, Node::Group(Vec::new()));
        work.push(ParseWork::FractionDenominator { numerator });
        work.push(ParseWork::Arg { depth });
    }

    /// Build `Frac` from the saved numerator and the completed denominator.
    fn step_fraction_denominator(value: &mut Node, numerator: Node) {
        *value = Node::Frac(Box::new(numerator), Box::new(std::mem::replace(value, Node::Group(Vec::new()))));
    }

    /// Restore the outer parser after the root-index subparse and queue the radicand argument.
    fn step_sqrt_index(&mut self, work: &mut Vec<ParseWork<'a>>, value: &mut Node, outer: Parser<'a>, depth: usize) {
        *self = outer;
        let index = Some(Box::new(std::mem::replace(value, Node::Group(Vec::new()))));
        work.push(ParseWork::SqrtArgument { index });
        work.push(ParseWork::Arg { depth });
    }

    /// Build `Sqrt`, retaining the optional root index.
    fn step_sqrt_argument(value: &mut Node, index: Option<Box<Node>>) {
        *value = Node::Sqrt(index, Box::new(std::mem::replace(value, Node::Group(Vec::new()))));
    }

    /// Wrap the completed body in `Fenced` with the opening and resolved closing delimiter.
    fn step_fence(&mut self, value: &mut Node, open: String) {
        let close = self.delimiter();
        *value = Node::Fenced(open, Box::new(std::mem::replace(value, Node::Group(Vec::new()))), close);
    }

    /// Start a fence-body sequence that stops at a real `\right`, or an empty group at the nesting cap.
    fn step_until_right(work: &mut Vec<ParseWork<'a>>, value: &mut Node, depth: usize) {
        if depth >= MAX_NESTING {
            *value = Node::Group(Vec::new());
        } else {
            work.push(ParseWork::Sequence { kind: SequenceKind::Fence, depth, out: Vec::new() });
        }
    }

    /// Queue one atom and its one-item sequence collector.
    fn step_one(work: &mut Vec<ParseWork<'a>>, depth: usize) {
        work.push(ParseWork::OneAtom { depth });
        work.push(ParseWork::Atom { depth });
    }

    /// Wrap the completed atom in a `Group` and continue the `One` sequence.
    fn step_one_atom(work: &mut Vec<ParseWork<'a>>, value: &mut Node, depth: usize) {
        let out = vec![std::mem::replace(value, Node::Group(Vec::new()))];
        work.push(ParseWork::Sequence { kind: SequenceKind::One, depth, out });
    }

    /// Parse a number, identifier, operator, punctuation mark or symbol with its spacing class, dispatching braces and commands.
    fn step_atom(&mut self, work: &mut Vec<ParseWork<'a>>, value: &mut Node, depth: usize) {
        self.skip_ws();
        let Some(c) = self.peek() else {
            *value = Node::Group(Vec::new());
            return;
        };
        self.i += 1;
        match c {
            '{' => work.push(ParseWork::List { end: Some('}'), depth }),
            '\\' => self.step_command(work, value, depth),
            c if c.is_ascii_digit() || c == '.' => {
                let mut s = c.to_string();
                while let Some(d) = self.peek().filter(|d| d.is_ascii_digit() || *d == '.') {
                    s.push(d);
                    self.i += 1;
                }
                *value = Node::Sym(s, Class::Number);
            }
            c if c.is_alphabetic() => *value = Node::Sym(c.to_string(), Class::Ident),
            '+' => *value = Node::Sym("+".into(), Class::Bin),
            '-' => *value = Node::Sym("−".into(), Class::Bin),
            '*' => *value = Node::Sym("∗".into(), Class::Bin),
            '<'..='>' => *value = Node::Sym(c.to_string(), Class::Rel),
            ',' | ';' => *value = Node::Sym(c.to_string(), Class::Punct),
            '\'' => *value = Node::Sym("′".into(), Class::Plain),
            c => *value = Node::Sym(c.to_string(), Class::Plain),
        }
    }

    /// Dispatch fractions, roots, fences, raw-text commands and named symbols/operators.
    fn step_command(&mut self, work: &mut Vec<ParseWork<'a>>, value: &mut Node, depth: usize) {
        let name = self.command();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                work.push(ParseWork::FractionNumerator { depth });
                work.push(ParseWork::Arg { depth });
            }
            "sqrt" => self.push_sqrt(work, depth),
            "left" => {
                let open = self.delimiter();
                work.push(ParseWork::Fence { open });
                work.push(ParseWork::UntilRight { depth });
            }
            "text" | "mathrm" | "textrm" | "operatorname" => *value = Node::Text(self.raw_group()),
            n => *value = command_atom(n),
        }
    }

    /// Parse an optional bracketed root index with the existing first-`]` grammar, otherwise queue a braced or atomic radicand.
    fn push_sqrt(&mut self, work: &mut Vec<ParseWork<'a>>, depth: usize) {
        self.skip_ws();
        if self.peek() == Some('[') {
            self.i += 1;
            let start = self.i;
            let mut end = start;
            // Keep the existing first-']' index grammar, including EOF.
            while let Some(c) = self.peek() {
                self.i += 1;
                if c == ']' {
                    break;
                }
                end = self.i;
            }
            let inner = self.s.get(start..end).unwrap_or_default();
            let outer = std::mem::replace(self, Parser { s: inner, i: 0 });
            work.push(ParseWork::SqrtIndex { outer, depth });
            work.push(ParseWork::List { end: None, depth: depth + 1 });
        } else {
            work.push(ParseWork::SqrtArgument { index: None });
            work.push(ParseWork::Arg { depth });
        }
    }
}

/// Map function names, spacing commands, escaped punctuation, big operators and symbols; an unknown command becomes literal `\name` text.
fn command_atom(name: &str) -> Node {
    match name {
        "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "lim" | "max" | "min" => Node::Text(name.to_owned()),
        "," => Node::Space(3.0 / 18.0),
        ":" | ">" => Node::Space(4.0 / 18.0),
        ";" => Node::Space(5.0 / 18.0),
        "quad" => Node::Space(1.0),
        "qquad" => Node::Space(2.0),
        "!" => Node::Space(-3.0 / 18.0),
        " " => Node::Space(0.25),
        "{" => Node::Sym("{".into(), Class::Punct),
        "}" => Node::Sym("}".into(), Class::Punct),
        "%" | "$" | "#" | "&" | "_" => Node::Sym(name.to_owned(), Class::Plain),
        n => match (big_op(n), symbol(n)) {
            (Some(op), _) => Node::Big(op.into()),
            (None, Some((s, cl))) => Node::Sym(s.into(), cl),
            _ => Node::Text(format!("\\{n}")),
        },
    }
}

fn parse(src: &str) -> Node {
    parse_at(src, 0)
}

/// Collect the input into chars and start parsing at the supplied depth.
fn parse_at(src: &str, depth: usize) -> Node {
    let chars: Vec<char> = src.chars().collect();
    Parser { s: &chars, i: 0 }.parse(depth)
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

struct GroupLayout<'a> {
    items: std::iter::Enumerate<std::slice::Iter<'a, Node>>,
    parts: Vec<(MBox, f64)>,
    prev: Option<Class>,
    size: f64,
}

struct ScriptLayout {
    out: MBox,
    bw: f64,
    basc: f64,
    bdesc: f64,
    big: bool,
    size: f64,
}

impl ScriptLayout {
    /// Seed script layout with the base box width and metrics, the big-operator flag and the size.
    fn new(base: MBox, big: bool, size: f64) -> Self {
        Self { bw: base.w, basc: base.asc, bdesc: base.desc, out: base, big, size }
    }
    /// Place a superscript, subscript or big-operator limit and expand the aggregate width, ascent and descent.
    fn add(&mut self, script: MBox, up: bool) {
        let Self { out, bw, basc, bdesc, big, size } = self;
        let y = if *big {
            if up { -*basc - *size * 0.1 - script.desc } else { *bdesc + *size * 0.1 + script.asc }
        } else if up {
            -(*size * 0.42).max(*basc - script.asc * 0.6)
        } else {
            (*size * 0.2).max(*bdesc * 0.8)
        };
        out.w = out.w.max(if *big { script.w } else { *bw + script.w + *size * 0.05 });
        let (sa, sd) = (script.asc, script.desc);
        let x = if *big { (*bw - script.w) / 2.0 } else { *bw + *size * 0.05 };
        out.els.extend(script.shifted(x, y));
        if up {
            out.asc = out.asc.max(-y + sa);
        } else {
            out.desc = out.desc.max(y + sd);
        }
    }
    /// For big operators, recentre the operator over the widest limit.
    fn finish(mut self) -> MBox {
        if self.big {
            let dx = (self.out.w - self.bw) / 2.0;
            let els = std::mem::take(&mut self.out.els);
            self.out.els = MBox { els, ..Default::default() }.shifted(dx, 0.0);
        }
        self.out
    }
}

enum LayoutWork<'a> {
    Visit(&'a Node, f64),
    GroupNext(GroupLayout<'a>),
    GroupAppend(GroupLayout<'a>, f64),
    FractionNumerator(&'a Node, f64),
    FractionDenominator(MBox, f64),
    SqrtBody(Option<&'a Node>, f64),
    SqrtIndex(MBox, f64),
    ScriptBase { sup: Option<&'a Node>, sub: Option<&'a Node>, big: bool, size: f64 },
    ScriptSup { state: ScriptLayout, sub: Option<&'a Node> },
    ScriptSub(ScriptLayout),
    Fence(&'a str, &'a str, f64),
}

/// Postorder evaluation moves each child's box into its parent before visiting the next child.
/// No ancestor retains a second flattened copy, and native stack use is independent of nesting.
fn layout(f: &Fonts, n: &Node, size: f64) -> MBox {
    let mut work = vec![LayoutWork::Visit(n, size)];
    let mut value = MBox::default();
    while let Some(next) = work.pop() {
        match next {
            LayoutWork::Visit(node, size) => match node {
                Node::Sym(s, cl) => value = text_box(f, s, size, *cl == Class::Ident),
                Node::Text(s) => value = text_box(f, s, size, false),
                Node::Space(em) => value = MBox { w: em * size, ..Default::default() },
                Node::Group(list) => {
                    work.push(LayoutWork::GroupNext(GroupLayout { items: list.iter().enumerate(), parts: Vec::new(), prev: None, size }))
                }
                Node::Frac(a, b) => {
                    work.push(LayoutWork::FractionNumerator(b, size));
                    work.push(LayoutWork::Visit(a, size * 0.85));
                }
                Node::Sqrt(index, body) => {
                    work.push(LayoutWork::SqrtBody(index.as_deref(), size));
                    work.push(LayoutWork::Visit(body, size));
                }
                Node::Scripts { base, sup, sub } => {
                    work.push(LayoutWork::ScriptBase { sup: sup.as_deref(), sub: sub.as_deref(), big: matches!(&**base, Node::Big(_)), size });
                    work.push(LayoutWork::Visit(base, size));
                }
                Node::Big(op) => {
                    let mut b = text_box(f, op, size * 1.5, false);
                    b.asc = size * 1.05;
                    b.desc = size * 0.35;
                    for e in &mut b.els {
                        if let Elem::Text { y, .. } = e {
                            *y = size * 0.32;
                        }
                    }
                    value = b;
                }
                Node::Fenced(open, body, close) => {
                    work.push(LayoutWork::Fence(open, close, size));
                    work.push(LayoutWork::Visit(body, size));
                }
            },
            LayoutWork::GroupNext(mut group) => {
                if let Some((k, item)) = group.items.next() {
                    let cl = match item {
                        Node::Sym(_, c) => Some(*c),
                        Node::Scripts { base, .. } => match &**base {
                            Node::Sym(_, c) => Some(*c),
                            _ => None,
                        },
                        _ => None,
                    };
                    let cl = if cl == Some(Class::Bin) && (k == 0 || matches!(group.prev, Some(Class::Bin | Class::Rel | Class::Punct))) {
                        Some(Class::Plain)
                    } else {
                        cl
                    };
                    let gap = |c: Option<Class>| match c {
                        Some(Class::Bin) => group.size * 4.0 / 18.0,
                        Some(Class::Rel) => group.size * 5.0 / 18.0,
                        _ => 0.0,
                    };
                    let mut before = gap(cl).max(gap(group.prev));
                    if k == 0 {
                        before = 0.0;
                    }
                    if group.prev == Some(Class::Punct) {
                        before = before.max(group.size * 3.0 / 18.0);
                    }
                    group.prev = cl;
                    let size = group.size;
                    work.push(LayoutWork::GroupAppend(group, before));
                    work.push(LayoutWork::Visit(item, size));
                } else {
                    value = hbox(group.parts);
                }
            }
            LayoutWork::GroupAppend(mut group, before) => {
                group.parts.push((std::mem::take(&mut value), before));
                work.push(LayoutWork::GroupNext(group));
            }
            LayoutWork::FractionNumerator(denominator, size) => {
                work.push(LayoutWork::FractionDenominator(std::mem::take(&mut value), size));
                work.push(LayoutWork::Visit(denominator, size * 0.85));
            }
            LayoutWork::FractionDenominator(na, size) => {
                let nb = std::mem::take(&mut value);
                let axis = size * 0.25;
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
                value = out;
            }
            LayoutWork::SqrtBody(index, size) => {
                let inner = std::mem::take(&mut value);
                let axis = size * 0.25;
                let t = (size * 0.05).max(0.4);
                let gap = size * 0.12;
                let top = inner.asc + gap + t;
                let bottom = inner.desc;
                let hook = size * 0.55;
                let mut out = MBox { w: hook + inner.w + size * 0.1, asc: top, desc: bottom, els: Vec::new() };
                out.els.push(Elem::Line {
                    pts: vec![(0.0, -axis * 0.6), (hook * 0.25, -axis), (hook * 0.55, bottom), (hook, -top + t / 2.0), (out.w, -top + t / 2.0)],
                    w: t,
                });
                out.els.extend(inner.shifted(hook, 0.0));
                if let Some(index) = index {
                    work.push(LayoutWork::SqrtIndex(out, size));
                    work.push(LayoutWork::Visit(index, size * 0.55));
                } else {
                    value = out;
                }
            }
            LayoutWork::SqrtIndex(mut out, size) => {
                let ib = std::mem::take(&mut value);
                let (ia, iw) = (ib.asc, ib.w);
                let axis = size * 0.25;
                let hook = size * 0.55;
                out.els.extend(ib.shifted((hook * 0.3 - iw).max(0.0) - 0.0, -axis - size * 0.1));
                out.asc = out.asc.max(axis + size * 0.1 + ia);
                value = out;
            }
            LayoutWork::ScriptBase { sup, sub, big, size } => {
                let state = ScriptLayout::new(std::mem::take(&mut value), big, size);
                if let Some(sup) = sup {
                    work.push(LayoutWork::ScriptSup { state, sub });
                    work.push(LayoutWork::Visit(sup, size * 0.7));
                } else if let Some(sub) = sub {
                    work.push(LayoutWork::ScriptSub(state));
                    work.push(LayoutWork::Visit(sub, size * 0.7));
                } else {
                    value = state.finish();
                }
            }
            LayoutWork::ScriptSup { mut state, sub } => {
                state.add(std::mem::take(&mut value), true);
                if let Some(sub) = sub {
                    let size = state.size;
                    work.push(LayoutWork::ScriptSub(state));
                    work.push(LayoutWork::Visit(sub, size * 0.7));
                } else {
                    value = state.finish();
                }
            }
            LayoutWork::ScriptSub(mut state) => {
                state.add(std::mem::take(&mut value), false);
                value = state.finish();
            }
            LayoutWork::Fence(open, close, size) => {
                let inner = std::mem::take(&mut value);
                let h = (inner.asc + inner.desc).max(size);
                let k = h / size;
                let ds = size * k.max(1.0);
                let mk = |s: &str| -> MBox {
                    if s.is_empty() {
                        return MBox::default();
                    }
                    let mut b = text_box(f, s, ds, false);
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
                value = hbox(vec![(l, 0.0), (inner, size * 0.05), (r, size * 0.05)]);
            }
        }
    }
    value
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
mod tests;
