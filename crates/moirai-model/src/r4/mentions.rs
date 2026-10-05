//! `links mentions` ([40 §3.7], §2.4 "Paths inside prose"; replay row 3 of [40 §8.3.4]): the textual mentions, in a
//! node's text or a tracked text file, of paths that resolve through a file node's aliases or through a root node's
//! `path_moves` to a current path. A read: it appends nothing (I-F5).
//!
//! What counts as a mention is the model's definition, since neither [40] nor [F18] freezes a grammar for paths in
//! prose: an occurrence of a path's exact bytes that no path-name byte (ASCII letters and digits, `_`, `-`, `.`, `/`,
//! and every byte ≥ `80`) precedes, and that no path-name byte follows, except a single `.` that ends a sentence (a
//! `.` followed by the end or by a byte that is not a path-name byte).

use crate::r4::cascade::View;
use crate::r4::uid::FileStatus;
use crate::value::PathMove;

/// How a mention resolves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// Through a node's `aliases`.
    Alias,
    /// Through the root node's `path_moves`.
    PathMove,
}

/// One mention: the byte range of the mentioned path in the text, the node and its current path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    /// The first byte.
    pub offset: usize,
    /// The mentioned (moved-away) path.
    pub path: String,
    /// The node's `#N`.
    pub node: u32,
    /// The node's current path.
    pub now: String,
    /// How it resolves.
    pub via: Via,
}

fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/') || b >= 0x80
}

/// Whether the occurrence [i, j) of a path is a mention by its boundaries (module documentation).
fn bounded(t: &[u8], i: usize, j: usize) -> bool {
    let before = i == 0 || !name_byte(t[i - 1]);
    let after = j == t.len()
        || !name_byte(t[j])
        || (t[j] == b'.' && (j + 1 == t.len() || !name_byte(t[j + 1])));
    before && after
}

/// The end of the maximal path-name run from i, without a sentence-ending `.`.
fn run_end(t: &[u8], i: usize) -> usize {
    let mut j = i;
    while j < t.len() && name_byte(t[j]) {
        j += 1;
    }
    if j > i && t[j - 1] == b'.' {
        j -= 1;
    }
    j
}

/// The composition of a path through `path_moves` entries in (hlc, from, to) order ([40 §2.4]).
fn compose(moves: &[&PathMove], p: &str) -> String {
    let mut x = p.to_string();
    for m in moves {
        if let Some(rest) = x.strip_prefix(m.from.text.as_str()) {
            x = format!("{}{rest}", m.to.text);
        }
    }
    x
}

/// The mentions of moved-away paths of root `root` in a text ([40 §3.7] `links mentions`), by offset.
// spec: [40 §3.7] links mentions; [40 §2.4] paths inside prose
pub fn mentions(text: &str, view: &View, root: &str) -> Vec<Mention> {
    let t = text.as_bytes();
    let live: Vec<_> = view
        .files
        .iter()
        .filter(|f| {
            f.root == root
                && !f.tombstone
                && matches!(f.status, FileStatus::Present | FileStatus::Planned)
        })
        .collect();
    let current = |p: &str| live.iter().find(|f| f.path == p);
    let mut out: Vec<Mention> = Vec::new();
    // Aliases that are not some node's current path.
    for f in &live {
        for a in &f.aliases {
            if a.is_empty() || current(a).is_some() {
                continue;
            }
            let ab = a.as_bytes();
            let mut i = 0;
            while i + ab.len() <= t.len() {
                if &t[i..i + ab.len()] == ab && bounded(t, i, i + ab.len()) {
                    out.push(Mention {
                        offset: i,
                        path: a.clone(),
                        node: f.n,
                        now: f.path.clone(),
                        via: Via::Alias,
                    });
                }
                i += 1;
            }
        }
    }
    // Paths under a `from` prefix that compose to a node's current path.
    let mut moves: Vec<&PathMove> = view.moves.get(root).into_iter().flatten().collect();
    moves.sort();
    for m in &moves {
        let fb = m.from.text.as_bytes();
        let mut i = 0;
        while i + fb.len() <= t.len() {
            if &t[i..i + fb.len()] == fb && (i == 0 || !name_byte(t[i - 1])) {
                let j = run_end(t, i);
                if let Ok(o) = std::str::from_utf8(&t[i..j])
                    && j > i + fb.len()
                    && current(o).is_none()
                {
                    let c = compose(&moves, o);
                    if c != o
                        && let Some(f) = current(&c)
                        && !out.iter().any(|x| x.offset == i)
                    {
                        out.push(Mention {
                            offset: i,
                            path: o.to_string(),
                            node: f.n,
                            now: c,
                            via: Via::PathMove,
                        });
                    }
                }
            }
            i += 1;
        }
    }
    out.sort_by(|a, b| {
        a.offset
            .cmp(&b.offset)
            .then(a.path.len().cmp(&b.path.len()).reverse())
    });
    out.dedup_by(|a, b| a.offset == b.offset);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::r4::cascade::FileNode;
    use crate::value::{MoveClass, PathVal};
    use std::collections::BTreeMap;

    fn node(n: u32, path: &str, aliases: &[&str]) -> FileNode {
        FileNode {
            n,
            root: "project".into(),
            path: path.into(),
            oid: None,
            bytes: None,
            observed_git: None,
            observed_blob: None,
            relink: None,
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            status: FileStatus::Present,
            artifact_kind: None,
            tombstone: false,
            obs_hlc: 0,
            conflict: None,
            path_claim: false,
        }
    }

    fn pm(from: &str, to: &str) -> PathMove {
        PathMove {
            hlc: 1,
            class: MoveClass::Explicit,
            from: PathVal {
                root: "project".into(),
                text: from.into(),
            },
            to: PathVal {
                root: "project".into(),
                text: to.into(),
            },
            git: None,
        }
    }

    #[test]
    fn aliases_and_path_moves_resolve_mentions() {
        let view = View {
            files: vec![
                node(1, "docs/archive/PLAN.md", &["docs/PLAN.md"]),
                node(2, "src/sync/lock.rs", &[]),
            ],
            moves: BTreeMap::from([("project".to_string(), vec![pm("src/", "src/sync/")])]),
            anchor_conflicts: Vec::new(),
        };
        let text =
            "See docs/PLAN.md. Also src/lock.rs, not xdocs/PLAN.md or docs/PLAN.mdx; src/other.rs.";
        let m = mentions(text, &view, "project");
        assert_eq!(m.len(), 2, "{m:?}");
        assert_eq!(
            (m[0].node, m[0].via, m[0].now.as_str()),
            (1, Via::Alias, "docs/archive/PLAN.md")
        );
        assert_eq!(
            (m[1].node, m[1].via, m[1].path.as_str()),
            (2, Via::PathMove, "src/lock.rs")
        );
        // A current path is not a moved-away mention.
        assert!(mentions("docs/archive/PLAN.md", &view, "project").is_empty());
    }
}
