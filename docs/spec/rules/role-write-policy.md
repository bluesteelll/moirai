# Role write policy: roles x verbs, kinds, fields and edges -> allowed or refused

| Field | Value |
|---|---|
| Status | draft, pass 1 pending |
| Work package | WP-90 (R-MODEL), rule table for V3 signing ([m0/PLAN §3.2] item 9 "the role write policy", [m0/PLAN §5] V3); consumed by WP-93b (`TX` policy checks on the model), WP-94 (GT10 and GT18 cases "verb = named mutation") and, from M7/M8, the engine's binder and CLI |
| Sources | [AR §7.3] (where rights come from; the role table; per statement, op and field; R4 rows); [AR §7.1] conventions, write and coordination verbs, exit codes; [AR §7.2] MCP tools, `write` restrictions, branch resolution, stamp; [AR §6.2] leases, fencing, `complete`; [AR §6.4] idempotency; [AR §3.1]-[AR §3.6] header columns, kinds, edges, status machines; [AR §13] policy data (`policy.self-claim-roles`, `policy.mint.role-lease`, `policy.hook-label`, `policy.role.<role>.mcp-write`, `.tx`, `.define-query`, `.authority-owner`, `policy.role.developer.fields`), `query.safelist.<role>`, `query.safelist.model.<profile>`, `files.confirm-roles`, `hooks.session-start.orchestrator-lease`, `hooks.stamp.ask-for`; [50 §3.10] mutation semantics; [50 §4.2] write verbs as named mutations; [50 §4.4] definitions and safelist; [50 §5.2] error table; [50 §6.3] MCP `write`; [50 §6.5] per-role table; [90 §4.1] resolver; [90 §4.3] role policy without hooks; [90 §4.4] leases per harness; [90 §7.1]-[90 §7.3] dispatcher, `result.v1`, `apply --from`; [90 §8.1] L2; [90 §10.1]; [40 §3.7] `links fix`; [40 §5.3] writer tree; [40 §6.3] role policy rows |
| Format | [RULES/README] |
| Cited as | [RULES/role-write-policy]; a row as [RULES/role-write-policy WC-012] |

## 1. What this table decides

For every write that reaches the store — a CLI write verb, `moirai tx`, `apply`, and the MCP tools `write`, `remember`,
`claim` and `complete` — this file decides whether the caller's **effective role** may perform it, and if not, which
**refusal code** and exit code it gets:

- where the effective role comes from: the presented lease, the owner attestation, hook labels that narrow
  (`role-rights`, `role-rows`);
- who may mint, renew, release and reclaim leases (`role-mint`);
- which verbs and surfaces each role may use (`role-verbs`) and which `TX` statement classes need which role
  (`role-statements`);
- per op inside a `TX` block: which kinds a role may create (`role-create`), which field values need which role
  (`role-values`), which fields it may set (`role-fields`), which status transitions it may make (`role-status`) and
  which edges it may create or delete (`role-edges`);
- the read-side restrictions that share the same keys (`role-reads`), what hooks write (`role-hooks`) and the refusal
  codes (`role-refusals`).

Rights come **only from a presented lease** ([AR §7.3], [90 §4.3]); an unleased caller gets the `general-purpose` row;
hook labels only narrow. The policy prevents honest mistakes, not a hostile agent that copies a lease it has seen
([90 §4.3]). It grants nothing the schema, the status machines or the invariants refuse: E115, E404, E405, E409 and
E410 still apply after a row allows an op (WR-011).

