# 17 — Store parameters

| | |
|---|---|
| Title | Store parameters: every threshold that switches a code path of the storage engine, init-fixed in `HEAD` or tunable as a store-scope `config` key; production values (named holes where an M0 measurement decides them) and the test profile |
| Chapter | [F17], `docs/spec/format/17-store-parameters.md` |
| Status | draft, pass 1 pending |
| Work package | WP-16c, the store-parameter part of WP-16 ([PLAN §3.2] item 1), author role R-SPEC-P |
| Sources | [60 §2.5]: the rows "`HEAD`" and "Log" of the [AR] rows, the row "Store parameters" of the issue-2 rows, the audit rows "Commit body", "Segments" and "Ref table", and the paragraph "New store parameters" that follows the audit rows; [60 §3.1] "Decisions fixed at M0 exit"; [60 §4.3]; [60 §4.4] items 1, 2 and 8; [60 §5.1]; [60 §5.2] rows 2, 3, 5, 6, 8, 10, 12 and 14. [AR §2.2] (quiet cap), [AR §2.3] (promotion), [AR §2.5] (the `suspect` budget), [AR §2.11] (FTS tier 2), [AR §2.15] (loose/pack), [AR §3.5] (`suspect`), [AR §4.1] (files, extents, `hist`, `cs`, `dict`, the deletion grace), [AR §4.2] (`HEAD` flags), [AR §4.3] (commit size bound), [AR §4.5] steps 4, 7 and 12, [AR §4.7], [AR §4.9], [AR §5a.3], [AR §5a.7] step 6, [AR §5b.6], [AR §6.4], [AR §6.6], [AR §8.1]–[AR §8.3] (RAM and SPEED rows), [AR §13] (tables "Store parameters", "Durability, quiet mode, maintenance, locks, leases, retention", "Memory", "Queries and `TX`", and "Never a key"). [74 §5.1] item 5 (A10); [71] RAM-B1, RAM-M5, RAM-M6, RAM-m3; [70] S1, S9, S15; [20] G10, G16, G26; [80 §2.2.1], [80 §2.2.3], [80 §2.3.3], [80 §2.4.3], [80 §3] X-F3 and X-F11; [50 §5.8], [50 §5.10], [50 §8.1] F12, F15, F16; [90 §11.3] |
| Depends on | [F01]; cites [F02], [F04], [F05], [F06], [F09], [F10], [F11], [F13], [F15], [F16], [F19], [F20], [CFG], [API] |

This chapter lists every threshold that switches a code path of the storage engine. For each one it gives the key, the
type and unit, the valid range, the production value, the test-profile value, the decision point and what the value
counts. Production values that an M0 measurement decides are named holes ([F01 §2.5]), listed in the Holes section.
The configuration syntax, precedence, unknown-key rule and registry format are [CFG]'s. The protocol that uses the values
is [F16]'s. The byte layouts of the structures the parameters compare against belong to [F04], [F05], [F06], [F09], [F10]
and [F11].

## 1. Scope and conventions

### 1.1 What a store parameter is

A **store parameter** is a value that switches a code path of the storage engine: a trigger (a checkpoint, a fold, a
promotion, a rollup, a retirement), a bound (a commit size, a frame size, a pack size, a lock wait), a fallback (full Kahn,
tier-2 search), or a retention window. By [60 §2.5] every such value is format-visible and is never a compile-time
constant. It has exactly one of two reload classes ([74 §5.1] item 5, [AR §13]):

- **init-fixed**: chosen at `init`, recorded in `HEAD` (§2), and never changed afterwards;
- **tunable**: a store-scope `config` key with reload class `hot`, read at the parameter's decision point (§1.4).

Two further groups sit at the edge of this definition, and §13 lists both. The first group is configuration keys that
switch a path outside the storage engine, such as query and file-link budgets, caches and hook bounds. [CFG] owns them,
with [50] and [40]. The second group is layout and protocol constants that [AR §13] "Never a key" or a frozen layout fixes.

### 1.2 What this chapter fixes, and precedence

For every store parameter, this chapter is normative for its meaning (what is counted and compared), its valid range, its
decision point, its production value (or the hole that decides it), its test-profile value and its semantic visibility.
[CFG] registers the key: its registry row, type spelling, scope and reload class. Where [CFG] and this chapter disagree on
a store parameter's meaning or range, this chapter wins. On the registry format, [CFG] wins.

### 1.3 Production values: design-fixed or measured

Every production value is of one of two kinds:

- **D (design-fixed).** The design states the value, and neither [AR §8.2] nor [60 §5.2] names an M0 measurement that
  decides it. WP-81a records the value unchanged in [AR §13], together with the filled holes. A later measurement can
  change a D value only through the revisit path of the design section that set it.
- **M (measured).** [AR §8.2], [60 §5.2] or [60 §3.1] name an M0 measurement that decides the value. The production value
  is written as HOLE(<id>). The design's own figure is listed as the leading candidate in the Holes section. It is never
  used as the production value before WP-81a fills the hole.

[60 §3.1] fixes "the production value of every store parameter" at M0 exit. For D values, WP-81a fixes them by recording
them (OP-17-01).

### 1.4 Decision points

A process reads a tunable parameter when it makes the decision the parameter governs. It uses the configuration it holds
at that moment. A CLI or hook process reads configuration once per command. The MCP server re-reads it on its next request
after `HEAD.config_gen` changes ([AR §13]). Each parameter's section names its decision point.

- **SP-R1 (no retroactive effect).** A changed value takes effect at the next decision point. Segments, frames, packs,
  records and runtime rows written under an earlier value stay valid. None of them is rewritten because a value changed.
- **SP-R2 (decoding never depends on a tunable value).** No reader consults a tunable parameter to decode or validate a
  stored structure. The sizes, counts and bounds that a decoder needs are either stored in the structure itself (for
  example `total_len`, frame headers, `cs_ref.len`) or init-fixed in `HEAD` (§2).
- **SP-R3 (init-fixed values come from `HEAD`).** A process reads init-fixed parameters only from the `HEAD` slot it uses,
  never from `config`, the environment or a flag (§2.2 IP-4).

### 1.5 Semantic visibility

Each parameter is marked with one of three classes in §3:

- **I (invisible).** The value changes only physical layout, time or private memory. It never changes a `Store` API
  result, an exit class, a commit id or a `state(ref)` digest. The reference model ignores it.
- **V (visible).** The value changes results that the `Store` API exposes. The reference model implements it with the
  model function named in the parameter's section.
- **Rs (resource).** Beyond the value, a command is refused with a resource-class refusal (E501 from the write-size switch
  of §4.4, or exit 7 on a lock-wait timeout, §10). The outcome depends on engine memory accounting or on timing, which the
  reference model does not model ([60 §4.3]).

Invariance property:

- **SP-1.** Take one `Store` API command stream and two profiles that differ only in the values of class-I parameters. The
  engine produces the same exit classes, result data (the `--json v1` `data`, engine-internal fields excluded, [API]),
  commit ids and `state(ref)` digests under both.
- **Gates for SP-1.** GT2 draws every run's parameters from the test profile or the production profile ([60 §4.4] item 1)
  and compares both against the model; this is mandatory from M1 and extended per milestone. Nightly GT3 sweeps the
  production values at 1e5 with the engine's self-check ([60 §4.4] item 8).

Resource refusals in the differential:

- **SP-2.** A resource-class refusal writes nothing ([AR §4.5] step 4; E501 "nothing written", [50 §5.10]). When the engine
  returns one, the GT2 harness applies nothing to the model for that command, and it counts the refusals per run. The model
  never predicts a resource-class refusal.
- **Deterministic refusals stay compared.** Refusals that come from deterministic caps (`tx.max-statements`, `tx.max-ops`)
  are compared like any other result.
- **`wmem` under the test profile.** Under the test profile, `wmem` is the caller's `query.caps.<role>.wmem` ([CFG §10.5])
  instead of being derived from RSS headroom ([AR §4.5] step 4). A run therefore draws the same `wmem` every time
  (OP-17-12; pass 1, P1-11: `tx.wmem-max` is retired, [CFG §6.4]).

### 1.6 Units and clocks

- **Sizes** are bytes. KiB = 1,024 B and MiB = 1,048,576 B. **Percentages** are integers.
- **Durations** of one second or more are measured on the store's HLC, in its millisecond part `hlc_ms(h) = h >> 16`
  ([F01 §5.7]). The elapsed time of a window is `hlc_ms(now) − hlc_ms(t0)`, where `now` is the HLC the deciding process
  reads by [F16]'s rule and `t0` is the `append_hlc` ([F06], [50] F14) of the record that opened the window. Each window's
  opening record is named in §11.
- **Clock steps.** A backward wall-clock step pauses such a window, because the HLC does not go backward. A forward step
  shortens it. [F16] decision (e) states when that is acceptable (OP-17-10).
- **Lock waits** (§10) are measured on the waiting process's monotonic clock ([80 §2.2.1] item 5).
- **Test profile.** Under the test profile, every clock is the injected deterministic clock of [60 §4.4] item 1.

## 2. Init-fixed parameters: the `InitParams` block and `project_oid_algo` in `HEAD`

### 2.1 Layout

`InitParams` is 32 bytes and byte-packed ([F01 §4.3]); its integers are little-endian ([F01 §4.1]). [F04 §4.4] places it at
slot offset 1024 ([AR §4.2]; [60 §2.5] "`HEAD`" row, "the store parameters of the row below"), and the slot's `xxh3_128`
covers it. Offsets are relative to the first byte of the block. The layout is [F04 §4.4]'s, restated here for the
parameters' meanings (pass 1, A1-13, S1-12, P1-4).

