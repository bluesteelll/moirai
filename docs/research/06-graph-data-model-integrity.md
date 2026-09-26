# 06 — Graph data model, referential integrity, IDs and queries for moirai

*Research report. Lens: the graph data model of a combined task tracker and agent memory store, how references stay consistent, how nodes are identified, and how agents query the graph. Written 2026-09-25. No code was written. This file is the only one created.*

---

## 0. Conventions used in this report

Every external claim carries a URL. Claims are tagged:

| Tag | Meaning |
|---|---|
| **[MEASURED]** | A number that someone measured and published (benchmark, bug report with timings, thesis experiment). The source is named. Nothing was measured in this session. |
| **[CLAIMED]** | Stated by a vendor, maintainer, or paper without numbers I could check, or with numbers I could not reproduce. |
| **[DOC]** | Documented behaviour of a product (official docs or source code). |
| **[ESTIMATE]** | My own back-of-envelope calculation, shown with its inputs. |
| **[OPINION]** | My recommendation or judgement. |

Dates are the publication or filing dates shown on the source page when I read it. Versions are the versions the source named.

---

## 1. Executive summary

1. **Use a typed property graph with the schema stored as data.** Do not use RDF or a pure EAV store as the primary model. Each node gets a small fixed header (kind, status, priority, criticality, flags, revision, parent) and kind-specific typed fields declared in a versioned schema. Edges are binary and typed, with a small closed set of kinds and an optional property record. Relationships with more than two participants become nodes. Datomic-style datoms are still worth keeping, but only as the format of the change log, not as the query model. [OPINION, §3]
2. **Classify every edge kind as *structural* or *historical*.** A structural edge (`parent`, `blocks`, `duplicate-of`, `scoped-to`) must always point at a live node, and each kind has a declared policy for when its target is deleted (restrict, cascade, or drop-and-notify), in the style of Gel's `on target delete`. A historical edge (`derived-from`, `cites`, `supersedes`, `mentions`, `discovered-from`) may point at a deleted node and is shown as a tombstone. The DB records the delete in history, so soft delete is not needed to make it recoverable. [OPINION, §8; evidence: Gel docs, Datomic `retractEntity`, Beads `bd delete`]
3. **Store references in both directions, inside the same write transaction.** A forward list plus a reverse list per node (the equivalent of Datomic's VAET index) is what makes "node 40 was deleted and every referrer knows at once" cheap: the delete walks 40's reverse list and applies each edge kind's policy atomically. Text mentions such as `#40` in a body are parsed into `mentions` edges when the text is written, so text references are covered by the same mechanism. That avoids Beads' approach of rewriting other issues' text to `[deleted:ID]`. [OPINION, §8]
4. **IDs: small sequential integers that are never reused, handed out by a counter that sits outside the versioned data and is shared by every branch in the store.** An ID looks like `#40` (claimed ≈2 tokens, versus ≈24 for a UUID). Because every branch draws from the same counter, parallel branches cannot collide. Dolt does the same with a global auto-increment in single-server mode, and Postgres sequences work the same way. Never reusing a number gives the same protection as a slotmap generation counter, so no generation bits are needed. A 128-bit UUIDv7 `uid` is only needed if stores on different machines must merge; that decision belongs to the owner. **Do not make IDs hierarchical** (`bd-a3f8.1.2`): Beads and Task Master both have bugs caused by mixing identity with position. [OPINION + evidence, §9]
5. **Enforce acyclicity when an edge is written, using Pearce–Kelly dynamic topological order.** It runs over one precedence graph that combines `blocks` edges with child→parent completion edges. Add a rule that a node can never block one of its own descendants, and let a node inherit blockers from its ancestors only when those blockers come from outside the ancestor's subtree. That exact gap caused a real Beads deadlock (issue #6506, Sept 2026) that its cycle detector could not see. Merging branches can create cycles that neither branch had, so the merge must run a full O(V+E) check. [evidence, §7]
6. **Serialize all writes through one writer, and give readers snapshots.** Maintain derived state (count of open blockers, ready set, child rollups, stale flags) eagerly, and only for the nodes a change touches. Beads' public issue tracker has measured failures from the opposite choices: write skew in the `is_blocked` recompute under REPEATABLE READ (reproduced 263/263, with a 61-hour visibility gap in production), a graph-wide recompute that makes `bd close` take 5–24 s on 4,483 issues, and a recursive SQL CTE that takes 7.46 s where the direct query takes 0.19 s. [MEASURED by Beads users, §10]
7. **Knowledge nodes need provenance, a lifecycle, and staleness that propagates.** Supersession and retraction are edges plus a status change, applied atomically. Retracting a node marks everything that transitively depends on it (walking reverse `derived-from`/`cites` edges) as `suspect`; nothing is deleted. Citations can pin the revision they relied on (`#40@r7`), as the PLANFENCE protocol does, so staleness can be computed without an LLM. Recent benchmarks find that LLMs are poor at noticing stale memory on their own: the best model on STALE reached 55.2%. [MEASURED/CLAIMED, §11]
8. **Queries: a small set of purpose-built commands plus a GitHub/Linear-style filter syntax, not Cypher or Datalog.** Output should be line-oriented and compact, with `--json` as an option and field projection. Keep the MCP tool count very small. Claude Code now omits its own Task tools on newer models because tool definitions cost context. Text-to-Cypher execution accuracy was about 50% for GPT-4. [DOC/MEASURED, §12]

---

## 2. What existing systems do

### 2.1 Comparison table

| System (state as of 2026) | Node model | Hierarchy | Dependencies / links | IDs | Delete semantics | Notes |
|---|---|---|---|---|---|---|
| **Beads** (`bd`), repo moved to `gastownhall/beads`; v1.0.3 Apr 2026, 1.3.0-dev in Sept 2026 issues | One `Issue` struct with ~60 fields. Types: bug, feature, task, epic, chore, **decision**, message, molecule, gate, spike, story, milestone, event. Statuses: open, in_progress, **blocked**, deferred, closed, pinned, hooked. Also leasing fields (`LeaseExpiresAt`, `HeartbeatAt`), `RowVersion`, and compaction fields. [DOC](https://raw.githubusercontent.com/steveyegge/beads/main/internal/types/types.go) | `parent-child` dependency type, plus dotted child IDs (`bd-a3f8.1.1`, up to 3 levels) [DOC](https://beads.gascity.com/core-concepts/hash-ids) | ~20 dependency types. Blocking: `blocks`, `conditional-blocks`, `waits-for` (with `parent-child` counted for readiness). Association: `related`, `discovered-from`, `relates-to`, `duplicates`, `supersedes`, `replies-to`, `caused-by`, `validates`, `tracks`, `until`, entity types (`authored-by`, `assigned-to`, …). [DOC](https://raw.githubusercontent.com/steveyegge/beads/main/internal/types/types.go), [DOC](https://beads.gascity.com/cli-reference/dep) | Hash IDs `prefix-xxxx` built from title, timestamp and random salt. Default length 4, grows on collision. Replaced sequential IDs because of branch collisions. [DOC](https://beads.gascity.com/core-concepts/hash-ids) | Since v0.50, `bd delete` is a hard delete. It removes all dependency links in both directions and rewrites text references in directly connected issues to `[deleted:ID]`. It fails when dependents exist unless `--cascade` or `--force` (which orphans them) is given. [DOC](https://beads.gascity.com/cli-reference/delete) | Storage is Dolt (embedded or server). JSONL is export only. [DOC](https://github.com/steveyegge/beads) |
| **GitHub Issues** | Issue with issue types (GA Apr 2025) | Sub-issues: ≤100 per parent, ≤8 levels, cross-repo allowed [DOC](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/adding-sub-issues) | "Blocked by / blocking", GA 2025-08-21. ≤50 per relationship type. Search filters `is:blocked`, `blocked-by:` [DOC](https://github.blog/changelog/2025-08-21-dependencies-on-issues/) | Per-repo sequential `#N` from a central server, plus a global node ID | n/a | The changelog does not say whether cycles are prevented |
| **Linear** | Issue with team workflow states | Parent/sub-issues. Optional team-level auto-close: parent closes when all children are done, or children close when the parent is done. [DOC](https://linear.app/docs/parent-and-sub-issues) | blocks, blocked-by, related, duplicate. Mentioning an issue makes it "related". A resolved blocker's relation moves to "Related". Marking a duplicate moves the issue to a reserved Duplicate status and merges it. [DOC](https://linear.app/docs/issue-relations) | UUID internally, display `TEAM-123`. Moving the issue to another team changes the display ID; old IDs redirect. [CLAIMED via integration PR](https://github.com/QuackbackIO/quackback/pull/585) | Trash for 30 days. Auto-archive of closed issues is blocked while the parent or any child is open. [DOC](https://linear.app/docs/delete-archive-issues) | Good example of a display alias kept separate from a stable ID |
| **Jira** | Issue with a configurable hierarchy | Epic > issue > sub-task | Admin-defined link types, each with a name plus outward and inward descriptions ("blocks" / "is blocked by"). Clone links are created automatically. [DOC](https://support.atlassian.com/jira-cloud-administration/docs/configure-issue-linking/), [DOC](https://developer.atlassian.com/cloud/jira/platform/issue-linking-model/) | `KEY-123` plus a numeric internal ID | n/a | Each link has one direction, with a label for each end |
| **Claude Code Tasks** (the harness moirai must fit) | Task: subject, description, status (pending / in progress / completed), owner, blocks / blockedBy | none | Dependencies. Completing a task unblocks its dependents automatically. Claiming uses **file locking**. Stored at `~/.claude/tasks/{team}/`. [DOC](https://code.claude.com/docs/en/agent-teams) | local | n/a | Task tools are **on by default only for older models** (up to Opus 4.7 / Sonnet 4.6 / Haiku 4.5). They are left out on newer models because "the tools' definitions and reminders take up context". [DOC](https://code.claude.com/docs/en/tools-reference). Known limitation: "Task status can lag … which blocks dependent tasks" |
| **Task Master** (claude-task-master) | Task JSON: id, title, status, dependencies, priority, details, testStrategy, metadata | Subtasks with dotted IDs (`1.2`) | dependencies, with circular-dependency detection [DOC](https://github.com/eyaltoledano/claude-task-master/blob/main/docs/task-structure.md) | Sequential with dotted subtasks | n/a | Bug #795 (2025-06-16): the API reported subtask `1.3` as `1.1.3`, and status updates then failed [DOC](https://github.com/eyaltoledano/claude-task-master/issues/795) |
| **TerminusDB** v12.0.x, maintained by DFRNT | JSON documents with typed classes. Keys: `Lexical`, `Hash`, `ValueHash`, `Random`. `@subdocument` (owned, deleted with its parent), `@shared` (deleted at commit time once nothing references it), `@foreign` ("no referential integrity checking") [DOC](https://terminusdb.org/docs/schema-reference-guide/) | via subdocuments | Any property typed as a class reference | Keys derived from the schema | Subdocuments cascade. Shared documents are garbage-collected when unreferenced. | Schema migration operations are classed as *weakening* (backwards compatible, e.g. adding an optional field or an enum value) or *strengthening* [DOC](https://terminusdb.com/docs/schema-migration-reference-guide/). Git-like branches and merges. |
| **TypeDB 3.x** (rewritten in Rust) | Entities, n-ary **relations with roles**, attributes as independent values | via relations | Relations are first-class and n-ary | internal | "Relations without role players will be removed (no dangling relations)". Attributes with no owner are removed unless marked `@independent`. Opt-in `@cascade`. [DOC](https://typedb.com/docs/typeql-reference/data-model/) | Deletion keeps relations consistent automatically |
| **Gel (ex-EdgeDB)**. The company shut down and the team joined Vercel (announced 2025-12-02). Remains open source. [DOC](https://www.geldata.com/blog/gel-joins-vercel) | Object types with links | links | `on target delete`: `restrict` (default), `delete source`, `allow`, `deferred restrict`. `on source delete`: `allow`, `delete target`, `delete target if orphan`. Backlinks are **computed** (`.<link[is Type]`). [DOC](https://docs.geldata.com/reference/datamodel/links) | UUID | Per-link policy | The cleanest statement of per-edge delete policies |
| **Datomic** | Datoms `[e a v tx added]`. Indexes EAVT, AEVT, AVET, **VAET** (reverse references) [DOC](https://docs.datomic.com/indexes/index-model.html) | via ref attributes / `isComponent` | ref attributes | entity IDs | `:db/retractEntity` "retracts all the attribute values where the given entity id is either the entity or value, effectively retracting the entity's own data and any references to the entity as well. Entities that are components … are also recursively retracted." [DOC](https://docs.datomic.com/transactions/transaction-functions.html) | Reified transactions: the `datomic.tx` tempid lets you attach provenance to a transaction [DOC](https://docs.datomic.com/transactions/transaction-data-reference.html) |
| **Graphiti / Zep** | Entity nodes, fact edges, episodes | none | `EntityEdge` has `fact`, `episodes` (provenance), `valid_at`, `invalid_at`, `expired_at`, `reference_time`, and typed `attributes` [DOC](https://raw.githubusercontent.com/getzep/graphiti/main/graphiti_core/edges.py) | UUID | Invalidates rather than deletes: "sets their t_invalid to the t_valid of the invalidating edge" [DOC, arXiv 2501.13956](https://arxiv.org/html/2501.13956v1) | Bitemporal |
| **Mem0 graph** | Entities and relations | none | An LLM "update resolver" marks conflicting relations as not valid | UUID | Soft invalidation with a `valid=false` flag [CLAIMED](https://docs.mem0.ai/platform/features/graph-memory) | The LLM decides what is invalid, so it is not deterministic |
| **Basic Memory** | Markdown notes, `[category]` observations, `relation [[Note]]` links, optional schemas [DOC](https://docs.basicmemory.com/concepts/knowledge-format) | via links | Free-form relation names | Titles and permalinks | File deletes | Human-editable, but there is no enforced integrity |
| **lemmalog** (Rust, Datalog, MCP) | Facts with semiring provenance and bitemporal columns | rules | rules | n/a | "DRed-lite scoped recompute": retracting a fact clears and re-derives only the predicates that depend on it [CLAIMED](https://github.com/musibal/lemmalog) | Claims ~100 µs point lookups over 4M facts [CLAIMED] |
| **MemTX** (arXiv 2607.23929, July 2026) | Typed records (belief, summary, profile, index, tool action) with a derivation DAG | none | derived-from | n/a | Revoking a record walks its transitive descendants and dispatches on type: beliefs are revoked, summaries are quarantined for rebuild, tool actions are compensated or flagged [CLAIMED](https://arxiv.org/html/2607.23929v2) | An eight-state lifecycle |

### 2.2 Failure catalogue from Beads' public issue tracker

Beads is the closest prior system: a graph issue tracker for coding agents, backed by versioned SQL in Dolt. Its 2026 issue tracker contains the failure modes moirai should design out.

| # | Failure | Numbers | What moirai should do |
|---|---|---|---|
| [#6506](https://github.com/gastownhall/beads/issues/6506) (2026-09-12, bd 1.2.2) | A parent blocked only by its own children makes those children inherit its blocked state, so nothing ever becomes ready. `bd dep cycles` does not report it because the cycle mixes `blocks` and `parent-child` edges, and cycle detection skips `parent-child`. | deadlock | Run one precedence graph for cycle checks. A node inherits only blockers from outside its ancestor's subtree ("exogenous only"). §7 |
| [PR #5131](https://github.com/gastownhall/beads/pull/5131) (2026-09-18) | Parent→child `blocks` edges deadlock the same way. "After `bd dep reparent` the ID keeps its old dotted prefix while the edge moves", so dotted IDs can no longer be trusted as hierarchy. | n/a | Never encode hierarchy in the ID. §9 |
| [#5887](https://github.com/gastownhall/beads/issues/5887) (2026-08-20, 1.1.2) | The tree renderer had no visited set, and one cycle sent it into runaway recursion. | [MEASURED] **17,418 MB peak RSS and 119 s**, versus 75 MB and 0.21 s with the edge removed | Keep the invariant that the graph is acyclic, and still use visited sets in every traversal. |
| [#6716](https://github.com/gastownhall/beads/issues/6716) (2026-09-24, 1.3.0-dev) | Write skew. Two transactions close the two blockers of one issue concurrently. Under REPEATABLE READ each sees the other blocker as still open, so the stored `is_blocked` flag is never cleared. | [MEASURED] reproduced **263/263**; one production instance stayed invisible for **61 hours** | One serialized writer, or at least SERIALIZABLE isolation for derived-state updates. §10 |
| [#5939](https://github.com/gastownhall/beads/issues/5939) (2026-08-22) | Closing an issue recomputes `is_blocked` across the whole graph. | [MEASURED] 4,483 issues: closing a leaf takes 5–6 s, closing an issue with one dependent takes ~24 s, closing one with three dependents times out at ~10.3 s | Recompute only the affected nodes, in O(out-degree). §10 |
| [#6128](https://github.com/gastownhall/beads/issues/6128) (2026-09-01, Dolt 2.2.4) | `bd ready --parent` uses a recursive CTE over a materialized edge CTE, and Dolt rescans it for every row. | [MEASURED] **7.46 s vs 0.19 s** for 483 descendants; synthetic benchmark 1.26–2.28 s/op vs 32–36 ms/op | Keep an in-memory adjacency index. Traversals should cost microseconds, not depend on a query planner. |
| [#6105](https://github.com/gastownhall/beads/issues/6105) (2026-08-31) | `bd stats`, `bd list` and `bd ready` count from "three different universes". `--status=blocked` filters on a stored status that nothing ever writes, because blocking is tracked in a separate derived column. | n/a | Define each derived predicate in exactly one place. Never store a status that is really derived. |
| [#5308](https://github.com/gastownhall/beads/issues/5308) | `bd delete` removes the row from Dolt but leaves the JSONL record, which permanently wedges auto-export. | n/a | Keep a single source of truth. Exports are views, never a second store. |

The lesson for moirai [OPINION]: most of Beads' integrity bugs come from derived graph state (blocked flags, readiness, cycles) being maintained with SQL on top of a general-purpose versioned database, spread over many commands, under snapshot isolation. moirai is written from scratch, so the graph invariants and derived state can live in one engine module with a single writer.

---

## 3. Data model families

### 3.1 Options

| Family | Representative | Strengths | Weaknesses for moirai |
|---|---|---|---|
| **Property graph, schemaless** (label plus a property map per node or edge) | Neo4j / openCypher, ISO GQL (ISO/IEC 39075:2024, published 2024-04-11) [DOC](https://www.iso.org/standard/76120.html) | Flexible. Familiar to people and to LLMs. | A property map per node is expensive in RAM: a hash map with string keys on every node. Typos go unnoticed (`stauts`). No types to enforce invariants on. |
| **Property graph with schema** | PG-Schema (Angles et al., SIGMOD/PACMMOD 2023) [DOC](https://arxiv.org/abs/2211.10962), Gel, TypeDB | Typed fields make columnar storage possible, and invariants can be declared. | The schema has to evolve over time (§4). |
| **RDF / triples** | RDF 1.2 (Candidate Recommendation 2026-04-07; adds *triple terms*, i.e. statements about statements) [DOC](https://www.w3.org/TR/rdf12-concepts/) | A universal model. Reification exists for statement-level provenance. | IRIs are long, which is bad for RAM and for agent tokens. It is open-world, whereas moirai needs closed-world checks ("is task X blocked?"). Standard reification is verbose. |
| **EAV / datoms** | Datomic (`[e a v tx added]` plus 4 indexes), CozoDB (Datalog; time travel through a `Validity` key column [DOC](https://docs.cozodb.org/en/latest/timetravel.html)) | History and provenance are built in (the transaction is part of each fact). Reverse references come from VAET. Schema is data. Adding an attribute needs no migration. | Every field is a row stored in about 4 indexes. Reading a whole node means gathering its datoms. Harder to make cache-friendly in Rust than columns. |
| **Document store with references** | TerminusDB | Diffs and merges work per document. JSON is natural for agents. | References are fields, not first-class edges, so a reverse index is needed anyway. Delete semantics differ by document class (§2.1). |

### 3.2 Recommendation [OPINION]

A **typed property graph** whose schema is stored as data, laid out as:

* **Node header (fixed, columnar, dense, indexed by `NodeId`):** `kind:u8`, `status:u8`, `priority:u8`, `criticality:u8`, `flags:u16` (done, pinned, deleted, stale/suspect, has-dangling, claimed…), `rev:u32`, `parent:u32`, `created_tx:u32`, `updated_tx:u32`, `title:StrRef`, `body:BlobRef` (loaded lazily), plus derived counters (§10).
* **Kind-specific fields:** typed columns declared in the schema (bool, int, enum, timestamp, text, string-set, node-ref). Only kinds that declare a field store it. There is an escape hatch, an `extra` map for undeclared keys, which is not queryable by index and is flagged as a smell by `doctor`.
* **Edges:** binary, typed, uniquely keyed by `(src, kind, dst)` with set semantics, which makes merges idempotent. An optional property record per edge holds `created_tx`, `reason`, and `pinned_rev` for citations. Adjacency is stored in **both directions**.
* **Relationships with more than two participants are reified as nodes.** For example, a *decision* node links to the options it chose and rejected. This keeps edges small and uniform (TypeDB's n-ary relations are powerful, but they multiply engine complexity).
* **The change log uses datom-style records internally:** `(tx, node, field|edge, old, new)`. That is the natural input for versioning, diff, and "what changed since commit Z" (§12). The query model stays typed and columnar.

**[ESTIMATE] RAM for this layout.**

* Node header ≈ 48 bytes, with titles averaging ~60 bytes in an arena. Bodies stay on disk or mmap until read.
* An edge costs ≈ 8 bytes in each direction (u32 target plus kind, padded), so ≈16 bytes per edge.
* A large project with 100k nodes and 500k edges: 100k × (48 + 60) ≈ 10.8 MB, plus 500k × 16 ≈ 8 MB, totalling **≈ 19 MB** before indexes.
* A typical project with 5k nodes and 20k edges needs **< 1 MB**.
* Compare a schemaless property map per node (a `HashMap<String, Value>` with ~10 entries) at roughly 400–800 bytes per node, and datoms at ≈4 index entries per field.

These figures are not measured. The Rust implementation research should benchmark them.

### 3.3 Status: an enum, not a boolean [OPINION]

The brief uses a boolean "done" as its example. Use a **status enum grouped into categories**, and derive `done` from it:

* Tasks: `open`, `in_progress`, `done`, `cancelled`, `deferred`.
* A `resolution` field on closed tasks: `completed`, `wontdo`, `duplicate`, `superseded`, `obsolete`.
* **`blocked` is never a stored status.** It is derived. Beads #6105 shows what happens otherwise: a stored `blocked` status that nothing writes, alongside a derived `is_blocked` column.
* Linear groups workflow states into categories (backlog / unstarted / started / completed / canceled) and GitHub pairs `open/closed` with a close reason. Both follow the same pattern. *[Not re-verified in this session; see Linear's workflow docs at https://linear.app/docs/configuring-workflows.]*

---

## 4. Schema definition and evolution

| Approach | Pros | Cons |
|---|---|---|
| **A. Kinds and fields hard-coded as Rust enums and structs** | Fastest. Checked at compile time. | Adding a kind (for example `experiment`) means recompiling. Users cannot extend it. |
| **B. Schema as data, versioned in the DB (recommended)** | Schema changes are commits: they diff, merge and roll back like data. Kinds can be added per project. | Needs a migration story. Merging two branches that changed the schema differently is hard. |
| **C. Schemaless** | Zero friction | Agents invent field names. Nothing can be validated. |

**Recommendation [OPINION]: B, with a built-in core schema.** The core kinds (task, rule, note, decision, finding, question, area, summary) and the header fields are fixed in the engine (the fast path). Projects can add fields to kinds, add enum values, and add kinds. Two rules:

* Classify every schema change as **weakening** or **strengthening**, as TerminusDB does [DOC](https://terminusdb.com/docs/schema-migration-reference-guide/). Weakening examples: add an optional field, add an enum value, add a kind, widen a type. These apply instantly and merge freely. Strengthening examples: make a field required, remove an enum value, narrow a type, rename. These need an explicit migration operation that rewrites instance data inside the same commit, and a merge that includes one must be re-validated.
* Enum values are stored as small integers mapped through the schema. **Never reuse an enum value's integer**, for the same reason IDs are never reused.

---

## 5. Node kinds (proposed)

| Kind | Purpose | Kind-specific fields (beyond the header) | Status set |
|---|---|---|---|
| `task` | A unit of work. Subtasks are simply tasks with a parent, so no separate kind is needed. | `assignee`, `claim_lease_until`, `estimate`, `due`, `acceptance` (text), `labels` | open / in_progress / done / cancelled / deferred, plus `resolution` |
| `rule` | Normative project knowledge ("never X", "always Y") | `enforcement` (must / should), `applies_to` (area refs or path globs), `rationale` | active / superseded / retracted / archived |
| `note` | Descriptive knowledge or a gotcha | `applies_to` | active / superseded / retracted / archived |
| `decision` | ADR-like. Once accepted it is never edited, only superseded (Nygard 2011: "keep the old one around, but mark it as superseded") [DOC](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions) | `context`, `options` (via edges), `consequences` | proposed / accepted / rejected / superseded |
| `finding` | An observation or piece of evidence from an agent: a measurement, a bug root cause | `confidence`, `evidence` (text, file:line, command), `observed_git_sha` | active / refuted / superseded / archived |
| `question` | An open question to the owner or another agent | `asked_of` | open / answered (via an `answers` edge) / dropped |
| `summary` | Derived knowledge (compaction, a digest of an area) | must have ≥1 `derived-from` edge | active / stale (derived) / archived |
| `area` | A scope node: a subsystem, directory, or topic | `path_globs` | active / archived |

A **critical note** from the brief is a `note` or `rule` with `criticality = critical`. Criticality is kept separate from task **priority**: priority is about scheduling, criticality is about how important it is to surface the item. [OPINION]

---

## 6. Edge kinds, semantics, constraints and delete policy

"Structural" edges must point to a live node. "Historical" edges may point to a deleted node and render as a tombstone.

| Edge (src → dst) | Meaning | Class | Acyclic? | Cardinality | On **dst** deleted | On **src** deleted |
|---|---|---|---|---|---|---|
| `parent` (child → parent) | Decomposition | structural | **yes: a forest** | ≤1 parent per node | **restrict** by default. Options: `--cascade` (delete the subtree) or `--reparent` (children move to the grandparent). | drop the edge and update the parent's rollups |
| `blocks` (A → B: A must be done before B can start) | Precedence | structural | **yes, in combination with the hierarchy (§7)** | many-to-many | **drop and notify**: the edge is removed, B's count of open blockers is decremented, and B gets an event "blocker #A deleted" | drop the edge |
| `duplicate-of` (dup → canonical) | Dedup | structural | yes, chain length 1: the target must be canonical | ≤1 out | restrict, or re-point to the target's own canonical | drop |
| `supersedes` (new → old) | Knowledge replaced | historical | yes | many-to-many (a split or a merge of decisions) | keep as a tombstone ref | keep. The old node stays superseded; `doctor` warns. |
| `derived-from` (summary/finding → source) | Provenance of derived knowledge | historical | yes, by construction (derived things are newer) | many | keep as a tombstone. **Mark the src `suspect`** (§11). | n/a |
| `cites` (any → knowledge node, with optional `pinned_rev`) | "I relied on this" | historical | no requirement | many | keep as a tombstone. Mark the src `suspect`. | n/a |
| `discovered-from` (new task/finding → task being worked on) | Where the work came from | historical | yes, by construction | ≤1 out (typical) | keep as a tombstone | n/a |
| `relates` | Symmetric "see also" | historical | no (store one canonical direction) | many | keep as a tombstone | n/a |
| `refutes` / `verifies` (finding → finding/decision/rule) | Evidence | historical | no | many | keep as a tombstone | n/a |
| `answers` (note/decision → question) | Closes a question | structural | n/a | ≤1 active answer | restrict | drop. The question goes back to open. |
| `scoped-to` (knowledge → area) | Scoping | structural | n/a | many | restrict, or reassign to the parent area | drop |
| `mentions` (any → any) | Created automatically from `#N` in title or body text | historical | no | many | keep as a tombstone, rendered `#40 (deleted in c123)` | recomputed from text |

Notes:

* Beads puts its entity-reference edges (`authored-by`, `assigned-to`) in the same table as its dependency edges [DOC](https://raw.githubusercontent.com/steveyegge/beads/main/internal/types/types.go). For moirai I recommend fields for assignee and author, not edges, unless agents or people become nodes. [OPINION]
* Linear moves a resolved blocking relation to "related" [DOC](https://linear.app/docs/issue-relations). In moirai, a `blocks` edge whose source is done simply stops counting; it is kept so history can answer "what blocked this".
* The delete-policy vocabulary is taken from Gel's `restrict / delete source / allow / deferred restrict` [DOC](https://docs.geldata.com/reference/datamodel/links). Datomic's `retractEntity` is the "drop all references" extreme [DOC](https://docs.datomic.com/transactions/transaction-functions.html). TypeDB is the "no dangling relations" extreme [DOC](https://typedb.com/docs/typeql-reference/data-model/).

---

## 7. Enforcing acyclicity cheaply

### 7.1 Algorithms

| Algorithm | Bound (total over m insertions) | Practical |
|---|---|---|
| Naive: DFS from `dst` looking for `src` on every insert | O(m·(n+m)) | Fine for small graphs. The DFS can be bounded. |
| **Pearce–Kelly (PK)** dynamic topological order (JEA 2007) [DOC](https://whileydave.com/publications/pk07_jea/) | Asymptotically worse than the best known, but it only searches the "affected region" between the two endpoints' positions in the order | "Best for sparse digraphs and only a constant factor slower than the best on dense" [CLAIMED by the authors]. Implemented in **petgraph** as `acyclic::Acyclic` with `try_add_edge` returning `AcyclicEdgeError` [DOC](https://docs.rs/petgraph/latest/petgraph/acyclic/struct.Acyclic.html) (docs.rs latest 0.8.3), and in the `incremental-topo` crate [DOC](https://docs.rs/incremental-topo/latest/incremental_topo/) |
| Haeupler–Kavitha–Mathew–Sen–Tarjan (HKMST, TALG 2012) | O(m^{3/2}) sparse; O(n² log n) dense | Needs an order-maintenance data structure |
| Bender–Fineman–Gilbert–Tarjan (BFGT, TALG 2015/16) | O(m·min(m^{1/2}, n^{2/3})) / Õ(n²) | Simpler than HKMST-Sparse |
| Bernstein–Chechik (SODA 2018) | Õ(m√n) expected | Theoretical |

**[MEASURED]** In Sigurðsson's Chalmers MSc thesis (2016), all of these were implemented in C# and timed on edge insertions. HKMST-Sparse was fastest on very sparse graphs (0–2% density), and PK was fastest from medium to high density (35–80%). The conclusion: "the Pearce & Kelly algorithm stands out for its overall good performance over a wide variety of graph densities and exceptional simplicity." [source](https://publications.lib.chalmers.se/records/fulltext/248308/248308.pdf)

**Recommendation [OPINION]: PK.**

* Task graphs are sparse and small (10³–10⁵ nodes).
* PK keeps a topological order that moirai can reuse for free: topological listing, critical path (§10), and "what to do next" ordering.
* [ESTIMATE] Even a worst-case full DFS over 10⁵ nodes and 5·10⁵ edges is on the order of a millisecond in Rust. Typical PK affected regions are tiny.
* Bulk imports can skip per-edge checks and run one final Kahn sort. Beads does the same with `--no-cycle-check`, "bulk --file adds still run one final whole-graph check before commit" [DOC](https://beads.gascity.com/cli-reference/dep).

### 7.2 Which graph must be acyclic: combine the hierarchy with the blockers

Beads #6506 and PR #5131 show that checking `blocks` alone is not enough. The precedence relation agents actually rely on is:

* `blocks(A,B)`: A finishes before B starts. This is a precedence edge **A→B**.
* `parent(C,P)`: a container P completes after all its children. This is a completion edge **C→P**.
* Inheritance: if an ancestor P of C has an open blocker X, C cannot start. This is an implied edge **X→C**.

**Proposed invariants [OPINION]:**

1. `parent` edges form a forest. The check walks up from the new parent looking for the child, which costs O(depth). A depth cap such as GitHub's 8 levels [DOC](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/adding-sub-issues) is optional.
2. The graph `blocks ∪ {child→parent}` is a DAG, maintained with PK. This automatically rejects `blocks(P, descendant-of-P)`, which is exactly the deadlock that PR #5131 had to catch after the fact.
3. Inheritance is **exogenous only**: C inherits blocker X from ancestor P only if X is not inside P's subtree. This is the fix proposed in #6506: "A parent that is blocked only by its own children never darkens those children."
4. Containers (tasks with children) are never "ready to work". They become "ready to close" once every child is done, with auto-close optional, as in Linear. This removes the second path to deadlock.

### 7.3 Merges create cycles that neither branch had

If branch A adds `X blocks Y` and branch B adds `Y blocks X`, each branch is acyclic but the merged result is not. The same happens with a concurrent reparent: move B under A on one branch and A under B on the other. Kleppmann et al. solve the concurrent-move case for replicated trees by applying moves in timestamp order and skipping a move that would create a cycle [DOC](https://martin.kleppmann.com/papers/move-op.pdf).

For moirai, a merge must:

1. build the merged edge set;
2. run a full Kahn topological sort over the combined precedence graph and a forest check over `parent`, O(V+E) [ESTIMATE: ~ms at 10⁵ nodes];
3. report any cycle as a **merge conflict record**, not silently drop an edge. Dolt handles referential violations the same way (§8.3).

A deterministic automatic policy (for example, drop the later edge by transaction time) can be offered as an option. [OPINION]

### 7.4 Concurrency

Invariant checks and derived-state updates must see a serializable view. Beads #6716 is the concrete counterexample: each of two transactions sees the other blocker as open and neither clears the flag. **One writer at a time** makes every check trivially correct, with readers on MVCC snapshots. That matches a local tool with bursty, small writes, and it is what Claude Code's shared task list does (claims "use file locking") [DOC](https://code.claude.com/docs/en/agent-teams). [OPINION]

---

## 8. "Maximally synchronous" references

Requirement: when node 40 is deleted, every node that referenced it knows immediately.

### 8.1 Mechanisms compared

| Mechanism | What it guarantees | Cost | Verdict for moirai |
|---|---|---|---|
| **Cascade delete** | Nothing dangles, because referrers are deleted too | Can delete far more than intended; bad for knowledge | Only for `parent` with an explicit `--cascade` flag |
| **Restrict** | The delete fails while referrers exist | Friction for agents | Default for structural edges whose loss would be surprising (`parent`, `answers`, `scoped-to`) |
| **Drop the reference** (Datomic `retractEntity`, Beads `bd delete`) | No dangling edges, and referrers lose the edge atomically | Cheap with a reverse index. Loses the "it used to point at 40" information unless history is kept. | Good for `blocks`, because moirai has history |
| **Soft delete (`deleted_at`)** | Recoverable | "Leaks into code", weakens foreign keys, and deleted parents keep live children (Brandur Leach, 2022) [DOC](https://brandur.org/soft-deletion) | **Unnecessary**: moirai is versioned, so a hard delete in the current state is recoverable from history |
| **Tombstones** | A deleted ID resolves to "deleted in commit c by actor a, reason r" | One small record per deleted ID | Yes, as a *view over history* (§8.2), not as a live row that filters must remember to exclude |
| **Bidirectional adjacency, updated transactionally** | Every edge is visible from both ends, and the delete walks the reverse list | 2× edge storage [ESTIMATE ≈8 B per direction] | **Core mechanism** |
| **Reverse index** (Datomic VAET [DOC](https://docs.datomic.com/indexes/index-model.html); Gel backlinks are computed [DOC](https://docs.geldata.com/reference/datamodel/links)) | Same as above if maintained eagerly | Same | Same, and maintained eagerly |
| **Generation-tagged IDs** (slotmap: key = index + version; a removed key "stays removed, even if the physical storage … is reused"; the version wraps after 2³¹ reuses [DOC](https://docs.rs/slotmap/latest/slotmap/)) | A stale handle can never alias a new object | 4 extra bytes per handle | **Not needed for persistent IDs if IDs are never reused** (§9). Useful only inside the engine if in-memory slots are compacted. |
| **Detect dangling references on read** | A reader notices a missing target | Every reader pays for the check, and the stale state is visible until then | Keep only as a defensive fallback for text links from outside moirai |
| **Push notification to live subscribers** | Running agents learn about the delete without polling | MCP supports `resources/subscribe` and `notifications/resources/updated` [DOC](https://modelcontextprotocol.io/specification/2026-07-28/server/resources) | Optional. It is **not verified** whether Claude Code passes resource-update notifications to the model mid-turn. |

### 8.2 Proposed design [OPINION]

1. **Every reference is an edge.** That includes `#N` mentions in text, which are parsed into `mentions` edges when the text is written. There is no other way to reference a node.
2. **Deleting a node is one transaction:**
   1. check `restrict` policies on the node's incoming edges and fail with a list of blockers if any apply;
   2. apply `cascade` policies (with `--cascade`);
   3. apply `drop` policies: remove the edges and update counters in both directions (§10);
   4. keep historical edges. Their dst is now a dead ID;
   5. set `suspect` on the sources of `derived-from` and `cites` edges (§11);
   6. write a tombstone entry for the dead ID in the commit's change log;
   7. emit an event on each affected node's feed.

   After the commit, every referrer carries the change in its own state: a counter, a flag, an event. That is what "immediately knows" means in practice.
3. **Rendering a reference to a dead node** looks up the tombstone view: `#40 (deleted c812 by impl-agent: "dup of #52")`. Nothing is rewritten in other nodes' text, unlike Beads' `[deleted:ID]` [DOC](https://beads.gascity.com/cli-reference/delete).
4. **IDs are never reused** (§9), so a reference to `#40` can never silently resolve to a different node.

### 8.3 Across versions and branches

This scenario, "node exists on branch A, deleted on B", is a classic delete/modify conflict:

| Case on merge (A into B, where B deleted #40) | Proposed resolution |
|---|---|
| A did not touch #40 or its edges | The delete wins. Clean. |
| A **modified** #40 (fields) | Delete/modify conflict. By default the result is a conflict record. Policy options: `delete-wins` or `resurrect`. |
| A **added a structural edge** to #40 (for example, a new task blocked by #40) | Referential violation. Dolt's model: merges happen at the storage layer, so cascades are *not* run during a merge ("a merge processes all rows concurrently, therefore we do not have an order upon which to apply referential actions"). Violations go into `dolt_constraint_violations_<table>`, and committing with unresolved violations is refused unless forced [DOC](https://www.dolthub.com/blog/2021-07-20-merging-branches-with-foreign-keys/). moirai should do the same: record the violation, apply the edge kind's delete policy as a *suggested* resolution, and require it to be resolved or auto-resolved by an explicit flag. |
| A added a **historical** edge to #40 (a new summary derived from #40) | Allowed. It becomes a tombstone reference, and the new summary is marked `suspect`. |
| Each side's changes are fine alone but the merged precedence graph has a cycle | §7.3 |

---

## 9. IDs

### 9.1 Schemes compared

| Scheme | Example | Tokens in agent context | Merge-safe across branches? | Sortable / dense? | Evidence |
|---|---|---|---|---|---|
| Sequential integer | `#40` | [CLAIMED] "a 3-digit number is encoded by a single token" (so `#40` ≈ 2) | **Only if one allocator is shared by all branches** | Dense: direct array index, bitsets | BAML blog [link](https://boundaryml.com/blog/uuid-swap) |
| Short hash with adaptive length | `bd-a3f8` | [ESTIMATE] ~4–6 | Yes, probabilistically. Beads grows the length on collision. | No; needs a hash map | Beads [DOC](https://beads.gascity.com/core-concepts/hash-ids) |
| Hierarchical dotted | `bd-a3f8.1.2`, `1.2` | grows with depth | Children use per-parent counters, so concurrent children still collide | No | Beads, Task Master; **bugs**: PR #5131, TM #795 |
| UUID v4 / v7 | `0192…-7c3e-…` | [MEASURED by BAML on an OpenAI tokenizer] "Each UUID costs a whopping 24 tokens" | Yes | v7 sorts by time (RFC 9562: 48-bit Unix ms plus 74 random bits) [DOC](https://www.rfc-editor.org/info/rfc9562/) | |
| ULID | 26 chars in Crockford base32 | [ESTIMATE] ~15–20 | Yes | Time-sortable | |
| Content hash | TerminusDB `Hash` / `ValueHash` | long | Yes | No | Suits immutable documents only; a mutable task's content hash would change [DOC](https://terminusdb.org/docs/schema-reference-guide/) |
| Shortest unique prefix of a long ID | jj: `kxsz` | ~2–4 | Yes | No | jj computes the prefix against a *small active set* (`revsets.short-prefixes`), which keeps prefixes 1–2 characters long [DOC](https://jonathan-frere.com/posts/jujutsu-shortest-ids/) |

**Agents and identifiers.**

* [MEASURED, third party] BAML ran Claude Haiku on 200 items over 100 IDs. Raw UUIDs produced 29 and 68 errors in two runs. Integer IDs produced 7 and 5. UUIDs remapped to integers produced 5 and 6. [link](https://boundaryml.com/blog/uuid-swap)
* [CLAIMED, Anthropic, 2025-09-11] Replacing "arbitrary alphanumeric UUIDs" with semantically meaningful identifiers "or even a 0-indexed ID scheme" notably improves Claude's retrieval accuracy [link](https://www.anthropic.com/engineering/writing-tools-for-agents).
* I could not measure token counts with Claude's tokenizer in this session. The owner can check with Anthropic's token-counting endpoint.

### 9.2 Collisions on parallel branches

* Beads abandoned sequential `bd-1, bd-2` because they "caused frequent collisions when multiple agents or branches created issues concurrently" [CLAIMED, FAQ via search].
* Dolt keeps "a global auto_increment counter for all branches when running a single server" (since Aug 2022). It still recommends UUIDs because *clones* do not know about each other [DOC](https://www.dolthub.com/blog/2023-10-27-uuid-keys/). A 2026 doltlite issue shows the failure when allocation is per branch: two branches each insert a row, get the same `max(rowid)+1`, and the merge conflicts [DOC](https://github.com/dolthub/doltlite/issues/2684).
* Postgres sequences are deliberately non-transactional: a value handed out by `nextval` "is never rolled back", which leaves gaps [DOC](https://www.postgresql.org/docs/9.6/functions-sequence.html).

**Conclusion [OPINION].** moirai is a single local store per project. Its branches are internal to the DB, and all git worktrees point at one store. Under those assumptions the best choice is a **store-global, non-versioned, monotonic u32/u64 counter**:

* a node created on any branch takes the next number, and no other branch will ever take it;
* numbers are never reused, not even after a delete or after aborting a branch (gaps are fine);
* merges can never collide;
* `#40` stays short;
* arrays are indexed directly by ID (header columns, bitsets for ready, done and stale), which is the most RAM- and CPU-efficient layout available.

Where the store lives relative to worktrees is one of the owner's open questions (§15).

**Only if stores on different machines must merge:** also give every node a `uid: u128` (UUIDv7) as its global identity. On import, a foreign node gets a new local `#N`, recorded in an alias map (foreign store + foreign number → local number). Structured edges are remapped exactly. Text mentions are edges, so they can be remapped too. Linear follows the same pattern: a stable UUID, with a display identifier that changes when an issue moves team and redirects from the old value.

**Display rules [OPINION]:**

* the canonical form is `#40`;
* the kind is printed as a separate column, not baked into the ID, because a kind prefix breaks when a note is turned into a task;
* the hierarchy is shown as a computed path (`#7 › #40`) when useful, and never parsed back as identity;
* the CLI accepts `40`, `#40`, and a uid prefix.

---

## 10. Derived state maintained incrementally

| Derived value | Definition | Update rule | Cost per change |
|---|---|---|---|
| `open_blockers[n]` | Number of `blocks` in-edges whose source is not done | On X → done: for each out-edge `blocks(X,Y)`, `open_blockers[Y]--`. On reopen: `++`. On edge add/remove: ±1. | O(out-degree) |
| `ready` set (bitset) | kind=task ∧ status=open ∧ leaf (no open children) ∧ `open_blockers==0` ∧ no exogenous open blocker on any ancestor ∧ not claimed (or lease expired) ∧ `defer_until` ≤ now | Recompute membership only for nodes whose inputs changed. Check the ancestor condition by walking up the parent chain, which is O(depth ≤ 8). | O(out-degree + depth) |
| Rollups `children_total`, `children_done` | Direct children. Descendant totals optional. | Adjust along the ancestor chain on status change or reparent | O(depth) |
| `ready_to_close` for containers | All children done | from the rollup | O(1) |
| Critical path | Longest path over open tasks in the precedence DAG | **On demand**: a DP over the PK topological order restricted to a subtree | O(V+E) of the subtree |
| Orphans | Nodes left without a required edge (for example, knowledge with no `scoped-to` when the project requires scoping) | Maintained set, updated on edge and node deletes | O(1) amortized |
| `suspect` / stale | A source of a `derived-from`/`cites` edge was retracted, superseded, deleted, or moved past its `pinned_rev` | Propagate along reverse `derived-from`/`cites` edges (transitive closure over derivation only) | O(affected closure) |
| Dangling count | Number of historical edges pointing at dead IDs | Maintained on delete | O(1) |

Rules [OPINION]:

* **Each predicate is defined once, in the engine.** Every command and every MCP tool calls the same function. This avoids Beads #6105's three universes.
* **Derived values are never persisted as the source of truth.** Either recompute them on open, or persist them with a checksum and let `moirai doctor --verify` recompute and compare (Beads needed `bd recompute-blocked` because its derived values drifted).
* **Property tests** assert that incremental value = full recomputation after random operation sequences, including merges.
* A heavier alternative for later: general incremental view maintenance (DBSP, Budiu et al., VLDB 2023; it supports recursion [DOC](https://arxiv.org/abs/2203.16684)). It is overkill for the handful of fixed predicates above, but relevant if user-defined queries ever need to stay live.

---

## 11. Memory semantics for agent knowledge

### 11.1 Time

* **Transaction time comes free from versioning.** Every change sits in a commit with a timestamp, so as-of queries run against a commit. That is Graphiti's T′ timeline (`created_at` / `expired_at`) [DOC](https://arxiv.org/html/2501.13956v1). XTDB 2 also makes every table bitemporal by default [DOC](https://docs.xtdb.com/concepts/key-concepts.html).
* **Valid time is optional and only for facts about the world**, such as "the API was deprecated in lib v3". Use `valid_from` and `valid_to` fields on knowledge kinds. Graphiti's rule: on contradiction, set the old fact's `invalid_at` to the new fact's `valid_at`, and never delete.
* **Code-anchored validity [OPINION].** Most moirai knowledge is about the code, and it stops being true when the code changes. Record `observed_git_sha` and `applies_to` path globs. "Possibly stale" is then computed deterministically: the files under `applies_to` changed between `observed_git_sha` and HEAD. This is cheaper and more reliable than asking an LLM to judge. STALE (May 2026) reports that the best model scored "only 55.2% overall accuracy" on recognising outdated memory [CLAIMED/MEASURED in paper](https://arxiv.org/abs/2605.06527).

### 11.2 Supersession and retraction

* **Supersede** = a `supersedes(new, old)` edge plus `old.status = superseded`, written atomically. The engine refuses to accept one without the other.
* **Retract** = `status = retracted` plus a reason, used for things that were never true or were refuted, optionally with a `refutes` edge from a finding.
* **Propagation:** retract, supersede, or delete marks every node reachable by reverse `derived-from`/`cites` edges as `suspect`, with a pointer to the cause. Suspect nodes appear in `moirai stale`, and default queries render them with a warning. An agent clears the flag by re-confirming the node, which re-pins revisions, or by rewriting it.
* MemTX (July 2026) proposes the same typed cascade: beliefs are revoked, summaries are quarantined for rebuild, tool actions are compensated. It reports "zero downstream harm" in its suite [CLAIMED](https://arxiv.org/html/2607.23929v2). lemmalog's "DRed-lite scoped recompute" is the Datalog version of this [CLAIMED](https://github.com/musibal/lemmalog).
* **Pinned citations (PLANFENCE, arXiv 2609.03340, Sept 2026):** "Plans cite the exact public records they used, and an executor validates only the records that can affect the pending external action." In 30 controlled workflows, an executor that only checked freshness acted on the obsolete plan in every task, while PLANFENCE completed all of them without an invalid action [MEASURED in paper](https://arxiv.org/pdf/2609.03340). For moirai: `cites` edges carry `pinned_rev`, and `moirai check #task` verifies that nothing a plan depends on has moved.

### 11.3 Confidence, provenance, criticality, decay, scope

| Aspect | Proposal [OPINION] |
|---|---|
| Confidence | A small enum on `finding`/`note`: `verified` (backed by a `verifies` edge to a test or command), `observed`, `inferred`, `speculative`. An enum costs fewer tokens than a float and is harder for agents to game. |
| Provenance | Stored **per transaction**, not per node: actor (role or agent name), session ID, git SHA, and message are recorded once on the commit. Nodes store `created_tx` and `updated_tx` (u32). This is Datomic's reified-transaction pattern: "make assertions about the current transaction … the provenance of the data it added, or the user who caused it" [DOC](https://docs.datomic.com/transactions/transaction-data-reference.html). Also add `source` (file:line, URL, command) on findings. |
| Criticality | `critical / high / normal / low`. `moirai brief` always returns the active critical rules and notes for the current scope, within a token budget, which suits a SessionStart hook. |
| Decay and archival | `last_confirmed_tx` and `review_after`. `archived` hides a node from default queries but keeps it. Compaction produces `summary` nodes with `derived-from` edges; the Beads equivalent is its "memory decay" compaction fields. |
| Scope | `area` nodes with `path_globs` and `scoped-to` edges. `moirai notes --path src/net/x.rs` resolves path → areas → knowledge, ordered by criticality. |

---

## 12. Query surface

### 12.1 What agents actually ask for

From the brief and the Beads, Claude Code and Linear usage patterns above:

1. What can I work on? (`ready`, filtered by area or label, ordered by priority then topological order)
2. What blocks X, recursively, and why? (`blockers #X`, `why #X`)
3. Show X with a compact neighbourhood (`show #X`)
4. Subtree and rollup of X (`tree #X`)
5. Rules, notes and decisions relevant to area or path Y (`notes --path … | --area …`)
6. What changed since commit Z or time T, optionally by actor (`changes --since c812`)
7. Claim, release, done, reopen, with a lease (`claim #X --lease 30m`). Beads has lease fields; Claude Code uses file locks for claims.
8. Write memory: a critical note, a finding with evidence, a decision that supersedes an older one
9. Stale or suspect items to review (`stale`)
10. IDs only, for scripting: `moirai ready --ids`, or `moirai blockers --all --ids` for the brief's "ids of all blocking tasks"

### 12.2 Commands vs a query language

| Option | Pros | Cons |
|---|---|---|
| **Purpose-built commands** | Few tokens per call. Encodes the right semantics (exogenous blockers, suspect flags). Easy to cache and optimize. | Ad-hoc questions need new commands |
| **Filter syntax** (GitHub/Linear-like: `kind:task status:open prio:<=1 area:net label:perf blocked-by:#12`) | Familiar to LLMs from GitHub search. Composable. Parsing is trivial. | Not a graph language |
| **Cypher/GQL subset** | Expressive; GQL is the ISO standard | [MEASURED] Text2Cypher: GPT-4 execution accuracy **49.07% zero-shot and 50.42% 2-shot** [paper](https://arxiv.org/html/2412.10064v1). Newer models are likely better but unmeasured here. Adds a planner and a large grammar. |
| **Datalog** | Recursion comes naturally. Incremental maintenance is well studied (lemmalog, Cozo). | Few LLMs write it fluently. Rules also need a stratification story. |

**Recommendation [OPINION]:**

* v1 is **commands plus the filter syntax**. The traversals agents need (subtree, transitive blockers, reverse derivations) are built-in verbs with `--depth`.
* A read-only Datalog or GQL subset can come later as a power tool behind the same indexes. It should never be the main path for agents.

### 12.3 Keeping output token-efficient

* **Default output is one line per node**:
  `#40 task open P1 "Implement WAL" blockers:#12,#13 parent:#7 area:storage`.
  Use ASCII sigils only, because Unicode arrows cost more tokens.
* **`--fields` projection**, **`--ids` mode**, **`--limit` with a cursor**, and `--json` for scripts.
* Keep a concise and a detailed mode (Anthropic's example: a detailed tool response of 206 tokens versus a concise one of 72 [DOC](https://www.anthropic.com/engineering/writing-tools-for-agents)). Claude Code caps tool responses at 25,000 tokens by default [same source], so moirai must paginate.
* **TOON** (a token-oriented notation) [CLAIMED by its authors]: 39.9–42.6% fewer tokens than formatted JSON, with similar accuracy [repo](https://github.com/toon-format/toon). An independent study found the savings are often cancelled out by a "prompt tax" in short contexts [paper, Feb 2026](https://arxiv.org/abs/2603.03306). A fixed-column line format gets most of the benefit with no instruction overhead. [OPINION]
* **Linear MCP evidence:** one agent project replaced "the Linear MCP's full relations payload" with "one line per live relation — kind, id, state, title" for blocker checks [DOC](https://github.com/RationallyPrime/agent-affordances/pull/35) (the PR title mentions a 180k-token fallback, but no before/after numbers were published).
* **Keep the MCP tool count small.** Claude Code dropped its own Task tools from newer models' defaults because "the tools' definitions and reminders take up context" [DOC](https://code.claude.com/docs/en/tools-reference). Suggested set: `moirai_ready`, `moirai_show`, `moirai_find`, `moirai_write` (a batch of create/update/link/transition operations), `moirai_changes`, `moirai_brief`. A Bash-driven CLI loaded through a skill costs no context until it is used.

---

## 13. Proposed core data model (v0), with alternatives

### 13.1 Entities

```
Store
  counter: u64 (non-versioned, monotonic)       -- NodeId allocator shared by all branches
  schema: versioned (kinds, fields, enums)      -- weakening/strengthening changes
  commits: [Commit{ id, parents, ts, actor, session, git_sha, message, ops[] }]

Node (header, columnar, indexed by NodeId)
  kind, status, resolution?, priority, criticality, flags{done,deleted,suspect,pinned,claimed,has_dangling}
  rev (u32, ++ on every change), parent (NodeId|NONE), created_tx, updated_tx
  title, body (lazy), labels (interned set), kind-specific typed fields
  derived: open_blockers, children_total, children_done

Edge (src, kind, dst) unique; optional props {created_tx, reason, pinned_rev}
  kinds: parent | blocks | duplicate-of | answers | scoped-to        (structural)
         supersedes | derived-from | cites | discovered-from |
         relates | refutes | verifies | mentions                    (historical)
```

### 13.2 Invariants (checked on every write; re-checked at merge)

| ID | Invariant |
|---|---|
| I1 | A NodeId is unique across all branches of a store and never reused. |
| I2 | Every structural edge has live endpoints in the same version. |
| I3 | Historical edges may reference dead IDs. Those IDs resolve through the tombstone view. |
| I4 | `parent` forms a forest (≤1 parent, acyclic), with an optional depth cap. |
| I5 | `blocks ∪ child→parent` is acyclic. No node blocks one of its own descendants. Blockers are inherited from outside the subtree only. |
| I6 | `supersedes` is acyclic, and its target has status `superseded` in the same transaction. |
| I7 | The target of `duplicate-of` is canonical (not itself a duplicate). The source's status is closed/duplicate. |
| I8 | Status transitions follow the kind's state machine. `blocked` is never stored. |
| I9 | Derived counters and sets equal a full recomputation (debug assertions, `doctor --verify`, property tests). |
| I10 | Every mutation belongs to exactly one transaction, and that transaction carries provenance. |
| I11 | Fields conform to the schema version of their commit. |
| I12 | A merge result satisfies I1–I11 or carries explicit conflict records. Committing is refused while any conflict is unresolved. |

### 13.3 Alternatives and trade-offs

| Decision | Chosen [OPINION] | Alternative | Trade-off |
|---|---|---|---|
| Model | Typed property graph, schema as data | EAV/datoms plus Datalog | Datoms give history and flexibility for free, but cost ~3–4× RAM and CPU per field read. Chosen: datoms for the change log only. |
| Edge identity | `(src, kind, dst)` set | Edges with their own IDs | Multi-edges and edge-level history references become possible, but merges are harder and IDs are wasted. |
| n-ary facts | Reify as nodes | TypeDB-style n-ary relations | More expressive, more engine complexity. |
| Node IDs | Global sequential `#N`; `uid` only if stores sync across machines | Hash IDs (Beads) | Hash IDs are decentralized, but cost more tokens and RAM (hash map instead of array) and need prefix resolution. |
| Deletes | Hard delete in the current state, history keeps the node, per-edge-kind policies | Soft delete | Soft delete duplicates what versioning already gives and invites filter bugs. |
| Acyclicity | Pearce–Kelly on the combined precedence graph | DFS per insert, or check only at commit | DFS is simpler and adequate at small scale. PK also yields a topological order. |
| Derived state | Eager, local, single writer | Lazy per query (SQL/CTE) | Beads' measurements show lazy SQL recompute breaks down at a few thousand nodes. |
| Blocker inheritance | Exogenous inheritance only | No inheritance (Claude Code style) | No inheritance is simpler, but blockers on an epic then do not stop its subtasks. |
| Query | Commands plus the filter syntax | GQL/Cypher subset | More flexible, with lower agent accuracy and higher implementation cost. |
| Staleness | Deterministic (edges, `pinned_rev`, `observed_git_sha` versus changed files) | LLM judgement (Mem0 resolver) | LLM judgement handles semantic contradictions, but it is nondeterministic and costly. |

---

## 14. Anti-patterns checklist

* Hierarchy encoded in IDs (Beads PR #5131, Task Master #795).
* A stored `blocked` status alongside a derived blocked flag (Beads #6105).
* Graph-wide recompute on a local change (Beads #5939).
* Maintaining derived state under snapshot isolation without serialization (Beads #6716).
* Traversals without visited sets (Beads #5887: 17.4 GB).
* Graph traversal through recursive SQL CTEs (Beads #6128: 7.46 s vs 0.19 s).
* Rewriting referrers' text on delete, which loses history (Beads `[deleted:ID]`).
* Exports acting as a second source of truth (Beads #5308).
* Cycle detection that ignores some of the edge kinds used for readiness (Beads #6506).
* Unbounded tool payloads, and many MCP tools (Claude Code docs, Anthropic tool guidance).
* An LLM as the only detector of stale or contradictory memory (STALE: 55.2%).

## 15. Open questions (only the owner can answer)

See the structured output. In short:

* Must stores on different machines merge?
* Where does the store live relative to git worktrees?
* Must moirai branches mirror git branches?
* Default delete policy for `blocks`?
* Do agents and roles become nodes?
* Which knowledge kinds and statuses are wanted?
* Blocker inheritance semantics?
* Depth cap?
* Token budget for `brief`?
* Should valid time be supported, or only code-anchored staleness?

## 16. Other ecosystem facts noted during the research (relevant to "build from scratch")

* Kuzu (embedded graph DB) was archived on 2025-10-10 at v0.11.3 after its team was acquired by Apple. The LadybugDB fork is at v0.19.1 as of 2026-08-12 [CLAIMED by a secondary source](https://oneuptime.com/blog/post/2026-08-12-kuzu-archived-pin-0-11-3-fork-or-migrate/view).
* Gel is shutting down as a company, with Gel Cloud off by 2026-01-31 [DOC](https://www.geldata.com/blog/gel-joins-vercel).
* CozoDB's maintenance status is unclear.
* TerminusDB is maintained by DFRNT, at 12.0.7 per its releases page [CLAIMED via search](https://github.com/terminusdb/terminusdb/releases).

This churn is a weak argument in favour of owning the engine.

---

## Sources

* Beads: https://github.com/steveyegge/beads ; types.go https://raw.githubusercontent.com/steveyegge/beads/main/internal/types/types.go ; docs https://beads.gascity.com/core-concepts/hash-ids , https://beads.gascity.com/cli-reference/delete , https://beads.gascity.com/cli-reference/dep ; issues #6506 https://github.com/gastownhall/beads/issues/6506 , #5887 https://github.com/gastownhall/beads/issues/5887 , #6716 https://github.com/gastownhall/beads/issues/6716 , #5939 https://github.com/gastownhall/beads/issues/5939 , #6105 https://github.com/gastownhall/beads/issues/6105 , #6128 https://github.com/gastownhall/beads/issues/6128 , #5308 https://github.com/gastownhall/beads/issues/5308 ; PR #5131 https://github.com/gastownhall/beads/pull/5131 ; DoltHub blog 2026-05-29 https://www.dolthub.com/blog/2026-05-29-evolving-with-beads/
* GitHub: sub-issues https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/adding-sub-issues ; dependencies GA 2025-08-21 https://github.blog/changelog/2025-08-21-dependencies-on-issues/
* Linear: https://linear.app/docs/issue-relations , https://linear.app/docs/parent-and-sub-issues , https://linear.app/docs/delete-archive-issues ; team-move behaviour https://github.com/QuackbackIO/quackback/pull/585
* Jira: https://support.atlassian.com/jira-cloud-administration/docs/configure-issue-linking/ , https://developer.atlassian.com/cloud/jira/platform/issue-linking-model/
* Claude Code: https://code.claude.com/docs/en/agent-teams , https://code.claude.com/docs/en/tools-reference ; Anthropic tool-writing guide (2025-09-11) https://www.anthropic.com/engineering/writing-tools-for-agents
* Task Master: https://github.com/eyaltoledano/claude-task-master/blob/main/docs/task-structure.md , https://github.com/eyaltoledano/claude-task-master/issues/795
* TerminusDB: https://terminusdb.org/docs/schema-reference-guide/ , https://terminusdb.com/docs/schema-migration-reference-guide/ , https://github.com/terminusdb/terminusdb/releases
* TypeDB: https://typedb.com/docs/typeql-reference/data-model/ , https://typedb.com/blog/typedb-3-0-is-now-live/
* Gel: https://docs.geldata.com/reference/datamodel/links , https://www.geldata.com/blog/gel-joins-vercel
* Datomic: https://docs.datomic.com/indexes/index-model.html , https://docs.datomic.com/transactions/transaction-functions.html , https://docs.datomic.com/transactions/transaction-data-reference.html
* Dolt: https://www.dolthub.com/blog/2021-07-20-merging-branches-with-foreign-keys/ , https://www.dolthub.com/blog/2023-10-27-uuid-keys/ , https://github.com/dolthub/doltlite/issues/2684
* Graphiti/Zep: https://arxiv.org/html/2501.13956v1 (2025-01-20) ; https://raw.githubusercontent.com/getzep/graphiti/main/graphiti_core/edges.py
* Mem0 graph memory: https://docs.mem0.ai/platform/features/graph-memory ; Basic Memory: https://docs.basicmemory.com/concepts/knowledge-format ; lemmalog: https://github.com/musibal/lemmalog
* Agent-memory papers: MemTX https://arxiv.org/html/2607.23929v2 ; PLANFENCE https://arxiv.org/pdf/2609.03340 ; STALE https://arxiv.org/abs/2605.06527 ; TOKI https://arxiv.org/abs/2606.06240 ; agent-native memory study https://arxiv.org/abs/2606.24775
* Incremental cycle detection: Pearce–Kelly https://whileydave.com/publications/pk07_jea/ ; Sigurðsson thesis (2016) https://publications.lib.chalmers.se/records/fulltext/248308/248308.pdf ; petgraph Acyclic https://docs.rs/petgraph/latest/petgraph/acyclic/struct.Acyclic.html ; incremental-topo https://docs.rs/incremental-topo/latest/incremental_topo/ ; BFGT (TALG 12(2), 2015) https://www.semanticscholar.org/paper/A-New-Approach-to-Incremental-Cycle-Detection-and-Bender-Fineman/58be0a7a3051ed5228bf017adde820818424955b ; Bernstein–Chechik https://aaronbernstein.cs.rutgers.edu/wp-content/uploads/sites/43/2018/12/Dynamic-Cycle-Detection.pdf
* Replicated tree moves: https://martin.kleppmann.com/papers/move-op.pdf
* IDs: RFC 9562 https://www.rfc-editor.org/info/rfc9562/ ; BAML UUID experiment https://boundaryml.com/blog/uuid-swap ; jj short prefixes https://jonathan-frere.com/posts/jujutsu-shortest-ids/ ; slotmap https://docs.rs/slotmap/latest/slotmap/ ; Postgres sequences https://www.postgresql.org/docs/9.6/functions-sequence.html
* Soft delete: https://brandur.org/soft-deletion ; ADRs: https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions
* Query languages and formats: ISO GQL https://www.iso.org/standard/76120.html ; PG-Schema https://arxiv.org/abs/2211.10962 ; RDF 1.2 https://www.w3.org/TR/rdf12-concepts/ ; Text2Cypher https://arxiv.org/html/2412.10064v1 ; DBSP https://arxiv.org/abs/2203.16684 ; TOON https://github.com/toon-format/toon , https://arxiv.org/abs/2603.03306 ; Linear relations one-liner https://github.com/RationallyPrime/agent-affordances/pull/35
* MCP resources: https://modelcontextprotocol.io/specification/2026-07-28/server/resources ; XTDB https://docs.xtdb.com/concepts/key-concepts.html ; CozoDB time travel https://docs.cozodb.org/en/latest/timetravel.html
