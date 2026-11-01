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
        let cl =
            if cl == Some(Class::Bin) && (k == 0 || matches!(prev, Some(Class::Bin | Class::Rel | Class::Punct))) { Some(Class::Plain) } else { cl };
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
