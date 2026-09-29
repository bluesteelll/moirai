# M0 authorship: the WP → role ledger and path precedence

- **Status:** draft (WP-01, R-HARN-I). WP-80a reviews it together with PLAN §6.2; `xtask authors` (WP-02) enforces it.
- **Sources:** [PLAN.md](PLAN.md) §2.2 (crates and authors), §2.5 (directory tree), §3.1 (roles, S1–S6, mechanics),
  §3.2 (work packages), §4 (lanes and phases); AGENTS.md ("Git", "Data").
- **Precedence:** PLAN.md and the design documents win over this file. A disagreement is a finding for WP-80a, and
  this file is corrected.

## 1. Rules

- A role writes only the paths §3 gives it. For a path claimed by two entries, the most specific entry wins (§3.1).
- Every commit subject starts `WP-xx:`. `xtask authors`, inside the gate, looks up the WP's role in §2 and refuses a
  commit that touches a path the role may not write.
- A role's sessions share the role's paths: R-SPEC-F, -P and -R; R-HARN-I, -S, -O and -M. §2 names the session that
  does each WP.
- A role that needs a file it may not read has found a gap in the specification: it files a review finding and does
  not read the file. Compiling a crate is not reading it (PLAN §3.1).
- Separation rules (PLAN §3.1):
  - **S1:** the fixture author (R-FIX), the oracle author (R-ORA) and the M1 product-codec author are three authors.
  - **S2:** R-MODEL never reads engine code or FL-1's anchor resolver; engine authors never read the model.
  - **S3:** the non-core GT10 fixtures are written by an author who sees neither engine nor model code.
  - **S4:** the seeded-bug author (R-TOY) is not the enumerator's author (R-HARN-S).
  - **S5:** each review lens is a separate session and never reviews its own text.
  - **S6:** gold queries and gold results are written from the task text by R-BENCH, not R-MODEL.

## 2. WP → role ledger

"Paths" names the §3 entries the WP writes through.

