# Owner questions raised while closing review pass 1

| | |
|---|---|
| Title | Questions that need the owner's call, raised by the author roles while closing review pass 1 |
| Status | **decided 2026-09-28**: the owner accepted every recommendation ("Подтверждаю все", "I confirm all"): OQ-R-1 (a), OQ-R-2 (a) with (b) as the fallback, OQ-R-3 (a), OQ-R-4 (a), OQ-P-1 (a), OQ-P-2 (a), OQ-P-3 (a), OQ-F-1 (a), OQ-F-2 (a), OQ-F-3 (b), OQ-M-1 (a) (the rows are accepted; the owner's signature in `rules/SIGNED.md` follows under V3), OQ-M-2 (a). WP-81a edits the [AR], [60] and [50] texts each entry names. **Open:** OQ-F-4, raised in spec sync 2a after that decision. |
| Scope | only questions that change an approved design decision or need the owner's call; runtime policy goes to config keys ([AGENTS.md]) and is not listed |

Each entry names the raising role, the findings, the options and the role's recommendation. Roles append entries; ids
carry the role letter (R = R-SPEC-R, P = R-SPEC-P, F = R-SPEC-F, M = R-MODEL).

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
