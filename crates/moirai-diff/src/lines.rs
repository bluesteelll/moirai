//! Lines of a text value and their dense ids ([F12 §7.5] "Lines").
//!
//! lines(x) splits x after every LF (`0A`): each line is a maximal run of bytes ending with LF, except that a final run
//! without LF is the last line. The empty text has no lines, and concatenating lines(x) gives x. Two lines are equal
//! when their bytes are equal, LF included. The histogram diff works on line ids, so the texts of one comparison (the
//! base, ours and theirs of a diff3) are interned into one id space by one [`Interner`].

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::iter::FusedIterator;

use crate::LengthError;

/// The lines of `text` ([F12 §7.5]).
#[must_use]
pub fn lines(text: &[u8]) -> Lines<'_> {
    Lines { rest: text }
}

/// The iterator of [`lines`]: each item is one line, with its LF when it has one.
#[derive(Clone, Debug)]
pub struct Lines<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.rest.is_empty() {
            return None;
        }
        let cut = self
            .rest
            .iter()
            .position(|&b| b == b'\n')
            .map_or(self.rest.len(), |at| at + 1);
        let (line, rest) = self.rest.split_at(cut);
        self.rest = rest;
        Some(line)
    }
}

impl FusedIterator for Lines<'_> {}

/// The id of the `held`-th distinct line: ids stop below `u32::MAX`.
fn id_for(held: usize) -> Result<u32, LengthError> {
    u32::try_from(held)
        .ok()
        .filter(|&id| id < u32::MAX)
        .ok_or(LengthError)
}

/// Assigns dense ids 0, 1, 2, … to distinct lines, in order of first appearance. The ids index the histogram diff's
/// tables, so its memory stays proportional to the input ([`crate::Differ`]).
///
/// Lines are borrowed, not copied: the texts must outlive the interner. Hashing uses the standard library's keyed
/// hasher, so crafted input cannot force collisions.
#[derive(Clone, Debug, Default)]
pub struct Interner<'a> {
    ids: HashMap<&'a [u8], u32>,
}

impl<'a> Interner<'a> {
    /// An empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of `line`, assigning the next id when it is new. Fails when `u32::MAX` distinct lines are held.
    pub fn intern(&mut self, line: &'a [u8]) -> Result<u32, LengthError> {
        let next = self.ids.len();
        match self.ids.entry(line) {
            Entry::Occupied(held) => Ok(*held.get()),
            Entry::Vacant(slot) => Ok(*slot.insert(id_for(next)?)),
        }
    }

    /// Appends the ids of the lines of `text` ([`lines`]) to `out`.
    pub fn intern_lines(&mut self, text: &'a [u8], out: &mut Vec<u32>) -> Result<(), LengthError> {
        for line in lines(text) {
            out.push(self.intern(line)?);
        }
        Ok(())
    }

    /// The number of distinct lines held; every id is below it.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether no line is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Interner, id_for, lines};
    use crate::LengthError;
    use proptest::prelude::*;

    fn split(text: &[u8]) -> Vec<&[u8]> {
        lines(text).collect()
    }

    #[test]
    fn splits_after_every_lf() {
        assert!(split(b"").is_empty());
        assert_eq!(split(b"a"), [&b"a"[..]]);
        assert_eq!(split(b"a\n"), [&b"a\n"[..]]);
        assert_eq!(split(b"a\nb"), [&b"a\n"[..], b"b"]);
        assert_eq!(split(b"\n\n"), [&b"\n"[..], b"\n"]);
        assert_eq!(split(b"a\r\nb\r\n"), [&b"a\r\n"[..], b"b\r\n"]);
        assert_eq!(split(b"\na"), [&b"\n"[..], b"a"]);
    }

    #[test]
    fn ids_are_dense_in_first_appearance_order() {
        let text = b"x\ny\nx\nz\ny";
        let mut int = Interner::new();
        assert!(int.is_empty());
        let mut ids = Vec::new();
        int.intern_lines(text, &mut ids).unwrap();
        // "y" without LF differs from "y\n".
        assert_eq!(ids, [0, 1, 0, 2, 3]);
        assert_eq!(int.len(), 4);
        assert!(!int.is_empty());
        let other = b"z\nq\n";
        int.intern_lines(other, &mut ids).unwrap();
        assert_eq!(&ids[5..], [2, 4]);
        assert_eq!(int.intern(b"x\n").unwrap(), 0);
    }

    #[test]
    fn ids_stop_below_u32_max() {
        assert_eq!(id_for(0), Ok(0));
        assert_eq!(id_for(u32::MAX as usize - 1), Ok(u32::MAX - 1));
        assert_eq!(id_for(u32::MAX as usize), Err(LengthError));
        assert_eq!(id_for(usize::MAX), Err(LengthError));
    }

    proptest! {
        #![proptest_config(crate::test_config(256))]

        #[test]
        fn lines_concatenate_to_the_text(text in prop::collection::vec(prop_oneof![Just(b'\n'), Just(b'a'), any::<u8>()], 0..200)) {
            let parts = split(&text);
            prop_assert_eq!(parts.concat(), text.clone());
            for (n, part) in parts.iter().enumerate() {
                prop_assert!(!part.is_empty());
                let lf = part.iter().position(|&b| b == b'\n');
                // An LF only ever ends a line, and only the last line may lack one.
                prop_assert!(lf.is_none_or(|at| at + 1 == part.len()));
                prop_assert!(lf.is_some() || n + 1 == parts.len());
            }
        }

        #[test]
        fn equal_ids_iff_equal_lines(text in prop::collection::vec(prop_oneof![Just(b'\n'), 0u8..3], 0..120)) {
            let parts = split(&text);
            let mut int = Interner::new();
            let mut ids = Vec::new();
            int.intern_lines(&text, &mut ids).unwrap();
            prop_assert_eq!(ids.len(), parts.len());
            for a in 0..parts.len() {
                prop_assert!((ids[a] as usize) < int.len());
                for b in 0..parts.len() {
                    prop_assert_eq!(ids[a] == ids[b], parts[a] == parts[b]);
                }
            }
        }
    }
}
