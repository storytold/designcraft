//! The interpreter: stacks, the execution loop and error handling.
//!
//! Execution never recurses on the Rust stack for PostScript control flow: procedures, loops
//! and `stopped` are frames on an explicit execution stack. Operators that must run a procedure
//! and use its result (tint transforms, BuildChar, `pathforall`) call [`Interp::call`], a
//! nested run of the loop with a bounded depth.

use crate::gstate::GState;
use crate::obj::{Key, Obj, PArr, PDict, PStr, name};
use crate::ops::{self, OpId};
use crate::pdf::Out;
use crate::scan;
use crate::source::{Source, Sources};

/// A PostScript error (`typecheck`, …), `stop`, `quit`, or a fatal problem that ends the run.
#[derive(Debug, Clone, PartialEq)]
pub enum PsErr {
    Ps(&'static str),
    Stop,
    Quit,
    Fatal(String),
}

pub type R<T = ()> = Result<T, PsErr>;

pub fn err<T>(name: &'static str) -> R<T> {
    Err(PsErr::Ps(name))
}

#[derive(Debug, Clone)]
pub enum Frame {
    Proc {
        arr: PArr,
        i: usize,
    },
    File {
        id: usize,
    },
    For {
        cur: f64,
        inc: f64,
        limit: f64,
        int: bool,
        proc: Obj,
    },
    Repeat {
        n: i64,
        proc: Obj,
    },
    Loop {
        proc: Obj,
    },
    ForallArr {
        arr: PArr,
        i: usize,
        proc: Obj,
    },
    ForallStr {
        s: PStr,
        i: usize,
        proc: Obj,
    },
    ForallDict {
        items: Vec<(Obj, Obj)>,
        i: usize,
        proc: Obj,
    },
    Stopped,
    /// The bottom of a nested run ([`Interp::call`]).
    Barrier,
}

impl Frame {
    pub fn is_loop(&self) -> bool {
        matches!(
            self,
            Frame::For { .. }
                | Frame::Repeat { .. }
                | Frame::Loop { .. }
                | Frame::ForallArr { .. }
                | Frame::ForallStr { .. }
                | Frame::ForallDict { .. }
        )
    }
}

/// How much work and memory one EPS may use.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub steps: u64,
    pub memory: usize,
}

impl Default for Limits {
    /// Generous for real EPS files: an Illustrator file with its procsets takes about 0.6 million
    /// steps and 2 MB, one carrying a 15 MB CJK font 2 million steps and 30 MB. A program that
    /// runs into the step limit ends within a few seconds. Smaller on the web.
    fn default() -> Self {
        if cfg!(target_arch = "wasm32") { Limits { steps: 20_000_000, memory: 256 << 20 } } else { Limits { steps: 50_000_000, memory: 1 << 30 } }
    }
}

/// Memory a path segment and a graphics state take.
pub const SEG_BYTES: usize = std::mem::size_of::<crate::gstate::Seg>();
pub const GSTATE_BYTES: usize = 512;

/// Content-stream bytes one written path segment takes, about.
pub const OUT_SEG_BYTES: usize = 24;

/// Memory a file takes (charged when one is made).
pub const SOURCE_BYTES: usize = 128;

/// Bytes read, copied or scanned per step of work charged.
pub const BYTES_PER_STEP: u64 = 16;

const MAX_OSTACK: usize = 500_000;
const MAX_ESTACK: usize = 100_000;
const MAX_DSTACK: usize = 1_000;
const MAX_GSTACK: usize = 10_000;
const MAX_CALL_DEPTH: usize = 48;

pub struct Interp {
    pub o: Vec<Obj>,
    pub d: Vec<PDict>,
    pub e: Vec<Frame>,
    pub srcs: Sources,
    pub gs: GState,
    pub gstack: Vec<GState>,
    /// Graphics-state depths that `save` objects restore to.
    pub saves: Vec<usize>,
    pub gstates: Vec<GState>,
    pub systemdict: PDict,
    pub globaldict: PDict,
    pub userdict: PDict,
    pub errordict: PDict,
    pub dollar_error: PDict,
    pub statusdict: PDict,
    pub fontdir: PDict,
    /// Resource categories: name → dictionary of instances.
    pub resources: PDict,
    pub out: Out,
    pub limits: Limits,
    pub steps: u64,
    pub memory: usize,
    /// How many of `srcs.read` have been charged as steps, and of the segments written.
    pub read_charged: u64,
    pub written_charged: u64,
    pub depth: usize,
    pub next_fid: usize,
    pub main: usize,
    pub warnings: Vec<String>,
    /// Tiling pattern cells already written, by pattern and colour.
    pub cells: std::collections::HashMap<(crate::obj::Key, Vec<u64>), usize>,
    /// CMaps by index (CMap dictionaries refer to them).
    pub cmaps: Vec<crate::composite::CMap>,
    /// CFF font sets read by StartData.
    pub cff: Vec<std::rc::Rc<crate::cff::FontSet>>,
    /// Type 42 fonts' TrueType data by FID.
    pub sfnts: std::collections::HashMap<usize, std::rc::Rc<Vec<u8>>>,
    /// `charpath` is running: glyphs add to the path instead of painting.
    pub charpath: bool,
    /// Glyph outlines by font (FID) and glyph name.
    pub glyphs: std::collections::HashMap<(usize, Vec<u8>), std::rc::Rc<crate::type1::Glyph>>,
    /// The last names executed (for error reports).
    pub recent: std::collections::VecDeque<crate::obj::Name>,
}

