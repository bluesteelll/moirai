# Owner questions raised while closing review pass 1

| | |
|---|---|
| Title | Questions that need the owner's call, raised by the author roles while closing review pass 1 |
| Status | **decided 2026-09-28**: the owner accepted every recommendation ("Подтверждаю все", "I confirm all"): OQ-R-1 (a), OQ-R-2 (a) with (b) as the fallback, OQ-R-3 (a), OQ-R-4 (a), OQ-P-1 (a), OQ-P-2 (a), OQ-P-3 (a), OQ-F-1 (a), OQ-F-2 (a), OQ-F-3 (b), OQ-M-1 (a) (the rows are accepted; the owner's signature in `rules/SIGNED.md` follows under V3), OQ-M-2 (a). WP-81a edits the [AR], [60] and [50] texts each entry names. **Open:** OQ-F-4, raised in spec sync 2a after that decision; OQ-A-1 to OQ-A-4, raised in spec sync 2b (2026-09-29); OQ-A-5 and OQ-A-6, raised at the wave 3a gate (2026-10-05). |
| Scope | only questions that change an approved design decision or need the owner's call; runtime policy goes to config keys ([AGENTS.md]) and is not listed |

Each entry names the raising role, the findings, the options and the role's recommendation. Roles append entries; ids
carry the role letter (R = R-SPEC-R, P = R-SPEC-P, F = R-SPEC-F, M = R-MODEL, A = R-REV-A, which merged the owner
items of spec sync 2b).

## OQ-R-1 — The side ref's alias table fans out (P1-17)

- **Raised by** R-SPEC-R, pass 1 round 1. **Where** [F14 §14.1], [F14] open point 32; [AR §5b.1], [AR §5b.6] step 4,
  [AR §5b.9].
- **Question.** [AR §5b.1] names one blob `aliases.moi` in `refs/moirai/meta/<store-id>`. Rewritten whole at every export
  run it is ≈ 4.2 MB at 1e5 nodes (42 MB at 1e6), which the ≤ 50 ms incremental export of [AR §5b.9] cannot absorb, and the
  side-ref layout freezes at M0. [F14 §14.1] now writes `aliases/<h1>.moi`: 256 blobs keyed by the uid's first byte, of
  which a run rewrites only the prefixes it touched.
- **Options.** (a) Sign the fan-out; WP-81a edits [AR §5b.1] and §5b.6 step 4. (b) Keep one file and accept the rewrite
  cost against the export gate; [F14 §14.1] reverts to `aliases.moi` with the same row grammar.
- **Recommendation.** (a).

## OQ-R-2 — Scope scanners before the freeze (A1-14, S1-4, P1-20; closure open point 2)

- **Raised by** R-SPEC-R, pass 1 round 1. **Where** [F20 §6.1] "The interim scanner rule", [F20] open point 30;
  [F08 §10.3.1]; [40 §2.7], [40 §2.7.1]; [PLAN] FB-4.
- **Question.** The Rust, Markdown and TOML scanners decide an anchor's `scope` and, for the `symbol` and `heading`
  forms, its header line, quote, hint and header span hash, all of which are hashed. No chapter specifies the scanners
  yet. Until a normative scanner appendix exists, [F08 §10.3.1] records no scope and [F20 §6.1] refuses the `path::A/B`
  and `path#H` authoring forms, so no hashed byte depends on a scanner. This withdraws two [40 §2.7] authoring forms
  during M0.
- **Options.** (a) Confirm the interim rule for M0 and require the appendix (R-SPEC-R with WP-63) before the freeze.
  (b) If the appendix misses the freeze: keep the interim rule in format v1 (no scope, no `symbol`/`heading` forms until
  a format change). (c) If it misses the freeze: remove the scanner-derived bytes from `captured` and the selector block
  (a change of FB-4) and allow the forms with unhashed scanner output.
- **Recommendation.** (a) now; (b) as the fallback, since (c) changes anchor identity rules.

## OQ-R-3 — The residue of review S-16 after measurement 15 (S1-31, lens S decision D-1)

- **Raised by** R-SPEC-R, pass 1 round 1. **Where** [F20 §5.9] copy-rule line 2, [F20] open point 15,
  `HOLE(F20-btime-ntfs)`.
- **Question.** With the hole's constraint (`TunneledNotCopied` only if no measured tool copies creation times; else
  `Absent`), the residue of S-16 is unreachable for every tool measurement 15 tests. A creation-time-copying tool outside
  the measured set could still make a copy re-bind silently on NTFS.
- **Options.** (a) Accept that residue as a known risk when WP-81a fills the hole. (b) Take `Absent` for NTFS regardless
  (line 2 never applies on Windows: more same-volume moves become proposals).
- **Recommendation.** (a), with the measured tool list of [F20]'s Holes row.

## OQ-R-4 — Section rows of [AR §4.4] changed by pass 1 (confirmation)

- **Raised by** R-SPEC-R, pass 1 round 1. **Where** [F09 §7.2], [F11 §8], §12.8, §13; [F10 §7.1]; [AR §4.4], [AR §8.1].
- **Question.** Pass 1 changed rows the design states: `EDGE_PROPS` is `{edge u32, pflags u8, pad, pinned_commit [32]}`
  (40 B), not `(edge idx u32, pinned_commit id16)` (S1-2, A1-3: the canonical form hashes the full pin and the `flagged`
  bit); `IDEM` is 72 B per slot, not 40 B per key (P1-28); `FPRINT` carries the `blobs` file number (P1-22); a pair's
  `gitmap` pages are tiered by `store.fold-width` (P1-22); three runtime tables `CURSORS`, `SESSMARKS` and `BACKUPS` hold
  cursors, session marks and backups (A1-23).
