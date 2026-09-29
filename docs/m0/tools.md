# M0 tools: pinned versions, installs and spikes

- **Status:** WP-06 (R-HARN-I), recorded 2026-09-28 on the owner's Windows laptop; §4 updated by WP-02b (fallback A
  of §4.4 implemented and verified; its review's findings applied: the allocation-error hook, and the gate's checks
  of `fuzz/` in §4.7). It is reviewed like any other `docs/m0/` file.
- **Sources:** [PLAN.md](PLAN.md) §2.1 (target directories, the `cl.exe` record), §2.2 (`fuzz/`), §2.4 (external
  programs), §3.2 WP-06, §6.2 R7; [60 §3.13] GT5 and GT16, [60 §5.1] (spawn floors); [90 §11.1–§11.3];
  [a1-dispositions](../spec/reviews/a1-dispositions.md) R7 (the tsoracle grammar pin).
- **Rules.** A pin changes only through this file, in the same commit as the change it records. Paths are written with
  `%USERPROFILE%` and `%APPDATA%`, so no user name reaches the public repository. WP-06 changed no `PATH` entry and no
  system or Claude setting: tools outside `%USERPROFILE%\.cargo\bin` are called by absolute path.

## 1. Pins at a glance

| Tool | Pinned version | Installed by | Location | § |
|---|---|---|---|---|
| Rust stable (root workspace) | 1.98.1 | rustup, `rust-toolchain.toml` | `%USERPROFILE%\.rustup\toolchains\1.98.1-x86_64-pc-windows-msvc` | 2.1 |
| Rust nightly (`fuzz/` only) | `nightly-2026-09-27` (1.101.0-nightly 75a75c3e0) | rustup, `fuzz/rust-toolchain.toml` | `%USERPROFILE%\.rustup\toolchains\nightly-2026-09-27-x86_64-pc-windows-msvc` | 2.2 |
| cargo-fuzz | 0.13.2 | `cargo install --locked` | `%USERPROFILE%\.cargo\bin\cargo-fuzz.exe` | 3, 4 |
| libfuzzer-sys | 0.4.13 | `fuzz/Cargo.toml` (`=0.4.13`) | the fuzz lockfile | 4.1 |
| cc (fuzz build dependency) | 1.5.1 | `fuzz/Cargo.toml` (`=1.5.1`) | the fuzz lockfile | 4.1, 4.4 |
| cargo-mutants | 27.1.0 | `cargo install --locked` | `%USERPROFILE%\.cargo\bin\cargo-mutants.exe` | 3, 5 |
| hyperfine | 1.20.0 | `cargo install --locked` | `%USERPROFILE%\.cargo\bin\hyperfine.exe` | 3, 7 |
| `zstd` CLI | v1.5.7 (win64) | official GitHub release zip | `D:\moirai-tools\zstd\zstd-v1.5.7-win64\zstd.exe` | 6 |
| MSVC `cl.exe` | 19.44.35228 (toolset 14.44.35207, Build Tools 2022 17.14.40) | preinstalled | `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools` | 8 |
| Claude Code | 2.1.281 (desktop-managed); 2.1.110 (standalone) | preinstalled | §9 | 9 |
| git | 2.54.0.windows.1 | preinstalled (Git for Windows) | on `PATH` | 10 |
| `typeperf`, WPR | Windows 11 10.0.26200.9457 | built in | `C:\Windows\System32` | 10 |
| VMMap | not installed | the measurement 11 session | — | 10 |
| tree-sitter-rust (tsoracle grammar) | 0.24.2 on tree-sitter 0.27.0 | root `Cargo.lock` | — | 11 |

## 2. Rust toolchains

### 2.1 Stable

`rust-toolchain.toml` pins 1.98.1 (PLAN §2.1): rustc 1.98.1 (48a229cea 2026-09-01), cargo 1.98.1 (797e8a9bc
2026-08-05), LLVM 22.1.8, host `x86_64-pc-windows-msvc`. rustup is 1.29.1 (d95a37b6a 2026-08-13).

### 2.2 Nightly for `fuzz/`

- **Selection rule:** the latest nightly whose channel manifest lists `rustc`, `cargo`, `rust-std`, `clippy-preview`
  and `rustfmt-preview` as available for `x86_64-pc-windows-msvc`. When WP-06 ran (2026-09-27 23:48 UTC),
  `static.rust-lang.org/dist/2026-09-28/` did not exist yet (HTTP 404) and `dist/2026-09-27/channel-rust-nightly.toml`
  listed all five (and `llvm-tools-preview`, which `cargo fuzz coverage` would need).
- **Manifest SHA-256:** `ad7c5ca64d80a5c7771089f741ab90c9eb5dcbc766ce81f2c90af8fc295b40cc`, equal to the published
  `channel-rust-nightly.toml.sha256`. rustup checks every component against it.
- **Install:** `rustup toolchain install nightly-2026-09-27-x86_64-pc-windows-msvc --profile minimal --component clippy,rustfmt`
- **Result:** rustc 1.101.0-nightly (commit `75a75c3e0a67d3fa3d03982775f5bb0356e7b510`, 2026-09-26), LLVM 23.1.1;
  cargo 1.101.0-nightly (3d7cf6e93 2026-09-25); clippy 0.1.100 (75a75c3e0a 2026-09-26); rustfmt 1.11.0-nightly
  (75a75c3e0a 2026-09-26).
