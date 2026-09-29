# Review pass 1, lens P: performance, RAM, Windows and crash safety

| Field | Value |
|---|---|
| Title | Independent review pass 1 of `docs/spec/` (format chapters 01–20, `os/`, `lq/`, `store-api.md` with its examples, `config.md`, `COVERAGE.md`, `HOLES.md`) and `docs/spec/rules/*.md`, lens P |
| Status | review, pass 1; every finding awaits its disposition by the chapter owner named in its fix |
| Work package | WP-80 ([PLAN §3.2] item 8), role R-REV-P. The author of this file wrote none of the reviewed text and did not read the pass-1 findings of lenses S and A |
| Lens | speed on the hot paths (commit path, lock holdings, open path, reads), private bytes and RAM bounds, Windows 11 behaviour (NTFS, Defender, sharing, reparse points), crash and power-loss safety; the style of `docs/research/design/20-critique-perf-ram-windows.md` |
| Reviewed | every file of `docs/spec/` and `docs/spec/rules/`; depth per file in §1 |
| Checked against | [AR] §0, §4 (log, `HEAD`, write path, maintenance, namespace durability), §5b.9, §6.1, §8.2–§8.3 (RAM, SPEED and TOKENS rows), §13; [40 §2.5], [40 §3.4]–§3.6; [50 §3.10], [50 §5.10], [50 §5.12]; [60 §2.5], [60 §4.2]–§4.4, [60 §5.1]–§5.2; [80 §2.2]–§2.12, [80 §3.1]; [90 §2.4], [90 §10.1]; `reviews/a1-dispositions.md` (FB-1–FB-11, FS-1–FS-4, the §5 obligations); `reviews/a1-P.md` (A1P-01–A1P-17) |

Severity, as WP-80 defines it for this pass:

- **blocker**: two independent implementers would produce different bytes, or a rule of the design of record (a signed A1
  disposition included) is violated;
- **major**: an ambiguity, a missing case or an inconsistency that must be fixed before the freeze;
- **minor**: editorial, or a local inconsistency with no byte or rule consequence once settled as proposed.

## 0. Verdict

**CHANGES REQUIRED: 6 blockers, 16 majors, 22 minors (44 findings).**

On this lens the chapters are careful where they are self-contained: every offset table I recomputed sums to its stated
total, every worked example I re-derived is correct (§5), the fault model of [F15] is complete, and the protocol of [F16]
gives every rule a seeded bug. The defects cluster in four places:

1. **Bytes owned twice.** Values, anchor records and commit references (P1-1), the runtime records and the rows they fold
   into (P1-2), the `InitParams` block (P1-4) and `cs_ref`'s digest (P1-6) each have two incompatible layouts in two
   chapters. Two record kinds that the protocol writes have no bytes at all (P1-3).
2. **The HLC.** [F16] P-36 and [API §6.2] CK-4 assign commit HLCs differently, and the HLC is hashed (P1-5); the log also
   loses the HLC floor and the chain anchor that a slot-less `repair` needs (P1-8).
3. **Waits on the commit path and long lock holdings.** Rotation zero-fills 64 MiB inside the flush byte (P1-7); long
   maintenance jobs hold the maintenance byte while the tail grows past the overlay RAM bound (P1-9); the write budget
   `wmem` cannot hold a default-capacity `TX` and has no raise path (P1-11).
4. **Windows and crash safety at the project-file boundary.** Project paths with `:` reach NTFS alternate data streams
   (P1-15); a project volume without directory flush turns every `file mv` into a fail-stop after the rename and blocks
   intent recovery (P1-16); `swap_recover` is run by discovery against [F16] P-86 (P1-13).

Every blocker has a fix that changes no approved architecture. `COVERAGE.md` has no UNMAPPED row, but three of its rows
(60-AR-Values, 60-AR-HEAD-params, R-15) map to sections that contradict each other (P1-1, P1-2, P1-4).

## 1. Method

- **Read completely, bytes recomputed:** [F01]–[F11], [F17], [F04]'s slot table, [F05]'s record tables, [F06]'s example,
  [F07]'s examples, [F08]'s `NodeHdr` and derivation examples, [F09]'s `SegHdr`, [F11]'s `REFS` and `MARKERS` rows,
  `os/*.md`, `config.md`, `HOLES.md`, `COVERAGE.md`, [API] §2–§8, §12, §17, and examples 01 and 18.
- **Read for the lens** (protocol, cost, RAM, crash behaviour, cross-chapter names and codes): [F12]–[F16], [F18]–[F20],
  the other [API] sections and examples, `lq/*`, `rules/*.md` (every row whose basis is `gap` or `proposed`, and the tables
  that carry clocks, budgets or lease liveness).
- Each finding names the chapter that should change. Where two chapters disagree, the fix says which one is made to agree
  and why, from this lens.

## 2. Decisions owed by lens P

1. Every stored structure has **one** owning chapter; the others cite it byte for byte (P1-1, P1-2, P1-4, P1-6).
2. The HLC sequence is advanced only by **semantic durable records** and never falls below any commit the store holds,
   across epochs (P1-5, P1-8).
3. Nothing that can take tens of milliseconds or more runs under the writer or flush byte, and nothing long holds the
   maintenance byte while the tail may outgrow its RAM bound (P1-7, P1-9).
4. Every OS call on a project path is preceded by the Windows name check, and every project-side barrier that can fail
   for lack of a capability is tried **before** the namespace step, not after it (P1-15, P1-16).

## 3. Findings at a glance

