# Review pass 1: dispositions of R-SPEC-F

| | |
|---|---|
| Title | Dispositions by R-SPEC-F of the pass-1 findings routed to it ([P-pass1], [S-pass1], [A-pass1]) |
| Status | review, pass 1; dispositions await the owner's signature (WP-80, V2) |
| Work package | WP-80 pass 1, author side: R-SPEC-F (WP-10, WP-12, WP-14, WP-18, WP-19, WP-25) |
| Files of R-SPEC-F | [F01], [F02], [F06], [F07], [F08], [F12], [F18], [F19], `lq/*`, [API] with its examples, [CFG], `COVERAGE.md`, `HOLES.md`, `README.md` |

Dispositions:

- **fixed**: the change is made in the named files of R-SPEC-F.
- **fixed (F part)**: the part of the fix that falls in files of R-SPEC-F is made; the rest of the finding lies in files
  of another author role (R-SPEC-P: [F03]–[F05], [F13], [F15]–[F17], `os/*`; R-SPEC-R: [F09]–[F11], [F14], [F20];
  R-MODEL: `rules/*`), which disposes of it in its own dispositions file. Where the fix of R-SPEC-F fixes a name, size,
  code or number that another chapter must use, the entry says so.
- **not F**: no text of R-SPEC-F is concerned; the finding is left to the owning role.
- **rejected**: with the reason.

**Summary.** 148 findings routed: 42 fixed, 55 fixed in their R-SPEC-F part, 51 not in R-SPEC-F's files, 0 rejected. Three
proposed alternatives were not taken, each with its reason in its row: P1-2's and A1-8's full git-ref spelling (the short
form of [F18 §3.2] rule 1 is kept), and A1-3's bit order for the edge property block ([F08 §10.2]'s order is kept). Numbers
that other roles must now use: [F08 §5.1]'s registry (type 0 `absent`, `commitref` 32 B), [F08 §10.2]'s `pflags` (bit 0
`has_pin`, bit 1 `flagged`, bit 2 `anchor`) and 32-byte pin, [F06 §6.1]'s `ckey` classes 1–9, op 16 `CreateDeleted`,
presence bits 16 `pruned` and 17 `ckimg` with order 44, `prov` after `theirs`, [F18 §4.10]'s codes and proposal tuple, the
keys `query.budget.default.wmem`, `query.caps.<role>.wmem` and `input.max-bytes` (and `tx.wmem-max` retired), and the
[F19 §10.2] codes `maintenance_busy`, `no_dir_flush`, `commit_pruned`.

