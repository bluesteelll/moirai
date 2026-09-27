//! The toy log, generic over `V: Vfs`: group commit, the two-slot `HEAD`, the barrier and recovery, with the
//! seeded-bug switches in their own module. It is also the vehicle for measurements 1, 2, 12 and T2 on the real
//! Windows `Vfs`.
//!
//! Test-only crate; checked by GT20 (e) on every target. Filled by WP-40 and WP-40b (R-TOY). Sources:
//! `docs/spec/format/16-protocol.md` (WP-16), [60 §3.1] item 4, [80 §2.4.4]; `docs/m0/PLAN.md` §2.2, §6.2 R10.
