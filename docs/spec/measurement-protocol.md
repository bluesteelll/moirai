# Measurement protocol

| Field | Value |
|---|---|
| Title | The measurement protocol: hosts, conditions (idle, loaded, synthetic), sample-size tiers, repetitions and statistics, interleaved floors, the timer, process readings, gates, the noise band, records and aggregates, the pre-run guard, and the load fixture with its recorder and generator |
| Chapter | `[MP]`, `docs/spec/measurement-protocol.md` |
| Status | draft, pass 1 pending |
| Work package | WP-50 (role R-HARN-I), [PLAN §3.2] item 5; §9 WP-51 (R-HARN-I) |
| Sources | [60 §5.1] (every row: load, sample size, floors, idle CPU, RSS, counts, spawn, noise band, and the paragraph above the table); [60 §5.2] items 11, 16, 19, 21; [60 §5.3] (floor-relative gates); [60 §3.15] (profile L: "everything refused below 1.5 GB free"; the disk row: "the harness refuses to start below 25 GB free"); [74 A04] (the tiers), [74 A24] (the disk budget); [61 M-6] (why the protocol exists); [AR §8.2] (the paragraph "M0 measurements on the owner's machine"); [80 §2.9] (metering per OS, the floor-relative gate form); [PLAN §2.1] (target directories, "WP-05's disk guard counts all four"), [PLAN §3.2] item 5 (the shared acceptance of WP-50 to WP-58), WP-03 (the private manifest), WP-05 (the guard's refusals), WP-50, WP-51 (the load fixture, its recorder and generator, the noise bands, `noise.yml`), [PLAN §5] (V4, V5; the owner's deferral of measurements of 2026-09-29); [AR §11] #35 and #37 (the fixture: a resource profile only, never off the laptop); `docs/m0/tools.md` §10 (typeperf, WPR) |
| Depends on | [OS/README §4.3] (the `Meter` seam), [OS/mem] (its quantities and sources), [OS/fs §4.11] (`free_space`), [OS/fs §4.13] (`counters`) |

---

## 1. Scope and conventions

### 1.1 What this protocol governs

This file freezes the measurement protocol of [60 §5.1] ([61 M-6]). It governs:
- every M0 measurement of [60 §5.2] (WP-51 to WP-57);
- every exit criterion that concerns time, memory or Windows behaviour, at every milestone ([60 §5.1], [60] P6);
- the nightly regression runs of profile L (GT11, [60 §3.13]), which use the same protocol on the same machine;
- the noise runs of measurement 16 on the laptop and on the hosted runners.

It is implemented once, by the test-only library `moirai-probes` (the framework: tiers, plans, the interleaved runner,
statistics, the idle observation, records, gates, noise bands, aggregates and the guard's logic), and wired to the
Windows OS layer by the
composition root `moirai-probes-bin` ([PLAN §2.2], §6.2 R17); the load fixture's recorder of §9 is the tool
`cargo xtask loadrec`. The measurements themselves and their probe bodies are the later work packages' ([PLAN §3.2]
item 5). By the owner's decision of 2026-09-29 no measurement is run until the
owner resumes them ([PLAN §5]); this protocol and its framework are written now.

### 1.2 Normative words and citations

Normative words are [F01 §2.1]'s. This file is cited `[MP §x]` (open point 13). A row of [60 §5.2] is "measurement
<n>"; [60 §5.1]'s rows are cited by their first word ("the RSS row").

### 1.3 Units

| Kind | Unit | Representation |
|---|---|---|
| time | nanoseconds (`ns`) | unsigned 64-bit integer |
| memory and disk | bytes | unsigned 64-bit integer |
| counts | events | unsigned 64-bit integer |

- **KB, MB, GB, TB** are 10³, 10⁶, 10⁹ and 10¹² bytes; **KiB, MiB, GiB, TiB** are 2¹⁰, 2²⁰, 2³⁰ and 2⁴⁰ bytes. A budget
  is read in the unit it is written in ("4 MB" is 4,000,000 bytes, "4 MiB" is 4,194,304 bytes) (open point 6).
- **ms, µs, s** are 10⁻³, 10⁻⁶ and 1 second; a duration bound such as "1 ms" is 1,000,000 ns.

## 2. Hosts and conditions

### 2.1 Hosts

| Host kind | What it is | What it may decide |
|---|---|---|
| `laptop` | the owner's machine ([60 §5.1]): Ryzen 9 5900HS, 16 GB, consumer NVMe without power-loss protection, NTFS, Windows 11, Defender real-time protection on | every timing, memory and Windows-behaviour gate, every M0 decision of [60 §5.2] and every exit decision |
| `hosted` | a GitHub-hosted Windows runner (`noise.yml`, [PLAN §3.2] WP-51) | nothing: its noise band is reported, and a hosted runner never decides a timing gate ([60 §5.1]); `pr.yml` runs no timing, RAM, floor, crash or kill gate ([PLAN §3.2] WP-04) |

Laptop runs are made in windows agreed with the owner ([PLAN §5] V4), never during the owner's benchmark windows
([60 §5.1], [02 §9]).

### 2.2 The Windows build and the Defender versions

Every run records a **host snapshot** at its start and another at its end:

| Field | Source | Form |
|---|---|---|
| Windows version | the output of `cmd.exe /d /c ver`: the last space-separated token inside the last `[`…`]` pair, which is the dotted version number in every display language | `<major>.<minor>.<build>.<revision>` (three or four dot-separated decimal fields) |
| Defender product version | `Get-MpComputerStatus`, property `AMProductVersion` | the string as reported |
| Defender engine version | property `AMEngineVersion` | the string as reported |
| Defender signature version | property `AntivirusSignatureVersion` | the string as reported |
| Defender real-time protection | property `RealTimeProtectionEnabled` | `true` or `false` |

The Defender properties are read with
`powershell.exe -NoProfile -NonInteractive -Command "Get-MpComputerStatus | Select-Object AMProductVersion,AMEngineVersion,AntivirusSignatureVersion,RealTimeProtectionEnabled | ConvertTo-Json -Compress"`
and parsed as JSON. Both programs are run by their full paths below `%SystemRoot%` (`System32\cmd.exe` and
`System32\WindowsPowerShell\v1.0\powershell.exe`), never looked up by name, so a program of the same name beside the
probe binary or on `PATH` is never run in their place. Each command is given at most 30 s; a command still running then
is stopped. A source that fails (`%SystemRoot%` unset, no Defender module, a non-zero exit, unparsable output, a
command stopped at 30 s) is recorded as an error with its reason; the run itself is still recorded (§7.3 decides what
it may then decide). Both commands are documented interfaces of Windows; neither reads a user, host or machine
identifier.

### 2.3 Conditions

Each measurement is recorded idle and loaded, as two runs under this protocol ([PLAN §3.2] item 5).

| Condition | Definition | Checks the framework makes |
|---|---|---|
| **idle** | No user process beyond the OS baseline ([60 §5.1]): the run is made in an agent-free window, with no agent session, build, test, load generator or other measurement running. The driver attests the condition when it starts the run. | `Meter::available_physical` read at every round boundary (§4.3) is ≥ 1.5 GB (the guard's default RAM floor, §8.1; a `--ram-floor` override of the guard does not move it, so a record's reasons can be recomputed when it is read back, §7.3); a reading below it, or a reading that fails, makes the run invalid |
| **loaded** | The **16-agent load fixture** — the CPU, disk and memory profile of one real 16-agent campaign of the owner, resource counters only and no content ([60 §5.2] item 16; recorded by `xtask loadrec`, WP-51a) — replayed by `moirai-probes-bin loadgen` (WP-51, §9) on the machine under test, with available physical memory held at 1.8 GB ([60 §5.1]) | every round-boundary reading of `Meter::available_physical` lies in [1.6 GB, 2.0 GB], the tolerance of "≈ 1.8 GB" (open point 5); a reading outside it, or a reading that fails, makes the run invalid |
| **synthetic** | A load that is not the fixture: the hosted runners' synthetic load of `noise.yml` ([PLAN §3.2] WP-51), described in the record | readings are recorded, not checked |

