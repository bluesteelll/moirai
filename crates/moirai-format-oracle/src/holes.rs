//! The named holes ([F01 §2.5], `docs/spec/HOLES.md`) whose values the decoders need. Each constant is the bound the
//! hole's constraint column fixes, or the one value every candidate shares, so the decoders accept exactly what any
//! candidate admits; WP-95b replaces each with the filled value (PLAN WP-95b).

/// `HOLE(F20-window-lines)`: `WIN`, the most hashes on one side of a window value. The constraint `4 + 4 × WIN ≤ 68`
/// bounds every candidate (16, 8) by 16 ([F20 §2.7.3], \[F20\] Holes).
pub const WIN_MAX: usize = 16;

/// `HOLE(F10-codec-values)`: the numbering is fixed (0 `none`, 1 `lz4`, 2 `lz4-dict`, 3 `zstd`, 4 `zstd-dict`); the
/// admitted set is the hole. Every value of the numbering is accepted here; payloads of codecs 1–4 stay opaque at M0
/// ([PLAN §6.2] R3).
pub const CODEC_MAX: u8 = 4;

/// `HOLE(F10-dict-form)`, raw-content candidate: at most 65,536 bytes of dictionary content ([F10 §6.2], §9).
pub const DICT_RAW_MAX: usize = 65_536;

/// `HOLE(F10-dict-form)`, formatted candidate: a zstd dictionary of at most 112,640 bytes ([F10 §6.2], §9) that begins
/// with [`ZSTD_DICT_MAGIC`] and whose `Dictionary_ID` is the file's number. Until the fill, content of at most
/// [`DICT_RAW_MAX`] bytes is accepted as raw content whatever it begins with; longer content must be formatted.
pub const DICT_FORMATTED_MAX: usize = 112_640;

/// The magic number `0xEC30A437` of a formatted zstd dictionary as it lies on disk, little-endian ([RFC 8878] §5).
pub const ZSTD_DICT_MAGIC: [u8; 4] = [0x37, 0xA4, 0x30, 0xEC];