| offset | width | type | name | meaning |
|---|---|---|---|---|
| 0 | 8 | `u64` | `log_extent_bytes` | `store.log-extent-bytes` (§4.1): the length of every `log.<n>` extent; a power of two in [2^16, 2^30] |
| 8 | 4 | `u32` | `hist_frame_commits` | `store.hist-frame-commits` (§4.3): the most commits in one `hist` frame; 1 to 65,536 |
| 12 | 4 | `u32` | `hist_frame_bytes` | `store.hist-frame-bytes` (§4.3): the most raw bytes in one `hist` frame, and the block size of a split frame; 4,096 to 1,048,576 |
| 16 | 16 | `b16` | `store_id` | the store id ([F02 §4]); never all zero (IP-2) |
| total | 32 | | | |

One further value is init-fixed and lives beside the block: **`project_oid_algo`**, the `u8` at slot offset 1072
([F04 §5.16]) that holds the `project` root's content-hash algorithm A(`project`) of [F20 §2.3] (1 `sha1`, 2 `sha256`).
It is not a configuration key and has no `--set`: `init` takes it from the repository's `extensions.objectFormat`, or 1
without a repository ([CFG §7.6]). It is not a threshold, so it has no row in §3 (pass 1, A1-15, S1-28, P1-4).

### 2.2 Rules

- **IP-1 (written once).** `init` writes the block and `project_oid_algo` into both slots. Every later publish is a
  read-modify-write of the newest valid slot ([80 §2.4.3]), so it copies them unchanged. `restore` and
  `repair --rebuild-from-log` keep the source store's values, the store id included ([F02 §4]). No verb changes an
  init-fixed value. A different value needs a new store, filled by an image import.
- **IP-2 (validated on read).** A process that selects a slot validates the block and the byte: every parameter lies in
  its range (§3), `store_id` is not all zero, and `project_oid_algo` ∈ {1, 2}. If a slot has a valid checksum but a block
  or byte that fails this check, the process exits 7, naming `HEAD` and `moirai doctor --fsck`.
- **IP-3 (both slots agree).** When both slots are valid, their blocks must be byte-identical and their `project_oid_algo`
  equal. If they differ, the process exits 7 in the same way.
- **IP-4 (never from `config`).** `moirai config set` of an init-fixed key is refused with exit 2. If a hand-edited `config`
  file names one, `config check` and `doctor` report the value as ignored. The `HEAD` value governs.
- **IP-5 (source at `init`).** `init` takes each value from its command line ([CFG] names the flag), or else uses the
  production value. The test harness passes the test profile (§12).
- **IP-6 (derived format facts).**
  - Every `log.<n>` file is exactly `log_extent_bytes` long ([80 §2.3.3]). A different length found at open or by
    recovery is corruption — exit 7, then `doctor --fsck` — for an extent at or below the one that holds the end of the
    valid log, and for a longer file anywhere. A shorter file beyond that extent is an interrupted preparation, which the
    next rotation re-prepares ([F05 §2.2], [F16] P-72; pass 1, S1-24, A1-24).
  - `hist` framing follows §4.3 with the block's two values.

## 3. Parameter table

Class: `init` = init-fixed (§2); `hot` = tunable store-scope key. The Vis. column holds the visibility class of §1.5. A
production value written HOLE(…) is listed in the Holes section, with the design's figure in brackets. Keys marked
*(new)* were fixed by the design as constants without a key; this chapter registers them (OP-17-03).

| # | Key | Class | Type (unit) | Valid range | Production | Test profile | Vis. | § |
|---|---|---|---|---|---|---|---|---|
| P01 | `store.log-extent-bytes` | init | size | a power of two, 2^16 to 2^30 | 64 MiB (D) | 64 KiB | I | 4.1 |
| P02 | `store.log-active-extents` *(new)* | hot | int | 1 to 64 | 4 (D) | 2 | I | 4.2 |
| P03 | `store.hist-frame-commits` | init | int | 1 to 65,536 | 256 (D) | 4 | I | 4.3 |
| P04 | `store.hist-frame-bytes` | init | size | 4 KiB to 1 MiB | 1 MiB (D) | 4 KiB | I | 4.3 |
| P05 | `store.commit.inline-max-bytes` | hot | size | 4 KiB to P01 / 8 | 1 MiB (D) | 4 KiB | I; Rs for agent verbs | 4.4 |
| P06 | `store.checkpoint.ops` | hot | int | 1 to 2^20 | HOLE(F17-ckpt-ops) [4,096] | 8 | I | 5 |
| P07 | `store.checkpoint.bytes` | hot | size | 1 KiB to 2^30 | HOLE(F17-ckpt-bytes) [4 MiB] | 2 KiB | I | 5 |
| P08 | `store.checkpoint.body-bytes` | hot | size | 1 KiB to 2^32 | HOLE(F17-ckpt-body) [32 MiB] | 8 KiB | I | 5 |
| P09 | `store.tail.max-overlay-bytes` | hot | size | 512 B to 2^26 | HOLE(F17-tail-overlay) [1 MiB] | 1 KiB | I | 5 |
| P10 | `store.tail.max-overlay-bytes.quiet` | hot | size | P09 to 2^26 | HOLE(F17-tail-overlay-quiet) [2 MiB] | 2 KiB | I | 5.3 |
| P11 | `quiet.tail-cap-multiplier` | hot | int | 1 to 64 | HOLE(F17-quiet-mult) [8] | 2 | I | 5.3 |
| P12 | `store.tail.runtime-bytes` | hot | size | 512 B to 2^26 | 2 MiB (D) | 1 KiB | I | 5.4 |
| P13 | `maintenance.cli-threshold-multiplier` | hot | int | 1 to 16 | 2 (D) | 2 | I | 5.2 |
| P14 | `store.fold-width` | hot | int | 1 to 5 | 3 (D) | 2 | I | 6.1 |
| P15 | `maintenance.rollup-threshold` | hot | percent | 1 to 1,000 | 25 (D) | 25 | I | 6.2 |
| P16 | `store.dict.train-sample-bytes` | hot | size | 4 KiB to 2^26 | 4 MiB (D) | 4 KiB | I | 6.3 |
| P17 | `store.dict.retrain-growth` *(new)* | hot | percent | 1 to 1,000 | 25 (D) | 25 | I | 6.3 |
| P18 | `store.fts.tier2-nodes` | hot | int | 1 to 2^32 − 1 | 20,000 (D) | 16 | I | 6.4 |
| P19 | `store.promotion.overlay-ops` | hot | int | 1 to 2^32 − 2 | HOLE(F17-promo-ops) [3,000–5,000] | 16 | I | 7 |
| P20 | `store.promotion.overlay-bytes` | hot | size | 1 KiB to 2^32 − 2 | HOLE(F17-promo-bytes) [1 MiB] | 1 KiB | I | 7 |
| P21 | `store.promotion.age-checkpoints` | hot | int | 1 to 65,536 | HOLE(F17-promo-age) [16] | 2 | I | 7 |
| P22 | `store.kahn-fallback-edges` | hot | int | 0 to 2^32 − 1 | 1,000 (D) | 4 | I | 8.1 |
| P23 | `store.suspect-budget` | hot | int | 1 to 2^32 − 1 | 10,000 (D) | 4 | V | 8.2 |
| P24 | `store.image.loose-pack-threshold` | hot | int | 0 to 65,536 | HOLE(F17-loose-pack) [8] | 2 | I | 9.1 |
| P25 | `store.pack-objects-max` | hot | int | 16 to 2^24 | 65,536 (D) | 16 | I | 9.2 |
| P26 | `lock.writer-wait-ms` | hot | int (ms) | 100 to 60,000 | HOLE(F17-lock-writer) [2,000] | as production | Rs | 10 |
| P27 | `lock.flush-wait-ms` | hot | int (ms) | 100 to 60,000 | HOLE(F17-lock-flush) [2,000] | as production | Rs | 10 |
| P28 | `idempotency.retention` | hot | duration | P29 to 3,650 d | 30 d (D) | 1 h | V | 11.1 |
| P29 | `idempotency.default-window` | hot | duration | 1 s to P28 | 10 min (D) | 1 min | V | 11.1 |
| P30 | `gc.reflog-expire` | hot | duration | 1 h to 3,650 d | 90 d (D) | 2 h | V (through `gc`) | 11.2 |
| P31 | `gc.cruft-delay` | hot | duration | 0 to 3,650 d | 14 d (D) | 30 min | V (through `gc`) | 11.2 |
| P32 | `gc.trash-expire` | hot | duration | 0 to 3,650 d | 14 d (D) | 30 min | I | 11.3 |
| P33 | `gc.fileobs-idle-expire` | hot | duration | 1 h to 3,650 d | 30 d (D) | 1 h | I (R4 subset-consistent, §11.3) | 11.3 |
| P34 | `gc.delete-grace` *(new)* | hot | duration | 0 to 1 h | 60 s (D) | 1 s | I | 11.4 |

Cross-parameter constraints (pass 1, P1-12). `init` checks them over its `--set` values and the production values of every
other key, and refuses a violating combination with exit 2 (`config_value` naming the constraint) before anything is
created ([CFG §7.6], [API §8.1]). `config set` and every read check them for tunable values; a violating tunable value
falls back to min(its production value, the largest value the constraints admit under the recorded init-fixed values)
([CFG §5.3] step 2) and is reported as an invalid value ([AR §13]), so a fallback never breaks a constraint on a store
whose init-fixed values are not the production ones:

- **C-1.** `P05 ≤ P01 / 8`, and every log group fits one extent (§4.4 W3).
- **C-2.** `P09 ≤ P10`.
- **C-3.** `P29 ≤ P28`.
- **C-4.** `2 + P14 + n_other ≤ 8`. Here `n_other` is the number of `HEAD.segments` entries that [F04] reserves for segment
  kinds other than `main`'s base and deltas (OP-17-06), and the further 1 is the one yield delta that a long maintenance
  holding may add above P14 ([F16] P-98; pass 1, P1-9). With `n_other` = 1, P14 ≤ 5.

