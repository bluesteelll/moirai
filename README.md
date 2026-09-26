# moirai

A from-scratch Rust graph database for AI coding agents: a memory system, a task tracker and a store of project rules
and decisions in one typed graph.

- **Tasks as a graph:** subtasks, blockers and links, with the ready queue and blocked state computed by the engine.
- **Its own git-like version control:** branches and typed three-way merge, independent of git, plus a git-compatible
  image to save state and history into git and load them back.
- **Consistent references:** when a node is deleted, every node that referenced it knows at once.
- **File links that survive moves:** explicit file commands plus lazy, deterministic re-binding.
- **Lachesis (LQ):** a Cypher-shaped query language.
- **Agent interfaces:** a CLI and an MCP server that work with Claude Code, Codex and other agent harnesses.
- **Priorities:** speed, minimal RAM, correctness and minimal agent tokens.

**Status:** design stage. There is no code yet. The architecture, the roadmap and the research behind them are in
[docs/ARCHITECTURE-RESEARCH.md](docs/ARCHITECTURE-RESEARCH.md).

## License

Copyright 2026 Mark Boyko. Licensed under the [Apache License, Version 2.0](LICENSE). You may use, modify and
redistribute moirai. Redistributions must keep the copyright and [NOTICE](NOTICE) attribution and mark changed files.
The license grants no right to use the moirai name to present your product as the original.