The model implements `role-rights` as its caller-context function and evaluates the other tables as data inside its
`TX` evaluator, so every write verb, `apply` entry and MCP call is checked by the same rows ([50 §6.5]: "the reference
model implements one rule", [90 §4.3]).

## 2. How to read a row

Every table follows [RULES/README]. In addition:

- **Evaluation: allowlists.** `role-create`, `role-values`, `role-fields`, `role-status`, `role-edges` and the `roles`
  cells of `role-verbs`, `role-mint` and `role-statements` are allowlists: an operation is allowed if and only if at
  least one row matches it; no matching row means refused with the row's table's refusal (E406 unless a row says
  otherwise). Row order does not matter.
- **Role cells.** Role names are those of `role-rows`. `*`: every role, `general-purpose` included. `leased`: every role
  of `role-rows` except `general-purpose`. `holder`: the holder of the lease the operation names. `-`: nobody.
- **Scope cells** (`role-terms` WT-004 to WT-008): `any`, `own-role`, `leased-task`, `created-in-tx`, `lease-run`.
- **Field cells.** A field name of [AR §3.1]-[AR §3.2], `status`, or `*` = the writable field set W (WT-009).
- **Constraint cells** (`role-create`): `field=value` or `field=v1,v2` (a comma without a space stays inside one
  token); `role=self` means the created verdict's `role` field equals the caller's effective role.
- **Refusal cells.** A code of `role-refusals`: an LQ code of [LQ/errors] (`E406`, `E407`, ...) or a named code of
  [F19 §10.2] (`not_writer_tree`). `-` where the row cannot refuse. Exit cells hold the exit code or `-`.

## 3. Terms

<!-- table: role-terms -->
| row | term | sort | basis | source | definition |
|---|---|---|---|---|---|
| WT-001 | R | input | design | [AR §7.3]; [90 §4.3] | The effective role of the call, computed by `role-rights`. |
| WT-002 | lease | input | design | [90 §4.1] Rights row | The presented lease: explicit `--lease` / `lease`, else `MOIRAI_LEASE` under WR-002, else none. |
| WT-003 | actor | input | design | [90 §4.1] Actor row | The resolved actor; the presented lease's holder first. |
| WT-004 | own-role | scope | proposed | [AR §7.3] "own findings", "own anchors"; [50 §6.5] "own docs, decisions and questions"; [50 §8.1] F4; [OP-2] | For a node n: CREATOR(n).role = R. For an `AT` anchor: the role recorded in the commit that added the anchor ([AR §3.4] I10 provenance) = R. |
| WT-005 | leased-task | scope | design | [AR §7.3] developer "`files_owned` of its leased task"; [AR §6.2] | n is the task of the presented lease, and that lease is a task lease. |
| WT-006 | created-in-tx | scope | derived | [50 §3.10] items 2, 6; [50 §4.2] `remember` | n is created by an earlier statement of the same `TX` block, or by the same named mutation or `remember` call. |
| WT-007 | any | scope | design | [AR §7.3] | Every node. |
| WT-008 | lease-run | scope | proposed | [AR §7.4] step 5; [90 §4.3] run-scoped leases; [OP-21] | n is the `run` node the presented lease names (a run-scoped role lease, or a task lease claimed with `--run`). |
| WT-009 | W | set | proposed | [50 §3.10] item 6; [50 §6.5] "any writable field"; [OP-17] | The writable fields of a kind: `title`, `abstract`, `body` (`SET x.body`, `PATCH`), `parent` and `order` (`MOVE`), `priority`, `criticality`, `confidence`, `labels`, and every kind field whose schema class is writable. Not in W: `status` and `resolution` (`role-status`), `authority` (`role-values`), and every field E115 refuses (derived, runtime, tree-derived, observation and identity fields, edge properties). |
| WT-010 | may-write(n) | pred | proposed | [AR §7.3] R4 rows "every role that may write the referring node"; [50 §6.3]; [OP-10] | R is orchestrator or owner; or a `role-fields` or `role-status` row matches (R, kind(n), n); or n is own-role and a `role-create` row lets R create kind(n). |
| WT-011 | bulk | pred | design | [50 §3.10] item 3 | A `MATCH` target whose `EXPECT` allows more than 10 bindings or has no upper bound. |
| WT-012 | owner-attested | pred | proposed | [AR §7.3] owner row "(main session, `--by owner`)", orchestrator "`authority = owner` only with `--owner-quote`"; [AR §13] `policy.role.<role>.authority-owner`, `hooks.stamp.ask-for`; [OP-1] | The call presents an orchestrator session role lease and carries the owner attestation: `--by owner`, or `--authority owner` with `--owner-quote-file` on the CLI; through MCP, `authority=owner` with an owner quote, which the stamp's `ask` permission puts before the human by default. |
| WT-013 | label | input | design | [90 §4.3] "Hooks only narrow"; [AR §7.2] | A hook-attested role label: the dispatch marker's `role=` (the `PostToolUse(Agent)` map), else Claude Code's `agent_type`, else Codex's `SubagentStart.agent_type`. |
| WT-014 | known-subagent | pred | design | [90 §4.3] mint (i)-(ii) | Codex `_meta.threadId` differs from `_meta.sessionId`, or a Claude stamp or marker names a subagent. |
| WT-015 | dispatched-worker | pred | design | [90 §4.3]; [90 §7.5] | The environment carries `MOIRAI_LEASE` or `MOIRAI_RUN`. |
| WT-016 | acceptor | input | design | [40 §3.7] `--confirm` | The actor recorded with the `agent/*` acceptance that a `--confirm` would turn into `confirmed/*`. |

## 4. Where rights come from

<!-- table: role-rights -->
| row | step | rule | basis | source | definition |
|---|---|---|---|---|---|
| WR-001 | 1 | rights-source | design | [AR §7.3]; [90 §4.1] Rights row; [90 §4.3] | The rights source is the presented lease (WT-002) and nothing else. A declared `--role` or `MOIRAI_ROLE` is recorded and grants nothing. |
| WR-002 | 2 | env-binding | design | [90 §4.1] binding rule; [90 §10.1] `LEASES.bound` | An environment lease binds, at its first use, to the attested thread that used it (Codex `_meta.threadId` or `CODEX_THREAD_ID`), recorded in the lease row's `bound`. A later use through the environment from another thread is refused (WZ-005, exit 5). An explicit `--lease` is never refused on this ground. Where the harness names no thread, the environment identifies the process. |
| WR-003 | 3 | lease-valid | design | [AR §6.2] fencing; [AR §3.4] I17′; [50 §3.10] item 4 | A presented lease must be live and carry the current fencing token; otherwise E407 (exit 5) naming the current holder. |
| WR-004 | 4 | declared-agent | design | [90 §4.1] Actor row; [AR §7.1] | Outside `claim` (where `--agent` names the holder of the new lease), a declared `--agent` / `agent` that differs from the presented lease's holder is refused (WZ-004, exit 5). |
| WR-005 | 5 | branch | design | [90 §4.1] Branch row; [AR §7.2]; [50 §3.10] item 1 | A task lease and a run-scoped role lease fix the branch: an explicit branch (or `TX ON`) that differs is E407 (exit 5); an MCP call with `lease` and no `branch` writes on the lease's branch. The orchestrator's session role lease carries no branch, so its holder writes on any branch ([AR §7.3] "everything on any branch"). |
| WR-006 | 6 | row | design | [AR §7.3]; [90 §4.3] | No lease: `general-purpose`. A task lease: the role it was claimed for. A run-scoped role lease: its role. The orchestrator's session role lease: `orchestrator`, or `owner` when the call is owner-attested (WT-012). A lease whose role has no `role-rows` row: `general-purpose` ([OP-20]). |
| WR-007 | 7 | narrow | design | [90 §4.3] "Hooks only narrow"; [AR §13] `policy.hook-label = narrow`; [OP-5] | When a label (WT-013) names a `role-rows` role other than `general-purpose` and differs from R, the call's rights are the intersection of the two roles' rows, per op and per field, and one warning line is printed. A label that names no role of `role-rows` (a harness built-in agent type such as `general-purpose`, `Explore` or `Plan`) is recorded and ignored. An unleased call ignores labels. |
| WR-008 | 8 | surface | design | [50 §6.3]; [AR §7.2]; [50 §6.5] | `role-verbs` and `role-statements` are checked first: a CLI-only verb or statement through MCP, an MCP `write` by a role whose `mcp_write` is `no`, and a write statement in a read surface are refused before the block runs. |
| WR-009 | 9 | per-op | design | [AR §7.3] "per statement, per op and per field"; [50 §6.5]; [50 §3.10] item 5 | Every write verb, `apply` entry and MCP write compiles to one `TX` block. For each statement in order, each op it produces and each field an op sets: `role-statements`, `role-create`, `role-values`, `role-fields`, `role-status` and `role-edges` must each allow it, by the allowlist rule of §2. |
| WR-010 | 10 | refuse-whole | design | [AR §7.3]; [50 §3.10] item 5; [50 §5.3] | The first refused op refuses the whole block: nothing is written, E406 (exit 6) names the statement (1-based) and the rule (WZ-001). Statically known ops are refused by the binder; ops whose scope depends on data (own-role, leased-task) are checked on the candidate overlay; nothing is checked after the commit. |
| WR-011 | 11 | other-checks | design | [50 §3.10] item 5 | An allowed op still passes schema, status-machine, invariant and delete-policy checks (E115, E404, E405, E409, E410). |
| WR-012 | 12 | model-profile | design | [90 §8.1] L2; [90 §8.2]; [AR §13] `query.safelist.model.<profile>` | After the role policy: a caller whose model profile is `unknown` writes named mutations only (`named-only`, the default for `unknown`); a free-form `TX` is refused with WZ-010, naming the matching named mutation. `dry-targets` (opt-in) also allows a `TX` applied with `IF TARGETS` from a `DRY`. |
| WR-013 | 13 | side-effects | proposed | [40 §3.3]; [40 §3.7] `links sync`; [AR §7.1] `file mv` example "2 globs rewritten"; [50 §3.10] item 7; [OP-16] | The fixed side effects of an allowed verb are part of it and are not checked against `role-fields`: capture registers file nodes and the root node (`link --at`, `file add`); `links sync` and settles write observations, `path_moves` and `PENDING`; `file mv` rewrites the globs of other nodes' `files_owned` and `applies_to`; ops produce markers. |
| WR-014 | 14 | hooks | proposed | [AR §7.5]; [90 §2.5]; [OP-15] | A hook handler writes only what its `role-hooks` row lists; it never executes write text a model supplied. |

## 5. Roles

<!-- table: role-rows -->
| row | role | carried_by | self_claim | mcp_write | basis | source | note |
|---|---|---|---|---|---|---|---|
| WO-001 | orchestrator | session-role-lease | no | no | design | [AR §7.3]; [90 §4.3]; [AR §13] `policy.role.<role>.mcp-write` | Everything on any branch; branch, merge and image verbs. `mcp_write` follows the key's literal default list ([OP-8]). |
| WO-002 | owner | owner-attestation | no | no | proposed | [AR §7.3] owner row; [50 §6.5] "orchestrator, owner"; [OP-1] | The orchestrator's rows plus owner-only rows (`hooks install --git`, `answer --by owner`, `owner/*` provenance). |
| WO-003 | architect | task-lease, run-role-lease | no | yes | design | [AR §7.3]; [50 §6.5]; [AR §13] | - |
| WO-004 | researcher | task-lease, run-role-lease | no | yes | design | [AR §7.3]; [AR §13] | - |
| WO-005 | architecture-critic | task-lease, run-role-lease | no | yes | design | [AR §7.3]; [50 §6.5]; [AR §13] | - |
| WO-006 | code-reviewer | task-lease, run-role-lease | no | no | design | [AR §7.3]; [50 §6.5]; [AR §13] | A Bash role; writes through the CLI. |
| WO-007 | refuter | task-lease, run-role-lease | no | no | design | [AR §7.3] "refuter (`general-purpose` with `role=refuter`)"; [50 §6.5] | Rights need a role lease like every other role; a label `refuter` without one grants nothing (WR-001). |
| WO-008 | developer | task-lease, run-role-lease | yes | no | design | [AR §7.3]; [90 §4.3]; [AR §13] `policy.self-claim-roles` | A role-less self-claim is `developer`. |
| WO-009 | tester | task-lease, run-role-lease | yes | no | design | [AR §7.3]; [AR §13] `policy.self-claim-roles` | - |
| WO-010 | results-analyst | task-lease, run-role-lease | no | no | design | [AR §7.3] | - |
| WO-011 | project-analyst | task-lease, run-role-lease | no | yes | design | [AR §7.3]; [AR §13] | - |
| WO-012 | doc-writer | task-lease, run-role-lease | no | no | design | [AR §7.3] | - |
| WO-013 | general-purpose | none | no | no | design | [AR §7.3] "unleased callers get the `general-purpose` row"; [90 §4.3] "`remember` findings, notes and questions only" | The unleased row. MCP `remember` only (WV-044). |

## 6. Leases

<!-- table: role-mint -->
| row | form | commands | allowed | key | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|
| WM-001 | task-self-claim | claim-id, claim-next | * | policy.self-claim-roles | E406 | 6 | design | [AR §7.3]; [90 §4.3] "task self-claims ... any caller" | One id, no `--run`; the new lease's role is in `policy.self-claim-roles` (default `developer`, `tester`); a role-less self-claim is `developer`. The holder is the caller (`--agent` names it). |
| WM-002 | bulk-claim | claim-ids, claim-run | orchestrator, owner | policy.mint.role-lease | E406 | 6 | design | [90 §4.3] "dispatcher bulk claims"; [90 §7.1] step 2 | More than one id, or `--run ID`. The lease role may be any `leased` role except orchestrator and owner. |
| WM-003 | claim-other-role | claim-id-role | orchestrator, owner | policy.self-claim-roles | E406 | 6 | derived | [90 §4.3] | A one-id claim for a role outside `policy.self-claim-roles`. |
| WM-004 | run-role-lease | claim-role-run | orchestrator, owner | policy.mint.role-lease | E406 | 6 | design | [90 §4.3] "run-scoped role leases ... only a caller presenting an orchestrator lease, or the owner" | `claim --role R --run ID [--branch B]` with R not orchestrator; the lease fixes its branch (WR-005). |
| WM-005 | session-role-lease | claim-role-orchestrator-session | * | hooks.session-start.orchestrator-lease | E406 | 6 | design | [90 §4.3] mint (i), (ii); [AR §7.3]; [OP-19] | Refused when known-subagent (WT-014) or dispatched-worker (WT-015). Minted by a main session's `SessionStart` hook (when the key is true) or by the orchestrate skill's first step; bound to the minting thread where the harness names threads; `lease.orchestrator-ttl` where no slot anchors it. |
| WM-006 | renew | heartbeat, lease-write | holder | - | E407 | 5 | design | [AR §6.2]; [90 §4.4] | `heartbeat L`, or any write presenting a TTL lease, moves the deadline when more than half the TTL has elapsed. |
| WM-007 | release | release | holder | - | E407 | 5 | design | [AR §6.2] fencing | - |
| WM-008 | reclaim | reclaim-older-than, reclaim-run | orchestrator, owner | - | E406 | 6 | proposed | [AR §6.2]; [90 §7.1] step 5 "the safety net"; [OP-27] | - |
| WM-009 | complete-lease | complete | holder | - | E407 | 5 | design | [AR §6.2] "`complete` ... must present the lease" | The presented lease must be the task's live lease; the status move is then `role-status`'s (WS-004, WS-006). |

## 7. Verbs and surfaces

A verb whose `class` is `graph` compiles to one `TX` block and is checked op by op (WR-009); its row only states the
surface and any condition beyond the ops. `ref` and `admin` verbs are not `TX` blocks, so their rows decide alone.

<!-- table: role-verbs -->
| row | verb | class | surface | roles | key | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|---|
| WV-001 | branch | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3] orchestrator "branch/merge/image verbs"; [AR §7.1] "orchestrator rituals, CLI only" | Create, list is a read, `-d`/`-D`. |
| WV-002 | checkout | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | - |
| WV-003 | worktree | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | `bind`, `unbind`; `--list` is a read. |
| WV-004 | lane | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | `open`, `close`, `freeze`. |
| WV-005 | sync | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | `--check` is a read (WV-007). A hook's clean auto-sync is WH-002. |
| WV-006 | merge | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | Including `--continue` and `--abort`. |
| WV-007 | merge-check | read | cli | * | - | - | - | design | [AR §7.1] | Also `sync --check`: previews, no write. |
| WV-008 | cherry-pick | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | - |
| WV-009 | revert | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | - |
| WV-010 | undo | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | - |
| WV-011 | tag | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.1] | - |
| WV-012 | image-export | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3]; [AR §7.5] | Also run by `SessionStart` (WH-001) and as `--if-older` on the orchestrator's write paths. |
| WV-013 | image-import | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3] | - |
| WV-014 | image-push-pull | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3] | - |
| WV-015 | image-maintenance | ref | cli | orchestrator, owner | - | E406 | 6 | design | [AR §7.3] | `image gc`, `image doctor --rebuild-map`. |
| WV-016 | rm | graph | cli | orchestrator, owner | policy.role.<role>.tx | E406 | 6 | design | [AR §7.3] "Node `DELETE` ... orchestrator/owner only, through the CLI"; [50 §4.2] | Node `DELETE` (WX-001). |
| WV-017 | resolve | graph | cli | orchestrator, owner | policy.role.<role>.tx | E406 | 6 | design | [AR §7.3]; [50 §4.2] | WX-002. |
| WV-018 | answer | graph | cli | owner | policy.role.<role>.authority-owner | E406 | 6 | design | [AR §7.3] owner row "`question.answered`"; [AR §7.1] `answer Q --by owner --verbatim-file` | Needs owner-attested (WT-012). |
| WV-019 | write-verbs | graph | cli | * | - | E406 | 6 | design | [AR §7.3] "behind every write verb"; [50 §4.2] | `add`, `set`, `link`/`unlink` of graph edges, `move`, `reopen`, `supersede`, `retract`, `doc patch`, and `rule`/`note`/`decision`/`finding`/`verdict`/`measurement`: checked op by op. |
| WV-020 | tx | graph | cli | * | policy.role.<role>.tx | E406 | 6 | design | [AR §7.1] `moirai tx`; [50 §6.5] | Checked statement by statement. |
| WV-021 | apply-batch | graph | cli | * | - | E406 | 6 | design | [AR §7.3] "behind every write verb, `apply`"; [AR §6.4] | `apply FILE` or `-`: a JSON op batch, checked op by op under the presenter's row. |
| WV-022 | apply-from | graph | cli | orchestrator, owner | - | E406 | 6 | proposed | [90 §7.1] step 5; [90 §7.2]; [90 §7.3]; [OP-6]; [OP-7] | `apply --from KIND:SRC`: each entry's lease is validated against the run; findings and notes an entry carries are checked against the entry lease's role row and created with that role; entry outcomes (complete, release) run under the presenter's rights. |
| WV-023 | run | graph | cli | orchestrator, owner | - | E406 | 6 | proposed | [90 §7.1] step 1; [AR §7.1] `run open\|close`; [OP-27] | `run open`, `run close`. |
| WV-024 | check | runtime | cli | * | - | - | - | proposed | [AR §7.1] `check` "appends only lazy ANCESTRY facts"; [AR §7.6] step 7 (the tester runs it); [OP-21] | Writes only derivable runtime facts. |
| WV-025 | claim | runtime | both | * | - | - | - | design | [AR §7.1]; [AR §7.2] `claim` | Decided by `role-mint`. |
| WV-026 | complete | graph | both | * | - | - | - | design | [AR §7.1]; [AR §7.2] `complete` | Decided by WM-009 and `role-status`. |
| WV-027 | heartbeat-release | runtime | both | holder | - | E407 | 5 | design | [AR §6.2] | WM-006, WM-007. |
| WV-028 | reclaim | runtime | cli | orchestrator, owner | - | E406 | 6 | proposed | [AR §7.1]; [OP-27] | WM-008. |
| WV-029 | file-add | file-link | cli | leased | - | E406 | 6 | proposed | [AR §7.1] `file add`; [40 §3.3]; [OP-9] | Registers file nodes by capture. Capture as a side effect of an allowed `link --at` is WR-013's. |
| WV-030 | file-mv | file-fs | cli | orchestrator, owner, developer, tester, doc-writer | - | E406 | 6 | proposed | [AR §7.3] "`file mv\|rm\|revert` CLI-only, in the writer tree"; [AR §7.5] core skill; [OP-9] | Outside the writer tree of the caller's branch: WZ-009 (exit 5). |
| WV-031 | file-rm | file-fs | cli | orchestrator, owner, developer, tester, doc-writer | - | E406 | 6 | proposed | [AR §7.3]; [40 §3.5]; [OP-9] | As WV-030. |
| WV-032 | file-revert | file-fs | cli | orchestrator, owner, developer, tester, doc-writer | - | E406 | 6 | proposed | [AR §7.3]; [40 §3.6]; [OP-9] | As WV-030. |
| WV-033 | file-relink-after | file-link | both | orchestrator, owner, developer, tester, architect | - | E406 | 6 | proposed | [40 §3.6]; [40 §6.3] `write` named mutations; [OP-9] | The roles of `links fix` (WV-037). |
| WV-034 | link-at | file-link | both | * | - | E406 | 6 | design | [AR §7.3] R4 rows "`link --at`/`link_file` for every role that may write the referring node"; [40 §6.3] | Allowed iff may-write(n) (WT-010) for the referring node n. |
| WV-035 | unlink-at | file-link | both | * | - | E406 | 6 | derived | [50 §6.3] "allowed to every role the policy lets write the edge's source"; [50 §6.5] developer "`DELETE` of own `AT` anchors" | Allowed iff may-write(n) for the source n, and the anchor is own-role unless R is orchestrator or owner. |
| WV-036 | links-sync | file-link | both | * | - | - | - | design | [AR §7.3] "`links sync` for every role (it records only exact observations)"; [40 §6.3] | A settle point; `general-purpose` included. |
| WV-037 | links-fix | file-link | both | orchestrator, owner, developer, tester, architect | - | E406 | 6 | design | [AR §7.3] "`links fix` for the orchestrator, developer, tester, architect (for docs) and owner"; [40 §6.3] | The architect only on file nodes with `artifact_kind` = doc. Provenance by WA-003. |
| WV-038 | links-fix-confirm | file-link | both | orchestrator, owner | files.confirm-roles | E406 | 6 | design | [AR §7.3] "`--confirm` for the orchestrator and owner by default, never by the acceptor"; [40 §3.7]; [AR §13] `files.confirm-roles` | Refused (WZ-013) when the actor is the acceptor (WT-016). |
| WV-039 | links-fix-prefix | file-link | cli | orchestrator, owner | files.confirm-roles | E406 | 6 | proposed | [40 §3.7] "`--prefix FROM TO` confirms an inferred directory move"; [OP-9] | A confirmation, so the confirm roles. |
| WV-040 | pack-record-run | graph | cli | * | - | E406 | 6 | proposed | [AR §7.4] step 5; [RULES/pack-classes PX-013]; [OP-21] | Allowed iff the presented lease names a run (WT-008) and the edges start at that run; orchestrator and owner for any run. |
| WV-041 | links-import | graph | cli | orchestrator, owner | - | E406 | 6 | proposed | [AR §7.1] `links import --from-markdown`; [OP-27] | The cutover import of existing citations. |
| WV-042 | read-verbs | read | both | * | - | - | - | design | [50 §6.5] "Reads are the default everywhere" | `brief`, `pack` without `--record-run`, `ready`, `blocking`, `blockers`, `show`, `find`, `tree`, `notes`, `stale`, `changes`, `stats`, `conflicts`, `log`, `diff`, `blame`, `reflog`, `op log`, `links check`, `links mentions`, `file where`, `image show`, `q` (WQ rows), MCP `brief`, `pack`, `get`, `query`, `changes`, `branch`. |
| WV-043 | mcp-write | surface | mcp | architect, researcher, architecture-critic, project-analyst | policy.role.<role>.mcp-write | E406 | 6 | design | [AR §13] `policy.role.<role>.mcp-write` "(yes for architect, architecture-critic, researcher, project-analyst)"; [OP-8] | The `write` tool; the rows of WO `mcp_write`. |
| WV-044 | mcp-remember | surface | mcp | * | - | E406 | 6 | design | [AR §7.2] `remember`; [90 §4.3] | Kinds per `role-create`; `general-purpose`: findings, notes and questions. |
| WV-045 | mcp-claim | surface | mcp | * | - | - | - | design | [AR §7.2] `claim` | `role-mint`. |
| WV-046 | mcp-complete | surface | mcp | * | - | - | - | design | [AR §7.2] `complete` | WM-009, `role-status`. |
| WV-047 | hooks-install-git | admin | cli | owner | - | E406 | 6 | design | [AR §7.3] R4 rows "`hooks install --git` owner only"; [40 §6.3] | Owner-attested (WT-012). |
| WV-048 | store-admin | admin | cli | orchestrator, owner | - | E406 | 6 | proposed | [AR §13]; [90 §4.3] "policy edits"; [OP-14] | Verbs that change store state outside the graph: `config set\|unset` at store scope, `quiet on\|off`, `gc`, `backup`, `restore`, `repair`, `migrate`. |
| WV-049 | outside-store | admin | cli | * | - | - | - | proposed | [AR §7.1]; [OP-14] | Verbs that write no store: `init` (no store exists yet), `hooks install` without `--git`, `integrate`, `export`, `config set --user`, `mcp`, `hook` handlers (WH rows), `schema`. |
| WV-050 | doctor | read | cli | * | - | - | - | proposed | [AR §7.1] `doctor`; [OP-14] | `doctor lanes --refresh-graph` spawns `git` for the project repository, not the store. |

