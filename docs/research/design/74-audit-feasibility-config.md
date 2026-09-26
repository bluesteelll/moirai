# 74 — Audit: feasibility on the owner's machine, scope, and the configuration boundary

*moirai research/design, 2026-09-26. Status: audit of the design of record; nothing is implemented and no other document was edited. Axis: **feasibility, scope and configuration**. (a) Can the gates, rigs and calendar of the design run on the owner's actual setup? (b) Does every component serve R1–R5, the synchronous-reference requirement, the CLI/MCP/skills surface, or one of the four priorities (speed, minimal RAM, correctness, minimal agent tokens), and at a justified cost? (c) Which open decisions are real owner decisions, and which are `config` keys under the owner's rule of 2026-09-26?*

**Inputs.** Read in full: [AR] `docs/ARCHITECTURE-RESEARCH.md` (§0–§12, Review log). Read in the parts this axis needs: [40] (§0, §2.11, §7, §8.1, §9, Review log), [50] (§0, §5.5–§5.8, §9, §10, §12), [60] (§0–§8, §10), [61] (M-6, M-7). Evidence: [02 §9, §12.6], [05 §2, §6.4], [07 §4], [08 §2], [09 §0, §4], and the four cross-platform reports [17]–[20] (`docs/research/17-…` to `20-…`; not to be confused with the critique [20] = `design/20-critique-perf-ram-windows.md`, which this file writes as [20c]). Web checks on 2026-09-26 are listed in §9.

**Tags.** **[M]** measured on the owner's machine, quoted from the cited report; **[D]** documented (vendor documentation or specification); **[C]** third-party claim; **[I]** inference of this audit; **est.** arithmetic with the inputs shown. Finding ids `A01…A24` are defects or gaps; `C01…C50` are the decision classifications that part (c) asks for.

---

## 0. Verdict

**Feasible, with changes before M0.** Nothing in the architecture is infeasible on a Windows 11 Home laptop, but the plan around it assumes capacity that the laptop does not have, and several operational choices are still filed as owner decisions.

- **One blocker (A01).** Four research reports dated 2026-09-26 ([17]–[20]) state an owner requirement that Linux and macOS are first-class. The design of record excludes both ([AR §9], [60 §1.3]) and no audited document cites the reports. If the requirement stands, M0 would freeze the wrong format (holder identity, `LOCK` layout, fault model, group commit). The owner must confirm or reject it before M0 entry.
- **Ten majors.** The majors are about capacity, not architecture:
  - **Infrastructure (A02, A03, A06).** M0's exit criteria depend on a purchase the owner has not made, with no quantified alternative. The two-lane calendar assumes build capacity the laptop lacks while campaigns run (≈ 1.8 GB free [M]). The OS-crash rig's Windows 11 guest has no licence plan, and it cannot run beside the agents.
  - **Machine time (A04, A05).** The measurement protocol's sample sizes make long operations take days: a full import at 1e6 measured as specified is 69–194 h. The nightly plan is over-subscribed, and mutation testing is not budgeted.
  - **Agent tokens (A07, A08).** The CLI `pack` default (40,000 characters) exceeds the Bash tool's ≈ 30,000-character inline cap. The token priority has no end-to-end budget and no gate.
  - **RAM and latency (A09).** A rollup inside the MCP server is neither RAM- nor latency-budgeted. The aggregate RAM of up to 16 servers is not gated.
  - **Configuration (A10, A11).** The configuration system itself is unspecified. Real owner decisions about money and data leaving the machine are missing from the decision list.
- **Scope is mostly justified.** Of about forty components and features reviewed (§3.1), five are over-engineered for the owner's setup, and each has a simplification that keeps the final-specification principle:
  - the USN-journal reader, which gives no benefit on D: (A13);
  - the second LQ-Bench model, although the owner runs every agent on Opus (A14);
  - exact BM25 parity across search tiers (A15);
  - `links import` built before its decision (A16);
  - three built-but-off features (A17).
- **Configuration.** Of the 50 classified items:
  - 24 are real owner decisions;
  - 22 are `config` keys or policy-data rows, and 7 of these [60 §3.14] still lists as owner decisions due before M0;
  - 2 are split between a real part and a config part;
  - 1 is a design rule that must not become a key;
  - 1 is decided by measurement.

  §5.3 gives a registry of 67 keys with type, default, scope and reload behaviour.

---

## 1. Findings at a glance

| Id | Sev. | Where | Finding (one line) |
|---|---|---|---|
| A01 | blocker | [AR] §1 row 14, §9; [60 §1.3]; [17]–[20] headers | A cross-platform owner requirement asserted by [17]–[20] is not integrated, and it would change format v1 |
| A02 | major | [60 §3.1] item 7 and exit; §3.14; §3.15 | M0 cannot exit without a purchase decision; the no-purchase path has no calendar number (it is +2–4 weeks and ≈ 125–175 agent-free nights) |
| A03 | major | [60 §7.1]; [AR §11] #2 | The two-lane rate assumes build capacity the laptop does not have while campaigns run (≈ 1.8 GB free); otherwise the one-lane calendar applies (P50 60 instead of 39 weeks) |
| A04 | major | [60 §5.1] sample size; §5.4; RG6 | n ≥ 1,000 × 5 repetitions × idle/loaded makes multi-second rows take 8–194 h; M5's exit alone needs ≈ 8–28 h against 4–16 h of windows |
| A05 | major | [60 §3.15], §3.13 GT11/GT16 | The nightly plan sums to 10–21.5 h against 8–10 h; GT11 needs a quiet host while fuzzers run "on idle cores"; mutation testing (≈ 75–930 CPU-h per full run, est.) is unbudgeted |
| A06 | major | [60 §3.1] item 7, §5.2 item 17; [AR §8.2] | OS-crash rig: no guest licence plan (90-day evaluation), a 4 GB guest against 1.8 GB free, calibration placed on the wrong machine; Windows Sandbox is unusable |
| A07 | major | [AR §7.1], §7.4; [07 §4] | CLI `pack` 40,000 characters > Bash inline ≈ 30,000; failures show ≈ 10,000; Cyrillic makes MCP's 32,000 characters ≈ 11–16k tokens |
| A08 | major | [AR §7.4], §7.5, §8.1 | The token priority has no end-to-end budget or gate; packs fill to the budget; critical rules are injected twice per dispatch |
| A09 | major | [AR §4.9], §6.1, §8.1; [60 §3.11] | Rollup inside the MCP server: RSS (est. 8–40 MB at 0.3–1 M nodes) and latency (up to 3 s blocking a `current_thread` server) are unbudgeted; 16 servers × 18 MB is ungated |
| A10 | major | [AR §4.1] `config`, §2.14; [60 §2.5] | Config: only a store scope; no types, validation, verb, reload rule or sweep plan; `discovery.git-hint` is circular; [60] freezes the key set at M0 |
| A11 | major | [AR §11], [60 §3.14] | Real decisions not listed: repository hosting and CI, residency of owner-derived corpora, the LQ-Bench model budget, code signing |
| A12 | minor | [60 §6] RG3, §3.12 | "Any engine change restarts" 14 nights + 1,000 OS-crash cycles + 72 h soak, even for a resolver-only fix |
| A13 | minor | [AR §5e.3], §5e.7; [40] FL-4 E2 | The USN reader (E2) gives zero benefit: D: holds every repository and has no journal; C:'s journal keeps 1.1–1.7 h |
| A14 | minor | [60 §3.1] item 10, §3.15 | LQ-Bench runs a second "cheapest model" although all agents run Opus; ≈ 130–260 M tokens at M0 as specified |
| A15 | minor | [50 §5.5]; [AR §2.11], §4.4 F12 | Exact BM25 parity across tiers costs a frozen column, per-view statistic correction and a parity test; decide by an M0 ablation |
| A16 | minor | [60 §3.10]; [AR §11] #23 | `links import` (1–2 units) is built in M9 while its decision is due at M11 |
| A17 | minor | [AR §7.5], §5b.8; [60 §1.3] | Built but off with no measured revisit trigger: `fs-nudge`, the `PostToolBatch` delta, the `refs/moirai/*` destination |
| A18 | minor | [AR §8.2] item 17; [60 GT15] | Rig calibration proves only page-cache loss (W2); record (c): issued-but-unflushed loss (W3) is not observable |
| A19 | minor | [60 §3.1] item 7, §3.15 | Test-host specification: consumer NVMe without PLP, 32 GB RAM, VBS state matched, "same Windows build" unenforceable on Home |
| A20 | minor | [60 GT4], RG7 | Disk-full variants need an elevated VHDX on Windows Home |
| A21 | minor | [60 §4.2], §4.6 | The reference model memoises a `BTreeMap` state per commit with `std` only; est. GBs per GT3 case |
| A22 | minor | [AR §2.15], §5b.8; CM8 | The "daily" lane checkpoint export has no trigger (no timers by design) |
| A23 | minor | [AR §4.4] `SegHdr`; [60 §2.5] | No "derived, ignorable" section flag, so every later derived index is format version 2; this forces D9 before M0 |
| A24 | minor | [60 §3.15]; [02 §9] | Test-infrastructure disk (≈ 20–55 GB est.) is unbudgeted on a 512 GB drive whose owner rule is "stop under 15 GB free" |
| C01–C50 | minor | [AR §11], [40 §9.2], [50 §9.2], [60 §3.14] | Classification of every open decision (§5.2) |

---

## 2. Feasibility on the owner's setup

### 2.1 What the owner's machine can run, and when

Machine [M, 05 §2, 08 §2, 09 §0]:
- Ryzen 9 5900HS, 8C/16T;
- 16 GB RAM, with 1,823 MB free while 16 `claude` processes (16 sessions) hold 3.5 GB private;
- one 512 GB consumer NVMe with C: and D: on it; the flush floor is ≈ 1.7 ms;
- Windows 11 Home Single Language 10.0.26200, Defender real-time protection on;
- `HypervisorPresent = False`, VBS off, no WSL, no VirtualBox, VMware or QEMU installed [M, 20 §1];
- D: holds every repository and all 44 worktrees, and has no USN journal.

The owner's rules [02 §9]:
- no agent runs during a timed pass;
- stop under 15 GB of free disk;
- the development machine has experienced OS crashes and disk-full events.

| Workload | RAM | Runs beside the 16-agent load? | Notes |
|---|---|---|---|
| GT1 crash enumeration, GT2/GT3 simulator cases | 50 MB–1 GB per case (est.; A21) | **yes**, at idle priority, capped at 1 GB total | CPU only; must pause during the owner's benchmark windows |
| GT5 fuzzers | libFuzzer's default `-rss_limit_mb` is 2,048; ASan adds shadow memory | only without a sanitizer and with `-rss_limit_mb=256`, 1–2 targets | cargo-fuzz works on Windows through the MSVC AddressSanitizer [D] |
| GT4 kill loop (16 processes at 1e4) | ≈ 100 MB | RAM yes; disk no: flush storms slow the agents' builds | run in windows |
| GT7 git-CLI differential, GT17 NTFS matrix | small | yes | spawns cost 34–74 ms each [M] |
| GT11 budgets (idle and replayed load) | — | **no**: the protocol needs a quiet or replayed machine | windows only |
| GT15 OS-crash rig | Windows 11 guest ≥ 4 GB; Server 2025 Core guest ≈ 2 GB [D] | **no** (1.8 GB free) | windows only; see §2.2 |
| GT16 mutation testing | 1–3 GB per `rustc` job (est.) | **no** | windows only (A05) |
| GT14 soak (72 h) | small, but continuous disk and CPU | **no** | needs a 3-day agent freeze on the laptop |
| moirai build lanes (2 Opus lanes compiling a 66–99k-line workspace) | 1–3 GB per building lane (est.) | **contended** | A03 |

