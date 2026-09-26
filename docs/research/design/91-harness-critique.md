# 91 — Critique of 90 (harness-agnostic design and the cargo-check gate)

*Adversarial review of `docs/research/design/90-harness-agnostic-design.md` ([90]) against the harness research [H21] `research/21-harness-openai-codex.md`, [H22] `research/22-harness-capability-matrix.md`, [H23] `research/23-model-agnostic-tokens-queries-tools.md`, the design of record [AR] `docs/ARCHITECTURE-RESEARCH.md` and [50], [60], [80]. Date: 2026-09-26. Research only: no code, no configuration changed, nothing installed. Public docs.rs pages were read for one fact check.*

**Owner decisions under test.** (A) "It must work not only for Claude Code but also for Codex and other harnesses." (B) "Do cargo check for Linux and Mac." — a gate in the local gate from M0; pure-Rust dependencies only.

---

## 0. Verdict

**Revise before integration: 2 blockers, 12 majors, 12 minors.**

The architecture of [90] is sound: contract C0, the Tier A/B/C split, "hooks are accelerators", text-only results, byte budgets, `result.v1` with `apply --from`, and GT20 (e) all follow the research. The edit list is mechanically exact: all 155 Find texts occur exactly once. The problems sit in four places:

- **Two facts that would be frozen at M0 are wrong.**
  - `ruzstd` 0.9.0 cannot write the stored format. Its encoder has no dictionary API, and its docs mark levels Default, Better and Best as unimplemented. The codec consequence of #44 therefore does not hold (B1).
  - The Codex liveness anchor hashes the root session, but slots live per thread. Once the thread servers that held a session's slots exit, a live agent's lease is declared Dead (B2).
- **The identity and role chain fails open without hooks.**
  - `policy.unleased-root-role` grants orchestrator rights where "root" is only assumed.
  - Inherited `MOIRAI_*` environment variables outrank per-thread identity.
  - A declared `agent` outranks the lease holder.
  - A Codex worker spawned from Claude Code is detected as Claude.
- **Budget claims fail in each harness's own unit.**
  - In code mode, several results printed by one `exec` share a single 40,000-byte cut.
  - The "worst family" gate for MCP results cannot pass at 25,000 B.
  - `write.ops` fails the design's own three-validator strict check.
- **The edit list is incomplete.** It leaves live "non-gating", "dispatch label", C-allocator and `-chars`/600-character statements in [60], [50], [80] and [AR]. That contradicts decision B's "change that everywhere".

---

## 1. What was checked, and how

| Check | Method | Result |
|---|---|---|
| Every Find text of [90 §12] occurs once in its target | a script that parsed §12.1–§12.5 and counted each Find text in the current documents | **155 / 155 exactly once** |
| What remains after applying the edit list | a script applied all 155 Find→Replace pairs to copies, then searched for `non-gating`, `dispatch label`, `mimalloc`, `C allocator`, `-max-chars`, `600 char`, `weighted` outside the review logs | **residual live contradictions** (M8) |
| Instruction and block sizes | counted | instructions 430 chars, `codex` profile 476, `AGENTS.md` block 556 B, all ASCII: **correct** |
| Calendar (§10.3) | re-ran the calendar Monte Carlo script with [90]'s units (probe scripts are not published) | reproduces P50 42 / P90 50.5 (30–60) **only if M0's 4–5.5 units are split half per lane**; adding them to each lane gives P50 42 / P90 51 (30–61) (m9) |
| `ruzstd` 0.9.0 claims ([90] header, §11.3) | docs.rs: crate page, `encoding::CompressionLevel`, `encoding::FrameCompressor`, the all-items list | **contradicted** (B1) |
| Codex, Claude Code and Gemini behaviour claims | cross-read against [H21]–[H23] | mostly faithful; the deviations are in B2, M1, M2, M4, M7 and m1 |

---

## 2. Blockers