## 8. Statements

<!-- table: role-statements -->
| row | statement | roles | surface | key | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|
| WX-001 | node-delete | orchestrator, owner | cli | policy.role.<role>.tx | E406 | 6 | design | [AR §7.3]; [50 §6.5]; [50 §3.10] `DELETE x` | `DELETE x` on a node variable or literal. |
| WX-002 | resolve | orchestrator, owner | cli | policy.role.<role>.tx | E406 | 6 | design | [AR §7.3]; [50 §6.5] | Also the only statement a staging ref accepts. |
| WX-003 | define-query | orchestrator, owner | cli | policy.role.<role>.define-query | E406 | 6 | design | [AR §7.3]; [50 §4.4] | - |
| WX-004 | drop-query | orchestrator, owner | cli | policy.role.<role>.define-query | E406 | 6 | design | [AR §7.3]; [50 §4.4] | - |
| WX-005 | bulk-target | orchestrator, owner | both | policy.role.<role>.tx | E406 | 6 | design | [AR §7.3] "bulk `MATCH` targets ... orchestrator-only by default"; [50 §6.5]; [OP-22] | WT-011. |
| WX-006 | reopen | orchestrator, owner | both | - | E406 | 6 | proposed | [AR §3.6] task "`done -> open` only through `reopen`"; [OP-27] | No role row grants it. |
| WX-007 | schema-change | orchestrator, owner | cli | - | E406 | 6 | proposed | [AR §13] policy data "schema rows, versioned per branch"; [90 §4.3] "policy edits"; [OP-14] | `Schema` ops other than query definitions, policy data rows included. |
| WX-008 | call-tx-complete | developer, tester, orchestrator, owner | both | - | E406 | 6 | design | [AR §7.3]; [50 §6.5] developer "`CALL tx.complete` with a lease", tester "`tx.complete` of own claim"; [OP-6] | For developer and tester the target is the leased task (WT-005). |
| WX-009 | call-tx-claim | * | both | - | - | - | design | [50 §4.2] | `role-mint`. |
| WX-010 | call-tx-heartbeat-release | holder | both | - | E407 | 5 | design | [50 §4.2]; [AR §6.2] | - |
| WX-011 | call-tx-reclaim | orchestrator, owner | both | - | E406 | 6 | proposed | [50 §4.2]; [OP-27] | - |
| WX-012 | tx-on | * | both | - | E407 | 5 | design | [50 §3.10] item 1; [90 §4.1] | `TX ON` a branch other than the lease's: WR-005. |
| WX-013 | node-delete-mcp | - | mcp | - | E406 | 6 | design | [AR §7.2] `write` "node `DELETE`, `RESOLVE` and query definitions refused (CLI only)"; [50 §6.3] | Refused for every role, the orchestrator included. |
| WX-014 | resolve-mcp | - | mcp | - | E406 | 6 | design | [AR §7.2]; [50 §6.3] | - |
| WX-015 | define-query-mcp | - | mcp | - | E406 | 6 | design | [AR §7.2]; [50 §6.3] | - |
| WX-016 | drop-query-mcp | - | mcp | - | E406 | 6 | design | [AR §7.2]; [50 §6.3] | - |
| WX-017 | write-in-read-surface | - | both | - | E006 | 2 | design | [50 §5.2] E006; [50 §6.5] "no write productions, refuse `tx.*` calls" | A write statement or a `tx.*` call in `q` or MCP `query`. |
| WX-018 | other-statements | * | both | - | - | - | design | [50 §3.10] item 6; [50 §6.5] | `CREATE`, `SET`, `REMOVE`, `PATCH`, `MOVE`, edge `DELETE`, `ASSERT`, a non-bulk `MATCH ... EXPECT`: allowed as statements and checked per op. |

