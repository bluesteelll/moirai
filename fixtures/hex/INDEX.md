# fixtures/hex: byte fixtures of format v1

| | |
|---|---|
| Title | The hand-written byte fixtures of format version 1: every record kind, op, value type, segment section and sealed-file header; `LOCK` and `HEAD` with a torn slot, the nine two-slot states and the fatal slots; Unknown-boot; the log chain cases (first group, chained group, `Noop` pad and `ExtentHead`, a wrong-position record of the same epoch, a lost lazy tail, corruption below `durable_lsn`, an epoch re-roll); codec `none` payloads in full and candidate-codec payloads that the M0 oracle treats as opaque |
| Work package | WP-20 (R-FIX; [PLAN §3.2] item 1) |
| Acceptance | E3: WP-95's format oracle decodes every `.bin` here and re-encodes it byte-identically, compressed payloads opaque ([PLAN §6.2] R3); WP-20b replaces the files that depend on a filled hole (§4) |
| Separation | S1 ([PLAN §3.1]): written from the specification text only. The author read no line of `moirai-format-oracle`, `moirai-model`, `moirai-toylog` or any product crate. The only project code run was `cargo xtask hex`, the generic assembler (R-HARN's; it knows no structure). Throw-away scripts that implement only what the chapters say wrote the `.hex` text and a byte model of each file; their BLAKE3 was checked against the canonical fixtures' derivations and their XXH3 against the assembler's directives on random inputs, and every assembled `.bin` was compared with the model byte for byte, hash outputs included. Candidate-codec payloads were decoded back with `lz4_flex` 0.14.0 and `ruzstd` 0.9.0, the pure-Rust decoders of [90 §11.2] (§3.10) |
| Sources | [F01]–[F11], [F17], [F18 §3], [F19 §12], [F20 §2.6–§2.8], [OS/clock §3], [OS/proc §4–§5] (normative for every byte here); [60 §2.5] and `docs/spec/COVERAGE.md` for §5; `docs/spec/HOLES.md` for §4 |
| Status | Written against the specification after review pass 1 (owner answers of 2026-09-28) and spec sync 2a (`docs/spec/reviews/spec-sync-2a.md`); updated for spec sync 2b (`docs/spec/reviews/spec-sync-2b.md`): store A's promoted-field sections and `IDEM` results (S2B-R-1, S2B-R-6), g44 (S2B-P-24), §2's commit ids (S2B-F-14), the frames appended to `fragments/ops/variants.hex` (a `Resolve` with `choice` 5 `drop`, `Schema` ops of the item classes 1–4 and 6; S2B-F-10, S2B-F-13, S2B-F-21), the class-6 item appended to `fragments/schema/items.hex` (S2B-F-21), and §3.9, §5.1 and §6 with them (bit 18 `stage` stays uncovered, S2B-F-1). A specification change that moves a byte updates the files it touches (commit subject `WP-20:`, `WP-20b:` or `WP-73:`) |

The consumers are the format oracle (WP-95, WP-95b) and, from M1, the product codec, whose author is neither R-FIX nor
R-ORA (S1). Every value is synthetic: no owner data, no real paths, users, hosts, machine ids or boot ids.

## 1. How to read these files

- **Format.** A `.hex` file is the input of `cargo xtask hex` (see `cargo xtask hex --help`): hex bytes, strings,
  repetitions, labels, and the directives `{len}`, `{u8}`…`{u64}`, `{i64}`, `{uvar}`, `{svar}`, `{align}`, `{pad_to}`,
  `{xxh3_64 … seed=…}`, `{xxh3_128}`, `{blake3_256}` and `{blake3_128}`. Every checksum, digest, length and offset that the
  format computes over bytes of the same file is a directive over those bytes, so the assembler recomputes it. Values
  that come from another file (a sealed file's `total_len` and digest in the records and `HEAD`s that name it, lsns,
  commit ids) are literals taken from that file's assembled bytes; so is the hash of a compressed payload whose raw
  content is longer than 4,096 bytes (§3.10).
- **The `.bin` is the interface.** Each `.hex` has its assembled `.bin` beside it; decoders read the `.bin`.
  `cargo xtask hex --check` re-assembles every `.hex` and compares it with its `.bin` (the gate's `hex` step).
- **Comments.** The first comment block of each file says what the file holds, which sections of the specification it
  exercises, how to decode it (a whole file, or a sequence of one structure to the end of the file) and what a reader
  must conclude where that is the point (a selected slot, the end of the valid log, an exit 7). Every field carries a
  comment with its name, its value and, where it helps, the rule it follows (`[F05 §9.9]`).
- **Paths** in this file are relative to `fixtures/` (the convention of `docs/spec/COVERAGE.md`'s fixture column).
  Chapters are cited as the specification cites them: `[F05 §4]` is `docs/spec/format/05-log.md` §4, `[OS/proc §5]` is
  `docs/spec/os/proc.md` §5.
- **Stores.** Most files belong to one of a few synthetic stores (§3). Files of one store are mutually consistent: the
  `SegRef`, `FileEntry`, `cs_ref`, `BlobRef`, `Retirement` and `Promotion` values in its log and `HEAD` carry the
  `total_len` and digest of the sealed file they name, and every lsn a file holds is the lsn of the record it means.
  Store histories are made to exercise the formats; they are not claimed to follow the protocol's timing (when a
  checkpoint or a promotion runs is [F17 §5]–[F17 §7]'s decision), except where a file says so.

| Directory | Files | Holds |
|---|---|---|
| `hex/store-a/` | 11 | store A: log extent 1 and ten sealed files (§3.1) |
| `hex/store-b/` | 7 | store B: two extents, rotation, retirement, a gc rewrite, its `HEAD` (§3.2) |
| `hex/head/` | 19 | store A's `HEAD`: `init`, the nine two-slot states, the other durable publishes, fatal slots (§3.3) |
| `hex/lock/` | 2 | store A's `LOCK` as `init` creates it and in use (§3.4) |
| `hex/chain/` | 8 | stores C, D, E and the re-rolled store A: log validity and the end of the log (§3.5, §3.6) |
| `hex/unknown-boot/` | 3 | store U, used in Unknown-boot mode (§3.7) |
| `hex/decode-only/` | 2 | store J: the reserved record kind `JournalCursor` (§3.8) |
| `hex/fragments/` | 10 | structures outside a file of their own: values, anchor records, schema items, op variants, OS ids, `BindingExt` (§3.9) |
| `hex/codec/` | 7 | `dict` files and `blobs` and `hist` files under the candidate codecs (§3.10) |

## 2. Synthetic values

| Name | Value | Where |
|---|---|---|
| Wall time | `MS0` = 1,790,000,000,000 ms (2026-09-21T14:13:20Z, the `unix_ms` of [F01 §5.7]'s example); `hlc` values are `(ms << 16) \| counter` with ms = `MS0` + a few seconds per event | every `hlc`, `append_hlc`, `Stamp.wall` |
| Commit ids | `BLAKE3-256("moirai-fixture-commit:" ‖ label)`, label `A/c1` … `A/c18`, `B/b1` … `B/b7`, `C/c1`, `D/c1`, `E/c1` … `E/c3`, `U/c1`; not the canonical form of any commit (the canonical commit-id fixtures are `canonical/`). Commit-id and changeset-digest correctness are C-rules ([F06 §4.3] orders 2 and 38, [F07 §12] intro; spec sync 2b S2B-F-14): a decoder never recomputes them, so records with these synthetic ids are valid | commit records, parents, `ref_old`, `REFS.tip`, markers, `HCIDX`, `gitmap` |
| Changeset digests | `BLAKE3-256("moirai-fixture-commit:" ‖ label ‖ "/changeset")`: synthetic, like the ids | `changeset_digest` |
| Other 16-byte ids | `BLAKE3-128("moirai-fixture:" ‖ label)`: `values/U` (the uid U of [F08 §5.6]), `anchors/src`, `vol-C`, idempotency keys, statement hashes | fragments, runtime rows |
| Other 32-byte ids | `values/C`, `values/C2`, `values/D256` (C, C2 and D256 of [F08 §5.6]) as commit ids above | fragments |
| Fixed digests | D = `d94db1affb1bddd9491cd7666232dfce915c3110` (SHA-1), `SHA256_D` = `5e4b1c31…8f18a27`, `docs/storage/lock.md`'s oid `a936e37c…89fd77ae`, git heads `83bcc6a1…69b5f074` and `9e8d7c6b…79881203`, lane base `7c1e0a4d5b3a92c1e6f40d8a7b2c9e0133445566`: the values `canonical/` uses | values, anchors, `BindingExt`, runtime rows |
| Store ids | `5701de00…0001` (A), `…0002` (B), `…0003`–`…0007` (C, D, E, U, J) | `InitParams.store_id` |
| Epochs | A `0x3141592653589793`, B `0x2718281828459045`, C `0x1111222233334444`, D `0x5555666677778888`, E `0x9999aaaabbbbcccc`, U `0x0f1e2d3c4b5a6978`, J `0x2468ace013579bdf`, re-rolled A `0x6a09e667f3bcc908` | `HEAD.epoch`, `RecHdr.epoch`, chain seeds |
| Node uids | store A: small fixed patterns (`1a00…0001`), except #4 = the root-node derivation of `project` and #5 = the file derivation of `project:docs/storage/lock.md` ([F08 §11.2]–[F08 §11.3]); store B: `b0…0n` | `Create`, `UID`, `ALLOC`, `UIDX` |
| Boot identities | [OS/proc §4.2]'s derivations over fixed inputs: Windows counter 7 (and 8 after a reboot) with MachineGuid `00000000-0000-4000-8000-00000000000a`; Linux boot uuid `…00b1`; macOS `…00c1`; `boot_hash` = low 64 bits with bit 0 set ([OS/proc §4.3]) | `HEAD.boot_id`, `ProcId`, `Anchor`, `Stamp` |
| Session identities | `claude:5e551070-0000-4000-8000-000000000001` (and `…002` after `/clear`), `codex:0199aa00-0000-7000-8000-000000000009`; session hash = BLAKE3-128 of the identity ([F03 §9]) | `SlotRec`, `Anchor`, `LEASES`, `HEADS`, cursors |
| Parameters | the test profile of [F17 §12]: P01 `log_extent_bytes` 65,536, P03 4, P04 4,096 in `InitParams`; tunable values as the test profile gives them (P18 = 16 rows) | every `HEAD`, extent sizes, `hist` frames |

## 3. The fixture sets

### 3.1 Store A (`hex/store-a/`, `hex/head/`, `hex/lock/`)

A Windows store with a SHA-1 project root (`project_oid_algo` 1), E = 65,536. `log.1.hex` is its first extent: 52
groups from the epoch-start group at lsn 0 to the end of the valid log at label `end`, then zeros to the end of the
extent ([F05 §2.4]). It holds every record kind but 12 `Noop` (store B) and 21 `JournalCursor` (store J), every commit
kind and import provenance, every presence bit but `pruned` (store B) and `stage` (§6), every `stmt_origin` and `actor_src` value, and
every op tag.

| Group | lsn | Class | Content |
|---|---|---|---|
| g1 | 0 | durable | the epoch-start group: `ExtentHead` of log.1 |
| g2 | 138 | durable | init: `RefUpdate` create main (ref id 0), `RefTable`, `ClientHead` binding main to the main worktree |
| g3 | 601 | durable | commit c1 (seq 1): the root commit on main: `Schema` (a named query), `Create`, `Move`, `AddEdge` (with anchors) |
| g4 | 3414 | durable | claim #1 as developer (lease L-1, token 1), then its `Idem` result |
| g5 | 3791 | durable | a session head (kind 3) on main and a client head (kind 2) detached at c1 |
| g6 | 4244 | durable | c2: `SetStatus`, `SetField` of every value type, `Incr`, `SetBody`, `Move`, `SetEdgeProps` (a repin) |
| g7 | 5619 | durable | `Lease` event 3: set `files_owned`, `bound` and `anchor` of L-1 |
| g8 | 5745 | durable | c3 completes #1; its `settled` marker; L-1 released into settled |
| g9 | 6090 | durable | create lane/l5np forked at c3; the fork extends #1's marker holders; bind its worktree (R-15, `BindingExt`) |
| g10 | 6630 | durable | c4 on lane/l5np deletes #6, creates #11 and #12; the `deleted` marker |
| g11 | 7248 | durable | c5 on lane/l5np deletes #12 |
| g12 | 7551 | durable | merge c6 of lane/l5np into main (`CreateDeleted` #12); the markers now held by main |
| g13 | 8077 | durable | c7 on main |
| g14 | 8312 | durable | c8: sync of main into lane/l5np; its residue is empty (`n_ops` 0) |
| g15 | 8544 | durable | revert c9 undeletes #6 (origin c6); its marker `cleared`; #1's marker `nonlinear` |
| g16 | 8962 | durable | c10: cherry-pick onto lane/l5np (origin c2) |
| g17 | 9284 | durable | create the staging ref `merge/main/from/lane/l5np` (ref id 2) |
| g18 | 9555 | durable | c11 on the staging ref: `Conflict` (TextHunk, DeleteVsModify) and `Violation` (DanglingEdge) ops |
| g19 | 10098 | durable | c12 on the staging ref: `Resolve` ops (choices `value` and `ours`) |
| g20 | 10581 | durable | `merge --abort` deletes the staging ref (`RefUpdate` reason 2; the `RefTable` entry sets `deleted`) |
| g21 | 10818 | durable | `GitMap` (dest 1 `origin`, SHA-1, with last-seen oids) and `GitMap` (dest 2 `mirror-256`, SHA-256) |
| g22 | 11264 | durable | the first checkpoint (delta seg.d1; blobs.2, gitmap.3, gitmap.4 added; per-ref lists); `Pin` of the merge c6 (holder 3) |
| g23 | 11558 | durable | create lane/b2 (ref id 3) forked at c9; `Pin` of its fork base (holder 1) |
| g24 | 11859 | durable | a standalone promotion of lane/l5np (seg.b1.5; `ckflags` bit 3 only); `Pin` holder 2 |
| g25 | 11982 | durable | `Backup` |
| g26 | 12085 | durable | `FsIntent` (`file mv`) |
| g27 | 12288 | durable | c13 records the move; `FsIntentDone` in the same group |
| g28 | 12723 | durable | `FsIntent` (`rm --trash --recursive` of a directory) |
| g29 | 12869 | durable | `FsIntentAborted` (reason 1, not renamed, written by intent recovery) |
| g30 | 12921 | durable | create the staging ref `import/1` (ref id 4, kind import) |
| g31 | 13165 | durable | `Reserve` (#13–#15, cs.6, no `blobs` file, project kind id 64 `incident`) |
| g32 | 13298 | durable | c14: a bulk import-checkpoint on import/1; its changeset is `cs.6` |
| g33 | 13654 | durable | c15: a foreign import, demoted after a `Moirai-Commit` mismatch (`verified` 0) |
| g34 | 13882 | durable | c16: a native import whose parent was demoted (stated id), `verified` 1, with `xtr`; sets #7's `abstract` |
| g35 | 14256 | durable | c17: an inline import-checkpoint with `ckpt` and `ckimg` (presence bits 10 and 17) |
| g36 | 14779 | durable | `undo 1` on main (`RefUpdate` reason 3) |
| g37 | 15018 | durable | `op restore` of main to seq 17 (reason 4) |
| g38 | 15257 | durable | `park` (reason 5) creates `orphans/main` (ref id 5) |
| g39 | 15505 | durable | `tag tags/v1 --pin` (ref id 6; `RefTable` eflags pinned and has_message; `Pin` holder 4); `Idem` of a ref move |
| g40 | 15979 | durable | gc: `RefTable` op 2 removes the expired deleted staging ref |
| g41 | 16022 | durable | `ClientHead` op 2 removes the lane binding |
| g42 | 16088 | durable | role lease L-2 (lkind 2, `session_role`, anchor `session-ttl`) |
| g43 | 16277 | durable | `Lease` event 4: renew L-2 |
| g44 | 16394 | durable | `Lease` event 2: L-2 released (reason 1 `release`): a role lease never ends with reason 4, which only the next claim of a task writes ([F05 §9.4] field 18; spec sync 2b S2B-P-24) |
| g45 | 16453 | durable | claim #11 on lane/l5np (L-3, anchor `session-ttl`); it stays live |
| g46 | 16660 | durable | the delta checkpoint seg.d7 over [ck1, ck2), per-ref lists of main and import/1; `next_id − 1` = 16 reaches `store.fts.tier2-nodes` (16), so the record carries `fts_tier2` and seg.d7 is the first tier-2 segment ([F17 §6.4]) |
| g47 | 16823 | lazy | a settle's `TreeReg` records (register, full-tree and lane-owned epochs, dirty row, forget) |
| g48 | 17305 | lazy | `FileObs`, `Pending`, `FPrint`, `DirMap`, `PrefixEv`, `GitFacts` and `AnchorRes` row batches |
| g49 | 19475 | durable | a runtime-only fold (delta seg.d8, fingerprint file blobs.9; bits `set_change`, `runtime_fold`, `files_added`, `quiet_bounded`: quiet mode, which only `HEAD` records ([F04 §6]), was on here and is off at the end of the extent) |
| g50 | 19642 | durable | a rollup (seg.base.10 replaces seg.d1, seg.d7 and seg.d8; bits `files_released`, `rollup`); the merge pin is unpinned (`Pin` op 2) |
| g51 | 19780 | lazy | a `Lazy` heartbeat, a cursor (feed 1), a pack cursor (feed 2), `SessionMark` |
| g52 | 20090 | durable | a cursor record made durable by `durability.lazy-kinds` without `cursor` (`RecHdr.flags` bit 0 = 0) |

Sealed files:

| File | Kind | Holds |
|---|---|---|
| `seg.d1.hex` | `seg-delta` | the first delta, written at g22: rows #1–#12 (no base below: every row touched, every set a plus list), `SCHEMA` (the named query), `FPROMO`/`FCOL`/`FIDX`, `BM` ± lists, `SYMTAB`, and every store-level section as of its bound. Every `FPROMO` row implies its `FCOL`, so `FCOL.assignee` (0x4002) is present with every absent bit set; there is no `FIDX.assignee`, since an upper segment carries an `FIDX` only for a changed value set ([F09 §10.1]). The two used `IDEM` entries without `result_inline` hold `result` (0, 0), outside the heap order ([F11 §2.3], §8). Both as spec sync 2b states them (S2B-R-1, S2B-R-6) |
| `seg.b1.5.hex` | `seg-branch` | lane/l5np promoted over the pinned set {seg.d1}: rows #1, #6, #11, #12, ± lists against the pin, `TOUCH` = `IDS`, no store-level section. `FCOL` slots 1–9, every one its `FPROMO` rows imply (slot 0 `labels` is a set with a bitmap index); slots 2–8 (`assignee`, `metric`, `local_id`, `severity`, `f_kind`, `round`, `outcome`) have every absent bit set, since no touched row holds those fields at c10 (#6 is deleted there); `FIDX.metric` is the one changed value set (#6 leaves it) ([F09 §10.1]–§10.3; spec sync 2b S2B-R-6) |
| `cs.6.hex` | `cs` | c14's changeset: rows #13 and #14 as created (append-time values fixed), a conflict value, the project kind in `SCHEMA`, `PREV`, a `Violation` op in `VIOLATIONS`, `CKIMG` |
| `seg.d7.hex` | `seg-delta` | [ck1, ck2): rows #5, #7, #16; the five derived-optional caches absent; full-text tier 2 for its touched rows (§4) |
| `seg.d8.hex` | `seg-delta` | the runtime-only fold: empty graph window, runtime-window sections up to `rt_upto_lsn`, all five derived-optional caches present; tier-2 sections present and empty, `FTSSTAT` repeating the view's statistics; an `IDEM` table with no entry, which still has capacity 16: 16 empty slots, `n_rows` = `SecEnt.count` = 16, `aux` 0 ([F11 §2.1], §8) |
| `seg.base.10.hex` | `seg-base` | the rollup: main at ck2, rows #1–#16 (#13–#15 absent), frozen bitsets, `STATS`, `SCHEMA`, `SYMTAB`, every store-level table, full-text tier 2 over the whole view: 48 terms in three front-coded blocks, postings in title, abstract and body. Exactly the `FCOL` and `FIDX` set its `FPROMO` rows imply: `FCOL.assignee` (0x4002) with every absent bit set and `FIDX.assignee` (0x5002) as its 16-byte header with `n_values` 0, since no row holds `assignee` ([F09 §10.1]–§10.3). The two used `IDEM` entries without `result_inline` hold `result` (0, 0) ([F11 §2.3], §8). Both as spec sync 2b states them (S2B-R-1, S2B-R-6) |
| `blobs.2.hex` | `blobs` | the three bodies g22 sealed (class 1, codec 0) |
| `blobs.9.hex` | `blobs` | the fingerprint blob (class 2, codec 0) of the runtime-only fold, named by `FPRINT` |
| `gitmap.3.hex`, `gitmap.4.hex` | `gitmap` | the pages of destination 1 (SHA-1, 36-byte entries) and destination 2 (SHA-256, 48-byte entries) |

`hex/lock/` and `hex/head/` are store A's (§3.3, §3.4).

### 3.2 Store B (`hex/store-b/`)

A Linux store with a SHA-256 project root (`project_oid_algo` 2).

- `log.1.hex`: the epoch-start group; init without a `ClientHead` (init ran outside a worktree); commits b1–b6 (b2 on
  lane/x, which is deleted after it, so b2 becomes unreachable); b6 carries a 5,000-byte body, so its record is longer than
  P04 = 4,096; then the **rotation pad**: b7 (60,213 bytes) does not fit the rest of the extent, so one lazy `Noop` group
  pads it to its last byte ([F05 §4.4] G-3). The pad's trailer is the chain value at lsn 65,536.
- `log.2.hex`: extent 2 opens with its **`ExtentHead`** ([F05 §4.5]; `chain_in` = the pad's trailer, seed of its own
  trailer), then b7, a run-scoped lease (`Stamp::NEVER`, anchor `none`, a Linux `ProcId`), a `Reserve` whose bulk commit
  never lands, the checkpoint that retires extent 1 into `hist.2` (`Retirement`, bit 4), and a gc checkpoint that
  replaces `hist.2` by its rewrite `hist.5` (`files_added`, `files_released` only).
- `hist.2.hex`: extent 1's records but the pad, byte for byte, in frames of at most 4 commits and 4,096 raw bytes; b6's
  record is a **split frame** of 4,096-byte blocks; `HCIDX` over the six commits; codec 0.
- `hist.5.hex`: the gc rewrite: b2 in its header-only form ([F06 §4.4.15], presence bit `pruned`), `len` and `xxh3_64`
  recomputed, its group trailer kept.
- `blobs.4.hex`: the bodies of b6 and b7 (60,000 bytes, near the 64 KiB cap), codec 0.
- `seg.d3.hex`: the only layer of the main set: rows #1–#7 (#2 absent: it lived only on the deleted lane), the live
  lease, `ALLOC` with the reserved holes #8–#9, `SCHEMAIDS` with the reserved kind id 64, `FILES` with `cs.1` marked
  `reserved`, and an `IDEM` table with no entry: capacity 16, so 16 empty slots and `aux` 0 ([F11 §8]).
- `HEAD.hex`: after log.2: `active_log` 2, `checkpoint_lsn` in extent 2, the main set {seg.d3}, a Linux `boot_id`; both
  slots equal after a durable publish.

### 3.3 `HEAD` (`hex/head/`)

All of store A, E = 65,536.

- `init.hex`: the file `init` writes ([F04 §10], [F16] P-88): both slots hold the fold of g1–g2, slot_seq 1 and 2.
- `two-slot/<a>-<b>.hex`, the **nine two-slot states** of [F04 §8.2]: a durable publish that turns `quiet` on writes N1
  into slot A (slot_seq 42) and N2 into slot B (43) over O1 (40) and O2 (41); `old`, `new` or `torn` per slot. A **torn
  slot** mixes 512-byte pieces of the write and of the content it overwrote, so its checksum fails. Each file states the
  selection of [F04 §8.1]; `torn-torn` has no valid slot (read again, then exit 7).
- `flags/boot-change.hex`, `flags/readonly.hex`, `flags/retired.hex`: the other uses of the durable publish
  ([F04 §9.3]): boot-change recovery (a new `boot_id`, [F04 §9.4]), `readonly` with `config_gen` 3, and `retired` on the
  old store after `restore`.
- `fatal/*.hex`: a slot with `format` 2, a reserved flag bit, equal `slot_seq` with different bytes, `InitParams` that
  differ between the slots (IP-3), and `checkpoint_lsn` above `durable_lsn`: each exit 7 ([F04 §7], §8.1).
- `absent-zero.hex`: slot B never written (4,096 zeros): slot A is used.

### 3.4 `LOCK` (`hex/lock/`)

- `init.hex`: `LockHdr` at 0 and zeros to 36,864 bytes ([F03 §2.3]): every record absent.
- `live.hex`: `WriterDiag` of the writer byte's holder, `LeaderRec` of a running leader, and `SlotRec` records at the
  slots their holders chose ([F03 §8.7]): a Claude Code server (primary and alias hashes), a Codex server, a `file mv`
  intent holder (kind 2), a Linux server (`ProcId` with a boot-relative start and `pidns`), a macOS server, and one record
  whose checksum is another nonce's (absent, [F03 §8.5]).

The lock bytes beyond the end of the file (2^62 + role, 2^62 + 2^16 + slot) are byte-range locks, not file content; no
byte fixture can hold them ([F03 §3]).

### 3.5 The log chain and the end of the log (`hex/chain/`)

Small stores whose first two groups are the **first group** (the epoch-start `ExtentHead`, trailer seeded with
XXH3-64(epoch)) and a **chained group** (init, seeded with the trailer before it) ([F05 §4.3]). Each has its `HEAD`.

- `wrong-position/` (store C): at the boundary p after c1's group lies a record of the same epoch with a valid checksum
  whose `lsn` says p + 64 ([F05 §5.2] check 5): the group is invalid, p ≥ `durable_lsn`, so p is the end of the log.
- `lazy-tail/` (store D): c1's durable group ends at `durable_lsn`; lazy group L1 is valid; lazy group L2, covered by
  `committed_lsn` but never flushed, lost its last 512-byte sector: the end of the log is L2's start, a lost lazy tail,
  not an error ([60 §2.5] decision (b) as restated).
- `corrupt/` (store E): a flipped bit in c2's message fails its record checksum below `durable_lsn`: corruption, exit 7
  and `moirai repair`; c3's valid group after it repairs nothing. c2's `Commit` record is at lsn 597, and every value
  that names it carries 597: `seq_ring[2]` in both `HEAD` slots ([F04 §5.12], [F05 §10]) and c3's `parents[0].lsn` and
  `prev_on_ref` ([F06 §4.3]). A scan stops at 597, so no fold derives them; c2's `RecHdr.lsn` (597) is still readable
  in its literal bytes.

### 3.6 The epoch re-roll (`hex/chain/re-roll/`)

Store A after `restore` ([F05 §2.6], [F16] P-75, P-85): a new epoch drawn, every old extent retired, and extent 2 opening
with the epoch-start group at lsn 65,536 (`epoch_lsn` = E, `chain_in` = XXH3-64(new epoch), the restored counters and
HLC maxima), then c18. The restored segment set (`seg.base.11`) and the `hist` file of the old extent are not part of
this fixture: the `HEAD`'s `SegRef` to `seg.base.11` carries a stand-in digest, and a decoder checks it only for form.

### 3.7 Unknown-boot (`hex/unknown-boot/`)

Store U, used by processes that cannot read their boot identity ([OS/proc §5]): `HEAD.boot_id` all zero (U1), a Codex
server's `SlotRec` whose `ProcId` has `boot_known` 0 and `boot_hash` 0 (U4), and a claim whose anchor and deadline carry
`boot_hash` 0 and `mono` 0, so every checker judges it by the wall-clock deadline (U3).

### 3.8 Decode-only (`hex/decode-only/journal-cursor/`)

Store J: record kind 21 `JournalCursor` is reserved (E2 is not built): format-v1 writers never append it, readers decode
it ([F05 §7], §9.21). One lazy record with an upsert and a delete of `JOURNALCUR` rows.

### 3.9 Fragments (`hex/fragments/`)

Structures that no store file holds in every form. Each is decoded as a sequence of one structure to the end of the file.

| File | Holds |
|---|---|
| `values/op-values.hex` | [F08 §5.6]'s value of every type in the stored op-value form, and sets of the element types the stores do not use (enum, text, ref, commitref, oid) |
| `values/field-block.hex` | a field block with one entry of every type, and [F08 §6.2]'s example block |
| `values/cv.hex` | the canonical `cv` of each value ([F07 §7.1]) |
| `anchors/records.hex` | anchor records the stores do not hold: `heading` and `symbol` with Markdown and Rust scopes ([F08 §10.3.1], as an import keeps them), hash-only `quote` and `range` (`end_h`), `pred` with `occurrence`, a planned `file` anchor (blob `none`) |
| `schema/items.hex` | schema item records of classes 1–6 ([F08 §8.5]), in item key order: field items with default, range, index, coerce and flags; enumeration values with a lattice; an edge kind with core and extension `KindSet` bits; a retired item; a `policy` row ([F08 §8.5.6]: `iflags` 0, `name` and `value` as `vstr`) |
| `ops/variants.hex` | op frames the stores do not carry: the value-conflict classes FieldEdit, StatusFork, OwnerFieldEdited, PathClaim; conflicts on status, body, observation, edge and schema keys; `Violation` QueryInvalid and QueryCycle; `Resolve` theirs, base, repoint and drop (a flagged `blocks` out-edge of a tombstone on a work branch, [F12 §6.5]); a changed and a dropped named query; `Schema` ops of the item classes 1–4 and 6 with their stored keys ([F08 §8.5]: the names joined by one `00` byte, `*` = `2A`): a kind, a field strengthened (mode 1), an enumeration value of `priority` (kind `*`) and one of a project kind's `status`, an edge kind, a `policy` row removed back to its default and another written for the first time |
| `runtime/os-ids.hex` | `OsFileId` of every kind (the Linux id with its handle digest, a macOS `docid`), `VolumeCaps` of other volume classes (`dir_flush_doubtful` included), `FsTime` granularities and the absent form |
| `binding/binding-ext.hex`, `binding/heads-row.hex`, `binding/clienthead-payload.hex` | [F18 §3.2]'s example `BindingExt` alone, inside a `HEADS` row image, and inside a `ClientHead` payload |

### 3.10 Candidate codecs (`hex/codec/`)

One synthetic store's sealed files that no `HEAD` or log names. Each compressed payload is a valid frame of its candidate
codec that decodes to the stated raw bytes; it was decoded back with `lz4_flex` 0.14.0 (candidates 1, 2) and `ruzstd`
0.9.0 (3, 4). The frames are not the output of the encoder the product will pin, so the files are hole-dependent (§4).

| File | Candidate | Holds |
|---|---|---|
| `dict.1.hex` | `F10-dict-form` raw content | `DictHdr` and 319 bytes of raw dictionary content (351 bytes in all) |
| `blobs.2.hex` | `F10-blob-codec` 2 `lz4-dict`, `dict_no` 1 | two bodies as LZ4 blocks whose matches reach into dict.1, a short body at codec 0 (the fallback of [F10 §3.4]), a fingerprint blob (always codec 0) |
| `blobs.5.hex` | `F10-blob-codec` 3 `zstd` | store B's b6 and b7 bodies as zstd frames (libzstd 1.5.7), a short body at codec 0 |
| `dict.6.hex` | `F10-dict-form` formatted zstd dictionary | a 34,182-byte [RFC 8878] §5 dictionary with `Dictionary_ID` 6 |
| `blobs.7.hex` | `F10-blob-codec` 4 `zstd-dict`, `dict_no` 6 | two bodies as zstd frames with `Dictionary_ID` 6, a short body at codec 0 |
| `hist.3.hex` | `F10-hist-codec` 3 `zstd` | store B's extent 1 in hist.2's frames, one zstd frame per block (the split frame's two blocks each compressed) |
| `hist.4.hex` | `F10-hist-codec` 1 `lz4` | the same frames as LZ4 blocks |

A compressed blob's hash and a compressed frame's `raw_xxh3` are directives over literal operands that spell the raw
bytes, so the raw content is in the file and the assembler checks the hash (a raw content above 4,096 bytes has its hash
as literal bytes instead).

## 4. Holes and candidates

The fixtures are written before WP-81a fills the holes of `docs/spec/HOLES.md`. Files outside this table depend on no
hole's value: they use codec 0, no dictionary and values valid under every candidate.

| Hole | What the fixtures assume | Files | After the fill (WP-20b) |
|---|---|---|---|
| `F09-doclen` | **kept**: `DOCLEN` and `FTSSTAT` exist beside `TERMS` and `POST` | `hex/store-a/seg.d7.hex`, `seg.d8.hex`, `seg.base.10.hex` | dropped: the two sections leave these files, whose `total_len` and digests change, and with them the `SegRef`s and `Checkpoint` records of `hex/store-a/log.1.hex` and every store-A `HEAD` |
| `F10-codec-values`, `F10-blob-codec`, `F10-hist-codec` | each candidate value once: 1 `lz4`, 2 `lz4-dict`, 3 `zstd`, 4 `zstd-dict` | `hex/codec/*` | the files of rejected candidates are deleted; the chosen ones are rebuilt with the pinned encoder's bytes, and store fixtures gain compressed `blobs` and `hist` files |
| `F10-dict-form`, `F02-dict-file` | raw content (`dict.1`) and formatted (`dict.6`); the stores hold no `dict` file (the candidate `absent`) | `hex/codec/dict.*` | a store whose form is present gains its `dict.<D>` and the `SegRef` of kind 3 in `HEAD.segments` and in its `Checkpoint` records |
| `F10-body-placement` | **raw**: every body in a log record has codec 0 | all logs | if compressed: a commit carrying a compressed body |
| `F20-window-lines` (`WIN`) | 16: one anchor's window has 16 hashes on each side | `hex/fragments/anchors/records.hex` | with `WIN` 8 that window becomes invalid and is shortened |
| `F20-btime-ntfs` | store A's NTFS `VolumeCaps.btime` is 1 `tunneled-not-copied` | `hex/store-a/log.1.hex` (`TreeReg`), `TREES` sections | if the class is `absent`, the value becomes 0 |
| `OS-win-boot-source` | the Windows boot identity of [OS/proc §4.2]'s current text (counter `B` with MachineGuid) | store A's `boot_id`s, `ProcId`s, anchors | a different source changes only the synthetic inputs |
| `F17-*` production values | none: every store uses the test profile ([F17 §12]) | — | WP-20b adds a `HEAD` with production `InitParams` |

## 5. The rows of [60 §2.5]

One row per item of `docs/spec/COVERAGE.md` (its §3–§11), in its order. "Not a byte structure" names what verifies the
item instead; "outside `hex/`" names the fixture family that holds it.

### 5.1 Preamble and the [AR] rows

| Item | Fixtures |
|---|---|
| 60-pre | every `HEAD` (`format` 1 in both slots), every `SegHdr` and `GitmapHdr` (`format` 1); `hex/head/fatal/format-2.hex` (a newer version refused) |
| 60-AR-LOCK | `hex/lock/init.hex`, `hex/lock/live.hex`, `hex/unknown-boot/LOCK.hex`; the lock bytes beyond EOF are not file content (§3.4) |
| 60-AR-HEAD | `hex/head/*.hex`, `hex/head/two-slot/*.hex`, `hex/head/flags/*.hex`, `hex/store-b/HEAD.hex`, `hex/chain/*/HEAD.hex`, `hex/unknown-boot/HEAD.hex`, `hex/decode-only/journal-cursor/HEAD.hex`; `refs_lsn`/`pins_lsn`/`heads_lsn`/`markers_lsn`, `image_cursor` and 17 `seq_ring` entries in `hex/head/two-slot/*.hex`; flag bits 0 (the `new` slots of `hex/head/two-slot/`), 1 (every store-A state after g46), 2 (`flags/readonly.hex`); a reserved bit in `hex/head/fatal/reserved-bit.hex` |
| 60-AR-HEAD-params | `InitParams` in every `HEAD`; `hex/head/fatal/init-mismatch.hex`; `project_oid_algo` 1 (store A) and 2 (`hex/store-b/HEAD.hex`) |
| 60-AR-Log-extents | every `log.*.hex` is exactly E = 65,536 bytes and zero beyond its valid log; extent 2: `hex/store-b/log.2.hex`, `hex/chain/re-roll/log.2.hex` |
| 60-AR-Log-RecHdr | every record of every `log.*.hex` and every record inside `hex/store-b/hist.*.hex` |
| 60-AR-Log-chain | every `log.*.hex`; groups never spanning extents: `hex/store-b/log.1.hex` (pad) and `log.2.hex` (`ExtentHead.chain_in`) |
| 60-AR-Log-kinds | `hex/store-a/log.1.hex` (kinds 1–11, 13–20, 22–28), `hex/store-b/log.1.hex` (12 `Noop`), `hex/decode-only/journal-cursor/log.1.hex` (21); `Marker` settled, deleted, cleared, holders, nonlinear; `Checkpoint` with per-ref lists (g22, g46) and a promotion (g24) |
| 60-AR-CommitBody | `hex/store-a/log.1.hex` (every presence bit but `pruned` and `stage` (§6): `ref_old`, `prev_on_ref`, `sync_base`, `absorbed` (c6, c8), `foreign_git`, `verified`, every `import`); `hex/store-b/hist.5.hex` (`pruned`) |
| 60-AR-Ops | `hex/store-a/log.1.hex` (every op tag, with before-images and `prev`); `hex/fragments/ops/variants.hex` |
| 60-AR-Values | `hex/fragments/values/op-values.hex`, `field-block.hex`, `cv.hex`; stored values throughout store A's ops, field blocks and promoted columns |
| 60-AR-Canonical | outside `hex/`: `canonical/`, `carrier/` (WP-21). The commit ids and digests in `hex/` are synthetic (§2) |
| 60-AR-Seg-hdr | `hex/store-a/seg.*.hex`, `hex/store-a/cs.6.hex`, `hex/store-b/seg.d3.hex`: every section tag of [F09 §3.1]; `EDGE_PROPS` in each; `TERMS`/`POST` in `seg.d7`, `seg.d8`, `seg.base.10` |
| 60-AR-Seg-runtime | `REFS`, `PINS`, `HEADS`, `MARKERS` in `hex/store-a/seg.d1.hex`, `seg.d7.hex`, `seg.d8.hex`, `seg.base.10.hex`, `hex/store-b/seg.d3.hex`; the `RefTable`, `Pin`, `ClientHead` and `Marker` records of `hex/store-a/log.1.hex` that fold into them; `hex/fragments/binding/heads-row.hex` |
| 60-AR-Seg-branch | `hex/store-a/seg.b1.5.hex` (`TOUCH`) |
| 60-AR-Seg-sealed | `hex/store-b/hist.2.hex`, `hist.5.hex`, `hex/codec/hist.*.hex` (frames, split frames, `HCIDX`); `hex/store-a/blobs.*.hex`, `hex/store-b/blobs.4.hex`, `hex/codec/blobs.*.hex`, `hex/codec/dict.*.hex`; `hex/store-a/gitmap.3.hex`, `gitmap.4.hex` |
| 60-AR-Schema | `SCHEMA` in `hex/store-a/seg.d1.hex`, `seg.base.10.hex`, `cs.6.hex`; the `Schema` op of c1; `hex/fragments/schema/items.hex` (item classes 1–6); the `Schema` ops of `hex/fragments/ops/variants.hex` (modes 0 and 1, item classes 1–6, stored keys) |
| 60-AR-Image | outside `hex/`: `moi/`, `carrier/` (WP-21) |
| 60-AR-Layout-dir | not a byte structure: the file names of every store directory here follow [F02 §5]–§6; directory contents are checked by the engine's and the simulator's tests |
| 60-AR-Layout-config | not a byte structure of this family: the `config` text is checked by `moirai-config`'s tests (M1) and [CFG]'s examples |
| 60-AR-Layout-pointer | not a byte structure of this family (a text file, [F02 §3.3]) |
| 60-AR-Layout-names | every file name here: `log.1`, `seg.base.10`, `seg.d7`, `seg.b1.5`, `cs.6`, `hist.2`, `blobs.4`, `gitmap.3`, `dict.1` |
| 60-AR-Layout-userconf | not a byte structure (paths per OS, [F02 §7]) |

### 5.2 The issue-2 rows

| Item | Fixtures |
|---|---|
| 60-I2-FM(1) | the torn slots of `hex/head/two-slot/torn-*.hex` and `*-torn.hex` (512-byte pieces); the lost sector of `hex/chain/lazy-tail/log.1.hex` |
| 60-I2-FM(2)–FM(6), FM(8)–FM(11) | not byte structures: the in-memory `Vfs` and the crash enumerator enforce them (WP-30, WP-31) |
| 60-I2-FM(7) | `hex/unknown-boot/*.hex` (Unknown-boot mode); the `Stamp` deadlines `{wall, boot_hash, mono}` of store A's leases |
| 60-I2-FM(12) | `hex/chain/lazy-tail/` (a failed record above `durable_lsn`: the end of the log), `hex/chain/corrupt/` (below it: corruption) |
| 60-I2-PD(a), (c), (e), (f), (j), (k), (l), (m), PD-added | not byte structures: protocol rules, checked by WP-40's seeded bugs and the crash gates. Their byte results are here: a durable publish leaves both slots with one state (`hex/head/two-slot/new-new.hex`, `hex/store-b/HEAD.hex`), and the retirement and gc `Checkpoint` records that a barrier precedes are in `hex/store-b/log.2.hex` |
| 60-I2-PD(b) | `hex/chain/wrong-position/`, `hex/chain/lazy-tail/`, `hex/chain/corrupt/` |
| 60-I2-PD(d) | `hex/head/two-slot/*-torn.hex`, `torn-*.hex`: a torn slot selects the other one |
| 60-I2-StoreParams | `InitParams` in every `HEAD`; the tunable parameters are `config` keys, not bytes |
| 60-I2-TestProfile | every store (§2): E = 64 KiB, `hist` frames of 4 commits and 4 KiB (`hex/store-b/hist.2.hex`), tier 2 at 16 rows (`hex/store-a/seg.d7.hex`) |
| 60-I2-Gate0 | outside `hex/`: `carrier/` (WP-21) |
| 60-I2-Derived | not a byte structure beyond the bitsets: `BM`/`BMDIR` in store A's and store B's segments hold the persisted predicates; `affected` lists in store A's commits |

### 5.3 The rows added by the priority audits

| Item | Fixtures |
|---|---|
| 60-AU-HEAD-fields | `durable_lsn` below `committed_lsn`: `hex/chain/lazy-tail/HEAD.hex`, `hex/decode-only/journal-cursor/HEAD.hex`; `boot_id`: every `HEAD`, zero in `hex/unknown-boot/HEAD.hex`, changed in `hex/head/flags/boot-change.hex`; `config_gen` 3 in `hex/head/flags/readonly.hex`; bit 3 `retired` in `hex/head/flags/retired.hex` |
| 60-AU-HEAD-bootrecovery | `hex/head/flags/boot-change.hex`; the two-slot barrier: both slots equal after a durable publish (`hex/head/init.hex`, `hex/store-b/HEAD.hex`, `hex/chain/*/HEAD.hex`) |
| 60-AU-HEAD-bootid | every `boot_id` (Windows, Linux, macOS derivations: `hex/head/*`, `hex/store-b/HEAD.hex`, the `ProcId`s of `hex/lock/live.hex`); `hex/unknown-boot/*.hex` |
| 60-AU-HEAD-rmw | not a byte structure: the publish rule; its results are the `slot_seq` pairs of every `HEAD` |
| 60-AU-LOCK | `hex/lock/live.hex` (slot records of every kind, primary and alias hashes, `ProcId` per OS), `hex/lock/init.hex` |
| 60-AU-Log-validity | every group of every log; `hex/chain/wrong-position/` (lsn ≠ position), `hex/chain/lazy-tail/` (bad trailer and record), `hex/chain/corrupt/` (bad checksum) |
| 60-AU-Log-kinds | `hex/store-a/log.1.hex`: `AnchorRes` (g48), `Backup` (g25), `SessionMark` (g51) |
| 60-AU-Log-TreeReg | `hex/store-a/log.1.hex` g47 (register, settle epochs, dirty row, forget) |
| 60-AU-Commit-actor | every commit's `actor`; `CREATOR` in every segment |
| 60-AU-Commit-digest | every commit's `changeset_digest` (synthetic, §2) |
| 60-AU-Commit-csref | c14 in `hex/store-a/log.1.hex`, `hex/store-a/cs.6.hex`; the unlanded reservation of `hex/store-b/log.2.hex` (`FILES` `reserved` in `seg.d3`) |
| 60-AU-Commit-inline | not a byte structure: P05 decides inline or bulk; both forms are here (c17 inline, c14 bulk) |
| 60-AU-Seg-cs | `hex/store-a/cs.6.hex` |
| 60-AU-Seg-derivedopt | `hex/store-a/seg.d8.hex`, `seg.base.10.hex` (the five sections with the flag), `seg.d7.hex` (absent) |
| 60-AU-Seg-markers | `MARKERS`, `MARKERS_OLD` in store A's segments |
| 60-AU-Seg-leases | `LEASES` in store A's segments and `hex/store-b/seg.d3.hex` (a run-scoped lease); the anchors (`session`, `session-ttl`, `none`), `Stamp` deadlines and `files_owned` globs of the `Lease` records in `hex/store-a/log.1.hex` and `hex/store-b/log.2.hex` |
| 60-AU-Seg-alloc | `ALLOC`, `UIDX` in store A's segments; the reserved holes in `hex/store-b/seg.d3.hex` |
| 60-AU-Seg-r4rt | `ANCHORRES`, `GLOBIDX`, `TREES` in `hex/store-a/seg.d8.hex`, `seg.base.10.hex` |
| 60-AU-Seg-hist | `hex/store-b/hist.2.hex`, `hist.5.hex`, `hex/codec/hist.*.hex` (the frame rule at the test values P03 4, P04 4,096) |
| 60-AU-RefTable | `overlay_ops`, `overlay_bytes` in `REFS` rows and `RefTable` entries of store A |
| 60-AU-Schema-lane | not a byte structure (the core schema row, [F08 §9.3]); no store holds a lane node |
| 60-AU-Canon-digest, -lineorder, -anchordigest | outside `hex/`: `canonical/`, `carrier/`; the hash-only anchor records of `hex/fragments/anchors/records.hex` hold the digests |
| 60-AU-Image | outside `hex/`: `moi/`, `carrier/` |
| 60-AU-Vfs-classes, -renames, -sims | not byte structures ([F15], [OS/fs]) |
| 60-AU-Vfs-projfs | `VolumeCaps` and `OsFileId` in `TREES`/`FILEOBS` rows and records of store A (NTFS) and every kind in `hex/fragments/runtime/os-ids.hex`; `JOURNALCUR` rows in `hex/decode-only/journal-cursor/log.1.hex` (store A's `JOURNALCUR` sections are empty); `DIRMAP` in `hex/store-a/seg.d8.hex`, `seg.base.10.hex` |
| 60-AU-R14-E3d, -reuse, -E6, -twin, -copy, -denials | not byte structures: resolver rules (`r4/`, WP-21; the model and the resolver's tests) |
| 60-AU-R14-fold | outside `hex/`: `r4/`; `PATHIDX` keys in store A's segments are ordered by `fold_v1` |
| 60-AU-R14-identity | whole `OsFileId` values: kind 1 in store A's `FILEOBS` rows; kinds 0–4, the Linux id with its handle digest, in `hex/fragments/runtime/os-ids.hex` |
| 60-AU-Config | not a byte structure of this family |
| 60-AU-CrossPlatform | the X-F rows (§5.6) |
| 60-AU-Harness | `LEASES` `kind`, `role`, `run`, `anchor`, `bound` (store A, store B); `actor_src` in every commit; the rest is text output (not bytes here) |

### 5.4 The paragraph after the audit rows

| Item | Fixtures |
|---|---|
| 60-PA-(a), (c), (g), (h), (i) | not byte structures (protocol rules; their byte results as in §5.2) |
| 60-PA-(b) | `hex/chain/lazy-tail/`, `hex/chain/corrupt/` |
| 60-PA-NewParams | not byte structures (config keys); P03/P04 in `InitParams` |

### 5.5 R4 and R5 reservations

| Item | Fixtures |
|---|---|
| R-1 | `path`, `oid`, `pathmove` in `hex/fragments/values/*.hex` and in store A's fields (`path_moves`, `aliases`, `origin_path`, `oid`) |
| R-2 | the artifact #5 of store A (`origin_path`, `observed_git`, `artifact_kind`, `aliases`); the `observation` key in `hex/fragments/ops/variants.hex` |
| R-3 | derived uids #4 (root) and #5 (file) of store A; anchor uids; `uid_derivation` in `hex/fragments/schema/items.hex` |
| R-4 | `at` edges with discriminators and anchor records in store A (c1, c2's `SetEdgeProps` repin, `ANCHORS`, `ANCHOR_UID`); `hex/fragments/anchors/records.hex` (`pred`) |
| R-5 | the root node #4 of store A (`root`, `path_moves`) |
| R-6 | `next_anchor` in every store-A `HEAD` |
| R-7 | `hex/store-a/log.1.hex` (15–20, 22–26), `hex/decode-only/journal-cursor/log.1.hex` (21) |
| R-8 | every R-8 section in `hex/store-a/seg.d8.hex` and `seg.base.10.hex` |
| R-9 | `hex/store-a/blobs.9.hex` and `FPRINT`; `hex/codec/blobs.2.hex` |
| R-10 | outside `hex/`: `canonical/cases/anchors.cases`; the stored records it maps from are in store A and `hex/fragments/anchors/records.hex` |
| R-11 | outside `hex/`: `moi/`, `carrier/` |
| R-12 | not byte structures (invariants) |
| R-13 | not byte structures (config keys) |
| R-14 | the fingerprint value of `hex/store-a/blobs.9.hex`; window values and span hashes in anchor records; the rest is `r4/` |
| R-15 | `hex/fragments/binding/*.hex`; the binding of lane/l5np in `hex/store-a/log.1.hex` g9 and `HEADS` |
| R-16, R-17 | not byte structures (strings of the output contract); the `relink` symbol `explicit/intent` in store A is one of R-17's tokens |
| R-18 | `FILEOBS` rows in `hex/store-a/log.1.hex` g48 and store A's segments |
| F1, F2 | `hex/fragments/schema/items.hex` (edge kind: `lq_name`, `src_kinds`, `dst_kinds`, `reverse_names`, `reading`; field: `optional`, `default`, `index`, `coerce`, `sort_rank`) |
| F3 | the `QUERIES` item in `SCHEMA` and c1's `Schema` op (store A); `hex/fragments/ops/variants.hex` (changed and dropped query) |
| F4 | `CREATOR` in every segment |
| F5 | `FPROMO`, `FCOL.*`, `FIDX.*` in `hex/store-a/seg.d1.hex`, `seg.b1.5.hex`, `seg.base.10.hex`, with all-absent `FCOL`s and a base `FIDX` with `n_values` 0 ([F09 §10.1] presence rule); no `FPROMO` in `seg.d7.hex`, `seg.d8.hex`, `cs.6.hex` and `hex/store-b/seg.d3.hex`, whose rows hold no promoted value |
| F6 | frozen bitsets (chunk `card`, `SecEnt.count` totals) in `hex/store-a/seg.base.10.hex`, `TOUCH` in `seg.b1.5.hex` |
| F7 | `STATS` in `hex/store-a/seg.base.10.hex` |
| F8 | `FrameHdr` in `hex/store-b/hist.*.hex`, `hex/codec/hist.*.hex` |
| F9 | per-ref lists with `append_hlc` in the `Checkpoint` records of store A (g22, g46) and store B |
| F10, F14, F16 | every commit header in store A (`stmt_origin`, `stmt_sym`, `stmt_hash`, `append_hlc`, `affected_len`, `affected_complete`) |
| F11 | `CONFLICTS` in store A's segments (`cs.6` holds a conflict value) |
| F12 | `tok_ver` 1, `TERMS`, `POST`, `DOCLEN`, `FTSSTAT` in `hex/store-a/seg.d7.hex`, `seg.d8.hex`, `seg.base.10.hex` (§4) |
| F13 | `PATHIDX`, `ALIASIDX`, `ANCHORS` in store A's segments |
| F15 | not a byte structure (an invariant); `affected_complete` values are in store A's commits |
| F17 | `ALLOC`, `UIDX` in store A's and store B's segments |
| F18 | `Violation` QueryInvalid and QueryCycle in `hex/fragments/ops/variants.hex` |

### 5.6 Cross-platform rows and [90 §10.1]

| Item | Fixtures |
|---|---|
| X-F1 | `hex/lock/*.hex`, `hex/unknown-boot/LOCK.hex` |
| X-F2 | `ProcId` of all three OS tags in `hex/lock/live.hex` and `hex/store-b/log.2.hex`; `Anchor` kinds 0, 1, 2 and 4 in the `Lease` and `FsIntent` records and the `LEASES` and `FSINTENT` rows of stores A and B; `hex/unknown-boot/*.hex`; kind 3 `leader` is held by no v1 record (§6) |
| X-F3 | every chained group; `hex/chain/*` |
| X-F4, X-F5, X-F12 | not byte structures beyond `RecHdr.flags` bit 0 (every record; g52 shows a configurable kind made durable) |
| X-F6 | `total_len` in every `SegHdr`, `GitmapHdr` and `DictHdr` |
| X-F7 | `PATHIDX` keys in store A; the canonical path form is `r4/` |
| X-F8 | `OsFileId`, `VolumeCaps`, `FsTime`, `DIRMAP`, `TREES` rows in store A; `JOURNALCUR` rows in store J; every `OsFileId` kind and further `VolumeCaps` and `FsTime` forms in `hex/fragments/runtime/os-ids.hex` |
| X-F9 | the query name in store A's `QUERIES` item; the file name is `moi/`'s |
| X-F10 | every file name here |
| X-F11 | not a byte structure |
| 90-LEASES | `LEASES` in store A's and store B's segments |
| 90-Anchor | session hashes (§2) in `hex/lock/live.hex` and the leases' anchors |
| 90-CommitHeader | `actor_src` in every commit of store A |
| 90-Output, 90-Errors, 90-Card | not byte structures |
| 90-Codec | `hex/codec/*` (§3.10, §4) |

## 6. Structure checklist

| Structure | Values in `hex/` | Not covered, and why |
|---|---|---|
| Record kinds ([F05 §7]) | 1–28 | — |
| Commit kinds, import provenance, presence bits ([F06 §3], §4.2) | kinds 0–5, provenance 0–3, bits 0–17 | bit 18 `stage` and its order-45 group ([F06 §4.4.16]; spec sync 2b S2B-F-1): the format oracle decodes the group now, but no fixture can hold it yet. Store A's staged c11 had no `--base`, policy override or `strict`, so its group is absent, and no fragment kind holds a lone commit record; a new fixture file needs its own row in the format oracle's fixture table first, so none is added in this update |
| `stmt_origin`, `actor_src` ([F06 §3.4], §3.5) | 0–6 each | — |
| Op tags ([F06 §7.2]) | 1–16 | — |
| Value-conflict classes ([F12 §6.1]) | 1, 2, 3, 4, 6, 7 | 5 `SupersedeFork`: which key holds it is open in [F12] ([RULES/merge-table] open point 9) |
| `Resolve` choices ([F06 §7.7]) | 0–5: 0 and 3 in store A; 1, 2, 4 and 5 `drop` (a flagged `blocks` out-edge of a tombstone, [F12 §6.5]; spec sync 2b S2B-F-10) in `fragments/ops/variants.hex` | — |
| `Schema` op ([F06 §7.6]) | modes 0 and 1, item classes 1–6: a new query in store A (c1); in `fragments/ops/variants.hex` a changed and a dropped query, a kind, a field strengthened (mode 1), two enumeration values (one with the `*` component), an edge kind, a `policy` row removed and one written, each `item_key` in [F08 §8.5]'s stored key form (spec sync 2b S2B-F-13, S2B-F-21) | — |
| Violation classes ([F19 §12.2]) | 66, 70, 71, 75 | the other structural classes differ only in their code |
| Value types ([F08 §5]) | 0–13, sets of every element type | — |
| Anchor records ([F08 §10.3]) | kinds 1–6, every `aflags` bit, scopes for `rust` and `markdown` | scope `lang` 3 `toml`: bytes identical in form |
| Schema items ([F08 §8.5]) | classes 1–6 (6 `policy`, [F08 §8.5.6]; spec sync 2b S2B-F-21), retired | — |
| Section tags ([F09 §3.1]) | every tag and both ranges (`FCOL`, `FIDX`, `BM`) | — |
| `SegHdr.seg_kind` | 2, 3, 4, 5, 6, 9 | — |
| Sealed headers | `SegHdr`, `GitmapHdr` (`sha1`, `sha256`), `DictHdr` (raw and formatted content) | — |
| Codec byte ([F10 §3.1]) | 0 in every store; 1–4 in `hex/codec/` | — |
| `hist` frames | one-block, split, codec 0, 1, 3, a header-only commit | a frame that falls back to codec 0 inside a compressed `hist` file (the fallback is shown in `blobs`) |
| `LOCK` records | `LockHdr`, `WriterDiag`, `LeaderRec`, `SlotRec` kinds 1 and 2 (the only valid ones, [F03 §8.2]), an absent record | — |
| `OsFileId`, `VolumeCaps`, `FsTime` ([F11 §12.1]–§12.3) | `OsFileId` kinds 0–4; five `VolumeCaps` classes besides store A's NTFS; `FsTime` exponents 0, 2, 9 and absent | — |
| `Anchor` kinds ([F03 §10]) | 0, 1, 2, 4 | 3 `leader`: made only by the optional leader for its own records; no v1 record holds it |
| `HEAD` | every field; flags 0–3 and a reserved bit; nine two-slot states; torn slots; five fatal cases | `SegRef` kind 3 (`dict`): hole-dependent (§4) |
| Log chain | first group, chained group, `Noop` pad, `ExtentHead` (epoch start, extent start, re-roll), wrong position, lost lazy tail, corruption | — |
| `Checkpoint` bits ([F05 §9.9]) | 0–9 | — |
| `RefUpdate` reasons, `ClientHead` ops and row kinds, `Lease` events, `Marker` kinds, `Pin` ops and holders | every value | `Lease` release reasons other than 1 and 2, marker causes other than a commit and a fork: values of one byte, no other layout |
