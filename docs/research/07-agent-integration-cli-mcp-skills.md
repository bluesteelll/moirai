# 07 — How agents should talk to moirai: CLI, MCP server, Claude Code skills and hooks

*Research lens: the agent-facing interface layer. Date: 2026-09-25. Research only — nothing implemented.*
*Companion report: [01-boyko-workflow-roles.md](01-boyko-workflow-roles.md) (the owner's role pipeline and a per-role operation table). This report does not repeat it. It covers the mechanics: process model, protocols, hooks, concurrency, and output contracts.*

**Evidence labels** (same convention as report 01):

| Label | Meaning |
|---|---|
| **[M]** | Measured by me in this session on the owner's machine (Ryzen 9 5900HS, 16 logical CPUs, 15.4 GB RAM, Windows 11 build 26200, Windows PowerShell 5.1.26100, Git for Windows 2.54, Claude Code 2.1.281 desktop). |
| **[D]** | Documented in a primary source: a spec, official docs, a maintainer's release notes, or source. |
| **[C]** | Claimed by a third party: a benchmark or blog post I did not reproduce. |
| **[I]** | My inference or recommendation. |

---

## 0. TL;DR

1. **Build one Rust binary with several faces.** `moirai` is the CLI. `moirai daemon` is a per-repository single-writer store server. `moirai mcp` is a stdio MCP server that proxies to the daemon. `moirai hook <event>` handles Claude Code hooks. They share one core and one wire protocol: newline-delimited JSON-RPC, which the MCP spec itself recommends for custom byte-stream transports [D]. On Windows the transport is a named pipe; on Unix it is a Unix domain socket. [I]
2. **Offer both CLI and MCP. They are not rivals.**
   - The **CLI** is mandatory. SessionStart hooks cannot call MCP tools at launch [D]. Workflow scripts have no shell or filesystem [D]. Bash-capable agents need no schema tokens.
   - The **MCP server** is needed too:
     - 3 of the owner's 9 roles have no Bash tool, so they cannot run a CLI at all: architect, architecture-critic and researcher [M]. §5.4 lists every role's path.
     - Per-tool allowlists in agent frontmatter give per-role write permissions [D].
     - JSON arguments avoid Windows PowerShell 5.1's argument mangling, which I measured stripping quotes [M].
     - A PreToolUse hook can stamp agent identity deterministically [D].
   - The old "MCP costs 10–50k tokens" argument is mostly about bloated servers. Claude Code has deferred tool loading on by default [D]. A lean 11-tool moirai surface is about 4.9k characters of schema [M], roughly 1.2–1.5k tokens (estimate), and only about 216 characters of names load upfront.
3. **"Maximally synchronous" is a property of the database, not of the agent.**
   - Referential consequences commit atomically in the single writer: a delete writes a tombstone and fixes the back-references.
   - Every other process sees them on its next read.
   - An LLM can only learn about a change at its next turn. The fastest channels are: the next tool result (stale-reference markers plus a store `rev`), a `UserPromptSubmit` hook, a plugin **monitor** or the Monitor tool streaming `moirai watch`, and (research preview only) channels. MCP `resources/updated` notifications do **not** reach the model in Claude Code [D].
4. **Claims are leases.** Each lease has an id, a holder, a TTL, and a monotonically increasing **fencing token**.
   - Every mutation takes compare-and-set guards (`--if-rev`, `--if-status`, `--lease`).
   - Every write takes an **idempotency key**. This is required because a resumed Workflow run re-executes completed agents that come after a failed one [D].
   - Beads 1.3.0 (2026-09-15) converged on the same design: leases, heartbeat, reclaim, guards, and a dedicated exit code for guard mismatch [D].
5. **Use a single-writer daemon rather than cross-process file locking.**
   - `LockFileEx` locks are mandatory on Windows. An exclusive lock denies other processes both read *and* write, and release after a crash is "not immediate" [D].
   - A local named-pipe round trip costs ~20 µs p50 [M]. A process spawn costs ~15 ms [M], and one through Git Bash costs ~45–50 ms [M].
   - The daemon auto-starts, is elected by a lock file, exits when idle, and checks its version against the client. A `--no-daemon` direct mode stays as a fallback.
6. **Worktrees.** Keep one store per repository, located through `git-common-dir`, so all worktrees and lanes share live coordination state (Beads does the same [D]).
   - Knowledge written from a branch carries provenance (branch, commit, worktree, agent) and a `proposed` status. It is reconciled at merge time.
   - Coordination state (claims, task status) is never branch-local. Otherwise two worktrees can claim the same task.
7. **Hook set** (all exec-form command hooks calling the moirai binary; no Python or Bash wrappers):
   - `SessionStart`: a budgeted brief of at most ~8k characters. Claude Code caps it at 10,000 characters and spills the rest to a file the model is not asked to read [D].
   - `SubagentStart`: a role-specific pack injected *into the subagent* [D].
   - `PostToolUse(Agent)`: maps `agentId` to a task from the prompt marker.
   - `SubagentStop`: releases or flags leases and records `last_assistant_message`.
   - `PreToolUse(mcp__moirai__*)`: stamps `agent_id`, `agent_type`, and `cwd`.
   - `UserPromptSubmit`: a delta since the last brief.
   - `PreCompact` is not needed, because SessionStart fires again with `source=compact` [D].
8. **For Workflow scripts, prefer "orchestrator as dispatcher and single writer".**
   - The orchestrator claims tasks in bulk before launching.
   - It passes task ids, lease tokens, and a run id through `args`.
   - Agents return schema output (as they do today).
   - The orchestrator persists everything with one `moirai apply` batch keyed by `run:<id>/agent:<label>`.
   - Agents may still call moirai directly to read context packs.
9. **Ship it as a Claude Code plugin** containing skills, `hooks/hooks.json`, `.mcp.json`, and an optional `monitors/monitors.json`. Install the binary separately. Use rmcp 3.4.x, the official Rust SDK (Tier 1 since Aug 2026, 3.4.1 released 2026-09-23 [D]), dual-era, because Claude Code still speaks the legacy `initialize` handshake to stdio servers by default [D].

---

## 1. Pinned versions and facts (as of 2026-09-25)

| Item | Value | Source |
|---|---|---|
| MCP spec, latest | **2026-07-28**. Previous revision 2025-11-25. | [D] modelcontextprotocol.io changelog |
| rmcp (official Rust SDK) | **3.4.1**, 2026-09-23. 3.0.1 shipped 2026-07-29 targeting 2026-07-28. | [D] crates.io API |
| Rust SDK tier | Tier 1 (assessment issue #3179 closed; secondary source dates the promotion 2026-08-21) | [D] GitHub issue #3179, [C] digitalapplied.com |
| Claude Code | 2.1.281 observed (`AI_AGENT=claude-code_2-1-281_agent`). Docs reference features up to v2.1.280. | [M], [D] |
| Claude Code MCP client | v1 runtime (TS SDK 1.x) or v2 runtime (TS SDK 2.0 plus 2026-07-28). The v2 runtime is the default on ≥2.1.232 when flags are fetched. Stdio servers use the legacy handshake unless `MCP_PROTOCOL_NEGOTIATION=auto`. | [D] code.claude.com/docs/en/mcp |
| Beads (closest prior art) | v1.3.0, 2026-09-15 (leases, guards, `bd serve`, events journal). v1.3.1-rc.1, 2026-09-21. Repo moved to `gastownhall/beads`, 27.4k stars. | [D] GitHub releases |
| interprocess crate | 2.4.4 (2026-09-03). Cross-platform local sockets. | [D] crates.io |
| notify crate | 9.0.0-rc.5 (2026-08-30) | [D] crates.io |

---

## 2. MCP in September 2026

### 2.1 What changed in 2026-07-28, and why it matters for moirai

| Change (SEP) | What it means for moirai | |
|---|---|---|
| **Protocol-level sessions removed.** No `Mcp-Session-Id`. The `initialize` handshake is replaced by per-request `_meta` (protocol version, client capabilities, clientInfo). New `server/discover` (SEP-2567, SEP-2575). | A moirai MCP server must not keep hidden per-connection state. Anything stateful, such as a claim, must be an **explicit handle** passed as a tool argument. The spec's non-normative "Stateful Tools" section describes exactly lease-like handles: state the lifetime in the tool description, return expiry as a *tool execution error* the model can recover from. | [D] |
| **`subscriptions/listen`** replaces `resources/subscribe` and the HTTP GET stream. The client opts in to `toolsListChanged`, `promptsListChanged`, `resourcesListChanged`, and `resourceSubscriptions` (a list of URIs). On stdio, notifications are demultiplexed by `subscriptionId`. After a reconnect the client must re-send `listen`; the server keeps no subscription state. | This is the protocol path for push. It is useful for non-Claude clients and dashboards. In Claude Code it only drives list refreshes, not model context (§2.6). | [D] |
| **MRTR** (SEP-2322). Server-initiated requests (elicitation, sampling, roots) are replaced by `InputRequiredResult` plus a client retry with `inputResponses` and `requestState`. | Elicitation stays possible for human confirmations, such as "delete node with 30 dependents?", but Workflow runs allow **no mid-run user input** [D]. Prefer `--dry-run` and explicit `confirm` parameters. | [D] |
| **Tasks moved to an extension** (`io.modelcontextprotocol/tasks`, polling via `tasks/get`) (SEP-2663). | Not needed. moirai operations take milliseconds. | [D] |
| **Cacheable list results** (`ttlMs`, `cacheScope`). Deterministic `tools/list` order is a SHOULD, "to improve LLM prompt cache hit rates". | Keep the tool list static and ordered. Never make it dynamic per role; use client-side tool allowlists for that. | [D] |
| **Schemas loosened to any JSON Schema 2020-12.** `structuredContent` may be any JSON value (SEP-2106). | Output schemas are possible, but see the Claude Code caveat in §2.6. | [D] |
| **Deprecated:** Roots, Sampling, Logging (12-month window), HTTP+SSE transport. Log level is per request. | Log to stderr on stdio. Take the project directory from `CLAUDE_PROJECT_DIR` and tool arguments, not Roots. | [D] |
| **Error codes:** resource-not-found is now `-32602`. Codes `-32020..-32099` are reserved for the spec. | Use `-32000..-32019` for implementation-defined protocol errors. Put domain errors in `isError` tool results. | [D] |

### 2.2 Primitives: tools, resources, prompts

| Primitive | Controlled by | Fit for moirai | Recommendation |
|---|---|---|---|
| **Tools** | Model ("model-controlled") | All reads and writes an agent decides to make | Primary surface: ~11 consolidated tools (§9.3). |
| **Resources** | Application ("application-driven"). In Claude Code: `@`-mentions and auto-provided list and read tools [D]. | Stable, addressable views: `moirai://brief`, `moirai://node/{id}` (template), `moirai://task/{id}/tree` | Implement them. They are cheap and useful for humans (`@moirai://node/T-12`) and for other clients. |
| **Prompts** | User. In Claude Code they surface as slash commands [D]. | Orchestrator rituals: "plan this epic into a task graph", "reconcile after merge" | Optional. Skills cover the same need with more control (§4). |

### 2.3 Notifications and "maximally synchronous"

The spec gives three change signals [D]:

- `notifications/{tools,prompts,resources}/list_changed`: the *list* changed.
- `notifications/resources/updated`: one subscribed URI's content changed.
- Request-scoped `notifications/progress`.

None of them is delivered to the **model**. They go to the client application. Claude Code documents only that it refreshes tools, prompts, and resources on `list_changed` [D]. Its own comparison table says a standard MCP server is queried by Claude "during a task; nothing is pushed to the session" [D].

So "node 40 is deleted, every node that referenced it immediately knows" must be implemented in four levels [I]:

| Level | Who learns | Mechanism | Latency |
|---|---|---|---|
| L0 data | Every referencing node | The same write transaction tombstones node 40 and marks each incoming edge `dangling(40, rev)` (or removes it, per edge policy). Readers see all or nothing. | Inside one commit |
| L1 processes | Any CLI, MCP shim, or hook process | They read from the daemon, which is the single source of truth, so there are no stale client caches. Every response carries `rev`. | Next call. ~20 µs IPC [M]. |
| L2 observers | Long-lived watchers (`moirai watch`, MCP `subscriptions/listen`, dashboards) | The daemon's in-process event bus fans out committed events. | Sub-millisecond [I] |
| L3 models | The agent's context window | (a) The next moirai tool result prints `→ T-40 [deleted rev 812 by developer#2: "duplicate of T-39"]`. (b) The `UserPromptSubmit` hook injects "3 changes since your brief affect your claims". (c) A plugin monitor or the Monitor tool streams `moirai watch --for-session …` lines as notifications. (d) Channels, a research preview (§4.7). | The next model turn, typically seconds |

### 2.4 Transports

| Transport | Pros | Cons | Verdict |
|---|---|---|---|
| **stdio** (client spawns the server) | Zero configuration. Claude Code manages the lifecycle. Portable. `CLAUDE_PROJECT_DIR` and `CLAUDE_CODE_SESSION_ID` are passed in the environment [D]. | One server process per Claude Code session, so several processes touch the store. In Claude Code it stays on the legacy handshake by default [D]. | **Default**, as a *thin shim* that proxies to the daemon. |
| **Streamable HTTP** (localhost) | One process for every session. Claude Code's v2 runtime probes HTTP servers for 2026-07-28 by default and holds a notification stream [D]. Hooks can also be `type: "http"` [D], so no process spawn per hook. | Must validate `Origin`, bind to 127.0.0.1, and authenticate [D]. Needs port and token discovery. Loopback is still reachable by any local process. rmcp had a 40 ms SSE-framing floor until `json_response: true` shipped in v0.17.0 [C]. | Optional mode (`moirai daemon --http 127.0.0.1:PORT`) for people who prefer it. |
| **Custom: named pipe / UDS with stdio framing** | The spec says transports over byte streams "SHOULD reuse the stdio framing" [D]. Per-user ACLs. No port. Claude Code itself uses a named pipe for its messaging socket (`CLAUDE_CODE_MESSAGING_SOCKET=\\.\pipe\LOCAL\cc-msg-…`) [M]. | Claude Code cannot connect to it directly; the shim bridges. | **Internal transport** between the CLI, the shim, and the daemon. |

### 2.5 Elicitation, MRTR, and tasks: mostly avoid

- **Elicitation via MRTR** blocks the tool call until a human answers [D]. In Workflow runs there is "no mid-run user input" [D]. Use it only for interactive-session destructive operations, and always offer a non-interactive path such as `confirm: "delete T-40 and detach 30 edges"`. [I]
- **Tasks extension**: skip it. [I]

### 2.6 What Claude Code actually does with an MCP server

| Behaviour | Detail | |
|---|---|---|
| **Tool search (deferred loading) is on by default** | Only tool *names* and *server instructions* load at start. Schemas load through `ToolSearch`. Opt out per server with `alwaysLoad: true`, or per tool with `_meta["anthropic/alwaysLoad"]: true`. Requires a 4.5-generation or later model. | [D] |
| Description and instruction truncation | Each tool description and each server's `instructions` is truncated at **2,048 characters** by default. Put "when to use moirai" at the start of `instructions`. | [D] |
| Output limits | A warning above **10,000 tokens** and a default cap of **25,000 tokens** (`MAX_MCP_OUTPUT_TOKENS`). A per-tool `_meta["anthropic/maxResultSizeChars"]` can go up to 500,000. Overflow is persisted to a file. | [D] |
| **`structuredContent` caveat** | Open issues report that when a result has both `content[].text` and `structuredContent`, the CLI forwards **only `structuredContent`** to the model (#55677 closed "not planned"; #79944 open since 2026-07-21). | [C]/[D] (GitHub issues) |
| `list_changed` | Supported. On v2, a notification stream is held open, with reopen rules of 3 retries if it closes within 10 s, and about 6 h backoff after 5 reopens per hour. | [D] |
| Resources | `@`-mention autocomplete, auto-attach, and auto-provided list and read tools | [D] |
| Prompts | Surface as slash commands | [D] |
| Elicitation | A dialog that blocks the tool until answered | [D] |
| Environment for stdio servers | `CLAUDE_PROJECT_DIR` (stable project root) and `CLAUDE_CODE_SESSION_ID` (the ID the server was spawned with) | [D] |
| Subagents | Inherit the parent's MCP tools. They can be restricted with `tools: …, mcp__moirai__get` or `mcp__moirai__*`, or given inline per-agent servers through `mcpServers` [D]. Workflow agents reach session MCP tools through ToolSearch (workflow reference). | [D] |
| Long calls | Tool calls in the main conversation that run longer than 2 minutes are auto-backgrounded (not for subagents) | [D] |

**The `structuredContent` caveat has a design consequence [I]:** for model-facing tools, return **compact text only**, with no `structuredContent` or `outputSchema`. Alternatively make `structuredContent` itself the compact form. Otherwise the model receives verbose serialised JSON instead of the terse text you designed. Keep a `format: "json"` argument for programmatic callers.

### 2.7 Rust SDK choice

| SDK | Status | Notes |
|---|---|---|
| **rmcp** (modelcontextprotocol/rust-sdk) | Official. 3.4.1 (2026-09-23). Tier 1. Targets 2026-07-28 and stays compatible with 2025-11-25. Serves 2026-07-28 statelessly and has `legacy_session_mode` for older clients. | Tokio plus serde plus schemars. `#[tool]`, `#[tool_router]`, `#[tool_handler]` macros derive input and output schemas from Rust types. Features `transport-io` (stdio) and `transport-streamable-http-server`. Implements `subscriptions/listen`, the tasks extension, MRTR with HMAC-sealed `requestState`, and caching. [D] |
| rust-mcp-sdk (2.x) | Community. Claims 100% conformance on 2026-07-28. 1.x covers 2025-11-25. | [C] |
| turbomcp, pmcp | Community. pmcp negotiates the era per request. turbomcp runs interop tests against rmcp. | [C] |

**Overhead, as claimed by third parties:**

- Streamable HTTP load test (Feb 2026, 50 VUs, I/O-bound): rmcp at 4,845 RPS, 5.09 ms average latency, 10.9 MB RSS; Go at 3,616 RPS and 23.9 MB; Java at 194–368 MB [C].
- Stateless continuations versus sessions in rmcp: "not measurable against an in-process map lookup" on loopback [C].

For moirai, SDK overhead will be dwarfed by LLM turn latency. The real cost is **binary size and compile time from tokio**. [I]

**Recommendation [I]:**

- Use **rmcp** behind a thin internal trait, so the storage core has **no dependency on tokio or MCP**.
- Only the `moirai mcp` / `moirai daemon` front-ends link tokio. The hot `moirai` CLI path can be a small synchronous named-pipe client, which keeps spawn time near the ~15 ms floor measured for a 2.5 MB Rust exe [M].
- Build dual-era (legacy plus modern), because Claude Code talks legacy to stdio by default [D].

---

## 3. Anthropic's guidance, turned into moirai rules

| Guidance (source, date) | Rule for moirai [I] |
|---|---|
| Consolidate: "instead of `list_users`, `list_events`, `create_event`, consider `schedule_event`" (*Writing effective tools for agents*, 2025-09-11 [D]) | High-leverage verbs: `claim` also finds the next ready task; `complete` closes, unblocks, and returns newly ready ids; `brief` and `pack` give one-call context. Do not mirror CRUD endpoints. |
| Namespace tools by service and resource [D] | Claude Code already prefixes `mcp__moirai__`. Keep short verbs (`ready`, `claim`). For the CLI, use `moirai <verb>` with kind-specific aliases (`moirai rule …`). |
| Return meaningful context; agents handle natural-language identifiers better than cryptic ones [D] | Short, typeable, prefixed ids (`T-4k2`, `R-9f`) always printed **with a title**. No UUIDs in output. Accept unique prefixes. |
| `response_format` enum: concise was ~72 tokens versus ~206 for detailed in their example [D] | Every read takes `detail=concise` (default) or `full`. The CLI equivalent is `--full`. |
| Paginate, filter, and truncate with sensible defaults. Claude Code caps results at 25k tokens [D]. | `limit` default 20, `cursor`, and an explicit truncation footer: `… 42 more (cursor c_7q)`. No output may be silently cut. Aim for ≤ 8k tokens per result. |
| Errors should give "specific and actionable improvements, rather than opaque error codes" [D] | Every error names the fix: `T-12 is claimed by developer#3 (expires in 11m). Ready now: T-14 "…", T-15 "…".` |
| Minimal viable tool set: "If a human engineer can't definitively say which tool should be used…" (*Effective context engineering*, 2025-09-29 [D]) | About 11 MCP tools, with no overlapping semantics. The CLI can be richer, because its `--help` text is not in context. |
| Just-in-time retrieval with lightweight identifiers; hybrid of upfront CLAUDE.md and on-demand tools [D] | `brief` upfront (small), `get` and `pack` on demand. Never dump the whole graph. |
| Structured note-taking and memory outside the context; sub-agents return a 1–2k-token condensed summary [D] | `remember` for durable knowledge. Subagent outcomes are stored as nodes, and only ids plus a one-line summary flow back. |
| Tool definitions: 58 tools ≈ 55K tokens. Tool search saved 85%. Accuracy rose 49%→74% (Opus 4) and 79.5%→88.1% (Opus 4.5) (*Advanced tool use*, 2025-11-24 [D]) | Keep the surface small. Mark the hottest tools `alwaysLoad` and defer the rest. |
| Code execution with MCP: 150,000 → 2,000 tokens by keeping intermediate data out of context (2025-11-04 [D]) | `apply` batches and `--ids` pipelines. The orchestrator persists schema output in one call, instead of the model re-typing results into N tool calls. |
| Skills use progressive disclosure: metadata, then SKILL.md, then linked files. Scripts are cheaper than generated tokens (*Agent Skills*, 2025-10-16 [D]) | A small core skill plus linked reference files. Deterministic work (formatting, packing, budget accounting) lives in the binary, not in prose. |
| Memory tool (`memory_20250818`): client-side, `/memories` path, auto-injected "ALWAYS VIEW YOUR MEMORY DIRECTORY BEFORE DOING ANYTHING ELSE", path-traversal protection [D] | Optional later: a `moirai memory-tool` handler, so API-based agents outside Claude Code can use moirai as their memory backend. |

---

## 4. Claude Code extension points: capabilities and hard limits

### 4.1 Limits that shape the design

| Limit | Value | Consequence for moirai |
|---|---|---|
| Hook `additionalContext`, `systemMessage`, and plain stdout | **10,000 characters** per string. Overflow is saved to a file plus a 2,000-character preview, and Claude Code "doesn't ask Claude to read" it [D]. | `moirai brief` must self-budget to about 8,000 characters, with critical rules first and a footer like `dropped: 14 notes, 3 tasks → moirai brief --more`. |
| SessionStart `mcp_tool` hooks | **Skipped at launch** ("no MCP client context"). They only run after `/clear` or compaction [D]. | SessionStart must be a **command** hook, so the CLI is required. |
| `UserPromptSubmit` hook timeout | Lowered to 30 s. On timeout the context is discarded [D]. | Keep it fast, served by the daemon. |
| Command hook default timeout | 600 s [D] | Set explicit short timeouts (5–10 s). |
| Stop/SubagentStop continuation | Loop protection: `stop_hook_active` plus a cap of 8 consecutive continuations [D] | Claim-hygiene nudges should block at most once. |
| Skill listing | Each `description` plus `when_to_use` is capped at **1,536 characters**. The listing budget is about 1% of the context window [D]. | Front-load the trigger phrases. |
| Skill body lifetime | Stays in context across turns. After compaction it is re-attached at 5,000 tokens per skill, 25,000 in total [D]. | The core skill body should stay under ~1.5k tokens. Push details into linked files. |
| MCP tool description and server instructions | Truncated at 2,048 characters [D] | See §2.6. |
| MCP output | 10k-token warning, 25k default cap [D] | Paginate. |
| Bash tool result | About 30,000 characters inline for successes, ~10,000 for failures. **Exit 1 counts as failure** except for grep, rg, find, diff, test, `git diff`, and `git grep` [D]. | **An empty result exits 0**, unlike grep. Reserve non-zero codes for real errors. |
| Monitor tool | A deadline of 5 min by default, 30 min at most (10 min with `-p`) [D] | `moirai watch` has to be re-armed. Plugin monitors avoid this (next row). |
| Plugin monitors | A persistent background command for the whole session. Each output line reaches Claude as a notification. **Interactive sessions only.** Triggered `always` or `on-skill-invoke:<skill>` [D]. | The best "push into the orchestrator" path (§7.6). |
| Workflow runtime | No filesystem or shell in the script. At most 16 concurrent agents by default (`CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS` up to 256). Resume re-runs the first failed agent **and every agent started after it, even completed ones** [D]. | Idempotency keys are mandatory (§7.5). |
| Task tools (`TaskCreate` etc.) | Off by default on newer models since v2.1.268 [D] | Do not build on `TaskCreated`/`TaskCompleted` mirroring. Treat it as optional. |

### 4.2 Hooks relevant to moirai (verified payloads)

| Event | Useful input | Useful output | moirai use |
|---|---|---|---|
| `SessionStart` (sources `startup`, `resume`, `clear`, `compact`, `fork`) | `source`, `session_id`, `cwd`, `agent_type` (with `--agent`) | `additionalContext` (or plain stdout), `sessionTitle`, `watchPaths`, `CLAUDE_ENV_FILE` [D] | Brief. After `compact` it re-injects, so **no PreCompact hook is needed**. Beads made the same choice [D]. |
| `UserPromptSubmit` | Prompt | `additionalContext` [D] | "Since your last brief: T-40 deleted (was blocking your T-12); R-7 added (critical)". Print nothing if nothing changed. |
| `SubagentStart` | `agent_id`, `agent_type` (**no prompt**) | `additionalContext` injected **into the subagent** [D] | Role-specific rules and pack: critical rules that apply to `developer`, plus the moirai protocol line. Resolves report 01 §8.2's "not verified". |
| `PreToolUse` | `tool_name`, `tool_input`, `agent_id`, `agent_type`, `cwd` | `permissionDecision`, **`updatedInput`** (replaces the whole input), `additionalContext` [D] | Matcher `mcp__moirai__.*`: stamp `ctx: {agent_id, agent_type, cwd, session_id}` into the arguments, so attribution does not depend on the model. Note that returning `updatedInput` needs `allow` or `ask` [D]. |
| `PostToolUse` on `Agent` | `tool_input.prompt`, `tool_response.agentId`, `status` (`async_launched` is the default since v2.1.198, because subagents run in the background) [D] | `additionalContext` | Parse a `moirai:task=T-12 lease=L-9` marker from the prompt and record `agentId → task, lease`. |
| `SubagentStop` | `agent_id`, `agent_type`, `agent_transcript_path`, **`last_assistant_message`** [D] | `decision: block` + `reason` keeps the subagent running [D] | Look up the leases held by `agent_id`. If a lease is not completed, either block once ("call `moirai complete` or `release`") or auto-release it with the final message stored as a `needs-triage` note. |
| `Stop` | `last_assistant_message`, `background_tasks` | block once | Optional: "you still hold 2 leases". |
| `PostCompact` | `compact_summary` [D] | — | Optional, opt-in: archive the summary as a session-journal node. |
| `FileChanged` | `file_path`, `event` [D] | `watchPaths` | Not needed if the daemon notifies. Could watch an exported `.moirai/brief.md`. |
| `WorktreeCreate` / `WorktreeRemove` | `name` / `worktree_path` | Must print the path | **Do not use it for notification.** A WorktreeCreate hook **replaces** Claude Code's git worktree creation entirely [D]. |
| `TaskCreated` / `TaskCompleted` | `task_id`, `task_subject` | Exit 2 blocks | Optional mirror. Tools are off by default on new models (§4.1). |

Hook handler details that matter on Windows [D]:

- **Exec form** (`command` plus `args`) spawns the executable directly, with no shell and no quoting problems. On Windows it requires a real `.exe`.
- `shell: "powershell"` is available.
- `if: "Bash(moirai *)"` filters before spawning.
- `async` and `asyncRewake` run in the background. With `asyncRewake`, exit 2 wakes Claude with stderr as a system reminder.
- Hook types are `command`, `http`, `mcp_tool`, `prompt`, and `agent`.

### 4.3 Skills

Relevant `SKILL.md` frontmatter fields [D]:

- `description` and `when_to_use` (1,536-character cap)
- `disable-model-invocation`, `user-invocable`
- `allowed-tools` (a per-turn grant)
- `context: fork` plus `agent`
- `paths` (auto-activates on matching files)
- `hooks` (registered on invoke, persisting for the session, with `once`)
- `shell: bash|powershell`
- Inline `` !`cmd` `` / ` ```! ` **dynamic context**: the command runs before the content reaches Claude, under the Bash tool's 2-minute timeout.

Subagents can **preload** skills (`skills:` in agent frontmatter injects the full content) [D].

### 4.4 Subagents and worktrees

- `isolation: worktree` branches **from the default branch by default, not from the parent's HEAD** [D]. That matters for task provenance.
- A subagent's `cwd` is its worktree. The stdio MCP server's `cwd` and `CLAUDE_PROJECT_DIR` belong to the **session**, not the subagent [D]. So an MCP call cannot infer which worktree the caller is in: the PreToolUse stamp hook must supply it [I].
- `CLAUDE_CODE_SESSION_ID` in a workflow subagent's tool process equals the parent session id, and **no per-agent id variable exists** [M] (I inspected my own environment as a workflow subagent). Agent identity is only available to hooks (`agent_id`) or by self-declaration.

### 4.5 Plugins

A plugin bundles skills, agents, `hooks/hooks.json`, MCP servers, LSP servers, monitors, output styles, and default settings [D].

- **Executables in `bin/`** go on the Bash tool's `PATH`, after user entries [D]. However, claude.ai and Cowork will not install a plugin with a top-level `bin/` [D]. Plugin subagents ignore `hooks`, `mcpServers`, and `permissionMode` [D].
- MCP tool names from a plugin become `mcp__plugin_<plugin>_<server>__<tool>` [D]. That is long but harmless, since only names are listed.

### 4.6 Rules and auto memory (a cheap export channel)

- `.claude/rules/*.md` files with `paths:` frontmatter load **only when Claude works with matching files** [D].
- The project-root CLAUDE.md is re-read after `/compact`; path rules reload on matching reads [D].
- Auto memory loads the first 200 lines or 25 KB of `MEMORY.md` [D].

**Idea [I]:** `moirai export rules --to .claude/rules/moirai/` generates one file per path-scoped critical rule, as a *view*. Claude Code's own loader then does path-sensitive injection at zero hook cost. The rule text stays in moirai, and the export is regenerated on change (gitignored or committed, which is the owner's choice).

### 4.7 Channels (push into a live session)

- A channel is an MCP server with the `claude/channel` capability. Its events arrive in the model as `<channel source="…">` blocks in the **running** session [D].
- Channels are a **research preview**: they need `--channels` with plugins on an allowlist, or `--dangerously-load-development-channels` for your own. They require a claude.ai or Console login.
- A server that negotiates 2026-07-28 **cannot** be a channel [D].

**Verdict [I]:** do not depend on channels. Revisit when they are generally available.

---

## 5. CLI versus MCP

### 5.1 Evidence

| Source | Finding | Label |
|---|---|---|
| Zechner, *MCP vs CLI* (2025-08-15). 120 runs; terminalcp MCP vs CLI vs tmux vs screen. | Cost was similar ($19.45 MCP, $19.95 CLI, $22 tmux); success 100/100/100 (screen 67%). Tool design mattered more than protocol. In Claude Code, CLI calls pay for the bash-safety classifier (Haiku 35k tokens for MCP versus 1.3–2M for CLI variants). His conclusion: "just make a good CLI." | [C] |
| Scalekit (2026-03-11). `gh` versus GitHub's Copilot MCP (43 tools), Sonnet 4. | CLI used 1,365–9,386 tokens against 32,279–82,835 for MCP. MCP reliability was 72% (TCP timeouts to a *remote* server). The author admits schema bloat, not the protocol, drives the cost, and deferred loading was **not** tested. | [C] |
| Anthropic, *Advanced tool use* (2025-11-24) | 58 tools ≈ 55K tokens. Tool search cut this 85%. | [D] |
| Beads docs | "MCP tool schemas can add 10-50k tokens"; `bd prime` adds about 1–2k. Recommends **CLI + hooks**, with MCP for MCP-only environments. | [C]/[D] |
| Task Master | Tool tiers: core 7 tools ≈ 5K tokens, standard 15 ≈ 10K, all ≈ 21K | [C] |
| Ronacher, *Skills vs Dynamic MCP Loadouts* (2025-12-13) | Moved all his MCPs to skills plus CLIs; deferred loading "does not fix" his objections (schema churn, stripped docs). | [C] |
| **This session** | This harness itself runs with deferred MCP tools: dozens of servers visible by name only [M]. The sketched moirai surface of 11 tools is 4,924 characters of compact JSON schema (≈1.2–1.5k tokens, estimated at chars/4 and chars/3.3), and 216 characters as a names-only listing [M]. | [M] |

### 5.2 Windows-specific measurements [M]

**Process spawn cost** (PowerShell `&` operator loop; includes about 1–3 ms of PowerShell overhead):

| Invocation | n | min | p50 | p90 |
|---|---|---|---|---|
| Rust exe, 2.5 MB (`mdbook-regex --version`) | 40 | 11.8 ms | **14.9 ms** | 18.5 ms |
| Rust exe, 12 MB (`mdbook --version`) | 40 | 14.9 ms | 19.4 ms | 25.8 ms |
| `git --version` (mingw64) | 40 | 28.1 ms | 33.9 ms | 41.4 ms |
| `cmd /c exit` | 40 | 18.5 ms | 21.0 ms | 24.0 ms |
| Git Bash `bash -c true` | 20 | 29.0 ms | 52.1 ms | 59.0 ms |
| Git Bash → Rust exe | 20 | 40.5 ms | 44.1 ms | 83.5 ms |

**Named-pipe round trip:** a 64-byte JSON line echoed, with a .NET client and server in the same PowerShell process tree, n = 5,000. min 7.4 µs, **p50 19.5 µs**, p90 31.3 µs, p99 69.1 µs.

**Windows PowerShell 5.1 native-argument passing:** the argument `Critical: never call "unsafe" code in hot loop` arrived at a native exe as `Critical: never call unsafe code in hot loop` (quotes stripped). An empty-string argument was dropped entirely. This matches Microsoft's documentation of the pre-7.3 "Legacy" behaviour [D]. The owner's PowerShell tool is 5.1 [M].

**Implications [I]:**

- CLI calls cost about 15–50 ms of wall time. That is negligible against multi-second LLM turns, but it matters in hooks that fire on *every* tool call. Keep per-tool-call hooks narrow (`if:` filters, `async`).
- **Free text must never travel as argv** on Windows. The CLI takes bodies from stdin (`--stdin`, `-`) or `@file`. MCP's JSON arguments are immune.

### 5.3 What tool search changes

The classic MCP penalty is "N schemas in every request". It now applies only when tool search is off (for example a non-first-party `ANTHROPIC_BASE_URL`, or models older than 4.5) [D]. With tool search on, the costs are different [I]:

- **MCP:** about 216 characters of names upfront, plus ≤2,048 characters of server instructions, plus one `ToolSearch` round trip the first time each agent uses moirai. That round trip is one extra model step, so it costs latency, not much in tokens.
- **CLI:** zero schema tokens. The agent must *know* the commands, through a skill, CLAUDE.md, or the SessionStart brief (~1–2k tokens, which Beads also uses). Each call costs a Bash tool invocation. On Claude Code, that includes the command-safety classifier Zechner measured [C] and a Git Bash spawn on Windows [M].

### 5.4 Who uses which path (the owner's roles)

From the `.claude/agents/*.md` frontmatter [M]:

| Caller | Has Bash? | Recommended path |
|---|---|---|
| Hooks (SessionStart etc.) | n/a | **CLI** (exec form) |
| Workflow script body | No shell or filesystem [D] | None directly. Go through agents or the orchestrator (§8.3). |
| Orchestrator (main chat) | Yes | CLI for bulk and batch work (`apply`, `pack`, `--ids` pipes). MCP for quick typed calls. |
| developer, tester, code-reviewer, results-analyst, project-analyst, doc-writer | Yes | CLI through the core skill, or MCP. Both work. |
| **architect, architecture-critic, researcher** | **No** (tools: Read/Glob/Grep/Web*/[Agent]) | **MCP** with a per-role allowlist, e.g. `tools: Read, Glob, Grep, WebSearch, WebFetch, mcp__moirai__get, mcp__moirai__search, mcp__moirai__remember` [D]. Or orchestrator ingest of their schema output. |
| Non-Claude agents and IDEs | Varies | MCP (portable) or CLI |

**Verdict [I]:** one core, two faces. The CLI comes first, because hooks and bootstrapping need it. The MCP server follows immediately, because three roles and per-role write policy need it. Neither face may have features the other lacks for agent-relevant operations. Richer admin and VCS verbs can stay CLI-only.

---

## 6. CLI output contract for agents

### 6.1 Prior art

| Tool | Practice worth copying | |
|---|---|---|
| `gh` | `--json field,field` (listing fields when none are given), built-in `--jq`, `--template`. `NO_COLOR`, `CLICOLOR=0`, `GH_FORCE_TTY`, `GH_PROMPT_DISABLED`. | [D] |
| git porcelain | `--porcelain` "will remain stable across Git versions and regardless of user configuration". `-z` NUL-terminates entries. `v2` has typed header lines (`# branch.oid …`). | [D] |
| jj | A template language (`-T 'change_id ++ "\n"'`) with a `json()` function. Lock-free operation log (§7.2). | [D] |
| bd (Beads) | `--json` everywhere. A stable JSON contract with `schema_version`, an envelope `{schema_version, data, pagination:{returned,total,truncated}}`, error objects `{error, code, hint}`, snake_case, additive changes do not bump the version. **Exit code 13 = guard mismatch** only. RFC 9457 problem+json in `bd serve`. | [D] |
| Backlog.md | `--plain` (agent-oriented text) and `--json` are mutually exclusive | [C] |
| clig.dev | Primary output to stdout, messages to stderr. `--json`, `--plain`. Disable colour when not a TTY, with `NO_COLOR`, `TERM=dumb`, or `--no-color`. Never prompt when stdin is not a TTY; `--no-input`. | [D] |

### 6.2 Proposed moirai contract [I]

- **Default output is compact, line-oriented text** designed for LLMs.
  - The first line is a header, for example `rev 812 · 3 ready · scope T-1`. In lists it may be suppressed with `-q`.
  - Each record is one line: **id first**, then kind and status, then the title, then a few key fields, tab- or two-space-separated.
  - Example: `T-4k2  task  open  P1  "Wire lease reclaim"  blocked_by=T-3x9(done)`
  - Truncation is always explicit: `… 42 more · moirai ready --cursor c_7q`.
- **`--ids`** prints one id per line, for pipes: `moirai blocking --ids | xargs moirai show`.
- **`--json`** prints one object `{"v":1,"rev":812,"data":…,"next":"c_7q"|null,"dropped":{…}}`. **`--jsonl`** is for streams (`watch`, `changes`). The schema is versioned; additive changes do not bump `v`, following bd.
- **Errors** go to stderr, as text such as `error[lease_lost]: … hint: …`, or in JSON mode as `{"v":1,"error":{"code":"lease_lost","message":"…","hint":"…","current":{…}}}`. A failed compare-and-set always returns the **current** value, so the agent can retry without another read.
- **Exit codes:**

  | Code | Meaning |
  |---|---|
  | 0 | OK, including empty results |
  | 1 | Internal error |
  | 2 | Usage |
  | 3 | Not found |
  | 4 | Guard or compare-and-set conflict |
  | 5 | Lease lost or not the holder |
  | 6 | Precondition: blocked or open blocking review |
  | 7 | Store unavailable or locked |
  | 8 | Partial batch failure (details per item in JSON) |

- **No ANSI** unless stdout is a TTY and `NO_COLOR` is unset. No pager. **Never prompt.** Destructive operations take `--yes` or `--dry-run`.
- **Deterministic order** (by priority, then id) and stable formatting, for prompt-cache friendliness and diffability.
- **Input:** `--stdin` or `-` for bodies, `@path` for files, `--field k=v` for typed fields. JSON for `apply`.
- **Identity:** `--agent <label>`, defaulting to `$MOIRAI_AGENT`, then the hook-stamped value, then `session:<CLAUDE_CODE_SESSION_ID>`.
- **Worktree discovery without spawning git:** read the `.git` file (`gitdir: …`) and then `commondir`. This avoids a ~34 ms `git rev-parse` per call [M].
- **UTF-8 always.** Write bytes; do not rely on the console code page.

The owner's two example requests become:

- "Give me the ids of all blocking tasks": `moirai blocking --ids`. This means open tasks that have at least one outgoing `blocks` edge to an open task. Variants: `moirai blockers T-12 --transitive --ids`.
- "Write a critical note about the project into memory": `moirai rule --severity critical --stdin` with the text on stdin. MCP: `remember{kind:"rule",severity:"critical",text:…}`. Critical rules then appear first in every SessionStart brief and in every role pack whose scope matches.

---

## 7. Multi-agent concurrency

### 7.1 Who touches the store in the owner's workflow [I]

Several OS processes do, all at once:

- The orchestrator's MCP shim.
- Every hook invocation (a short-lived process).
- Every CLI call from up to 16 concurrent Workflow agents, or 20 concurrent subagents (the default `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` [D]). Some of these run in other worktrees.
- Possibly a second Claude Code session in another lane.
- Optionally a monitor running `moirai watch`.

Report 01 records three concurrent lanes observed on 2026-09-23 [C].

### 7.2 Options

| Option | How | Pros | Cons |
|---|---|---|---|
| **A. File locks per process** | Each CLI opens the store, takes `LockFileEx` (exclusive for write, shared for read), and does its own I/O | No daemon | On Windows, locks are **mandatory**: exclusive denies *all* other processes read and write on the region [D]. Every process rebuilds indexes (cost grows with the store). No push. "The time it takes for the operating system to unlock these locks depends upon available system resources" after a crash [D]. |
| **B. Database-native multi-process** | For example SQLite WAL (one writer, many readers, same host only [D]), or redb's in-progress multi-writer mode (PR #1462 merged 2026-09-06 [D]) | Proven where available | moirai's store is built from scratch, and multi-process MVCC with shared memory is a large project. It still gives no push. |
| **C. Single-writer daemon** | One process owns the store. Clients speak JSON-RPC over a named pipe or UDS. | Serialised writes make atomic claims trivial. One hot index in RAM (the minimal-RAM *total*). Push notifications are natural. ~20 µs IPC [M]. | Operational pain: stale or orphaned daemons, version skew after upgrade, antivirus, and "who starts it". Beads **removed** its daemon for simplicity and now uses embedded Dolt (single writer) or an external `dolt sql-server` for multi-writer [D]. |
| **D. Lock-free operation log (jj)** | Every command appends an operation. Concurrent operation heads are 3-way merged on load [D]. | No locks. Works over synced folders. | Merging is *after the fact*: two agents can both "claim" T-12 and the merge must pick one. That is wrong for leases, though good for knowledge. jj itself notes the git backend is not fully lock-free [D]. |

**Recommendation [I]:** **C + A-fallback, borrowing D for history.**

- The daemon is the default. It is auto-started by the first client, uses a lock file for election, exits when idle, and is version-checked.
- `--no-daemon` direct mode is for CI and recovery. It holds an exclusive `LockFileEx` write lock for the duration of the command, retries release with backoff, and runs read-only commands under a shared lock.
- The versioning core (commits, branches) can still use an append-only operation log internally, which is another lens's topic. Coordination operations are *serialised* by the daemon rather than merged.

### 7.3 Daemon lifecycle details [I]

- **Location:** `<git-common-dir>/moirai/` (shared by all worktrees), overridable with `MOIRAI_DIR`. The endpoint is `\\.\pipe\moirai-<hash(store path)>` with a per-user DACL, or `<store>/moirai.sock` on Unix. Tokio provides named pipes and the `interprocess` crate provides cross-platform local sockets [D]. Windows AF_UNIX (build 17063 and later, stream only) exists, but tokio exposes UDS only on Unix [C].
- **Election:** a client that fails to connect takes `LockFileEx(EXCLUSIVE|FAIL_IMMEDIATELY)` on `daemon.lock`. The winner spawns a detached `moirai daemon` (`CREATE_NO_WINDOW`) and every client polls-connects for ≤ 500 ms. The daemon holds the lock for its whole life.
- **Version skew:** the handshake carries a build hash. If the client is newer, the daemon drains and exits, and the client restarts it.
- **Idle exit** after N minutes with no connections, which minimises idle RAM. A connected MCP shim keeps it alive while any Claude session is open.
- **Crash safety:** the store's own write-ahead log is another lens's topic. Leases survive a daemon restart because they are persisted.

### 7.4 Claims: leases, fencing, compare-and-set, idempotency [I, grounded in D]

- **`claim T` is atomic in the single writer.** The task must be open, all blockers done, no open blocking review, and no live lease. The result is `{lease: "L-9", token: 1043, holder, expires_at}`.
  - **The fencing token** increases monotonically per store. `complete`, `update`, and `release` must present the lease, and a stale token fails with exit 5. Kleppmann's argument: leases alone are unsafe under pauses, so storage must reject writes carrying an older token [D].
  - **Claiming the same task again as the same holder is idempotent.** Beads does the same [D].
- **`claim --next --scope E --role developer`** is "pull from the ready queue", for self-claiming teams.
- **Liveness:**
  - A TTL, 15 min by default for LLM agents; Beads defaults to 5 min [D].
  - `heartbeat` on explicit call.
  - An **optional `async` PostToolUse hook** that auto-touches leases held by `agent_id`.
  - A check of `CLAUDE_PID` [M]: if the holder's Claude Code process is dead, the lease expires immediately.
  - `SubagentStop` releases or flags leases deterministically.
  - `reclaim --older-than 30m` for sweeps. Beads 1.3.0 added heartbeat and reclaim for the same reason: "a worker that dies mid-task no longer strands its bead" [D].
- **Compare-and-set guards on every mutation:** `--if-rev N`, `--if-status S`, `--if-holder H`. Beads has `--if-assignee`, `--if-status`, and `expected_version`, with exit code 13 reserved for "every failure was a guard mismatch" [D].
- **Idempotency keys** on every write (`--idempotency-key`, and `idempotency_key` in MCP). The daemon stores `(key → result)` for N days and returns the original result on a retry. This is **required**: Workflow resume re-runs agents after a failure [D], MCP stdio clients "SHOULD restart" crashed servers and retry lost requests [D], and models retry.
- **Serialising non-task resources** (for example "one agent runs benchmarks", or merge-queue work): a `mutex` node kind with the same lease semantics. Beads calls these "merge slots" [D].

### 7.5 Change propagation (implementation of §2.3) [I]

- **The daemon event bus** carries committed events `{rev, op, ids, actor, cause}`. Each event also lists the "affected" set, such as dependents newly unblocked or referrers of a tombstone. This mirrors Beads' events journal, where each mutation is written with a post-mutation snapshot in the same transaction [D].
- **`moirai watch [--for-agent A | --for-session S | --scope E] [--important]`** prints JSONL or compact lines. It is used as:
  - a **plugin monitor** (session-long, interactive only [D]) filtered to events relevant to the orchestrator: task unblocked, lease expired, verdict posted, critical rule added;
  - the **Monitor tool** for subagents (5–30 minute deadline [D]).
- **`changes --since REV`** is for re-sync after a pause. Every response carries `rev`, so an agent can pass `--since` without guessing.
- **MCP `subscriptions/listen`** exposes `moirai://node/{id}` and `moirai://brief` updates for non-Claude clients and UIs.
- **Hooks:** `UserPromptSubmit` sends a delta digest to the main session. `asyncRewake` can be used for "your lease was stolen / your task was deleted" alarms, since exit 2 wakes Claude [D].

### 7.6 Worktrees, branches, and provenance [I]

- **Shared store**, found through the git common dir. Beads says: "All worktrees in the same repository use the same beads workspace" [D].
- **Provenance on every write:** `session_id` (from the environment), `agent_id` and `agent_type` (from the hook stamp), `cwd`, the worktree, the git branch, and HEAD. The CLI reads the last three cheaply from the `.git` file; the stamp hook supplies them for MCP calls.
- **Coordination objects are global.** Leases, task status, and mutexes are never forked by branch.
- **Knowledge objects** (rules, decisions, findings) written from a non-default branch are created as `status: proposed, branch: feat/x`. Readers on that branch see them; readers on main see them only with `--include-proposed`. **Merge-time reconciliation** (§8.5) promotes or rejects them. Whether moirai-internal branches mirror git branches one-to-one is an open question for the owner (report 01 Q4; here Q3).

---

## 8. How the owner's harness would use moirai

The role pipeline is described in report 01 §5: researcher → architect ⇄ critic → developer(s) ⇄ reviewer → tester → results-analyst → merge. Report 01 §8.1 lists per-role operations. This section covers *orchestration mechanics*.

### 8.1 Session start, resume, and compaction

`SessionStart` runs the command `moirai hook session-start`, which reads the hook JSON from stdin and returns `additionalContext` of at most ~8k characters:

```
moirai rev 812 · lane ecs · you: orchestrator
CRITICAL RULES (3): R-7 "…"  R-2 "…"  R-11 "…"
OWNER QUESTIONS OPEN (2): Q-5 blocks T-4k2 …
IN FLIGHT (4): T-4k2 developer#1 lease 11m left · T-4k9 tester lease EXPIRED → reclaim?
READY (5 of 12): T-51 P1 "…" · T-52 P1 "…" · …
VERDICTS SINCE LAST SESSION: V-88 results-analyst REWORK→developer on E-3
dropped: 7 ready tasks, 12 notes → moirai brief --more
```

This replaces the hand-maintained Russian "resume checkpoint" at the top of `MEMORY.md` and the hand-written `HDR` blocks (report 01 §4). The same hook runs on `source=compact`.

### 8.2 Planning and phase transitions

- The architect's plan becomes a graph. The architect cannot write files, so it returns schema output or calls `mcp__moirai__remember`/`create`. Then the orchestrator runs `moirai apply plan.json`, which atomically creates the epic, subtasks with `blocked_by`, decisions, and section nodes. It uses local references (`"$t1"`) for intra-batch links.
- **Before each phase:** `moirai ready --scope E-3 --role developer --ids` gives the parallelisable set, and `moirai pack <T> --role developer --budget 12k` gives each agent's context. Report 01 names this the "pack". It replaces `${…}` interpolation and `.slice()` truncation, and carries explicit drop accounting.
- **Gates as data:** a critic or reviewer verdict is a `review` node with `verdict ∈ {APPROVED, CHANGES_REQUESTED, …}` and a `blocks` edge to the task. `complete` refuses (exit 6) while an open blocking review exists.

### 8.3 Dispatch patterns

**Pattern 1: dispatcher claims (recommended for Workflow scripts).** The orchestrator acts inline (the Workflow script cannot run commands):

1. `moirai claim T-51 T-52 T-53 --agent wf:<run>/dev#{1..3} --ttl 60m --json`, which returns the leases.
2. The Workflow is started with `args: {run, tasks:[{id, lease, pack_path?}]}`.
3. Each `agent()` prompt includes `moirai:task=T-51 lease=L-9`, which the PostToolUse(Agent) mapping and the SubagentStop cleanup rely on. The agent returns schema output.
4. After the run, the orchestrator writes all outcomes, findings, and verdicts atomically and idempotently with `moirai apply results.json --idempotency-key run:<run>`.
5. A retry after a resume therefore cannot duplicate anything.

This reuses the owner's existing habit: 199 of 347 scripts already use `schema` (report 01 [M]).

**Pattern 2: self-claim.** Used for agent teams, independent sessions, and long-lived lanes. Agents call `moirai claim --next --scope E --role developer --agent <label>`. Two agents cannot get the same task because claims are serialised in the daemon.

**Pattern 3: hybrid.** The dispatcher claims; agents *read* through moirai (`pack`, `get`) and *append* findings directly with `remember --idempotency-key <run>/<label>/<n>`. Agents with Bash use the CLI; architect, critic, and researcher use MCP.

### 8.4 Subagent reporting

| Moment | Mechanism | |
|---|---|---|
| Launch | `PostToolUse(Agent)` records `agentId → {task, lease}` from the prompt marker | Deterministic, no model cooperation |
| Start | `SubagentStart` injects the role rules and protocol line into the subagent | Deterministic |
| During | The agent calls `get`/`pack`/`remember`. The PreToolUse stamp adds `ctx`. | Model-driven, attribution deterministic |
| End | The agent calls `complete` with `summary` and `evidence` (commit, file:line, measurement id), or returns schema output for Pattern 1 | Model-driven |
| Safety net | `SubagentStop`: if a lease is still open, store `last_assistant_message` as a `needs-triage` note linked to the task, then release the lease or block once | Deterministic |

### 8.5 Merge-time reconciliation

`moirai reconcile --branch feat/x --into main` does four things:

1. Promotes `proposed` knowledge from `feat/x`, or lists conflicts for a human or architect to decide. Report 01 §8.3 asks for status fields to merge along a lattice.
2. Marks tasks `done-on-main` when their evidence commits are ancestors of main (`git merge-base --is-ancestor`).
3. Flags tasks whose worktree branch was abandoned.
4. Writes a `merge` event.

The trigger is an explicit orchestrator step, or a git `post-merge` hook (a git hook, not a Claude Code hook). [I]

### 8.6 End-to-end sequence

```
User → Orchestrator: "implement lease reclaim"
SessionStart hook  → moirai brief ────────────────▶ orchestrator context (≤8k chars)
Orchestrator       → Workflow(research, architect⇄critic) … agents return schema
Orchestrator       → moirai apply plan.json  (epic E-3, T-51..T-55, blocks edges, decisions)
Orchestrator       → moirai claim T-51 T-52 --agent wf:r7/dev#1,#2 → leases
Orchestrator       → Workflow(args={run:r7, tasks}) → agent(dev#1, isolation:'worktree')
   PostToolUse(Agent)? (Agent-tool launches only) / SubagentStart → inject rules + pack ref
   dev#1           → moirai pack T-51 --role developer (CLI) … edits … returns schema
   SubagentStop    → lease check / triage note
Orchestrator       → moirai apply results.json --idempotency-key run:r7
Daemon event bus   → monitor line "T-53 unblocked (T-51,T-52 done)" → orchestrator notified
… reviewer / tester / analyst phases as review/verdict nodes …
Orchestrator       → git merge feat/x ; moirai reconcile --branch feat/x --into main
```

**Unverified:** whether `SubagentStart`/`SubagentStop` fire for **Workflow `agent()`** calls, and whether `PostToolUse(Agent)` fires for them. Workflow agents are not spawned through the Agent tool. Needs a 5-minute experiment (§11 R3). Pattern 1 does not depend on it.

---

## 9. Recommended integration architecture

### 9.1 Process model

```
                      ┌────────────────────────── one binary: moirai(.exe) ──────────────────────────┐
 Claude Code session  │                                                                               │
  ├─ stdio ─▶ moirai mcp (shim, per session) ─┐                                                      │
  ├─ hooks ─▶ moirai hook <event> (exec form) ─┤  NDJSON JSON-RPC over \\.\pipe\moirai-<hash>          │
  ├─ Bash  ─▶ moirai <verb> (CLI) ─────────────┼──────────────▶ moirai daemon (single writer, per repo)│
  └─ plugin monitor ─▶ moirai watch ───────────┘                  │  store in <git-common-dir>/moirai/ │
 Other sessions / worktrees / CI ─▶ moirai … ──┘                  │  hot index, event bus, leases      │
                                                                   └─ optional: --http 127.0.0.1 (MCP)│
 Fallback: moirai --no-daemon <verb> → direct store access under LockFileEx                           │
                      └───────────────────────────────────────────────────────────────────────────────┘
```

**RAM [I]:** only the daemon holds caches. The CLI, shim, and hooks are small and transient, and the daemon exits when idle. rmcp servers have been reported at around 7–11 MB RSS in HTTP benchmarks [C]; the storage index dominates in practice (another lens).

### 9.2 CLI surface sketch [I]

```
# context
moirai brief   [--scope ID] [--role R] [--budget-chars N] [--more] [--format text|json]
moirai pack    ID --role R [--budget N]           # budgeted per-spawn context pack, with drop footer
# read
moirai ready   [--scope ID] [--role R] [--limit N] [--cursor C] [--ids]
moirai show    ID... [--full] [--neighbors N]
moirai find    [TEXT] [--kind K] [--status S] [--where 'done=false severity>=high'] [--ids]
moirai blockers ID [--transitive] [--ids]          moirai blocking [--scope ID] [--ids]
moirai tree    ID [--depth N]
moirai changes --since REV | --for-agent A          moirai watch [filters] [--jsonl]
# write (all accept --idempotency-key, --agent, --stdin/@file for text)
moirai add task "title" [--parent ID] [--blocked-by ID,..] [--field k=v].. [--role R]
moirai rule|note|decision|finding|review "text"|--stdin [--severity critical] [--about ID..] [--paths GLOB..] [--verdict V]
moirai set     ID [k=v..] [--status S] [--if-rev N] [--if-status S]
moirai link    ID --blocks|--parent|--relates|--supersedes|--evidence ID     moirai unlink …
moirai rm      ID [--detach|--cascade] [--dry-run] [--yes]     # prints the referrer impact report
moirai apply   FILE|-  [--idempotency-key K] [--dry-run]       # atomic batch (JSON/JSONL, local $refs)
# coordination
moirai claim   ID..|--next [--scope ID] [--role R] --agent A [--ttl 15m]
moirai heartbeat LEASE    moirai release LEASE    moirai reclaim --older-than 30m
moirai complete ID --lease L --outcome done|failed|abandoned --summary -|TEXT [--evidence X..]
moirai reopen  ID [--reason]
# versioning (surface only; semantics belong to the storage/VCS lenses)
moirai log|diff|branch|merge|checkout|reconcile …
# integration
moirai hook    session-start|subagent-start|subagent-stop|agent-launched|prompt|stamp|stop
moirai mcp     [--legacy|--modern|--auto]      moirai daemon start|stop|status [--http ADDR]
moirai export  rules --to .claude/rules/moirai/   moirai doctor [agents|hooks|store]
```

### 9.3 MCP surface sketch [I]

Eleven tools. Their schemas total 4,924 characters as compact JSON [M] (see §5.1). Compact text output; no `structuredContent` by default (§2.6).

| Tool | Purpose (description ≤ 2 lines, front-loaded) | Key params | Hints |
|---|---|---|---|
| `brief` | Digest at the start of work: ready tasks, your claims, critical rules | `scope`, `agent` | read-only, alwaysLoad |
| `ready` | Unblocked, unclaimed tasks | `scope`, `role`, `limit`, `cursor` | read-only, alwaysLoad |
| `get` | Nodes by id, concise or full, with neighbours | `ids[]`, `detail`, `neighbors` | read-only |
| `search` | Text and field-predicate search | `text`, `kind`, `status`, `where`, `limit`, `cursor` | read-only |
| `create` | Batch create with `parent` and `blocked_by` | `nodes[]`, `idempotency_key` | |
| `update` | Patch with compare-and-set guards. A failed guard returns the current value. | `id`, `set`, `status`, `if_rev`, `if_status`, `idempotency_key` | idempotent with a key |
| `claim` | claim, next, heartbeat, or release. Returns lease, token, and expiry; lifetime stated in the description. | `action`, `id`, `scope`, `agent`, `lease`, `ttl_s` | alwaysLoad |
| `complete` | Finish a claimed task. Returns newly ready ids. | `id`, `lease`, `outcome`, `summary`, `evidence[]`, `idempotency_key` | alwaysLoad |
| `remember` | Store a rule, note, decision, finding, or review | `kind`, `text`, `severity`, `about[]`, `paths[]`, `idempotency_key` | alwaysLoad |
| `link` | Add or remove an edge | `op`, `from`, `rel`, `to` | |
| `changes` | Changes since a revision, or history of one node | `since_rev`, `id`, `limit` | read-only |

Plus:

- **Every tool** accepts an optional `ctx` object, filled by the stamp hook. The server trusts it only when it comes from the hook path, not from the model: the hook adds an HMAC with a per-session secret from the daemon [I].
- **Resources:** `moirai://brief`, `moirai://node/{id}` (template), and `moirai://task/{id}/tree`, with `subscribe` and `listChanged`.
- **Prompts (optional):** `plan-epic`, `reconcile`.
- **Server `instructions`** (≤ 2,048 characters, front-loaded): "moirai = this repo's task graph + rules/decisions memory. Call `brief` when starting work; `claim` before working a task; `complete` when done; `remember` for rules/decisions/findings. Ids like T-4k2 …"
- **Per-role least privilege**, using agent `tools:` lists [D]. For example, the critic gets `mcp__moirai__get, mcp__moirai__search, mcp__moirai__remember`. The **daemon enforces** a role write policy on the stamped `agent_type` (report 01 §8.3 item 1). The client allowlist is convenience; the server check is enforcement.

### 9.4 Skills [I]

| Skill | Invocation | Content |
|---|---|---|
| `moirai` (core) | Model-invocable. Description: "Project task graph + rules/decisions memory. Use when starting or finishing a task, when asked what is ready or blocked, or when recording a rule/decision/finding/critical note." | About 60 lines: the verbs, the output conventions, "always pass `--agent`", "bodies via `--stdin`", exit-code meanings, and one example per verb. Links to `reference.md` for the full CLI. Optional `` !`moirai brief --budget-chars 3000` `` dynamic context. |
| `moirai-orchestrate` | User-invocable, and preloaded into the orchestrator if it runs as an `--agent` | The dispatch patterns (§8.3), ingest format, verdict routing, and the reconcile ritual |
| `moirai-report` | Preloaded into developer, tester, and reviewer through `skills:` [D] | Five lines on how to finish: `complete` fields and evidence format, `remember --kind finding` |

### 9.5 Hooks (plugin `hooks/hooks.json`, exec form) [I]

```json
{
  "hooks": {
    "SessionStart":   [{"hooks": [{"type": "command", "command": "moirai", "args": ["hook", "session-start"], "timeout": 10}]}],
    "UserPromptSubmit":[{"hooks": [{"type": "command", "command": "moirai", "args": ["hook", "prompt"], "timeout": 5}]}],
    "SubagentStart":  [{"hooks": [{"type": "command", "command": "moirai", "args": ["hook", "subagent-start"], "timeout": 10}]}],
    "PostToolUse":    [{"matcher": "Agent", "hooks": [{"type": "command", "command": "moirai", "args": ["hook", "agent-launched"], "async": true}]}],
    "SubagentStop":   [{"hooks": [{"type": "command", "command": "moirai", "args": ["hook", "subagent-stop"], "timeout": 10}]}],
    "PreToolUse":     [{"matcher": "mcp__moirai__.*", "hooks": [{"type": "command", "command": "moirai", "args": ["hook", "stamp"], "timeout": 5}]}]
  }
}
```

Notes:

- The PreToolUse matcher needs `.*`, because an exact-character matcher like `mcp__moirai` matches nothing [D].
- The stamp hook returns `permissionDecision: "allow"` plus `updatedInput`, which auto-approves moirai calls. Deny rules are still evaluated [D]. If the owner wants prompts, return `"ask"`.
- Whether `"command": "moirai"` resolves `moirai.exe` through PATH on Windows exec form must be verified. The docs only say a real `.exe` is required. Otherwise use an absolute path.
- Stamping the **CLI** (rewriting `Bash` commands with `updatedInput`) is possible but fragile with pipes. Prefer the SubagentStart-injected label for the CLI, plus the MCP stamp for attribution-critical writes.

### 9.6 Packaging [I]

- Plugin `moirai`: `skills/`, `hooks/hooks.json`, `.mcp.json` (`moirai mcp`), and `monitors/monitors.json` (`moirai watch --for-session ${CLAUDE_CODE_SESSION_ID}` if the variable expands; otherwise the monitor reads the variable itself).
- The binary is installed separately (`cargo install`, a release zip, later winget), because plugin `bin/` blocks claude.ai and Cowork installs and multi-OS binaries bloat the plugin [D].
- `moirai doctor hooks` validates the setup. It would also catch agent-frontmatter parse failures, like report 01's `results-analyst` finding.

### 9.7 Trade-offs and rejected alternatives

| Decision | Chosen | Rejected | Why |
|---|---|---|---|
| Interface | CLI plus MCP from one core | MCP-only; CLI-only | Hooks and bootstrapping need the CLI [D]. Three roles have no Bash, and per-role permissions need MCP [M][D]. |
| MCP transport to Claude | stdio shim | HTTP daemon only | Zero configuration, lifecycle managed by Claude Code, no port or auth. HTTP stays optional. |
| Store access | Single-writer daemon with direct-mode fallback | Per-process LockFileEx; lock-free merge | Atomic claims, one index, push. Windows mandatory-lock semantics [D]. |
| Output to model | Compact text, ids first | JSON by default; `structuredContent` | Tokens [D], and the Claude Code `structuredContent` quirk [C/D]. |
| Push to model | Hooks plus plugin monitor | Channels; MCP resource subscriptions | Channels are preview and gated [D]; resource updates don't reach the model [D]. |
| Workflow persistence | Orchestrator batch `apply` with idempotency | Every agent writes | Resume re-runs agents [D], and interpolation truncation (report 01). |
| Claim liveness | TTL, PID check, SubagentStop, optional async heartbeat | Heartbeat-only | LLM agents have no timers. Stop hooks are deterministic [D]. |
| PreCompact hook | None | Snapshot on PreCompact | SessionStart(compact) re-injects [D]. |
| Worktree mapping | Shared store plus provenance plus `proposed` knowledge | Store per worktree or branch | Coordination must be global. Knowledge follows code. |

### 9.8 Phasing [I]

1. **P0:** CLI in direct mode, core skill, SessionStart hook, compact output contract, idempotency keys.
2. **P1:** daemon with auto-start, leases and fencing, `apply`, `watch`, SubagentStart/Stop and PostToolUse(Agent) hooks.
3. **P2:** `moirai mcp` (rmcp, dual-era), per-role allowlists, stamp hook, role write policy, plugin packaging.
4. **P3:** plugin monitors, rules export, resources and subscriptions, optional HTTP mode, memory-tool handler. Revisit channels at GA.

---

## 10. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | Daemon operational pain (orphans, skew, antivirus), the reason Beads dropped its daemon [D] | Version handshake, idle exit, `daemon status/stop`, a direct-mode fallback, `doctor` |
| R2 | The Claude Code `structuredContent` behaviour changes | Keep text-first. Test per Claude Code release. |
| R3 | Hooks may not fire for Workflow `agent()` calls (unverified) | Pattern 1 (dispatcher claims plus batch ingest) works without them. Run a 5-minute experiment. |
| R4 | Model skips the protocol (forgets `complete`) | SubagentStop safety net, TTL, orchestrator batch ingest |
| R5 | Hook latency accumulates (15–50 ms per spawn [M]) | Only boundary events. `if:` filters. `async` for PostToolUse. |
| R6 | Hook output cap of 10,000 characters [D] silently hides the tail | Self-budgeting brief and pack with an explicit drop footer |
| R7 | Stamp hook auto-approves moirai tools | Server-side role policy. The owner can choose `ask`. |
| R8 | Spec churn: stdio defaults to legacy today; the 2026-07-28 channel incompatibility [D] | rmcp dual-era; watch Claude Code's `MCP_PROTOCOL_NEGOTIATION` defaults |
| R9 | Windows exec-form PATH resolution of `moirai` | Verify. Fall back to an absolute path. |

---

## 11. Open questions for the owner

1. **Dispatch style:** should Workflow runs use dispatcher claims plus batch ingest (deterministic, less agent protocol) as the default, with agents calling moirai only to read? Or should every agent write its own nodes?
2. **Permission posture:** may moirai MCP calls be auto-approved (the stamp hook returns `allow`)? Or should writes prompt?
3. **Branch semantics:** is moirai knowledge from a feature worktree `proposed` until merge (recommended), or should moirai branches mirror git branches one-to-one (report 01 Q4)?
4. **Daemon acceptability:** is a background `moirai daemon` process per repository acceptable on your machine? It is idle-exiting and per-user. Or must P0 and P1 stay daemonless, which costs push notifications and a hot index?
5. **Push into the orchestrator:** are plugin monitors (interactive sessions only) acceptable, or do you want only hook-time digests? Do you ever run the orchestrator headless (`-p`), where monitors don't start [D]?
6. **Read-only roles:** may architect, critic, and researcher write their own node kinds through MCP while keeping no file Write/Edit (report 01 Q3)?
7. **Rules export:** should critical path-scoped rules be exported into `.claude/rules/moirai/` so Claude Code loads them natively? If so, committed or gitignored?
8. **Lease TTL defaults:** 15 minutes for developers and 60 minutes for dispatcher claims across a Workflow? Should a dead Claude process (PID check) release leases immediately?
9. **Other clients:** will non-Claude agents (Codex, Cursor, CI bots) use moirai? That raises the priority of MCP HTTP mode and a stable `--json` contract.
10. **Distribution:** plugin plus separately installed binary, or a plugin that ships binaries (which blocks claude.ai and Cowork installs)?

---

## 12. Sources

All fetched 2026-09-25 unless noted.

**MCP spec and SDKs**
- Changelog 2026-07-28: https://modelcontextprotocol.io/specification/2026-07-28/changelog
- Release post: https://blog.modelcontextprotocol.io/posts/2026-07-28/
- Tools: https://modelcontextprotocol.io/specification/2026-07-28/server/tools · Resources: …/server/resources · Subscriptions: …/basic/patterns/subscriptions · Transports: …/basic/transports · stdio: …/basic/transports/stdio · Streamable HTTP: …/basic/transports/streamable-http · Versioning: …/basic/lifecycle · Discover: …/server/discover
- SDK tiers: https://modelcontextprotocol.io/community/sdk-tiers · Rust Tier 1 assessment: https://github.com/modelcontextprotocol/modelcontextprotocol/issues/3179 · secondary: https://www.digitalapplied.com/blog/mcp-sdk-conformance-tiers-what-tier-1-means
- rmcp: https://github.com/modelcontextprotocol/rust-sdk · https://crates.io/crates/rmcp (3.4.1, 2026-09-23)
- Alternatives: https://crates.io/crates/rust-mcp-sdk · https://lib.rs/crates/turbomcp · https://github.com/paiml/rust-mcp-sdk
- Benchmarks [C]: https://www.tmdevlab.com/mcp-server-performance-benchmark-v2.html · https://dmitrii.app/stateless-servers-stateful-payloads-sessions-vs-continuations-measured-in-rust/
- Reference memory server: https://github.com/modelcontextprotocol/servers/tree/main/src/memory

**Anthropic guidance**
- Writing tools for agents (2025-09-11): https://www.anthropic.com/engineering/writing-tools-for-agents
- Context engineering (2025-09-29): https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- Agent Skills (2025-10-16): https://www.anthropic.com/engineering/equipping-agents-for-the-real-world-with-agent-skills
- Code execution with MCP (2025-11-04): https://www.anthropic.com/engineering/code-execution-with-mcp
- Advanced tool use (2025-11-24): https://www.anthropic.com/engineering/advanced-tool-use
- Memory tool: https://platform.claude.com/docs/en/agents-and-tools/tool-use/memory-tool

**Claude Code docs** (code.claude.com/docs/en/…; the `.md` variants were fetched)
- `hooks`, `skills`, `mcp`, `sub-agents`, `agent-teams`, `workflows`, `tools-reference`, `channels`, `memory`, `env-vars`, `plugins/components`, `plugins/manifest-reference`
- Issues: https://github.com/anthropics/claude-code/issues/55677 (closed, not planned) · https://github.com/anthropics/claude-code/issues/79944 (open, 2026-07-21)

**CLI versus MCP debate**
- https://mariozechner.at/posts/2025-08-15-mcp-vs-cli/
- https://www.scalekit.com/blog/mcp-vs-cli-use (2026-03-11)
- https://lucumr.pocoo.org/2025/12/13/skills-vs-mcp/
- https://docs.task-master.dev/capabilities/mcp
- https://github.com/MrLesk/Backlog.md

**Beads**
- https://github.com/steveyegge/beads (now gastownhall/beads) · releases v1.3.0: https://github.com/gastownhall/beads/releases/tag/v1.3.0
- Docs: https://beads.gascity.com/integrations/claude-code · …/reference/worktrees.md · …/multi-agent/coordination.md · …/reference/json-schema.md
- DoltHub, *Restoring Beads Classic* (2026-04-02): https://www.dolthub.com/blog/2026-04-02-restoring-beads-classic/

**CLI output design**
- https://cli.github.com/manual/gh_help_formatting · https://cli.github.com/manual/gh_help_environment
- https://git-scm.com/docs/git-status
- https://docs.jj-vcs.dev/latest/templates/
- https://clig.dev/

**Concurrency and IPC**
- LockFileEx: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex
- jj concurrency: https://docs.jj-vcs.dev/latest/technical/concurrency/
- Kleppmann on fencing tokens: https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html
- SQLite WAL: https://www.sqlite.org/wal.html
- redb multi-writer PR: https://github.com/cberner/redb/pull/1462
- AF_UNIX on Windows: https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/
- tokio named pipes: https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html
- https://crates.io/crates/interprocess · https://crates.io/crates/notify
- PowerShell native argument passing: https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing

**Local, read-only (the agent workflow only, per the owner's instruction)**
- The BoykoEngine repository: `.claude/settings.json`, `.claude/agents/*.md` (frontmatter `tools:`), and CLAUDE.md §Agents / §Orchestration discipline
- Sibling report [01-boyko-workflow-roles.md](01-boyko-workflow-roles.md)
- Measurements: PowerShell `Stopwatch` loops and an in-memory `NamedPipeServerStream` echo, run in this session. No files were created.