## 9. Ops: create, values, fields, status, edges

Kind-level validation applies to every role and is not repeated below: a `finding` needs `failure_scenario`; a node
with `authority = owner` needs `owner_quote` (for rules, [AR §3.2]); a `verdict` written through `remember` writes its
`DERIVED_FROM` edges ([50 §4.2]); `CREATE (x:artifact ...)` is E115 because file nodes are registered by capture
([50 §3.10]), so the artifact rows below apply to capture and to the `artifact_kind` set that follows it.

<!-- table: role-create -->
| row | role | kind | constraint | required | basis | source | note |
|---|---|---|---|---|---|---|---|
| WC-001 | orchestrator | * | - | - | design | [AR §7.3]; [50 §6.5] | Every kind, project kinds included. |
| WC-002 | owner | * | - | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-003 | architect | doc | doc_kind=plan,section | - | design | [AR §7.3] "`doc` (plan, section)" | - |
| WC-004 | architect | decision | - | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-005 | architect | question | - | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-006 | architect | finding | f_kind=plan | - | proposed | [AR §7.3] "`deviation` findings"; [OP-3] | A deviation finding. |
| WC-007 | researcher | note | - | - | design | [AR §7.3] | - |
| WC-008 | researcher | artifact | artifact_kind=research | - | design | [AR §7.3] "`artifact{research}`"; [OP-4] | By capture. |
| WC-009 | researcher | question | - | - | design | [AR §7.3] | - |
| WC-010 | researcher | finding | - | confidence | design | [AR §7.3] "findings with `confidence`" | `confidence` given explicitly. |
| WC-011 | architecture-critic | finding | - | - | design | [AR §7.3] "`finding` (with `failure_scenario`)"; [50 §6.5] | - |
| WC-012 | architecture-critic | verdict | role=self | - | design | [AR §7.3] "`verdict{role}` (+ `derived_from` edges)" | The verdict's `role` field is the caller's role. |
| WC-013 | code-reviewer | finding | - | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-014 | code-reviewer | verdict | role=self | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-015 | refuter | finding | - | refutes-or-confirms-edge | derived | [AR §3.6] "A refutation is a finding with a `refutes` edge"; [AR §7.3] refuter "new findings of other kinds" (may not) | The same block creates a `REFUTES` or `CONFIRMS` edge from it. |
| WC-016 | developer | note | - | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-017 | developer | question | - | - | design | [AR §7.3]; [50 §6.5] | - |
| WC-018 | developer | finding | f_kind=plan | - | proposed | [AR §7.3] "`deviation`"; [50 §6.5] "deviation finding"; [OP-3] | - |
| WC-019 | developer | artifact | artifact_kind=impl | - | design | [AR §7.3] "`artifact{impl}`"; [OP-4] | By capture. |
| WC-020 | tester | measurement | - | env, measured_on | design | [AR §7.3] "`measurement` (env + `measured_on` mandatory)"; [50 §6.5] | - |
| WC-021 | tester | artifact | artifact_kind=test | - | design | [AR §7.3] "`artifact{test}`"; [OP-4] | By capture. |
| WC-022 | tester | finding | f_kind=test | - | design | [AR §7.3] "findings `f_kind=test`"; [50 §6.5] | - |
| WC-023 | results-analyst | verdict | role=analyst | return_to | design | [AR §7.3] "`verdict{role=analyst, return_to}`" | - |
| WC-024 | results-analyst | task | work_kind=debt | - | design | [AR §7.3] "`task{work_kind=debt}`" | - |
| WC-025 | project-analyst | finding | - | local_id | design | [AR §7.3] "findings with global `local_id`" | "Global" is not defined further by the design. |
| WC-026 | project-analyst | note | - | - | design | [AR §7.3] | - |
| WC-027 | doc-writer | artifact | artifact_kind=page | - | design | [AR §7.3] "`artifact{page}` + `derived_from`"; [OP-4] | By capture. |
| WC-028 | general-purpose | finding | - | - | design | [AR §7.3]; [90 §4.3]; [50 §6.5] | - |
| WC-029 | general-purpose | note | - | - | design | [AR §7.3]; [90 §4.3]; [50 §6.5] | - |
| WC-030 | general-purpose | question | - | - | design | [AR §7.3]; [90 §4.3]; [50 §6.5] | - |