| WP | Item | Lane | Role (session) | Paths | Notes |
|---|---|---|---|---|---|
| WP-01 | 7 | A | R-HARN-I | root `Cargo.toml`, `rust-toolchain.toml`, `.cargo/`, `.gitattributes`, `.gitignore`, `xtask/`, `docs/m0/`; the crate skeletons (§4) | `xtask worktree` |
| WP-02 | 7 | A | R-HARN-I | `xtask/` | gate, lints, `authors`, `coverage`, `host-only --list` |
| WP-02b | 7 | A | R-HARN-I | `xtask/`, `fuzz/` (`build.rs`, `src/`, the shared manifest), `docs/m0/` | the WP-02 review re-checked; `xtask hex` (§6 item 3); fallback A of tools.md §4.4 |
| WP-03 | 7 | A | R-HARN-I | `.githooks/`, `xtask/`, `.claude/settings.json`, `docs/m0/` | `xtask hook`, `xtask private index`, the Codex attribution record |
| WP-04 | 7 | A | R-HARN-I | `.github/` | `pr.yml` |
| WP-05 | 7 | A | R-HARN-I | `xtask/` | `xtask nightly` |
| WP-06 | 7 | A | R-HARN-I | `docs/m0/tools.md`, `fuzz/` (the workspace and its toolchain) | |
| WP-10 | 1 | A | R-SPEC-F | `docs/spec/` (`format/01`, `format/02`, `COVERAGE.md`) | |
| WP-11 | 1 | A | R-SPEC-P | `docs/spec/` (`format/03`–`05`) | |
| WP-12 | 1 | A | R-SPEC-F | `docs/spec/` (`format/06`, `07`, `12`) | |
| WP-13 | 1 | A | R-SPEC-R | `docs/spec/` (`format/09`–`11`) | |
| WP-14 | 1 | A | R-SPEC-F | `docs/spec/` (`format/08`, `18`) | |
| WP-14b | 1 | A | R-SPEC-R | `docs/spec/` (`format/20`) | |
| WP-15 | 1 | A | R-SPEC-R | `docs/spec/` (`format/14`) | |
| WP-16 | 1 | A | R-SPEC-P | `docs/spec/` (`format/13`, `15`–`17`) | |
| WP-17 | 1 | A | R-SPEC-P | `docs/spec/os/` | |
| WP-18 | 1 | A | R-SPEC-F | `docs/spec/` (`config.md`, `format/19`) | |
| WP-19 | 1 | A | R-SPEC-F | `docs/spec/lq/` | |
| WP-20 | 1 | A | R-FIX | `fixtures/` (`hex/`) | S1; `xtask hex` itself is R-HARN's (§6 item 3) |
| WP-21 | 1 | A | R-FIX | `fixtures/` (`canonical/`, `r4/`, `carrier/`, `moi/`) | S1 |
| WP-22 | 1 | A | R-FIX | `fixtures/` (`lq/`, `gt10/`) | S1, S3 |
| WP-20b | 1 | A | R-FIX | `fixtures/` | S1 |
| WP-25 | 2 | A | R-SPEC-F | `docs/spec/` (`store-api.md`, `store-api/`) | |
| WP-30 | 3 | A | R-HARN-S | `crates/moirai-vfs/` | |
| WP-31 | 3 | A | R-HARN-S | `crates/moirai-vfs-sim/` | |
| WP-32 | 3 | A | R-HARN-S | `crates/moirai-vfs-sim/` | S4: the enumerator's author |
| WP-33 | 3 | A | R-HARN-O | `crates/moirai-os/` | |
| WP-40 | 4 | A | R-TOY | `crates/moirai-toylog/` | S4: the seeded-bug author |
| WP-40b | 4 | A | R-TOY | `crates/moirai-toylog/` | S4 |
| WP-50 | 5 | A | R-HARN-I | `docs/spec/measurement-protocol.md`, `crates/moirai-probes/`, `crates/moirai-probes-bin/` | `empty`, `guard` |
| WP-51 | 5 | A | R-HARN-I | `xtask/` (`loadrec`), `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `.github/` (`noise.yml`), `docs/measurements/` | WP-51a first |
| WP-52 | 5 | A | R-HARN-I | `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `docs/measurements/` | |
| WP-53a–e | 5 | B | R-HARN-M | `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `docs/measurements/` | |
| WP-54 | 5 | B | R-HARN-M | `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `crates/moirai-tokcount/`, `docs/measurements/` | |
| WP-55 | 5 | A | R-HARN-I | `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `docs/measurements/` | |
| WP-56 | 5 | A | R-HARN-I | `crates/moirai-harness-stub/`, `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `docs/measurements/` | builds the stub (§6 item 4) |
| WP-57 | 5 | A | R-HARN-I | `crates/moirai-probes/`, `crates/moirai-probes-bin/`, `docs/measurements/` | |
| WP-58 | 5 | B | R-HARN-M | `crates/moirai-tokcount/` | |
| WP-60 | 6 | B | R-FL1B | `crates/moirai-diff/` | |
| WP-61 | 6 | A | R-FL1A | `xtask/src/ucd`, `fixtures/ucd/`, `crates/moirai-files/` (`path`, `fold`), `LICENSES/`, `NOTICE` (append) | |
| WP-61b | 6 | B | R-FL1B | `crates/moirai-files/` (`ignore`) | |
| WP-62 | 6 | A | R-FL1A | `crates/moirai-files/` (`text`, `oid`, `uid`, `r14`) | |
| WP-63 | 6 | B | R-FL1B | `crates/moirai-files/` (`scan`) | |
| WP-64 | 6 | A | R-FL1A | `crates/moirai-files/` (`anchor`) | S2: the model's author never reads it |
| WP-65 | 6 | B | R-FL1A | `fuzz/` (R-FL1A targets), `.cargo/mutants.toml` | |
| WP-66 | 11 | B | R-FL1B | `crates/moirai-files/` (`sketch`) | |
| WP-67 | 11 | B | R-FL1B | `fuzz/` (R-FL1B targets) | |
| WP-70 | 10 | B | R-BENCH | `crates/moirai-lqbench/`, `fixtures/lqbench/` | S6 |
| WP-71a | 10 | B | R-BENCH | `crates/moirai-lqbench/`, `fixtures/lqbench/` | |
| WP-71b | 10 | B | R-BENCH | `crates/moirai-lqbench/`, `docs/measurements/m0/lqbench/` | |
| WP-72 | 10 | B | R-BENCH | `crates/moirai-lqbench/`, `docs/measurements/m0/lqbench/` | |
| WP-73 | 10 | A and B | R-SPEC, R-FIX, R-MODEL, R-BENCH; owner | each role within its own paths | one commit per role |
| WP-74 | 12 | B | R-REPLAY | `crates/moirai-replay/`, `crates/moirai-tsoracle/` | |
| WP-75 | 12 | B | R-REPLAY | `crates/moirai-replay/` | owner data only in `/private/` |
| WP-76 | 12 | B | R-REPLAY | `crates/moirai-replay/`, `docs/measurements/m0/replay/` | counts and rates only |
| WP-77 | 12 | B | R-REPLAY | `crates/moirai-replay/` | |
| WP-80a | 8 | A | R-REV-P, R-REV-S, R-REV-A | `docs/spec/reviews/` (`a1-<lens>.md`) | S5 |
| WP-80 | 8 | A | R-REV-P, R-REV-S, R-REV-A; owner (V2) | `docs/spec/reviews/` (`<lens>-<pass>.md`) | S5 |
| WP-81a | 8 | A | R-SPEC | `docs/spec/`, `docs/ARCHITECTURE-RESEARCH.md`, `docs/research/design/` | owner review |
| WP-81b | 8 | A | R-SPEC with the owner | none (the tag `format-v1`) | |
| WP-90 | 9 | B | R-MODEL | `crates/moirai-model/`, `docs/spec/rules/` | S2 |
| WP-91 | 9 | B | R-MODEL | `crates/moirai-model/` | S2 |
| WP-92 | 9 | B | R-MODEL | `crates/moirai-model/`, `docs/spec/rules/` | S2 |
| WP-93a | 9 | B | R-MODEL | `crates/moirai-model/` | |
| WP-93b | 9 | B | R-MODEL | `crates/moirai-model/` | |
| WP-94 | 9 | B | R-MODEL | `crates/moirai-model/` | |
| WP-95 | 9 | B | R-ORA | `crates/moirai-format-oracle/` | S1 |
| WP-95b | 9 | B | R-ORA | `crates/moirai-format-oracle/` | S1 |
| WP-99 | exit | A | R-SPEC with the owner | `docs/research/design/` ([60 §7]), `docs/ARCHITECTURE-RESEARCH.md` | owner review |

