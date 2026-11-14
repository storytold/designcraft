//! A schema-less protocol buffer reader. iWork's message definitions are not published, so
//! messages are read by field number as they are needed; malformed data ends a message early
//! instead of failing the import.

/// A varint at `pos` and the position after it.
pub(crate) fn varint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *data.get(pos)?;
        pos += 1;
        value |= u64::from(b & 0x7F) << shift;
        if b < 0x80 {
            return Some((value, pos));
        }
    }
    None
}

/// A field value on the wire.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Value<'a> {
    Varint(u64),
    Fixed64([u8; 8]),
    Bytes(&'a [u8]),
    Fixed32([u8; 4]),
}

/// An encoded message.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Msg<'a>(pub &'a [u8]);

/// The fields of a message in order; ends at the first malformed field.
pub(crate) struct Fields<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Iterator for Fields<'a> {
    type Item = (u32, Value<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let (key, p) = varint(self.data, self.pos)?;
        let field = u32::try_from(key >> 3).ok()?;
        let (value, p) = match key & 7 {
            0 => {
                let (v, p) = varint(self.data, p)?;
                (Value::Varint(v), p)
            }
            1 => (Value::Fixed64(self.data.get(p..p + 8)?.try_into().ok()?), p + 8),
            2 => {
                let (len, p) = varint(self.data, p)?;
                let end = p.checked_add(usize::try_from(len).ok()?)?;
                (Value::Bytes(self.data.get(p..end)?), end)
            }
            5 => (Value::Fixed32(self.data.get(p..p + 4)?.try_into().ok()?), p + 4),
            _ => return None,
        };
        self.pos = p;
        Some((field, value))
    }
}

impl<'a> Msg<'a> {
    pub fn fields(self) -> Fields<'a> {
        Fields { data: self.0, pos: 0 }
    }

    /// Every value of field `n`.
    pub fn all(self, n: u32) -> impl Iterator<Item = Value<'a>> {
        self.fields().filter(move |(f, _)| *f == n).map(|(_, v)| v)
    }

    /// The last value of field `n` (the one protobuf readers keep).
    pub fn get(self, n: u32) -> Option<Value<'a>> {
        self.all(n).last()
    }

    pub fn has(self, n: u32) -> bool {
        self.all(n).next().is_some()
    }

    pub fn msg(self, n: u32) -> Option<Msg<'a>> {
        match self.get(n)? {
            Value::Bytes(b) => Some(Msg(b)),
            _ => None,
        }
    }

    pub fn msgs(self, n: u32) -> impl Iterator<Item = Msg<'a>> {
        self.all(n).filter_map(|v| match v {
            Value::Bytes(b) => Some(Msg(b)),
            _ => None,
        })
    }

    pub fn uint(self, n: u32) -> Option<u64> {
        match self.get(n)? {
            Value::Varint(v) => Some(v),
            _ => None,
        }
    }

    pub fn bool(self, n: u32) -> Option<bool> {
        self.uint(n).map(|v| v != 0)
    }

    /// A finite `float` field.
    pub fn f32(self, n: u32) -> Option<f64> {
        match self.get(n)? {
            Value::Fixed32(b) => Some(f64::from(f32::from_le_bytes(b))).filter(|v| v.is_finite()),
            Value::Fixed64(b) => Some(f64::from_le_bytes(b)).filter(|v| v.is_finite()),
            _ => None,
        }
    }

    pub fn bytes(self, n: u32) -> Option<&'a [u8]> {
        match self.get(n)? {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn str(self, n: u32) -> Option<&'a str> {
        std::str::from_utf8(self.bytes(n)?).ok()
    }

    /// A reference to another object (`TSP.Reference`: the identifier in field 1).
    pub fn reference(self, n: u32) -> Option<u64> {
        self.msg(n)?.uint(1)
    }

    /// A point or size message: x/width in field 1, y/height in field 2.
    pub fn pair(self, n: u32) -> Option<(f64, f64)> {
        let m = self.msg(n)?;
        Some((m.f32(1).unwrap_or(0.0), m.f32(2).unwrap_or(0.0)))
    }
}

#[cfg(test)]
pub(crate) mod write {
    //! Encoding for tests.

    pub fn varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    #[derive(Default, Clone)]
    pub struct M(pub Vec<u8>);

    impl M {
        pub fn new() -> Self {
            M(Vec::new())
        }
        pub fn uint(mut self, f: u32, v: u64) -> Self {
            varint(&mut self.0, u64::from(f) << 3);
            varint(&mut self.0, v);
            self
        }
        pub fn f32(mut self, f: u32, v: f32) -> Self {
            varint(&mut self.0, u64::from(f) << 3 | 5);
            self.0.extend_from_slice(&v.to_le_bytes());
            self
        }
        pub fn bytes(mut self, f: u32, b: &[u8]) -> Self {
            varint(&mut self.0, u64::from(f) << 3 | 2);
            varint(&mut self.0, b.len() as u64);
            self.0.extend_from_slice(b);
            self
        }
        pub fn msg(self, f: u32, m: M) -> Self {
            self.bytes(f, &m.0)
        }
        pub fn reference(self, f: u32, id: u64) -> Self {
            self.msg(f, M::new().uint(1, id))
        }
        pub fn pair(self, f: u32, a: f32, b: f32) -> Self {
            self.msg(f, M::new().f32(1, a).f32(2, b))
        }
    }
}
