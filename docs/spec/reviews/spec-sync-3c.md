# Spec sync 3c

Changes to `docs/spec/` made in wave 3c for owner question OQ-A-11, decided on 2026-10-06 as recommended ("Согласно
рекомендации запиши", "record it as recommended"; `owner-questions.md` OQ-A-11; [m0/PLAN §5] "Owner decisions of
2026-10-06"; [AR §11]): 11.1 (A) together with (B), with (A) alone as the fallback; 11.2 (a); 11.3 (a). The spec arbiter's
check of (B)'s statement is `wave-3c-arbiter.md`. Each author role appends its own section. Each row gives:

- **id**: `S3C-<role letter>-<n>`, the letter being M (R-MODEL) or F (R-SPEC-F);
- **file §**: the file and the sections changed;
- **change**: what the text now says;
- **source**: the decision and the open-point case a row closes;
- **affects**: the code or fixtures that must follow the change, or "none".

## R-MODEL

Rows S3C-M-1 to S3C-M-5 apply OQ-A-11 to `rules/merge-table.md`. No table changes shape, so `rules/README.md` is
unchanged; the edited file's digest changes and it stays unsigned (V3). The model follows in the same wave (WP-91): the
model's rule parser and reference checks pass on the edited file, and the whole model suite passes (`cargo test -p
moirai-model --locked`: 614 passed).

| id | file § | change | source | affects |
|---|---|---|---|---|
| S3C-M-1 | `rules/merge-table.md` RS-007 | Restructured into step keys, the one-sided merge, the replay start, the merge/sync/virtual replay, revert and cherry-pick, applying the steps, and guarantees. **One-sided merge:** when the base of a merge or a `sync` is state(tip(dst)) (single LCA = tip(dst), `--base` names tip(dst), or both ε), every hierarchy key takes t with no replay and no key is `kleppmann-skipped`; every merge of a branch into `main` is one-sided after step 0; a virtual merge, a revert and a cherry-pick never are. **Replay start:** R is the greatest commit of A(B) ∩ A(o) ∩ A(t) that every other commit of A(o) ∪ A(t) descends from or is an ancestor of, ε when none is; such commits form a chain, so R is unique; R is the base's commit when every commit of both sides since a single-LCA base descends from it. The replay starts from state(R), its steps are the commits of A(o) \ A(R) and A(t) \ A(R), and the (0, 0) step compares a side's value with state(R). **Undo:** only a move that changes its node's parent is undone; a move that sets its node's current value or changes only its order never makes its key `kleppmann-skipped`. **Step keys:** "is a step key of" is read recursively. The old guarantee (1), a linear single-parent src chain into a dst at B, is subsumed by the one-sided merge. | OQ-A-11 11.1 (A), (B); 11.2 (a); 11.3 (a); open point 35 (iv), (v), (vi), (i) | `moirai-model` (`vcs.rs` `Dag::replay_start`, `Bases::vbase`; `history.rs` `plan_merge`; `merge.rs` `Start`, `Engine::kleppmann`); [F12 §7.4] row, VM-7 and §7.1 (S3C-F-1 to S3C-F-3) |
| S3C-M-2 | `rules/merge-table.md` CS-013, MR-039 (source, note), MR-040 (source, note) | CS-013 and MR-039: only a move that changes its node's parent is undone, so a move that sets its node's current value or changes only its order never makes its key `kleppmann-skipped`; a one-sided merge replays nothing and never reaches MR-039. MR-040: a one-sided merge takes t for every key, the value I25′ asks for. Sources add [AR §11] OQ-A-11. | OQ-A-11 11.1 (A), 11.2 (a) | `moirai-model` (as S3C-M-1) |
| S3C-M-3 | `rules/merge-table.md` PR-016, VB-011 | PR-016's inputs add whether the base is state(tip(dst)), the ancestor sets A(B), A(o), A(t) and the state of the replay start R; the commits whose step keys RS-007 reads are each side's commits since R for a merge, `sync` or virtual merge that is not one-sided, read recursively through two-parent commits. VB-011: a virtual side's steps are its commits since the replay start of the pair; a virtual merge is never one-sided. | OQ-A-11 11.1 (B), 11.3 (a) | `moirai-model` (`Bases::vbase`); [F12 §7.1], VM-7 |
| S3C-M-4 | `rules/merge-table.md` open points 15 and 19 | Open point 15's closing sentence and open point 19's Kleppmann clause name the one-sided merge and the replay start instead of the (0, 0) steps from the base. | OQ-A-11 | none |
| S3C-M-5 | `rules/merge-table.md` open point 35 | Status: **decided** by OQ-A-6 (a) and OQ-A-11, except E5 of (v). (i): "moved" reads "is a step key of", recursively, decided (11.3 (a)); spec sync 3's example kept, marked as computed under the replay from B. (iv): the replay start is stated over A(B), so it covers `--base` and the virtual base; the (0, 0) step stays as a safety rule that the suite never found non-empty. (v): **decided** (A) together with (B), with the model tests for E1 to E4, the widened and the new property, the mutation result (with (A) off, E1 to E3 land through (B) and E4 stages) and the cost of (B); **E5, still open**: E4's history on a two-sided merge (a `sync` after a cross-lane merge resolved to `ours`) still stages, which GT6's I25′ property (M3) would meet; only (C) or (D) cover it, and (D) is a format addition, so the owner decides before the format freeze (WP-81b). (vi): **decided** (11.2 (a)). A decision record replaces R-MODEL's recommendation. | OQ-A-11; review RS-007-A | `moirai-model` tests (`suite::vcs`, `merge::tests`, `vcs::tests`); owner question for E5 (raised in chat) |

## R-SPEC-F

Rows S3C-F-1 to S3C-F-4 follow R-MODEL's RS-007 (S3C-M-1) and open point 35 (S3C-M-5) in [F12]. No byte of any record,
segment or fixture changes: the hierarchy rule is a merge function, and the model reproduces every fixture unchanged
(its canonical, carrier and gt10 fixture tests pass in the suite above).

| id | file § | change | source | affects |
|---|---|---|---|---|
| S3C-F-1 | `format/12-vcs.md` §7.4 row "Kleppmann steps (RS-007)" | The row states RS-007 as S3C-M-1 rewrites it: recursive step keys, the one-sided merge, the replay start R and its properties, the replay from state(R) with the (0, 0) step against state(R), revert and cherry-pick unchanged, the parent-only undo, and two guarantees (a one-sided merge lands T's hierarchy; a pick or revert of a commit with no hierarchy entry keeps O's). The open cases are reduced to E5. | OQ-A-11 | `moirai-model` (as S3C-M-1) |
| S3C-F-2 | `format/12-vcs.md` §7.1 | The inputs add whether B is state(tip dst) and the state of the replay start R; the commits whose step keys are read are each side's commits since R for a merge, `sync` or virtual merge that is not one-sided. | OQ-A-11 11.1, 11.3 | none |
| S3C-F-3 | `format/12-vcs.md` §5.3 VM-7, §5.5 "Hierarchy cycles" | VM-7: a virtual merge replays from the replay start of its pair, over A(V_{i−1}), A(Lᵢ) and A(Bᵢ); it is never one-sided; the undo takes only the moves that changed their node's parent. §5.5: the undo takes the moves that changed their node's parent. | OQ-A-11 11.1 (B), 11.2 (a) | `moirai-model` (`Bases::vbase`) |
| S3C-F-4 | `format/12-vcs.md` open point 32 (new) | Records wave 3c's change to the row, the coverage of (iv) through the replay start, the arbiter's check, the old guarantee (1) subsumed by the one-sided merge, and E5 left open. | OQ-A-11 | none |