- **Pinned by** `fuzz/rust-toolchain.toml`. Nothing else uses it. The gate's `fuzz` step reads the channel from that
  file (§4.7).
- **Nightly feature.** `fuzz/src/lib.rs` uses `alloc_error_hook` (tracking issue rust-lang/rust#51245) for the
  allocation-error hook of §4.4. A move of the pin re-checks that the feature still exists: the library's tests
  compile and pass on the new date.
- **The rule applies at install time.** `dist/2026-09-28/` appeared later: at 07:37 UTC on 2026-09-28 its manifest
  (SHA-256 `e50323e4e94848283c27cd5391cd6fa99115bf261eddbafaa40a8cfe69759ebd`) listed all five for
  `x86_64-pc-windows-msvc`, with rustc 1.101.0-nightly (d080e7dff 2026-09-27). The pin was not moved: every spike of
  §4.3 ran on `nightly-2026-09-27`, and a move is a change to this section that repeats those spikes on the new date.

## 3. Cargo-installed tools

Each was the latest release on crates.io when installed. The commands pin the versions for a reinstall:

```
set CARGO_TARGET_DIR=D:\moirai-target\tools
set CARGO_BUILD_JOBS=6
cargo +1.98.1-x86_64-pc-windows-msvc install --locked cargo-fuzz@0.13.2
cargo +1.98.1-x86_64-pc-windows-msvc install --locked cargo-mutants@27.1.0
cargo +1.98.1-x86_64-pc-windows-msvc install --locked hyperfine@1.20.0
```

`--locked` builds each tool from its own published `Cargo.lock`. All three were built with 1.98.1 in the release
profile for `x86_64-pc-windows-msvc` and installed into `%USERPROFILE%\.cargo\bin`, which is on `PATH`. The build
cache `D:\moirai-target\tools` (≈ 730 MB) can be deleted.

| Crate | Version | Published | Licence | `.crate` SHA-256 (crates.io) | Binary | Binary SHA-256 |
|---|---|---|---|---|---|---|
| cargo-fuzz | 0.13.2 | 2026-06-09 | MIT OR Apache-2.0 | `5acfd01930e49823e58c30dd8012d3338a620377d7c7d4cc140ca4b2169400e2` | 1,907,712 B | `b2699f01ab23ec1bd93670cfe8ce1ad3fd2de3b0331478c7a71dd70618b7eb99` |
| cargo-mutants | 27.1.0 | 2026-06-02 | MIT | `07072e7bcdeb425d5e5fdbfd9f15a2c749e23cb2edf5ef40aee5876760ae1cf9` | 8,177,152 B | `358f87bebe6da6261be133fb69ccc2667154e4162ecaa27412d02a71415931c1` |
| hyperfine | 1.20.0 | 2025-11-18 | MIT OR Apache-2.0 | `c5fe697e6fdd7bd20438836076f8901094926ae40cf13f3f7255c1f3f085122c` | 1,290,240 B | `d3efc52c87e97a7e45fb3c4add5d2ac67c216d9ba3f90c9f2bb0da909ff4d810` |

## 4. Fuzzing: cargo-fuzz and libFuzzer (GT5)

### 4.1 The `fuzz/` workspace

- **Files.** `fuzz/Cargo.toml`: the package `moirai-fuzz` with cargo-fuzz's `[package.metadata] cargo-fuzz = true`, its
  own `[workspace]` (the root excludes `fuzz`), `libfuzzer-sys = "=0.4.13"`, `moirai-files` by path, `sha1` (the
  root's requirement, the package `moirai-files` already brings in), the build dependency `cc = "=1.5.1"`, and
  `unsafe_code = "forbid"`. `fuzz/rust-toolchain.toml`: the nightly of §2.2. `fuzz/.gitignore`: `target/`, `corpus/`,
  `artifacts/`, `coverage/`. Fallback A of §4.4 (WP-02b): `fuzz/build.rs`, `fuzz/src/sancov_sections.c`, and the
  library `fuzz/src/lib.rs` with the panic and allocation-error hooks in `fuzz/src/artifact.rs`. There are no targets
  yet: FL-1's work packages add them (WP-65, WP-67), and each calls `moirai_fuzz::record(data)` first (§4.5), which
  the gate enforces (§4.7).
- **The lockfile.** Cargo refuses a package without any target ("no targets specified in the manifest"); the library
  target lets the manifest resolve before the first fuzz target exists, so `fuzz/Cargo.lock` exists from WP-02b on
  (25 packages: `moirai-files`'s graph, `libfuzzer-sys`, `arbitrary`, `sha1`, `cc` with `find-msvc-tools` and
  `shlex`, and, through `cc`'s `parallel` feature, `jobserver`, `getrandom` 0.4.3, `libc` and `r-efi`). The gate
  worktree writes it (the `lock` step of `xtask gate --branch` creates it when it is missing; authors.md §3), and every
  other gate run fails closed without it, since the fuzz graph would go unscanned; outside `--branch`, the `lock`
  step names a missing or stale `fuzz/Cargo.lock` itself. Every package name the two lockfiles share resolves to the
  same version, except where the graphs need different major versions: `getrandom` (0.3.4 in `Cargo.lock`, through
  proptest's `rand_core`; 0.4.3 in `fuzz/Cargo.lock`, through `cc`'s `jobserver`) and its dependency `r-efi` (5.3.0
  and 6.0.0). The fuzz graph's build scripts have `graphs = ["fuzz"]` entries in
  `xtask/native-allow.toml`: `libfuzzer-sys` and `moirai-fuzz` (C++ and C, allowed there only), `getrandom` 0.4.3,
  and a second `libc` entry, whose features differ from the checked graph's (`default` and `std` against none);
  `blake3`'s entry lists both graphs. The loader refuses two entries that cover one version of one graph, so no
  reviewed entry is dead text.
- **Skeleton check.** A scratch copy of `fuzz/` with one throwaway target resolved (25 packages, `moirai-files`'s graph
  included), and `cargo check` passed on the pinned nightly under `unsafe_code = "forbid"`: libfuzzer-sys's
  `fuzz_target!` expands to no code that the lint refuses. The nightly's cargo also runs its manifest lint
  `cargo::unused_dependencies` (warn by default): a target set that never uses `moirai-files` draws "unused dependency
  `moirai-files`". FL-1's targets use it, so the warning ends with the first real target.
- **libfuzzer-sys 0.4.13:** published 2026-06-04, licence `(MIT OR Apache-2.0) AND NCSA` (NCSA is allowed in
  `fuzz/Cargo.lock` only, PLAN §2.4), `.crate` SHA-256 `a9fd2f41a1cba099f79a0b6b6c35656cf7c03351a7bae8ff0f28f25270f929d2`.
  Its build script compiles libFuzzer's C++ with `cc`. The spike's lockfile added `arbitrary` 1.4.2, `cc` 1.5.1,
  `cfg-if` 1.0.5, `find-msvc-tools` 0.1.14, `getrandom` 0.4.3, `jobserver` 0.1.35, `libc` 0.2.189, `r-efi` 6.0.0 and
  `shlex` 2.0.1, the set WP-02's lints will see in `fuzz/Cargo.lock` beside `moirai-files`'s graph.

### 4.2 How to run

- **Run from `fuzz/`**, for example `cd fuzz` and then `cargo fuzz run -s none <target> -- -rss_limit_mb=256
  -max_total_time=<s>`. There `fuzz/rust-toolchain.toml` applies and cargo-fuzz finds the project. From the
  repository root rustup resolves the root's 1.98.1 pin. Before WP-02b's review a sanitizer-off build then
  compiled on stable without any error (observed), bypassing the nightly pin silently; the library's
  `#![feature(alloc_error_hook)]` now makes such a build fail with E0554, "`#![feature]` may not be used on the
  stable release channel" (observed with `cargo +1.98.1 check --lib` in `fuzz/`). From elsewhere, use `cargo +nightly-2026-09-27 fuzz …`. A process that rustup started carries
  `RUSTUP_TOOLCHAIN`, which outranks the toolchain file (observed: a program run with `cargo +1.98.1 run` sees
  `1.98.1-x86_64-pc-windows-msvc`), so a cargo that `cargo xtask` spawns in `fuzz/` would build on stable; the gate's
  `fuzz` step therefore sets it to the pinned channel (§4.7).
- **`-s none` on every command.** cargo-fuzz's default sanitizer is `address`.
- **Build mode.** Without `-O`, cargo-fuzz builds at `opt-level=3` with `-Cdebug-assertions` (overflow checks
  included); `-O` drops the debug assertions. Both add `--cfg fuzzing`, `-Cpasses=sancov-module`, sancov level 4 with
  inline 8-bit counters, the PC table and trace-compares, and `-Ccodegen-units=1`.
- **Target directory.** cargo-fuzz honours `CARGO_TARGET_DIR` (verified) and `--target-dir`. The fuzz directory is
  `D:\moirai-target\fuzz`, its own capped directory (PLAN §2.1), which WP-05's disk guard counts. Corpora and artifacts
  default to `fuzz/corpus/<target>` and `fuzz/artifacts/<target>`, both git-ignored.

### 4.3 Spike results

The spike ran in a scratch directory (never in the repository) on a toy crate: `count_pairs` parses `key=value;…`
and never panics; `read_record` has a planted out-of-bounds slice behind the magic `MOI!`. The layout copied the
repository's: the root pinned 1.98.1 and `fuzz/` pinned the nightly. Other lanes were building at the same time, so
exec/s figures are indicative only.

| Run | Arguments | Result |
|---|---|---|
| `clean`, `-O`, fallback A (§4.4) | `-s none -O -- -rss_limit_mb=256 -max_total_time=60` | exit 0 after 61 s; 22,055,883 executions, 361,571 exec/s; cov 23, ft 93; peak RSS 30 MB |
| `clean`, debug assertions, fallback A | `-s none -- -rss_limit_mb=256 -max_total_time=60` | exit 0 after 61 s; 30,962,000 executions, 507,573 exec/s; peak RSS 29 MB |
| `planted`, no shim | `-s none -O` | link error LNK1120, on 1.98.1 and on the pinned nightly (failure 1) |
| `planted`, section shim only | `-s none -O -- -rss_limit_mb=256` | bug found after 224 executions (< 1 s); exit `0xC0000409`; **no artifact** (failure 2) |
| `planted_hooked`, fallback A | same | bug found in < 1 s; the crashing input (`MOI!\n`; `MOI!?\n` in the repeat) written to `artifacts/planted_hooked/crash-<hash>`; cargo-fuzz printed the reproduce and `tmin` commands |
| `oom` (touches 512 MiB on input `M!`) | `-s none -O -- -rss_limit_mb=256` | `ERROR: libFuzzer: out-of-memory (used: 540Mb; limit: 256Mb)`; `oom-<sha1>` written; exit 71 |
| `planted`, `-s address` | `-s address -O -- -rss_limit_mb=256`, with the MSVC bin directory on `PATH` | links without the shim; bug found; exit `0xC0000409`; no artifact; RSS 40 MB at start |

Every sanitizer-off run printed three benign warnings: libFuzzer found no `__sanitizer_acquire_crash_state`,
`__sanitizer_print_stack_trace` or `__sanitizer_set_death_callback`, because no sanitizer runtime is linked. In every
run the number of PC-table entries equalled the number of 8-bit counters (for example 148 and 148).

**Repeat (2026-09-28, 01:00 UTC).** The spike was run again on the same pins: `planted` without the shim failed to
link with LNK2019 and LNK1120 again; `clean` with `-O` and fallback A ran 60 s and exited 0 (29,306,914 executions,
480,441 exec/s, cov 23, ft 93, RSS 30 MB); `planted_hooked` wrote its crash artifact; the cargo-mutants spike of §5
gave 14 mutants again, 10 caught and 4 missed. `dist/2026-09-28/` was still absent (HTTP 404), so the nightly pin of
§2.2 is still the latest.

**Check after a host crash (2026-09-28, 07:40 UTC).** Windows restarted unexpectedly during the M0 session, so the
installs were checked again: the three binaries of §3 and `zstd.exe` still match their SHA-256, the pinned nightly
still has its five components, and both Claude Code installs and `cl.exe` report the versions of §8 and §9. `clean`
with `-O` and fallback A ran 60 s and exited 0 (42,748,477 executions, 700,794 exec/s, cov 23, ft 93, RSS 29 MB;
72 s including the build) on a less loaded machine. A scratch copy of `fuzz/` with one throwaway target resolved to
25 packages and passed `cargo check` on the nightly, and without a target it failed with "no targets specified in the
manifest" (§4.1). The cargo-mutants spike gave 14 mutants, 10 caught and 4 missed, in 4 s. The later nightly is
covered in §2.2.

### 4.4 libFuzzer on MSVC with the sanitizer off: two failures and the fallback (A, decided and implemented)

1. **Link failure.** Out of the box, `cargo fuzz build -s none` fails on `x86_64-pc-windows-msvc` with LNK2019 and
   LNK1120: `__start___sancov_cntrs`, `__stop___sancov_cntrs`, `__start___sancov_pcs` and `__stop___sancov_pcs` are
   unresolved. On COFF, LLVM's SanitizerCoverage expects a runtime to define these section bounds (ELF linkers
   synthesise them). With MSVC only the ASan runtime thunk defines them (`clang_rt.asan_dynamic_runtime_thunk-x86_64.lib`,
   member `sanitizer_coverage_win_sections.cpp.obj`), and libfuzzer-sys compiles libFuzzer's own sources only.
2. **A panic or an allocation failure leaves no artifact.** libfuzzer-sys turns a panic into
   `std::process::abort()`, which on Windows is `__fastfail` (exit `0xC0000409`). No in-process handler runs, so
   libFuzzer never writes `crash-*`, with or without ASan. The crash is still detected (non-zero exit, the panic
   message on stderr), but the input is lost and `cargo fuzz tmin` has nothing to minimise. An allocation that fails
   (a parser that sizes an allocation from the input) goes through `handle_alloc_error`, which calls no panic hook
   and aborts the same way; libFuzzer's `-malloc_limit_mb` cannot catch it first, because it needs the sanitizer's
   malloc hooks, which a sanitizer-off build lacks. Findings that libFuzzer makes itself still write artifacts (the
   `-rss_limit_mb` OOM case above).

**Fallback A (chosen by the design team; implemented in WP-02b and verified, §4.6).** Two additions to `fuzz/`,
neither with unsafe Rust:
- **A section shim**, `fuzz/src/sancov_sections.c`. It defines the eight bounds the way compiler-rt's
  `sanitizer_coverage_win_sections` does: 8-byte start objects in `.SCOV$CA`, `.SCOV$GA`, `.SCOV$BA` and `.SCOVP$A`,
  1-byte stop objects in the matching `$Z` sections, and `/MERGE` of `.SCOV` into `.data` and `.SCOVP` into `.rdata`.
  LLVM adds 8 to each start symbol on COFF, so the start objects are skipped. `fuzz/build.rs` compiles it with `cc`,
  which is already in the fuzz graph through libfuzzer-sys, when `target_env` is `msvc` and no sanitizer is set
  (`CARGO_CFG_SANITIZE`: under `-s address` the ASan runtime defines the same symbols), and passes the object to the
  linker of every linked target with `cargo::rustc-link-arg`. The spike's `rustc-link-arg-bins` is refused by cargo
  while the package has no binary target (before FL-1's first target), and a plain `rustc-link-lib` reaches only the
  targets that use the package's library. In the library's unit tests, which carry no SanitizerCoverage, the object
  defines eight unused symbols.
- **Artifact hooks**, `fuzz/src/artifact.rs`, re-exported by the library. Each target first calls
  `moirai_fuzz::record(data)`, which copies the input into one reused buffer (a process-wide `Mutex<Vec<u8>>`: one copy
  per execution, no allocation once it has grown to the largest input). The first call installs two hooks, so no
  `init:` block is needed:
  - a panic hook in front of libfuzzer-sys's. It writes the buffer where libFuzzer would have written the crash: the
    `-exact_artifact_path=` argument, else `<-artifact_prefix=>crash-<SHA-1 of the input>`, libFuzzer's own name, the
    last occurrence of each flag winning. It prints libFuzzer's line `artifact_prefix='…'; Test unit written to …` and
    then lets libfuzzer-sys's hook print the panic and abort. It never panics itself (a panic inside a panic hook
    aborts before the message is printed);
  - an allocation-error hook (`std::alloc::set_alloc_error_hook`, the nightly feature of §2.2). It writes the buffer
    the same way as `oom-<SHA-1>`, libFuzzer's name for an out-of-memory input, prints the same line, and then calls
    the hook it replaced, which prints "memory allocation of N bytes failed"; the standard library aborts after it.

Its cost is C code in a host-only workspace that already compiles libFuzzer's C++, one copy and one uncontended lock
per execution, a nightly feature, and a library target in `fuzz/`, which also lets the manifest resolve before the
first fuzz target exists (§4.1).

**What fallback A does not cover** (recorded, not verified):
- a process truly out of memory, where the hook's own small allocations (the file name) fail too: a reentrancy
  guard returns at once, and the process aborts without an artifact. The allocations the hook exists for, one large
  request sized from the input, leave the heap usable. Growth that `-rss_limit_mb` sees is libFuzzer's own finding
  and writes `oom-*` itself (§4.3);
- aborts that bypass both hooks: `std::process::abort` called directly, and a stack overflow, which Rust's own
  handler reports and ends. libFuzzer's Windows exception handler may still write `crash-*` for the latter; no run
  checked it.

**Fallback B: `-s address` (not recommended).** MSVC's ASan runtime supplies the bounds, so the build links
(verified). It needs `clang_rt.asan_dynamic-x86_64.dll` from the MSVC bin directory on `PATH` at run time. It leaves
failure 2 unsolved (verified), breaks the sanitizer-off rule of WP-05's nightly and raises RSS. FL-1 forbids unsafe
code, so ASan has little to find.

**Fallback C: fuzz on Linux** (a hosted Ubuntu runner, or WSL2). There ELF linkers synthesise the bounds and libFuzzer
catches `SIGABRT`. This was not verified here. It does not cover the laptop's nightly windows (WP-05), which run on
Windows.

### 4.5 Notes for WP-05 and WP-65

- **Every target starts with `moirai_fuzz::record(data);`** (the example in `fuzz/src/lib.rs`), the closure's own input.
  A target that skips it still finds panics, but leaves no artifact to reproduce or minimise. The gate's `fuzz` step
  refuses such a target (§4.7).
- **Artifacts** are `crash-<sha1>` for a panic and `oom-<sha1>` for a failed allocation (and for libFuzzer's own
  `-rss_limit_mb` finding); both reproduce and minimise the same way.
- **Reproduce and minimise** as cargo-fuzz prints after a crash: `cargo fuzz run -O --sanitizer=none <target>
  <artifact>` and `cargo fuzz tmin -O --sanitizer=none <target> <artifact>`; `tmin`'s child processes get
  `-exact_artifact_path=`, which the hook honours (§4.6).
- **`-rss_limit_mb` is not a hard cap.** A thread checks it about once a second: the OOM spike reached 540 MB under a
  256 MB limit before libFuzzer stopped it. The nightly RAM guard has to budget for the overshoot, or run each fuzz
  process in a job object with a hard memory limit.
- **CPU and RAM.** One libFuzzer process uses one core. `-fork` and `-jobs` multiply the processes, and so the RAM.

### 4.6 Fallback A in the repository: verification (WP-02b, 2026-09-28)

The repository's `fuzz/` package (manifest, `build.rs`, `src/`, lockfile) was copied to a scratch directory outside
the repository, beside a scratch root package, and two scratch targets were added to the copy's manifest: `clean`,
which calls `moirai_files::text::is_text`, `text::norm` and, on UTF-8 input, `fold::fold_v1`, and `planted`, whose
`MOI!` record trusts its length byte. Both call `record` first; `moirai-files` came by path from the repository. The
runs used the nightly of §2.2, `CARGO_TARGET_DIR=D:\moirai-target\fuzz` and `CARGO_BUILD_JOBS=4`, while other lanes
were building.

| Run | Arguments | Result |
|---|---|---|
| build | `cargo fuzz build -s none -O` | both targets link (17.5 s, libFuzzer's C++ included) |
| control: no shim | the same, with `build.rs` returning early | LNK2019 for `__start___sancov_cntrs` and the other bounds, then LNK1120: failure 1 again |
| control: `-s address` | `cargo fuzz build -s address -O planted` | links: `build.rs` leaves the shim out under a sanitizer, so nothing collides with the ASan runtime's bounds |
| `clean`, 60 s | `-s none -O -- -rss_limit_mb=256 -max_total_time=60` | exit 0 after 61 s; 2,697,040 executions, 44,213 exec/s; cov 337, ft 1,651; 930 inline 8-bit counters and 930 PC-table entries; peak RSS 31 MB |
| `planted`, 60 s limit | the same | the bug found in < 1 s, within the first 1,000 executions; exit `0xC0000409`; the 5-byte input `MOI!&` written by the hook to `artifacts/planted/crash-99e106a21b8bfaece8e5b50fdfc8347b4b985531`, the SHA-1 of its bytes; cargo-fuzz printed the failing input, its `Debug` form and the reproduce and `tmin` commands |
| reproduce | `cargo fuzz run -O --sanitizer=none planted <artifact>` | the same panic (a slice range out of bounds at the planted line) |
| minimise | `cargo fuzz tmin -O --sanitizer=none planted <a 33-byte crashing input> -r 2000` | exit 0; minimised to 11, 10, 6 and then 5 bytes through `-exact_artifact_path=` files; 5 bytes is this bug's minimum |

The library's own tests pass (`cargo test --lib` in `fuzz/`: libFuzzer's flag rules and SHA-1 names; the panic hook
writing the recorded input under a prefix and to an exact path before it chains to the previous hook; and, in a child
process of the test binary, a request for 2^62 bytes that aborts after the allocation-error hook has written
`oom-<sha1>` and the previous hook has printed its line), and `cargo clippy --all-targets -- -D warnings` is clean on
the pinned nightly. The nightly's `cargo::unused_dependencies` warnings (`libfuzzer-sys` and `moirai-files`, §4.1)
stay until the first target uses them; they are cargo's manifest warnings, which `-D warnings` does not turn into
errors.

### 4.7 The gate's checks of `fuzz/` (WP-02b review)

`fuzz/` is its own workspace, so the root's `fmt`, `clippy` and `test` steps never see it. The gate covers it in two
places, and CI runs both on every pull request:
- **`fmt`** also runs `cargo fmt --manifest-path fuzz/Cargo.toml -- --check`. Formatting compiles nothing, so the
  root's stable rustfmt does it, from the repository root.
- **`fuzz`**, a step of its own:
  - always, the target lint: in every file of `fuzz/fuzz_targets/` and every binary target of the fuzz manifest, the
    body of each `fuzz_target!` closure starts with `moirai_fuzz::record(<input>)`, and a target file has at least one
    `fuzz_target!` (`xtask/src/lint_fuzz.rs`, with its seeded violations as unit tests);
  - then `cargo clippy --all-targets --locked --keep-going -- -D warnings` and `cargo test --lib --locked` in `fuzz/`,
    on the channel read from `fuzz/rust-toolchain.toml` (through `RUSTUP_TOOLCHAIN`, §4.2) and without the poisoned
    compiler variables, since libFuzzer's C++ and the section shim are C. Outside CI they run when the range or the
    working tree touches `fuzz/` or a path crate of the fuzz graph (`crates/moirai-files` and its path
    dependencies), because a change there can break a target's build; with `--ci`, always.

The choice of a gate step over WP-05's nightly job: the checks are cheap once the target directory is warm (about
1 s for both commands on the laptop when nothing changed; the first build compiles libFuzzer's C++ and
`moirai-files` on the nightly), and a merge that breaks the hooks or a target's build is refused before it lands.
`pr.yml` installs the fuzz nightly beside the stable toolchain (`rustup toolchain install` in `fuzz/`), so its
`gate --ci` runs the step too.

## 5. Mutation testing: cargo-mutants (GT16)

- **Spike.** The toy crate had three functions, one of them with a deliberately weak test. `cargo mutants -j 2 -o <dir>`,
  with `CARGO_TARGET_DIR` unset and `TMP`/`TEMP` on D:, found 14 mutants: 10 caught and 4 missed, with none
  unviable and no timeouts, in 8 s. Two of the missed mutants are equivalent (`<` → `<=` and `>` → `>=` at the bounds
  of a clamp); the other two come from the weak test. `outcomes.json` records `cargo_mutants_version` 27.1.0.
- **Hazard: a shared `CARGO_TARGET_DIR`.** `xtask worktree` sets `CARGO_TARGET_DIR` for every lane. With it set,
  cargo-mutants builds its scratch copies into that shared directory. The crate's artifact hash does not depend on
  the source path (the test binary was `toymut-78a6ee12e7f27092` in both runs), so cargo reported mutated builds as
  `Fresh`. That run reported 8 missed and 6 caught instead of the true 4 and 10. It also left a mutant's test binary
  in the shared directory: a later plain `cargo test` in the original tree reused it as up to date and failed. A
  lane's target directory would be poisoned the same way.
- **Rules** (inputs to WP-65's `.cargo/mutants.toml` and WP-05's nightly job):
  - run cargo-mutants with `CARGO_TARGET_DIR` removed from its environment;
  - set `TMP` and `TEMP` to `D:\moirai-target\mutants`. By default the scratch copies go to `%TEMP%` on C:, which had
    ≈ 14 GB free at WP-06. Each copy then builds in its own `target\`, and cargo-mutants deletes it after the run
    (verified);
  - do not use `--in-place`, which mutates the working tree.

## 6. The `zstd` CLI

- **Source.** The official release v1.5.7 (2025-02-19), the latest on github.com/facebook/zstd when installed:
  asset `zstd-v1.5.7-win64.zip`, 1,747,181 bytes. The release binary was chosen over winget `Meta.Zstandard`, which
  installs the same zip, to avoid a system-level package install.
- **Integrity.** GitHub publishes no digest for the Windows zips; the release's `.sha256` and `.sig` files cover the
  source tarballs only. The zip's SHA-256 is `acb4e8111511749dc7a3ebedca9b04190e37a17afeb73f55d4425dbf0b90fad9`, equal
  to `InstallerSha256` in winget-pkgs' `manifests/m/Meta/Zstandard/1.5.7/Meta.Zstandard.installer.yaml`, an
  independent record. `zstd.exe` is 1,601,409 bytes, SHA-256
  `8076aae03feac7c66b319579e82172eed168deed2a3f25e5e2d3c60f55e84111`. `zstd -V` prints
  `*** Zstandard CLI (64-bit) v1.5.7, by Yann Collet ***`.
- **Install.** `curl -L -o zstd-v1.5.7-win64.zip https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-v1.5.7-win64.zip`,
  compare the SHA-256, unzip into `D:\moirai-tools\zstd\` and delete the zip.
- **Use.** It is not on `PATH`: callers use `D:\moirai-tools\zstd\zstd-v1.5.7-win64\zstd.exe`. Its users are
  measurement 6's proxy (WP-54) and, from M1, the oracle's decoder for zstd frames ([90 §11.3]). A level-1 round trip
  of 1 MB of random data was checked.

## 7. hyperfine

1.20.0 (§3). The spawn floor of the measurement protocol is `hyperfine -N --warmup 5` ([60 §5.1]). A smoke run,
`hyperfine -N --warmup 3 --runs 20 "<zstd.exe> -V"`, completed (6.8 ms ± 0.9 ms on the loaded machine).

## 8. MSVC and the Windows SDK

- **Install:** Visual Studio Build Tools 2022, catalog version 17.14.40 (`installationVersion` 17.14.37628.2), from
  `"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -all -products * -format text`.
- **Toolset:** 14.44.35207 (`VC\Auxiliary\Build\Microsoft.VCToolsVersion.default.txt`).
- **`cl.exe`:** `VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe`, banner "Microsoft (R) C/C++ Optimizing Compiler
  Version 19.44.35228 for x64", file version 19.44.35228.0. `link.exe` is in the same directory.
- **Windows SDK:** 10.0.26100.0.
- **Who compiles C++ or C with it:** libFuzzer in `fuzz/`, and tree-sitter's C in `moirai-tsoracle`, built only by the
  replay job, unpoisoned (PLAN §2.1). The gate's poisoned `CC_`/`CXX_`/`AR_` variables keep C out of every checked
  graph. The toolset also ships the `clang_rt` fuzzer and ASan libraries in `lib\x64` (§4.4).
- **CI:** `pr.yml`'s `replay` job builds, clippy-checks and tests `moirai-tsoracle` and `moirai-replay` unpoisoned on
  the hosted runner and prints that runner's toolset and `cl.exe` banner in its log; the version above is this
  laptop's.

## 9. Claude Code

Two native installs exist, and neither is on `PATH` (`where claude` finds nothing). WP-06 ran only `--version`: no
login, no settings change.

| Install | Version | Path | Size | SHA-256 |
|---|---|---|---|---|
| desktop-managed: the Claude app (MSIX `Claude_2.9939.2.0_x64`) runs the M0 sessions with it | 2.1.281 | `%APPDATA%\Claude\claude-code\2.1.281\claude.exe` | 240,767,648 B | `39be063c2512b43347fe7b0ab18c46f1596141701c9c5fc895ddfca9a051067c` |
| standalone native | 2.1.110 | `%USERPROFILE%\.local\bin\claude.exe` | 244,445,344 B | `6d267d4bc70b98436d4e55c0c3a6cd310b2e97bb42d215abb646093fce9ea3bd` |

- The desktop-managed path contains the version and changes whenever the app updates (2.1.281 arrived on
  2026-09-25). `%USERPROFILE%\.local\share\claude\versions` holds 2.1.110 and 2.1.85 for the standalone install.
- WP-58 runs a native `claude.exe` and never a `.cmd` shim. It therefore takes the executable path as a setting, not a
  constant. V9 confirms which install and version the runner uses.

## 10. Windows built-ins, other programs and deferred tools

- **Windows:** 11, `ver` 10.0.26200.9457.
- **`typeperf`:** `C:\Windows\System32\typeperf.exe`, built in (WP-51).
- **WPR:** `C:\Windows\System32\wpr.exe` is present. A counters-only `.wprp` is checked in only if WP-51 needs WPR;
  none is now.
- **git:** 2.54.0.windows.1 (Git for Windows).
- **VMMap:** not installed. The measurement 11 session (WP-52) installs it and records its version here.
- **Codex:** not found on `PATH`. PLAN §2.4 names Codex 0.157; installing it and confirming the version are V9 items
  (WP-56).
- **Codex's commit attribution (WP-03's record): deferred.** It cannot be observed until Codex is installed. Owner:
  V9 installs Codex 0.157; WP-56 (R-HARN-I) records the behaviour here. The check, in a scratch repository with the
  hooks installed (`git config core.hooksPath <main worktree>/.githooks`) and a synthetic identity:
  1. stage a one-line change and let Codex commit it (`codex exec "commit the staged change with a short message"`);
  2. read what it wrote: `git log -1 --format='%an <%ae>%n%cn <%ce>%n%B'` and `git log -1 --format='%(trailers)'`,
     looking for a co-author trailer, a generated-by line or an agent identity;
  3. note whether `commit-msg` refused the commit, and which rule;
  4. look in Codex's configuration (`~/.codex/config.toml` and its documentation) for a setting that turns commit or
     pull-request attribution off, as A2 did for Claude Code (`.claude/settings.json`).
  Until then any Codex commit meets the same layers as every other: the hooks, the gate's marker scan of
  `master..HEAD` and PR CI over every commit and the pull request body (authors.md §5 item 4).