- **Options.** (a) Confirm; WP-81a updates [AR §4.4] and [AR §8.1]. (b) Name the row to revisit.
- **Recommendation.** (a).

## OQ-P-1 — The `suspect` budget's reading (A1-26; [F17] OP-17-15; lens S decision D-4)

- **Raised by** R-SPEC-P, pass 1 round 1. **Where** [F17 §8.2], [F17] OP-17-15; [AR §2.5]; [F13 §6.3]; [F19 §12.3]
  `SuspectBudget`.
- **Question.** [AR §2.5] says that beyond the `suspect` closure budget "a violation record is written". `Violation` ops
  exist only on staging refs ([AR §4.6]) and I9 forbids leaving the `suspect` bitset stale. [F17 §8.2] therefore reads the
  budget as: the bitset is always complete; `affected` omits the `suspect`-only changes and sets `affected_complete = 0`
  (I42′); the command returns the hint `SuspectBudget`. Review pass 1 (lens S, D-4) confirmed the reading from the
  correctness side; it changes the design's wording, so it needs the owner.
- **Options.** (a) Sign the reading; WP-81a edits [AR §2.5]. (b) Refuse or stage the commit with a new violation class
  (blocks deletes of widely cited nodes). (c) Truncate the eager maintenance (violates I9).
- **Recommendation.** (a).

## OQ-P-2 — A default-cap agent `TX` does not fit the inline bound (P1-11 residue; [F17] OP-17-25)

- **Raised by** R-SPEC-P, pass 1 round 1. **Where** [F17 §4.4] W1, W4; [CFG §10.5]; [AR §4.3], [AR §8.3] RAM row "a
  default-cap `TX` ≤ 4 MB"; [50 §5.10], [50 §5.12].
- **Question.** Agent verbs never create bulk commits ([AR §4.3]), so an agent changeset above
  `store.commit.inline-max-bytes` (P05, design-fixed 1 MiB) is refused with E501 whatever its `wmem`. At ≈ 190–240 B per
  op that is ≈ 4,400–5,500 ops, while `tx.max-ops` defaults to 10,000 and [AR §8.3] budgets 4 MB of write memory for a
  default-cap `TX`. The op cap therefore never binds at production values.
- **Options.** (a) Keep both values; a `TX` is bounded by P05 and E501 names the split (no text change). (b) Lower the
  default `tx.max-ops` to what P05 holds (≈ 4,000), so the visible cap is the op count. (c) Raise P05 within C-1
  (≤ `store.log-extent-bytes` / 8 = 8 MiB), which grows every inline group and the writer's in-lock re-serialisation.
- **Recommendation.** (a) now; (b) if the owner wants the op cap to be the bound agents see.

## OQ-P-3 — Format and protocol additions of pass 1 in R-SPEC-P's chapters (confirmation)

- **Raised by** R-SPEC-P, pass 1 round 1. **Where** [F03 §3], §3.1; [F04 §3.1], §5.15, §5.16; [F05 §4.4], §4.5, §9.27,
  §9.28; [F16] P-36, P-72, P-92, P-96–P-98; [F17 §3] C-4.
- **Question.** Closing the pass-1 findings added bytes and rules that the design of record does not state; each stays
  inside reserved space or an open design choice, but together they are worth one confirmation:
  (a) `LOCK`: eight more quiet bytes at `ROLE_BASE` + 5 … + 12, from X-F1's reserved range (P1-10);
  (b) `HEAD`: `project_oid_algo` (A1-15) and the HLC maxima `hlc_seq`, `hlc_commit` (P1-5) in the reserved area, beside
  [80 §2.4.3]'s "No `HEAD` field is added" (which concerns group commit, [F04] open point 3);
  (c) the log: record kinds 27 `Reserve` (P1-3) and 28 `ExtentHead` (P1-8), the extent head replacing the 40-byte `Noop`
  epoch-start group and beginning every extent, and a rotation reserve of 178 bytes (S1-18);
  (d) the protocol: a spare extent prepared ahead under a temporary name (P1-7); long maintenance jobs that yield with
  release-free checkpoints, which caps `store.fold-width` at 5 (P1-9);
  (e) a writer that meets a read error above `durable_lsn` refuses (exit 7) instead of treating it as the end of the
  log, so a persistent read error blocks writes until `repair` (S1-25; X5 over availability).
- **Options.** (a) Confirm all; WP-81a records them in [AR §4.1]–§4.3 and [80] where those restate the layouts. (b) Name
  the item to revisit.
- **Recommendation.** (a).

## OQ-F-1 — The cut `--ids` page (A1-31; [F19] open point 6)

- **Raised by** R-SPEC-F, pass 1 round 1. **Where** [F19 §6.2], [F19] open point 6; [LQ/envelope §6.5]; [CFG]
  `output.ids-max-bytes`, `output.nonzero-exit-max-bytes`; [AR §7.1]; [90 §2.1], [90 §6.1].
- **Question.** [AR §7.1] pages `--ids` at `output.ids-max-bytes` (24,000 B) with exit 10, and also caps the stdout of every
  non-zero exit at 8,000 B, because a harness shows little of a failed call (Claude Code ≈ 10,000 characters). The two
  rules meet on a cut page. [F19 §6.2] resolves them: a result that fits 24,000 B prints whole with exit 0; a cut page
  prints at most min(`output.ids-max-bytes`, `output.nonzero-exit-max-bytes`) = 8,000 B with exit 10; and
  [LQ/envelope §6.5] now cites that rule (review pass 1, A1-31). This changes [AR §7.1]'s "pages at 24,000 B" for the cut
  case.
