//! The in-memory `Vfs` that enforces fault-model items 1–12, and the crash enumerator that drives it.
//!
//! Test-only crate, never linked into a product binary; checked by GT20 (e) on every target. Filled by WP-31 (the
//! simulator) and WP-32 (the crash enumerator), both R-HARN-S; the enumerator's author is never the author of the
//! seeded bugs (S4). Sources: `docs/spec/format/15-fault-model.md` (WP-16), [60 §3.1] item 3, §3.13 GT1,
//! [80 §2.4.4]; `docs/m0/PLAN.md` §2.2.
