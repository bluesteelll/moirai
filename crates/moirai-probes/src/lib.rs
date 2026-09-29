//! The measurement-protocol framework and its statistics, the permanent layout micro-benchmarks, and the probe
//! bodies of measurements 1–16 and 18–22, generic over `V: Vfs` and `M: Meter`.
//!
//! Test-only library; checked by GT20 (e) on every target. It never depends on `moirai-os`: the composition root
//! `moirai-probes-bin` wires it to the Windows OS layer. Filled by WP-50 (the framework) and WP-51, WP-52 and WP-55
//! to WP-57 (R-HARN-I), and by WP-53a–e and WP-54 (R-HARN-M). Sources: [60 §5.1–§5.2],
//! `docs/spec/measurement-protocol.md`; `docs/m0/PLAN.md` §2.2, §6.2 R10, R17.
