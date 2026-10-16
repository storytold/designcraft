//! PostScript objects. Composite objects (strings, arrays, dictionaries) share their storage the
//! way the language requires: `dup` copies the reference, `getinterval` a window onto the same
//! values.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ops::OpId;

/// A name: its bytes, shared.
pub type Name = Rc<[u8]>;

pub fn name(s: &[u8]) -> Name {
    Rc::from(s)
}

#[derive(Clone, Debug)]
pub struct PStr {
    pub buf: Rc<RefCell<Vec<u8>>>,
    pub off: usize,
    pub len: usize,
}

impl PStr {
    pub fn new(bytes: Vec<u8>) -> Self {
        let len = bytes.len();
        PStr { buf: Rc::new(RefCell::new(bytes)), off: 0, len }
    }

    /// The bytes in view (empty when the window no longer fits its storage).
    pub fn bytes(&self) -> Vec<u8> {
        let b = self.buf.borrow();
        self.off.checked_add(self.len).and_then(|end| b.get(self.off..end)).map(<[u8]>::to_vec).unwrap_or_default()
    }

    pub fn get(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        self.buf.borrow().get(self.off.checked_add(i)?).copied()
    }

    pub fn set(&self, i: usize, v: u8) -> bool {
        if i >= self.len {
            return false;
        }
        let Some(at) = self.off.checked_add(i) else { return false };
        match self.buf.borrow_mut().get_mut(at) {
            Some(slot) => {
                *slot = v;
                true
            }
            None => false,
        }
    }

    pub fn interval(&self, start: usize, count: usize) -> Option<PStr> {
        (start.checked_add(count)? <= self.len).then(|| PStr { buf: self.buf.clone(), off: self.off + start, len: count })
    }

    /// Copy `src` into the start of this string: the written window, or None when it doesn't fit.
    pub fn put_bytes(&self, at: usize, src: &[u8]) -> Option<PStr> {
        if at.checked_add(src.len())? > self.len {
            return None;
        }
        let mut b = self.buf.borrow_mut();
        let start = self.off.checked_add(at)?;
        b.get_mut(start..start.checked_add(src.len())?)?.copy_from_slice(src);
        Some(PStr { buf: self.buf.clone(), off: start, len: src.len() })
    }
}

#[derive(Clone, Debug)]
pub struct PArr {
    pub buf: Rc<RefCell<Vec<Obj>>>,
    pub off: usize,
    pub len: usize,
}

impl PArr {
    pub fn new(v: Vec<Obj>) -> Self {
        let len = v.len();
        PArr { buf: Rc::new(RefCell::new(v)), off: 0, len }
    }

    pub fn get(&self, i: usize) -> Option<Obj> {
        if i >= self.len {
            return None;
        }
        self.buf.borrow().get(self.off.checked_add(i)?).cloned()
    }

    pub fn set(&self, i: usize, v: Obj) -> bool {
        if i >= self.len {
            return false;
        }
        let Some(at) = self.off.checked_add(i) else { return false };
        match self.buf.borrow_mut().get_mut(at) {
            Some(slot) => {
                *slot = v;
                true
            }
            None => false,
        }
    }

    pub fn items(&self) -> Vec<Obj> {
        let b = self.buf.borrow();
        self.off.checked_add(self.len).and_then(|end| b.get(self.off..end)).map(<[Obj]>::to_vec).unwrap_or_default()
    }

    pub fn interval(&self, start: usize, count: usize) -> Option<PArr> {
        (start.checked_add(count)? <= self.len).then(|| PArr { buf: self.buf.clone(), off: self.off + start, len: count })
    }

    pub fn same(&self, o: &PArr) -> bool {
        Rc::ptr_eq(&self.buf, &o.buf) && self.off == o.off && self.len == o.len
    }
}

/// A dictionary key: names (strings become names), integers (integral reals too), booleans, and
/// anything else by identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Name(Name),
    Int(i64),
    Real(u64),
    Bool(bool),
    Null,
    Ident(usize),
}

#[derive(Debug, Default)]
pub struct DictInner {
    pub map: HashMap<Key, (Obj, Obj)>,
    pub cap: usize,
}

#[derive(Clone, Debug)]
pub struct PDict(pub Rc<RefCell<DictInner>>);

impl PDict {
    pub fn new(cap: usize) -> Self {
        PDict(Rc::new(RefCell::new(DictInner { map: HashMap::new(), cap })))
    }

    pub fn get(&self, k: &Key) -> Option<Obj> {
        self.0.borrow().map.get(k).map(|(_, v)| v.clone())
    }

    pub fn get_name(&self, n: &[u8]) -> Option<Obj> {
        self.get(&Key::Name(name(n)))
    }

    pub fn put(&self, key: Obj, v: Obj) {
        let k = key.key();
        self.0.borrow_mut().map.insert(k, (key, v));
    }

    pub fn put_name(&self, n: &[u8], v: Obj) {
        self.put(Obj::Name(name(n), false), v);
    }

