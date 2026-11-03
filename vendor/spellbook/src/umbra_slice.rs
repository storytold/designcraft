//! Safe owned storage for Spellbook stems and flag sets.
//!
//! Retains the upstream private API, compact handles, and unit tests.
//! Standard owned slices replace raw allocations, unions and manual auto traits.
use crate::alloc::{boxed::Box, string::String, vec::Vec};
use crate::Flag;
use core::{borrow::Borrow, fmt, hash, mem::size_of};

const fn prefix_len<T>() -> usize {
    if size_of::<T>() == 0 {
        0
    } else {
        (size_of::<usize>() - size_of::<u16>()) / size_of::<T>()
    }
}
const fn suffix_len<T>() -> usize {
    if size_of::<T>() == 0 {
        0
    } else {
        size_of::<usize>() / size_of::<T>()
    }
}
pub type FlagSlice = UmbraSlice<Flag, { prefix_len::<Flag>() }, { suffix_len::<Flag>() }>;
#[derive(Clone, PartialEq, Eq)]
pub struct UmbraSlice<T: Copy, const PREFIX_LEN: usize, const SUFFIX_LEN: usize>(Box<[T]>);
impl<T: Copy, const PREFIX_LEN: usize, const SUFFIX_LEN: usize> UmbraSlice<T, PREFIX_LEN, SUFFIX_LEN> {
    pub fn from_vec(values: Vec<T>) -> Self {
        Self(values.into_boxed_slice())
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}
impl<T: Copy + Ord, const PREFIX_LEN: usize, const SUFFIX_LEN: usize> UmbraSlice<T, PREFIX_LEN, SUFFIX_LEN> {
    pub fn sorted_contains(&self, value: &T) -> bool {
        if self.len() <= PREFIX_LEN + SUFFIX_LEN {
            self.0.contains(value)
        } else {
            self.0.binary_search(value).is_ok()
        }
    }
}
impl<T: Copy, const PREFIX_LEN: usize, const SUFFIX_LEN: usize> Default for UmbraSlice<T, PREFIX_LEN, SUFFIX_LEN> {
    fn default() -> Self {
        Self::from_vec(Vec::new())
    }
}
#[derive(Debug)]
pub struct TooLongError;
impl fmt::Display for TooLongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("input slice was too long (longer than u16::MAX)")
    }
}
impl<T: Copy, const PREFIX_LEN: usize, const SUFFIX_LEN: usize> TryFrom<&[T]> for UmbraSlice<T, PREFIX_LEN, SUFFIX_LEN> {
    type Error = TooLongError;
    fn try_from(source: &[T]) -> Result<Self, Self::Error> {
        if source.len() > u16::MAX as usize {
            return Err(TooLongError);
        }
        Ok(Self(source.into()))
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct UmbraString(Box<str>);
impl UmbraString {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for UmbraString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl Borrow<str> for UmbraString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
impl hash::Hash for UmbraString {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}
impl PartialEq<str> for UmbraString {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
impl fmt::Debug for UmbraString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}
impl From<&str> for UmbraString {
    fn from(s: &str) -> Self {
        Self(s.into())
    }
}
impl From<String> for UmbraString {
    fn from(s: String) -> Self {
        Self(s.into_boxed_str())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    type U16UmbraSlice = UmbraSlice<u16, { prefix_len::<u16>() }, { suffix_len::<u16>() }>;

    #[test]
    fn sizing() {
        use crate::alloc::boxed::Box;

        assert_eq!(size_of::<U16UmbraSlice>(), 2 * size_of::<usize>());
        assert_eq!(size_of::<UmbraString>(), 2 * size_of::<usize>());
        assert_eq!(size_of::<Box<str>>(), 2 * size_of::<usize>());
        assert_eq!(size_of::<Box<[crate::Flag]>>(), 2 * size_of::<usize>());
    }

    impl fmt::Debug for U16UmbraSlice {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.as_slice().fmt(f)
        }
    }

    #[test]
    fn eq() {
        assert_eq!(U16UmbraSlice::try_from([1u16].as_slice()).unwrap(), U16UmbraSlice::try_from([1u16].as_slice()).unwrap(),);
        assert_eq!(UmbraString::from(""), UmbraString::from(""));
        assert_eq!(UmbraString::from("foo"), UmbraString::from("foo"),);
    }

    #[test]
    fn partial_eq_str() {
        // Small (prefix)
        assert_eq!(UmbraString::from("hello"), *"hello");
        // Medium (prefix+suffix)
        assert_eq!(UmbraString::from("foobarbaz"), *"foobarbaz");
        // Large (allocated buffer)
        assert_eq!(UmbraString::from("hellohellohello"), *"hellohellohello");
    }

    #[test]
    fn clone() {
        let s = UmbraString::from("hellohellohello");
        assert_eq!(s.clone(), s);
        assert_eq!(s.clone(), *"hellohellohello");

        let s = UmbraString::from("foo");
        assert_eq!(s.clone(), s);
        assert_eq!(s.clone(), *"foo");
    }

    #[test]
    fn hash() {
        use core::hash::{BuildHasher, Hash, Hasher};
        fn hash_once<T: Hash, S: BuildHasher>(hash_builder: &S, val: &T) -> u64 {
            let mut state = hash_builder.build_hasher();
            val.hash(&mut state);
            state.finish()
        }
        let hasher = crate::DefaultHashBuilder::default();

        let s = UmbraString::from("hellohellohello");
        assert_eq!(hash_once(&hasher, &s), hash_once(&hasher, &s));
        assert_eq!(hash_once(&hasher, &s), hash_once(&hasher, &s.clone()));
        assert_eq!(hash_once(&hasher, &s), hash_once(&hasher, &UmbraString::from("hellohellohello")));
        assert_eq!(hash_once(&hasher, &s), hash_once(&hasher, &"hellohellohello"));
    }
}