- **Options.** (a) Confirm the 8,000 B cut page; WP-81a edits [AR §7.1]. (b) Keep 24,000 B cut pages with exit 10 and
  accept that Claude Code truncates them (the page then fails its purpose there). (c) Cut at 24,000 B but exit 0 with a
  continuation footer (changes the exit-10 contract of [AR §7.1]).
- **Recommendation.** (a).

## OQ-F-2 — `std.ready` without `topo` (A1-38; [API] open point 11)

- **Raised by** R-SPEC-F, pass 1 round 1. **Where** [LQ/std §4.1] and its catalog row; [API §16.6]; [50 §4.1]
  (`ORDER BY t.priority, t.topo, t.id`).
- **Question.** No open precedence path orders two ready tasks, so `topo` breaks their priority ties by whichever valid
  topological order an implementation computes, and the engine and the reference model can list different first pages
  of `ready` (GT2 would have to compare it as a set). Review pass 1 (A1-38) offered two fixes; [LQ/std §4.1] took the first: `ORDER BY t.priority, t.id`.
  The text of [50 §4.1], which [LQ/std §4.1] otherwise follows verbatim, changes at WP-81a.
- **Options.** (a) Confirm `priority, id`. (b) Keep `topo` and define it as one total, deterministic order (for example
  Kahn's algorithm taking the smallest `#N` first), which every implementation must then compute identically for every
  view.
- **Recommendation.** (a): the tie-break among ready tasks carries little meaning, and (b) freezes an algorithm every
  implementation must reproduce for every view.

## OQ-F-3 — Who appends the C8 pack cursor (closure NC-10's residue; [API] open point 48)

- **Raised by** R-SPEC-F, pass 1 round 3, from R-SPEC-P's round-3 note (`pass1-dispositions-P.md` §8.4 item 3); not a
  pass-1 finding. **Where** [RULES/pack-classes] PX-011 (proposed), PT-028, PM-025 and open point 8; [F05 §9.11] `Lazy`
  `feed` 2 and [F11 §13.1] `CURSORS` (round 3); [AR §7.4] C8; [AR §5d.1] row "change-feed `seq`, per-session cursors";
  [AR §7.7.1] "Reads never write"; [40] I-F5; [API §14.1].
- **Question.** [AR §7.4] C8 is the "delta since this (agent, T) cursor". PX-011 has the `pack` verb itself append that
  cursor after emitting, a lazy record whose bytes [F05 §9.11] now defines (`feed` 2). [40] I-F5 lists `pack` among the
  read verbs that "append nothing to the log", asserted by counting, and [API §14.1] maps `pack` to `Query`, which
  appends nothing. The two approved texts cannot both hold for `pack`, and the engine and the reference model render a
  different C8 as soon as one side appends the cursor and the other does not.
- **Options.** (a) `pack` is the one read verb that appends: I-F5 gains the exception "one lazy pack cursor after
  emitting" and its counting test excludes it; [API §14.1] says a `Query` of `pack` appends the cursor, and the model
  does too, so C8 compares in GT2. (b) The cursor is appended by the layer that delivers the pack (the `SubagentStart`
  hook or the MCP server), as [AR §5d.1] already stores the per-session cursors (server memory, else a lazy record with a
  try-lock); the read verb appends nothing and I-F5 stands; GT2 streams at M0 have no such command ([API] open point 25),
  so C8 is empty on both sides. (c) C8 reads the session cursor of [AR §7.5] (`std.delta` since the session cursor), no
  per-(agent, T) cursor exists, and `feed` 2 is withdrawn before the freeze.
- **Recommendation.** (b): it keeps I-F5 and "reads never write" whole, follows the design's own rule for the other
  read cursors, and changes no byte: [F05 §9.11] `feed` 2 is the record the delivering layer appends. PX-011 would then
  name the delivering layer instead of the verb.

## OQ-M-1 — Rule rows changed by pass 1, for the re-signature (V3)

- **Raised by** R-MODEL, pass 1 round 1 (extended in round 2). **Where** `rules/merge-table.md`,
  `rules/link-merge-rules.md`, `rules/state-definition.md`, `rules/status-machines.md`, `rules/pack-classes.md`,
  `rules/role-write-policy.md`, `rules/delete-policy-matrix.md`, `rules/README.md`; [AR §3.4] I31′, [AR §3.5],
  [AR §5a.7] step 1, [60 §3.4], [60 §3.13] GT6.
- **Question.** Every edit breaks a rule file's signature ([RULES/README §6]). Pass 1 changed rows where the design is
  silent or where a row now reads the design's wording in a stated way:
  (a) the re-key re-points every edge S added to U, `replaced_by` and `ref` values (RK-004 to RK-006, RK-010, RK-011;
  S1-14, S1-48);
  (b) a conflict-valued base conflicts only when both sides changed the key and differ, one untouched side letting the
  other land (MR-001 to MR-004 in that order, CS-006, RS-010, VB-018; S1-15), which edits the wording of [60 §3.4],
  [AR §5a.7] step 1 and I31′;
  (c) the marker's origin reading and holder-set cache (state-definition OR, MF-006, MF-007, ME rows; RE-003; S1-16);
  (d) `gates` counts only in the completion guard, not in `open_blockers` (BT rows; S1-30), which edits [AR §3.5];
  (e) a plain base with a conflict-valued side takes the flat conflict form of [F12 §5.4] (MR-005, RS-015; P1-21), a rule
  the design does not state;
  (f) the validators VA-005, VA-007, VA-008 and VA-013 stage with the new classes `DepthExceeded`, `Cardinality` and
  `PlanMask` of [F19 §12.2] (P1-21);
  (g) a root node dead on one side (unreachable by [F08 §11.3]) is a failed internal check, exit 1 (LM-013, LR-006;
  P1-21);
  (h) a take towards a live `DeleteVsModify` side restores hierarchy and out-edges from that side's state (RS-008; S1-22).
  Round 2 added rows that align the tables with the chapters that own the rule (`pass1-dispositions-M.md` round 2):
  (i) a revert never makes a node `absent`: a reverted creation leaves a tombstone, a reverted `CreateDeleted` leaves
  dst's value (DM-017, proposed, from [F06 §7.10]); a pruned commit is never reverted or picked (DM-016);
  (j) merge results are never empty values, and a clean diff3 longer than 65,536 bytes is a `TextHunk` (CS-001,
  CS-011, RS-004 to RS-006, RS-013; [F08 §5.3], [F12 §7.5]);
  (k) the `suspect` budget reads [F17 §8.2] (DS-010; OQ-P-1 covers the reading itself);
  (l) the last `gap` row, LL-014 (a `leader` lease anchor), is unreachable by [F03 §10.3] and [F11 §6]; `orphans`
  refs never hold (VK-007);
  (m) refusal codes named from [F19 §10.2] and [API §9.1]: `not_writer_tree` (WZ-009), `not_found` (DP-003,
  GR-005), `usage` (DP-004), E409 for a live lease (DP-005), `staging_exists`, `conflicted_src` and
  `revert_refused` (PR-002, PR-003, DM-006 to DM-008).
  Round 3 added (`pass1-dispositions-M.md` round 3):
  (n) a root node anywhere in the deleted set refuses a delete with E409's root-node case, checked after the options and
  before the lease (DP-010, from [F08 §11.3]); DP-006 and DP-007 cite their E409 texts;
  (o) the lease holder and the outcome of a `settled` marker are set only on the entry a `complete` writes, and a
  re-emit carries 0 (MF-009, as [F05 §9.5] fields 9 and 11 store it);
  (p) the pack's C2 and C8 inputs read the `SESSMARKS` and `CURSORS` rows of the session the pack runs in, T held as its
  `#N` (PT-027, PT-028, PX-011, from [F05 §9.11], §9.14 and [F11 §13]); who appends the C8 cursor stays with OQ-F-3.
- **Options.** (a) Re-sign the files as they stand; WP-81a edits the [AR] and [60] texts named above. (b) Name the rows to
  revisit.
- **Recommendation.** (a).

## OQ-M-2 — I13's "different actor" has two readings in two chapters

- **Raised by** R-MODEL, pass 1 round 1 (found while aligning the rule files; no pass-1 finding). **Where**
  [RULES/status-machines] GD-005 and its open point 11; [F13] OP-13-09; [AR §3.4] I13.
- **Question.** I13 lets a `perf` or `complexity` finding reach `fixed` only "with an `addresses` edge from a different
  actor". GD-005 reads the actor of the `addresses` edge as different from the creator of the reviewing verdict (the
  reviewer did not review their own fix); [F13] OP-13-09 proposes different from the actor of the commit that moves the
  finding to `fixed`, and says the status-machine table carries the reading. The two readings accept different histories.