## 11. The tsoracle grammar pin (a1-dispositions R7)

`moirai-tsoracle`'s grammar is tree-sitter-rust 0.24.2 on tree-sitter 0.27.0, as resolved in the root `Cargo.lock`:

| Crate | Version | `Cargo.lock` checksum |
|---|---|---|
| tree-sitter | 0.27.0 | `2038684e0058edba0d17302619f62eabce4a8e11c6ac59506996a8d79848851d` |
| tree-sitter-rust | 0.24.2 | `439e577dbe07423ec2582ac62c7531120dbfccfa6e5f92406f93dd271a120e45` |
| tree-sitter-language | 0.1.8 | `ca0d1bf6fdd806e43ae5198f82f527056d359def39e54e67a0f478ac09dac081` |

Every gate and replay run uses `--locked`, so the lockfile entry is the pin, and a change of any of these versions is
a change to this section. `[workspace.dependencies]` states `version = "0.24.2"`, a caret requirement (§12 item 3).

## 12. Directories and open points

| Directory | Holds | Set by |
|---|---|---|
| `D:\moirai-target\<lane>` | a lane's shared target directory | `xtask worktree` |
| `D:\moirai-target\fuzz` | cargo-fuzz's `CARGO_TARGET_DIR` | WP-05, WP-65 |
| `D:\moirai-target\mutants` | cargo-mutants' `TMP`/`TEMP`: scratch copies, each with its own `target\` | WP-05, WP-65 |
| `D:\moirai-target\tools` | the `cargo install` build cache (deletable) | WP-06 |
| `D:\moirai-tools\zstd` | the `zstd` CLI | WP-06 |