## 3. Path map and precedence

**Matching.** Paths are repository-relative with `/`. `dir/**` is everything under `dir/`; `*` matches within one
path component; `{a,b}` lists alternatives; a module path ending in `src/<m>` stands for both `src/<m>.rs` and
`src/<m>/**`. The most
specific entry is the one with the longest literal prefix, counted in path components; a full file name is more
specific than any pattern that contains it. When two entries are equally specific, the path is shared and every role
they name may write it. A path that no entry covers may be written by no role; adding it here is an R-HARN-I change
to this file, reviewed like any other.

| Path | Writers | Notes |
|---|---|---|
| `Cargo.toml` (root) | R-HARN-I | members; `[workspace.dependencies]` is where each external crate is adopted after WP-02's lints accept it (PLAN §2.4, §6 item 1) |
| `Cargo.lock`, `fuzz/Cargo.lock` | the gate worktree only | updated only by the gate run (PLAN §3.1); never by hand |
| `rust-toolchain.toml` | R-HARN-I | |
| `.cargo/**` | R-HARN | the `xtask` alias |
| `.cargo/mutants.toml` | R-FL1A | GT16 configuration (WP-65) |
| `.gitattributes`, `.gitignore` | R-HARN | |
| `.githooks/**`, `.github/**` | R-HARN | |
| `.claude/settings.json` | R-HARN | attribution off (A2); a change needs the owner's approval |
| `LICENSES/**` | R-FL1A | the Unicode-3.0 text (PLAN §2.4) |
| `NOTICE` | R-FL1A | append-only: the third-party section after the owner's text, which never changes (§6 item 5) |
| `LICENSE`, `README.md`, `AGENTS.md`, `CLAUDE.md` | owner only | |
| `private/**` | none | never committed; the pre-commit hook refuses it |
| `xtask/**` | R-HARN | |
| `xtask/src/ucd` | R-FL1A | `xtask ucd` (PLAN §3.1) |
| `fuzz/**` | R-HARN-I | the separate workspace and its pinned nightly (WP-06) |
| `fuzz/Cargo.toml` | R-HARN-I, R-FL1A, R-FL1B | shared: each role adds its own targets' `[[bin]]` entries |
| `fuzz/fuzz_targets/{anchor,path,spec}_*.rs` | R-FL1A | anchor selectors, path specs, the authoring-spec parser (WP-65) |
| `fuzz/fuzz_targets/{scan,ignore,sketch}_*.rs` | R-FL1B | scanners, ignore patterns, part-2 inputs (WP-67) |
| `fixtures/**` | R-FIX | |
| `fixtures/lqbench/**` | R-BENCH | |
| `fixtures/ucd/**` | R-FL1A | |
| `crates/moirai-vfs/**`, `crates/moirai-vfs-sim/**` | R-HARN | sessions -S |
| `crates/moirai-os/**` | R-HARN | session -O |
| `crates/moirai-vfs/clippy.toml`, `crates/moirai-os/clippy.toml`, `crates/moirai-diff/clippy.toml`, `crates/moirai-files/clippy.toml` | R-HARN-I | the type-aware GT20 (d) layer (WP-02): each product crate's clippy `disallowed-methods`; the gate refuses a product crate without the file or with an incomplete list, so a new product crate's file is added here with the crate |
| `crates/moirai-probes/**`, `crates/moirai-probes-bin/**` | R-HARN | sessions -I and -M |
| `crates/moirai-harness-stub/**`, `crates/moirai-tokcount/**` | R-HARN | |
| `crates/moirai-toylog/**` | R-TOY | R-HARN may not read its bug module before WP-32 is accepted |
| `crates/moirai-toylog/src/{bug,bugs}` | R-TOY | the seeded-bug module (S4), at exactly this path: `xtask/roles.toml` denies it to R-HARN until WP-32 is accepted, and the gate warns when the crate has other modules but nothing here |
| `crates/moirai-model/**` | R-MODEL | including the rule tables under `rules/` |
| `crates/moirai-format-oracle/**` | R-ORA | |
| `crates/moirai-lqbench/**` | R-BENCH | |
| `crates/moirai-replay/**`, `crates/moirai-tsoracle/**` | R-REPLAY | |
| `crates/moirai-diff/**` | R-FL1B | |
| `crates/moirai-files/**` | R-FL1A, R-FL1B | shared: `Cargo.toml`, `src/lib.rs` (each role declares its own modules), `tests/`, `benches/` |
| `crates/moirai-files/src/{path,fold,text,oid,uid,r14,anchor}` | R-FL1A | path rules and P1–P12; `fold_v1` and its generated tables; the two-pass reader, `is_text`, EOL and normalised lines; `oid`; uid derivations and predecessor order; chapter 20's R-14 constants; anchor capture and resolve |
| `crates/moirai-files/src/{scan,ignore,sketch}` | R-FL1B | the scope scanners; the gitignore and never-candidate matchers; sketch, winnowing, similarity, containment and their predicates |
| `docs/spec/**` | R-SPEC | the session per chapter is in §2 |
| `docs/spec/measurement-protocol.md` | R-HARN | WP-50, session -I |
| `docs/spec/rules/**` | R-MODEL | |
| `docs/spec/rules/SIGNED.md` | owner only | the owner-committed table digests (V3) |
| `docs/spec/reviews/**` | R-REV-P, R-REV-S, R-REV-A | each lens writes only the files named with its lens: its letter as a `-`, `_` or `.`-separated token of the file name (`P-pass1.md`, `a1-p.md`) |
| `docs/measurements/**` | R-HARN | aggregates only |
| `docs/measurements/m0/lqbench/**` | R-BENCH | scores and aggregates of WP-71b and WP-72 (§6 item 6) |
| `docs/measurements/m0/replay/**` | R-REPLAY | WP-76's counts and rates (§6 item 6) |
| `docs/m0/**` | R-HARN | |
| `docs/m0/PLAN.md` | no M0 role | changed only by a plan issue the owner reviews |
| `docs/ARCHITECTURE-RESEARCH.md`, `docs/research/design/**` | R-SPEC | only in WP-81a and WP-99, with owner review |
| `docs/research/**`, `docs/architecture-approval-ru/**` | owner only | |

