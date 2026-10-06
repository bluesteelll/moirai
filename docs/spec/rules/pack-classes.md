# Pack and brief classes: membership, levels, order, byte budgets, stale-pack notice

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL), rule table for V3 signing ([m0/PLAN §3.2] item 9, [m0/PLAN §5] V3, [m0/PLAN §7] E1); consumed by WP-93b (the class queries on the model's evaluator), WP-94 (GT10 pack fixtures) and, from M9, the engine's pack renderer |
| Sources | [AR §7.4] steps 1-5 and the `brief` paragraph (context-pack algorithm); [AR §7.5] hooks table rows `SessionStart`, `UserPromptSubmit`, `SubagentStart`; [AR §6.2] `complete` bullet (pack-staleness notice); [AR §4.5] step 11; [AR §3.1]-[AR §3.6] (header columns, kinds, edges, derived state, status machines); [AR §6.3] change feed; [AR §8.3] TOKENS rows and the "Stale-pack notice" row; [AR §13] keys `pack.*`, `brief.*`, `hooks.*.budget`, `hooks.delta.max-commits`, `mcp.result-max-bytes`; [50 §2.5]-[50 §2.6] edge types and built-ins; [50 §4.1] `std.delta`, `std.links_broken`, `links_guesses`; [50 §4.3] classes as named queries, `pack_rules_unmerged`; [50 §6.4] node lines, untrusted text, footers; [90 §4.1] branch, actor and tree rows; [90 §6.1]-[90 §6.4] byte unit, both-ends rule, client profiles; [90 §7.5] worker pack; [90 §10.8] keys; [73 F1]-[73 F5], [73 F12], [73 F13], [73 F16], [73 F17], [73 §6]; [72 m2]; [40 §2.9], [40 §6.2]; [70 S5], [70 S6], [70 S17]; [60 §4.2] "Packs" row |
| Format | [RULES/README] |
| Cited as | [RULES/pack-classes]; a row as [RULES/pack-classes PM-006] |

## 1. What this table decides

A **pack** is the text moirai places into an agent's context for one task (`moirai pack T --role R`, MCP `pack`), and a
**brief** is the same machine with fixed classes for a session ([AR §7.4]). This file decides, for every pack kind:

- **which nodes enter** it: the candidate classes C1-C8 of the role pack (`pack-members`, `pack-header`), the classes of
  the brief (`brief-classes`), the hook role pack (`hook-pack`) and the prompt and resume deltas (`delta-rules`);
- **at which level** each node renders (L0, L1, L2, or an id in an ids line) and **what a level contains**
  (`pack-levels`, `pack-render`);
- **in which order** classes and nodes are emitted, and in which order the budget is filled (`pack-order`, `pack-fill`);
- **the budgets**, all in UTF-8 bytes, with their configuration keys, transport ceilings and quotas (`pack-kinds`,
  `pack-budgets`, `pack-ceilings`, `pack-quotas`, `pack-floors`, `pack-bytes`);
- **the stale-pack notice**: the digest a pack prints, and what `complete` and `apply` report when a caller passes it
  back (`notice-sets`, `notice-rules`, `notice-digest`, `notice-entry`, `notice-modes`).

The reference model checks **class membership and the render-once rule** by evaluating these rows over its materialised
state ([60 §4.2] "Packs" row; [AR §8.3] "Node rendered in more than one pack class: 0, pack fixtures against the model").
The class queries are also named LQ queries (`pack_header`, `pack_rules`, ..., [50 §4.3]) whose text [LQ/std] (WP-19)
freezes; the model evaluates both forms and they must agree ([OP-1]). Quotas, degradation and byte accounting are the
engine's renderer ([50 §4.3]: "stays in code"); they are fixed here so that the renderer, the GT10 pack fixtures and the
owner read one definition. The frozen output strings (header layout, footers, markers) belong to [F19]; this file
fixes their content and order, not their spelling.

Who may **write** anything a pack reports is [RULES/role-write-policy]; the role names used below are its `role-rows`.

## 2. How to read a row

Every table follows [RULES/README]. In addition:

- **Evaluation.** `pack-members`, `brief-classes`, `hook-pack` and `notice-sets` are procedure tables: each row is one
  model function tagged `rule: <row id>`, which returns the row's candidate set; the `definition` cell is the predicate
  the owner checks. A node that satisfies several rows is one candidate (RN-001).
- **Terms.** Every name used in a `definition` cell (`T`, `R`, `B`, `auth(n)`, `sub(T)`, ...) is defined in
  `pack-terms`. LQ built-ins (`subtree`, `ancestors`, `glob_match`, `relevant_to`) have their [50 §2.6] meaning.
- **Roles.** A `roles` cell holds role names of [RULES/role-write-policy] `role-rows`, `*` (every role), or `other`
  (every role that no other row of the same class and part names).
- **Views.** `B`: the state at tip(B). `M`: the state at tip(main). `U`: main's version of the nodes in the set `U`
  (PT-011). `feed`: rows of the change feed ([AR §6.3]).
- **Levels.** `ID` (the id alone, inside one ids line of its class), `L0`, `L1`, `L2` (`pack-levels`); `crit-map`:
  `critical` -> L2, `high` -> L1, otherwise L0 ([AR §7.4] C2); `crit-map-l1`: `critical` or `high` -> L1, otherwise L0
  ([AR §7.4] C7 "criticality-ordered L1/L0"). Levels order as ID < L0 < L1 < L2.
- **Order keys** (`pack-order`): `crit-asc` (critical first), `authr-asc` (owner first), `sev-asc` (blocker first),
  `id-asc`, `seq-asc`, `rev_seq-desc` (most recently touched first), `hop-asc`, `depth-asc`, `doc-position` (pre-order
  of the doc tree by `order`, then id), `part-order` (the parts in the order the class's rows list them), `rank-asc`,
  `prop-last` (every entry without prop(n) before every entry with it, PT-033; spec sync 3).
- **Bytes.** Every size is UTF-8 bytes of the emitted text, LF line ends ([90 §6.2]); `B` in a note means bytes.

## 3. Terms

<!-- table: pack-terms -->
| row | term | sort | basis | source | definition |
|---|---|---|---|---|---|
| PT-001 | T | input | design | [AR §7.4] step 1; [AR §7.1] `pack` | The target node: the `ID` of `pack` (MCP `id`). Any kind; the design uses tasks and plan docs. |
| PT-002 | R | input | design | [AR §7.1] `pack --role`; [AR §7.5] `SubagentStart` | The role: `--role` (MCP `role`). For the hook role pack, the role label of the dispatch marker or `agent_type`, or `unknown`; for a dispatched worker's `SessionStart`, the role of `MOIRAI_LEASE`. |
| PT-003 | P | input | design | [AR §7.4] step 1 | The phase: `--phase` (MCP `phase`); absent when not given. |
| PT-004 | B | input | design | [90 §4.1] Branch row; [AR §5a.4] | The branch resolved in the order of record of [90 §4.1] (explicit branch, the presented lease's branch, sandbox or stamp binding, `MOIRAI_BRANCH`, marker, client and directory binding, git-worktree hint, `default-branch`). |
| PT-005 | A | input | design | [90 §4.1] Actor row; [AR §7.4] C2 | The agent: the resolved actor, the presented lease's holder first. Keys `mark(A)` and `cursor(A,T)`. |
| PT-006 | E | input | design | [AR §7.4]; [90 §6.4] | The effective byte budget of this pack (PX-001). |
| PT-007 | k | input | design | [AR §7.4] step 1; [AR §7.1] `--since-round` | The round: `--since-round` (MCP `since_round`) when given; otherwise the `round` of the verdict v visible on view B with (v)-[:ABOUT]->(T) or (v)-[:GATES]->(T) whose `created` seq is greatest; 0 when there is none. |
| PT-008 | LB | input | derived | [AR §3.2] `lane`; [AR §5a.4] | The lane of B: the live `lane` node on view B with `moirai_branch` = B and status not in {`merged`, `abandoned`}; absent for `main` and for a branch no lane binds. |
| PT-009 | U | set | design | [50 §4.3] `pack_rules_unmerged`; [AR §7.4] C2 | Empty when B = main. Otherwise the nodes that `diff(tip(B)...tip(main))` yields with side `theirs` or `both`: changed on main since the merge base and not yet merged into B. |
| PT-010 | live(n) | pred | design | [AR §3.1] flags | n exists on the view and its `deleted` flag is clear. |
| PT-011 | auth(n) | pred | derived | [50 §4.1] `notes`; [AR §3.2] status sets | live(n), and status(n) is the authoritative status of its kind: `rule` active, `decision` accepted, `note` active, `doc` current. False for every other kind. A `conflicted` node stays authoritative and renders by RN-006. |
| PT-012 | at-rpl(r) | pred | design | [AR §7.4] C2 "applies_to ∩ {R, P, lane, *} ≠ ∅"; [AR §3.2] `applies_to` | `applies_to(r)` is empty, or contains `*`, or contains `role:R`, or (P is given and it contains `phase:P`), or (LB exists and it contains `lane:LB`). The dimensions are joined by OR, as the design's set intersection reads ([OP-13]). |
| PT-013 | at-role(r) | pred | design | [50 §4.3] `pack_rules_unmerged` | `applies_to(r)` is empty, or contains `*`, or contains `role:R`. |
| PT-014 | ruling(n) | pred | proposed | [AR §7.4] C3, step 3; [AR §7.3] owner row; [OP-3] | An owner ruling: kind(n) in {`rule`, `decision`, `note`}, `authority(n)` = owner, and auth(n). An owner answer to a question counts through the decision or note that `ANSWERS` it. |
| PT-015 | about(n,S) | pred | design | [50 §2.5] `ABOUT` | An edge (n)-[:ABOUT]->(m) exists on the view with m in S. See [OP-2] for the source kinds. |
| PT-016 | sub(X) | set | design | [50 §2.6] `subtree` | X and its `CHILD_OF` descendants, on view B. |
| PT-017 | anc(X) | set | design | [50 §2.6] `ancestors` | The strict ancestors of X by the `parent` column, on view B. |
| PT-018 | files(T) | set | design | [AR §3.2] `files_owned`; [AR §7.4] C7 | `T.files_owned` on view B when kind(T) = task; empty otherwise. |
| PT-019 | infile(f) | pred | design | [AR §7.4] C7; [40 §6.2] reverse index | f is a live `artifact` on view B with root `project` whose `path` matches some glob of files(T) by `glob_match`. |
| PT-020 | overlap(n) | pred | design | [AR §7.4] C7; [50 §2.6] `applies` | Some `path:` glob of `applies_to(n)` and some glob of files(T) overlap as `applies(k, glob)` defines ([70 S17]: a `GLOBIDX` range probe by literal prefix). |
| PT-021 | base(T) | set | proposed | [AR §7.4] C4 "sections reachable via implements/about"; [OP-5] | If kind(T) = doc: the descendants of T of kind doc. If kind(T) = task: every d of kind `doc` or `decision` with (a)-[:IMPLEMENTS]->(d) for some a in {T} ∪ anc(T), plus the descendants of kind doc of each such doc. Otherwise empty. |
| PT-022 | hop(d) | fn | proposed | [AR §7.4] step 3 "C4 within two depends_on hops"; [OP-5] | The least number of `DEPENDS_ON` edges, followed in either direction, from a member of base(T) to d; 0 for members of base(T); undefined when no such path exists. |
| PT-023 | spec(T) | set | proposed | [AR §7.4] C4; [OP-5] | base(T) together with every doc d for which hop(d) is defined. |
| PT-024 | changed(d) | pred | design | [AR §7.4] C4 critic; [AR §3.2] `changed_in_round` | `changed_in_round(d)` > k. |
| PT-025 | S6 | set | proposed | [AR §7.4] C6 "for the lane and main"; [OP-6] | sub(T) ∪ anc(T), plus LB when it exists. |
| PT-026 | own(n) | pred | proposed | [AR §7.4] C5 "own previous findings"; [50 §2.5] `created_role` (F4); [OP-4] | CREATOR(n).role = R: the node was created under the same role, in any round and by any agent. |
| PT-027 | mark(A) | input | design | [AR §7.4] C2; [AR §7.5] `SubagentStart`; [AR §4.3] `SessionMark`; [F05 §9.14]; [F11 §13.2] | The latest lazy `SessionMark` record `{agent, rule ids, rev}` whose agent is A: the `SESSMARKS` row (session, A) of the session the pack runs in, which each record replaces; absent when none. |
| PT-028 | cursor(A,T) | input | proposed | [AR §7.4] C8 "delta since this (agent, T) cursor"; [F05 §9.11]; [F11 §13.1]; [OP-8] | The seq of the latest lazy pack-cursor record for (A, T) (PX-011): the `cursor_seq` of the `CURSORS` row (session, A, `feed` 2, `#N` of T) of the session the pack runs in. Absent when that row is absent (none appended, dropped by retention, or T re-keyed to a new `#N`). |
| PT-029 | crit(n) | fn | design | [AR §3.1] `criticality` | 0 critical, 1 high, 2 normal, 3 low. |
| PT-030 | authr(n) | fn | design | [AR §7.4] C2 "owner > orchestrator > measured > research > agent" | 0 owner, 1 orchestrator, 2 measured, 3 research, 4 agent. |
| PT-031 | sev(f) | fn | design | [AR §3.2] `finding.severity` | 0 blocker, 1 important, 2 optional. |
| PT-032 | rev | input | design | [50 §6.4] header "`rev <seq>`" | The seq of the commit at tip(B) that the pack read. |
| PT-033 | prop(n) | pred | design | [AR §11] OQ-A-8 "packs do not hide proposed records"; [AR §3.2] status sets; [RULES/status-machines GR-019]; [OP-21] | live(n), kind(n) in {`rule`, `decision`}, and status(n) = `proposed`: a knowledge record that waits for review. Packs show such a record, marked as proposed (RN-018), where its authoritative counterpart would be shown, and never as authoritative: auth(n) and ruling(n) stay false for it, so it is never protected (RN-004), never in the stale-pack notice sets (NS rows) and never counted by PH-008 (spec sync 3). |

## 4. Pack kinds, budgets and ceilings

<!-- table: pack-kinds -->
| row | pack | trigger | classes | budget_key | default_bytes | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| PK-001 | role-pack | cli-pack, mcp-pack | C1, C2, C3, C4, C5, C6, C7, C8 | pack.budget.<role> | per-role | design | [AR §7.4]; [AR §7.1]; [AR §7.2] `pack` | Default N from `pack-budgets`; `--budget N` (MCP `budget`) replaces it; the ceiling of PX-001 always applies. |
| PK-002 | rules-reshow | cli-pack-rules | C1, C2 | pack.budget.<role> | per-role | design | [AR §7.4] C2 "`moirai pack 51 --rules`"; [73 F4] | The C2 rules in full: RN-009 is off. |
| PK-003 | more-page | cli-pack-more, mcp-pack-more | C1, C2, C3, C4, C5, C6, C7, C8 | pack.budget.<role> | per-role | proposed | [AR §7.4] step 4 "`more: moirai pack 51 --more`"; [OP-9] | The complement page: PX-012. |
| PK-004 | hook-role-pack | hook-subagent-start, hook-session-start-worker | HP | hooks.subagent-start.budget | 3000 | design | [AR §7.5] `SubagentStart`; [90 §7.5]; [AR §13] | The same key bounds a dispatched worker's `SessionStart` role pack (`hooks.session-start.worker-pack` = true). |
| PK-005 | brief | cli-brief, mcp-brief, hook-session-start-startup, hook-session-start-clear, hook-session-start-compact | BR | brief.budget | 8000 | design | [AR §7.4] brief paragraph; [AR §7.5] `SessionStart`; [AR §13] `brief.budget` (size <= 9,500) | - |
| PK-006 | brief-more | cli-brief-more, mcp-brief-more | BR | brief.budget | 8000 | proposed | [AR §7.1] `brief --more`; [OP-9] | The complement page, as PX-012 over BR rows. |
| PK-007 | resume | hook-session-start-resume | DL | hooks.delta.budget | 600 | design | [AR §7.5] `SessionStart` "resume <= 600 B"; [73 F12]; [OP-19] | Header plus the delta since the session cursor. |
| PK-008 | prompt-delta | hook-user-prompt-submit | DL | hooks.delta.budget | 600 | design | [AR §7.5] `UserPromptSubmit`; [AR §13] | 0 bytes when empty. |
| PK-009 | stale-notice | cli-complete, mcp-complete, cli-apply | NS | - | 600 | design | [AR §6.2] `complete`; [AR §13] `pack.staleness-notice` "T: <= 600 B"; [72 m2] | A fixed bound, not a key. Part of the write's result, not a separate result. |

<!-- table: pack-budgets -->
| row | role | key | default_bytes | final_at | basis | source | note |
|---|---|---|---|---|---|---|---|
| PB-001 | developer | pack.budget.developer | 16000 | M9 | design | [AR §7.4]; [AR §13] `pack.budget.<role>` | Provisional default; M9's recorded-dispatch test sets the final one ([AR §7.4] "Budgets are set by need"). |
| PB-002 | tester | pack.budget.tester | 16000 | M9 | design | [AR §7.4]; [AR §13] | As PB-001. |
| PB-003 | code-reviewer | pack.budget.code-reviewer | 16000 | M9 | design | [AR §7.4]; [AR §13] | As PB-001. |
| PB-004 | architect | pack.budget.architect | 24000 | M9 | design | [AR §7.4]; [AR §13] | As PB-001. |
| PB-005 | architecture-critic | pack.budget.architecture-critic | 24000 | M9 | design | [AR §7.4]; [AR §13] | As PB-001. |
| PB-006 | other | pack.budget.<role> | 16000 | M9 | design | [AR §13] "16,000 (developer, tester, code-reviewer and roles not listed)" | Every role without its own row, `general-purpose` included. |

In `pack-ceilings`, `default_bytes` is the ceiling under the default configuration and `max_bytes` the largest ceiling
any valid configuration gives: the upper bound of the row's `key` ([CFG §10.8]), for `mcp` capped by that of the
profile's `mcp.result-max-bytes.<client>` (36,000 for `codex`, [CFG §10.8]); a row without a key has a fixed bound, and
`none` means no ceiling. The model checks both columns against the registry.

<!-- table: pack-ceilings -->
| row | surface | client | key | default_bytes | max_bytes | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| PE-001 | cli | * | pack.cli.max-bytes | 24000 | 28000 | design | [AR §13] `pack.cli.max-bytes`; [73 F1]; [90 §6.1] | Store scope; a user file may lower it. A value above 28,000 is refused by `config set` (exit 2). |
| PE-002 | file | * | - | none | none | design | [AR §7.1] `pack ... -o FILE`; [AR §13] "(larger needs `-o FILE`)" | With `-o FILE` the pack is written to the file and E = N. |
| PE-003 | mcp | claude | pack.mcp.max-bytes | 25000 | 48000 | design | [AR §13] `pack.mcp.max-bytes`; [90 §6.4] `claude`; [CFG §10.8] | Capped by `mcp.result-max-bytes.claude` (by default `mcp.result-max-bytes`, 25,000). Both keys allow up to 48,000 ([CFG §10.8]); the first draft's `max_bytes` of 25,000 was the default, not the bound (review of WP-90b). |
| PE-004 | mcp | codex | pack.mcp.max-bytes | HOLE(CFG-codex-mcp-result) | 36000 | design | [90 §6.4] `codex`; [90 §10.8] `mcp.result-max-bytes.codex`; [CFG §10.8]; [F19 §3.2] | 25,000 capped by the profile's MCP result ceiling `mcp.result-max-bytes.codex`, whose default is HOLE(CFG-codex-mcp-result), owned by [CFG] (16,000 B, the design value, until measurement 7 decides it); the key allows up to 36,000 for a classic-mode model (review pass 1, A1-57). |
| PE-005 | mcp | generic | pack.mcp.max-bytes | 25000 | 48000 | design | [90 §6.4] `generic`; [CFG §10.8] | As PE-003 under `mcp.result-max-bytes.generic`. |
| PE-006 | hook | claude | - | 10000 | 10000 | design | [90 §6.1] hook-injected context; [90 §6.4] `claude`; [AR §7.5]; [F19 §3.2] | Every hook output, whatever its own budget key says. Review pass 1 (A1-57) split the first draft's one row for every client into PE-006 to PE-008 by [90 §6.4]'s profile table. |
| PE-007 | hook | codex | - | 10000 | 10000 | design | [90 §6.1] hook-injected context; [90 §6.4] `codex`; [AR §7.5]; [F19 §3.2] | As PE-006: Codex's `additionalContextLimit` of 2,500 approximate tokens is 10,000 B per handler ([90 §6.1]). |
| PE-008 | hook | generic | - | 8000 | 8000 | design | [90 §6.4] `generic`; [AR §7.5]; [F19 §3.2] | As PE-006, at the `generic` profile's hook context of 8,000 B. |

<!-- table: pack-bytes -->
| row | rule | basis | source | definition |
|---|---|---|---|---|
| PY-001 | unit | design | [90 §6.2]; [AR §7.4] | Every budget, ceiling and count is the number of bytes of the UTF-8 encoding of the emitted text, LF line ends, no byte-order mark. ASCII costs 1 byte per character, Cyrillic 2. No token estimate is printed ([90 §6.2]); `--explain` prints per-family estimates from the M0 conversion table. |
| PY-002 | whole-text | proposed | [AR §7.4] "the rendered text never exceeds the transport ceiling"; [OP-12] | The count covers the whole emitted text: the header line, every class line, the legend line, the footer and every line ending. |
| PY-003 | used | proposed | [AR §7.4] step 4 header "`15,200/16,000 B`"; [OP-12] | The header prints the count of PY-002 as `used/E B`. The renderer fixes `used` by iteration (the digit count of `used` changes the count at most once), so the printed number equals the final count. |
| PY-004 | both-ends | design | [90 §6.3]; [AR §7.4] step 4 | When anything was dropped or a continuation exists, the first line carries the drop count and the continuation, and the last line repeats both. The same rule holds for the brief, the hook packs and the stale-pack notice. |
| PY-005 | hard-ceiling | design | [AR §7.4]; [90 §6.3] | E never exceeds the surface's ceiling (PE rows) except under `-o FILE` (PE-002), so a harness never cuts a pack delivered alone. |
| PY-006 | ascii | design | [90 §8.1] L5; [50 §6.4] | Every string moirai generates (separators, arrows, markers, footers) is ASCII; user text is rendered as RN-012 says. |
| PY-007 | one-block | design | [90 §6.3] | One text block per result. |
| PY-008 | explain | design | [AR §7.4]; [50 §4.3] | `pack --explain` names, for every rendered or dropped node, its class, the member row (PM, BR or HP id) and the named query behind it. |

## 5. The role pack

### 5.1 Classes, quotas and floors

<!-- table: pack-classes -->
| row | class | rank | name | named_query | basis | source | definition |
|---|---|---|---|---|---|---|---|
| CL-001 | C1 | 1 | header | pack_header | design | [AR §7.4] C1; [50 §4.3] | Always emitted, fixed items (`pack-header`). |
| CL-002 | C2 | 2 | rules | pack_rules | design | [AR §7.4] C2; [50 §4.3] | Rules on B for the role, phase and lane, and critical rules on `main` not yet merged into B (PM-001, PM-002); the proposed ones of both, marked (PM-026, PM-027; spec sync 3). |
| CL-003 | C3 | 3 | target | pack_target | design | [AR §7.4] C3; [50 §4.3] | T, its ancestors, open questions blocking T, owner rulings about T's subtree (PM-003 to PM-006), and proposed owner rulings about it, marked (PM-028; spec sync 3). |
| CL-004 | C4 | 4 | spec | pack_spec | design | [AR §7.4] C4; [50 §4.3] | The effective spec: implemented sections and decisions, per role (PM-007 to PM-014). |
| CL-005 | C5 | 5 | findings | pack_findings | design | [AR §7.4] C5; [50 §4.3] | Findings about T, per role (PM-015 to PM-019). Branch-local ([OP-17]). |
| CL-006 | C6 | 6 | measurements | pack_measurements | design | [AR §7.4] C6; [50 §4.3] | Current pins with their cached staleness, and known reds (PM-020 to PM-022). |
| CL-007 | C7 | 7 | hazards | pack_hazards | design | [AR §7.4] C7; [50 §4.3] | Notes, rules and decisions scoped to or anchored in T's files (PM-023, PM-024), proposed ones marked (PM-029; spec sync 3). |
| CL-008 | C8 | 8 | delta | delta | design | [AR §7.4] C8; [50 §4.1] `std.delta` | Changes since this agent's cursor for T (PM-025). |

<!-- table: pack-quotas -->
| row | class | roles | key | default_pct | basis | source | note |
|---|---|---|---|---|---|---|---|
| PQ-001 | C2 | * | pack.quota.c2 | 15 | design | [AR §7.4] step 3; [AR §13] `pack.quota.*` | Minimum share of E. |
| PQ-002 | C3 | * | pack.quota.c3 | 20 | design | [AR §7.4] step 3; [AR §13] | - |
| PQ-003 | C4 | developer, tester | pack.quota.c4-dev | 30 | design | [AR §7.4] step 3 "C4 >= 30 % developer/tester" | - |
| PQ-004 | C4 | architecture-critic | pack.quota.c4-critic | 40 | design | [AR §7.4] step 3 ">= 40 % critic" | "Critic" is the architecture-critic; C4's critic rows (PM-011, PM-012) name it too. |
| PQ-005 | C4 | other | - | 0 | proposed | [AR §7.4] step 3; [OP-5] | No C4 quota for roles the design does not name (code-reviewer, architect and the rest); C4 still fills in the fill phase. |
| PQ-006 | C5 | * | pack.quota.c5 | 10 | design | [AR §7.4] step 3; [AR §13] | - |
| PQ-007 | C6 | * | - | 0 | design | [AR §7.4] step 3 (no quota listed) | - |
| PQ-008 | C7 | * | - | 0 | design | [AR §7.4] step 3 (no quota listed) | - |
| PQ-009 | C8 | * | - | 0 | design | [AR §7.4] step 3 (no quota listed) | C8 is capped at 10 rows by PM-025. |

A quota row's percentage applies to E: the class may take up to floor(E × pct / 100) bytes in the quota phase
(PX-006). C1 is not a quota: its bytes are charged before any quota (PX-005).

<!-- table: pack-floors -->
| row | class | basis | source | definition |
|---|---|---|---|---|
| PF-001 | C4 | design | [AR §7.4] step 3 "C4 within two depends_on hops" | hop(n) <= 2 (a decision in base(T) has hop 0). |
| PF-002 | C5 | design | [AR §7.4] step 3 "C5 severity >= important or on the same files" | sev(n) <= 1, or an edge (n)-[:AT]->(f) exists with infile(f). |
| PF-003 | C7 | design | [AR §7.4] step 3 "C7 criticality >= normal" | crit(n) <= 2. |

A class without a floor row (C2, C3, C6, C8) has no floor. A floor restricts only the fill phase (PX-007); the quota
phase takes a class's candidates in class order whatever their floor ([AR §7.4] step 3 reads "then the remaining budget
... only with candidates above each class's relevance floor").

### 5.2 Membership

<!-- table: pack-members -->
| row | class | part | roles | view | kinds | level | basis | source | definition |
|---|---|---|---|---|---|---|---|---|---|
| PM-001 | C2 | rules | * | B | rule | crit-map | design | [AR §7.4] C2; [50 §4.3] `pack_rules` | auth(n) and at-rpl(n). |
| PM-002 | C2 | unmerged | * | U | rule | L2 | design | [AR §7.4] C2 "critical rules on `main` not yet merged into B marked `~main`"; [50 §4.3] `pack_rules_unmerged` | n in U, and on view M: auth(n), crit(n) = 0 and at-role(n). Rendered from main's version with the `~main` marker (RN-016). Empty when B = main. |
| PM-026 | C2 | proposed | * | B | rule | L0 | design | [AR §11] OQ-A-8; [AR §7.4] C2; [OP-21] | prop(n) and at-rpl(n). Rendered with `~proposed` (RN-018), after the authoritative rules (PO-001). |
| PM-027 | C2 | proposed-unmerged | * | U | rule | L0 | derived | [AR §11] OQ-A-8; [AR §7.4] C2 `~main`; [OP-21] | n in U, and on view M: prop(n), crit(n) = 0 and at-role(n): PM-002's set for proposed rules. Rendered from main's version with `~main` and `~proposed` (RN-016, RN-018). Empty when B = main. |
| PM-003 | C3 | target | * | B | * | L2 | design | [AR §7.4] C3 | n = T. Content by PL-010 for a task, by the kind's L2 content (PL-004, PL-007) otherwise. |
| PM-004 | C3 | ancestors | * | B | * | L0 | design | [AR §7.4] C3 "ancestors L0" | n in anc(T). |
| PM-005 | C3 | questions | * | B | question | L1 | design | [AR §7.4] C3 "open questions blocking T at L1 with options"; [50 §4.3] `pack_target` | status(n) = open and an edge (n)-[:BLOCKS]->(T) exists. |
| PM-006 | C3 | rulings | * | B | rule, decision, note | L1 | proposed | [AR §7.4] C3 "owner rulings about the subtree at L1 (verbatim, never truncated)"; [50 §4.3] `pack_target`; [OP-2]; [OP-3] | ruling(n) and about(n, sub(T)). |
| PM-028 | C3 | proposed-rulings | * | B | rule, decision | L1 | derived | [AR §11] OQ-A-8; [AR §7.4] C3; [OP-3]; [OP-21] | prop(n), `authority(n)` = owner and about(n, sub(T)): an owner ruling that waits for the owner's confirmation ([RULES/role-write-policy] WR-015). Rendered with `~proposed` (RN-018) and escaped but never truncated, like a ruling (RN-012), but not protected (RN-004). |
| PM-007 | C4 | sections | developer | B | doc, decision | L2 | design | [AR §7.4] C4 "developer -> implementation sections L2" | n in spec(T). All implemented sections count as implementation sections ([OP-5]). |
| PM-008 | C4 | sections | tester | B | doc | L2 | proposed | [AR §7.4] C4 "tester -> metrics-and-validation sections"; [OP-5] | n in spec(T) and `targets(n)` is not empty. |
| PM-009 | C4 | sections | tester | B | doc, decision | L0 | proposed | [AR §7.4] C4; [OP-5] | n in spec(T) and n is not a member of PM-008. |
| PM-010 | C4 | baselines | tester | B | measurement | L1 | proposed | [AR §7.4] C4 "baselines with env L2/L1"; [AR §3.2] `measurement.baseline`; [OP-5] | status(n) = current, and either about(n, spec(T)) or n is the `baseline` of a measurement m with about(m, sub(T)). |
| PM-011 | C4 | sections | architecture-critic | B | doc | L2 | design | [AR §7.4] C4 "critic -> sections with `changed_in_round > k` plus `depends_on` dependents L2" | n in spec(T), and changed(n) or an edge (n)-[:DEPENDS_ON]->(d) exists with d in spec(T) and changed(d). |
| PM-012 | C4 | sections | architecture-critic | B | doc, decision | L0 | design | [AR §7.4] C4 "unchanged ones L0 (`unchanged since r<k>`)" | n in spec(T) and n is not a member of PM-011. Rendered with RN-014. |
| PM-013 | C4 | sections | other | B | doc, decision | L1 | proposed | [AR §7.4] C4; [OP-5] | n in spec(T). |
| PM-014 | C4 | artifacts | * | B | artifact | L0 | design | [AR §7.4] C4 "artifacts L0" | live(n) and an edge (n)-[:IMPLEMENTS]->(d) exists with d in base(T). |
| PM-015 | C5 | findings | developer | B | finding | L1 | design | [AR §7.4] C5 "developer -> `confirmed` only (L1 with `failure_scenario`)" | about(n, sub(T)) and status(n) = confirmed. |
| PM-016 | C5 | findings | architecture-critic | B | finding | L0 | design | [AR §7.4] C5 "critic round k+1 -> own previous findings L0"; [OP-4] | about(n, sub(T)), own(n), `round(n)` <= k and status(n) is not refuted. |
| PM-017 | C5 | findings | architecture-critic | B | finding | ID | design | [AR §7.4] C5 "refuted ones as ids with `do not re-raise`" | about(n, sub(T)), own(n) and status(n) = refuted. Rendered by RN-015. |
| PM-018 | C5 | findings | code-reviewer | B | finding | L1 | design | [AR §7.4] C5 "reviewer -> open findings on the same files" | status(n) = open, and about(n, sub(T)) or an edge (n)-[:AT]->(f) exists with infile(f). |
| PM-019 | C5 | findings | other | B | finding | L1 | proposed | [AR §7.4] C5; [OP-7] | As PM-015. |
| PM-020 | C6 | pins | * | B | measurement | L1 | proposed | [AR §7.4] C6 "current pins L1 with their cached staleness verdict or `unverified`"; [OP-6] | status(n) = current, and about(n, S6) or an edge (n)-[:VERIFIES]->(x) exists with x in S6. Staleness by RN-017. |
| PM-021 | C6 | pins | * | M | measurement | L1 | proposed | [AR §7.4] C6 "for the lane and `main`"; [OP-6] | B is not main; n satisfies PM-020's predicate evaluated on view M (with S6 taken from view B) and n is not a member of PM-020. |
| PM-022 | C6 | reds | * | B | finding | L0 | proposed | [AR §7.4] C6 "known reds L0"; [AR §7.3] tester "findings `f_kind=test`"; [OP-6] | `f_kind(n)` = test, status(n) = confirmed and about(n, S6). |
| PM-023 | C7 | globs | * | B | note, rule, decision | crit-map-l1 | design | [AR §7.4] C7 "whose `applies_to` paths intersect T's `files_owned`" | auth(n) and overlap(n). |
| PM-024 | C7 | anchored | * | B | note, rule, decision | crit-map-l1 | design | [AR §7.4] C7 "or that are anchored (`at`) in those files"; [40 §6.2] | auth(n) and an edge (n)-[:AT]->(f) exists with infile(f). |
| PM-029 | C7 | proposed | * | B | rule, decision | L0 | derived | [AR §11] OQ-A-8; [AR §7.4] C7; [OP-21] | prop(n), and overlap(n) or an edge (n)-[:AT]->(f) exists with infile(f). Rendered with `~proposed` (RN-018); PO-006 puts it after the authoritative entries. |
| PM-025 | C8 | delta | * | feed | change | L0 | design | [AR §7.4] C8 "delta since this (agent, T) cursor: L0, max 10"; [50 §4.1] `std.delta`; [OP-8] | The rows of `std.delta(since: cursor(A,T), agent: A)` with its limit replaced by 10: change rows after the cursor whose node is `relevant_to` A and whose actor is not A, ordered by seq. Empty when cursor(A,T) is absent. |

### 5.3 Levels

<!-- table: pack-levels -->
| row | kind | level | content | basis | source | note |
|---|---|---|---|---|---|---|
| PL-001 | * | ID | id | design | [AR §7.4] C2, C5; [73 F16] | The id inside its class's ids line (`also:` lines, RN-009, RN-015). |
| PL-002 | * | L0 | node-line | design | [AR §7.4] step 2 "L0 one line ~ 80 chars"; [50 §6.4] node lines | The node line of [50 §6.4]: id, kind, status, priority for tasks, criticality and authority when not default, quoted title, kind keys, flags. |
| PL-003 | * | L1 | node-line, abstract, key-fields | design | [AR §7.4] step 2 "L1 abstract + key fields ~ 300 chars" | `key-fields` = the content of the kind's own L1 row without `node-line` and `abstract`; none for a kind without such a row. |
| PL-004 | * | L2 | node-line, abstract, key-fields, body | design | [AR §7.4] step 2 "L2 full body" | The body fenced as untrusted text (RN-012). A kind's own L2 row replaces this one. |
| PL-005 | rule | L0 | id, text-first-line | design | [AR §7.4] step 2 "for rules L0 = id + first line of `text`"; [73 F16] | - |
| PL-006 | rule | L1 | text, authority, applies_to | design | [AR §7.4] step 2 "L1 = `text` + authority + `applies_to`" | `owner_quote` is never part of a level (RN-011). |
| PL-007 | rule | L2 | text, authority, applies_to, rationale | design | [AR §7.4] step 2 "L2 adds `rationale`" | - |
| PL-008 | task | L1 | node-line, abstract, status, priority, acceptance, files_owned, lease | proposed | [AR §7.4] C3; [50 §6.4] task node line | Key fields taken from C3's target list. |
| PL-009 | task | L0 | node-line | design | [50 §6.4] | As PL-002; listed so the task rows are complete. |
| PL-010 | task | L2 | node-line, abstract, status, priority, acceptance, files_owned, lease, markers, links, body | design | [AR §7.4] C3 "T at L2 (body, acceptance, `files_owned`, status, lease, `settled`/`deleted` notices, its `at` links with their states)" | `markers`: the settled-elsewhere and deleted-elsewhere notices; `links`: every `AT` anchor with its live state and RN-008 markers. |
| PL-011 | finding | L1 | node-line, abstract, severity, f_kind, round, local_id, failure_scenario | design | [AR §7.4] C5 "L1 with `failure_scenario`"; [50 §6.4] finding node line | - |
| PL-012 | question | L1 | node-line, abstract, q_kind, asked_of, options | design | [AR §7.4] C3 "at L1 with options" | - |
| PL-013 | measurement | L1 | node-line, metric, value, unit, target, env, measured_on, staleness | design | [AR §7.4] C6 "L1 with their cached staleness verdict"; [AR §7.3] tester "env + `measured_on`" | `staleness` by RN-017. |
| PL-014 | decision | L1 | node-line, abstract, what, why | proposed | [AR §3.2] `decision` | - |
| PL-015 | doc | L1 | node-line, abstract, heading, revision, changed_in_round, targets | proposed | [AR §3.2] `doc` | - |
| PL-016 | note | L1 | node-line, abstract, note_kind, applies_to | proposed | [AR §3.2] `note` | - |
| PL-017 | verdict | L1 | node-line, role, round, outcome, return_to | proposed | [AR §3.2] `verdict`; [50 §6.4] | - |
| PL-018 | artifact | L1 | node-line, path, root, artifact_kind, link-state | proposed | [40 §6.2]; [50 §6.4] artifacts "`path` and, on live results, `state`" | - |

### 5.4 Order, header, rendering and fill

<!-- table: pack-order -->
| row | class | keys | basis | source | note |
|---|---|---|---|---|---|
| PO-001 | C2 | part-order, crit-asc, authr-asc, id-asc | design | [AR §7.4] C2 "order criticality desc, authority (owner > ...), id asc"; [OP-21] | Parts: PM-001 (`rules`), then PM-002 (`unmerged`), PM-026 (`proposed`) and PM-027 (`proposed-unmerged`; spec sync 3). RN-010 then moves `contradicts` partners together. |
| PO-002 | C3 | part-order, depth-asc, id-asc | proposed | [AR §7.4] C3; [OP-21] | Parts: target, ancestors (root first), questions, rulings, proposed rulings (PM-028; spec sync 3). |
| PO-003 | C4 | hop-asc, doc-position, id-asc | proposed | [AR §7.4] C4 | Artifacts (PM-014) after sections and baselines, by id. |
| PO-004 | C5 | sev-asc, id-asc | proposed | [AR §7.4] C5 | Ids-level members (PM-017) form the class's last line. |
| PO-005 | C6 | part-order, id-asc | proposed | [AR §7.4] C6 | Parts: pins (PM-020, then PM-021), reds. |
| PO-006 | C7 | prop-last, crit-asc, authr-asc, id-asc | design | [AR §7.4] C7 "criticality-ordered"; [OP-21] | Proposed records (PM-029) after the authoritative entries; PM-023 and PM-024 stay interleaved (spec sync 3). |
| PO-007 | C8 | seq-asc | design | [50 §4.1] `std.delta` "ORDER BY seq" | - |
| PO-008 | * | rank-asc, prop-last, crit-asc, rev_seq-desc, id-asc | design | [AR §7.4] step 3 "then the remaining budget by (class rank, criticality, recency, id)"; [OP-21] | The fill-phase order (PX-007) only; emission uses the class orders above (PX-010). Inside a class, every proposed entry (PT-033) is filled after the authoritative ones (spec sync 3). |

<!-- table: pack-header -->
| row | position | item | when | basis | source | definition |
|---|---|---|---|---|---|---|
| PH-001 | 1 | pack-line | always | design | [AR §7.4] step 4; [90 §6.2]; [90 §6.3] | The first line: T, R, B, rev, `used/E B` (PY-003), `dropped <n>` and the continuation when n > 0 (PY-004), and the digest token (NR-002). Spelling in [F19]. |
| PH-002 | 2 | ahead-behind | branch-not-main | design | [AR §7.4] C1 | Commits of B not in main and of main not in B. |
| PH-003 | 3 | staged | staged-exists | design | [AR §7.4] C1 | Every staging ref `merge/<dst>/from/<src>` whose dst or src is B. |
| PH-004 | 4 | tree | tree-resolved | design | [AR §7.4] C1; [90 §4.1] Tree row | The resolved tree: worktree path, git branch, base, tip, and LB's `target_dir`; `files: no tree bound` when none. |
| PH-005 | 5 | dirty | tree-resolved | design | [AR §7.4] C1 "from `TREES.dirty`, never a worktree scan"; [70 S5] | The tree's dirty count and its age. |
| PH-006 | 6 | do-not-touch | leases-elsewhere | design | [AR §7.4] C1; [AR §6.2] captured `files_owned`; [70 S5] | The `files_owned` globs captured in the live `LEASES` rows whose branch is not B, per lane. Read from lease rows only, O(live leases). |
| PH-007 | 7 | quiet | quiet-on | design | [AR §7.4] C1; [AR §6.6]; [F17 §5.3]; [F03 §3.1] | Quiet state as [F17 §5.3] defines it: `HEAD.flags.quiet` set, a quiet byte of `LOCK` held (the probe rule of [F03 §3.1]; review pass 1, P1-10), or a lane in status `measuring` while `quiet.from-lane-measuring` is true. |
| PH-008 | 8 | critical-count | always | design | [AR §7.4] C1 "global critical-rule count" | The number of rules n on view B with auth(n) and crit(n) = 0, not filtered by role. |
| PH-009 | 9 | link-segment | links-rendered | design | [AR §7.4] C1; [40 §6.2] header segment | Link states counted over the `AT` anchors of every node rendered at L1 or L2 ([OP-18]); only `absent-in-tree (behind)` is folded into a count; every other non-`ok` state is also marked on its link. |

<!-- table: pack-render -->
| row | rule | basis | source | definition |
|---|---|---|---|---|
| RN-001 | render-once | design | [AR §7.4] step 3 "each node renders once"; [73 F16]; [AR §8.3] | A node that several member rows select is one entry: it renders once, at the highest level any of its rows assigns, in the class of lowest rank among them. |
| RN-002 | also-line | design | [AR §7.4] step 3 "the other classes list its id (`also: #212`)" | Every other class that selected the node lists its id in one `also:` line of that class. |
| RN-003 | degrade-before-drop | design | [AR §7.4] step 3 | An entry that does not fit at its level is tried at each lower level down to L0 before it is dropped (PX-006, PX-007). |
| RN-004 | protected | design | [AR §7.4] step 3 "owner rulings and critical rules never below L1"; [OP-21] | An entry with ruling(n), or a rule with auth(n) and crit(n) = 0, never renders below L1, except by RN-009. An entry with prop(n) is never protected (PT-033; spec sync 3). It is taken in PX-005's protected pass; if E cannot hold it at L1 it is dropped whole and counted. |
| RN-005 | whole-entry | design | [AR §7.4] step 3 "never cut mid-text" | An entry renders whole at one level or not at all. |
| RN-006 | conflicted | design | [AR §7.4] step 3 (N15) | A `conflicted` knowledge node renders as one L1 line with the base text and `~conflicted (moirai resolve '#N.<field>')`, never with conflict markers. |
| RN-007 | markers-kept | design | [AR §7.4] step 3 "`suspect`/`stale` items keep their marker rather than being dropped" | Flags (`SUSPECT`, `CONFLICTED`, `SETTLED-ELSEWHERE`, `DELETED-ELSEWHERE`) and cached staleness stay on the line at every level. |
| RN-008 | link-marker | design | [AR §7.4] step 3; [40 §6.2]; [73 F13]; [F19 §4.6] | A link that is not `ok` renders one ASCII marker, spelled and bounded (at most 50 B) by [F19 §4.6]: its state and `verify <handle>`, or `confirm <handle>` on an accepted guess, the handle being the file node's `#N` for a file-level state and the anchor's `aN` for an anchor-level one; never a command that accepts a guess. The legend lines are [F19 §4.6] rule 4's, one per pack: `verify #N: moirai file where N --evidence` when any marker carries `verify`, and the `confirm` line when any carries `confirm`. Non-`ok` links of critical rules and owner rulings are never dropped below L1. |
| RN-009 | mark-ids | design | [AR §7.4] C2; [73 F4] | When mark(A) exists, every PM-001 or PM-002 member whose id is in mark(A)'s rule ids and whose `rev_seq` on its view is at most mark(A)'s rev renders at ID level in one line (`rules: <n> critical shown at start (#...) \| moirai pack <T> --rules`). A member added or changed after the mark renders by its row. No mark, or `--rules` (PK-002): every member renders by its row. |
| RN-010 | contradicts-adjacent | design | [AR §7.4] C2 "`contradicts` pairs shown together" | After ordering C2, each rule with a `CONTRADICTS` edge to an earlier C2 entry is moved to directly after that entry; ties keep PO-001's order. |
| RN-011 | owner-quote | design | [AR §7.4] step 2; [73 F16] | `owner_quote` is shown only in C3, and only for a ruling whose `text` is empty; otherwise the ruling's line carries `quote: show N`. |
| RN-012 | untrusted-text | design | [50 §6.4] untrusted text; [AR §7.4] C3 "verbatim, never truncated" | Free text is quoted with `"`, `\`, CR, LF, TAB and control characters escaped, and truncated at 120 bytes with `...` on L0 and L1 lines; bodies are fenced as untrusted text. Owner rulings in C3 are escaped but never truncated. |
| RN-013 | ascii | design | [90 §8.1] L5 | As PY-006. |
| RN-014 | critic-unchanged | design | [AR §7.4] C4 | PM-012 lines carry `unchanged since r<k>`. |
| RN-015 | refuted-ids | design | [AR §7.4] C5 | PM-017 members form one ids line ending `do not re-raise`. |
| RN-016 | unmerged-marker | design | [AR §7.4] C2; [AR §5d.2] | PM-002 lines carry `~main`. |
| RN-017 | staleness-cached | design | [AR §7.4] C6; [70 S6]; [50 §2.6] `staleness` | Staleness is read from the `ANCESTRY` cache only; with nothing cached the line shows `unverified` and the `moirai check` command. Never computed on the pack path. |
| RN-018 | proposed-marker | design | [AR §11] OQ-A-8 "packs do not hide proposed records"; [OP-21] | An entry with prop(n) carries the marker `~proposed` on its line at every level, after its other markers (RN-007, RN-016), so a record that waits for review shows as such, never as authoritative; a rule's L0 line (PL-005) otherwise shows no status. The marker's spelling is [F19]'s (spec sync 3). |

<!-- table: pack-fill -->
| row | step | action | basis | source | definition |
|---|---|---|---|---|---|
| PX-001 | 1 | budget | design | [AR §7.4] step 4; [AR §13]; [90 §6.4] | N = `--budget` (MCP `budget`) when given, else the role's PB row. E = N under `-o FILE`; otherwise E = min(N, the surface's ceiling from PE rows for the calling client profile). |
| PX-002 | 2 | resolve | design | [AR §7.4] step 1 | Resolve T, R, P, B, A, k, LB, mark(A) and cursor(A,T). |
| PX-003 | 3 | candidates | design | [AR §7.4] step 2; [60 §4.2] | Evaluate every PM row whose `roles` cell matches R; each selected node gets (class, level) from the row. |
| PX-004 | 4 | merge | design | [AR §7.4] step 3 | Apply RN-001 and RN-002, then RN-009. Order each class by its PO row and RN-010. |
| PX-005 | 5 | reserve | proposed | [AR §7.4] steps 3-4; [OP-15] | Charge C1 (every PH row that applies), the legend line and the footer first. Then the protected pass: every RN-004 entry, in the PO-008 order, at its level or degraded to L1, never lower. |
| PX-006 | 6 | quotas | design | [AR §7.4] step 3 "per-class minimum quotas" | For each class with a PQ row matching R and pct > 0, in rank order: take its remaining entries in class order while the class's bytes stay within floor(E × pct / 100); an entry that does not fit is degraded (RN-003) or left for step 7. |
| PX-007 | 7 | fill | design | [AR §7.4] step 3 "then the remaining budget ... only with candidates above each class's relevance floor ... the budget is a cap, not a target" | Every entry not yet taken whose class has no PF row or satisfies it, in the PO-008 order: take it at its level if it fits in what remains of E, else degraded (RN-003), else it stays out. Entries below their class's floor are never taken here. |
| PX-008 | 8 | drop | design | [AR §7.4] step 4; [01 §7] L1 via [AR] | Every entry not taken is dropped; the footer lists the dropped ids per class, and PY-004 holds. Nothing is truncated silently. |
| PX-009 | 9 | degrade-bookkeeping | proposed | [AR §7.4] step 3 | An entry taken below its assigned level is not dropped and is not counted as dropped; `--more` (PX-012) re-renders it at its assigned level. |
| PX-010 | 10 | emit | design | [AR §7.4] step 4 "deterministic order (prompt-cache friendly)" | Emit C1, then C2 to C8 in rank order; inside a class, the entries in class order (not fill order), each at the level it was taken; then the legend and the footer. |
| PX-011 | 11 | cursor | proposed | [AR §7.4] C8; [AR §6.5] lazy kinds "read cursors"; [AR §5d.1] per-session cursors; [F05 §9.11]; [F11 §13.1]; [OP-8] | After the pack is delivered, one lazy pack-cursor record (A, T, rev) is appended: a `Lazy` record of `sub` 2, `feed` 2, `task` = T's `#N` and `cursor_seq` = rev (PT-032), in the session the pack runs in. It is runtime state, never versioned. The layer that delivers the pack appends it, the `SubagentStart` hook or the MCP server, as [AR §5d.1] keeps the per-session cursors; the `pack` verb never does and stays a read that appends nothing ([40] I-F5, [API §14.1]). Decided by the owner on 2026-09-28 ([OP-8]). No M0 command is a delivering layer, so C8 is empty in the engine and the model alike. |
| PX-012 | 12 | more | proposed | [AR §7.4] step 4 "`more: moirai pack 51 --more`"; [OP-9] | `--more` recomputes steps 1-4 at the current view and emits C1 and, in PO-008 order, the entries the base pack drops or degrades, at their assigned levels, within E. If that page must drop again, its footer names `--budget` and `-o FILE`; there is no third page. |
| PX-013 | 13 | record-run | design | [AR §7.4] step 5 | With `--record-run` only: one commit of `consumed` edges with `pinned_commit` from the run to every entry rendered at L1 or L2; who may do so is [RULES/role-write-policy] WV-040. Without it the pack writes nothing versioned. |

## 6. Hook packs, brief and deltas

<!-- table: hook-pack -->
| row | position | item | level | basis | source | definition |
|---|---|---|---|---|---|---|
| HP-001 | 1 | header | L0 | design | [AR §7.5] `SubagentStart`; [90 §6.3] | B, the bound lane, rev; the drop count and continuation when anything is dropped. |
| HP-002 | 2 | critical-rules | L2 | design | [AR §7.5] `SubagentStart` "critical rules for the role label (only `applies_to = *` rules while the label is unknown)"; [73 F4] | Rules n on view B with auth(n), crit(n) = 0 and at-role(n) for R = the role label. When R is `unknown`: only rules whose `applies_to` is empty or contains `*`. Degrade to L1, never lower (RN-004); order by PO-001. |
| HP-003 | 3 | unmerged-rules | L2 | proposed | [AR §7.4] C2 "owner rulings are never hidden by branching"; [OP-14] | PM-002's predicate for the same R; rendered with `~main`. Empty when R is `unknown`. |
| HP-004 | 4 | pack-reference | L0 | design | [AR §7.5] "the `pack` reference for the task in the dispatch table" | When the marker (or `MOIRAI_LEASE`) names a task: one line `moirai pack <T> --lease <L>`. |
| HP-005 | 5 | sync-line | L0 | design | [AR §7.5] `SubagentStart` (D5) | The outcome of `sync --check` for the bound lane: nothing when an auto-applied sync succeeded, else `behind main: <n> commits, <c> conflicts ... -> moirai sync`. Not emitted by a worker's `SessionStart`. |
| HP-006 | 6 | mark | - | design | [AR §7.5]; [AR §4.3] `SessionMark` | Appends one lazy `SessionMark` {A, ids of the rules HP-002 and HP-003 rendered at L1 or L2, rev}. Rules the budget dropped are not in the mark, so the agent's pack renders them in full. |
| HP-007 | 7 | proposed-rules | ID | derived | [AR §11] OQ-A-8; [AR §7.5] `SubagentStart`; [OP-21] | The rules n on view B with prop(n), crit(n) = 0 and at-role(n) for R = the role label (only `applies_to` empty or `*` while R is `unknown`, as HP-002), in one ids line marked `~proposed` (RN-018); nothing when there is none. They are not in the mark (HP-006), which holds only rendered rules (spec sync 3). |

The worker variant (`hooks.session-start.worker-pack` = true, [90 §7.5]) emits HP-001 to HP-004, HP-006 and HP-007 for
the role and task of `MOIRAI_LEASE` and mints no orchestrator lease. Budget: PK-004.

<!-- table: brief-classes -->
| row | class | rank | named_query | kinds | level | basis | source | definition |
|---|---|---|---|---|---|---|---|---|
| BR-001 | header | 1 | - | - | L0 | design | [AR §7.4] brief; [90 §4.3] "prints the lease id in the brief's header" | B, rev, the drop count and continuation (PY-004), and `orchestrator lease L-<n>` when this `SessionStart` minted it. |
| BR-002 | checkpoint | 2 | brief_lanes | note | L1 | proposed | [AR §7.4] brief "checkpoint per open campaign"; [OP-10] | For each open campaign c (a live task with no parent whose status is not done or cancelled): the note n with `note_kind` = checkpoint, auth(n) and an edge (n)-[:DERIVED_FROM]->(x) with x in sub(c), of greatest id. |
| BR-003 | lanes | 3 | brief_lanes | lane | L1 | design | [AR §7.4] brief "live lanes (branch, ahead/behind, staged merges, live leases, dirty count with its age)" | Live lanes: status not in {`merged`, `abandoned`}. The line carries the bound branch, ahead/behind main, staged merges, live leases and the dirty count with its age. |
| BR-004 | runs | 4 | brief_lanes | run | L0 | design | [AR §7.4] brief "runs in flight" | status(n) = running. |
| BR-005 | merge-queue | 5 | brief_lanes | lane | L0 | design | [AR §7.4] brief "merge queue (`merge_after`)" | Live lanes with a `MERGE_AFTER` edge, each with the lanes it waits for. |
| BR-006 | triage | 6 | brief_triage | task | L0 | design | [AR §7.4] brief; [50 §4.3] `brief_triage` | `settled_elsewhere` or `deleted_elsewhere` or `has_dangling`. |
| BR-007 | questions | 7 | brief_questions | question | L1 | design | [AR §7.4] brief "open owner questions" | status(n) = open and `asked_of` = owner. |
| BR-008 | critical | 8 | brief_critical | rule, note | L1 | design | [AR §7.4] brief "top critical rules/hazards" | auth(n) and crit(n) = 0; with `--role R`, rules also need at-role(n). When B is not main, also PM-002's unmerged critical rules with `~main`. Never below L1 (RN-004). |
| BR-009 | stale-summaries | 9 | brief_critical | note | L0 | proposed | [AR §7.4] brief "stale summaries"; [OP-10] | `note_kind(n)` = summary, auth(n) and `suspect(n)`; git staleness is never computed on the brief path (RN-017). |
| BR-010 | verdicts | 10 | brief_verdicts | verdict | L0 | design | [AR §7.4] brief "verdicts since last session" | Verdicts whose `created` seq is greater than the session cursor. |
| BR-011 | links | 11 | links_broken | artifact | L0 | design | [AR §7.4] brief "at most three non-`ok` link lines ... with a `moirai links check` footer"; [40 §6.2] | At most 3 lines of `std.links_broken` over the brief's scope, in [40 §2.9]'s severity order, then id; plus the count of `links_guesses` rows and of interrupted file operations; the `moirai links check` footer when more exist. |
| BR-012 | proposed | 12 | brief_proposed | rule, decision | L0 | design | [AR §11] OQ-A-8 "The brief gains a 'proposed / needs review' line"; [LQ/std §6.2]; [F19 §4.7]; [API §14.5]; [OP-21] | One line, the brief's "proposed / needs review" line: the number of nodes n on view B with prop(n) (with `--scope ID`, those in sub(ID) or about it), the ids of the newest five by `created` seq, newest first, and the command that lists them all, `moirai q proposed` ([LQ/std §4.24], [API §14.5]); no line when the number is 0. It is charged with BR-001, before any class, so it is never dropped, and it is the brief's last line before the drop line ([F19 §4.7] rule 3), so the drop line still ends the brief. Spelling: [F19 §4.7] (spec sync 3). |

The brief fills in rank order, each class in its own order, degrading L1 to L0 before dropping (RN-003, RN-004 hold);
the lowest ranks are dropped first, except BR-012's line, which is charged with the header (spec sync 3). `--scope ID`
restricts BR-006 to BR-012 to nodes in sub(ID) or about it.

<!-- table: delta-rules -->
| row | pack | rule | basis | source | definition |
|---|---|---|---|---|---|
| DL-001 | prompt-delta | rows | design | [AR §7.5] `UserPromptSubmit`; [50 §4.1] `std.delta` | The rows of `std.delta(since: session cursor, agent: A)`: change rows relevant to A whose actor is not A, ordered by seq, at most 12. |
| DL-002 | prompt-delta | scan-cap | design | [AR §6.3]; [AR §13] `hooks.delta.max-commits` | At most `hooks.delta.max-commits` (2,000) commits are read; beyond, one line `<n> older changes: moirai changes --since <S>`. |
| DL-003 | prompt-delta | links | design | [AR §7.5] "state changes of links the agent cites or leases"; [40 §6.4] | Cross-branch notices and state changes of links the agent cites or leases are delta rows. |
| DL-004 | prompt-delta | behind-main | design | [73 F12]; [AR §7.5] | The `behind main` line is printed only when N or its counts changed since the session's last hook output. |
| DL-005 | prompt-delta | empty | design | [AR §7.5] "nothing when empty" | No row and no changed `behind main` line: 0 bytes. |
| DL-006 | resume | content | design | [AR §7.5] `SessionStart` resume; [73 F12] | The header line and DL-001's rows since the session cursor, with `moirai brief` named for the full view. |
| DL-007 | * | cursor | design | [AR §7.5] `UserPromptSubmit` | The session cursor advances to the last seq read; it lives in the server's memory or in a lazy record appended with a try-lock that skips when the writer byte is busy. |

## 7. The stale-pack notice

A pack prints a **digest token** (PH-001). `complete` and `apply` accept it back and, when the critical rules, owner
rulings or blockers that concern the task changed since the pack, print a notice ([AR §6.2], [72 m2]). The sets and the
digest below differ from the design's wording "the rev plus the C2/C3 member uids and their `rev_seq`" for two reasons
([OP-11]): blockers are not C2/C3 members although the notice must list them, and C2 depends on the phase, which
`complete` does not know.

<!-- table: notice-sets -->
| row | set | code | view | kinds | basis | source | definition |
|---|---|---|---|---|---|---|---|
| NS-001 | K1 | 1 | B | rule | derived | [AR §6.2] "critical rules"; [AR §7.4] C2; [OP-11] | auth(n), crit(n) = 0, and (`applies_to(n)` is empty, or contains `*`, `role:R`, any `phase:` entry, or `lane:LB` when LB exists). R is the role of the lease presented to `complete` (for `apply --from`, the entry's lease). |
| NS-002 | K1M | 2 | M | rule | derived | [AR §6.2]; [AR §7.4] C2 `~main`; [OP-11] | B is not main, n in U, and on view M the predicate of NS-001 holds. |
| NS-003 | K2 | 3 | B | rule, decision, note | derived | [AR §6.2] "owner rulings"; [AR §7.4] C3 | ruling(n) and about(n, sub(T)), T being the task `complete` finishes. |
| NS-004 | K3 | 4 | B | task, question, verdict | derived | [AR §6.2] "blockers"; [AR §3.3] `blocks`, `gates`; [AR §3.5] `open_blockers_exo`; [50 §2.6] `blockers` | The sources x of an edge (x)-[:BLOCKS]->(T) or (x)-[:GATES]->(T), or of such an edge into some a in anc(T) with x not in sub(a); any status; flagged edges included. |

<!-- table: notice-rules -->
| row | step | rule | basis | source | definition |
|---|---|---|---|---|---|
| NR-001 | 1 | input | proposed | [AR §6.2] "when the caller passes the digest its pack printed"; [OP-11] | The token passed to `complete`, MCP `complete` or an `apply` entry (named `--pack-digest` on the CLI, and `pack_digest` as the MCP `complete` parameter and as the additive `result.v1` field an `apply --from` entry carries; [API §10.5], [API] open point 29) matches `^[0-9]{1,20}-[0-9a-f]{8}$`; any other value is a usage error (exit 2) found before the write, and nothing is written. |
| NR-002 | 2 | token | proposed | [AR §7.4] step 4 "digest of the rev plus ..."; [OP-11] | The pack prints `<rev>-<h>`: rev in decimal without separators, then 8 lower-case hex digits h = the first 4 bytes of BLAKE3-256 over the `notice-digest` bytes computed at the pack's views (tip(B) at rev, tip(main) when the pack read it). |
| NR-003 | 3 | old-views | proposed | [AR §5a.2] reflog; [OP-11] | B_old = the value of ref B after the last ref update whose seq is at most rev; M_old likewise for main. No value at rev means an empty old set. |
| NR-004 | 4 | fast-path | proposed | [72 m2] | Recompute the digest over K1, K1M, K2 and K3 at the current tips; if its 8 hex digits equal h, nothing is listed and no notice is printed. |
| NR-005 | 5 | listed | derived | [AR §6.2]; [AR §8.3] "list every critical rule, owner ruling and blocker changed since the pack digest" | Otherwise a node n is listed when n is in the union of the old sets (on B_old, M_old) and the new sets (on the current tips) and either n is in exactly one of them, or its `rev_seq` now, on the set's view, is greater than rev. |
| NR-006 | 6 | change-type | proposed | [72 m2] | `added` (new sets only), `removed` (old sets only), `changed` (both). |
| NR-007 | 7 | order | proposed | [72 m2] | By set code (K1, K1M, K2, K3), then id ascending; a node listed by two sets appears once, under the lower code. |
| NR-008 | 8 | render | design | [AR §13] `pack.staleness-notice`; [AR §4.5] step 11 | By the `notice-modes` row of the configured mode, inside the write's result, within 600 bytes, with PY-004. |
| NR-009 | 9 | never-refuses | design | [72 m2] "print a notice" | The notice never changes the write, its exit code or its idempotency payload; the token is not part of the canonical bound AST ([AR §6.4]). |
| NR-010 | 10 | foreign-token | proposed | [72 m2] | A well-formed token whose rev exceeds the store's highest seq prints one line `pack digest not from this store` and lists nothing. A token from another task's or role's pack is not refused: the fast path fails and NR-005 lists the changes for this task and role. |

The digest input has no fixed size; the table gives each field's offset from the start of the input. Integers are
little-endian; there is no padding.

<!-- table: notice-digest -->
| row | offset | width | type | name | basis | source | meaning |
|---|---|---|---|---|---|---|---|
| ND-001 | 0 | 21 | bytes | tag | proposed | [OP-11] | The ASCII bytes `moirai-pack-digest-v1`. |
| ND-002 | 21 | 16 | uid | target | proposed | [F07] uid encoding | T's uid, as the canonical form encodes a uid. |
| ND-003 | 37 | 2 | u16 | role_len | proposed | [OP-11] | Byte length of the role name, 1 to 64. |
| ND-004 | 39 | role_len | bytes | role | proposed | [OP-11] | R's name, ASCII. |
| ND-005 | 39+role_len | 4 | u32 | count | proposed | [OP-11] | Number of entries. |
| ND-006 | 43+role_len | 25*count | entry[] | entries | proposed | [OP-11] | `notice-entry` records sorted ascending by (set, uid bytes); one per (set, uid). |

<!-- table: notice-entry -->
| row | offset | width | type | name | basis | source | meaning |
|---|---|---|---|---|---|---|---|
| NE-001 | 0 | 1 | u8 | set | proposed | [RULES/pack-classes NS-001] | The set code: 1 K1, 2 K1M, 3 K2, 4 K3. Other values are reserved. |
| NE-002 | 1 | 16 | uid | uid | proposed | [F07] uid encoding | The member's uid. |
| NE-003 | 17 | 8 | u64 | rev_seq | proposed | [AR §3.1] `rev_seq` | The member's `rev_seq` on the set's view (view M for K1M, view B otherwise). |

<!-- table: notice-modes -->
| row | mode | output | cap_bytes | basis | source | note |
|---|---|---|---|---|---|---|
| NM-001 | off | none | 0 | design | [AR §13] `pack.staleness-notice` | The token is still checked by NR-001 and then ignored. |
| NM-002 | ids | one-line | 600 | design | [AR §13] | One line: the count and the listed ids in NR-007 order, each with its change type; past 600 B the line ends `+<n> more`. |
| NM-003 | lines | per-node | 600 | design | [AR §13] (default `lines`) | A first line with the count, then one line per listed node: change type, set, and the node's L0 line (for `removed`, its line on the old view). Past 600 B the remaining count and `moirai pack <T>` end the notice. |

## 8. Source map

<!-- table: pack-source-map -->
| row | source_row | realized_by | note |
|---|---|---|---|
| PS-001 | AR-7.4-step1-resolve | PT-004, PT-007, PT-008, PX-002 | Ancestor chain, branch, round, ahead/behind. |
| PS-002 | AR-7.4-step2-levels | PL-001, PL-002, PL-003, PL-004, PL-005, PL-006, PL-007, RN-011 | - |
| PS-003 | AR-7.4-C1 | CL-001, PH-001, PH-002, PH-003, PH-004, PH-005, PH-006, PH-007, PH-008, PH-009 | - |
| PS-004 | AR-7.4-C2 | CL-002, PM-001, PM-002, PO-001, RN-009, RN-010, RN-016 | - |
| PS-005 | AR-7.4-C3 | CL-003, PM-003, PM-004, PM-005, PM-006, PL-010, RN-011 | - |
| PS-006 | AR-7.4-C4 | CL-004, PM-007, PM-008, PM-009, PM-010, PM-011, PM-012, PM-013, PM-014, RN-014 | - |
| PS-007 | AR-7.4-C5 | CL-005, PM-015, PM-016, PM-017, PM-018, PM-019, RN-015 | - |
| PS-008 | AR-7.4-C6 | CL-006, PM-020, PM-021, PM-022, RN-017 | - |
| PS-009 | AR-7.4-C7 | CL-007, PM-023, PM-024 | - |
| PS-010 | AR-7.4-C8 | CL-008, PM-025, PX-011 | - |
| PS-011 | AR-7.4-step3-fill | PQ-001, PQ-002, PQ-003, PQ-004, PQ-006, PF-001, PF-002, PF-003, PX-005, PX-006, PX-007, RN-001, RN-002, RN-003, RN-004, RN-005, RN-006, RN-007, RN-008 | - |
| PS-012 | AR-7.4-step4-emit | PX-008, PX-010, PY-003, PY-004, PH-001, NR-002 | - |
| PS-013 | AR-7.4-step5-record | PX-013 | - |
| PS-014 | AR-7.4-budgets | PB-001, PB-002, PB-003, PB-004, PB-005, PB-006, PE-001, PE-002, PE-003, PE-004, PE-005, PE-006, PE-007, PE-008, PY-001 | PE-006 to PE-008: the hook ceilings of [90 §6.4]. |
| PS-015 | AR-7.4-brief | PK-005, BR-001, BR-002, BR-003, BR-004, BR-005, BR-006, BR-007, BR-008, BR-009, BR-010, BR-011 | - |
| PS-016 | AR-7.5-SubagentStart | PK-004, HP-001, HP-002, HP-003, HP-004, HP-005, HP-006 | - |
| PS-017 | AR-7.5-UserPromptSubmit | PK-008, DL-001, DL-002, DL-003, DL-004, DL-005, DL-007 | - |
| PS-018 | AR-7.5-SessionStart-resume | PK-007, DL-006 | - |
| PS-019 | AR-6.2-complete-notice | PK-009, NS-001, NS-002, NS-003, NS-004, NR-001, NR-002, NR-003, NR-004, NR-005, NR-006, NR-007, NR-008, NR-009, NR-010, ND-001, NE-001, NM-001, NM-002, NM-003 | - |
| PS-020 | AR-8.3-render-once | RN-001 | The model's GT fixture: no node in two classes. |
| PS-021 | AR-11-OQ-A-8 | PT-033, PM-026, PM-027, PM-028, PM-029, PO-001, PO-002, PO-006, PO-008, RN-004, RN-018, BR-012, HP-007 | Packs show proposed records as proposed; the brief's "proposed / needs review" line (spec sync 3). |

## Coverage

Rule tables specify semantics; the frozen strings (header, footer, marker and notice spellings) are [F19]'s, the
`SessionMark` and pack-cursor record layouts are [F05]'s ([F05 §9.14], §9.11), their rows and the `LEASES` rows are
[F11]'s ([F11 §13.2], §13.1, §6) and the uid encoding is [F07]'s.
The one byte-level structure this file defines is the digest input of §7 (ND and NE rows), which no checklist row
reserves: it is hashed, never stored.

| Checklist row | Covered by |
|---|---|
| [90 §10.1] "Output contract": byte units in headers and footers, no token estimate, the both-ends rule, ASCII | PY-001, PY-003, PY-004, PY-006, PH-001, NR-008 (content and order; strings in [F19]) |
| [60 §2.5] priority-audit Log row: record kind `SessionMark` (lazy) | PT-027, RN-009, HP-006 (semantics; layout [F05]) |
| [60 §2.5] priority-audit Segments row: `LEASES` with captured `files_owned` | PH-006 (use only; layout [F11]) |
| [40 §2.11] R-16: the frozen link-state strings as rendered in packs | RN-008, PH-009, BR-011 (use only; strings [F18], [F19]) |
| [50 §8.1] F4: cold column `CREATOR` (actor, role) | PT-026, PM-016, PM-017 (use only; layout [F09]) |

No X-F row concerns packs.

## Holes

None. No value in this file waits on an M0 measurement of its own. The final per-role pack budgets (PB rows) are set at M9
by the recorded-dispatch test ([AR §7.4]); the provisional defaults stand until then and are configuration, not holes.
The `codex` MCP ceiling in PE-004 is the default of `mcp.result-max-bytes.codex`, `HOLE(CFG-codex-mcp-result)`, owned by
[CFG] (review pass 1, A1-57). The
names of the digest parameter, first written as a hole, were a naming decision, which [F01 §2.5] does not make a hole;
WP-25 decided them (review pass 1, S1-40; [HOLES.md](../HOLES.md) §3): the CLI flag `--pack-digest`, the MCP `complete`
parameter `pack_digest`, the additive `result.v1` field `pack_digest` (a `string` or `null`), and the API argument
`Complete.pack_digest` ([API §10.5]).

## Open points for the review

1. **Two definitions of each class.** The class predicates are both LQ named queries ([50 §4.3], text frozen by WP-19 in
   [LQ/std]) and PM rows here. Proposed: the PM rows are the definitional form the owner signs; WP-94 runs every pack
   fixture through both, and any difference is a specification finding before either side changes ([60 §4.5]).
2. **`ABOUT` from rules, decisions and notes.** [50 §2.5] declares `ABOUT` sources as finding, verdict, measurement
   and question, but [AR §7.1] offers `rule|note|decision --about`, and [AR §7.4] C3 and [50 §4.3] `pack_target` need
   "owner rulings `ABOUT` its subtree". With [50]'s endpoint kinds PM-006 and NS-003 are empty by construction. [50]
   owns F1's endpoint kinds, but its own §4.3 needs the wider set, so the conflict is inside [50]. Proposed for WP-14
   ([F08] schema rows): add `rule`, `decision` and `note` to `ABOUT`'s source kinds.
3. **"Owner ruling" has no definition in the design.** Proposed (PT-014): a rule, decision or note with
   `authority = owner`, in its authoritative status; an owner answer counts through the decision or note that
   `ANSWERS` the question.
4. **"Own previous findings" means same role, not same agent** (PT-026). Each critic round is a new agent, so an
   actor-based reading would make PM-016 and PM-017 always empty; `CREATOR.role` ([50 §8.1] F4) serves this. The same
   reading is used by [RULES/role-write-policy] (its Open point 2).
5. **C4's candidate set.** (a) "Sections reachable via implements/about": `ABOUT` cannot start at a task or doc, so
   base(T) uses `IMPLEMENTS` from T or its ancestors, the implemented docs' descendant sections and implemented
   decisions; for a doc target (the critic's plan pack) its descendant sections. (b) The design has no section typing,
   so "implementation sections" are all of spec(T) and "metrics-and-validation sections" are those with non-empty
   `targets` (PM-008). (c) Roles without a C4 rule (code-reviewer, architect, the rest) get spec(T) at L1 (PM-013) and
   no C4 quota (PQ-005). (d) `hop` follows `DEPENDS_ON` in both directions (prerequisites and dependents).
6. **C6's scope and "known reds".** The design gives no selection for C6. Proposed: pins are current measurements
   `ABOUT` or `VERIFIES` a node of T's subtree, T's ancestors or the lane node, on B and on `main` (PM-020, PM-021);
   known reds are confirmed test findings about the same set (PM-022), since [AR §7.3] records test failures as
   findings with `f_kind = test`.
7. **C5 for roles without a rule** (tester, architect and the rest) uses the developer rule (PM-019).
8. **The C8 cursor.** "Delta since this (agent, T) cursor" needs a cursor that `pack`, a pure read, must advance.
   Proposed: a lazy pack-cursor record (A, T, rev) appended after emitting (PX-011), in the lazy class that already
   holds "read cursors" ([AR §6.5]); its record kind is WP-11's (PLAN §3.3 "record kinds for ... cursor"). No cursor:
   C8 is empty. C8's limit is 10 ([AR §7.4]) where `std.delta` has 12. Review pass 1 round 2 (residue of A1-23): the
   cursor records then had no `feed` value that carries T, so PX-011's record had no bytes. **Bytes settled** in round 3
   (closure NC-10): [F05 §9.11] `Lazy` `sub` 2 gains `feed` 2 with a `task` field, T's `#N`, and [F11 §13.1] `CURSORS`
   keys the row by (session, agent, feed, task); PT-028 and PX-011 cite both. R-MODEL's round-2 request named T's uid;
   the `#N` is taken instead (store-wide, never reused, [F11 §9]; a re-keyed T gets a new `#N` and starts with no
   cursor), which changes no rule. The key includes the session, so cursor(A, T), like mark(A) (PT-027, [F11 §13.2]),
   is read in the session the pack runs in; a dropped row leaves C8 empty. **Who appends** was owner question OQ-F-3:
   [40] I-F5 lists `pack` among the read verbs that append nothing, while PX-011 had the pack append the cursor.
   **Decided** 2026-09-28 (OQ-F-3, option (b)): the layer that delivers the pack (the `SubagentStart` hook or the MCP
   server, as [AR §5d.1] keeps the per-session cursors) appends it after delivery, and the `pack` verb appends nothing;
   PX-011 names that actor, and its record is unchanged. No M0 command is a delivering layer ([API] open points 25 and
   48), so C8 is empty on both sides of GT2 and the model appends no pack cursor. WP-81a edits [AR §7.4] C8.
9. **`--more`.** The design prints the continuation but not what the next page holds. Proposed (PX-012, PK-006): a
   stateless complement page, recomputed at the current view; no third page.
10. **Brief classes.** (a) [AR §7.4] names five brief queries but eleven kinds of content; BR rows map them (checkpoint,
    lanes, runs and merge queue to `brief_lanes`; stale summaries to `brief_critical`; links to `std.links_broken`).
    (b) "Checkpoint per open campaign" is read as the latest checkpoint note `DERIVED_FROM` the campaign's subtree, a
    campaign being a root task that is not finished. (c) "Stale summaries" use `suspect`, because git staleness is
    never computed on the brief path. (d) The walk-through's `dropped: 7 ready tasks` ([AR §7.6] step 1) shows ready
    tasks in a brief, but the class list of record has none; this file follows the class list.
11. **The digest's inputs and token.** [72 m2] and [AR §7.4] hash "the rev plus the C2/C3 member uids and their
    `rev_seq`". That set misses blocker tasks, which the notice must list, and depends on the phase, which `complete`
    does not know, so a fast path over it would be unsound. Proposed: hash the notice sets K1, K1M, K2, K3 (NS rows)
    and carry the rev in the token in clear (`4471-7f3a9c01`), because `complete` needs the rev to list what changed
    and a hash cannot give it back. The listing is conservative: a rule changed on `main` between the lane's tip and
    the moment of the pack is listed although the pack showed it. `result.v1` ([90 §7.2]) needs an additive digest
    field for `apply --from` entries (`pack_digest`, a `string` or `null`; [API] open point 29).
12. **Byte accounting and the header limit.** The budget covers the whole text, header and footer included (PY-002).
    [90 §6.3]'s <= 90/130 B header limits are for the result header `branch: ... | rev ... | n rows`; the pack header
    of [AR §7.4] step 4 is about 120 B in the design's own example, so [F19] needs a separate limit for the pack line.
13. **`applies_to` dimensions join by OR** (PT-012), as [AR §7.4]'s set intersection reads: a rule scoped
    `role:tester,path:crates/phys/**` enters every tester pack's C2, not only packs for tasks under that path. The AND
    reading (each non-empty dimension must match) is narrower. The model implements the literal reading; the review
    decides.
14. **Hook pack with an unknown label.** HP-002 falls back to rules with `applies_to` empty or `*` ([AR §7.5]); HP-003
    adds unmerged critical rules for a known label, because owner rulings are never hidden by branching ([AR §7.4]).
15. **Protected entries** (RN-004, PX-005) are taken before the quotas, at L1 at least. When E cannot hold them all,
    the rest are dropped whole and counted, never rendered at L0; the ceiling is never exceeded (PY-005).
16. **PLAN §3.3 assigns no gap to WP-90.** Every resolution above was found while writing this table.
17. **`--across` for packs.** [AR §7.4] C5 says "branch-local unless `--across`", but neither `pack` ([AR §7.1]) nor MCP
    `pack` ([AR §7.2]) has an `--across` parameter. Proposed: C5 is branch-local.
18. **The C1 link segment** counts the anchors of the nodes the pack renders at L1 or L2 (PH-009); the design's example
    (44 links) implies more than T's own links.
19. **The resume budget key.** [AR §7.5] bounds the resume output at 600 B without naming a key; PK-007 uses
    `hooks.delta.budget`, which has the same default.
20. **Registry rows for [RULES/README] §7.** The README now lists `pack-classes.md` as written in its §1.1 and
    registers its tables as RG-031 to RG-053 (review pass 1 S1-47), with these rows:

    ```
    | RG-031 | `pack-terms` | pack-classes.md | vocabulary | PT | row:id, term:token, sort:enum(input/view/set/pred/fn/order), basis:enum, source:cite, definition:text | §3 |
    | RG-032 | `pack-kinds` | pack-classes.md | decision | PK | row:id, pack:token, trigger:tokens, classes:tokens, budget_key:token, default_bytes:token, basis:enum, source:cite, note:text | §4 |
    | RG-033 | `pack-budgets` | pack-classes.md | decision | PB | row:id, role:token, key:token, default_bytes:int, final_at:token, basis:enum, source:cite, note:text | §4 |
    | RG-034 | `pack-ceilings` | pack-classes.md | decision | PE | row:id, surface:enum(cli/mcp/file/hook), client:token, key:token, default_bytes:token, max_bytes:token, basis:enum, source:cite, note:text | §4 |
    | RG-035 | `pack-bytes` | pack-classes.md | procedure | PY | row:id, rule:token, basis:enum, source:cite, definition:text | §4 |
    | RG-036 | `pack-classes` | pack-classes.md | vocabulary | CL | row:id, class:token, rank:int, name:token, named_query:token, basis:enum, source:cite, definition:text | §5.1 |
    | RG-037 | `pack-quotas` | pack-classes.md | decision | PQ | row:id, class:token, roles:tokens, key:token, default_pct:int, basis:enum, source:cite, note:text | §5.1 |
    | RG-038 | `pack-floors` | pack-classes.md | procedure | PF | row:id, class:token, basis:enum, source:cite, definition:text | §5.1 |
    | RG-039 | `pack-members` | pack-classes.md | procedure | PM | row:id, class:token, part:token, roles:tokens, view:enum(B/M/U/feed), kinds:tokens, level:token, basis:enum, source:cite, definition:text | §5.2 |
    | RG-040 | `pack-levels` | pack-classes.md | procedure | PL | row:id, kind:token, level:enum(ID/L0/L1/L2), content:tokens, basis:enum, source:cite, note:text | §5.3 |
    | RG-041 | `pack-order` | pack-classes.md | procedure | PO | row:id, class:token, keys:tokens, basis:enum, source:cite, note:text | §5.4 |
    | RG-042 | `pack-header` | pack-classes.md | procedure | PH | row:id, position:int, item:token, when:token, basis:enum, source:cite, definition:text | §5.4 |
    | RG-043 | `pack-render` | pack-classes.md | procedure | RN | row:id, rule:token, basis:enum, source:cite, definition:text | §5.4 |
    | RG-044 | `pack-fill` | pack-classes.md | procedure | PX | row:id, step:int, action:token, basis:enum, source:cite, definition:text | §5.4 |
    | RG-045 | `hook-pack` | pack-classes.md | procedure | HP | row:id, position:int, item:token, level:token, basis:enum, source:cite, definition:text | §6 |
    | RG-046 | `brief-classes` | pack-classes.md | procedure | BR | row:id, class:token, rank:int, named_query:token, kinds:tokens, level:token, basis:enum, source:cite, definition:text | §6 |
    | RG-047 | `delta-rules` | pack-classes.md | procedure | DL | row:id, pack:token, rule:token, basis:enum, source:cite, definition:text | §6 |
    | RG-048 | `notice-sets` | pack-classes.md | procedure | NS | row:id, set:token, code:int, view:enum(B/M), kinds:tokens, basis:enum, source:cite, definition:text | §7 |
    | RG-049 | `notice-rules` | pack-classes.md | procedure | NR | row:id, step:int, rule:token, basis:enum, source:cite, definition:text | §7 |
    | RG-050 | `notice-digest` | pack-classes.md | procedure | ND | row:id, offset:token, width:token, type:token, name:token, basis:enum, source:cite, meaning:text | §7 |
    | RG-051 | `notice-entry` | pack-classes.md | procedure | NE | row:id, offset:token, width:token, type:token, name:token, basis:enum, source:cite, meaning:text | §7 |
    | RG-052 | `notice-modes` | pack-classes.md | decision | NM | row:id, mode:token, output:token, cap_bytes:int, basis:enum, source:cite, note:text | §7 |
    | RG-053 | `pack-source-map` | pack-classes.md | map | PS | row:id, source_row:token, realized_by:tokens, note:text | §8 |
    ```

    The RG numbers are placeholders for the README's next free ids. The decision tables here are allowlists or lookups,
    not first-match rows: README §8's decision-table paragraph describes the merge tables and needs one sentence for
    lookup tables.
21. **Proposed records in packs and the brief** (PT-033, PM-026 to PM-029, PO-001, PO-002, PO-006, PO-008, RN-004,
    RN-018, BR-012, HP-007; spec sync 3). **Decided** by the owner on 2026-10-06 (OQ-A-8, option (c)): the brief gains a
    "proposed / needs review" line, and packs do not hide proposed records. Before, auth(n) kept every `proposed` rule
    and decision out of every class, so the review queue that the capture pipeline feeds (owner decision #46) was
    invisible. Readings, for the review: (a) a proposed record enters the classes where its authoritative counterpart
    enters: C2 on B and from `main` (PM-026, PM-027), C3 as an owner ruling about T's subtree (PM-028), C7 (PM-029) and
    the hook pack's critical rules (HP-007); C4 already shows a proposed decision through spec(T), whatever its status.
    (b) It renders at L0 (an owner ruling at L1, as a ruling), with `~proposed` (RN-018), after the authoritative
    entries of its class (PO rows, `prop-last`), and it is never protected: unreviewed records do not take the budget of
    authoritative ones. (c) auth(n) is unchanged, so the stale-pack notice sets, PH-008's critical count, RN-004 and
    BR-008 count no proposed record; the brief shows proposed records through BR-012's line, which is charged with the
    header so it is never dropped. (d) A `note` has no `proposed` status ([F08 §9.1]) and a `draft` doc is not
    "proposed": neither changes here. (e) The class queries of [LQ/std] and their fixtures gain the same parts, since
    both forms must agree ([OP-1]): C2 through `pack_rules_proposed` and `pack_rules_unmerged_proposed`, C3 through
    `pack_target`'s part `proposed ruling`, C7 through `pack_hazards_proposed` ([LQ/std §5.1] "Proposed records"), while
    `brief_critical` (BR-008, BR-009) is unchanged, as (c) says; BR-012's query is [LQ/std §6.2] `brief_proposed`.