### B1. `ruzstd` 0.9.0 cannot write the frozen `blobs`/`dict.D` format

**Claim in [90].**
- Header: `ruzstd` 0.9.0 "compresses at levels Uncompressed/Fastest/Default/Better/Best, supports dictionaries for encoding and decoding and has a `dict_builder` trainer" [D, docs.rs].
- §10.1 and §11.3: the stored format "stays RFC 8878 frames and dictionaries, so no frozen byte moves".
- §11.4: `ruzstd` "keeps the zstd format and ratio class".

**What docs.rs shows for 0.9.0** (read 2026-09-26):
- **Levels.** `encoding::CompressionLevel` documents `Default`, `Better` and `Best` each as **"UNIMPLEMENTED"**; only `Uncompressed` and `Fastest` (≈ zstd level 1) are implemented. The crate README says "any compression level", which contradicts the enum's own docs.
- **Encoding with a dictionary.**
  - `encoding::FrameCompressor` has no dictionary method: only `new`, `new_with_matcher`, `set_source`, `set_drain`, `compress`, `set_compression_level`, `replace_matcher` and the accessors.
  - The all-items list shows dictionary types only on the decoding side (`decoding::Dictionary`, `DictionaryDecodeError`).
- **Training.** The README says the `dict_builder` feature creates only **"raw content" dictionaries; "tagged dictionaries are currently unsupported"**.
- **Decoder speed.** 1.4–3.5× slower than C zstd, as [90] says.

**Failure scenario.** [AR §4.1] stores bodies as "zstd-dictionary frames" with `dict.D` "retrained at rollup". Under #44 moirai has no way to *write* a dictionary frame:
- M0 item 6 ("dictionary ratio … of `ruzstd` at Fastest/Default/Better") cannot be run as written.
- The codec byte, the `dict.D` content (formatted versus raw-content dictionary, and whether a dictionary id appears in frame headers) and the ratio assumptions behind the disk and RSS rows would be frozen on an encoder that does not exist.
- The fallback "a hand-written zstd-format encoder with `ruzstd` decoding" is a real own-code component: a match finder, FSE/Huffman table construction and dictionary-seeded history. It has no estimate in §10.3.

**Fix.**
1. Correct the [D, docs.rs] statements.
2. Make the codec an explicit M0 pre-freeze decision item with three costed options:
   - (a) an own dictionary-aware zstd-format encoder at a Fastest-class level, decoded by `ruzstd`, with the `zstd` CLI as the independent oracle (estimate it in M0/M1);
   - (b) upstream dictionary encoding in `ruzstd`, pinned to a released version (a third-party schedule risk);
   - (c) a different codec or no dictionaries, decided before the freeze (for example `lz4_flex`, after verifying its block-dictionary API).
3. Freeze `dict.D` explicitly as a raw-content or a formatted dictionary.
4. Re-state M0 item 6 as what is implementable.
5. Add the units to §10.3.

### B2. The Codex liveness anchor would release live leases, and it would be frozen at M0 as the X-F2 amendment

**Design.** [90 §4.4] and §10.1:
- A Codex server's slot record uses primary = `codex:<_meta.sessionId>` (the root session, shared by every thread) and alias = `codex:<threadId>`.
- The slot is taken lazily at the first call that carries `_meta.sessionId`.
- The CLI matches `CODEX_SESSION_ID`.
- [AR §6.2]: a lease is **Dead** when "no held slot's record names" its session, and Dead is released at the next read.

**Mismatch.** Under Codex a slot lives as long as one *thread's* server, not as long as the session ([H21 §2.1]: one server per thread). With "CLI first on Codex" (P-H7) and lazy slots, the root thread typically never holds a slot.