- **Options.** (a) GD-005's reading. (b) [F13] OP-13-09's reading. (c) Different from the finding's author (GD-005's
  stated alternative).
- **Recommendation.** (a): it is the one that makes "retest is not re-review" bite, since the review verdict must come
  from someone other than the fixer; [F13] OP-13-09 then cites GD-005.

## OQ-F-4 — Three additions to the closed `unverified` set (open; [F18] open point 12)

- **Raised by** R-SPEC-F, spec sync 2a. These additions date from WP-14 and pass 1, but no earlier entry asked for them,
  so the decisions of 2026-09-28 do not cover them. **Where** [F18 §4.6] codes 59–61, [F18] open point 12; [F20 §1.5],
  [F20] open point 22; [40 §2.9] (the closed `unverified` set); [80 §2.11.4] rule 9; [40 §2.4], [F02 §7.3].
- **Question.** [40 §2.9] closes the `unverified` reasons at `budget`, `cloud-only`, `commit not in this repository`,
  `no tree`, `git` and `size`, and every reason is a frozen string. [F18 §4.6] adds three:
  (1) `unreadable` (code 59): a denial on the stat of the path or on a content read ([80 §2.11.4] rule 9, [F20 §4.8]);
  (2) `unmapped root` (code 60): a named root with no `roots.<name>` mapping on this machine, the rendering [40 §2.4] and
  [F02 §7.3] already name but the closed set lacks;
  (3) `oid algorithm differs` (code 61): a `file` anchor with `span` watch whose captured `blob` has another algorithm than
  the current content ([F20 §6.5]).
  Each is a frozen-string addition to the set [40 §2.9] closes, as `none` and `unresolved` were at the A1 re-review.
- **Options.** (a) Sign the three additions; WP-81a adds them to [40 §2.9]. (b) Decline: `unmapped root` renders
  `no tree`, and `unreadable` and `oid algorithm differs` render `budget`, which [F20] open point 22 shows to be
  misleading (a retry with a larger budget changes nothing). (c) Name the addition to revisit.