| Finding | Severity | Disposition | Where / reason |
|---|---|---|---|
| P1-23 | minor | fixed | [F01 §5.2]: the tenth byte of a `uvar64` must be `01`. |
| S1-36 | minor | fixed | [F01 §8.3] cites [F11 §8] for the `IDEM` row and keeps only the width of `branch_sym`. |
| P1-1 | blocker | fixed (F part) | [F08 §5.1] is the one value registry and [F08 §10.2]–§10.3 the one edge property block and anchor record; [F06 §5] and [F06 §7.5.2]–§7.5.3 cite them byte for byte and define no tag or layout of their own. `commitref` and `pinned_commit` are 32 bytes in every stored form ([F08 §5.1], §10.2). Empty text and empty set are absent everywhere ([F08 §5.3], [F06 §5.2]). `pathmove` classes 1–4, 0 invalid. `prov` added to [F06 §6.2], [F06 §7.7] and [F07 §7.3]. [F07 §7.1]'s canonical tags declared its own. Cross-chapter numbers other roles must use: [F09 §7.2] `EDGE_PROPS` stores the full 32-byte pin and [F08 §10.2]'s `pflags` (R-SPEC-R); [F11 §10] `CONFLICTS` stores `kval` sides and `prov` (R-SPEC-R, S1-7). The [F06 §11] example holds no value and is unchanged; `COVERAGE.md` rows 60-AR-Values and R-4 updated |
| S1-1 | blocker | fixed | As P1-1. Also: ±infinity refused and invalid everywhere ([F08 §5.3]); one set order ([F08 §5.5]); `ref` is `u32` ([AR §3.1] "node refs are u32"). [F06] open point 26, [F08] open point 12 |
| A1-1 | blocker | fixed | As P1-1 and S1-1; empty set and empty text decided once: absent ([F07] open point 26's body case too, S1-34) |
| S1-2 | blocker | fixed (F part) | [F08 §10.2] owns one block: `pflags` (bit 0 `has_pin`, bit 1 `flagged`, bit 2 `anchor`), `pinned_commit` `b32`, the anchor record. [F06 §7.5.2] cites it; [F07 §8.1] uses the same bit order. The [F09 §7.2] row `{edge u32, pflags u8, pad[3], pinned [32]}` = 40 B is R-SPEC-R's |
| A1-3 | blocker | fixed (F part) | As S1-2. A1-3's alternative (align [F08] to [F06]'s bit order, bit 0 `flagged`) is not taken: [F08] owns the block (P1-1), and S1-2 keeps [F08]'s order; the full 32-byte id is kept as A1-3 asks |
| S1-3 | blocker | fixed | [F08 §10.3] is the only layout; the op carries it plus `anchor_no` ([F06 §7.5.1], V: record `uid` = `disc`); `blob` may be `none` for a planned target (order 23); texts are `vbytes` (not necessarily UTF-8); `captured` uses [F08 §10.3.1]'s bytes; [F06 §7.5.3] is a citation. [F14 §5.6] stays the bijective text form (R-SPEC-R) |
| A1-2 | blocker | fixed | As S1-3; the scope conversion is [F14 §5.6]'s bijection onto [F08 §10.3.1] (R-SPEC-R) |
| S1-5 | blocker | fixed | `prov` u8 after `theirs` for existence keys in [F06 §6.2] `cstate` and [F06 §7.7] `Conflict`; [F07 §7.3] hashes it by name ([F07 §2.2]); [F12 §6.3] updated. [F11 §10] carries it (R-SPEC-R) |
| A1-9 | blocker | fixed | As S1-5 |
| S1-6 | blocker | fixed (F part) | [F06 §7.4] op 16 `CreateDeleted` (uid, kind, creator, reason, replaced_by, retained-title image) with NF-11 and no inverse ([F06 §7.10]); [F07 §10.1] and §13 map it; [F12 §7.8] states merges emit it. The [F14 §11.2] import cell is R-SPEC-R's |
| A1-4 | blocker | fixed (F part) | As S1-6 |
| S1-21 | major | fixed | [F06 §4.4.15]: presence bit 16 `pruned`, `n_ops` = `n_bodies` = 0, `changeset_digest` kept, V-rules, `show` prints `changes pruned by gc`, `revert`/`cherry-pick`/`diff` refused with `commit_pruned` (exit 3, [F19 §10.2]) |
| A1-5 | blocker | fixed | As S1-21 |
| S1-23 | major | fixed (F part) | [F06 §4.4.14] `ckimg` (presence bit 17, order 44): per node file the `created:`, `updated:`, `deleted:` values and the ledger lines; [F06 §4.4.11] allows all-zero `first`/`last` exactly when `n_folded` = 0; [F06 §9] BK-5 requires the same entries in a bulk checkpoint's `cs.<n>` ([F09 §16.4], R-SPEC-R) |
| A1-10 | blocker | fixed (F part) | As S1-23 |
| P1-6 | blocker | fixed (F part) | [F06 §9] BK-2: `cs_ref.b3` = `seg_digest[0..16]`; V-8 compares it at open, `doctor --fsck` recomputes `seg_digest`. [F09]/[F10] texts are R-SPEC-R's |
| S1-19 | major | fixed (F part) | As P1-6 (OP-09-03 adopted on the [F06] side) |
| A1-21 | major | fixed (F part) | As P1-6 |
| S1-20 | major | fixed (F part) | [F06 §7.3]: inline commits take `prev` from the chain head under the writer byte; bulk commits rest on [F16] P-34's node-granular re-validation (R-SPEC-P), which [F06 §9] BK-5 cites |
| A1-19 | major | fixed | [F07 §7.3] is the one equality rule; [F12 §7.3] cites it: plain `live` values carry no image, a conflict side's image is part of the value |
| A1-20 | major | fixed | [F08 §8.3]: the store-local id is the item record's own store-local field (`kind_id`, `edge_id`, `value`), carried in the `Schema` op's `new`; [F06 §7.6] says so; no op field added |
| A1-41 | major | fixed (F part) | [F08 §8.5] defines the item key order (class, then name strings bytewise, `*` = `2A`); [F09 §8.3] cites it (R-SPEC-R) |
| S1-34 | minor | fixed | [F08 §7.2]: an empty body is no body; `SetBody` to empty stores `new` absent ([F06 §7.4]); [F07] open point 26 closed |
| S1-22 | major | fixed | Second option: [F12 §6.5] — a take towards a live side restores value keys from the image and the hierarchy key and out-edges from that side's state at the introducing commit, as `Move`/`AddEdge`/`SetEdgeProps` ops of the `Resolve` commit, under the write path's checks. Images unchanged, so no hashed byte changes; [F07] open point 12 closed. [RULES/merge-table] open point 5 is R-MODEL's |
| P1-19 | major | fixed (F part) | [F07 §10.6]: every producer whose entries exceed `wmem` spills to `tmp/sort` runs, merges, syncs, reverts and cherry-picks included. [F17 §4.4] W1 listing `sync` and GT11's case are R-SPEC-P's |
| P1-21 | major | fixed (F part) | [F12 §5.4] adopts open point 3 as the normative MR-005 rule; V05, V07 and V12 have codes 67, 68 and 72 in [F19 §12.2] and keys in [F12 §7.9]; LM-013 is decided by [F08 §11.3] (root nodes are never deleted). The merge-table rows are R-MODEL's |
| P1-29 | minor | fixed | [F12 §5.5a]: virtual-base work and memory are charged to the merge's `wmem` and budgets, spilling per [F07 §10.6]; [F12 §7.5]: HD is O(64 · n²) worst case, measured by WP-60 |
| P1-26 | minor | fixed | [F06] open point 16: kept, and measurement 2 sweeps the inline size up to P05 and reports the writer hold; the fallback (`prev` against a phase-1 base) is named for the case the gate fails |
| S1-9 | blocker | fixed (F part) | [F18 §4.10] owns every state, detail and evidence code, which [F11] and [F05] cite verbatim; anchor state 6 is never stored; a proposal is (`class` u8 per [F20 §1.5] numbered 1 `exact`–4 `weak`, `evidence` u8 = a §5.2 token 13–26, `path` with its root, `score` in [F11]'s encoding of the exact rational). [F11 §12.5] `FILEOBS.state` and [F05]'s records are R-SPEC-R's and R-SPEC-P's to align |
| A1-7 | blocker | fixed (F part) | As S1-9 |
| S1-10 | blocker | fixed (F part) | [F18 §3.7]: `BindingExt` (40 B) is embedded verbatim in [F11] `HEADS` and in [F05] `ClientHead` (whose payload is the `HEADS` row), with no separate flag, base or git-ref field; [F18 §3.2] rule 1 makes the short form the one spelling in every chapter. [F11]/[F05] rows are R-SPEC-R's/R-SPEC-P's |
| A1-8 | blocker | fixed (F part) | As S1-10. A1-8's preference for the full name is not taken (reason in [F18 §3.2] rule 1: canonical item 5 already hashes the short form of the same symbol class, [F07 §3.6]) |
| P1-2 | blocker | fixed (F part) | The [F18] part: codes (§4.10) and `BindingExt` (§3.7) have one owner and are cited verbatim; the git-ref spelling is the short form (not the full form P1-2 proposes; reason as A1-8). The record-to-row alignment (F11 OP-33, `RefTable`, `TreeReg`, `FsIntent` roots, `Marker`) is R-SPEC-R's and R-SPEC-P's |
| S1-8 | blocker | fixed (F part) | As P1-2: the proposal row and the link-state codes are fixed in [F18 §4.10]; the remaining record/row alignment is outside R-SPEC-F's files |
| A1-6 | blocker | fixed (F part) | As P1-2 and S1-8 |
| A1-44 | minor | fixed (F part) | [F18 §2.2]: no chapter encodes a foreign-uid mark; [F18 §2.15] makes §2 the statements of record for [F13 §3.9] (R-SPEC-P); [F18] open point 17 closed in favour of [F19 §4.6] |
| A1-45 | minor | fixed (F part) | [F18] keeps detail 59 `unreadable` (open point 12, owner sign-off); [F20 §1.5]'s "no detail yet" is R-SPEC-R's to align |
| P1-15 | major | fixed (F part) | [F18 §4.6] detail 44 is decided by `representable_here` before any OS call and is never overridden by `--allow-nonportable`. The `ProjectFs` segment check ([OS/path], [OS/project], [OS/fs]) and [F20]'s cascade order are R-SPEC-P's and R-SPEC-R's |
| A1-29 | major | fixed | [F19 §4.6]: the unapproved 90 B / median gate is withdrawn; `moved-needs-confirm` drops the candidate path and an over-long qualified `ambiguous` falls back to the bare state, so every marker is ≤ 49 B with an 11-byte handle and [AR §8.3]'s ≤ 50 B holds without an owner decision ([F19] open point 23) |
| S1-17 | blocker | fixed | [F19 §8.3]: JSON commit ids are `c` + 64 lower-case hex ([50 §2.9] Q18, A-m7) |
| A1-16 | blocker | fixed | As S1-17 |
| S1-44 | minor | fixed | [F19 §7.1] exit 8 includes a file verb whose commit recorded failed items |
| P1-31 | minor | fixed | [F19 §10.2] `store_locked` names the writer or the flush lock (P-72's rotation wait); new `maintenance_busy`, exit 7; `DATA` removed from [F19 §12.1], [F07 §2.2], [F06] (the [F14 §6.8] mention is R-SPEC-R's) |
| S1-32 | minor | fixed (F part) | As P1-31 for [F07 §2.2] and [F19 §12.1]; [F14 §6.8] is R-SPEC-R's |
| A1-43 | minor | fixed (F part) | As S1-32 |
| A1-57 | minor | fixed (F part) | [F19 §3.2] cites `HOLE(CFG-codex-mcp-result)` for the Codex MCP ceiling; the `generic` hook ceiling stays 8,000 B by [90 §6.4]'s profile table, so PE-006's 10,000 B for every profile is R-MODEL's to align |
| A1-59 | minor | fixed | `cross_volume` (and the new `no_dir_flush`) help: `move it with a raw mv; links re-bind by evidence or show a proposal to confirm` |
| A1-30 | major | fixed | [F19] open point 1 adopted in [LQ/envelope §3.2], §3.3, §9.3 and [F19 §4.2]: no `nothing written`, no per-part `<c8>`, and `tx (dry)` counted in the base part (the step the arithmetic also needs: without it the extras are 68 B); `DRY` extras ≤ 57 B |
| A1-31 | major | fixed (F part) | [LQ/envelope §6.5] cites [F19 §6.2] (a cut page ≤ min(24,000, 8,000) B, exit 10). The owner's confirmation against [AR §7.1]'s 24,000 B is listed for WP-81a ([F19] open point 6) |
| A1-32 | major | fixed | [LQ/errors §3.3]–§3.4: the closing line `nothing was written` is never dropped; detail lines cut to 120 B; at most two other detail lines, then a new step that drops them all; the bound recomputed for interpolated lines (≥ 443 B left for the message) |
| A1-48 | minor | fixed | `verify <handle>` with the line's own handle ([LQ/envelope §5.13]); `\u{h}` without leading zeros in [LQ/envelope §5.16], [LQ/errors §2.1] and [F19 §2.5]; the cursor is an offset table for its 36-byte head plus a sequence table ([LQ/envelope §8.1]) |
| A1-51 | minor | fixed (F part) | [F19 §11.3] and [LQ/errors §5.5] state the split as a rendering of the design's one-line text; WZ-001 (R-MODEL) quotes the same words |
| A1-52 | minor | fixed | E408: `key` may be `null` (default key, with its own message and help) and `original` may be `null` (no commit), with the text `original: recorded without a commit` ([LQ/errors §5.5], §5.7) |
| A1-53 | minor | fixed | E304's help renders the caller's `query.caps.<role>.refs` ceiling ([LQ/errors §5.4]) |
| A1-62 | minor | fixed | [LQ/errors §4.2]: `CFGnn: ` diagnostics indent continuations by seven spaces and follow the `W` and `N` codes; [F19 §10.6] and [CFG §6.2] cite it |
| A1-39 | major | fixed | [F19 §10.2]–§10.5 add `bad_ref_name`, `ref_exists`, `ref_prefix` (exit 2), a `not_found` case for a node that is not live (exit 3), `commit_pruned` (exit 3), `name_taken`, `not_merged`, `revert_refused`, `not_fresh` (exit 6), `maintenance_busy`, `no_dir_flush` (exit 7) and the warning `graph_only_revert`; [F19 §10.6] freezes `CFG01`–`CFG17`. [API] open points 5, 20 and 38 and [F12] open point 16 closed |
| A1-36 | major | fixed | [LQ/std §7.3] `tx.complete` writes `done` for every outcome, with the resolution mapping of [API §10.5] and the outcome in the `settled` marker |
| A1-37 | major | fixed | `tx.claim`'s `$ttl` is `text? = NULL`, meaning `lease.ttl-default`; the `BUDGET` classes are a tenth, one and ten times `query.budget.default.work` ([LQ/std §2.4], [CFG] open point 18) |
| A1-38 | major | fixed | `std.ready` orders by `priority, id` ([LQ/std §4.1] and its catalog row); [API §16.6] updated; [50 §4.1]'s text is corrected at WP-81a |
| A1-54 | minor | fixed | [LQ/std §2.10] gives the scalar built-ins' signatures, `subtree(n [, depth])` unbounded by default; [LQ/canonical-ast] Table 5.3 notes the two namespaces' defaults |
| A1-55 | minor | fixed | The difference is stated in both chapters ([CFG §4.1], [LQ/lexical §5.6]): configuration `ms s m h d`, LQ `s m h d w`; they never meet as text |
| A1-56 | minor | fixed | [LQ/canonical-ast §5.4] holds the forward-synonym table (`SUBTASK_OF` → `CHILD_OF`); C-8 and [F08] open point 43 closed |
| A1-58 | minor | fixed | [LQ/std §1] paragraphs renumbered in order |
| P1-39 | minor | fixed (F part) | `input.max-bytes` (16 MiB) in [CFG §10.5]; [LQ/lexical §8] reads stdin and `-f` incrementally and refuses beyond it with exit 2 ([F19 §10.2] `usage` case). The reading code's statement in [OS/shell §5.2] is R-SPEC-P's |
| P1-41 | minor | fixed | [LQ/json-ir §2] item 7: a JSON nesting limit of 256, checked while parsing, E001; [LQ/grammar-v1.ebnf] P13 notes it |
| P1-42 | minor | fixed | [LQ/card §7.3] pre-specifies steps 5–8 (567 B, 18.2 % in all); step 6 already covers the 1,160-token upper estimate; `HOLE(LQ-card-shrink)` candidates are 0–8 |
| P1-11 | major | fixed (F part) | [CFG §10.5]: `wmem` is a budget (`query.budget.default.wmem` 1 MiB, `query.caps.<role>.wmem` 4 MiB for agents, 40 MiB for orchestrator and owner), effective as max(256 KiB, min(requested, headroom)); `tx.wmem-max` retired (§6.4); a default-cap `TX` needs the raise, which E501 names ([LQ/errors §5.4]). [F17 §4.4] W2, W4, §12 TP-3 and its tables are R-SPEC-P's to restate with these keys |
| P1-12 | major | fixed (F part) | [CFG §7.6] and [API §8.1]: `init` refuses values that fail C-1–C-4 (exit 2, `config_value` naming the constraint); [CFG §5.3] and RG-2: a tunable's fallback is min(default, the largest admissible value under the recorded `init` values); examples 01 and 02 pass `store.commit.inline-max-bytes=4KiB`. The [F17 §3], §12 TP-1 texts are R-SPEC-P's |
| S1-39 | minor | fixed | [CFG §4.2]: root names use [F08 §5.4.1]'s grammar (start with a letter) |
| A1-50 | minor | fixed | `[RULES/delete-policy]` → `[RULES/delete-policy-matrix]`; `[RULES/policy-keys]` stays cited as a planned file, as [F01 §2.2] allows (RG-6 says so) |
| P1-16 | major | fixed (F part) | [API §12.4] step 1 calls `sync_dir` on both parents before the `FsIntent` group and refuses `Unsupported`/`AccessDenied` with `no_dir_flush` (exit 7, [F19 §10.2]) before anything changes. The `VolumeCaps` bit, recovery's doctor text and the FL-2 simulator cases are R-SPEC-R's and R-SPEC-P's |
| P1-5 | blocker | fixed (F part) | [API §6.2] CK-4 is confirmed as the rule of record; [F06 §4.4.4], §4.4.5 and [F07 §3.4] cite it. [F16] P-36, [OS/clock §7] and the seeded bug are R-SPEC-P's |
| S1-13 | blocker | fixed (F part) | As P1-5 |
| A1-17 | blocker | fixed (F part) | As P1-5 |
| P1-44 | minor | fixed (F part) | [API §6.2] CK-6: h never restarts; the engine carries it across an epoch re-roll as the HLC floor of the epoch's anchor record (P1-8). [F16 §14] is R-SPEC-P's |
| P1-14 | major | fixed (F part) | R-SPEC-P named the seam `Entropy::fill_random` ([OS/README §4.6], S1-27) and cited it in [F02 §4], §5.3, [F08 §2.2] and [CFG §7.5]; R-SPEC-F adds [API §6.4] (the simulator derives from the seed) and [F08] open point 41's pass-1 note |
| P1-3 | blocker | fixed (F part) | [F12 §8.2] and open point 14 cite [F05 §9.2]'s reason 5 `park`. Kind 27 `Reserve`, reason 5's bytes and [F16]'s seeded bugs are R-SPEC-P's |
| S1-11 | blocker | fixed (F part) | As P1-3; the merge pins of the checkpoint group ([F05 §4.7]) are R-SPEC-P's; [F12 §8.1] already defers the pin to [F16] |
| A1-11 | blocker | fixed (F part) | As P1-3 |
| P1-20 | major | fixed (F part) | [F08 §10.3.1]: the scope bytes are fixed there; the scanner grammar must be a normative [F20] appendix before the freeze and before WP-63; until it exists no writer records a scope and `captured` takes `lp("")` for it, so engine and model cannot diverge; if the appendix misses the freeze, the owner chooses between that rule and removing scope from `captured` (FB-4). The appendix is R-SPEC-R's |
| S1-4 | blocker | fixed (F part) | As P1-20 |
| A1-14 | blocker | fixed (F part) | As P1-20 (the "exclude scope from hashed inputs" option is the interim rule) |
| A1-46 | minor | fixed (F part) | [F06 §6.1]'s `ckey` classes take [F07 §6.1]'s codes 1–8 (schema 9), with [F06 §7.9]'s ranks and [F12 §6.2]'s numbers updated. The [F04 §10], [F17 §4.3]/[F10] and [F17 §5.4] parts are R-SPEC-P's and R-SPEC-R's |
| P1-32 | minor | fixed | [F02 §5.3], §6.3: the `tmp/` word `probe` (with a nonce) and the fixed name `settle.stamp`, both removable by the orphan sweep. [OS/env §5]'s switch to `probe.<nonce>` names is R-SPEC-P's |
| S1-38 | minor | fixed (F part) | As P1-32 |
| A1-40 | major | fixed (F part) | As P1-32 |
| P1-13 | major | fixed (F part) | [F02] open point 17 settled: discovery never runs `swap_recover` ([F16] P-86); the [OS/fs §4.9.4] parenthesis is R-SPEC-P's |
| S1-26 | major | fixed (F part) | As P1-13 |
| A1-18 | major | fixed (F part) | As P1-13 |
| S1-41 | minor | fixed (F part) | [F02 §3.3] rule 6 names the pointer file's durability point as an [F16] protocol point with the seeded bug "success before `durable-name`"; the P-rule, its number and the `LOCK`-record statement are R-SPEC-P's |
| S1-37 | minor | fixed (F part) | `README.md`: the [F10] row no longer claims the `cs` layout ([F09 §16.4]'s); the [F06] and [F08] rows follow P1-1. `FileFamily` in [F11 §2.5] and the `FileRefV` rename in [F05 §8.4] are R-SPEC-R's and R-SPEC-P's |
| P1-25 | minor | not F | `FileRef` in [F05 §8.4] and [F11 §2.5]; no text of R-SPEC-F names either structure |
| P1-33 | minor | fixed (F part) | [F01] open point 15 adopted; `HOLES.md` §1, §4 and its open point 1 record the renames (the id changes were made in [OS/lock], [OS/fs], [F03], [F16], [CFG], [F19] and `lq/` during this pass); [F19] Holes and §3.2 cite `LQ-display-spelling` and `CFG-codex-mcp-result` |
| S1-40 | minor | fixed (F part) | As P1-33. The three decided rule-file "holes" are listed as values in `HOLES.md` §3; replacing the inline `HOLE(...)` in [RULES/pack-classes] and [RULES/role-write-policy] is R-MODEL's |
| A1-42 | minor | fixed (F part) | As P1-33 and S1-40 for [CFG], [F19], [LQ/canonical-ast], [LQ/gql-spelling] and [API]; [F19] open point 22 no longer calls `pack-digest-param` a hole. The [OS/lock], [F03], [F16], [OS/fs] and rule-file parts are R-SPEC-P's and R-MODEL's |
| S1-47 | minor | not F | The `os/*` Coverage sections and [RULES/README §1.1] are R-SPEC-P's and R-MODEL's (R-SPEC-P reports both done; `COVERAGE.md` open points 1 and 6 were marked resolved by it) |
| A1-61 | minor | fixed | `COVERAGE.md` §1 adds the CONFLICTING marker and §2 counts it (146 mapped, 6 CONFLICTING, 0 UNMAPPED). R-4 and R-10 now cite [F08 §10.3] alone for the anchor layout (A1-2 closed); R-4 stays CONFLICTING only for [F09 §7.2] (S1-2), and 60-AR-Values, 60-AR-HEAD-params, R-7, R-15 and R-18 are marked with the findings that other roles still owe |
| P1-4 | blocker | not F | [F04 §4.4], [F17 §2.1] (R-SPEC-P) and [F20] (R-SPEC-R). `COVERAGE.md` row 60-AR-HEAD-params is marked CONFLICTING until they close |
| S1-12 | blocker | not F | As P1-4 |
| A1-13 | blocker | not F | As P1-4 |
| A1-15 | blocker | not F | As P1-4 (`project_oid_algo`) |
| S1-28 | major | not F | As P1-4 |
| S1-7 | blocker | not F | [F11 §10] `CONFLICTS` (R-SPEC-R). R-SPEC-F's side is ready: a `CONFLICTS` side can store the [F06 §6.2] `kval` bytes of the `Conflict` op, `deleted` now carries its kind, and `prov` is defined ([F06 §6.2], §7.7) |
| S1-33 | minor | not F | [F13 §5] (R-SPEC-P). [F19 §12.2] already assigns 67, 68 and 72 and now marks them confirmed |
| A1-12 | blocker | not F | [F05] kind 27 `Reserve` (R-SPEC-P) |
| P1-7 | major | not F | [F16], [OS/fs] (R-SPEC-P) |
| P1-8 | major | not F | [F16], [F04], [F05], [F10], [OS/clock]. [API §6.2] CK-6 now relies on the HLC floor it adds (P1-44) |
| P1-9 | major | not F | [F16], [F17 §5.2], [OS/proc] (R-SPEC-P) |
| P1-43 | minor | not F | As P1-9 |
| P1-10 | major | not F | [F03 §3.1], [OS/lock] (R-SPEC-P) |
| P1-17 | major | not F | [F14 §14.1] (R-SPEC-R) and an [AR §5b.1] change for the owner |
| P1-18 | major | not F | [F16] P-23, [F17 §9.1], [F14 §13] |
| P1-22 | major | not F | [F10 §7.3], [F09] OP-09-14, [F17] |
| P1-24 | minor | not F | [F03 §6] WD-1 (R-SPEC-P) |
| P1-27 | minor | not F | [F10 §8], [F09 §16.4] (R-SPEC-R) |
| S1-35 | minor | not F | As P1-27 |
| A1-22 | major | not F | As P1-27 |
| P1-28 | minor | not F | [F11 §8] and [AR §4.4] size rows; [F01 §8.3]'s restated row is fixed under S1-36 |
| P1-30 | minor | not F | [F17 §4.3], [F10 §4.2] |
| S1-29 | major | not F | As P1-30 |
| P1-34 | minor | not F | [OS/clock] (R-SPEC-P) |
| S1-43 | minor | not F | As P1-34 |
| P1-35 | minor | not F | [OS/README] (R-SPEC-P) |
| A1-47 | minor | not F | [OS/README], [OS/fs] (R-SPEC-P) |
| P1-36 | minor | not F | [OS/mem] (R-SPEC-P) |
| P1-37 | minor | not F | [OS/path] (R-SPEC-P) |
| P1-38 | minor | not F | [OS/project], [OS/mapping-appendix] (R-SPEC-P) |
| P1-40 | minor | not F | [OS/map] (R-SPEC-P) |
| A1-60 | minor | not F | [OS/path] (R-SPEC-P) |
| A1-33 | major | not F | [OS/project], [OS/fs §6.1] (R-SPEC-P) |
| A1-49 | minor | not F | `os/` offset tables (R-SPEC-P) |
| S1-18 | major | not F | [F05 §4.4], [F17 §4.4] W3 (R-SPEC-P); [F06 §4.6] cites [F05]'s reserve and needs no change |
| S1-24 | major | not F | [F16], [F05 §2.2], [F17] IP-6 (R-SPEC-P) |
| A1-24 | major | not F | As S1-24 |
| S1-25 | major | not F | [F16], [F15] (R-SPEC-P) |
| S1-31 | major | not F | [F20 §5.9] and measurement 15 (R-SPEC-R) |
| S1-42 | minor | not F | [F03 §8] (R-SPEC-P) |
| S1-45 | minor | not F | [F04 §10], [F11 §3.7] |
| A1-25 | major | not F | [F04 §10] (R-SPEC-P) |
| S1-49 | minor | not F | [F17 §4.2] (R-SPEC-P) |
| A1-23 | major | not F | [F05 §9], [F11] |
| A1-26 | major | not F | [F13], [F17] owner-list items (R-SPEC-P) |
| A1-27 | major | not F | seeded bugs of [F03] and `os/` (R-SPEC-P) |
| A1-28 | major | not F | [F15 §6.5] (R-SPEC-P) |
| A1-34 | major | not F | [F20 §5.11.4] (R-SPEC-R) |
| A1-35 | major | not F | [RULES/README] registry (R-MODEL) |

## Round 1 of closing pass 1

Round 1 re-checked every finding whose fix lands in R-SPEC-F's files, every open item [pass1-closure] lists against them
(§3, §4, its §5 citations and its §7 rows), and the notes R-SPEC-R and R-SPEC-P left for R-SPEC-F in their round-1
dispositions (§4 of each). Each section named below was re-read after those roles' round-1 edits.

- **fixed**: text changed in this round (R-SPEC-F's files, or a cross-role edit marked **x** and listed below).
- **verified**: the finding's remaining part was another role's; its edit was re-read, agrees with R-SPEC-F's text, and
  needed no change here.
- **open**: not closable in R-SPEC-F's files; the owing role or the owner question is named.

**Round 1 summary.** 96 dispositions: 56 fixed, 38 verified, 0 rejected, 2 left open (S1-22's rule row, R-MODEL; A1-31,
the owner's confirmation); P1-21 is fixed in its F part, and its rule rows stay open with R-MODEL. The declined alternatives of round 0 stand: the full git-ref spelling of P1-2 and A1-8 (now
also against the design's own text, [40 §5.3] item 2 and [40 §5.4] item 1) and A1-3's bit order. Owner questions OQ-F-1
(A1-31) and OQ-F-2 (A1-38) are in [owner-questions]. `COVERAGE.md` has no CONFLICTING and no UNMAPPED row (152 of 152
mapped); a scan of every `HOLE(` outside `reviews/` finds exactly the 53 ids of `HOLES.md` §2.

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| P1-1 | blocker | 1 | fixed | Closure residue (fixtures) and a second 16-byte `commitref` form. [F08 §5.1]: the promoted `commitref` element of [F09 §10.1] is an `id16` index key; the 32-byte value stays in the field block, which a reader reads and confirms a match against (the index column S1-1 allows); **x** [F09 §10.1]'s row says so. [F08 §5.6] (new): one value of every type with its stored bytes and its [F07 §7.1] `cv`, the values of row 60-AR-Values' fixture. `COVERAGE.md` 60-AR-Values and R-4 lose CONFLICTING. The fixture files and R-4's per-kind anchor fixtures (they need computed uid, `captured`, window and span-hash bytes) are R-FIX's (WP-20), as `COVERAGE.md` §1 assigns the fixture column; the rows state what each must hold |
| S1-1 | blocker | 1 | fixed | As P1-1 (the promoted column was the last stored `commitref` narrower than [F08 §5.1]) |
| A1-1 | blocker | 1 | fixed | As P1-1 |
| A1-3 | blocker | 1 | fixed | [F09 §7.2] (R-SPEC-R) re-read: `pflags` and the 32-byte pin as [F08 §10.2]; `COVERAGE.md` R-4 lifted |
| S1-2 | blocker | 1 | fixed | As A1-3 |
| A1-6 | blocker | 1 | fixed | [F05 §9]/[F11] alignment (R-SPEC-R, R-SPEC-P) re-read; `COVERAGE.md` R-7 lifted, and 60-AR-Seg-runtime, 60-AU-Log-TreeReg, 60-AU-Seg-leases and 90-LEASES cite [F11 §2.9], [F11 §3.9] and [F05 §9.4] `lflags`; [API] open point 34 closed ([F11 §7] `outcome`) |
| S1-8 | blocker | 1 | fixed | As A1-6 |
| P1-2 | blocker | 1 | fixed | As A1-6. R-15's fixture: [F18 §3.2]'s example now places the 40 bytes in the 161-byte `HEADS` row (offsets 105–144) and in its `ClientHead` image; `COVERAGE.md` R-15 names the fixture for R-FIX and is lifted |
| A1-7 | blocker | 1 | fixed | [F18 §4.10]: the example no longer says `FILEOBS.state` 5 is stored; which codes a row records is [F11 §12.5]'s (1–4, 6, 8, 12; 5, 9–11 computed at render). R-SPEC-R's `PENDING` sentence kept. Open point 29 closed; `COVERAGE.md` R-18 lifted |
| S1-9 | blocker | 1 | fixed | As A1-7 |
| A1-8 | blocker | 1 | fixed | [F18 §3.7]: `HEADS` carries `BindingExt` at offset 105 and stores `root_id`, so open point 5 closes; `ClientHead` carries the row image ([F05 §9.3], [F11 §2.9]); open point 32. The full-name alternative stays declined: [40 §5.3] item 2 and [40 §5.4] item 1 write the short form |
| S1-10 | blocker | 1 | fixed | As A1-8 |
| A1-9 | blocker | 1 | verified | [F11 §10] now stores `prov` at offset 5; [F12] open point 4's statement is true |
| S1-5 | blocker | 1 | verified | As A1-9 |
| S1-7 | blocker | 1 | fixed | [F11 §10] (R-SPEC-R) holds `ckey`, the `kval` sides and `prov` of [F06 §6.2]; [F06 §7.7]'s citation resolves; [F12] open point 18 closed (`n` = 0 for schema keys); `COVERAGE.md` F11 |
| A1-10 | blocker | 1 | fixed | [F06 §9] BK-5 cites [F09 §16.4] `CKIMG`; `COVERAGE.md` 60-AU-Seg-cs |
| S1-23 | major | 1 | fixed | As A1-10 |
| S1-6 | blocker | 1 | fixed | [F06 §9] BK-5 cites [F09 §16.4]'s absent-to-deleted row; `COVERAGE.md` 60-AU-Seg-cs |
| A1-4 | blocker | 1 | fixed | As S1-6 |
| A1-11 | blocker | 1 | fixed | [F05 §9.2] reason 5 exists (R-SPEC-P); [F12 §8.2]'s citation resolves; [F12] open point 14 closed; `COVERAGE.md` 60-AR-Log-kinds (reasons 1–5, kinds 27 and 28) |
| S1-11 | blocker | 1 | fixed | As A1-11 |
| P1-3 | blocker | 1 | fixed | As A1-11; the `README.md` row of [F05] names kinds 27 and 28 |
| A1-12 | blocker | 1 | fixed | As P1-3 |
| A1-13 | blocker | 1 | fixed | [F17 §2.1] = [F04 §4.4] (R-SPEC-P); `COVERAGE.md` 60-AR-HEAD-params lifted, citing [F04 §5.16] and [F05 §9.28] |
| S1-12 | blocker | 1 | fixed | As A1-13 |
| P1-4 | blocker | 1 | fixed | As A1-13; R-SPEC-P's **x** edit of [CFG §7.6] (`project_oid_algo` from `extensions.objectFormat`) re-read and kept |
| A1-15 | blocker | 1 | fixed | As P1-4 |
| S1-28 | major | 1 | fixed | As P1-4 |
| A1-14 | blocker | 1 | fixed (interim rule) | [F08 §10.3.1] adds what [F20 §6.1] now states: no `heading` (kind 2) or `symbol` (kind 3) anchor is captured while the interim rule holds, since their header line, quote, hint and span hash are scanner output; imported ones keep their bytes; the fallback cites OQ-R-2. [F19 §10.2] `anchor_spec` gains the refusal (case `no-scanner`). [F08] open points 39 and 48. The scanner appendix itself stays R-SPEC-R's (WP-63, OQ-R-2) |
| S1-4 | blocker | 1 | fixed (interim rule) | As A1-14 |
| P1-20 | major | 1 | fixed (interim rule) | As A1-14 |
| A1-17 | blocker | 1 | fixed | [F16] P-36 and [OS/clock §7] (R-SPEC-P) now state CK-4 over `hlc_seq` and `hlc_commit`. [F06 §4.4.4] adds `Reserve` to the records that never advance the sequence and names the maxima; [F06] open point 32 closed. [API §6.2] CK-4 adds `Reserve` and cites P-36 as agreeing; [API] open points 39 (closed) and 46. `COVERAGE.md` 60-I2-PD(e), F14 |
| S1-13 | blocker | 1 | fixed | As A1-17 |
| P1-5 | blocker | 1 | fixed | As A1-17 |
| A1-18 | major | 1 | fixed | [OS/fs §4.9.4] (R-SPEC-P) agrees; [F02] open point 17 closed; `COVERAGE.md` 60-AU-Vfs-renames |
| P1-13 | major | 1 | fixed | As A1-18 |
| S1-26 | major | 1 | fixed | As A1-18 |
| A1-22 | major | 1 | verified | [F10 §8] (R-SPEC-R): rows, `VIOLATIONS`, `CKIMG`; no `OPS` |
| S1-19 | major | 1 | verified | [F10 §8] `b3` = `seg_digest[0..16]`, as [F06 §9] BK-2 |
| A1-21 | major | 1 | verified | As S1-19 |
| P1-6 | blocker | 1 | verified | As S1-19 |
| S1-20 | major | 1 | verified | [F16] P-34 "by node" and [F09 §16.4] `PREV` agree with [F06 §7.3] and BK-5 |
| A1-23 | major | 1 | fixed | `COVERAGE.md` 60-AU-Log-kinds cites [F11 §12.13] and [F11 §13] |
| A1-40 | major | 1 | verified | [OS/env §5] uses `tmp/probe.<nonce>`, which [F02 §5.3] and §6.3 admit |
| P1-32 | minor | 1 | verified | As A1-40 |
| S1-38 | minor | 1 | verified | As A1-40 |
| P1-7 | major | 1 | fixed | R-SPEC-P's **x** edit of [F02 §5.3] and §6.3 (the `tmp/` word `extent`) re-read and kept; `COVERAGE.md` X-F3 cites [F05 §4.5], [F05 §9.28], [F16] P-96, P-97 |
| P1-8 | major | 1 | fixed | R-SPEC-P's **x** edit of [API §6.2] CK-6 (the extent head carries h) re-read and kept; [API] open point 46; `COVERAGE.md` X-F3 |
| P1-44 | minor | 1 | verified | As P1-8 |
| P1-10 | major | 1 | fixed | R-SPEC-P's **x** edit of [F19 §10.2] `store_locked` (`quiet`) kept; its case column and §10.5 now name the quiet bytes' retries; `COVERAGE.md` 60-AR-LOCK cites [F03 §3.1] |
| P1-11 | major | 1 | fixed | [CFG §10.5]: a `wmem` raise never admits a larger changeset; an agent `TX` is bounded by P05 ([F17 §4.4] W1, W4; OQ-P-2); [CFG] open point 33. [LQ/errors §5.4] E501 gains the inline-bound case, which names the split; [LQ/errors] open point 16; [F19] open point 16 closed |
| P1-15 | major | 1 | fixed | [F20 §4.9] and [OS/project §2.3] agree with [F18 §4.6] detail 44; `COVERAGE.md` R-14 and R-16 cite [F20 §4.9] |
| P1-16 | major | 1 | fixed | [OS/project §6.2] and [F16] P-71 agree with [API §12.4] step 1; [F19 §10.2] `no_dir_flush` also covers `doctor`'s report of an intent that recovery left open |
| P1-18 | major | 1 | fixed | `COVERAGE.md` X-F5 and 60-PA-(h) cite [F16] P-100 |
| P1-19 | major | 1 | fixed | [F17 §4.4] W1 lists `sync` (R-SPEC-P); [F07 §10.6] says GT11's 14-days-behind case joins [60 §3.13] at WP-81a |
| P1-21 | major | 1 | fixed (F part); open (R-MODEL) | [LQ/errors §5.5] E405 gains `at most one <edge> (cardinality)`, `named queries bind (QueryInvalid)` and `schema conformance (I11)`, and E409 the root-node case; [F19 §10.5], §12.4 and §12.5.6 cite them. [F13 §5] (R-SPEC-P) is aligned. Open: [RULES/merge-table] MR-005, VA-005, VA-007, VA-008, VA-013 and [RULES/link-merge-rules] LM-013 still read `gap` (R-MODEL; [F12 §5.4], [F19 §12.2] and [F08 §11.3] are the single owners) |
| S1-22 | major | 1 | open (R-MODEL) | No F change: [F12 §6.5] and [F14 §6.8.1] agree; [RULES/merge-table] RS-008 and its open point 5 (c) still defer (R-MODEL) |
| S1-25 | major | 1 | verified | R-SPEC-P's **x** edit of [F19 §10.2] `store_io_fault` and §10.5 re-read and kept |
| S1-29 | major | 1 | fixed | `COVERAGE.md` 60-AU-Seg-hist: [F10 §4.2] is the one frame rule, [F17 §4.3] cites it |
| S1-31 | major | 1 | fixed | `HOLES.md` `F20-btime-ntfs`: the constraint, the candidate `Absent` and measurement 15's added copy paths, as [F20]'s Holes row |
| A1-34 | major | 1 | fixed | `COVERAGE.md` R-14 cites [F20 §5.11.4] (normative since round 1, R-SPEC-R) |
| A1-31 | major | 1 | open (owner) | Text fixed in round 0; the owner's confirmation of the 8,000 B cut page against [AR §7.1] is OQ-F-1; [F19] open point 6 cites it |
| A1-38 | major | 1 | fixed | The change of [50 §4.1]'s text awaits the owner: OQ-F-2; [API] open point 11 cites it |
| P1-31 | minor | 1 | fixed | [F19 §10.2] `maintenance_busy`: the maintenance byte is only tried ([F16] P-1, P-76; [OS/lock §6]), so no "bounded wait", no `(waited <t> ms)` and no `waited_ms` key; §10.5 cites P-76 and [OS/lock §6] |
| P1-37 | minor | 1 | fixed | [F19 §10.2] new code `no_canonical_path` (exit 7, [OS/path §4.1] step 3) with its JSON keys, and a `bad_path` case for drive-relative and device-form arguments (exit 2, [OS/path §7] step 3; `rule` `drive-relative` or `device`); §7.3 and §10.5 list both; [F19] open point 35 |
| S1-40 | minor | 1 | fixed | [F01] open point 15 no longer writes the old ids in `HOLE(...)` form, so a scan of every `HOLE(` outside `reviews/` finds exactly the 53 ids of `HOLES.md` §2 |
| P1-33 | minor | 1 | fixed | As S1-40 |
| A1-42 | minor | 1 | fixed | As S1-40 |
| A1-44 | minor | 1 | fixed | [F13 §3.9] (R-SPEC-P) takes [F18 §2] as the statements of record; [F18 §2.15] and open point 21 say so |
| S1-41 | minor | 1 | verified | R-SPEC-P's **x** edit of [F02 §3.3] rule 6 (P-99) re-read and kept |
| S1-37 | minor | 1 | fixed | `README.md`: the rows of [F04], [F05], [F11], [F16] and [F20] name the pass-1 additions (the `HEAD` maxima and `project_oid_algo`; kinds 27 and 28; the session and backup tables and row images; P-1–P-100; the git pair score and the interim scanner rule) |
| A1-51 | minor | 1 | verified (F part) | [F19 §11.3] and [LQ/errors §5.5] unchanged; WZ-001's note is R-MODEL's |
| A1-57 | minor | 1 | verified (F part) | [F19 §3.2] unchanged; PE-006's 10,000 B is R-MODEL's |
| P1-23, S1-36, S1-34, P1-29, P1-26, S1-44, S1-32, A1-43, A1-59, A1-48, A1-52, A1-53, A1-54, A1-55, A1-56, A1-58, A1-62, P1-39, P1-41, P1-42, S1-39, A1-50, A1-61 | minor | 1 | verified | Round-0 fixes re-read in [F01 §5.2], [F01 §8.3], [F08 §7.2], [F12 §5.5a], §7.5, [F06] open point 16, [F19 §7.1], §12.1, §10.2, [F07 §2.2], [LQ/envelope §5.13], §5.16, §8.1, [LQ/errors §2.1], §5.4, §5.5, §5.7, [LQ/std §1], §2.4, §2.10, [LQ/canonical-ast §5.4], [LQ/lexical §5.6], §8, [LQ/json-ir §2], [LQ/card §7.3], [CFG §4.1], §4.2, §10.5 and `COVERAGE.md` §1: all present and unaffected by the other roles' round-1 edits (23 dispositions) |

### Cross-role edit of this round (after a fresh read of the section)

- **[F09 §10.1] (R-SPEC-R):** the promoted-encoding row of `commitref` says the `id16` is an index key, and that the value
  is read from, and a match confirmed against, the field block ([F08 §5.1]; P1-1). Please review.

### Other R-SPEC-F edits of this round with no finding of their own

- [F19 §10.1] defines the placeholder `<spec>`, which `ambiguous_path` already used and `anchor_spec`'s new case uses.
- [F06 §9] BK-5, [F07 §10.6], [F12] open points 14 and 18, [F18 §3.7], [API] open points 34, 39 and 46, [CFG] open point
  33, [F02] open point 17: citations and statuses brought up to date with the other roles' round-1 text.

### Notes for other roles

- **R-MODEL:** P1-21 (MR-005 as [F12 §5.4]'s rule; VA-005, VA-007, VA-008 and VA-013 with [F19 §12.2]'s classes 67, 68
  and 72; LM-013 by [F08 §11.3]); S1-22 (RS-008 and open point 5 (c): [F12 §6.5]'s restore rule); A1-51 (WZ-001's note:
  the split rendering of [F19 §11.3]); A1-57 (PE-006: the `generic` hook ceiling is 8,000 B by [90 §6.4]).
- **R-SPEC-R:** review the [F09 §10.1] edit above; the per-kind anchor fixtures of row R-4 need [F20]'s window and span
  hash as R-FIX's inputs.
- **R-FIX (WP-20):** rows 60-AR-Values ([F08 §5.6]), R-4 and R-15 ([F18 §3.2]) name the cross-chapter byte fixtures
  pass 1 requires.

## Round 2 of closing pass 1

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1], at every severity, whose fix lands in [F01], [F02], [F06],
[F07], [F08], [F12], [F18], [F19], [API] with its examples, [CFG], `lq/*`, `COVERAGE.md`, `HOLES.md` or `README.md`; every
item [pass1-closure] (round 1) lists against these files: contradictions NC-2, NC-5, NC-6 and NC-7 (§4), the four
`COVERAGE.md` rows of its §5, the editorial residue of [F18 §4.7]; and the notes the other roles left for R-SPEC-F
(`pass1-dispositions-P.md` §4 and §7.4, `-R.md` §4 and §6.5, `-M.md` "Notes for other roles"). Every section named below
was re-read in its current text, after the round-1 and round-2 edits of the other roles. As the closure's open point 3
asks, each datum a pass-1 fix changed in these files (the value registry and its fixture, the `pathmove` prefixes, the
JSON commit-id form, the HLC record list, the interim scanner rule, the reservation record, the `park` reason, the
`VolumeCaps` bit, the read-error rule) was searched in every file of R-SPEC-F for a restatement that still carried the old
value; the hits are the residues fixed below.

- **fixed**: text changed in this round.
- **verified**: re-read; the text agrees with the fix and with every chapter that restates it; no change.
- **not F**: no text of R-SPEC-F carries any part of the finding (listed for completeness; the owning role disposes of it).

**Round 2 summary.** 155 findings: 21 fixed (residues of findings closed in round 1: 15 blockers, 5 majors, 1 minor),
102 verified with no change (25 blockers, 37 majors, 40 minors), 32 not F, 0 rejected, 0 left open in the text. Two
verified findings keep an owner confirmation: A1-31 (OQ-F-1) and A1-38 (OQ-F-2). Closure contradictions closed by this
role: NC-2, NC-5, NC-7, and NC-6 together with R-SPEC-P's cross edit (reviewed and kept). No record kind, offset, section
tag, enumeration value or hole changed; one error code was added ([F19 §10.2] `store_retired`), and `store_corrupt` gained
a case. No new owner question.

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-2 (closure) | major | 2 | fixed | [API] example `13-file-mv.json`, step n = 8: `LinkFile` takes `docs/api.md:3` and captures a `quote` anchor (watch `span`, hint [3, 3], quote `## Commands`, prefix `# API\n\n`, suffix `\n\nTx, Mutation and Apply.`, derived by [F20 §2.5] and §6.1 from the file text at the draft `CONTEXT`); the `yields` row says `quote`; the `illustrative` list says which anchor values are derived and why the `path#H` form is not used. All 20 examples still parse |
| NC-5 (closure) | major | 2 | fixed | [F08 §5.6] row 13: `project:a/` → `project:b/`, stored `03 00 00 6C 50 C4 A0 01 01 01 00 02 61 2F 01 00 02 62 2F 00`, `cv` as the closure gives it, both re-derived from [F08 §5.2] and [F07 §7.1]; every other row of §5.6 re-derived and unchanged. The same datum in example 13: `path_move.from`/`to` are `project:docs/` and `project:handbook/` ([F08 §5.4.2], [API §5.2]). [F08] open point 49 |
| NC-6 (closure) | major | 2 | fixed | R-SPEC-P's cross edit of [F06 §2.4] (a V-rule break is a malformed payload, corrupt wherever it lies, [F05 §5.4]) re-read and kept; tagged "pass 1". [F06 §4.1]'s decoder rule now points at §2.4's consequence. [F19 §10.2] `store_corrupt` names the malformed payload of a valid log record with its own `<what>` text and `moirai doctor --fsck` (the invalid group keeps `moirai repair`), and §10.5 lists the refusal ([F05 §5.4], [F06 §2.4]); before, [F19] had no row for the case. [F06] open point 34, [F19] open point 36 |
| NC-7 (closure) | major | 2 | fixed | [F06 §4.2] bit 17: set when the kind is `checkpoint`, the commit is inline and its checkpoint tree holds a node file that differs from its parent checkpoint's tree (C); clear for an inline checkpoint that differs in no node file; a bulk checkpoint carries `CKIMG` under the same condition ([F09 §16.4], which already said so). [F06 §4.4.14]'s `n_files` ≥ 1 now always has an encoding |
| closure §5 | — | 2 | fixed | `COVERAGE.md` rows 60-AU-Vfs-projfs and X-F8 (NC-1 closed by R-SPEC-R: [F11 §12.3] bit 14 as [OS/project §4.2]; both rows also cite [OS/fs §6.2]'s `ProjectFs::sync_dir` reactions), 60-I2-FM(12) (NC-3 closed by R-SPEC-P: [OS/fs §6.2] states [F16] P-92; the row cites P-92, [F05 §5.3]'s writer rule and [F19 §10.2] `store_io_fault`), 60-AR-Values (NC-5): none needs CONFLICTING; §2's note records the check |
| closure §4, editorial | — | 2 | fixed | [F18 §4.7]: a sentence after the link-line example says that `a31` is a `symbol` anchor, which a store holds only by import while the interim scanner rule holds, as [F14 §17.2] says of its own example |
| P1-1 | blocker | 2 | fixed (residue) | NC-5 (the fixture values of row 60-AR-Values). `COVERAGE.md` R-4: R-FIX's per-kind anchor fixtures take [F20 §2.7.3]'s window, [F20 §2.8]'s span hash and [F20 §6.1]'s capture with no scope (R-SPEC-R's note) |
| S1-1 | blocker | 2 | fixed (residue) | As P1-1 |
| A1-1 | blocker | 2 | fixed (residue) | As P1-1 |
| A1-10 | blocker | 2 | fixed (residue) | NC-7 |
| S1-23 | major | 2 | fixed (residue) | NC-7 |
| A1-14 | blocker | 2 | fixed (residue) | NC-2 and the [F18 §4.7] residue: no text of R-SPEC-F captures, or shows a capture of, a `symbol` or `heading` anchor while the interim rule holds (searched: `path#`, `.md#`, `::`) |
| S1-4 | blocker | 2 | fixed (residue) | As A1-14 |
| P1-20 | major | 2 | fixed (residue) | As A1-14 |
| S1-17 | blocker | 2 | fixed (residue) | [F19] open point 24 still said "JSON commits 64 hex digits", and [API] open point 1 still said that [F19 §8.3] omits the `c`; both now state `c` + 64 lower-case hex, as [F19 §8.3] does since round 0; [API] open point 1 closed |
| A1-16 | blocker | 2 | fixed (residue) | As S1-17 |
| P1-5 | blocker | 2 | fixed (residue) | [F06 §4.4.4] listed `Checkpoint`, `Reserve`, `Lazy` and runtime records as never advancing the HLC sequence; it now also names `SessionMark`, the list of [API §6.2] CK-4 and [F16] P-36 |
| S1-13 | blocker | 2 | fixed (residue) | As P1-5 |
| A1-17 | blocker | 2 | fixed (residue) | As P1-5 |
| P1-3 | blocker | 2 | fixed (residue) | The reservation record reaches [API]: a bulk-class command interrupted by `EnvCrash` `in-next`, or refused after its `Reserve` group landed, leaves `next_id`, `next_anchor` and the other counters advanced and its ids skipped ([F05 §9.27], [F11 §9.1]); [API §6.7] and §16.4 give the model that third candidate, §9.10 states that a bulk commit takes the ids an inline one would, in §9.6's order, and §15.7's `alloc` lists no skipped id. Without it the model's next `#N` would differ from the engine's after such a command (DT-4). §15.7's `moves` admits reason 5 `park` ([F05 §9.2], [F12 §8.2]). `COVERAGE.md` 60-AR-Log-kinds cites [F09 §14.4]'s `reserved` flag; 60-AU-Seg-alloc and F17 cite [F05 §9.27] and [F11 §9.1] (R-SPEC-R's notes). [API] open point 47 |
| S1-11 | blocker | 2 | fixed (residue) | As P1-3 |
| A1-12 | blocker | 2 | fixed (residue) | As P1-3 |
| S1-14 | blocker | 2 | fixed (residue) | [F12 §7.6] cited RK-001 to RK-010 for the procedure that RK-011 now completes; it cites RK-001 to RK-011. [F08] open point 35 (re-key scope) closed by [F12 §7.6] and RK-006, as R-MODEL noted. OQ-M-1 (a) stays with the owner |
| P1-21 | major | 2 | fixed (residue) | R-MODEL's round-1 rows reached [F12] and [F19] only as requests: [F12 §5.4]'s plain-base paragraph now cites MR-005 with RS-015 as done (re-signature OQ-M-1); [F12] open point 3 marked done; [F19] open point 14 records §9's sentence and VA-005, VA-007, VA-008, VA-013 as done |
| P1-16 | major | 2 | fixed (residue) | `COVERAGE.md` rows 60-AU-Vfs-projfs and X-F8 (closure §5 above); [API §12.4] step 1 re-read against [OS/project §6.2], [OS/fs §6.2] and [F16] P-71: agrees |
| S1-25 | major | 2 | fixed (residue) | `COVERAGE.md` row 60-I2-FM(12) (closure §5 above); [F19 §10.2] `store_io_fault` and §10.5 re-read against [F16] P-92, [F05 §5.3] and [OS/fs §6.2]: agree |
| A1-57 | minor | 2 | fixed (residue) | `HOLES.md` row `CFG-codex-mcp-result` cites [RULES/pack-classes] PE-004, whose `default_bytes` is that hole (R-MODEL's note) |
| S1-2, A1-3, S1-3, A1-2, S1-5, A1-9, S1-6, A1-4, A1-5, P1-6, S1-9, A1-7, S1-10, A1-8, P1-2, S1-8, A1-6, P1-4, S1-12, A1-13, A1-15, S1-7, S1-15, S1-16, A1-11 | blocker | 2 | verified | Re-read after the other roles' round-2 edits: [F08 §5.1] (with R-SPEC-R's [F09 §10.2] uniqueness sentence, which agrees), §10.2, §10.3, §10.3.1; [F06 §6.2], §7.4, §7.5, §7.7, §4.4.15, §9 BK-2, BK-5; [F07 §7.1], §7.3, §8; [F12 §5.4], §6.3, §6.5, §7.8, §8.2; [F18 §3.2], §3.7, §4.2, §4.10; [API §10.8], §11.4, §15.7 `markers`; [CFG §7.6]; `COVERAGE.md` 60-AR-HEAD-params, R-7, R-15, R-18 |
| P1-7, P1-8, P1-10, P1-11, P1-12, P1-13, P1-14, P1-15, P1-18, P1-19, S1-19, S1-20, S1-21, S1-22, S1-26, S1-27, S1-28, S1-29, S1-30, S1-31, A1-18, A1-19, A1-20, A1-21, A1-22, A1-23, A1-29, A1-30, A1-31, A1-32, A1-34, A1-36, A1-37, A1-38, A1-39, A1-40, A1-41 | major | 2 | verified | [F02 §3.2], §5.3, §6.3, open point 17; [F06 §4.6] (the usable length is E minus [F05]'s rotation reserve, as S1-18 left it), §7.3, §7.6, §9 BK-2; [F07 §7.3], §10.6; [F08 §3.4] (R-MODEL's S1-30 text), §8.3, §8.5; [F12 §6.5], §7.3; [F18 §4.6]; [F19 §4.6], §10.2 (`store_locked` with `quiet`, `maintenance_busy`), §10.5; [API §6.2] CK-6, §6.4, §8.1, examples 01 and 02; [CFG §7.6], §10.5 (the `wmem` sentence already says a raise never admits a larger changeset, `pass1-dispositions-P.md` §4 item 5); `lq/envelope` §3.2, §3.3, §6.5, §9.3; `lq/errors` §3.3, §3.4, §5.4; `lq/std` §2.4, §4.1, §7.3; `HOLES.md` `F20-btime-ntfs`; `COVERAGE.md` X-F3, X-F5, 60-PA-(h), R-14, 60-AU-Seg-hist. A1-31 and A1-38 keep their owner questions (OQ-F-1, OQ-F-2) |
| P1-23, P1-26, P1-29, P1-31, P1-32, P1-33, P1-37, P1-39, P1-41, P1-42, P1-44, S1-32, S1-33, S1-34, S1-36, S1-37, S1-38, S1-39, S1-40, S1-41, S1-44, S1-46, S1-48, A1-42, A1-43, A1-44, A1-45, A1-46, A1-48, A1-50, A1-51, A1-52, A1-53, A1-54, A1-55, A1-56, A1-58, A1-59, A1-61, A1-62 | minor | 2 | verified | Checked by search in their current text: [F01 §5.2] (tenth byte `01`), §8.3; [F06] open point 16, §6.1 (`ckey` 1–9); [F07 §2.2] and [F19 §12.1] (`DATA` has no code); [F08 §7.2], §8.4.6 and the paragraph after §9.6 (R-MODEL's S1-46 text); [F12 §5.5a], §7.5, §7.6 step 2 (R-MODEL's S1-48 text); [F02 §3.3] rule 6, §5.3, §6.3; [F18 §2.2], §2.15, detail 59; [F19 §3.2], §7.1, §10.2 (`no_canonical_path`, `bad_path` cases, `cross_volume` help), §10.6, §12.2; [CFG §4.1], §4.2, §6.2, §10.5 (`input.max-bytes`); [LQ/lexical §5.6], §8; [LQ/json-ir §2]; [LQ/card §7.3]; [LQ/envelope §5.13], §5.16, §8.1; [LQ/errors §2.1], §4.2, §5.4, §5.5, §5.7; [LQ/std §1], §2.10; [LQ/canonical-ast §5.4]; [API §6.2] CK-6; `README.md`; `HOLES.md` §1–§4; `COVERAGE.md` §1. Every `HOLE(` outside `reviews/` is indexed in `HOLES.md` §2 (no hole changed this round) |
| S1-18, S1-24, A1-24, A1-25, A1-26, A1-27, A1-28, A1-33, A1-35, P1-9, P1-17, P1-22 | major | 2 | not F | [F03]–[F05], [F13], [F15]–[F17], `os/*`, [F09]–[F11], [F14], `rules/*`; R-SPEC-P, R-SPEC-R and R-MODEL dispose of them (their round-2 tables report them closed) |
| P1-24, P1-25, P1-27, P1-28, P1-30, P1-34, P1-35, P1-36, P1-38, P1-40, P1-43, S1-35, S1-42, S1-43, S1-45, S1-47, S1-49, A1-47, A1-49, A1-60 | minor | 2 | not F | As above |

Rejected: none. Left open in the text: none. With the owner: OQ-F-1 (A1-31) and OQ-F-2 (A1-38), unchanged.

### Review of the cross-role edit of this round

- **[F06 §2.4] (R-SPEC-P, closure NC-6):** kept; the parenthesis now reads "pass 1, closure NC-6". [F06 §4.1] and [F19 §10.2]
  `store_corrupt` follow it (above).

### Other R-SPEC-F edits of this round with no finding of their own

- **A store left `retired`** ([F16] open point 10, `pass1-dispositions-P.md` §7.4 item 4): [F02 §3.6] states what a
  process does when discovery yields the same store again with `retired` set and no swap intent (it repeats the probe with
  [F16] P-86's delays, since a running `restore` sets the flag before it creates its intent, then exits 7 instead of
  looping), and [F19 §10.2] adds the code `store_retired` (exit 7, JSON `path`, §7.3's exit-7 list, §10.5). [F02] open
  point 21, [F19] open point 36.
- [F12 §5.3] VM-2's row cites VB-019; [F12] open points 7, 8 and 23 record R-MODEL's round-1 rows (open point 9 of
  [RULES/merge-table], VB-019, PR-014) as done (`pass1-dispositions-M.md` "Notes for other roles").
- Not taken: adding [F04] open point 4 (P14 ≤ 5) to row 60-AR-HEAD-params (`pass1-dispositions-P.md` §7.4 item 2). P14 is
  the hot tunable `store.fold-width` ([F17 §3]), not an `init`-fixed parameter, so it is not part of that row's item.

### Notes for other roles

- **R-SPEC-P:** [F16] open point 10 can cite [F19 §10.2] `store_retired` and [F02 §3.6] as the text it asked for. [F02 §3.6]
  uses [F16] P-86's probe delays for the retired-with-no-intent case; if P-86 should own that case as a rule (with a
  seeded bug such as "a process loops on a store left `retired`"), P-86 may add it and [F02 §3.6] will cite it.
- **R-SPEC-R:** none. [F18 §4.7] now says of its `symbol` anchor what [F14 §17.2] says of its own.
- **R-MODEL:** none.
- **Closure checker (round 2):** the sections to re-read are [F06 §2.4], §4.1, §4.2; [F08 §5.6]; [F18 §4.7]; [F19 §10.2]
  (`store_corrupt`, `store_retired`), §10.5; [F02 §3.6]; [API §6.7], §9.10, §15.7, §16.4 and example `13-file-mv.json`;
  `COVERAGE.md` rows 60-AU-Vfs-projfs, X-F8, 60-I2-FM(12), 60-AR-Values, R-4, 60-AR-Log-kinds, 60-AU-Seg-alloc, F17.
## Round 3 of closing pass 1

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1], at every severity, whose fix lands in [F01], [F02], [F06],
[F07], [F08], [F12], [F18], [F19], [API] with its examples, [CFG], `lq/*`, `COVERAGE.md`, `HOLES.md` or `README.md`; the
items [pass1-closure] (round 2) lists against these files: NC-8, NC-9 (§4.2) and two of its three editorial residues (the
[API §6.7] counters, the "recovery" wording of the parker); and the notes the other roles left for R-SPEC-F
(`pass1-dispositions-M.md` round 2 "Notes for other roles" items (1)–(3), `-P.md` §8.4, `-R.md` §7.5). As the closure's
open point 3 asks, each rule row a round-3 edit here cites was re-read for the exact case in the same round:
[RULES/delete-policy-matrix] DP-001 to DP-009, open points 3 and 9, §9 (`Undelete`); [RULES/merge-table] DM-004, DM-005,
DM-017 and open point 33; [RULES/pack-classes] PX-011, PT-028, PM-025; and the chapters the new text cites:
[F11 §6] (the `LEASES` order), [F11 §7] and [F05 §9.5] field 9 (R-SPEC-R's and R-SPEC-P's round-3 `actor`/`holder`
text), [F05 §9.11] and [F11 §13.1] (NC-10, round 3), [F05 §10.2] (the `Reserve` fold), [F16] P-70, [F11 §3.2],
[F07 §6.4], §7.2, §13, [F14 §6.10], [LQ/envelope §7.4], [F19 §2.4], [AR §5d.1], §5d.3, §7.1, §7.4, §7.7.1, [40] I-F5.
R-MODEL's round-2 cross edits of [F08 §4] (GR-002 and WT-004) and [F08 §11.2] (RK-001–RK-011 and [F12 §7.6]), made after
R-SPEC-F's round 2, were reviewed: both rows exist and agree; kept.

- **fixed**: text changed in this round (R-SPEC-F's files, or a cross-role alignment edit marked **x**).
- **verified**: re-read against the current text, the other roles' round-3 edits included; no change.
- **not F**: no text of R-SPEC-F carries any part of the finding.

**Round 3 summary.** Closure items: NC-8 and NC-9 fixed, and the two editorial residues owned here fixed. Findings (155):
11 fixed (residues: 8 blockers, 3 majors), 113 verified with no change (32 blockers, 40 majors, 41 minors), 31 not F,
0 rejected, 0 left open in the text. One owner question added, OQ-F-3 (who appends the C8 pack cursor, a contradiction
between [40] I-F5 and [RULES/pack-classes] PX-011 that NC-10's bytes exposed); OQ-F-1 and OQ-F-2 unchanged. No record
kind, offset, section tag, enumeration value or hole changed. Hashed bytes now fixed where they were unstated: the inverse
`Delete` of a reverted `Create` (`reason` 0, `replaced_by` 0). New error texts: E409's lease and replacement cases and the
`leases` JSON key.

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-8 (closure §4.2) | major | 3 | fixed | [LQ/errors §5.5] E409 gains the live-lease case: `statement <i>: DELETE <id> refused: leased by <holder> on <ref> (<lease>)` ([AR §5d.3]'s text after the statement prefix), naming the first live task lease on the deleted set in [F11 §6]'s `LEASES` order, prefixed by the leased node's id when it is not the target; the other leases as detail lines (at most 10); `nothing was written`; help `add RELEASE to release the lease; a triage note goes to the tombstone (moirai rm --release)`. The same row gains DP-007's invalid-replacement case (`replacement <id> is not live` / `is in the deleted set` / `does not fit <id> -[:<edge>]-> <id>`), which [API §9.1] also mapped to E409 with no text, and fixes the order of the reference lists. A paragraph after the table gives the case order ([RULES/delete-policy-matrix §4]: DP-005, then DP-006, then DP-007). §5.7: `leases` key for the lease case (members as [LQ/envelope §7.4]'s `lease` object); §2.2 defines `<lease>` and `<holder>`, which E407 already used. [F19 §10.5] lists both refusals; [API §9.1]'s E409 row names all three delete cases; [LQ/errors] open point 17, [F19] open point 37. **x** [RULES/delete-policy-matrix] DP-005 cites the text, open point 9 records it |
| NC-9 (closure §4.2) | major | 3 | fixed | [F06 §7.10]: the inverse `Delete` of a `Create` has `reason` 0 (empty) and `replaced_by` 0 (none), so a reverted creation leaves `deleted(kind, "", none)` ([F07 §7.2], §13), written with no `field reason:` line ([F14 §6.10]); reason: no revert takes a reason or a replacement ([AR §7.1]) and canonical item 8 already names the reverted commit. The same bullet now states that a `Delete`'s inverse is an `Undelete` (and back) with the other's `reason`, `replaced_by` and image ([AR §5d.3], [RULES/delete-policy-matrix §9]); "`Create` ↔ `Delete` … and back" read as if reverting a delete re-created the node. [F06] open point 35. `COVERAGE.md` 60-I2-Gate0 cites [F06 §7.10] for the revert fixture's tombstone. **x** [RULES/merge-table] DM-017 quotes "`Create` → `Delete`" and names `deleted(kind, "", none)`; open point 33 records the bytes |
| closure §4.2, editorial ([API §6.7]) | — | 3 | fixed | [API §6.7]: a surviving reservation moves `next_id` and `next_anchor` only ([F05 §10.2]; its file numbers and schema ids are not in the runtime snapshot); `commit_seq`, `fence` and `next_ref_id` stand where the not-applied state leaves them. "The other allocation counters" is gone |
| closure §4.2, editorial ("recovery") | — | 3 | fixed | [F12 §2.2] ref-kind row `orphans`, the paragraph under it, §8.2 and open point 14, and [API §15.7] `moves` now name the parker as [F16] P-70 and [F11 §3.2] do: the first appender whose scan meets a commit whose ref CAS failed |
| A1-39 | major | 3 | fixed (residue) | As NC-8: the E409 lease case (DP-005's code of round 2) and DP-007's replacement case now have text; [F19 §10.5], [API §9.1] |
| S1-6 | blocker | 3 | fixed (residue) | As NC-9 (the tombstone a revert of a creation leaves, DM-017) |
| A1-4 | blocker | 3 | fixed (residue) | As S1-6 |
| P1-3 | blocker | 3 | fixed (residue) | The [API §6.7] counters and the parker wording of [F12 §2.2], §8.2 and [API §15.7] (closure editorial residues above) |
| S1-11 | blocker | 3 | fixed (residue) | As P1-3 |
| A1-11 | blocker | 3 | fixed (residue) | As P1-3 (the `park` wording) |
| A1-12 | blocker | 3 | fixed (residue) | As P1-3 (the `Reserve` counters of [API §6.7]) |
| A1-6 | blocker | 3 | fixed (residue) | [API §15.7] `markers`: `"actor"` is `<name or null>`, null where the row's `actor` is 0, which since round 3 is every entry except the `settled` one written by a `complete` that presented a lease ([F11 §7] `actor`, [F05 §9.5] field 9, MF-009); it read `"actor":…`, which could not print the 0 that most rows now hold |
| S1-16 | blocker | 3 | fixed (residue) | As A1-6 |
| A1-23 | major | 3 | fixed (residue); owner question OQ-F-3 | `README.md`: the [F11] row names `CURSORS` as change-feed and pack cursors (R-SPEC-R's note). [API] open point 48: `pack` maps to `Query`, which appends nothing ([API §14.1], [40] I-F5), while PX-011 has `pack` append [F05 §9.11]'s `feed` 2 cursor; until the owner decides, no M0 command appends a pack cursor, so C8 is empty on both sides of GT2 (R-SPEC-P's note §8.4 item 3). OQ-F-3 records the options and recommends that the delivering layer (hook or MCP server) append it, as [AR §5d.1] does for session cursors |
| A1-26 | major | 3 | fixed (`COVERAGE.md` only) | Rows F15 and 60-I2-Derived cite [RULES/merge-table] RE-010 and [RULES/delete-policy-matrix] DS-010 beside [F13 §6.3] (R-MODEL's round-2 note (3)); the chapter text is R-SPEC-P's and R-MODEL's; OQ-P-1 unchanged |
| P1-1, P1-2, P1-4, P1-5, P1-6, S1-1, S1-2, S1-3, S1-4, S1-5, S1-7, S1-8, S1-9, S1-10, S1-12, S1-13, S1-14, S1-15, S1-17, A1-1, A1-2, A1-3, A1-5, A1-7, A1-8, A1-9, A1-10, A1-13, A1-14, A1-15, A1-16, A1-17 | blocker | 3 | verified | Re-read after the round-3 edits of R-SPEC-P and R-SPEC-R: [F08 §5.1], §5.6, §10.2 (the round-3 precision of [F09 §7.2] on `at` blocks rebuilds the same bytes: `anchor` is the one bit an `at` edge admits), §10.3, §10.3.1, §11.2 and §4 (R-MODEL's round-2 cross edits, kept); [F06 §4.2], §4.4.4 (the HLC list equals [F04 §5.15], [F16] P-36, [OS/clock §7], [API §6.2] CK-4), §4.4.14, §4.4.15, §6.2, §7.4, §7.5, §7.7, §9 BK-2, BK-5; [F07 §7.1], §7.3, §8; [F12 §5.4], §6.3, §7.6, §7.8; [F18 §3.2], §3.7, §4.2, §4.7, §4.10 (the `anchor_spec` cases that [F20 §6.1] now cites exist in [F19 §10.2] with exit 2); [F19 §8.3]; [CFG §7.6]; `COVERAGE.md` 60-AR-Values, 60-AR-HEAD-params, R-4, R-7, R-15, R-18. A1-14 and S1-4 stay "closed, interim" (OQ-R-2); S1-14 and S1-15 "closed, owner" (OQ-M-1) |
| P1-7, P1-8, P1-10, P1-11, P1-12, P1-13, P1-14, P1-15, P1-16, P1-18, P1-19, P1-20, P1-21, S1-19, S1-20, S1-21, S1-22, S1-23, S1-25, S1-26, S1-27, S1-28, S1-29, S1-30, S1-31, A1-18, A1-19, A1-20, A1-21, A1-22, A1-29, A1-30, A1-31, A1-32, A1-34, A1-36, A1-37, A1-38, A1-40, A1-41 | major | 3 | verified | [F02 §3.6], §5.3, §6.3 ([F16] open point 10 and L-9 now cite [F02 §3.6] and [F19 §10.2] `store_retired`, as R-SPEC-F's round-2 note offered); [F06 §4.6], §7.3, §7.6; [F07 §7.3], §10.6; [F08 §3.4], §8.3, §8.5; [F12 §6.5], §7.3; [F18 §4.6]; [F19 §4.6], §10.2, §10.5; [API §6.2] CK-6, §6.4, §8.1, §12.4; [CFG §7.6], §10.5; `lq/envelope` §3.2, §3.3, §6.5, §9.3; `lq/errors` §3.3, §3.4, §5.4; `lq/std` §2.4, §4.1, §7.3; `HOLES.md` `F20-btime-ntfs`; `COVERAGE.md` X-F3, X-F5, 60-PA-(h), R-14, 60-AU-Seg-hist. A1-31 and A1-38 keep OQ-F-1 and OQ-F-2; P1-11 keeps OQ-P-2; S1-22, S1-30 and P1-21 their OQ-M-1 items |
| P1-23, P1-26, P1-29, P1-31, P1-32, P1-33, P1-37, P1-39, P1-41, P1-42, P1-44, S1-32, S1-33, S1-34, S1-36, S1-37, S1-38, S1-39, S1-40, S1-41, S1-44, S1-46, S1-48, A1-42, A1-43, A1-44, A1-45, A1-46, A1-48, A1-50, A1-51, A1-52, A1-53, A1-54, A1-55, A1-56, A1-57, A1-58, A1-59, A1-61, A1-62 | minor | 3 | verified | Checked by search in their current text, as in round 2 ([F01 §5.2], §8.3; [F06] open point 16, §6.1; [F07 §2.2]; [F08 §7.2], §8.4.6; [F12 §5.5a], §7.5; [F02 §3.3] rule 6; [F18 §2.2], §2.15; [F19 §3.2], §7.1, §10.2, §10.6, §12.1, §12.2; [CFG §4.1], §4.2, §6.2, §10.5; the `lq/*` sections of round 2; `README.md`; `HOLES.md` §1–§4; `COVERAGE.md` §1). [F08 §4] cites WT-004 and GR-002 (A1-50), both present. Every `HOLE(` outside `reviews/` is still indexed in `HOLES.md` §2 (no hole changed) |
| S1-18, S1-24, A1-24, A1-25, A1-27, A1-28, A1-33, A1-35, P1-9, P1-17, P1-22 | major | 3 | not F | [F03]–[F05], [F13], [F15]–[F17], `os/*`, [F09]–[F11], [F14], `rules/*`; their round-3 tables report them verified |
| P1-24, P1-25, P1-27, P1-28, P1-30, P1-34, P1-35, P1-36, P1-38, P1-40, P1-43, S1-35, S1-42, S1-43, S1-45, S1-47, S1-49, A1-47, A1-49, A1-60 | minor | 3 | not F | As above |

Rejected: none. Left open in the text: none. With the owner: OQ-F-1 (A1-31), OQ-F-2 (A1-38), and the new OQ-F-3
(A1-23's residue: who appends the C8 pack cursor; [API] open point 48 states the interim, under which both sides agree).

### Cross-role edits of this round (after a fresh read of each section; please review)

- **[RULES/merge-table] DM-017** (R-MODEL): the source cell quotes [F06 §7.10] "`Create` → `Delete`" (it quoted
  "`Create` ↔ `Delete`", which §7.10 no longer says), and the note names the tombstone `deleted(kind, "", none)`
  (`reason` 0, `replaced_by` 0). **Open point 33**: its last sentence states the bytes instead of "does not yet name".
  Row id, columns and outcome unchanged.
- **[RULES/delete-policy-matrix] DP-005** (R-MODEL): the source cell adds "[LQ/errors §5.5] E409 lease case" and the note
  ends "the text is [LQ/errors §5.5]'s lease case". **Open point 9**: its last sentence says the text exists and that
  DP-007 has its own E409 case. Row id, columns, code and exit unchanged.

### Other R-SPEC-F edits of this round with no finding of their own

- `COVERAGE.md` §2 records the round-3 check (no row cites the sections of NC-8 to NC-10 against a contradicting
  section; totals unchanged: 152 rows, 152 mapped, 0 CONFLICTING, 0 UNMAPPED).
- [F12 §2.2]: the paragraph under the ref-kind table ("a ref that only the park writes").

### Notes for other roles

- **R-MODEL:** (1) Review the two cross edits above. (2) The root-node refusal of [F08 §11.3] (E409's root-node case)
  has no row among [RULES/delete-policy-matrix §4]'s preconditions, so which E409 case a delete set that holds a root
  node and a live lease prints is unstated; a DP row (proposed: right after DP-003, since it concerns the delete set
  itself) would settle it ([LQ/errors] open point 17). (3) PX-011 has the `pack` verb append the C8 cursor, while [40]
  I-F5 lists `pack` among the verbs that append nothing. OQ-F-3 asks the owner; the recommended answer (the delivering
  layer appends) would change PX-011's actor, not its record. (4) DM-017 and merge-table open point 33 stay "proposed"
  until the owner re-signs (OQ-M-1 (i)).
- **R-SPEC-P:** [F05 §9.11]'s `feed` 2 record is unaffected by OQ-F-3's options (a) and (b); option (c) would withdraw
  `feed` 2 before the freeze. [F13 §3] I32′'s row may cite [LQ/errors §5.5]'s E409 lease case for its text (optional).
- **R-SPEC-R:** none. [F14 §6.10] already omits the `field reason:` line of an empty reason, which NC-9's tombstone has.
- **Closure checker (round 3):** the sections to re-read are [LQ/errors §2.2], §5.5 (E409 and the paragraph after the
  table), §5.7, open point 17; [F19 §10.5], open point 37; [API §6.7], §9.1, §15.7 (`moves`, `markers`), open point 48;
  [F06 §7.10], open point 35; [F12 §2.2], §8.2, open point 14; [RULES/delete-policy-matrix] DP-005 and open point 9;
  [RULES/merge-table] DM-017 and open point 33; `COVERAGE.md` rows 60-I2-Gate0, 60-I2-Derived, F15 and §2; the
  `README.md` [F11] row; [owner-questions] OQ-F-3.