A loaded run is valid only if, in addition:
- (a) the load generator reported its replay settled before the first pilot sample (§4.2) and kept replaying until
  after the last sample of the last repetition;
- (b) the generator's validation of the replay against the profile ([PLAN §3.2] WP-51) passed over the run's window
  (§9.5).

The driver records (a) and (b) together as the run's **replay verdict** after the run (§7.1, `load_replay_valid`). The
record names the fixture by the BLAKE3-256 of its file. Until the owner has recorded the fixture (V5, deferred), no
loaded run can be recorded; a synthetic run never stands in for one.

A hosted runner runs idle or under synthetic load, never loaded (§2.1, §6): the framework refuses a hosted loaded run
before its pilot, and a record of one when it is read back (§7.3).

## 3. Sample sizes, repetitions and statistics

### 3.1 Tiers

The sample size of a quantity is tiered by the duration *d* of one operation ([60 §5.1], [74 A04]):

| Tier | Duration of one operation | Samples per repetition, at least | Repetitions | Gated statistic |
|---|---|---|---|---|
| `t1` | d < 1 ms | 10,000 | 5 | p99 |
| `t2` | 1 ms ≤ d < 50 ms | 1,000 | 5 | p99 |
| `t3` | 50 ms ≤ d < 1 s | 200 | 5 | p95 and the maximum |
| `t4` | d ≥ 1 s (explicit commands) | 20 | 3 | the maximum |

The intervals are half-open; a duration of exactly 1 ms is `t2`, exactly 50 ms `t3`, exactly 1 s `t4` (open point 1).
A requirement **covers** another when it takes at least as many samples per repetition and at least as many
repetitions.

### 3.2 The duration that selects a tier, and the plan

- For an arm (§4.1) whose samples are times, *d* is the median (§3.4) of its pilot samples (§4.2). For an arm whose
  samples are bytes or counts, *d* is the median of the elapsed times of its pilot samples, as the runner times each
  sample.
- A probe may declare a **minimum tier** for a quantity: for a gated quantity, the tier its budget falls in.
- One **plan** serves every arm of a run, because the arms are interleaved block by block (§4.3): its samples per
  repetition are the largest requirement among the arms' pilot tiers and the declared minimum, and its repetitions the
  largest likewise (open point 2). Each arm is still gated on its own tier's statistic (§3.3).
- A faster tier takes more samples and at least as many repetitions, so every plan is the requirement of one tier, a
  **tier plan**: 20 × 3, 200 × 5, 1,000 × 5 or 10,000 × 5.
- The tiers govern distributions. The idle-CPU row is an observation and is recorded outside them (§4.7).

### 3.3 The tier check

After the last repetition, for every arm and every repetition, the framework takes the tier of that repetition's median
duration (the p50 of a time arm's samples; the median elapsed time of a bytes or counts arm's samples).
- If the plan does not cover that tier's requirement, the run is **under-sampled**. Its samples are discarded, its plan
  is recorded as an escalation, and the run is repeated with the plan raised to cover the fastest such tier. A tier only
  rises, so a run escalates at most three times (`t4` → `t3` → `t2` → `t1`): the escalations are tier plans that rise
  strictly, none covers the final plan's tier, and the first covers every arm's pilot tier.
- Otherwise the run stands. An arm's **gate tier** is the tier of the median, over the repetitions, of its
  per-repetition median durations; the arm is gated on that tier's statistic, and the larger samples a slower arm took
  stay valid.

### 3.4 Statistics

- **Percentiles are nearest-rank.** With the n samples of one arm in one repetition sorted ascending as x₁ ≤ … ≤ xₙ, the
  q-th percentile is x_k with k = ⌈q·n / 100⌉, computed in integers as (q·n + 99) div 100; the minimum is x₁ and the
  maximum xₙ (open point 3).
- **Per repetition**, every arm reports n, the minimum, p50, p95, p99 and the maximum.
- **The median over the repetitions.** For each statistic, the median of its per-repetition values: the middle value
  of the sorted values, rank ⌈r / 2⌉ for r values. The repetitions are 5 or 3, so the median is the middle one. Every
  gate applies to this median ([60 §5.1]).
- **Bytes and counts** are gated on the maximum unless the row names another statistic; the median over the
  repetitions applies the same way.
- The 1e6 and 0.3–0.5 M rows run weekly in a laptop window and at M11 ([60 §5.1]).

## 4. Running a measurement

### 4.1 Arms

A run measures one quantity. Its **arms** are the measured operation (arm 0) and the floors it is gated against
(arms 1, 2, …); a run of floors alone (measurement 11) has only floor arms. Each sample of an arm is one operation (or
one batch, §4.5) and yields one unsigned 64-bit value in the arm's unit (§1.3). Setup and teardown that are not part of
the operation (a fresh store copy for an operation that consumes its input, for example) stay outside the timed region.
Arm and quantity names are 1 to 64 bytes of `[a-z0-9._-]` starting with a letter or digit.

### 4.2 The pilot

Before the first repetition, each arm in arm order runs a **pilot**: the runner announces one block of 5 to the arm
(§4.3), then takes samples until 5 have been taken or 1 s has elapsed since the announcement returned, and at least one.
A time arm's pilot runs with a batch of 1. The pilot samples select the tiers (§3.2) and the batch (§4.5) and are the
warm-up; they are not part of any statistic.

### 4.3 Interleaved blocks: the floors

Floors are re-measured interleaved with the gated operation, in the same run, in alternating blocks ([60 §5.1]), so a
slow disk day moves the floor and the operation together ([60 §5.3]):
- Each repetition takes n samples of every arm, in **rounds**. In each round every arm takes one **block** of
  B = min(100, ⌊n / 20⌋) consecutive samples; the last round takes the remainder. Since n ≥ 20, B ≥ 1 and n / B ≥ 20,
  so every repetition has at least 20 rounds; the tiers' sample counts give B = 1, 10, 50 and 100 (open point 4).
- In repetition r (counted from 0), the arm order is rotated by r: with k arms, the i-th block of a round belongs to arm
  (r + i) mod k.
- Before the first sample of every block, the runner **announces** the block to its arm with the block's length
  (`Arm::begin_block`). An arm that takes a block's samples in one go (a **bulk arm**: a hyperfine invocation, §4.9)
  takes them then and returns them in order as its next samples; samples of an earlier block that were not read are
  discarded. An arm that samples one operation at a time ignores the announcement. A failed announcement stops the run,
  as a failed sample does.
- `Meter::available_physical` is read at the start of every round and once after the last round of each repetition;
  these are the round-boundary readings of §2.3.
- Repetitions follow one another without a pause.

### 4.4 The timer

Times come from the process's monotonic clock (`std::time::Instant`; `QueryPerformanceCounter` on Windows), as whole
nanoseconds. At the start of every run the framework measures the timer's **resolution**: the smallest positive
difference between consecutive readings over 10,000 readings. A run whose timer does not advance over those readings is
refused.

### 4.5 Operations below the timer's resolution

If a time arm's pilot median is below 20 times the resolution, each of its samples times a batch of
k = ⌈20 × resolution / max(pilot median, 1 ns)⌉ consecutive operations and records the per-operation mean (the batch
time divided by k, rounded down). The record states k; the arm's percentiles then describe means over batches of k,
and the aggregate says so (open point 11). With the 100 ns resolution of `QueryPerformanceCounter` this applies below
2 µs.

The runner sets k after the pilot and refuses the run before its first repetition when the arm does not take it (an arm
that cannot batch, such as a bulk arm, is measurable only where k is 1); a record whose time arm's batch is not the k of
its pilot median and timer resolution is refused when it is read back (§7.3). Each operation's result is kept from the
optimiser (`std::hint::black_box`), so a pure operation whose result is unused is still executed k times; a probe body
returns what it computes.

### 4.6 Process readings and RSS

