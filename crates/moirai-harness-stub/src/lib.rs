//! The permanent harness probe and conformance fixture: a hand-written dual-era MCP stdio loop, with `initialize` for
//! protocol versions 2025-03-26, 2025-06-18 and 2025-11-25 and `server/discover` for 2026-07-28; stub tools with sized
//! results, `_meta` logging, hidden `hook_*` handlers, argv and stdin echo, and the Claude and Codex plugin wrappers.
//! Its MCP loop also serves LQ-Bench and measurement 19.
//!
//! Test-only crate; checked by GT20 (e) on every target. Written by R-HARN for the Codex probes and measurements 7
//! and 19 (WP-56) and for the LQ-Bench server (WP-71b). Sources: [90 §10.5]; `docs/m0/PLAN.md` §2.2, §6.2 R11.