    pub fn remove(&self, k: &Key) {
        self.0.borrow_mut().map.remove(k);
    }

    pub fn len(&self) -> usize {
        self.0.borrow().map.len()
    }

    pub fn known(&self, k: &Key) -> bool {
        self.0.borrow().map.contains_key(k)
    }

    pub fn entries(&self) -> Vec<(Obj, Obj)> {
        self.0.borrow().map.values().cloned().collect()
    }

    pub fn same(&self, o: &PDict) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }

    fn ident(&self) -> usize {
        Rc::as_ptr(&self.0) as *const u8 as usize
    }
}

/// An open file: an index into the interpreter's sources.
pub type FileId = usize;

#[derive(Clone, Debug)]
pub enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    /// A name; `true` when executable.
    Name(Name, bool),
    Str(PStr, bool),
    Array(PArr, bool),
    Dict(PDict),
    Mark,
    Op(OpId),
    File(FileId),
    /// A `save` snapshot: the graphics-state depth it restores to.
    Save(usize),
    /// A font identifier (`FID`).
    FontId(usize),
    /// A saved graphics state (`gstate`).
    GState(usize),
}

impl Obj {
    pub fn key(&self) -> Key {
        match self {
            Obj::Name(n, _) => Key::Name(n.clone()),
            Obj::Str(s, _) => Key::Name(name(&s.bytes())),
            Obj::Int(i) => Key::Int(*i),
            Obj::Real(r) if r.fract() == 0.0 && r.abs() < 9.0e15 => Key::Int(*r as i64),
            Obj::Real(r) => Key::Real(r.to_bits()),
            Obj::Bool(b) => Key::Bool(*b),
            Obj::Null => Key::Null,
            Obj::Array(a, _) => Key::Ident(Rc::as_ptr(&a.buf) as *const u8 as usize ^ a.off),
            Obj::Dict(d) => Key::Ident(d.ident()),
            Obj::Op(o) => Key::Ident(0x4000_0000 + *o as usize),
            Obj::File(f) => Key::Ident(0x5000_0000 + f),
            Obj::FontId(f) => Key::Ident(0x6000_0000 + f),
            Obj::Mark => Key::Ident(1),
            Obj::Save(s) => Key::Ident(0x7000_0000 + s),
            Obj::GState(g) => Key::Ident(0x7800_0000 + g),
        }
    }

    pub fn type_name(&self) -> &'static [u8] {
        match self {
            Obj::Null => b"nulltype",
            Obj::Bool(_) => b"booleantype",
            Obj::Int(_) => b"integertype",
            Obj::Real(_) => b"realtype",
            Obj::Name(..) => b"nametype",
            Obj::Str(..) => b"stringtype",
            Obj::Array(..) => b"arraytype",
            Obj::Dict(_) => b"dicttype",
            Obj::Mark => b"marktype",
            Obj::Op(_) => b"operatortype",
            Obj::File(_) => b"filetype",
            Obj::Save(_) => b"savetype",
            Obj::FontId(_) => b"fonttype",
            Obj::GState(_) => b"gstatetype",
        }
    }

    pub fn is_exec(&self) -> bool {
        match self {
            Obj::Name(_, x) | Obj::Str(_, x) | Obj::Array(_, x) => *x,
            Obj::Op(_) => true,
            _ => false,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }

    pub fn str(s: &[u8]) -> Obj {
        Obj::Str(PStr::new(s.to_vec()), false)
    }

    pub fn lit(n: &[u8]) -> Obj {
        Obj::Name(name(n), false)
    }

    pub fn proc(v: Vec<Obj>) -> Obj {
        Obj::Array(PArr::new(v), true)
    }

    /// Equality as `eq` sees it: numbers by value, strings by contents, names and strings
    /// interchangeably, composites by identity.
    pub fn ps_eq(&self, o: &Obj) -> bool {
        match (self, o) {
            (a, b) if a.num().is_some() && b.num().is_some() => a.num() == b.num(),
            (Obj::Str(a, _), Obj::Str(b, _)) => a.bytes() == b.bytes(),
            (Obj::Name(a, _), Obj::Name(b, _)) => a == b,
            (Obj::Name(a, _), Obj::Str(b, _)) | (Obj::Str(b, _), Obj::Name(a, _)) => **a == *b.bytes(),
            (Obj::Bool(a), Obj::Bool(b)) => a == b,
            (Obj::Null, Obj::Null) | (Obj::Mark, Obj::Mark) => true,
            (Obj::Array(a, _), Obj::Array(b, _)) => a.same(b),
            (Obj::Dict(a), Obj::Dict(b)) => a.same(b),
            (Obj::Op(a), Obj::Op(b)) => a == b,
            (Obj::File(a), Obj::File(b)) | (Obj::FontId(a), Obj::FontId(b)) | (Obj::Save(a), Obj::Save(b)) => a == b,
            (Obj::GState(a), Obj::GState(b)) => a == b,
            _ => false,
        }
    }
}