- At the end of every run the framework records the probe process's own `Meter::private_peak` and, when the binary
  installed `CountingAlloc`, the heap high-water mark of `Meter::heap_counts` ([OS/mem §3, §6]). The runner resets the
  heap high-water mark (`Meter::reset_heap_high_water`) before the first pilot, so the recorded mark is the run's.
  `private_peak` cannot be reset: it is the process's peak since it started, so a driver that records it for a run
  makes that run in a process of its own (one process per run).
- **RSS** of a measured process ([60 §5.1] RSS row, [OS/mem §2]): peak private bytes (`PeakPagefileUsage`) read at the
  process's exit through `Meter::peak_of_child` ([OS/proc §9]), with the heap-only high-water mark reported beside it;
  for the MCP server also the steady state at the end of a scripted session (`Meter::child_private_now`); aggregates
  summed over every moirai process in the 16-session fixture; a VMMap breakdown (private, page tables, shareable) per
  process kind at M0 and M11; the peak working set reported, not gated ([71 RAM-M2, RAM-m9]).
- The floor-relative RSS form of the ports, `private_peak(command) − private_peak(empty binary)`, uses
  `moirai-probes-bin empty`, built with the same toolchain and allocator and measured in the same run ([80 §2.9],
  [OS/mem §8]); on Windows it is reported beside the absolute gate.

### 4.7 Idle CPU

Idle CPU is zero CPU-time delta (`Meter::cpu_times`, user plus kernel) **and** zero context switches (ETW, read by the
harness outside `Meter`, [OS/mem §7]) of the process over 10 minutes, measured from 15 s after its last request (a
blocking-pool thread may wake once at its keep-alive, [71 RAM-m2]) ([60 §5.1]). It is an observation, not a
distribution, so it is not run by the tiered runner of §3–§4.5 and the tiers do not apply to it. The framework records
it as an **idle observation** (§7.1, `moirai-probes/idle/1`) of three windows:
- Before each window the driver has the observed process answer one request. The window starts 15 s after the answer
  and lasts 10 minutes on the monotonic clock (§4.4). The OS timer the framework waits on is not that clock and may
  wake a little early against it, so each wait (the 15 s and the 10 minutes) is followed by up to four top-up waits of
  the remainder while the monotonic clock shows it short.
- At the start and the end of each window the framework reads the process's cumulative CPU time and its cumulative
  context switches; the window records the two deltas and its elapsed time.
- `Meter::available_physical` is read at the start of every window and once after the last, four readings in all; these
  are the observation's round-boundary readings, checked against its condition as in §2.3.
- The **gate** holds when the maximum over the three windows of the CPU-time delta and the maximum of the context-switch
  delta are both zero. A delta that cannot be read (a failed reading, or a counter that went down) and a window shorter
  than 10 minutes are validity reasons (§7.3), not gate failures; a non-zero delta is a gate failure, not a validity
  reason.
- A request that fails stops the observation; nothing is recorded.

An observation takes three times 10 minutes and 15 s, about 31 minutes, plus the requests. Its validity and exit grade
follow §7.3 like a run's.

### 4.8 Counts

Flushes per commit, bytes read on open and log bytes appended by read verbs are counted by the `Vfs` instrumentation —
the difference of `Vfs::counters` ([OS/fs §4.13]) around the operation — never inferred from time ([60 §5.1]). They are
count or bytes arms (§4.1), gated on the maximum.

### 4.9 Spawn

Spawn-to-exit is measured with `hyperfine -N --warmup 5`, directly and through the agent's Git-Bash wrapper
([60 §5.1]). It is a time arm like any other: its tier follows §3.2, and its floor (the spawn of `moirai-probes-bin
empty` from the same install path) is interleaved per §4.3. A spawn arm is a bulk arm (§4.3): each block of B samples
is one hyperfine invocation with `--runs B --warmup 5 --export-json`, run when the runner announces the block, and the
block's samples are the per-run times of the export (`results[0].times`, seconds, rounded to the nearest nanosecond),
in order. An invocation that fails, or that returns another number of times than B, stops the run. The pilot is
announced as one block of 5 (§4.2), so a spawn arm's pilot is one invocation with `--runs 5`; the pilot's 1 s starts
when the invocation returns, so its five runs are the five pilot samples. A spawn arm cannot batch; every spawn takes far more
than 20 × the timer's resolution, so its k is 1 (§4.5).

## 5. Gates

