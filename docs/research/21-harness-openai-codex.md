# 21 — Harness lens: OpenAI Codex as a moirai client

*Research lens: OpenAI Codex (CLI, IDE extension, desktop app, `codex exec`, Codex cloud) as a client of moirai. Date: 2026-09-26. Research only — nothing implemented, no repository modified.*
*Trigger: owner requirement of 2026-09-26, «It must work not only for Claude Code but also for Codex and other harnesses.» Companion to [07] (agent integration, Claude-Code-centric) and the design of record [AR] §7 (agent interface), §8.3 (token budgets). This report does not repeat them; it says what changes for Codex and what the core must provide so that nothing required depends on a Claude-only feature.*

**Evidence labels** (same convention as [07]):

| Label | Meaning |
|---|---|
| **[D]** | Documented in a primary source: official docs (`learn.chatgpt.com`, formerly `developers.openai.com/codex`), release notes, the MCP spec. |
| **[S]** | Read in the source code of `openai/codex` (`main`, fetched 2026-09-26) or in files shipped with the installed app. |
| **[C]** | Third-party claim (GitHub issue, blog, secondary guide) not reproduced. |
| **[I]** | My inference or recommendation. |
| **[M]** | Measured/observed harmlessly on the owner's machine (read-only commands: `--version`, `--help`, `features list`, reading local config/cache files; nothing installed, nothing run against a model). |

---

## 0. TL;DR