impl Interp {
    pub fn new(program: Vec<u8>, origin: (f64, f64), limits: Limits) -> Self {
        let mut srcs = Sources { max_read: limits.steps.saturating_mul(BYTES_PER_STEP), ..Sources::default() };
        let main = srcs.add(Source::bytes(program));
        let systemdict = PDict::new(1000);
        let globaldict = PDict::new(100);
        let userdict = PDict::new(200);
        let mut ip = Interp {
            o: Vec::new(),
            d: vec![systemdict.clone(), globaldict.clone(), userdict.clone()],
            e: Vec::new(),
            srcs,
            gs: GState::default(),
            gstack: Vec::new(),
            saves: Vec::new(),
            gstates: Vec::new(),
            systemdict,
            globaldict,
            userdict,
            errordict: PDict::new(40),
            dollar_error: PDict::new(10),
            statusdict: PDict::new(40),
            fontdir: PDict::new(100),
            resources: PDict::new(20),
            out: Out::new(origin),
            limits,
            steps: 0,
            memory: 0,
            read_charged: 0,
            written_charged: 0,
            depth: 0,
            next_fid: 1,
            main,
            warnings: Vec::new(),
            recent: std::collections::VecDeque::new(),
            glyphs: std::collections::HashMap::new(),
            charpath: false,
            sfnts: std::collections::HashMap::new(),
            cmaps: Vec::new(),
            cells: std::collections::HashMap::new(),
            cff: Vec::new(),
        };
        ops::install(&mut ip);
        ip
    }

    // ----- operand stack -----

    pub fn push(&mut self, o: Obj) -> R {
        if self.o.len() >= MAX_OSTACK {
            return err("stackoverflow");
        }
        self.o.push(o);
        Ok(())
    }

    pub fn pop(&mut self) -> R<Obj> {
        self.o.pop().ok_or(PsErr::Ps("stackunderflow"))
    }

    pub fn top(&self, n: usize) -> R<&Obj> {
        let i = self.o.len().checked_sub(n + 1).ok_or(PsErr::Ps("stackunderflow"))?;
        self.o.get(i).ok_or(PsErr::Ps("stackunderflow"))
    }

    pub fn need(&self, n: usize) -> R {
        if self.o.len() < n { err("stackunderflow") } else { Ok(()) }
    }

    pub fn pop_num(&mut self) -> R<f64> {
        match self.pop()? {
            o @ (Obj::Int(_) | Obj::Real(_)) => o.num().ok_or(PsErr::Ps("typecheck")),
            _ => err("typecheck"),
        }
    }

    pub fn pop_int(&mut self) -> R<i64> {
        match self.pop()? {
            Obj::Int(i) => Ok(i),
            Obj::Real(r) if r.fract() == 0.0 && r.abs() < 9.0e15 => Ok(r as i64),
            _ => err("typecheck"),
        }
    }

    pub fn pop_usize(&mut self) -> R<usize> {
        usize::try_from(self.pop_int()?).map_err(|_| PsErr::Ps("rangecheck"))
    }

    pub fn pop_bool(&mut self) -> R<bool> {
        match self.pop()? {
            Obj::Bool(b) => Ok(b),
            _ => err("typecheck"),
        }
    }

    pub fn pop_dict(&mut self) -> R<PDict> {
        match self.pop()? {
            Obj::Dict(d) => Ok(d),
            _ => err("typecheck"),
        }
    }

    pub fn pop_arr(&mut self) -> R<PArr> {
        match self.pop()? {
            Obj::Array(a, _) => Ok(a),
            _ => err("typecheck"),
        }
    }