**Failure scenario.**
1. The root thread R works through the CLI.
2. Subagent S1 makes one MCP call, and its server takes a slot naming `codex:<session>`.
3. R self-claims task 89 through the CLI. The CLI finds S1's slot, so R's lease gets kind `session` anchored to the root session.
4. S1 finishes and its server exits. That is normal behaviour once the leak bugs #12333/#38353 are fixed; today's leaks only mask it.
5. No held slot names the session any more, so R's lease is Dead and is released at the next read.
6. Another agent claims 89. R's `complete` fails with exit 5 and its work is refused or duplicated.

The design argues only the opposite direction ("leaked servers … delay only an early release").

A second, related gap: [H21 §4.2] says Codex hook input `session_id` is **the thread**, not the root session. [90]'s registry (§3.4) and templates (§3.7) treat it as a session id, so hook-side slot and alias matching is inconsistent with `_meta.sessionId`.

**Fix.**
- Hash as primary the id whose lifetime the process actually tracks. For a Codex per-thread server that is `codex:<threadId>`, with the root session kept as the alias for grouping only.
- The CLI matches `CODEX_THREAD_ID`.
- A lease taken on a thread whose server holds no slot gets kind `none` (TTL).
- Alternatively, flag anchors from per-thread harnesses so that "no slot names it" reads **Unknown**, never Dead.
- State that Codex hook `session_id` is a thread id.
- Add the R/S1 scenario to the GT2/GT18 differential and to probes P2 and P8.

---

## 3. Majors

### M1. The role policy fails open without hooks, and lease-minting itself is unguarded

- [90 §4.3] gives an unleased caller "proven **or assumed**" to be a session's root the role `policy.unleased-root-role` = `orchestrator` ("everything on any branch", DELETE, merges; [AR §7.3]).
- Root is assumed in every Tier B/C harness. It is also **always** assumed for Claude Code CLI calls, even with hooks: a subagent's Bash call carries the parent's `CLAUDE_CODE_SESSION_ID`, the stamp covers MCP writes only, and no hook attaches an identity to a CLI invocation. The sentence "with hooks installed the subagent is narrowed" (H-R8) is false for the CLI.
- **Scenario.** An ad-hoc Explore or general-purpose subagent that honestly runs `moirai rm 40` or `moirai merge …` is allowed. That is exactly the honest mistake the policy exists to stop.
- [H21] HA-3, [H22] K3/C0.5 and [H23 §6.2] rule 1 all keep "unknown → `general-purpose`", which is [AR §7.3]'s current fail-closed default.

Further gaps:
- `claim ID --role R` and `claim --role R --run ID` (role leases) can be issued by any caller for any role. "Grants come only from leases" is therefore only as strong as `claim`'s own authorization, and that is undefined.
- The role of a role-less self-claim is undefined. C0's own `AGENTS.md` flow is `moirai claim ID` then `complete`, so `complete` may be refused under the new rule.
- "Selects the narrower row" is undefined for incomparable rows (architect vs tester).

**Fix.**
- Orchestrator rights also come from a lease: a session role lease (`moirai claim --role orchestrator --session`) taken by the Tier A `SessionStart` hook, or by the first step of the orchestrate skill.
- Unleased callers get `general-purpose` everywhere.
- Policy rows state who may mint role leases (orchestrator or owner only) and which roles may be self-claimed (`policy.self-claim-roles`, default developer and tester).
- A role-less self-claim defaults to developer.
- "Narrow" is the intersection of the two rows.

### M2. Inherited environment is ranked as explicit identity, and a declared agent outranks the lease

- [90 §4.1] rank 1 puts the dispatcher's `MOIRAI_*` variables beside explicit flags and MCP arguments, and `env_vars` forwards them into every Codex thread's server.
- Environment variables are per *process*. [H23 §6.1] calls them "useless for in-process subagents sharing one server", and R-MA-29 limits them to one-process-per-worker dispatch.