## 4. The crate skeletons (WP-01)

WP-01 creates every crate of PLAN §2.2: its `Cargo.toml` and its crate root (`src/lib.rs` holding only the crate doc
comment; for the bins-only root `moirai-probes-bin`, `src/bin/empty.rs`, measurement 11's empty executable; for
`xtask`, `src/main.rs`, which prints the planned subcommands and exits 2). `xtask authors` accepts these paths only
as additions, in a commit whose subject names `WP-01` itself (not a suffixed `WP-01b` read as WP-01), and only where
no ancestor of the commit had the path; from then on each belongs to the owner that §3 names. WP-01 also declares
the workspace edges of PLAN §2.2's "Allowed dependencies" column; external dependencies are added by the WP that
first needs them.

## 5. Residual risks

1. The worktree deny rules block Read, Grep and Glob, not reads through Bash (`cat`, `git show m0/r-model:…`). The
   briefs forbid those reads (PLAN §3.1).
2. The deny rules are Claude Code settings. A non-Claude-Code authoring session would need an equivalent rule, so M0
   uses none (PLAN §3.1).
3. A role may build crates it must not read. The gate reduces the diagnostics of such crates to "crate X: n errors,
   file a review finding" (PLAN §3.1).
4. The hooks run only where `core.hooksPath` is set, and `git commit --no-verify` skips them; `git cherry-pick` and
   `git rebase` do not run `commit-msg`. AGENTS.md forbids `--no-verify`; the gate's AI-marker scan of every commit in
   `master..HEAD` (WP-02) and PR CI over every commit and the PR body (WP-04) are the layers that cannot be skipped.
   Until the owner installs the hooks (day-1 bundle point 8), the owner reviews every commit message (PLAN §5).