## 4. Log, history and commit size

### 4.1 `store.log-extent-bytes` (P01)

- **Meaning.** The fixed length of every log extent ([AR §4.1], [80 §2.3.3]). It is init-fixed because an extent's length,
  and so the mapping from an `lsn` to an extent and an offset, must not change during the store's life ([F05] defines that
  mapping). A power of two keeps the mapping a shift and a mask (OP-17-04).
- **Decision points.** Extent creation and rotation. A group that does not fit into the remainder of the active extent
  causes a rotation. The old extent is padded with one lazy `Noop` group that ends at its last byte ([80 §2.4.3]), and the
  next extent opens with its extent-head group ([F05 §4.4] G-3, §4.5). [F05 §4.4] G-4 leaves 0 or at least 40 bytes at
  every group boundary, so the pad always has room for a minimal `Noop` group (pass 1, S1-18, P1-8).
- **Visibility.** I.
- **Test value 64 KiB** ([60 §2.5]). At model scale (≤ 1e4 commands of ≈ 0.35–0.5 KB, [AR §4.3]) this gives dozens of
  rotations and retirements per run.

### 4.2 `store.log-active-extents` (P02, new)

- **Meaning.** The number of unretired log extents that are kept before the oldest is retired into `hist.<n>`
  ([AR §4.1]: "≤ 4 kept as active history before retirement"; [AR §4.9] "History retirement").
- **Retirement rule.** At a delta checkpoint (the decision point), while more than P02 extents are unretired, the
  maintenance holder retires the oldest unretired extent n, provided every record in it lies before the new
  `checkpoint_lsn` ([F05 §2.5] EX-4) and that `checkpoint_lsn` > n·E, so at least one group of extent n + 1 is folded too
  (EX-5). An extent that holds a record at or after `checkpoint_lsn` is never retired. Retirement follows [AR §4.9] and
  [F16] P-73 (pass 1, S1-49).
- **Visibility.** I. **Test value 2**, so that retirement happens in every run of more than a few hundred commits.

### 4.3 `store.hist-frame-commits`, `store.hist-frame-bytes` (P03, P04)

- **Meaning.** Retirement packs the records of an extent, in `lsn` order, into compressed frames ([AR §4.1], [F10]).
- **Frame and split rules.** [F10 §4.2] is the one statement of the cut: P03 bounds the `Commit` records of a frame, P04
  bounds its raw bytes counting every record, and a record above P04 is a split frame of blocks of at most P04 raw bytes
  each ([AR §4.1], [71] RAM-B1). This section gives only the two values (pass 1, S1-29, P1-30).
- **Frame header.** The frame header states the frame's counts and bounds ([F10], [50] F8). A reader therefore decodes a
  frame without consulting P03 or P04 (SP-R2). The init-fixed values bind the writer only.
- **Why init-fixed.** They are init-fixed by [AR §13].
- **Range.** The upper bound of P04 (1 MiB) keeps the RAM gate "decoding one commit ≤ 2 MB" ([AR §8.3] RAM) true for every
  valid value.
- **Visibility.** I. **Test values 4 commits and 4 KiB**, so that the count bound, the byte bound and the split rule all
  decide some frames at model scale.

### 4.4 `store.commit.inline-max-bytes` (P05) and the write-size switch

**Counted quantity.** `cs_bytes(c)` is the byte length of commit `c`'s changeset part as [F06] encodes it: `n_ops`, the ops
with their before-images and `prev` deltas, and the body payloads the commit record carries. Header fields, the message,
`affected` and the absorbed vector are not counted (OP-17-05).

**Decision point.** Phase 1 of every write, when the candidate is serialised ([AR §4.5] step 4).

The write-size switch has four rules:

- **W1.** If `cs_bytes(c) > P05`, `c` is a **bulk commit** ([AR §4.3]): its ops are streamed into a sealed `cs.<n>`
  segment, and the `Commit` record carries `cs_ref` and no inline ops. Only verbs of the bulk class may produce a bulk
  commit: image import, `migrate`, `rm --cascade`, a directory `file mv`, `merge` and `merge --continue`, `sync`, `revert`
  and `cherry-pick` ([AR §4.3], [API §9.10]; pass 1, P1-19: `sync` joins the long merges, whose item-10 production
  spills by [F07 §10.6]). For an agent verb (`TX`, `apply`, MCP `write`), W1 is a refusal instead: E501 naming the
  split, with nothing written ([AR §4.3] "Agent-facing verbs never create bulk commits").
- **W2.** Independently of W1, phase 1's working set — the candidate, its canonical form, and every sort a producer holds
  in memory ([F07 §10.6]) — is charged to `wmem`, the write budget of [CFG §10.5]: requested by
  `query.budget.default.wmem` (1 MiB) or a per-call `--budget wmem=<v>` up to `query.caps.<role>.wmem` (4 MiB for agents),
  and effective as `max(256 KiB, min(requested, RSS headroom))` ([AR §4.5] step 4, [50 §5.10]; pass 1, P1-11). Above
  `wmem`, a bulk-class verb switches to the bulk commit and spills its sorts to `tmp/sort.<nonce>` runs ([F07 §10.6]),
  and an agent verb refuses with E501, whose text names the raise ([LQ/errors §5.4]).
- **W3.** Every group appended to the log, bulk commits included, fits one extent. That is, its length is at most P01 minus
  the rotation reserve R = 178 bytes of [F05 §4.4] G-2 (pass 1, S1-18). The recovery scan treats a longer record as
  invalid ([AR §4.3]). C-1 makes W3 hold for every inline group at the test and production scales (OP-17-05).
- **W4.** Both W1 and W2 apply to agent verbs. An agent `TX` whose candidate fits `wmem` (up to the caller's
  `query.caps.<role>.wmem`) but whose `cs_bytes` exceeds P05 is refused under W1. This closes the case in which a raised
  `wmem` above P05 would otherwise admit a changeset that only a bulk commit could hold (OP-17-11). An agent `TX` is
  therefore bounded by P05 of changeset whatever its `wmem`: at the production P05 of 1 MiB and ≈ 190–240 B per op
  ([50 §5.12]) that is ≈ 4,400–5,500 ops, below the default `tx.max-ops` of 10,000. Within that bound a `TX` whose working
  set exceeds the default 1 MiB needs a `wmem` raise, which E501 names ([CFG §10.5]). Whether a default-cap `TX` should
  fit (a larger P05 or a smaller `tx.max-ops` default) is an owner question (OP-17-25).

**Visibility.** I for bulk-class verbs: a bulk commit and an inline commit give the same commit id, state and result. Rs for
agent verbs, because the refusal depends on the stored encoding, which the model does not produce (SP-2).

**Test value 4 KiB.** A merge, `rm --cascade` or import of a few dozen ops at model scale then takes the bulk path. With
P01 = 64 KiB and the model's scale (≤ 2e3 nodes, so an `affected` list of ≤ 8 KB), C-1 and W3 hold.

## 5. Tail, checkpoint and runtime-fold triggers

### 5.1 Tail measures

For the published `HEAD` slot a process last read, the **tail** `T` is the sequence of valid records from `checkpoint_lsn` up
to `committed_lsn` ([F04] and [F05] define both bounds and the `lsn` convention). The measures below are deterministic
functions of `T`:

| Measure | Definition |
|---|---|
| `ops(T)` | Σ `n_ops` over the `Commit` records in `T` that have no `cs_ref`. A bulk commit adds 0, because its changeset is mapped as a delta layer and not replayed ([AR §4.3]). |
| `rec_bytes(T)` | Σ over records `r` in `T` whose kind is not in `K_RT` of `len(r) − body_bytes(r)` |
| `body_bytes(T)` | Σ `body_bytes(r)` over records `r` in `T`. `body_bytes(r)` is the length of the body payloads `r` carries, as [F06] encodes them: raw, or compressed if the body-placement hole of [F10] (measurement 6) puts compressed bodies in the tail |
| `rt_bytes(T)` | Σ `len(r)` over records `r` in `T` whose kind is in `K_RT` = {`FileObs`, `Pending`, `FPrint`, `JournalCursor`, `DirMap`, `TreeReg`, `PrefixEv`, `GitFacts`, `AnchorRes`}. These are the lazy runtime kinds of [40] R-7 that the runtime-only fold targets ([AR §4.5] step 12, [71] RAM-M5) |
| `ovl(T)` | the **overlay charge**: an engine-defined function of `T` alone (not of process history) that is an upper bound of the private bytes the compact overlay of [AR §4.5] step 1 allocates to hold `T`, including its index entries and derived-state ± lists. The overlay allocator count of [AR §8.3] (RAM row "Compact tail overlay") verifies the bound. The model does not compute it, because the parameters that use it are class I |

Lazy records outside `K_RT`, such as heartbeats, cursors (`Lazy`) and `SessionMark`, count in `rec_bytes` (OP-17-07).

### 5.2 Triggers and the process-kind multiplier

A process evaluates the triggers at two kinds of decision point. A CLI or hook process evaluates them in phase 3 of each of
its own writes ([AR §4.5] step 12). The MCP server evaluates them between requests ([AR §4.9], [70] S9), after reads as
well as writes. A CLI or hook process that only reads never evaluates them.

Define `m = 1` in the MCP server and `m = P13` in a CLI or hook process ([70] S15); `q = 1` while quiet mode is off (§5.3).

- **C1, delta checkpoint.** `ops(T) > m·q·P06`, or `rec_bytes(T) > m·q·P07`, or `body_bytes(T) > m·q·P08`, or
  `ovl(T) > P09`.
