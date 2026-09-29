# Named holes of the specification

| | |
|---|---|
| Title | Every named hole of `docs/spec/`: the value an M0 measurement or benchmark decides, where it sits, what decides it and its candidates; the ids that alias a hole owned elsewhere; the rule-file "holes" that are already decided; the measured decisions that are not holes |
| Chapter | `docs/spec/HOLES.md` (an index; no citation tag) |
| Status | draft, pass 1 pending |
| Work package | assembled by R-SPEC-F from every chapter's Holes section ([PLAN §3.2] item 1); WP-81a fills the holes and records each value in the owning chapter |
| Sources | [F01 §2.5] (the hole convention); the Holes section of every file in `docs/spec/`; [60 §3.1] "Decisions fixed at M0 exit"; [60 §5.2] (the M0 measurements) |
| Depends on | every chapter that owns a hole; [COVERAGE.md](COVERAGE.md) for the rows the holes affect |

## 1. How to read this file

- A hole is written `HOLE(<id>)` where its value is used and is one row of its owning chapter's Holes table ([F01 §2.5]).
  The owning chapter is authoritative: it holds the full candidate list and the constraint the value must meet. This file
  indexes the holes; it adds no candidate and no constraint.
- Columns: `id`; `chapter §`, the sections that use the hole (the owner first); `what`; `decided by`, the measurement of
  [60 §5.2] or the benchmark and the work package that runs it; `candidates`, the first one being the design's value or
  the draft value where there is one.
- When WP-81a fills a hole, the value replaces the inline `HOLE(...)` in the owning chapter and the owner's Holes row
  stays with the value and the measurement record ([F01 §2.5]). This file then gets the value in its `candidates` cell.
- Counts: 53 holes to fill (§2), all in the `<part>-<name>` form. The 3 rule-file "holes" of §3 are decided and written
  as values, and the 7 ids of §4 are renamed to their owners' ids (review pass 1, S1-40, P1-33, A1-42), so a scan of
  every `HOLE(` in `docs/spec/` outside `reviews/` finds exactly the ids of §2. First checked on 2026-09-28 by a scan of
  every `HOLE(` in `docs/spec/` and every Holes section; re-checked after the renames.

## 2. Holes that WP-81a fills

### 2.1 Format chapters

