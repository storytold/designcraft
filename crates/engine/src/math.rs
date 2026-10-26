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
mod tests {
    use super::*;

    // Frozen pre-repair arithmetic; called only for shallow fixtures, never deep input.
    /// Frozen recursive layout oracle for shallow test fixtures only, never deep input.
    fn recursive_layout_reference(f: &Fonts, n: &Node, size: f64) -> MBox {
        match n {
            Node::Sym(s, cl) => text_box(f, s, size, *cl == Class::Ident),
            Node::Text(s) => text_box(f, s, size, false),
            Node::Space(em) => MBox { w: em * size, ..Default::default() },
            Node::Group(list) => recursive_group_reference(f, list, size),
            Node::Frac(a, b) => recursive_fraction_reference(f, a, b, size),
            Node::Sqrt(idx, a) => recursive_sqrt_reference(f, idx, a, size),
            Node::Big(op) => recursive_big_reference(f, op, size),
            Node::Scripts { base, sup, sub } => recursive_scripts_reference(f, base, sup, sub, size),
            Node::Fenced(open, body, close) => recursive_fenced_reference(f, open, body, close, size),
        }
    }

    /// Oracle for operator spacing, leading-sign treatment and punctuation gaps.
    fn recursive_group_reference(f: &Fonts, list: &[Node], size: f64) -> MBox {
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
            parts.push((recursive_layout_reference(f, item, size), before));
            prev = cl;
        }
        hbox(parts)
    }

    /// Oracle for fraction rule, child scale, math axis, gaps and bounds.
    fn recursive_fraction_reference(f: &Fonts, a: &Node, b: &Node, size: f64) -> MBox {
        let axis = size * 0.25;
        let s2 = size * 0.85;
        let (na, nb) = (recursive_layout_reference(f, a, s2), recursive_layout_reference(f, b, s2));
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

    /// Oracle for radical geometry, body offset and optional root index.
    fn recursive_sqrt_reference(f: &Fonts, idx: &Option<Box<Node>>, a: &Node, size: f64) -> MBox {
        let axis = size * 0.25;
        let inner = recursive_layout_reference(f, a, size);
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
            let ib = recursive_layout_reference(f, i, size * 0.55);
            let (ia, iw2) = (ib.asc, ib.w);
            out.els.extend(ib.shifted((hook * 0.3 - iw2).max(0.0) - 0.0, -axis - size * 0.1));
            out.asc = out.asc.max(axis + size * 0.1 + ia);
        }
        out
    }

    /// Oracle for enlarged, math-axis-centred big operators.
    fn recursive_big_reference(f: &Fonts, op: &str, size: f64) -> MBox {
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

    /// Oracle for ordinary sup/sub placement and big-operator limits above and below.
    fn recursive_scripts_reference(f: &Fonts, base: &Node, sup: &Option<Box<Node>>, sub: &Option<Box<Node>>, size: f64) -> MBox {
        let big = matches!(base, Node::Big(_));
        let b = recursive_layout_reference(f, base, size);
        let s2 = size * 0.7;
        let (bw, basc, bdesc) = (b.w, b.asc, b.desc);
        let mut out = MBox { w: bw, asc: basc, desc: bdesc, els: b.els };
        if big {
            // Limits above and below.
            let mut w = bw;
            if let Some(sp) = sup {
                let sb = recursive_layout_reference(f, sp, s2);
                let y = -basc - size * 0.1 - sb.desc;
                w = w.max(sb.w);
                let (sw, sa) = (sb.w, sb.asc);
                out.els.extend(sb.shifted((bw - sw) / 2.0, y));
                out.asc = out.asc.max(-y + sa);
            }
            if let Some(sbn) = sub {
                let sb = recursive_layout_reference(f, sbn, s2);
                let y = bdesc + size * 0.1 + sb.asc;
                w = w.max(sb.w);
                let (sw, sd) = (sb.w, sb.desc);
                out.els.extend(sb.shifted((bw - sw) / 2.0, y));
                out.desc = out.desc.max(y + sd);
            }
            // Centre the operator over the widest limit.
            let dx = (w - bw) / 2.0;
            let els = std::mem::take(&mut out.els);
            return MBox { w, asc: out.asc, desc: out.desc, els: MBox { els, ..Default::default() }.shifted(dx, 0.0) };
        }
        let mut w = bw;
        if let Some(sp) = sup {
            let sb = recursive_layout_reference(f, sp, s2);
            let y = -(size * 0.42).max(basc - sb.asc * 0.6);
            w = w.max(bw + sb.w + size * 0.05);
            let sa = sb.asc;
            out.els.extend(sb.shifted(bw + size * 0.05, y));
            out.asc = out.asc.max(-y + sa);
        }
        if let Some(sbn) = sub {
            let sb = recursive_layout_reference(f, sbn, s2);
            let y = (size * 0.2).max(bdesc * 0.8);
            w = w.max(bw + sb.w + size * 0.05);
            let sd = sb.desc;
            out.els.extend(sb.shifted(bw + size * 0.05, y));
            out.desc = out.desc.max(y + sd);
        }
        out.w = w;
        out
    }

    /// Oracle for content-scaled, centred delimiters.
    fn recursive_fenced_reference(f: &Fonts, open: &str, body: &Node, close: &str, size: f64) -> MBox {
        let inner = recursive_layout_reference(f, body, size);
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

    /// Asserts fraction/nested-root, big scripts, relation, symbol, fence and raw `\text` parse shapes.
    #[test]
    fn parses_the_common_constructs() {
        let n = parse(r"\frac{a^2+b}{\sqrt[3]{x}}");
        let Node::Group(v) = &n else { panic!() };
        assert!(matches!(&v[0], Node::Frac(_, _)));
        let n = parse(r"\sum_{i=1}^{n} i^2 \le \alpha");
        let Node::Group(v) = &n else { panic!() };
        assert!(matches!(&v[0], Node::Scripts { base, sup: Some(_), sub: Some(_) } if matches!(**base, Node::Big(_))));
        assert!(v.iter().any(|x| *x == Node::Sym("≤".into(), Class::Rel)));
        assert!(v.iter().any(|x| *x == Node::Sym("α".into(), Class::Ident)));
        let n = parse(r"\left( \frac{1}{2} \right) + \text{if } x");
        let Node::Group(v) = &n else { panic!() };
        assert!(matches!(&v[0], Node::Fenced(o, _, c) if o == "(" && c == ")"));
        assert!(v.contains(&Node::Text("if ".into())));
    }

    /// A command that merely starts with "right" (`\rightarrow`) must not close a `\left` fence.
    #[test]
    fn right_prefixed_commands_do_not_close_a_fence() {
        let n = parse(r"\left( x \rightarrow y \right)");
        let Node::Group(v) = &n else { panic!() };
        assert_eq!(v.len(), 1, "{v:?}");
        let Node::Fenced(o, body, c) = &v[0] else { panic!("{v:?}") };
        assert!(o == "(" && c == ")", "{v:?}");
        let Node::Group(b) = &**body else { panic!("{v:?}") };
        assert_eq!(*b, vec![Node::Sym("x".into(), Class::Ident), Node::Sym("→".into(), Class::Rel), Node::Sym("y".into(), Class::Ident)]);
        // An unknown `\right…` command stays inside the fence too.
        let Node::Group(v) = &parse(r"\left[ \rightfoo \right]") else { panic!() };
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(matches!(&v[0], Node::Fenced(o, _, c) if o == "[" && c == "]"), "{v:?}");
        // Real closers still close it.
        for (src, close) in [(r"\left( x \right)", ")"), (r"\left( x \right.", ""), (r"\left( x \right)y", ")")] {
            let Node::Group(v) = &parse(src) else { panic!() };
            assert!(matches!(&v[0], Node::Fenced(_, _, c) if c == close), "{src}: {v:?}");
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
            format!("{}z{}", r"\sqrt{".repeat(1_024), "}".repeat(1_024)),
            format!("{}z{}", "x^{".repeat(1_024), "}".repeat(1_024)),
        ] {
            let (svg, (w, h)) = to_svg(&latex, 12.0);
            assert!(!svg.is_empty());
            assert!(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0, "{w} {h}");
        }
    }
    /// Count actual tree shape without recursively comparing or formatting a deep AST.
    fn shape(root: &Node) -> ([usize; 6], usize) {
        let mut counts = [0; 6]; // groups, fractions, roots, fences, scripts, sentinel z
        let mut deepest = 0;
        let mut work = vec![(root, 1)];
        while let Some((node, depth)) = work.pop() {
            deepest = deepest.max(depth);
            match node {
                Node::Group(nodes) => {
                    counts[0] += 1;
                    work.extend(nodes.iter().map(|node| (node, depth + 1)));
                }
                Node::Frac(a, b) => {
                    counts[1] += 1;
                    work.extend([(a.as_ref(), depth + 1), (b.as_ref(), depth + 1)]);
                }
                Node::Sqrt(index, body) => {
                    counts[2] += 1;
                    work.push((body, depth + 1));
                    if let Some(index) = index {
                        work.push((index, depth + 1));
                    }
                }
                Node::Fenced(_, body, _) => {
                    counts[3] += 1;
                    work.push((body, depth + 1));
                }
                Node::Scripts { base, sup, sub } => {
                    counts[4] += 1;
                    work.push((base, depth + 1));
                    if let Some(sup) = sup {
                        work.push((sup, depth + 1));
                    }
                    if let Some(sub) = sub {
                        work.push((sub, depth + 1));
                    }
                }
                Node::Sym(s, _) if s == "z" => counts[5] += 1,
                _ => {}
            }
        }
        (counts, deepest)
    }

    /// With `MAX_NESTING == 256`, all supported 255-level constructs stay nested and still render their sentinel.
    #[test]
    fn the_full_256_budget_keeps_deep_valid_formulas_intact() {
        assert_eq!(MAX_NESTING, 256, "preserve the existing expression capability");
        let groups = MAX_NESTING - 1;
        let arguments = (MAX_NESTING - 1) / 2;
        let index_groups = MAX_NESTING - 3;
        let cases = [
            (format!("{}z{}", "{".repeat(groups), "}".repeat(groups)), [groups + 1, 0, 0, 0, 0, 1], groups + 2),
            (format!("{}z{}", r"\left(".repeat(groups), r"\right)".repeat(groups)), [groups + 1, 0, 0, groups, 0, 1], groups * 2 + 2),
            (format!("{}z{}", r"\frac{".repeat(arguments), "}{y}".repeat(arguments)), [arguments * 2 + 1, arguments, 0, 0, 0, 1], arguments * 2 + 2),
            (format!("{}z{}", r"\sqrt{".repeat(arguments), "}".repeat(arguments)), [arguments + 1, 0, arguments, 0, 0, 1], arguments * 2 + 2),
            (format!("{}z{}", "x^{".repeat(arguments), "}".repeat(arguments)), [arguments + 1, 0, 0, 0, arguments, 1], arguments * 2 + 2),
            (format!("{}z{}", "x_{".repeat(arguments), "}".repeat(arguments)), [arguments + 1, 0, 0, 0, arguments, 1], arguments * 2 + 2),
            (format!(r"\sqrt[{}z{}]{{y}}", "{".repeat(index_groups), "}".repeat(index_groups)), [index_groups + 3, 0, 1, 0, 0, 1], index_groups + 4),
        ];
        for (latex, counts, depth) in cases {
            assert_eq!(shape(&parse(&latex)), (counts, depth), "all constructs must remain nested, not flatten into siblings");
            let (svg, (w, h)) = to_svg(&latex, 12.0);
            assert!(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0, "{w} {h}");
            assert!(String::from_utf8(svg).unwrap().contains(">z<"), "the innermost content must render");
        }
    }

    /// A capped list consumes nothing; capped root-index/body cursor behavior and the permissive first-`]` grammar remain unchanged.
    #[test]
    fn cap_and_index_returns_preserve_their_exact_cursor_semantics() {
        let chars: Vec<_> = "x".chars().collect();
        let mut p = Parser { s: &chars, i: 0 };
        assert_eq!(p.parse(MAX_NESTING - 1), Node::Group(vec![Node::Sym("x".into(), Class::Ident)]));
        assert_eq!(p.i, 1);
        let mut p = Parser { s: &chars, i: 0 };
        assert_eq!(p.parse(MAX_NESTING), Node::Group(vec![]));
        assert_eq!(p.i, 0, "a capped List does not consume its input");
        // Both the index subparse and radicand still have their entire old depth allowance.
        let source = r"\sqrt[x]{y}z";
        assert_eq!(
            parse_at(source, MAX_NESTING - 4),
            Node::Group(vec![
                Node::Sqrt(
                    Some(Box::new(Node::Group(vec![Node::Sym("x".into(), Class::Ident)]))),
                    Box::new(Node::Group(vec![Node::Sym("y".into(), Class::Ident)]))
                ),
                Node::Sym("z".into(), Class::Ident),
            ])
        );
        // At this starting depth the index and braced body are capped. The outer cursor is
        // restored after ']', then the old non-consuming body cap leaves y/}/z as siblings.
        let chars: Vec<_> = source.chars().collect();
        let mut p = Parser { s: &chars, i: 0 };
        assert_eq!(
            p.parse(MAX_NESTING - 2),
            Node::Group(vec![
                Node::Sqrt(Some(Box::new(Node::Group(vec![]))), Box::new(Node::Group(vec![]))),
                Node::Sym("y".into(), Class::Ident),
                Node::Sym("}".into(), Class::Plain),
                Node::Sym("z".into(), Class::Ident),
            ])
        );
        assert_eq!(p.i, chars.len());
        // Do not change the permissive first-']' index grammar into balanced bracket parsing.
        assert_eq!(
            parse(r"\sqrt[x[y]z]q"),
            Node::Group(vec![
                Node::Sqrt(
                    Some(Box::new(Node::Group(vec![
                        Node::Sym("x".into(), Class::Ident),
                        Node::Sym("[".into(), Class::Plain),
                        Node::Sym("y".into(), Class::Ident)
                    ]))),
                    Box::new(Node::Sym("z".into(), Class::Ident))
                ),
                Node::Sym("]".into(), Class::Plain),
                Node::Sym("q".into(), Class::Ident),
            ])
        );
    }

    /// Repeated scripts replace slots; a fence `^` stays literal; incomplete fractions, escaped braces and unknown commands retain the prior grammar.
    #[test]
    fn heap_continuations_preserve_small_grammar_and_script_precedence() {
        let sym = |s: &str| Node::Sym(s.into(), Class::Ident);
        assert_eq!(
            parse("x^{a}_{b}^{c}_{d}"),
            Node::Group(vec![Node::Scripts {
                base: Box::new(sym("x")),
                sup: Some(Box::new(Node::Group(vec![sym("c")]))),
                sub: Some(Box::new(Node::Group(vec![sym("d")]))),
            }])
        );
        assert_eq!(
            parse(r"\left( ^x \right)"),
            Node::Group(vec![Node::Fenced("(".into(), Box::new(Node::Group(vec![Node::Sym("^".into(), Class::Plain), sym("x")])), ")".into())])
        );
        assert_eq!(parse(r"\frac{x"), Node::Group(vec![Node::Frac(Box::new(Node::Group(vec![sym("x")])), Box::new(Node::Group(vec![])))]));
        assert_eq!(
            parse(r"\text{a{b}}\{\}\_\unknown"),
            Node::Group(vec![
                Node::Text("a{b}".into()),
                Node::Sym("{".into(), Class::Punct),
                Node::Sym("}".into(), Class::Punct),
                Node::Sym("_".into(), Class::Plain),
                Node::Text(r"\unknown".into())
            ])
        );
    }

    /// Iterative layout equals the frozen recursive oracle for representative formulas, including malformed inputs.
    #[test]
    fn heap_layout_preserves_the_existing_element_order_and_geometry() {
        let db = FontDb::global();
        let f = Fonts { upright: db.face(FAMILY, "Regular"), italic: db.face(FAMILY, "Italic"), fallback: db.face(FALLBACK, "Regular") };
        for latex in [
            "E = mc^2",
            "x^{a}_{b}^{c}_{d}",
            r"\frac{a^2+b}{\sqrt[3]{x}}",
            r"\sum_{i=0}^{n} i^2",
            r"\int_a^b x",
            r"\left( x \rightarrow y \right) + \text{if } x",
            r"\left. x \right]",
            r"-x,+y;\quad z",
            r"\sqrt[x[y]z]q",
            r"\frac{x",
            r"\left( ^x \right)",
        ] {
            let tree = parse(latex);
            let actual = layout(&f, &tree, 12.0);
            let expected = recursive_layout_reference(&f, &tree, 12.0);
            assert_eq!((actual.w, actual.asc, actual.desc, actual.els), (expected.w, expected.asc, expected.desc, expected.els), "{latex}");
        }
    }

    /// Build a deep five-way alternating AST for destructor stress tests.
    fn mixed_chain(depth: usize) -> Node {
        let mut node = Node::Sym("z".into(), Class::Ident);
        for i in 0..depth {
            node = match i % 5 {
                0 => Node::Group(vec![node]),
                1 => Node::Frac(Box::new(node), Box::new(Node::Text("d".into()))),
                2 => Node::Sqrt(Some(Box::new(Node::Text("3".into()))), Box::new(node)),
                3 => Node::Scripts { base: Box::new(node), sup: Some(Box::new(Node::Text("p".into()))), sub: Some(Box::new(Node::Text("q".into()))) },
                _ => Node::Fenced("(".into(), Box::new(node), ")".into()),
            };
        }
        node
    }

    /// Drops 50k-node chains, replaces one script slot, and unwinds parser continuations while preserving the retained slot.
    #[test]
    fn deep_nodes_drop_when_replaced_or_owned_by_unwinding_continuations() {
        // Far beyond the parser budget: this directly proves the private destructor's traversal.
        drop(mixed_chain(50_000));
        for up in [false, true] {
            let old = Box::new(mixed_chain(50_000));
            let other = Box::new(Node::Text("other".into()));
            let (sup, sub) = if up { (Some(old), Some(other)) } else { (Some(other), Some(old)) };
            let root = attach_script(Node::Scripts { base: Box::new(Node::Big("∑".into())), sup, sub }, Node::Text("new".into()), up);
            let Node::Scripts { base, sup, sub } = &root else { panic!("script wrapper") };
            assert!(matches!(&**base, Node::Big(s) if s == "∑"));
            let (replaced, retained) = if up { (sup, sub) } else { (sub, sup) };
            assert!(matches!(replaced.as_deref(), Some(Node::Text(s)) if s == "new"));
            assert!(matches!(retained.as_deref(), Some(Node::Text(s)) if s == "other"));
        }
        let result = std::panic::catch_unwind(|| {
            // Actual continuation variants own completed children while awaiting later input.
            let _partial = [
                ParseWork::FractionDenominator { numerator: mixed_chain(25_000) },
                ParseWork::SqrtArgument { index: Some(Box::new(mixed_chain(25_000))) },
            ];
            panic!("exercise partial-parser-state unwinding");
        });
        assert!(result.is_err(), "the intentional panic must actually unwind the owned frames");
    }
}
