//! What the program paints, written as a one-page PDF.
//!
//! Paths arrive in device space (the EPS's default user space). Clipping is applied lazily:
//! before each painting operation the clips in force are matched against those the content
//! stream has applied, popping (`Q`) and pushing (`q … W n`) the difference, so the PostScript
//! program's own gsave/grestore pattern doesn't matter.

use std::rc::Rc;

use pdf_writer::types::{LineCapStyle, LineJoinStyle};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};

use crate::gstate::{Clip, DevColor, M, Seg, clip_chain};

/// The most path segments and images written.
const MAX_SEGMENTS: usize = 20_000_000;
const MAX_IMAGES: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub enum ImgSpace {
    Gray,
    Rgb,
    Cmyk,
    Indexed { base: Box<ImgSpace>, hival: u8, lookup: Vec<u8> },
}

#[derive(Debug)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub bpc: u8,
    /// None for a stencil mask (`imagemask`).
    pub space: Option<ImgSpace>,
    pub decode: Option<Vec<f32>>,
    pub data: Vec<u8>,
    /// `data` is a JPEG (DCTDecode passed through).
    pub dct: bool,
    pub interpolate: bool,
    /// An 8-bit soft mask (w, h, alpha).
    pub smask: Option<(u32, u32, Vec<u8>)>,
}

/// The device space a shading's colours were sampled in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutSpace {
    Gray,
    Rgb,
    Cmyk,
}