| Id | Severity | Where | Summary |
|---|---|---|---|
| P1-1 | blocker | [F06 §5], [F06 §7.5.3], [F08 §5], [F08 §10.3], [F09 §7.2], [F12 §6.3], [F07 §7.3] | values, anchor records and commit refs have two incompatible encodings; `prov` byte missing from the hashed conflict state |
| P1-2 | blocker | [F05 §9.3], §9.5, §9.10, §9.15–§9.19, §9.23, §9.25, §9.26; [F11 §3], §5, §7, §12.4–§12.13; [F18 §3.2], §4.2, §4.10 | runtime records and the rows they fold into disagree; the fold is undefined |
| P1-3 | blocker | [F05 §7], [F05 §9.2]; [F16] P-84, P-70, P-6 | the P-84 reservation record and `RefUpdate` reason `park` have no bytes |
| P1-4 | blocker | [F04 §4.4], [F17 §2.1]; [F20] OP27 | `InitParams` bytes 16–31 are `store_id` in one chapter and reserved-zero in the other; the project root's `oid` algorithm has no field |
| P1-5 | blocker | [F16] P-36, [OS/clock §7], [API §6.2] CK-4 | two HLC rules; `hlc` is hashed, so commit ids diverge |
| P1-6 | blocker | [F06 §9] BK-2, [F09] OP-09-03 | `cs_ref.b3` is defined as two different digests |
| P1-7 | major | [F16] P-8, P-72, OP-3 | rotation zero-fills and flushes a 64 MiB extent inside the flush byte on the commit path |
| P1-8 | major | [F16] P-85, [F04 §8.1], [F05 §4.5], [F10 §4.1] | slot-less `repair` has no durable anchor: the epoch-start group is retired, the pad is dropped, `HEAD`-only fields and the HLC floor are lost |
| P1-9 | major | [F16] P-76, P-87; [F17 §5.2]; [OS/proc §11] | long maintenance holdings defer every checkpoint; the 1 MiB overlay RAM bound has no enforcement |
| P1-10 | major | [F03 §3.1] rule 4 | the quiet-advisory byte's "Busy means quiet is on" is unsound |
| P1-11 | major | [F17 §4.4] W2, W4; [CFG §10.4], §10.5 | `wmem` has no raise path to the 4 MiB agent maximum; the default-cap `TX` does not fit the default `wmem` |
| P1-12 | major | [F17 §3] C-1, [CFG §5.3], [API] example 01 | constraint C-1 at `init` and its fallback are undefined when an init-fixed value is not the default |
| P1-13 | major | [OS/fs §4.9.4] vs [F02 §3], [F16] P-86 | `swap_recover` run by discovery is exactly P-86's seeded bug |
| P1-14 | major | [OS/README §4], [OS/proc]; [F08 §2.2] OP41; [F02 §4], [F04 §5.3], [API §6.4] | no OS seam supplies the cryptographic random source four format values need |
| P1-15 | major | [OS/path §2.1], [OS/project §2.3], [F20 §3], [OS/fs §2.1] | project path segments with `:`, trailing `.` or space reach NTFS through `\\?\` unchecked |
| P1-16 | major | [OS/project §6.2], §6.5; [OS/fs §6.2]; [API §12.4] | a project volume without directory flush fails `file mv` after the rename and blocks intent recovery |
| P1-17 | major | [F14 §14.1], OP-32, OP-33 | `aliases.moi` is rewritten whole per export against the ≤ 50 ms export gate, and the side-ref layout freezes now |
| P1-18 | major | [F16] P-23, [F17 §9.1] | the loose-object export path has no durability sequence or seeded bug |
| P1-19 | major | [F07 §10.5], §10.6; [F17 §4.4] W1 | merge and sync item-10 production has no memory bound |
| P1-20 | major | [F20] OP30, [F08 §11.4] | scope scanners are unspecified, yet `scope` enters the anchor uid |
| P1-21 | major | [RULES/merge-table] MR-005, VA-005, VA-007, VA-008, VA-013; [F13 §5] V05, V07, V12, OP-13-06; [RULES/link-merge-rules] LM-013 | merge rows that raise `SpecGap` and violation classes with no code |
| P1-22 | major | [F17 §9.2], [F10 §7.3], [F10 §5.5], [F09] OP-09-14 | gitmap pages and blobs files are probed one by one with no compaction or locator bound |
| P1-23 | minor | [F01 §5.2] | uvar64 tenth byte "00 or 01" |
| P1-24 | minor | [F03 §6] WD-1 | a 512 B `WriterDiag` write inside every writer holding is unaccounted |
| P1-25 | minor | [F05 §8.4], [F11 §2.5] | two different structures named `FileRef` |
| P1-26 | minor | [F06] OP16, [F16] P-2 | in-lock re-serialisation is O(`cs_bytes`) and not in measurement 2's sweep |
| P1-27 | minor | [F10 §8], OP-10-11; [F09 §16.4] | `cs` "OPS" section named in one chapter, absent from the other |
| P1-28 | minor | [F11 §8]; [AR §4.4], [AR §8.1] | `IDEM` sizing (72 B per slot) differs from the design's 40 B per key |
| P1-29 | minor | [F12 §5], [F12 §7.5] | recursive virtual base and the text merge have no cost bound |
| P1-30 | minor | [F17 §4.3], [F10 §4.2] | the frame rule counts commits in one chapter and records in the other |
| P1-31 | minor | [F19 §10.2], §12.1; [F07 §2.2]; [F14 §6.8]; [F12 §6.1] | `store_locked` names only the writer lock; no code for a busy maintenance byte; `DATA` class has no code |
| P1-32 | minor | [F02 §5.3], §6.3; [OS/env §5]; [OS/project §5.9] | `tmp/probe*` and `tmp/settle.stamp` are outside the `tmp/` grammar |
| P1-33 | minor | [OS/lock §4], §7.3; [F03] Holes; [OS/fs §6.3]; `HOLES.md` §4 | seven hole ids outside the `<part>-<name>` form |
| P1-34 | minor | [OS/clock §4.5], §6; [F16] P-89, OP-8 | the deletion grace is measured on two different clocks |
| P1-35 | minor | [OS/README §4.2] | stale "open for part 2" text on the cross-volume move |
| P1-36 | minor | [OS/mem §3], §6.2 | per-query `private_now` cost on Linux; atomic count per allocation |
| P1-37 | minor | [OS/path §4.1], §5, §7 | no failure mapping for `GetFinalPathNameByHandleW`; drive-relative and device-namespace CLI forms unclassified |
| P1-38 | minor | [OS/project §5.5]; [OS/mapping-appendix §2.1] | the cloud-placeholder gate is checked by path, then the file is opened |
| P1-39 | minor | [OS/shell §5.2], [LQ/lexical §8], [CFG §10] | stdin and `-f` inputs are read whole with no size cap |
| P1-40 | minor | [OS/map §7], OP-3 | no per-process bound or release rule for sealed maps; `RegistryFull` fails the command |
| P1-41 | minor | [LQ/json-ir §2], [LQ/grammar-v1.ebnf] P13 | no JSON nesting limit before the S-AST depth check |
| P1-42 | minor | [LQ/card §7.3], `HOLE(LQ-card-shrink)` | the shrink ladder may not reach 1,000 tokens at the upper estimate |
| P1-43 | minor | [F16] P-87 | `backup` holds the maintenance byte for the whole copy (folded into P1-9's fix) |
| P1-44 | minor | [API §6.2] CK-2, CK-6; [F16 §14] | retention windows after an epoch re-roll need the HLC floor of P1-8 |

## 4. Findings

### Blockers

#### P1-1 (blocker) — Values, anchor records and commit references have two encodings

- **Where.** [F06 §5.1], [F06 §5.5], [F06 §6.2], [F06 §7.5.3]; [F08 §5.1], [F08 §5.2], [F08 §10.2], [F08 §10.3];
  [F09 §7.2] (`EDGE_PROPS`); [F11 §10] (`CONFLICTS`); [F12 §6.3], OP4; [F07 §7.3], OP9; `COVERAGE.md` rows 60-AR-Values
  and R-4.
- **Problem.** One closed type set has two tag registries. [F06 §5.1] `tvalue`: tags 0 `absent` … 14 `pathmove`, `ref` as
  `uvar32`, `commit-ref` as 32 bytes, an empty text or set distinct from absent. [F08 §5.1]: type byte 1 `bool` (value
  bit) … 13, `ref` as `u32`, `commit-ref` as 16 bytes, empty equal to absent. `pathmove.class` is 0–3 in [F06 §5.5] and
  1–4 in [F08 §5.2]. The anchor record of [F06 §7.5.3] (0-based kind, mode and watch codes; `aflags` bit 3 = text
  present; `hint` `uvar32`; the window as `n_before`/`n_after` plus hashes; blob algorithm 0 allowed; no uid) and that of
  [F08 §10.3] (1-based codes; the uid inside; `aflags` bit 5 = `text_unavailable`; `hint` `u32`; the window as `vbytes`;
  blob algorithm ≠ none; another field order) are different structures. [F09 §7.2] `EDGE_PROPS.pinned_commit` is 16
  bytes, so the checkpoint fold op → segment truncates a 32-byte commit reference, and a state rebuilt from segments
  (`doctor --verify`, the `state(ref)` digest) cannot reproduce [F07]'s hashed 32-byte value. The existence-conflict
  `prov` byte that [F12 §6.3] requires in the stored conflict state is absent from [F06 §6.2] and from the hashed form of
  [F07 §7.3]. Two implementers of the log writer and of the segment writer produce different bytes for the same op, and
  the commit ids of an existence conflict differ with and without `prov`. From this lens: the fold also costs a lookup
  per commit ref to widen 16 bytes back to 32 wherever [F07] hashes.
- **Fix.** [F08] owns the value encoding and the anchor record; [F06 §5], [F06 §7.5.3] and [F09 §7.2] cite them byte for
  byte (WP-13 with WP-12 and WP-14). `commit-ref` is 32 bytes in every stored form (log, segments, `EDGE_PROPS`,
  `CONFLICTS`). Empty text and empty set are absent everywhere ([F12] OP27). One set of `pathmove` class codes, 1–4 with
  0 invalid. Add `prov` to [F06 §6.2] and [F07 §7.3] at a stated position. Recompute [F06]'s and [F07]'s worked examples,
  and add one cross-chapter byte fixture per value type and one per anchor mode to rows 60-AR-Values and R-4.

#### P1-2 (blocker) — Runtime log records and the rows they fold into disagree

- **Where.** [F05 §9.3] `ClientHead`, §9.5 `Marker`, §9.10 `RefTable`, §9.15–§9.17 `FsIntent*`, §9.18 `FileObs`, §9.19
  `Pending`, §9.23 `TreeReg`, §9.25 `GitFacts`, §9.26 `AnchorRes`; [F11 §3.1] `REFS`, §5 `HEADS`, §7 `MARKERS`, §12.4
  `TREES`, §12.5 `FILEOBS`, §12.6 `PENDING`, §12.7 `FSINTENT`, §12.12 `GITRENAMES`, §12.13 `ANCHORRES`; [F18 §3.2]
  `BindingExt`, [F18 §4.2], [F18 §4.10]; `reviews/a1-dispositions.md` FB-1; `COVERAGE.md` row R-15.
- **Problem.** Replay folds each record into a row, but the two sides are different structures, so a recovered runtime
  table depends on which chapter the implementer followed:
  - `RefTable` (§9.10) lacks `fork_lsn`, `fork_ref_id`, `ops_total`, `bytes_total`, `trunk_mark_ops`,
    `trunk_mark_bytes`, `moves` and the `pinned` flag of the 164-byte `REFS` row; `ops_since_fork` is `uvar64` against
    `u32`; `tip` is `cid32` against `id16`. Its "complete entry, upsert" semantics would reset, at every ref move, the
    counters that drive promotion ([F17 §7]).
  - `TreeReg`: scope kinds 0 full / 1 lane / 2 partial ([F05]) against 1 full / 2 lane / 3 brief ([F11]; FB-1 names
    "partial"); the sensitivity bit is `case_sensitive` in one and `case_insensitive` in the other; `journal_vol` exists
    only in [F05], the `git` flag only in [F11].
  - `FileObs` and `Pending`: proposals carry `evidence u8` and `score u16 /10000` with a root-qualified path in [F05],
    a `vstr` evidence token, a class and a rational score with a root-less path in [F11], and an evidence code from
    [F18 §5.2] per [F18 §4.10]. `FILEOBS.state` codes are 1 ok, 2 moved-auto, 3, 4 ambiguous, 5 replaced, 6 missing in
    [F11] and 5 deleted, 6 replaced, 8 missing, 9–12 in [F18 §4.2].
  - `FsIntent`: [F05] items are root-qualified paths with a `recursive` flag and its own outcome and abort-reason codes;
    [F11]'s items are root-less `vstr`s with other outcome codes. With the root lost, recovery cannot redo or verify an
    intent on a named root other than `project` (a crash-safety gap, [40 §3.4]).
  - `Marker`: `cause` 1–4 and `outcome` = the `complete --outcome` value in [F05]; `origin` 1–5, `outcome` = the status,
    `orig_ref_id` and `emit_lsn` in [F11].
  - `AnchorRes` and `GitFacts`: different state codes, score forms, and a blob digest and `author_time` present on one
    side only.
  - The binding: [F18 §3.2] `BindingExt` (40 B: `designated` bit 0, `base_algo`, the expected git branch as a **short**
    name symbol, `base[32]`), the [F11 §5] `HEADS` row (flags bit 1 binding, bit 2 designated, `base_commit` as a 33-byte
    `OidSlot`, `git_ref` as the **full** `refs/heads/…` name) and [F05 §9.3] `ClientHead` (`bflags` bit 0 binding, bit 1
    designated, bit 2 `has_dir_id`; `expected_git_ref` `vstr`; `base_commit` `oidv`). [F18 §3.6]'s writer-tree predicate
    compares these bytes, so short against full spelling decides whether a tree may write.
- **Fix.** Adopt [F11] OP-33 as the rule: the payload of every runtime record **is** the row it folds into; [F11] owns
  the row bytes, [F05] frames them (length, kind, `K_RT`) and cites [F11]; [F18] owns the codes and [F11] stores them.
  `RefTable` becomes a partial upsert with an explicit field list (only the fields a ref move changes); counters are
  derived at replay as §10.3 of [F05] says. Paths in `FsIntent`, `Pending` and `FileObs` carry their root symbol. One
  spelling of the expected git ref (full name, as git stores it). Recompute row R-15's fixture.

#### P1-3 (blocker) — The reservation record and the `park` reason have no bytes

- **Where.** [F16] P-84 and OP-5 (asks [F05] for kind 27 `Reserve`), P-70, P-6 (lists the reservation among durable
  kinds); [F12 §8.2], OP14 (`RefUpdate` reason 5 `park`); [F05 §7], [F05 §9.2].
- **Problem.** The bulk-commit protocol appends a durable reservation group before it writes `cs.<n>`, and a staging ref
  is parked by a `RefUpdate` with reason 5, but [F05]'s kind registry ends at 26 and its reason table at 4. Two engines
  invent different bytes, and a scanner of one refuses the other's log as corrupt (an unknown kind below `durable_lsn` is
  `store_corrupt`, decision (b)).
- **Fix.** [F05] adds kind 27 `Reserve` (durable) with its payload (file number, `cs` length bound, the reserving
  process's `ProcId` or slot, `hlc`) and `RefUpdate` reason 5 `park`, with their replay effect; [F16] P-84 and P-70 cite
  them; the seeded bugs of P-84 and P-70 name the bytes.

#### P1-4 (blocker) — `InitParams` has two layouts and misses a field

- **Where.** [F04 §4.4] (`store_id` at bytes 16–31 of `InitParams`); [F17 §2.1] (bytes 16–31 `_reserved`, IP-2 requires
  zero); [F04] OP2 (says WP-16c edits [F17]; not done); [F20] OP27; `COVERAGE.md` row 60-AR-HEAD-params.
- **Problem.** A slot written per [F04] fails [F17]'s IP-2 check and the reverse; `init`, `restore` and `repair` write
  different bytes. Separately, the `project` root's content-hash algorithm is `init`-fixed ([40 §2.5], FB-10, [F20 §2.3])
  but no field records it: after `restore`, `repair`, or on another machine, the resolver cannot know which algorithm the
  stored `oid`s used, and the three-valued membership test of [F20] reads every comparison as "unknown".
- **Fix.** [F17 §2.1] adopts [F04 §4.4]'s layout. Add `project_oid_algo u8` (the [F01 §7.5] registry value) to
  `InitParams` at a stated offset (the 32-byte block has no spare byte after `store_id`, so either grow the block within
  the slot's reserved area, which [F04 §3] has, or narrow a field); [F17] lists it as an init-fixed parameter;
  [CFG §7.6] records it at `init` from the repository's object format.

#### P1-5 (blocker) — Two HLC rules; the commit id depends on which one is used

- **Where.** [F16] P-36 and [OS/clock §7] (`h_last` = the greatest append-time HLC of **every** record in the scanned log);
  [API §6.2] CK-4 and [API] open point 39 (one sequence over **semantic durable records** only; a local commit's `hlc` is
  also above every commit the store holds); [F06 §4.4.4]; [F07 §3.4] (item 3 is hashed).
- **Problem.** Under P-36 a `Checkpoint`, `Lazy` or `SessionMark` record appended in the same millisecond as a commit
  raises the next commit's `hlc`; under CK-4 it does not. `hlc` is canonical item 3, so the engine (following [F16]) and
  the reference model (following [API]) compute different commit ids whenever maintenance or a lazy record runs between
  two commits within one millisecond, or after a backward wall step; a lazy record lost in a crash changes later ids too.
  This contradicts [F17 §1.5] SP-1 (class-I maintenance changes no commit id). Independently, P-36 does not include the
  `hlc` of native imported commits, so a local commit on an imported parent whose `hlc` lies ahead gets
  `hlc < parent.hlc`, which [API] excludes and [F06] leaves open. From this lens: CK-4 is also cheaper, because the
  scanner needs only the last semantic record's HLC, not a max over every record.
- **Fix.** [F16] P-36 and [OS/clock §7] adopt CK-4: `h_last` is the greatest HLC of the semantic durable records
  (`Commit`, `RefUpdate`, `ClientHead`, `Lease`, `Marker`, `Idem`, `Backup`, `FsIntent*`) and of every commit the store
  holds (kept as one `u64` in the `HEAD` fold, so the append needs no scan); other records take `hlc_next` without
  advancing it. Seed the bug "a `Checkpoint` advances the sequence" in P-36's row. The floor must survive epoch re-rolls
  (P1-8).

#### P1-6 (blocker) — `cs_ref.b3` is defined as two different digests

- **Where.** [F06 §9] BK-2 (BLAKE3-128 of the whole file `[0, len)`); [F09] OP-09-03 and [F04 §4.1] (`seg_digest[0..16]`
  from the file's header); [F09 §14.4] `FILES.digest16`.
- **Problem.** One 16-byte field cannot hold both values, although OP-09-03 says a segment "satisfies both". Two engines
  write different `Commit` records for the same bulk commit, and each fails the other's check. From this lens BK-2's form
  is also the costly one: checking it reads the whole `cs` file, while the header form lets the open check V-8 compare
  fields without reading the file.
- **Fix.** [F06 §9] BK-2 adopts `cs_ref.b3 = seg_digest[0..16]`, as [F09] proposes and `FILES.digest16` already does;
  `doctor --fsck` verifies `seg_digest` over the file.

### Majors

#### P1-7 (major) — Rotation prepares a 64 MiB extent inside the flush byte

- **Where.** [F16] P-8, P-72 steps 1–4, OP-3; [OS/fs §4.5] (`ZeroFill` on NTFS); [F17 §4.1]; [AR §8.3] SPEED rows
  (writer hold, last acknowledgement).
- **Problem.** The appender whose group opens extent m holds the flush byte while it zero-fills 64 MiB, flushes it and
  flushes the directory. On NTFS with Defender scanning the new file this is hundreds of milliseconds on an SSD and over a
  second on a slow disk. Every other writer waits in phase 2b for the flush byte meanwhile, so the rotation lands in every
  concurrent writer's last-acknowledgement latency, and with `lock.flush-wait-ms` of 1–2 s (HOLE(F17-lock-flush)) a slow
  disk turns the wait into exit 7 `store_locked`. OP-3's reason for not preparing ahead ("a spare extent prepared ahead by
  maintenance under the flush byte costs the same wait at another moment") does not hold: a file no group has entered
  needs no flush byte to be prepared, because P-72's rule that the first group of m is appended by a flush holder is kept
  if the rotator only **re-issues** `durable+meta` on the already-prepared file and `durable-name` on the directory
  (sub-millisecond on a clean file).
- **Fix.** Maintenance (under the maintenance byte only, outside the writer and flush bytes, in background mode) prepares
  `log.<n+1>` once the valid log passes a stated fraction of extent n (proposal: 50 %); P-78 and the orphan sweep keep a
  prepared spare beyond the valid log; P-72 step 2 becomes "if log.<m> is absent or short, prepare it (fallback), then
  re-issue `durable+meta` and `durable-name`". Add a rotation to measurement 2's workload and to M1's writer-hold and
  last-acknowledgement gates, and seed the bug "a group enters a spare whose `durable+meta` was not re-issued".

#### P1-8 (major) — Slot-less `repair` and restored stores lose their anchors

- **Where.** [F16] P-85; [F04 §8.1]; [F05 §4.5] (epoch-start group), [F05 §2.5] (retirement); [F10 §4.1] (the rotation
  pad is dropped from `hist`); [F04 §9.5]; [OS/clock §7]; [F16 §14].
- **Problem.** `repair --rebuild-from-log` with no valid slot validates the log from the epoch-start group's known chain
  value. That group lives in the epoch's first extent, which retirement deletes after `store.log-active-extents` extents,
  and [F10 §4.1] drops the pad group whose trailer carries the chain value at the next extent's first byte. After the
  first retirement, no chain value is known from which the active log can be validated. The fields that exist only in
  `HEAD` — `InitParams` with `store_id`, `epoch`, `epoch_lsn`, the `readonly`, `retired` and `quiet` flags,
  `config_gen` — are not in the log at all, so they cannot be rebuilt. And after an epoch re-roll (`restore`, `repair`)
  the new tail has no append-time HLC floor ([OS/clock §7] takes "the greatest HLC in the log as scanned"), so on a machine
  whose clock is behind, new commits get HLCs below restored ones and I43′ breaks.
- **Fix.** A durable anchor record (a new kind, or fields of `Checkpoint`) written at every epoch start **and** as the
  first group of every extent: `epoch`, `InitParams`, `store_id`, the persistent flags, the HLC floor of P1-5, and the
  chain value at the extent's first byte. Retirement keeps it in `hist`. [F16] P-85 states the repair algorithm from the
  oldest surviving anchor; a seeded bug covers "repair after one retirement".

#### P1-9 (major) — Long maintenance holdings break the overlay RAM bound

- **Where.** [F16] P-76 (every maintenance job under the maintenance byte, `try_acquire` only; automatic triggers skip on
  `Busy`), P-87 (`backup` copies under it); [F17 §5.2] (C1 skips when the byte is busy; "the cap is a RAM bound for every
  process kind"); [OS/proc §11] (`enter_background`: `PROCESS_MODE_BACKGROUND_BEGIN`); [AR §8.3] RAM row "compact tail
  overlay ≤ 1 MiB".
- **Problem.** A rollup, a GC, a tiered fold or a backup holds the maintenance byte for its whole run, and in background
  mode Windows lowers its I/O and memory priority, so the run is long exactly when the machine is busy. Meanwhile every
  writer that meets C1 finds the byte busy and skips; the tail grows without bound, every process replays it into its
  overlay at open, and the 1 MiB overlay bound and the open-time budget stop holding. No rule caps this.
- **Fix.** Long jobs do their long work outside the byte: they pin a segment set, build their output in `tmp/`, and take
  the maintenance byte only for the publish (the pattern P-84 already uses for bulk commits); or, at minimum, a long job
  releases the byte between steps and runs a pending delta checkpoint itself when `ovl(T) > P09`. State the resulting
  bound in [F17 §5.2] and gate it in measurement 10. Backup (P-87) copies a pinned set and takes the byte only for its
  record.

#### P1-10 (major) — The quiet-advisory byte's rule 4 is unsound

- **Where.** [F03 §3.1] rules 2 and 4; [OS/lock §8] (a Windows probe holds the byte transiently); [PLAN §3.2] item 5 (the
  M0 measurement drivers take the byte).
- **Problem.** "A second requester that gets `Busy` knows quiet mode is already in effect" fails twice: on Windows the
  `Busy` may come from a maintenance decider's probe, which releases the byte a moment later; and the first holder may exit
  while the second requester's measurement still runs. In both cases maintenance resumes under a measurement that believes
  it is quiet, and the M0 numbers that decide HOLE(F17-ckpt-ops) and HOLE(F17-tail-overlay) are taken with maintenance
  noise.
- **Fix.** Reserve a small set of quiet bytes (proposal: `ROLE_BASE + 5 … ROLE_BASE + 12`); a requester holds one of
  them for its whole run (try each; if all are busy, retry with a bounded wait); rule 2's decider probes all of them and
  treats any `Held` or `Unknown` as quiet. [OS/lock §2] adds the bytes to the map.

#### P1-11 (major) — `wmem` cannot hold a default-capacity `TX` and has no raise path

- **Where.** [F17 §4.4] W2 (`wmem = min(1 MiB, headroom)`, ≥ 256 KiB, "at most `tx.wmem-max`") and W4 ("up to 4 MiB with
  `tx.wmem-max`"); [CFG §10.4] `tx.wmem-max`, [CFG §10.5] (nine budgets, none of them `wmem`); [50 §5.10] table (agent
  maximum `wmem` 4 MiB), [50 §5.12] (190–240 B per op); [AR §8.3] row "a default-cap `TX` ≤ 4 MB".
- **Problem.** W2's `min(1 MiB, …)` never exceeds 1 MiB, so `tx.wmem-max` (default 4 MiB) never binds and W4's "up to
  4 MiB" is unreachable; the design's 4 MiB is a per-call agent maximum, but [CFG] has no `wmem` budget, cap or flag to
  raise it. A default-cap `TX` (`tx.max-ops` = 10,000 at 190–240 B per op, 1.9–2.4 MB) therefore exceeds the default
  `wmem` and is refused with E501 although it is within its op cap.
- **Fix.** Make `wmem` a tenth budget: `query.budget.default.wmem` = 1 MiB, `query.caps.<role>.wmem` = 4 MiB (orchestrator
  and owner as [CFG §10.5]'s rule), raised by `--budget wmem=` and the MCP `budget` parameter; W2 becomes
  `wmem = max(256 KiB, min(requested, headroom))`; delete W4's parenthesis and `tx.wmem-max` (or make it the test-profile
  value only, TP-3). Either lower `tx.max-ops`'s default to what 1 MiB holds (≈ 4,000) or state that a default-cap `TX`
  needs the raise; E501's text names which.

#### P1-12 (major) — Constraint C-1 at `init` and its fallback are undefined

- **Where.** [F17 §3] C-1 (`P05 ≤ P01 / 8`; "checked by `init` for init-fixed values"); [F17 §12] TP-1; [CFG §5.3] step 3
  (RG-2: "the defaults satisfy every constraint, so one pass suffices"), [CFG §7.6]; [API §8.1]; [API] example
  `01-init-and-clock.json` (`store.log-extent-bytes=64KiB` with `store.commit.inline-max-bytes` at its 1 MiB default).
- **Problem.** Neither [F17] nor [CFG] says what `init` does when C-1 fails (refuse, fall back, or record), and example 01
  records a 64 KiB extent with a 1 MiB inline bound, which violates C-1 and W3. After `init`, a hand-edited or invalid
  `store.commit.inline-max-bytes` falls back to its production value 1 MiB, which violates C-1 again on any store whose
  init-fixed P01 is not the default: RG-2 assumes every key is at its default, which init-fixed keys are not. The
  consequence is groups larger than an extent (W3 refusals, `commit_too_large`) on test-profile stores and different
  outcomes in two implementations.
- **Fix.** `init` refuses a violating combination with exit 2 naming C-1 ([CFG §7.6], [API §8.1]); the fallback of a
  tunable in a constraint with an init-fixed key is `min(production value, the largest value the recorded init values
  allow)` ([CFG §5.3] step 2); example 01 passes the whole test profile or `store.commit.inline-max-bytes=4KiB` (TP-1).

#### P1-13 (major) — `swap_recover` run by discovery is P-86's seeded bug

- **Where.** [OS/fs §4.9.4] ("run by `doctor` (and by store discovery when it finds `<a>.swap`)"); [F02 §3] ("Discovery
  never runs `swap_recover` itself", open point 17); [F16] P-86 and its seeded bug "discovery runs `swap_recover` on a
  running swap".
- **Problem.** An implementer following [OS/fs] rolls back or completes a swap that a live `restore` is performing, which
  can leave the old store in place of the restored one or remove the intent under the restorer.
- **Fix.** [OS/fs §4.9.4] drops the parenthesis and cites P-86: only `doctor` (and `restore` itself on restart) runs
  `swap_recover`; discovery probes and retries as [F02 §3] states.

#### P1-14 (major) — No OS seam supplies the cryptographic random source

- **Where.** [F08 §2.2] and OP41 (random uids); [F02 §4] (store id), [F02 §5.3] (`tmp/` nonces); [F04 §5.3] (`epoch`);
  [CFG §7.5] (user-file nonce); [API §6.4], [API §17.3], [API §17.4]; [OS/README §4] (the `Vfs`, `ProjectFs`, `Clock`,
  `ProcHost` and `Meter` surfaces).
- **Problem.** Four format values and the temporary names need a CSPRNG, but no seam method provides one, while GT20 keeps
  OS access out of product crates. Each crate will call its own source, and the simulator cannot inject the stream's seed
  that [API §17] derives store ids and uids from, so engine and model differ on the first `Init`.
- **Fix.** Add `fn fill_random(&self, buf: &mut [u8]) -> Result<(), VfsError>` to one seam (proposal: `ProcHost`), backed
  by `ProcessPrng`/`BCryptGenRandom` on Windows, `getrandom(2)` on Linux and `getentropy` on macOS; the simulators derive it
  from the seed by [API §17]; list it in [OS/mapping-appendix].

#### P1-15 (major) — Project path segments reach NTFS alternate data streams

- **Where.** [OS/path §2.1] (`RelPath` allows `:`), §6 (`\\?\` bypasses Win32 normalisation), §8.1
  (`representable_here`), §8.2 (P5); [OS/project §2.3] (`InvalidName`); [OS/fs §2.1] (the check exists for store names
  only); [F20 §3.5], [F20 §4] (the cascade stats a path without a representability check); [F18 §4.6] detail 44.
- **Problem.** A git-tracked path such as `x::$DATA` (legal on Linux) is opened on Windows as the default stream of `x`,
  so `stat` and `read_for_hash` hash another file's content and a link can read `ok`; `a:b` addresses stream `b` of file
  `a`; a segment ending in `.` or space is created literally and becomes undeletable from Explorer; and
  `file mv --allow-nonportable … 'a:b'` can create a stream. Git itself refuses these paths on Windows (`core.protectNTFS`).
- **Fix.** `ProjectFs` on Windows applies [OS/fs §2.1]'s segment check to every `At` path and returns `InvalidName` before
  any OS call; [F20]'s cascade tests `representable_here` for every segment first and yields detail 44 with no OS call;
  `--allow-nonportable` relaxes only `portable_issues` for other OSes, never `representable_here`.

#### P1-16 (major) — A project volume without directory flush fails after the rename

- **Where.** [OS/project §6.2] (`sync_dir`: "any error is a `DurabilityFailure`"), §6.3, §6.5 (the recovery re-barrier);
  [OS/fs §6.2] (`ERROR_INVALID_FUNCTION` and `ERROR_NOT_SUPPORTED` map to `Unsupported`); [OS/env §3] (the guard covers the
  store volume only); [API §12.4] steps 1–3; [F16] P-17–P-19.
- **Problem.** On an SMB share, some FUSE mounts or `\\wsl$`, `FlushFileBuffers` on a directory handle fails, and opening a
  directory with `GENERIC_WRITE` can be denied where the rename itself was allowed. `file mv` then renames, fails the
  barrier and fail-stops; intent recovery must re-barrier before its roll-forward commit (A1P-02) and fails the same way on
  every attempt, so the intent never resolves and every later writer open repeats the failure.
- **Fix.** The plan step (API §12.4 step 1) calls `sync_dir` on both parents before the `FsIntent` group; `Unsupported`
  or `AccessDenied` refuses with exit 7 and nothing changed. Record the capability as a `VolumeCaps` flag bit. Recovery
  that meets `Unsupported` leaves the intent open with a `doctor` text instead of failing the open. Add both cases to the
  `ProjectFs` simulator (FL-2).

#### P1-17 (major) — `aliases.moi` is rewritten whole, and the side-ref layout freezes now

- **Where.** [F14 §14.1], OP-32 (≈ 4.2 MB at 1e5 nodes, "M5 measures it"), OP-33 (the side-ref layout is frozen at M0);
  [AR §5b.9] (incremental checkpoint export ≤ 50 ms); HOLE(F17-loose-pack)'s constraint.
- **Problem.** Every export run writes and hashes a 4.2 MB blob at 1e5 nodes (42 MB at 1e6) into the destination, which
  alone exceeds the 50 ms export gate on Windows with Defender, and M5 would have to change a layout frozen at M0.
- **Fix.** Decide now: fan out to `aliases/<h1>.moi` by the first byte of the uid (256 blobs; an export rewrites only the
  prefixes it touched), as [F14] OP-32 names; record it as a change of [AR §5b.1] for the owner. Add the aliases write to
  measurement 8.

#### P1-18 (major) — The loose-object export path has no durability sequence

- **Where.** [F16] P-23 (packs and refs only) and its seeded bug; [F17 §9.1] (P24: runs with ≤ 8 objects write loose
  objects); [F14 §13].
- **Problem.** Most incremental exports are small and take the loose path, whose sequence (temporary write, flush, rename
  into `objects/xx/`, the directory barrier on a possibly new `xx/` and its parent) no chapter states. A crash can leave a
  ref pointing at an object whose name was never made durable.
- **Fix.** Add the loose path to P-23: each object `durable+meta`, `rename_noreplace` into `objects/<xx>/` (an existing
  name is success: the content is the same), `durable-name` on `objects/<xx>/` and, when created, on `objects/`; all
  before the ref's `.lock` step; seed "ref updated before the loose object's name is durable".

#### P1-19 (major) — Merge and sync item-10 production has no memory bound

- **Where.** [F07 §10.5], §10.6 (the external-sort rule names "bulk producers"); [F17 §4.4] W1 (the bulk class lists
  "long merges" but not `sync`); [AR §8.3] RAM row "sync of a lane 14 days behind ≤ 8 MB".
- **Problem.** A merge or sync over a long window produces its net changeset sorted by uid for item 10. Nothing says when
  that production spills, so an implementation may hold all entries in RAM and break the 8 MB gate; another spills and
  produces the same bytes more slowly.
- **Fix.** [F07 §10.6] applies to every producer whose entries exceed `wmem`, merges and syncs included (spill runs in
  `tmp/sort.<nonce>`, [F02 §5.3]); [F17 §4.4] W1 lists `sync` beside merges; GT11 covers the 14-days-behind case.

#### P1-20 (major) — Scope scanners are unspecified, yet `scope` enters the anchor uid

- **Where.** [F20] OP30; [F08 §11.4] (`captured` hashes `lp(scope)`); [40 §2.7.1]; R-14 (the resolver-version constants
  freeze at M0).
- **Problem.** The Rust, Markdown and TOML scanners decide `scope`, which enters `captured` and hence every anchor uid and
  the commit ids that hash them. Without a grammar, the engine (WP-63) and the reference model derive different uids for
  the same capture, and a later scanner change silently changes derivations.
- **Fix.** Add a scanner-grammar appendix to [F20] as part of resolver version 1 before the freeze, with a fixture per
  language; until then, `scope` is captured as empty and the anchor is a `quote` anchor.

#### P1-21 (major) — Merge rows that raise `SpecGap` and violation classes with no code

- **Where.** [RULES/merge-table] MR-005, VA-005, VA-007, VA-008, VA-013 and OP-10, OP-18; [F13 §5] V05, V07, V12 and
  OP-13-06; [F12] open point 3 (a proposal for MR-005); [RULES/link-merge-rules] LM-013.
- **Problem.** A merge where both sides changed a key and one holds a conflict value, a merge that deepens the forest
  beyond 12, builds a two-step `duplicate_of` chain, breaks `runs_in` or `answers` cardinality, or writes read-only
  fields on `plan/*`, reaches a row the model evaluates as `SpecGap`. The engine must still stage or refuse, and the
  violation class it records has no code in [F19 §12], so the staged commit's bytes are undefined. These are ordinary
  outcomes of parallel lanes, not corner cases.
- **Fix.** Adopt [F12] open point 3's form for MR-005; give V05 (depth), V07 and V12 structural classes with codes in
  [F19 §12] and rows in [RULES/merge-table]; decide LM-013 (a root created on one side and dead on the other).

#### P1-22 (major) — Gitmap pages and blobs files are probed one by one

- **Where.** [F10 §7.3] (a lookup probes every gitmap page), [F17] (no compaction trigger); [F10 §5.5] (a fingerprint
  lookup binary-searches every blobs file `FILES` lists); [F09] OP-09-14; [70 S10] (open-count gates ≤ 10/14/18).
- **Problem.** Each folding checkpoint adds one gitmap page per destination and algorithm, and each seal adds a blobs file;
  both lookups visit every file, each a lazy open and map (≈ 0.2 ms on Windows) counted against the open gates. The cost
  grows with store age and has no bound.
- **Fix.** Compact gitmap pages at rollup (and when their count passes a stated number, proposal 8); adopt OP-09-14 (the
  blobs file number in the `FPRINT` row) so a fingerprint lookup opens one file.

### Minors

#### P1-23 (minor) — [F01 §5.2]: the tenth byte of a `uvar64`

"00 or 01" is inconsistent with the canonical rule that forbids a zero final byte; write "must be 01".

#### P1-24 (minor) — [F03 §6] WD-1: `WriterDiag` in every writer holding

A 512 B `write_at` into `LOCK` inside each holding (three per durable commit) adds a syscall to the hold and dirties a page
the lazy writer flushes. State the cost, include it in measurement 2, and consider writing it only for holdings that
exceed a threshold.

#### P1-25 (minor) — Two structures named `FileRef`

[F05 §8.4] (variable, `uvar` file number) and [F11 §2.5] (fixed 9 B, used by `FILES` and `PINS`) share a name. Rename one
(proposal: [F05]'s `FileRefV`).

#### P1-26 (minor) — [F06] OP16: re-serialisation inside the writer byte

Rewriting `prev` deltas and `#N` placeholders, the record's XXH3 and the chain under the writer byte is O(`cs_bytes`), up to
1 MiB (≈ 1 ms) against a hold budget of tens of microseconds. [F16] P-2 and measurement 2 should sweep inline sizes up to
P05, or `prev` should be encoded relative to a base known in phase 1.

#### P1-27 (minor) — [F10 §8] and OP-10-11 name a `cs` `OPS` section

[F09 §16.4] defines `PREV` and `VIOLATIONS` only and registers no `OPS` tag. Align the two chapters.

#### P1-28 (minor) — [F11 §8] `IDEM` sizing

72 B per slot at load ≤ ½ (≈ 4.3 MB at the design's count, [F11] OP-14) against [AR §4.4]'s and [AR §8.1]'s 40 B per key.
Update the design's size rows or narrow the slot.

#### P1-29 (minor) — [F12 §5], [F12 §7.5]: cost of the recursive virtual base and of the text merge

The recursive virtual base needs `state(L_i)` of historical commits (replay from a pinned set) with no work or memory
rule; the one-line peel recursion of §7.5 is O(64 n²) in the worst case for a 64 KiB text. State that virtual-base work is
charged to `wmem` and the bulk rules, and give the text merge's complexity with a WP-60 measurement.

#### P1-30 (minor) — The frame rule counts two different things

[F17 §4.3] counts commits for P03; [F10 §4.2] counts records for P04's bytes (OP-10-03). State one rule in both.

#### P1-31 (minor) — [F19] codes

`store_locked`'s text names only the writer lock, while [F16] P-72 step 1 uses it for a flush-byte timeout (F16 OP-3);
explicit verbs that find the maintenance byte busy (P-76, P-85 step 2) have no code (proposal: `maintenance_busy`, exit 7);
`DATA` appears as a value-conflict class in [F07 §2.2], [F14 §6.8] and [F19 §12.1] but has no code in [F12 §6.1] (OP5):
remove it everywhere.

#### P1-32 (minor) — `tmp/` names outside [F02]'s grammar

`tmp/probe`, `tmp/probe.1`, `tmp/probe.2` ([OS/env §5]) and `tmp/settle.stamp` ([OS/project §5.9], OP-18) are not in
[F02 §5.3]'s table or §6.3's `tmp-entry` grammar. Add them, with the rule that the orphan sweep may remove them.

#### P1-33 (minor) — Hole ids outside the `<part>-<name>` form

[OS/lock §4] and §7.3 write `lock-writer-wait-ms`, `lock-flush-wait-ms` and `lock-release-delay` for `F17-lock-writer`,
`F17-lock-flush` and `F15-lock-release`, and say [CFG] owns them, which [CFG] open point 19 denies; [F03] writes
`os-win-boot-source`; `share-retry-ms` has no prefix. `HOLES.md` §4 indexes them. Rename before WP-81a (proposal:
`OS-share-retry-ms`) so one scan finds every hole.

#### P1-34 (minor) — The deletion grace on two clocks

[OS/clock §4.5] and §6 list the GC grace under the cross-process stamp form; [F16] P-89 and [F17 §11.4] measure it on the
HLC from the releasing `Checkpoint` (F16 OP-8). Align [OS/clock] with [F16].

#### P1-35 (minor) — [OS/README §4.2]: stale open text

"Open for part 2: the cross-volume `file mv` … either becomes a refusal or a `ProjectFs` copy" is resolved by
[OS/project §6.4] (refusal); say so, and close [OS/README] open point 5.

#### P1-36 (minor) — [OS/mem §3], §6.2

On Linux, `/proc/self/smaps_rollup` walks every mapping under the mm lock (tens to hundreds of microseconds with hundreds
of maps), so "cheap enough to call at the start of every query" needs a port measurement or a per-request cache. §6.2
counts two atomic operations per allocation; `ACTIVE.store(true)` makes three (store it once, after a relaxed load).

#### P1-37 (minor) — [OS/path §4.1], §5, §7: Windows path edge cases

`GetFinalPathNameByHandleW(…, VOLUME_NAME_DOS)` fails for a volume mounted only in a folder and on some virtual providers
(`ERROR_PATH_NOT_FOUND`, `ERROR_INVALID_FUNCTION`); state the mapping (exit 7 "root cannot be canonicalised", `doctor`
text). The CLI boundary does not classify drive-relative `X:rel` or the device forms `\\.\…` and `\\?\…` (`//./`, `//?/`
after conversion); refuse them with exit 2.

#### P1-38 (minor) — [OS/project §5.5]: placeholder gate by path, then open

Attributes are read by path and the file is opened afterwards; a file replaced by a placeholder in between is hydrated
by the first `ReadFile`. Re-check the cloud bits through the handle (`FileAttributeTagInfo`) before the first read, in
§5.5 and in [OS/mapping-appendix §2.1].

#### P1-39 (minor) — Unbounded stdin and `-f` reads

[OS/shell §5.2] step 2 reads all bytes, and [LQ/lexical §8] sets no length limit because "the budgets bound the work";
the input is buffered before any budget applies. Add a key (proposal: `input.max-bytes`, default 16 MiB, [CFG §10.5]),
read incrementally, exit 2 beyond it.

#### P1-40 (minor) — [OS/map §7]: no mapping bound or release rule

The 512-entry registry is sized from an estimate (< 200 mappings), but no chapter bounds how many sealed files a
long-lived MCP server keeps mapped (`hist`, `blobs`, `gitmap`, `cs` grow with store age), and `RegistryFull` fails the
command. State the bound in [F17] or [F16] (OS/map OP-3), release sealed maps not used by the current request at request
end (as `mcp.overlay-bytes` does for overlays), and fall back to positional reads on `RegistryFull`.

#### P1-41 (minor) — [LQ/json-ir §2]: no JSON nesting limit

The S-AST depth limit (P13, 64) applies after parsing; a deeply nested JSON document can exhaust a recursive decoder's
stack first. State a JSON nesting limit (proposal: 256, E001) checked while parsing, for the IR and for MCP arguments.

#### P1-42 (minor) — [LQ/card §7.3]: the shrink ladder may not reach the gate

Steps 1–4 save 291 B of 3,116 B (≈ 9 %); at the upper estimate of 1,160 tokens the card stays near 1,050 tokens, above the
1,000-token gate, and the remedy re-runs the baseline. Pre-specify further steps (for example the PowerShell line to
`reference-ql.md`) so `HOLE(LQ-card-shrink)` always has a feasible candidate.

#### P1-43 (minor) — [F16] P-87: `backup` holds the maintenance byte for the whole copy

A copy of a large store defers every checkpoint for its duration; folded into P1-9's fix (copy a pinned set, take the byte
for the record only).

#### P1-44 (minor) — Retention windows across an epoch re-roll

[API §6.2] CK-6 and [F16 §14] measure windows from `max(wall_ms, h >> 16)`, with h taken from the scanned log. After
`restore` or `repair` re-rolls the epoch, h restarts low unless the floor of P1-8 is carried; windows then open early on a
machine whose clock is behind. Covered by P1-8's anchor; state it in CK-6 and [F16 §14].

## 5. Checked and found correct

- [F01 §5.7] the `hlc` example `0x01A0C4506C000003`; [F02 §3.3] the 78-byte pointer example; [F03] the regions and the
  36,864-byte file; [F04 §3] every `HeadSlot` offset to 1072, the reserved area to 4080 and the `xxh3_128` trailer.
- [F05] embedded widths: `OsFileId` 57 B, `VolumeCaps` 16 B, `JOURNALCUR` 41 B, `Stamp` 24 B.
- [F06] the 205-byte example, presence bits `0x7043`, the header bound `659 + 10A + M + 5F`.
- [F07] the 43, 74 and 196-byte examples.
- [F08] `NodeHdr` 60 B, the 12-byte field block, the 46-byte `uid_file` input.
- [F09] `SegHdr` 120 B, section entries 32 B; [F11] `REFS` 164 B, the 88-byte `MARKERS` example.
- [OS/project §3], §4.2 layouts agree with [F05] and [F11 §12]; [OS/mapping-appendix] agrees with its chapters except
  P1-38.
- [CFG] the `config_gen` bump as an unflushed publish under the writer byte agrees with [F16] P-20 and [F04 §5.6].
- `COVERAGE.md`: 152 rows, none UNMAPPED; `HOLES.md`: 53 holes to fill, every alias listed.

## Holes

None. This review adds no hole; the proposals above (a spare-extent fill fraction, the number of quiet bytes, a gitmap page
count, an input cap, a JSON nesting limit) are design choices for the owning chapters, not measured values.

## Open points for the review

1. **P1-7 and measurement 2.** If the owner keeps preparation under the flush byte, measurement 2 must include a rotation
   and the M1 gates must state the rotation's allowed tail; either way the rotation cost should be measured on the owner's
   NTFS volume with Defender on.
2. **P1-9's two variants** (work outside the byte, or yield and checkpoint) differ in complexity; the first also removes
   P1-43. WP-16 chooses.
3. **P1-16 on Linux and macOS.** `fsync` on a directory of an NFS or FUSE mount can also return `EINVAL`; the same
   pre-flight covers the port.
4. **P1-20's interim rule** (empty `scope` until the scanner appendix exists) changes no byte layout; it only fixes the
   input so that engine and model agree.