### 2.2 The OS-crash rig: options on a Home machine

| Option | Available on Windows 11 Home? | RAM / disk | Licence | Fidelity | Verdict |
|---|---|---|---|---|---|
| **VirtualBox 7**, guest disk with host I/O cache off and `IgnoreFlush 0` [D, 20 §7.4] | yes. No hypervisor is running, so VirtualBox uses AMD-V directly [M, 20 §1]. Keep WSL2 and Memory Integrity off on the rig machine, or VirtualBox drops to the slow Hyper-V backend [C] | Windows 11 guest ≥ 4 GB RAM, ≈ 25–30 GB disk (est.); Server Core guest ≈ 2 GB, ≈ 10–12 GB | see the licence rows | W1, W2 and kernel crash (NotMyFault); **not W3** (A18) | primary, but only in agent-free windows on the laptop, or on a test host |
| **VMware Workstation Pro** | yes; free for all uses since 2024-11-11 [D, 20 §7.4]; `vmrun stop … hard` | as VirtualBox | as below | needs its own calibration | cross-check hypervisor, as [60] risk 8 plans |
| **Hyper-V** (`Stop-VM -TurnOff`) | **no**: Pro, Enterprise and Education only | — | — | — | only if the test host runs Pro |
| **Windows Sandbox** | **no**: not supported on Home [D]; even on Pro it is disposable and "everything is discarded when the user closes" it [D] | — | — | cannot show the post-crash state, so it cannot run GT15 | **unusable** |
| **Second physical machine with a smart-plug power cut** | yes (hardware) | — | its own OEM licence | the only rig that exposes a drive lying about FLUSH | optional (L5 in [20 §7.6]); the design assumes FLUSH is honoured |
| **QEMU/KVM on a hosted Linux runner** booting a Windows Server Core guest [I] | not local: `/dev/kvm` is usable on GitHub's Linux runners [D, 20 §5.1] | runner 2 vCPU / 8 GB / 14 GB SSD when private: tight for a ≈ 10 GB guest image | evaluation terms to check | needs its own calibration; hypervisor-independent otherwise | a no-purchase GT15 source that never touches the laptop; ≈ 100 cycles per 6 h job, ≈ $2 per job private, $0 public (est. from [20 §5.2]); **probe it at M0** |

**Guest licensing** (A06, A11):
- The owner's Home licence is OEM and cannot be moved into a VM [I].
- **Windows 11 Enterprise evaluation** lasts 90 days, with at most one more by `slmgr /rearm` [D/C]. After expiry it shuts down every hour.
- **Windows Server 2025 evaluation** lasts 180 days plus one rearm, 360 days in total [C]. Server 2025 is build 26100, "based on Windows 11, version 24H2" [D], the same code base as the owner's 26200 (25H2) [I].
- Server Core needs ≈ 2 GB of RAM in a VM [C].

GT15 is a **correctness** gate: acknowledged commits must be present after the crash. It does not measure performance. A Server 2025 Core guest therefore exercises the same NTFS and flush stack for half the RAM, at no licence cost, for the whole build. This rests on two conditions:
- measurement 17 calibrates that exact guest (A06);
- performance budgets stay on the host OS.

### 2.3 CI without a repository

The moirai workspace is not a git repository [M, this audit; 61 M-7]. [60] M0 item 7 assumes "hosted Windows Server runners for PR-level short seeds", which presupposes a GitHub-hosted repository. Nobody has decided that.

| Option | Cost (est., [20 §5.2]) | What leaves the machine | Defender-on budgets | Notes |
|---|---|---|---|---|
| Private GitHub repository + hosted runners | Free plan: 2,000 included minutes; Windows drains 2×, macOS 10×. Beyond that, Windows costs $0.010/min and Linux $0.006/min. At 150–450 PRs/month × (10 Linux + 25 Windows) min: ≈ $30–140/month | the moirai source and anything committed with it | no: hosted Windows images disable Defender [S, 20 §5.1] | secondary signal only, as [60] says |
| Public GitHub repository | $0 | everything in the repository, publicly | no | owner-derived fixtures could never be committed |
| Self-hosted runner on the test host | $0 | nothing | yes | also carries daytime PR checks, so the host is busy day and night |
| Local-only repository, no runner (laptop-only profile) | $0 | nothing | — | PR checks become the agents' own `cargo test` with short seeds, run in their worktrees (RAM contention, A03) |

**Data residency** is a real decision (A11), because several M0 assets are derived from the owner's work:
- R4's replay corpora are read-only walks of the owner's repositories [60 §3.1] item 12;
- LQ-Bench has a stratum of ~30 questions mined from recorded sessions [AR §7.7.5];
- GT10 contains the register incidents and the recorded HDRs [60 §3.13];
- the 16-agent load fixture is recorded on this machine (a resource profile only).

A safe default: owner-derived corpora and fixtures live outside the repository on the owner's machine (or on the test host), and hosted CI runs synthetic data only.

### 2.4 The test host: purchase or not

[60 §3.14] recommends buying a mini-PC and says that otherwise "the calendar lengthens", without a number. The arithmetic is below (est.; the inputs are [60 §3.15] and §2.1).

**Capacity.**
- A test host gives ≈ 10 h × 7 nights = 70 h/week of gate time, plus daytime PR CI.
- The laptop gives only agent-free windows: N nights × 8 h. The 16-agent load leaves less free RAM than any VM guest needs.

**(a) GT15 at M1 exit.** 1,000 cycles × 3–5 min = 50–83 h:

| Where | Weeks |
|---|---|
| host, one Windows 11 guest | 0.7–1.2 |
| host, two Server Core guests (≈ 2–3 min per cycle, est.) | ≈ 0.3–0.6 |
| laptop, 5 windows per week | 1.25–2.1 |
| laptop, 2 windows per week | 3.1–5.2 |

**(b) Exits M1–M10.** Each exit needs:
- re-certification (GT1/GT3/GT4 with the real sections);
- 24 h of fuzzing per new parser target;
- sampled mutation testing (A05).

That is 1–2 host nights, or 2–4 laptop windows, per exit: +0.2–0.5 week per exit at 5 windows per week. About half of the exits lie on the two-lane critical path, so the total is **+1–2.5 weeks**.

**(c) M11 (RG3).** RG3 needs 14 consecutive clean nightly runs, the last 1,000 GT15 cycles and the 72 h soak, all on the release-candidate commit.
- Host: ≈ 2.5 weeks [60 §3.12].
- Laptop: 14 consecutive agent-free nights plus a 3-day agent freeze: ≈ 2.5–3.5 weeks, repeated on every RC restart (A12).

**(d) Fuzzing.** ≈ 3,000 CPU-hours [60 §3.15]:
- host alone: ≈ 8 days;
- laptop windows (8 threads × 8 h × 5): ≈ 320 CPU-h/week, so ≈ 9.4 window-weeks competing with (a)–(c).

**Result: no purchase costs ≈ +2–4 weeks** on the two-lane dates: P50 39 → ≈ 41–43, P90 47.5 → ≈ 50–52. It also needs **≈ 5 agent-free nights a week from M1 exit to release** (≈ 25–35 weeks, so ≈ 125–175 nights) and a **3-day agent freeze per RC iteration**. If the laptop also cannot carry lane B's builds (A03), the one-lane calendar applies instead: P50 ≈ 60, P90 ≈ 73 weeks (+21 and +25.5).

**Purchase.** A mini-PC with 8C/16T, **32 GB**, 1 TB **consumer** NVMe, Windows 11 (A19) costs ≈ $400–700 once (est., [I]; prices not checked). It carries the nightly gates, GT15 with two Server Core guests, daytime self-hosted CI and, if the owner wants, lane B's builds.

### 2.5 Machine time

- **Nightly budget [60 §3.15]: 8–10 h.**
  - GT15 at 100–150 cycles × 3–5 min takes 5–12.5 h.
  - GT4 in four variants takes ≈ 4 h.
  - GT3 at ≥ 1e7 steps takes est. 1–3 h.
  - GT17 and GT11 come on top.
  - Run sequentially, that is **10–21.5 h**.
  - Concurrency is not planned. GT11 ("idle" = no user process beyond the OS [60 §5.1]) cannot share the host with fuzzers "on idle cores" (A05).
- **The measurement protocol** gates every p99 at n ≥ 1,000 (≥ 10,000 under 1 ms) × 5 repetitions, idle and loaded [60 §5.1]. For long rows this is infeasible (A04):
  - full export (≤ 3 s) and import (≤ 7 s) of 1e5 at M5 exit: 10,000 runs each, ≈ 8–28 h;
  - rollup at 1e6 (1–3 s) at M1 exit: 2.8–8.3 h, plus a fresh store copy per sample, because a rollup consumes its input;
  - RG6's import at 1e6 (25–70 s): **69–194 h**.
- **Mutation testing** (GT16, "at every milestone exit", ≥ 90 % in the semantic crates) is not in §3.15. Inputs (est. [I]):
  - ≈ 45–65k product lines in the semantic crates;
  - ≈ 1 mutant per 8–15 lines, so 3,000–8,000 mutants;
  - 1.5–7 min each (incremental rebuild plus a short differential run);
  - so ≈ 75–930 CPU-h per full run, 20–230 h wall with 4 parallel jobs on a 32 GB host.

### 2.6 Disk

The laptop's drive is disk-starved: lanes stop under 15 GB free, and incremental build caches have filled the disk before [02 §9]. Test infrastructure adds (est. [I]):

| Item | Size |
|---|---|
| a VM guest | 10–30 GB |
| target directories for the model, the engine, fuzz and mutation copies | 5–20 GB |
| 1e6-node stores | 0.55 GB each plus history |
| image repositories (commit granularity 1e5 commits, before repack [AR §5b.9]) | ≈ 6 GB |
| corpora and logs | 1–5 GB |
| **total** | **≈ 20–55 GB** |

None of it is budgeted (A24).

---

## 3. Scope review

### 3.1 Components against requirements and priorities

"Serves" names the requirement (R1–R5, sync refs = "node 40", CLI/MCP/skills) or the priority (S speed, R RAM, C correctness, T tokens).

