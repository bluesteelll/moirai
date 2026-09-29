//! Git objects of the image ([F14 §3.2], §10.1; [git-objects]): the object id of a blob, tree or commit, the bytes of
//! a tree object, and git's entry order.

use crate::prim::{Algo, Oid};
use crate::sha::{Sha1, Sha256};

/// The object id of a git object of `kind` (`blob`, `tree`, `commit`) with `content`: the destination's hash of
/// `<kind> SP <decimal length> NUL content`, hashed as it streams (the content is not copied).
pub fn object_id(kind: &str, content: &[u8], algo: Algo) -> Oid {
    let header = format!("{kind} {}\0", content.len());
    match algo {
        Algo::Sha1 => {
            let mut h = Sha1::new();
            h.update(header.as_bytes());
            h.update(content);
            Oid::Sha1(h.finish())
        }
        Algo::Sha256 => {
            let mut h = Sha256::new();
            h.update(header.as_bytes());
            h.update(content);
            Oid::Sha256(h.finish())
        }
    }
}

/// One entry of a tree object: the mode text (`100644` or `40000`, [F14 §3.2]), the name and the object id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    /// `100644` for a blob, `40000` for a tree.
    pub mode: &'static str,
    /// The entry name.
    pub name: String,
    /// The object id.
    pub oid: Oid,
}

impl TreeEntry {
    /// True for a tree entry.
    pub fn is_tree(&self) -> bool {
        self.mode == "40000"
    }

    /// The name as git sorts it: a tree's name compared as if it ended in `/` ([F14 §3.2]).
    fn sort_key(&self) -> Vec<u8> {
        let mut k = self.name.as_bytes().to_vec();
        if self.is_tree() {
            k.push(b'/');
        }
        k
    }
}

/// True when `entries` are in git's order, strictly ([F14 §3.2]).
pub fn in_git_order(entries: &[TreeEntry]) -> bool {
    entries
        .windows(2)
        .all(|w| w[0].sort_key() < w[1].sort_key())
}

/// The content of a tree object: per entry `<mode> SP <name> NUL <raw object id>`, in the given order.
pub fn tree_object(entries: &[TreeEntry]) -> Vec<u8> {
    let mut o = Vec::new();
    for e in entries {
        o.extend_from_slice(e.mode.as_bytes());
        o.push(b' ');
        o.extend_from_slice(e.name.as_bytes());
        o.push(0);
        o.extend_from_slice(e.oid.digest());
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::hex;

    /// Well-known ids: the empty blob and the empty tree in both object formats; git's order puts `a.b` before the
    /// tree `a` (`a/` > `a.`), and the tree `a` before the blob `a0`.
    #[test]
    fn ids_and_order() {
        assert_eq!(
            hex(object_id("blob", b"", Algo::Sha1).digest()),
            "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
        );
        assert_eq!(
            hex(object_id("tree", b"", Algo::Sha1).digest()),
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        );
        assert_eq!(
            hex(object_id("tree", b"", Algo::Sha256).digest()),
            "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321"
        );
        let e = |mode: &'static str, name: &str| TreeEntry {
            mode,
            name: name.into(),
            oid: Oid::Sha1([0; 20]),
        };
        assert!(in_git_order(&[
            e("100644", "a.b"),
            e("40000", "a"),
            e("100644", "a0")
        ]));
        assert!(!in_git_order(&[e("40000", "a"), e("100644", "a.b")]));
        assert_eq!(tree_object(&[e("100644", "x")]).len(), 6 + 1 + 1 + 1 + 20);
    }
}
