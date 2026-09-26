# moirai — instructions for agents

moirai is a from-scratch Rust graph database that serves AI coding agents as a memory system, a task tracker and a store
of project rules and decisions. It has its own git-like version control, a git-compatible image, file links that survive
moves, and its own query language (Lachesis). The project is at the design stage: there is no code yet.

Start with [docs/ARCHITECTURE-RESEARCH.md](docs/ARCHITECTURE-RESEARCH.md). Its §0 summarizes the design, §9 has the roadmap
and §11 has the owner decisions. Detailed designs are in [docs/research/design/](docs/research/design/) and the research
reports in [docs/research/](docs/research/).

## Git

- Every commit is authored by the owner. **Never add `Co-Authored-By: Claude` or any other AI co-author trailer or marker
  to a commit message.** Never add "Generated with Claude Code" or a similar line to a pull request description.
- Commit or push only when the owner asks. Never use `--force` or `--no-verify` without explicit permission.

## License

- Apache-2.0, copyright Mark Boyko. See LICENSE and NOTICE.
- Keep NOTICE intact.
- Every dependency must have an Apache-2.0-compatible license (MIT, Apache-2.0, BSD, ISC, Zlib and similar).

## Data

- This repository is public. Never commit owner-derived data: corpora, real session prompts, recorded briefs, load
  fixtures, memory contents, or anything under `private/`. Tests and CI use synthetic data only.
- Never commit secrets, tokens or credentials.

## Binding rules (details in docs/ARCHITECTURE-RESEARCH.md)

- No SQLite or any other third-party embedded database, anywhere: not as a backend, a test oracle or a benchmark. The
  engine is written from scratch. The test oracle is a naive reference model written in Rust.
- Every component is built to its final specification in dependency order. There are no interim or throwaway stages.
- The priorities are speed, minimal RAM, correctness and minimal agent tokens. Each has budgets and gates.
- Windows, Linux and macOS are all designed for. Windows is built and tested now. `cargo check` for the Linux and macOS
  targets is a gate from M0, so every dependency must be pure Rust.
- The system must work with Claude Code, Codex and other harnesses. Hooks are optional accelerators.
- Runtime and operational policy goes into config keys with defaults, not into owner questions.
- Repository artifacts are written in English. Chat with the owner may be in Russian.