**Scenarios.**
- **(a) Role bypass.** A `codex exec` worker (MOIRAI_LEASE=L-18, role developer) spawns a `code-reviewer` subagent. The subagent's CLI and its server inherit L-18 and write with developer rights; without hooks nothing narrows it.
- **(b) Identity collapse.** `MOIRAI_AGENT` outranks `_meta.threadId`, so all threads become one agent. The default idempotency key BLAKE3(session, agent, AST) ([AR §6.4]) then collides: two subagents each running `claim --next --role developer` → the second receives the first's replayed lease, and two agents work one task.
- **(c) Attribution spoofing.** "Conflicts are refused" covers only `branch` and `tree`. An agent holding L-18 can pass `agent: architect`, and the commit records actor = architect (`actor_src = declared`). [H23 §6.2] rule 2 orders lease holder → hook → declared.
- **(d) Stray variables.** User-scope `env_vars` forwards any stray `MOIRAI_*` in the owner's shell into every session.

**Fix.**
- Actor resolution: lease holder > `_meta`/stamp attestation > declared > environment. A declared agent that differs from the lease holder is refused (exit 5) or recorded as `holder/declared`.
- `MOIRAI_*` rank below `_meta.threadId` and the stamp. An environment-supplied lease applies only to the root thread (`threadId == sessionId`); subagents pass a lease explicitly.
- The default idempotency key includes the thread id when one exists.

### M3. Mixed-harness campaigns: a Codex worker spawned from Claude Code is detected as Claude