1. **Codex is closer to Claude Code than the design assumed.** As of Codex 0.155–0.157 (Sep 2026) it has lifecycle hooks with near-identical events and I/O (`SessionStart`, `UserPromptSubmit`, `PreToolUse`/`PostToolUse` incl. MCP tools, `SubagentStart`/`SubagentStop`, `PreCompact`/`PostCompact`, `Stop`, `SessionEnd`, `PermissionRequest`), `additionalContext`, `updatedInput`, and even **`mcp_tool` hook handlers** [D, S]; it reads **SKILL.md skills** (Agent Skills format) from `.agents/skills` [D]; it has subagents with custom agent files and `agent_type` [D]; it has plugins (`.codex-plugin/plugin.json`) bundling skills, MCP servers and hooks, and it even sets `CLAUDE_PLUGIN_ROOT` for plugin hooks, for compatibility [D]. So ~80 % of §7.5 maps 1:1. What does **not** exist: the Workflow tool (deterministic JS orchestrator with `journal.jsonl`), Claude's tool allowlists that create "no-Bash roles", and Claude's permission-rule matchers such as `Bash(mv *)`.
2. **Attribution without hooks is better on Codex than on Claude Code.** Every MCP `tools/call` from Codex carries `_meta.threadId` (the agent's thread), `_meta.sessionId` (the root session shared by the parent and all descendant threads), `_meta.callId` and `_meta["x-codex-turn-metadata"]` (`turn_id`, workspaces with git head and dirty flag, sandbox) [S]. A server that declares the experimental capability `codex/sandbox-state-meta` also receives the caller's **`sandboxCwd`** and permission profile [S]. Shell commands get `CODEX_THREAD_ID`, `CODEX_SESSION_ID`, `CODEX_VERSION` in their environment [S]. The `PreToolUse` identity stamp (§7.2) is therefore unnecessary on Codex; only the **role label** still needs the dispatch marker or a `SubagentStart` hook.
3. **The MCP output cap is smaller and cuts the middle.** Codex truncates each tool output to the model's `truncation_policy` — **10,000 "tokens" for every model in the owner's catalog** [M] — where a "token" is **UTF-8 bytes / 4** [S], plus a 20 % serialization allowance for MCP tools [D, S]; truncation keeps head and tail and inserts `…N tokens truncated…` [S]. moirai must cap Codex-bound MCP and CLI output in **bytes** (≤ 36,000 recommended), not characters: 32,000 Cyrillic characters are 64,000 bytes and would be cut. Hook `additionalContext` is capped at **2,500 approximate tokens (10,000 bytes)** by default, spilling the rest to a temp file [S]; moirai's hook entries must set `additionalContextLimit` or budget in bytes.
4. **`structuredContent` wins over text on Codex too.** When a result has non-null `structuredContent`, Codex serializes it as JSON and drops `content[].text` [S] — the same caveat as Claude Code. The design's "compact text, no `structuredContent`" rule is right for both.
5. **All MCP tools are deferred on Codex, and GPT-5.6 models run in "code mode".** Since June 2026 every MCP tool is deferred behind `tool_search` (BM25 over names and descriptions) whenever the model supports it; there is no opt-out [D PR #29486]. The owner's configured model, `gpt-5.6-luna`, has `tool_mode: code_mode_only` [M]: the model gets one `exec` tool that runs JavaScript in V8, and calls MCP tools as `await tools.mcp__moirai__pack({...})`, receiving a `CallToolResult` object, printing with `text(...)` [S]. Server `instructions` become the tool namespace description [S]; docs ask for the first **512 characters** to be self-contained [D].
6. **The protocol is legacy by default.** Codex's MCP client is rmcp 3.2.0 and speaks the legacy `initialize` handshake with protocol version **2025-06-18**; 2026-07-28 needs the unfinished feature flag `mcp_2026_07_28` *and*, for stdio, `CODEX_MCP_PROTOCOL_VERSION=2026-07-28` in the server's `env` [S, M]. moirai's server must be dual-era (it already plans to be) and must accept 2025-06-18.
7. **Each Codex thread (subagent) starts its own MCP server process** and some are leaked after the subagent ends [C: issues #12333, #25015, #37453, #38353, open as of Aug 2026]. The "one warm server per session serving all subagents" assumption of §6.1 does not hold: under Codex, moirai's MCP server must cold-start in milliseconds, keep idle RAM tiny, and Bash-capable roles should use the CLI.
8. **The sandbox blocks the CLI's writes to `<repo>/.git/moirai`.** In `workspace-write`, `.git` (directory or pointer file, and the resolved gitdir), `.agents`, `.codex` and `.aws` stay read-only on macOS, Linux and Windows [D, S]; linked worktrees' common dir is outside the writable root anyway. Readers still work (they open read-only and take no locks). Fixes, in order of preference: writes through the MCP server (MCP servers are spawned directly by Codex, not inside the command sandbox [C #7635, I from `codex/sandbox-state-meta`]); an execpolicy rule `prefix_rule(pattern=["moirai"], decision="allow")`, which runs matching commands **outside** the sandbox [D]; or a writable root pointing exactly at `<git-common-dir>/moirai`, which the source exempts from the `.git` protection [S, C]. Unix sockets are denied in the Linux and macOS sandboxes with network off [S]; moirai's leaderless direct path (design [80]) already copes.
9. **Windows:** native, first-class, elevated sandbox with separate local users `CodexSandboxOffline/Online` [D, M]. Agent commands run in **PowerShell** — `pwsh` if found, else Windows PowerShell 5.1 [S]; the owner has no `pwsh`, so Codex agents here would use **PowerShell 5.1.26100** [M]. Codex prefixes every PowerShell script with `[Console]::OutputEncoding=UTF8` [S] (moirai's UTF-8 output decodes correctly), but piping non-ASCII text *into* `moirai` still degrades to `?` in 5.1 [I, as in [AR §7.7.3]]. Command hooks run through `%COMSPEC% /C "<command>"` (cmd.exe) [S].
10. **Tokenizer:** GPT-5.x models use `o200k_base` [C]; the GPT-6 family (added in 0.157.0, 2026-09-25) is unverified. Codex's own caps use the bytes/4 heuristic regardless of tokenizer [S]. The single `pack.cyrillic-weight` calibrated for Claude does not transfer.
11. **Recipe in one line:** register `moirai mcp` as a stdio server with `required = true` and `default_tools_approval_mode = "approve"` (or `"writes"`), annotate every tool with `readOnlyHint`/`destructiveHint`, add a `moirai` execpolicy rule, install hooks into `~/.codex/hooks.json` (trusted once via `/hooks`), place the three skills in `.agents/skills/`, put a three-line pointer in `AGENTS.md`, and run dispatch through `codex exec --json --output-schema … -o …` or the Codex SDK, ingested by a harness-neutral `moirai apply` (§10).
12. **What the core must change** (§11): a harness profile chosen from MCP `clientInfo` or environment (caps in the transport's own unit, tokenizer ratios); a caller-context resolver whose sources are explicit parameters, then Codex `_meta`, then Claude's stamp, then environment; role policy keyed on the lease and the dispatch marker, never on a Claude hook; `apply` ingestion adapters for Claude Workflow journals, Codex exec output and Codex CSV jobs; a per-harness hook and skill installer; tool schemas and descriptions that survive OpenAI's schema rules, BM25 tool search and code mode; LQ-Bench on the Codex model if Codex is first-class (owner call).

---

## 1. Pinned versions and facts (as of 2026-09-26)

| Item | Value | Source |
|---|---|---|
| Codex CLI, latest stable | **0.157.1** (2026-09-26); 0.157.0 2026-09-25 ("Added GPT-6 Sol and Luna", "Restrict Unix local MCP servers to stdio descriptors", "Restrict Windows sandbox default object access to the logon session"); pre-releases 0.158.0-alpha, 0.159.0-alpha | [D] github.com/openai/codex/releases |
| Owner's machine: Codex desktop app | Microsoft Store package `OpenAI.Codex` **26.917.9434.0**, x64 | [M] `Get-AppxPackage` |
| Owner's machine: bundled CLI | `codex-cli 0.155.0-alpha.9.2` at `%LOCALAPPDATA%\OpenAI\Codex\bin\<hash>\codex.exe`; `codex` is **not on PATH** | [M] `--version` |
| Owner's Codex config | `model = "gpt-5.6-luna"`, `model_reasoning_effort = "xhigh"`, `[windows] sandbox = "elevated"`, bundled plugins enabled, a `notify` program owned by the computer-use plugin | [M] `~/.codex/config.toml` (secrets not read) |
| Owner's feature flags (0.155.0-alpha.9.2) | `hooks` stable **on**; `multi_agent` stable on; `multi_agent_v2` stable off; `memories` stable off; `skill_search` stable on; `code_mode_host` stable on; `mcp_2026_07_28` under development **off**; `tool_search` *removed* (off) and `tool_search_always_defer_mcp_tools` *removed* (**true** = behaviour fixed on); `non_prefixed_mcp_tool_names` under development off; `powershell_shell_version` under development off | [M] `codex features list` |
| Owner's model catalog (fetched 2026-09-20, client 0.155.0) | `gpt-5.6-terra`, `gpt-5.6-luna` (listed), `gpt-5.5` (listed), `gpt-reserve`, `codex-auto-review` (hidden). All: `truncation_policy = {mode: tokens, limit: 10000}`, `context_window 272000` (`max_context_window 872000` except gpt-5.5), `shell_type unified_exec`, `supports_search_tool true`; 5.6 models and `gpt-reserve`: **`tool_mode: code_mode_only`**; `multi_agent_version` v1 (luna) / v2 (terra) | [M] `~/.codex/models_cache.json` |
| Owner's Windows sandbox | elevated; users `CodexSandboxOffline`, `CodexSandboxOnline` created 2026-09-19; read ACEs granted across the user profile for the sandbox users | [M] `~/.codex/.sandbox/*` |
| Owner's shells | no `pwsh`; Windows PowerShell **5.1.26100.9444** | [M] |
| Codex MCP client | rmcp **=3.2.0**; legacy `initialize`, protocol **2025-06-18** by default; 2026-07-28 behind a flag | [S] `codex-rs/Cargo.toml`, `rmcp-client/src/protocol_mode.rs` |
| Docs location | `developers.openai.com/codex/*` now 308-redirects to `learn.chatgpt.com/docs/*` | [D] |
| Models named in docs/release notes | 0.157.0 notes: "GPT-6 Sol and Luna"; subagent docs example: `gpt-6-luna`; the repo's bundled openai-docs skill: `gpt-6-astra`, `gpt-5.6-terra`, `gpt-5.6-luna`. Names are in flux; treat as unverified. | [D]/[S]/[C] |

---

## 2. Codex as an MCP client

### 2.1 Transports and configuration

| Aspect | Codex behaviour | Source |
|---|---|---|
| Transports | **stdio** (`command`, `args`, `env`, `env_vars`, `cwd`) and **streamable HTTP** (`url`, `bearer_token_env_var`, `http_headers`, `env_http_headers`, `http_headers_helper`, OAuth with CIMD/DCR, `codex mcp login`) | [D] learn.chatgpt.com/docs/extend/mcp |
| Config | `[mcp_servers.<id>]` in `~/.codex/config.toml` or a trusted project `.codex/config.toml`; `codex mcp add <name> -- <cmd>` / `--url`; plugin-provided servers in the plugin's `.mcp.json` | [D], [M] `codex mcp add --help` |
| Timeouts | `startup_timeout_sec` default **10 s**; `tool_timeout_sec` default **60 s**; `mcp_optional_startup_grace_ms` default **1000 ms** (how long Codex waits for optional servers when building the first tool catalog) | [D] |
| Required | `required = true` fails startup/resume if the server cannot initialize | [D] |
| Tool filters | `enabled_tools`, `disabled_tools` | [D] |
| Approvals | `default_tools_approval_mode` and `tools.<tool>.approval_mode` ∈ `auto \| prompt \| writes \| approve`; `writes` "prompts for tools that aren't marked read-only" | [D] |
| Per-tool output budget | `tools.<tool>.output_token_limit` — "positive token budget for one tool's output, before the standard 20% serialization allowance"; most restrictive of plugin and user policy wins (PR #41421, merged 2026-08-28) | [D], [C] |
| Environment | The server gets only a whitelist: on Unix `HOME, LOGNAME, PATH, SHELL, USER, __CF_USER_TEXT_ENCODING, LANG, LC_ALL, TERM, TMPDIR, TZ`; on Windows the "core" Windows variables; plus `env` and `env_vars`. **No Codex session or thread id in the server's environment.** | [S] `rmcp-client/src/utils.rs` |
| Process per thread | Each thread, including every subagent, starts its own stdio server processes; leaks after `close_agent` reported on Linux and Windows | [C] #12333, #25015, #37453, #38353 (opened 2026-08-13, app 26.803, open) |
| Server sandboxing | MCP server processes are not placed in the command sandbox; an MCP tool "can do network call or write in the file system even when codex is in read-only mode" (closed *not planned*) | [C] #7635; [I] consistent with the opt-in `codex/sandbox-state-meta` capability, which exists so a server can apply the caller's sandbox itself [S] |

### 2.2 Protocol version and lifecycle

- Two modes, chosen once per session: `Legacy` (default; `initialize` with `ProtocolVersion::V_2025_06_18`) and `V20260728` (2026-07-28 discovery and stateless lifecycle, falling back to 2025-06-18) [S `protocol_mode.rs`].
- For stdio, the modern mode needs both the session mode (feature `mcp_2026_07_28`, "under development", off [M]) **and** `CODEX_MCP_PROTOCOL_VERSION = "2026-07-28"` in the server's configured `env`, which Codex removes before spawning [S `rmcp_client.rs` l.484–489].
- **Implication [I]:** moirai's rmcp-based server must answer a 2025-06-18 `initialize` (Codex), the Claude Code legacy handshake and 2026-07-28 `server/discover`. It must keep no hidden per-connection state that matters for correctness — already a rule in [07 §2.1]. Nothing moirai needs is 2026-07-28-only.

### 2.3 How tools reach the model: deferral, tool search, code mode

| Mode | What the model sees upfront | How it gets a moirai tool | Source |
|---|---|---|---|
| Classic (e.g. `gpt-5.5`) with tool search | a `tool_search` tool whose description lists sources as `- <name>: <description>`; MCP tools are **all deferred** (no opt-out since PR #29486, merged 2026-06-22) | `tool_search` (BM25 over tool metadata, default limit 8) returns matching tools for the next request | [D] PR #29486; [S] `tools/handlers/tool_search*.rs` |
| **Code mode** (`tool_mode: code_mode_only`: `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-reserve` in the owner's catalog [M]) | one `exec` tool ("Run JavaScript code to orchestrate/compose tool calls", V8 isolate, no Node/fs/network) and `wait`; enabled nested tools rendered as TypeScript signatures; deferred ones omitted with the note "filter `ALL_TOOLS` by `name` and `description`" | `await tools.mcp__moirai__pack({...})` returns a `CallToolResult` object (`content[]`, `structuredContent?`, `isError?`); the script prints with `text(...)`, which `JSON.stringify`s non-strings; `exec` output defaults to `max_output_tokens` 10,000 | [S] `code-mode-protocol/src/description.rs` |
| Older model/provider without search | MCP tools exposed directly | — | [D] PR #29486 |

- **Server `instructions` become the model-visible namespace description** of the server's tools (`namespace_description: server_instructions`) [S `codex-mcp/src/rmcp_client.rs` l.839–853], shown in the `tool_search` source list (total source-description budget 512 KiB) [S]. Docs: "Keep the first 512 characters self-contained so the most important guidance is available when Codex is deciding how to use the server" [D]. Claude Code truncates instructions and descriptions at 2,048 characters [07 §2.6].
- **Naming:** tools appear as `mcp__<server>__<tool>` (legacy prefix; a flag to drop it per server is under development) [S, M]; names are sanitized and hashed past **128** characters [S `codex-mcp/src/tools.rs`]. moirai's names are short.
- **Schemas:** OpenAI rejects tool parameter schemas whose root is not a plain `object` or carries `anyOf/oneOf/allOf/enum/const/not` at the root, and Codex has had bugs with local `$ref/$defs` [C #3152, #13746; PR #24118 adds `oneOf/allOf` support]. moirai's hand-written `tools/list` must use plain object roots, inline everything, and keep `write.ops` items as `{op: <enum>, …}` objects (already so in [AR §7.2]).
- **Known defects:** `tool_search` can miss an exactly named deferred MCP tool (#21503) [C]; `notifications/tools/list_changed` does not refresh the deferred catalog (#33266) [C]; after compaction a turn can lose `tool_search` and MCP tools (#34719) [C]. The CLI must remain a complete fallback.

### 2.4 Results: text vs `structuredContent`, truncation, images

- **`structuredContent` preferred:** `CallToolResult::as_function_call_output_payload()` returns `serde_json::to_string(structured_content)` when it is present and non-null, and ignores `content` [S `protocol/src/models.rs`]. Otherwise each text block becomes an `input_text` item (plain text to the model) [S].
- **`isError`** maps to `success: false` [S].
- **Truncation** [S `utils/output-truncation`, `utils/string/src/truncate.rs`]:
  - "tokens" are estimated as `ceil(bytes / 4)` (`APPROX_BYTES_PER_TOKEN = 4`) — a heuristic, not the model tokenizer;
  - the model's `truncation_policy` (10,000 for all catalogued models [M]) applies to stored tool outputs; `tool_output_token_limit` (global) and `tools.<tool>.output_token_limit` (MCP) override it; MCP budgets get ×1.2 (`with_serialization_allowance`) [S, D];
  - text is cut **in the middle**, keeping head and tail, with the marker `…N tokens truncated…`; multi-item results drop later items with `[omitted N text items ...]` [S].
  - Effective ceiling for one MCP text result ≈ 12,000 × 4 = **48,000 bytes**; for shell output ≈ 40,000 bytes (the `exec_command`/`exec` default `max_output_tokens` is 10,000) [S, I].
- **Images/audio** are replaced by a text notice when the model lacks the modality [S] (moirai returns none).

### 2.5 Resources, prompts, notifications, elicitation, cancellation

| Primitive | Codex | Source |
|---|---|---|
| Resources | built-in model tools `list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource`; `/mcp` shows them; the tool-search description tells the model to prefer `tool_search` for discovery | [S] tool-search spec text; [C] |
| Prompts | not surfaced to users as commands (custom prompts are deprecated in favour of skills) | [C], [D] custom-prompts page |
| `list_changed` | not honoured for deferred tools | [C] #33266 |
| Push to the model | none (same as Claude Code: notifications reach the client, not the model) | [I] |
| Elicitation | supported (`tool_call_mcp_elicitation` stable) but unanswerable in delegated subagents (#31565) and auto-cancelled in `codex exec` (#24135) | [M], [C] |
| Annotations | tools without `readOnlyHint` are treated as writes (approval in `writes` mode); `readOnlyHint: true` also allows concurrent execution | [D] (writes mode), [C] |

### 2.6 Attribution: what the server learns per call

`build_mcp_tool_call_request_meta` + `with_mcp_tool_call_ids_meta` attach to **every** `tools/call` (all servers, before any server-specific branch) [S `core/src/mcp_tool_call.rs`]:

| `_meta` key | Content | Use for moirai [I] |
|---|---|---|
| `callId` | tool-call id | default idempotency-key component |
| `threadId` | the calling agent's thread (a subagent has its own) | `agent` default (`codex:<threadId>`) |
| `sessionId` | "identity shared by the root thread and all descendant threads" [S `core/src/session/session.rs` l.647] | `session` default; lease holder anchor; the "campaign" grouping |
| `windowId`, `itemId` | UI origin of the call, when present | ignore |
| `x-codex-turn-metadata` | JSON: `session_id`, `turn_id`, `workspaces{<path>: {latest_git_commit_hash, has_changes}}`, `sandbox` (example in #17468, v0.120) | turn cursor; worktree and git head without a scan |
| `codex/sandbox-state-meta` (only if the server declares this experimental capability) | `{permissionProfile, codexLinuxSandboxExe, sandboxCwd, useLegacyLandlock}` [S `codex-mcp/src/runtime.rs`] | **`sandboxCwd` = the caller's worktree** → branch/tree binding exactly as the Claude stamp's `cwd` |

What `_meta` does **not** carry: the role (`agent_type`) and moirai's dispatch marker. Those come from explicit parameters, the lease, or a `SubagentStart` hook (§4).

---

## 3. Instructions, skills, plugins

### 3.1 AGENTS.md

- **Discovery** [D, S `core/src/agents_md.rs`]: global `~/.codex/AGENTS.override.md` or `~/.codex/AGENTS.md` (under `CODEX_HOME`); then from the project root (first ancestor with a `project_root_markers` entry, default `.git`) down to the cwd, one file per directory: `AGENTS.override.md` (replaces, not adds) else `AGENTS.md` else each `project_doc_fallback_filenames` entry (e.g. `CLAUDE.md`). Never above the project root. Concatenated root→cwd, user and project parts separated by `--- project-doc ---`.
- **Size:** stops adding files when the combined size reaches `project_doc_max_bytes` (**32 KiB** default); empty files skipped [D].
- **Refresh:** built once per run (TUI: per session) [D]; a newer refresh test exists in the source (`agents_md_refresh`) [S].
- **Other injection points:** `developer_instructions` (config string), `model_instructions_file` (replaces built-in instructions) [D].
- **For moirai [I]:** AGENTS.md is versioned in the project repo and static, so it is the wrong place for a dynamic brief. It should carry a ≤ 3-line pointer; the dynamic brief belongs to the `SessionStart` hook (or a first `moirai brief` call when hooks are off). `project_doc_fallback_filenames = ["CLAUDE.md"]` lets one instruction file serve both harnesses.

### 3.2 Skills (Agent Skills format)

- **Format:** a directory with `SKILL.md` (YAML frontmatter `name`, `description`) plus optional `scripts/`, `references/`, and Codex-only `agents/openai.yaml` (UI metadata, MCP-server dependencies) [D]. The format is the open Agent Skills specification (agentskills.io, published by Anthropic, Dec 2025) [C].
- **Locations** [D]: repo `.agents/skills` in every directory from cwd up to the repo root; user `$HOME/.agents/skills`; admin `/etc/codex/skills`; system (bundled). (`~/.codex/skills` exists on the owner's machine, empty — an older location [M].)
- **Progressive disclosure:** the catalog (names + descriptions) costs at most **2 % of the context window** (`skills.max_context_tokens`), or 8,000 characters when the window is unknown; descriptions are shortened first when many skills are installed; the full `SKILL.md` loads on selection [D]. At 272k context that is ≈ 5,400 tokens for all skills together.
- **Invocation:** explicit `$skill-name` in the CLI (`@` in ChatGPT), or implicit by description match [D].
- **Custom prompts** (`~/.codex/prompts/*.md` as slash commands) are deprecated in favour of skills [D].
- **For moirai [I]:** the three skills (`moirai`, `moirai-orchestrate`, `moirai-ql`, [AR §7.5]) can be one source tree installed twice — `.claude/skills/` and `.agents/skills/` — if they use only `name`/`description` frontmatter and put Claude-only fields (if any) where Codex ignores them. `.agents/` is read-only inside Codex's sandbox, which is fine for skills.

### 3.3 Plugins and migration

- **Manifest:** `.codex-plugin/plugin.json` with `name`, `version`, `description`, `skills` (path), `mcpServers` (path to a `.mcp.json` whose entries use Codex keys: `command`, `args`, `cwd`, `env_vars`, `enabled`, `startup_timeout_sec`, `tool_timeout_sec`, `default_tools_approval_mode`, `tools.<t>.approval_mode`, `tools.<t>.output_token_limit`, `omit_tools_from`, `enabled_tools`), `hooks` (default `hooks/hooks.json`), `interface{…}` [M: bundled plugins in the app package; D].
- **Marketplace:** `.agents/plugins/marketplace.json` listing plugins with `source` and `policy` [M]; `codex plugin marketplace add|list|upgrade|remove`, `codex plugin add|list|remove` [M].
- **Plugin hook environment:** `PLUGIN_ROOT`, `PLUGIN_DATA`, and "for compatibility with existing plugin hooks" `CLAUDE_PLUGIN_ROOT`, `CLAUDE_PLUGIN_DATA` [D].
- **Claude plugins are not native Codex plugins**; `/import` migrates instructions (CLAUDE.md → AGENTS.md), MCP servers, skills, **synchronous command hooks only** (async and HTTP hooks skipped), subagents and recent chats [C].
- **For moirai [I]:** ship one repository with both manifests (`.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`) sharing `skills/`, but with **separate** hook files and MCP files per harness (matchers, `mcp_tool` templates and MCP keys differ, §4 and §10). The binary stays a separate install ([07 §9.6]).

---

## 4. Hooks and lifecycle events

### 4.1 Parity table against moirai's hook set ([AR §7.5])

| moirai hook | Claude Code | Codex | Notes for moirai [I unless marked] |
|---|---|---|---|
| `SessionStart` startup/resume (command) | ✓ | ✓ matcher on `source` ∈ `startup, resume, clear, compact` [D]; "SessionStart hooks can run before an MCP server is ready. If that happens, they don't block the session." [D] | keep the exec-form command at startup, as for Claude |
| `SessionStart` clear/compact | ✓ | ✓ (+ `PreCompact`/`PostCompact` events) [D] | same |
| `UserPromptSubmit` delta | ✓ | ✓ plain stdout or `additionalContext` → "extra developer context"; can block [D] | same handler |
| `SubagentStart` role pack | ✓ | ✓ input `agent_id`, `agent_type`, `turn_id`, `permission_mode`; `additionalContext` → "extra developer context for the subagent"; cannot prevent the agent [D, S] | same handler; `agent_type` = custom agent name = role label |
| `PostToolUse` matcher `Agent` (marker → agentId map) | ✓ | ✓ "`spawn_agent` also matches `Agent`" [D] | payload fields differ (`spawn_agent` args: `message`, `agent_type`, `fork_turns`) [S]; parse both |
| `SubagentStop` lease release | ✓ | ✓ `agent_id`, `agent_type`, `agent_transcript_path`, `stop_hook_active`, `last_assistant_message` [D] | same handler |
| `PreToolUse` stamp `mcp__moirai__(claim\|complete\|remember\|write)` | ✓ (`mcp_tool`) | possible: MCP tools fire `PreToolUse`, `updatedInput` replaces the MCP arguments object [D]; input carries `agent_id`/`agent_type` only inside subagents [S] | **not needed**: `_meta.threadId/sessionId` and `sandboxCwd` give the same context (§2.6) |
| `PostToolUse` `Bash(mv *)`, `PowerShell(Move-Item *)` evidence | ✓ (Claude permission-rule syntax) | matcher is a **regex on the tool name only**; every shell call matches `Bash` (also on Windows) [D]; `apply_patch` matches `apply_patch\|Edit\|Write` [D] | fires on every shell call → must be an `mcp_tool` handler (cheap, in-process filter on `${tool_input.command}`) or be disabled on Codex; a command hook per shell call costs a cmd.exe + moirai spawn |
| `PostToolUse` `Write\|Edit` edit evidence | ✓ | ✓ via `apply_patch` (input is the patch text; paths must be parsed from it) [D] | parse `*** Update File:`/`*** Add File:`/`*** Delete File:`/`*** Move to:` lines |
| `Stop`, `SessionEnd`, `PermissionRequest`, `Interrupt` | ✓/partial | ✓ [D] | unused by moirai |

### 4.2 Mechanics

| Aspect | Codex | Source |
|---|---|---|
| Config locations | `~/.codex/hooks.json` or `[hooks]` in `~/.codex/config.toml`; `<repo>/.codex/hooks.json` or `[hooks]` in `<repo>/.codex/config.toml` (project layer loads only when trusted); plugin `hooks/hooks.json` or manifest `hooks`; managed hooks in `requirements.toml` | [D] |
| Enable | `[features] hooks = true` (stable, on for the owner [M]) | [D], [M] |
| **Trust** | "Before a non-managed hook can run, Codex requires you to review and trust the exact hook definition" (`/hooks`); a changed definition becomes `Modified` and stops running until re-trusted; `--dangerously-bypass-hook-trust` for vetted automation | [D], [S] `hooks/src/engine/discovery.rs` |
| Handler types | `command` (with `commandWindows`, `timeout` default 600 s, `async`, `statusMessage`, `additionalContextLimit`) and **`mcp_tool`** (`server`, `tool`, `input` template, `timeout`); `prompt` and `agent` handlers are parsed but skipped | [D] |
| `mcp_tool` templates | `${field.nested}` placeholders resolved from the hook input; **a missing field fails the hook** | [S] `hooks/src/engine/mcp_runner.rs` |
| Command execution | Windows: `%COMSPEC%` (cmd.exe) `/C "<command>"`; Unix: `$SHELL -lc`; environment = session snapshot + hook env | [S] `hooks/src/engine/command_runner.rs` |
| Common input | `session_id` (the thread), `cwd`, `hook_event_name`, `model`, `transcript_path`; turn-scoped add `turn_id`, `permission_mode` | [D], [S] |
| Outputs | `continue`, `stopReason`, `systemMessage`, `suppressOutput`, `hookSpecificOutput.additionalContext`; `PreToolUse`: `permissionDecision`, `updatedInput`, exit code 2 blocks; `PostToolUse`: `decision: block` replaces the result | [D] |
| Context limit | `additionalContextLimit` default **2,500 approximate tokens** (bytes/4 → 10,000 bytes); above it the text is written to `<temp>/hook_outputs/<thread_id>/<uuid>.txt` and the model sees a head/tail preview plus the path; `0` disables | [D], [S] `hooks/src/output_spill.rs` |
| Async hooks | `async: true`, delivered at the next safe point; ≤ 8 concurrent per session | [D] |
| `notify` | legacy single command receiving a JSON payload at turn end; on the owner's machine it is already used by the computer-use plugin | [D], [M] |
| Hooks in `codex exec`, IDE, app | not stated in the docs; the hooks engine is in the shared core | [I] — probe (§12) |

---

## 5. Subagents, orchestration, identity

### 5.1 Subagents

- Triggered when the user or AGENTS.md/skill instructions ask for delegation ("spawn two agents") [D]. Tools: `spawn_agent` (v2 args include `agent_type`, `fork_turns`), `send_input`/`send_message`, `wait`, `close_agent`, `list_agents` [S].
- **Custom agents:** one TOML per agent in `~/.codex/agents/` or `.codex/agents/`; required `name`, `description`, `developer_instructions`; optional `model`, `model_reasoning_effort`, `sandbox_mode`, `mcp_servers`, `skills.config`; omitted settings inherit from the parent [D].
- **Concurrency:** `agents.max_concurrent_threads_per_session` (legacy `agents.max_threads`), `agents.max_depth` [D]; secondary sources give defaults 6 and 1 [C].
- **Batch jobs:** `spawn_agents_on_csv` (CSV rows → workers with an instruction template, each must call `report_agent_job_result`; output CSV with `job_id, item_id, status, last_error, result_json`) [C, experimental; verify].
- **Identity:** each subagent is a thread with `parent_thread_id`; `session_id` is shared by the whole tree [S].

### 5.2 Scripted orchestration (the Workflow-tool substitute)

| Mechanism | What it gives | Source |
|---|---|---|
| `codex exec [PROMPT\|-]` | non-interactive run; `--json` JSONL events (`thread.started{thread_id}`, `turn.started`, `item.*` incl. MCP tool calls and command executions, `turn.completed{usage}`); `--output-schema FILE` (final message shape); `-o FILE` (last message); `--ephemeral`; `-C DIR`; `--worktree`; `-s read-only\|workspace-write\|danger-full-access` (default **read-only**); `--ignore-rules`; `--ignore-user-config`; `exec resume <id>` | [D], [M] help |
| Codex SDK (`@openai/codex-sdk`) | `startThread()`, `run(prompt, {outputSchema})`, `resumeThread(id)` from Node 18+ | [D] |
| `codex app-server` | JSON-RPC server driving threads (experimental) | [M] help |

There is no Codex counterpart of Claude Code's Workflow `agent()` with a durable `journal.jsonl`; a dispatcher is an ordinary script (any language) or an SDK program. The moirai dispatcher pattern (claim in bulk, pass `task/lease/branch/role` in the prompt, agents return schema output, one idempotent `apply`) carries over unchanged if `apply` can ingest Codex outputs (§11 HA-8).

### 5.3 Environment variables exposed to tools

| Variable | Where | Meaning | Source |
|---|---|---|---|
| `CODEX_THREAD_ID` | shell commands (injected even with `include_only`) | calling thread | [S] `core/src/exec_env.rs` |
| `CODEX_SESSION_ID` | shell commands | root session shared by descendants | [S] |
| `CODEX_VERSION` | shell commands | Codex version | [S] |
| `CODEX_PERMISSION_PROFILE` | shell commands | informational profile name ("must not be treated as proof of enforcement") | [S] |
| `CODEX_SANDBOX_NETWORK_DISABLED=1` | shell commands with network off | — | [D] codex repo AGENTS.md |
| `CODEX_SANDBOX=seatbelt` | children of Seatbelt on macOS | — | [D] same |
| `PLUGIN_ROOT`, `PLUGIN_DATA`, `CLAUDE_PLUGIN_ROOT`, `CLAUDE_PLUGIN_DATA` | plugin hook commands | — | [D] |
| (none) | MCP stdio servers | only the whitelist of §2.1 | [S] |

**Attribution without hooks [I]:** CLI calls default `--agent` to `codex:<CODEX_THREAD_ID>` and the session to `codex:<CODEX_SESSION_ID>`; MCP calls use `_meta.threadId/sessionId`. The role comes from the lease (`claim --role R`) or the dispatch marker. This is strictly better than Claude Code, where the CLI sees only the session id and subagent identity needs the `PreToolUse` stamp.

---

## 6. Sandbox and approvals

### 6.1 Modes per OS

| | macOS | Linux | Windows (native) |
|---|---|---|---|
| Mechanism | Seatbelt (`sandbox-exec`), deny-default profile; `process-fork/exec` allowed, children inherit | bubblewrap (`--unshare-user`, `--unshare-ipc`, `--unshare-pid`, `--unshare-net` when offline, ro-binds) + seccomp; legacy Landlock deprecated | **elevated**: separate local users `CodexSandboxOffline` (firewall-blocked) / `CodexSandboxOnline`, restricted tokens, ACL grants; **unelevated** fallback: restricted token of the current user with a synthetic `sandbox-write` SID, env-level offline |
| Source | [S] `sandboxing/src/seatbelt*.{rs,sbpl}` | [S] `linux-sandbox/src/{bwrap,landlock}.rs` | [D] windows-sandbox page; [C] InfoQ 2026-06-05; [M] owner's setup |
| `read-only` | reads everywhere allowed by policy; no writes | same | same |
| `workspace-write` | writes to cwd, `/tmp`, `$TMPDIR`, `writable_roots`/`--add-dir` | same | writes to the workspace and configured roots via ACL grants |
| Protected under writable roots | `.git` (dir **or pointer file**, and the resolved gitdir), `.agents`, `.codex`, `.aws` read-only, recursively [D, S `protocol/src/permissions.rs` `PROTECTED_METADATA_PATH_NAMES`] | same | "Git metadata directories remained protected through ACL enforcement" [C InfoQ] |
| Exception | a writable root that lies **inside** the protected path (e.g. exactly `<repo>/.git/moirai`) is exempt [S l.1084–1087; C] | same | [I] same policy, applied as ACLs — probe |
| `danger-full-access` | no sandbox | no sandbox | no sandbox |
| Network | off by default; on via `sandbox_workspace_write.network_access` | seccomp denies `connect/bind/listen/accept…` in restricted mode; `socket(AF_UNIX)` itself allowed, so a UDS cannot connect [S] | offline user firewalled; online user not |
| Unix sockets / pipes | allowed only with network on or an allowlist (`network-bind/outbound (local/remote unix-socket)`) [S] | not connectable (above) [S] | 0.157.0: default object access restricted to the logon session [D]; a pipe DACL naming the owner denies the sandbox user [I] |
| Byte-range locks | Codex denies only `fcntl` commands 80/110 (`F_MAKECOMPRESSED`, `F_TRANSFEREXTENTS`) under restricted write policies (PR #46500) [C]; whether `F_OFD_SETLK` is allowed under deny-default is unverified | locks belong to the inode and are visible across the bind mount [I, 80 §2.7.2]; an exclusive `F_WRLCK` needs a writable fd, impossible on a ro-bind | `LockFileEx` needs a handle the sandbox user may open; liveness probes from another principal answer Unknown ([AR §6.2]) |
| Child processes | allowed | allowed (fresh PID namespace) | allowed |

### 6.2 Approvals and rules

- **Policies:** `on-request` (model asks for escalation), `never`, granular (`sandbox_approval`, `rules`, `mcp_elicitations`, `request_permissions`, `skill_approval`); `untrusted` deprecated; `--approve-for-me` routes approvals to an automatic reviewer [D, M].
- **Execpolicy rules** (Starlark, `~/.codex/rules/*.rules`, `<repo>/.codex/rules/` when trusted): `prefix_rule(pattern=[…], decision="allow"|"prompt"|"forbidden", justification=…, match=[…], not_match=[…])`; **`allow` = "run the command outside the sandbox without prompting"**; simple chains (`&&`, `||`, `;`, `|` with plain words) are split by tree-sitter and each part is judged; scripts with redirection, substitution, variables, wildcards or control flow are judged as one command; most restrictive decision wins [D]. How PowerShell pipelines (here-strings) are split is undocumented [I].
- **MCP approvals:** per server/tool `approval_mode`; `writes` asks for tools not marked `readOnlyHint`; unannotated tools count as writes [D, C]. In `codex exec`, approval prompts auto-cancel (stdin closed, #24135, v0.130) [C]; `approve` mode (present in 0.155 docs and in the bundled plugins [M]) should avoid this — probe.

### 6.3 Consequences for moirai's store [I]

| Client path | Reads | Writes | Fix |
|---|---|---|---|
| MCP server (`moirai mcp`) | ✓ | ✓ (not sandboxed) | none needed; set `approval_mode` and annotations |
| Command hooks (`moirai hook …`) | ✓ | ✓ (hooks are not run in the command sandbox — [I], probe) | none |
| CLI in `read-only` | ✓ (no locks, [AR §2.2]) | ✗ exit 7 | writes through MCP |
| CLI in `workspace-write`, store in `<git-common-dir>/moirai` | ✓ | ✗ (`.git` protected; linked worktrees' common dir is outside the root) | (a) execpolicy `allow` rule for `moirai`; (b) `sandbox_workspace_write.writable_roots = ["<common-dir>/moirai"]`; (c) writes through MCP |
| CLI in `danger-full-access` | ✓ | ✓ | — |
| Windows elevated: CLI as `CodexSandboxOffline` | ✓ (profile read ACEs [M]; D:\ drives by inherited ACL [I]) | ✗ unless (a)/(b) | as above; files created under (b) must stay writable by the owner (inherit the store directory's ACL, as [80] requires for `srt-win`) |

The design already makes the direct path complete, readers lock-free, IPC optional and liveness three-valued ([80 X6]); Codex adds nothing that breaks those rules. What it adds is a **refusal text per harness**: exit 7 must print the Codex fix (the rules line or the writable-root line, with the resolved absolute path), detected from `CODEX_THREAD_ID`/`CODEX_SANDBOX*` in the environment.

---

## 7. Windows specifics

- **Native, first-class** in the desktop app, CLI and IDE extension; Windows 11 recommended, Windows 10 1809+ best effort; WSL remains an option [D].
- **Agent shell** [S `shell-command/src/shell_detect.rs`]: on Windows the default is PowerShell — `pwsh` on PATH or `C:\Program Files\PowerShell\7\pwsh.exe`, else Windows PowerShell 5.1 (`System32\WindowsPowerShell\v1.0\powershell.exe`); ultimate fallback `cmd.exe`; Store-installed PowerShell under `WindowsApps` is skipped for the elevated sandbox because the sandbox users cannot run it. The model may name another shell (e.g. `bash`), from which only the type is taken and the executable rediscovered. Invocation `powershell -NoLogo -NoProfile -Command <script>` with the prefix `try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}` [S `shell-command/src/powershell.rs`].
- **Owner's machine:** no `pwsh` → Codex agents run **Windows PowerShell 5.1.26100** [M]. Every pitfall moirai already designs around applies unchanged ([AR §7.1], [07 §5.2], [AR §7.7.3]): `#40` starts a comment at a token start (ids bare in argv), embedded double quotes are stripped when passing arguments to native programs (texts via `--stdin`/`-f`), an unquoted `@x` splats, `$name` interpolates inside double quotes (LQ `$params` only in single-quoted here-strings), and piping non-ASCII text into a native program uses `$OutputEncoding` (ASCII by default in 5.1) → `?` (W08; `-f FILE` for Cyrillic). New for Codex: stdout decoding is fixed by the UTF-8 prefix, so moirai must write **UTF-8 without relying on the console code page**; and Codex's safety layer parses the PowerShell AST, so exotic syntax may trigger approvals [S `command_safety/powershell_parser.*`].
- **Command hooks** run under `cmd.exe /C "…"` [S]: the hook command must be a plain `moirai hook <event> --harness codex` with no inner double quotes; `commandWindows` can override [D].
- **Elevated sandbox** runs commands as another local user [D, M]: Unknown liveness for session-slot probes (already tolerated), and files the CLI creates in a writable root get that user's ownership → the store must set/inherit an ACL that keeps them writable by the owner (already a rule in [80] for `srt-win`).
- `codex` is not on PATH for app-only installs [M]; scripted `codex exec` dispatchers must use the absolute path or ask the owner to add it.

---

## 8. Models and tokenizer

- **Catalog [M]:** `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-5.5` (listed); `gpt-reserve`, `codex-auto-review` (hidden); release 0.157.0 added the GPT-6 family (names vary between sources, §1). All catalogued models: context 272k (872k max for 5.6), `truncation_policy` 10,000 tokens, `unified_exec` shell, tool search supported, and **code mode only** for 5.6.
- **Tokenizer:** GPT-5.6 Sol/Terra/Luna are reported to use **`o200k_base`** [C: tiktokenizer.com; morphic PR #1021]; GPT-6 unknown [open]. Not measured here (no tokenizer library installed; installing was out of scope).
- **Harness caps are tokenizer-independent:** Codex converts every cap with bytes/4 [S]. For budgeting, moirai therefore needs two numbers per harness: a **cap in the harness's unit** (Codex: UTF-8 bytes; Claude Code: its token count and 2,048/10,000-character limits) and a **cost ratio in real model tokens** (o200k for Codex's GPT-5.x; Claude's tokenizer for Claude). Cyrillic is 2 bytes per character in UTF-8, so a Russian-heavy pack hits Codex's cap at half the characters of an English one.
- **Code mode changes the call surface [I]:** a GPT-5.6 agent calls moirai from JavaScript. Good: it can batch (`ready` then `pack` for the first id) in one `exec` and print only what it needs. Risky: `text(await tools.mcp__moirai__pack(a))` stringifies the whole `CallToolResult` (escaped `\n`, quotes, wrapper keys) — roughly 5–15 % more tokens and worse readability for multi-line text [I, not measured]; the right idiom is `text((await tools.mcp__moirai__pack(a)).content[0].text)`. LQ text inside JS template literals is safe (`$param` without braces is not interpolated), but inside PowerShell double quotes it is not.

---

## 9. What degrades on Codex compared with Claude Code

| moirai mechanism ([AR] ref) | Claude Code | Codex | Degradation / change [I] |
|---|---|---|---|
| Hook transport `mcp_tool` (§7.5) | ✓ | ✓ | none; templates must not reference fields that can be absent (`${agent_id}` on the main thread) |
| Hook trust | settings-based | per-definition hash; re-trust after every change | `moirai hooks install` must keep definitions byte-stable across upgrades; `doctor hooks --harness codex` reports `Untrusted/Modified` |
| `SessionStart` brief ≤ 8,000 units | cap 10,000 chars | default 2,500 approx tokens = 10,000 **bytes**; spill to file | set `additionalContextLimit` on moirai's entries and cap in bytes |
| PreToolUse stamp (§7.2) | needed | replaced by `_meta` | improvement |
| Evidence hooks `Bash(mv *)` (§5e, §7.5) | selective matcher | regex on tool name only | `mcp_tool` handler that filters in-process, or off |
| No-Bash roles via tool allowlists (§7.3, §7.6) | ✓ | every Codex agent has a shell (`exec_command`, or `tools.exec_command` in code mode) | "MCP roles" are optional on Codex; role policy must not rely on missing tools |
| Workflow dispatcher + `apply --from-journal` (§6.4, §7.6) | ✓ | no Workflow; `codex exec`/SDK/`spawn_agents_on_csv` | ingestion adapters; resume semantics per mechanism |
| One MCP server per session (§6.1) | ✓ | one per thread; leaks reported | cold-start and idle-RAM gates per process; prefer CLI |
| MCP output ≤ 32,000 units (§7.2) | warn 10k tok, cap 25k tok | ~40–48 KB, **middle cut** | Codex profile: ≤ 36,000 bytes |
| Deferred tools, names-only upfront (§7.2) | `ToolSearch`, `alwaysLoad` opt-out | always deferred, BM25 search; code mode | descriptions need search keywords; no always-load |
| Server instructions ≤ 600 chars (§7.2) | 2,048 cap | namespace description; first 512 chars matter | ≤ 512 chars for both |
| `structuredContent` off (§7.2) | text dropped if present | text dropped if present | unchanged |
| CLI output ≤ 24,000 chars (§7.4) | Bash ~30,000 chars | ~40,000 bytes, middle cut | byte cap per harness |
| CLI writes in sandbox | Claude sandbox `allowWrite` | `.git` protected | rules/writable root/MCP; per-harness exit-7 text |
| Memory export (`export memory-md`, `export rules`) | MEMORY.md, `.claude/rules` | AGENTS.md (static, 32 KiB), `developer_instructions` | pointer-only export; no dynamic content in AGENTS.md |
| Cloud | Claude cloud sessions | Codex cloud: repo only, "no local MCP servers", agent internet off by default | read-only via the git image at best; writes returned as a batch file in the PR — out of scope for v1 |
| Push to the model | none | none | unchanged |
| LQ-Bench (§7.7.5) | Opus 5.5 | GPT-5.6/6, code mode | re-run needed if Codex is first-class (owner call) |

---

## 10. Integration recipe (Codex 0.155–0.157, Windows first)

All snippets are recommendations [I]; items marked "probe" must be confirmed by §12 before they are documented to users.

### 10.1 MCP server (`~/.codex/config.toml`, or a trusted `<repo>/.codex/config.toml`)

```toml
[mcp_servers.moirai]
command = "moirai"                 # absolute path if not on PATH
args = ["mcp", "--auto"]           # dual-era; answers 2025-06-18 initialize
# cwd omitted: Codex starts the server in the session cwd; the store is found via git-common-dir
env_vars = ["MOIRAI_CONFIG", "MOIRAI_LOG"]   # the default MCP env is a whitelist (§2.1)
required = true                    # a missing moirai is loud, not silent
startup_timeout_sec = 10
tool_timeout_sec = 60
default_tools_approval_mode = "approve"      # autonomous runs; use "writes" to prompt on claim/complete/remember/write
# no output_token_limit: moirai caps itself at 36,000 bytes under the 'codex' profile

[mcp_servers.moirai.tools.write]
approval_mode = "writes"           # optional: keep one prompt on the raw batch tool in interactive sessions
```

Server side (moirai, no user config): declare experimental capability `codex/sandbox-state-meta` to receive `sandboxCwd`; annotate `brief, pack, get, query, changes, branch` with `readOnlyHint: true`, `claim, complete, remember` with `readOnlyHint: false, destructiveHint: false, idempotentHint: true`, `write` with `destructiveHint: true` only when a batch contains an edge delete (annotations are static, so mark `write` destructive); instructions ≤ 512 characters; descriptions ≤ 200 characters that contain the search words agents use ("task", "blocked", "rule", "context pack", "claim", "finding").

### 10.2 CLI writes from the sandbox (`~/.codex/rules/moirai.rules`)

```python
prefix_rule(
    pattern = ["moirai"],
    decision = "allow",   # runs outside the sandbox, no prompt
    justification = "moirai writes its store under <git-common-dir>/moirai, which workspace-write keeps read-only",
    match = ["moirai ready --ids", "moirai complete 51 --lease L-9 --outcome done --summary -"],
    not_match = ["moiraix ready"],
)
```

Alternative without unsandboxed execution (probe): `[sandbox_workspace_write] writable_roots = ['<repo-root>\.git\moirai']` per repository (the main checkout's common dir, also for linked worktrees). Owner decision (§14 Q3).

### 10.3 Hooks (`~/.codex/hooks.json`; trust once with `/hooks`)

```json
{
  "hooks": {
    "SessionStart": [
      { "matcher": "startup|resume",
        "hooks": [ { "type": "command", "command": "moirai hook session-start --harness codex",
                     "timeout": 10, "additionalContextLimit": 2600 } ] },
      { "matcher": "clear|compact",
        "hooks": [ { "type": "mcp_tool", "server": "moirai", "tool": "hook_session_start",
                     "input": { "harness": "codex", "session_id": "${session_id}", "cwd": "${cwd}", "source": "${source}" },
                     "timeout": 10 } ] }
    ],
    "UserPromptSubmit": [
      { "hooks": [ { "type": "mcp_tool", "server": "moirai", "tool": "hook_prompt",
                     "input": { "harness": "codex", "session_id": "${session_id}", "cwd": "${cwd}" }, "timeout": 5 } ] }
    ],
    "SubagentStart": [
      { "hooks": [ { "type": "mcp_tool", "server": "moirai", "tool": "hook_subagent_start",
                     "input": { "harness": "codex", "session_id": "${session_id}", "agent_id": "${agent_id}",
                                "agent_type": "${agent_type}", "cwd": "${cwd}" }, "timeout": 10 } ] }
    ],
    "SubagentStop": [
      { "hooks": [ { "type": "mcp_tool", "server": "moirai", "tool": "hook_subagent_stop",
                     "input": { "harness": "codex", "agent_id": "${agent_id}", "agent_type": "${agent_type}",
                                "last": "${last_assistant_message}", "stop_hook_active": "${stop_hook_active}" }, "timeout": 10 } ] }
    ],
    "PostToolUse": [
      { "matcher": "^Bash$",
        "hooks": [ { "type": "mcp_tool", "server": "moirai", "tool": "hook_fs_evidence",
                     "input": { "harness": "codex", "cwd": "${cwd}", "command": "${tool_input.command}" }, "async": true } ] },
      { "matcher": "^apply_patch$",
        "hooks": [ { "type": "mcp_tool", "server": "moirai", "tool": "hook_edit_evidence",
                     "input": { "harness": "codex", "cwd": "${cwd}", "patch": "${tool_input.command}" }, "async": true } ] }
    ]
  }
}
```

Notes [I]: the `hook_*` tools are internal handlers the server accepts but does not list (probe that Codex calls unlisted tools); no `PreToolUse` stamp; `SessionStart` at startup stays a command because the server may not be ready [D]; whether `async` applies to `mcp_tool` handlers is undocumented (probe); `hooks.transport = command` generates the same file with `"type": "command"` entries (`moirai hook <event> --harness codex`) for hosts without the server.

### 10.4 Skills, AGENTS.md, custom agents

- Skills: `.agents/skills/moirai/SKILL.md`, `.agents/skills/moirai-orchestrate/SKILL.md`, `.agents/skills/moirai-ql/SKILL.md` (same files as `.claude/skills/…`), or user-level `~/.agents/skills/…`.
- `AGENTS.md` block (static, ≤ 3 lines, marker-delimited, written by `moirai export agents-md`):
  ```
  <!-- moirai:begin -->
  This repo tracks tasks, rules and findings in moirai. Start with `moirai brief`; before editing, `moirai pack <id> --role <role>`; report with `moirai complete`/`moirai finding`. Skill: $moirai.
  <!-- moirai:end -->
  ```
- Custom agents per role (`.codex/agents/developer.toml`, …): `name`, `description`, `developer_instructions` stating the marker convention (`moirai:task=#51 lease=L-9 branch=lane/demo role=developer`); for Bash-capable roles consider `mcp_servers` without moirai to avoid one server process per subagent (probe whether an agent file can disable an inherited server).

### 10.5 Dispatcher on Codex (replaces the Workflow pattern)

```powershell
# orchestrator (any shell or script)
moirai claim 89 90 --agent codex:dispatch --role developer --ttl run --run r7 --branch lane/demo --json > leases.json
& $codex exec --json -C <lanes-dir>\demo -s workspace-write --output-schema moirai-result.schema.json `
    -o out\89.json "moirai:task=#89 lease=L-18 branch=lane/demo role=developer. Implement, test, then return the result object."
moirai apply --from codex-exec out --run r7 --idempotency-key run:r7     # proposed adapter, §11 HA-8
```

`moirai-result.schema.json` is the same result schema the Claude Workflow agents return. Alternatives: the Codex SDK (`run(prompt, {outputSchema})`), or in-session `spawn_agents_on_csv` fed by `moirai ready --ids --csv` and ingested with `moirai apply --from codex-csv results.csv` [C feature; verify].

---

## 11. What the core must provide (harness-agnostic requirements)

Each item names the design-of-record section it changes. Confidence and evidence in the structured summary.

| # | Requirement [I] | Changes |
|---|---|---|
| **HA-1** | **Harness profile.** `harness = auto \| claude \| codex \| generic`, detected from MCP `clientInfo.name` at `initialize`/`discover` and, for CLI/hooks, from the environment (`CODEX_THREAD_ID` → codex; `CLAUDE_CODE_SESSION_ID`/`AI_AGENT` → claude; else generic). The profile supplies transport caps **in the transport's own unit** (`codex: mcp.result-max-bytes 36000, cli.output-max-bytes 36000, hook.context-max-bytes 9600`; `claude: the current char/token caps`; `generic: the strictest of both`), the refusal texts, and the tokenizer ratio table. | §7.1 output limits, §7.2 `mcp.result-max-chars`, §7.4 `pack.cli.max-chars`/`pack.mcp.max`, §7.5 hook budgets, §13 new `harness.*` keys |
| **HA-2** | **Caller-context resolver** with a fixed precedence: explicit (`--agent/--role/--branch/--lease`, MCP params, `MOIRAI_AGENT`) → lease binding → Codex `_meta` (`threadId`, `sessionId`, `x-codex-turn-metadata`, `codex/sandbox-state-meta.sandboxCwd`) → Claude stamp (`PreToolUse` context keyed by `(session, key)`) → environment (`CODEX_THREAD_ID/SESSION_ID`, `CLAUDE_CODE_SESSION_ID`) → `session:unknown`. Identities are namespaced (`codex:<thread>`, `claude:<agent_id>`). | §7.1 `--agent` default, §7.2 branch resolution and stamp, §6.4 default idempotency key |
| **HA-3** | **Role policy never depends on a hook.** The role comes from the lease (`claim --role`), the dispatch marker parameter, or `SubagentStart.agent_type` when present; unknown → the restrictive `general-purpose` row (already the fallback). | §7.3 |
| **HA-4** | **Every hook effect has a pull equivalent** and the server instructions/skill make the pull explicit (`brief` first, `pack` before work, `changes --since`). Hooks are accelerators; a harness without hooks, or with untrusted hooks, loses freshness, never correctness. | §7.5 (already partly: "missing, never wrong") |
| **HA-5** | **Output survives middle truncation**: drop counts and cursors in the first line *and* the footer (already), results always under the profile cap (so truncation never happens), and one text block per result. | §7.4 step 4 |
| **HA-6** | **MCP surface that fits both clients**: dual-era handshake incl. 2025-06-18; plain-object root schemas without `$ref/$defs` or root composition; short names; `readOnlyHint/destructiveHint/idempotentHint` on every tool; no `structuredContent` by default; `format: "json"` for programmatic (code-mode) callers; instructions ≤ 512 characters; descriptions with search keywords; hidden `hook_*` handlers. | §7.2, §8.3 TOKENS rows |
| **HA-7** | **Stateless, cheap server processes**: no warm-up scans at start (lazy open), ≤ 8 MB idle, correctness without per-connection state, session-slot anchor of [80 §2.7.2] taken lazily from the first call's `_meta.sessionId` (Codex passes no session id in the server environment) or from env (Claude). New gate: cold start to first `pack` ≤ 30 ms; RAM for N per-thread servers. | §6.1 MCP row, §6.2 holder anchor, §8.1 RAM rows |
| **HA-8** | **Ingestion adapters for `apply`**: `--from claude-journal:RUN` (today's `--from-journal`), `--from codex-exec DIR` (`-o` files or `--json` streams), `--from codex-csv FILE` (`result_json`), `--from jsonl FILE` (generic). Same idempotency key rules. | §6.4, §7.1 `apply` |
| **HA-9** | **Per-harness installers and doctors**: `moirai hooks install --harness claude\|codex`, `moirai skills install --harness …` (`.claude/skills`, `.agents/skills`), `moirai export agents-md`, `moirai doctor hooks --harness codex` (registered vs enabled vs **trusted**), `moirai doctor sandbox --harness codex` (can the CLI write the store here; which fix). | §7.1 integration verbs, §7.5 |
| **HA-10** | **Sandbox-aware refusals**: exit 7 prints the harness-specific fix (Codex: the rules line or the exact writable root; Claude: the `allowWrite` entry). | [80] sandbox bullet, §7.1 exit 7 |
| **HA-11** | **Tokenizer-neutral budgets**: `pack.cyrillic-weight` becomes a per-tokenizer table (`claude`, `o200k_base`, measured at M0); the token ledger (GT19) records the harness and model. | §7.4 units, §8.3 TOKENS |
| **HA-12** | **Plugin packaging for both**: one repo, two manifests, shared `skills/`, per-harness hook and MCP files; binary separate. | §7.5 packaging |
| **HA-13** | **Code-mode aware guidance**: when `clientInfo` says Codex, one extra instruction sentence ("in JavaScript, print `r.content[0].text`"); LQ-Bench includes a code-mode arm if Codex is first-class. | §7.2 instructions, §7.7.5 |
| **HA-14** | **No Claude-only file is required**: `MEMORY.md`, `.claude/rules`, Workflow journals and `CLAUDE_*` variables are optional inputs/outputs; nothing in the store, the CLI contract or the MCP contract names them. | §7.4 `export memory-md/rules`, §6.4 |

---

## 12. Probes (M0 for the contract, M10 for the server) — all on the owner's machine

1. Handshake: Codex 0.157 + moirai stub server — negotiated version (expect 2025-06-18), `instructions` delivery, where they appear (tool_search description; code-mode `exec` description), token cost of the listing.
2. `_meta` contents on a real call from the main thread and from a subagent: `threadId`, `sessionId`, `x-codex-turn-metadata`, and `codex/sandbox-state-meta.sandboxCwd` when declared. Check that hook `agent_id` equals the subagent's `threadId`.
3. Truncation: 30/40/48/60 KB results (ASCII and Cyrillic) in classic and code mode — where the cut lands and what marker the model sees.
4. Code mode: does `gpt-5.6-luna` print `content[0].text` or the whole object? Token cost of each (via `codex exec --json` `usage`).
5. Hooks: fire in the desktop app, CLI, IDE and `codex exec`? Trust flow for user vs plugin hooks; `mcp_tool` calling an unlisted tool; `async` on `mcp_tool`; `additionalContextLimit` on a 9.6 KB Cyrillic brief; `SessionStart` at startup racing the server.
6. Approvals: `approve` vs `writes` in the TUI, the app and `codex exec`; subagent approvals (#31565).
7. Sandbox, Windows elevated: can the CLI (as `CodexSandboxOffline`) read the store in the main checkout and in a linked worktree; write with (a) the rules `allow` entry — including PowerShell here-string pipelines, (b) `writable_roots = [<common-dir>/moirai]`; ACL of files created in (b); `LockFileEx` behaviour; liveness answers Unknown.
8. Sandbox, Linux/macOS (if decision #32 makes them first-class): same as 7 plus Seatbelt `fcntl` locks and bwrap OFD locks.
9. Per-thread servers: count moirai processes and RAM after spawning 6 and 16 subagents and closing them; check for leaks; cold-start latency.
10. Tool search: does BM25 find `mcp__moirai__pack` from the words agents use ("context for task 51")?
11. Tokenizer: o200k_base ratios for the M0 fixture (English, 20 % and 100 % Cyrillic) and, if published, the GPT-6 tokenizer.

---

## 13. Risks

| Risk | Likelihood / impact | Mitigation |
|---|---|---|
| Codex changes fast (0.15x releases weekly; docs moved domains; model names in flux) | high / medium | pin tested Codex versions in the M10 matrix; probes re-run per minor; rely on documented surfaces (MCP, hooks) over incidental ones (`_meta` keys are source-only [S]) |
| `_meta` keys are not a documented contract | medium / medium | treat as an optimisation after explicit params and the lease; fall back to env and hooks |
| Per-thread MCP servers multiply RAM and leak | high / medium (owner priority: minimal RAM) | CLI-first on Codex; lazy server; idle ≤ 8 MB; document the Codex bug; revisit when #38353 lands |
| Hook trust silently disables moirai hooks after an upgrade | medium / low | stable hook definitions; `doctor hooks` shows trust state; hooks are accelerators (HA-4) |
| Sandbox blocks CLI writes and the model loops on escalation prompts | high without the recipe / medium | rules entry or writable root; clear exit-7 text; MCP writes |
| Code-mode stringification inflates tokens | medium / low | instruction sentence (HA-13); `format` parameter; measure (probe 4) |
| LQ accuracy on GPT models unknown | medium / medium | LQ-Bench arm on the Codex model (owner call) |

---

## 14. Open questions for the owner

1. Is Codex a **first-class** target (its own gates, token ledger, LQ-Bench arm on the GPT model, M10 test matrix) or **supported best-effort** (recipe + probes only)? A first-class LQ-Bench arm roughly doubles the M0 model-token spend (≈ 45–90 M more tokens).
2. Which Codex surfaces count: local CLI, desktop app and IDE only, or also **Codex cloud** (no local store, no local MCP; read-only through the git image at best)?
3. How should sandboxed Codex agents write: execpolicy `allow` for `moirai` (runs moirai unsandboxed), a per-repo writable root on `<common-dir>/moirai`, or MCP-only writes?
4. MCP approval mode for moirai's write tools: `approve` (autonomous) or `writes` (a prompt per write in interactive sessions)?
5. Which model do Codex agents use (the local default is `gpt-5.6-luna`, code mode, `xhigh`)? It sets the tokenizer, the code-mode question and the LQ-Bench target.
6. May one campaign mix harnesses (e.g. a Claude orchestrator dispatching `codex exec` workers against the same store)? The store handles it; identities, leases and the token ledger need the harness tag.
7. Package as a Codex plugin in a marketplace, or have `moirai install --harness codex` write `config.toml`, `hooks.json`, rules and skills directly?

---

## 15. Sources

Official documentation (all fetched 2026-09-26; `developers.openai.com/codex/*` redirects to `learn.chatgpt.com/docs/*`):
- MCP: https://learn.chatgpt.com/docs/extend/mcp?surface=cli
- Configuration reference: https://learn.chatgpt.com/docs/config-file/config-reference
- Hooks: https://learn.chatgpt.com/docs/hooks
- AGENTS.md: https://learn.chatgpt.com/docs/agent-configuration/agents-md.md
- Skills: https://learn.chatgpt.com/docs/build-skills
- Subagents: https://learn.chatgpt.com/docs/agent-configuration/subagents.md
- Rules (execpolicy): https://learn.chatgpt.com/docs/agent-configuration/rules.md
- Approvals and security: https://learn.chatgpt.com/docs/agent-approvals-security
- Windows sandbox: https://learn.chatgpt.com/docs/windows/windows-sandbox
- Non-interactive mode: https://learn.chatgpt.com/docs/non-interactive-mode
- Custom prompts (deprecated): https://developers.openai.com/codex/custom-prompts
- Codex SDK: https://developers.openai.com/codex/sdk ; https://github.com/openai/codex/blob/main/sdk/typescript/README.md
- Releases: https://github.com/openai/codex/releases (0.157.0, 0.157.1)

Source code (`openai/codex`, `main`, 2026-09-26; raw files under https://raw.githubusercontent.com/openai/codex/main/):
- `codex-rs/core/src/mcp_tool_call.rs` (request `_meta`, result sanitizing)
- `codex-rs/protocol/src/models.rs` (`as_function_call_output_payload`, structuredContent preference)
- `codex-rs/utils/output-truncation/src/lib.rs`, `codex-rs/utils/string/src/truncate.rs` (bytes/4, middle truncation, ×1.2)
- `codex-rs/rmcp-client/src/protocol_mode.rs`, `rmcp_client.rs`, `utils.rs` (protocol modes, env whitelist); `codex-rs/Cargo.toml` (rmcp =3.2.0)
- `codex-rs/codex-mcp/src/rmcp_client.rs`, `runtime.rs`, `tools.rs` (instructions as namespace description, `codex/sandbox-state-meta`, name limits)
- `codex-rs/core/src/tools/handlers/tool_search*.rs`; `codex-rs/code-mode-protocol/src/description.rs` (tool search, code mode)
- `codex-rs/hooks/src/engine/{mcp_runner,command_runner,discovery}.rs`, `hooks/src/output_spill.rs`, `hooks/src/events/{pre_tool_use,common,session_start,stop}.rs`
- `codex-rs/core/src/session/session.rs` (`session_id` shared by descendants); `codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs`
- `codex-rs/core/src/exec_env.rs`, `codex-rs/protocol/src/shell_environment.rs` (`CODEX_THREAD_ID`, `CODEX_SESSION_ID`)
- `codex-rs/core/src/agents_md.rs`; `codex-rs/ext/skills/src/loader/*`
- `codex-rs/protocol/src/permissions.rs` (`PROTECTED_METADATA_PATH_NAMES`, writable-root exemption); `codex-rs/sandboxing/src/seatbelt*.{rs,sbpl}`; `codex-rs/linux-sandbox/src/{bwrap,landlock}.rs`
- `codex-rs/shell-command/src/{shell_detect,powershell}.rs`
- `AGENTS.md` of the codex repo (`CODEX_SANDBOX*` variables); `codex-rs/skills/src/assets/samples/openai-docs/references/latest-model.md`

Pull requests and issues (third-party-visible, [C] unless read as code):
- PR #29486 (MCP tools always deferred, 2026-06-22): https://github.com/openai/codex/pull/29486
- PR #41421 (per-tool MCP output limits, 2026-08-28): https://github.com/openai/codex/pull/41421
- PR #20260 (truncate MCP outputs in rollouts): https://github.com/openai/codex/pull/20260
- PR #24118 (oneOf/allOf in tool schemas): https://github.com/openai/codex/pull/24118
- PR #46500 (Seatbelt fcntl denials): https://github.com/openai/codex/pull/46500
- #7635 MCP tools don't respect sandboxing: https://github.com/openai/codex/issues/7635
- #12333, #25015, #37453, #38353 per-thread MCP processes and leaks: https://github.com/openai/codex/issues/38353
- #17468 `x-codex-turn-metadata` contents: https://github.com/openai/codex/issues/17468
- #21503 tool_search misses named tools; #33266 list_changed ignored; #34719 post-compaction tool loss
- #24135 MCP calls auto-cancelled in `codex exec`: https://github.com/openai/codex/issues/24135
- #31565 approval elicitation in delegated subagents
- #3152, #13746 `$ref` in MCP schemas
- Discussion #26901 (command environment, `CODEX_THREAD_ID`): https://github.com/openai/codex/discussions/26901
- anywhere-agents #50 (hooks at 0.153.3): https://github.com/yzhao062/anywhere-agents/issues/50

Secondary:
- InfoQ, Codex Windows sandbox design (2026-06-05): https://www.infoq.com/news/2026/06/codex-windows-sandbox-design/
- Codex Knowledge Base (D. Vaughan): subagents/TOML and `spawn_agents_on_csv` (2026-03-26, updated 2026-09-26): https://codex.danielvaughan.com/2026/03/26/codex-cli-subagents-toml-parallelism/ ; MCP annotations and approvals: https://codex.danielvaughan.com/2026/04/12/mcp-tool-annotations-risk-vocabulary-codex-cli/
- sunpeak, MCP server instructions for ChatGPT/Claude (Sep 2026): https://sunpeak.ai/blogs/mcp-server-instructions-chatgpt-claude/
- Codex cloud limits: https://developers.openai.com/codex/cloud/environments ; https://www.agent37.com/blog/codex-cloud
- o200k for GPT-5.6: https://tiktokenizer.com/ ; https://github.com/miurla/morphic/pull/1021
- Claude-to-Codex migration (`/import`): https://codex.danielvaughan.com/2026/05/13/codex-cli-agent-migration-system-import-claude-code-sessions-skills-config/

Local observations [M] (owner's machine, 2026-09-26, read-only): `Get-AppxPackage OpenAI.Codex`; `codex.exe --version|--help|features list|mcp --help|exec --help|sandbox --help|plugin --help`; `~/.codex/config.toml` (secrets not read), `~/.codex/models_cache.json`, `~/.codex/.sandbox/*`; bundled plugin manifests in the app package; `$PSVersionTable`.