impl OutSpace {
    fn comps(self) -> usize {
        match self {
            OutSpace::Gray => 1,
            OutSpace::Rgb => 3,
            OutSpace::Cmyk => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShadeKind {
    Axial([f64; 4]),
    Radial([f64; 6]),
    /// A function of (x, y) over its domain, mapped by `matrix`.
    Function {
        domain: [f64; 4],
        matrix: M,
    },
    /// Vertices in threes; `Coons` and `Tensor` patches of 12 and 16 points.
    Triangles,
    Coons,
    Tensor,
}

/// A smooth shading with its colours sampled: along the axis (`samples` of them), over a
/// `samples` × `samples` grid, or one per vertex or patch corner.
#[derive(Clone, Debug)]
pub struct Shading {
    pub kind: ShadeKind,
    pub extend: [bool; 2],
    pub space: OutSpace,
    pub data: Vec<u8>,
    pub samples: usize,
    pub bbox: Option<[f64; 4]>,
    pub points: Vec<(f64, f64)>,
}

/// A tiling pattern cell: its content (in pattern space), cell box, steps and pattern matrix.
pub struct Tiling {
    content: Vec<u8>,
    bbox: [f64; 4],
    step: (f64, f64),
    matrix: M,
}

/// What a pattern cell's content replaces while it is captured.
pub struct Capture {
    c: Content,
    applied: Vec<Rc<Clip>>,
    state: PaintState,
    saved: Vec<PaintState>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct PaintState {
    fill: Option<DevColor>,
    stroke: Option<DevColor>,
}

pub struct StrokeParams<'a> {
    pub width: f64,
    pub cap: i64,
    pub join: i64,
    pub miter: f64,
    pub dash: &'a (Vec<f64>, f64),
}

pub struct Out {
    c: Content,
    images: Vec<Image>,
    shadings: Vec<Shading>,
    tilings: Vec<Tiling>,
    origin: (f64, f64),
    applied: Vec<Rc<Clip>>,
    state: PaintState,
    saved: Vec<PaintState>,
    segments: usize,
    /// Anything painted at all.
    pub painted: bool,
}

fn f(v: f64) -> f32 {
    if v.is_finite() { v.clamp(-3.0e7, 3.0e7) as f32 } else { 0.0 }
}

impl Out {
    pub fn new(origin: (f64, f64)) -> Self {
        let mut c = Content::new();
        c.transform([1.0, 0.0, 0.0, 1.0, f(-origin.0), f(-origin.1)]);
        Out {
            c,
            images: Vec::new(),
            shadings: Vec::new(),
            tilings: Vec::new(),
            origin,
            applied: Vec::new(),
            state: PaintState::default(),
            saved: Vec::new(),
            segments: 0,
            painted: false,
        }
    }

    fn sync_clip(&mut self, clip: &Option<Rc<Clip>>) -> Result<(), String> {
        let target = clip_chain(clip);
        let common = self.applied.iter().zip(&target).take_while(|(a, b)| Rc::ptr_eq(a, b)).count();
        while self.applied.len() > common {
            self.applied.pop();
            self.c.restore_state();
            self.state = self.saved.pop().unwrap_or_default();
        }
        for node in target.into_iter().skip(common) {
            self.c.save_state();
            self.saved.push(self.state.clone());
            self.path(&node.path)?;
            if node.eo {
                self.c.clip_even_odd();
            } else {
                self.c.clip_nonzero();
            }
            self.c.end_path();
            self.applied.push(node);
        }
        Ok(())
    }

    fn path(&mut self, path: &[Seg]) -> Result<(), String> {
        self.segments = self.segments.saturating_add(path.len());
        if self.segments > MAX_SEGMENTS {
            return Err("too many path segments".into());
        }
        for s in path {
            match *s {
                Seg::Move(x, y) => {
                    self.c.move_to(f(x), f(y));
                }
                Seg::Line(x, y) => {
                    self.c.line_to(f(x), f(y));
                }
                Seg::Curve(a, b, c, d, e, g) => {
                    self.c.cubic_to(f(a), f(b), f(c), f(d), f(e), f(g));
                }
                Seg::Close => {
                    self.c.close_path();
                }
            }
        }
        Ok(())
    }

    fn set_fill(&mut self, color: DevColor) {
        if self.state.fill == Some(color) {
            return;
        }
        match color {
            DevColor::Gray(g) => self.c.set_fill_gray(f(g)),
            DevColor::Rgb(r, g, b) => self.c.set_fill_rgb(f(r), f(g), f(b)),
            DevColor::Cmyk(c, m, y, k) => self.c.set_fill_cmyk(f(c), f(m), f(y), f(k)),
        };
        self.state.fill = Some(color);
    }

    fn set_stroke(&mut self, color: DevColor) {
        if self.state.stroke == Some(color) {
            return;
        }
        match color {
            DevColor::Gray(g) => self.c.set_stroke_gray(f(g)),
            DevColor::Rgb(r, g, b) => self.c.set_stroke_rgb(f(r), f(g), f(b)),
            DevColor::Cmyk(c, m, y, k) => self.c.set_stroke_cmyk(f(c), f(m), f(y), f(k)),
        };
        self.state.stroke = Some(color);
    }

    pub fn fill(&mut self, clip: &Option<Rc<Clip>>, path: &[Seg], eo: bool, color: DevColor) -> Result<(), String> {
        if !path.iter().any(|s| !matches!(s, Seg::Move(..))) {
            return Ok(());
        }
        self.sync_clip(clip)?;
        self.set_fill(color);
        self.path(path)?;
        if eo {
            self.c.fill_even_odd();
        } else {
            self.c.fill_nonzero();
        }
        self.painted = true;
        Ok(())
    }

    /// Stroke a device-space path drawn under `ctm` (the pen is shaped in user space).
    pub fn stroke(&mut self, clip: &Option<Rc<Clip>>, path: &[Seg], ctm: &M, p: &StrokeParams, color: DevColor) -> Result<(), String> {
        if !path.iter().any(|s| !matches!(s, Seg::Move(..))) {
            return Ok(());
        }
        let Some(inv) = ctm.inv() else { return Ok(()) };
        self.sync_clip(clip)?;
        self.set_stroke(color);
        self.c.save_state();
        self.c.transform(ctm.0.map(f));
        let user: Vec<Seg> = path.iter().map(|s| s.map(&inv)).collect();
        self.c.set_line_width(f(p.width.max(0.0)));
        self.c.set_line_cap(match p.cap {
            1 => LineCapStyle::RoundCap,
            2 => LineCapStyle::ProjectingSquareCap,
            _ => LineCapStyle::ButtCap,
        });
        self.c.set_line_join(match p.join {
            1 => LineJoinStyle::RoundJoin,
            2 => LineJoinStyle::BevelJoin,
            _ => LineJoinStyle::MiterJoin,
        });
        self.c.set_miter_limit(f(p.miter.max(1.0)));
        if crate::gstate::dashes_ok(&user, &p.dash.0) {
            self.c.set_dash_pattern(p.dash.0.iter().map(|d| f(*d)), f(p.dash.1));
        }
        self.path(&user)?;
        self.c.stroke();
        self.c.restore_state();
        self.painted = true;
        Ok(())
    }

    /// Paint an image: `cm` maps the unit square onto the page; stencil masks use `fill`.
    pub fn image(&mut self, clip: &Option<Rc<Clip>>, cm: &M, img: Image, fill: Option<DevColor>) -> Result<(), String> {
        if self.images.len() >= MAX_IMAGES {
            return Err("too many images".into());
        }
        if !cm.finite() || img.w == 0 || img.h == 0 {
            return Ok(());
        }
        self.sync_clip(clip)?;
        if let Some(color) = fill {
            self.set_fill(color);
        }
        let n = self.images.len();
        self.images.push(img);
        self.c.save_state();
        self.c.transform(cm.0.map(f));
        let name = format!("Im{n}");
        self.c.x_object(Name(name.as_bytes()));
        self.c.restore_state();
        self.painted = true;
        Ok(())
    }

    /// Start a pattern cell: painting goes to its own content until [`Out::end_cell`].
    pub fn begin_cell(&mut self) -> Capture {
        Capture {
            c: std::mem::replace(&mut self.c, Content::new()),
            applied: std::mem::take(&mut self.applied),
            state: std::mem::take(&mut self.state),
            saved: std::mem::take(&mut self.saved),
        }
    }

    /// Finish a pattern cell: its pattern number.
    pub fn end_cell(&mut self, cap: Capture, bbox: [f64; 4], step: (f64, f64), matrix: M) -> Result<usize, String> {
        let mut cell = std::mem::replace(&mut self.c, cap.c);
        for _ in 0..self.applied.len() {
            cell.restore_state();
        }
        self.applied = cap.applied;
        self.state = cap.state;
        self.saved = cap.saved;
        if self.tilings.len() >= MAX_IMAGES {
            return Err("too many patterns".into());
        }
        // Pattern space maps to the page's default space, which the content moved to the origin.
        let matrix = matrix.then(&M([1.0, 0.0, 0.0, 1.0, -self.origin.0, -self.origin.1]));
        self.tilings.push(Tiling { content: cell.finish().into_vec(), bbox, step, matrix });
        Ok(self.tilings.len() - 1)
    }

    /// Fill a device-space path with tiling pattern `n`.
    pub fn fill_tiling(&mut self, clip: &Option<Rc<Clip>>, path: &[Seg], eo: bool, n: usize) -> Result<(), String> {
        if !path.iter().any(|s| !matches!(s, Seg::Move(..))) {
            return Ok(());
        }
        self.sync_clip(clip)?;
        self.c.set_fill_color_space(pdf_writer::types::ColorSpaceOperand::Pattern);
        let name = format!("P{n}");
        self.c.set_fill_pattern(None, Name(name.as_bytes()));
        self.state.fill = None;
        self.path(path)?;
        if eo {
            self.c.fill_even_odd();
        } else {
            self.c.fill_nonzero();
        }
        self.painted = true;
        Ok(())
    }

    /// Paint a shading in shading space `m` (inside `path` too, for a shading pattern fill).
    pub fn shade(&mut self, clip: &Option<Rc<Clip>>, path: Option<&[Seg]>, eo: bool, m: &M, s: Shading) -> Result<(), String> {
        if self.shadings.len() >= MAX_IMAGES {
            return Err("too many shadings".into());
        }
        if !m.finite() || m.inv().is_none() {
            return Ok(());
        }
        self.sync_clip(clip)?;
        self.c.save_state();
        if let Some(p) = path {
            self.path(p)?;
            if eo {
                self.c.clip_even_odd();
            } else {
                self.c.clip_nonzero();
            }
            self.c.end_path();
        }
        self.c.transform(m.0.map(f));
        if let Some([x0, y0, x1, y1]) = s.bbox {
            self.c.rect(f(x0), f(y0), f(x1 - x0), f(y1 - y0));
            self.c.clip_nonzero();
            self.c.end_path();
        }
        let name = format!("Sh{}", self.shadings.len());
        self.shadings.push(s);
        self.c.shading(Name(name.as_bytes()));
        self.c.restore_state();
        self.painted = true;
        Ok(())
    }

    /// The PDF: one page of `size`, its content, images and shadings.
    /// Path segments written so far (clips included).
    pub fn segments(&self) -> usize {
        self.segments
    }

    pub fn finish(self, size: (f64, f64)) -> Vec<u8> {
        let mut pdf = Pdf::new();
        // Nothing newer is used (no transparency groups): PDF/X-1a files can take it.
        pdf.set_version(1, 4);
        let (catalog, pages, page, content) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
        let mut next = 5;
        let mut alloc = || {
            let r = Ref::new(next);
            next += 1;
            r
        };
        let image_refs: Vec<(Ref, Option<Ref>)> = self.images.iter().map(|i| (alloc(), i.smask.as_ref().map(|_| alloc()))).collect();
        let shading_refs: Vec<(Ref, Ref)> = self.shadings.iter().map(|_| (alloc(), alloc())).collect();
        let tiling_refs: Vec<Ref> = self.tilings.iter().map(|_| alloc()).collect();
        // Every resource is listed for the page and for each pattern cell (names are global).
        let resources = |res: &mut pdf_writer::writers::Resources<'_>| {
            let mut xo = res.x_objects();
            for (i, (r, _)) in image_refs.iter().enumerate() {
                let name = format!("Im{i}");
                xo.pair(Name(name.as_bytes()), *r);
            }
            xo.finish();
            let mut sh = res.shadings();
            for (i, (r, _)) in shading_refs.iter().enumerate() {
                let name = format!("Sh{i}");
                sh.pair(Name(name.as_bytes()), *r);
            }
            sh.finish();
            let mut pa = res.patterns();
            for (i, r) in tiling_refs.iter().enumerate() {
                let name = format!("P{i}");
                pa.pair(Name(name.as_bytes()), *r);
            }
            pa.finish();
        };
        pdf.catalog(catalog).pages(pages);
        pdf.pages(pages).kids([page]).count(1);
        {
            let mut p = pdf.page(page);
            p.parent(pages);
            p.media_box(Rect::new(0.0, 0.0, f(size.0.max(1.0)), f(size.1.max(1.0))));
            p.contents(content);
            let mut res = p.resources();
            resources(&mut res);
            res.finish();
            p.finish();
        }
        let mut c = self.c;
        for _ in 0..self.applied.len() {
            c.restore_state();
        }
        let raw = c.finish().into_vec();
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        pdf.stream(content, &z).filter(Filter::FlateDecode);
        for (img, (r, smask)) in self.images.iter().zip(&image_refs) {
            let packed;
            let (data, filter) = if img.dct {
                (&img.data[..], Filter::DctDecode)
            } else {
                packed = miniz_oxide::deflate::compress_to_vec_zlib(&img.data, 6);
                (&packed[..], Filter::FlateDecode)
            };
            let mut x = pdf.image_xobject(*r, data);
            x.filter(filter);
            x.width(i32::try_from(img.w).unwrap_or(i32::MAX));
            x.height(i32::try_from(img.h).unwrap_or(i32::MAX));
            match &img.space {
                None => {
                    x.image_mask(true);
                }
                Some(space) => {
                    x.bits_per_component(i32::from(img.bpc));
                    match space {
                        ImgSpace::Gray => x.color_space().device_gray(),
                        ImgSpace::Rgb => x.color_space().device_rgb(),
                        ImgSpace::Cmyk => x.color_space().device_cmyk(),
                        ImgSpace::Indexed { base, hival, lookup } => {
                            let base = match **base {
                                ImgSpace::Rgb => Name(b"DeviceRGB"),
                                ImgSpace::Cmyk => Name(b"DeviceCMYK"),
                                _ => Name(b"DeviceGray"),
                            };
                            x.color_space().indexed(base, i32::from(*hival), lookup);
                        }
                    }
                }
            }
            if img.space.is_none() {
                x.bits_per_component(1);
            }
            if let Some(d) = &img.decode {
                x.decode(d.iter().copied());
            }
            if img.interpolate {
                x.interpolate(true);
            }
            if let Some(m) = smask {
                x.s_mask(*m);
            }
            x.finish();
            if let (Some(m), Some((w, h, alpha))) = (smask, &img.smask) {
                let z = miniz_oxide::deflate::compress_to_vec_zlib(alpha, 6);
                let mut s = pdf.image_xobject(*m, &z);
                s.filter(Filter::FlateDecode);
                s.width(i32::try_from(*w).unwrap_or(i32::MAX));
                s.height(i32::try_from(*h).unwrap_or(i32::MAX));
                s.color_space().device_gray();
                s.bits_per_component(8);
                s.finish();
            }
        }
        for (s, (r, fr)) in self.shadings.iter().zip(&shading_refs) {
            write_shading(&mut pdf, s, *r, *fr);
        }
        for (t, r) in self.tilings.iter().zip(&tiling_refs) {
            let z = miniz_oxide::deflate::compress_to_vec_zlib(&t.content, 6);
            let mut p = pdf.tiling_pattern(*r, &z);
            p.filter(Filter::FlateDecode);
            p.paint_type(pdf_writer::types::PaintType::Colored);
            p.tiling_type(pdf_writer::types::TilingType::ConstantSpacing);
            let [x0, y0, x1, y1] = t.bbox;
            p.bbox(Rect::new(f(x0), f(y0), f(x1), f(y1)));
            p.x_step(f(t.step.0));
            p.y_step(f(t.step.1));
            p.matrix(t.matrix.0.map(f));
            let mut res = p.resources();
            resources(&mut res);
            res.finish();
            p.finish();
        }
        pdf.finish()
    }
}

fn set_space(cs: pdf_writer::writers::ColorSpace<'_>, s: OutSpace) {
    match s {
        OutSpace::Gray => cs.device_gray(),
        OutSpace::Rgb => cs.device_rgb(),
        OutSpace::Cmyk => cs.device_cmyk(),
    }
}

fn write_shading(pdf: &mut Pdf, s: &Shading, r: Ref, fr: Ref) {
    use pdf_writer::types::FunctionShadingType;
    use pdf_writer::writers::StreamShadingType;
    let n = s.space.comps();
    let range: Vec<f32> = (0..n).flat_map(|_| [0.0, 1.0]).collect();
    let samples = i32::try_from(s.samples.max(2)).unwrap_or(2);
    match &s.kind {
        ShadeKind::Axial(_) | ShadeKind::Radial(_) | ShadeKind::Function { .. } => {
            {
                let mut func = pdf.sampled_function(fr, &s.data);
                if let ShadeKind::Function { domain, .. } = &s.kind {
                    func.domain(domain.map(f));
                    func.size([samples, samples]);
                } else {
                    func.domain([0.0, 1.0]);
                    func.size([samples]);
                }
                func.range(range.iter().copied());
                func.bits_per_sample(8);
            }
            let mut sh = pdf.function_shading(r);
            match &s.kind {
                ShadeKind::Axial(c) => {
                    sh.shading_type(FunctionShadingType::Axial);
                    sh.coords(c.map(f));
                    sh.extend(s.extend);
                }
                ShadeKind::Radial(c) => {
                    sh.shading_type(FunctionShadingType::Radial);
                    sh.coords(c.map(f));
                    sh.extend(s.extend);
                }
                ShadeKind::Function { domain, matrix } => {
                    sh.shading_type(FunctionShadingType::Function);
                    sh.domain(domain.map(f));
                    sh.matrix(matrix.0.map(f));
                }
                _ => {}
            }
            set_space(sh.color_space(), s.space);
            sh.function(fr);
        }
        ShadeKind::Triangles | ShadeKind::Coons | ShadeKind::Tensor => {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (x, y) in &s.points {
                x0 = x0.min(*x);
                y0 = y0.min(*y);
                x1 = x1.max(*x);
                y1 = y1.max(*y);
            }
            if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
                return;
            }
            let (x1, y1) = (x1.max(x0 + 1e-3), y1.max(y0 + 1e-3));
            let q = |v: f64, lo: f64, hi: f64| (((v - lo) / (hi - lo)).clamp(0.0, 1.0) * f64::from(u32::MAX)).round() as u32;
            let mut data = Vec::new();
            let (per, cols) = match s.kind {
                ShadeKind::Triangles => (1, 1),
                ShadeKind::Coons => (12, 4),
                _ => (16, 4),
            };
            let colors: Vec<&[u8]> = s.data.chunks(n).collect();
            for (i, chunk) in s.points.chunks(per).enumerate() {
                if chunk.len() != per {
                    break;
                }
                data.push(0u8);
                for (x, y) in chunk {
                    data.extend(q(*x, x0, x1).to_be_bytes());
                    data.extend(q(*y, y0, y1).to_be_bytes());
                }
                for k in 0..cols {
                    data.extend_from_slice(colors.get(i * cols + k).copied().unwrap_or(&[]));
                }
            }
            let z = miniz_oxide::deflate::compress_to_vec_zlib(&data, 6);
            let mut sh = pdf.stream_shading(r, &z);
            sh.filter(Filter::FlateDecode);
            sh.shading_type(match s.kind {
                ShadeKind::Triangles => StreamShadingType::FreeformGouraud,
                ShadeKind::Coons => StreamShadingType::CoonsPatch,
                _ => StreamShadingType::TensorProductPatch,
            });
            set_space(sh.color_space(), s.space);
            sh.bits_per_coordinate(32);
            sh.bits_per_component(8);
            sh.bits_per_flag(8);
            let mut decode = vec![f(x0), f(x1), f(y0), f(y1)];
            decode.extend(range);
            sh.decode(decode);
        }
    }
}
