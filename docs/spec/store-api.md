# The logical `Store` API

| | |
|---|---|
| Title | The logical `Store` API: typed commands forming the semantic core of every write, version-control and maintenance verb; typed results in the `--json v1` data shape; the caller context resolved as data; the injected deterministic environment (clock, boot identity, liveness slots, entropy, simulated project trees and git histories); `state(ref)` with its digests; and the rule by which the reference model and the engine are compared |
| Chapter | [API], `docs/spec/store-api.md`; examples in `docs/spec/store-api/examples/*.json` |
| Status | draft, pass 1 pending |
| Work package | WP-25 (R-SPEC-F), [PLAN §3.2] item 2 |
| Sources | [60 §3.1] item 2 and item 9; [60 §4.1]–[60 §4.4] (the model's role, scope, what is out of scope, how engine and model are compared); [60 §3.13] GT2, GT18; [60 §2.5] rows "Store parameters", "Derived-state semantics", "Harness-agnostic interface and pure Rust"; [AR §2.13] (T13: the `Store` API is an M0 deliverable); [AR §2.16] (T16: versioned versus runtime state); [AR §3.1]–[AR §3.6]; [AR §4.5] steps 2–11 (the three-phase write, idempotency order, markers from net ops, the printed result); [AR §4.6] "Not hashed" and "Net changeset = state diff"; [AR §5a.1]–[AR §5a.9]; [AR §5d.1]–[AR §5d.3]; [AR §6.2]–[AR §6.6]; [AR §7.1] (verbs, flags, conventions, exit codes), [AR §7.2] (MCP tools, branch resolution), [AR §7.3], [AR §7.6]; [AR §8.3] row "GT2 differential, runtime tables included"; [50 §3.1], [50 §3.8]–[50 §3.10], [50 §4.2], [50 §4.4]; [90 §4.1]–[90 §4.4], [90 §7.1]–[90 §7.2], [90 §10.1]; [40 §3.2]–[40 §3.7], [40 §8.3.2] (subset consistency); [80 §2.7.1], [80 §2.7.2]; [PLAN §3.2] WP-25 and WP-90, [PLAN §6.2] R4; the delegations of the written chapters: [F05 §8.7] and §9.6 (`Idem.result`), [F06 §4.4.7] (the payload of a verb without a `TX` block), [F06 §5.5] (`pathmove.hlc` under the injected clock), [F08 §5.4.4] (order-key generation), [F11 §8] and its open point 4, [F17 §1.5] SP-1, SP-2 and OP-17-17, [F19 §1.2], §8.2 and its open point 30, [CFG §7.6] and its open point 11, [OS/clock §1], [RULES/status-machines] open point 16 (rows CO-002, CO-003), [RULES/role-write-policy] WS-004, [RULES/pack-classes] `HOLE(pack-digest-param)`; the rules this chapter follows from chapters written after its first draft: [F12 §2.4]–§2.6 (ref-name rules and completion), §3.2 and §3.8 (commit literals and forms), §6.6 (key texts), §9 (staging); [F07 §6.3] (canonical values, defaults absent); [LQ/std §4.15] (the `diff` order) and §7.3 (procedure yields); [LQ/errors §5.7] (code-specific keys); [RULES/role-write-policy] WR-005; [RULES/delete-policy-matrix] DP-003; [OS/clock §7] |
| Depends on | [F01], [F02], [F05], [F06], [F08]; cites [F03], [F04], [F07], [F11], [F12], [F13], [F14], [F16], [F17], [F18], [F19], [F20], [CFG], [OS/clock], [OS/proc], [OS/path], [LQ/envelope], [LQ/errors], [LQ/std], [LQ/canonical-ast], [LQ/json-ir], [RULES/state-definition], [RULES/status-machines], [RULES/role-write-policy], [RULES/delete-policy-matrix], [RULES/merge-table], [RULES/link-merge-rules], [RULES/pack-classes] |

## 1. Scope

### 1.1 What the API is

The logical `Store` API is the interface of [60 §3.1] item 2. It has four parts:

- **Commands.** Typed operations, each the semantic core of one or more verbs. Every write, version-control and
  maintenance verb of [AR §7.1], every MCP tool call of [AR §7.2] and every hook that writes compiles to exactly one command
  (the door table of §18). Every read verb is one command, `Query`, because read verbs are named queries ([AR §7.7.2]).
- **Results** in the `--json v1` data shape of [F19 §8] and [LQ/envelope §7] (§3).
- **`state(ref)`**, a snapshot of the versioned state at a view, and its two digests (§15).
- **The injected deterministic environment** (§6): the wall and boot clocks, the boot identity, the liveness-slot table,
  the entropy, and, for R4, simulated project trees and abstract git histories. [OS/clock §1] calls its clock part "the
  Store API's injected deterministic clock".

### 1.2 Who implements it

- **The reference model**, from M0: every command of this chapter, with its own Rust types (WP-90 to WP-94, [PLAN §3.2]
  item 9). The model has no JSON code; `moirai-testkit` (M1) converts its results into the JSON of §3–§5 ([PLAN §3.2] item 9),
  and `moirai-lqbench` converts JSON IR into its AST ([LQ/json-ir §1]).
- **The engine**, from M1: through the storage driver and `moirai-testkit` ([PLAN §2.3]), each command group from the
  milestone §2.2 names.
- **No shared Rust type** ([PLAN §6.2] R4). The two implementations meet only in the JSON of this chapter. GT2 runs one
  stream of commands on both and compares them by §16 ([60 §4.4]).

### 1.3 What this chapter owns and what it cites

| This chapter owns | § |
|---|---|
| the command envelope, the result families and their keys | §3 |
| the caller context and its resolution, as data ([90 §4.1]) | §4 |
| the JSON encodings of identifiers, typed values, keys and diff rows | §5 |
| the injected environment and the environment commands | §6 |
| idempotency: which commands are keyed, the payload of a command that compiles to no `TX` block, replays | §7 |
| every command: arguments, effects, refusals and result `data` | §8–§14 |
| `state(ref)`, `content_digest` and `state_digest`; the runtime and history snapshots | §15, §14 |
| the comparison of model and engine, and the fields it excludes | §16 |
| the bytes of `Idem.result` ([F05 §9.6]); the order-key generation ([F08 §5.4.4]); the random-uid and store-id derivations under the injected entropy | §17 |
| the door table | §18 |

| This chapter cites | Owner |
|---|---|
| the bytes of log records, commit records, values, rows | [F05], [F06], [F08], [F11] |
| the canonical form, `commit_id`, `changeset_digest`, message normalisation | [F07] |
| ref names and their rule, revisions, the recursive virtual base, the conflict classes and the text form of conflict keys | [F12] |
| invariants, the validator order, the marker cache and its equivalence obligation | [F13], [RULES/state-definition] |
| which groups a command appends, their durability classes, when they are flushed and published | [F16] |
| store parameters and their visibility classes; configuration keys | [F17], [CFG] |
| exit codes, error codes, error and warning texts | [F19], [LQ/errors] |
| LQ semantics, the `TX` result keys, the standard named mutations and relations, the canonical AST and `H` | [LQ/envelope], [LQ/std], [LQ/canonical-ast] |
| status machines, role write policy, delete policies, merge rules, link merge rules | [RULES/*] |
| clocks, stamps, deadlines, boot identity, lock-anchored liveness | [OS/clock], [OS/proc] |
| R4 strings, bindings and the resolver constants | [F18], [F20] |

### 1.4 Terms

| Term | Meaning |
|---|---|
| **stream** | a sequence of command envelopes, numbered n = 1, 2, 3, … (§2.1) |
| **executor** | the one sequential runner of a stream: the model, or the engine behind the testkit |
| **write command** | a command of the groups S, G, C, V, F and I (§2.2) that can append a durable record |
| **observation** | a command of group O: it appends nothing ([40] I-F5) |
| **door** | the front-end path a command came through: `cli` (a CLI verb or `moirai tx`), `mcp` (an MCP tool call), `hook`, or `apply` (an entry of a batch); it decides `stmt_origin` ([F06 §3.4]) and, with the client profile, texts |
| **caller** | the identity, rights, branch, session and tree resolved from a command's context (§4) |
| **view**, **tip**, **live**, **tombstone** | as [F08 §1] |
| **class V**, **class I**, **Rs** | the visibility classes of [F17 §1.5] and [CFG §9.4] |

## 2. Execution model

### 2.1 Streams

- A stream is a sequence of command envelopes (§3.1). One executor runs them in order, and each command sees every effect
  of every earlier one. Environment commands (§6) may appear anywhere; the first store command is `Init` (§8.1).
- The API specifies the logical outcome of each command run alone. Concurrency, crashes and multi-process behaviour are the
  subject of GT1, GT3 and GT4, which check the engine against the model's acknowledged-commit semantics ([60 §4.1],
  [60 §4.4] items 3 and 4); `EnvCrash` (§6.7) is the one crash event a stream carries.
- A command never observes a partial effect of another command: every command is atomic in the sense of §2.3.

### 2.2 Command groups

| Group | Commands | § | Engine from |
|---|---|---|---|
| E environment | `EnvClock`, `EnvSlots`, `EnvTree`, `EnvGit`, `EnvCrash` | §6 | harness: M1 (clock, slots, crash), M6 (trees, git) |
| S store | `Init`, `ConfigSet`, `ConfigUnset`, `Quiet`, `Maintain`, `Gc`, `Backup`, `Restore`, `Repair`, `Verify` | §8 | M1 |
| G graph | `Tx`, `Mutation`, `Apply`, `Schema`, `Migrate` | §9 | M2 (`Tx` in its `lq` and `ir` forms: M7) |
| C coordination | `Claim`, `Heartbeat`, `Release`, `Reclaim`, `Complete`, `RunOpen`, `RunClose` | §10 | M2; the minting policy M8, `session-ttl` renewal M10 ([90 §10.2]) |
| V version control | `BranchCreate`, `BranchDelete`, `Checkout`, `WorktreeBind`, `WorktreeUnbind`, `LaneOpen`, `LaneClose`, `Tag`, `Merge`, `MergeContinue`, `MergeAbort`, `Sync`, `Revert`, `CherryPick`, `Undo`, `OpRestore` | §11 | M1 (refs, pins, `RefUpdate`), M3 |
| F file links | `FileAdd`, `LinkFile`, `UnlinkFile`, `FileMv`, `FileRm`, `FileRelink`, `FileRevert`, `LinksFix`, `LinksSync`, `Check` | §12 | M2 (the data of `FileAdd`, `LinkFile`, `UnlinkFile`), M4 (`Check`), M6 |
| I image | `ImageExport`, `ImageImport` | §13 | M5 |
| O observation | `Query`, `State`, `Runtime`, `History` | §14 | M1 (`State`, `Runtime`, `History`), M7 (`Query`) |

The model implements groups E to F and O at M0. For group I it supplies `state(ref)`, the canonical changesets and the
commit ids ([60 §4.2] row "Image"); it produces no `.moi` bytes ([60 §4.3]).

### 2.3 Outcomes

Every write command ends in exactly one outcome:

| Outcome | What is appended | Exit | Result |
|---|---|---|---|
| `ok` | the groups the command's section names ([F16] fixes their composition and durability) | 0 | success envelope |
| `replayed` | nothing | 0 | the recorded result with `replayed` = true (§7.5) |
| `dry` | nothing | 0 | the would-be result with `dry` = true |
| `staged` | the commit on the staging ref `merge/<dst>/from/<src>` or `import/<ref>` ([AR §5a.7] step 8) | 6 | success keys, then `errors` with `staged` ([F19 §8.6]) |
| `refused` | nothing; for `FileMv`, `FileRm` and `FileRevert` possibly the intent and its abort record (§12.4) | 1–9 | error envelope ([F19 §8.6]) |
| `partial` | the units that committed: the refs of an `ImageImport`, the items of a `FileMv` or `FileRm` of several items (§12.4) | 8 | per unit (`partial_batch`, [F19 §10.2]) |

A refusal writes nothing that changes versioned or runtime state, except the intent records named above, whose net effect is
none. An observation ends in `ok`, `refused` or, for `Query`, `cut` (exit 10, [F19 §7.1]).

### 2.4 The logical store a command changes

| Part | Content | Changed by |
|---|---|---|
| versioned | the commit DAG; each commit's state (`state_at`, [60 §4.2]) | commits of groups G, C, V, F, I |
| refs | names, ids, kinds, tips, `ref_seq_next`, absorbed vectors, deleted flags, reflog ([F11 §3]) | commits and `RefUpdate` records |
| runtime | `LEASES`, `MARKERS`, `IDEM`, `HEADS` (checkouts and bindings), `ALLOC`/`UIDX`; the counters `commit_seq`, `next_id`, `next_anchor`, `fence`, `next_ref_id`; R4's runtime tables; the quiet flag ([F11]) | records of the kinds of [F05 §7] |
| configuration | the effective value of every key ([CFG]) | `Init`, `ConfigSet`, `ConfigUnset` |
| environment | §6 | environment commands |

Engine-internal, and never part of a result, a snapshot or a comparison: lsns, epochs, file numbers, segments, overlays,
pins' file sets, promotion state, `hist` frames, `gitmap` pages, `seq_ring`, `config_gen`, `ProcId`s ([60 §4.3]).

### 2.5 Determinism

- **DT-1.** Every value of every result, every commit id and every digest of §15 is a function of the stream (its environment
  commands included) and of the effective values of the class-V configuration keys ([CFG §9.4]).
- **DT-2.** Class-I store parameters and keys change none of them ([F17 §1.5] SP-1).
- **DT-3.** Every source of variation is injected (§6). An implementation never reads the OS clock, the OS random source, the
  process environment or a real file system while it executes a stream.
- **DT-4.** Allocation is in command order, and inside a command in the order §9.6 fixes: `seq` and `ref_seq`, `#N`, `aN`,
  lease ids and fencing tokens, ref ids, symbol ids, schema ids.
- **DT-5.** Resource-class refusals (Rs) are outside DT-1 (§16.4).

## 3. Envelopes

### 3.1 The command envelope

One JSON object per command:

| # | Key | Type | Present | Content |
|---|---|---|---|---|
| 1 | `api` | int | always | `1` |
| 2 | `n` | int | always | the command's position in the stream, from 1, increasing by 1 |
| 3 | `cmd` | string | always | a command name of §6–§14 |
| 4 | `ctx` | object | always | the caller context (§4.1); `{}` for environment commands and `Init` |
| 5 | `args` | object | always | the command's arguments; `{}` when it takes none |

- An argument whose value equals its default may be omitted; `null` is the same as omitted.
- An unknown key, a missing required argument or a value of the wrong type is refused with `usage` (exit 2), and nothing is
  written. A value of the right type that its field or statement refuses is the command's own refusal (for example
  `bad_value`, [F19 §10.2], or an LQ code).
- In streams and in example files the envelope may carry insignificant whitespace; its canonical form is §5.6.

### 3.2 Result families

| Family | Commands | Keys |
|---|---|---|
| **T** (an LQ write) | `Tx`, `Mutation`, `Claim`, `Heartbeat`, `Release`, `Reclaim`, `Complete`, and the file named mutations `LinkFile`, `UnlinkFile`, `FileRelink`, `LinksFix`, `LinksSync` | [LQ/envelope §7.1] with §7.7, as §3.3 reads them, then `yields` |
| **R** (an LQ read) | `Query` | [LQ/envelope §7.1] |
| **W** (every other write) | groups S and V; `Apply`, `Schema`, `Migrate`, `RunOpen`, `RunClose`, `FileAdd`, `FileMv`, `FileRm`, `FileRevert`, `Check`, `ImageExport`, `ImageImport` | §3.4 |
| **X** (a snapshot or an environment change) | `State`, `Runtime`, `History`; group E | §3.5 |

Every family's refusal is the error envelope of [F19 §8.6]. A `staged` outcome keeps the success keys and appends `errors`
and `exit` ([F19 §8.6]).

### 3.3 Family T

The keys and their order are [LQ/envelope §7.1] and §7.7:
`v`, `branch`, `rev`, `commit`, `rev_new`, `key`, `lease`, `replayed`, `dry`, `targets`, `statements`, `affected`, `markers`,
`view`, (`use`, `live`, `tree`, `parts`, `reads` when they apply), `cols`, `data`, `next`, `dropped`, `notices`, `warnings`,
`budget`. This chapter reads them as follows and appends one key (open point 3):

| Key | Content in an API result |
|---|---|
| `branch` | the branch the block was committed on: the `ON` ref, else the caller's resolved branch (§4.2) |
| `rev` | the seq of that branch's tip before the command; 0 when the branch had no commit |
| `commit` | the id of the commit the command created. When it created none (a `DRY`, a lease-only command such as `Claim` without `start`): the tip it read, or null when the branch has no commit. On a replay: the original commit, or null when the original result recorded none |
| `rev_new` | the seq of the created commit; null when none; on a replay, the original commit's seq |
| `key` | the explicit idempotency key (`ctx.key`), else null |
| `lease` | the presented lease (§4.2), else null |
| `statements` | [LQ/envelope §7.7]; for a named mutation, the statements of its expansion; for a data-level block, those of its LQ equivalent (§9.3); `targets` lists the nodes the statement matched, created or changed, ascending; empty on a replay and for a file named mutation, which is no `TX` block ([LQ/std §7.4]) |
| `affected` | `{"ready":[…],"other":[…]}`: `ready` lists the newly ready ids (§5.8); `other` the rest of the commit's `affected` set ([F13 §6.3]); both ascending by `#N` |
| `markers` | every marker the command emitted, in the order of §10.8 |
| `cols`, `data` | the columns and rows of the created commit's `diff` (§5.7); empty with no commit |
| `budget` | [LQ/envelope §7.5]; its values are excluded from the comparison (§16.3) |
| `yields` (appended after `budget`) | array, always present, empty when the block calls no procedure: one object per procedure call of the block, in statement order: `{"index":<statement index>,"proc":"tx.<name>","rows":[{…}…]}`; each row holds the procedure's yield columns of [LQ/std §7.3] first, in that table's order, then the further members the command's section lists (§10, §12), in the encodings of §5 (open point 36). A file named mutation (§12), which is no `TX` statement, has one entry with `index` 0 |

### 3.4 Family W

| # | Key | Type | Present | Content |
|---|---|---|---|---|
| 1 | `v` | int | always | `1` |
| 2 | `branch` | string or null | always | the ref the command acted on, named in its section; null for a command that acts on no ref |
| 3 | `rev` | int or null | always | that ref's tip seq before the command; 0 when it had no commit; null with `branch` null |
| 4 | `commit` | string or null | always | the last commit the command created; null when none |
| 5 | `rev_new` | int or null | always | that commit's seq |
| 6 | `key` | string or null | always | the explicit idempotency key, else null |
| 7 | `replayed` | bool | always | §7.5 |
| 8 | `dry` | bool | always | the command ran with `ctx.dry` |
| 9 | `data` | object | success envelope | the command's result object, in the key order its section lists |
| 10 | `next` | null | success envelope | |
| 11 | `dropped` | null | success envelope | |
| 12 | `warnings` | array | success envelope | [F19 §9.2] objects, in the order raised |

Then `hints`, `errors` and `exit` as [F19 §8.2] rows 9–11. Keys 4 to 8 are this family's leading keys and key 12 its trailing
key in the sense of [F19 §8.2] row 4 and row 8.

### 3.5 Family X

`{"v":1,"branch":<string or null>,"rev":<int or null>,"data":{…},"next":null,"dropped":null}`: `branch` and `rev` name the
view a snapshot was taken at, and are null for an environment command.

## 4. The caller context

### 4.1 `ctx`

`ctx` carries the raw inputs of [90 §4.1]'s resolver, exactly as a front end received them; the executor resolves them by
§4.2. Every key is optional.

| Key | Type | Models |
|---|---|---|
| `door` | `"cli"`, `"mcp"`, `"hook"`, `"apply"` | the front end (§1.4); default `cli` |
| `branch` | ref name | explicit `--branch` / MCP `branch` |
| `lease` | `"L-<n>"` | explicit `--lease` / MCP `lease` |
| `agent` | string | declared `--agent` / MCP `agent` |
| `client` | string | `--client` |
| `tree` | absolute path | explicit `--tree` / MCP `tree` |
| `model` | string | `--model` |
| `cwd` | absolute path | the process working directory |
| `env` | object of strings | the process environment; only these names are read: `MOIRAI_LEASE`, `MOIRAI_BRANCH`, `MOIRAI_AGENT`, `MOIRAI_CLIENT`, `MOIRAI_RUN`, `MOIRAI_ROLE`, `MOIRAI_MODEL`, `CODEX_THREAD_ID`, `CLAUDE_CODE_SESSION_ID`, `CLAUDECODE`, `AI_AGENT`, `GEMINI_CLI`, `CURSOR_AGENT`, `AGENT` |
| `meta` | object: `threadId`, `sessionId`, `sandboxCwd` | Codex `_meta` of an MCP call |
| `stamp` | object: `session_id`, `agent_id`, `agent_type`, `cwd` | the Claude Code stamp ([AR §7.2]) |
| `marker` | string | the dispatch marker `moirai:task=#N lease=L-n branch=<ref> role=<r>` ([AR §2.9]) |
| `client_info` | string | the MCP `clientInfo` name |
| `hook_label` | string | a hook-attested role label (Claude `agent_type`, the `PostToolUse(Agent)` map, Codex `SubagentStart.agent_type`) |
| `hook_model` | string | a hook's `model` field |
| `key` | string | `--idempotency-key` / MCP `idempotency_key` / `TX … KEY` |
| `no_dedupe` | bool | `--no-dedupe`: no default key (§7.2) |
| `dry` | bool | `--dry-run` / MCP `dry_run` / `DRY` |
| `if_tip` | commit | `--if-tip` / MCP `if_tip` / `IF TIP` |

A path is the canonical absolute form of [80 §2.10] P12 with `/` separators ([OS/path §2.2]). In a stream, paths name the
simulated trees of §6.5.

### 4.2 Resolution

Each field group takes the first source that has it, in the order of record of [90 §4.1] ([AR §5a.4], [AR §7.2]). The
reference model encodes these rows as data ([60 §3.13] GT2 row, M8); the ids are this chapter's.

| Row | Field group | Order (first source that has it) |
|---|---|---|
| CX-1 | **Rights** (the presented lease) | `ctx.lease` → `ctx.env.MOIRAI_LEASE` under the binding rule CX-9 → none |
| CX-2 | **Branch** | `ctx.branch` → the presented lease's branch, for a task lease or a run-scoped role lease (the session role lease carries no branch, [RULES/role-write-policy] WR-005, open point 40) → the binding (longest bound prefix, [F11 §5]) of `ctx.meta.sandboxCwd`, else of `ctx.stamp.cwd` → `ctx.env.MOIRAI_BRANCH` → the `branch=` field of `ctx.marker` → the client head of `ctx.client`, else of `ctx.env.MOIRAI_CLIENT` → the binding of `ctx.cwd` → the binding of the git top-level that contains `ctx.cwd` (the git-worktree hint, §6.5) → for `door` = `mcp`, the session's client head `session:<harness>:<id>` → the `default-branch` key ([CFG]) |
| CX-3 | **Actor** and `actor_src` | the presented lease's holder (`lease`) → `codex:` + `ctx.meta.threadId` (`meta`) → `claude:` + `ctx.stamp.agent_id` (`stamp`) → `ctx.agent` as written (`declared`) → `ctx.env.MOIRAI_AGENT` as written, else `codex:` + `CODEX_THREAD_ID`, else `session:claude:` + `CLAUDE_CODE_SESSION_ID` (`env`) → `client:` + `ctx.client_info` (`client`) → the resolved session identity prefixed `session:`, else the empty string (`none`) ([F06 §3.5]) |
| CX-4 | **Session** (the liveness identity) | `codex:` + `ctx.meta.threadId` → `ctx.stamp.session_id` with the prefix of the detected harness (CX-7) → `codex:` + `CODEX_THREAD_ID` → `claude:` + `CLAUDE_CODE_SESSION_ID` → none; never a `MOIRAI_*` variable |
| CX-5 | **Tree** | `ctx.tree` → `ctx.meta.sandboxCwd` → `ctx.stamp.cwd` → the `worktree_path` of the lane whose `moirai_branch` is the presented lease's branch (a task lease or a run-scoped role lease) → `ctx.cwd` |
| CX-6 | **Model** | the `model` of the run the presented lease is scoped to, else of the run `ctx.env.MOIRAI_RUN` names → the `model=` field of `ctx.marker` → `ctx.model`, else `ctx.env.MOIRAI_MODEL` → `ctx.hook_model` → `lq.model-profile.default.<client>` ([90 §8.2], [CFG]) |
| CX-7 | **Client** (profile) | `ctx.client`, else `ctx.env.MOIRAI_CLIENT` → `ctx.client_info` (`codex-mcp-client` → `codex`; a Claude Code name → `claude`; anything else → `generic`) → detection from `ctx.env`: the variables of exactly one harness (`CLAUDECODE`, `CLAUDE_CODE_SESSION_ID` or `AI_AGENT` = `claude-code_*` → `claude`; `CODEX_THREAD_ID` → `codex`; `GEMINI_CLI`, `CURSOR_AGENT`, `AGENT` → their label with the `generic` profile); variables of more than one harness → `generic` and **no session identity** (CX-4 yields none) and the warning `two_harnesses` ([F19 §10.4]) → `generic` |
| CX-8 | **Effective role** | the presented lease's role; a `ctx.hook_label` that differs narrows it to the intersection of the two rows of [RULES/role-write-policy], with the warning `hook_label_narrowed`; no lease → `general-purpose` ([90 §4.3]) |
| CX-9 | **Binding rule** | an environment lease binds, at its first use, to the attested thread that uses it (`codex:` + `ctx.meta.threadId`, else `codex:` + `CODEX_THREAD_ID`): the `LEASES.bound` field is set by a `Lease` record of event 3 with mask bit 2 ([F05 §9.4]) in the command's group. A later use of the same lease through the environment from another attested thread is refused (E407, [F19 §11.2] text 2). An explicit `ctx.lease` is never refused on this ground ([90 §4.1]) |

Notes:
- The session's client head in CX-2 is [AR §5a.4]'s "unresolved reads use the session's checkout", placed before the
  `default-branch` key and used by `door` = `mcp` only (open point 6).
- CX-2's git-worktree hint needs a simulated tree with a git history (§6.5, §6.6); without one the step yields nothing.
- A `ctx.marker` that does not match the marker grammar ([AR §2.9]) contributes nothing.

### 4.3 Refusals of the resolution

Checked before anything else of the command, in this order; each refuses with nothing written:

| # | Condition | Refusal |
|---|---|---|
| 1 | `ctx.lease` or the environment lease names no live lease ([RULES/state-definition] `lease-live`, `lease-ends`) | E407 `lease`, exit 5 ("lease lost") |
| 2 | CX-9: an environment lease bound to another thread | E407, [F19 §11.2] text 2 |
| 3 | the presented lease is a task lease or a run-scoped role lease, `ctx.branch` differs from its branch, and the command has no `move_lease` argument set to that branch ([AR §5a.4], WR-005; the session role lease writes on any branch) | E407, exit 5 |
| 4 | outside `Claim`, `ctx.agent` differs from the presented lease's holder ([90 §4.1] Actor row) | E407, [F19 §11.2] text 1 |
| 5 | a tree-derived write (group F, except `FileAdd` and `LinkFile` in an eligible tree) with `ctx.tree` outside the presented lease's lane tree | `tree_mismatch`, exit 5 |
| 6 | the resolved branch is a detached client head and the command writes, without `branch_new` ([AR §5a.4]) | E305 `read_only_view`, exit 6 |
| 7 | the resolved branch is a ref the command may not write: a tag, an `import/*` ref, a commit or reflog revision; a staging ref for anything but `RESOLVE` ([50 §3.9] item 6); a `plan/*` ref for a masked field (I33′, [RULES/status-machines] BM-002) | E305, exit 6 |
| 8 | quiet mode refuses the command ([AR §6.6]) | `quiet_mode`, exit 6 |

A move with `move_lease` succeeds with the warning the verb prints ([AR §5a.4]) and a `Lease` record of event 3, mask bit 1
([F05 §9.4]).

### 4.4 What the resolved caller sets in a commit

| Commit header field ([F06 §4.3]) | Value |
|---|---|
| `actor`, `actor_src` | CX-3 |
| `role` | the presented lease's role; the empty string with no lease |
| `session` | CX-4, the namespaced identity; empty with none |
| `ref`, `ref_id` | the branch the commit lands on |
| `git` group | from the resolved tree's simulated git state (§6.6): `algo`, `head` (the tree's HEAD commit), `branch` (the short name of its symbolic HEAD; empty when detached), `worktree` (the tree's display label of [F19 §4.3]), `base` (the base of the tree's designated binding, [F18 §3.2]); absent when the tree has no git (open point 7) |
| `stmt_origin`, `stmt_sym`, `stmt_hash` | the command's section, from `ctx.door` |
| `idem` pair | §7 |

## 5. Values in JSON

### 5.1 Identifiers and scalars

| Value | JSON |
|---|---|
| node (`#N`) | string `"#N"`, N decimal without leading zeros |
| uid | string `"#u:"` followed by 32 lower-case hexadecimal digits ([50 §4.4]) |
| commit id | string `"c"` followed by 64 lower-case hexadecimal digits ([F12 §3.8]); in arguments also a commit literal of 7 to 64 digits, resolved by [F12 §3.2] and §3.4 (a prefix that names no commit, or several, is E301, exit 3, with the candidates in the error's `candidates` key, [LQ/errors §5.7]) (open point 1) |
| revision in an argument | a ref name or a revspec of [F12] |
| sequence number | integer |
| `hlc`, `append_hlc` | string of the decimal digits of the `u64` ([F19 §8.3]: values that can exceed 2^53 are strings) |
| ref | its name, as [F12] spells it |
| lease | `"L-<lease_id>"` ([F19 §2.4]) |
| anchor handle | `"a<n>"` |
| file intent | `"i-<n>"` |
| 16- or 32-byte digest | 32 or 64 lower-case hexadecimal digits |
| git object id | `"<algo>:<hex>"`, `algo` `sha1` or `sha256` ([F01 §7.5]) |
| timestamp | RFC 3339 UTC with milliseconds (`2026-09-25T12:03:00.000Z`) |
| duration | integer milliseconds; an argument that takes a duration also accepts the LQ duration text (`"15m"`, [LQ/lexical]) |
| deadline (`Stamp`, [OS/clock §3.1]) | `{"wall":<timestamp>,"boot":<the boot_hash or null>,"boot_ns":<string or null>}`, null when the stamp's `boot_hash` is 0; `Stamp::NEVER` is the string `"never"` |
| a `u64` hash value (`boot_hash`, `span_hash`) | 16 lower-case hexadecimal digits of the value, most significant first |
| integer | JSON integer when its magnitude is below 2^53, else a string of decimal digits with an optional `-` |

### 5.2 Typed values

The closed value set of [F06 §5.1] and [F08 §5.1]. Tag numbers never appear in JSON: a value's type is its field's.

| Type | In arguments and results | In `state(ref)` (§15) |
|---|---|---|
| bool | `true`, `false` | same |
| int | integer (§5.1) | same |
| counter | integer: the total; an increment carries a delta (§9.2 `incr`) | the total |
| f64 | JSON number in the shortest round-trip form with a `.` or an exponent ([LQ/envelope §5.2]); NaN and infinities are refused (`bad_value`), −0.0 is +0.0 ([F06 §5.2]) | same |
| enum | the value's name; for `priority` also `"P0"`–`"P4"` and 0–4 in arguments ([50 §3.2]); results give `priority` as an integer | name; `priority` integer |
| text (both stored forms, [F08 §5.1]) | string | same |
| set | array; in arguments any order, a duplicate element is `bad_value`; in results in the order of §5.5 | array in §5.5's order |
| ref | `"#N"` | the uid, `"#u:…"` |
| commit-ref | `"c…"`, 64 digits | same |
| path | `"<root name>:<path text>"`, split at the first `:` (root names have no `:`, [F08 §5.4.1]) | same |
| oid | `"<algo>:<hex>"` | same |
| pathmove | `{"hlc":<string>,"class":<name>,"from":<path>,"to":<path>,"git":<git id or null>}` | same |
| absent | `null` | the field is omitted |

A field whose schema row has `coerce` = `timestamp` ([F08 §8.4.4]) is an `int` of Unix seconds; an argument may give it as an
ISO 8601 string, which [50 §3.2]'s coercion converts; results give the integer. Record lists ([F08 §5.4.5]) are `text`.

### 5.3 Keys

The text form of a conflict or violation key is [F12 §6.6]'s `skey`, in arguments and in results; results write its output
form (the `#`, stored names, `-` for a violation without a key). For reference (open point 8):

| Key class ([F06 §6.1]) | Text ([F12 §6.6]) |
|---|---|
| existence | `#N.existence` |
| status (with its resolution) | `#N.status` |
| field, counter | `#N.<field>` |
| observation | `#N.observation` |
| body | `#N.body` |
| hierarchy | `#N.parent` |
| edge | `edge:#S:<stored edge kind>:#D`, and `:a<n>` (the anchor handle) for an `at` edge |
| schema item | `schema:kind:<kind>`, `schema:field:<kind or *>.<field>`, `schema:enum:<kind or *>.<field>.<value>`, `schema:edge:<edge kind>` |
| named query | `query:<name>` |

**Snapshot form.** `state(ref)` (§15) holds no store-local number, so its keys replace every `#N` by the node's uid
(`#u:<32 hex>`) and an anchor handle by the anchor's uid as 32 hexadecimal digits: `#u:….body`,
`edge:#u:…:at:#u:…:<32 hex>`. This form is this chapter's and is never input.

### 5.4 Edges and anchors

- An edge in a result: `{"src":"#N","kind":<stored kind>,"dst":"#N"}`, plus `"anchor":"a<n>"` and `"anchor_kind":<name>` for an
  `at` edge and `"flagged":true` for a flagged edge. Family T and R rows follow [LQ/envelope §7.3] instead, which names the kind
  by its LQ name under `type`.
- An anchor record, where a result or a snapshot carries it: the members of [F08 §10.3] by name — `uid`, `kind`, `mode`,
  `watch`, `resolver`, `captured`, `pred`, `hint` (`[first, last]`), `scope` (`{"lang":<name>,"segments":[{"kind":<name>,
  "name":<text>,"qual":<text>}…]}`), `quote_h`, `prefix_h`, `suffix_h`, `end_h`, `occurrence`, `window` (hex), `span_hash`
  (§5.1), `blob`, `git`, `marker` — with absent members omitted; enumeration members
  by name; and the texts `quote`, `prefix`, `suffix`, `end` where §15 or the command says so.
- A **link object** ([40 §6.1], [F19 §8.8]), one per link a result shows a state for: `{"node":"#N","anchor":<"a<n>" or null>,
  "file":"#N","path":<path>,"now":<path or null>,"kind":<anchor kind or "file">,"state":…,"detail":…,"parts":[…],"score":…,
  "resolver":<int>,"next":<text>}`. `state`, `detail`, `parts`, `score` and `resolver` are [F19 §8.8]'s with the strings of
  [F18 §4]; `node`, `anchor`, `file`, `path` and `next` are the `links` relation's columns ([LQ/std §2.8] item 2); `now` is the
  path the file resolved to when it differs from `path`, else null.

### 5.5 Order

Arrays whose order the command's section does not fix are ordered by these keys, ascending ([F01 §6.6] for bytes):

| Array of | Order |
|---|---|
| node ids | `#N` numerically |
| uids, digests, hex | bytewise |
| texts, names, ref names | bytewise UTF-8 |
| set elements (results) | `int` numerically; `enum` by the value's `sort_rank` then name; `text` bytewise; `ref` by `#N` (by uid in `state(ref)`); `commit-ref` bytewise; `path` by (root name, text); `oid` by (algo name, digest); `pathmove` by (`hlc`, `from`, `to`, `class`, `git`) ([F08 §5.5] with names for ids) |
| edges | (src, stored kind, dst, discriminator), nodes as above |
| markers | (`#N`, ref name, commit, kind) |
| leases | (`#N`, lease id) with role leases (`#N` = 0) first ([F11 §6]) |

### 5.6 Canonical JSON

The **canonical JSON** (CJ) of a value is its JSON text written by these rules, used by the digests of §15, the payload hash of
§7.3 and the example goldens:

1. UTF-8 without a byte-order mark; no whitespace outside strings.
2. Object members in the order the owning table lists them; a member marked "omitted when absent" is omitted when absent, every
   other member is written, `null` included. A map whose keys are data (a node's `fields`, `args` of §7.3) has its members in
   bytewise order of their keys.
3. Strings escape `"`, `\` and U+0000–U+001F exactly as [F19 §8.3] states; nothing else is escaped.
4. Integers as §5.1; f64 as §5.2; `true`, `false`, `null`.
5. Arrays in the order the owning table or §5.5 fixes.

### 5.7 Diff rows

The `data` of a family-T result holds the rows of the created commit's net changeset against its first parent ([AR §4.6]
"Net changeset = state diff"; for `sync`, the full state diff), in the shape of the `diff` relation of [LQ/std §2.9]: columns
`change`, `node`, `kind`, `aspect`, `name`, `before`, `after`, `side`, `last_commit`, `actor`. For a commit's own rows `side` is
null, `last_commit` is the commit and `actor` its actor; for the would-be rows of a `DRY` both are null. `before` and `after`
are the key's canonical values ([F07 §6.3]): an absent key is null, and so is a value equal to its field's default (`priority`
`P2`, the initial status with resolution `none`), so the first move away from a default is a `+` row. This chapter fixes the
aspects and values (open point 4):

| Key class | `aspect` | `name` | `before`, `after` |
|---|---|---|---|
| existence | `existence` | the kind | `"live"`, `"deleted"` or null |
| status | `status` | `status` | `{"status":<name>,"resolution":<name>}` or null |
| field | `field` | the field | §5.2, or null |
| counter | `counter` | the field | the totals |
| observation | `observation` | `observation` | `{"path":…,"oid":…,"bytes":…,"observed_git":…,"observed_blob":…,"relink":…}`, absent members null |
| body | `body` | `body` | the 32-hex body hash, or null |
| hierarchy | `parent` | `parent` | `{"parent":"#N" or null,"order":<text or null>}` |
| edge | `edge` | the LQ name of the stored kind | `{"dst":"#N","disc":<32 hex or null>,"props":{…}}` or null; `props` holds `flagged`, `pinned` and `anchor` as present; `anchor` is an object whose first member is `"handle":"a<n>"`, followed by the anchor record of §5.4 with its texts |
| schema item | `schema` | the key text (§5.3) | the item as an object of [F08 §8.5]'s members by name, or null |

- A key that holds a conflict value shows `{"conflict":<class>,"base":…,"ours":…,"theirs":…}` in `before` or `after`.
- `change` is `+` when `before` is null, `-` when `after` is null, `~` otherwise. `node` is null for schema rows.
- Rows are in the order of the `diff` query ([LQ/std §4.15]: `ORDER BY node, aspect, name`) — `node` by `#N`, schema rows
  (`node` null) last because absent sorts last ([50 §3.5]), `aspect` and `name` bytewise — and, among edge rows that agree on
  all three, by the edge's `dst` and then its discriminator (absent first). The aspect words are the key parts of
  [F12 §6.6] where one exists.

### 5.8 Newly ready

`newly_ready(c)` of a command that created commit c on branch B is the set of task ids that are `ready` for the caller on B
after the command and were not before it ([RULES/state-definition] PD-010 to PD-017; the runtime clauses at the injected clock
of §6.2). It is tip-only runtime data and never stored ([F13 §6.3]).

## 6. The injected environment

### 6.1 What is injected

Every source of variation an implementation would otherwise take from the machine is part of the stream (DT-3):

| Source | State | Command | § |
|---|---|---|---|
| wall clock, boot clock, boot identity | `wall_ms`, `boot`, `boot_ns` | `EnvClock` | §6.2 |
| lock-anchored liveness of sessions | the slot table | `EnvSlots` | §6.3 |
| randomness | the seed of `Init` | — | §6.4, §17.3 |
| project files, file ids, creation times, volume capabilities | simulated trees | `EnvTree` | §6.5 |
| git history | abstract repositories | `EnvGit` | §6.6 |
| crashes | — | `EnvCrash` | §6.7 |

In a production process the OS supplies every one of them ([OS/clock], [OS/proc], [OS/project]); only the test harness and the
model use this section. Environment commands take no `ctx`, are never keyed and append nothing to the store.

### 6.2 The clock: `EnvClock`

**State.** `wall_ms` (i64, milliseconds since the Unix epoch), `boot` (`Known(k)` with a boot number k ≥ 1, or `Unknown`) and
`boot_ns` (u64). At the start of a stream: `wall_ms` = 1,790,000,000,000, `boot` = `Known(1)`, `boot_ns` = 1,000,000,000.

**Arguments**, applied in this order:

| Argument | Type | Effect |
|---|---|---|
| `set_wall_ms` | int | `wall_ms` := the value: a wall-clock step; `boot_ns` unchanged |
| `step_ms` | int, signed | `wall_ms` += the value: a wall-clock step of fault-model item (7) ([F15 §3.7]); `boot_ns` unchanged |
| `advance_ms` | int ≥ 0 | elapsed time, suspend included: `wall_ms` += the value; `boot_ns` += the value × 10^6 when `boot` is `Known` |
| `reboot` | bool | `k` := `k` + 1, `boot` := `Known(k)`, `boot_ns` := 1,000,000,000; every simulated process ends, so the slot table is emptied (§6.3) |
| `boot_mode` | `"known"` or `"unknown"` | every later command runs in Unknown-boot mode ([OS/proc §5]) until `"known"` restores `Known(k)` |

**Rules.**
- **CK-1 (constant within a command).** Every clock read an implementation makes while it executes one command returns the same
  values. Only an `EnvClock` command changes them. The monotonic clock of in-process intervals ([OS/clock §2]) is not
  observable in a result; an implementation may derive it from `boot_ns`.
- **CK-2 (boot identity).** The simulated `boot_id` of boot k is
  `BLAKE3-128( lp("moirai-boot-id-v1") ‖ lp("api-sim-boot") ‖ lp(u64le(k)) )`, the framing of [OS/proc §4.2] with the source
  name `api-sim-boot`; `boot_hash` is [OS/proc §4.3]'s. In Unknown-boot mode both are unknown and every stamp carries
  `boot_hash` = 0 ([OS/proc §5] U4).
- **CK-3 (now).** The stamp `now` is [OS/clock §3.2]'s over (`wall_ms`, `boot`, `boot_ns`). Deadlines, their evaluation and
  the half-TTL renewal are [OS/clock §4.2]–§4.4's over it.
- **CK-4 (one store HLC).** The HLC values a command's records carry are drawn from one sequence, in the order the command
  appends the records ([F16]; a group's records in [F05 §4.7]'s order): each record takes `hlc_next(wall_ms, h)` of
  [OS/clock §7], where h is the greatest value the sequence has produced before it (0 in a new store) and, for a local
  commit, also the greatest `hlc` of any commit the store holds (an imported commit's `hlc` can lie ahead). The records that
  take a value are the **semantic durable records** a command of this API writes: `Commit` (a local commit's `hlc`, which is
  also its `append_hlc`, [F06 §4.4.4]; an imported commit keeps its own `hlc` and takes only its `append_hlc`),
  `RefUpdate`, `ClientHead`, `Lease`, `Marker` (one value per record, carried by each entry), `Idem`, `Backup`, `FsIntent`,
  `FsIntentDone` and `FsIntentAborted` ([F05 §9]). A record of another kind that has an HLC field — `Checkpoint`, `Lazy`,
  `SessionMark`, the lazy runtime rows — carries `hlc_next(wall_ms, h)` for the current h but does not advance the sequence,
  and no result, snapshot or digest of this chapter shows its value. So class-I maintenance changes no HLC and no commit id
  ([F17 §1.5] SP-1), and a lazy record lost in a crash changes none either. This departs from [F16] P-36, which advances
  one HLC over every "HLC at append" field of [F05 §9] (open point 39).
- **CK-5 (`pathmove.hlc`).** A `path_moves` entry that a command adds carries the `hlc` of the first commit the command
  appends. This is the value "the writer's HLC when the candidate was computed" of [F06 §5.5] under a clock that is constant
  within the command; a re-parent never changes it.
- **CK-6 (windows).** A retention window measures `now_ms − (t >> 16)`, with t the HLC value of the record that opens it
  (a commit's `append_hlc`, a record's `hlc`) and `now_ms` = max(`wall_ms`, h >> 16), h the greatest value of CK-4's sequence
  and of any commit's `hlc` ([OS/clock §6], [F17 §1.6]).
- **CK-7 (wall now).** `defer_until ≤ now()` and the other wall-clock tests of `ready` use `floor(wall_ms / 1000)` seconds
  ([RULES/state-definition] PD-016).

**Result** (family X): `data` = `{"wall":<timestamp>,"wall_ms":<int>,"boot":"known"|"unknown","boot_no":<k>,"boot_hash":<16 hex
or null>,"boot_ns":<string>}`.

### 6.3 Liveness slots: `EnvSlots`

**State.** A table of at most 256 held slots, each `{"identity":<namespaced session>,"alias":<namespaced session or null>}`,
and a flag `readable` (true at the start of a stream). A held slot models the MCP server of that session holding its
liveness-slot byte and a valid slot record ([F03], [90 §4.4]). Identities are `claude:<id>` or `codex:<id>`.

**Arguments**, applied in this order: `release` (array of identities), `hold` (array of identities), `alias` (array of
`[primary, alias]` pairs: Claude Code's `/clear`), `readable` (bool: false models a `LOCK` the process cannot read, for example
another principal or a sandbox).

**Rules.**
- **SL-1 (anchor kind at a claim).** A lease a caller takes gets the anchor kind `session` when the caller's session (CX-4)
  has the prefix `claude:` and holds a slot, `session-ttl` when it has the prefix `codex:` and holds a slot, and `none`
  otherwise ([90 §4.4]; "a lease of a thread whose own server holds no slot gets anchor `none`").
- **SL-2 (liveness).** An anchor is judged by [OS/proc §6.2] over this table: step B with the current boot (§6.2), step C with
  `readable`, step D matching the anchor's session hash against the primary identities of held slots (never the alias).
  Lease liveness is [RULES/state-definition] `lease-live` over the result.
- **SL-3 (intent slots).** A `FileMv`, `FileRm` or `FileRevert` command holds its own intent slot from before its intent to
  its end ([AR §5e.5]); the slot is released when the command ends, unless `EnvCrash` ends it first (§6.7).
- **SL-4 (capacity).** A `hold` beyond 256 slots takes no slot; the result lists it under `unheld`, and its session's leases
  get anchor `none` (SL-1).
- A reboot (§6.2) empties the table.

**Result** (family X): `data` = `{"held":[<identity>…],"unheld":[<identity>…],"readable":<bool>}`, identities in bytewise order.

### 6.4 Entropy

The `seed` argument of `Init` (§8.1) is the stream's entropy. The store id (§17.3) and every random uid (§17.4) derive from it;
nothing else visible is random. Engine-internal random values (the epoch, the `tmp/` nonces, [F02 §5.3], [F04]) are never
compared.

### 6.5 Simulated project trees: `EnvTree`

Visibility: at M0 the reference model's exact-evidence resolution over a simulated tree ([60 §4.2] row "File links", WP-92);
from M6 the engine's `ProjectFs` simulator ([PLAN §2.3] `moirai-projfs-sim`). The input format is fixed here; the resolver's
use of it is [F20]'s and [40 §4]'s.

**State.** A set of trees, each with:

| Member | Content |
|---|---|
| `root` | the tree's canonical top level (an absolute path, §4.1); a path belongs to the tree whose `root` is its longest prefix |
| `volume` | a volume name; two trees on one volume share file-id space and may rename into each other |
| `caps` | the `VolumeCaps` of [F11 §12.3] by member name; default: the NTFS row of [OS/project] |
| `files` | root-relative path → `{"id":<u64>,"bytes":<base64>,"btime_ns":<string>,"mtime_ns":<string>,"attrs":[<name>…]}` |
| `dirs` | root-relative directory path → `{"id":<u64>}` |

**Arguments.** `tree` (the root; created on first use with `volume` and `caps` from the arguments), then `ops`, an array applied
in order:

| Op | Members | Effect |
|---|---|---|
| `write` | `path`, `bytes` (base64) or `text` (UTF-8), optional `btime_ns` | creates the file with the next file id of its volume and `btime_ns` = `mtime_ns` = `wall_ms` × 10^6 (§6.2); an existing file keeps its id and `btime_ns` and gets the new bytes and `mtime_ns` |
| `mv` | `from`, `to` | a rename: id, `btime_ns` and bytes move with the file; parents are created; `to` must not exist |
| `cp` | `from`, `to`, optional `keep_btime` | a new file with a new id and the same bytes; `btime_ns` is now, or the source's with `keep_btime` |
| `rm` | `path` | removes a file, or a directory with everything below it |
| `mkdir` | `path` | creates a directory with the next id |
| `attrs` | `path`, `set`, `clear` | sets or clears attribute names (`cloud-only`, `read-only`, …, [F11 §12.2]) |
| `deny` | `path`, `on` | reads of the path fail with access denied while `on` |

**Result** (family X): `data` = `{"tree":<root>,"files":<count>,"dirs":<count>}`.

### 6.6 Abstract git histories: `EnvGit`

Git history is model input as abstract data ([60 §4.2] row "File links", the A1 re-review A-M4, [PLAN §6.2] R16): commits
with parents, committer times and `path → blob id` maps, one HEAD per simulated tree. The M4 git object layer reads real
objects in the engine; the harness writes a repository that realises the same abstract history.

**Arguments.**

| Argument | Content |
|---|---|
| `repo` | a repository name; created on first use with `algo` (`sha1` or `sha256`) |
| `commits` | array of `{"id":<git id>,"parents":[<git id>…],"committer_time":<int seconds>,"author_time":<int seconds>,"tree":{<path>:<git blob id>…}}`; a commit is added once and never changes |
| `refs` | object `refs/heads/<name>` → git id (set) or null (deleted) |
| `heads` | object tree root → `{"ref":"refs/heads/<name>"}` or `{"detached":<git id>}`; binds the tree to `repo` |

A tree bound to a repository is a git worktree for §4.2 CX-2, §4.4's `git` group, [F18 §3.6]'s writer-tree predicate and the
tree gate of [40 §5.2].

**Result** (family X): `data` = `{"repo":<name>,"commits":<count>}`.

### 6.7 Crashes: `EnvCrash`

**Arguments.** `at`: `"between"` or `"in-next"`.

- **`between`.** Every simulated process ends: the slot table is emptied (their sessions' `session` and `session-ttl`
  anchors become Dead by SL-2), and the next store command first runs recovery ([F16]). Nothing logical changes: the model's
  state is unchanged, and the engine must equal it after recovery.
- **`in-next`.** The next write command is interrupted at a point the engine's harness chooses. Its result is the error
  envelope with `outcome_unknown` (exit 7, [F19 §10.2]). After recovery the store equals either the state with that command
  applied or the state without it ([60 §4.4] item 4). The model keeps both candidates and adopts the one the engine's next
  `State` and `Runtime` snapshots show; an acknowledged commit is never missing, and a retry with the same key converges
  (I14′). A write that was not applied leaves no marker, lease or idempotency entry (I27′).

**Result** (family X): `data` = `{"at":<value>}`.

## 7. Idempotency

### 7.1 Which commands are keyed

- **Keyed**: every write command of groups G, C, V, F and I, with the exceptions below. A keyed command takes the explicit key
  `ctx.key`; without one it takes the default key of §7.2, unless `ctx.no_dedupe` is true ([AR §6.4]).
- **Explicit key only** (no default key): `LinksSync`, a settle, which must run again when the tree changed (open point 10).
- **Never keyed**: groups E, S and O; `Heartbeat` and `Check`, which append only lazy records, which cannot carry the durable
  `Idem` record, and whose repetition is harmless. A `ctx.key` on them is ignored and nothing is recorded.

### 7.2 The key hash

`idem_key` is [F06 §4.4.7]'s:
- an explicit key k: `BLAKE3-128( lp("moirai-idem-key-v1") ‖ lp(k) )`;
- the default key: `BLAKE3-128( lp("moirai-idem-default-v1") ‖ lp(s) ‖ lp(a) ‖ lp(P) )`, where s is the resolved session (CX-4,
  namespaced; empty with none), a is the attested thread or agent (`codex:` + `ctx.meta.threadId`, else `claude:` +
  `ctx.stamp.agent_id`) and otherwise the resolved actor (CX-3), and P is the payload of §7.3 (16 bytes).

### 7.3 The payload

- **A command whose core is one `TX` block** — family T, `Apply` (§9.4) — has `idem_payload` = `H` of that block's root
  ([LQ/canonical-ast §7.2], the entry forms R3–R5 of [LQ/canonical-ast §5.9]): `Tx` by its block (a data-level block by its
  LQ equivalent, §9.3); `Mutation` with `ctx.door` = `cli` by R5 (the verb's expansion); with `ctx.door` = `mcp` by R4
  (`TX { CALL tx.<name>(…) }`); `Claim`, `Release`, `Reclaim`, `Complete` and the file named mutations by the
  same rule as `Mutation`. `ON`, `KEY`, `LEASE` and `DRY` are outside `H` ([LQ/canonical-ast §5.2]).
- **Every other keyed command** has the payload this chapter defines, which [F06 §4.4.7] cites for "a verb that compiles to no
  `TX` block":

  ```
  payload(c) = BLAKE3-128( lp("moirai-api-payload-v1") ‖ lp(name) ‖ lp(CJ(args′)) )
  ```

  where `name` is the command name in ASCII and `args′` is the command's `args` with: every node id replaced by its uid
  (`"#u:…"`); every commit prefix replaced by the full id; every argument the caller omitted still omitted (no default is
  filled in, as [LQ/canonical-ast §5.8] N2 does); and the members in bytewise key order (CJ rule 2, §5.6).

### 7.4 Lookup

One lookup per keyed command, against the store after every earlier command ([AR §4.5] steps 2 and 6 collapse into one in a
sequential stream). Entries older than `idempotency.retention` (P28), or, for a default key, `idempotency.default-window`
(P29), are ignored ([F17 §11.1], CK-6). An orphaned commit never satisfies a lookup (I27′). The first matching row decides:

| # | The entry for `idem_key` | Outcome |
|---|---|---|
| 1 | none | the command executes |
| 2 | equal payload, same branch (`ref_id`) | `replayed` (§7.5), exit 0 |
| 3 | equal payload, another branch, the entry records a commit, and the caller's branch has absorbed it (`absorbed_caller[ref_id] ≥ ref_seq`, [F11 §8]), the entry's ref being live or deleted | `replayed`, exit 0 (N13e) |
| 4 | a different payload | E408 `idempotency_mismatch`, exit 9, with [LQ/errors §5.7]'s keys: `key` (the explicit key, or null for a default key) and `original` = `{"rev":<the original commit's seq>,"commit":<its id>,"ref":<its ref>}`, or null when the entry records no commit |
| 5 | equal payload, another branch, otherwise | E408, exit 9, with the same keys |

- The branch of a command is the branch it writes: the resolved branch (CX-2) for most commands; the destination for `Merge`,
  `MergeContinue` and `Sync`; the batch branch for `Apply` (§9.4).
- **What is recorded.** Only an outcome that appended records: `ok`, `staged`, and the `partial` units that committed. A
  refusal, a `DRY` and a replay record nothing, so a refused command can be retried with the same key ([50 §3.10] item 8).
- **Where.** When the command appends a commit, the idempotency pair goes into the header of the commit that carries the
  command's result ([F06 §4.4.7]): the only commit, or for a command of several commits the one its section names (the merge
  commit of a sync-first merge, the lane-node commit of `LaneOpen`, the completing commit of `Complete`). A command that appends
  no commit writes an `Idem` record in its group ([F05 §9.6]) whose `result` is §17.1's `IdemResult`.

### 7.5 A replay's result

- **Family T.** The keys of §3.3 with `replayed` = true, `branch` the command's branch, `rev` its current tip's seq, `commit`
  and `rev_new` the original commit's, `statements` empty, `data` the original commit's diff rows, `affected` = `{"ready":[],"other":<its affected set>}`,
  `markers` those of its group, `yields` rebuilt from its group's `Lease` records or its `IdemResult`.
- **Family W.** `replayed` = true, `commit` and `rev_new` the original's, and `data` rebuilt from the original commits, their
  group's records, or the `IdemResult` (§17.1). A replayed `staged` outcome keeps its `errors` and exit 6.
- The rebuilt result equals the original result's `data` except for members marked "not replayed" in the command's section.

## 8. Store and maintenance commands (group S)

Family W; none is keyed (§7.1). `branch` is null unless the section says otherwise.

### 8.1 `Init`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `seed` | int (u64) | required | the stream's entropy (§6.4) |
| `params` | array of `"key=value"` | `[]` | [CFG §7.6]'s `--set`: an init-fixed key goes into `InitParams` ([F17 §2.2] IP-5), any other store key into the initial configuration |
| `default_branch` | string | none | `--default-branch B`, the same as `default-branch=B` in `params` |
| `tree` | absolute path | none | the directory `init` runs in; when it is a simulated tree bound to no git repository, `main` gets its designated binding ([F18 §3.5] row `init`, [40 §5.1]) |

**Effect.** A new store: its store id (§17.3); `InitParams`; the configuration; the ref `main` (`ref_id` 0, kind `work`, no
commit); no commit; `next_id` = 1, `next_anchor` = 1, `fence` = 0, `commit_seq` = 0; schema version 1 with the core schema of
[F08 §9] and no schema item; every runtime table empty. The first commit that lands on `main` is the store's root commit
([F06 §3.3]). [F16] fixes the groups `init` writes and their order ([F04 §10], [F02 §5.5]).

**Refusals.** An unknown key, a user-scope key or an invalid value: `config_key` or `config_value`, exit 2 ([CFG §7.6]). A second
`Init` in one stream: `usage`, exit 2.

**Result.** `branch` = `"main"`, `rev` = 0; `data` = `{"store_id":<32 hex>,"main_ref_id":0,"schema_version":1,"init":{<init-fixed
key>:<int>…},"config":{<key>:<value>…}}`: `init` holds the three init-fixed values ([F17 §2.1]) in bytes or counts, always
all three (from `params` or the production values); `config` the store keys the command set, each value a string in its
canonical form ([CFG §4.1]); both objects have their members in bytewise order of the keys (§5.6 rule 2).

### 8.2 `ConfigSet`, `ConfigUnset`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `key` | string | required | a key instance of [CFG]'s registry |
| `value` | string | required for `ConfigSet` | the value in [CFG]'s syntax |
| `scope` | `"store"` or `"user"` | `"store"` | the file written; the user-scope file of a stream is simulated |

**Effect.** From the next command on, the key's effective value is the new one ([CFG]; a hot store key takes effect at its next
decision point, [F17 §1.4]). This is the route by which GT2 streams change hot keys between commands ([CFG] open point 11). The
model takes the effective values of class-V keys as its configuration snapshot ([CFG §9.4]).

**Refusals.** `config_key`, `config_value` (exit 2); an init-fixed key (exit 2, [F17 §2.2] IP-4).

**Result.** `data` = `{"key":…,"scope":…,"value":<new effective value>,"previous":<previous effective value>}`, both strings
in the key's canonical form ([CFG §4.1]); after `ConfigUnset` `value` is the value that applies without the entry (the
default, or the other scope's).

### 8.3 `Quiet`

Argument `on` (bool, required). Sets or clears `HEAD.flags.quiet` ([AR §6.6], [F04]). While it is set, the commands [AR §6.6]
names refuse without `force` (`quiet_mode`, exit 6): `Gc`, `ImageExport`, `ImageImport`, and `LinksSync` with `deep` or
`all`. Result `data` = `{"quiet":<bool>}`.

### 8.4 `Maintain`

Argument `op`: `"checkpoint"` (a delta checkpoint), `"runtime-fold"`, `"fold"` (a tiered fold), `"rollup"`, `"promote"`; with
`"promote"` also `ref` (a ref name). **Effect.** The named maintenance runs when its preconditions hold ([AR §4.9], [F17 §5]–§7);
otherwise nothing happens. It is class I: it changes no result, commit id, state digest or runtime snapshot ([F17 §1.5] SP-1).
The model executes nothing. Result `data` = `{"op":…,"ran":<bool>}`; `ran` is excluded from the comparison (§16.3).

### 8.5 `Gc`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `rollup` | bool | false | also run a rollup |
| `prune` | bool | false | `--prune` |
| `reflog_expire` | duration | `gc.reflog-expire` (P30) | overrides P30 for this run |
| `cruft_delay` | duration | `gc.cruft-delay` (P31) | overrides P31 for this run |
| `force` | bool | false | runs in quiet mode |

**Effect.** [AR §4.9], [F17 §11]. Visible only through reachability ([F17] OP-17-17, confirmed here: `Gc` is a command of GT2
streams): the commits outside `gc::reachable_after_gc(dag, refs, reflog, pins, now, reflog_expire, cruft_delay)` whose
`append_hlc` is older than the cruft delay (CK-6) stop resolving, so `Undo`, reflog revisions and as-of views cannot reach them
(E301). It also drops `REFS` rows of expired deleted refs ([F11 §3.8]), `MARKERS_OLD` rows older than P30, expired `IDEM`
entries, trash and `FILEOBS` rows (class I). **Refusal.** Quiet mode without `force`. **Result** `data` =
`{"reachable":<commits>,"dropped":<commits>}`: the counts of commits in the reachable set and of commits that stopped
resolving.

### 8.6 `Backup`, `Restore`, `Repair`, `Verify`

| Command | Arguments | Effect | Result `data` |
|---|---|---|---|
| `Backup` | `dir` (absolute path), `force` (bool) | a transaction-consistent copy of the published store ([AR §4.10]); a `Backup` record ([F05 §9.13]). The model keeps the complete logical store (§2.4) under `dir`. Refused with `placement_refused` (exit 6) when `dir` exists and `force` is false | `{"dir":…,"commit_seq":<int>}` |
| `Restore` | `dir`, `into` (absolute path) | the store becomes the backup's logical store; the stream continues on it; the epoch is re-rolled (engine-internal). Refused with `placement_refused` when `into` is not empty, `not_found` when `dir` holds no backup | `{"dir":…,"commit_seq":<int>}` |
| `Repair` | none | `repair --rebuild-from-log` ([AR §4.10]); the logical store is unchanged | `{}` |
| `Verify` | none | `doctor --verify`: every derived structure recomputed and compared ([AR §4.10], [F13] EP-DV); the model evaluates its invariant predicates ([F13 §1.4]) | `{"findings":[{"check":<name>,"detail":<text>}…]}`; empty for a correct store |

## 9. Graph commands (group G)

### 9.1 `Tx`

A `TX` block: the semantic core of every graph write ([50 §3.10], [AR §4.5]).

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `stmts` | array of statements (§9.2) | — | a **data-level** block. Exactly one of `stmts`, `lq`, `ir` is given |
| `lq` | string | — | a `TX { … }` block in LQ text ([LQ/grammar-v1.ebnf]); the engine accepts it from M7, the model from M0 (LQ-3) |
| `ir` | string | — | a `tx` JSON IR document ([LQ/json-ir]) |
| `params` | object | `{}` | parameter values for `lq` and `ir` ([50 §6.1], [50 §3.2]) |
| `message` | string | `""` | the commit message, normalised by [F07] ([AR §4.6] item 6); a last paragraph beginning `Moirai-` is `bad_value`, exit 2 |
| `if_targets` | 32 hex | none | `IF TARGETS` ([50 §3.10] item 4); only with `lq` or `ir` |

`ctx.branch`, `ctx.lease`, `ctx.key`, `ctx.dry` and `ctx.if_tip` are the block's `ON`, `LEASE`, `KEY`, `DRY` and `IF TIP`
([LQ/canonical-ast §5.9] R3). A block that also spells one of them in its `lq` text takes the text's value; the two differing is
`usage`, exit 2.

**Semantics.** [50 §3.10] items 1–11: one block is one commit on one branch, all or nothing; statements run in order on a
candidate and later statements see earlier effects; the immediate validators run per statement and the deferred ones at the
end in I37′ order ([F13 §5]); markers come from the net ops (MC-1, [F13 §4.2]); idempotency is §7; `DRY` runs every check and
writes nothing. A block whose net changeset is empty and that emits no runtime record appends nothing: the result has
`rev_new` = null, and nothing is recorded for idempotency.

**The commit.** Kind `ordinary` ([F06 §3.1]); `stmt_origin` ([F06 §3.4]): `named-mutation` when the block is one
`CALL tx.<name>(…)` (then `stmt_sym` = the name), else `tx` for `ctx.door` = `cli` and `mcp-write` for `ctx.door` = `mcp`;
`stmt_hash` = `H` of the block (§7.3). The `Lease`, `Marker` and `Idem` records the block implies are in its group ([F05 §4.7]).

**Refusals.** Each writes nothing; the first one found in statement order decides ([50 §3.10] item 5):

| Condition | Code | Exit |
|---|---|---|
| an unknown kind, field, edge kind or value; a name that does not bind | E105, E101, E104, E102, E108 | 2 |
| a type that does not match; a counter assigned | E103 | 2 |
| a derived, runtime or tree-derived property; an artifact observation field, identity field or status; `CREATE (:artifact …)`; a created `AT` edge; an edge property | E115, naming the verb that writes it | 2 |
| a value outside its field's shape (record list, range, NaN, one-line text) | `bad_value` | 2 |
| a node id never allocated, a uid this store does not know | E111 | 2 |
| a literal target that is not live on the view (a tombstone, a node of another branch) | `not_found` with `what` = `node` and `value` = the id, the tombstone line (N01's text) or N06's text as its `detail` ([AR §5d.3] L3, [RULES/delete-policy-matrix] DP-003, [50 §3.6]) (open point 5) | 3 |
| a guard: `EXPECT`, `if_rev`, `if_status`, `if_holder`; `IF TIP`; `IF TARGETS` | E401; E402 | 4 |
| a stale fencing token | E407 | 5 |
| the role write policy ([RULES/role-write-policy]) | E406 | 6 |
| a transition or a guard of [RULES/status-machines] (a gating `fail_*` verdict, an unfinished child, a missing `answers` edge, …) | E404 | 6 |
| an invariant of [F13 §3] or a deferred validator (I2, I4, I5′, I6, I7, I11, `QueryInvalid`, `QueryCycle`) | E405 | 6 |
| a second live holder of an (root, exact path) key (I-F1) | `path_claimed` | 6 |
| a restricted delete; a live lease without `release` (I32′) | E409 | 6 |
| `UNLESS EXISTS` matching more than one node | E410 | 6 |
| a free-form block under the `unknown` model profile ([90 §8.1] L2) | E411 | 6 |
| a read-only view (§4.3 row 7) | E305 | 6 |
| an exhausted id space | `id_space_exhausted` | 7 |
| `tx.max-statements`, `tx.max-ops` | E501 (a deterministic cap, compared, [F17 §1.5] SP-2) | 10 |
| `wmem`, the inline bound for an agent verb ([F17 §4.4] W1, W2, W4) | E501 (resource class, §16.4) | 10 |

**Result.** Family T (§3.3).

### 9.2 Data-level statements

A data-level statement is a JSON object with the member `op`. Its members, its LQ equivalent ([50 §3.10] item 6) and the ops
it produces ([F06 §7.2]):

| `op` | Members | LQ equivalent | Ops |
|---|---|---|---|
| `create` | `as` (a variable name, referable as `"$<name>"` by later statements), `kind`, `fields` (object: field → value, `title` and header fields included), `body` (text), `under` (a target), `position` (§9.5), `edges_out` (array of `{"kind":…, "dst":<target>}`), `edges_in` (array of `{"kind":…, "src":<target>}`) | `CREATE (<v>:<kind> {<f>: …})[ UNDER <p> <position>]`, then `CREATE` of each edge, then `SET <v>.body = …` | `Create` (+ `AddEdge`, `Move`) |
| `set` | `target`; `fields` (object: field → value; `null` removes the field; `status`, `resolution` and `done` allowed); `incr` (object: counter field → non-zero delta); `body` (text, or `null` to remove); `guard` (object with `if_rev`, `if_status`, `if_holder`) | without `guard`: `SET <t>.<f> = …[, …]`, `REMOVE <t>.<f>`, `SET <t>.<c> = <t>.<c> + …`; with `guard`: `MATCH (n {id: <t>}) WHERE <guards> EXPECT 1 SET n.<f> = …` exactly as [LQ/std §7.2] `tx.set` | `SetField`, `SetStatus`, `Incr`, `SetBody` |
| `patch` | `target`, `remove`, `add` (texts) | `PATCH <t>.body REMOVE … ADD …` | `SetBody` |
| `link` | `src`, `kind`, `dst`, `pinned` (a commit, for kinds whose `props` is `pinned`) | `CREATE (<src>)-[:<T>[ {pinned: …}]]->(<dst>)` | `AddEdge`; for `supersedes` also the target's `SetStatus` (I6) |
| `unlink` | `src`, `kind`, `dst` | `MATCH (<src>)-[e:<T>]->(<dst>) EXPECT 1 DELETE e` | `RemoveEdge` |
| `move` | `target`, `under` (a target, or null to detach), `position` | `MOVE <t> UNDER <p> <position>`, or `SET <t>.parent = NULL` | `Move` |
| `reopen` | `target`, `reason` | `REOPEN <t> REASON …` | `SetStatus` + `Incr` of `reopen_count` |
| `delete` | `target`, `policy` (`restrict`, `cascade`, `reparent`; default `restrict`), `replaced_by`, `release` (bool), `reason` | `DELETE <t>[ POLICY …][ REPLACED BY …][ RELEASE][ REASON …]` | `Delete` + the policy ops of [RULES/delete-policy-matrix] |
| `resolve` | `key` (§5.3), `take` (`ours`, `theirs`, `base`), or `value`, or `repoint` (a target) | `RESOLVE '<key>' TAKE …` | `Resolve` |
| `call` | `proc` (`tx.claim`, `tx.complete`, `tx.heartbeat`, `tx.release`, `tx.reclaim`), `args` (object, the procedure's parameters by name) | `CALL tx.<proc>(<k>: …, …)` | as §10 |
| `define_query` | `text` (a `DEFINE QUERY …` statement in LQ text) | the text | `Schema` (mode weaken, class `query`) |
| `drop_query` | `name` | `DROP QUERY <name>` | `Schema` (mode weaken, class `query`) |

- **Targets** are `"#N"`, `"#u:<32 hex>"` or `"$<name>"` of an earlier `create` in the same block.
- **Kinds** are stored edge-kind names (`blocks`); the LQ equivalent writes their LQ names (`BLOCKS`). Field and kind names are
  [F08 §8.2]'s.
- **Values** are §5.2's JSON values, bound by the field's type ([50 §3.2]).
- A statement the data level does not have (an `ASSERT`, a `MATCH … EXPECT` target set, `UNLESS EXISTS`) is written with `lq`
  or `ir`.

### 9.3 The LQ equivalent of a data-level block

The LQ equivalent of a data-level block is `TX { s1; s2; … }`, where each `si` is the equivalent of the i-th statement by §9.2,
written with these rules:
- every value is a parameter `$p<k>`, k = 1, 2, … in the order the values occur in the rendering, bound to the value; the
  parameter substitution of [LQ/canonical-ast §5.5] makes this the same C-AST as the same text with literals;
- a target is its node literal (`#12`, `#u:…`) or the variable of its `create`;
- `create`'s variable is its `as` name, or `v<i>` for the i-th unnamed `create`.

The equivalent defines `H` (§7.3, `stmt_hash`) and the texts of `statements` (§3.3). An implementation may build the
equivalent's C-AST directly; the text is needed only for `statements`.

### 9.4 `Apply`

A batch: the ingestion of one run's `result.v1` records ([90 §7.1]–[90 §7.2]) or a data-level op batch ([AR §7.1] `apply`),
committed as one `TX` block ([AR §6.4], [50 §4.2]).

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `run` | text | none | the run name (`run:<id>` batches, [AR §6.4]); required with `results` |
| `results` | array of `result.v1` objects ([90 §7.2]) | `[]` | the workers' results |
| `stmts` | array of data-level statements | `[]` | an op batch; its record format on the CLI is M8's ([LQ/json-ir §9]) |
| `message` | string | `""` | the commit message |

**Branch.** `ctx.branch` when given, else the branch of the run's lane (`run` → `runs_in` → lane → `moirai_branch`), else the
resolved branch (CX-2) ([AR §6.4] D6; N13e: a run bound to a closed lane resolves to the branch the lane merged into).

**Expansion** (one block, in this order):
1. For each `results` entry in order: its `lease` must be a lease of the run (`LEASES.run` = the run's `#N`) on the batch branch;
   every other self-reported identity field is ignored ([90 §7.2]). `outcome` `done`, `failed` or `abandoned` becomes
   `CALL tx.complete(<task>, outcome: …, summary: …, evidence: …, lease: '<lease>')`, with the entry's lease presented for that
   call only (open point 16); `none` becomes `CALL tx.release('<lease>')` when the lease is live and not run-scoped (a live
   run-scoped lease is released by step 4; open point 42). `task`, `recorded` and `about` are the integers of [90 §7.2], read as `#N`.
2. Each carried finding and note becomes a `create` of a `finding` (`title`, `severity`, `failure_scenario`, `ABOUT` edges to
   `about`) or a `note` (`note_kind` = `kind`, `title`, `body` = `text`), deduplicated within the batch by the key
   `run:<run>/task:<n>/<kind>:<hex(BLAKE3-128(lp(title) ‖ lp(failure_scenario or text)))>` ([90 §7.2]); the first wins.
3. Then `stmts`.
4. Every run-scoped lease the batch names and that is still live is released (`Lease` event 2, reason 6 `apply`, [F05 §9.4]).

**Idempotency.** Without `ctx.key` the key is the explicit key `run:<run>` ([AR §6.4]: "keyed once per run"); `idem_payload` is
`H` of the expansion. **The commit.** One commit; `stmt_origin` `tx`, `stmt_sym` `apply` ([F06 §3.4]).

**Refusals.** A lease whose branch differs from the batch branch, or that belongs to another run: E407, exit 5, before any
write ([AR §6.4] D6). Any refusal of §9.1 for any statement refuses the whole batch.

**Result.** Family W; `branch` = the batch branch; `data` = `{"run":…,"entries":[{"task":<"#N" or null>,"lease":…,"outcome":…,
"completed":<bool>}…],"created":["#N"…],"released":["L-n"…],"recorded":["#N"…],"markers":[…],"affected":["#N"…]}`:
`completed` is true for an entry that step 1 turned into a `tx.complete`; `created` lists the nodes step 2 and `stmts`
created, ascending; `released` every lease the batch ended — by a completion (into `settled`), by step 1's release or by step 4
— ascending by lease id; `recorded` repeats the entries' `recorded` ids that exist on the batch branch; `markers` is §10.8's;
`affected` is the commit's.

### 9.5 Positions and the `order` field

`create … under` and `move` take `position`: `"first"`, `"last"`, `{"before":<target>}` or `{"after":<target>}`
([50 §3.10] `MOVE … [BEFORE y | AFTER y | FIRST | LAST]`).
- The node's `order` ([F08 §5.4.4]) is set when a position is given, and, without one, for a node of kind `doc` (as `last`).
  Otherwise `order` stays absent.
- The ordered siblings are the live children of the parent that have an `order`, sorted by (`order` bytewise, uid).
  `first`: `between(⊥, o₁)`; `last`: `between(oₙ, ⊤)`; `before y`: `between(order of y's predecessor or ⊥, order of y)`;
  `after y`: `between(order of y, order of y's successor or ⊤)`; with no ordered sibling, `between(⊥, ⊤)`. `between` is §17.2.
- `before` or `after` a node that is not a live child of the parent, or has no `order`: E404, exit 6.

### 9.6 Allocation order inside a command

DT-4 in detail. Within one command:
1. Commits take `seq`, `ref_seq` and `hlc` in the order the command appends them.
2. `#N`s: in the order the command's candidate creates nodes, which is statement order and, within a statement, the order of
   its LQ equivalent; the root node of R4 before the file nodes it is created with ([F08 §11.3]); a node that lands with a uid
   `UIDX` already knows reuses its `#N` (I1); nodes a merge, sync, cherry-pick, revert or import lands with uids new to the store
   take `#N`s in ascending uid order.
3. Random uids (§17.4): the ordinal i counts the command's random-derivation creates in the order of item 2.
4. Anchor handles `aN`: in the order the anchors are created (the order of the `--at` specs).
5. Leases: in the order of `Claim.ids` as given, one fencing token each.
6. Ref ids: in the order the refs are created.

### 9.7 `Mutation`

A named mutation of [LQ/std §7] by name: the semantic core of the CLI write verbs and of MCP `write` with `name` ([50 §4.2]).

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | required | `tx.add`, `tx.set`, `tx.link`, `tx.unlink`, `tx.move`, `tx.reopen`, `tx.supersede`, `tx.doc_patch`, `tx.rm`, `tx.resolve`, `tx.remember`, `tx.retract`, `tx.answer`; the procedures `tx.complete`, `tx.claim`, `tx.heartbeat`, `tx.release`, `tx.reclaim` (§10); the file mutations `tx.link_file`, `tx.unlink_file`, `tx.record_move`, `tx.links_fix`, `tx.links_sync` (§12) |
| `params` | object | `{}` | the parameters of [LQ/std §7.2]–§7.4 by name, values by §5 |
| `message` | string | `""` | the commit message |
| `move_lease` | ref | none | `--move-lease <ref>` for `tx.set` and `tx.complete` ([AR §5a.4]) |

- **Expansion.** [LQ/std §7.2]'s template, with the verb's flags as the block's options ([LQ/std §7.1]). Two expansions are
  this chapter's (open point 17): `tx.retract($id: node, $reason: text)` is `TX { SET <id>.status = 'retracted', <id>.reason =
  $reason }`; `tx.answer($q: node, $text: text, $by: text = 'owner')` is `TX { CREATE (a:note {title: $title, authority:
  $by}); SET a.body = $text; CREATE (a)-[:ANSWERS]->(<q>); SET <q>.answer = $text, <q>.status = 'answered' }`, `$title`
  being the first line of `$text` cut to 200 bytes at a scalar boundary, or `answer` when that line is empty
  ([RULES/status-machines] GD-003, GD-004).
- **The commit.** As §9.1; `stmt_origin` `verb` for `ctx.door` = `cli` and `named-mutation` for `mcp`, `stmt_sym` = the name
  ([F06 §3.4]).
- **Bulk class.** `tx.rm` with `policy` = `cascade` through `ctx.door` = `cli` is a bulk-class verb (§9.10).
- **Result.** Family T. The CLI's `rm` without `--yes` sends `ctx.dry` = true ([LQ/std §7.2]: "without `--yes` the verb runs it
  with `DRY`").

### 9.8 `Schema`

Weakening schema changes ([AR §2.12], [F08 §8.1]): adding a project kind, a field (also on a core kind), an enumeration value
(also of a core enumeration) or an edge kind.

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `items` | array of item objects | required | each `{"item":"kind"\|"field"\|"enum"\|"edge", …}` (the item classes 1–4 of [F08 §8.5]; `enum` is an enumeration value, the word of [F12 §6.6]'s `schema:enum:` key; the member is `item` because a field record has its own member `class`, the merge class; open point 43) followed by the members of [F08 §8.5] by name: symbols as their strings, enumeration bytes by name (`type` and `elem` by the type names of [F08 §5.1], `elem` null when 0), flag bytes as arrays of the names of their set bits, without a `has_*` bit (the presence of the member it announces says it), `KindSet`s and `covers` as §15.3 writes them; without the store-allocated members (`kind_id`, `edge_id`, an enumeration value's `value`, a field's `decl`, [F08 §8.3], §8.5.2) |
| `message` | string | `""` | the commit message |

**Effect.** One commit with one `Schema` op of mode `weaken` per item ([F06 §7.6]); the store-local ids are allocated by
[F08 §8.3]. Named queries are defined through `Tx` (`define_query`, `drop_query`). **Refusals.** An item whose key the view
already holds (a change, which needs `Migrate`), or that changes or shadows a core item: E405 with the rule `schema weakening`,
exit 6; a name outside [F08 §8.2]'s grammar: `bad_value`, exit 2. **The commit.** `stmt_origin` `verb`, `stmt_sym` `schema`, no
`stmt_hash`; the payload is §7.3's `payload(c)` (open point 18). **Result.** Family W; `data` =
`{"items":[<key text>…]}`.

### 9.9 `Migrate`

Strengthening schema changes and the data migration they need ([AR §2.12], [F08 §8.1]).

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `items` | array | required | item objects as §9.8, each with `"retire":true` to retire the item, or with the changed members |
| `stmts` | array of data-level statements | `[]` | data changes made in the same commit |
| `message` | string | `""` | the commit message |

**Effect.** One commit (bulk class, §9.10) with the `Schema` ops of mode `strengthen` and the ops of `stmts`. After it, every node
on the branch must conform to the new effective schema (I11). **Refusals.** A non-conforming node: E405 with the rule
`schema conformance (I11)` ([F19] open point 16), listing at most 10 ids; a change to a core item: E405. **The commit.**
`stmt_origin` `verb`, `stmt_sym` `migrate`, no `stmt_hash` ([F06 §3.4]); payload `payload(c)`. **Result.** Family W; `data` =
`{"items":[<key text>…],"migrated":["#N"…]}`.

### 9.10 The bulk class

The commands whose commits may be bulk commits ([F17 §4.4] W1, [AR §4.3]): `Merge`, `MergeContinue`, `Sync`, `Revert`,
`CherryPick`, `Migrate`, `Mutation` `tx.rm` with `policy` = `cascade` through `ctx.door` = `cli`, `FileMv` of a directory,
`ImageImport` (open point 19). A bulk and an inline commit of one changeset are identical to every observer of this API
(class I). Every other write command is an agent verb: above the inline bound it refuses with E501 (Rs, §16.4).

## 10. Coordination commands (group C)

The commands `Claim`, `Heartbeat`, `Release`, `Reclaim` and `Complete` are the procedures of [LQ/std §7.3]: each equals
`Mutation` with the procedure's name and the same arguments. Their result is family T; the procedure's rows are in `yields`.

### 10.1 `Claim` (`tx.claim`)

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `ids` | array of nodes | none | task claims, by id |
| `next` | bool | false | claim the first ready task (`claim --next`) |
| `scope` | node | none | with `next`: a subtree |
| `role` | text | none | the role the lease grants; a role lease with `run` or `session` |
| `agent` | text | none | the holder of the new lease; default the caller's actor (CX-3) |
| `ttl` | duration or `"run"` | `lease.ttl-default` ([CFG]) | `"run"` makes the lease run-scoped |
| `start` | bool | false | also `open → in_progress` |
| `run` | text | none | the run the lease is scoped to (`--run`) |
| `session` | bool | false | the orchestrator's session role lease (`--role orchestrator --session`) |

A `Claim` is a **task claim** — exactly one of `ids` and `next` = true, with `role` the role of the task leases and `run`
making them run-scoped to that run — or a **role-lease mint** — neither `ids` nor `next`, with `role` and exactly one of
`run` and `session`. Any other combination is `usage`, exit 2 (open point 41).

**Task claims.**
- Each task must be live on the claimer's branch (CX-2) and `ready` there with the claimer as the caller
  ([RULES/state-definition] PD-017, LF-003, LF-004): `unblocked`, no live lease of another holder, not excluded (I26′), and
  `defer_until` ≤ now (CK-7). `gates` never constrain `claim` ([AR §3.3] X5).
- A task the same holder already holds a live lease on returns that lease and allocates nothing ("claiming again as the same
  holder is idempotent", [AR §6.2]).
- `next` picks, among the ready tasks of the branch in `scope` that `fits_role(t, role)` ([LQ/std §4.1]), the one least by
  (`priority`, `#N`) (open point 11). No ready task: `ok`, no lease, `yields` rows empty, nothing recorded.
- The role of a task lease is `role`, else `developer` for a self-claim ([90 §4.3]); who may claim which is
  [RULES/role-write-policy] WM-001 to WM-003.

**Role leases.** `role` + `run`: a run-scoped role lease ([RULES/role-write-policy] WM-004). `role` = `orchestrator` +
`session`: the session role lease, bound to the minting thread where the harness names one (`LEASES.bound`), with the TTL
`lease.orchestrator-ttl` (its deadline decides liveness only where no slot anchors it or the slot table is unreadable,
[RULES/state-definition] LL-003 to LL-008), refused for a known subagent or a dispatched worker (WM-005).

**Each new lease** ([F05 §9.4] event 1, [F11 §6]):

| Field | Value |
|---|---|
| `lease_id` = `token` | `fence + 1`, allocated in `ids` order (§9.6) |
| `node`, `lkind` | the task and `task`; `#N` = 0 and `role` for a role lease |
| `role` | above |
| `holder` | `agent`, else the caller's actor |
| `anchor` | SL-1 (§6.3), for the caller's session |
| `expires`, `ttl_ms` | `after(now, ttl)` ([OS/clock §4.2]) and the TTL; `Stamp::NEVER` and 0 when run-scoped |
| `run` | the `#N` of the run named by `run` (§10.6); 0 = none |
| `branch` | the claimer's branch; for the session role lease it is recorded ([F05 §9.4] field 13 is always present) but fixes nothing (WR-005, §4.2 CX-2, §4.3 row 3) |
| `bound` | the session role lease: the minting thread's hash; else zero until CX-9 binds it |
| `root_session` | BLAKE3-128 of `codex:` + `ctx.meta.sessionId` for a Codex holder; else zero |
| `files_owned` | the task's `files_owned` globs ([70 S5]) |

**`start`.** The same command also commits `open → in_progress` on the branch (door `claim-start`, [RULES/status-machines]
DR-003), in the commit group with the `Lease` record.

**Records.** A lease group (`Lease` records, then `Idem`) without `start`; a commit group (the commit, its `Lease` records) with
it ([F05 §4.7]).

**Refusals.** A task that is not live or not ready: E404, exit 6, naming the failing clause (live, `unblocked`, leased by
`<holder>`, done or deleted on `<branch>`, deferred until) (open point 12); the minting and claiming rows: E406, exit 6.

**Yields** (one row per lease, in allocation order; a lease the claim returned again counts in `ids` order):
`{"lease":"L-n","token":<int>,"branch":…,"expires":<deadline>,"task":<"#N" or null>,"role":…,"holder":…,
"anchor":"session"|"session-ttl"|"none","run":<name or null>,"reused":<bool>}` — [LQ/std §7.3]'s `lease`, `token`, `branch`,
`expires`, then this chapter's members; `reused` is true for a returned existing lease (the item field of §17.1).

### 10.2 `Heartbeat` (`tx.heartbeat`)

Argument `lease` (required). The lease must exist and not have ended; the caller need not present it. When the lease is
TTL-bound and due for renewal ([OS/clock §4.4]), a `Lazy` heartbeat record ([F05 §9.11], cause 1) moves `expires` to
`after(now, ttl)`; a lease whose deadline has passed and that no one reclaimed is renewed by its holder with a durable `Lease`
record of event 4 (I17′). A run-scoped lease has no deadline: nothing is written. **Refusals.** An ended lease, or one whose
anchor is Dead: E407, exit 5. **Yields:** `{"lease":…,"expires":<deadline>,"renewed":<bool>}`.

### 10.3 `Release` (`tx.release`)

Argument `lease` (required), presented with its current token. One `Lease` record, event 2, reason 1 ([F05 §9.4]). **Refusals.**
An ended lease or a stale token: E407, exit 5. **Yields:** `{"lease":…}`.

### 10.4 `Reclaim` (`tx.reclaim`)

Arguments `older_than` (duration) or `run` (text), at most one; with neither, `older_than` is `lease.reclaim-older-than`
([CFG]; open point 37). It releases every task lease whose `claimed_hlc` is older than `older_than` (CK-6), whatever its
liveness, or every lease scoped to the run; one `Lease` record each, event 2, reason 3.
Rights: [RULES/role-write-policy] WM-008. **Yields:** one row per released lease, `{"lease":…,"task":<"#N" or null>}`, by lease
id.

### 10.5 `Complete` (`tx.complete`)

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `id` | node | required | the leased task |
| `outcome` | `done`, `failed`, `abandoned` | required | [AR §6.2] |
| `summary` | text | `""` | the summary (`--summary -`) |
| `evidence` | array of text | `[]` | `commit:<sha>` or `#N` entries |
| `pack_digest` | text | none | the digest token its pack printed ([RULES/pack-classes] NR-001) |
| `move_lease` | ref | none | [AR §5a.4] |

`ctx.lease` must present the task's live lease with its current token (door `tx-complete`, [RULES/status-machines] DR-002).

**Effect** ([AR §6.2], [50 §4.2], [RULES/status-machines] CO-001 to CO-003 as confirmed here, open point 13):
1. The task's status becomes `done` on the lease's branch for every outcome, from `open` or `in_progress` (from `open` the
   compound `open → in_progress → done` in one commit); its `resolution` becomes `completed` for `done`, `rework` for `failed`
   and `wontdo` for `abandoned`.
2. The lease is released into `settled`: a `Lease` record of event 2, reason 2, and the `settled` marker of MC-1, whose
   `outcome` byte ([F05 §9.5] field 11) records `done`, `failed` or `abandoned` and whose `holder` is the lease holder.
3. The commit message is `summary`; with evidence it is `summary`, an empty line, and `evidence: ` followed by the entries
   joined by `, ` (open point 14).
4. When the task's branch has a designated tree with links in the completed subtree, the link settle follows as a separate,
   CAS-guarded commit of the same command ([AR §6.2], [72 M11]; §12.6); at M0 over the simulated tree.
5. With `pack_digest`, the staleness notice of [RULES/pack-classes] NR-001 to NR-010 is computed; it never changes the write,
   its exit code or its payload (NR-009).

**Refusals.** An unfinished child (TG-001) or a gating `fail_*` verdict (GD-002): E404, exit 6, naming the child or the verdict;
no presented lease, another task's lease, a stale token: E407, exit 5; a malformed `pack_digest`: `usage`, exit 2 (NR-001).

**Yields:** `{"task":"#N","status":"done","ready":["#N"…],"outcome":…,"lease":…,"settle_commit":<commit or null>,
"changed_since_pack":<array or null>}` ([LQ/std §7.3]'s `task`, `status`, `ready`, then this chapter's members); `ready` is
the newly ready ids (§5.8); `changed_since_pack` holds
`{"id":"#N","change":"added"|"removed"|"changed","set":"K1"|"K1M"|"K2"|"K3"}` rows in NR-007's order, or null without a digest.
`commit` and `rev_new` of the result are the completing commit's. `ready` and `changed_since_pack` are not replayed: a
replay gives `[]` and null (§7.5).

### 10.6 Lease rules for every command that presents a lease

- **LP-1 (fencing).** The presented lease's current token is checked (I17′); a stale token is E407, exit 5.
- **LP-2 (renewal by use).** A write that presents a TTL lease due for renewal renews it: a `Lazy` heartbeat record of cause 2
  in the command's group ([AR §6.2], [90 §4.4]). A read never writes one (I-F5).
- **LP-3 (first write).** The first `set` of the leased task that presents its lease while the task is `open` also performs
  `open → in_progress` in the same commit ([RULES/status-machines] DR-004).
- **LP-4 (`files_owned`).** A `set` of `files_owned` under the lease re-captures it: a `Lease` record of event 3, mask bit 0.
- **LP-5 (run names).** A run is named by the title of a live `run` node on the resolved branch; `run` arguments resolve by it
  (`not_found`, `what` = `run`, exit 3, when none; open point 15).

### 10.7 `RunOpen`, `RunClose`

| Command | Arguments | Effect | Result `data` |
|---|---|---|---|
| `RunOpen` | `name` (text, required), `lane` (lane name), `harness`, `model`, `wf_id`, `bg_task_id`, `session_id`, `script_path`, `args_hash`, `journal_path`, `expected_artifacts` | one commit creating a `run` node: `title` = `name`, status `running`, the given fields ([F08 §9.3] `run`; `harness` and `model` per open point 15), `started` = now (CK-7), and a `runs_in` edge to the lane node of `lane`. A run node with this title already on the view: `name_taken`, exit 6 (open point 20). `stmt_origin` `verb`, `stmt_sym` `run open`, no `stmt_hash` | `{"run":"#N","name":…}` |
| `RunClose` | `name` (required), `outcome` (`green`, `red`, `stopped`, `died`) | one commit: status `outcome`, `ended` = now; I14's guard for `green`; then every lease scoped to the run is released (`Lease` event 2, reason 7) in the same group; `stmt_sym` `run close` | `{"run":"#N","status":…,"released":["L-n"…]}` |

Both are family W with `branch` = the resolved branch. Rights: [RULES/role-write-policy] WV-023 and its open point 27.

### 10.8 Markers in results

Every write result lists the markers its commits emitted ([F13 §4.2] MC-1, MC-5; [F05 §9.5]) as
`{"kind":"settled"|"deleted"|"cleared","id":"#N","ref":<ref name>,"commit":<commit>,"outcome":<name or null>,"cause":"ops"|"undo"|"op-restore"|"branch-delete"}`,
ordered by §5.5. Family T carries them in `markers` ([LQ/envelope §7.7], which gives `kind`, `id`, `ref`; the other members are
additive); family W in `data.markers`.

## 11. Version-control commands (group V)

Family W. Ref names, their validation (the ref-name rule of [80] X-F9, NFC on input) and revisions are [F12]'s. Every command
of this group that creates no commit writes an `Idem` record with an `IdemResult` (§17.1) when keyed. `stmt_origin` of the
commits of this group is `merge` for `Merge`, `MergeContinue` and `Sync`, and otherwise `verb` with `stmt_sym` = the verb as
[AR §7.1] spells it, its words joined by one space (`lane open`, `lane close`, `revert`, `cherry-pick`); none has a `stmt_hash`
([F06 §3.4]).

Unless a section says otherwise, the result's `branch` is the ref the command creates, moves, deletes, binds, checks out or
lands its commit on — `ref` of `WorktreeBind` and `Undo`, the checked-out ref of `Checkout` (null for a detached commit),
`into` of the merge family, `onto` of `Revert` and `CherryPick`, `main` for `LaneOpen` and `LaneClose` — and `rev` is that
ref's tip seq before the command, or, for a ref the command creates, its fork commit's seq. `OpRestore`, which moves several
refs, has `branch` and `rev` null.

### 11.1 `BranchCreate`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `name` | ref name | required | the new branch: `lane/<u>` or `plan/<u>` as written, any other name completed by [F12 §2.5] IN-3 (`lane/<name>`, or `plan/<name>` with `kind` = `plan`), after NFC normalisation (IN-1) |
| `from` | revision | the resolved branch | a ref (its tip) or a commit |
| `kind` | `work` or `plan` | from the name: `plan` for `plan/…`, else `work` | must agree with the name ([AR §5a.1], IN-3) |

**Effect** ([AR §5a.3] "Fork", [AR §5d.1], [F11 §3]): a `RefUpdate` record (reason 1, create) with the new ref's absorbed
vector — the source ref's vector with `absorbed_new[source ref]` = the fork commit's `ref_seq` — and the `RefTable` entry, in one
ref group ([F05 §4.7]). The new ref's tip is the fork commit; its `ref_seq_next` starts at 1; its `ref_id` is `next_ref_id`. The
pin is engine-internal. No commit. **Refusals.** The rules RN-1 to RN-8 of [F12 §2.4], checked in that order, each exit 2 with
[F12]'s codes (its open point 16): RN-1 to RN-6 `bad_ref_name`, RN-7 (a live ref holds the name) `ref_exists`, RN-8
`ref_prefix`; a `from` that names nothing, or a ref without a commit: E301, exit 3 (open point 20). **Result.** `branch`
= the new ref, `rev` = the fork commit's seq; `data` = `{"ref":…,"ref_id":<int>,"kind":…,"fork":<commit>}`.

### 11.2 `BranchDelete`

Arguments `name` (required) and `force` (bool, `-D`). **Effect** ([AR §5a.9], [F13 §4.2] MC-5, [RULES/state-definition]
LE-008): a `RefUpdate` (reason 2) with the `RefTable` entry marked deleted; every live lease on the branch released (`Lease`
event 2, reason 5); the ref's pins released (engine-internal); with `force`, the markers the branch still held unabsorbed are
re-attributed to a live ref that contains their commit or cleared (`Marker` records, cause 4); each with a triage line.
**Refusals.** Without `force`, a branch whose tip `main` has not absorbed: `not_merged`, exit 6 (open point 20); `main`, a
staging ref (use `MergeAbort`), an `import/*` or an `orphans/*` ref ([F12 §2.6]): `usage`, exit 2; a name no live ref holds:
E301, exit 3. **Result.**
`data` = `{"ref":…,"ref_id":<int>,"dropped":{"commits":<int>,"completions":<int>,"deletions":<int>},"released":["L-n"…],
"markers":[…],"triage":[<text>…]}`: `dropped` counts the commits reachable from the branch and from no other live ref, and the
completions and deletions among them ([AR §5a.9]).

### 11.3 `Checkout`

Arguments `target` (a revision, required) and `branch_new` (a ref name). **Key**: `ctx.client` or `MOIRAI_CLIENT` (kind
`client`); else, for `ctx.door` = `mcp`, the session key `session:<harness>:<id>`; else the directory `ctx.cwd` (kind
`directory`) ([AR §5a.4], [F11 §5.1]). **Effect**: a `ClientHead` record setting the key to the ref, or to a detached commit when
`target` is a commit; writing a directory key's row that changes its ref clears its designation ([F18 §3.5] last row). With
`branch_new`: `BranchCreate(branch_new, from: target)` and the checkout of the new ref, in one group. **Result.** `data` =
`{"key_kind":…,"key":<text>,"ref":<name or null>,"commit":<commit or null>,"designation_cleared":<bool>}`.

### 11.4 `WorktreeBind`, `WorktreeUnbind`

`WorktreeBind` takes `dir` (absolute path), `ref` and `replace` (bool); `WorktreeUnbind` takes `dir`. **Effect**: [F18 §3.5]'s
table and checks: a `ClientHead` record of key kind `directory` with the `binding` flag, `designated` when `dir` is a tree, and
the expected git ref and base from the tree's simulated git state (§6.6); with `replace`, the displaced row is rewritten in the
same group. **Refusals.** The I-F12 checks: `binding_conflict`, exit 5; a designation of a directory that is not a tree is not
refused: the result carries the warning `not_a_tree` ([F19 §10.4]). **Result.** `data` = `{"dir":…,"ref":…,"designated":<bool>,
"expected_ref":<text or null>,"base":<git id or null>,"replaced":[{"dir":…,"ref":…}…]}`; `WorktreeUnbind` `{"dir":…,
"removed":<bool>}`.

### 11.5 `LaneOpen`, `LaneClose`

`LaneOpen` takes `name` (required), `worktree` (absolute path, required), `git_branch` and `base` (a git commit prefix).
**Effect** — one group ([AR §4.5] "Flush grouping", [AR §7.6] step 3): `BranchCreate(lane/<name>, from: main)`;
`WorktreeBind(worktree, lane/<name>)` with the designation of [F18 §3.5] row `lane open`; and one commit on `main` creating the
`lane` node: `title` = `name`, status `active`, `worktree_path` = the path value of `worktree` (root `abs`), `git_branch`,
`base_sha`, `moirai_branch` = `lane/<name>` ([F08 §9.3]). The idempotency pair is on that commit. **Refusals.** As
`BranchCreate` and `WorktreeBind`; `worktree` not a tree: exit 2 ([F18 §3.5]). **Result.** `branch` = `main`; `data` =
`{"lane":"#N","ref":"lane/<name>","ref_id":<int>,"fork":<commit>,"binding":<the WorktreeBind data>}`.

`LaneClose` takes `name` and `mode` (`close` or `freeze`). **Effect** ([AR §5a.9]): one commit on `main` setting the lane node's
status — `merged` when `main` has absorbed the lane's tip, `abandoned` otherwise, `frozen` for `freeze`
([RULES/status-machines] open point 8) — and the removal of the lane's directory binding, in one group. **Result.** `branch` =
`main`; `data` = `{"lane":"#N","status":…,"unbound":<dir or null>}`.

### 11.6 `Tag`

Arguments `name` (the part after `tags/`, required), `commit` (default the resolved branch's tip), `message`, `pin` (bool).
**Effect** ([AR §5a.9]): a `RefUpdate` (reason 1) creating `tags/<name>` of kind `tag` at the commit, its `RefTable` entry with
the message, and with `pin` the pin (engine-internal) and the entry's `pinned` flag. `name` is completed by [F12 §2.5] IN-3
(`tags/` is prefixed unless present). **Refusals.** RN-1 to RN-8 as `BranchCreate` (`bad_ref_name`, `ref_exists`, `ref_prefix`,
exit 2); a `commit` that names nothing: E301, exit 3. **Result.** `branch` = the new tag, `rev` = its commit's seq; `data` =
`{"ref":"tags/<name>","commit":…,"pinned":<bool>}`.

### 11.7 `Merge`

| Argument | Type | Default | Meaning |
|---|---|---|---|
| `src` | ref | required | the source ref |
| `into` | ref | the resolved branch | the destination |
| `policy` | `delete-wins` or `resurrect` | the kinds' existence policies ([F08 §9.1]) | `--policy` ([AR §5a.7]) |
| `strict` | bool | `merge.strict` ([CFG]) | conflicts stage too |
| `base` | commit | none | `--base` overrides the LCA |
| `message` | string | `""` | the merge commit's message |

**Effect** — [AR §5a.7] steps 0–8, with the recursive virtual base of [F12], the typed rules of [RULES/merge-table] and
[RULES/link-merge-rules], the validators in I37′ order ([F13 §5]), markers from the net ops (MC-1) and the absorbed vector of
step 7:
- **Up to date.** `src`'s tip is an ancestor-or-self of `into`'s tip: nothing is appended; `outcome` = `up-to-date`.
- **Sync first** (`into` = `main` and `main`'s tip is not an ancestor of `src`'s tip): step 0's `sync` commit on `src`, then the
  merge commit on `main`, in one group; when the sync stages, the whole merge stages on `merge/<src>/from/main`.
- **Otherwise** one merge commit with two parents (`into`'s tip, `src`'s tip), always a new commit (open point 22), landing on
  `into` or, with a structural violation (or a conflict under `strict`), on `merge/<into>/from/<src>`.
- The merge commit carries the idempotency pair.

**Refusals.** A second merge of a pair whose staging ref is open: `staging_exists`, exit 6; a merge into `main` while `src` holds
unresolved conflicts: `conflicted_src`, exit 6; an unknown ref: E301, exit 3; `into` a tag, `import/*` or staging ref: E305,
exit 6. A staged outcome is exit 6 `staged` with the success keys (§2.3).

**Result.** `branch` = `into`; `data`:

| Member | Content |
|---|---|
| `src`, `into` | ref names |
| `outcome` | `landed`, `staged` or `up-to-date` |
| `sync` | null, or `{"commit":…,"outcome":"landed"\|"staged","conflicts":[…],"violations":[…]}` for step 0 |
| `lca` | the LCA commits, by generation then commit id |
| `virtual_base` | true when more than one LCA was merged into a virtual base |
| `conflicts` | `[{"key":<key text>,"class":<name>}…]` landed as conflict values ([F12] classes) |
| `violations` | `[{"key":<key text>,"class":<name>,"code":<int>,"description":<text>,"suggested":<text>}…]` ([F19 §12.6]) |
| `staging_ref` | the staging ref written, or null |
| `absorbed` | `into`'s (or the staging ref's) absorbed vector after the command: object ref name → `ref_seq` |
| `markers` | §10.8 |
| `affected` | the merge commit's `affected` ids |

These members are the items [60 §4.4] item 5 compares.

### 11.8 `MergeContinue`, `MergeAbort`

Both take `src` and `into` (default: the current branch's single open staging ref; with several, both are required: `usage`,
exit 2). `MergeContinue` re-runs [AR §5a.7] steps 5–8 against `into`'s current tip with the staged resolutions as an overlay; on
success it lands the merge commit on `into` and deletes the staging ref (a `RefUpdate`, reason 2) in one group; otherwise it
stages again. Its result is `Merge`'s. `MergeAbort` deletes the staging ref (a `RefUpdate`, reason 2); `data` =
`{"staging_ref":…}`.

### 11.9 `Sync`

Arguments `lane` (default the resolved branch) and `check` (bool). `Sync` is `Merge(src: main, into: lane)` with a commit of kind
`sync` that stores the residue ([AR §5a.3]) and whose canonical changeset is the full state diff ([AR §4.6]). With `check`
nothing is appended (the verb's `--check`, used by hooks, [AR §5a.7] step 0). **Refusals.** As `Merge`; a sync of L while
`merge/<L>/from/main` is open: `staging_exists` (I41′); a sync of `main`: `usage`. `--refork` ([AR §5a.3]) is M3's (open point 26).
**Result.** `Merge`'s, `src` = `main`.

### 11.10 `Revert`, `CherryPick`

`Revert` takes `commit` (required), `onto` (default the resolved branch), `mainline` (1 only) and `message`; `CherryPick` takes
`commit`, `onto` and `message`. **Effect** ([AR §5a.5], I34′): the inverse of the commit's net ops (revert) or their three-way
application with base = its first parent (cherry-pick), through the validators; a commit of kind `revert` or `cherry-pick` whose
`origin` is the commit ([F06 §3.1]). A `NotFound` stages on `merge/<onto>/from/<commit>` ([F12] spells the ref); a `DATA`
mismatch lands a `FieldEdit` conflict value. **Refusals.** Reverting a `sync` commit, a merge without `mainline` 1, a commit with
a dependent set ([AR §5a.5]): `revert_refused`, exit 6, listing the dependents (open point 20); `mainline` other than 1:
`usage`. History commands never touch a tree ([40 §3.6]): reverting or cherry-picking a commit whose group carried an
`FsIntentDone` changes the graph only, and the result carries the warning `graph_only_revert`, which names `FileRevert`
(open point 38). **Result.**
`branch` = `onto`; `data` = `{"origin":<commit>,"onto":…,"outcome":"landed"|"staged","conflicts":[…],"violations":[…],
"staging_ref":…,"markers":[…],"affected":[…]}`.

### 11.11 `Undo`, `OpRestore`

`Undo` takes `ref` (default the resolved branch), `n` (default 1) and `expect` (a commit). **Effect** ([AR §5a.5]): a
`RefUpdate` (reason 3) moving the ref back to its value n moves ago, with its recomputed absorbed vector and `moves_back` = n;
the markers of the moved range recomputed (MC-5): `cleared` for every completion or deletion that leaves the ref's history,
re-emitted `settled`/`deleted` for every one that re-enters it while the state still holds, each with a triage line. `expect`
absent means no check (open point 24). **Refusals.** `expect` differs from the tip: E402, exit 4; fewer than n moves: E301, exit
3. **Result.** `data` = `{"ref":…,"old":<commit>,"new":<commit or null>,"moved_back":<int>,"markers":[…],"triage":[<text>…]}`.

`OpRestore` takes `seq` (required). **Effect** ([AR §5a.5]): every ref is restored to its value after the last ref move whose
seq is at most `seq`: one `RefUpdate` (reason 4, `restore_seq`) per moved ref, the markers recomputed in both directions (MC-5);
a ref created after `seq` is deleted, a ref deleted after `seq` and still within its reflog window is restored (open point 23).
**Result.** `data` = `{"seq":<int>,"moved":[{"ref":…,"old":<commit or null>,"new":<commit or null>}…],"markers":[…],
"triage":[…]}`.

## 12. File-link commands (group F; R4 at M0 visibility)

### 12.1 Visibility and common rules

- **At M0** the reference model implements this group as link intent over data: capture, registration, the intent protocol and
  exact-evidence resolution over the simulated trees and git histories of §6.5 and §6.6 ([60 §4.2] row "File links", WP-92).
  **The engine** implements the data of `FileAdd`, `LinkFile` and `UnlinkFile` at M2 (FL-3), `Check` at M4 and the rest at M6.
- **Tree.** The command's tree is CX-5's; its eligibility, the writer-tree predicate and freshness are [40 §5.1], [F18 §3.6] and
  [40 §5.3]. Roots: `project` is the tree's root, a named root comes from the user-scope `roots.<name>` ([CFG]), `abs` is a
  machine-local absolute path ([F08 §5.4.1]).
- **Path arguments** are texts resolved by [40 §3.8] and P1–P12 ([OS/path]); refusals `bad_path`, `nonportable_name`,
  `ambiguous_path` (exit 2).
- **Commits** of this group carry `relink` values of [F18 §5] and `path_moves` entries with CK-5's `hlc`.
- **Runtime rows** (`FILEOBS`, `PENDING`, `TREES` epochs, `FSINTENT`) are written as [F05 §9.15]–§9.26 state; they are never in
  `state(ref)` (I-F4), and of them only `FSINTENT` is in the runtime snapshot (§15.7).
- **Comparison** of tree-derived answers is subset consistency (§16.5).
- **Results.** `branch` is the caller's resolved branch (CX-2), on which the command's commits land, and `rev` its tip seq
  before the command. Path values in results are §5.2's `"<root name>:<path text>"`.

### 12.2 `FileAdd`

Arguments `paths` (array, required), `kind` (a file kind of `artifact_kind`), `root`. **Effect** ([40 §3.3], [F08 §11.2]): each
path is registered in the tree — an existing live file node is reported, not duplicated; a new one is created with its derived
uid, `oid` over its bytes ([F20 §2.3]) and, on the root's first file, the root node — in one commit; `stmt_origin` `file-verb`,
`stmt_sym` `add`. **Refusals.** A path not in the tree: `not_found` (`path`), exit 3. **Result.** `data` =
`{"files":[{"path":<path>,"id":"#N","created":<bool>}…]}`.

### 12.3 `LinkFile` (`tx.link_file`), `UnlinkFile` (`tx.unlink_file`)

`LinkFile` takes `node` (required), `specs` (array of anchor specs, required), `watch`, `planned`, `quote`, `end`. **Effect**
([40 §3.2], [40 §2.7], [F20 §6.1]): capture of each spec, then one commit with the file nodes (created or reused), the root node
on the root's first link, and the `at` edge with one anchor per spec (`AddEdge`, with `aN` allocated in spec order). A planned
link records the planning tree's HEAD in `observed_git`. `UnlinkFile` takes `node` and exactly one of `anchor` (`aN`) or `path`:
one commit removing that anchor, or every anchor into the file at the path, and the edge with its last anchor ([40 §3.2]).
**Refusals.** `anchor_spec`, `ambiguous_path`, `bad_path` (exit 2); `not_found` (`anchor`), exit 3. **Yields:** one row per anchor:
`{"file":"#N","path":<path>,"anchor":"a<n>","kind":<anchor kind>,"created":<bool>}` (`created` false for a de-duplicated capture,
[F08 §11.4]).

### 12.4 `FileMv`, `FileRm`, `FileRevert`: the intent protocol

`FileMv` takes `srcs` (array), `dst`, `git` (bool) and `retry_ms`; `FileRm` takes `paths`, `reason`, `replaced_by` (a path or
node), `trash`, `recursive` and `yes`; `FileRevert` takes `commit`, a commit whose group carried an `FsIntentDone` (else
`not_found`, `what` = `intent`, exit 3). `FileRevert` runs the inverse file-system operations of that intent through the same
protocol ([40 §3.6]): each moved item back from its destination to its source, each item a `trash` removal moved into the trash
back to its path; an item removed without `trash` has no bytes to restore and ends as `missing` (open point 38). The protocol is
[40 §3.4]–§3.6 at the logical level:

1. **Plan.** The tree must be the writer tree of the caller's branch (`not_writer_tree`, exit 5); sources exist; the destination
   does not; one volume (`cross_volume`, exit 7); an alias source is accepted only when the tree is fresh for the node (open point
   20: `not_fresh`, exit 6); affected nodes by path and aliases; affected globs by literal prefix. `FileRm` without `yes` stops
   here and returns the impact as a dry run (exit 0).
2. **Intent.** An `FsIntent` record in its own group; the command's intent slot (SL-3).
3. **File system.** The rename or deletion in the simulated tree (§6.5); `trash` moves into `trash/<intent>/<i>` ([F02 §5.4]).
   A failure after the intent writes `FsIntentAborted` (the outcome `refused`, §2.3).
4. **Commit.** One commit: `FileMv` sets each affected node's observation (`relink` `explicit/intent`, the old path added to
   `aliases`, `observed_git` = the tree's HEAD, `observed_blob` empty), adds for a directory the `explicit` `path_moves` entry,
   and rewrites globs; `FileRm` sets status `removed` with `reason` and `replaced_by` and re-points anchors ([40 §3.5] step 3);
   `FsIntentDone` is in the same group. Each node is guarded on its `rev_seq`. `stmt_origin` `file-verb`, `stmt_sym` `mv`,
   `rm`, or `revert` (open point 27).
5. **Several items.** Items that fail are reported and the rest commit; the command exits 8 ([40 §3.4]).
6. **Crash.** With `EnvCrash` `in-next`, recovery follows [40 §3.4]'s table (roll forward after the re-barrier, abort,
   `ambiguous`, `missing`); the model decides it from the simulated tree.

**Result.** `data` = `{"intent":"i-<n>","items":[{"src":<path>,"dst":<path or null>,"outcome":"done"|"busy"|"exists"|"missing"
|"failed"}…],"repointed":["#N"…],"path_move":<pathmove or null>,"globs":[{"id":"#N","field":…,"from":…,"to":…}…],
"impact":<object or null>}`; `impact` only for a `FileRm` dry run (the lists of [40 §3.5] step 1).

### 12.5 `FileRelink` (`tx.record_move`), `LinksFix` (`tx.links_fix`)

`FileRelink` takes `from` and `to`: `to` must exist in the tree; one commit re-pointing the file node, provenance `owner/manual`
or `agent/manual` by the caller's role ([40 §3.6], [F18 §5]). `LinksFix` takes `target` (a node or `aN`) and `action` (`accept`,
`to`, `confirm`, `accept-replacement`, `drop`, `same-as`, `split`, `repin`, `pin`, `restore`, `prefix`) with `expect`, `to`,
`at`, `same_as`, `reason`, `replaced_by`, `from` as the action needs ([40 §3.7]). **Refusals.** `repin_needs_at`,
`confirm_refused`, `path_claimed` (exit 6); `accept` whose re-evaluated top proposal differs from `expect`: E404, exit 6.
**Yields:** `{"target":…,"action":…,"relink":<text or null>,"commit":<commit or null>}`.

### 12.6 `LinksSync` (`tx.links_sync`)

Arguments `scope`, `budget_ms`, `since`, `deep`, `all`, `force`. A settle point ([40 §3.7], [AR §5e.3], [F18 §2.6] I-F6): resolve
the scope against the tree, then one commit with every exact re-bind the write rule allows (writer tree, freshness, the quiescence
re-check — which a simulated tree, constant within a command, always passes — and the `rev_seq` guard), plus lazy `TreeReg` epoch
and `FileObs` rows; in a reader tree only `Pending` rows. **Yields:** `{"rebound":[{"file":"#N","from":<path>,"to":<path>,
"evidence":<token>}…],"pending":<int>,"states":{<state>:<count>…},"commit":<commit or null>}`.

### 12.7 `Check`

Argument `id` (required). Ancestry of the node's pinned git commit (`measured_on`, `observed_git_sha`) against the tip of the
bound worktree's git history (§6.6) ([AR §2.14] CM7); appends lazy `GitFacts` ancestry facts only ([F05 §9.25]) and never a
commit. **Result.** `data` = `{"node":"#N","commit":<git id or null>,"tip":<git id or null>,"verdict":"ancestor"|
"not-ancestor"|"unknown"}`.

## 13. Image commands (group I; M5)

The engine implements them at M5 ([AR §5b.6]); the model supplies `state(ref)`, the canonical changesets and the commit ids
([60 §4.2] row "Image"). Family W with `branch` and `rev` null (both act on a set of refs), bulk class for `ImageImport`.

| Command | Arguments | Effect | Result `data` |
|---|---|---|---|
| `ImageExport` | `dest`, `refs` (glob list), `granularity` (`checkpoint`, `commit`), `object_format` (`sha1`, `sha256`), `create`, `if_older` (duration), `force` | [AR §5b.6] export; `GitMap` records; no commit. Quiet mode refuses without `force` | `{"dest":…,"refs":[{"ref":…,"git_commits":<int>,"head":<git id>}…],"cursor_seq":<int>}` |
| `ImageImport` | `from`, `refs`, `force` | [AR §5b.6] import: native commits verified, foreign commits recomputed by the typed rules, violations staged on `import/<ref>`; several refs → `partial` (exit 8) when some fail | `{"refs":[{"ref":…,"imported":<int>,"native":<int>,"foreign":<int>,"outcome":"applied"\|"staged","staging_ref":<name or null>}…]}` |

## 14. Observations (group O)

Observations append nothing ([40] I-F5) and are never keyed.

### 14.1 `Query`

Every read verb ([AR §7.7.2], [LQ/std §3]). Arguments: `name` and `params` (a named query), or `lq` or `ir` with `params`; `use`
(`--at`); `limit`; `cursor`; `mode` (`run`, `check`, `explain`, `profile`); `budget` (object); `ids` (bool). **Semantics**: [50 §3]
and [LQ/std]; the view is `use`, else the resolved branch (CX-2, [50 §3.9]); tree-derived built-ins use CX-5's tree. **Result.**
Family R ([LQ/envelope §7]); refusals are LQ codes ([LQ/errors]); a budget cut is exit 10 with the rows before it.

### 14.2 `State`

Arguments `ref` (default the resolved branch) or `at` (a revision), and `parts` (array of `content`, `local`, `derived`; default
all). **Result.** Family X; `branch`, `rev` the view's; `data` = the snapshot of §15.2.

### 14.3 `Runtime`

No arguments. **Result.** Family X with `branch` and `rev` null (the runtime state is store-wide); `data` = the runtime
snapshot of §15.7.

### 14.4 `History`

Arguments `ref` (default: every ref) and `since_seq` (default 0). **Result.** Family X; `data` = §15.8's history snapshot of the
commits with seq above `since_seq` that are reachable from the ref (from any ref, reflog or pin without `ref`), and of the ref
moves.

## 15. `state(ref)` and the snapshots

### 15.1 Definition

For a view V — a ref's tip, or a commit c named by `State.at` — `state(V)` is the logical state `state_at(c)` of [60 §4.2]
projected into three parts:

| Part | Content | Equal across stores holding c | Compared by |
|---|---|---|---|
| `content` | the portable versioned state: what canonical item 10 can express ([AR §4.6]), keyed by uid and by name | yes, in both anchor-text modes (I28′, I38′, I39′) | GT2, and GT8's state-identical gate through `content_digest` ([60 §3.6]) |
| `local` | store-local values that are deterministic in one stream: `#N`, `CREATOR`, the `rev`, `created` and `updated` seqs, anchor handles and anchor texts | no | GT2 |
| `derived` | the predicates of `P_F15` ([F13 §6.2]) at V | yes | GT2; `Verify` |

Tip-only runtime data — leases, markers, `ready`, `claimed`, `*_elsewhere` — is not state; the runtime snapshot (§15.7) holds it.
Tree-derived states are never in a snapshot.

### 15.2 The snapshot

`data` of `State` is one object:

| # | Member | Content | Hashed |
|---|---|---|---|
| 1 | `state` | `1`, the snapshot version | no |
| 2 | `ref` | the view's ref name, or null for a commit view | no |
| 3 | `commit` | the view's commit, or null for a ref with no commit | no |
| 4 | `content` | §15.3 | yes |
| 5 | `local` | §15.4 | yes |
| 6 | `derived` | §15.5 | yes |
| 7 | `digests` | `{"content":<64 hex>,"state":<64 hex>}` (§15.6) | no |

A part not requested by `State.parts` is null; `digests` are computed over the parts regardless.

### 15.3 `content`

`{"schema_version":1,"schema":[<item>…],"nodes":[<node>…],"schema_conflicts":[<conflict>…]}`

- **`schema`**: every schema item on V ([F08 §8.1]; the core schema is implied by `schema_version`), sorted by class (`kind`,
  `field`, `enum`, `edge`, `query`: [F08 §8.5]'s class order) and then by key text (§5.3). An item object is `{"item":…,"key":<key text>, …}` followed
  by the members of [F08 §8.5] by name in that table's order, written as §9.8 writes them, with: symbol ids as their strings; `KindSet`s as
  `{"any":<bool>,"kinds":[<kind name>…]}`; `covers` as value names; the store-local members (`kind_id`, `edge_id`, an enumeration
  value's `value`, `ast_hash`) omitted; `retired` as a bool.
- **`nodes`**: every node that exists on V, live or tombstone, sorted by uid. A node object, members in this order:

| Member | Content |
|---|---|
| `uid` | `"#u:…"` |
| `kind` | the kind name ([F08 §9.1], or a project kind's name) |
| `existence` | `"live"` or `"deleted"` |
| `tombstone` | only when deleted: `{"reason":<text>,"replaced_by":<uid or null>}` ([F08 §3.5]) |
| `status`, `resolution` | names |
| `priority` | integer 0–4 |
| `criticality`, `confidence`, `authority` | names |
| `flags` | the set source-truth flags among `pinned`, `archived`, `frozen`, sorted ([F08 §3.2]) |
| `title` | the stored title, or null (a live artifact, [F08 §7.1]) |
| `parent` | the parent's uid, or null |
| `order` | the `order` text, or null |
| `fields` | object: every present field whose storage is `field` or `cold` ([F08 §8.4.2]), `order` excepted, name → value in §5.2's state form; members in bytewise order of the names |
| `body` | the 32-hex BLAKE3-128 of the body bytes, or null |
| `edges` | the node's out-edges on V, retained out-edges of a tombstone included (I39′): `{"kind":<stored name>,"dst":<uid>,"disc":<32 hex or null>,"props":{…}}` with `props` holding `flagged`, `pinned` and `anchor` (the anchor record of §5.4 **without** its texts) as present; sorted by (`kind`, `dst`, `disc`) |
| `conflicts` | the node's keys that hold a conflict value: `{"key":<the snapshot key text of §5.3 without the node part: existence, status, <field>, observation, body, parent, edge:<kind>:<dst uid>[:<32 hex anchor uid>]>,"class":<name>,"base":…,"ours":…,"theirs":…}`, sorted by `key`; the key's own member (`status`, a field, `body`, `parent`, an edge) holds the value the node's row holds while the conflict stands, which is [F12]'s (open point 31) |

- A tombstone keeps the members [F08 §3.5] keeps: `kind`, the header enumerations, `title`, retained edges; `fields` is `{}`,
  `body` null, `parent` null, `order` null.
- **`schema_conflicts`**: schema keys that hold a conflict value, as `conflicts` entries keyed by the key text of §5.3
  (`schema:…` or `query:…`), sorted by key.

### 15.4 `local`

`{"nodes":[{"uid":…,"id":"#N","created_by":<actor>,"created_role":<role>,"rev":<int>,"created":<int>,"updated":<int>,
"anchors":[{"uid":…,"handle":"a<n>","quote":…,"prefix":…,"suffix":…,"end":…}…]}…]}`, nodes sorted by uid, anchors by uid; an
anchor text member is omitted when the store does not hold it (`text_unavailable`, [F08 §10.3]).

The seqs are functions of V's history (open point 21). Let the **first-parent chain** of c be c, its first parent, that commit's
first parent, and so on to a root; the **net changeset** of a commit is against its first parent (for `sync` the full state
diff, [AR §4.6]):
- `rev` (`rev_seq`, the `--if-rev` target, [AR §3.1]): the seq of the newest commit on the chain whose net changeset changes a key
  that n owns ([F06 §2.3]);
- `created` (`created_tx`): the seq of the newest commit on the chain whose net changeset takes n's existence from absent to live
  or deleted;
- `updated` (`updated_tx`): the seq of the newest commit on the chain whose net changeset changes n's existence, hierarchy,
  status, body, a field or a counter.

### 15.5 `derived`

`{"nodes":[<entry>…]}`, one entry per **live** node, sorted by uid; an entry is `{"uid":…,"done":…,"unfinished":…,
"unblocked":…,"blocked":…,"open_blockers":…,"open_blockers_exo":…,"is_blocker":…,"children_total":…,"children_done":…,
"ready_to_close":…,"container":…,"suspect":…,"answered":…,"conflicted":…,"has_dangling":…,"depth":…}` with the definitions of
[F13 §6.2] and [RULES/state-definition] §4:
- `done` and `unfinished` are null for a kind without a status machine's `done` ([F08 §8.5.1] `has_done`), `answered` for a
  kind other than `question`;
- `unblocked` holds the structural clauses only; `defer_until ≤ now()` is outside `P_F15`;
- the counters are exact counts; a stored `u16` saturates at 65,535 ([F08 §3.4]) and the snapshot carries the true value;
- `topo` is not in `derived` ([F13 §6.2]: only its validity is semantic).

### 15.6 Digests

```
content_digest = BLAKE3-256( lp("moirai-api-content-v1") ‖ lp(CJ(content)) )
state_digest   = BLAKE3-256( lp("moirai-api-state-v1") ‖ lp(CJ(content)) ‖ lp(CJ(local)) ‖ lp(CJ(derived)) )
```

`CJ` is §5.6, `lp()` and BLAKE3-256 are [F01 §6.3] and §7.1; the digests are written as 64 lower-case hexadecimal digits. Each
`CJ(…)` is below 2^32 bytes at every scale this API runs at (≈ 400 MB at 1e6 nodes, est.). Two views with equal digests have
equal snapshots except with probability 2^-128; GT2 compares `state_digest` after every write for every touched ref and the full
snapshot on a mismatch ([60 §4.4] item 2).

### 15.7 The runtime snapshot

`data` of `Runtime`; members in this order:

| Member | Content |
|---|---|
| `counters` | `{"commit_seq":…,"next_id":…,"next_anchor":…,"fence":…,"next_ref_id":…}` ([F04]) |
| `refs` | per ref, by `ref_id`: `{"ref_id":…,"name":…,"kind":…,"deleted":<bool>,"tip":<commit or null>,"tip_seq":<int or null>,"ref_seq_next":…,"fork":<commit or null>,"absorbed":{<ref name>:<ref_seq>…},"message":<text or null>,"pinned":<bool>}` ([F11 §3]) |
| `moves` | the ref moves that no commit carries, by the order they happened: `{"ref":…,"reason":"create"\|"delete"\|"undo"\|"op-restore","old":…,"new":…,"actor":…,"hlc":<string>}` ([F05 §9.2]) |
| `leases` | every lease that has not ended, by (`#N`, lease id): `{"lease":"L-n","task":<"#N" or null>,"kind":"task"\|"role","role":…,"holder":…,"branch":…,"run":<name or null>,"token":…,"run_scoped":<bool>,"session_role":<bool>,"ttl_ms":…,"expires":<deadline>,"claimed_hlc":<string>,"anchor":{"kind":…,"session":<32 hex or null>},"bound":<32 hex or null>,"root_session":<32 hex or null>,"files_owned":[…],"live":"alive"\|"dead"\|"unknown"\|"deadline"}`; `live` is `lease-live` of [RULES/state-definition] at the current environment, `deadline` meaning not live only because its deadline passed ([RULES/state-definition] LE-009) |
| `markers` | every marker, by §5.5: `{"kind":…,"id":"#N","ref":…,"commit":…,"outcome":<name or null>,"origin":"op"\|"undo"\|"op-restore"\|"reattributed"\|"branch-delete","ref_seq":…,"actor":…,"orig_ref":…,"seq":…,"hlc":<string>,"active_on":[<ref>…]}` ([F11 §7]); `active_on` lists the live refs on which MC-4 makes it active |
| `exclusions` | every pair of a live ref R and a task `#N` with `excluded(R, #N)` of [F13 §4.1], by (ref name, `#N`): `{"ref":…,"id":"#N","held_on":[<ref>…]}`; the engine computes it from the marker cache, the model from the definition (MC-7, GT18) |
| `idem` | every entry within its retention window, by key: `{"key":<32 hex>,"payload":<32 hex>,"branch":…,"commit":<commit or null>,"default_key":<bool>,"append_hlc":<string>}` ([F11 §8]) |
| `heads` | every client head and binding, by (kind, key text): `{"kind":"directory"\|"client"\|"session","key":<text>,"ref":<name or null>,"commit":<commit or null>,"binding":<bool>,"designated":<bool>,"expected_ref":<text or null>,"base":<git id or null>}` ([F11 §5], [F18 §3]) |
| `alloc` | every allocated `#N`: `{"id":"#N","uid":…,"ref":<name>,"create_seq":…}` ([F11 §9]) |
| `intents` | every `FsIntent` of the last `gc.trash-expire` window: `{"intent":"i-n","op":…,"items":[…],"state":"open"\|"done"\|"aborted","outcomes":[…]}` ([F11 §12.7]) |
| `quiet` | bool |
| `digest` | `BLAKE3-256( lp("moirai-api-runtime-v1") ‖ lp(CJ(<this object without digest>)) )`, 64 hex |

Excluded by construction: lsns (`emit_lsn`, `origin_lsn`, `tip_lsn`, `fork_lsn`), `ProcId`s, pins' file sets, the overlay and
total counters, `ops_since_fork`, `promoted_seg`, `base_pin`, the move cache, `config_gen`, `FILEOBS`, `PENDING`, `FPRINT`,
`DIRMAP`, `TREES`, `ANCHORRES`, `GITFACTS` (their effect on answers is compared by §16.5).

### 15.8 The history snapshot

`data` of `History` = `{"commits":[<commit>…]}`, by seq; a commit object has the fields of [F06 §4.3] by name, values by §5:
`{"id","seq","ref","ref_seq","kind","import","parents":[<commit>…],"stated":[<commit or null>…],"gen","hlc","append_hlc",
"actor","actor_src","role","session","git":<object or null>,"message","schema_version","origin","foreign_git","verified",
"stmt_origin","stmt_sym","stmt_hash","idem":<bool>,"affected":["#N"…],"affected_complete":<bool>,"absorbed":<object or null>,
"sync_base":<commit or null>,"changeset_digest":<64 hex>,"orphan":<bool>}`. `changeset_digest` is [F07]'s; the model computes it
with its own canonical encoder ([60 §4.2]).

## 16. Comparing the model and the engine

### 16.1 What is compared

GT2 runs one stream on both and compares, per command ([60 §4.4] items 2 and 5, [AR §8.3] row "GT2 differential, runtime
tables included"):

1. the exit code;
2. on a refusal, the codes of the errors in order and their code-specific keys of [F19 §10.3], minus §16.3;
3. on success, the result envelope minus §16.3;
4. the id of every commit the command created;
5. after every write command, `state_digest` of every touched ref (the command's branch, every ref whose tip moved, every staging
   ref written) and the runtime digest; the full `State` and `Runtime` snapshots on a mismatch and every k commands, k being a
   harness parameter;
6. for `Merge`, `MergeContinue`, `Sync`, `Revert` and `CherryPick`, every member of their result (§11.7), which carries the
   items of [60 §4.4] item 5;
7. after every `EnvCrash`, the runtime snapshot, markers and leases included ([72 M1]).

The model's results reach the comparison through the testkit's converter (§1.2).

### 16.2 What the engine may not add

A member this chapter does not define is a comparison failure, except the additive members a later chapter registers under
[F19 §8.4] (which the comparison then covers or lists in §16.3).

### 16.3 Excluded members

| Where | Member | Why |
|---|---|---|
| every envelope | `budget` | engine work and memory accounting ([LQ/envelope §7.5]) |
| family R | `next`; `dropped` | the cursor bytes carry the engine's remaining work budget ([LQ/envelope §8.1]); `dropped` depends on rendering budgets. The rows of every page are concatenated and compared instead |
| families R and T | `reads` | rendering: the reading echo is the reference renderer's (WP-71a) |
| families R and T | the entries of `warnings` and `notices` whose code [LQ/errors §5.1] marks `product` or `renderer` | the model raises only codes marked `model` |
| family T | `statements[].text` | the display printer's rendering ([LQ/gql-spelling §5]) |
| family W | `warnings` other than `hook_label_narrowed`, `two_harnesses`, `not_a_tree`, `graph_only_revert` (§11.10) | environment diagnostics of `doctor` |
| every error | `message`, `help`, `detail`, `suggest`, `span`, `expected` | texts ([60 §4.3]: CLI text rendering is outside the model) |
| error keys | `store_locked.*`, `fs_busy.retries`, `fs_busy.waited_ms`, `fs_busy.os`, `disk_full.os`, `internal.*`, `refused_location.*`, `sealed_size.*`, `store_corrupt.*` | engine and OS faults |
| `Maintain` | `data.ran` | class I |
| `Verify` | `findings[].detail` | text |
| `BranchDelete`, `Undo`, `OpRestore` | `data.triage` | text; the markers and counts are compared |
| `ImageExport`, `ImageImport` | git object ids, `cursor_seq`, `git_commits` | the model produces no git bytes ([60 §4.3]); GT8 checks them |
| `Query` with `mode` `explain` or `profile` | everything but the exit code | the plan lines are not frozen ([LQ/envelope §10.2]) |
| `FileMv`, `FileRm`, `FileRevert`; `Runtime` `intents` | the intent id (`i-<n>`) | it is the lsn of the `FsIntent` record ([F05 §9.15]); intents are compared in the order they were opened |

### 16.4 Outcomes that are not compared

When the engine's outcome is a resource-class refusal — E501 from `wmem` or from the inline bound of an agent verb
([F17 §4.4] W1, W2, W4), E502, E503, E504, `store_locked`, `outcome_pending`, `disk_full`, `fs_busy`, `durability_failure`,
`store_io_fault` — or `outcome_unknown` under `EnvCrash`, the command is not compared: the model applies nothing (for
`EnvCrash`, it adopts the candidate of §6.7), and the harness counts such commands per run ([F17 §1.5] SP-2). A refusal from a
deterministic cap (`tx.max-statements`, `tx.max-ops`) is compared.

### 16.5 Subset consistency for tree-derived answers

For link and anchor states in `Query` results, and for the re-binds a `LinksSync` or a `Complete` settle writes, the engine's
answer must equal the model's or be more conservative, never a different target ([60 §4.2] row "File links", [40 §8.3.2] P11,
the order of conservativeness of [40 §8.3.2]). For a write, every re-bind the engine wrote must be one the model allows; the model
then applies exactly the engine's commit, so the streams stay in lock step.

### 16.6 Orders the engine chooses

A result whose order uses `topo` (for example `std.ready`, whose order is `priority, topo, id`, [LQ/std §4.1]) is compared with
the rows that agree on every order key before `topo` taken as a set, and each side's order checked to be a valid topological
order ([F13 §6.2]: only `topo`'s validity is semantic) (open point 11).

### 16.7 Class-I invariance

The same stream under two profiles that differ only in class-I values gives the same compared data, commit ids and digests
([F17 §1.5] SP-1; [CFG §9.4] class I). GT2 draws each run's profile from the test profile or the production values
([60 §4.4] item 1).

## 17. Bytes and derivations this chapter owns

### 17.1 `IdemResult`: the `result` of an `Idem` record

[F05 §9.6] field 7 (`result`, `vbytes`) and [F11 §8] (`IDEM.result` with `result_inline`) hold these bytes. An `Idem` record is
written only by a keyed command that appends no commit (§7.4). A sequence table ([F01 §2.6]); symbol references resolve through
the enclosing record's `SymDefs` block or earlier definitions ([F05 §8.1]):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `n_sections` | `u8` | always | 1 to 4 |
| 2 | `sections` | `n_sections` × `Section` | always | strictly ascending by `rtype` |

`Section`:

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `rtype` | `u8` | always | 1 `lease-grant`, 2 `lease-end`, 3 `ref-move`, 4 `head`; other values invalid |
| 2 | `n` | `uvar32` | always | ≥ 1 |
| 3 | `items` | `n` × the item of `rtype` | always | in the order the command's result lists them |

Item of `rtype` 1 (`lease-grant`; `Claim`):

| order | name | encoding | present when | meaning |
|---|---|---|---|---|
| 1 | `lease_id` | `uvar64` | always | the lease |
| 2 | `node` | `nodeid` | always | the task; 0 for a role lease |
| 3 | `branch` | `refid` | always | the lease's branch |
| 4 | `role` | `sym(role)` | always | the role granted |
| 5 | `holder` | `sym(actor)` | always | the holder |
| 6 | `expires` | `Stamp` (24 B) | always | the deadline at the grant; `Stamp::NEVER` when run-scoped |
| 7 | `ttl_ms` | `uvar64` | always | 0 when run-scoped |
| 8 | `run` | `nodeid` | always | the run node; 0 = none |
| 9 | `anchor_kind` | `u8` | always | 0 `none`, 1 `session`, 4 `session-ttl` |
| 10 | `reused` | `bool8` | always | 1 when the claim returned the holder's existing lease |

Item of `rtype` 2 (`lease-end`; `Release`, `Reclaim`): `lease_id` `uvar64`, then `reason` `u8`, the release reason of
[F05 §9.4] field 18.

Item of `rtype` 3 (`ref-move`; `BranchCreate`, `BranchDelete`, `Tag`, `Undo`, `OpRestore`, `MergeAbort`, `Checkout` with
`branch_new`): `ref_id` `refid`, `reason` `u8` ([F05 §9.2] field 1), `old` `cid32`, `new` `cid32`.

Item of `rtype` 4 (`head`; `Checkout`, `WorktreeBind`, `WorktreeUnbind`): `key_kind` `u8` ([F05 §9.3] field 2), `key` `b16`,
`op` `u8` (1 set, 2 remove), then when `op` = 1: `target_kind` `u8` (1 ref, 2 commit), `target_ref` `refid` (`target_kind` = 1)
or `target_commit` `cid32` (`target_kind` = 2), and `bflags` `u8` ([F05 §9.3] field 9) when `key_kind` = 1.

Rules: the bytes are consumed exactly; a value outside an enumeration, a reserved bit, an undefined symbol or bytes left over
make the `Idem` payload malformed with [F05 §5.4]'s consequence. A replay rebuilds the result `data` of §7.5 from these items and
the current names of the refs they reference.

### 17.2 Order keys

[F08 §5.4.4] leaves the generation of `order` keys to this chapter. The alphabet A is `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ`
followed by `abcdefghijklmnopqrstuvwxyz` (62 digits); d(x) is a digit's index in A. Because A is in ascending ASCII order, the
bytewise order of keys is the order of the fractions 0.k₁k₂…kₙ in base 62. A key is non-empty and never ends in `0`.

```
between(a, b)            -- a: a key or ⊥ (the low end); b: a key or ⊤ (the high end); a < b
  = mid(a = ⊥ ? "" : a,  b = ⊤ ? nil : b)

mid(a, b)                -- a: digits (the empty string reads as 0); b: digits or nil (reads as 1); value(a) < value(b)
  if b ≠ nil:
    n := the length of the longest common prefix of b and pad(a), pad(a) reading a missing digit of a as '0'
    if n > 0: return b[0..n) ‖ mid(a[n..), b[n..))        -- a[n..) is empty when n ≥ len(a)
  da := len(a) > 0 ? d(a[0]) : 0
  db := b ≠ nil ? d(b[0]) : 62
  if db − da > 1:              return A[(da + db) div 2]
  if b ≠ nil and len(b) > 1:   return b[0..1)
  return A[da] ‖ mid(a[1..), nil)
```

The result k satisfies a < k < b bytewise and never ends in `0`. *(Informative)* `between(⊥, ⊤)` = `V`; `between(⊥, "V")` = `F`;
`between("V", ⊤)` = `k`; `between("V", "W")` = `VV`.

### 17.3 The store id

`store_id = BLAKE3-128( lp("moirai-api-store-id-v1") ‖ lp(u64le(seed)) ‖ lp(u32le(t)) )`, with t = 0, then 1, 2, … until the
value is not all zero ([F02 §4]). Only under the injected entropy; a production `init` draws the OS random source.

### 17.4 Random uids

The uid of the i-th node with `uid_derivation` = `random` ([F08 §2.2]) that command n creates (i from 0, in §9.6's order):

```
uid = BLAKE3-128( lp("moirai-api-uid-v1") ‖ lp(u64le(seed)) ‖ lp(u64le(n)) ‖ lp(u32le(i)) ‖ lp(u32le(t)) )
```

with t = 0, then 1, 2, … while the value is all zero or names a uid `UIDX` holds ([F08 §2.2] "drawn again"). A command that runs
its phase 1 again derives the same uids. Derived uids (file, root and anchor keys) never use this rule ([F08 §11]).

## 18. Doors

Every front end compiles each call to one command ([AR §7.7.2]: "every write verb … compiles to one LQ `TX` block"). The flags
every verb takes map to `ctx` (§4.1): `--branch`, `--lease`, `--agent`, `--client`, `--tree`, `--model`, `--idempotency-key`,
`--no-dedupe`, `--dry-run`, `--if-tip`.

| CLI verb ([AR §7.1]) | Command |
|---|---|
| `brief`, `pack`, `ready`, `blocking`, `blockers`, `show`, `find`, `tree`, `notes`, `stale`, `changes`, `stats`, `lane conflicts`, `conflicts`, `q`, `log`, `diff`, `blame`, `reflog`, `op log`, `file where`, `links check`, `links mentions`, `merge-check` (its preview part) | `Query` (the named query of [LQ/std §3]); `merge-check`'s merge preview is `Merge` with `ctx.dry` |
| `tx` | `Tx` (`lq`) |
| `add`, `set`, `link A --<kind> B`, `unlink A --<kind> B`, `move`, `reopen`, `doc patch`, `supersede`, `retract`, `answer`, `rm`, `resolve`, `rule`, `note`, `decision`, `finding`, `verdict`, `measurement` | `Mutation` with `tx.add`, `tx.set`, `tx.link`, `tx.unlink`, `tx.move`, `tx.reopen`, `tx.doc_patch`, `tx.supersede`, `tx.retract`, `tx.answer`, `tx.rm`, `tx.resolve`, `tx.remember` |
| `apply` | `Apply` (one command per run; several runs make the CLI's exit 8, [F19 §7.2] rule 4) |
| `migrate` | `Migrate` |
| `schema add` (the verb's name is M8's; proposal, open point 18) | `Schema` |
| `claim`, `heartbeat`, `release`, `reclaim`, `complete` | `Claim`, `Heartbeat`, `Release`, `Reclaim`, `Complete` |
| `run open`, `run close` | `RunOpen`, `RunClose` |
| `branch`, `branch -d`/`-D`, `checkout`, `worktree bind`/`unbind`, `lane open`, `lane close`/`freeze`, `tag`, `merge`, `merge --continue`/`--abort`, `sync`, `cherry-pick`, `revert`, `undo`, `op restore` | `BranchCreate`, `BranchDelete`, `Checkout`, `WorktreeBind`, `WorktreeUnbind`, `LaneOpen`, `LaneClose`, `Tag`, `Merge`, `MergeContinue`, `MergeAbort`, `Sync`, `CherryPick`, `Revert`, `Undo`, `OpRestore` |
| `link ID --at`, `unlink ID --at`, `file add`, `file mv`, `file rm`, `file relink --after`, `file revert`, `links sync`, `links fix`, `check` | `LinkFile`, `UnlinkFile`, `FileAdd`, `FileMv`, `FileRm`, `FileRelink`, `FileRevert`, `LinksSync`, `LinksFix`, `Check` |
| `image export`, `image import`; `image push`/`pull` (transport, then import) | `ImageExport`, `ImageImport` |
| `init`, `config set`/`unset`, `quiet`, `gc`, `backup`, `restore`, `repair`, `doctor --verify` | `Init`, `ConfigSet`/`ConfigUnset`, `Quiet`, `Gc`, `Backup`, `Restore`, `Repair`, `Verify` |
| `config get`/`list`/`check`, `doctor` (other modes, `--fsck`), `image doctor`/`show`/`gc`, `export`, `schema result-v1`, `integrate`, `hooks install`, `hook`, `mcp`, `links import` | none: front-end, integration and diagnostic verbs outside the `Store` API; `links import` (M9) issues `LinkFile` commands |

| MCP tool ([AR §7.2]) | Command |
|---|---|
| `brief`, `pack`, `get`, `query`, `changes`, `branch` | `Query` |
| `claim` | `Claim`, `Heartbeat` or `Release` by `action` |
| `complete` | `Complete` |
| `remember` | `Mutation` `tx.remember` |
| `write` | `Tx` (`lq`) with `tx`; `Mutation` with `name` and `params` |

Hooks that write: `SessionStart` (the settle: `LinksSync`; the orchestrator lease: `Claim` with `role` = `orchestrator` and
`session`), `SubagentStop` (`Release`). The lazy records hooks append on their own (`SessionMark`, cursors, evidence rows) change
no compared state and are outside the API at M0 (open point 25).

## 19. Examples

`docs/spec/store-api/examples/*.json` hold informative examples ([F01 §2.1]). Each file is one JSON object
`{"example":<name>,"spec":<the sections it illustrates>,"setup":<the state the first step starts from, in words>,
"illustrative":[<what is a placeholder>],"steps":[{"command":<envelope>,"result":<envelope>}…]}`, pretty-printed; the
results are shown with insignificant whitespace, and their canonical form is §5.6. A file's steps continue the stream its
`setup` names, with that stream's command numbers `n`; most start from the stream of `02-tx-create.json` through `n` = 4
(open point 44).

**Computed values.** Every value that the rules of this chapter and the environment of §6 determine from the stream is
written as those rules give it: `n`, seqs, `#N`, lease ids and tokens, ref ids, order keys (§17.2), timestamps, `wall_ms`,
`boot_ns`, deadlines, the store HLC values of CK-4, key orders, keys, members and result families.

**Placeholders.** Values that a hash would give are placeholders chosen to read easily; they are not computed, and each file
lists its placeholders under `illustrative`:

| Value | Placeholder |
|---|---|
| the commit of seq s | `c`, then s as 8 lower-case hexadecimal digits, then 56 zeros, so its text form `c<8 hex>` shows the seq (`c00000003` for seq 3) |
| the uid of `#N` | `#u:` and N as 32 hexadecimal digits |
| the uid of anchor `aN` | `ac` and N as 30 hexadecimal digits |
| the store id | `5a` sixteen times |
| the `boot_hash` of boot k | `b` and k as 15 hexadecimal digits |
| every other digest, hash, git object id and `oid` | a two-digit pattern repeated to the value's length (`cd…cd`), `sha1:` before a git id |
| intent ids, budget values, error, warning, violation and triage texts, statement texts | as the file says |

Fixtures are written from the rules of this chapter, never from the examples. Where an example and a rule disagree, the rule
holds and the example is a finding.

| File | Shows | Families and groups |
|---|---|---|
| `01-init-and-clock.json` | `EnvClock`, `Init` with the test profile's values, `ConfigSet` (a success and a `config_value` refusal), `ConfigUnset` | X, W; E, S |
| `02-tx-create.json` | the base stream: `EnvSlots`, `Init`, the orchestrator's session role lease, a data-level `Tx` creating a parent, two ordered children and a `blocks` edge; an `E105` refusal | X, W, T; E, S, C, G |
| `03-set-guard.json` | `Mutation` `tx.set` with `if_rev`: a guard conflict (E401, exit 4), then success | T; G |
| `04-claim-complete.json` | `EnvSlots` with an alias, `Claim` with `start` and a `session` anchor through MCP with a Claude stamp, `EnvClock`, `Complete` with its marker, newly ready id and yields | X, T; E, C |
| `05-idempotency.json` | a replay, a payload mismatch (E408, exit 9), and a replay across a merged branch (§7.4 row 3) | T, W; G, V |
| `06-rm-policy.json` | `tx.rm` with `DRY`, a restricted delete (E409), then the delete with `replaced_by` and its `deleted` marker | T; G |
| `07-lane-merge.json` | `EnvTree`, `EnvGit`, `LaneOpen`, a lane claim and completion, a sync-first `Merge` that lands, `LaneClose` | X, W, T; E, V, C, G |
| `08-merge-staged.json` | a `Sync` staged by a `DanglingEdge` (exit 6), `tx.resolve` on the staging ref, `MergeContinue` | W, T; V, G |
| `09-undo.json` | `Undo` with `expect`, the recomputed `cleared` marker, and a stale `expect` (E402) | W; V |
| `10-caller-context.json` | lease-first branch resolution over `MOIRAI_BRANCH`, the environment-lease binding (CX-9) and its refusal, and an explicit branch that differs from a task lease (E407) | W, T; V, C, G |
| `11-state-snapshot.json` | `State` with its three parts and digests | X; O |
| `12-runtime-snapshot.json` | `Runtime` after a claim with `start`, with the CK-4 HLC values | X; O |
| `13-file-mv.json` | `EnvTree`, `EnvGit`, `WorktreeBind`, `LinkFile` with its root and file nodes, `FileMv` of a directory, a raw move and the `LinksSync` settle that re-binds it | X, W, T; E, V, F |
| `14-apply-batch.json` | `RunOpen`, a run-scoped task claim, a run-scoped role lease, `Apply` of two `result.v1` entries, `RunClose` | W, T; C, G |
| `15-lease-clock.json` | a TTL lease, its renewal by `Heartbeat`, its expiry across `EnvClock` advances, and a reboot | X, T; E, C |
| `16-image.json` | `Quiet`, `ImageExport` refused in quiet mode and forced, `ImageImport` | W; S, I |
| `17-query.json` | `Query` of a named query (node shape, a live cursor), of LQ text (table shape), and an `E101` refusal | R; O |
| `18-maintenance.json` | `Maintain`, `Gc`, `Backup` (a success and a `placement_refused` refusal), `Verify`, `Repair` | W; S |
| `19-schema-migrate.json` | `Schema` adding a field, a write of it, and `Migrate` narrowing its range with a data statement | W, T; G |
| `20-vcs-refs.json` | `BranchCreate` with namespace completion and a `bad_ref_name` refusal, `Checkout`, `Tag`, `Revert`, `CherryPick`, `BranchDelete` refused (`not_merged`) and forced | W; V |

## Coverage

| Item | Part covered here | Section |
|---|---|---|
| [60 §3.1] item 2: the logical `Store` API — typed commands, results in the `--json v1` data shape, `state(ref)`, the injected deterministic clock | complete | §1–§18 |
| [60 §4.4] items 1–5: the command stream, lock-step comparison, crash semantics, merges and recoveries | the stream, the environment events and the comparison with its exclusions; the harness mechanics are `moirai-testkit`'s (M1) | §2, §6, §16 |
| [60 §2.5] "Store parameters": semantic visibility | the API side: class-I invariance, class-V keys through `ConfigSet`; the parameters are [F17]'s | §8.2, §16.7 |
| [60 §2.5] "Derived-state semantics" | the `derived` part of `state(ref)`, newly ready ids at the tip; the semantics are [F13 §6]'s | §5.8, §15.5 |
| [60 §2.5] "Harness-agnostic interface and pure Rust": `LEASES` `kind`, `role`, `run`, `anchor`, `bound`; `actor_src`; the refusal code and texts | the values the commands write and the resolver that decides them; the layouts are [F05 §9.4], [F11 §6], [F06 §3.5] and the texts [F19 §11] | §4, §10.1 |
| [90 §10.1] "`LEASES` runtime rows" | the fields `Claim` writes; their form in the runtime snapshot | §10.1, §15.7 |
| [90 §10.1] "Holder anchor (X-F2)" | the anchor kind a claim takes (SL-1) and liveness over the injected slot table (SL-2); the bytes are [F03]'s | §6.3 |
| [90 §10.1] "Commit header" (`actor_src`) | the resolver row that sets it (CX-3) | §4.2, §4.4 |
| [90 §10.1] "Error table and refusal texts" | where E411, the two E407 refusals and E406 are raised; the texts are [F19 §11]'s | §4.3, §9.1 |
| [90 §10.1] "Output contract" | the result families; the byte rules are [F19]'s | §3 |
| [40] R-3 | the injected random-uid rule, for `random` kinds only; derived uids are [F08 §11]'s | §17.4 |
| [40] R-6 | the order in which `aN` handles are allocated | §9.6, §12.3 |
| [40] R-7 | which commands write `FsIntent`, `FsIntentDone`, `FsIntentAborted` and the lazy R4 rows | §12.4, §12.6 |
| [40] R-12 | I-F5 (observations append nothing), I-F6 (the settle write rule), I-F12 (the binding checks) as command behaviour; the invariants are [F18 §2]'s | §11.4, §12.6, §14 |
| [40] R-15 | `WorktreeBind` and `LaneOpen` as commands; the row is [F18 §3]'s | §11.4, §11.5 |
| [40] R-16 | the link object in results, with the members [F19 §8.8] leaves to this chapter | §5.4 |
| [40] R-17 | the `relink` values `FileMv`, `FileRelink` and `LinksFix` write; the vocabulary is [F18 §5]'s | §12.4, §12.5 |
| [50] F3 | named-query definitions through `define_query` and `drop_query` | §9.2 |
| [50] F10 | `stmt_origin`, `stmt_sym` and `stmt_hash` of every command | §9.1, §9.7, §11, §12 |
| [50] F14 | `append_hlc` under the injected clock | §6.2 CK-4 |
| [50] F15, F16 | `affected` in results; the `derived` part | §3.3, §15.5 |
| [50] F17 | `alloc` in the runtime snapshot | §15.7 |
| [50] F18 | `QueryInvalid` and `QueryCycle` as violations in merge results | §11.7 |
| [80] X-F2 | deadlines and anchors evaluated on the injected clock and slot table | §6.2, §6.3 |
| [80] X-F9 | the ref-name rule applied by `BranchCreate`, `Tag`, `LaneOpen`; the rule is [F12]'s | §11.1 |
| [F05 §9.6] delegation: the stored idempotency result | complete | §17.1 |
| [F06 §4.4.7], [F06 §5.5] delegations: the payload of a verb without a `TX` block; `pathmove.hlc` under the injected clock | complete | §7.3, §6.2 CK-5 |
| [F08 §5.4.4] delegation: order-key generation | complete | §17.2 |

No other [60 §2.5] row, R-item, F-item or X-F item is specified here: this chapter defines no on-disk structure other than
`IdemResult`.

## Holes

None. No value of this chapter is decided by an M0 measurement or benchmark. `HOLE(pack-digest-param)` of [RULES/pack-classes]
is a naming decision, not a measurement; it is decided here (open point 29).

## Open points for the review

1. **The JSON form of commit ids (conflict between spec files).** The A1 dispositions record A-m7 as "fixed (varied)": JSON
   carries `c` + 64 lower-case hex. [LQ/envelope §7.3], [LQ/errors §5.7] and [F12 §3.8] follow that; [F19 §8.3] and its open
   point 24 write "64 hexadecimal digits without the `c` prefix", the finding's original proposal. This chapter follows the
   dispositions (§5.1). [F19 §8.3] should be corrected by WP-18's owner. Commit literals in arguments are [F12 §3.2]'s (7 to 64
   digits), not the 4-digit minimum an earlier draft of §5.1 had.
2. **What this chapter closes.** [PLAN §3.3] assigns WP-25 no gap. The delegations of the written chapters are closed here: the
   `Idem.result` bytes ([F05 §8.7], §9.6) in §17.1; the payload of a verb that compiles to no `TX` block ([F06 §4.4.7]) in §7.3;
   `pathmove.hlc` under the injected clock ([F06 §5.5]) in CK-5; the order-key generation ([F08 §5.4.4]) in §17.2; the displayed
   run name ([F11] open point 4, [F19] open point 30): a run is named by its node's `title` (§10.6 LP-5, §10.7); the Store API form
   of `init --set` ([CFG] open point 11): `Init.params`, and `ConfigSet`/`ConfigUnset` between commands (§8.1, §8.2); `gc` as a
   GT2 command ([F17] OP-17-17): confirmed (§8.5); the `complete` outcomes ([RULES/role-write-policy] WS-004,
   [RULES/status-machines] CO-002, CO-003): point 13; `HOLE(pack-digest-param)`: point 29.
3. **Family T readings** (§3.3). [LQ/envelope §7.7] does not say what `commit` holds for a write, a `DRY` or a replay; §3.3 reads
   it as the created commit, else the tip read, else the original. The appended key `yields` and the marker members `commit`,
   `outcome` and `cause` (§10.8) are additive under [F19 §8.4]. WP-19 folds them into [LQ/envelope §7.7], or states other forms
   that this chapter then cites.
4. **Diff rows** (§5.7). [LQ/std §2.9] gives the `diff` relation's columns without the aspect vocabulary or the value forms;
   §5.7 fixes them because family-T results carry them and GT2 compares them. Proposal for WP-19: [LQ/envelope §5.8] and
   [LQ/std] adopt §5.7's table.
5. **A write to a node that is not live on the view** (§9.1) exits 3: [AR §5d.3] L3 ("any write that assumed #40 (`set 40`,
   `link --blocks 40`, …) fails with exit 3/4") and [RULES/delete-policy-matrix] DP-003 ("the target must be live in the view;
   the tombstone is printed otherwise", exit 3) fix the exit code, and a guard on a live node (`--if-rev`) keeps exit 4. No
   error code carries it yet: [F19 §10.2]'s `not_found` excludes LQ node ids and [LQ/errors] has no exit-3 write code. This
   chapter uses `not_found` with `what` = `node` and N01's or N06's text as the detail; WP-18 extends `not_found`'s `<what>`
   list, or WP-19 assigns an LQ code at exit 3, and this chapter then cites it. An earlier draft read the case as E401 (exit 4),
   which contradicts both sources.
6. **The session's checkout in branch resolution** (§4.2 CX-2). [AR §5a.4] and [AR §7.2] say that unresolved MCP reads use the
   session's checkout; [90 §4.1]'s Branch row does not list it. It is placed before the `default-branch` key and used by
   `ctx.door` = `mcp` only.
7. **The `git` provenance group** (§4.4). [F06 §4.4.6] holds `worktree` as a symbol but no chapter states its value. A canonical
   absolute path would put a machine-local datum into the hashed canonical form (item 5), against the spirit of I-F4; §4.4 uses
   the tree's display label of [F19 §4.3]. [F06]/[F07] confirm.
8. **Key texts** (§5.3) follow [F12 §6.6], now written: `#N.existence` (an earlier draft of this chapter proposed `#N.exists`),
   `schema:enum:` (it proposed `schema:value:`), `query:<name>`, and the anchor handle `aN` as an `at` edge's discriminator. The
   snapshot form, with uids in place of `#N` and `aN`, is this chapter's, because `state(ref)` holds no store-local number;
   [F12] may adopt it if another chapter needs store-independent key texts. The diff aspects (§5.7) and the schema item class
   words (`enum`, §9.8, §15.3) use the same words.
9. **Injected entropy** (§6.4, §17.3, §17.4). A reproducible stream needs reproducible random uids and store ids. The derivations
   use the command number and the creation ordinal, so a phase-1 re-run derives the same values; production draws the OS random
   source ([F08 §2.2], [F02 §4]) and is unaffected. The model and the engine must both implement this test-only rule.
10. **Default keys and settles** (§7.1). A default key over an identical payload would replay a second `links sync` or `heartbeat`
    within `idempotency.default-window` although the tree or the clock changed. `LinksSync` therefore takes an explicit key only;
    `Heartbeat` and `Check` append only lazy records, which cannot carry the durable `Idem` record, and are never keyed.
    [AR §6.4] says "every write verb … takes a key"; the review confirms the exemptions.
11. **`topo` in orders** (§10.1 `next`, §16.6). `std.ready` orders by `priority, topo, id`; among ready tasks no precedence path
    exists (a ready task has no open blocker, is no container and has no ancestor with an open exogenous blocker), so any
    topological order is valid and `topo` only breaks ties arbitrarily between implementations. `Claim.next` therefore picks by
    (`priority`, `#N`), and the comparison treats `topo` ties as sets. Proposal for WP-19: replace `t.topo` in `std.ready`'s order
    by `t.id`.
12. **`claim` of a task that is not ready** is E404 (exit 6) with the failing clause (§10.1). [LQ/errors] adds the case text.
13. **`complete --outcome failed|abandoned`** ([RULES/status-machines] CO-002, CO-003; [RULES/role-write-policy] WS-004).
    Confirmed: every outcome writes status `done` ([AR §6.2]: "writes `done` on the lease's branch"; [50 §4.2]), so a failed task
    is excluded from dispatch until reopened. The outcome is recorded twice: in the versioned `resolution` (`completed`, `rework`,
    `wontdo`), which [F11] open point 1 expects, and in the `settled` marker's `outcome` byte ([F05 §9.5]). Conflict: [LQ/std §7.3]
    says `failed` and `abandoned` "release the lease without the transition"; [AR] wins ([F01 §2.4] rule 2), and WP-19 aligns
    [LQ/std §7.3]. The alternative (release without the transition) would leave a failed task `in_progress` and re-dispatchable.
14. **`complete`'s summary and evidence** (§10.5) become the commit message; the design names both flags but no carrier. The
    evidence line keeps the message one normalised text ([F07]) and adds no field.
15. **Run nodes** (§10.6, §10.7). [90 §7.1] has the run record its harness and model, which CX-6 reads; [F08 §9.3]'s `run` kind has
    no such fields. Proposal for WP-14: `harness` (`sym`, scalar, decl 29) and `model` (`sym`, scalar, decl 30). Runs are named by
    their title, unique among the view's run nodes (§10.7).
16. **A lease per call inside a batch** (§9.4). `Apply` completes several tasks, each under its own lease, in one block, but
    `tx.complete` takes the block's single `LEASE`. Proposal for WP-19: `tx.complete` gains `$lease: text? = NULL`, used only by
    `apply`'s expansion, so the batch keeps one `H`.
17. **`retract` and `answer`** ([LQ/std] open point 13) get the expansions of §9.7. `answer` creates the answering `note`
    because [RULES/status-machines] GD-003 requires a live `answers` edge; `--by` sets the note's `authority`.
18. **Weakening schema changes have no verb.** [AR §2.12] makes weakening changes apply at once, but neither [AR §7.1] nor LQ has
    a statement for adding a kind, field, value or edge kind. §9.8 defines the `Schema` command; M8 names its CLI verb (proposal
    `moirai schema add`), and its payload is `payload(c)`.
19. **The bulk class** (§9.10) adds `Revert` and `CherryPick` to [AR §4.3]'s list (import, `migrate`, `rm --cascade`, a directory
    `file mv`, long merges): both are orchestrator verbs whose changesets can be as large as a merge's.
20. **Refusal codes this chapter needs and [F19] lacks**: `name_taken` (exit 6: a run name in use, §10.7), `not_merged` (exit
    6: `branch -d` of an unmerged branch, [AR §5a.9]), `revert_refused` (exit 6: a `sync` commit, a merge without `--mainline 1`,
    a dependent set, [AR §5a.5]), `not_fresh` (exit 6: a `file mv` of an alias source the tree is not fresh for, [40 §3.4]), and
    `not_found`'s `what` = `node` (point 5). Ref and tag names use [F12]'s proposed codes instead (its open point 16:
    `bad_ref_name`, `ref_exists`, `ref_prefix`, all exit 2; an earlier draft of this chapter used `bad_value` and `name_taken`,
    exit 6, for them). WP-18 adds them to [F19 §10.2] and §7.3, or maps each onto an existing code.
21. **`rev`, `created`, `updated`** (§15.4) are defined over the first-parent chain, which gives "after a merge, the merge
    commit's seq" ([AR §3.1]) and, for a `sync`, the sync commit's seq for the nodes `main`'s window changed. WP-13 and M2 confirm
    that the engine's by-reference expansion of sync windows sets the same values.
22. **No fast-forward** (§11.7). [AR §5a.7] always emits a merge commit; a merge whose source is already an ancestor of the
    destination appends nothing (`up-to-date`).
23. **`op restore`** (§11.11) is read as "every ref to its value at `seq`": refs created after it are deleted, refs deleted after
    it and still within the reflog window are restored. [AR §5a.5] says only "restores all refs to their values at `seq`".
24. **`undo --expect`'s default** is "the tip the client last read" ([AR §5a.5], N13f). The API keeps no per-client read memory:
    the front end passes that tip as `expect`, and an absent `expect` checks nothing.
25. **Lazy records written by hooks alone** (`SessionMark`, cursors, evidence rows) change no compared state and have no command at
    M0. M6 and M9 add them if a gate needs them in GT2 streams.
26. **`sync --refork`** ([AR §5a.3], optional, unexported lanes only) changes a lane's fork point; its logical effect on commit ids
    and the image is not stated in the design. It is left to M3's specification.
27. **`file revert`'s `stmt_sym`** is `revert`; [F06 §3.4] lists the file-verb words `mv`, `rm`, `add` and should add it.
28. **An empty `TX`** (§9.1): a block whose net changeset is empty and that emits no runtime record appends nothing, so no commit
    has an empty changeset; `TX { REOPEN t; SET t.done = true }` still commits its `reopen_count` increment ([F06 §7.8] NF-10).
29. **`HOLE(pack-digest-param)`** ([RULES/pack-classes]) is decided with its candidates: the CLI flag `--pack-digest`, the MCP
    `complete` parameter `pack_digest`, and the `result.v1` field `pack_digest` (additive, `string` or `null`). The API argument is
    `Complete.pack_digest` (§10.5). R-MODEL records the value in the table.
30. **[F06 §11]'s example message** `claim --start` is illustrative: `Claim` with `start` writes an empty message (§9.1).
31. **Conflicted keys in `state(ref)`** (§15.3): the key's own member holds the provisional value the node's row holds while
    the conflict stands, which is [F12]'s and [RULES/merge-table]'s; the conflict itself is listed under `conflicts`.
32. **Two layouts of one anchor record.** [F06 §7.5.3] and [F08 §10.3] both lay out the anchor record, with different member sets
    and enumeration numbers (`akind` 0–5 against `kind` 1–6). The API names members and values by name and is unaffected; the
    review settles one owner ([F06] open point 15, [F08] open point 37).
33. **Two value-tag registries.** [F06 §5.1] (tags 0–14, `absent`, `false`, `true` as tags) and [F08 §5.1] (type ids 1–13 with the
    bool in bit 7) number the closed type set differently. The API's JSON is by type and unaffected; the review settles one.
34. **The marker's `outcome`.** [F05 §9.5] carries `complete --outcome` in the `Marker` record, and [F11 §7]'s row keeps only the
    status (`done`/`cancelled`). The runtime snapshot (§15.7) shows the record's `outcome` for `settled` markers; [F11] should keep
    it in the row, or the snapshot shows the status only.
35. **Runtime-snapshot scope** (§15.7). The R4 evidence tables are excluded from the runtime digest because their answers are
    compared by subset consistency (§16.5); `intents` are included because `FsIntent` outcomes are durable facts ([AR §6.5]).
36. **Procedure yields** (§3.3, §10). [LQ/std §7.3] gives `tx.claim` the yields `lease, token, branch, expires` and
    `tx.complete` `task, status, ready`; the API's rows carry those columns first, in that order, and then members GT2 needs
    (`task`, `role`, `holder`, `anchor`, `run`, `reused`; `outcome`, `lease`, `settle_commit`, `changed_since_pack`). A `YIELD`
    in LQ text can name only [LQ/std]'s columns; WP-19 adds the others to [LQ/std §7.3] or states that the API's members are
    additive. Also for WP-19: [LQ/std §7.3]'s `tx.claim` default `$ttl = '15m'` is the production value of the key
    `lease.ttl-default`, which a store may change ([CFG]); the key governs (§10.1).
37. **`reclaim` without arguments** (§10.4). [CFG]'s `lease.reclaim-older-than` (default 30 min) is the bound of a `reclaim`
    given neither `--older-than` nor `--run`; an earlier draft of §10.4 required exactly one of them.
38. **`file revert` and plain history verbs over file-moving commits** (§11.10, §12.4). [40 §3.6] says `file revert` runs "the
    inverse filesystem operations" of a commit that carried `FsIntentDone`, and that a plain `revert` of such a commit warns.
    This chapter adds: an item removed without `--trash` cannot be restored and ends `missing`; the warning's name is
    `graph_only_revert` (text proposed for [F19 §10.4]: `warning[graph_only_revert]: <c8> moved files on disk; moirai file
    revert <c8> moves them back`), compared by GT2 because it depends only on the stream (§16.3).
39. **One store HLC over semantic records — conflict with [F16] P-36** (§6.2 CK-4). [F16] P-36 assigns every append-time HLC
    of a group — a local commit's `hlc`, an imported commit's `append_hlc` and every [F05 §9] field described as "HLC at
    append", `Checkpoint.append_hlc` and `Lazy.hlc` included — from `h_last`, "the greatest append-time HLC in the scanned
    log", so all of them are strictly increasing in log order ([OS/clock §7] states the same rule). Under that rule a class-I
    `Maintain` (a `Checkpoint` record) between two commits in one millisecond, or anywhere while the
    clock runs behind the HLC after a backward wall step (fault-model item 7), raises the counter of the next commit's `hlc`,
    which is hashed (canonical item 3): the commit id then depends on engine-internal maintenance timing, against
    [F17 §1.5] SP-1, and a lazy record lost in a crash makes the engine's later commit ids differ from the model's
    (the model cannot predict either). CK-4 therefore advances one sequence over the semantic durable records only; the other
    records carry a value without advancing it, which gives up P-36's strict order across record kinds but keeps I43′
    (`append_hlc` strictly increasing in `seq` order). Resolution requested from WP-16: P-36's `h_last` becomes the greatest
    HLC of the semantic durable records of CK-4 (the scanner knows each record's kind), or [F16] states another rule under
    which SP-1 and crash determinism hold, and this chapter then cites it. A rejected alternative: advance the injected wall
    clock by 1 ms before every store command, which makes counters restart per command but fails after a backward step.
    An earlier draft of CK-4 drew commits and non-commit records from separate maxima, which left the `hlc` of `Lease`,
    `Marker`, `Idem` and `RefUpdate` records, and so the runtime digest, unspecified.
40. **The session role lease has no branch** (§4.2 CX-2, §4.3 row 3, §10.1). [RULES/role-write-policy] WR-005 (citing
    [AR §7.3], "everything on any branch") says the orchestrator's session role lease "carries no branch, so its holder writes
    on any branch". [90 §4.1]'s Branch row lists "the presented lease's branch" without distinguishing lease kinds. This
    chapter follows WR-005: CX-2 skips a session role lease, and the explicit-branch refusal applies to task and run-scoped
    leases only; the `Lease` record still stores the minting branch because [F05 §9.4] field 13 is always present.
41. **`Claim` shapes** (§10.1). A task claim may name a `role` and a `run` (the dispatcher's bulk claim of [90 §7.1] step 2),
    which an earlier draft's "exactly one of `ids`, `next`, and `role` with `run` or `session`" excluded. The rule now separates
    task claims from role-lease mints by `ids`/`next`.
42. **`Apply` and run-scoped leases** (§9.4). Step 1 released the lease of a `none` entry and step 4 released every live
    run-scoped lease of the batch, so a run-scoped `none` entry would have been released twice with two reasons. Step 1 now
    releases only a TTL lease; a run-scoped one is released by step 4 (reason 6 `apply`).
43. **Schema item objects** (§9.8, §15.3). A field record of [F08 §8.5.2] has its own member `class` (the merge class), so the
    item class is carried as `item` (`kind`, `field`, `enum`, `edge`, `query`).
44. **The examples** (§19) were extended beyond the draft's list with `16-image.json` (group I), `17-query.json` (family R),
    `18-maintenance.json`, `19-schema-migrate.json` and `20-vcs-refs.json`, so that every command group and result family has at
    least one request and result. Every example value that the rules determine is computed; the rest are the placeholders §19
    lists. Values whose rules other chapters still leave open — the R4 observation values of `13-file-mv.json`, the `affected`
    sets of staged commits, the texts of errors whose templates [F19] has not frozen (`bad_ref_name`, `not_merged`) — are
    marked illustrative in their files.