| id | chapter § | what | decided by | candidates |
|---|---|---|---|---|
| `F02-dict-file` | [F02 §5.1]; [F04 §4.1], [F10 §2.1], [F10 §3.5], [F17 §6.3] | whether a store holds `dict.<D>` files | measurement 6 (WP-54), filled with `F10-dict-form` (one decision in two chapters) | absent; present with a raw-content dictionary behind the `MDIC` header; present with a formatted zstd dictionary behind it |
| `F09-doclen` | [F09 §12.4] | whether the sections `DOCLEN` (`0x0052`) and `FTSSTAT` (`0x0053`) exist in format v1 | LQ-Bench's search-stratum ablation, BM25 against the statistics-free scorer (WP-72, GT13) | kept (the layouts of [F09 §12.4]); dropped (both tags stay reserved; `TERMS`, `POST` and the tokenizer unchanged) |
| `F10-codec-values` | [F10 §3.1], [F10 §3.3] | the codec values admitted besides 0 `none`, and each one's payload format | measurement 6 (WP-54) | numbering fixed (1 `lz4`, 2 `lz4-dict`, 3 `zstd`, 4 `zstd-dict`); admitted set {0, 2, 3} (expected), {0, 1, 3}, {0, 3} or {0, 3, 4}, with 1 replacing 3 if `lz4` is chosen for `hist` |
| `F10-blob-codec` | [F10 §3.4], [F10 §5.3] | the codec a seal writes for body blobs | measurement 6 (WP-54), by [90 §11.3]'s decision rule | 2 `lz4-dict` (expected); 1 `lz4`; 3 `zstd`; 4 `zstd-dict` |
| `F10-hist-codec` | [F10 §3.4], [F10 §4.3] | the codec retirement writes for `hist` frames | measurement 6 (WP-54) | 3 `zstd` (expected); 1 `lz4` |
| `F10-dict-form` | [F10 §6.2], [F10 §3.5], [F10 §9] | the form of `dict.<D>` files | measurement 6 (WP-54) | absent; raw content (≤ 64 KiB); formatted zstd dictionary (32–110 KiB, `Dictionary_ID` = D) |
| `F10-body-placement` | [F10 §3.4]; touches [F06 §8] and [F17 §5.1] | whether bodies in the log tail may be compressed | measurement 6 (WP-54) | raw (the design's default); compressed with `F10-blob-codec` |
| `F15-lock-release` | [F15 §3.8] FM-8.1; used by [F03] Holes and [F16] Holes (the release delays the gates inject); alias in [OS/lock §7.3] | the lock-release delay after `TerminateProcess` per byte kind (writer, flush, slot), idle and loaded: p50, p99, max and the CDF the in-memory `Vfs` samples | measurement 12 (WP-52) | prior evidence ≤ 32 ms observed, p99 1–8 ms; the unbounded tail always stays |
| `F17-ckpt-ops` | [F17 §3] P06, [F17 §5]; [CFG §10.2] | production value of `store.checkpoint.ops` | measurement 10 (WP-53d); checked by measurement 14 (WP-53e) | 4,096; 2,048; 1,024 |
| `F17-ckpt-bytes` | [F17 §3] P07, [F17 §5]; [CFG §10.2] | production value of `store.checkpoint.bytes` | measurement 10 (WP-53d) | 4 MiB; 2 MiB; 1 MiB |
| `F17-ckpt-body` | [F17 §3] P08, [F17 §5]; [CFG §10.2] | production value of `store.checkpoint.body-bytes` | measurements 6 (WP-54) and 10 (WP-53d) | 32 MiB; 16 MiB; 8 MiB |
| `F17-tail-overlay` | [F17 §3] P09, [F17 §5]; [CFG §10.2] | production value of `store.tail.max-overlay-bytes` | measurement 10 (WP-53d), with measurement 11 (WP-52) | 1 MiB; 512 KiB |
| `F17-tail-overlay-quiet` | [F17 §3] P10, [F17 §5.3]; [CFG §10.2] | production value of `store.tail.max-overlay-bytes.quiet` | measurement 10 (WP-53d) | 2 MiB; 1.5 MiB |
| `F17-quiet-mult` | [F17 §3] P11, [F17 §5.3]; [CFG §10.2] | production value of `quiet.tail-cap-multiplier` | measurement 10 (WP-53d) | 8; 4 |
| `F17-promo-ops` | [F17 §3] P19, [F17 §7]; [CFG §10.2] | production value of `store.promotion.overlay-ops` | measurement 3 (WP-53a) | 3,000; 4,000; 5,000 |
| `F17-promo-bytes` | [F17 §3] P20, [F17 §7]; [CFG §10.2] | production value of `store.promotion.overlay-bytes` | measurement 3 (WP-53a) | 1 MiB; 768 KiB |
| `F17-promo-age` | [F17 §3] P21, [F17 §7]; [CFG §10.2] | production value of `store.promotion.age-checkpoints` | measurements 3 (WP-53a) and 5 (WP-53c) | 16; 8 |
| `F17-loose-pack` | [F17 §3] P24, [F17 §9.1]; [CFG §10.2] | production value of `store.image.loose-pack-threshold` | measurement 8 (WP-54) | 8; 4; 16 |
| `F17-lock-writer` | [F17 §3] P26, [F17 §10]; [CFG §10.2]; used by [F03] Holes and [F16] P-27, P-41, P-72; alias in [OS/lock §4] | production value of `lock.writer-wait-ms` | measurements 2 and 12 (WP-52) | 2,000 ms; 1,000 ms; 5,000 ms |
| `F17-lock-flush` | [F17 §3] P27, [F17 §10]; [CFG §10.2]; used by [F03] Holes and [F16] P-27, P-41, P-72; alias in [OS/lock §4] | production value of `lock.flush-wait-ms` | measurement 2 (WP-52), with measurement 12 for the flush byte | 2,000 ms; 1,000 ms; 5,000 ms |
| `F20-window-lines` | [F20 §2.7.2], [F20 §6.1], [F20 §7]; lengths in [F07], [F08], [F14] | `WIN`: non-trivial lines hashed on each side of an anchor's quote span | replay row 2 of [40 §8.3.4] (WP-76) | 16 (draft); 8 |
| `F20-quote-lines` | [F20 §6.1], [F20 §7] | most non-trivial lines a `quote` span may have before it becomes a `range` | replay row 2 (WP-76) | 4 (draft); 2; 6 |
| `F20-quote-max` | [F20 §6.1], [F20 §7] | most bytes of a `quote` span and of a header quote | replay row 2 (WP-76) | 128 (draft); 96 |
| `F20-quote-default` | [F20 §6.1], [F20 §7] | length of the start and end quotes of a `range` | replay row 2 (WP-76) | 64 (draft); 32; 128 |
| `F20-context` | [F20 §6.1], [F20 §7] | prefix and suffix length at capture | replay row 2 (WP-76) | 32 (draft); 16 |
| `F20-context-max` | [F20 §6.1], [F20 §7] | widened prefix and suffix length | replay row 2 (WP-76) | 64 (draft); 48 |
| `F20-context-margin` | [F20 §6.2], [F20 §7] | margin of the context score that breaks a duplicate-quote tie | replay row 2 (WP-76) | 1/10 (draft) |
| `F20-window-margin` | [F20 §6.2], [F20 §7] | margin of the window score (duplicate quotes, `lines` anchors) | replay row 2 (WP-76) | 15/100 (draft); 1/10; 1/5 |
| `F20-range-spread` | [F20 §6.2], [F20 §7] | how far after its start quote a range's end quote may lie, in multiples of the captured hint length | replay row 2 (WP-76) | 2 (draft); 3 |
| `F20-fuzzy-budget` | [F20 §6.4], [F20 §7] | Myers error budget per quote byte | replay row 2 (WP-76) | 1/4 (draft); 1/5 |
| `F20-fuzzy-accept` | [F20 §6.4], [F20 §7] | least quote similarity of a fuzzy match | replay row 2 (WP-76) | 3/4 (draft); 4/5 |
| `F20-fuzzy-span` | [F20 §6.4], [F20 §7] | bytes searched on each side of the hint before the scope and the whole file | replay row 2 (WP-76) | 16,384 (draft); 16,000 |
| `F20-fuzzy-weights` | [F20 §6.4], [F20 §7] | weights (quote, prefix, suffix, window) of the fuzzy score | replay row 2 (WP-76) | (50, 20, 20, 10) (draft); (50, 20, 20, 2) |
| `F20-fuzzy-margin` | [F20 §6.4], [F20 §7] | top-2 margin of an accepted fuzzy match | replay row 2 (WP-76) | 2/100 (draft) |
| `F20-header-margin` | [F20 §6.4], [F20 §7] | top-2 margin for a same-kind header match when the scope did not resolve | replay row 2 (WP-76) | 1/10 (draft) |
| `F20-lines-min` | [F20 §6.5], [F20 §7] | least window score for aligning a `lines` anchor | replay row 2 (WP-76) and P11's generated cases (WP-77) | 1/2 (draft); 3/4 |
| `F20-winnow-k` | [F20 §2.9], [F20 §7] | winnowing k-gram length in tokens | WP-66 (stage-2 measurements on synthetic rename, reflow and identifier-rename sets), with WP-76 | 5 (draft); 4; 6 |
| `F20-winnow-w` | [F20 §2.9], [F20 §7] | winnowing window in k-grams | as `F20-winnow-k` | 4 (draft); 8 |
| `F20-btime-ntfs` | [F20 §5.9], [F20 §7]; [OS/project §4.3]; [OS/mapping-appendix] Holes | `VolumeCaps.btime` class of NTFS volumes on Windows 11; constraint ([F20] Holes): `TunneledNotCopied` only if no measured tool gives a new file its source's creation time, else `Absent` | measurement 15 (WP-55), with the copy paths of review pass 1 (S1-31): `robocopy /COPY:DAT`, `Copy-Item`, Explorer copy and paste, archive extraction, `git checkout` of a moved file | `TunneledNotCopied` (draft); `Unforgeable`; `Absent` |
| `F20-ctime-rename` | [F20 §5.9], [F20 §7]; [OS/project §4.3]; [OS/mapping-appendix] Holes | whether copy-rule line 2 requires the candidate's ChangeTime clearly after V | measurement 15 (WP-55) | required (draft); line 2 never `exact` on Windows |
| `F20-clock-skew` | [F20 §5.1], [F20 §7]; [OS/clock §6], [OS/clock §8] | `SKEW`: the largest difference between a file timestamp and the `hlc` wall time of the same moment on one machine | measurements 15 (WP-55) and 22 (WP-52) | the smallest whole number of milliseconds ≥ the measured maximum plus the volume's granularity; no draft (at M0 only WP-92 uses it, as a parameter) |

### 2.2 OS layer

| id | chapter § | what | decided by | candidates |
|---|---|---|---|---|
| `OS-win-boot-source` | [OS/proc §4.2], [OS/proc §4.5]; used by [F03], [F16] P-60; [OS/mapping-appendix] Holes | the Windows source of the counter `B` of the boot identity | measurement 22 (WP-52) | (a) `KUSER_SHARED_DATA.BootId` with the `PrefetchParameters\BootId` registry fallback (the design's choice); (b) the registry value only; (c) none: Windows runs in Unknown-boot mode |
| `OS-win-boot-clock` | [OS/clock §2.1], [OS/clock §11]; [OS/mapping-appendix §2.2] | the Windows source of `boot_ns` | measurement 22 (WP-52) | (a) `QueryInterruptTimePrecise` (the design's choice); (b) `QueryInterruptTime`; (c) `GetTickCount64` × 10^6 |
| `OS-pfs-gran-probe-k` | [OS/project §4.4]; [OS/mapping-appendix] Holes | the number of distinct stamp mtimes the effective-granularity probe waits for | measurement 15 (WP-55) | 3; 4 |
| `OS-pfs-gran-probe-budget` | [OS/project §4.4]; [OS/mapping-appendix] Holes | the mono-time budget of that probe | measurement 15 (WP-55) | 50 ms; 100 ms |
| `OS-share-retry-ms` | [OS/fs §6.3]; used by [F16] P-85 | `total_ms` of `ShareRetry::Bounded` for image-export renames, `packed-refs` and loose-ref replace-renames and the store `config` rename | measurements 8 (WP-54) and 15 (WP-55) | 1,000 ms (the `--retry-ms` default [40 §3.4] gives `file mv`) |

### 2.3 Query language

| id | chapter § | what | decided by | candidates |
|---|---|---|---|---|
| `LQ-display-spelling` | [LQ/gql-spelling §4]; [LQ/card §1], [LQ/card §3], [LQ/card §4], [LQ/envelope §4], [LQ/errors §2] | the quantifier forms that every printed text uses: the card, `--show-query` and `--show-tx`, error replacement texts and the reading echo | LQ-Bench display-spelling ablation (WP-72, [90 §8.1] L1) | the Cypher column of [LQ/gql-spelling §4] (kept unless GQL wins beyond the run-to-run spread); the GQL column |
| `LQ-card-shrink` | [LQ/card §1], [LQ/card §7] | how many shrink steps of [LQ/card] §7.3 the card takes (0 when the body passes as written) | the card's token count: WP-58 (Claude) and `moirai-tokcount` (o200k), gated in WP-72 | 0 to 8 steps (pass 1, P1-42: step 6 covers the 1,160-token upper estimate) |
| `LQ-card-examples` | [LQ/card §6] | how many examples the frozen card keeps | LQ-Bench 0/4/7 ablation (WP-72, [50 §7.4] item 7) | 7; 4; 0 |

### 2.4 Configuration

| id | chapter § | what | decided by | candidates |
|---|---|---|---|---|
| `CFG-codex-store-writes` | [CFG §10.9] | default of `integrate.codex.store-writes` | probe P7 of measurement 7 (WP-56) | `writable-root` (the design default; also the value without Codex access); `execpolicy-store` |
| `CFG-codex-approval` | [CFG §10.9] | default of `integrate.codex.approval` | probe P6 of measurement 7 (WP-56) | `split` (the design default; also the value without Codex access); `writes`; `prompt`; `approve` |
| `CFG-codex-mcp-result` | [CFG §10.8]; [F19 §3.2]; [RULES/pack-classes] PE-004 (the `default_bytes` it cites) | default of `mcp.result-max-bytes.codex` | probes P3 and P4 of measurement 7 (WP-56) | 16,000 (the design value; also the value without Codex access); 12,000; 8,000 |
| `CFG-model-profile-opus` | [CFG §10.9] | default of `lq.model-profile.claude-opus-5-5` | GT13 on LQ-3 (WP-72) | `gated`; `compatible`; `unknown` |

### 2.5 By measurement or work package

| measurement or WP | holes |
|---|---|
| measurement 2 (WP-52) | `F17-lock-writer`, `F17-lock-flush` |
| measurement 3 (WP-53a) | `F17-promo-ops`, `F17-promo-bytes`, `F17-promo-age` |
| measurement 5 (WP-53c) | `F17-promo-age` |
| measurement 6 (WP-54) | `F02-dict-file`, `F10-codec-values`, `F10-blob-codec`, `F10-hist-codec`, `F10-dict-form`, `F10-body-placement`, `F17-ckpt-body` |
| measurement 7 (WP-56) | `CFG-codex-store-writes`, `CFG-codex-approval`, `CFG-codex-mcp-result` |
| measurement 8 (WP-54) | `F17-loose-pack`, `OS-share-retry-ms` |
| measurement 10 (WP-53d) | `F17-ckpt-ops`, `F17-ckpt-bytes`, `F17-ckpt-body`, `F17-tail-overlay`, `F17-tail-overlay-quiet`, `F17-quiet-mult` |
| measurement 11 (WP-52) | `F17-tail-overlay` |
| measurement 12 (WP-52) | `F15-lock-release`, `F17-lock-writer`, `F17-lock-flush` |
| measurement 14 (WP-53e) | `F17-ckpt-ops` (check) |
| measurement 15 (WP-55) | `F20-btime-ntfs`, `F20-ctime-rename`, `F20-clock-skew`, `OS-pfs-gran-probe-k`, `OS-pfs-gran-probe-budget`, `OS-share-retry-ms` |
| measurement 22 (WP-52) | `OS-win-boot-source`, `OS-win-boot-clock`, `F20-clock-skew` |
| WP-66 | `F20-winnow-k`, `F20-winnow-w` |
| WP-72 (LQ-Bench, GT13) | `F09-doclen`, `LQ-display-spelling`, `LQ-card-shrink` (with WP-58), `LQ-card-examples`, `CFG-model-profile-opus` |
| WP-76 (replay row 2) | `F20-window-lines`, `F20-quote-lines`, `F20-quote-max`, `F20-quote-default`, `F20-context`, `F20-context-max`, `F20-context-margin`, `F20-window-margin`, `F20-range-spread`, `F20-fuzzy-budget`, `F20-fuzzy-accept`, `F20-fuzzy-span`, `F20-fuzzy-weights`, `F20-fuzzy-margin`, `F20-header-margin`, `F20-lines-min` (with WP-77), `F20-winnow-k`, `F20-winnow-w` |

## 3. Rule-file holes that are already decided

These were written `HOLE(...)` in rule files, but a naming or code choice is not a measured value ([F01 §2.5]), and the
work packages named in them have decided each one. R-MODEL has replaced the inline `HOLE(...)` with the value and dropped
the rows from the files' Holes sections (review pass 1, S1-40); the rows stay here until pass 2 as the record.

| id | chapter § | what | decided by | candidates |
|---|---|---|---|---|
| `pack-digest-param` | [RULES/pack-classes] §7 (NR-001) | the names of the `complete` flag, the MCP `complete` parameter and the additive `result.v1` field that carry the pack digest token | decided by WP-25: [API] open point 29, with [F19] agreeing in its open points | **decided:** `--pack-digest`, `pack_digest`, `pack_digest` (`string` or `null`); the API argument `Complete.pack_digest` ([API §10.5]) |
| `unknown-model-write-code` | [RULES/role-write-policy] §10 (WZ-010, WQ-004) | the code, name and exit code refusing a free-form `TX` under the `unknown` model profile | decided by WP-18 and WP-19: [F19 §11.1], [LQ/errors §5.5] | **decided:** `E411 unknown_model_write`, exit 6 |
| `exit5-codes` | [RULES/role-write-policy] §10 (WZ-004, WZ-005) | the codes and names of the two exit-5 refusals | decided by WP-18: [F19 §11.2], [LQ/errors §5.5] | **decided:** both are rows of `E407 lease`, exit 5 ("declared agent" and "bound lease") |

## 4. Aliases

Ids that were used in place of a hole's own id. Each named the same value as the owning id. [F01] open point 15, adopted
in review pass 1 (S1-40, P1-33, A1-42), renamed them, so one check finds every hole by its `<part>-` prefix; this table
records the old and the new id until pass 2.

| old id | was used in | now |
|---|---|---|
| `lock-writer-wait-ms` | [OS/lock §4], [OS/lock] Holes, [CFG] open point 19 | `F17-lock-writer` (renamed) |
| `lock-flush-wait-ms` | [OS/lock §4], [OS/lock] Holes, [CFG] open point 19 | `F17-lock-flush` (renamed) |
| `lock-release-delay` | [OS/lock §7.3], [OS/lock] Holes | `F15-lock-release` (renamed) |
| `os-win-boot-source` | [F03] Holes, [F01] open point 15 | `OS-win-boot-source` (renamed; [F01] open point 15 quotes the old id as history) |
| `display-spelling` | [F19] Holes, [LQ/canonical-ast] Holes, [LQ/gql-spelling] open points, [F01] open point 15 | `LQ-display-spelling` (renamed; [F01] open point 15 quotes the old id) |
| `r4-clock-skew` | [reviews/a1-dispositions] Holes (written before [F20] named it) | `F20-clock-skew`; the review record is not edited |
| `share-retry-ms` | [OS/fs §6.3], [F16] Holes, [CFG] Holes | `OS-share-retry-ms` (renamed) |

## 5. Measured decisions that are not holes

Decisions an M0 measurement or benchmark takes that the specification records without a `HOLE(...)`, because they choose
between whole designs or confirm one rather than fill a value:

| decision | where recorded | decided by | effect |
|---|---|---|---|
| whether M1 builds the leader | [reviews/a1-dispositions] Holes; [F16] P-95 | measurements 1, 2 and T2 (WP-52, WP-53e) | changes no rule of [F16] |
| T1's structure (Option A, sealed columnar segments and a tail overlay) | [F09] open point OP-09-01 | measurement 14 (WP-53e) | Option B would replace [F09], not fill it |
| `MOVEFILE_WRITE_THROUGH` on every Windows rename | [F15 §5.8], [F16] P-82 | measurement 17, deferred with the rig to after the release | stays until then; decides nothing at M0 |
| failed-flush behaviour | [F15 §3.3] FM-3.9 | measurement 18, deferred | item (3) keeps its widest reading |
| Unknown-boot mode on Windows | [F15 §3.7] FM-7.4 | measurement 22 (WP-52), through `OS-win-boot-source` | the model contains both modes |
| `files.read-budget-ms`, `files.session-start-cap-ms`, `files.links-sync-ms` | [CFG] open point 24 | design-fixed; they become holes `CFG-files-read-ms`, `CFG-files-session-start-ms`, `CFG-files-links-sync-ms` (WP-55) only if the review reads [60 §5.2] row 15 so | none at present |
| the range bound of `pack.cli.max-bytes` | [CFG] open point 25 | measurement 7's inline cap | the review lowers the bound if the cap is below 28,000 characters |
| `tx.max-work-in-lock`, `pack.budget.<role>`, `query.budget.default.fs` | [CFG] open point 26; [reviews/a1-dispositions]; [RULES/pack-classes] Holes | M7's work-unit calibration; M9's recorded-dispatch test | provisional configuration values, not M0 holes |

## Open points for the review

1. **Hole-id form** — closed in review pass 1 (S1-40, P1-33, A1-42). The seven ids of §4 outside the `<part>-<name>` form
   of [F01 §2.5] (six aliases and `share-retry-ms`) are renamed in every chapter that used them ([F01] open point 15);
   only the review record [reviews/a1-dispositions] keeps `r4-clock-skew`.
2. **Decided rule-file holes** — closed in review pass 1 (S1-40). [RULES/pack-classes] NR-001 and [RULES/role-write-policy]
   WQ-004, WZ-004, WZ-005 and WZ-010 now write the values of §3, and their Holes sections say "None", so WP-81a's list
   is exactly §2.
3. **Constraints added by the A1 re-review.** [reviews/a1-dispositions] adds constraints to `F17-ckpt-ops`,
   `F17-ckpt-bytes` and `F17-tail-overlay` (measured with a product-shaped overlay and the lazy-record share at the quiet
   cap, A1P-03); WP-81a applies them from there and from [F17]'s Holes table.