- **C2, runtime-only fold.** C1 is false and `rt_bytes(T) > m·q·P12`.

When C1 or C2 holds, the process tries the maintenance byte (try only, [80 §2.2.3]). If the byte is busy, another process is
maintaining, and nothing more is done: the holder keeps the tail bounded itself (below, [F16] P-98). Otherwise the process runs the delta checkpoint of [AR §4.9] (C1) or the runtime-only
fold of §5.4 into the runtime-window sections of [F09 §15.1] (C2). Both run outside the writer byte, as [F16] specifies.

The overlay cap P09 is never multiplied by `m`. Every process replays the whole tail into its own overlay, so the cap is a
RAM bound for every process kind ([AR §8.3] RAM: "compact tail overlay ≤ 1 MiB"). A CLI that meets C1 on its `ovl` clause
therefore checkpoints at 1× (OP-17-08).

**Long maintenance holdings** (pass 1, P1-9, P1-43). A busy maintenance byte must not suspend the bound: a rollup, a GC
rewrite or a `backup` copy that holds the byte for seconds would otherwise let the tail, and every process's overlay, grow
without limit. Such a **long job** evaluates C1 with m = 1 (and the quiet cap of §5.3 while quiet mode is on) at every step
boundary — after each output file it seals, or each file it copies — and when C1 holds it runs one **yield checkpoint**
under its own holding before its next step ([F16] P-98): a delta checkpoint that folds the tail, retires and
promotes nothing and releases only the job's own earlier yield deltas, so the files the job reads stay named. The resulting bound: while any maintenance job holds the byte,
`ovl(T)` exceeds P09 (or P10 in quiet mode) by at most the tail that writers append during one step of the job. Measurement
10 (WP-53d) records the step durations of a rollup and a GC at 1e6 on the owner's NTFS volume in background mode, and the
overlay's high-water mark while they run, which must stay within the RAM row's 1 MiB plus that margin.

"Exceeds" is strict (`>`) in every clause. A **full tail** in the sense of [AR §4.7] and [60 §5.2] item 10 is a tail at the
threshold. Writers that append concurrently before a checkpoint runs may exceed it by the ops of their own commits.

### 5.3 Quiet mode: the cap

While `HEAD.flags.quiet` is set, or a process holds one of the quiet bytes of `LOCK` (the probe rule of [F03 §3.1]), or
quiet mode is implied by a lane in `measuring` (under `quiet.from-lane-measuring`, [AR §6.6]):