5. `git config moirai.xtask` and `moirai.private-guard` are local settings that any process can change. The pre-commit
   guard protects against mistakes, not against a hostile session; the manifest and shingle checks are re-run by the
   gate before every merge into `master` (WP-02, WP-03 part 2).
6. `xtask authors` checks paths against the role of the WP a subject names. In `xtask gate --branch m0/<role>` every
   commit of the branch must name a WP whose §2 roles include the branch's role, and its paths are checked against
   that role; a commit without a `WP-xx:` subject is refused there. Outside branch mode (the owner's own range) a
   subject that names the wrong WP still passes as that WP's role, and a subject without `WP-` is an owner commit and
   is skipped; the gate worktree's review of each merge catches both.
7. Path-level checks cannot split a file: `NOTICE` (append-only) and the shared `moirai-files` manifest and crate root
   rely on a diff rule (§6 items 2 and 5).
8. `pr.yml` runs its AI-marker and identity checks with the xtask built from the base commit, so a pull request cannot
   loosen the rules that check it. A change to `pr.yml` itself, and the first pull request whose base has no
   `xtask ci` yet (the job then builds the pull request's own xtask and says so in its log), still rely on the
   ruleset's review of every pull request (PLAN §8, V10).

## 6. Open points for the review

1. **External-dependency adoption point.** PLAN §2.4 says WP-02's lints check a crate "before a crate is adopted" but
   names no place. This file makes the root `[workspace.dependencies]` that place (R-HARN-I); members refer to it with
   `<name>.workspace = true`, so each crate's features (`blake3` with `pure`, `sha2` without `asm`) are set once.
2. **`moirai-files` module split.** PLAN §3.1 divides the crate between R-FL1A and R-FL1B by function without naming
   paths. §3 fixes the modules `path`, `fold`, `text`, `oid`, `uid`, `r14`, `anchor` (R-FL1A) and `scan`, `ignore`,
   `sketch` (R-FL1B), with the manifest and `src/lib.rs` shared. `xtask authors` should check that a shared file's
   diff by one role touches only that role's `mod` lines and dependencies.
3. **`xtask hex`.** PLAN §3.2 specifies it in WP-20 (R-FIX), while PLAN §3.1 gives `xtask` (except `ucd`) to R-HARN.
   The ledger keeps it with R-HARN-I, to WP-20's specification; it is generic and knows no structure, so S1 holds.
   WP-02b builds it (`xtask/src/hex.rs`), and the gate's `hex` step runs `xtask hex --check` over `fixtures/hex/`.
   **The committed `.bin` is the fixture's interface.** WP-95's oracle has no workspace dependency, so it cannot link
   the assembler, and the M1 codec also reads bytes; both read `fixtures/hex/**.bin`. `--check` therefore requires a
   `.bin` beside every `.hex` and compares it byte for byte with the re-assembled text; an `!expect` line is an extra
   pin, never a substitute. R-FIX writes each `.bin` with `cargo xtask hex <file.hex>` and commits it with its
   `.hex`.
4. **`moirai-harness-stub`.** No WP names its construction. The ledger gives it to WP-56 (R-HARN-I), its first user
   (measurement 7, the Codex probes, measurement 19); WP-71b builds LQ-Bench's server on its loop.
5. **`NOTICE`.** R-FL1A appends the third-party section (PLAN §2.4); the owner's text above it never changes.
   `xtask authors` should accept only appended lines from R-FL1A.
6. **Committed LQ-Bench and replay results.** PLAN §3.2 commits only aggregates (WP-71b, WP-72, WP-76) but names no
   path; `docs/measurements/**` is R-HARN's. This file gives `docs/measurements/m0/lqbench/` to R-BENCH and
   `docs/measurements/m0/replay/` to R-REPLAY.
7. **`moirai-probes-bin`'s line cap.** PLAN §2.1 gives this root "a line cap" without a value. `xtask/roots.toml` uses
   200 lines per file, [90 §11.1]'s cap for `moirai`.
8. **`#![forbid(unsafe_code)]`.** PLAN §2.1 asks for the attribute in every crate but `moirai-os`. WP-01 puts it in
   `[workspace.lints]` (`unsafe_code = "forbid"`), which every member inherits with `[lints] workspace = true`, so crate
   roots hold only their doc comment. `moirai-os` has its own table (`unsafe_code = "deny"`, opted into per module,
   plus `clippy::undocumented_unsafe_blocks`); WP-02's gate should check that no other member leaves the workspace
   table.
9. **The `empty` binary.** Cargo needs at least one target in `moirai-probes-bin`, which holds binaries only. WP-01
   creates `empty` (measurement 11's empty executable, which is final as written: `fn main() {}`); WP-50 owns it.
10. **How `xtask authors` reads this file (WP-02).** It parses §2's table (a `WP-53a–e` row covers `WP-53a` to
    `WP-53e`) and §3's table directly, so this file stays the single record. A commit subject may name several WPs
    (`WP-02, WP-03:` or `WP-02+03b:`); a suffixed id that §2 does not list (`WP-03b`, a WP's second part) is read as
    its numbered row. A subject that starts with `WP-` but does not parse (`WP-02 fix: …`) is a finding. A path no §3
    entry covers is refused. Two limits in §3's notes column are parsed and enforced: `only in WP-…` (the path may be
    written only by a commit naming one of those WPs) and `named with its lens` (a review lens writes only files
    carrying its letter). Two rules sit beside the tables: WP-01's commit may add the crate skeletons of §4, and
    `Cargo.lock` and `fuzz/Cargo.lock` are accepted in any WP commit, because they change only as a side effect of
    cargo and every gate command runs `--locked` (a lockfile that disagrees with the manifests cannot pass). `NOTICE`
    changes must keep the old text as their prefix (item 5). A merge commit is checked on the files it changes
    relative to every parent (its combined diff). The per-line rule of item 2 for the shared `moirai-files` manifest
    and crate root is not automated: the gate worktree's review of each merge covers it.
11. **What the tooling reads besides this file.** `xtask/roles.toml` gives each role its lane and the paths it must
    not read (PLAN §3.1 "Must not", S1–S6), for `xtask worktree` and `xtask gate --branch`; `xtask/crates.toml` gives
    each workspace member its PLAN §2.2 kind (product, test-only, tool, host-only), for the GT20 (d) and (a) scopes.
12. **PLAN amendments that WP-02b's manifests need** (a plan issue for the owner: PLAN.md changes only through one, and
    no M0 role edits it). The manifests and tools.md already record each change; PLAN still says otherwise:
    - **§2.2, `fuzz/`'s allowed dependencies** (`libfuzzer-sys`, `moirai-files`): the fuzz package also depends on
      `sha1` (the `crash-<sha1>` and `oom-<sha1>` artifact names, tools.md §4.4; the requirement and features of the
      root's entry, so the package `moirai-files` already brings in) and has the build dependency `cc` (the
      SanitizerCoverage section shim; already in the fuzz graph through libfuzzer-sys, pinned `=1.5.1`).
    - **§2.5, the tree of `fuzz/`** (`fuzz_targets/`, `rust-toolchain.toml`, `Cargo.lock`): it also holds `build.rs`
      and `src/` (`lib.rs`, `artifact.rs`, `sancov_sections.c`), the library target of fallback A (tools.md §4.4), which
      also lets the manifest resolve before the first target.
    - **§2.4, `proptest`** ("dev only, test crates"): the tool crate `xtask` takes it as a dev-dependency for the
      property tests of `xtask hex` (varints, hex round trips, directives over random ranges), a test-only use that
      never reaches a built tool.
