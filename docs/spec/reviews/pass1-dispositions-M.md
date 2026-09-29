# Review pass 1: dispositions of R-MODEL

| | |
|---|---|
| Title | Dispositions by R-MODEL of the pass-1 findings routed to it ([S-pass1], [P-pass1], [A-pass1]) |
| Status | review, pass 1; dispositions await the owner's signature (WP-80, V2); every rule file changed here awaits the owner's re-signature (V3) |
| Work package | WP-80 pass 1, author side: R-MODEL (WP-90, WP-92) |
| Files of R-MODEL | `docs/spec/rules/**` |

Dispositions:

- **fixed**: the change is made in the rule files, and in the other chapters where the fix changes a rule, name, size, enum
  or record those chapters use (each such edit was made against a fresh read of its section).
- **fixed (M part)**: the part of the finding in R-MODEL's files is fixed; the rest lies with the role named.
- **rejected**: with the reason.

| Finding | Severity | Disposition | Where / reason |
|---|---|---|---|
| S1-14 | blocker | fixed | [RULES/link-merge-rules] RK-006 (`repoint-added-referrers`) re-points every edge key present in S and absent in the base whose destination is U — every kind: `at` with its anchors, `produced`, `consumed`, `mentions`, `implements`, `cites` — and every `deleted(replaced_by = U)` and `ref`-typed value equal to U that S set since the base ([F12 §7.6] step 3 and its open point 25). RK-005 names every key U owns (out-edges included) and sets `origin_pred` to the last predecessor of RK-004, so a re-derived uid′ verifies (I-F2). The residue is a new row RK-011 (the finding says RK-010, which is `no-resurrection`; a new row keeps row ids stable). RK-010 lists [40 §8.3.2] P13's cases, the non-anchor edges on the re-keyed side included. LX-006, CP-012, Coverage and a new open point 10 follow. Cross-chapter: [F12 §7.6] steps 2 and 3 (the dead test; `origin_pred` = the last predecessor, not U), its closing paragraph and open point 25; [COVERAGE.md] row R-3. The owner re-signs the file (V3). |
| S1-15 | blocker | fixed | Lens S decision applied. [RULES/merge-table]: the `conflicted-key` rows are evaluated MR-001, MR-003, MR-004, MR-002, MR-005 (line order is evaluation order, [RULES/README] §4; ids unchanged); CS-006 is now "b a conflict value, o ≠ b, t ≠ b, o ≠ t" (RVB-4); RS-010 takes flat sides and falls back to b's class as RVB-4 does; VB-014's note; new VB-018 (one side untouched → the other side's value, VBC-3); SM-020, SM-027, Coverage and open point 18 rewritten. Cross-chapter: [F13 §3.5] I31′ restated ("conflicts when both sides changed it and they differ") with VBC-1 and VBC-3 in its gates; [F12 §5.4]'s closing paragraph and [F12] open point 2 marked decided. [60 §3.4], [60 §3.13] GT6 (VBC-3), [AR §5a.7] step 1 and [AR §3.4] I31′ are marked for WP-81a (merge-table open point 18, the [F13] I31′ row). The owner re-signs the file (V3). |
| S1-16 | blocker | fixed | Lens S decision applied: the origin reading and the holder-set cache. [F13 §4.1] states the origin reading and cites VK, HV, OR and PD-012 to PD-018; [F13 §4.2] cites the MF, ME, AB and VR rows, keeping MC-1 to MC-7 as labels that point at them (other chapters cite those labels); OP-13-04 and OP-13-05 are closed; the I26′ row's enforcement text follows. [F11 §7]: the row is keyed (`n`, origin ref, origin commit), sort key (`n`, `ref_id`, `commit`, `emit_lsn`), with `cause` (1 `ops`, 2 `undo`, 3 `op-restore`, 4 `branch-delete`, 5 `fork`), `flags` bit 0 `nonlinear`, and `holders` (a `HeapRef` `list<u32>` of live work `ref_id`s); `orig_ref_id` becomes reserved; 64 → 72 bytes; §1.3's map, the §13 example (now a 100-byte body with a 4-byte heap) and open points 1, 2 and 36 follow. [F05 §9.5]: `MarkerEntry` gains `mkind` 4 `holders` and 5 `nonlinear`, `cause` 4 (ref deletion) and 5 (fork), and field 12 `holders`; every holder change is recorded, because replay cannot recompute a holder set from net ops (a `sync` stores only its residue; forks and ref moves have no ops); [F05 §10.3] says the fold applies the records. [F16] P-65, P-52 and the phase-1 wording follow. [API §10.8], §11.2, §11.11 and the `markers` member of §15.7 follow (`holders`, `nonlinear`, the `cause` values with `fork`; no `reattributed` or `orig_ref`). Rule files: [RULES/state-definition]'s `record` column names every holder change (ME-002, ME-004 to ME-007, ME-013), ME-011 and ME-013 settle the `nonlinear` flag for revived markers, and open points 1 to 4 and 9 record the adoption; [RULES/merge-table] RE-003 follows ME-001 to ME-004 and RE-005 cites VR-003. [COVERAGE.md] row 60-AU-Seg-markers. |
| S1-30 | major | fixed | [F13 §6.2]: `open_blockers` and `open_blockers_exo` count `blocks` in-edges only (BT-001 to BT-004, BT-006); a new row `gated` (in `P_F15`, so a gate change still lists the task in `affected`; it drives only GD-002) and a paragraph giving the reason (X5). [F08 §3.4] states the counting. [RULES/state-definition] open point 7 and [RULES/status-machines] open point 22 record the adoption; [AR §3.5] is marked for WP-81a. |
| S1-40 | minor | fixed | Rule files write the decided values: [RULES/pack-classes] NR-001 and open point 11 (`--pack-digest`; `pack_digest` as the MCP parameter and the `result.v1` field); [RULES/role-write-policy] WQ-004 and WZ-010 (E411 `unknown_model_write`, exit 6), WZ-004 and WZ-005 (the "declared agent" and "bound lease" rows of E407 `lease`, exit 5), its §2 cell rules and a Coverage note; both Holes sections say None. Renames per [F01] open point 15, as single-token edits in the files that used the old ids: `OS-share-retry-ms` ([OS/fs §6.3] and Holes, [F16] Holes, [CFG] Holes); `F17-lock-writer`, `F17-lock-flush` and `F15-lock-release` ([OS/lock §4], §7.3 and Holes, whose rows now name [F17] and [F15] as owners); `OS-win-boot-source` ([F03] Holes); `LQ-display-spelling` ([F19] Holes, [LQ/canonical-ast] Holes); [LQ/gql-spelling] open point 9 and [CFG] open point 19 closed. Stale mentions of the decided `pack-digest-param` in [API] (Sources, Holes, open points 2 and 29) and [F19]'s open points rewritten. [HOLES.md] counts, §2.2, §2.5, §3, §4 and open points 1–2, and [COVERAGE.md] open point 5, updated. `r4-clock-skew` stays in `reviews/a1-dispositions.md`, a review record that is not edited ([HOLES.md] §4). |
| S1-46 | minor | fixed | The fix's second alternative: the enumeration values stay (no schema byte, `.moi` ABNF or canonical change; a project edge kind is always `tombstone`/`retain`, [F08 §8.5.4]). [F08 §8.4.6]'s `drop` (`on_dst`) and `repoint-or-flag` (`on_src`) rows name the refinements (EG-006, EG-008 to EG-010, EG-012, EG-014 to EG-016), and a paragraph after [F08 §9.6] states that [RULES/delete-policy-matrix] `edge-policy` refines the values per option, policy and condition and is authoritative. [RULES/delete-policy-matrix] open point 2 records it. |
| S1-47 | minor | fixed (M part) | `rules/README.md` §1.1 lists the eight rule files as they exist, `policy-keys.md` still planned (its rows were aligned concurrently under A1-35; checked). §7 now registers every table of the five wave-1 files as RG-031 to RG-111, with the columns and prefixes each file proposed, checked by script against every table's header row (110 tables, all registered) and against every prefix (no two tables share one); the note under §1.1 and open point 4 follow, and each file's own registry open point now names its RG rows. The `os/` Coverage sections are R-SPEC-P's (fixed in pass1-dispositions-P, S1-47); [COVERAGE.md] open points 1 and 6 were already marked resolved. |
| S1-48 | minor | fixed | RK-004 applies [F08 §11.2] step 4's test — the uid names any node: live at another path, `removed`, or a tombstone — over the base, o and t, with the same loop bound; its basis is now `derived`; open point 3 rewritten. [F12 §7.6] step 2 states the same test. |

## Round 1 (closing pass 1)

Every finding of [P-pass1], [S-pass1] and [A-pass1] whose fix lands in `rules/*`, every item [pass1-closure] §4 lists
against R-MODEL (P1-21, S1-22) and the notes other roles addressed to R-MODEL (pass1-dispositions-F "Notes for other
roles": P1-21, S1-22, A1-51, A1-57). [pass1-closure] §5 and §7 list no item against `rules/*`. The closure check did not
verify the minors round 0 fixed, so this round re-read each. Each cross-chapter edit was made against a fresh read of its
section. Rule files changed in this round: `merge-table.md`, `link-merge-rules.md`, `status-machines.md`,
`pack-classes.md`, `role-write-policy.md`, `README.md`; the owner re-signs them (V3; OQ-M-1).

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| P1-21 | major | 1 | fixed | **MR-005**: result `conflict-plain-base` (new RS-015), disposition `value`, basis `derived` from [F12 §5.4]'s plain-base rule: base b, flat sides, class from the key's `value` rows on (b, flat(o), flat(t)), else the conflicted side's class (o's when both); RS-010 and RS-015 give an existence key's provisional state as [F12 §5.4]'s last bullet does. **Validators**: VA-005 `DepthExceeded`, VA-007 and VA-008 `Cardinality`, VA-013 `PlanMask`, all `structural`, basis `derived`, citing [F19 §12.2] codes 67, 68, 72, [F13 §5] V05, V07, V12 and [F12 §7.9]'s keys; the `order` column now carries [F13 §5]'s positions V01–V13 (the draft had ties at 6, 9 and 11, which left VO-2's emission order undefined), the prose above the table says so, and every VA row cites its V id. New class-map rows CM-023 to CM-025; §9's sentence now gives [F19 §12.1]'s one code space ([F19] open point 14). **LM-013**: result `refuse-internal` (new LR-006: [F19 §10.2] `internal`, exit 1, a failed internal check), disposition `none`, basis `derived` from [F08 §11.3] (root nodes are never engine-deleted; `rm` refused with E409's root-node case), so no store's history reaches LC-005, which now says so; link-merge-rules open point 4 records the decision and the model's generator rule. merge-table open points 10 and 18 record the closure. No `gap` row remains in merge-table or link-merge-rules. [F13 §5] (R-SPEC-P) and [F19 §12.2] (R-SPEC-F) already carried their parts; S1-33's merge-table side is this fix. |
| S1-22 | major | 1 | fixed | RS-008: a `live` side carries its value-key node image ([F06 §6.2] `snap`); a `--take` towards it restores the value keys from the image and the hierarchy key and out-edges from that side's state at the conflict's introducing commit M (M's first parent for `ours`, second for `theirs`) as ordinary `Move`, `AddEdge`, `SetEdgeProps` ops of the `Resolve` commit, under the write path's checks ([F12 §6.5]). PR-013's note says the same. Open point 5 (c) and (d) marked settled by [F06 §6.2], §6.3, [F12 §6.5] and [F14 §6.8.1]. |
| A1-51 | minor | 1 | fixed | WZ-001's note: the one-line text is the design's; [F19 §11.3] and [LQ/errors §5.5] render the same words split at the semicolon as message and `= help:` line, deliberately and with no change of wording. |
| A1-57 | minor | 1 | fixed | PE-004's `default_bytes` is `HOLE(CFG-codex-mcp-result)` (the default of `mcp.result-max-bytes.codex`, owned by [CFG]; 16,000 B is the design value it names), with [CFG §10.8] and [F19 §3.2] cited; the Holes section names the hole as used, not owned, as [F19]'s does. PE-006 (one hook ceiling of 10,000 B for every client) is split by [90 §6.4]'s profile table: PE-006 `claude` 10,000, PE-007 `codex` 10,000, PE-008 `generic` 8,000; PS-014 lists them. Cross-role edit: [F19 §3.2]'s hook row no longer says PE-006 is R-MODEL's to align; it cites PE-006 to PE-008. |
| A1-35 | major | 1 | fixed (residue) | Verified closed by [pass1-closure]. A re-run by script of the parser contract of [RULES/README] §2–§8 over all eight files (types, enums, tokens, ids, `[OP-n]` targets, vocabulary and class-map references) found one latent error: RG-078 typed `marker-fields.field` as `token`, while MF-008 (`hlc, seq`) and MF-009 (`holder-actor, outcome`) name two fields; RG-078 now types it `tokens`. The same run found CM-011 missing VA-001 as an emitter; added. After both: 110 tables, 1,949 row ids, no error. |
| A1-36 | major | 1 | verified; aligned | Closed by R-SPEC-F. CO-001 to CO-003 now name the resolution each outcome writes (`completed`, `rework`, `wontdo`) and the `settled` marker's `outcome` byte, citing [API §10.5] step 1 and [LQ/std §7.3]; status-machines open point 16 records WP-25's confirmation. |
| S1-14 | blocker | 1 | verified | RK-004 to RK-006, RK-010 and RK-011 re-read against [F12 §7.6] after the other roles' round-1 edits: unchanged and consistent. Owner re-signature: OQ-M-1 (a). |
| S1-15 | blocker | 1 | verified | MR-001, MR-003, MR-004, MR-002 in that order, CS-006, RS-010 and VB-018 agree with [F12 §5.4] RVB-1 to RVB-4 and [F13 §3.5] I31′. OQ-M-1 (b). |
| S1-16 | blocker | 1 | verified | The state-definition OR, MF, ME, AB and VR rows agree with [F11 §7] (72-byte row, `holders`, `flags` bit 0, `outcome` 0–3) and [F05 §9.5] as they stand after R-SPEC-R's round 1. OQ-M-1 (c). |
| S1-30 | major | 1 | verified | The BT rows and status-machines open point 22 agree with [F13 §6.2]. OQ-M-1 (d). |
| S1-33 | minor | 1 | verified | [F13 §5] cites codes 67, 68 and 72 (R-SPEC-P); the merge-table rows are P1-21's fix above. |
| S1-40 | minor | 1 | verified | A scan of `rules/*.md` finds no `HOLE(` except the CFG-owned id PE-004 now cites (A1-57), and none of the retired ids (`pack-digest-param`, `exit5-codes`, `unknown-model-write-code`, `share-retry-ms`, `lock-*-wait-ms`, `display-spelling`). |
| A1-42 | minor | 1 | verified | As S1-40, for the rule files. |
| S1-46 | minor | 1 | verified | [F08 §8.4.6], the paragraph after [F08 §9.6] and delete-policy-matrix open point 2 state that the `edge-policy` rows refine `on_dst`/`on_src` and are authoritative. |
| S1-47 | minor | 1 | verified | [RULES/README §1.1] lists the eight files as they exist and `policy-keys.md` as planned; §7 registers every table (checked by script, A1-35 above). |
| S1-48 | minor | 1 | verified | RK-004 applies [F08 §11.2] step 4's any-node test over the base, o and t, as [F12 §7.6] step 2 does. |
| A1-26 | major | 1 | verified (M part) | OP-13-05 was closed by S1-16 (round 0); the [F17] part is R-SPEC-P's (OQ-P-1). |
| A1-50 | minor | 1 | verified (no M text) | The citations are in [CFG] (fixed by R-SPEC-F); `[RULES/policy-keys]` names a file that [RULES/README §1.1] lists as planned. |

### Other R-MODEL edits of this round (cross-chapter alignment, no finding of their own)

- **VB-019** (`auto-policy=ignored`): the virtual-merge rule that [F12 §5.3] VM-2 states and that [F12] open point 8 asked
  R-MODEL to add; merge-table open point 19 lists it.
- **PR-014**: a staged resolution whose key dst changed since the tip it was made against is stale, keeps the candidate's
  value and is named in a notice ([F12 §9.4] step 2; [F12] open point 23). Basis `derived`.
- **Open point 9** (`SupersedeFork`): confirms [F12 §6.3]'s ownership rule, so only the src-side superseder is
  `conflicted` ([F12] open point 7).
- **CM-007** and open point 20: `DATA` has no code ([F12 §6.1]). Open points 13 and 14 note that [F12 §7.5] specifies the
  line diff, diff3 and the guard.

### Owner questions

OQ-M-1 (the pass-1 rule-row changes the owner re-signs, V3) and OQ-M-2 (I13's "different actor": [RULES/status-machines]
GD-005 against [F13] OP-13-09) are in `owner-questions.md`.

### Notes for other roles

- **R-SPEC-F:** `HOLES.md` row `CFG-codex-mcp-result`: add [RULES/pack-classes] PE-004 to its `chapter §` cell. [F12 §5.3]
  VM-2's row cell "new (open point 8)" can read VB-019; [F12] open points 3, 7, 8 and 23 and the last sentence of §5.4's
  plain-base paragraph can say that R-MODEL's part is done (MR-005 with RS-015, open point 9, VB-019, PR-014). [F19] open
  point 14: the §9 sentence and VA-005, VA-007, VA-008 and VA-013 are done. [F08] open point 35 (re-key scope) was closed
  by S1-14's RK-006. No `COVERAGE.md` row cites a row this round changed.
- **R-SPEC-P:** merge-table's `validators.order` now mirrors [F13 §5]'s positions; [F13] OP-13-09's reading of I13 differs
  from GD-005's (OQ-M-2).

**Round 1 summary.** 18 dispositions: 6 fixed (P1-21, S1-22, A1-51, A1-57, and the residues of A1-35 and A1-36), 12
verified, 0 rejected, 0 left open on R-MODEL's side. The owner's re-signature of the changed rule files (V3, OQ-M-1) and
OQ-M-2 remain with the owner.

## Round 2 (closing pass 1)

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1] with a part in `rules/*`, re-read against the text as the
other roles' round-2 edits left it. [pass1-closure] lists nothing against R-MODEL (§4: "R-MODEL: none"; its §5 names no
rule-file row; the file has no §7), and no role's round-2 notes address R-MODEL. Method, as [pass1-closure] open point 3
and [A-pass1] open point 2 ask: for every datum a pass-1 fix changed in its owning chapter, the rule files were searched
for a restatement of the old value; the parser contract of [RULES/README] §2–§8 was re-run by script after the edits
(110 tables, 1,952 row ids, no error); every rule row id that another chapter cites was checked to exist in the cited
file (one did not: [F08 §4]'s `SC-002`); every section citation in the edited files resolves. Cross-role edits (**x**)
were made against a fresh read of the section. Rule files changed: `merge-table.md`, `link-merge-rules.md`,
`state-definition.md`, `status-machines.md`, `delete-policy-matrix.md`, `pack-classes.md`, `role-write-policy.md`;
OQ-M-1 gains items (i) to (m) and `delete-policy-matrix.md` in its list, for the owner's re-signature (V3).

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| S1-14 | blocker | 2 | fixed (residue) **x** | [F08 §11.2] still cited "[RULES/link-merge-rules] RK-001–RK-010 and [F12]"; it now cites RK-001–RK-011 and [F12 §7.6] (RK-011, the sync residue, was added in round 0). RK-001 to RK-011 re-read against [F12 §7.6] after R-SPEC-F's round-2 edit: they agree. OQ-M-1 (a). |
| S1-6, A1-4 | blocker | 2 | fixed (M residue) | [F06 §7.10] (pass 1) inverts a `Create` to a `Delete` and gives `CreateDeleted` no inverse, but DM-004's and DM-005's three-way apply reads src = state(p1(c)), where a node c brought into being is `absent`: `theirs-only` would make it absent, a state no op of [F06 §7.4] writes ([F12 §7.8]). New DM-017 (`src-absent-reads-inverse`, proposed): a revert never makes a node absent; a reverted creation leaves the tombstone of the inverse `Delete`, a reverted `CreateDeleted` leaves dst's value and emits no existence op. DM-004 and DM-005 cite it; merge-table open point 33 records that [F06 §7.10] does not yet name the inverse `Delete`'s `reason` and `replaced_by` (note for R-SPEC-F). OQ-M-1 (i). |
| S1-21, A1-5 | major, blocker | 2 | fixed (M residue) | New DM-016: a revert or cherry-pick of a commit `gc` pruned to its header ([F06 §4.4.15], §7.10) is refused with [F19 §10.2] `commit_pruned`, exit 3, before any state is read; DM-003 to DM-005 and SM-028 cite it. Without it the model would three-way apply a commit whose ops the engine no longer has. OQ-M-1 (i). |
| S1-1, P1-1, A1-1 (empty values), S1-34 | blocker, minor | 2 | fixed (M residue) | "Empty is absent, everywhere" ([F08 §5.3], §6.2, §7.2) had not reached the rule results. CS-001 states it; RS-004 (a sum of 0), RS-005 (an empty union), RS-006 and RS-013 (an empty diff3 result, [F12 §7.5] "Empty result") give `absent`. CS-011 now carries [F12 §7.5]'s length bound: a clean diff3 longer than 65,536 bytes falls to the class's `TextHunk` row (MR-032, MR-038 notes), the outcome [F12 §7.5] states and the table lacked. Merge-table open point 34. OQ-M-1 (j). |
| S1-3, A1-2 (and P1-1's anchor part) | blocker | 2 | fixed (M residue) | Link-merge-rules §2 said quote, prefix and suffix enter the anchor's value as digests; [F08 §10.3] and [F07 §8.2] also hash a `range` anchor's end (`end_h`). §2 now names it, cites the selector block of [F07 §8.2] over [F08 §10.3]'s record, and the `text_unavailable` form of a hash-only import ([F07 §8.3]). CP-003 cites [F08 §5.5]'s one `pathmove` order (S1-1 (g)) and says why entries tied on (hlc, from, to) rewrite alike. |
| S1-5, A1-9, A1-19 | blocker, major | 2 | fixed (M residue) | Merge-table §2 "Values" defined a conflict value as {class, base, ours, theirs} only. It now names the existence key's provisional side `prov` and a live side's node image as parts of the value and of its equality ([F12 §6.3], §7.3), and states the provisional value that derived state and validators read ([F12 §6.3]). Prose; no row changes. |
| P1-3, S1-11, A1-11 | blocker | 2 | fixed (M residue) **x** | Reason 5 `park` writes `orphans/<ref>` refs (kind 6, [F16] P-70), which `view-kinds` did not cover, while [F13 §4.1] cited VK-002 to VK-005 for them. New VK-007 (`orphans`: never holds, not being of kind `work`; its tip is read like VK-004's; derived). **x** [F13 §4.1] cites VK-007. OQ-M-1 (l). |
| P1-21 | major | 2 | fixed (residue) | LL-014 (a `leader` lease anchor) was the last `gap` row of the rule files: a case reaching it raised `SpecGap`, which P1-21 removed from the merge tables. [F03 §10.3] and [F11 §6] allow lease anchors of kinds 0, 1 and 4 only, so the row is unreachable: basis `derived`, and reaching it is a failed internal check, as LR-006. State-definition open point 12. A search finds no `gap` basis in any rule file. OQ-M-1 (l). |
| A1-26 | major | 2 | fixed (M residue) | DS-010 said "the transitive `suspect` closure" and "readers recompute `suspect`", against [F17 §8.2] and [F13 §6.3] (the single-hop predicate; beyond the budget `affected` omits the `suspect`-only changes with `affected_complete = 0`, the bitset stays complete, the hint `SuspectBudget` of [F19 §12.3] is printed). DS-010 now states that rule; delete-policy-matrix open point 10 records it as decided (owner sign-off OQ-P-1); RE-010 cites [F13 §6.3] and [F17 §8.2]. OQ-M-1 (k). |
| A1-39 | major | 2 | fixed (M residue) | Refusing rows now name the code [F19 §10.2] or [API §9.1] assigns: WZ-009 `not_writer_tree`, text owner [F19] (was `F18:not_writer_tree`; [F18] defines no code), with role-write-policy §2's refusal-cell rule; DP-003 and GR-005 `not_found` (exit 3, `what` = `node`); DP-004 `usage`; DP-005 E409, as [API §9.1] assigns the live-lease refusal (was the placeholder `F19`; delete-policy-matrix open point 9 decided); PR-002 `staging_exists`, PR-003 `conflicted_src`, DM-006 to DM-008 `revert_refused` (exit 6). OQ-M-1 (m). |
| P1-10 | major | 2 | fixed (M residue) | PH-007 (the pack header's quiet state) and DE-027 named the `HEAD` flag and a measuring lane only; [F17 §5.3] adds a held quiet byte of `LOCK` ([F03 §3.1], P1-10's fix) and the key `quiet.from-lane-measuring`. Both rows follow [F17 §5.3]. |
| A1-29, A1-48 | major, minor | 2 | fixed (M residue) | RN-008 rendered `verify #N` only. [F19 §4.6], which owns the marker, writes `verify <handle>` with `aN` for an anchor-level state, a `confirm` form on an accepted guess, two legend lines and the 50 B bound; RN-008 cites it for spelling and bound. |
| A1-50 | minor | 2 | fixed (residue) **x** | Row-id citations into the rule files, checked by script across `docs/spec/`: all resolve except [F08 §4]'s "[RULES/status-machines] SC-002" (no `SC` table exists). **x** [F08 §4] cites WT-004 "which [RULES/status-machines] GR-002 applies to status writes". |
| A1-23 | major | 2 | left open (M residue; the bytes are [F05]'s and [F11]'s) | Pack-classes PT-028 and PX-011 read and append a per-(agent, T) pack cursor ([AR §7.4] C8, proposed). The cursor records A1-23's fix provides ([F05 §9.11] `Lazy` `sub` 2, [F11 §13.1] `CURSORS`) are keyed by (session, agent, feed) with `feed` 1 only, so PX-011's record has no bytes. The rule rows need no change; pack-classes open point 8 records the request (notes below). |
| S1-15, S1-16 | blocker | 2 | verified | MR-001, MR-003, MR-004, MR-002, MR-005, CS-006, RS-010, RS-015 and VB-018 against [F12 §5.4] after R-SPEC-F's round-2 edit of its plain-base paragraph; the MF, ME, OR, AB and VR rows against [F11 §7] (72 B) and [F05 §9.5]. OQ-M-1 (b), (c). |
| S1-22, S1-30 | major | 2 | verified | RS-008 and PR-013 against [F12 §6.5] and [F14 §6.8.1]; the BT rows, FL-001 and FL-002 against [F13 §6.2] and [F08 §10.2] (`flagged` bit). OQ-M-1 (d), (h). |
| A1-35 | major | 2 | verified | The parser contract re-run after this round's edits: 110 tables, 1,952 row ids (DM-016, DM-017, VK-007 added), no error. |
| A1-36 | major | 2 | verified | CO-001 to CO-003 against [LQ/std §7.3] and [F11 §7] `outcome` 1–3. |
| A1-31, A1-32, A1-37, A1-38 | major | 2 | verified (no M text) | No rule file restates the `--ids` cut, the 600 B error bound, a lease TTL default or `std.ready`'s order (searched; "15 min" in state-definition open point 12 quotes [AR §6.2]). |
| A1-6, A1-7, A1-8, S1-8, S1-9, S1-10, P1-2 | blocker | 2 | verified (no M text) | No rule row restates a runtime-record or row code; LV-010's state names are [F18 §4.2]'s; the `relink` values of CP-006, LV-001 and LV-005 match [F18 §5.1]'s grammar. |
| S1-32, S1-33, S1-40, S1-46, S1-47, S1-48, A1-42, A1-43, A1-51, A1-57, P1-31 | minor | 2 | verified | VA rows cite codes 67, 68, 72; the only `HOLE(` in the rule files is PE-004's CFG-owned id, which `HOLES.md` now cites; [F08 §8.4.6] and §9.6 state the EG refinements; README §1.1 and §7; RK-004's test; WZ-001; PE-004 to PE-008; CM-007 and DM-013 (`DATA` has no code). |

### Cross-role edits of this round (please review)

- **[F08 §4]** (R-SPEC-F): the dangling `[RULES/status-machines] SC-002` replaced by GR-002 (A1-50).
- **[F08 §11.2]** (R-SPEC-F): RK-001–RK-011 and [F12 §7.6] (S1-14).
- **[F13 §4.1]** (R-SPEC-P): "(VK-002 to VK-005, VK-007)" (P1-3, S1-11, A1-11).

### Notes for other roles

- **R-SPEC-F:** (1) [LQ/errors §5.5]'s E409 message covers restricted references and a root node, not the live-lease case
  that [API §9.1] maps to E409; DP-005's text is `leased by <holder> on <ref> (<lease>)`. Please add the case, or give it
  another code and DP-005 follows. (2) [F06 §7.10] could name the `reason` and `replaced_by` of the inverse `Delete` of a
  reverted `Create` (merge-table open point 33; R-MODEL proposes an empty reason and no replacement). (3) `COVERAGE.md`:
  no row needs a change; rows F15 and 60-I2-Derived may cite [RULES/merge-table] RE-010 and [RULES/delete-policy-matrix]
  DS-010 beside [F13 §6.3]. `HOLES.md`: no hole added, renamed or removed. `README.md`: no change.
- **R-SPEC-P:** (1) Please review the [F13 §4.1] edit. (2) [F05 §9.11]: pack-classes PX-011 needs a per-(agent, T) pack
  cursor ([AR §7.4] C8); proposal: `feed` 2 `pack`, with the task's uid in the record.
- **R-SPEC-R:** [F11 §13.1] `CURSORS` would key that cursor by (session, agent, feed, task); to be settled with R-SPEC-P.

**Round 2 summary.** 21 rows: 13 fixed (24 finding ids, all residues of fixes whose owning text changed in other
chapters, three with a cross-role edit), 7 verified (28 finding ids), 1 left open (A1-23's cursor bytes, owned by
[F05] and [F11]), 0 rejected. The owner's re-signature (OQ-M-1, now (a) to (m)) and OQ-M-2 remain with the owner.

## Round 3 (closing pass 1)

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1], at every severity, with a part in `rules/*`; the items
[pass1-closure] (round 2) lists against R-MODEL (§4.2 "By role": DP-005 and DM-017 follow NC-8 and NC-9; PX-011 and
PT-028 for NC-10; the closure has no §7, and its §5, the `COVERAGE.md` state, names no rule row); the notes the other
roles left for R-MODEL (`pass1-dispositions-F.md` round 3 items (1)–(4), `-P.md` §8.6, `-R.md` §7.6); and the cross
edits R-SPEC-F made in R-MODEL's files this round. As [pass1-closure] open point 3 asks, every row changed here was
re-read with the exact case of the section it cites: [LQ/errors §5.5] E409 (all four cases, the paragraph after the
table, §5.7, open point 17), [F19 §10.5], [API §9.1], [F08 §11.3], [F06 §7.4] `Undelete`, [F06 §7.10], [F05 §9.5]
fields 9 and 11, [F05 §9.11], §9.14, [F11 §7] `actor` and "Records", [F11 §13.1], §13.2, [API §10.5], [API] open point
48, [owner-questions] OQ-F-3. Mechanical checks, by script in the scratch space (not committed), after the edits: the
parser contract of [RULES/README] §2–§8 over all eight files (110 tables, 1,953 row ids with DP-010, every registry
column list equal to its table's header, every enum and token cell valid, every `[OP-n]` present, no error); every
rule-row id cited anywhere in `docs/spec/` outside `reviews/` exists (the one miss is README's template row `SG-001`);
every section citation in `rules/*.md`, [LQ/errors] and [F19] resolves (the one miss is README's template
`[F12 §x.y]`). Cross-role edits (**x**) were made against a fresh read of the section. Rule files changed:
`delete-policy-matrix.md`, `pack-classes.md`, `state-definition.md`; OQ-M-1 gains items (n) to (p) for the owner's
re-signature (V3).

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-8 (closure §4.2), A1-39 | major | 3 | fixed (M part) **x** | R-SPEC-F's cross edit of DP-005 (cites [LQ/errors §5.5]'s E409 lease case) and open point 9 reviewed against [LQ/errors §5.5], §5.7 and [API §9.1]: kept. R-SPEC-F's note (2): the root-node refusal of [F08 §11.3] had no precondition row, so which E409 case prints for a delete set holding a root node and a live lease was unstated. New **DP-010** (`root-node`, E409, exit 6, derived): a root node anywhere in the deleted set refuses the delete, a `--cascade` whose subtree holds one included ([F08 §11.3] never engine-deletes one; LM-013 relies on it), naming the root node with the smallest `#N`. It is placed after DP-004, not after DP-003 as the note proposed, because under `--cascade` the deleted set exists only once DP-004 has accepted the options; it precedes DP-005, so the root-node case prints first. Open point 14 records it. DP-006 and DP-007 now cite their E409 texts (restricted references; the replacement case), open point 3's "[F19] may give it its own code" is settled, and the Coverage sentence names E409's four cases. **x** [LQ/errors §5.5]: the root-node case names DP-010 and the node it prints; the paragraph after the table orders the four cases (DP-010, DP-005, DP-006, DP-007); open point 17's last sentence records the answer. **x** [F19 §10.5]: the row "`rm` of a root node" cites DP-010. OQ-M-1 (n) |
| NC-9 (closure §4.2), S1-6, A1-4 | major, blocker | 3 | fixed (residue) | R-SPEC-F's cross edit of DM-017 (quotes [F06 §7.10] "`Create` → `Delete`"; the tombstone `deleted(kind, "", none)`) and open point 33 reviewed against [F06 §7.10], §7.4 and [F07 §13]: kept. [F06 §7.10] now also states that an `Undelete` inverts to a `Delete` taking its `reason`, `replaced_by` and image ([F06 §7.4] `Undelete` carries all three); delete-policy-matrix §9 said only that `Undelete` inverts `Delete`, and now states both directions and that DM-004's three-way apply gives the same tombstone in state terms. DM-017 stays `proposed` until the re-signature (OQ-M-1 (i)) |
| NC-10 (closure §4.2), A1-23 | major | 3 | fixed (M part) | [F05 §9.11] `Lazy` `feed` 2 (R-SPEC-P) and [F11 §13.1] `CURSORS` keyed (session, agent, feed, task) (R-SPEC-R) now hold the C8 cursor. PT-028 states cursor(A, T) as the `cursor_seq` of the row (session, A, `feed` 2, `#N` of T) of the session the pack runs in, absent when none was appended, the row was dropped or T was re-keyed; PX-011 names the record's fields (`sub` 2, `feed` 2, `task` = T's `#N`, `cursor_seq` = rev) and both cite [F05 §9.11] and [F11 §13.1]. The same residue in PT-027: mark(A) said "the latest `SessionMark` whose agent is A", while [F11 §13.2] `SESSMARKS` keeps one row per (session, agent); PT-027 now reads that row of the session the pack runs in and cites [F05 §9.14] and [F11 §13.2]. R-SPEC-F's note (3): PX-011 no longer says that the `pack` verb appends; who appends is OQ-F-3's (the owner's), and until it is answered no M0 command appends a pack cursor ([API] open point 48), so PX-011 appends nothing in the model and C8 is empty on both sides, as [API] states. Open point 8 records the bytes (T as `#N`, not the uid R-MODEL asked for: store-wide and never reused, [F11 §9]), the per-session reading and OQ-F-3; the Coverage paragraph names the sections. OQ-M-1 (p) |
| A1-6, S1-16 | blocker | 3 | fixed (residue) | MF-009 ("for `settled` records written by `complete`: the lease holder and the outcome") and [F05 §9.5] fields 9 and 11 (the holder and `--outcome` of the entry a `complete`'s commit writes; 0 for every other entry, re-emits included), which [F11 §7] `actor` and `outcome` take, did not visibly state the same set of writes (`-P.md` §8.6 item 2, `-R.md` §7.6 item 2). MF-009 now states [F05]'s reading: one entry carries them (ME-001 written by the commit of a `complete`, with the presented and released task lease's holder and the outcome 1–3 of CO-001 to CO-003); every other entry carries 0, the other ME-001 doors, `cancelled` holds and every re-emit (ME-003, ME-006, ME-007) included, so a re-emit resets `actor` and `outcome`; `holders` and `nonlinear` entries and an ME-013 revival carry neither field and leave the row's values, as [F11 §7] "Records" moves a revived row unchanged. Taken rather than carrying the old holder into a re-emit: a re-emit of a marker never written (an origin on a staging ref) has no holder to carry, and replay would need a lookup the records do not hold. OQ-M-1 (o) |
| S1-14, S1-15, S1-1, P1-1, A1-1, S1-3, A1-2, S1-5, A1-9, P1-3, S1-11, A1-11, A1-5 | blocker | 3 | verified | RK-001 to RK-011 against [F12 §7.6] and [F08 §11.2]; MR-001, MR-003, MR-004, MR-002, MR-005, CS-006, RS-010, RS-015, VB-018 against [F12 §5.4]; CS-001, CS-011, RS-004 to RS-006, RS-013 against [F08 §5.3], §6.2, [F12 §7.5]; link-merge-rules §2 and CP-003 against [F08 §10.3], §5.5 and [F07 §8.2] ([F09 §7.2]'s round-3 precision on `at` blocks restates no rule-file text); merge-table §2 "Values" (`prov`, node image) against [F12 §6.3], §7.3; VK-007 against [F12 §2.2]'s round-3 parker wording and [F16] P-70 (the same actor); DM-016 against [F06 §4.4.15]. None of the other roles' round-3 edits touches them. OQ-M-1 (a), (b), (i), (j), (l) |
| S1-21 | major | 3 | verified | As A1-5 (DM-016) |
| A1-7, A1-8, S1-8, S1-9, S1-10, P1-2 | blocker | 3 | verified (no M text) | No rule row restates a runtime-record or row code; the `MARKERS` fields the rules name are MF-009's above. LV-010's states and the `relink` values of CP-006, LV-001 and LV-005 unchanged against [F18 §4.2], §5.1 |
| S1-22, S1-30, P1-21, A1-19, A1-26, A1-29, A1-35, A1-36, P1-10 | major | 3 | verified | RS-008, PR-013 against [F12 §6.5], [F14 §6.8.1]; BT rows, FL-001, FL-002 against [F13 §6.2], [F08 §3.4]; MR-005, VA-005, VA-007, VA-008, VA-013 (codes 67, 68, 72; [F13 §5] V05, V07, V12), LM-013, LR-006, LL-014 against [F12 §5.4], [F19 §12.2], [F08 §11.3] (with DP-010 the case LM-013 needs stays unreachable under `--cascade` too), [F03 §10.3], [F11 §6]; DS-010, RE-010 against [F17 §8.2], [F13 §6.3]; RN-008 against [F19 §4.6]; the parser contract as above (A1-35); CO-001 to CO-003 against [LQ/std §7.3], [API §10.5] and the `outcome` byte of [F05 §9.5] field 11 and [F11 §7]; PH-007, DE-027 against [F17 §5.3]. OQ-M-1 (b) to (h), (k), (l) |
| A1-31, A1-32, A1-37, A1-38 | major | 3 | verified (no M text) | Searched again: no rule file restates the `--ids` cut, the 600 B error bound, a lease TTL default or `std.ready`'s order |
| S1-32, S1-33, S1-34, S1-40, S1-46, S1-47, S1-48, A1-42, A1-43, A1-48, A1-50, A1-51, A1-57, P1-31 | minor | 3 | verified | CM-007 and DM-013 (`DATA` has no code); VA rows cite 67, 68, 72; CS-001 (an empty body is absent); the only `HOLE(` in the rule files is PE-004's CFG-owned id; [F08 §8.4.6] and §9.6 state the EG refinements; README §1.1 and §7 (110 tables registered); RK-004's any-node test; RN-008's `verify <handle>`; the WT-004 and GR-002 rows [F08 §4] cites exist; WZ-001's note; PE-004 to PE-008 |

Rejected: none. Left open in the text: none. With the owner: OQ-M-1 (now (a) to (p)) and OQ-M-2, unchanged in substance;
OQ-F-3 decides PX-011's appender (the rows state the interim that [API] open point 48 states).

### Review of the other roles' edits in R-MODEL's files (round 3)

- **[RULES/merge-table] DM-017 and open point 33** (R-SPEC-F): kept; they state the bytes [F06 §7.10] now gives.
- **[RULES/delete-policy-matrix] DP-005 and open point 9** (R-SPEC-F): kept; open point 9's mention of DP-007's own E409
  case is now matched by DP-007's source cell.

### Cross-role edits of this round (please review)

- **[LQ/errors §5.5]** (R-SPEC-F): the E409 root-node case reads "for a root node in the deleted set ([F08 §11.3],
  [RULES/delete-policy-matrix] DP-010) … naming the root node of the deleted set with the smallest `#N`"; the paragraph
  after the table orders the four cases DP-010, DP-005, DP-006, DP-007; open point 17's last sentence records DP-010.
- **[F19 §10.5]** (R-SPEC-F): the row "`rm` of a root node" cites DP-010 beside [F08 §11.3].

### Notes for other roles

- **R-SPEC-F:** (1) Please review the two cross edits above. (2) [API §9.1]'s E409 row names the restricted, lease and
  replacement cases (DP-005 to DP-007); it may add "a root node in the deleted set ([F08 §11.3], DP-010)" so that the
  refusal table lists all four; optional, no code or exit changes. (3) `COVERAGE.md`: no row needs a change (no row cites
  delete-policy-matrix §4, PT-027, PT-028, PX-011 or MF-009 against a contradicting section). `HOLES.md`: no hole
  added, renamed or removed. `README.md`: no change.
- **R-SPEC-P:** none. MF-009 now states the set of writes that [F05 §9.5] fields 9 and 11 carry (`-P.md` §8.6 item 2);
  PT-028 and PX-011 cite [F05 §9.11] (§8.6 item 1).
- **R-SPEC-R:** none. [F11 §7] `actor` and `outcome` follow MF-009 as now worded; PT-027 and PT-028 cite [F11 §13.2] and
  §13.1 (`-R.md` §7.6).

**Round 3 summary.** 10 rows: 4 fixed (9 ids: the R-MODEL side of the closure's NC-8, NC-9 and NC-10, and the findings
A1-39, S1-6, A1-4, A1-23, A1-6, S1-16), 6 verified (47 finding ids), 0 rejected, 0 left open; two cross-role alignment
edits ([LQ/errors §5.5], [F19 §10.5]). The owner's re-signature (OQ-M-1 (a) to (p)), OQ-M-2 and OQ-F-3 (PX-011's
appender) remain with the owner.