- **Recommendation.** (a): each names a cause the agent can act on (fix a permission, map the root, re-capture the
  anchor), and the codes and strings already stand in [F18 §4.6] and [F20 §1.5], so (a) changes no spec chapter.

## 2026-09-29, spec sync 2b

Raised by R-REV-A from the owner items of the spec sync 2b routing table (its OWNER O-1 to O-10), after the spec roles
applied their rows (`spec-sync-2b.md`). Duplicates are merged: O-1 and O-10 → OQ-A-1; O-2 → OQ-A-2; O-3 to O-8 →
OQ-A-3; O-9 → OQ-A-4. Source ids (S, R, A, F) are those of `spec-sync-2b.md`, whose Source index names the work package
of each. Rows there that cite "OWNER O-n" depend on the question named here, and the design-text edits that follow are
its WP-81a list (W-n).

### OQ-A-1 — E4 and seeded bugs that no toy state reaches (O-1, O-10)

- **Raised by** the WP-40 author, review and closure; routed in spec sync 2b. **Where** [PLAN §7] E4, [PLAN §3.2]
  WP-40; [80 §2.4.4] bug (5) (G5); [F16 §17.2] (vehicle "none (masked)"), [F16 §17.3] rows P-45, P-59, P-79, P-97, [F16]
  open point 1; `spec-sync-2b.md` S2B-P-41, S2B-P-44, S2B-P-45, S2B-P-53. Sources S8(9), S9(3), S9(5), S10(3), S11(1),
  S12(2), S12(4), R39, R47.
- **Question.** E4 requires the toy-log enumeration to find every seeded bug, "the 13 of [80 §2.4.4]" among them. WP-40
  found no reachable toy state for three catalogue bugs, and spec sync 2b gave each a disposition in its [F16 §17.3] row:
  (1) P-45 (G5, "publish a smaller `durable_lsn`") is masked: every publish that raises `durable_lsn` holds the flush
  byte, so max(`durable_lsn`, E) = E in every reachable state, also after a failed `HEAD` flush. Its ig6 detector is
  still asserted at every publish and is unit-tested on a synthetic publish sequence that lowers `durable_lsn`; vehicle
  "none (masked)". The engine keeps the same flush-byte rule, so M1 cannot reach it either.
  (2) P-59 (a reader falls back to an older segment set) is masked in the toy, where a fallback set whose files and log
  still exist replays to the same state; vehicle M1, whose retirement and promotions can reach it.
  (3) P-79 (the orphan sweep deletes a file that a pending group names): the toy has no sweep; vehicle M1, where a bulk
  writer's `cs.<n>` is the pending namer that matters.
  The other bugs first found unreachable are now reached: P-54 (T13) and P-55 through setup-injected external rewrites
  ([F15] FM-10.1) at `repair`'s scan start, P-61 through a setup-injected rewrite of one `HEAD` slot (FM-10.1), and P-13
  and P-40 by re-formed bugs (R47). One rule moved the other way: P-97 (extent heads), at M1 before, is now carried by
  the toy (S2B-P-53), which rotates, retires and repairs from the extent heads, so the toy's E4 list gains its bug and
  [F16] open point 1 counts 76 rules in the toy and 23 at milestone gates. E4 as written cannot pass with (1), and it
  covers (2) and (3) only through [F16] open point 1's reading (a rule whose mechanism WP-40 does not build is carried by
  the gate of the milestone that builds it). WP-40 stays unaccepted until this is decided.
- **Options.** (a) Amend E4 to "… the 13 of [80 §2.4.4] (G5 by a unit test of its detector, which the flush byte masks,
  [F16 §17.3] P-45) …", with a general rule: a catalogue bug that another rule masks in every reachable toy state
  carries a written disposition in its §17.3 row (re-vehicled to a milestone gate, re-formed into a reachable bug, or
  covered by a unit test of its detector), accepted at WP-80. P-59 and P-79 stay at M1. WP-81a edits [80 §2.4.4] (W-7).
  The spec rows already hold these dispositions. (b) Keep E4 and require a reachable toy form of each bug: WP-40 adds an
  orphan sweep (`tmp/` ageing, P-78 claims) for P-79 and a toy form of P-59's fallback; no form of P-45 was found, so
  WP-40 could not pass E4. (c) Leave all three out of M0's E4 and carry them at M1; P-45 is masked there too, so this
  only defers its question.
- **Recommendation.** (a). A bug that no state can reach proves nothing about the harness, while the unit test still
  proves that its detector fires. P-59 and P-79 then follow the reading [F16] open point 1 already gives the 23 rules
  carried at milestone gates (24 before S2B-P-53 moved P-97 to the toy), and an orphan sweep in WP-40 would build a
  mechanism whose relevant namer exists only at M1.

### OQ-A-2 — Who writes the detectors behind E4 (O-2)