    pub fn pop_str(&mut self) -> R<PStr> {
        match self.pop()? {
            Obj::Str(s, _) => Ok(s),
            _ => err("typecheck"),
        }
    }

    /// A name, or a string read as one.
    pub fn pop_name(&mut self) -> R<Vec<u8>> {
        match self.pop()? {
            Obj::Name(n, _) => Ok(n.to_vec()),
            Obj::Str(s, _) => Ok(s.bytes()),
            _ => err("typecheck"),
        }
    }

    pub fn pop_matrix(&mut self) -> R<crate::gstate::M> {
        let a = self.pop_arr()?;
        matrix_of(&a)
    }

    /// Charge `units` steps of work (elements visited, or [`BYTES_PER_STEP`] bytes each): what an
    /// operator does in proportion to its operands counts against the step limit, checked as it
    /// goes rather than after.
    pub fn charge(&mut self, units: u64) -> R {
        self.steps = self.steps.saturating_add(units);
        if self.steps > self.limits.steps { Err(PsErr::Fatal("the program runs too long".into())) } else { Ok(()) }
    }

    /// Charge work for `n` bytes handled.
    pub fn charge_bytes(&mut self, n: usize) -> R {
        self.charge((n as u64) / BYTES_PER_STEP + 1)
    }

    /// Charge memory for `n` bytes made (new objects and copies alike; the cap bounds what one
    /// conversion makes in all, not only what it keeps).
    pub fn alloc(&mut self, n: usize) -> R {
        self.memory = self.memory.saturating_add(n);
        if self.memory > self.limits.memory { Err(PsErr::Fatal("out of memory".into())) } else { Ok(()) }
    }

    // ----- dictionaries -----

    pub fn lookup(&self, k: &Key) -> Option<Obj> {
        self.d.iter().rev().find_map(|d| d.get(k))
    }

    pub fn begin(&mut self, d: PDict) -> R {
        if self.d.len() >= MAX_DSTACK {
            return err("dictstackoverflow");
        }
        self.d.push(d);
        Ok(())
    }

    pub fn current_dict(&self) -> R<PDict> {
        self.d.last().cloned().ok_or(PsErr::Ps("dictstackunderflow"))
    }

    // ----- execution -----

    pub fn push_frame(&mut self, f: Frame) -> R {
        if self.e.len() >= MAX_ESTACK {
            return err("execstackoverflow");
        }
        self.e.push(f);
        Ok(())
    }

    /// Execute an object as `exec` does: procedures run, names are looked up, strings and files
    /// are scanned as programs, anything else is pushed.
    pub fn exec_value(&mut self, o: Obj) -> R {
        match o {
            Obj::Array(a, true) => {
                if a.len > 0 {
                    self.push_frame(Frame::Proc { arr: a, i: 0 })?;
                }
                Ok(())
            }
            Obj::Name(n, true) => {
                if self.recent.len() >= 24 {
                    self.recent.pop_front();
                }
                self.recent.push_back(n.clone());
                let v = self.lookup(&Key::Name(n.clone())).ok_or(PsErr::Ps("undefined"));
                match v {
                    Ok(v) => self.exec_found(v),
                    Err(e) => {
                        self.dollar_error.put_name(b"offending", Obj::Name(n, false));
                        Err(e)
                    }
                }
            }
            Obj::Op(op) => self.call_op(op),
            Obj::Str(s, true) => {
                // A string runs as a file reading it in place.
                self.alloc(SOURCE_BYTES)?;
                let id = self.srcs.add(Source::string(&s));
                self.push_frame(Frame::File { id })
            }
            Obj::File(id) => self.push_frame(Frame::File { id }),
            other => self.push(other),
        }
    }

    /// The value of an executable name: executed (procedures run).
    fn exec_found(&mut self, v: Obj) -> R {
        match v {
            Obj::Array(a, true) => {
                if a.len > 0 {
                    self.push_frame(Frame::Proc { arr: a, i: 0 })?;
                }
                Ok(())
            }
            Obj::Op(op) => self.call_op(op),
            // A name bound to another executable name: one more level (bounded by the loop).
            // Files are literal unless `exec` runs them (`cvx exec`).
            o @ (Obj::Name(_, true) | Obj::Str(_, true)) => self.exec_value(o),
            other => self.push(other),
        }
    }

    pub fn call_op(&mut self, op: OpId) -> R {
        let (opname, f) = ops::OPS.get(op as usize).copied().ok_or(PsErr::Ps("undefined"))?;
        match f(self) {
            Err(PsErr::Ps(e)) => {
                self.dollar_error.put_name(b"command", Obj::lit(opname));
                Err(PsErr::Ps(e))
            }
            r => r,
        }
    }

