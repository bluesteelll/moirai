# LQ-Bench corpus check (R-BENCH checker, pass 1)

| | |
|---|---|
| **Title** | Check of the 150 synthetic and 40 adversarial LQ-Bench tasks against plan.md, README.md, store-design.md and [50 §7.4] |
| **Status** | draft, pass 1 pending |
| **Work package** | WP-70 (lane B, role R-BENCH; S6), cross-batch check of [plan.md §7] item 3 |
| **Sources** | [50 §2.2] (lexical rules), §2.3 (grammar v1), §2.5 (edge types, aliases, properties), §2.6 (built-ins, table functions), §2.8, §3.3–§3.10 (semantics), §4.1–§4.2 (std queries, named mutations), §5.2 (codes), §6.1 (argv), §7.4 items 1–6 (LQ-Bench); [plan.md §1–§7]; [README.md §3–§7, §11]; [store-design.md §2–§16] (cited SD §n) |
| **Files checked** | `tasks/batch-1.jsonl` … `tasks/batch-5.jsonl` (T001–T150), `adversarial/batch-1.jsonl` … `adversarial/batch-5.jsonl` (A01–A40) |

---

## 1. Method

1. **Mechanical** (script, every line): UTF-8 without BOM, LF only, final LF, no blank line, one JSON object per line, ids
   unique and ordered, key set of README §3.1, stratum per id range (plan §1), phrasing keys, 1–4 construct tags from the
   closed list of README §5.1, trap ids and trap keys of README §5.2, primary construct of each trap, `write` against the
   gold kind, row width against the columns, value encodings of README §7.2, `order.by` naming a gold column, `tolerance`
   only with a float column, at most 20 rows, every `@handle` and placeholder resolving to store-design, no card id
   (#51, #88, #89, #93, #130), `short` at most 12 words, no LQ spelling in code form in any phrasing, Cyrillic exactly on
   `non-ascii` tasks, `named-query` exactly where `named` is set, exact and id-normalised duplicate gold queries,
   identical `named` calls, identical gold row sets.
2. **Quotas**: construct minima per batch and in prompts (plan §4), named-query, view, `non-ascii`, `error`, `surface`
   quotas (plan §5), the write-stratum mix (plan §5), the trap allocation (plan §3).
3. **By reading**: every gold query parsed by hand against [50 §2.3] (entry points, `standalone_call` against
   `call_clause`, quantified groups, `rev_arg` positions, reserved words, `TX` statements and options); every gold
   result recomputed from store-design (statuses per view, §7 schedule, §6 edges, §9 link states, §10 conflicts, §11
   derived sets, §11.6 chain, §12–§14 filler and signal terms) and checked against the facts SD §16 excludes; every
   phrasing checked against README §4 (one question, same referents, no hint of the LQ form).
4. **Owner-derived content**: searched for owner names, local paths, harness or session text; none found.

## 2. Results

| Check | Result |
|---|---|
| JSON validity, encoding, line ends, ordering | pass (190 lines, 190 objects) |
| Counts per stratum | pass: lookup 15, aggregation 15, filter 20, search 10, traversal 20, merge 10, derived 15, write 15, history 20, links 10; adversarial 40 (8 per batch) |
| Unique ids, id ranges per batch | pass |
| Trap id per adversarial task (plan §3) and primary construct (README §5.2) | pass |
| Construct minima (plan §4), per batch and in prompts | pass for all 44 tags (§5 lists the tags that sit exactly at their minimum) |
| Named-query quota ≥ 8 per batch | pass: 8, 8, 8, 8, 9 |
| View quota (plan §5) | pass: 3, 4, 11, 7, 18 (minima 2, 3, 6, 4, 6) |
| `non-ascii` quota ≥ 3 per batch | pass: 3, 5, 3, 3, 4 |
| `error` golds | pass: A35 only (E302) |
| `surface: cli` | pass: A32, A40 only |
| Write-stratum mix (plan §5) | pass: every listed item present (T106–T120); lane commits T109, T110, T119 (and T118 on the staging ref) |
| Grammar v1 conformance of all 190 gold queries | pass (§1 item 3); no production outside [50 §2.3] |
| Handles and placeholders | pass: every `@handle`, `@c.<commit>`, `{{commit:…}}`, `{{seq:…}}`, `{{lease:…}}` exists in store-design |
| Gold results derivable from store-design | pass for all 190; no gold rests on an SD §16 fact (T096 holds with #242's open_blockers 0 or 1) |
| Exact duplicate gold queries | none |
| Near-duplicate tasks | **fail**: §4 F3, F4 |
| Trap families of [50 §7.4] item 2 | **fail**: two families absent (§4 F1) |
| Owner-derived content | none |

## 3. Fixes applied in place

| Task | File | Change | Reason |
|---|---|---|---|
| A17 | `adversarial/batch-3.jsonl` | `constructs` `["edge-direction-blocks"]` → `["edge-direction-blocks","named-query"]` | `named` is `blockers id=277`; README §5.1 and plan §4 give the `named-query` tag to every task whose `named` is set |
| A23 | `adversarial/batch-3.jsonl` | `constructs` `["reverse-alias","edge-direction-blocks"]` → `[…,"named-query"]` | `named` is `blockers id=264`; same rule |
| T149 | `tasks/batch-5.jsonl` | `short` "files produced or consumed by the net lane's run, with states" → "files the run in lane #700 produced or consumed, with states" | README §4: the short phrasing keeps the literal's referents; it had dropped `#700` |

After the fixes the adversarial `named-query` set is A12, A17, A23, A25, A32, A37, A40, and the tag has 130 prompts.

## 4. Findings not fixed mechanically

Each finding names the file owner who must act (plan §7 item 1: the batch lead owns README.md, plan.md and
store-design.md; each batch owns its task file).

**F1. Two trap families of [50 §7.4] item 2 have no task (batch lead; plan §3, README §5.2).** Item 2 lists, besides the
families plan §3 allocates, "`link_state(n) <> 'ok'` over unlinked nodes (A1 re-review S-05)" and "walks that reuse a
fixed part's edge or run over undirected and mixed-kind patterns (S-21)". Plan §3 says the forty tasks "cover every trap
family of item 2"; they cover 18 of the 20 families item 2 names. README §5.2 has no trap id for either. No synthetic task exercises S-21 either
(no gold query has an undirected or mixed-kind quantified edge, a quantified walk on a cyclic kind, or a walk re-using a
fixed part's edge), although [50 §2.8] says "LQ-Bench tags both"; T146 exercises `link_state(t) <> 'ok'` only in its
guarded, correct form. Proposed resolution: add trap ids `link-state-unlinked` (primary `file-link`; lure
`MATCH (t:task) WHERE link_state(t) <> 'ok'` without the `EXISTS { (t)-[:AT]->() }` guard, exposed by W10; store shape SD
§9.4, where only 15 nodes carry anchors) and `walk-reuse` (primary `hop-bound` or `closure`; shapes: an undirected
`-[:BLOCKS]-+` from #274, an alternation `[:BLOCKS|CHILD_OF]+` over campaign C, or a fixed `(a)-[:BLOCKS]->(b)` beside
`(b)<-[:BLOCKS]-+(c)`), and take their two slots from families with three tasks. Taking one each from `count-anonymous`
(A01–A03) and `hand-ready` (A25–A27) keeps every construct minimum: `count-bag` falls from 21 to 20 prompts and
`derived-ready` from 22 to 21.

**F2. The file layout differs from the one the plan and the README specify (batch lead).** Plan §1, §2 and README §1,
§2, §3 name `tasks/B1.jsonl` … `tasks/B5.jsonl`, each holding a batch's synthetic **and** adversarial tasks ("the same
files"). The corpus is `tasks/batch-<n>.jsonl` (synthetic) and `adversarial/batch-<n>.jsonl`. No code reads these paths
yet (`crates/moirai-lqbench` holds only a stub). Either rename and merge the files, or amend README §1–§3 and plan §1–§2
to the layout that exists; the loader (WP-70) follows whichever is fixed.

**F3. Near-duplicate tasks (the owning batches; plan §7 item 3).** Each pair asks the same fact of the same referent;
the second task adds no construct evidence. Rewrite one task of each pair on another referent, keeping its tags so the
minima of §5 still hold, and keeping a `named` call where the task has one (B1 and B4 have exactly 8 named tasks):

| Pair | Why it is a near-duplicate | Suggested direction for the rewrite |
|---|---|---|
| T006 (B1) / T060 (B2) | Both ask which task is titled with 'анализатор'; both golds are #266; identical `named` call `find kind=task text=анализатор` | Keep T060 (search stratum); move T006 to another Cyrillic lookup with a named call, e.g. `show` of the Cyrillic-titled rule #218 (criticality, authority) |
| T007 (B1) / T058 (B2) | Both find #234 by the token 'пакетов' in its title (T036 also returns #234 by title) | Keep T058; move T007 to another Cyrillic question with a named call, e.g. `notes` for a `src/net/` path asked in Russian |
| T005 (B1) / T135 (B5) | Both read the tombstone of #293; T135's columns are a superset of T005's | Move T135 to the tombstone of #296 (no replacement) or #232, or to #299 on lane/shaders |
| T109 / T110 (B4) | Gold queries differ only in lane, id, lease and summary (`TX ON lane/… LEASE '…' { CALL tx.complete(…) }`) | Plan §5 asks for "a lease-guarded change" on lane/audio: make T110 a `LEASE '{{lease:LS-264}}'` block with a guarded `MATCH … EXPECT 1 SET` (e.g. `files_owned` of #264) instead of a second `tx.complete` |
| T091 (B4) / A25 (B4) | Both are `std.ready` scoped to a campaign on main; the phrasings differ only in the id and T091's "most important first" | Keep A25 (trap); move T091 to `ready` with `role` or with a priority filter, or to a campaign whose ready set differs in kind (e.g. #285 is ready_to_close with no ready task) |

**F4. Template repetition (advisory; the owning batches).** These groups use one gold-query template with only ids,
refs or a literal changed. Each task has a distinct data twist, so none is a defect, but the review may diversify them
when F3 is resolved: `ready` scoped by `subtree` at a view (T091, T092, T093, T094, A25); `CALL blockers(#N) YIELD
blocker RETURN blocker` (T061, T074, T098); the direct-subtask `INTERSECT`/`EXCEPT` of two views (T014, T045, T046);
active rules for `src/net/tick.rs` (T008 is the critical subset of T039); the run of lane #700 through
`PRODUCED|CONSUMED` (T019 counts, T149 lists); the single note `SUPERSEDES` edge #237→#236 (T043, T076, T137); `tree`
(T012, T073).

**F5. Both `indirect-hop-shortcut` tasks use the same store shape (batch 3 and batch 5; plan §3).** A24 and A39 both walk
the `DEPENDS_ON` chain from #259 with its shortcut to #255 (T023, T024 and T064 use it too). Plan §3 lists three shapes
and its open point 2 splits the family over two batches "to vary their store shapes". Move A39 to a `BLOCKS` shortcut
(#241→#243→#245 with #241→#245, or #274→#275→#280 with #274→#280) at a past view; both shapes exist on every `main~n`
with n ≤ 67 (SD §11.6).

**F6. The diff-row encoding the write golds use is not written down (batch lead; README §7.5).** README §7.5 defines
the keys of a `changes` row but not their values for existence, parent and edge rows. The corpus is consistent: an
existence row is `"aspect":"exists","name":"exists"` with `"present"`/`null` as after/before; a parent row is
`"aspect":"parent","name":"parent"` with node values; an edge row sits on the stored source with
`"name":"<STORED TYPE>->@<dst>"` and `"present"`/`null`; `created` maps carry every field the statement sets, not only
`kind` and `title`. README §7.5 should state this, since the scorer (WP-70) compares these rows as a set. Two golds
depend on shapes no design document fixes: T118 writes `RESOLVE … TAKE REPOINT #1400` as the removal of `#252 BLOCKS
#251` plus the addition of `#252 BLOCKS #1400` (a single `~` edge row is equally plausible), and T117 writes the node
`DELETE` as one `-` existence row. The scorer's implied-row table (README §7.5) must say how a repoint is compared, or
T118's gold must be restated when chapter 12 fixes the `RESOLVE` diff.

**F7. A38's trap cannot fall (batch 5; advisory).** A38's `trap_result` equals its gold: under [50 §2.2] rule 4 the
lured spelling (`s1`, `s2` as variables beside `USE s<seq>`) is correct, so the task measures only that the agent does
not break a correct query. This is what the `s1-variable` family can measure in grammar v1; the review confirms that the
item is kept as a regression guard rather than as a discriminating trap.

**F8. README §3.3's A17 example lacks the `named-query` tag (batch lead).** The example sets `named` to `blockers
id=277` with `constructs` `["edge-direction-blocks"]`. The corpus line now carries the tag (§3); the README example should
too, and README §3.1 should list "`named` set without `named-query`, or the reverse" among the loader's rejections.

**F9. Accepted deviations from plan §3's store shapes (no action).** A18 uses the out-edges of #245 rather than "BLOCKS
into #277, #280, #295"; A23 uses #264 instead of #277 or #297 (its note explains that #297's only blocker is a tombstone
no plain pattern binds, and #277 is A17's); A06 uses the share 1/4 of round-1 findings about #254's sections instead of
1/5. Each keeps the family's point and its gold is derivable.

## 5. Tags at exactly their minimum

A rewrite for F1, F3 or F5 must not remove a task carrying one of these tags without replacing the tag elsewhere:
`text-predicate` 21, `time-provenance` 21, `edge-direction-supersedes` 20, `edge-other` 21, `reverse-alias` 22, `hop-bound`
20, `quantified-group` 21, `derived-ready` 22, `count-bag` 21, `count-quantified` 20, `arith` 22, `set-op` 21, `search` 30,
`file-link` 31, `tombstone-elsewhere` 20, `ordering` 23, `surface` 22 (prompts; minima of plan §4).

## Holes

None. This check adds no value decided by a measurement; the corpus's own holes are README's HOLE(lqb-display-spelling)
and HOLE(lqb-search-scorer) and store-design's HOLE(lqb-r14-margins).

## Open points for the review

1. **F1 changes the adversarial allocation.** The proposal keeps 40 adversarial tasks and every construct minimum by
   taking one task each from `count-anonymous` and `hand-ready`; the alternative is 42 adversarial tasks, which changes
   [50 §7.4] item 2's prompt total (520) and needs the owner.
2. **F2's layout choice** is the batch lead's; the checker made no rename because plan §7 item 1 gives each batch only its
   own file.
3. **F3 is scored as a defect, F4 as advice.** The line drawn here: a pair is a near-duplicate when it asks the same
   fact of the same referent, or when its gold queries and phrasings differ only in ids and refs; a shared template with
   a different data fact per task (a question as blocker, a re-pointed edge, a lane view) is acceptable.
4. **F6's convention** is the corpus's as written by batch 4; the review adopts it into README §7.5 or replaces it before
   the scorer is built.