- §7.6 says "nothing else changes". But a `codex exec` worker started from a Claude Code Bash tool inherits `CLAUDECODE`, `CLAUDE_CODE_SESSION_ID` and `AI_AGENT` ([H23 §2.1] shows them in Claude Code's environment).
- The detection table (§4.1) tests Claude first. The worker's CLI therefore gets:
  - the `claude` profile and Claude exit-7 texts;
  - self-claims anchored to the *orchestrator's* Claude session (Alive for as long as the orchestrator lives);
  - under M1, orchestrator rights for its unleased calls.

**Fix.**
- Detection precedence by specificity: variables set per command by the innermost harness (`CODEX_THREAD_ID`) beat inherited session variables.
- The dispatcher recipe scrubs `CLAUDE*`, `CODEX_*` and `AI_AGENT` from worker environments.
- GT12 fixtures for Claude→Codex and Codex→Claude nesting.

### M4. Code mode breaks "no harness ever cuts a moirai result"

- The owner's `gpt-5.6-luna` is `code_mode_only` ([H21 §1]). Nested MCP results return to JavaScript, and only what the program prints reaches the model. That output is capped at the `exec` tool's `max_output_tokens` of 10,000 approximate tokens ≈ 40,000 B, with a middle cut ([H21 §2.3]).
- The cap applies to the whole `exec`, yet §6.5 advises "batch reads in one exec". Two 25,000-B packs, or one 36,000-B `codex`-profile result plus anything else, exceed it.
- A middle result then loses both its first and its last line. `text(r)` escaping inflates the output further. "Print only what you need" invites programs that drop the header and footer lines.
- The pinned per-tool `output_token_limit` values (§3.7) probably do not apply to nested calls. That is unverified.

**Fix.**
- In the `codex` profile, cap MCP results at about 16,000 B, or instruct one pack per `exec`.
- Keep 36,000 B only for classic (non-code) mode.
- The code-mode sentence says "print the first and the last line of every result".
- P3 and P4 test batched `exec` calls.

### M5. The MCP result gate uses the wrong unit and cannot pass

- AR-H36 and §9.2: "Pack through MCP ≤ 25,000 B; under Claude Code's 10k-token warning **by the worst family on each fixture class**".
- By §9.1's own ratios:
  - id-dense lists are 25,000 tokens in Gemma (1.0 B/token) and ≈ 14,300 in o200k (1.75);
  - code is ≈ 9,300 Claude tokens (2.69 B/token), a 7 % margin.
- The claim "under the 10k-token warning for every measured content class" (§6.1, §10.8, AR-H24, AR-H41) is therefore false as stated. Claude's id-dense ratio is not even measured ("M0").
- The deeper error is mixing units. Claude Code's warning counts Claude tokens, Codex counts bytes/4, and Gemini CLI counts characters. A harness cap judged by another vendor's tokenizer is meaningless.

**Fix.**
- Check each harness cap only in its own unit.
- Check cost budgets (session, per spawn) with the tokenizer of the model that harness actually runs.
- Keep a worst-family maximum, if at all, only for shared static text (card, skills).
- Page id-dense MCP output at a lower byte ceiling (the 8,000-B query page already does this; extend it to `pack` id lists).

### M6. `write.ops` contradicts the portable schema profile and its own CI check

- MPSP rule 3 allows "at most one level of `object` under the same rules", so `additionalProperties: false` applies. `write.ops` items are "left without an inner schema".
- An object with no properties and `additionalProperties: false` accepts only `{}`.
- Without `additionalProperties: false`, the object fails OpenAI's strict normalisation and Claude's strict tool use, both of which require it ([H23 §4.1]).
- Rule 10's three-validator CI check therefore either fails on `write`, or `ops` becomes unusable under any strict consumer.

**Fix.**
- Since `tx` is primary, remove `ops` from the MCP surface (keep it for CLI `--json`), or make it an array of strings, each a JSON-encoded op.
- State rule 10's expected result for every tool.

### M7. C0.2 store discovery rests on an unverified harness behaviour

- Discovery order is `--store → MOIRAI_DIR → walk-up from the server's working directory → git hint`.
- With the default **user-scope** entries, no project path can be written into the harness configuration.
- The server's cwd is stated for no harness except Codex, and even there it is an inference ([H21 §10.1] comment).
- [H22 K5] kept the per-call `tree` parameter as the last discovery fallback; [90] dropped it.

**Scenario.** A Tier B or C harness that starts stdio servers in its install directory or `$HOME` gets "no store" on every call. The REQUIRED contract then fails in exactly the harnesses that have no other surface than MCP.

**Fix.**
- Also discover from the per-call `tree` or Codex's `sandboxCwd`.
- The instructions add "pass `tree` = your working directory if moirai reports no store".
- `integrate` writes `cwd`/`--store` where the harness expands workspace variables.
- GT12 adds a "server cwd" row per harness.

### M8. The edit list leaves live contradictions

After all 155 edits were applied by script, these statements remain outside the review logs:

| Location | Residual text | Conflicts with |
|---|---|---|
| [60 §1.3] (deferred-items table, OS-layer row) | "a non-gating cross-target type check" | decision B, 60-H2 |
| [60 §3.2] M1 size basis | "+ 0.5–1 for the non-gating cross-target type check ([80 §5.4])" | decision B; the unit moved to M0 (80-H6) |
| [60 §2.3] C9 edge row; [60 §3.11] M10 scope | "role policy is keyed on C9's dispatch label"; "the role policy on the dispatch label" | AR-H9, AR-H25, AR-H27 |
| [50 §6.5] | "keyed on the dispatch label" | AR-H27 |
| [80 §5.2] port probes | "allocator × libc matrix (glibc + system, musl + mimalloc, musl + jemalloc)" | #44, 80-H3 |
| [80 §5.5] (a) | "the C allocator chosen under §2.9" in the allow-list | #44; the same phrase was removed from [AR] and [60] |
| [AR §7.1] `pack` line | "capped by pack.cli.max-chars (24,000)" (M-2 does not cover §7.1) | AR-H41 |
| [AR §9] M10 row; [60 §5.4] M10 row | "instructions ≤ 600 chars" | ≤ 512 (AR-H26, AR-H36) |
| [AR §10] tokens risk row | "weighted units for Cyrillic" | AR-H8, AR-H28 |
| [AR §7.7.4]; [50 §0.4] | "8,000 characters per page" | `query.budget.default.bytes` (AR-H41) |

Decision B says to change the non-gating wording everywhere, and [90]'s own checks §12.7 items 2 and 5 would fail.

**Fix.** Add edits for each row, and extend M-2 to [AR §7.1], §7.7.4 and §10.

### M9. Codex per-thread servers: the RAM aggregate claim is unsupported

- [90 §4.5] says "the existing 16-session aggregate (Σ ≤ 256 MB …) holds with the Codex profile". Under Codex the process count scales with **threads**, not sessions.
- Leaked servers ([H21 §2.1], open issues through August 2026) that were *used* keep their maps and overlay (≤ 8 MB + 1 MiB), not the 2.5 MB baseline of an unused server, until the Codex app exits. The desktop app runs all day.
- **Scenario.** Five fan-outs of six subagents in one app session leave about 30 used, leaked servers, ≈ 270 MB. That breaks the aggregate on a laptop with ≈ 1.8 GB free. Their slots leak too.

**Fix.**
- The `codex` profile releases every per-view structure and the overlay at request end (`mcp.overlay-bytes.codex = 0`; reopening costs ≤ 1.5 ms), so an idle used server returns to near the baseline.
- Restate the aggregate gate per server count, with a leak scenario in P8.
- `doctor` reports leaked servers and slot use.

### M10. Model profiles: an undeclared model defaults to `unknown`, and the `unknown` rules conflict with each other and overclaim

- **(a) Default.** No declared model means `unknown`. Claude Code's `SessionStart` carries `model` only "not always" ([H23 §6.2]), and interactive Codex sessions set no `MOIRAI_MODEL`. So the owner's own Opus sessions and Luna sessions become `unknown`, with named-only writes: a Tier A regression.
- **(b) L2 vs L3.** L2 allows `DRY → IF TARGETS` for `unknown` models. L3's default `query.safelist.model.unknown = named-only` forbids any free-form write. §10.7 and AR-H39 state both.
- **(c) Overclaim.** "`IF TARGETS` after a shown `DRY` catches right count, wrong nodes" holds only if the model reads the listing. For weak models it is a race guard, not an intent check.
- **(d) Reads.** Free-form reads by `unknown` models stay free-form, without a forced reading echo, although 52 % of small-model errors are valid-but-wrong ([H23 §3.1]).

**Fix.**
- A key `lq.model-profile.default.<client>` (claude → the gated Claude result, codex → the Luna result), and record a hook's `model` field whenever it is present.
- One rule for `unknown` writes: named-only by default, `DRY → IF TARGETS` opt-in.
- The `DRY` output lists titles.
- The reading echo is always on for `compatible` and `unknown` models.
- The floor tier reports the read confident-wrong rate.

### M11. Scope and cost creep beyond decision A, without an owner decision

Only the Tier B templates sit under #45. The following are built by default in M0 or M9–M10:
- `integrate package` (Agent Plugins 1.0 + Codex + Claude manifests);
- `export rules` in four formats;
- the `gemini`, `copilot` and `cursor` client profiles;
- Gemini CLI as the third conformance harness, although Gemini CLI is enterprise-only for new consumers ([H22 §1]);
- a three-tokenizer ledger including Gemma, whose tokenizer is a gated download under Gemma's terms;
- hand-written validators for three vendors' schema dialects;
- the `codex-csv` adapter and `--structured`;
- the LQ-Bench compatibility tier.

The total is ≈ 16–24 units (two-lane P50 + 3 weeks). The calendar basis still excludes [80]'s 9.5–16.5 units and the audits' deltas, so the headline "P50 42" understates the release date.

**Fix.**
- Default scope: Tier A + generic C0.
- The third conformance arm becomes a scripted generic stdio client (the neutral runner) instead of Gemini CLI.
- Tokenizers: Claude and o200k only (the families the owner runs).
- Move `integrate package`, the extra `export` formats, the extra profiles and the compatibility tier under #45.
- Print the combined calendar with [80] and the audits.

### M12. GT20 (e) and the pure-Rust rule have enforcement gaps

- **(a) The binary crate is excluded without a size limit.** If CLI parsing, the MCP front-end or rendering live in `moirai`, the product's own code escapes decision B.
- **(b) The lint misses direct compiler calls.** It only catches `links` and known build-dependencies (`cc`, `cmake`, …). A build script that calls a compiler through `std::process::Command` is caught by neither layer; the poisoned `CC_*` variables only affect `cc`-crate users.
- **(c) Dev-dependencies leak.** `--all-targets` compiles dev-dependencies, so a product crate that dev-depends on the tree-sitter oracle crate fails the gate. The dependency direction (oracle → product, never the reverse) is not stated.

**Fix.**
- The binary crate is a composition root only: `main.rs` wires `moirai-os` into a checked `moirai-app` library.
- The lint requires every package with a `custom-build` target to be in `native-allow.toml` with a reviewed reason.
- A lint rule: no checked crate depends on a host-only crate, dev-dependencies included.

---

## 4. Minors

| # | Problem | Fix |
|---|---|---|
| m1 | Exit 7's "do now" says to use MCP and "do not request escalation", but MCP is the volatile Codex path: tool-search misses (#21503), tool loss after compaction (#34719), GPT-5.6 models not seeing MCP tools (#35153), and `required = false` with `mcp_optional_startup_grace_ms` 1,000 ms and no `list_changed` refresh ([H21 §2.1, §2.3]). If P7 switches the default to `mcp`, CLI-first Codex roles lose every write. | Exit-7 text adds "if the moirai tools are unavailable, put the write in your final result.v1 or tell the user". P7's fallback is a narrowed execpolicy rule for store-only verbs (claim, complete, remember verbs, heartbeat, release; not `file mv` or `image export`) instead of `mcp`. P1 measures cold start against the 1,000 ms grace. |
| m2 | Whether Codex command hooks run sandboxed is "[I, probe]" in the elevated row but in no probe. `SessionStart`'s R4 settle and daily image export into `.git` ([AR §7.5]) would then fail open silently, and the git image goes stale during Codex-only periods. | Add hook sandboxing to P5/P7. The brief prints a triage line when a hook write was refused. |
| m3 | Hand-written TOML: tables appended at the end of `config.toml` capture any bare key a human appends later. A duplicate `[sandbox_workspace_write]`, a dotted key or a profile form would break Codex's configuration for every session. A "minimal reader" does not cover dotted keys, inline tables or profiles. | Validate the whole file with a complete parser after writing (own code, or the pure-Rust `toml` crate inside `integrate` only), and keep a backup. Prefer the Codex plugin for the MCP entry and hooks, leaving only `writable_roots` in `config.toml`. |
| m4 | Workers "record their work with moirai" **and** return findings and notes in `result.v1`. `apply` keys per run and task, while direct writes use content-hash keys ([AR §6.4]), so findings are duplicated. | `result.v1` lists ids of findings already written, or `apply` uses the same content-hash key per finding. |
| m5 | Unverified template fields: `${tool_input.command}` for `apply_patch`, and whether `PostToolUse ^Bash$` fires for shell calls nested in a code-mode `exec`. A missing field fails an async `mcp_tool` hook silently. | Add both to P5; keep `edit-evidence` off on Codex until P5 passes. |
| m6 | `integrate.codex.approval = approve` also covers `write` (destructive; edge deletes) in interactive sessions. [H21 §10.1] kept `tools.write.approval_mode = "writes"`, and [H22 §9] recommended `prompt`. | Interactive user scope: `approve` for reads and claim/complete/remember, `writes` for `write`. Headless workers run with a Codex profile (`codex exec -p moirai-worker`) that sets `approve`. |
| m7 | The block's position in `AGENTS.md` is unspecified. Codex stops adding files at 32 KiB concatenated ([H21 §3.1]), so a block at the end of a large file can drop out. | Insert the block at the top; `integrate --check` warns above 28 KiB. |
| m8 | Data leaving the machine outside #38: P10 and M0 item 6 send fixture text (the owner's notes for item 6) to Anthropic's `count_tokens` and, through `codex exec`, to OpenAI; the Gemma tokenizer requires accepting Gemma's terms. | Use synthetic fixtures, or add these sends to #38's vendor question. |
| m9 | The calendar reproduces only with M0's units split half per lane (unstated). The 0.5–1 unit moved from M1 to M0 is still listed in [60 §3.2]. | State the split; fix [60 §3.2]; print the combined figure (see M11). |
| m10 | The floor tier's "≈ 9B model on the laptop" needs ≈ 5–6 GB at 4-bit against ≈ 1.8 GB free. The C0 arm's fallback, "OpenCode driven by the floor model", confounds model failures with transport failures. | Run the floor tier only on the neutral runner and the test host; run the C0 transport arm with a gate model. |
| m11 | `--ids` output stays uncapped on the CLI ([H23 §4.4]). In Codex's 40,000-B shell cut, a large `moirai ready --ids` loses middle ids, with only Codex's marker to show it. | Cap `--ids` at the profile's CLI ceiling, with a count footer and a cursor. |
| m12 | [80]'s review log uses "former #42" and "former #43" for the withdrawn type-check and platform-matrix questions, while [90] assigns #43 and #44 to new decisions. | Add a one-line numbering note to AR-H39 and 80-H9. |

---

## 5. What holds (checked, no change needed)

- **C0's shape.** Stdio MCP plus the CLI covers every live harness ([H22 §1, §3]). Tools only, text in `content[0]`, no `structuredContent` by default: correct for all three delivery paths ([H23 §4.2]).
- **Dual-era handshake** including 2025-06-18 for Codex ([H21 §2.2]).
- **Names and sizes.** Server name `moirai`; tool names ≤ 16 characters; instructions ≤ 512 characters (430/476 verified); the `AGENTS.md` block (556 B) with a `CLAUDE.md` import line; portable skills in `~/.agents/skills` and never also in `.claude/skills` ([H22 §3.3]).
- **Codex rendering.** Hooks and events match [H21 §4]. No `PreToolUse` stamp is needed under Codex. `additionalContextLimit` 2,500 ≈ 10,000 B; `output_token_limit` pins; `cmd.exe /C`-safe commands.
- **Sandbox analysis.** The writable-root exemption inside `.git` is correct as [S] ([H21 §6.1]). Readers need no locks. Unknown liveness is tolerated.
- **Bytes as the unit.** No number changes for ASCII and Cyrillic; `pack.cyrillic-weight` is removed. The both-ends rule for single results under head, tail and middle cuts is right.
- **`result.v1`** in the strict-compatible subset; `apply --from` adapters with one idempotency key per run.
- **LQ changes are presentation and policy only.** The canonical form and hashes are untouched (L1 display spelling).
- **GT20 (e) design.** Several `--target` flags in one invocation; poisoned `CC_*`/`CXX_*`/`AR_*`/`HOST_*`; musl targets; no binary and no test run; `blake3` `pure`, `sha1`/`sha2` without `asm`, `rmcp` with `server` + `transport-io`; the system allocator.
- **The edit list's Find texts are exact** (155/155), and the Monte Carlo figures reproduce under the stated split.

---

## 6. Disposition requested

1. Fix B1 and B2 before any M0 freeze text is integrated into [AR]: the codec decision and the X-F2 amendment.
2. Rework §4 (M1–M3) as one change: actor resolution lease-first, environment demoted and bound to the root thread, orchestrator rights by lease, role-minting policy, detection precedence by specificity.
3. Re-state §6 and §9 budgets per harness unit (M4, M5) and fix MPSP `ops` (M6).
4. Complete the edit list (M8) and re-run [90 §12.7]'s checks by script after applying.
5. Put the scope items of M11 under #45 or cut them, and print the combined calendar.