    /// Execute one object met in a procedure body or a program: executable names and operators
    /// run, everything else (procedures included) is pushed.
    fn exec_direct(&mut self, o: Obj) -> R {
        match o {
            Obj::Name(_, true) | Obj::Op(_) => self.exec_value(o),
            other => self.push(other),
        }
    }

    fn step(&mut self) -> R {
        // Bytes read since the last step are work too (decoding included).
        let read = self.srcs.read;
        let fresh = read.saturating_sub(self.read_charged);
        self.read_charged = read;
        self.charge(1 + fresh / BYTES_PER_STEP)?;
        // So is what was written: re-emitted clips included.
        let written = self.out.segments() as u64;
        let fresh = written.saturating_sub(self.written_charged);
        if fresh > 0 {
            self.written_charged = written;
            self.charge(fresh / 4)?;
            self.alloc(usize::try_from(fresh).unwrap_or(usize::MAX).saturating_mul(OUT_SEG_BYTES))?;
        }
        if self.srcs.exhausted {
            return Err(PsErr::Fatal("the program reads too much data".into()));
        }
        let Some(frame) = self.e.last_mut() else { return Ok(()) };
        match frame {
            Frame::Proc { arr, i } => {
                let Some(o) = arr.get(*i) else {
                    self.e.pop();
                    return Ok(());
                };
                *i += 1;
                if *i >= arr.len {
                    // The last element: drop the frame first, so tail recursion doesn't grow it.
                    self.e.pop();
                }
                self.exec_direct(o)
            }
            Frame::File { id } => {
                let id = *id;
                let d = &self.d;
                let mut resolve = |n: &[u8]| d.iter().rev().find_map(|dict| dict.get(&Key::Name(name(n))));
                let before = self.srcs.read;
                let token = scan::next(&mut self.srcs, id, &mut resolve);
                // What the scanner read became objects: memory.
                let scanned = usize::try_from(self.srcs.read.saturating_sub(before)).unwrap_or(usize::MAX);
                self.alloc(scanned)?;
                match token {
                    Ok(Some(o)) => self.exec_direct(o),
                    Ok(None) => {
                        self.e.pop();
                        Ok(())
                    }
                    Err(msg) => {
                        self.e.pop();
                        self.warnings.push(msg);
                        err("syntaxerror")
                    }
                }
            }
            Frame::For { cur, inc, limit, int, proc } => {
                let done = if *inc > 0.0 { *cur > *limit } else { *cur < *limit };
                if done || !cur.is_finite() {
                    self.e.pop();
                    return Ok(());
                }
                let v = if *int { Obj::Int(*cur as i64) } else { Obj::Real(*cur) };
                *cur += *inc;
                let p = proc.clone();
                self.push(v)?;
                self.exec_value(p)
            }
            Frame::Repeat { n, proc } => {
                if *n <= 0 {
                    self.e.pop();
                    return Ok(());
                }
                *n -= 1;
                let p = proc.clone();
                self.exec_value(p)
            }
            Frame::Loop { proc } => {
                let p = proc.clone();
                self.exec_value(p)
            }
            Frame::ForallArr { arr, i, proc } => {
                let Some(v) = arr.get(*i) else {
                    self.e.pop();
                    return Ok(());
                };
                *i += 1;
                let p = proc.clone();
                self.push(v)?;
                self.exec_value(p)
            }
            Frame::ForallStr { s, i, proc } => {
                let Some(v) = s.get(*i) else {
                    self.e.pop();
                    return Ok(());
                };
                *i += 1;
                let p = proc.clone();
                self.push(Obj::Int(i64::from(v)))?;
                self.exec_value(p)
            }
            Frame::ForallDict { items, i, proc } => {
                let Some((k, v)) = items.get(*i).cloned() else {
                    self.e.pop();
                    return Ok(());
                };
                *i += 1;
                let p = proc.clone();
                self.push(k)?;
                self.push(v)?;
                self.exec_value(p)
            }
            Frame::Stopped => {
                self.e.pop();
                self.push(Obj::Bool(false))
            }
            Frame::Barrier => Ok(()),
        }
    }