<!-- table: role-values -->
| row | field | value | roles | requires | basis | source | note |
|---|---|---|---|---|---|---|---|
| WA-001 | authority | owner | owner, orchestrator | owner-attested, owner_quote | design | [AR §7.3] orchestrator "`authority = owner` only with `--owner-quote`", owner "`rule{authority=owner}`"; [AR §13] `policy.role.<role>.authority-owner` | On create or `SET`. The orchestrator row reaches it only with the owner quote, which makes the call owner-attested (WT-012). |
| WA-002 | authority | orchestrator, measured, research, agent | * | - | proposed | [AR §3.1]; [OP-17] | The design restricts only `owner`. |
| WA-003 | relink | owner/* | owner | owner-attested | design | [40 §3.7] "Provenance is `owner/<evidence>/<score>` for the owner, `agent/<evidence>/<score>` for any agent role"; [40 §2.11] R-17 | Written by `links fix`; any other role writes `agent/*`. |
| WA-004 | relink | confirmed/* | orchestrator, owner | not-acceptor | design | [40 §3.7] `--confirm`; [AR §13] `files.confirm-roles` | Only through WV-038. |
| WA-005 | criticality | critical, high, normal, low | * | - | proposed | [AR §3.1]; [OP-17] | Unrestricted by the design, for every kind a role may create. |

<!-- table: role-fields -->
| row | role | kind | scope | fields | key | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| WF-001 | orchestrator | * | any | * | - | design | [AR §7.3]; [50 §6.5] "any writable field" | - |
| WF-002 | owner | * | any | * | - | design | [AR §7.3]; [50 §6.5] | - |
| WF-003 | * | * | created-in-tx | * | - | derived | [50 §3.10] items 2, 6; [50 §4.2] `remember` | The fields of a node the same block creates, within the role's `role-create` row (its constraint values may not be changed). |
| WF-004 | architect | doc | own-role | * | - | design | [50 §6.5] "fields of own docs, decisions and questions"; [OP-2]; [OP-24] | Including `body` (`doc patch`) and `parent` (`MOVE`). |
| WF-005 | architect | decision | own-role | * | - | design | [50 §6.5]; [OP-24] | An accepted decision is never edited, only superseded ([AR §3.2]); the status machine refuses that, not this row. |
| WF-006 | architect | question | own-role | * | - | design | [50 §6.5] | - |
| WF-007 | developer | task | leased-task | files_owned | policy.role.developer.fields | design | [AR §7.3] "a developer may set `files_owned` of its leased task"; [50 §6.5]; [AR §13] | The key widens the list (policy data); its default is `files_owned`. |
| WF-008 | researcher | artifact | own-role | artifact_kind | - | derived | [RULES/role-write-policy WC-008] | Only to `research`. |
| WF-009 | developer | artifact | own-role | artifact_kind | - | derived | [RULES/role-write-policy WC-019] | Only to `impl`. |
| WF-010 | tester | artifact | own-role | artifact_kind | - | derived | [RULES/role-write-policy WC-021] | Only to `test`. |
| WF-011 | doc-writer | artifact | own-role | artifact_kind | - | derived | [RULES/role-write-policy WC-027] | Only to `page`. |

<!-- table: role-status -->
| row | role | kind | scope | from | to | via | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|
| WS-001 | orchestrator | * | any | * | * | any | design | [AR §7.3]; [50 §6.5] | Within the kind's status machine ([AR §3.6]). |
| WS-002 | owner | * | any | * | * | any | design | [AR §7.3] owner row | `question` becomes `answered` through an `ANSWERS` edge (WV-018). |
| WS-003 | developer | task | leased-task | open | in_progress | claim-start | design | [AR §6.2] "`--start` ... performs `open -> in_progress`, otherwise the first `set`/`complete` under the lease performs it" | Also the implicit move by the first write under the lease. |
| WS-004 | developer | task | leased-task | open, in_progress | done | tx-complete | design | [AR §7.3] "`claim`/`complete` with lease"; [50 §6.5]; [AR §6.2] | `complete` from `open` is the compound move in one commit. Outcomes `failed` and `abandoned` are [API]'s. |
| WS-005 | tester | task | leased-task | open | in_progress | claim-start | design | [AR §6.2]; [AR §7.3] tester "task status except `complete` of own claim" | As WS-003. |
| WS-006 | tester | task | leased-task | open, in_progress | done | tx-complete | design | [AR §7.3]; [50 §6.5] "`tx.complete` of own claim" | - |
| WS-007 | architecture-critic | finding | own-role | open | withdrawn | set | design | [AR §7.3] "own findings -> withdrawn"; [50 §6.5]; [OP-2] | - |
| WS-008 | code-reviewer | finding | own-role | open | withdrawn | set | design | [AR §7.3]; [50 §6.5]; [OP-2] | - |
| WS-009 | refuter | finding | any | open | confirmed | set | design | [AR §7.3] "finding status confirmed/refuted"; [50 §6.5] | - |
| WS-010 | refuter | finding | any | open | refuted | set | design | [AR §7.3]; [50 §6.5] | - |

<!-- table: role-edges -->
| row | role | edge | ops | src_scope | dst_scope | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| WE-001 | orchestrator | * | create, delete | any | any | design | [AR §7.3]; [50 §6.5] | `AT` only through the link verbs (WV-034, WV-035) for every role. |
| WE-002 | owner | * | create, delete | any | any | design | [AR §7.3]; [50 §6.5] | - |
| WE-003 | * | about | create | created-in-tx | any | derived | [AR §7.2] `remember` `about[]`; [AR §7.1] `--about ID,..` | The `ABOUT` edges given with a node the same block creates. |
| WE-004 | architect | depends_on | create, delete | own-role | any | design | [50 §6.5] "`CREATE`/`DELETE` of `DEPENDS_ON`, `IMPLEMENTS`, `ABOUT` edges from own nodes" | - |
| WE-005 | architect | implements | create, delete | any | own-role | proposed | [50 §6.5]; [50 §2.5] `IMPLEMENTS` "task, artifact -> decision, doc"; [OP-11] | `IMPLEMENTS` cannot start at an architect's node, so the row reads "into own docs and decisions". |
| WE-006 | architect | about | create, delete | own-role | any | design | [50 §6.5] | From the architect's questions and deviation findings. |
| WE-007 | architecture-critic | derived_from | create | own-role | any | design | [AR §7.3] "(+ `derived_from` edges)"; [50 §6.5] "verdict (+ `DERIVED_FROM`, `GATES`)"; [OP-13] | - |
| WE-008 | architecture-critic | gates | create | own-role | any | design | [50 §6.5]; [OP-13] | - |
| WE-009 | code-reviewer | derived_from | create | own-role | any | design | [AR §7.3]; [50 §6.5]; [OP-13] | - |
| WE-010 | code-reviewer | gates | create | own-role | any | design | [50 §6.5]; [OP-13] | - |
| WE-011 | refuter | refutes | create | own-role | any | design | [AR §7.3] "`refutes`/`confirms` edges"; [50 §6.5]; [OP-13] | - |
| WE-012 | refuter | confirms | create | own-role | any | design | [AR §7.3]; [50 §6.5]; [OP-13] | - |
| WE-013 | results-analyst | derived_from | create | own-role | any | derived | [50 §4.2] `remember` "a `verdict`'s `DERIVED_FROM` edges written"; [OP-13] | From the analyst's verdicts. |
| WE-014 | doc-writer | derived_from | create | own-role | any | design | [AR §7.3] "`artifact{page}` + `derived_from`"; [OP-12] | - |
| WE-015 | * | consumed | create | lease-run | any | proposed | [AR §7.4] step 5; [OP-21] | Only through `pack --record-run` (WV-040). |
| WE-016 | * | mentions | - | any | any | design | [AR §3.3] `mentions` "parsed from `#N` ... at write time" | Derived from text by the engine; never written by a statement, never checked. |

## 10. Reads, hooks and refusals

<!-- table: role-reads -->
| row | subject | what | allowed | key | refusal | exit | basis | source | note |
|---|---|---|---|---|---|---|---|---|---|
| WQ-001 | * | read-verbs | yes | - | - | - | design | [50 §6.5] "Reads are the default everywhere" | WV-042. |
| WQ-002 | * | named-query | yes | - | - | - | design | [50 §4.4] | - |
| WQ-003 | * | free-form-query | yes | query.safelist.<role> | E406 | 6 | design | [AR §7.3] "unless its safelist (`query.safelist.<role> = named-only`) restricts it"; [50 §4.4]; [OP-18] | Default `off`; under `named-only` a free-form `q` or `query` is refused. |
| WQ-004 | profile-unknown | free-form-tx | no | query.safelist.model.unknown | E411 | 6 | design | [90 §8.1] L2; [AR §13] `query.safelist.model.<profile>`; [F19 §11.1] | Default `named-only` for `unknown`; WR-012. |
| WQ-005 | profile-unknown | dry-targets-tx | yes | query.safelist.model.unknown | - | - | design | [90 §8.1] L2 | Only when the key is `dry-targets` (opt-in): a `TX` applied with `IF TARGETS` from a `DRY`. |

<!-- table: role-hooks -->
| row | hook | writes | basis | source | note |
|---|---|---|---|---|---|
| WH-001 | session-start | orchestrator-session-lease, link-settle, image-export-checkpoint, session-cursor | design | [AR §7.5] `SessionStart`; [90 §4.3] mint (i); [OP-15] | The lease only in a main session (WM-005); a worker's `SessionStart` mints none ([90 §7.5]). |
| WH-002 | subagent-start | session-mark, clean-auto-sync | design | [AR §7.5] `SubagentStart`; [AR §13] `hooks.subagent-start.auto-sync`, `hooks.sync-auto-keys`; [OP-15] | The sync only on the bound lane, with zero conflicts, zero violations and at most `hooks.sync-auto-keys` keys. |
| WH-003 | user-prompt-submit | session-cursor | design | [AR §7.5] `UserPromptSubmit` | Lazy; skipped when the writer byte is busy. |
| WH-004 | subagent-stop | release-or-flag-own-leases, needs-triage-note | design | [AR §7.5] `SubagentStop`; [OP-15] | Only leases held by the stopping `agent_id`; the note is written with the `general-purpose` row. |
| WH-005 | agent-launched | none | design | [AR §7.5]; [73 F17] | An in-memory map only. |
| WH-006 | stamp | none | design | [AR §7.2]; [AR §7.5]; [73 F17] | An in-memory context only; returns `permissionDecision`. |
| WH-007 | fs-evidence | runtime-evidence | design | [AR §7.5]; [40 §6.4] | `PENDING`, `FILEOBS`, `ANCHORRES`, the tree's dirty row; never a versioned write. |
| WH-008 | git-hooks | link-settle, binding-refresh | design | [AR §7.5] git rows | Installed only by WV-047. |

<!-- table: role-refusals -->
| row | situation | code | name | exit | text_owner | basis | source | note |
|---|---|---|---|---|---|---|---|---|
| WZ-001 | role-policy | E406 | role_policy | 6 | F19 | design | [50 §5.2] E406; [AR §7.3]; [90 §4.3] | Names the statement (1-based), the table and the (role, op, kind, field) no row allowed ([OP-23]). For an unleased caller it adds the fix line of [AR §7.3]: "this write needs a lease; an orchestrator presents its session lease with --lease (mint it once per session: moirai claim --role orchestrator --session)" (152 B). That is the design's one-line text; [F19 §11.3] and [LQ/errors §5.5] render the same words split at the semicolon, as the message `this write needs a lease` and a `= help:` line with the rest, a deliberate rendering and no change of wording (review pass 1, A1-51). |
| WZ-002 | cli-only-over-mcp | E406 | role_policy | 6 | F19 | design | [50 §6.3]; [AR §7.2] | WX-013 to WX-016. |
| WZ-003 | lease-invalid | E407 | lease | 5 | F19 | design | [50 §5.2] E407 "missing, stale token, branch mismatch"; [AR §6.2] | WR-003, WR-005. |
| WZ-004 | declared-agent-mismatch | E407 | lease | 5 | F19 | design | [90 §4.1] Actor row; [90 §10.1] "two exit-5 texts"; [F19 §11.2]; [LQ/errors §5.5] | WR-004. The "declared agent" row of E407. |
| WZ-005 | env-lease-bound | E407 | lease | 5 | F19 | design | [90 §4.1] binding rule; [90 §10.1]; [F19 §11.2]; [LQ/errors §5.5] | WR-002. The design's text: "`L-18 is bound to codex:T1; pass your own lease`". The "bound lease" row of E407. |
| WZ-006 | write-in-read-surface | E006 | read_only | 2 | F19 | design | [50 §5.2] E006 | WX-017. |
| WZ-007 | not-writable | E115 | not_writable | 2 | F19 | design | [50 §5.2] E115; [50 §3.10] | Fields outside W (WT-009) that no verb may set. |
| WZ-008 | read-only-view | E305 | read_only_view | 6 | F19 | design | [50 §5.2] E305; [50 §3.9] item 6 | A `TX ON` a commit, tag, masked `plan/*` field or staging ref. |
| WZ-009 | not-writer-tree | not_writer_tree | not_writer_tree | 5 | F19 | design | [40 §5.3]; [AR §7.1] exit 5 "a file verb outside the writer tree"; [F19 §10.2] `not_writer_tree` | WV-030 to WV-032. The code and its text are [F19 §10.2]'s, which owns the store and file-verb codes (review pass 1 round 2, A1-39); the first draft named [F18], which defines no code. |
| WZ-010 | unknown-model-write | E411 | unknown_model_write | 6 | F19 | design | [90 §8.1] L2 "a new error code naming the matching named mutation"; [90 §10.1]; [F19 §11.1]; [LQ/errors §5.5] | WR-012, WQ-004. |
| WZ-011 | safelist | E406 | role_policy | 6 | F19 | proposed | [50 §4.4] safelist; [OP-18] | WQ-003. |
| WZ-012 | mint-refused | E406 | role_policy | 6 | F19 | proposed | [90 §4.3] mint; [OP-19] | WM-001 to WM-005, WM-008. |
| WZ-013 | confirm-by-acceptor | E406 | role_policy | 6 | F19 | design | [40 §3.7] "must come from an actor other than the acceptor" | WV-038. |

## 11. Source map

<!-- table: role-source-map -->
| row | source_row | realized_by | note |
|---|---|---|---|
| WY-001 | AR-7.3-where-rights-come-from | WR-001, WR-002, WR-003, WR-006, WR-007, WM-001, WM-002, WM-004, WM-005, WZ-001 | - |
| WY-002 | AR-7.3-orchestrator | WO-001, WC-001, WF-001, WS-001, WE-001, WV-001, WV-006, WV-012, WA-001 | - |
| WY-003 | AR-7.3-owner | WO-002, WC-002, WF-002, WS-002, WE-002, WV-018, WV-047, WA-001, WA-003 | - |
| WY-004 | AR-7.3-architect | WO-003, WC-003, WC-004, WC-005, WC-006, WF-004, WF-005, WF-006, WE-004, WE-005, WE-006 | - |
| WY-005 | AR-7.3-researcher | WO-004, WC-007, WC-008, WC-009, WC-010, WF-008 | - |
| WY-006 | AR-7.3-critic-reviewer | WO-005, WO-006, WC-011, WC-012, WC-013, WC-014, WS-007, WS-008, WE-007, WE-008, WE-009, WE-010 | - |
| WY-007 | AR-7.3-refuter | WO-007, WC-015, WS-009, WS-010, WE-011, WE-012 | - |
| WY-008 | AR-7.3-developer | WO-008, WC-016, WC-017, WC-018, WC-019, WF-007, WF-009, WS-003, WS-004, WX-008, WV-035 | - |
| WY-009 | AR-7.3-tester | WO-009, WC-020, WC-021, WC-022, WF-010, WS-005, WS-006, WX-008 | - |
| WY-010 | AR-7.3-results-analyst | WO-010, WC-023, WC-024, WE-013 | - |
| WY-011 | AR-7.3-project-analyst | WO-011, WC-025, WC-026 | - |
| WY-012 | AR-7.3-doc-writer | WO-012, WC-027, WF-011, WE-014 | - |
| WY-013 | AR-7.3-general-purpose | WO-013, WC-028, WC-029, WC-030, WV-044 | - |
| WY-014 | AR-7.3-per-statement | WR-008, WR-009, WR-010, WX-001, WX-002, WX-003, WX-004, WX-005 | - |
| WY-015 | AR-7.3-R4-rows | WV-029, WV-030, WV-031, WV-032, WV-033, WV-034, WV-035, WV-036, WV-037, WV-038, WV-039, WV-047, WA-003, WA-004 | - |
| WY-016 | 50-6.3-write-tool | WV-043, WX-013, WX-014, WX-015, WX-016, WZ-002 | - |
| WY-017 | 50-6.5-table | WF-003, WE-003, WX-008, WX-017, WX-018, WR-009 | The per-role rows of [50 §6.5] are mapped by WY-002 to WY-013. |
| WY-018 | 90-4.3-mint | WM-001, WM-002, WM-003, WM-004, WM-005, WZ-012 | - |
| WY-019 | 90-4.1-resolver | WR-002, WR-004, WR-005, WT-002, WT-003, WT-013 | - |
| WY-020 | 90-8.1-L2 | WR-012, WQ-004, WQ-005, WZ-010 | - |

## Coverage

Rule tables specify semantics. The layouts of the values used here are [F11]'s (`LEASES` rows), [F09]'s (`CREATOR`),
[F06]'s (ops) and [F12]'s (the commit's role and `actor_src`); the refusal texts are [F19]'s.

| Checklist row | Covered by |
|---|---|
| [90 §10.1] `LEASES` runtime rows: `kind ∈ {task, role}`, `role`, `run`, `bound` | WR-002, WR-005, WR-006, WO rows `carried_by`, WM-002, WM-004, WM-005 (meaning of each field for rights; layout [F11]) |
| [90 §10.1] "Error table and refusal texts": the new unknown-profile write code; the two exit-5 texts | WZ-004, WZ-005, WZ-010, WR-004, WR-012 (the codes are [F19 §11]'s: E411 and two E407 rows) |
| [60 §2.5] "Harness-agnostic interface" row: `LEASES` fields, one error code, two exit-5 texts | as the two rows above |
| [50 §8.1] F4: cold column `CREATOR` (actor, role) | WT-004 (use; layout [F09]) |
| [40 §2.11] R-13: config key `files.confirm-roles` | WV-038, WV-039, WA-004 |
| [40 §2.11] R-17: `relink` provenance vocabulary, `agent/*` distinct from `owner/*` and `confirmed/*` | WA-003, WA-004 (who may write which prefix; the vocabulary is [F18]'s) |

No X-F row concerns the role policy.

## Holes

None. No value in this file waits on an M0 measurement. The three refusal codes first written as holes were naming
decisions, which [F01 §2.5] does not make holes, and are decided (review pass 1, S1-40; [HOLES.md](../HOLES.md) §3):
E411 `unknown_model_write`, exit 6, for WQ-004 and WZ-010 ([F19 §11.1]); the "declared agent" and "bound lease" rows
of E407 `lease`, exit 5, for WZ-004 and WZ-005 ([F19 §11.2]).

## Open points for the review

1. **How the owner is recognised.** The design's owner row is "main session, `--by owner`", but rights come only from a
   presented lease and no lease has the role `owner`. Proposed (WT-012, WR-006): `owner` is the orchestrator's session
   role lease plus the owner attestation (`--by owner`, or `--authority owner` with the quote file), with the stamp's
   `ask` permission putting MCP owner-authority writes before the human. The human at a terminal mints the session
   lease like any orchestrator (`moirai claim --role orchestrator --session`, allowed because the terminal is neither a
   known subagent nor a dispatched worker). The review confirms, or names another attestation.
2. **"Own" means same role** (WT-004). The design's "own findings", "own docs" and "own anchors" have no definition.
   Each critic or architect round is a new agent, and [AR §7.6] step 4 has a round-2 `doc patch` of the round-1 plan, so
   an actor-based reading would block the review loop. Proposed: `CREATOR.role` ([50 §8.1] F4) for nodes, and the role
   of the adding commit for anchors. The same reading is [RULES/pack-classes] Open point 4.
3. **"Deviation" is not an `f_kind`.** [AR §7.3] and [50 §6.5] grant "deviation findings" to the architect and the
   developer, but the `f_kind` set of [AR §3.2] has no `deviation`. Proposed: a deviation finding is `f_kind = plan`
   (WC-006, WC-018). Alternative for WP-14: add `deviation` to the enum (a weakening schema change).
4. **Artifact kinds and capture.** `artifact{research}`, `{impl}`, `{test}` and `{page}` need `research`, `impl`,
   `test` and `page` in the "run-output kinds" of `artifact_kind`, which [AR §3.2] leaves unlisted; WP-14 ([F08]) must
   list them. Since `CREATE (x:artifact ...)` is E115 ([50 §3.10]), these rows apply to capture and to the
   `artifact_kind` set on the node the role captured (WF-008 to WF-011).
5. **Harness agent types are not roles.** Claude Code's built-in subagent type is literally `general-purpose`, the
   name of the unleased row. If labels always narrowed, a Workflow worker of that type presenting a developer lease
   would drop to the `general-purpose` row and could not complete its task. Proposed (WR-007): a label narrows only
   when it names a `role-rows` role other than `general-purpose`; built-in types (`general-purpose`, `Explore`,
   `Plan`) are recorded and ignored.
6. **Who completes.** Only developer and tester (and the orchestrator and owner) may `tx.complete` (WX-008, WS-004,
   WS-006); [AR §7.3] denies task status to the other roles. In the dispatcher pattern `apply --from` completes an entry
   under the orchestrator's rights after validating the entry's lease against the run (WV-022), so a reviewer's result
   still completes its review task.
7. **What `apply --from` checks per entry.** [90 §7.2] says `apply` "validates each lease against the run and ignores
   every self-reported identity field other than the lease" but not whose rights apply. Proposed (WV-022): findings and
   notes carried in `result.v1` are checked against the entry lease's role row and created with that role as
   `CREATOR.role`; outcomes run under the presenter's rights.
8. **`mcp_write` defaults.** [AR §13] lists `policy.role.<role>.mcp-write` as "yes for architect, architecture-critic,
   researcher, project-analyst"; read literally, every other role (orchestrator included) is refused the MCP `write`
   tool by default and writes through the CLI, `remember`, `claim` and `complete`. A Codex code-mode developer whose CLI
   write exits 7 is told to use the MCP tool ([90 §5.3]); for `set files_owned` that is `write`, which WO-008 refuses,
   so its fallback is `result.v1`. The review confirms or adds developer and tester.
9. **File verbs and `file add`.** The design says where `file mv|rm|revert` run (CLI, writer tree) but not for whom.
   Proposed: the orchestrator, owner, developer, tester and doc-writer (the roles with file work in [AR §7.3]); the
   architect's `links fix` is limited to doc files; `file relink --after` follows `links fix`; `file add` is open to
   every leased role, since `link --at` already registers file nodes as a side effect.
10. **`may-write`** (WT-010) decides `link --at` and `unlink`: a role may link a node it may change by some row, or a
    node of its role it was allowed to create. The unleased row may therefore not link its own findings after the
    call that created them.
11. **Architect `IMPLEMENTS`.** [50 §6.5] grants `IMPLEMENTS` "from own nodes", but `IMPLEMENTS` starts at tasks and
    artifacts, which an architect cannot create. Proposed (WE-005): `IMPLEMENTS` edges into the architect's own docs
    and decisions.
12. **Doc-writer `DERIVED_FROM`.** [AR §7.3] grants `artifact{page}` + `derived_from`, but [50 §2.5] declares
    `DERIVED_FROM` sources as note, doc and verdict only. WP-14 ([F08]) should add `artifact`, or the grant is empty.
13. **Edge deletion.** [50 §6.3] allows `DELETE` of an edge "to every role the policy lets write the edge's source",
    and [50 §6.5] says edge `DELETE` "follows the table". Proposed: the rows state the ops; critics, refuters, analysts
    and doc-writers may only create their edges (a verdict is "immutable once written", [AR §3.2]); the architect
    creates and deletes its three kinds; `AT` anchors follow WV-035.
14. **Administrative verbs.** The design names only "branch/merge/image verbs" and `hooks install --git`. Proposed: verbs
    that change store state outside the graph (`config set` at store scope, `quiet`, `gc`, `backup`, `restore`,
    `repair`, `migrate`) are orchestrator and owner (WV-048); verbs that write no store (`init`, `hooks install`
    without `--git`, `integrate`, `export`, `config set --user`) are not role-policed (WV-049), because rights come from
    a lease in a store and `init` has none. The risk: an agent may change user-scope keys (for example the security
    policy for data leaving the machine); the review decides whether `config set --user` needs a TTY or the owner.
15. **Hooks write as fixed functions** (WR-014, WH rows). The design lets hooks mint the session lease, auto-apply a clean
    sync on the lane (a `ref` operation, WV-005), release leases and write a triage note, without saying under which
    rights. Proposed: a hook handler writes only its listed functions and never model-supplied text.
16. **Side effects of allowed verbs** (WR-013). `file mv` rewrites globs in other nodes' `files_owned` and `applies_to`
    ([AR §7.1] example), which the moving role could not set directly. Proposed: such fixed side effects are part of
    the verb.
17. **Values the design leaves open.** `criticality` and every `authority` except `owner` are unrestricted (WA-002,
    WA-005), so an unleased caller may write a `critical` note, which then reaches every brief ([RULES/pack-classes]
    BR-008). The review decides whether `critical` needs the orchestrator; this file keeps the design's literal reading.
18. **Safelist refusal code.** [50 §4.4] does not name the code for a free-form query refused under `named-only`.
    Proposed: E406 (WZ-011), since it is a role rule, although E406 is listed as a `tx` error.
19. **Mint refusals** use E406 (WZ-012). `hooks.session-start.orchestrator-lease` governs only the hook's mint; the
    orchestrate skill's mint is open to any caller that is neither a known subagent nor a dispatched worker.
20. **A lease role without a row** (for example a project-specific role minted by the orchestrator) gets the
    `general-purpose` row (WR-006): fail closed, no refusal at mint.
21. **Rights for `check`, `links sync` and `pack --record-run`.** `check` and `links sync` record only derivable facts
    and exact observations, so every role may run them (WV-024, WV-036). `pack --record-run` writes `CONSUMED` edges
    from the run its lease names (WV-040, WE-015); without a run lease only the orchestrator and owner may.
22. **Bulk targets.** [AR §13] says "orchestrator only", [50 §6.5] gives bulk to "orchestrator, owner". The owner row
    contains the orchestrator's, so WX-005 lists both.
23. **E406's text.** An allowlist has no row to cite for a refusal. Proposed: E406 names the statement, the table and
    the (role, op, kind, field) tuple no row allowed; [F19] freezes the spelling.
24. **The architect changes no status.** "Fields of own docs, decisions and questions" is read without `status`, which
    [50 §6.5] treats apart from fields; accepting a decision or making a plan `current` stays with the orchestrator and
    owner.
25. **PLAN §3.3 assigns no gap to WP-90.** Every resolution above was found while writing this table.
26. **Registry rows for [RULES/README] §7.** README §1.1 now lists `role-write-policy.md` (the WP-90 "role write
    policy" of [m0/PLAN §3.2] item 9), and §7 registers its tables as RG-054 to RG-068 (review pass 1 S1-47), with
    these rows:

    ```
    | RG-054 | `role-terms` | role-write-policy.md | vocabulary | WT | row:id, term:token, sort:enum(input/scope/set/pred), basis:enum, source:cite, definition:text | §3 |
    | RG-055 | `role-rights` | role-write-policy.md | procedure | WR | row:id, step:int, rule:token, basis:enum, source:cite, definition:text | §4 |
    | RG-056 | `role-rows` | role-write-policy.md | vocabulary | WO | row:id, role:token, carried_by:tokens, self_claim:enum(yes/no), mcp_write:enum(yes/no), basis:enum, source:cite, note:text | §5 |
    | RG-057 | `role-mint` | role-write-policy.md | decision | WM | row:id, form:token, commands:tokens, allowed:tokens, key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §6 |
    | RG-058 | `role-verbs` | role-write-policy.md | decision | WV | row:id, verb:token, class:enum(ref/graph/runtime/file-fs/file-link/admin/read/surface), surface:enum(cli/mcp/both), roles:tokens, key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §7 |
    | RG-059 | `role-statements` | role-write-policy.md | decision | WX | row:id, statement:token, roles:tokens, surface:enum(cli/mcp/both), key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §8 |
    | RG-060 | `role-create` | role-write-policy.md | decision | WC | row:id, role:token, kind:token, constraint:tokens, required:tokens, basis:enum, source:cite, note:text | §9 |
    | RG-061 | `role-values` | role-write-policy.md | decision | WA | row:id, field:token, value:tokens, roles:tokens, requires:tokens, basis:enum, source:cite, note:text | §9 |
    | RG-062 | `role-fields` | role-write-policy.md | decision | WF | row:id, role:token, kind:token, scope:token, fields:tokens, key:token, basis:enum, source:cite, note:text | §9 |
    | RG-063 | `role-status` | role-write-policy.md | decision | WS | row:id, role:token, kind:token, scope:token, from:tokens, to:token, via:token, basis:enum, source:cite, note:text | §9 |
    | RG-064 | `role-edges` | role-write-policy.md | decision | WE | row:id, role:token, edge:token, ops:tokens, src_scope:token, dst_scope:token, basis:enum, source:cite, note:text | §9 |
    | RG-065 | `role-reads` | role-write-policy.md | decision | WQ | row:id, subject:token, what:token, allowed:enum(yes/no), key:token, refusal:token, exit:token, basis:enum, source:cite, note:text | §10 |
    | RG-066 | `role-hooks` | role-write-policy.md | procedure | WH | row:id, hook:token, writes:tokens, basis:enum, source:cite, note:text | §10 |
    | RG-067 | `role-refusals` | role-write-policy.md | vocabulary | WZ | row:id, situation:token, code:token, name:token, exit:token, text_owner:token, basis:enum, source:cite, note:text | §10 |
    | RG-068 | `role-source-map` | role-write-policy.md | map | WY | row:id, source_row:token, realized_by:tokens, note:text | §11 |
    ```

    The RG numbers follow the placeholders of [RULES/pack-classes] Open point 20. These decision tables are allowlists
    (§2), not first-match tables; README §8 needs one sentence for them.
27. **Allowlist readings of silent verbs.** `reclaim`, `run open|close`, `links import` and `reopen` appear in no role's
    row, so only the orchestrator and owner may run them (WM-008, WV-023, WV-028, WV-041, WX-006, WX-011).
