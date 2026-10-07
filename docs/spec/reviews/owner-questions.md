# Owner questions raised while closing review pass 1

| | |
|---|---|
| Title | Questions that need the owner's call, raised by the author roles while closing review pass 1 |
| Status | **decided 2026-09-28**: the owner accepted every recommendation ("Подтверждаю все", "I confirm all"): OQ-R-1 (a), OQ-R-2 (a) with (b) as the fallback, OQ-R-3 (a), OQ-R-4 (a), OQ-P-1 (a), OQ-P-2 (a), OQ-P-3 (a), OQ-F-1 (a), OQ-F-2 (a), OQ-F-3 (b), OQ-M-1 (a) (the rows are accepted; the owner's signature in `rules/SIGNED.md` follows under V3), OQ-M-2 (a). WP-81a edits the [AR], [60] and [50] texts each entry names. **Decided 2026-10-06**: the owner accepted every recommendation ("Зафиксируй везде рекомендуемые варианты", "record the recommended options everywhere"): OQ-F-4 (a), OQ-A-1 (a), OQ-A-2 (a), OQ-A-3 (a), OQ-A-4 (a), OQ-A-5 (a), OQ-A-6 (a), OQ-A-7 (a), OQ-A-8 (c), OQ-A-9 (a), OQ-A-10 (a); each entry's Decision line names the follow-up. OQ-A-11, raised by the review of wave 3b's RS-007 change, was decided the same day as recommended ("Согласно рекомендации запиши", "record it as recommended"): 11.1 (A) together with (B), (A) alone as the fallback; 11.2 (a); 11.3 (a); applied in wave 3c. OQ-A-12, raised by wave 3c's spec arbiter, was decided the same day as recommended ("Согласно рекомендации запиши", "record it as recommended"): (b) together with (c), prototyped in wave 3d, (d) as the fallback. Its prototype did not meet the acceptance (wave 3d). **Open**: OQ-A-13, raised 2026-10-07 by the comparison of alternatives that followed wave 3d. |
| Scope | only questions that change an approved design decision or need the owner's call; runtime policy goes to config keys ([AGENTS.md]) and is not listed |

Each entry names the raising role, the findings, the options and the role's recommendation. Roles append entries; ids
carry the role letter (R = R-SPEC-R, P = R-SPEC-P, F = R-SPEC-F, M = R-MODEL, A = R-REV-A, which merged the owner
items of spec sync 2b; the A series also carries the orchestrator's questions of 2026-10-06, OQ-A-7 to OQ-A-10).

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

## OQ-F-4 — Three additions to the closed `unverified` set (decided 2026-10-06; [F18] open point 12)

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
- **Decision (2026-10-06).** (a). The three additions are signed: `unreadable` (59), `unmapped root` (60) and
  `oid algorithm differs` (61). WP-81a adds them to [40 §2.9]; no byte or rule of a spec chapter changes. The next spec
  sync marks them signed: R-SPEC-F in [F18 §4.6] and [F18] open point 12, R-SPEC-R in [F20 §1.5] and [F20] open
  point 22.

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
- **Decision (2026-10-06).** (a). E4 reads "… the 13 of [80 §2.4.4] (G5 by a unit test of its detector, which the flush
  byte masks, [F16 §17.3] P-45) …", with the general rule: a catalogue bug that another rule masks in every reachable
  toy state carries a written disposition in its [F16 §17.3] row (re-vehicled to a milestone gate, re-formed into a
  reachable bug, or covered by a unit test of its detector), accepted at WP-80. P-59 and P-79 stay at M1. The plan issue
  of 2026-10-06 ([PLAN §5]) amends E4, and WP-81a edits [80 §2.4.4] (W-7). In wave 3b's spec sync, R-SPEC-P replaces
  the "OWNER O-1" markers in [F16]'s open points with this decision.

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
- **Decision (2026-10-06).** (a), adopted by the plan issue of 2026-10-06 ([PLAN §5]): R-HARN-S's outputs (WP-32) gain
  the generic predicates (I-G4, I-G6), the ns check and the avail verdict, a WP-32 follow-up that WP-40's acceptance
  depends on; WP-40's acceptance gains "R-HARN-S reviews the toy's own checks"; S4 gains "the detectors are the
  enumerator author's, or reviewed by that author". Wave 3a already did this in code: the enumerator took the generic
  checks, and R-HARN-S reviewed the toy's three own checks. In wave 3b's spec sync, R-SPEC-P replaces the "OWNER O-2"
  markers in [F16 §17.2] and its open points and in [F13 §1.4] and OP-13-02 with this decision, and brings their
  statements of S4 ("S4 names only the enumerator's author") to the amended S4.

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
- **Decision (2026-10-06).** (a). One plan issue, that of 2026-10-06 ([PLAN §5]), applies items (a)–(e) and (g) exactly
  as stated above; R-HARN-I makes item (f), the two `[profile.dev.package.*]` opt-levels in the root manifest, in
  wave 3b.

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
- **Decision (2026-10-06).** (a). LE-012 stays; WP-81a edits [AR §6.2] (W-5).

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
- **Decision (2026-10-06).** (a). P3 is the port phase's. PLAN WP-61's deliverable reads "P1, P2 and P4–P12 as pure
  functions (P3 is the port phase's, [OS/path §3] and open point 2)" and its acceptance "the `r4/` fixtures pass, except
  `paths.cases` p3-01 … p3-06, which no M0–M11 crate runs; the macOS port runs them" (the plan issue of 2026-10-06,
  [PLAN §5]). In wave 3b's spec sync, R-SPEC-P adds the [OS/path §1] note, R-FIX marks the six cases "port phase"
  (p3-06 provisional) in `fixtures/r4/INDEX.md` §3.2 without changing a byte, and WP-81a edits [80 §5.4]'s M0 row to
  "P4–P12". Open point 2 stays open.

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
- **Decision (2026-10-06).** (a): RS-007 takes Option A, with case (i) in its narrow form and (ii) and (iii) as stated.
  In wave 3b, before the merge table's V3 signature and before the engine implements RS-007, R-MODEL changes RS-007,
  open point 35 and the model (`move_steps`, pick replay, `kleppmann` undo; three tests flip, one property widens), and
  R-SPEC-F changes the [F12 §7.4] row.
- **Outcome (wave 3b).** The narrow form is in the rules, [F12 §7.4] and the model, but only two of the three named
  tests flipped: the cherry-pick of (ii) and the no-op move of (iii). The third case (E2) still stages, and the review
  found a worse one (E1, a default `merge --into main` with no resolution). The recommendation's premise, that Option A
  fixes the daily sync-then-merge path, did not hold; OQ-A-11 below carries the rest.

## 2026-10-06, owner questions asked in chat

Raised by the orchestrator from the TencentDB Agent Memory research of 2026-09-30 and the LLM-principle discussion, and
asked in chat on 2026-10-06 together with the entries above that were still open. The owner decided all of them in one
answer (the Status row). The ids continue the A series.

### OQ-A-7 — Dropping bodies by hash (owner decision #33) before the format freeze

- **Raised by** the orchestrator, from the TencentDB Agent Memory research of 2026-09-30. **Where** [AR §11] decision
  #33 (row "Erasing history", milestone M0); [F06 §8] (the bodies a commit carries, keyed by `hash`), [F06 §4.4.15]
  (`pruned`, the header-only form); [F07 §6.3] (the body key); [F08 §7.2]; [F09 §6.3] `BLOBTAB`; [F10 §4.6] (the `gc`
  rewrite).
- **Question.** Decision #33 reads "no erase: retract and rotate; bodies droppable by hash without changing commit ids".
  The format chapters address a body by its BLAKE3-128, which is what the canonical form hashes ([F06 §8], [F07 §6.3],
  [F08 §7.2]), but none specifies dropping one: the only removal they define is `gc`'s header-only form of an
  unreachable commit, which drops its whole changeset ([F06 §4.4.15], [F10 §4.6]). No record says that a body was
  dropped and no reader rule renders a dropped body, so the one removal #33 allows has no bytes in the format that
  freezes at M0.
- **Options.** (a) The spec roles specify it before the format freeze: bodies addressed by hash, a record that a body was
  dropped, readers render the dropped body, commit ids unchanged. (b) Defer it to a later format version; until then no
  body can be dropped.
- **Recommendation.** (a). #33 is already decided, and its mechanism is format: added after the freeze it is a format
  change, which older readers refuse ([AR §12] row "Auto-migration of the on-disk format on open"). The canonical form
  already hashes a body's BLAKE3-128, not its bytes ([F07 §6.3]), so (a) can keep commit ids unchanged as #33 requires.
- **Decision (2026-10-06).** (a). The spec roles specify it before the format freeze: bodies addressed by hash, a record
  that a body was dropped, readers render the dropped body, commit ids unchanged. (b), deferring it to a later format
  version, was rejected.

### OQ-A-8 — The status of owner-authority knowledge and the visibility of proposed records

- **Raised by** the orchestrator, from the TencentDB Agent Memory research of 2026-09-30. **Where** [AR §7.1] (the sample
  session: `moirai rule --critical … --authority owner --owner-quote-file ruling.txt` →
  `#212 rule active critical … by orchestrator`); [AR §7.3] (orchestrator row: "`authority = owner` only with
  `--owner-quote`"); [AR §3.2] (`rule`: `proposed < active`; `decision`: `proposed < accepted`); [AR §3.6] (knowledge:
  `proposed|draft → active|accepted|current`); [AR §7.4] (the brief); [RULES/status-machines] ST-015, ST-020, GR-006;
  [RULES/pack-classes] PT-011 `auth(n)`, BR-001 to BR-011.
- **Question.** Knowledge starts at `proposed`: it is the one initial status of a `rule` and of a `decision` (ST-015,
  ST-020; [AR §3.6]). A proposed record is invisible: no brief class lists it (BR-001 to BR-011), and the pack classes
  that take knowledge require auth(n), which holds only for `rule` active, `decision` accepted, `note` active and `doc`
  current (PT-011). [AR §7.1]'s example, however, writes an owner-authority rule through the orchestrator straight to
  `active` (#212; GR-006 lets a `Create` name a later status as a checked path of transitions). No text says which
  writer may start a record active, and nothing shows a proposed record to a reviewer, so the review queue that the
  capture pipeline of OQ-A-10 feeds would be invisible.
- **Options.** (a) A fixed rule: owner-authority knowledge written by the orchestrator with an owner quote is `active` at
  once; every other knowledge write starts `proposed`. (b) Every knowledge write starts `proposed`, owner-authority
  writes included, until the owner confirms. (c) A config key that the spec names, with (a)'s rule as its default and a
  strict value that gives owner-authority writes (b)'s behaviour; the brief gains a "proposed / needs review" line, and
  packs do not hide proposed records.
- **Recommendation.** (c). Which writes start active is policy that a project may tighten, so it belongs in a config key
  with a default ([AR §11]). The default keeps [AR §7.1]'s owner ruling in force at once, behind the owner quote that
  [AR §7.3] already requires, while every other agent's or subagent's write waits for review; the brief line and visible
  proposed records make that review reachable.
- **Decision (2026-10-06).** (c): a config key (the spec names it; default as follows): owner-authority knowledge
  written by the orchestrator with an owner quote is active at once; knowledge written by any other agent or subagent
  starts `proposed`; a strict value makes owner-authority writes `proposed` until the owner confirms. The brief gains a
  "proposed / needs review" line, and packs do not hide proposed records. (a), a fixed default, and (b), always
  proposed, were not taken. It is recorded in the next spec sync, before the format freeze (the brief line is one of
  OQ-A-10's pre-freeze reservations): R-SPEC-F writes the key's WP-18 registry row, [AR §7.4] and the brief text;
  R-MODEL changes the rows of [RULES/status-machines] (ST-015, ST-020, GR-006), [RULES/pack-classes] (PT-011's `auth(n)`
  filter, the BR rows) and `rules/policy-keys`, which then need a fresh V3 signature (E1), and the M0 model implements
  every allowed value of the key (E8, WP-90). For that spec sync: the wording above leaves the orchestrator's own
  knowledge writes without an owner quote unstated; option (c) as asked takes (a)'s rule as its default, whose "every
  other knowledge write starts `proposed`" covers them.

### OQ-A-9 — A provenance field in the commit header

- **Raised by** the orchestrator, from the TencentDB Agent Memory research of 2026-09-30 and the LLM-principle
  discussion. **Where** [F06 §4.2] (the presence bitmap; bits 19–31 reserved-zero), [F06 §3.5] `actor_src`; [90 §4.1]
  (the resolver's Actor and Model rows), [90 §4.2]; [AR §4.3].
- **Question.** A commit records its `actor` and `actor_src`, the row of [90 §4.1]'s actor order that supplied the actor
  ([F06 §3.5]), but neither the kind of actor that wrote it nor a model. [90 §4.1] already resolves a model for every
  call (the run node, the lease, the marker, `--model`/`MOIRAI_MODEL`, a hook's `model` field, then a client default),
  declared and not attested, and stores it nowhere. Once the pipeline of OQ-A-10 writes `proposed` records with
  provenance, a reviewer needs both. The header's presence bits 19–31 are reserved-zero ([F06 §4.2]), so a group added
  before the format freeze takes no bytes while absent; added later it is a format change, as [90 §4.2] says of
  `actor_src`.
- **Options.** (a) Reserve before the format freeze a provenance field in the commit header: the actor kind and the
  declared model id, declared and not attested ([90 §4.1]); filled from M9–M10. (b) None: provenance stays `actor` and
  `actor_src`.
- **Recommendation.** (a). The reservation costs one presence bit now and no byte in a commit that leaves it absent, and
  it avoids a format change when M9–M10 build the pipeline. Like [90 §4.1]'s model, the field is declared: it informs a
  review and grants no rights, which come only from a presented lease ([90 §4.3]).
- **Decision (2026-10-06).** (a). The spec roles reserve the field in the commit header before the format freeze, in the
  next spec sync; M9–M10 fill it. (b), none, was rejected.

### OQ-A-10 — The LLM principle: who does the LLM-shaped work

- **Raised by** the orchestrator, from the LLM-principle discussion and the TencentDB Agent Memory research of
  2026-09-30. **Where** [AR §12] (rows "Any background activity: polling, file watchers, timers, auto-compaction on a
  schedule, telemetry, network calls" and "LLM-driven rewriting of stored facts"); [AR §7.5] (skills and hooks);
  [AR §7.8] and [90] (decision #43); [AR §7.1] `apply --from` with `result.v1` records ([90 §7.2]); OQ-A-8, OQ-A-9.
- **Question.** A memory needs work that only a model can do: capturing decisions, rules and findings at the end of a
  task, harvesting old transcripts in batches, and curating similar records and the proposed queue. [AR §12] rules out
  network calls and LLM rewriting of stored facts, but no text says who does this work or how its output enters the
  store. The TencentDB Agent Memory project runs such work in an LLM pipeline inside its server. moirai could call a
  model itself, through an API or command executor that a config key switches off, or leave the work to the harness's
  agents with shipped instructions.
- **Options.** (a) The LLM principle: moirai is the safe substrate (typed, validated, versioned, crash-safe, idempotent
  commands; leases; branches; statuses; file links; packs and briefs) and never calls an LLM or the network itself; its
  code has no API or command executor (an API path, if ever needed, is a documentation recipe). The harness's agent
  pipeline does the LLM-shaped work (capture, batch harvesting, curation) through shipped skills and instructions that
  spawn subagents, inline or headless where a harness has no subagents; its output enters only as normal writes with
  status `proposed` and provenance. (b) An API or command executor in moirai's code that runs the same jobs, behind a
  config key.
- **Recommendation.** (a). It keeps [AR §12]'s no-network row and the pure-Rust rule whole, works in every harness
  (decision #43) with no API key or billing, and leaves moirai the part it can check: typed, versioned writes whose
  `proposed` status and provenance (OQ-A-8, OQ-A-9) keep the pipeline's output reviewable.
- **Decision (2026-10-06).** (a), recorded as [AR §11] decision #46; (b) was rejected. [AR §12]'s ban on LLM rewriting of
  stored facts stays: pipeline output enters only as normal writes with status `proposed` and provenance. Pre-freeze
  reservations for the next spec sync: the "proposed / needs review" brief line (OQ-A-8), the provenance field
  (OQ-A-9), deterministic work-list named queries (`std.similar`, stale-link rules, rules without `applies_to`,
  needs-triage notes, long bodies without an abstract, unharvested transcripts), a harvest cursor (transcript file plus
  byte range done) and a curation-task contract (input bundle, answer schema, submission via `apply --from` /
  `result.v1`); implementation M9–M10. The next spec sync also takes, without an owner question: `PATCH` refuses an
  ambiguous `$old` (it must occur exactly once; an empty `$old` is refused; an E404 reason "occurs N times"), a
  deterministic "similar" notice after knowledge writes, and the token-usage rule (input + cache_read + cache_creation;
  never double-count Codex cached input).

## 2026-10-06, wave 3b

Raised by the orchestrator from the review and closure check of wave 3b's RS-007 change (review RS-007-A), from
R-MODEL's open point 35 (v) and (vi) in `rules/merge-table.md`; asked in chat with the options below and decided the
same day.

### OQ-A-11 — The limit of RS-007's narrow form, order-only moves and the reading of "moved" (open point 35 (v), (vi))

- **Raised by** the review and closure check of RS-007 Option A (wave 3b), from R-MODEL's spec findings. **Where**
  [RULES/merge-table] RS-007, MR-039, MR-040, CS-013, PR-016, open points 15 and 35 (v), (vi); [F12 §7.1], §7.4 row
  "Kleppmann steps"; [AR §3.4] I25′; VB-017 (step 0's sync before a merge into `main`); OQ-A-6.
- **Question.** OQ-A-6 (a) took Option A in its narrow form to fix the daily sync-then-merge path; two of its three
  named tests flipped, the third did not. Three calls remain:
  (11.1, open point 35 (v)) A two-parent commit re-asserts only the keys its second parent's branch moved, so a key it
  kept from its first parent is not re-asserted when the replay undoes that side's move of it. E1: lane/x puts #2 under
  #3, then #1 under #2, then #2 back at the root; main puts #3 under #1; a default `merge lane/x --into main` with no
  resolution stages `#1.parent HierarchyCycle`, although main never moved #1. E2: the sync-resolve-merge path of OQ-A-6
  (i) still stages `#2.parent`. E3: "sync, resolve, keep working, sync" stages the same key on a two-sided merge. E4: a
  history whose commits all descend from B stages after a merge between lanes resolved to `ours`.
  (11.2, open point 35 (vi)) A move that only reorders a node under the same parent counts as a change and can be
  undone on a cycle, although it cannot close one; it then stages keys both sides agree on.
  (11.3, open point 35 (i)) Whether "moved" in the narrow form means a direct move only, or "is a step key of",
  recursively through nested merges (a sync on top of a sync).
- **Options.**
  11.1: (A) when tip(dst) is B, every hierarchy key takes src's value without a replay (a one-sided merge, as I25′ asks);
  covers E1, E2 and E4, not E3. (B) start the replay from a commit that every commit of A(o) \ A(B) and of A(t) \ A(B)
  descends from, and replay both sides' commits since it, so no commit is replayed on a state it was not made on;
  covers E1, E2 and E3, not E4. (C) the broad form of (i): covers all four, but a routine sync can make a side's older
  move beat a third branch's later one, against MR-040. (D) record the keys a merge's resolutions set with the commit and
  use them with (B): covers all four, at the cost of a format addition.
  11.2: (a) the undo takes only the step's moves that changed their node's parent; a move that keeps its parent and
  changes only its order closes no cycle, is never undone and never makes its key `kleppmann-skipped`. (b) keep the
  undo over every changed value.
  11.3: (a) the recursive reading ("is a step key of"); PR-016 and [F12 §7.1] then name the history the step keys read
  (each side's commits since the base, every commit of A(p₂) \ A(p₁) of a two-parent commit among them, recursively, and
  for a revert or a cherry-pick the commits of A(o) after C). (b) direct moves only, which gives wrong results under
  nested syncs.
- **Recommendation.** 11.1 (A) together with (B): between them they cover E1 to E4 without re-keying a side's moves and
  without a format addition; R-MODEL states (B) exactly (the commit the replay starts from, chosen over a criss-cross as
  I31′ chooses a base) and a spec arbiter checks it before the merge table's V3 signature; (A) alone if (B) cannot be
  stated by then, leaving E3 recorded in open point 35. 11.2 (a). 11.3 (a).
- **Decision (2026-10-06).** As recommended ("Согласно рекомендации запиши", "record it as recommended"): 11.1 (A)
  together with (B), with (A) alone as the fallback; 11.2 (a); 11.3 (a). Wave 3c applies it before the merge table's V3
  signature and before the engine implements RS-007: R-MODEL rewrites RS-007, MR-039, CS-013, PR-016 and open point 35
  and changes the model (the one-sided merge, the replay start of (B), the parent-only undo, the recursive step keys)
  with tests for E1 to E4 and the order-only case; R-SPEC-F changes [F12 §7.1] and the §7.4 row; a spec arbiter checks
  (B)'s statement.
- **Outcome (wave 3c).** Applied as decided, with (B) and not the fallback (`spec-sync-3c.md`). The replay start R is
  the greatest commit of A(B) ∩ A(o) ∩ A(t) that every other commit of A(o) ∪ A(t) descends from or is an ancestor of
  (ε when none is): such commits form a chain, so R is unique, and no choice over a criss-cross arises, since R lies
  below every LCA. The spec arbiter ruled the statement exact and its use for `--base` and the virtual base (open point
  35 (iv)) within the decision (`wave-3c-arbiter.md`). E1 to E4 and the order-only case land; with (A) switched off in
  the model, E1 to E3 land through (B) and E4 stages, as expected. Three findings remain for the owner: OQ-A-12 below.

## 2026-10-06, wave 3c

Raised by the orchestrator from the spec arbiter's check of wave 3c (`wave-3c-arbiter.md` W3C-ARB-1 to W3C-ARB-3) and
from R-MODEL's open case E5 ([RULES/merge-table] open point 35 (v)); asked in chat.

### OQ-A-12 — Spurious hierarchy stagings that OQ-A-11 leaves: E5, E6, and a key equal in b, o and t

- **Raised by** R-MODEL (E5) and the wave 3c spec arbiter (W3C-ARB-1 to W3C-ARB-3). **Where** [RULES/merge-table]
  RS-007, open point 35 (v) E5 and E6; [F12 §7.2], §7.4 row "Kleppmann steps"; [AR §3.4] I25′; GT6 (M3); the model
  tests `suite::vcs::a_sync_after_a_merge_resolved_to_ours_stages_the_kept_move`,
  `suite::vcs::a_sync_after_a_sync_that_kept_the_lanes_later_move_stages_it_on_every_sync` and
  `suite::vcs::a_cross_lane_merge_after_a_resolved_sync_stages_a_key_all_three_states_hold`, which pin the results.
- **Question.** After OQ-A-11, three kinds of history still stage a `HierarchyCycle` on a key that one side, or no
  side, changed since the base, against I25′, which GT6's property (M3) checks:
  (E5) a `sync` after a resolution to `ours` kept a side's cycle-closing move: after a cross-lane merge resolved to
  `ours` ("merge another lane, resolve, sync"), and on "sync, resolve to `ours`, sync" when the lane's own move is the
  later one, where every later sync of the lane stages the key again until the lane merges into `main`. The replay
  meets again the cycle that the earlier resolution settled, and nothing re-asserts the kept move: the earlier merge's
  step re-asserts only the keys the merged branch moved.
  (E6) a cross-lane merge, after a sync whose cycle was resolved, stages a key that b, o and t all hold, with a value
  none of them holds; the replay from B (the rule before wave 3c) landed it. The arbiter's random search found three
  such histories in 1,950, against nine that (B) lands and the replay from B stages.
  (W3C-ARB-3) [F12 §7.2] says that a key equal in all three keeps its value; RS-007 decides every hierarchy key. E6 is
  where the two differ, and two engines must not read them differently. Wave 3c made the texts name the exception, so
  the specification now says RS-007 wins; that is an interim reading, not a decision.
- **Options.**
  (a) Keep RS-007 as applied: E5 and E6 stay, as spurious stagings that the user resolves; I25′ is amended to exclude
  hierarchy keys outside a one-sided merge, and GT6's I25′ property skips them.
  (b) A hierarchy key equal in b, o and t keeps its value and is never `kleppmann-skipped`; the replay decides the
  other hierarchy keys, and the validators' cycle check (I37′) is the backstop for a cycle the kept values close.
  Covers E6 and W3C-ARB-3, not E5; it changes RS-007's results.
  (c) A merge or `sync` commit is also a step for each hierarchy key that its resolutions set, the keys where its state
  differs from the candidate its own merge computed, derived from the commit graph by recomputing that candidate (no
  format addition; each commit's derived keys are computed once and kept, as step keys are). Covers E5 in both shapes,
  not E6. A recomputed candidate depends on the merge rules of the format version, which is fixed within it.
  (d) As (c), but the keys are recorded with the commit (OQ-A-11's option (D)): a format addition, so it must be decided
  before the format freeze (WP-81b) or wait for a later format version. Covers what (c) covers.
  (e) The broad form of OQ-A-6 (i) (OQ-A-11's option (C)): covers E5 and E6, but a routine `sync` can make a side's
  older move beat a third branch's later one, against MR-040; rejected twice already.
  (b) and (c) combine.
- **Recommendation.** (b) together with (c), prototyped in the reference model in wave 3d before the merge table's V3
  signature, with the arbiter's lockstep random search (wave 3c rule against the replay from B) as the acceptance: E5
  in both shapes and E6 land, no history stages that the replay from B lands, and the cycle backstop of (b) is never
  needed in the search, or its rule is stated. If (c) cannot be made exact (for example, if recomputing a commit's own
  candidate is not well defined for some commit kind), fall back to (d) before the format freeze. (a) leaves spurious
  structural stagings on two daily paths and weakens an invariant, against correctness and agent tokens, the binding
  priorities.
- **Decision (2026-10-06).** As recommended ("Согласно рекомендации запиши", "record it as recommended"): (b) together
  with (c), prototyped in the reference model in wave 3d before the merge table's V3 signature, with the arbiter's
  lockstep random search as the acceptance: E5 in both shapes and E6 land, no history stages that the replay from B
  lands, and the cycle backstop of (b) is never needed in the search, or its rule is stated. If (c) cannot be made
  exact, the fallback is (d), decided before the format freeze (WP-81b). (a) and (e) were not taken. Wave 3d: R-MODEL
  changes RS-007, open point 35 and the model, with the E5 and E6 tests flipped and the search as a model test;
  R-SPEC-F changes [F12 §7.2] and the §7.4 row; a spec arbiter re-runs the search and checks the statement.
- **Outcome (wave 3d).** The prototype does not meet the acceptance (`wave-3d-verify.md`). E5 in both shapes and E6
  land, but three classes of history stage where the replay from B lands, the backstop fires at the nightly tier, and
  (c) depends on the cache. A comparison of alternatives on one evaluation harness (`wave-3d-alternatives.md`) found
  that every replay variant leaves avoidable stagings and silent wrong landings, that E6 has no forest, and that the
  relative criterion cannot be met by a correct rule: OQ-A-13 below. The model keeps the prototype as its default rule
  until OQ-A-13 is decided.

## 2026-10-07, wave 3d

Raised by the orchestrator from the wave 3d verification of the OQ-A-12 prototype (`wave-3d-verify.md`) and from the
comparison of alternatives that followed it (`wave-3d-alternatives.md`); asked in chat.

### OQ-A-13 — Replace RS-007's replay for hierarchy keys (OQ-A-12's acceptance is not met)

- **Raised by** the wave 3d verification, which found OQ-A-12's acceptance unmet, and the comparison of three
  alternatives on one evaluation harness (`suite::rs007eval`: 12,000 generated histories and a 41-case corpus, judged by
  a three-way oracle on states that no rule computes), two judges and a completeness critic. **Where**
  [RULES/merge-table] RS-007, MR-039, MR-040, CS-013, PR-014, PR-016, PR-017, VB-011, VA-001, VA-005, the HT rows, open
  points 15 and 35; [F12 §5.3] VM-7, §7.1, §7.2, §7.4; [F13 §5] V01, V05; [AR §2.7] T7, [AR §3.4] I25′, [AR §5a.7]
  steps 3 and 4, [AR §5a.8]; [60] GT6 (M3); OQ-A-6, OQ-A-11, OQ-A-12.
- **Question.** RS-007 decides each node's (parent, order) by replaying each side's commits in (hlc, commit id) order and
  undoing the moves that close a cycle. Every variant measured leaves two classes of fault: wave 3c's rule (the text in
  force), the OQ-A-12 prototype (b)+(c) that the model runs, and the replay from B. In 12,000 histories they stage 577,
  581 and 800 `HierarchyCycle`s where a forest keeps every value only one side changed (21 % to 27 % of all such
  stagings), and silently land 1,267, 1,265 and 985 keys that only one side changed at another value. On a wider
  generator (6 tasks, multi-statement transactions, base and abort resolutions) the avoidable share is 33 % to 37 %.
  OQ-A-6, OQ-A-11 and OQ-A-12 each fixed the cases they named, and the search found new ones each time. OQ-A-12's
  acceptance cannot be met as written. E6 has no forest: every value it could land drops a move only one side made. And
  "no history stages that the replay from B lands" fails for any correct rule, because those histories are the replay
  from B's own wrong landings. Six calls: 13.1 the rule; 13.2 what "touched" means in I25′ for hierarchy keys, and with
  it E6; 13.3 a two-sided key's later move that would close a cycle (MR-039); 13.4 what "later" means in MR-040; 13.5 a
  reparent against a concurrent order-only move; 13.6 the acceptance.
- **Options.**
  13.1, the rule:
  (a) **K2 `threeway`, no replay.** A key takes o's value when o = t, t's when o = b, o's when t = b. When both sides
  changed it to different values (a two-sided key), the value with the later *origin* wins (MR-040). The origin is the
  (hlc, commit id) of the commit that produced the value: the walk along first parents passes over commits that leave
  the key unchanged, and over a two-parent commit that holds its second parent's value, so a `sync`, a merge into `main`
  and a resolution to ours or theirs pass it on. Cycle repair: while no choice for the two-sided keys gives a forest,
  one key on the unavoidable cycle that is not two-sided is reset to b and staged (MR-039; tie-breaks: for a revert or a
  cherry-pick a key only C changed, a one-sided key before a both-same one, a key whose b parent leaves the cycle, the
  latest origin, the least uid); then each two-sided key keeps its later value unless no forest remains, and otherwise
  lands at the other side's value. Evidence: in 12,000 histories 1 oracle fault (a real cycle through a node the oracle
  does not judge), 0 wrong landings, 0 stuck stagings, every corpus case as expected; on the wider generator 1 fault of
  the same kind against 1,276, 1,275 and 1,107 for the three replays; a `sync` takes 3.9 ms at 300 rounds of a long
  lane and 9.7 ms at 1,000, of which the rule's own work is about 80 µs and flat. No replay start, no step keys, no
  derived index, no format addition, the shortest text. Cost: it leaves the letter of T7, "Kleppmann move in HLC
  order", while Kleppmann's guarantees hold over net values (a forest results, the later move wins, a cycle-closing
  later move is skipped and logged). Seven pinned model tests and the two relative lockstep tests are rewritten. It
  supersedes OQ-A-6 (a) (i) to (iii), OQ-A-11 11.1 (B) and 11.3, and OQ-A-12 (b) to (d); OQ-A-11 11.1 (A) and 11.2
  hold by construction. The engine needs each side's per-node first-parent chain and an as-of read at second parents.
  The phrase "the keys that lie on a cycle within U" must be defined as the cycle of the option graph before V3: two
  readings diverge in 472 and 88 of about 49,000 decisions.
  (b) **K4 `fromb`**: the replay from B, with a net-change filter, one-sided shortcuts on states and repairs bounded at
  three flips. Evidence: 2 faults in 12,000 (one real: a deleted node read as a root), 43 stuck stagings, 364 later
  two-sided moves dropped where the later value fits; on the wider generator 3 faults and 26 stuck; a `sync` takes
  9.9 ms at 300 rounds and 32.6 ms at 1,000, growing with the lane's life. Step keys stay. It reverses OQ-A-11 11.1
  (B); its repair bound and tie order are heuristics.
  (c) **K3 `replayfix`**: RS-007's replay from the replay start, seven repairs, and a forest step after them, without
  which no replay meets the oracle. Evidence: 2 faults (one real), 0 stuck; on the wider generator 3 faults and 3 stuck;
  a `sync` takes 151 ms at 300 rounds and 547 ms at 600, so W3C-ARB-10's bound is not met. About ten clauses; 73 later
  moves dropped where they fit.
  (d) Keep RS-007 as wave 3c left it and amend I25′ to leave out hierarchy keys outside a one-sided merge (OQ-A-12
  (a)). Evidence: 1,846 faults in 12,000, 1,265 of them silent wrong landings.
  13.2, "touched" in I25′: (a) by state: a key is touched on side S when S's value differs from b's. E6 then stages as a
  genuine cycle (K2 stages `#4.parent`, and `--take theirs` resolves it), and I25′ for hierarchy keys reads "a merge
  stages a `HierarchyCycle` on a key untouched on one side only when no forest keeps every such key at the other side's
  value", which GT6 (M3) checks. (b) by changeset: a key is also touched when a commit of the side since the base has it
  in its net changeset (a move away and back, a sync that re-asserts a value). No candidate meets (b): 269 to 273
  avoidable stagings each; the replays that land E6 do it by dropping lane/y's move, which only lane/y made.
  13.3, a two-sided key whose later value would close a cycle that no other choice avoids, or whose parent the result
  does not hold live: (a) it lands at the other side's value, and the reply carries a hint (a new HT row, a log line,
  not stored); under K2 this happens to 442 keys in 12,000 histories (301 cycles, 141 dead parents). (b) as (a), and
  under `--strict` such a key stages. (c) every such skip stages, as MR-039 reads today: about 364 more avoidable
  stagings per 12,000.
  13.4, "later" in MR-040: (a) original origins: a value keeps the (hlc, commit id) of the commit that produced it; a
  `sync`, a merge into `main` and a resolution to ours or theirs pass it on; a move, a revert, and a resolution to base
  or to a new value produce a new one. In the CONTESTED cases (ADV-C-6, W3D-REV-5) a lane's earlier move loses to a
  third branch's later one. This is MR-040's own sentence: "a side's own earlier move keeps its own (hlc, commit id)
  against a third branch". (b) a resolution is a move at the resolving commit's time (OQ-A-12 (c)'s reading). (c) the
  narrow form's re-assertion (OQ-A-6 (a) (i)), which re-times the keys a two-parent commit kept.
  13.5, one side changes only a node's order and the other changes its parent: (a) whole value, as every rule does now:
  the later of the two wins; in 7,500 histories about 190 reparents, 59 % of such keys under every rule, were lost to a
  later reorder with no staging and no hint. (b) a change of parent beats a concurrent order-only change, whatever their
  times, and the node takes the reparenting side's (parent, order); the oracle allows either value, so no fault count
  changes. (c) as (a), with a hint.
  13.6, the acceptance: (a) absolute, on the harness's oracle at the nightly and exit tiers: 0 avoidable `HierarchyCycle`
  stagings, 0 wrong landings, 0 stuck stagings; before V3 the oracle is widened to judge every node live in the result,
  with absent as a value (today 14,311 of about 68,400 merges hold an unjudged task, and every remaining fault of every
  candidate lies there), and the generators gain existence policies, `blocks` and `gates` edges, re-key and more tasks
  (`wave-3d-alternatives.md` ALT-4); the relative lockstep tests are retired. (b) keep the relative criterion; no
  correct rule can meet it.
- **Recommendation.** 13.1 (a), with the exactness fixes and nothing grafted that has not been run: the cycle phrase
  defined as the cycle of the option graph; a both-same key's origin stated; absent compared canonically in the origin
  walk; a cycle with no repairable key sent to a validator that checks cycles, with VA-005's "cycles are VA-001's"
  corrected. If a literal replay must stay, (b) is the fallback: its cost grows linearly, (c)'s super-linearly. 13.2
  (a). 13.3 (b): by default the merge lands with a hint and costs few tokens, and an agent that wants to be asked uses
  `--strict`. 13.4 (a). 13.5 (b): a reparent carries more intent than a reorder, and the change cannot add a fault;
  prototyped in K2 and re-run on the harness before V3, with (c) as the fallback. 13.6 (a). OQ-A-6 (a), OQ-A-11 11.1
  (B) and 11.3 and OQ-A-12 (b), (c) and (d) are superseded, and OQ-A-12's acceptance clauses "E6 lands" and "no history
  stages that the replay from B lands" are withdrawn. No option adds to the format. Decide before the merge table's V3
  signature and before the engine implements RS-007. Then R-MODEL makes K2 the model's rule, removes the replay
  machinery (step keys, the replay start, resolved keys, the backstop), re-pins the seven tests and commits the widened
  harness as the acceptance, and rewrites RS-007, MR-039, MR-040, CS-013, PR-014, PR-016, PR-017, VB-011, VA-001,
  VA-005, a new HT row and open points 15 and 35; R-SPEC-F edits [F12 §5.3] VM-7, §7.1, §7.2 and §7.4 and [F13 §5] V01
  and V05; WP-81a edits [AR §2.7] T7, I25′, [AR §5a.7] steps 3 and 4, [AR §5a.8] and [60] GT6; a spec arbiter re-runs
  the widened harness and checks the statement.
