# The nightly runner: `cargo xtask nightly`

- **Status:** WP-05 (R-HARN-I), wave 3b, 2026-10-06; the review's findings applied the same day. No real run has been
  made: no window is agreed yet (PLAN §5 V4, deferred with the measurements by the owner on 2026-09-29), and the disk
  guard refuses on the laptop today (§7 item 3). It is reviewed like any other `docs/m0/` file.
- **Sources:** [PLAN.md](PLAN.md) §2.1 (target directories, test tiers), §3.2 WP-05, §5 V4; [60 §3.13] (GT1, GT5,
  GT16, GT18), [60 §3.15] (profile L: the night schedule, "by day … gate jobs beside the agents ≤ 1 GB in total,
  everything refused below 1.5 GB free", the 25 GB disk row); [AR §8.3];
  [measurement-protocol.md](../spec/measurement-protocol.md) §8 (`[MP §8]`, the guard); [tools.md](tools.md) §4 (fuzzing),
  §5 (cargo-mutants), §12 (directories).
- **Code:** `xtask/src/nightly/` (`mod.rs` the pre-checks, the run and the finish, `calendar.rs`, `config.rs`,
  `guard.rs`, `exec.rs`, `jobs.rs`); the configuration `xtask/nightly.toml`; the private manifest's exclusions in
  `xtask/src/private.rs`.

## 1. What it does

`cargo xtask nightly run` runs profile L's nightly job list on the owner's laptop inside an agreed window: GT1 full,
GT18 long, GT16's sample and fuzzing, one job at a time, each ending before the window does (E10: "a nightly run
completes inside an agreed window"). `cargo xtask nightly check` makes the same pre-checks and prints the plan; it starts
no job. Both refuse to start when a pre-check fails (§4).

```
cargo xtask nightly check [--private-dir <dir>] [--windows <file>] [--now <date-time>] [--guard <exe>]
                          [--inject-available-physical <bytes>] [--inject-volume-available <bytes>]
                          [--inject-dir-size <dir> <bytes>]...
cargo xtask nightly run   [--private-dir <dir>] [--guard <exe>]
```

- `--private-dir` names `/private/` (default: the main worktree's, as `xtask private index` finds it).
- `--guard` names a built `moirai-probes-bin guard`; without it the runner builds the guard into the build lane and
  copies it to `<target root>/nightly/`, outside every lane's target directory, where no cargo build replaces it and no
  `cargo clean` of a lane removes it during the run.
- `--windows`, `--now` and the `--inject-…` options are for `check` only: a synthetic calendar, an injected clock and
  injected guard readings, passed to the guard ([MP §8.2]); `--inject-dir-size` names a counted directory by its name
  under the target root (`laneA`, `laneB`, `fuzz`, `mutants`) or by any spelling of its path, and the runner passes the
  guard the exact path it gives `--dir`. `run` refuses them: a real run uses the agreed calendar, the clock and real
  readings.
- Run it from a worktree no agent uses during the window (in an agent-free window, the main worktree): the runner reads
  the tree again after each job and stops a run whose tree moved (§5).

| Exit code | Meaning |
|---|---|
| 0 | `run`: verdict `pass` or `pass-partial` (§6); `check`: the run would start now |
| 1 | `run`: verdict `fail`: a job failed, timed out, was stopped or could not run, or the tested tree changed |
| 2 | usage error |
| 3 | a pre-check refused; nothing started |
| 4 | the runner itself failed: git could not be read, or a record could not be written or renamed. A run that stops this way leaves its directory `<start>.partial`, which the next run seals `<start>.aborted` |
| 5 | `run`: verdict `no-job-ran`: every job was skipped, so nothing was tested |

## 2. The calendar: `/private/windows.toml`

The owner's agreed windows (PLAN §5 V4). The file is owner data: it stays in the gitignored `/private/`, and the
runner never copies its `note`s anywhere.

```toml
version = 1

[[window]]
start = "2026-10-13T23:00:00+03:00"   # RFC 3339 with Z or an offset; seconds optional
end = "2026-10-14T07:00:00+03:00"
kind = "agent-free"                   # or "beside-agents"
note = "optional, never read"
```

- `agent-free`: no agent session, build or test runs beside the runner (V4). `beside-agents`: agents keep working, and
  the jobs keep to the beside-agents caps ([60 §3.15] "by day"). With the default job list nothing runs beside agents
  yet (§5, §7 item 4).
- A window is `[start, end)`, at most 72 hours long (profile L's 3-day freeze); windows do not overlap. Unknown keys,
  a time without its offset and an empty or reversed window are refused.

## 3. The configuration: `xtask/nightly.toml`

Operational policy with reviewed defaults (AGENTS.md). The loader refuses unknown keys, and the limits PLAN WP-05
states cannot be widened: `max-targets` ≤ 2, `rss-limit-mb` ≤ 256, `[window.beside-agents] ram-budget` ≤ 1 GB,
`ram-floor` ≥ 1.5 GB and `disk-floor` ≥ 25 GB (a floor may be raised, never lowered).

| Key | Default | Meaning |
|---|---|---|
| `[run] lane` | `"A"` | the lane whose target directory the runner builds in; cargo's build-directory lock there is the lane's build semaphore (PLAN §2.1) |
| `[run] finish-margin-minutes` | 10 | every job ends this long before the window does; the finish (§6) must fit in it. Each run's `finish.json` records how long the finish took, so the margin is sized from the first real runs |
| `[run] watch-seconds` | 30 | the RAM watchdog's period while a job runs |
| `[run] log-cap` | `"8MiB"` | a longer job log keeps its first quarter, a line saying how much was left out, and its last three quarters |
| `[run] keep-runs` | 30 | raw result directories kept; the oldest beyond it are removed after each run (0 keeps all) |
| `[guard] ram-floor`, `disk-floor` | `"1.5GB"`, `"25GB"` | the guard's floors (PLAN WP-05, [MP §8.1]); no lower value loads |
| `[guard.caps]` | `laneA`, `laneB` 40 GB; `fuzz` 10 GB; `mutants` 20 GB | the four counted directories under the target root and their caps ([MP §8.1] open point 8); the loader requires exactly the lanes of `xtask/roles.toml` and `fuzz` and `mutants`. See §7 item 3 |
| `[window.<kind>] build-jobs` | 6 agent-free, 2 beside agents | `CARGO_BUILD_JOBS` for every cargo the runner starts (until measurement 21, WP-57, sets the lane cap); a job that runs `parallel` cargo processes gives each `build-jobs / parallel` (at least 1), so the job's total stays within it |
| `[window.<kind>] test-threads` | 4, 2 | `RUST_TEST_THREADS`, shared the same way |
| `[window.<kind>] parallel` | 2, 1 | the processes one job runs side by side (cargo-mutants `--jobs`); at most `build-jobs` |
| `[window.<kind>] ram-budget` | 6 GB, 1 GB | the largest RAM budget a job may declare in that kind of window; a larger one is skipped |
| `[[job]]` | §5 | `name`, `kind`, `title`, `ram-budget`, `min-minutes` (start only with this much time left), `max-minutes` (0: to the deadline), and the kind's keys: `packages`, `tier` and optional `args` (cargo-test); `packages`, `shards`, `tier` (mutants); `max-targets`, `rss-limit-mb`, `grace-seconds` (fuzz: 60 to 3600, default 120) |

## 4. Pre-checks

Every refusal is listed; any one stops the runner before anything starts (exit 3).

| Refusal | When |
|---|---|
| `host` | `%SystemRoot%\System32\taskkill.exe` is missing: the runner stops job process trees with it, so it runs on the Windows laptop of profile L |
| `config` | `xtask/nightly.toml` or `xtask/roles.toml` does not load |
| `calendar` | the calendar cannot be read or is malformed |
| `outside-window` | no window holds now; the next window is named |
| `window-ending` | the current window ends within the finish margin |
| `run-locked` | another runner holds `/private/nightly/run.lock`. Pre-checks that pass keep the lock, and `run` holds it until it ends, so a second runner is refused here (exit 3) even while the first builds its guard |
| `dirty-tree` | the working tree has a change or an untracked file, or `HEAD` names no commit: the results must name the commit they tested |
| `lane-busy` | agent-free windows only: a cargo build holds a lane's build-directory lock (`<lane>/<profile>/.cargo-lock` or `<lane>/<triple>/<profile>/.cargo-lock`). Beside agents a held lock is no refusal: the runner's cargo waits on it, within the job's deadline |
| `ram-low`, `disk-low`, `ram-unreadable`, `disk-unreadable`, `dir-unreadable` | the guard's refusals ([MP §8.1]): below 1.5 GB of available physical memory, or below 25 GB of disk headroom on the target root's volume after reserving each counted directory's growth up to its cap |
| `guard` | the guard cannot be built or run, or its output is not `moirai-probes/guard/1` with an agreeing exit code: the runner fails closed, as the guard does |

The guard runs when every check before it passed, or at once when `--guard` names a built one, so `check` with
`--guard` and injected readings exercises each guard refusal on its own. The unit tests drive the runner with the
guard's output; the end-to-end form of E10's refusal check, for the first real session, runs the real guard: build it
(`cargo build -p moirai-probes-bin --bin guard`), write a synthetic calendar outside `/private/` with a window around
the chosen `--now`, and run `cargo xtask nightly check --guard <built guard> --windows <synthetic file> --now <time>`
with `--inject-available-physical 1GB` (refused `ram-low`), then with `--inject-volume-available 1GB` (refused
`disk-low`), then with `--inject-volume-available 200GB` and `--inject-dir-size laneA 0` (no guard refusal), reading
each run's refusals and exit code. The unreadable refusals cannot be injected from the command line; the guard's own
tests cover them (`moirai-probes`).

## 5. The run and the jobs

The jobs run one at a time in file order, under the caps of the window's kind. A job is **skipped** when its RAM budget
exceeds the window's, or when less than its `min-minutes` is left before the deadline (the window's end less the finish
margin). Its own deadline is the earlier of the window's deadline and its start plus `max-minutes`; a job still running
then is stopped with its process tree and is **timed-out**. The guard checks the RAM floor alone before each job and
every `watch-seconds` while it runs; a refusal stops the job (**stopped**) and the run, and the jobs after it are
**not-run** ([60 §3.15]: "everything refused below 1.5 GB free"). Since the jobs never overlap, a job's RAM budget is the
total beside agents (PLAN WP-05: ≤ 1 GB).

After each job that ran, the runner reads the tree again: when `HEAD` or the branch moved, a tracked file changed, or
git could not be read, the run records `tree_changed`, stops (the remaining jobs are not-run) and fails, since its
results would no longer name one commit. Untracked files are not counted there: a job may leave one (a failing
proptest's regression file) without changing what it tested.

**RAM budgets.** No measurement backs a job's peak memory yet, and every job builds before it tests (debug builds and
links of large crates with 2 jobs, cargo-fuzz's release build with libFuzzer's C++). So every job of the default list
declares 3 GB, above the beside-agents 1 GB, and runs in agent-free windows only, as [60 §3.15] places the GT suites in
the night schedule. A beside-agents window then runs none of them (verdict `no-job-ran`); §7 item 4 says what would
change that.

**Turns.** The mutants and fuzz jobs rotate by their **turn**: one more than the turn the latest earlier run recorded
for that job in `run.json` (a run that skipped the job records none), or 0. So successive runs of a job walk every shard
and every target, however the windows fall on calendar days: two windows on one day take two shards, and a day without
a window skips none.

| Kind | Job (default list) | Command |
|---|---|---|
| `cargo-test` | `gt1`: GT1 full, the crash enumerator's nightly tier on its own mini log and on the toy log with every seeded bug (WP-32, WP-40); `gt18`: GT18 long, the model's suites at the nightly tier, its state oracles among them (WP-94) | `cargo test --locked --no-fail-fast -p …` in the build lane's target directory, at the job's `MOIRAI_TEST_TIER`, under the gate's poisoned C toolchain variables so the lane's build is shared with the gate's `test` step; optional `args` for the test binaries |
| `mutants` | `gt16`: GT16's sample over FL-1 (`moirai-files`, `moirai-diff`) | `cargo mutants --shard k/n --sharding round-robin --gitignore true --jobserver false --jobs <parallel> --output <run>/<job> -p …`, k = the job's turn mod n; `CARGO_TARGET_DIR` removed and `TMP`/`TEMP` in the mutants directory, never `--in-place`; each of the `--jobs` processes gets `CARGO_BUILD_JOBS` = build-jobs / parallel and `RUST_TEST_THREADS` = test-threads / parallel (tools.md §5); `.cargo/mutants.toml` applies when WP-65 adds it. In an agent-free window the job first removes the scratch copies (`cargo-mutants-*.tmp`) an earlier, stopped mutants job left in the mutants directory and records their names (`scratch_removed`) |
| `fuzz` | `fuzz`: GT5, at most two targets | for each of the turn's targets (k consecutive `[[bin]]` names of `fuzz/Cargo.toml` from position turn × k mod n): `cargo fuzz build -s none <t>`, then `cargo fuzz run -s none <t> -- -rss_limit_mb=256 -max_total_time=<s> -print_final_stats=1`, s an equal share of the time left before the deadline less `grace-seconds`; from `fuzz/` on its pinned nightly (`RUSTUP_TOOLCHAIN`), in the fuzz directory, unpoisoned |

**The fuzz grace.** cargo-fuzz's start and cargo's freshness check come before libFuzzer's clock starts, and libFuzzer
checks `-max_total_time` only about once a second, so a target given the time up to the deadline would always outlast
it, be stopped there (timed-out, a failed night) and never print its final statistics. `grace-seconds` (default 120,
at least 60) is kept free after the last target's time.

Outcomes: a cargo test passes on exit 0. cargo-mutants exits 0, 2 (mutants missed) or 3 (timeouts) when the sample ran,
and those pass: GT16's kill rate is judged per exit, and the record keeps the counts (total, caught, missed, timeout,
unviable, the share caught). A fuzz target fails on a non-zero exit or a new file in `fuzz/artifacts/<target>/`
(`crash-*`, `oom-*`, tools.md §4.5); reproduce it with `cargo fuzz run -s none <target> <artifact>` from `fuzz/`. A
fuzz job in which no target got a minute of fuzzing time is skipped. Until FL-1's first target exists (WP-65, WP-67)
the fuzz job is skipped, so a run's verdict is at best `pass-partial`.

## 6. Raw results and the finish

A run writes `/private/nightly/<start>.partial/` (`<start>` = `YYYYMMDDTHHMMSSZ`). The private manifest leaves out
`nightly/run.lock` and every `nightly/*.partial/`, so the pre-commit guard keeps accepting commits while a run writes.

**The finish**, after the last job: the directories crashed runs left (`<start>.partial`, other than the run's own) are
renamed `<start>.aborted`; the run's directory is renamed `/private/nightly/<start>/`; the oldest runs beyond
`keep-runs` are removed; the private manifest is rebuilt, which lists the sealed results' files with their hashes; last,
`<start>/finish.json` is written, which the manifest leaves out so it stays current. Sealing a crashed run's directory
only here, just before the rebuild, keeps the manifest current for the whole run: done at the start, the rename would
leave it stale, and every commit in every worktree refused, until the end.

**No shingles from nightly results.** Files under `nightly/` are machine output of tests over synthetic data, not owner
text, and they quote the repository: test names (often 8 words or more, so a log line repeats a test's `fn` line),
panic messages built from string literals, compiler snippets, mutated source lines. Their shingles would refuse the
tested branch's own commits, so the manifest lists their hashes only (a copied file is still refused) and takes no
shingles from them (`xtask/src/private.rs`).

**Empty results match nothing.** cargo-mutants creates `missed.txt`, `caught.txt`, `unviable.txt` and `timeout.txt` in
every `mutants.out`, empty when that list has no entries, so a sealed GT16 run almost always holds an empty file. The
manifest lists it (it stays in the tree digest), but the copy check never matches empty content: an empty file
carries no data, and its hash would refuse every commit that adds an empty file or empties one
(`xtask/src/private.rs`).

| File | Content |
|---|---|
| `run.json` | the run record, schema `moirai-xtask/nightly/1`, rewritten after every job: `started`, `ended` (the last job's end), `window`, `deadline`, `commit`, `branch`, `lane`, `caps`, the pre-check's `guard` output, `jobs` (per job: `name`, `kind`, `title`, `status`, `reason`, `ram_budget`, `started`, `ended`, `elapsed_secs`, `deadline`, `exit_code`, `commands`, `log`, `log_capped`, `min_available` (the smallest available physical memory the watchdog read), and per kind `tier`; `turn`, `shard`, `mutants` and `scratch_removed`; or `turn`, `rss_limit_mb`, `grace_secs` and `targets`), `tree_changed` (null, or the job after which the tree moved and what was read), `counts` (`passed`, `failed`, `skipped`, `not_run`), `aborted_runs_sealed`, `verdict` |
| `finish.json` | schema `moirai-xtask/nightly-finish/1`, written after the manifest rebuild and left out of the manifest: the seconds each step of the finish took (`seal_aborted_secs`, `seal_secs`, `prune_secs`, `manifest_secs`, `finish_secs` in all), `manifest` (`rebuilt` or its error), `left_secs` (the window's end less the finish's end, negative when over) and `inside_window` (E10). Durations and a flag only, no times of day |
| `<job>.log` | the job's commands and their merged output, capped (§3) |
| `<job>/mutants.out/` | cargo-mutants' output for a mutants job |

Statuses: `passed`, `failed`, `timed-out`, `stopped`, `error` (a command could not be started, waited for or stopped),
`skipped`, `not-run`. Verdicts: `pass` (every job passed), `pass-partial` (none failed, some were skipped: a job skipped
every night stays visible), `no-job-ran` (every job was skipped), `fail` (a job failed, timed out, was stopped or had an
error, or the tree changed). The last line of the output names the verdict and the counts; a run whose finish ended
after its window also prints a line saying so.

## 7. Open points for the review

1. **Hard memory caps.** A job's RAM budget is declared and kept by its caps (one job at a time, build jobs, test
   threads, `--jobs`, `-rss_limit_mb`), and the watchdog stops a job when available physical memory falls below the
   floor; nothing caps a job's tree at its budget. A Windows job object with a memory limit would, but `moirai-os` has
   no job-object call and xtask may not use OS bindings (GT20 (d)). If the review wants a hard cap, it is an
   `[OS/proc]` addition (R-SPEC-P, then R-HARN-O) that the guard's binary would apply.
2. **What GT18 long selects.** The `gt18` job runs the model's whole suite at the nightly tier, of which GT18's state
   oracles are a part; R-HARN may not read `moirai-model` (PLAN §3.1), so test-name filters (`args`) are R-MODEL's to
   propose once WP-94 names its tests.
3. **The disk caps and free space: a prerequisite of E10.** With the default caps, `nightly check --guard` refuses
   `disk-low` on the laptop today. On 2026-10-06 the review read 26.17 GB available on D: (25.28 GB later that day),
   with laneA at 26.6 GB of its 40 GB cap and laneB at 28.2 GB of 40, and the fuzz and mutants directories nearly
   empty: the four caps (110 GB) reserve 54.72 GB, so the headroom is 0 against the 25 GB floor. The caps are the
   space the directories may grow to, so lowering them to today's sizes would only hide the shortfall: even with
   nothing reserved, 26 GB available leaves about 1 GB above the floor. About 54 GB must be freed before a run can
   start, for example the old probe and scratch target directories beside the lanes under `D:\moirai-target\` (such
   as `laneA-probe`, `laneB-probe`, `laneB-scratch-review` and `spec3fix2`) once no session uses them; which to remove
   is the orchestrator's or the owner's call (tools.md §12 item 9). The values are operational; only buying space
   would be an owner question. Measurement 21 (WP-57) sets the lane build cap and, with the first runs, the space the
   lanes need.
4. **A job's own peak memory.** The record keeps `min_available` (the machine's lowest available memory during a job),
   not the job's own peak, so the beside-agents 1 GB of PLAN WP-05 cannot yet be shown for any job; that is why every
   job declares 3 GB (§5). Recording a process tree's peak needs an OS reading xtask may not make (GT20 (d)): an
   `[OS/proc]` call (process-tree peak working set, or the job object of item 1) that the guard's binary would read.
   With it, a job whose measured peak stays under 1 GB (the fuzzing itself, once its build is warm) could return to
   beside-agents windows.
5. **The finish margin.** `finish-margin-minutes` = 10 is a first value; the finish's steps are timed into each run's
   `finish.json` (the manifest rebuild walks all of `/private/` and `master`'s tracked text), and the first real runs
   size it.
