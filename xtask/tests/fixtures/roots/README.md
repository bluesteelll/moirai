Fixture workspace for the composition-root lint (docs/m0/PLAN.md §2.2: "a fixture workspace self-tests both").
Synthetic crates, never built: `xtask` unit tests describe them with synthetic `cargo metadata` and read their files.
`probes-bin-ok` and `moirai-ok` follow `xtask/roots.toml`; `probes-bin-bad` and `moirai-bad` break it.