- **Raised by** the WP-40 review and closure (R40, R48); routed in spec sync 2b. **Where** [PLAN §3.1] S4; [PLAN §2.2]
  (`moirai-toylog` does not depend on `moirai-model`); `docs/m0/authors.md` §3 (`moirai-vfs-sim` is R-HARN's,
  `moirai-toylog` R-TOY's); [F13 §1.4] "The toy vehicle (M0)", [F13] OP-13-02; [F16 §17.2] "Where the detectors live for
  the toy vehicle"; `spec-sync-2b.md` S2B-P-19, S2B-P-46. Sources S9(4), S10(5), S11(3), S12(7), S33(2), R40, R48.
- **Question.** S4 keeps the seeded-bug author from being the enumerator's author, so that the harness cannot be tuned
  to its own bugs. In WP-40, R-TOY also wrote the checks that report its bugs: the I-G4 and I-G6 trace predicates, the
  namespace check, read freshness and visibility, `doctor --verify` and the `TOY_DETECTION` family overrides
  (`tests/common/mod.rs`, `verify.rs`); 38 of the 76 detections come from them (R48). Spec sync 2b placed these checks:
  the generic predicates (I-G4 and I-G6 over the simulator's lock, flush and namespace events and the decoded `HEAD` slot
  writes) and the ns check of [F16 §17.2] live in `moirai-vfs-sim`, written by R-HARN-S, and avail joins ack, fresh and
  chain as a verdict of the enumerator; the checks that need the toy's own state (`doctor --verify`, which is the toy's
  `model` family, and read visibility against its replayed view) stay in the toy and are reviewed by R-HARN-S as a WP-40
  acceptance step; `TOY_DETECTION` goes, because the §17.3 rows now carry the families. PLAN gives this work to no WP
  and no role.