**Open points for the review:**
1. **VMMap.** PLAN §2.4 and WP-06's row list VMMap. WP-06 did not install it: its brief moves the install to the
   measurement 11 session.
2. **libFuzzer on MSVC.** Closed: the design team chose fallback A of §4.4, and WP-02b implemented and verified it
   (§4.6) before WP-65.
3. **tree-sitter-rust requirement.** To make the grammar pin explicit in the manifest as well, the root
   `[workspace.dependencies]` entry could read `=0.24.2`. That is a root `Cargo.toml` change, outside WP-06.
4. **cargo-mutants' directory.** PLAN §2.1 gives cargo-mutants "its own capped directory". §5 shows that directory has
   to be `TMP`/`TEMP`, with `CARGO_TARGET_DIR` unset, and never a shared target directory (WP-05, WP-65).
5. **Space on C:.** rustup's toolchains, `%TEMP%` and the Claude installs live on C:, which had ≈ 14 GB free. WP-05's
   disk guard counts only the D: directories.
6. **`-rss_limit_mb` overshoot** (§4.5, WP-05).
7. **The Claude Code path** for WP-58 (§9).
8. **PLAN amendments for `fuzz/` and `xtask`.** WP-02b's manifests diverge from PLAN §2.2, §2.4 and §2.5, which only an
   owner-reviewed plan issue changes: authors.md §6 item 12 lists the three amendments.

## 13. Gate timing (WP-02 acceptance: the incremental gate takes ≤ 90 s)

Measured on 2026-09-28 on the laptop, with lane A's warm target directory (`D:\moirai-target\laneA`,
`CARGO_BUILD_JOBS=6`), as `cargo xtask gate --role r-harn-i` with no source change since the previous run:

| Run | Total | `test` | `hooks` | `fmt`, `clippy`, `gt20-e` and the lints |
|---|---|---|---|---|
| with the `hooks` step | 122 s | 64 s | 45 s | 13 s |
| without it (`--skip hooks`) | 69 s | 58 s | — | 11 s |

- `test` is dominated by the members' own suites at tier `pr` (≈ 540 tests), so it grows with the crates.
- `hooks` runs `.githooks/test-hooks.sh`: 66 cases, each a `git commit` in a scratch repository. Outside CI it
  therefore runs only when the range or the working tree touches `.githooks/` or `xtask/`; a role branch that does
  not touch them stays within 90 s, and an R-HARN branch that does takes ≈ 2 min. CI (`gate --ci`) runs it always.
- `gt20-e` checks the three cross targets only: the host's check is the `clippy` step's.
