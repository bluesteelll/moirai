# `fake-claude` fixtures (PLAN WP-58, tier `pr`)

Hand-written, synthetic `claude -p --output-format stream-json --verbose` streams, one JSON object per line, in the
shape Claude Code 2.1.110 prints: the `system`/`init` message, `assistant` and `user` turns, other events, and the
`result` message with `usage` and `modelUsage`. No line comes from a real call; ids, texts and numbers are invented.
The `fake-claude` binary replays a file byte for byte.

PLAN WP-58 says the fake replays "recorded JSON". These files are written by hand instead, because AGENTS.md
("Data") keeps every real session's output out of this public repository. Whether their shape matches a real call
of the pinned version is item 12 of the V9 checklist (`src/claude/mod.rs`).

| File | What it exercises |
|---|---|
| `success.jsonl` | a one-turn call with no tools: usage, per-model usage, version |
| `success-with-text.jsonl` | the same call with 1,017 more cache-write tokens (the with/without delta) |
| `mcp-three-turns.jsonl` | a benchmark-shaped call: one MCP server, a tool use, a smaller model beside the pinned one |
| `mcp-failed.jsonl` | a one-turn call whose MCP server failed to connect, so its tools are missing |
| `error-max-turns.jsonl` | a call stopped by `--max-turns`: an error subtype with no `result` text |
| `wrong-model.jsonl` | an init reporting another model than the pin |
| `fallback-model.jsonl` | an init reporting the pin, then a main-loop assistant turn served by another model |
| `api-key.jsonl` | an init reporting an API-key login |
| `owner-context.jsonl` | an init listing owner context: a non-built-in agent, a skill, a plugin and a slash command |
| `no-result.jsonl` | a stream that ends before its result (the process fails) |
| `noise.jsonl` | a non-JSON line, a non-object line, an empty line, hook and rate-limit events before the result |
