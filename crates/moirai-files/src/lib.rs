//! FL-1, the file-link primitives: path rules and `fold_v1` (generated Unicode 17.0.0 tables); `is_text`, EOL
//! normalisation and `oid`; normalised lines; uid derivations; chapter 20's R-14 constant module; anchor capture and
//! resolve; the scope scanners; the gitignore and never-candidate matchers; sketch, winnowing, similarity and
//! containment. It spawns no process.
//!
//! Product crate; checked by GT20 (e) on every target. Filled by R-FL1A (WP-61, WP-62, WP-64: the path, fold, text,
//! oid, uid, R-14 and anchor modules) and R-FL1B (WP-61b, WP-63, WP-66: the ignore, scanner and sketch modules);
//! `docs/m0/authors.md` fixes the module paths. Sources: [40 §2.3–§2.7, §4.3–§4.5, §5.8], [80 §2.10, §2.11.4];
//! `docs/m0/PLAN.md` §2.2.

// R-FL1A (WP-61).
pub mod fold;
pub mod path;

// R-FL1A (WP-62).
pub mod oid;
pub mod r14;
pub mod text;
pub mod uid;