- **Options.** (a) Adopt the split by a plan issue: R-HARN-S's outputs gain the generic predicates, the ns check and
  the avail verdict (a WP-32 follow-up that WP-40's acceptance depends on), WP-40's acceptance gains "R-HARN-S reviews
  the toy's own checks", and S4 gains "the detectors are the enumerator author's, or reviewed by that author". (b) The
  review alone: R-TOY keeps every check, and R-HARN-S reviews `tests/common/mod.rs`, `verify.rs` and the families as a
  WP-40 acceptance step, an explicit S4 exception.
- **Recommendation.** (a). [F13 §1.4] already keeps the predicates out of the bug author's crate and PLAN §2.2 forbids
  the toy → model edge, so `moirai-vfs-sim` is the one crate that satisfies both. S2B-P-19's code follow-ups wait on
  this answer. (b) is the fallback if R-HARN-S cannot take the work before WP-40's acceptance.

### OQ-A-3 — PLAN rows that lag the spec or the code (confirmation; O-3 to O-8)

- **Raised by** the WP-32, WP-30b/31b/33b, WP-40 and WP-91 authors and reviews, and R-SPEC-P's and R-SPEC-R's spec
  sync 2a follow-ups; routed in spec sync 2b. PLAN.md changes only by a plan issue the owner reviews
  (`docs/m0/authors.md` §3), so the items are listed together for one answer. **Where** each item.
- **Question.** Each item brings PLAN to what the spec already states or the code already does:
  (a) **WP-32, the `HEAD` slots** (O-3). "Both `HEAD` slots, with 9 states per barrier" → "the 9 states, less (torn,
  torn) when both slots are dirty". Both slots lie in one file and FM-1.2 tears at most one dirty sector per file, so two
  torn dirty slots cannot occur; (torn, torn) is reached when one slot is poisoned (FM-3.3). The spec says so in
  [F15 §6.4] (S2B-P-11), and WP-81a edits [60 §3.13] (W-6). Sources S32(1), S33(1), S34(1), R19.
  (b) **WP-32, the other obligations** (O-4). The row cites [F15 §6.4] as a whole, which adds its "Reads" row
  (transient and persistent read errors, mapping-fault deaths, external truncation of a sealed file) and disk-full at
  namespace operations. WP-32's code already covers the wider reading. Source S32(3).
  (c) **WP-33, `test_host::small_volume`** (O-5). Not built at M0: it needs elevation (owner-run only), profile L's
  nightly never calls it, and it serves only WP-57's optional VHDX measurement 18, whose run the owner deferred on
  2026-09-29. [OS/proc §13] now says so (S2B-P-2), as PLAN WP-33 does. The alternative is to add it to an M0 WP.
  Sources S0(b), S29(b), R11.
  (d) **WP-91, `fixtures/carrier/`** (O-6). WP-91's dependencies and acceptance name `carrier/` beside `canonical/`, so
  that E3's "reproduces every commit-id fixture" covers the `changeset-digest` and `commit-id` of all 44 carrier cases,
  about 20 of which `canonical/` lacks. `fixtures/carrier/INDEX.md` and WP-21's acceptance already say so, and the
  carrier harness exists (S5(S6)). The alternative is to narrow the carrier INDEX's acceptance line. Sources S4(S6),
  S5(S6), R30.
  (e) **§2.1 GT20 (a) and §2.2 `moirai-os`: `os::spawn`** (O-8). `moirai-os`'s `spawn` module is built on Windows at
  M0, because `ProcHost`, a `Vfs` supertrait complete at M0, carries `spawn_gc_child` and `enter_background`; it is
  GT20 (a)'s one allowed spawn site in product non-test code ([OS/README §2.2], §3 row `os::spawn`; [OS/proc §11]; spec
  sync 2a). PLAN §2.1 still says "At M0 product crates spawn nothing", and §2.2's module list for `moirai-os` lacks
  `spawn`. The alternative is to revert the spec. Source F0.
  (f) **The gate's build cost** (O-7). `moirai-toylog`'s PR tier costs about 24.5 CPU-minutes (6–7 minutes of wall time
  on 4 threads) in debug builds, and hosted CI runs it on every push to `master`. Proposed: R-HARN-I sets
  `[profile.dev.package.xxhash-rust] opt-level = 3` and `[profile.dev.package.moirai-vfs-sim] opt-level = 2` in the root
  manifest, which it owns; the product crates stay in debug. No PLAN text changes; the item is listed so the owner can
  object. Alternatives: `[profile.test] opt-level = 1` for every crate, or no change. Source S8(11).
  (g) **Carried from spec sync 2a** (R-SPEC-R's follow-up list; not yet in PLAN). WP-63's inputs gain [F21], and its
  acceptance "Synthetic fixtures" reads "the [F21 §8] fixtures"; WP-21's `r4/` outputs gain the scanner constructs of
  [F21 §8]; WP-67's scanner fuzz targets follow [F21 §3–§5]; §2.5's tree `format/01..20-*.md` becomes `01..21`.
  Source F1.
- **Options.** (a) Confirm all: one plan issue applies (a)–(e) and (g), and R-HARN-I makes (f). (b) Name the items to
  revisit; each item states its alternative.
- **Recommendation.** (a). No item moves a byte or weakens a gate, and each removes a place where an agent that reads
  PLAN would build less than the spec requires, or something else.

### OQ-A-4 — What ends a Dead or expired lease (O-9)

- **Raised by** the WP-90b review and fix (R22, R23); routed in spec sync 2b. **Where** [AR §6.2] "Liveness" ("after a
  reboot every non-run-scoped lease is Dead and is released at the first read with a triage line"); [40] I-F5 and
  [API §14] (a read appends nothing); [RULES/state-definition] LE-009, LE-011, LE-012 and open point 18; [API §10.1],
  §10.2, §11.2, §15.7; [F05 §9.4] field 18, reason 4; `spec-sync-2b.md` S2B-F-69 to S2B-F-71, S2B-P-24, S2B-M-28.
  Sources S37(2), R22, R23.
- **Question.** A release "at the first read" needs the read to append a record, which I-F5 forbids; a release that a
  read keeps only in memory is lost with the process, so the engine could not match a model that keeps it. Leaving such
  leases unended had two defects (R22, R23): another holder could claim a task whose TTL lease had expired, and the old
  holder could then renew it by `heartbeat`, giving two live leases; and a Dead session lease lived again when its
  session held its slot again, beside a lease taken meanwhile. The rules and the spec now apply LE-012: a claim that
  grants a new lease on a task first ends, in its own group, every lease on that task that is not live (reason 4); a Dead
  lease that nothing ended is judged again at each evaluation and can revive only while it is the task's only lease;
  renewing an ended lease is E407; the first-read line stays as a triage line, and the runtime snapshot shows `live` =
  `dead`.
- **Options.** (a) Keep LE-012; WP-81a edits [AR §6.2] (W-5). (b) Every write group ends the Dead leases it sees, which
  adds records and a liveness probe to every write.
- **Recommendation.** (a). It keeps one live task lease per task (mutual exclusion) with no cost on the write path and
  no write from a read, and the engine can match the model record for record.

## 2026-10-05, wave 3a gate

Raised by R-REV-A at the wave 3a gate from two spec arbiter rulings made while wave 3a was being finished: the ruling
on the P3 cases of `fixtures/r4/` (WP-61, fault "spec"), which no text applies yet, and the ruling on RS-007's forest
sentence (the WP-91/92 sync closure), applied text-only as `spec-sync-2b.md` S2B-M-32 and S2B-F-99. Neither ruling
changes behaviour; each leaves one call to the owner.

### OQ-A-5 — P3 at M0: six `r4/` cases that no M0 crate can pass

- **Raised by** the spec arbiter's ruling on `fixtures/r4/cases/paths.cases` p3-01 … p3-06 (wave 3a, WP-61; fault
  "spec"). **Where** [PLAN §3.2] WP-61; [OS/path §1] placement table (row `moirai-files`), [OS/path §3] row P3,
  [OS/path] open point 2; [80 §5.2] (NFC precomposition (P3) in the macOS port column only), [80 §5.4] row M0; [60]
  owner decision #32; [PLAN §6.2] R6 (`fixtures/ucd/17.0.0/INDEX.md`); [F12 §2.5] IN-1, [F20 §3.4];
  `fixtures/r4/INDEX.md` §3.2.
- **Question.** WP-61 builds "P1–P12 as pure functions" and is accepted when "the `r4/` fixtures pass". The six P3
  cases (`stored_untracked_name`) match [OS/path §3]'s P3 row, which stores an untracked name as NFC on a
  normalization-insensitive volume, but no M0–M11 crate can compute NFC: the pinned UCD set (R6) holds no
  composition-exclusion data, M0 avoids a composition table elsewhere (IN-1, [F20 §3.4]), [80 §5.2] places P3 in the
  macOS port, and decision #32 builds no macOS behaviour before the port phase. Four texts say otherwise: PLAN WP-61;
  [OS/path §1]'s `moirai-files` row, which puts P3 in target-independent code and cites R6 as sufficient; [80 §5.4]'s
  M0 row ("path rules P3–P12 … in FL-1 and its tests"); and `fixtures/r4/INDEX.md` §3.2, which lists the six cases
  with no port marking. WP-61 cannot be accepted until this is decided.
- **Options.** (a) The port phase builds P3, as the ruling's Spec findings (1), (2) and (6) propose. PLAN WP-61's
  deliverable reads "P1, P2 and P4–P12 as pure functions (P3 is the port phase's, [OS/path §3] and open point 2)", and
  its acceptance "the `r4/` fixtures pass, except `paths.cases` p3-01 … p3-06, which no M0–M11 crate runs; the macOS
  port runs them". [OS/path §1]'s `moirai-files` row drops P3, and a note after the table says that the port phase
  adds it ([80 §5.2]), that it applies only where `VolumeCaps` sets `norm_insensitive_always`, and that its function
  needs composition data the R6 set does not hold. R-FIX marks the six cases "port phase" (p3-06 provisional) in
  `fixtures/r4/INDEX.md` §3.2 without changing a byte of them, and WP-81a edits [80 §5.4]'s M0 row to "P4–P12".
  (b) Build P3 at M0 with Unicode 17.0.0 NFC: the owner pins `CompositionExclusions.txt` in `fixtures/ucd/17.0.0/`
  (R6), `xtask ucd` generates composition tables, and WP-61 runs the six cases. This adds tables and a design input
  that M0 avoids elsewhere, builds behaviour that only macOS uses, and can still be wrong on p3-06 (below).
- **Recommendation.** (a). The cases test the spec correctly but cannot run before the port, and (a) changes no
  fixture or format byte, no frozen rule and no other gate: P3 stays frozen in [OS/path §3] and X-F7. Open point 2
  stays open under either option. Whether P3's function is git's precomposition (`iconv` from UTF-8-MAC, which may
  leave a canonical singleton such as U+212B unchanged) or Unicode 17.0.0 NFC (which maps it to U+00C5) decides
  p3-06's expected value; p3-01 … p3-05 stand under both. Only the port's test against git on APFS settles it, and the
  ruling's findings (3) and (4) (P3's function and the p3-06 re-check) wait for that test, with (5) (the composition
  data a port pins in `fixtures/ucd/17.0.0/INDEX.md`).