    /// Run until the execution stack is back to `floor` frames.
    pub fn run_to(&mut self, floor: usize) -> R {
        while self.e.len() > floor {
            if matches!(self.e.last(), Some(Frame::Barrier)) && self.e.len() == floor + 1 {
                break;
            }
            match self.step() {
                Ok(()) => {}
                Err(PsErr::Fatal(m)) => return Err(PsErr::Fatal(m)),
                Err(PsErr::Quit) => return Err(PsErr::Quit),
                Err(e) => {
                    if let PsErr::Ps(name) = &e {
                        self.dollar_error.put_name(b"errorname", Obj::lit(name.as_bytes()));
                        self.dollar_error.put_name(b"newerror", Obj::Bool(true));
                    }
                    // Unwind to the nearest `stopped` above the floor; none: the error is ours to return.
                    match self.e.iter().rposition(|f| matches!(f, Frame::Stopped | Frame::Barrier)) {
                        Some(i) if i >= floor && matches!(self.e.get(i), Some(Frame::Stopped)) => {
                            self.e.truncate(i);
                            self.push(Obj::Bool(true))?;
                        }
                        _ => {
                            self.e.truncate(floor);
                            return Err(e);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Run `proc` to completion (a nested run), for operators that need its results.
    pub fn call(&mut self, proc: Obj) -> R {
        if self.depth >= MAX_CALL_DEPTH {
            return err("limitcheck");
        }
        self.depth += 1;
        let floor = self.e.len();
        let r = self.push_frame(Frame::Barrier).and_then(|()| self.exec_value(proc)).and_then(|()| self.run_to(floor + 1));
        // Drop the barrier (and anything an error left above the floor).
        self.e.truncate(floor);
        self.depth -= 1;
        r
    }

    /// Run the program.
    pub fn run(&mut self) -> Result<(), String> {
        self.e.push(Frame::File { id: self.main });
        match self.run_to(0) {
            Ok(()) | Err(PsErr::Quit) => Ok(()),
            Err(PsErr::Stop) => Ok(()),
            Err(PsErr::Ps(e)) => {
                let cmd = self.dollar_error.get_name(b"command").map(|c| match c {
                    Obj::Name(n, _) => String::from_utf8_lossy(&n).into_owned(),
                    _ => String::new(),
                });
                let offending = match self.dollar_error.get_name(b"offending") {
                    Some(Obj::Name(n, _)) => format!(" ({})", String::from_utf8_lossy(&n)),
                    _ => String::new(),
                };
                let recent: Vec<String> = self.recent.iter().map(|n| String::from_utf8_lossy(n).into_owned()).collect();
                let stack: Vec<String> = self
                    .o
                    .iter()
                    .rev()
                    .take(6)
                    .map(|o| {
                        String::from_utf8_lossy(&crate::ops::basic::text_of(o)).chars().take(24).collect::<String>()
                            + ":"
                            + &String::from_utf8_lossy(o.type_name())
                    })
                    .collect();
                Err(format!(
                    "PostScript error {e} in {}{offending}; after {}; stack {}",
                    cmd.unwrap_or_default(),
                    recent.join(" "),
                    stack.join(" | ")
                ))
            }
            Err(PsErr::Fatal(m)) => Err(m),
        }
    }

    // ----- graphics-state stack -----

    pub fn gsave(&mut self) -> R {
        if self.gstack.len() >= MAX_GSTACK {
            return err("limitcheck");
        }
        self.charge_gstate()?;
        self.gstack.push(self.gs.clone());
        Ok(())
    }

    /// Copying the graphics state copies its path: work and memory in proportion.
    pub fn charge_gstate(&mut self) -> R {
        let n = self.gs.path.len() + self.gs.dash.0.len() + self.gs.clip_saves.len() + self.gs.color.len();
        self.charge(n as u64 / 4 + 1)?;
        self.alloc(n.saturating_mul(SEG_BYTES).saturating_add(GSTATE_BYTES))
    }

    /// Charge work and memory for copying or walking `n` path segments.
    pub fn charge_path(&mut self, n: usize) -> R {
        self.charge(n as u64 / 4 + 1)?;
        self.alloc(n.saturating_mul(SEG_BYTES))
    }

    pub fn grestore(&mut self) {
        // Not below the innermost save.
        let floor = self.saves.last().copied().unwrap_or(0);
        if self.gstack.len() > floor
            && let Some(g) = self.gstack.pop()
        {
            self.gs = g;
        }
    }
}

/// A six-number array as a matrix.
pub fn matrix_of(a: &PArr) -> R<crate::gstate::M> {
    if a.len != 6 {
        return err("rangecheck");
    }
    let mut m = [0.0; 6];
    for (i, slot) in m.iter_mut().enumerate() {
        *slot = a.get(i).and_then(|o| o.num()).ok_or(PsErr::Ps("typecheck"))?;
    }
    Ok(crate::gstate::M(m))
}

pub fn matrix_obj(m: &crate::gstate::M) -> Obj {
    Obj::Array(PArr::new(m.0.iter().map(|v| Obj::Real(*v)).collect()), false)
}