- no delta checkpoint and no runtime-only fold runs below the cap;
- the cap is C1 with `m = 1` for every process kind, `q = P11`, and P10 in place of P09 ([AR §2.2] "8× the tail threshold,
  or at 2 MiB of compact overlay, whichever comes first"; [71] RAM-M5). C2 likewise uses `m = 1`, `q = P11`;
- when the cap is crossed, the process that observes it runs exactly one delta checkpoint (G10). That is the crossing
  writer in its phase 3, or the MCP server between requests. The checkpoint folds the tail as of its start, including the
  tiered fold of §6.1 if that fold is due, and nothing else: no promotion, no rollup, no GC ([AR §6.6]).

Removing the process-kind multiplier in quiet mode is a resolution (OP-17-08). Without it, a CLI would checkpoint only at
16× the threshold, and the overlay cap would be the only bound.

### 5.4 Runtime-only fold

A runtime-only fold seals the `K_RT` records of the tail into the runtime sections of the segment set, the runtime window
that [F09 §15.1] lists (`TREES`, `FILEOBS`, `PENDING`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `PREFIXEV`, `ANCESTRY`,
`GITRENAMES` and `ANCHORRES`; [AR §4.5] step 12, [F11]; pass 1, A1-46). It does not touch graph sections. [F16] gives its record and its
publish. Replay indexes `K_RT` records by key → `lsn` and decodes them only on use ([AR §4.3], [71] RAM-M5), which is why
`rt_bytes` has its own threshold and is not part of `rec_bytes` (OP-17-07).

### 5.5 Constraints that the holes must meet

The production values of P06–P11 must keep these budgets together:

- open ≤ 1 / 1.5 / 3 ms at 1e4 / 1e5 / 1e6 with a full tail, and at the quiet cap ([AR §4.7], [AR §8.3] SPEED and RAM);
- a CLI read on `main` ≤ 4 MB private with the tail at the quiet cap ([AR §8.3] RAM);
- a delta checkpoint ≤ 30 / 50 / 100 ms and ≤ 8 MB ([AR §8.3]).

The Holes section states each hole's own constraint. WP-81a records the chosen values in [AR §13], and [AR §4.7] states
the open gate at the chosen checkpoint threshold ([60 §3.1], E8).

## 6. Segment-set maintenance

### 6.1 `store.fold-width` (P14)

- **Meaning.** The number of live delta segments of `main` that triggers a tiered fold. At a delta checkpoint that would
  leave more than P14 deltas, the maintenance holder folds `d1..dk` into a new `d1`. The new file always gets a new file
  number, never a delete-pending name. The maintenance holder also merges their `blobs` files into one ([AR §4.1],
  [AR §4.9]).
- **Decision point.** Each delta checkpoint.
- **Constraint.** C-4 (the `HEAD.segments[8]` capacity and the open budget of ≤ 8 maps, [AR §4.7]).
- **`gitmap` pages.** P14 also bounds the `gitmap` pages of one (destination, algorithm) pair: a checkpoint that would
  leave more than P14 + 1 of them folds them as a tiered fold does, so a lookup opens at most 1 + P14 pages, or 2 + P14
  while a long maintenance job yields ([F10 §7.1], [F16] P-98; pass 1, P1-22).
- **Visibility.** I. **Test value 2**, so that tiered folds happen every few checkpoints at model scale.

### 6.2 `maintenance.rollup-threshold` (P15)

- **Meaning.** A rollup is due when the sum of the `total_len` values of `main`'s delta segments exceeds P15 % of the
  `total_len` of its base segment (OP-17-09).
- **Decision point.** The same points as §5.2: the MCP server spawns the detached `moirai gc --rollup --if-needed` child
  after its request, and a CLI after its write ([AR §4.9]). The spawn happens when quiet mode is off and
  `maintenance.rollup = auto`. With `explicit`, `brief` and `doctor` warn instead.
- **Rollup location.** A rollup never runs inside the MCP server or a CLI or hook process.
- **Visibility.** I. **Test value 25** (production). The path is reached at model scale because the base is small.

### 6.3 Dictionary training: `store.dict.train-sample-bytes`, `store.dict.retrain-growth` (P16, P17)

Both parameters are inert when `HOLE(F02-dict-file)` ([F02 §5.1], [F10]; measurement 6, [90 §11.3]) decides that no
dictionary exists.

- **Retrain trigger.** At a rollup, the dictionary is retrained when the total raw body bytes stored has grown by more than
  P17 % since the training that produced the current `dict.<D>` ([AR §4.1]: "retrained at rollup when bodies grew > 25 %").
- **Sample cap.** Training reads a sample of at most P16 raw body bytes ([AR §4.9]).
- **Decision point.** Each rollup.
- **Visibility.** I: a different dictionary changes blob bytes, never body content ([AR §4.6]: bodies enter commit ids as
  BLAKE3-128 of the raw bytes).
- **Test values.** 4 KiB and 25.

### 6.4 `store.fts.tier2-nodes` (P18)

- **Meaning.** Full-text tier 2 (`TERMS`, `POST` and, if kept, `DOCLEN`, [AR §4.4], [50] F12) is built once the store's row
  count reaches P18. The count is `HEAD.next_id − 1`. It is monotonic and store-wide, so every branch view uses the same
  tier (OP-17-02).
- **Decision point.** Each delta checkpoint and each rollup.
  - At the first decision point where `next_id − 1 ≥ P18`, the maintenance holder builds tier 2 for the segment it writes.
  - The publish that covers that `Checkpoint` sets `HEAD.flags` bit 1 (`fts_tier2`).
  - From then on, every segment written carries the tier-2 sections, and the next rollup builds them for the base.
  - Bit 1 is never cleared. Raising P18 later does not switch tier 2 off; lowering it switches tier 2 on at the next
    decision point.
- **Mixed segment sets.** A segment set may mix segments with and without tier-2 sections. A reader searches a segment
  without them by the tier-1 scan.
- **Visibility.** I: tier 1 and tier 2 return the same ranking ([50] F12, [AR §2.11]). This holds for the BM25 scorer and
  for the statistics-free scorer that LQ-Bench may choose instead.
- **Test value 16** ([60 §2.5]).

## 7. Branch promotion (P19, P20, P21)

**Eligible refs.** Refs of kind `work` other than `main`, and refs of kind `plan` ([AR §5a.1]). `main`, tags, staging refs
(`merge/*`), `import/*` and `orphans/*` are never promoted (OP-17-13).

**Counters** ([AR §4.2], [F11]):

- `overlay_ops(X)` and `overlay_bytes(X)`: the u32 ref-table counters of what a view of X must replay beyond its promoted
  segment. That covers trunk ops from the pin to the fork, X's own ops, and the ops of every synced window since the last
  promotion ([AR §5a.3]).
- `age(X)`: the number of `main` checkpoint sets sealed after the set that `base_pin(X)` names ([20] G16).

**Trigger.** `overlay_ops(X) > P19`, or `overlay_bytes(X) > P20`, or `age(X) > P21` ([AR §5a.3]).

**Decision points.**

1. **After a `sync` of X that makes the trigger true.** The syncing process promotes X after releasing the writer byte,
   under the maintenance byte. The CLI multiplier of §5.2 does not apply. The MCP server runs the promotion in ≤ 5 ms slices
   ([AR §5a.3], [AR §4.5] step 12).
2. **At each delta checkpoint.** The maintenance holder promotes every eligible ref for which the trigger is true.
3. **After a rollup.** Every live branch pinned to the superseded base is promoted, whatever its counters, so that at most
   two bases stay resident ([AR §4.9], [71] RAM-M1).

In quiet mode no promotion runs unless the command carries `--force` ([AR §6.6]).

**Effect.** A promotion writes `seg.b<ref_id>.<K>`, moves `base_pin` to the newest sealed checkpoint of `main`, and resets the
counters. [F05] and [F11] give the record and the reset. `doctor --verify` compares pin ⊕ ops with every promoted segment
([AR §5a.3]).

**Range.** P19 and P20 are below 2^32 − 1, because the counters are u32 ([AR §4.2]).

**Visibility.** I.

**Test values.** 16 ops ([60 §2.5]), 1 KiB and 2 checkpoints. Each of the three triggers then decides some promotions at
model scale.

## 8. Validation and derived-state bounds

### 8.1 `store.kahn-fallback-edges` (P22)

- **Counted quantity.** For a candidate that adds precedence edges, `e` = the number of added `blocks` and `gates` edges
  plus the number of implied exogenous edges ([AR §3.4] I5′) whose endpoints moved in the candidate.
- **Rule.**
  - If `e > P22`, the acyclicity validator (V03 of [F13 §5]) runs full Kahn over the combined precedence graph. Implied
    edges are derived on the fly from the parent CSR and never materialised ([71] RAM-m4).
  - Otherwise it runs incremental Pearce–Kelly for each such edge ([AR §5a.7] step 6).
  - P22 = 0 means "always Kahn".
- **Decision point.** Every validation of a candidate: plain writes, `TX` deferred validators, merge, sync, import, revert
  and cherry-pick. [AR §5a.7] names the fallback for merges; applying the same rule to every candidate is a resolution
  (OP-17-14).
- **Visibility.** I. Both algorithms decide acyclicity of the same graph, and the violation reports the canonical witness
  that [F13 §5] V03 defines, not an algorithm-dependent path.
- **Test value 4** ([60 §2.5]).

### 8.2 `store.suspect-budget` (P23)

**Sets.** For a commit `c` landing on ref R:

- `S(c)`: the nodes whose `suspect` value on R's view differs between `state_at(first parent of c)` and `state_at(c)`. The
  predicate is [AR §3.5]'s single-hop predicate.
- `A(c)`: the nodes whose value of any other predicate of the set `P_F15` of [F13 §6] differs between the same two states.

**Rule** (for both branches, the `suspect` bitset itself is always maintained completely, so I9 holds):

- If `|S(c)| ≤ P23`: `affected(c) = A(c) ∪ S(c)` and `affected_complete = 1`.
- If `|S(c)| > P23`:
  - `affected(c) = A(c)` and `affected_complete = 0` (I42′, [50] F15, F16);
  - the command's result carries a hint-class record ([AR §5a.8] "hint"), not a `Violation` op, because `Violation` ops
    exist only on staging refs ([AR §4.6]). The hint is `SuspectBudget` (code 131, [F19 §12.3]), which never changes the exit code.
- **Consequence.** A past-view query whose window contains `c` recomputes derived state in full for the queried subgraph
  instead of trusting the stored cone ([50 §5.8]).

This reading resolves the "violation record beyond it" of [AR §2.5] and [20 §7] T5 (OP-17-15). Review pass 1 confirmed it from the correctness side (lens S decision D-4); the owner's sign-off is owner question OQ-P-1, and [AR §2.5] is edited at WP-81a.

**Decision point.** Phase 1 of every write ([AR §4.5] step 4).

**Visibility.** V.

**Model function.** `moirai_model::derived::affected_with_budget(parent: &State, child: &State, budget: u32)
-> (Vec<Id>, bool)`, tagged `spec: [F17 §8.2]`.

**Test value 4.** A delete or edit of a node cited by five nodes then takes the incomplete path at model scale.

## 9. Git image export

### 9.1 `store.image.loose-pack-threshold` (P24)

- **Meaning.** Let `n` be the number of objects an export run writes. If `n ≤ P24`, the run writes loose objects. Otherwise
  it writes packs ([AR §5b.6], [AR §2.15] "packs above ~8 objects"). P24 = 0 means "always pack".
- **Decision point.** Each export run.
- **Visibility.** I. Object ids and refs do not depend on packing, and GT7 `git fsck --strict` holds either way.
- **Test value 2.**

### 9.2 `store.pack-objects-max` (P25)

- **Meaning.** An export closes its current pack and starts a new one when the pack holds P25 objects. This bounds idx
  staging ([AR §5b.6], [71] RAM-m3).
- **Decision point.** Each object appended to a pack.
- **Visibility.** I. **Test value 16**, so that a run at model scale writes several packs.

## 10. Lock-wait bounds (P26, P27)

- **P26, the writer byte.** Every blocking acquisition of the writer byte waits at most P26 ms on the waiting process's
  monotonic clock ([80 §2.2.1] item 5, [80 §2.2.3]).
- **P27, the flush byte.** Every blocking acquisition of the flush byte waits at most P27 ms ([80 §2.4.3] phase 2b step 2).
- **Outcome of a timeout.** It is fixed per protocol point by [F16]: exit 7 with nothing appended in phase 2a; exit 7 with
  outcome `pending` at the flush byte ([AR §4.5] steps 5 and 10).
- **Other roles.** The maintenance byte and the slots are only tried, never waited on ([80 §2.2.3]). They have no bound.
- **Decision point.** Each acquisition.
- **Visibility.** Rs.
- **Test profile.** Both keep their production values, measured on the simulator's virtual clock. The lock-release delays of
  [F15 §3.8] FM-8.1, whose class (b) lies above every configured lock-wait bound, exercise the timeout paths.

## 11. Retention windows and the deletion grace

The opening record of each window is named below. Elapsed time is measured by §1.6.

### 11.1 Idempotency (P28, P29)

- **Opening record.** An `Idem` result ([AR §6.4], [F11] `IDEM`) opens its window at the `append_hlc` of the commit it
  records.
- **`idempotency.retention` (P28).** A lookup ignores an entry whose age exceeds P28, and the command executes as new. The
  first checkpoint fold after expiry drops the entry. The key's type requires P28 to be at least the longest Workflow resume
  ([AR §13]).
- **`idempotency.default-window` (P29).** An entry created under a **default key** matches only while its age is at most
  P29 ([AR §6.4], [72] m5). The default key is BLAKE3 of the namespaced session, the attested agent or actor, and the
  canonical bound AST. `IDEM` rows must therefore record whether a key was a default key ([F11], OP-17-16).
- **Decision point.** Each idempotency lookup: phase 1 step 2 and phase 2 step 6 ([AR §4.5]).
- **Visibility.** V.
- **Model function.** `moirai_model::idem::lookup(key, payload, branch, now) -> Lookup`, tagged `spec: [F17 §11.1]`. It
  implements both windows with the injected clock, together with I14′.
- **Test values.** 1 h and 1 min, so that the generator's clock steps reach both expiries.

### 11.2 Reflog and cruft (P30, P31)

- **Reachability at a `moirai gc` run** ([AR §4.9]): refs, plus reflog entries younger than P30 (window opened by the
  `append_hlc` of the ref move), plus pins.
- **Frame rewriting.** `hist` frames are rewritten to drop unreachable commits older than P31 (window opened by the
  commit's `append_hlc`). Commit headers are kept unless `--prune-headers` is given.
- **`MARKERS_OLD`.** Rows older than P30 are dropped ([AR §4.4]).
- **Decision point.** Each `gc` run.
- **Visibility.** V, through `gc` only: after a `gc`, `undo`, `reflog` and as-of cannot reach an expired entry or a dropped
  commit.
- **Model function.** `moirai_model::gc::reachable_after_gc(dag, refs, reflog, pins, now, expire, cruft) -> Set<CommitId>`,
  tagged `spec: [F17 §11.2]`. The model implements only this reachability rule; physical GC is outside its scope
  ([60 §4.3], OP-17-17).
- **Test values.** 2 h and 30 min.

### 11.3 Trash and file observations (P32, P33)

- **`gc.trash-expire` (P32).** A `trash/<intent>/` directory is purged by `gc` once P32 has elapsed since the `append_hlc`
  of the `FsIntentDone` record of its intent ([AR §4.1], [40 §3.5]).
- **`gc.fileobs-idle-expire` (P33).** `gc` drops the `FILEOBS` rows of every tree whose newest `TreeReg` settle epoch is
  older than P33 ([AR §4.9]).
- **Visibility.** I for the graph. For R4 resolution, a dropped row can only make the engine's answer more conservative
  (E3 has no stored file id), never a different target. That is [40 §8.3.2]'s subset-consistency rule: the production
  cascade returns the model's state or a more conservative one.
- **Test values.** 30 min and 1 h.

### 11.4 `gc.delete-grace` (P34, new)

- **Meaning.** A file that no valid `HEAD` slot names and no pin references is deleted by maintenance or `gc` only when both
  hold:
  - P34 has elapsed since the `append_hlc` of the first `Checkpoint` record whose covering publish stopped naming the file;
  - the two-slot `HEAD` barrier has run ([AR §4.1]: "deleted after `HEAD` has pointed elsewhere for 60 s";
    [60 §2.5] decision (c); [F16]).
- **Other deletion paths.** The grace does not apply to orphan-sweep candidates that no record ever named ([AR §4.1]):
  a numbered file that no slot, record or pin names is claimed by the sweeper's next `Checkpoint` and deleted without the
  grace once that `Checkpoint` passed its identity check ([F16] P-78, P-79). An entry of `tmp/` is deleted by the sweep
  when its last-modification time is more than P34 before the sweeper's wall clock ([F16] P-79), the one use of P34 on a
  file-system time.
- **Decision point.** Each deletion decision.
- **Visibility.** I. A reader that loses the race sees a delete-pending miss, then re-reads `HEAD` and retries, a bounded
  number of times ([AR §4.7]).
- **Test value 1 s.** [F16] decision (e) states the clock basis; this chapter uses the HLC (§1.6, OP-17-10).

## 12. The test profile

The **test profile** assigns a value to every parameter. Its values are the "Test profile" column of §3. Its purpose is that
the reference model and the engine reach every threshold-gated path at model scale: ≤ 1e4 commands and ≤ 2e3 nodes
([60 §2.5], [60 §4.4] item 8). Its rules:

- **TP-1.** A test run draws either the whole test profile or the whole production profile ([60 §4.4] item 1). [CFG]'s
  one-at-a-time and pairwise sweeps apply on top of the production profile.
- **TP-2.** Init-fixed values reach the store through `init` (IP-5), and tunable values through the store's `config`. The
  harness passes the whole profile to `init` with `--set` ([CFG §7.6]), never an init-fixed value alone, so C-1 holds from the
  first command: a test-profile P01 of 64 KiB with the production P05 of 1 MiB is refused by `init` (pass 1, P1-12).
- **TP-3.** Clocks are the injected deterministic clock. `wmem` is the caller's `query.caps.<role>.wmem`, not reduced by RSS headroom (§1.5 SP-2; [CFG §10.5]).
- **TP-4.** The profile is adequate only if GT2's coverage report (section tags, WP-94) shows every path in the table
  below reached in some run of the PR tier. Where a value fails this, the value is changed through a specification finding,
  not in the harness.

| Threshold-gated path | Parameters | Reached at model scale by |
|---|---|---|
| extent rotation, `Noop` padding | P01 | ≈ 60 extents of log per 1e4 commands |
| `hist` retirement, frame bounds, split frames | P02, P03, P04 | 2 active extents; 4-commit, 4 KiB frames; a commit with a body above 4 KiB |
| bulk commit (`cs.<n>`) | P05 | a merge or import of a few dozen ops |
| delta checkpoint by ops, bytes, bodies or overlay | P06–P09 | 8 ops; 2 KiB of records; 8 KiB of bodies; a 1 KiB overlay charge |
| quiet cap and the one bounded checkpoint | P10, P11 | quiet mode on with 2× multiplier and a 2 KiB overlay cap |
| runtime-only fold | P12 | 1 KiB of R4 runtime records (M6 generators) |
| CLI versus MCP multiplier | P13 | simulated client kinds in GT2 and GT3 |
| tiered fold, blob merge | P14 | fold at 2 deltas |
| rollup, dictionary retrain | P15–P17 | small base; 25 % growth |
| FTS tier 2 and mixed segment sets | P18 | 16 rows |
| promotion by ops, bytes or age; after rollup | P19–P21 | 16 ops, 1 KiB, 2 checkpoints |
| full Kahn | P22 | 5 precedence edges in one candidate |
| incomplete `affected` | P23 | a node with more than 4 citers changed or deleted |
| loose objects versus packs; pack split | P24, P25 | exports of ≤ 2 and > 16 objects (M4, M5 streams) |
| lock timeout | P26, P27 | the simulator's lock-delay tail |
| idempotency expiry; default-key window | P28, P29 | clock advances of > 1 h and > 1 min |
| reflog and cruft expiry | P30, P31 | clock advances of > 2 h and > 30 min, then `gc` |
| trash, `FILEOBS` expiry; deletion grace | P32–P34 | clock advances, then `gc` |

## 13. Boundary: what is not a store parameter

### 13.1 Adjacent configuration keys (registered and swept by [CFG])

These keys switch a path, but they bound agent-facing work, caches, hooks or R4 and LQ work, and none decides a stored
layout. [CFG] registers them. Their sweeps are [CFG]'s one-at-a-time and pairwise sweeps ([AR §13] "Sweep plan"), not the
test profile.

| Key | Owner | Note |
|---|---|---|
| `maintenance.rollup` | [CFG] | enum `auto`\|`explicit`; selects who starts the rollup of §6.2 |
| `quiet.from-lane-measuring` | [CFG] | bool; implies quiet mode (§5.3) |
| `durability.lazy-kinds` | [CFG], [F05] | a set; graph mutations are always durable (a rule) |
| `query.budget.default.wmem`, `query.caps.<role>.wmem` (the `wmem` budget; `tx.wmem-max` retired, pass 1, P1-11), `tx.max-statements`, `tx.max-ops`, `tx.max-work-in-lock` | [CFG §10.5], [50 §3.10], [50 §5.10] | `wmem` and `tx.max-*` feed the write-size switch of §4.4; `tx.max-work-in-lock` chooses between a recompute under the lock and a release with a phase-1 re-run ([AR §4.5] step 7) |
| `query.budget.default.*`, `query.caps.<role>.*`, `query.asof.max-ops.cli`, `.mcp` | [CFG], [50 §5.10] | query budgets; the as-of op caps replace [AR §5a.6]'s "within 50k ops" rule with [50 §5.8]'s `mem`-bounded strategy choice (OP-17-18) |
| `mcp.overlay-bytes`, `mcp.overlay-bytes.<client>`, `mcp.overlay-lru`, `git.delta-cache-bytes.cli`, `.mcp` | [CFG] | process-local caches |
| `hooks.sync-auto-keys`, `hooks.delta.max-commits` | [CFG] | hook bounds |
| `files.max-read-bytes`, `files.max-line-hashes`, `files.read.max-uncached-ancestry`, `files.read.max-e6-commits`, `files.read-budget-ms`, `files.session-start-cap-ms`, `files.links-sync-ms`, `mcp.links-sync-slice-ms`, `files.settle.others-after`, `files.pending-escalate`, `files.deep.*` | [CFG], [40] R-13 | R4 budgets; the E6 window bound is an R-14 constant ([F20]), never a key |
| `lease.ttl-default`, `lease.reclaim-older-than`, `lease.orchestrator-ttl` | [CFG] | lease policy on the boot clock ([80] X-F2) |
| `backup.max-age`, `image.export.max-age` | [CFG] | warning and export ages |

### 13.2 Constants that are never parameters

These values switch a path, but a frozen layout or [AR §13] "Never a key" fixes them. Changing one is a format or protocol
change, not a configuration change.

| Constant | Value | Fixed by |
|---|---|---|
| phase-1 re-runs after a failed re-validation; re-runs after a lost group | at most 2 each | [AR §4.5] steps 7 and 10.5; [80 §2.4.3] (group-commit rules are never keys, [AR §13]) |
| `HEAD.seq_ring` entries; `image_cursor` entries; `segments` entries | 32; 4; 8 | [AR §4.2], [F04] |
| cached moves per ref in `REFS` | 32 | [AR §5a.2], [F11] |
| liveness slots; lock-byte offsets | 256; X-F1 | [80] X-F1, [F03] |
| largest commit `seq` | 2^32 − 1 (exit 7 beyond it) | [AR §3.1], [AR §4.5] step 4 |
| `parent` depth | ≤ 12 | I4, [F13] |
| frozen-bitset chunk form | per 65,536-id chunk: a sorted u16 array while ≤ 4,096 members, else an 8 KiB bitmap | [AR §4.4], [F09]: the choice is part of the canonical bytes |
| `wmem` floor | ≥ 256 KiB (the request and its cap are the budget keys of [CFG §10.5]) | [AR §4.5] step 4, [50 §5.10] |
| inline body cap | 64 KiB (larger content is an `artifact` node) | [AR §2.6], [F08] (OP-17-19) |
| free-space check before a sparse extent rotation | 2 × P01 | [80 §2.3.3] (port file systems) |
| spare-extent preparation point | when the end of the valid log reaches offset E / 2 of its extent ([F16] P-96) | [F16] P-96; group-commit and extent rules are never keys ([AR §13]) (pass 1, P1-7) |
| extent-head group; rotation reserve | 138 B; 178 B | [F05 §4.4], §9.28 (a frozen layout) |
| quiescence, E3d, E6 window and every other resolver constant | R-14 | [40] R-14, [F20]; "never a key", [AR §13] |
| `MARKERS` to `MARKERS_OLD` inertness | globally inert markers at each fold | [AR §4.4]: a rule, not a threshold |
| MCP maintenance slice; body decode buffer; project read buffer | 5 ms; 64 KiB; 128 KiB | [AR §4.9], [AR §4.7], [AR §5e.3] (time quanta and buffer sizes; the slice is a GT11 budget, OP-17-20) |

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §2.5] issue-2 row "Store parameters" | complete: every parameter the row names (extent size; checkpoint ops, bytes and body bytes; the quiet-mode cap; tiered-fold width; promotion thresholds by overlay ops, overlay bytes and age; `hist` frame size; the FTS tier-2 node threshold; the Kahn fallback edge count; the `suspect` closure budget; the loose/pack threshold; the idempotency and reflog retention windows), their reload classes, production values or holes, and the test profile | §2–§12 |
| [60 §2.5] paragraph "New store parameters" after the audit rows | complete: `store.commit.inline-max-bytes`, `store.tail.max-overlay-bytes` and `.quiet`, `store.tail.runtime-bytes`, `store.promotion.overlay-ops` and `.overlay-bytes`, `store.pack-objects-max`, `store.dict.train-sample-bytes`, `store.hist-frame-bytes` | §3, §4.3, §4.4, §5, §6.3, §7, §9.2 |
| [60 §2.5] [AR] row "`HEAD`": "the store parameters of the row below" | the init-fixed parameter block and its rules. Its offset in the slot is [F04]'s | §2 |
| [60 §2.5] [AR] row "Log": "of a size that is a store parameter (default 64 MiB)" | the parameter. The extent format is [F05]'s | §4.1 |
| [60 §2.5] audit row "Commit body": "the inline bound `store.commit.inline-max-bytes`" | the bound and the write-size switch. The header fields are [F06]'s | §4.4 |
| [60 §2.5] audit row "Segments": "`hist` frames ≤ 256 commits and ≤ 1 MiB raw" | the two bounds as init-fixed parameters. The frame format is [F10]'s | §4.3 |
| [60 §2.5] audit row "Ref table": `overlay_ops`, `overlay_bytes` | the thresholds that the counters are compared against. The counters' layout is [F11]'s | §7 |
| [60 §2.5] "Protocol decisions" (e): the GC grace and the clocks | the clock basis of the retention windows and the deletion grace, pending [F16]'s decision (e) | §1.6, §11.4 |
| [50] F12 | the tier-2 threshold, its decision point and why it is invisible. The sections are [F09]'s | §6.4 |
| [50] F15, F16 | the `suspect`-budget cause of `affected_complete = 0`. The rule is [F13 §6.3]'s and the header is [F06]'s | §8.2 |
| [80] X-F3 | "a group never spans two extents", as the constraint W3 and C-1 on P01 and P05. The protocol is [F16]'s | §4.4 |
| [80] X-F11 | `lock.flush-wait-ms`, with `lock.writer-wait-ms`: meaning, range and hole. The registry row is [CFG]'s | §10 |
| [40] R-13 | none: the R4 keys are adjacent keys that [CFG] owns | §13.1 |
| [90 §10.1] | none | — |

## Holes

Every hole below is filled by WP-81a from the measurement named in the "Decided by" column, recorded in [AR §13], and given
a fixture by WP-20b (store-parameter values in `HEAD`, [60 §3.1]). Measurement numbers are those of [AR §8.2] and
[60 §5.2]. WP numbers are those of [PLAN §3.2].

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| F17-ckpt-ops | production value of `store.checkpoint.ops` (P06) | measurement 10 (WP-53d); checked by measurement 14's T1 point read (WP-53e) | 4,096 (the design's estimate, [AR §13]); 2,048; 1,024 | open with a full tail at this threshold ≤ 3 ms at 1e6, ≤ 1.5 ms at 1e5 and ≤ 1 ms at 1e4 (warm, store part, [AR §4.7], [AR §8.3]); a point read after a full tail ≤ 50 µs (T1's revisit trigger, [AR §2.1]); the full tail's overlay charge ≤ HOLE(F17-tail-overlay) |
| F17-ckpt-bytes | production value of `store.checkpoint.bytes` (P07) | measurement 10 (WP-53d) | 4 MiB; 2 MiB; 1 MiB | the open budgets above with a tail of this many record bytes, excluding bodies and `K_RT` records |
| F17-ckpt-body | production value of `store.checkpoint.body-bytes` (P08) | measurements 6 (WP-54: codec speed, where bodies are compressed) and 10 (WP-53d) | 32 MiB; 16 MiB; 8 MiB | a delta checkpoint that seals a body tail of this size with the chosen codec meets ≤ 30 / 50 / 100 ms at 1e4 / 1e5 / 1e6 and ≤ 8 MB private ([AR §8.3]) |
| F17-tail-overlay | production value of `store.tail.max-overlay-bytes` (P09) | measurement 10 (WP-53d; overlay allocator count), with measurement 11's heap high-water mark (WP-52) | 1 MiB; 512 KiB | ≤ 1 MiB (RAM row "compact tail overlay", [AR §8.3]); a CLI read on `main` ≤ 4 MB with the tail at this cap |
| F17-tail-overlay-quiet | production value of `store.tail.max-overlay-bytes.quiet` (P10) | measurement 10 (WP-53d) | 2 MiB; 1.5 MiB | ≤ 2 MiB; ≥ HOLE(F17-tail-overlay) (C-2); open ≤ 3 ms at 1e6 at the quiet cap; CLI ≤ 4 MB at the quiet cap ([AR §8.3] RAM) |
| F17-quiet-mult | production value of `quiet.tail-cap-multiplier` (P11) | measurement 10 (WP-53d) | 8 ([AR §2.2], [20] G10); 4 | with HOLE(F17-ckpt-ops) and HOLE(F17-ckpt-bytes): open ≤ 3 ms at 1e6 with a tail at P11 × those thresholds, or at the quiet overlay cap, whichever is reached first |
| F17-promo-ops | production value of `store.promotion.overlay-ops` (P19) | measurement 3 (WP-53a; the 14-day daily-sync fixture) | 3,000; 4,000; 5,000 (est., [AR §5a.3]) | first read of a lane ≤ 10 ms for the 14-day lane synced at every `SubagentStart` and for a 60-day `plan/*` branch; ≤ 3 / 10 / 10 / 20 ms for refs forked 1k / 7k / 14k / 60k commits ago, at the measured overlay build rate ([AR §8.3] SPEED) |
| F17-promo-bytes | production value of `store.promotion.overlay-bytes` (P20) | measurement 3 (WP-53a) | 1 MiB; 768 KiB | a branch overlay in any process ≤ 1 MiB including synced windows ([AR §8.3] RAM), given the measured ratio of `overlay_bytes` to the overlay's private bytes |
| F17-promo-age | production value of `store.promotion.age-checkpoints` (P21) | measurements 3 (WP-53a) and 5 (WP-53c) | 16 ([20] G16: ≈ 1–2 weeks); 8 | the first-read budgets of F17-promo-ops for quiet long-lived branches; ≤ 2 base segments mapped by live branches after any rollup; pinned disk for 50 branches within 25–60 MB at 1e5 ([AR §8.1]) |
| F17-loose-pack | production value of `store.image.loose-pack-threshold` (P24) | measurement 8 (WP-54) | 8 ([AR §2.15]); 4; 16 | an incremental checkpoint export (`main` + 2 lanes) ≤ 50 ms against measurement 8's rename floor ([AR §8.3] SPEED) |
| F17-lock-writer | production value of `lock.writer-wait-ms` (P26) | measurements 2 and 12 (WP-52) | 2,000 ms ([AR §13]); 1,000 ms; 5,000 ms | above measurement 12's maximum lock-release delay after `TerminateProcess` (idle and loaded, writer byte) with margin; no timeout in measurement 2's 16-writer bursts |
| F17-lock-flush | production value of `lock.flush-wait-ms` (P27) | measurement 2 (WP-52), with measurement 12 for the flush byte | 2,000 ms ([80] X-F11); 1,000 ms; 5,000 ms | above the measured flush-byte hand-off latency maximum under load plus one flush p99; above measurement 12's maximum release delay for the flush byte; no `pending` or `store_locked` timeout in measurement 2's bursts, which include extent rotations with a spare prepared ([F16] P-72, P-96) and with the fallback preparation under the flush byte (pass 1, P1-7) |

The following holes of other chapters condition this one. P16 and P17 are inert without a dictionary, which is
`HOLE(F02-dict-file)` ([F02 §5.1], [F10]; measurement 6). `body_bytes` counts raw or compressed bytes according to [F10]'s
body-placement hole (measurement 6).

## Open points for the review

- **OP-17-01 (the D and M split).** [60 §3.1] fixes "the production value of every store parameter" at M0 exit. This chapter
  makes holes of only those values that [AR §8.2] or [60 §5.2] tie to a measurement: the checkpoint thresholds and the quiet
  cap (item 10), the promotion thresholds (items 3 and 5), loose/pack (item 8) and the lock waits (items 2 and 12). The other
  values are design-fixed, and WP-81a records them unchanged. A reviewer who reads [60 §3.1] as requiring every value to be
  measured would turn the D values into holes with no deciding measurement. The chapter rejects that reading, because it
  would leave values undecidable.
- **OP-17-02 (FTS tier-2 count).** [AR §2.11] says "above ~20k nodes" without defining the count. The chapter resolves it as
  `HEAD.next_id − 1`, store-wide and monotonic, with `HEAD.flags` bit 1 set once and never cleared. Every view then uses one
  tier, and a raised threshold never switches tier 2 off.
- **OP-17-03 (keys registered here).** The design fixed three path-switching values as constants without a key:
  - "≤ 4 active extents" ([AR §4.1]), registered as `store.log-active-extents`;
  - "retrained when bodies grew > 25 %" ([AR §4.1]), registered as `store.dict.retrain-growth`;
  - "60 s" deletion grace ([AR §4.1]), registered as `gc.delete-grace`.

  [60 §2.5] forbids compile-time thresholds. [CFG] (WP-18) should register the three keys with the types and ranges of §3.
- **OP-17-04 (extent size).** The range is a power of two from 2^16 to 2^30, because the `lsn`-to-extent mapping is a shift
  and a mask. [F05] (WP-11) owns that mapping and should confirm the range. P01 is init-fixed because the mapping and the
  validity rule "exactly one extent long" ([80 §2.3.3]) rest on it.
- **OP-17-05 (the measure of the inline bound).** `cs_bytes` counts the ops, their before-images and `prev` deltas, and the
  body payloads the record carries, but not the header, the message, `affected` or the absorbed vector. C-1 (`P05 ≤ P01 / 8`)
  is a sufficient condition for W3 at the design's scales. [F06] should confirm the largest non-changeset record size and
  state W3's check in the writer.
- **OP-17-06 (`HEAD.segments` capacity).** The question was whether `HEAD.segments[8]` also lists `dict.D`, `blobs` or
  other kinds besides `main`'s base and deltas. **Closed:** [F04 §4.1] and [F04] open point 4 list only `main`'s base, its
  deltas, the one yield delta of [F16] P-98 and the dictionary, so `n_other` = 1 and C-4 gives P14 ≤ 5, the range of §3
  (pass 1, P1-9; closure NC-4).
- **OP-17-07 (what `rec_bytes` excludes).** The `K_RT` runtime records are excluded from `store.checkpoint.bytes` and get
  their own threshold, so that a large settle triggers only the cheap runtime-only fold ([71] RAM-M5 item 3). The
  alternative is to count them in both. It is rejected because it would restore the eager-decode problem that RAM-M5
  removed.
- **OP-17-08 (multipliers).**
  - The process-kind multiplier `maintenance.cli-threshold-multiplier` does not apply to the overlay cap P09. Every process
    replays the whole tail, so a CLI that waited until 2× the cap would break the ≤ 1 MiB RAM row in hookless harnesses
    that run no MCP server.
  - In quiet mode the multiplier is 1 for every process kind, so the hard cap is the same everywhere.

  [AR §4.5] step 12 does not say either.
- **OP-17-09 (rollup measure).** "Deltas exceed 25 % of the base" ([AR §4.9]) is read as the sum of the deltas' `total_len`
  against the base's `total_len`. An alternative is rows. Bytes are what the rollup rewrites and what the page cache holds.
- **OP-17-10 (clock basis).** Retention windows and the deletion grace are measured on the store HLC, from the `append_hlc` of
  their opening record. A forward wall-clock step can shorten them; a backward step pauses them. [F16] decision (e) (WP-16b)
  says which protocol quantities need the monotonic clock. For the deletion grace, the chapter argues the HLC is safe,
  because a lost race is a bounded retry ([AR §4.7]). The lock waits use the waiting process's monotonic clock.
- **OP-17-11 (the agent-verb bound).** With an agent `wmem` cap of 4 MiB (`query.caps.<role>.wmem`, [CFG §10.5]) and P05 =
  1 MiB, an agent `TX` could fit `wmem` with a changeset that only a bulk commit holds, and agent verbs never create bulk
  commits ([AR §4.3]). Rule W4 refuses it with E501. [50 §3.10] and [F19] should list both causes under E501. Pass 1
  (P1-11): `wmem` is a budget with a raise path; `tx.wmem-max` is retired.
- **OP-17-12 (resource refusals in GT2).**
  - E501 from `wmem` or from the inline bound, and exit-7 lock timeouts, depend on memory accounting and timing. The model
    does not predict them. The GT2 harness skips the command on the model side (SP-2) and fixes `wmem` under the test
    profile.
  - WP-90 (model) and the M1 testkit should adopt the rule. The deterministic caps `tx.max-statements` and `tx.max-ops` stay
    compared.
- **OP-17-13 (promotion eligibility).** Refs of kind `work` other than `main`, and `plan` refs, are promoted. Staging, import,
  orphan and tag refs are not. [AR §5a.3] says "a branch" without naming kinds. Staging and import refs are short-lived, and
  tags are pinned sets.
- **OP-17-14 (the Kahn fallback everywhere).** The fallback applies to every candidate that adds precedence edges, not only to
  merges ([AR §5a.7] names it for merges). A bulk import or a large `TX` benefits the same way. The violation witness must be
  canonical ([F13 §5] V03), or the fallback would be visible.
- **OP-17-15 (the `suspect` budget: major, needs a decision).** [AR §2.5] says "a violation record is written" beyond the
  budget. `Violation` ops exist only on staging refs ([AR §4.6]), and I9 forbids leaving the bitset stale. [50 §5.8] reads
  the budget as the reason why `affected` can be incomplete. §8.2 adopts that reading:
  - the bitset is always complete;
  - `affected` omits the `suspect`-only changes, and `affected_complete = 0`;
  - a hint-class record is returned.

  Rejected alternatives:
  - (a) refuse or stage the commit with a new violation class: it would block deletes of widely cited nodes;
  - (c) truncate the eager maintenance: it violates I9.

  The budget counts `|S(c)|`, which the model can compute, rather than engine edge visits. The review decides. The owner signs
  the rule table that carries it. **Pass 1 (A1-26, lens S decision D-4):** the review confirms the reading (the bitset
  complete for I9, `affected` incomplete with `affected_complete = 0` for I42′, the hint `SuspectBudget` of [F19 §12.3]);
  it stays open only for the owner's sign-off, listed as OQ-P-1 in `reviews/owner-questions.md`, and [AR §2.5]'s
  "a violation record is written" is edited at WP-81a.
- **OP-17-16 (default keys in `IDEM`).** The default-key window (P29) needs each `IDEM` row to record whether its key was a
  default key. [F11] (WP-13) owns the row layout and should reserve the bit.
- **OP-17-17 (GC semantics in the model).** [60 §4.3] puts GC out of the model's scope. Reflog and cruft expiry are still
  visible through `gc` followed by `undo`, `reflog` or as-of. The model implements only the reachability rule
  (`gc::reachable_after_gc`). [API] (WP-25) should confirm that `gc` is a `Store` API command in GT2 streams.
- **OP-17-18 (as-of distance: a conflict between documents).** [AR §5a.6] says "reverse-apply from the nearest later pinned set
  within 50k ops, else replay forward". [50 §5.8] replaces this with a `mem`-bounded choice among three strategies and the
  `query.asof.max-ops.*` caps. By the precedence [50] > [AR] for LQ execution, the 50k-op rule is not a store parameter.
  [AR §5a.6] should be edited at WP-81a.
- **OP-17-19 (the 64 KiB body cap).** [AR §2.6] fixes the inline body cap at 64 KiB, with a revisit trigger that would lower it
  to 16 KiB. It is a data-model limit, visible as a refusal, not a storage threshold. It is listed as a constant of [F08]. If
  the review wants it tunable, it becomes a visible parameter with a model function.
- **OP-17-20 (time quanta).** The MCP maintenance slice (≤ 5 ms) and `mcp.links-sync-slice-ms` are time quanta, not
  thresholds over data. The slice is a GT11 budget (M10) and stays a constant.
- **OP-17-21 (coordination with [F04] and WP-11).** WP-11's gap "the home of `init`-scope parameters in `HEAD`" is closed
  jointly:
  - this chapter defines the parameter set and the 32-byte `InitParams` block (§2);
  - [F04] fixes its offset in the slot's reserved area.

  If [F04] lays the three fields out individually instead, [F04] wins, and §2.1 becomes informative. **Pass 1 (A1-13,
  S1-12, P1-4, A1-15, S1-28):** [F04 §4.4]'s layout, with `store_id` at bytes 16–31, is the one layout, and §2.1 restates
  it; the `project` root's algorithm is the init-fixed byte `project_oid_algo` at slot offset 1072 ([F04 §5.16]).
- **OP-17-25 (a default-cap agent `TX` does not fit the inline bound; owner question OQ-P-2).** [AR §8.3] budgets "a
  default-cap `TX` ≤ 4 MB" of write memory, and [CFG §10.5] lets an agent raise `wmem` to 4 MiB, but W1 and W4 refuse any
  agent changeset above P05 = 1 MiB (design-fixed, [AR §4.3]), which holds ≈ 4,400–5,500 ops, while `tx.max-ops`
  defaults to 10,000 ([50 §5.10]). So the op cap never binds at production values: E501 refuses first. Options for the
  owner: (a) keep both values and document that a `TX` is bounded by P05 (E501 names the split); (b) lower the default
  `tx.max-ops` to what P05 holds (≈ 4,000); (c) raise P05 within C-1 (≤ P01 / 8 = 8 MiB). Recommendation: (a) now, with
  (b) if the owner wants the op cap to be the visible bound. No text of this chapter changes under (a).
- **OP-17-26 (long maintenance holdings; pass 1, P1-9, P1-43).** The review offered two variants: long jobs work outside
  the maintenance byte on a pinned set and take it only to publish, or they keep the byte and yield. This chapter and
  [F16] P-98 take the second: a job keeps the byte for its whole run (so a rollup's inputs, a GC's rewrite and a backup's
  copy set need no new pin holder), and at every step boundary runs a yield checkpoint when C1 holds. A yield checkpoint
  releases none of the job's inputs (only the job's own earlier yield deltas, which it folds), so it never invalidates them; one extra `HEAD.segments` entry holds its delta (C-4, P14 ≤
  5). The bound of §5.2 follows, and measurement 10 checks it.
