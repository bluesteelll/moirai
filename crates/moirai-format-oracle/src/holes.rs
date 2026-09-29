//! The named holes ([F01 §2.5], `docs/spec/HOLES.md`) whose values the decoders need. Each constant is the bound the
//! hole's constraint column fixes, or the one value every candidate shares, so the decoders accept exactly what any
//! candidate admits; WP-95b replaces each with the filled value (PLAN WP-95b).

/// `HOLE(F20-window-lines)`: `WIN`, the most hashes on one side of a window value. The constraint `4 + 4 × WIN ≤ 68`
/// bounds every candidate (16, 8) by 16 ([F20 §2.7.3], [F20] Holes).
pub const WIN_MAX: usize = 16;

/// `HOLE(F10-codec-values)`: the numbering is fixed (0 `none`, 1 `lz4`, 2 `lz4-dict`, 3 `zstd`, 4 `zstd-dict`); the
/// admitted set is the hole. Every value of the numbering is accepted here; payloads of codecs 1–4 stay opaque at M0
/// ([PLAN §6.2] R3).
pub const CODEC_MAX: u8 = 4;