### OQ-A-6 — Change the Kleppmann replay (RS-007) before the engine implements it (open point 35)

- **Raised by** the spec arbiter's ruling on RS-007's forest sentence (wave 3a, WP-91/92 sync closure), from R-MODEL's
  WP-91/92 fix 2 (Spec finding 1); its text-only part is `spec-sync-2b.md` S2B-M-32 and S2B-F-99. **Where**
  [RULES/merge-table] RS-007, MR-039, MR-040, CS-013, open points 15 and 35; [F12 §7.4] row "Kleppmann steps", [F12]
  open point 30 (d); [PLAN §7] E1, V3 (`rules/SIGNED.md`).
- **Question.** RS-007 replays hierarchy moves commit by commit from the base B and undoes a step's moves when they
  close a cycle. Its claim that "a one-sided history never meets a cycle" was false; the text now gives the guarantee
  only when src's commits since B are a single-parent chain from B, merged into a dst that made no commit since B.
  Open point 35 records three known cases outside it whose result is not one the sides' histories call for:
  (i) a two-parent commit (a merge or a `sync`) that kept its own value of a key the merged branch changed is no step
  for that key. It can land clean with a value that neither tip holds (a lane synced after resolving a skip to `ours`,
  then merged into another lane), or stage a spurious `HierarchyCycle` on the routine path "sync, resolve to `ours`,
  keep working, merge into an unmoved main";
  (ii) a cherry-pick, a revert or `--base` whose base is not where a side's commits start replays that side's commits
  from B and can stage a `HierarchyCycle`, even when the picked commit has no hierarchy entry;
  (iii) undoing a move that left its node where it was stages that key, although both sides hold its value.
  The reference model follows RS-007 as written, and no engine code implements it yet. A fix changes merge results, so
  the arbiter left it to the owner; Option B (RS-007 as written) is in force.
- **Options.** (a) Option A, with (i) in its narrow form. (i) A two-parent commit is also a step for the hierarchy
  keys where its state differs from its second parent's, limited to keys that a commit of A(p₂) \ A(p₁) moved. (ii) A
  revert or a cherry-pick starts from o's (parent, order), dst has no step, and src's one step (C's) leaves out each
  key that a dst commit after C, by (hlc, commit id), set. (iii) A move that sets its node's current value is never
  undone and does not make its key `kleppmann-skipped`. `--base` and a virtual base have no proposal yet and stay
  recorded in open point 35. (b) Option A with (i) in its broad form (every key where the state differs from the
  second parent's). This re-keys a side's own earlier moves to the merge commit's (hlc, commit id), so a routine sync
  can make the side's older move beat a third branch's later one, against MR-040's "later move wins". (c) Option B:
  keep RS-007; the three cases stage for the user to resolve, or, in case (i), land a value nobody chose.
  Cost of (a) or (b): R-MODEL changes RS-007, open point 35 and the model's step lists (`vcs.rs` `move_steps`), pick
  replay (`history.rs`) and undo (`merge.rs` `kleppmann`); R-SPEC-F changes [F12 §7.4]'s row; three tests flip
  (`suite::vcs::a_sync_resolved_to_ours_then_merged_stages_the_undone_lane_move`,
  `suite::vcs::a_cherry_pick_replays_dsts_moves_from_its_base`,
  `merge::tests::a_cycle_closing_step_undoes_its_no_op_move_of_a_lower_uid`), and the property
  `suite::vcs::a_linear_lane_merged_into_an_unmoved_main_takes_its_hierarchy` widens. The engine computes one extra
  hierarchy diff per two-parent commit in a side's range.
- **Recommendation.** (a). Case (i) can produce a wrong value with no staging, and correctness is a binding priority.
  Its spurious staging sits on the daily sync-then-merge path and costs agent tokens each time it occurs. (ii) and
  (iii) are contained fixes that leave every other result unchanged, and the narrow form of (i) keeps MR-040's winner
  against a third branch. Decide it no later than the merge table's V3 signature (E1; delegated, `rules/SIGNED.md`),
  and in any case before the engine implements RS-007, so that neither the signature nor engine code is redone.