- **OP-17-22 ([PLAN §3.3] gaps).** [PLAN §3.3] assigns WP-16 two gaps:
  - the `MOVEFILE_WRITE_THROUGH` rule without measurement 17;
  - the widest reading of fault-model item (3).

  Both are closed in [F16] and [F15], the other WP-16 sub-packages. This chapter has no §3.3 gap of its own. It uses the
  fault model only in §10, where the lock-delay injection exercises the timeouts.
- **OP-17-23 (lock waits placed here).** [60 §2.5] does not list the lock waits among the store parameters, but they switch a
  path (exit 7) and their values are M0 holes (WP-81a: "lock bounds"). They are specified here so that one chapter owns every
  hole of this kind. [F16] uses them. The lock-release delay classes that the simulator injects are [F15 §3.8] FM-8.1's.
- **OP-17-24 (coordination with [F02]).** [F02 §5.1] already cites `store.log-extent-bytes` and `store.log-active-extents`
  from this chapter. Two of its statements are fixed numbers that this chapter makes parameters, and should cite them:
  - "at most 3 live before a tiered fold" (the `seg.d<K>` row) becomes "at most `store.fold-width`" (§6.1, production 3);
  - "after 60 s of grace" ([F02 §5.2] rule 3) becomes "`gc.delete-grace`" (§11.4, production 60 s).