| Component / feature | Serves | Cost (source) | Verdict |
|---|---|---|---|
| Log + immutable segments + overlay; lock/HEAD protocol; recovery | sync refs, S, R, C | M1 46–57 units | keep |
| Optional leader | S (only if M0 items 1–2 require it) | +3–4 units | keep, conditional |
| Branch promotion (`seg.b*`, `TOUCH`) | R1 + S (G16) | ≈ 2 units | keep |
| `plan/*` kind with write mask | C (X13, CB3) | small | keep |
| Recursive virtual base; `op log`/`op restore`; cherry-pick; `revert --mainline 1`; tags; reflog; undo | R1 ("ideally" verbs, decision #5) | ≈ 4–5 units | keep |
| In-process git object layer | R3, R4 tree gate, `check`/`stale` speed | M4 15–21 units | keep |
| SHA-256 and reftable **reading** | R3/R4: Git 3.0 (rc on 2026-09-11, ≈ end of 2026) makes SHA-256 and reftable the defaults for **new** repositories [C] | small | keep: now clearly justified |
| Bundles | R3 transport without a remote | small | keep |
| Image at checkpoint and commit granularity; `--with-oplog` | R3; RG10 upgrade drill | M5 | keep |
| `refs/moirai/*` destination in the project repository | cross-machine sync riding the project remote; excluded by #11, off until #16 lists the remote | 0–2 units | **exclude by decision** (A17) |
| FTS tier 1 | search | small | keep |
| FTS tier 2 | S: tier 1 costs ≈ 10–50 ms at 0.3–0.5 M nodes (interpolated from 2–10 ms at 1e5 and 20–80 ms at 1e6, [AR §8.1]) | 2–3 units + ≈ 135 B/node | keep |
| Exact BM25 parity between tiers and across views | C (identical rankings) | `DOCLEN` + tokenizer byte (F12), per-view correction 0.5–3 ms, parity test | **decide by an M0 ablation** (A15) |
| R4 core (FL-1…FL-9 without E2) | R4 | ≈ 64–89 units | keep |
| R4 E2 (USN journal replay) | R4 accelerator | part of E2+E6's 3–4.5 units; `UsnCursor`, `USNCUR` | **exclude by decision**; keep the reservations (A13) |
| R4 evidence hook on mv/rm (default on) | R4, ~0.9 % of shell calls [M] | small | keep |
| R4 `Write`/`Edit` evidence hook (default off) | R4, measured revisit trigger (10 %) | small | keep |
| R4 `fs-nudge` hook (default off) | agent discipline; [40 §9.2] #8 recommends the skill rule only; no trigger | small + per-version hook fixtures | **exclude by decision** (A17) |
| `links mentions` | R4: the owner's 188 dangling references | small | keep |
| `links import` | cutover only | 1–2 units | build only if #23 = yes (A16) |
| R5 LQ core (grammar v1, binder, planner, executor, `TX`, versioned queries, named queries) | R5; C (verbs = named queries); T | M7 49.5–70 units | keep |
| LQ-Bench on two models × ablations | C, T | 3–5 units + ≈ 130–260 M model tokens at M0 (est.) | **one model; half-size ablations** (A14) |
| MCP, ten tools | MCP requirement; three Bash-less roles | M10 7–8 units | keep |
| `PostToolBatch` delta hook (default off) | sync refs L2 | small + per-version fixtures | **exclude by decision** with a trigger (A17) |
| Rollup inside the MCP server | S (folds deltas without a `gc` call) | an M1 entry point + RAM/latency risk | **remove** (A09) |
| Reference model (8–10k lines), format oracle, hand-written hex fixtures | C | M0 16–20 units | keep; bound its RAM (A21) |
| Test workstream (GT1–GT17), OS-crash loop, soak | C | ≈ 25 % of units | keep; resize the protocol and sampling (A04, A05) |
| `backup`/`restore`/`repair --rebuild-from-log` | C (OS crashes on the development machine) | M1 | keep |
| `export md`/`memory-md`/`rules` | CLI/skills (generated views) | small | keep |

### 3.2 What the simplifications save

- **Units.** A13, A16 and A17 together save est. 3–7.5 units: E2 ≈ 1–2, `links import` 1–2 (if not wanted), nudge + `PostToolBatch` + the second destination ≈ 1–3.5.
- **Tokens.** A14 saves ≈ 85–170 M model tokens at M0.
- **Recurring work.** They remove a recurring GT12 cost: hook fixtures re-checked on every Claude Code release, for two hooks nobody runs by default.
- **Risk.** A09 removes an unbudgeted RAM spike and a multi-second latency spike from the only long-lived process.

None of these introduces an interim stage. Each is an exclusion by decision with a measured revisit trigger, the same device [60 §1.3] uses for `rebase --onto`. The format reservations stay, so a later addition is additive.

---

## 4. The four priorities: where a budget or gate is missing

| Priority | Budgets and gates the design has | Missing | Finding |
|---|---|---|---|
| **Speed** | engine ≤ 5 ms per command, open ≤ 3 ms, flush-floor-relative commit, merge ≤ 50 ms, pack ≤ 8 ms, etc. | no bound on MCP request latency while the server does maintenance (a rollup blocks the `current_thread` server for 0.1–3 s) | A09 |
| **Minimal RAM** | CLI ≤ 4 MB; MCP ≤ 10 MB + 1 MB × min(active, 8); explicit export RSS; query `mem` | rollup RSS inside the MCP server; the aggregate over 16 concurrent sessions [M, 08 §2]; test-infrastructure RAM on the laptop | A09, A21, §2.1 |
| **Correctness** | GT1–GT17, RG3/RG4 | feasibility of the volumes on the available hardware (not the gates themselves) | A04, A05, A06, A18 |
| **Minimal agent tokens** | card ≤ 1,000 tokens; core skill ≤ 1.5k; brief ≤ 8,000 characters; hook delta ≤ 600; MCP results ≤ 32,000 characters | an end-to-end per-session and per-dispatch token budget with a baseline; a check against the harness's real output caps; token (not character) budgets for Cyrillic | A07, A08 |

---

## 5. Configuration

### 5.1 The configuration system the owner's rule needs (A10)

[AR §4.1] defines `config` as "text" in the store directory. It lists key families (`default-branch`, `image.*`, `discovery.git-hint`, `lease.*`, `gc.*`, `quiet`, `roots.*`, `files.*`, `query.*`). [60 §2.5] freezes "`config` keys" with the store layout at M0. The owner's rule puts most operational policy into this file, so it needs a specification at M0:

1. **Syntax and parser.** Use git-config style (`[files] policy.auto = exact`), with a hand-written parser (≈ 300 lines). [AR T10] admits no TOML crate. Freeze the **syntax, the precedence rules and the unknown-key rule** at M0, **not the key set**. An unknown key is a warning (exit 0, `doctor config` lists it), so later milestones add keys without a format change.
2. **Two scopes.**
   - **store**: `<store>/config`, shared by every worktree and every process of the store.
   - **user**: `%APPDATA%\moirai\config` on the owner's machine.
   - Rules:
     - Keys that change **versioned writes** are store-only, so every writer of one store behaves alike. Examples: `files.policy.auto`, `files.deletion-inference`, `merge.strict`, the policy rows.
     - Keys that name **machine-local paths or volumes**, or **security policy for data leaving the machine**, are user-only: `roots.*`, `files.main-tree`, `image.dest.*.path`, `files.usn`, `files.cloud`, `image.allowed-remotes`. A store restored from another machine cannot carry them.
     - **Discovery keys** (`discovery.git-hint`) must be user-scope or an environment variable. [AR §2.14] step 4 reads `config.discovery.git-hint` *to find the store whose config holds it*, which is circular.
   - **Policy data** (role policy, delete policies, per-kind merge auto-policies) stays schema rows, versioned per branch, as [AR §7.3] already has it.
3. **Typed registry.** Each key has a type (bool, int with range, size, duration, enum, set, path, glob list, URL list), a default, a scope and a reload class. The binary carries the registry. `moirai config list --defaults` prints it, and the documentation is generated from it. Add the verb `moirai config get|set|unset|list [--effective|--defaults]|check`, which is missing from §7.1. `set` validates the value.
4. **Reload.**
   - CLI and hook processes read config per command (every key is "hot" for them).
   - The long-lived MCP server re-reads config on its next request when a `config_gen u32` changes. `moirai config set` bumps `config_gen` in the `HEAD` slot's reserved bytes, a format-v1 reservation. The server already `pread`s `HEAD` before every request, so detection costs nothing.
   - Hand edits are detected by a stat at open (17–67 µs [M, 13 §1.7]).
   - Keys marked **restart** take effect when the server restarts. Keys marked **init** are fixed at store or destination creation.
5. **Store parameters versus keys.** [60 §2.5] records every threshold "in `HEAD` or `config` at init". Split them:
   - **init-fixed**, in `HEAD`: extent size, `hist` frame size;
   - **tunable**, in `config`, taking effect at the next checkpoint or decision point: checkpoint thresholds, fold width, promotion thresholds, the FTS tier-2 node count, the Kahn fallback, the `suspect` budget, the loose/pack threshold, retention windows.
6. **Sweep plan.** "The reference model and the tests implement every allowed value" [AR §11] multiplies the GT2 matrix. Specify it:
   - every key one-at-a-time away from its default;
   - pairwise for keys that interact (`files.policy.auto` × `files.deletion-inference`, `merge.strict` × per-kind policies);
   - the full volume on the default profile only.
7. **Visibility.** Every result header already names the branch and the tree. `--json` gains an additive `config` key listing non-default values that affected the result, so a behaviour change is explainable without reading the file.

### 5.2 Classification of every open decision

Legend:
- **REAL** = a real owner decision: it cannot be changed later by a key (format, identity, branch or merge semantics, the frozen query surface, the product's code boundary, money or hardware, data leaving the machine).
- **CONFIG** = a `config` key; see §5.3 for its type, default, scope and reload.
- **POLICY** = a policy-data row in the schema (versioned, store scope, hot).
- **RULE** = a design rule that must not become a key (§5.4).

"Due" is the milestone before which the decision is needed.

| Id | Decision (source) | Class | Key(s) or what stays with the owner | Due | Reason (one line) |
|---|---|---|---|---|---|
| C01 | #1 `shared` field class ([AR §11]) | REAL | — | M0 | changes view construction and the schema's field classes, both frozen at M0 |
| C02 | #2 remaining call: a second supervised lane | REAL | — | M0 | owner supervision time and machine capacity (A03); no key changes staffing |
| C03 | #3 git object I/O layer; may `image push/pull` spawn git | REAL (layer) + CONFIG (spawn) | `image.transport.spawn-git` | M4 | the layer is the product's code boundary; permission to spawn git is runtime policy |
| C04 | #4 image destination, refs, granularity, object format | CONFIG | `image.dest.<name>.{path, refs, granularity, object-format, kind}` | M5 | every value exists in the format; a change affects future exports only (the object format is fixed per destination) |
| C05 | #5 branch per lane; which R1 verbs ship | REAL | — | M0 | branch model and release scope (P8) |
| C06 | #6 permission posture of the MCP stamp hook | CONFIG | `hooks.stamp.permission`, `hooks.stamp.ask-for` | M10 | a per-installation approval policy. The recommended "`ask` for `rm`, `merge`" names verbs that are CLI-only and never reach the MCP stamp; the real candidates are owner-authority fields, edge deletes and `links_fix --confirm` |
| C07 | #7 Bash-less roles writing through MCP | POLICY | `policy.role.<role>.mcp-write` | M2 | a role-policy row, like [50] D2 and D12, which [AR] already reclassified |
| C08 | #8 prose in the repository | REAL | — | **M9** (not M0) | tracked Markdown as the source would be a sync feature. Bodies are inline under T6 either way, so no frozen byte depends on the answer |
| C09 | #9 deletion semantics | POLICY + RULE | `edges.blocks.on-src-deleted`, `edges.gates.on-src-deleted`; refuse `rm` under a lease = RULE (I32′) | M2 | [AR T5] already calls drop-and-notify "one schema row"; allowing `rm` under a live lease reopens S1 |
| C10 | #10 merge default: land or strict | CONFIG | `merge.strict` | M3 | the `--strict` flag already exists; land-versus-stage is policy, the typed merge is unchanged |
| C11 | #11 cross-machine or cloud writers in the release | REAL | — | M0 | identity scheme (`uid` primary, `#N` alias) |
| C12 | #12 durability classes and quiet windows | CONFIG + RULE | `durability.lazy-kinds`, `quiet.tail-cap-multiplier`, `quiet.from-lane-measuring`; graph mutations always durable = RULE | M1 | the lazy bit is per record (`RecHdr` flag); acknowledged-means-flushed for graph writes is the protocol contract the crash gates certify |
| C13 | #13 language and authority; which commit rule | CONFIG + POLICY | `brief.lang`; `policy.role.*.authority-owner`; the commit rule is content entered at cutover | M9 | rendering and policy; no frozen byte |
| C14 | #14 retention; `gc --squash-before` | CONFIG + REAL | `gc.reflog-expire`, `gc.cruft-delay`, `idempotency.retention`; squash = REAL (with C45) | M1 | windows are policy; squashing rewrites commit ids (identity) |
| C15 | #15 migration and generated views; Dev Drive / Defender exclusion | REAL | — | M9 (tooling), M11 (run) | one-time cutover content; a Defender exclusion is a security setting on the owner's machine, outside moirai |
| C16 | #16 may notes, findings and owner quotes leave the machine | REAL, enforced by CONFIG | `image.allowed-remotes` | M5 | data leaving the machine |
| C17 | #17 one store per repository or global | REAL | — | M0 | store layout, `#N` space and writer-lock scope are frozen |
| C18 | #18 Claude Code native Tasks | REAL | — (`hooks.native-tasks-mirror` if ever built) | M9 | scope: the mirror hook is not in the release |
| C19 | #19 `run` granularity | CONFIG | `runs.granularity` | M9 | the `run` kind is identical either way; only which calls open runs differs |
| C20 | #20 / [40] #13 file verbs outside the writer tree | REAL | — | M0 | the alternative adds versioned `pending_*` fields to format v1 |
| C21 | #21 / [40] #3 writing into repository files | REAL | — | M0 | product boundary over the user's files (I-F11) |
| C22 | #22 / [40] #9 tree-sitter in the product | REAL | — | M0 | code boundary (C runtime) |
| C23 | #23 / [40] #12 import existing citations | REAL | — | **M9** (A16) | one-time content; the tool costs 1–2 units only if wanted |
| C24 | #24 / [50] D1 may the language write | REAL | — | M0 | frozen query surface |
| C25 | #25 / [50] D4 where named queries live; who defines them | REAL (where) + POLICY (who) | `policy.role.<role>.define-query` | M0 / M7 | location is format (`QUERIES`, image files); definers are role policy |
| C26 | #26 / [50] D5 + D13 grammar scope | REAL | — | M0 | every v1 production is permanent |
| C27 | #27 / [50] D7 predicate names | REAL | — | M0 | frozen surface |
| C28 | #28 / [50] D8 absent-value logic | REAL | — | M0 | query semantics |
| C29 | #29 / [50] D11 counting semantics | REAL | — | M0 | query semantics |
| C30 | #30 / [50] D9 lease history | REAL | — | M0, or later with A23 | a new runtime structure in the frozen format; A23 would make it additive |
| C31 | #31 / [50] D10 the name | REAL | — | M0 | frozen surface (`*.lq`, `moirai-ql`) |
| C32 | [40] #1 `files.policy.auto` | CONFIG (confirmed) | `files.policy.auto` | M6 | store scope, because it changes versioned re-binds |
| C33 | [40] #2 `roots.<name>`, `files.scratchpads` | CONFIG (scope fix) | `roots.<name>` **user**; `files.scratchpads` store | M6 | [AR §5e.2] maps roots "per machine" but keeps them in the store config |
| C34 | [40] #4, #8 evidence, edit-evidence and nudge hooks | CONFIG (+ A17) | `files.hooks.*` | M9 | runtime policy; the "hard rule" alternative of #8 is a blocking hook, an anti-requirement [AR §12] |
| C35 | [40] #5 USN journal | CONFIG (user) or removed with E2 (A13) | `files.usn` | M6 | per-volume and machine-local; creating a journal is an owner admin action outside moirai |
| C36 | [40] #6 deletion inference | CONFIG | `files.deletion-inference` | M6 | the model implements both values |
| C37 | [40] #7 git index staging | CONFIG | `files.mv-git` | M8 | per-invocation `--git` already exists |
| C38 | [40] #14 cloud-synced roots | CONFIG (user) | `files.cloud` | M6 | cloud roots are machine-specific |
| C39 | [40] #15 anchor text in the image | CONFIG (per destination) | `image.dest.<name>.anchor-text` | M5 | a per-destination encoder option; what may leave at all is C16 |
| C40 | [40] #16 who confirms a guess | CONFIG | `files.confirm-roles` | M6 | role policy (could equally be a POLICY row) |
| C41 | [40] #10, #11 reader-tree moves and splits | RULE | — | — | agree with [AR §11]: the alternatives write unverified facts (DR1, I-F6) |
| C42 | [50] D2, D12 `TX` rights; developer field allowlist | POLICY | `policy.role.<role>.tx`, `policy.role.developer.fields` | M7 | [AR §11] already moved them; confirmed |
| C43 | [50] D3 named-only reads | CONFIG | `query.safelist.<role>` | M7 | confirmed |
| C44 | [50] D6 budget ceilings | CONFIG | `query.caps.<role>.*` | M7 | confirmed |
| C45 | [60 §3.14] erasing history [16 §11 Q5] | REAL | — | M0 | whether bodies may be dropped without changing commit ids is canonical-form identity |
| C46 | [60 §3.14] dedicated test host | REAL (money) | — | M0 | hardware purchase (A02) |
| C47 | [60 §3.14] recording the 16-agent load fixture | REAL (consent) | — | M0 | a recording of the owner's machine; resource profile only; stays local |
| C48 | [60 §3.14] leader in or out; T1 structure | neither | — | M1 | decided by M0 measurements, as [60] says; agree |
| C49 | [60 §3.14] cutover date | REAL | — | M11 | the owner's schedule |
| C50 | operational values hard-coded in [AR] with no key (lease TTL, pack/brief/delta budgets, MCP LRU size, writer-wait bound, `files` read and settle budgets, query defaults, as-of limit, `TX` work cap, backup age) | CONFIG | §5.3 rows 12–33, 41–52 | per milestone | the owner's rule names lease TTLs, budgets and pack sizes explicitly as config |

**Tally.**
- REAL: 24 (C01, C02, C05, C08, C11, C15–C18, C20–C24, C26–C31, C45–C47, C49). C45 also takes the squash half of C14.
- CONFIG or POLICY: 22 (C04, C06, C07, C09, C10, C12, C13, C14, C19, C32–C40, C42–C44, C50). The rule parts of C09 and C12 and the squash part of C14 are noted inline.
- Split between a REAL part and a CONFIG or POLICY part: 2 (C03, C25).
- RULE: 1 (C41).
- Decided by measurement: 1 (C48).

What moves:
- Seven entries that [60 §3.14] still lists as owner decisions due before M0 move to config or policy: #7, #9's policy half, #10, #12, #13, #14's retention half, #19.
- Two later-due entries move too: #4 and #6.
- #8 moves to M9.

### 5.3 Proposed key registry

Scope: **S** = store, **U** = user. Reload: **H** = hot (next command, request or hook), **R** = MCP restart, **I** = fixed at store or destination creation. "Src" is the decision or section the key comes from.

| # | Key | Type | Default | Scope | Reload | Src |
|---|---|---|---|---|---|---|
| 1 | `image.dest.<name>.path` | path | `<workspace>/<project>-moirai.git` | U | H | #4 |
| 2 | `image.dest.<name>.refs` | glob list | `main,tags/*,lane/*` | S | H | #4 |
| 3 | `image.dest.<name>.granularity` | enum `checkpoint\|commit` | `checkpoint` | S | H (future exports) | #4 |
| 4 | `image.dest.<name>.object-format` | enum `sha1\|sha256` | `sha1` | S | I (a change = a new destination) | #4 |
| 5 | `image.dest.<name>.kind` | enum `bare-repo\|project-refs` | `bare-repo` (`project-refs` excluded, A17) | S | I | #4 |
| 6 | `image.dest.<name>.anchor-text` | enum `full\|hash-only` | `full` | S | H | [40] #15 |
| 7 | `image.dest.<name>.git.pack-threads`, `.pack-window-memory` | int, size | `2`, `64m` | S | H (applied at `--create` and `image gc`) | G23 (RAM) |
| 8 | `image.export.on-merge-to-main` | bool | `true` | S | H | CM8 |
| 9 | `image.export.max-age` | duration | `1d` (A22 trigger) | S | H | CM8 |
| 10 | `image.allowed-remotes` | URL list | empty | U | H | #16 |
| 11 | `image.transport.spawn-git` | bool | `true` | U | H | #3 |
| 12 | `hooks.stamp.permission` | enum `allow\|ask` | `allow` | U | H | #6 |
| 13 | `hooks.stamp.ask-for` | set {owner-authority, edge-delete, links-confirm} | {owner-authority} | U | H | #6 |
| 14 | `hooks.subagent-start.auto-sync` | bool | `true` (clean previews only, D5) | S | H | [AR §7.5] |
| 15 | `hooks.session-start.settle` | bool | `true` | S | H | [AR §5e.3] |
| 16 | `hooks.post-tool-batch.delta` | bool | `false` (or excluded, A17) | S | H | [AR §7.5] |
| 17 | `files.hooks.evidence` | bool | `true` | S | H | [40] #4 |
| 18 | `files.hooks.edit-evidence` | bool | `false` | S | H | [40] #4 |
| 19 | `files.hooks.nudge` | bool | `false` (or excluded, A17) | S | H | [40] #8 |
| 20 | `mcp.overlay-lru` | int 1–8 | **4** (design: 8; A09) | S | R | G19 |
| 21 | `mcp.result-max-chars` | int | `32000` | S | H | CL4 |
| 22 | `pack.budget-chars.cli` | int ≤ 28000 | **24000** (design: 40000; A07) | S (U may lower) | H | [AR §7.4] |
| 23 | `pack.budget-chars.mcp` | int | `32000` | S | H | CL4 |
| 24 | `pack.budget-tokens.mcp` | int | `9000` (A07) | S | H | new |
| 25 | `pack.quota.{c2,c3,c4-dev,c4-critic,c5}` | percent | 15, 20, 30, 40, 10 | S | H | [AR §7.4] |
| 26 | `brief.budget-chars` | int ≤ 9500 | `8000` | S | H | [AR §7.4] |
| 27 | `hooks.delta-chars` | int | `600` | S | H | [AR §7.5] |
| 28 | `brief.lang` | enum `en\|ru` | `en` | U | H | #13 |
| 29 | `query.budget.default.{work, mem, rows, chars, visited, refs, fs, deadline-cli, deadline-mcp}` | ints, durations | 2e6, min(1 MiB, headroom), 50, 8000, 1e5, 4, 400, 2 s, 5 s | S | H | [50 §5.10] |
| 30 | `query.caps.<role>.*` | as above | 10× the agent maxima for orchestrator and owner | S | H | D6 |
| 31 | `query.safelist.<role>` | enum `off\|named-only` | `off` | S | H | D3 |
| 32 | `query.asof.max-ops` | int | `8000` (≤ 16000 in the CLI class) | S | H | [50 §5.10] |
| 33 | `tx.max-work-in-lock` | int | `5e5` | S | H | [AR §7.7.1] |
| 34 | `files.policy.auto` | enum `exact\|strong` | `exact` | S | H | [40] #1 |
| 35 | `roots.<name>` | path | none | **U** | H | [40] #2 |
| 36 | `files.scratchpads` | enum `refuse\|allow` | `refuse` | S | H | [40] #2 |
| 37 | `files.usn` | enum `auto\|off` | `auto` (or removed, A13) | U | H | [40] #5 |
| 38 | `files.deletion-inference` | enum `explicit\|main-tree-commits` | `explicit` | S | H | [40] #6 |
| 39 | `files.mv-git` | bool | `false` | S | H | [40] #7 |
| 40 | `files.cloud` | enum `metadata-only\|refuse` | `metadata-only` | U | H | [40] #14 |
| 41 | `files.confirm-roles` | role set | `orchestrator,owner` | S | H | [40] #16 |
| 42 | `files.main-tree` | path | the directory where `init` ran | **U** | H | [AR §5e.4] |
| 43 | `files.ignore` | glob list | `target/,node_modules/,build/` | S | H | [AR §5e.7] |
| 44 | `files.max-read-bytes` | size | `16 MiB` | S | H | [40 §7.3] |
| 45 | `files.read-budget-ms` | int | `20` | S | H | [AR §5e.3] |
| 46 | `files.session-start-cap-ms` | int | `150` | S | H | [AR §5e.3] |
| 47 | `lease.ttl-default` | duration | `15m` | S | H | [AR §6.2] |
| 48 | `lease.reclaim-older-than` | duration | `30m` | S | H | [AR §7.1] |
| 49 | `lock.writer-wait-ms` | int | `2000` (M0 items 2, 12) | S | H | G1 |
| 50 | `durability.lazy-kinds` | set ⊆ {heartbeat, cursor, session-mark} | all three | S | H | #12 |
| 51 | `quiet.tail-cap-multiplier` | int | `8` | S | H | #12, G10 |
| 52 | `quiet.from-lane-measuring` | bool | `true` | S | H | [AR §6.6] |
| 53 | `merge.strict` | bool | `false` | S | H | #10 |
| 54 | `runs.granularity` | enum `workflow\|agent-call` | `workflow` | S | H | #19 |
| 55 | `gc.reflog-expire` | duration | `90d` | S | H | #14 |
| 56 | `gc.cruft-delay` | duration | `14d` | S | H | #14 |
| 57 | `idempotency.retention` | duration ≥ the longest Workflow resume | `30d` | S | H | #14, F-A6 |
| 58 | `gc.trash-expire` | duration | `14d` | S | H | [40 §3.5] |
| 59 | `gc.fileobs-idle-expire` | duration | `30d` | S | H | [AR §4.9] |
| 60 | `backup.max-age` | duration | `1d` | S | H | risk 13 |
| 61 | `default-branch` | ref | `main` | S | H | [AR §5a.4] |
| 62 | `discovery.git-hint` | bool | `true` | **U** (or `MOIRAI_GIT_HINT`) | H | [AR §2.14]; circular in store scope |
| 63 | `store.checkpoint.{ops, bytes, body-bytes}` | int, size | 4096, 4 MiB, 32 MiB (M0 item 10) | S | H (next checkpoint) | [60 §2.5] |
| 64 | `store.fold-width`, `store.promotion.{ops, overlay-bytes, age-checkpoints}` | int, size | 3; 8192, 8 MiB, 16 (M0 item 3) | S | H | [60 §2.5] |
| 65 | `store.fts.tier2-nodes`, `store.kahn-fallback-edges`, `store.suspect-budget`, `store.image.loose-pack-threshold` | int | 20000, 1000, 10000, 8 (M0 item 8) | S | H | [60 §2.5] |
| 66 | `store.log-extent-bytes`, `store.hist-frame-commits` | size, int | 64 MiB, 256 | `HEAD` | I | [60 §2.5] |
| 67 | policy rows: `policy.role.<role>.{mcp-write, tx, fields, define-query, authority-owner}`, `edges.<kind>.on-src-deleted`, `merge.policy.<kind>` | schema rows | per [AR §7.3], §3.3, §5a.7 | S (versioned per branch) | H | #7, #9, #13, D2, D12, #25 |

### 5.4 What must never be a key

| Item | Why |
|---|---|
| R4 resolver-version constants: thresholds, the 50 ms quiescence, `is_text`, the case fold, never-candidate patterns, git-window bounds (R-14) | `resolve(link, tree, resolver version)` must be a pure function (I-F10). A key would let two machines classify the same tree differently. Changes go through an announced resolver-version bump |
| LQ semantics: grammar v1, two-valued logic, bag counting, hop bounds, error texts | the frozen surface; changed only by a new grammar version after LQ-Bench |
| Canonical form, the `.moi` codec, fan-out, hash algorithms, identity derivations | identity (I28′, I38′, I-F2) |
| Durability of graph mutations ("acknowledged = flushed") | the protocol contract GT1, GT3, GT4 and GT15 certify. A lazy graph class would double the crash matrix and weaken idempotency and marker guarantees ([AR T8]'s revisit trigger stays an owner decision with a protocol change, not a key) |
| The typed merge rules (lattices, key classes, the virtual base) | merge semantics; only per-kind auto-policies are schema data |
| I32′ (refuse `rm` under a live lease), [40] #10 and #11 | invariants whose alternatives write unverified or dangling facts |
| No timers, threads or watchers at idle | an anti-requirement [AR §12]; the "daily" behaviours need an explicit trigger (A22) |

### 5.5 Real owner decisions that no document lists (A01, A02, A06, A11)

| Decision | Due | Recommended default | Why it is real |
|---|---|---|---|
| Are Linux and macOS first-class in the release? ([17]–[20]) | before M0 entry | as the owner's brief to this audit lists: Windows only, with cheap format hedges (A01) | format, protocol, money |
| Where the moirai repository and CI live (private or public GitHub, or local only) | M0 | local-first git; a self-hosted runner on the test host; if GitHub is used, private and synthetic data only | money; the source leaves the machine |
| Residency of owner-derived corpora and fixtures (replay corpora, real-session LQ-Bench questions, register incidents, recorded HDRs, the load fixture) | M0 | never in the repository or on hosted runners; kept on the owner's machine and the test host | data leaving the machine |
| Guest OS licence for GT15 | M0 | Windows Server 2025 Core evaluation (180 days + one rearm), calibrated; buy a Windows 11 licence only if calibration fails | money (licence) |
| Model-call budget for LQ-Bench | M0 | single model (Opus 5.5), half-size ablations: ≈ 45–90 M tokens at M0 (A14) | money or subscription quota |
| Code signing for RG10's "signed binary" | M11 | unsigned or self-signed at a stable install path, unless an M0 item 11 probe shows signing changes spawn-to-exit. Defender's documented cost is rescans of **rebuilt** binaries, which the stable path addresses [05 §6.4]. Microsoft Artifact Signing ($9.99/month) serves individuals only in the US and Canada [C, 20 §4.4] | money |

### 5.6 The pre-M0 packet after reclassification

[60 §3.14] lists 27 rows before M0; [60 §3.15] counts ≈ 28. After §5.2:
- −7 move to config or policy (#7, #9's policy half, #10, #12, #13, #14's retention half, #19);
- #8 moves to M9;
- +4 are new (cross-platform, hosting and residency, guest licence, model budget).

That leaves **≈ 23 real decisions before M0**. A23 would also move D9 out. The owner's rule saves ≈ 3–5 of the 15–25 hours budgeted for decisions. More importantly, the packet then holds only questions whose answers change frozen bytes, identity or money.

---

## 6. Findings in detail

### A01 — blocker — An unintegrated cross-platform requirement would change format v1

- **Where.**
  - [AR] §1 row 14 ("Windows 11 first, portable later").
  - [AR] §9 "Not built, by decision … a Unix `Vfs` and `ProjectFs` (Windows 11 first; the seams keep them additive)".
  - [60 §1.3] row "Unix `Vfs` and `ProjectFs` | excluded (owner priority)".
  - The headers of [17], [18], [19] and [20], each citing an "owner requirement of 2026-09-26" that "macOS and Linux are first-class alongside Windows".
- **Problem.** The reports were written 06:21–06:26 on 2026-09-26, around the R4/R5 integration (06:25). No audited document cites them. The brief given to this audit lists R1–R5 and a Windows-only machine and does not mention the requirement. If the requirement stands, several frozen items are wrong at M0.
- **Evidence** (all from the four reports):
  - `HolderId {os, boot_id, ns_id, pid, start}` must enter lease records and the `LOCK` diagnostics, "adding it later would be a format-v2 change" [20 §8].
  - The fault model's item 2 ("metadata operations survive as a prefix of their issue order") is NTFS-shaped; Unix needs an explicit `sync_dir` [17 §1.4].
  - macOS durable flushes cost 3–20 ms, so "the 16-writer p99 ≤ 50 ms gate cannot hold on macOS while every writer flushes serially", and group commit "has to be settled at M0" [17 §1.2]. That contradicts [AR T2]'s optional leader.
  - Private CI ≈ $270/month [20 §5.2]; a Mac mini ≈ $599 [20 §7.6]; no calendar estimate exists.
- **Fix.** The owner confirms or rejects before M0 entry.
  - **Reject.** Add a one-line status to [17]–[20]: "portability notes; not a requirement of the release". Keep the Linux compile check. Optionally freeze three cheap hedges now: an `os` tag and holder fields in lease and `LOCK` records (≈ 16–32 B), `sync_dir` as a `Vfs` operation (a no-op on NTFS), and tri-state liveness (`Unknown` never reclaims).
  - **Confirm.** Apply [17 §8], [18 §10], [19 §10] and [20 §9] before the M0 specification review. Add Linux and macOS hardware and CI money to §3.14 as real decisions. Re-estimate M0, M1 and the calendar.

### A02 — major — M0 cannot exit without a purchase decision, and the no-purchase path is unquantified

- **Where.** [60 §3.1]:
  - item 7 (a "dedicated Windows 11 x64 test host");
  - the exit criterion "the test host completes nightly runs";
  - §3.14 row "provide one … otherwise the nightly jobs run on the owner's laptop in agreed windows, the gate volumes stay, and the calendar lengthens";
  - §3.15 "≈ 8–10 h per night from M1".
- **Problem.** The M0 exit is written for one infrastructure profile. If the owner declines the purchase, M0 cannot exit as written. The fallback has no number, so the owner cannot weigh ≈ $400–700 against its cost.
- **Evidence.** §2.4 gives the arithmetic:
  - GT15's 50–83 h at M1 exit;
  - +1–2.5 weeks across the exits;
  - M11's 14 consecutive nights plus the 72 h soak;
  - ≈ 3,000 CPU-hours of fuzzing.
- **Result.** No purchase costs ≈ **+2–4 weeks** (two-lane P50 39 → ≈ 41–43), **≈ 125–175 agent-free nights** from M1 exit to release, and a **3-day agent freeze per RC iteration**. If A03 applies, the one-lane calendar follows (P50 ≈ 60, P90 ≈ 73).
- **Fix.** Write two infrastructure profiles into M0 item 7 and the exit criteria:
  - **H (test host):** as specified, with A19's specification.
  - **L (laptop only):** nightly jobs in owner-granted agent-free windows (the number per week recorded as a parameter); GT15 on a laptop VirtualBox Server Core guest in those windows, or on the hosted QEMU/KVM rig of §2.2 if its M0 probe passes; PR CI either hosted with synthetic data only or none (agents' short seeds); RG3's "14 consecutive nightly runs" read as 14 consecutive completed runs.
  - Put the arithmetic of §2.4 in §3.14 next to the purchase row.

### A03 — major — The two-lane rate assumes build capacity the laptop does not have

- **Where.** [60 §7.1] "Two lanes assume the owner can supervise a second Opus lane" and "5–8 units per week"; [AR §11] #2.
- **Problem.** The only constraint the calendar names for a second lane is the owner's supervision. Machine capacity is not considered. The owner's machine has 1,823 MB free with 16 `claude` sessions resident [M, 05 §2.4]. `rust-analyzer` alone held 2.9 GB private for BoykoEngine [M, 08 §2]. Two moirai lanes compiling and testing a 66–99k-line workspace (est. 1–3 GB per `rustc`/test link at peak, [I]) cannot run beside a 16-agent campaign without paging.
- **Consequence.** One of three things happens: BoykoEngine campaigns pause while moirai is built; lane B runs on another machine; or the one-lane calendar applies. The difference is P50 39 against 60 weeks (+21), and P90 47.5 against 73 (+25.5) [60 §7.2].
- **Fix.** In M0, measure one moirai lane's peak RAM and CPU during a build-and-test cycle, and record the owner's intended overlap with other campaigns. State in §7.1 which machine each lane builds on. If the test host is bought, let it host lane B's builds by day (it is idle by day except for PR CI) and the gates by night. Report the calendar under the chosen arrangement.

### A04 — major — The measurement protocol's sample sizes are infeasible for long operations

- **Where.**
  - [60 §5.1] "n ≥ 1,000 for every p99 and ≥ 10,000 for operations under a millisecond; 5 repetitions";
  - "idle and loaded";
  - exit windows of "1–2 windows of 4–8 h per milestone exit" [60 §3.15];
  - RG6 "every row of §5.4 … at 1e4, 1e5 and 1e6 and at 0.3–0.5 M nodes, idle and loaded".
- **Problem.** The rule, adopted from [61 M-6] to fix n = 200 for ms-scale p99s, is applied uniformly, including to operations that take seconds.
- **Evidence.**

  | Milestone | Operation | Arithmetic | Total |
  |---|---|---|---|
  | M5 exit, owner's machine | full export 1e5 ≤ 3 s | 10,000 runs × 1–3 s | 2.8–8.3 h |
  | M5 exit, owner's machine | full import 1e5 ≤ 7 s | 10,000 runs × 2–7 s | 5.6–19.4 h |
  | M5 exit | both | | **≈ 8–28 h** against 4–16 h of windows |
  | M1 exit | rollup at 1e6 (1–3 s) | 10,000 runs, each needing a fresh store copy (a rollup consumes its input) | 2.8–8.3 h + copies |
  | RG6 | import at 1e6 (25–70 s [AR §8.1]) | | **69–194 h** |
- **Fix.** Tier n by duration and freeze the tiers in the protocol:

  | Duration | n | Gate on |
  |---|---|---|
  | < 1 ms | ≥ 10,000 | p99 |
  | 1–50 ms | ≥ 1,000 | p99 |
  | 50 ms – 1 s | ≥ 200 | p95 and max |
  | > 1 s (explicit commands) | ≥ 20 | the maximum |

  - Keep 5 repetitions below 1 s and 3 above.
  - Run the 1e6 and 0.3–0.5 M rows weekly on the test host, and only at M11 on the owner's machine.
  - With these tiers, M5's export and import rows take ≈ 20 min.

### A05 — major — The nightly plan is over-subscribed, and mutation testing is not budgeted

- **Where.** [60 §3.15] (test host ≈ 8–10 h per night: GT3, GT4 ≈ 4 h, GT15 ≈ 100–150 cycles, GT17; fuzzing "continuous on idle cores"); §3.13 GT11 "test host nightly" and GT16 "every exit from M0"; §5.1 "Idle = no user process beyond the OS baseline"; RG4 "GT16 ≥ 90 % … on the release candidate".
- **Problem.**
  - GT15 alone takes 5–12.5 h (100–150 cycles × 3–5 min). With GT4 (≈ 4 h) and GT3 (est. 1–3 h), the sequential sum is 10–21.5 h, and no concurrency plan exists.
  - GT11's idle measurements cannot share the host with fuzzers on "idle cores".
  - Mutation testing has no machine-time line at all. Est. [I]: 3,000–8,000 mutants in ≈ 45–65k product lines of semantic crates, × 1.5–7 min each, gives ≈ 75–930 CPU-h per full run, or 20–230 h wall with 4 parallel `rustc` jobs (RAM 1–3 GB each). It runs at each of up to 12 exits and again after every RC fix.
- **Fix.**
  1. **Night schedule.** GT11 runs exclusively first (≤ 1.5–2 h, rows tiered per A04). Then run concurrently, each with CPU and RAM caps:
     - GT15 with **two Server Core guests** (≈ 2–3 GB each, doubling throughput to ≈ 200–340 cycles per night, est.);
     - GT4 (4 h), then GT17;
     - GT3;
     - fuzzers on the remaining threads.

     The 1e6 GT11 rows and mutation samples run at weekends.
  2. **Sampled mutation testing.**
     - All mutants in functions changed since the previous exit (cargo-mutants' in-diff mode).
     - Plus a random pooled sample of 1,000 mutants over the semantic crates. For a 90 % kill rate that gives ±1.9 pp at 95 % confidence.
     - Gate: point estimate ≥ 90 % and lower 95 % bound ≥ 88 %.
     - Cost: ≈ 6–29 h wall per exit (est.).
     - The seeded protocol bugs of M1 stay mandatory in full.
  3. **Add the two lines to §3.15:** mutation testing and GT11's exclusive slot.

### A06 — major — The OS-crash rig lacks a licence plan and a RAM budget, and its calibration is on the wrong machine

- **Where.** [60 §3.1] item 7 ("a Windows 11 guest in VirtualBox or VMware Workstation"); §5.2 item 17 and [AR §8.2] item 17, listed under "M0 measurements on the owner's machine"; §3.15 GT15 volumes.
- **Problem.**
  1. **Licence.** A Windows 11 guest needs a licence. The Enterprise evaluation lasts 90 days (+1 rearm) [D/C], shorter than the ≈ 20–47 weeks from M1 exit to release. No licence decision is scheduled.
  2. **RAM.** A Windows 11 guest needs ≥ 4 GB, against 1.8 GB free under the agents' load. On the no-purchase path GT15 therefore runs only in agent-free windows (§2.1).
  3. **Calibration location.** Measurement 17 calibrates "the OS-crash rig", but [AR §8.2] lists it with the owner's-machine measurements. A calibration is valid only on the rig, hypervisor and guest that run GT15.
  4. **Windows Sandbox** (named in the brief) cannot be a rig: it is unavailable on Home, and it discards all state when closed [D].
- **Fix.**
  - Specify the default guest as **Windows Server 2025 Core evaluation**:
    - build 26100, "based on Windows 11, version 24H2" [D];
    - 180 days + one rearm [C];
    - ≈ 2 GB RAM [C];
    - ≈ 10–12 GB disk, Defender on;
    - run under VirtualBox 7 (host I/O cache off, `IgnoreFlush 0`) on whichever machine hosts GT15, with VMware Workstation Pro (free) as the cross-check hypervisor.
  - Keep WSL2 and Memory Integrity off on that machine ([20 §7.4]).
  - Move measurement 17 to the rig host and run it on the exact guest image.
  - Put "guest licence" into §3.14 as a money decision, needed only if calibration fails on Server Core.
  - Probe the hosted QEMU/KVM variant of §2.2 at M0 as the laptop-only profile's GT15 source.

### A07 — major — Output sizes exceed the harness's real caps

- **Where.**
  - [AR §7.1] `moirai pack … [--budget-chars 40000]`;
  - [AR §7.4] step 4 header "38,900/40,000 chars (~11.1k tokens)" as the CLI default;
  - [AR §7.2] MCP pack 32,000 characters "≈ 9k tokens";
  - [AR §7.1] exit 10 for incomplete results and `--ids` "has no row cap";
  - [07 §4] "Bash tool result: about 30,000 characters inline for successes, ~10,000 for failures. Exit 1 counts as failure" [D].
- **Problem.**
  1. A default CLI pack is 40,000 characters, 10,000 more than the Bash tool shows inline. Where the harness cuts, and whether the drop footer (`dropped: … → moirai pack 51 --more`) survives, depends on the harness's truncation mode. The design's "never silent truncation" is therefore not guaranteed on the primary path.
  2. Any non-zero exit (exit 10 included) is framed as a failure and capped at ≈ 10,000 characters. So an `--ids` listing cut by a budget and longer than 10,000 characters is truncated again by the harness.
  3. Budgets are in characters. Russian text ("halves pack capacity per character budget", [AR §11] #13) makes a 32,000-character MCP pack with owner rulings quoted verbatim ≈ 11–16k tokens, crossing the 10k-token warning that CL4 set out to avoid.
- **Fix.**
  - CLI default `pack.budget-chars.cli = 24000`. Values above 28,000 require `-o FILE`.
  - MCP adds `pack.budget-tokens.mcp = 9000`, computed with the per-script ratios measured at M0 item 6. The budget stops at whichever limit is hit first.
  - A result with a non-zero exit carries ≤ 8,000 characters on stdout; a longer `--ids` cut writes the remainder behind a cursor.
  - GT12 gains golden tests that measure the then-current Claude Code's inline caps and assert every default fits them, re-run on each harness release (risk 12).

### A08 — major — The token priority has no end-to-end budget or gate

- **Where.**
  - [AR §7.4] step 3 "then remaining budget by (class rank, criticality, recency, id)";
  - [AR §7.5] SubagentStart "role pack: critical rules for the role label …";
  - [AR §7.4] C2 "critical → L2";
  - [60 §3.10] M9 exit "the owner judges the pack complete against the HDR that was actually used" (completeness only).
- **Problem.** Every other priority has absolute budgets and gates. Tokens have only per-artifact caps (card, skill, brief, delta) and no per-dispatch or per-session budget against a baseline.
  - The fill rule spends the remaining budget, so the default cap is also the typical size (≈ 11k tokens per CLI pack).
  - Across ≈ 11 runs/day × 4–14 agents [M, 02 §12.6], that is ≈ 0.5–1.7 M pack tokens a day, and nothing compares it with today's HDR plus the plan files an agent reads itself.
  - Critical rules are injected twice per dispatch (SubagentStart and the pack's C2).
- **Fix.**
  - **M0 baseline.** Measure per-dispatch context tokens from recorded transcripts: HDR, role prompt and the plan/document reads in the first N turns, with the real tokenizer.
  - **M9 gate.** On ≥ 20 recorded dispatches, the median injected moirai tokens per dispatch (hook + pack) ≤ the baseline median, with the owner's completeness judgement kept.
  - **Fill.** Change step 3 to stop when the candidate classes are exhausted above a relevance floor. The budget becomes a cap, not a target.
  - **Duplicates.** `pack` omits C2 items the SubagentStart hook already delivered in this dispatch; the dispatch marker carries the rule revision, so `pack` stays a pure read. It prints one line: `C2: 6 critical rules delivered at start (rev 4471)`.
  - **Session budget.** Add a per-session overhead budget: SessionStart brief + skill listing + MCP names ≤ 3k tokens.

### A09 — major — The MCP server's RAM and latency are unbudgeted where they matter

- **Where.**
  - [AR §4.9] rollup "only on explicit `moirai gc` or in the resident MCP server after serving a request";
  - [AR §6.1] MCP row;
  - [60 §3.2] ("after serving a request" as an entry point for M10) and §3.11;
  - [AR §8.1] MCP ≤ 10 MB + 1 MB × min(active branches, 8), measured as **peak** private bytes [60 §5.1];
  - M10 "an unstamped read tool ≤ 5 ms at 1e5".
- **Problem.** A rollup rebuilds the base, reverse CSR, `TOPO` (Kahn), bitsets, the dictionary and tier-2 terms. Est. [I]:
  - reverse CSR at 1e6: 12 MB `IN_SRC` + 4 MB `IN_OFF`;
  - Kahn: ≈ 12 MB of in-degree, queue and output;
  - dictionary training: ≈ 11 MB of samples (100 × 110 KiB);
  - **≈ 20–40 MB peak at 1e6 and ≈ 8–20 MB at the owner's 0.3–0.5 M scale**, unless streamed through scratch files.

  Inside the MCP server this breaks the server's own peak-RSS gate. It also blocks the `current_thread` server for 0.1–0.3 s at 1e5 and 1–3 s at 1e6, so the agent's next tool call waits against a 5 ms budget. Separately, the owner runs 16 sessions [M, 08 §2]; each session's server may hold 10 MB + 8 overlays, so 16 × 18 MB = 288 MB (≈ 16 % of the 1.8 GB free) is possible and ungated. Branch overlays are also duplicated across servers.
- **Fix.**
  - Remove the MCP rollup entry point. Rollup runs only in an explicit `moirai gc` process, with its own budget (≤ 16 MB at 1e5, ≤ 64 MB at 1e6, est., gated at M1). It is triggered by the merge-into-`main` ritual or the owner's window driver; `brief` and `doctor` warn when deltas exceed 25 % of the base.
  - Add `mcp.overlay-lru` (default 4).
  - Add an M10 aggregate gate under a 16-session fixture: Σ private bytes of all moirai processes ≤ 16 × (10 + K) MB at 1e5, i.e. ≤ 224 MB with K = 4, and no MCP request delayed by maintenance.

### A10 — major — The configuration system is unspecified

- **Where.** [AR §4.1] `config` row ("text"); [AR §2.14] step 4 `config.discovery.git-hint`; [60 §2.5] "Store layout | directory contents, `config` keys …" frozen at M0; [AR §11] "the reference model and the tests implement every allowed value"; [AR T10] leaf crates.
- **Problem.** The owner's rule moves most operational policy into `config`, but the design specifies no syntax, scopes, types, validation, CLI verb, reload behaviour for the long-lived MCP server, or sweep plan.
  - **Key set frozen.** [60] freezes the key set, so every new key after M0 would be a format change.
  - **Machine-local keys in the store file.** `roots.*`, `files.main-tree`, `image.dest.*.path` and `files.usn` sit in the store file; a store restored on another machine carries them.
  - **Circular discovery.** `discovery.git-hint` is read to find the store that contains it.
  - **Typos fail open.** A mistyped key in a hand-edited file silently falls back to its default.
- **Fix.** §5.1 items 1–7. Reserve `HEAD.config_gen` (u32) in format v1. Freeze syntax, precedence and the unknown-key rule, but not the key set. Specify the registry of §5.3 at M0 and extend it per milestone.

### A11 — major — Real owner decisions about money and data leaving the machine are missing

- **Where.** [AR §11] #1–#31; [60 §3.14]; [60 §3.1] items 7, 10, 12; RG10 "Signed binary"; [60 §3.15] "Model calls | … per the owner's plan".
- **Problem.** The owner's rule lists "spending money/hardware, data leaving the machine" as the owner's calls. Four such calls are absent:
  1. **Repository hosting and CI.** Private hosted: ≈ $30–140/month (est.); public: free but public; local: no hosted runners (§2.3).
  2. **Residency of owner-derived corpora and fixtures.** The replay corpora from the owner's repositories, the real-session LQ-Bench questions, the register incidents and the recorded HDRs would reach GitHub the moment CI runs them there.
  3. **The LQ-Bench model budget**, est. below.
  4. **A code-signing certificate** for RG10.
- **LQ-Bench budget, est.**
  - 520 prompts × ≈ 10–20k tokens each (system, card ≤ 1k, tool schema ≈ 1.2–1.5k, tool results ≤ 8,000 characters, one retry) ≈ 5–10 M tokens per configuration.
  - × ≈ 13 configurations (baseline, ≈ 10 ablations, 2 alternative surfaces) × 2 models ≈ **130–260 M tokens at M0**, plus re-runs at M7, M8, M10 and before release.
- **Fix.** Add the four rows of §5.5 to [AR §11] and [60 §3.14] with those defaults.

### A12 — minor — The release-candidate restart rule is coarser than the guarantee needs

- **Where.** [60 §6] RG3: "any change to engine code (storage, graph, version control, image, resolver crates) restarts them"; §3.12 M11 ≈ 2.5 weeks of nights and soak.
- **Problem.** A resolver-only fix found by the M11 review restarts 1,000 OS-crash cycles and 14 nights of storage crash gates. Those cannot be affected when the storage crates are byte-identical. Each restart costs ≈ 2.5 weeks on a host and ≈ 2.5–3.5 weeks plus a 3-day agent freeze on the laptop (§2.4).
- **Fix.** Restart the gates whose covered crates' source hashes changed:
  - storage, graph or VCS crates changed → everything;
  - image codec only → GT8, GT4 with exports, GT5 `.moi`;
  - resolver only → GT2-links, GT17, GT4 with intents, GT5 fingerprints;
  - query crates only → GT2-queries, GT9, GT13.

  The dependency lint of M1 already proves which crates changed.

### A13 — minor — The USN-journal reader (E2) gives zero benefit on the owner's setup

- **Where.** [AR §5e.3] E2, §5e.7 ("USN-journal replay wherever a journal exists … D: has no journal"); [40] FL-4 with E2/E6 3–4.5 units; R-7 `UsnCursor`, R-8 `USNCUR`.
- **Problem.**
  - "Every repository and all 44 worktrees are on D:", and D: has no journal (`ERROR_JOURNAL_NOT_ACTIVE`) [M, 09 §4].
  - C:'s 32 MB journal kept only 1.07–1.70 h of history under load [M, 09 §4].
  - The design will not create a journal ("an optional admin action … not a moirai setting").
  - Under the defaults, E2 never contributes evidence on the owner's trees. It still costs code, simulator modelling of a journal, pattern-matrix rows and a config key.
- **Fix.**
  - Exclude E2 from the release by decision. Revisit trigger: a bound tree on a journaled volume whose journal window exceeds the median interval between settles.
  - Keep R-7 and R-8 in format v1 so adding E2 later is additive.
  - Drop `files.usn` until E2 exists.
  - Saves est. 1–2 units.

### A14 — minor — LQ-Bench runs a second model the owner does not use

- **Where.** [60 §3.1] item 10 "run as GT13 on the model the owner's agents run (Opus 5.5) and on the cheapest model any role may use"; §3.15 "450 prompts × 2 models × the ablations".
- **Problem.** Every agent in the owner's workflow runs on Opus. The second run measures a configuration that never occurs, and doubles the ≈ 130–260 M-token cost (A11).
- **Fix.**
  - One model, the one the owner's agents run; re-run when it changes.
  - The full 520 prompts for the baseline, the two gate-deciding ablations (D8 logic, D11 counting) and the two alternative surfaces.
  - A stratified 260-prompt half for the other ablations.
  - Total ≈ 45–90 M tokens at M0.

### A15 — minor — Exact BM25 parity across search tiers costs frozen bytes; decide it on evidence

- **Where.** [50 §5.5] "Ranking statistics are defined on the view … Both tiers therefore return identical rankings on every view, and a differential test checks it"; F12 (`DOCLEN` 6 B/node, tokenizer byte); [AR §2.11].
- **Problem.** Exact parity needs:
  - a frozen per-field length column;
  - per-segment statistics;
  - a per-view correction that re-tokenises overlay documents (est. ≤ 0.5 ms on `main`, 1–3 ms on a lane at first use);
  - a permanent parity test.

  A statistics-free deterministic scorer (matched terms weighted by field title 3, abstract 2, body 1; ties by recency, then id) makes the tiers identical by construction and needs none of it. Neither ranking has been measured on the owner's retrieval tasks.
- **Fix.** Add a ranking ablation (BM25 against the statistics-free scorer) to LQ-Bench's search stratum at M0. If the difference is within the benchmark's noise, drop F12's `DOCLEN` and the per-view correction before the freeze.

### A16 — minor — `links import` is built before its decision

- **Where.** [60 §3.10] M9 "`links import` (built and rehearsed here; run at the cutover only if the owner decides so)", 1–2 units; [AR §11] #23 "Due before the cutover (M11)".
- **Problem.** If the owner decides "no" at M11, 1–2 units and a rehearsal were spent on an unused tool.
- **Fix.** Make #23 due before M9. Build the importer only on "yes". The decision is one-time content, independent of the format.

### A17 — minor — Features built but off, without a measured revisit trigger

- **Where.** [AR §7.5] `fs-nudge` (`files.hooks.nudge`, default off) and the `PostToolBatch` delta (off by default); [AR §5b.8] and [60 §1.3] the `refs/moirai/*` project-repository destination (built in M5, "stays off until the owner lists the project remote", decision #16; its purpose, cross-machine sync, is out by #11).
- **Problem.** Each is built, tested and kept in hook or image fixtures that GT12 re-checks on every Claude Code release, yet none is on by default and none has a trigger that would turn it on. [40 §9.2] #8 itself recommends "skill rule only" over the nudge.
- **Fix.** Exclude the three by decision, with triggers:
  - nudge: when raw `mv` of linked files exceeds a measured share after the skill card ships;
  - `PostToolBatch`: when agents act on stale state that a mid-turn delta would have prevented, measured by guard-conflict rate;
  - `refs/moirai/*`: when #16 lists the project remote or #11 changes.

  Keep the `refs/moirai/` naming reserved. Saves est. 1–3.5 units.

### A18 — minor — The rig calibration must state what the rig cannot see

- **Where.** [AR §8.2] item 17 ("a deliberately unflushed write is lost at least once and a flushed write never is"); GT15 "Loss of the drive's own volatile cache is covered only by the simulator".
- **Problem.** With the host alive, a VM power-off loses only the guest page cache (W2). Writes the guest already issued (W3) survive in the host cache or on the drive "in every cache mode" [I from D, 20 §7.1]. The stated limitation covers a lying drive but not unflushed-but-issued writes, so GT15 could be read as testing flush placement.
- **Fix.** Adopt [20 §7.5]: record per rig (a) W2 loss seen, (b) a flushed write never lost, and (c) whether W3 loss was ever seen (expected: never). State in GT15 that flush placement is certified by GT1 only.

### A19 — minor — The test-host specification misses what makes it representative

- **Where.** [60 §3.1] item 7 "same Windows build as the owner's machine, NVMe, Defender on"; §3.15 "≥ 8 cores/16 threads, NVMe".
- **Problem.**
  - **Drive.** An enterprise NVMe with power-loss protection flushes in 1.6–12 µs against ≈ 1.7 ms on the owner's consumer drive [05 §2.2], which would make every floor-relative number unrepresentative.
  - **RAM.** Unspecified, yet the host must hold two GT15 guests, a 16-process kill loop, simulations and fuzzers, and emulate "1.8 GB free".
  - **VBS.** A fresh Windows 11 install often enables Memory Integrity, which slows syscalls and pushes VirtualBox onto the Hyper-V backend; the owner's machine has VBS off [M, 20 §1].
  - **Build.** "Same Windows build" cannot be held on Home editions, which update themselves.
- **Fix.** Specify:
  - a consumer NVMe without PLP;
  - 32 GB RAM, 1 TB;
  - VBS and Memory Integrity state recorded and matched to the owner's (turning it off on the host is the owner's security call);
  - "same feature version (25H2)", with the full build and the Defender engine and platform versions recorded in every measurement record. Floors are re-measured per run anyway.

### A20 — minor — The disk-full variants need elevation on Home

- **Where.** [60 GT4] "a small VHDX filled to disk-full"; RG7 "restore after a disk-full during a commit on a small VHDX".
- **Problem.** `New-VHD` needs the Hyper-V module, which Home lacks. `diskpart create vdisk`/`attach` need an elevated prompt [I, 20 §5.3]. A nightly job that elevates itself is a security posture the laptop should not have.
- **Fix.** One-time owner-run setup on the test host: a persistent small partition, or a VHDX re-attached at boot by an elevated scheduled task the owner creates. On the laptop profile, disk-full is covered by the simulator (fault model item 5) and by RG7 once, run by the owner.

### A21 — minor — The reference model's memoisation can exhaust RAM

- **Where.** [60 §4.2] "`state_at(tip)` by replaying the commit DAG from the root, memoised by commit id"; "`BTreeMap<#N, Node>` per state"; §4.6 "`std` only"; §4.4 item 8 "cases of ≤ 1e4 commands and ≤ 2e3 nodes".
- **Problem.** Without persistent (structurally shared) maps, which `std` does not have, memoising one full state per commit costs ≈ 0.4–1 MB × 1e4 commits ≈ **4–10 GB per case** (est.). GT3 re-evaluates reads "at its seq", so old states are needed. The simulations therefore could not run on the laptop at all, and only a few in parallel on a 32 GB host.
- **Fix.** Memoise tips plus a checkpoint every k commits, and replay between them; asymptotics do not matter for the model [60 §4.6]. Add a harness gate: ≤ 512 MB per model case, measured at M0's harness validation.

### A22 — minor — "Daily" exports have no trigger

- **Where.** [AR §2.15] and §5b.8 ("one per day … for every live lane", CM8); [AR §12] no timers; risk 13 (a warning at read time).
- **Problem.** No moirai process may run on a timer. The daily lane checkpoint (the only off-store copy of unmerged lane work) therefore happens only if a merge into `main` or the owner's window driver runs `image export`. On days without either there is no copy, only a warning.
- **Fix.** Make the first `SessionStart` hook after `image.export.max-age` (default 1 d) run an incremental checkpoint export of `main` and the live lanes (5–20 ms est. per checkpoint [AR §8.1]; skipped in quiet mode). That is deterministic, has no timer, and needs no owner script.

### A23 — minor — Format v1 has no "derived, ignorable" section flag

- **Where.** [AR §4.4] `SegHdr` sections `{tag u16, off u64, len u64, xxh3 u64}`; "readers refuse a newer version"; RG10 (a new section = format version 2 via `repair --rebuild-from-log`); [AR §11] #30 due before M0 because "an index is a new runtime structure in the frozen format".
- **Problem.** Every derived, rebuildable index added after M0 is a format-version bump with an upgrade drill. That forces decisions like D9 (lease history) before M0 and makes later speed improvements expensive. Lease records are already kept in the log forever [AR §4.9].
- **Fix.** Reserve one bit per section entry, `derived-optional`. An older reader ignores such a section, and `repair --rebuild-from-log` builds it. D9 then moves out of the pre-M0 packet (a later lease-history index is additive).

### A24 — minor — Test-infrastructure disk is unbudgeted

- **Where.** [60 §3.15] (no disk line); [02 §9] "stop under 15 GB free"; build caches have filled the disk before.
- **Problem.** §2.6: a VM guest (10–30 GB), target directories for the model, engine, fuzz and mutation copies (5–20 GB), 1e6 stores, 1e5-commit image repositories (≈ 6 GB before repack) and corpora add ≈ 20–55 GB (est.) on a 512 GB drive shared by C: and D:.
- **Fix.** Add a disk line to §3.15: ≤ 60 GB on the test host; on the laptop profile ≤ 25 GB with no VM image kept between windows (differencing disk deleted), and the harness refuses to start a job below 25 GB free.

---

## 7. Budgets of this audit

| Metric | Budget | Scale | Gate |
|---|---|---|---|
| GT15 guest RAM / disk | Server Core guest ≤ 3 GB RAM with the workload, ≤ 12 GB disk | per guest; 2 guests on a 32 GB host | calibration (a)(b)(c) on that guest before M1 |
| GT15 throughput | ≥ 200 cycles/night (2 guests) → ≥ 1,000 at M1 exit in ≤ 5 nights | test host | GT15 nightly log |
| Nightly window | GT11 exclusive ≤ 2 h first; all else concurrent within 10 h | test host | nightly completion ≥ 95 % |
| Exit-measurement sample tiers | n ≥ 10,000 (< 1 ms), ≥ 1,000 (1–50 ms), ≥ 200 (50 ms–1 s, p95 + max), ≥ 20 (> 1 s, gate on max) | every exit and RG6 | each exit's owner's-machine session ≤ 8 h |
| Mutation testing | in-diff mutants + 1,000 pooled sampled; point ≥ 90 %, lower 95 % bound ≥ 88 %; ≤ 30 h wall | each exit | GT16 |
| Fuzzing on the laptop profile | sanitizer off, `-rss_limit_mb=256`, ≤ 2 targets during agent work; 3,000 CPU-h cumulative | laptop / host | GT5 |
| Gate jobs beside the agents | ≤ 1 GB total; refuse to start below 1.5 GB free or 25 GB free disk | owner's laptop | harness pre-check |
| CLI `pack` default | ≤ 24,000 characters (Bash inline ≈ 30,000) | every CLI pack | GT12 against the current harness |
| MCP `pack` | ≤ 32,000 characters and ≤ 9,000 tokens (tokenizer-counted) | every MCP pack | M10 |
| Non-zero-exit output | ≤ 8,000 characters on stdout (harness failure cap ≈ 10,000) | every verb | GT12 |
| Per-dispatch injected tokens | median ≤ the M0 baseline median (HDR + plan reads) | ≥ 20 recorded dispatches | M9 |
| Session overhead | SessionStart brief + skill listing + MCP names ≤ 3k tokens | per session | M9 |
| MCP server RSS | ≤ 10 MB + 1 MB × min(active, K), K = 4 by default | per server, 1e5 | M10 |
| MCP aggregate RSS | Σ ≤ 224 MB (16 × 14 MB) | 16-session fixture, 1e5 | M10 |
| MCP latency under maintenance | no request delayed by maintenance; unstamped read ≤ 5 ms | 1e5 | M10 |
| Explicit rollup process RSS | ≤ 16 MB at 1e5, ≤ 64 MB at 1e6 (est.) | `moirai gc` | M1 |
| Reference model | ≤ 512 MB per case | GT2/GT3 cases (≤ 1e4 commands, ≤ 2e3 nodes) | M0 harness validation |
| Test-infrastructure disk | ≤ 60 GB (host), ≤ 25 GB (laptop) | per machine | harness pre-check |
| Config reload | MCP applies a change on its next request at 0 extra syscalls (`HEAD.config_gen`) | long-lived server | GT12 |
| LQ-Bench model tokens | ≈ 45–90 M at M0 (one model, half-size ablations) against ≈ 130–260 M as specified | M0 | owner budget decision |
| CI cost | $0 (self-hosted on the test host); ≈ $30–140/month if hosted private (est.) | monthly | owner decision |
| No-purchase calendar cost | +2–4 weeks (P50 39 → 41–43); ≈ 125–175 agent-free nights; a 3-day freeze per RC iteration; one-lane if A03 bites (P50 60) | release | owner decision (A02, A03) |
| RC restart | per changed crate set; ≤ 2.5 weeks per RC iteration on the host | M11 | RG3 |

---

## 8. What this audit did not re-raise

- **Provisioning.** [61 M-7]'s provisioning finding was adopted (test host, CI, OS-crash rig, §3.15). A02, A03, A05, A06, A19, A20 and A24 concern the arithmetic and feasibility of that adopted fix, not its absence.
- **Protocol.** [61 M-6]'s protocol was adopted. A04 concerns only its uniform sample size for multi-second operations.
- **Output caps.** CL4 (MCP pack 32,000 characters) is resolved. A07 concerns the CLI default, which CL4 left at 40,000, and the non-zero-exit cap.
- **Owner's rule for R4/R5.** The integration already reclassified the R4/R5 operational calls ([AR §11] config table, [60 §3.14] "specified, not decided"). §5.2 confirms those rows and adds only scope, type and reload attributes. [AR]'s Review log explicitly left #4, #6, #12 and #14 open ("reclassifying them is outside this integration"), and §5.2 closes them.
- **Durability model and leader.** Not reopened; A09 only moves a maintenance entry point.

---

## 9. Sources

**Design and research documents** (this repository): [AR] `docs/ARCHITECTURE-RESEARCH.md`; [40] `docs/research/design/40-file-links-design.md`; [50] `docs/research/design/50-query-language-design.md`; [60] `docs/research/design/60-roadmap.md`; [61] `docs/research/design/61-roadmap-critique.md`; [20c] `docs/research/design/20-critique-perf-ram-windows.md`; [02], [05], [07], [08], [09], [13] in `docs/research/`; [17] `docs/research/17-xplat-durability-mmap-memory.md`; [18] `docs/research/18-xplat-locking-ipc-processes.md`; [19] `docs/research/19-xplat-file-identity-change-tracking.md`; [20] `docs/research/20-xplat-toolchain-shells-ci-crash-testing.md`.

**Web, checked 2026-09-26:**
- Windows Server 2025 is based on Windows 11, version 24H2 (build 10.0.26100): [Wikipedia: Windows Server 2025](https://en.wikipedia.org/wiki/Windows_Server_2025)
- Server evaluation 180 days, one rearm on Server 2025; Server Core RAM in a VM: [Microsoft Q&A: Server 2025 evaluation](https://learn.microsoft.com/en-us/answers/questions/5724072/windows-server-2025-evaluation-edition-prematurely), [ALI TAJRAN: extend evaluation](https://www.alitajran.com/extend-windows-server-evaluation-period/), [omnisecu: Server 2025 minimum hardware](https://omnisecu.com/windows-server/2025/basics/minimum-hardware-requirements-windows-server-2025.php), [Microsoft Learn: hardware requirements](https://learn.microsoft.com/en-us/windows-server/get-started/hardware-requirements)
- Windows 11 Enterprise evaluation 90 days and rearm behaviour: [Windows OS Hub: rearm](https://woshub.com/extend-windows-evaluation-period-rearm/), [Eleven Forum: evaluation days](https://www.elevenforum.com/t/can-i-increase-licensing-days-of-windows-11-enterprise-evaluation.10081/)
- Windows Sandbox not on Home; disposable: [Microsoft Learn: Windows Sandbox](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/), [Microsoft Learn: Sandbox FAQ](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-faq)
- Git 3.0 defaults (SHA-256 and reftable for new repositories; 2.56-rc0 on 2026-09-11): [LWN: Git 2.56 and 3.0](https://lwn.net/SubscriberLink/1094575/2385e98583715c2b/), [Phoronix: Git 2.51-rc0](https://www.phoronix.com/news/Git-2.51-rc0), [byteiota: Git 3.0](https://byteiota.com/git-3-0-sha-256-default-rust-required-reftable-ships/)
- cargo-fuzz on Windows through the MSVC AddressSanitizer: [Rust Fuzz Book: setup](https://rust-fuzz.github.io/book/cargo-fuzz/setup.html), [Rust Fuzz Book: Windows](https://rust-fuzz.github.io/book/cargo-fuzz/windows.html)

*End of audit 74. No other file was modified.*
