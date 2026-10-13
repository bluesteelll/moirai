//! Measurement 6 (WP-54, [60 §5.2] item 6, `60-roadmap.md:762`): the body and `hist` codecs. It measures size, encode
//! and decode speed and RSS of `lz4_flex` 0.14 block with and without a raw-content dictionary of at most 64 KiB, of
//! `ruzstd` 0.9 at `Fastest`, and of the `zstd` CLI at level 1 as option (3)'s proxy, on the owner's notes, on the plan
//! sections of `docs/**` and on `hist` frames (`90-harness-agnostic-design.md:959-974`). It decides
//! `F02-dict-file`, `F10-codec-values`, `-blob-codec`, `-hist-codec`, `-dict-form` and `-body-placement`
//! (`docs/spec/HOLES.md:33-39`).
//!
//! **The corpora** ([`corpus`]): the notes through `/private/codec-bodies/manifest.tsv`, the plan sections split at
//! headings outside fences, where a recording run and a smoke find them, and the split of each corpus into a training
//! and a measured side, keyed by each input's id.
//!
//! **The dictionary** ([`dict`]): the probe's own deterministic trainer (R-33 Q1 (b)), the epoch cover of `ruzstd`'s
//! `dict_builder` with exact d-mer counts, over the training side only; at most 64 KiB of raw content. A product
//! trainer may do better or worse, so the gain the dictionary shows is this trainer's.

pub mod corpus;
pub mod dict;