- **Absolute gate.** The median over the repetitions of the arm's gated statistic is at most the budget.
- **Floor-relative gate** ([60 §5.3]). For each repetition r, dᵣ = op(r) − f × floor(r), where op(r) and floor(r) are
  the named statistic (p50, p99, …) of the operation's arm and of the floor's arm in repetition r, and f is the row's
  factor (1 unless the row states one, as the ports' "3 × floor p99"). The gate holds when the median of the dᵣ is at
  most the row's allowance. The floor is the one measured in the same run (§4.3). Pairing within a repetition keeps a
  slow stretch of the run on both sides of the difference.
- Only an exit-grade run (§7.3) decides a gate or an M0 decision.

## 6. The noise band

- **Definition.** For every gated quantity — a quantity, an arm, its gated statistic and a condition — on each host
  kind, the noise band is the run-to-run spread of the gated statistic over the repetitions of one run: band =
  max − min of the per-repetition values, in the quantity's unit; the relative band, band ÷ median, is reported beside it.
  When several noise runs exist for the same quantity, host and condition, the band is the largest of theirs.
- **Where it is measured.** At M0, by measurement 16: on the laptop (idle and loaded) and on the hosted runners (idle
  or under synthetic load, `noise.yml`) ([60 §5.1], [PLAN §3.2] WP-51; §9.7).
- **Baseline.** The median gated statistic of the quantity's reference run: the M0 measurement on the laptop under the
  same condition, replaced only by the accepted exit measurement of a later milestone. A nightly run never moves the
  baseline, so a slow drift of small steps cannot pass unnoticed (open point 7).
- **Regression.** A nightly value v regresses when v > baseline + band; a regression blocks ([60 §5.1]): the nightly
  run reports a failure. v < baseline − band is reported as an improvement. Otherwise v is within the band.
- An exit decision uses the laptop's value; the hosted runners' bands are reported and never decide ([60 §5.1]).
- An idle observation (§4.7) has no arm, statistic or repetitions, so it has no noise band, noise record or baseline;
  its gate (both maxima zero) is absolute in nightly runs too: a nightly observation that may decide (§7.3) and does
  not hold reports a failure, as a regression does.

### 6.1 Noise records and the comparison

**Noise runs.** A laptop band is taken only over exit-grade runs (§7.3), so a run with a Windows or Defender update
during it gives none; a hosted band over valid runs, idle or synthetic. A laptop band is idle or loaded, a hosted band
idle or synthetic.

**The noise record.** Each band is one JSON object, schema `moirai-probes/noise/1`:

| Field | Type | Content |
|---|---|---|
| `schema` | string | `moirai-probes/noise/1` |
| `measurement` | integer | the measurement's row number (≥ 1) |
| `quantity`, `arm` | string | the quantity and the arm (§4.1) |
| `statistic` | string | `min`, `p50`, `p95`, `p99` or `max` |
| `condition` | string | the condition kind: `idle`, `loaded` or `synthetic` |
| `host` | string | the host kind: `laptop` or `hosted` |
| `band` | integer | max − min of the per-repetition values, in the arm's unit; for several runs, the largest |
| `median` | integer | the median of the per-repetition values of the run whose band it is (relative band = `band` ÷ `median`) |
| `runs` | integer | the noise runs the band was taken over (≥ 1) |

A record whose host and condition the rules above exclude, or whose `measurement` or `runs` is 0, is refused when it is
read back. Bands of several runs combine only when measurement, quantity, arm, statistic, condition and host are all
equal.

**The comparison.** A nightly value is compared only when the reference run and the nightly run are exit-grade laptop
runs of the band's measurement and quantity under the same condition (for a loaded run, the same fixture), the band is
the laptop's band for that condition kind, the arm has the same unit in both runs, and the band's statistic is one the
reference arm is gated on (§3.3, §3.4). The baseline and the value are the medians over the repetitions of that
statistic in the two runs; otherwise the comparison is refused and the nightly run reports why.

## 7. Records and aggregates

### 7.1 The run record

Every run produces one JSON object, schema `moirai-probes/run/1`:

| Field | Type | Content |
|---|---|---|
| `schema` | string | `moirai-probes/run/1` |
| `measurement` | integer | the row number of [60 §5.2] (or of a later milestone's measurement list) |
| `quantity` | string | the quantity's name (§4.1) |
| `condition` | object | `{"kind":"idle"}`, `{"kind":"loaded","fixture":"<64 hex>"}` or `{"kind":"synthetic","description":"…"}` |
| `load_replay_valid` | boolean or null | the replay verdict of §2.3 (a) and (b); null until the driver sets it, and for idle and synthetic runs |
| `host` | object | `kind` (`laptop`, `hosted`), `start` and `end` snapshots (§2.2), each source as `{"ok":…}` or `{"error":"…"}` |
| `toolchain` | string | the `rustc` version the measured binaries were built with |
| `commit` | string | the git commit of the measured tree |
| `started`, `ended` | string | UTC, RFC 3339 with whole seconds (`2026-10-04T09:30:00Z`) |
| `timer_resolution_ns` | integer | §4.4 |
| `plan` | object | `n`, `repetitions`, `block` (§3.2, §4.3) |
| `escalations` | array | the plans of the discarded under-sampled attempts (§3.3), in order |
| `arms` | array | per arm: `name`, `unit` (`ns`, `bytes`, `count`), `batch` (§4.5), `pilot_median`, `pilot_tier`, `tier` (the gate tier), and `repetitions`: per repetition its `summary` (`n`, `min`, `p50`, `p95`, `p99`, `max`), `median_elapsed_ns` (bytes and counts arms; null for time arms) and `samples` (in the arm's unit, in sampling order) |
| `memory` | object | the round-boundary readings: `readings`, `failures`, `out_of_band`, `min`, `max` (bytes or null), `first_failure` (the error of the first failed reading, or null) |
| `process` | object | `private_peak`, `heap_high_water` (bytes or null, §4.6) |
| `reasons` | array of strings | why the run is invalid (§7.3); empty for a valid run |

A record without its `samples` arrays is a **summary record**; it carries every other field. The samples are on every
repetition of every arm or on none.

An idle observation (§4.7) produces one JSON object, schema `moirai-probes/idle/1`, with the fields `measurement`,
`quantity`, `condition`, `load_replay_valid`, `host`, `toolchain`, `commit`, `started`, `ended`, `memory` and `reasons`
of a run record, and:

| Field | Type | Content |
|---|---|---|
| `schema` | string | `moirai-probes/idle/1` |
| `settle_ns` | integer | 15,000,000,000 (the 15 s before each window) |
| `window_ns` | integer | 600,000,000,000 (the 10-minute window) |
| `windows` | array | three entries in order, each with `cpu_ns` and `context_switches` (the deltas, each `{"ok":n}` or `{"error":"…"}`) and `elapsed_ns` (the window's length) |

Its `memory` holds the four readings of §4.7.

### 7.2 Raw data

Raw records, run records and idle observations alike, go to
`/private/measurements/<n>/<quantity>.<condition kind>.<start>.json` in the main worktree, where `<start>` is the start
time as `YYYYMMDDTHHMMSSZ` ([PLAN §3.2] item 5). An existing file is never overwritten, and a file whose writing fails
is removed. The samples are streamed to the file, never held twice in memory. Raw data is never committed
([PLAN §2.5], AGENTS.md).

### 7.3 Validity and exit grade

A run is **valid** when its `reasons` is empty and, for a loaded run, `load_replay_valid` is `true`. The framework adds
a reason when a round-boundary reading fails or breaks its condition's band (§2.3), and when a repetition is
under-sampled (§3.3; the runner escalates until none is, so only a truncated or edited record can carry this reason);
for an idle observation, also when a window's delta cannot be read or a window is shorter than 10 minutes (§4.7).
A run whose names, condition, host or arms break this protocol is refused before its pilot, and a record that breaks
it is refused when it is read back. Read-back refuses a run record unless:
- the measurement number is at least 1, the names are valid (§4.1) and unique, the condition is well formed, a hosted
  run is not loaded (§2.3), only a loaded run carries a replay verdict, and both times are RFC 3339 UTC stamps;
- `timer_resolution_ns` is positive (§4.4); the plan is a tier plan with its block (§3.2, §4.3); the escalations are
  tier plans that rise strictly, none covers the final plan's tier, and the first plan covers every arm's pilot tier
  (§3.3);
- each arm's `batch` is the k of §4.5 for its pilot median and the timer resolution (1 for bytes and counts arms), its
  `pilot_tier` is its pilot median's tier and its `tier` the gate tier of its repetitions; it has the plan's number of
  repetitions, each with `n` samples, an ordered summary (min ≤ p50 ≤ p95 ≤ p99 ≤ max), `median_elapsed_ns` exactly
  for bytes and counts arms, and a summary that matches its samples when it has them;
- `memory` has repetitions × (rounds + 1) readings, `failures` + `out_of_band` ≤ `readings`, `first_failure` exactly when
  a reading failed, `min` and `max` null exactly when no reading succeeded, and `min`, `max` and `out_of_band`
  consistent with the condition's band;
- `reasons` contains every reason the framework adds for the record's readings and repetitions.

An idle observation is refused unless its header obeys the first rule, `settle_ns` and `window_ns` are those of §4.7,
it has three windows, its `memory` has four readings and obeys the same counting rules, and its `reasons` contains every
reason the framework adds.

A run is **exit-grade** when it is valid and all of these hold:
- its host kind is `laptop`;
- its condition is idle or loaded (never synthetic);
- both host snapshots were read, Defender real-time protection is on in both, and the start and end snapshots are
  equal — a Windows update or a Defender signature update during the run (which starts a scan burst) makes it not
  exit-grade (open point 10).

Only an exit-grade run decides anything (§5, §6). A run that is not exit-grade is still kept as raw data.

### 7.4 Aggregates

Aggregates go to `docs/measurements/m0/<n>.md`, one file per measurement ([PLAN §3.2] item 5), and later milestones
use `docs/measurements/<milestone>/<n>.md`. An aggregate holds, for every run: its condition, host kind, Windows
version, Defender versions and real-time state, toolchain, the first 12 hex digits of the commit, its start time,
validity and exit grade; for every arm of a run record: the quantity, the arm, the unit, the gate tier, n × repetitions,
the batch, the medians over the repetitions of p50, p95, p99 and the maximum, the gated statistic's median and its
spread over the repetitions; for every idle observation: the CPU-time and context-switch deltas of its three windows
and whether the gate holds; and the disqualifications of every run that is not exit-grade. It holds no samples, no
paths, and no user name, host name, volume serial, machine GUID, `BootId` or
process command line; the pre-commit scrub refuses them ([PLAN §3.2] WP-03). Each measurement's decision for WP-81a is
drafted in the same file (E8).

## 8. The pre-run guard

`moirai-probes-bin guard` is the pre-check of WP-05's nightly runner and of every measurement driver: nothing starts
when it refuses ([PLAN §3.2] WP-05, [60 §3.15]).

### 8.1 Refusals

| Refusal | Condition |
|---|---|
| `ram-low` | available physical memory (`Meter::available_physical`, [OS/mem §5]) below the RAM floor, by default 1.5 GB ([60 §3.15]: "everything refused below 1.5 GB free") |
| `disk-low` | the disk headroom below the disk floor, by default 25 GB ([60 §3.15] disk row, [74 A24]) |
| `ram-unreadable`, `disk-unreadable`, `dir-unreadable` | a reading the decision needs cannot be taken: the guard fails closed |

The **disk headroom** is the free space of the work volume counted after the counted directories ([PLAN §3.2] WP-05):
headroom = available − Σ max(0, capᵢ − sizeᵢ), floored at 0, where *available* is `Meter::free_space(volume).available`
([OS/fs §4.11]) and each counted directory i has a cap and a current size. A directory may still grow to its cap, so
its remaining growth is reserved before the 25 GB the nightly jobs need beside it are counted ([60 §3.15]: "beside each
lane's `target` directory") (open point 8). WP-05 passes the four directories of [PLAN §2.1] — the two lane target
directories and the fuzz and mutants directories, all on the work volume (`docs/m0/tools.md` §12) — with the caps of
its configuration. A directory over its cap reserves nothing and is reported as over its cap.

**The size of a directory** is the sum of the logical lengths of the regular files below it, walked recursively without
following symbolic links or junctions below the directory itself. A directory that does not exist has size 0 (it
reserves its whole cap); an entry that vanishes during the walk counts 0; any other error is `dir-unreadable`. Logical
lengths do not exceed allocated sizes for these directories, so the reserve errs towards refusing.

### 8.2 Invocation

```
guard --volume <dir> [--dir <path> <cap>]... [--ram-floor <bytes>] [--disk-floor <bytes>]
      [--inject-available-physical <bytes>] [--inject-volume-available <bytes>] [--inject-dir-size <path> <bytes>]...
guard --help
```

- `--volume` is required and names any existing directory on the work volume; `--dir` may repeat, with distinct paths.
  An empty path is a usage error. `--help` has no short form; any other argument is a usage error.
- A byte quantity is a decimal number with an optional fraction and an optional unit of §1.3 (`B`, `KB`, `MB`, `GB`,
  `TB`, `KiB`, `MiB`, `GiB`, `TiB`), and must come to a whole number of bytes: `25GB`, `1.5GB`, `30GiB`, `1500000000`.
- `--ram-floor` and `--disk-floor` override the default floors (operational policy, AGENTS.md); the output reports
  the floors used.
- The `--inject-…` options replace a reading with the given value, for testing the refusals (WP-05's acceptance:
  "the refusals are tested with injected values"). `--inject-dir-size` names a path given to `--dir`, byte for byte.
  The output marks every injected value; a real nightly run never passes one.

### 8.3 Output and exit codes

The guard prints one JSON object on one line to stdout, schema `moirai-probes/guard/1`: `verdict` (`pass` or
`refuse`); `ram` (`available`, `floor`, `injected`, `error`); `disk` (`volume`, `available`, `reserved`, `headroom`,
`floor`, `injected`, `error`); `dirs` (per directory: `path`, `cap`, `size`, `reserve`, `over_cap`, `injected`,
`error`); `refusals` (per refusal: `kind` of §8.1 and a `detail` text). It prints one line per refusal to stderr.

| Exit code | Meaning |
|---|---|
| 0 | pass (and `--help`) |
| 1 | refused |
| 2 | usage error: nothing was read |

## 9. The load fixture, its recorder and its generator

Measurement 16 ([60 §5.2] item 16) records and validates the 16-agent load fixture of the loaded condition (§2.3) and
measures the noise bands of §6. WP-51 builds its tools ([PLAN §3.2] WP-51): `cargo xtask loadrec`, which records the
fixture (WP-51a); `moirai-probes-bin loadgen`, which replays it; `moirai-probes-bin noise`, which computes noise bands;
and `noise.yml`, which runs the hosted noise runs. The owner records the fixture (V5) and the noise bands are measured
when the owner resumes the measurements ([PLAN §5]); the owner's procedure is
`docs/measurements/v5-load-recording.md`.

### 9.1 The counters

The fixture holds these 18 performance counters, in this order, and no other:

| # | Counter | What the generator does with it |
|---|---|---|
| 0 | `\Processor(_Total)\% Processor Time` | replays and validates it (processor time, percent of all logical processors) |
| 1 | `\Processor(_Total)\% Privileged Time` | — |
| 2 | `\Processor(_Total)\% User Time` | — |
| 3 | `\PhysicalDisk(_Total)\Disk Read Bytes/sec` | replays and validates it |
| 4 | `\PhysicalDisk(_Total)\Disk Write Bytes/sec` | replays and validates it |
| 5 | `\PhysicalDisk(_Total)\Disk Reads/sec` | sizes the replayed reads |
| 6 | `\PhysicalDisk(_Total)\Disk Writes/sec` | sizes the replayed writes |
| 7 | `\PhysicalDisk(_Total)\Avg. Disk Queue Length` | — |
| 8 | `\Memory\Available Bytes` | — (the replay holds 1.8 GB instead, [60 §5.1]) |
| 9 | `\Memory\Committed Bytes` | — |
| 10 | `\Memory\Pages/sec` | — |
| 11 | `\Paging File(_Total)\% Usage` | — |
| 12 | `\Process(_Total)\Private Bytes` | — |
| 13 | `\Process(_Total)\Working Set` | — |
| 14 | `\Process(_Total)\IO Read Bytes/sec` | — |
| 15 | `\Process(_Total)\IO Write Bytes/sec` | — |
| 16 | `\Process(_Total)\Thread Count` | — |
| 17 | `\Process(_Total)\Handle Count` | — |

- Every counter is system-wide: the `_Total` instance of the Processor, PhysicalDisk, Paging File and Process objects,
  and the Memory object, which has a single instance ([PLAN §3.2] WP-51). No per-process instance, no other object,
  no event trace and no name of a process, file, user or machine is read into the fixture.
- Counters 0, 3, 4, 5 and 6 are **required**: a recording does not start without them. typeperf leaves out a counter
  it cannot add when it starts (on a loaded machine the Process object's IO counters sometimes are); such a counter is
  missing in every sample of that recording.
- The columns marked "—" are recorded for the measurement's report ([60 §5.2] item 16: "a recorded CPU, disk and
  memory profile") and are not replayed.
- **WPR is not used.** [PLAN §3.2] WP-51 allows WPR only with WP-06's counters-only profile, which WP-06 did not check
  in (`docs/m0/tools.md` §10): typeperf reads every counter of the table, so no ETL file is ever written.

### 9.2 The fixture file

A sealed fixture is `/private/load/<start>.load` in the main worktree, where `<start>` is the recording's UTC start as
`YYYYMMDDTHHMMSSZ`. It is binary and little-endian:

| Offset | Width | Field | Rule |
|---|---|---|---|
| 0 | 8 | magic | `00 8C 9A 0D 0A 1A 0A 00` |
| 8 | 2 | version | 1 |
| 10 | 2 | counter count | 18 |
| 12 | 4 | interval | milliseconds, 1 to 60,000; the recorder writes 1,000 |
| 16 | — | name table | per counter of §9.1, in order: a 1-byte length L (1 to 255), then the L bytes of its name in printable ASCII, exactly as §9.1 writes it |
| h | 152 per sample | samples | per sample: its time `t_ms` (u64, milliseconds since the first sample's arrival; the first is 0 and each is greater than the one before), then 18 IEEE binary64 values in §9.1 order, each finite and non-negative, or the quiet NaN `0x7FF8000000000000` where the counter had no value |
| end − 8 | 8 | checksum | xxh3-64 (seed 0) of every byte before it |

- There is at least one sample, and no other byte. The name table is the only text the format can hold.
- Byte 0 is NUL, so the private manifest takes no shingles of the file ([PLAN §3.2] WP-03): its numbers never become
  shingles that would refuse an ordinary commit. No byte of the magic is printable.
- A file being recorded is `<start>.load.partial`: the header and the whole samples so far, without the checksum.
  Sealing cuts a trailing incomplete sample in place, appends the checksum, syncs and renames the file; it never cuts
  the file below its whole samples, so an interruption while sealing loses no sample and leaves a partial that seals
  again. Sealing a sealed body again gives the same bytes (the old checksum is shorter than a sample).
- The fixture's id is the BLAKE3-256 of the sealed file in lower-case hex (§2.3, §7.1).

### 9.3 The recorder: `cargo xtask loadrec`

```
cargo xtask loadrec start [--max-duration <seconds>] [--private-dir <dir>]
cargo xtask loadrec stop [--private-dir <dir>]
cargo xtask loadrec check <file>
```

- `start` writes into `/private/load/` of the main worktree (or `--private-dir`), and refuses while a
  `*.load.partial` exists there: one recording at a time. It runs `%SystemRoot%\System32\typeperf.exe` with the 18
  counter names and `-si 1`, by its full path (§2.2's rule for system programs).
- **What is dropped.** typeperf's standard output is read in memory; nothing typeperf prints reaches the fixture but
  the numbers. Its banner and time-zone note, the `\\<machine>` prefix of every column, the timestamp of every sample
  and its closing messages are dropped, and typeperf writes no file. A test proves that the fixture's bytes do not
  depend on any of these strings.
- **The header.** Every column after the first is one counter of §9.1, compared without the machine prefix and
  without regard to ASCII case, each at most once, and the required counters are there; otherwise the recording stops
  and leaves no file. A counter the header leaves out is missing in every sample.
- **Samples.** A sample's time is its arrival on the monotonic clock minus the first sample's, raised to the previous
  time plus 1 ms when it is not greater. A value is a decimal number `digits[.digits]`; any other field (blank, `-1`)
  is missing; missing values are counted per counter. A sample line whose field count is not its header's is skipped
  and counted. Each sample is written and flushed when it arrives, so an interrupted recorder loses no whole sample.
- **Progress.** Once a minute (every 60th sample) the recorder prints its sample count, the time recorded and the
  missing values of each required counter. From the 60th sample on, every progress line also warns about each required
  counter that has had no value in any sample yet (typeperf printing `-1` or a blank, or numbers with another decimal
  separator): `loadgen` refuses a fixture without any value of #0, #3 or #4 (§9.4), so the owner can stop and report
  it at once instead of after the campaign. The warning stops once the counter has a value.
- **Stopping.** The recorder stops on Enter in its window (when its standard input is a terminal), a stop request
  (`loadrec stop`), the maximum duration (default 12 hours) or the end of typeperf's output. It then stops typeperf,
  seals the fixture (§9.2), prints its sample count, missing values (in all and per counter, with a warning for each
  required counter without any value), skipped lines and BLAKE3, and rebuilds `/private/MANIFEST.b3`: a new file
  under `/private/` makes the manifest stale, and the pre-commit guard refuses every commit until it is rebuilt. A
  recording that stops on an error (a failed read of typeperf's output, a failed write, a failed last flush) is
  sealed with the samples that reached the file and reported as stopped early; one without typeperf's header or
  without any sample leaves no file.
- **The private manifest while recording.** `/private/MANIFEST.b3` ([PLAN §3.2] WP-03) leaves out
  `load/*.load.partial` and `load/stop.request`: the partial grows every second, so listing it would make the manifest
  stale within a second of every rebuild and the guard would refuse every commit in every worktree for the whole
  campaign. Neither file can feed a check: both are binary (the partial begins with NUL, the request is empty), so
  they give no shingles; while recording, the partial's hash changes every second; and any path under `private/` is
  refused anyway. Commits and merges therefore keep working while the recorder runs. The sealed fixture is listed like
  any other private file, which is why `start` and `stop` rebuild the manifest after sealing.
- **`stop`** creates the stop request `/private/load/stop.request` and waits up to 30 s for the recorder to seal. A
  partial fixture that does not grow for 10 s belongs to no running recorder (one sample a second is appended); `stop`
  then seals it itself and rebuilds the manifest, or removes it when it holds the recorder's header and no whole
  sample (a file with another header is left alone). That is the recovery of a recorder whose window was closed or
  that was interrupted.
- **`check`** validates a fixture by §9.2 and prints its BLAKE3, its sample count and duration, and per counter the
  values present and their minimum, mean, p95 (nearest rank, §3.4) and maximum: numbers and counter names only.

### 9.4 The generator: `moirai-probes-bin loadgen`

```
loadgen (--profile <fixture> | --synthetic <spec>) --scratch <dir> --log <file>
        [--ready-file <path>] [--stop-file <path>] [--max-duration <seconds>]
        [--max-hold <bytes>] [--read-pool <bytes>]
loadgen --help
```

**What it replays.** Processor time (§9.1 #0), disk read bytes (#3) and disk write bytes (#4) of the profile. Each
replayed I/O is the byte rate divided by the operation rate (#5, #6), rounded down to 4 KiB and held to
[4 KiB, 1 MiB]; it is 64 KiB where the operation rate is missing or below one a second. A missing value repeats the
value before it (leading missing values take the first value); a profile without any value of #0, #3 or #4 is
refused. The profile is a step function: at replay time τ the targets are those of the last sample at or before τ
modulo the profile's duration (its last time plus one interval), so the profile repeats until the generator stops.
Available physical memory is held at 1.8 GB ([60 §5.1]) instead of following #8.

**The load.**
- One CPU worker per logical processor, busy for its duty of every 100 ms and asleep for the rest.
- A disk writer: sequential writes of the replayed size over a 1 GiB file in the scratch directory, issuing its share
  every 100 ms and flushing each slice that wrote with a data flush, so its bytes reach the disk.
- A disk reader: sequential reads over a read pool in the scratch directory (default 4 GiB, at least 64 MiB: twice
  what the hold leaves available, so reads mostly miss the file cache), wrapping at its end. The pool is made before
  the replay starts when the profile reads at all, and is kept for the next run.
- A memory holder: every 250 ms it reads `Meter::available_physical` ([OS/mem §5]), allocates whole 64 MiB chunks
  while more than 1.9 GB is available (at most 1 GiB a step, never more than `--max-hold` in all, default 12 GB) and
  frees whole chunks while less than 1.7 GB is; every 4 s it writes one byte of every page it holds, so the pages stay
  resident. Three failed readings in a row stop the generator.
- Each worker carries its share of a second at most, so a slow stretch is not made up in a burst.

**The loop.** typeperf (by full path, `-si 1`) samples #0, #3, #4 and #8 once a second; the generator closes the loop
on the machine's totals, as the memory hold does. A sample line whose field count is not its header's is skipped and
counted, as the recorder skips one (§9.3); the sample is missing, and the coverage rule judges the gap (§9.5).
typeperf delivering no sample for 15 s (2 minutes for the first, while it enumerates its counters) ends the replay
with exit 3: every run riding on it would fail the coverage rule anyway. Each sample closes an interval:
- the generator's own share of the interval is measured: its processor time (`Meter::cpu_times`, [OS/mem §7], as a
  percentage of all logical processors) and the bytes its disk workers moved;
- the rest of the machine's load is the observed total minus the generator's share (never below 0), smoothed with
  weight 0.5 for the newest value; a missing observation keeps the previous estimate;
- the next interval asks of the generator the target minus the rest (never below 0); the CPU workers' duty is that
  share plus an integral correction of their own shortfall (gain 0.5, at most ±0.25, frozen while the duty is 0 or 1),
  since their sleeps overshoot by the OS timer's granularity.
So the machine's totals follow the profile whatever else runs beside the generator: a measured process takes its share
of the profile's load rather than adding to it, as it takes its share of the held memory.

**Settling and stopping.**
- The replay has **settled** when at least 60 s have passed since its first sample and the last 10 samples read
  available memory in [1.6 GB, 2.0 GB] (§2.3). The generator then writes the ready file, `{"settled":<ms>}`,
  atomically: into `<ready file>.tmp`, synced, then renamed into place, so a driver never reads it empty or partial.
- It stops when the stop file appears, when the maximum duration has passed since its first sample, or when typeperf
  ends. It then stops its workers, frees the held memory, removes the write file, and prints its verdict over its
  settled replay (§9.5), from settling (or, if it never settled, from its first sample) to its last sample, judged on
  the monotonic clock alone.
- It refuses to start when the stop file or the ready file already exists, and never overwrites a log.

**The log.** JSON lines, each written and flushed when it happens:
- a header: `{"schema":"moirai-probes/replay/1","source":…,"interval_ms":…,"sample_ms":1000,"duration_ms":…,
  "workers":…,"ram_target":…,"started":…}`, where `source` is `{"kind":"fixture","id":"<64 hex>"}` or
  `{"kind":"synthetic","spec":"<canonical spec>"}`, `interval_ms` is the profile's interval and `sample_ms` the
  generator's sampling interval (`-si 1`);
- one line per sample: `{"t":…,"mono":…,"tau":…,"target":[cpu,read,write],"observed":[cpu,read,write,available],
  "own":[cpu,read,write],"held":…,"settled":…}`, with `t` the wall-clock arrival in milliseconds since 1970 (UTC),
  `mono` the arrival on the generator's monotonic clock in milliseconds since its sampler started (never decreasing),
  `tau` the replay time the interval's targets were taken at, a missing observation `null`, and numbers rounded to
  whole units from 1,000 up and to three decimals below;
- an end line `{"end":…,"settled":…,"samples":…,"skipped":…,"ending":"stop-request"|"max-duration"|"sampler-ended"}`,
  with `skipped` the sample lines skipped for their width;
- the verdict (§9.5).

The log holds numbers, the fixture's id and a synthetic spec only. A loaded run's log is raw data, kept with the run in
`/private/measurements/` (§7.2).

| Exit code | Meaning |
|---|---|
| 0 | the replay reproduced the profile (and `--help`) |
| 1 | the replay ran and did not reproduce the profile |
| 2 | usage error: nothing ran |
| 3 | the replay could not run, or a worker failed |

### 9.5 The replay verdict

The verdict over a run's window [a, b] (wall-clock milliseconds) is read from the log:
- **Clocks.** A run's window is wall-clock time, the only clock a driver and the generator share. The samples are
  ordered, windowed and spaced by their monotonic time `mono`, and the window is placed on it by the wall-clock times
  `t` of its **stretch**: a run of consecutive samples without a wall-clock step, where a step is two consecutive
  samples whose `t` and `mono` differences disagree by more than 500 ms (half the sampling interval). The stretch that
  holds b — from its first sample to 3 sampling intervals after its last — gives the offset `t − mono` of the sample
  at or before b (when none holds b: the last stretch that begins at or before b, else the first). When two stretches
  hold b (the wall clock stepped back over it), or a step lies in the span rule (a) checks, the verdict fails with that
  reason. A step is never a read error: runs before and after it keep their own verdicts.
- **(a) Coverage.** The replay settled at or before a; it has a sample at or after b; and from the last sample at or
  before the start of the validated window to the first sample after b (or the last sample, if none is after b), no
  two consecutive samples are more than 3 sampling intervals (`sample_ms`, not the profile's interval) apart.
- **The validated window** is (max(settled, min(a, b − 60 s)), b] and holds at least 30 samples. A run shorter than a
  minute is judged on the minute of replay before its end; that is why a driver starts a loaded run at least 60 s after
  the ready file appears (§9.7).
- **(b) The profile.** For processor time, read bytes and write bytes, the window's samples form consecutive blocks of
  10 (a last block of 5 or more counts). A block is within tolerance when at least half of its samples carry an
  observation and the mean observation differs from the mean target by at most the larger of 15 % of the mean target
  and the absolute tolerance: 5 percentage points of processor time, 1 MB/s of disk bytes. At least 90 % of the blocks
  are within tolerance.
- **Memory.** At least 95 % of the window's samples read available memory in [1.6 GB, 2.0 GB].

The verdict passes when every rule holds. Its record, schema `moirai-probes/replay-verdict/1`, holds the run window
(`from`, `to`), `window_from`, `settled`, `samples`, per quantity (`cpu`, `read`, `write`) the blocks, the blocks within
tolerance, the mean target, the mean observation and whether it passes, `memory_in_band`, `pass` and `reasons`. A
loaded run's `load_replay_valid` (§2.3, §7.1) is this verdict over the run's own window.

### 9.6 Synthetic profiles

`--synthetic` takes `key=value` pairs separated by commas, each key at most once; the empty spec is the default:

| Key | Meaning | Default | Range |
|---|---|---|---|
| `seconds` | length; one sample a second | 600 | at least 10 |
| `cpu` | mean processor time, percent | 50 | 0 to 100 |
| `read` | mean disk read bytes per second, a byte quantity of §8.2 | `20MB` | — |
| `write` | mean disk write bytes per second | `20MB` | — |
| `step` | seconds each level holds | 30 | 1 to `seconds` |
| `seed` | the generator's seed | 1 | any u64 |

At the start of every step, processor time, read bytes and write bytes, in this order, each take their mean times a
factor of {0.5, 0.75, 1, 1.25, 1.5}, drawn by xorshift64*: the state, seeded with `seed` (with `0x9E3779B97F4A7C15`
for a seed of 0), is advanced by `x ^= x >> 12; x ^= x << 25; x ^= x >> 27`, and the factor's index is the high 32 bits
of `x × 0x2545F4914F6CDD1D` (mod 2⁶⁴), modulo 5. Processor time is held to 100 %; every I/O is 64 KiB. The canonical spec writes
every key, in the order of the table, with byte quantities in bytes. A run under a synthetic profile is a synthetic run
(§2.3), described as `moirai-probes-bin loadgen, synthetic profile <canonical spec>`.

### 9.7 Drivers, noise runs and `noise.yml`

- **A loaded run.** The driver starts `loadgen --profile <fixture>`, waits for the ready file and 60 s more, and makes
  the run. It then waits until the log's last line is a sample after the run's end, the end line or the verdict, at
  most 10 s (the sample after b arrives up to a sampling interval after it; one more than 3 intervals late fails rule
  (a) anyway), reads the log for the verdict over the run's window, records it with the run (`load_replay_valid`), and
  creates the stop file. `moirai-probes`' `validate::verdict_when_covered` is that wait and read. The log's header
  gives the run's condition: loaded with the fixture's id, or synthetic with the description of §9.6.
- **The driver contract** (for the measurement drivers of WP-52 to WP-57, which `noise.yml` runs): a driver of
  `moirai-probes-bin` accepts `--host laptop|hosted`, `--condition idle|loaded|synthetic`, `--load-log <file>` (the
  generator's log, required for a loaded or synthetic run) and `--out <dir>`, and writes every run record (§7.1) it
  makes as one JSON file in `<dir>`. On the laptop, `<dir>` is `/private/measurements/<n>/` (§7.2).
- **`moirai-probes-bin noise <run record>...`** prints one noise record (§6.1) per gated statistic of every arm of the
  records, combining the runs of one measurement, quantity, arm, statistic, condition and host into the widest band;
  it refuses a laptop run that is not exit-grade and a hosted run that is invalid or loaded (§6.1). Exit 0 printed, 1 a
  record refused, 2 usage error.
- **`noise.yml`** is run by hand (`workflow_dispatch`) on a hosted Windows runner with a condition (idle or synthetic),
  the drivers to run and, for a synthetic run, the synthetic spec (default: the default profile). It never uses the
  fixture, which never leaves the laptop ([AR §11] #37). It builds `moirai-probes-bin`, starts `loadgen` for a
  synthetic run without `--max-duration` (the stop file alone ends the replay, so it cannot stop under long drivers)
  and waits for it to settle and 60 s more, runs each driver by the contract above, checks that the generator is still
  replaying, stops it, checks that its log ends with `"ending":"stop-request"`, and prints the drivers' noise records
  (`noise`), the generator's verdict and, when the synthetic load ended early, a warning that the drivers' runs were
  not all under it, to the job log and summary. A hosted band is reported and never decides (§6); the workflow gates
  nothing.

## Coverage

No `COVERAGE.md` row cites this file ([F01 §2.7]): the protocol freezes no byte and no rule of [60 §2.5], [40 §2.11],
[50 §8.1], [80 §3] or [90 §10.1]. It freezes [60 §5.1], under which the measurements of [60 §5.2] are taken and the
time, memory and Windows-behaviour gates of [60 §3.13] and [60 §5.3]–[60 §5.4] are judged.

| Item | Part covered here | Section |
|---|---|---|
| [60 §5.1] Load | the idle, loaded and synthetic conditions and their checks; the fixture, its replay and its validation | §2.3, §9 |
| [60 §5.1] Sample size | tiers, the plan, the tier check, statistics, the median over repetitions | §3 |
| [60 §5.1] Floors | interleaved blocks | §4.3, §5 |
| [60 §5.1] Idle CPU | the observation, its gate and its record | §4.7, §7.1 |
| [60 §5.1] RSS | process readings | §4.6 |
| [60 §5.1] Counts | `Vfs` counter deltas | §4.8 |
| [60 §5.1] Spawn | hyperfine blocks, announced as bulk blocks | §4.3, §4.9 |
| [60 §5.1] Noise band | definition, baseline, regression rule, noise record, comparison | §6, §6.1 |
| [PLAN §3.2] item 5, shared acceptance | idle and loaded, the Windows build and Defender versions, raw data and aggregates | §2.2, §2.3, §7 |
| [PLAN §3.2] WP-05, the guard's refusals | `ram-low`, `disk-low`, fail-closed readings | §8 |
| [60 §5.2] item 16, [PLAN §3.2] WP-51 | the counters, the fixture file, `xtask loadrec`, `loadgen`, the replay verdict, synthetic profiles, the noise bands of noise runs, `noise.yml` | §9 |

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| — | none: the protocol fixes procedures, not values; the noise bands and baselines it defines are measurement data (measurement 16, the M0 runs), not specification values | — | — | — |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [60 §5.1] and [74 A04] write the tiers as "below 1 ms", "1–50 ms", "50 ms–1 s", "above 1 s" without saying where a boundary value falls | half-open intervals; exactly 1 s is `t4`, which matches "5 repetitions below 1 s and 3 above" | R-REV-P |
| 2 | [60 §5.1] tiers by "duration" without saying which duration, and an operation and its floor can fall in different tiers | the pilot median per arm (§3.2), an optional declared minimum tier, one plan covering every arm, and the tier check of §3.3 that repeats an under-sampled run | R-REV-P |
| 3 | No percentile definition is given; at n = 20 the choice changes the result | nearest-rank, exact in integers, and always a sample value | R-REV-P |
| 4 | "Alternating blocks" gives no block length | B = min(100, ⌊n/20⌋): at least 20 alternations per repetition, at most 100 consecutive samples of one arm | R-REV-P |
| 5 | "Free RAM held at ≈ 1.8 GB" gives no tolerance | [1.6 GB, 2.0 GB] at every round boundary; the lower edge stays above the guard's 1.5 GB floor | R-REV-P, WP-51 |
| 6 | The design writes budgets in MB and MiB side by side and never defines MB | MB = 10⁶ bytes, MiB = 2²⁰ bytes, each budget read as written; for "≤" budgets the decimal reading is the stricter one | R-REV-P |
| 7 | "A nightly regression beyond the band blocks" names no baseline | the reference run (M0, then each accepted exit), never the previous night, so a drift of small steps is caught | R-REV-P |
| 8 | "Below 25 GB free disk counted after the two lane directories and the fuzz and mutants directories" ([PLAN §3.2] WP-05) gives no formula, and "capped directory" ([PLAN §2.1]) gives no cap values | the headroom formula of §8.1, reserving each directory's growth up to its cap; the caps are WP-05's configuration, passed to the guard; a cap equal to the current size reserves nothing | WP-05, R-REV-P |
| 9 | [PLAN §3.2] item 5 asks for "the Windows build and Defender versions" without naming sources | `ver` (locale-independent token) and `Get-MpComputerStatus` (four properties), §2.2 | R-REV-P |
| 10 | A Defender signature update during a run starts a scan burst that no protocol row mentions | a change between the start and end snapshots makes the run not exit-grade (§7.3) | R-REV-P |
| 11 | Sub-microsecond operations (the `get` budget of 5 µs is 50 ticks of `QueryPerformanceCounter`) approach the timer's resolution | timed batches below 20 × resolution, recorded and stated in the aggregate (§4.5) | R-REV-P |
| 12 | The idle condition ("no user process beyond the OS baseline") cannot be verified through `Meter` | the driver attests it; the framework checks the available-memory floor at every round boundary | R-REV-P |
| 13 | [F01 §2.2] has no citation form for this file, and `docs/spec/README.md` lists it as "planned" with no cite | `[MP §x]` is proposed for [F01 §2.2]; the index row should read `[MP]`, "draft, pass 1 pending" (both files are R-SPEC's) | R-SPEC-F |
| 14 | The idle-CPU row is one 10-minute observation, which the duration tiers (20 samples × 3 above 1 s) do not fit; read as a `t4` quantity it would need 20 × 3 windows, over 10 hours | outside the tiered runner: an idle observation of three windows, each after a request and 15 s, with its own record `moirai-probes/idle/1`; the gate holds when both maxima over the windows are zero (§4.7, §7.1) | R-REV-P |
| 15 | [60 §5.1] measures spawn with hyperfine, which takes a whole block per invocation, while the runner samples one value at a time; the pilot of such an arm is not stated | the runner announces every block to its arm, and a bulk arm takes the block then; a spawn arm's pilot is one invocation with `--runs 5` (§4.2, §4.3, §4.9) | R-REV-P |
| 16 | "The probe process's own `private_peak` and heap high-water mark" does not say over what span | the heap mark is reset before the pilot, so it is the run's; `private_peak` cannot be reset, so a driver that records it makes one run per process (§4.6) | R-REV-P |
| 17 | The idle condition checks "the guard's floor", which `--ram-floor` can override | the protocol's 1.5 GB, the guard's default; an override moves only the guard, so read-back can recompute a record's reasons (§2.3) | R-REV-P |
| 18 | The noise band names no record and no rule for which runs and keys a comparison may use | the record `moirai-probes/noise/1`; laptop bands over exit-grade runs only; a comparison checks host, grade, measurement, quantity, condition, arm unit and gated statistic (§6.1) | R-REV-P |
| 19 | "A record that breaks the protocol is refused when it is read back" does not say what is checked | the list of §7.3, including tier plans, rising escalations, the batch rule, the reading count and the reasons the framework adds | R-REV-P |
| 20 | [PLAN §3.2] WP-51 names the counter objects ("Processor, PhysicalDisk, Memory, Paging File, Process(_Total)") but not the counters; Memory has no `_Total` instance | the 18 counters of §9.1; Memory's single instance is system-wide; five are required, and a counter typeperf cannot add is missing in every sample (observed: the Process IO counters on a loaded machine) | R-REV-P, WP-51 |
| 21 | No format is named for the fixture, and "the stored fixture contains no string except counter names" does not say how it is proved | a binary format whose only text is the name table (§9.2); typeperf's output is read in memory and only numbers are written (§9.3); a test proves the fixture's bytes independent of every other string typeperf prints | R-REV-P |
| 22 | WPR "only with WP-06's counters-only profile": WP-06 checked in none | WPR is not used: typeperf reads every counter, so no ETL is written (§9.1) | R-REV-P |
| 23 | [60 §5.2] item 16 asks that "the replay reproduces the recorded profile" without a rule | the verdict of §9.5: coverage, blocks of 10 samples within the larger of 15 % and 5 points or 1 MB/s in 90 % of blocks, 95 % of memory readings in the band | R-REV-P; measurement 16 confirms the tolerances |
| 24 | Whether the generator adds its load to what runs beside it (the measured process) or makes up the difference | it closes the loop on the machine's totals, as the memory hold of [60 §5.1] does (§9.4) | R-REV-P |
| 25 | A loaded run shorter than the validation's minimum has too few samples to judge | a short run is judged on the minute before its end; a driver starts a loaded run 60 s after the generator settles (§9.5, §9.7) | R-REV-P |
| 26 | "Synthetic load" on the hosted runners is not defined | a synthetic profile (§9.6) replayed by the same generator; never the fixture | R-REV-P |
| 27 | `noise.yml` runs the measurement drivers of WP-52 to WP-57, which have no common command line yet | the driver contract of §9.7 | WP-52 to WP-57, R-REV-P |
