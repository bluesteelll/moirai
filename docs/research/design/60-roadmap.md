# 60 — Roadmap: dependency-ordered, built once, done right

*moirai research/design, 2026-09-26. Status: research only; nothing is implemented. This file is the roadmap of record, **issue 2**. It replaces the build order of [AR §2.13, §9], the oracle-first plan of [22 §7.3], the adoption-driven draft [60d] and this file's own first issue, whose adversarial review [61] it answers point by point (Review log, §10). [AR] is amended by the edit list in §9. On 2026-09-26 the R4 and R5 designs ([40] revision 2, [50] revision 2) were integrated into [AR], replacing the slots of §9.3; this file was brought in line with them at the same time (§10.4). Amended again on 2026-09-26 by the final editorial pass over the priority audits [70]–[74] (speed, minimal RAM, correctness, minimal agent tokens, feasibility and configuration): the format-freeze list, M0–M11, the gate catalogue, the decision schedule, the measurement protocol and the budgets follow [AR]'s fixes (§10.5); [AR §8.3] is the normative budgets-and-gates table and [AR §13] the configuration reference. Amended a third time on 2026-09-26 by a verification pass (§10.6): no third-party embedded database run for any purpose, the M6 scope and exits, M9 certifying the command transport and M10 the `mcp_tool` one, the new gate GT20 (git independence and dependency lints), owner decision #41 on scope exclusions, and the calendar labelled as the pre-audit baseline. Amended a fourth time on 2026-09-26 for owner decision #32 by the cross-platform design [80], revision 2 (§10.7). Amended a fifth time on 2026-09-26 for owner decisions #43 and #44 by the harness-agnostic design [90], revision 2, and §7's calendar re-issued with every delta since the pre-audit baseline (§10.9). Amended a sixth time on 2026-09-26 by the owner's answers to [AR §11] — the laptop-only profile L (#34), two lanes both building on the laptop (#2), LQ-Bench on Opus 5.5 only (#38 (a)), a public GitHub repository (#36) with owner-only commit authorship, and every other decision as recommended: §0, §1, §3.1, §3.8–§3.15, §4, §5, §6, §7 and §8 follow, and §7's calendar is re-issued for two lanes in profile L (§10.11). Amended a seventh time on 2026-09-27 by the owner review of the approval checklist ([AR] binding inputs): [AR] with [40], [50], this file, [80] and [90] is approved as the M0 specification; the re-review of [40] and [50] runs inside M0's specification review; GT13 runs on the reference model's own parser and binder with [AR §7.7.5]'s gate list; five R4 replay targets gate M0 and three gate M6; LQ-Bench runs through the owner's Claude Code subscription with no API billing; the OS-crash rig (GT15) is deferred to after the release, and §7's calendar is re-issued without it (§10.14). Amended an eighth time on 2026-09-27 by the dispositions of the A1 re-review (M0 WP-80a): the M0 measurements that decide the leader and the checkpoint threshold reproduce the product's costs, the reference model takes git history as data, and the M0 gate texts follow [40], [50] and [AR] (§10.15).*

**Owner decisions of 2026-09-26 (binding; verbatim translations).**

1. "No, we do not use SQLite — we build our own [engine] right away."
2. "'So that you can start using it as early as possible' is not important — it must be done properly right away."

Standing owner preferences: maximum performance, minimal RAM, zero idle CPU; every build agent runs on Opus. Owner decision #32 (2026-09-26, verbatim translations): "Take into account that the system must work under macOS and Linux too." and "Don't build binaries for Linux and Mac for now, just take into account that this will need to be implemented. But we are not testing this for now — there is no possibility." — Windows is built and gated in M0–M11; Linux and macOS are designed now and ported later ([80], [AR §14]).

**The owner's answers of 2026-09-26 on [AR §11]** (verbatim translation): "For now no additional machine will be used, everything is here. Two lanes in parallel. For now benchmarks only on Opus 5.5. The moirai project itself will be stored in a public repository on GitHub. Record that all commits must be made WITHOUT Claude co-authorship. Everything else I approve as you wrote it." — #34 the laptop-only profile L, #2 two lanes both building on the laptop, #38 option (a), #36 a public GitHub repository, and every other decision as recommended (§3.14). The repository rule — every commit authored by the owner, with no Claude or other AI co-author — is in §3.1 item 7. **The owner review of 2026-09-27** ([AR] binding inputs; items A1–A8, B1–B14 and V1–V10 of the Russian approval checklist) decided A1–A8 as recommended — [AR] with the normative [40], [50], this file, [80] and [90] approved as the M0 specification (A4), the re-review of [40] and [50] revision 2 inside the M0 specification review (A1), the harness configuration with attribution off committed (A2), the published content accepted (A3), GT13 on the model's own parser and binder with [AR §7.7.5]'s gate list (A6), five R4 replay targets at M0 and three at M6 (A7), build windows agreed at M0 start (A8) — confirmed B1–B14, and changed two things (verbatim translations): "I will not buy API access; only Claude Code by subscription is available" (V1: LQ-Bench through the owner's Claude Code subscription, no API billing and no API key, §3.1 item 10) and "No way to install it for now; such a deep test is postponed until the release, like mac and linux" (V7: the OS-crash rig deferred to after the release with the port phase, §3.13 GT15).

**Sources.**

| Tag | Document | Used for |
|---|---|---|
| [AR] | `docs/ARCHITECTURE-RESEARCH.md` | the design of record: §0–§12 and the Review log |
| [61] | `research/design/61-roadmap-critique.md` | the adversarial review of issue 1 of this file: 2 blockers, 9 majors, 10 minors; every one is dispositioned in §10 |
| [40] | `research/design/40-file-links-design.md` | the R4 design (move-proof file links), revision 2: reservations R-1…R-18 (§2.11), layers FL-0…FL-9 (§8.1; revision 1's FL-10 is folded into FL-4), tests (§8.3), budgets (§7), owner calls (§9.2) |
| [41] | `research/design/41-file-links-critique.md` | the adversarial review of [40]: 4 blockers, 12 majors; its §5 lists the reservation and build-order changes [40] must make before M0 |
| [50] | `research/design/50-query-language-design.md` | the R5 design (the query language LQ), revision 2: reservations F1–F18 (§8.1), packages LQ-0…LQ-14 (§8.2), LQ-Bench (§7.4), owner calls (§9.2), changes to this file (§10) |
| [60d] | `research/design/60-engine-first-roadmap-draft1-adoption-driven.md` | reused where sound and independent of early adoption: format-freeze list, reference-model design, measurement items, floor-relative gates, gate catalogue, edit-list anchors |
| [20] | `research/design/20-critique-perf-ram-windows.md` | engine risks, grafts G1–G28, recomputed budgets, the F-A1 bugcheck scenario |
| [21] | `research/design/21-critique-semantics-correctness.md` | merge and invariant findings (N1–N16) |
| [22] | `research/design/22-critique-agent-fit-buildability.md` | effort units (§7.1), the adoption-first argument (§2.7) this plan answers differently |
| [05] | `research/05-rust-storage-perf-ram.md` | measured primitives (§2), Windows rules (§6), budgets (§16), measurement plan (§17, item 5 superseded, §5.6) |
| [08] | `research/08-concurrency-sync-git-interop.md` | protocol evidence (§3), Windows hazards W1–W12 (§4), crash-safety principles and the fsync-failure lesson (§8) |
| [09]–[13] | `research/09-…` to `research/13-…` | R4 research, as used by [40] |
| [14]–[16] | `research/14-…` to `research/16-…` | R5 research, as used by [50] |
| [80] | `research/design/80-cross-platform-design.md` | the cross-platform design (owner decision #32), revision 2 after its review [81]: the OS layer, per-OS mappings, the items M0 freezes for the ports (X-F1–X-F12), the port phase; built from the reports [X17]–[X20] (`research/17-…` to `research/20-…`) |
| [81] | [research/design/81-cross-platform-critique.md](81-cross-platform-critique.md) | the adversarial review of [80]'s first revision: 2 blockers, 7 majors, 15 minors; every finding is dispositioned in [80 §8] |
| [X17]–[X20] | [research/17-xplat-durability-mmap-memory.md](../17-xplat-durability-mmap-memory.md), [research/18-xplat-locking-ipc-processes.md](../18-xplat-locking-ipc-processes.md), [research/19-xplat-file-identity-change-tracking.md](../19-xplat-file-identity-change-tracking.md), [research/20-xplat-toolchain-shells-ci-crash-testing.md](../20-xplat-toolchain-shells-ci-crash-testing.md) | the cross-platform research reports: durability, mappings and memory; locking, IPC and processes; file identity and change tracking; toolchains, shells, CI and crash rigs |
| [70]–[74] | `research/design/70-audit-speed.md` … `74-audit-feasibility-config.md` | the priority audits of the integrated design; every finding is dispositioned in [AR]'s Review log and applied here (§10.5) |

**Tags.** **[M]** measured on the owner's machine (quoted from the cited report); **est.** arithmetic with inputs shown; **per [40]** / **per [50]** means the named design is authoritative and this file only points to it (P9). Everything else is a design decision of this roadmap. Effort is in the units of [22 §7.1] (proposal A = 100).

---

## 0. Summary

- **One build, in dependency order, with no interim stages.** Twelve milestones, M0–M11. Each builds one component to its **final** specification and passes its **full** gates before anything builds on it, and no milestone modifies a certified one except through an extension point certified earlier (P10). There is no SQLite or other third-party database anywhere, no swappable backend, no serialized-access stage, no node cap and no "v1.1" deferral taken to ship sooner. The owner's workflow starts using moirai once, at the release gate of §6.

  | M | Component | Requirement | R4 / R5 content built here |
  |---|---|---|---|
  | M0 | **Contract and evidence**: on-disk format v1 frozen with every reservation, the `Vfs` fault model and the store parameters; `Store` API; the complete reference model; the query surface frozen by an accuracy benchmark; measurements and the measurement protocol; the public GitHub repository and hosted CI (#36), the laptop-only infrastructure profile L (#34); the OS-crash rig deferred to after the release (the owner review of 2026-09-27) | — | R4 reservations [40 §2.11]; FL-1 pure libraries validated on the replay corpora; R5 reservations [50 §8.1]; LQ-3 (the model's own parser, binder and evaluator) and LQ-Bench (GT13) on it |
  | M1 | **Storage engine**: final multi-process protocol, recovery, physical branch machinery, section registry, GC, backup/restore/repair | — | every reserved R4/R5 section and record kind exercised through the registry |
  | M2 | **Graph core** | — | FL-3 graph layer; the [50] graph-side producers |
  | M3 | **Version control** | **R1** | FL-7 merge rules; the [50] history-side producers |
  | M4 | **Git object layer** | (serves R3, R4, `check`/`stale`) | in-process ancestry for R4's tree gate |
  | M5 | **Git image** | **R3** | FL-8 image carrier |
  | M6 | **File-link runtime** | **R4** | FL-2 `ProjectFs`, FL-4 resolver/settle/intent protocol with git (E6) evidence; E2 excluded by decision, reservations kept ([AR §11] #41) |
  | M7 | **Query language** | **R5** | LQ-1, LQ-2, LQ-4…LQ-7, LQ-9, LQ-11; FL-6 link built-ins |
  | M8 | **CLI** | **R2** (user-visible) | FL-5 file verbs; LQ-8 `q`/`tx` |
  | M9 | **Agent interface** | — | FL-9 and the hook accelerators; `links import`; LQ-12 |
  | M10 | **MCP** | — | LQ-14; FL-9's MCP operations |
  | M11 | **Release hardening** to the "complete and hardened" gate | release | the upgrade drill |

- **Owner decision #32 is decided (2026-09-26).** M0 specifies and freezes the OS layer for Windows, Linux and macOS ([80], [AR §14]); M1–M11 build and gate Windows only — nothing in M0–M11 builds, runs or tests a Linux or macOS binary; the Linux and macOS implementations form an unscheduled port phase ([80 §5]) outside this calendar and the release gate.
- **What issue 2 changes**, each on a finding of [61] (§10): R4 is no longer one late milestone but attaches to its host components as [40] specifies, and its reservations are [40]'s, not placeholders (B1); a specified `Vfs` fault model, crash enumeration over lost and torn writes, and an OS-crash loop on a virtual machine now gate the storage engine (B2; the OS-crash loop was deferred to after the release by the owner review of 2026-09-27, §3.13 GT15); M1 certifies a section-producer registry with every reserved section, so later milestones extend it instead of modifying it (M-1); every piece of evidence that can change frozen bytes — the query accuracy benchmark, R4's replay corpora, the T1/T2 trigger quantities, the gate-0 carrier table — moves into M0, and the image moves ahead of the query language (M-2); every owner decision is due before the first milestone it can change, which puts most of them before M0 (M-3); the reference model gets its own query parser and canonical-form encoder, owner-signed rule tables and mutation testing (M-4); every threshold becomes a store parameter so the model sees threshold-gated code (M-5); a measurement protocol is frozen at M0 (M-6); M0 provisions CI and the test infrastructure on the existing public repository (M-7; since 2026-09-26 on the laptop-only profile L, with the repository public on GitHub, [AR §11] #34, #36); the estimates are recomputed from [40] and [50] with P50 and P90 (M-8); the release gate drills a format upgrade (M-9).
- **The order differs from the suggested one in six places, each on evidence (§2.4), and item 7 records why M9 precedes M10 despite the owner's CLI → MCP → skills naming:** version control before the query language and the CLI; the git object layer as its own leaf; the physical half of branching inside the storage engine; the image right after version control and the object layer; R4 split across its host components; and the evidence that shapes the format gathered before the freeze.
- **Items the old plan deferred are resolved, not postponed (§1.3):** branch promotion, `op log`/`op restore`, FTS tier 2, schema strengthening, `--with-oplog`, SHA-256 images, bundles, in-process ancestry and the recursive virtual merge base are in the release. What stays out is out by decision, never for time.
- **The oracle is a Rust reference model** written from the specification by a separate author **before** the engine (M0), using definitional algorithms, its own query parser and binder and its own canonical-form encoder, and sharing no code with the engine (§4). The git CLI is the independent oracle for git objects and images; ground truth from the `ProjectFs` simulator is the oracle for file identity.
- **Benchmarks** are absolute budgets plus distance from physical floors re-measured in the same run, under a measurement protocol frozen at M0 (§5). Since the priority audits of 2026-09-26 ([70]–[74], §10.5), every budget belongs to one of the owner's four priorities — speed, minimal RAM, correctness, minimal agent tokens — and [AR §8.3] lists them with their gates; only real owner decisions remain in §3.14, and every operational policy is a `config` key of [AR §13]. No third-party embedded database (SQLite, redb, heed/LMDB or any other) is built, linked or run for any purpose, in or out of tree — performance is judged only against the absolute budgets and the M0 physical floors, and a `Cargo.lock` dependency lint enforces the rule (§3.13 GT20).
- **Calendar (§7, est.; with every delta since the pre-audit baseline — the priority audits, the cross-platform design and the harness-agnostic design with the pure-Rust rule, [90 §10.3]):** ≈ 369–508.5 units. **Two lanes, both on the owner's laptop in profile L** ([AR §11] #2, #34; the calendar of record, re-issued on 2026-09-27 without the OS-crash rig that the owner review of 2026-09-27 deferred): storage engine certified at 12–26.5 weeks, R1 at 21–45, R3 at 23.5–50, R4 at 24–52, R5 at 25.5–54.5, **release at 35–75 weeks (P50 ≈ 50.5, P90 ≈ 60.5)**; a test host (not bought) would have given 33–69.5 (P50 ≈ 47, P90 ≈ 57). One lane in profile L, the fallback if the laptop cannot carry two: release at 51–110.5 weeks (P50 ≈ 74.5, P90 ≈ 89.5). The pre-audit baseline was ≈ 321.5–428 units, two lanes P50 ≈ 39 / P90 ≈ 47.5, one lane P50 ≈ 60 / P90 ≈ 73. The rate (5–8 units per week) is the earlier plan's estimate, not a measurement; velocity is measured at the M0 and M1 exits and the calendar re-issued then.
- **The price is time to first use** (risk 1 of §8). It is answered by validating on recorded data — the query benchmark on the owner's agent model, R4's replay corpora and pattern matrix, HDR-vs-pack diffs on recorded dispatches, a synthetic campaign replay — not by early adoption.
- **§9 is an exact edit list for [AR]**, regenerated for this issue and applied: every SQLite use, oracle backend, engine-trait swap point, S0–S6 slice, adoption milestone and early-adoption rationale; the v1.1 deferrals; the leader's old `M6` label; and R4/R5 as marked slots that point to [40] and [50] (replaced on 2026-09-26 by the integration of both designs into [AR], §9.3).

---

## 1. Principles and the critical path

### 1.1 What the two decisions rule out

| Decision | Rules out | Keeps |
|---|---|---|
| 1 — no SQLite, own engine right away | SQLite or any other third-party embedded database (redb, LMDB/heed, fjall, sled, RocksDB, Turso) as a backend, a stepping stone, a test oracle or a benchmark; the engine trait as a swap point; the S0–S5 "throw-away backend" | citations of SQLite's *design* (WAL semantics, the WAL-reset race lesson, the session extension, the rebaser idea) — as literature only; no third-party embedded database is built, linked or run for any purpose, in or out of tree |
| 2 — done properly right away | early adoption as a goal; interim or throwaway stages; temporary modes (e.g. [60d]'s serialized-access stage); node caps; reduced guarantees taken to ship sooner (e.g. "v1.1" deferrals whose only reason is schedule); two implementations of one concern where the first exists to ship sooner (e.g. `git fast-import` now, a hand-written layer later); "useful first slices" of a subsystem ([40 §8.1]'s "first complete layer", [41 M9]); milestones ordered by time to adoption | adoption **after** a defined "complete and hardened" gate (§6); scope exclusions decided on requirements or evidence, each with a measured revisit trigger |

### 1.2 Rules that follow

| # | Rule | Consequence in this plan |
|---|---|---|
| P1 | **One implementation per concern.** A trait exists only at a seam that has a test double, never as a swap point between two product implementations. | Seams: `Vfs` (Windows in M1, Linux and macOS in the port phase of [80 §5], one implementation per target chosen at compile time; an in-memory simulator enforcing the weakest-OS fault model of §2.5), `ProjectFs` (Windows in M6, Linux and macOS in the port phase; deterministic simulator of project-file behaviour, [40] FL-2), `View` (live tip, branch, as-of, staging), `Store` API (the engine; the reference model is test-only). Extension points (tables of producers inside one implementation, never swap points, §2.6): the section-producer and record-fold registry (M1), the validator and merge-rule tables (M2–M3), the relation, built-in and named-query registries (M7). No engine trait, no `ImageBackend` trait. |
| P2 | **Final specification first, then build.** A component starts only when its specification, the owner decisions it depends on and the measurements that shape it are settled. **A decision is due before the first milestone whose format, protocol or code it can change** — and because the reference model carries every logical rule from M0 on (§4), every decision that changes a logical rule is due before M0. | M0 cannot start before [40] is revised after [41] and [50] has passed an independent review, and cannot exit before their reservations are frozen; the leader and T1's structure are decided by M0 measurements; the query surface is frozen by LQ-Bench before any of its executor is built; §3.14 schedules every decision. |
| P3 | **Gates before dependents.** A component's full gate set passes before any component that builds on it starts; a gate that becomes mandatory stays mandatory on every later change. | §3.13 lists each gate with the milestone from which it is mandatory. |
| P4 | **The format is frozen at M0 exit** — every on-disk byte layout including every reservation (§2.5), the `Vfs` fault model and the store parameters. | No milestone migrates data. A format change after M0 is a specification defect: M0 re-opens, every passed milestone re-runs its gates, pre-release stores are regenerated, never migrated. After the release a format change follows the upgrade procedure drilled in RG10. |
| P5 | **The reference model leads.** The model is written first, complete, at M0; a feature with no model counterpart cannot pass its gate; a model–engine disagreement is triaged as a specification finding before either side changes. | §4. |
| P6 | **Measured on the owner's machine under a frozen protocol.** Exit criteria that concern time, memory or Windows behaviour are measured there (Ryzen 9 5900HS, 16 GB, NTFS, Defender on), idle and under the replayed 16-agent load, per the protocol of §5.1, in agreed windows that never overlap the owner's benchmark windows [02 §9]. Nightly regression runs on the same laptop in owner-granted agent-free windows (profile L, [AR §11] #34; §3.1). | §5. |
| P7 | **Dependency order, not adoption order.** Components with no dependency between them may run in a second lane; nothing is ordered by when the owner could start using it. | §2, §7. |
| P8 | **Scope changes are owner decisions.** No schedule reaction may cut a guarantee; a proposed cut goes to [AR §11] as a decision with its consequence. | No "pre-agreed cut list" ([60d §6.2] is dropped). |
| P9 | **No placeholder contradicts an existing design.** Where the R4 or R5 design exists, this roadmap cites it and carries no content of its own for it; when [40] or [50] change, they win and this file follows. | §2.5 carries [40 §2.11] (revision 2) and [50 §8.1] (revision 2) by row; the [AR] edit list stated R4/R5 only as slots that point to them (E-slot edits of §9), and the integration of 2026-09-26 replaced those slots with [AR] §5e and §7.7, following both designs. |
| P10 | **Certified milestones are extended, never modified.** A later milestone adds behaviour only through an extension point certified in an earlier one, and re-certifies the earlier milestone's gates with its real producers as part of its own exit criteria. | §2.6; the "re-certification" exit criterion of M2, M3, M5, M6 and M7. |

### 1.3 Every item the old plan deferred, classified

"In release" means built to its final specification in the named milestone. "Excluded" means not built because a requirement or measurement says it is not needed; each keeps its measured revisit trigger. "Owner scope" means the owner decides, before the milestone named.

| Item ([AR] position) | Class | Where / why |
|---|---|---|
| Branch promotion `seg.b*` + `TOUCH` (v1.1) | **in release** | M1 (physical), M3 (policy). Without it a quiet 60-day `plan/*` branch grows its first-read cost and `--across` over all refs has no bounded form (G16, G19). |
| Hand-written git object layer (v1.1) and the `check`/`stale` `git merge-base` spawn (v1) | **in release** | M4. The fast-import path was chosen to ship sooner [22 §3.4 D10]; kept as the final design it makes git a runtime dependency of R3, keeps a 74 ms [M] spawn per uncached ancestry pair, and leaves R4's tree gate without an ancestry source on the daily path ([41 B2]: the trunk and every active lane head are newer than the commit-graph [M]). |
| SHA-256 images, bundles (v1.1) | **in release** | M4/M5. Hashing is parametrised by the object format; R4 reads blob ids in whatever format a project repository uses; bundles give an image transport that needs no remote. SHA-1 stays the default for anything that leaves the machine [D §12 S4]. |
| `op log` / `op restore` (v1.1) | **in release** | M3. |
| `--with-oplog` (deferred) | **in release** | M5; it is also the path of the RG10 upgrade drill (ii). |
| Recursive virtual merge base (v1.1 "if spurious conflicts") | **in release** | M3. With two LCAs that disagree on a key, the newest-LCA base takes one side's resolution silently (§3.4). |
| FTS tier 2 (v1.1, ≥ 20k nodes) | **in release** | M2. The owner's three-year scale is 0.3–0.5 M nodes [02 §12.6]; tier 1 costs 20–80 ms at 1e6 [AR §8.1]. The 20k threshold is a store parameter (M-5). |
| Schema strengthening migrations (later) | **in release** | M2. |
| `PostToolBatch` delta hook (v1.1, optional) | **excluded by decision** ([74 A17]) | Built-but-off with no trigger; revisit when agents act on stale state that a mid-turn delta would have prevented, measured by the guard-conflict rate. |
| Cross-store import rules (CL2, v1.1) | **in release** | M5. Needed for any import of another store's image (restore on a new machine). |
| `resource` mutex ergonomics (later) | **in release** | M2 (`task{work_kind = mutex}` with lease semantics). |
| Binary at a stable install path; code signing | **in release; unsigned at a stable path unless M0 item 11 shows that signing matters ([AR §11] #39, decided 2026-09-26)** | M11 (Defender rescans rebuilt binaries [05 §6.4], which the stable path addresses; M0 item 11 measures whether signing changes spawn-to-exit). |
| R4 accelerators: git per-commit rename evidence, the `PostToolUse` evidence hooks (`mv`/`rm`, `Write\|Edit`), git hook blocks | **in release** | M6 (git evidence), M9 (hooks); their on/off defaults are `config` keys ([AR §13]), not owner decisions. USN-journal replay (E2) and the move nudge are **excluded by decision** with revisit triggers, reservations kept ([74 A13, A17]). |
| R4 `links import` of existing citations ([40 §9.2] #12) | **in release ([AR §11] #23, decided 2026-09-26: import after the owner reviews the ambiguous list)** | M9 builds and rehearses it; it runs at the cutover ([74 A16]). |
| Leader, `watch` (M6 in [AR]) | **excluded unless M0 requires it** | Decided by M0 items 1–2 before M1: built in M1 only if the Defender close cost makes CLI commits > 20 ms or the 16-writer wait p99 exceeds 50 ms under G1 and group commit (the T2 revisit triggers [AR §2.2]). Never required for correctness. Group commit itself is not a leader feature: it is leaderless, in M1, on every OS ([AR §2.8], [80 §2.4]). |
| `rebase --onto` (deferred) | **excluded** | Not required by R1; it would change exported lane commit ids. |
| Tracked-directory and orphan-branch image destinations (deferred) | **excluded** | Owner decision #4 default is the separate bare repo; revisit if the owner wants PR review of the image. |
| `shared` field class (after one campaign) | **owner scope, before M0** | It changes view construction (M1) and the schema's field classes (M0). Default: isolation + markers, so `shared` is excluded; choosing it after the release is a format-version-2 change carried out with the procedure drilled in RG10. |
| Live cross-machine writers (decision #11) | **out ([AR §11] #11, decided 2026-09-26)** | Out unless the owner changes #11; `uid` identity is stored from M0, so adding it needs no migration of identities. |
| HTTP MCP mode | **excluded** | Only if non-Claude agents appear [07 §11]. |
| Reftable *writing* | **excluded** | Only the optional project-repo destination could need it; reading is in M4. |
| Network transport without git | **excluded** | `image push/pull` spawns git when present and prints the command otherwise. |
| Linux and macOS implementations of the OS layer (`Vfs`, `ProjectFs`, `LockBytes`, environment guard, metering), their binaries, runners and crash rigs | **port phase — designed now, unscheduled** (owner decision #32, 2026-09-26) | M0 specifies all three OSes and freezes every byte and rule they need ([80 §3]); M1–M11 build and gate Windows only; the port phase ([80 §5]) adds the implementations under their own gates and never changes format or protocol. The OS-layer lint (GT20 d) and the cross-target type check GT20 (e), a gate from M0 by owner decision #44 (no binary, no test), keep them additive. |
| tree-sitter in the product ([40 §9.2] #9) | **excluded by default (owner scope, before M0)** | Hand-written scope scanners; tree-sitter only as a test-only oracle for the Rust scanner. |
| LQ exclusions ([50 §1.3]: recursive rules, UDFs, regex, standing queries, valid time, `MERGE`, ISO GQL conformance, …) | **owner scope via [50], before M0** | [50] fixes the release language; the parser recognises the excluded keywords and names the LQ alternative. |
| `across ≤ 8 refs` in the query views ([16]) against `--across` over all refs (M3) | **resolved** | `across` is a counted budget with a resumable cursor over refs, and promoted refs are read through their `TOUCH` bitmaps; M3's all-ref form and M7's relation are one mechanism ([61 m-2]; [50] owns the wording). |

### 1.4 The critical path

```
lane A:  M0·A contract ─► M1 storage ─► M2 graph core ─► M3 version control ═ R1 ─► M6 file-link runtime ═ R4 ─┐
                               │ (Vfs certified)                                                                    │
lane B:  M0·B model, LQ-Bench, FL-1 ─► FL-2 ProjectFs ─► M4 git object layer ─► LQ-1 front end ─► LQ-2/4/5/6 ─► M5 git image ═ R3 ─┤
                                                                                  (after M2)          (after M3, M4)                   ▼
                                                              M11 release ◄─ M10 MCP ◄─ M9 agent interface ◄─ M8 CLI ═ R2 ◄─ M7 query language ═ R5
                                                                                                             (both lanes)        (both lanes)
test workstream: fault-model simulator, crash enumeration, kill loops, fuzzers, mutation testing,
                 reference model, differential harness — from M0, gates per §3.13 (the OS-crash loop after the release)
```

With one lane the same order is followed serially: M0, M1, M2, M3, M4, M5, M6, M7, M8, M9, M10, M11.

---

## 2. Component dependency graph

### 2.1 Components

Component numbers follow the milestones that build them. The R4 layers FL-0…FL-9 are [40 §8.1]'s (revision 1's FL-10 — USN and git evidence — is part of FL-4 since revision 2), placed as [40 §8.1] and [41 §5] require; the LQ packages are [50 §8.2]'s.

| Component | Final-specification content (source) |
|---|---|
| **C0 contract and evidence** | on-disk format v1 incl. every reservation (§2.5); the `Vfs` fault model; store parameters; canonical commit form [AR §4.6] and the gate-0 carrier table; `.moi` v1 ABNF [AR §5b.2] incl. [40] R-11 and [50] F3; the logical `Store` API; the frozen query surface (grammar, error table, shapes, standard-library signatures, skill card) [50 §2–§7] after LQ-Bench; the measurement protocol and results; the reference model (§4) and the format oracle; FL-0 and FL-1 [40 §8.1]; the test infrastructure |
| **C1 storage engine** | `LOCK`/`HEAD`/log/segments/overlay/checkpoints/rollup/`hist`/blobs/dictionary [AR §4] with the product codec; the multi-process protocol with G1, G2, G9–G11, G14, G18, G25 [AR §4.5, §6] as settled by the M0 specification review; refs, pins, `ClientHead`, per-ref chain index (G15), view construction `pin ⊕ op windows` incl. sync-by-reference expansion (G17), promotion (G16) [AR §5a.3]; the section-producer and record-fold registry (§2.6); GC with reachability over refs, reflog and pins [AR §4.9]; `backup`/`restore`/`repair --rebuild-from-log`; `doctor --fsck`; the optional leader only if M0 requires it |
| **C2 graph core** | 13 kinds, schema as data with weakening and strengthening [AR §2.12]; the closed field-type set incl. R4's types; edges and delete policies [AR §3.3]; invariants I1–I14, I14′, I17′, I32′, I36′ [AR §3.4]; I5′ with Pearce–Kelly; derived state [AR §3.5] with the persisted structural predicate [50 §3.8]; status machines; leases and fencing; idempotency; change feed; markers produced by ops (CB3); the role write-policy enforcement point; FTS tiers 1–2; `doctor --verify`; **FL-3** per [40] (file nodes, identity derivation, `at` edges with anchors and the discriminator, path/alias/anchor indexes, the `suspect` extension, I-F1–I-F3, I-F8, I-F9); the [50] graph-side producers (F4, F5, F6, F7, F12, F15) |
| **C3 version control** | ref kinds and write masks; branch/checkout/HEAD resolution chain; reflog, `undo`, `op log`/`op restore`, tags; `log`/`diff`/`show@`/`blame`/as-of; typed three-way merge with the recursive virtual base; sync-first merges; per-pair staging; validators in I37′ order; `resolve`/`--continue`/`--abort`; `merge-check`; `revert` (incl. `--mainline 1`)/`cherry-pick`; absorbed vectors (CM1); `--across`; branch delete semantics [AR §5a, §5d]; worktree bindings; **FL-7** per [40] (merge rules of the observation composite, path claims, prefix history, anchor add-wins, the re-key rule of [41 B1]); the [50] history-side producers (F8, F9, F10, F11, F14) |
| **C4 git object layer** | SHA-1/SHA-256 object model; loose objects; pack v2 + idx v2 read (OFS/REF deltas) and write; commit-graph read incl. split chains and generation data; loose refs and `packed-refs` with git's `.lock` protocol and read-after-write verification (G21); reftable read; bundles v2/v3; tree read and diff; exact-rename detection; generation-pruned ancestry and merge-base; Windows hygiene (G21–G23) |
| **C5 git image** | [AR §5b] complete: tree layout, `.moi` codec generated from the ABNF, commit mapping and trailers, `gitmap`, side ref, export at both granularities with the durability order, import (native, foreign, two-parent foreign merges, divergent imports through `import/<ref>`, checkpoint as bulk commits), staging, `image doctor/show/gc`, `--with-oplog`, cross-store alias rules, transport through git when present (the `refs/moirai/*` destination is excluded, [74 A17]); **FL-8** per [40] |
| **C6 file-link runtime** | per [40]: **FL-2** `ProjectFs` (Windows implementation and deterministic simulator); **FL-4** runtime tables and records, `resolve` (pure read), settle (write), `link`/`unlink`/`file add\|mv\|rm\|relink\|revert\|where` as `Store` API commands, the crash-safe intent protocol and recovery, tree identity, the ancestry gate, the freshness and write rules and bindings ([41 M9]); git per-commit rename evidence (E6) over C4 (revision 1's FL-10, folded into FL-4 by [40] revision 2; the USN-journal reader E2 is excluded, [74 A13]); `ANCHORRES`, settle epochs, the settle CAS ([70], [72]) |
| **C7 query language** | per [50]: LQ-1 front end, LQ-2 binder, LQ-4 executor core, LQ-5 graph operators, LQ-6 planner, LQ-7 transactions, LQ-9 versioned queries, the standard library of named queries and named mutations, LQ-11 (named-query items in the image, `QueryCycle`); **FL-6** link built-ins |
| **C8 CLI** | the output contract of [AR §7.1] as amended by [50 §6] and [40 §6.1] (transport rules, bare ids, exit codes incl. 10); every verb of C1–C7 as a named query, a named mutation or a VCS/maintenance ritual; LQ-8 (`q`, `tx`); **FL-5** file verbs; the image verbs; discovery and placement [AR §2.14]; `init` guard; `doctor store\|lanes\|agents\|image`; `check`/`stale` |
| **C9 agent interface** | packs and brief [AR §7.4] incl. link rendering; hooks [AR §7.5] incl. the R4 accelerator hooks; skills incl. the `moirai-ql` card and the file-link card; `export md`/`memory-md`/`rules`; the lease-role identity plumbing ([90 §4.3]); import tooling for standing rules, pins, lanes and questions, and `links import`; **FL-9**, LQ-12 |
| **C10 MCP** | [AR §7.2] ten tools with `find` → `query` and `write` accepting `TX` [50 §6.3]; explicit `branch` validated against `lease`; stamp hook on write tools only (an `mcp_tool` handler on this server where connected); every hook's `mcp_tool` handler; a byte-bounded overlay LRU (G19, `mcp.overlay-bytes`); maintenance slices, no rollup in the server; `mcp --read-only`; plugin packaging; LQ-14; FL-9's MCP operations |
| **C11 release** | the release gate of §6 |

### 2.2 The graph

```
                     ┌──────────────────────── C0 contract and evidence ─────────────────────────┐
                     │   (format, fault model, parameters, model, LQ surface, FL-1, protocol)     │
                     ▼                                                                              │
                 C1 storage ───────────────┬─────────────────── (Vfs) ──────────┐                   │
                     ▼                     │                                    ▼                   │
               C2 graph core ◄── FL-1 ─────┤                           C4 git object layer          │
                     ▼                     │                                    │                   │
            C3 version control ◄── FL-1 diff                                    │                   │
                     │                                                          │                   │
          ┌──────────┼──────────────────────────────────────┬───────────────────┘                   │
          ▼          ▼                                      ▼                                       │
   C5 git image   C6 file-link runtime (C1–C4)      (ancestry built-ins)                            │
          │          │                                      │                                       │
          └────┬─────┴──────────────────────────────────────┘                                       │
               ▼                                                                                     │
       C7 query language (C2, C3, C4; C5 via LQ-11; C6 via FL-6)                                     │
               ▼                                                                                     │
            C8 CLI (C3–C7) ◄───────────────────────────────────────────────────────────────────────┘
               ▼
       C9 agent interface (C5–C8)
               ▼
            C10 MCP (C7, C9)
               ▼
          C11 release
```

### 2.3 Every edge, justified

| Edge | Why it exists (evidence) |
|---|---|
| C0 → all | The frozen format, the canonical form and the `Store` API are what every component reads and writes; the fault-model simulator and the reference model are what every gate runs on (P4, P5). |
| C1 → C2 | Graph ops are applied through the overlay and sealed into segment columns, CSR and frozen bitsets [AR §4.4–§4.5]. The section *contents* (`BM_*`, `TOPO`, `TERMS`/`POST`, `STATS`, R4's sections) are produced by C2 and later components, which is why C1 exposes them as a certified registry (§2.6) rather than hard-coding them [61 M-1]. |
| C1 → C3 | Refs, pins, per-ref chains, view construction and promotion are storage mechanics [AR §4.2, §5a.3]; their protocol surface must be inside M1's simulation, crash enumeration and kill loops (and the OS-crash loop once it runs, after the release), not added after ([20 F-D1, F-D3]). |
| C2 → C3 | The merge is typed per key over graph ops; validators are graph invariants; markers come from C2's op applier (CB3); FL-7 merges FL-3's data. |
| FL-1 (C0) → C2, C3 | FL-3 uses FL-1's path rules and identity derivation; the histogram line diff is shared by C3's diff3 and by anchors [40 FL-1] ([61 m-7]). |
| C1 → C4 | Loose objects, packs and refs are written with the Windows primitives of the `Vfs` [08 W3], [20 G21]. Only the certified `Vfs` is needed, so C4 may start once M1's `Vfs` certification point passes (§3.2). |
| C3, C4 → C5 | Export/import map commit kinds, merges, syncs and staging (C3); object I/O is C4. C5 needs nothing from the query language or the CLI: the image verbs are packaging and land with the CLI ([61 M-2d]). |
| C2, C3 → C5 | FL-8: anchor lines, identity derivation on import and the `IdCollision` exemption are C2/C3 data and rules [40 FL-8]. |
| C1, C2, C3, C4 → C6 | FL-4 needs the commit path and runtime records (C1's registry), file nodes and anchors (C2), bindings, the write rule and the freshness rule (C3), and in-process ancestry and git evidence (C4) — the tree gate has no other ancestry source on the daily path [41 B2, M9]. |
| C2, C3 → C7 | The binder reads schema as data (C2); built-ins share code with derived state so a query and a verb cannot disagree [50 §1.4]; `USE` views, history relations and `RESOLVE` are version-control objects (C3) [50 §3.9–§3.10]. |
| C4 → C7 | Ancestry built-ins (weak edge). |
| C5 → C7 | LQ-11: named-query items travel in the image and are validated by `QueryCycle`, which C7 registers in C3's validator table; GT8 is re-run with named queries [50 §8.2]. |
| C6 → C7 | FL-6: `link_state()`, `links()` and path lookups evaluate C6's resolver [50 LQ-13], [40 §6.5]. |
| C7 → C8 | The CLI read verbs are named queries of the standard library and write verbs are named mutations [50 §4]; the transport rules come from [50 §6.1–§6.2]. |
| C3, C4, C5, C6 → C8 | Every VCS ritual, `check`/`stale`, the image verbs and the file verbs (FL-5) are CLI verbs; `--branch`/`--lease`/binding resolution is a C3 rule. |
| C5, C6, C7, C8 → C9 | Packs render file links and settle at `SessionStart` (C6, FL-9); brief and packs are named queries (C7); the post-merge export hook calls C5; hooks are CLI invocations (C8). |
| C7, C9 → C10 | `query` and `write(TX)` tools; the `brief`/`pack` tools and the stamp hook are C9's; the role policy is keyed on the role of the lease the caller presents, which C9's leases and markers carry ([90 §4.3]). |
| all → C11 | The release gate covers the whole system. |

Removed from issue 1: "C6 CLI → C7 image" (packaging only) and "C7 image → C8 file links, nothing in C7 depends on C8" (false: FL-8 lives in the image [61 B1]).

### 2.4 Where this order differs from the suggested one, and why

The suggested order was *format → storage → graph core → query language core → CLI → version control → git image → file links → agent interface → MCP*.

1. **Version control moves before the query language and the CLI.** [50] makes `USE` revisions, `diff`/`history`/`blame`/`conflicts` relations and `RESOLVE` part of the language, and every operator view-agnostic; a query language built before version control could not be completed or gated against replay from genesis. Version control needs no query language: it is driven and gated through the `Store` API against the reference model. The CLI's contract includes revision syntax and every VCS ritual, so it comes after both.
2. **The git object layer becomes its own leaf component (M4).** Three consumers need in-process git reading: the image (R3), `check`/`stale` ancestry (CM7) and R4's tree gate and rename evidence [41 B2]. [AR]'s plan had `git fast-import` first and a hand-written layer later behind an `ImageBackend` trait — a throwaway stage by construction.
3. **The physical half of branching moves into the storage engine (M1)**, so the multi-process protocol is certified once, with refs, pins, promotion and GC inside its simulation, crash enumeration and kill loops (the OS-crash loop follows after the release).
4. **The image comes right after version control and the object layer (M5), before the query language and the CLI.** It needs nothing from either (its verbs land with the CLI), image determinism is the second long pole [22 §7.1], and gate 0 proves the canonical form before three more milestones build on it [61 M-2d].
5. **File links are not one late milestone.** R4's layers attach to their hosts: FL-0/FL-1 in M0, FL-3 in M2, FL-7 in M3, FL-8 in M5, FL-2/FL-4 (with E6; E2 excluded by decision, [AR §11] #41) in M6, FL-6 in M7, FL-5 in M8, FL-9 in M9/M10 [40 §8.1], [41 §5]. Built as one milestone after the image, R4 would have had to modify five certified milestones [61 B1]. The runtime (M6) precedes the query language because link built-ins evaluate its resolver.
6. **The evidence that can change frozen bytes is gathered before the freeze (M0).** The query accuracy benchmark (GT13), R4's replay corpora, the T1/T2 trigger quantities and the gate-0 carrier table each can change the format; issue 1 had them at M5, M8, M1 and M7 [61 M-2]. M0 therefore carries the complete reference model (on whose own parser, binder and evaluator LQ-Bench runs, the owner review of 2026-09-27, A6), FL-1 (on which the corpora run) and the layout probes.
7. **The agent interface stays ahead of MCP (M9 before M10)**, as the suggested order had it, although the owner's brief names the surfaces CLI → MCP → skills. The MCP server's `pack` and `brief` tools and its `mcp_tool` hook handlers serve the pack algorithm, brief and hook logic that M9 builds; an MCP server built first would need that logic built inside it or stubbed. M9 therefore certifies packs, briefs, hooks and skills on the CLI's command transport, and M10 adds the MCP transport and certifies the `mcp_tool` hook budgets, `files.hooks.edit-evidence = auto` and the default `hooks.transport = auto` ([AR §9]). Nothing is used before the release gate, so the order changes which gates run first, not what the owner receives.

### 2.5 What M0 freezes

Frozen at M0 exit as **format version 1**; readers refuse a newer version; nothing auto-migrates on open [AR §12]; `moirai migrate` exists only for schema strengthening (data). Rows marked **per [40]** or **per [50]** are those designs' reservations; this file carries no other text for them (P9).

**Rows from [AR]:**

| Area | Frozen content | Reserved for |
|---|---|---|
| `LOCK` | layout v1 ([80] X-F1): lock bytes beyond EOF at 2^62 + {0 writer, 1 leader (only if built), 2 maintenance, 3 quiet-advisory, 4 flush} and 2^62 + 2^16 + i for 256 liveness slots; `LockHdr` at 0, writer diagnostics at 2048, leader record at 3072, slot records at 4096 (F-A7, [72 B2]) | leader, ports |
| `HEAD` | two 4 KiB slots, G18 layout incl. `refs_lsn`/`pins_lsn`/`heads_lsn`/`markers_lsn`, `image_cursor[4]`, `seq_ring`, `flags` bits 0–2; the store parameters of the row below; all other bits reserved-zero | R1, R3 |
| Log | extents that read as zero beyond the tail, created per [80 §2.3.3] (zero-filled on NTFS, G11), of a size that is a store parameter (default 64 MiB); 32 B `RecHdr` with lsn, epoch, xxh3 (X2); the `group_end` record's 8-byte chain trailer, groups never spanning extents ([80] X-F3); record kinds `Commit`, `RefUpdate`, `ClientHead`, `Lease`, `Marker` (settled/deleted/cleared), `Idem`, `GitMap`, `Pin`, `Checkpoint` (per-ref lsn lists, promotion entries), `RefTable`, `Lazy`, `Noop` | R1, R3 |
| Commit body | [AR §4.3] incl. `ref_old`, `prev_on_ref`, `ref_seq`, `sync_base`, absorbed vector, `foreign_git`, `verified`, `import` | R1, R3 |
| Ops and values | every op of [AR §4.3] with before-images and `prev` deltas; value encodings for the closed type set {bool, int, counter, f64, enum-with-lattice, text, set, ref, commit-ref} | R1 |
| Canonical form | [AR §4.6] items 1–10 | R1, R3 |
| Segments | `SegHdr` (with `total_len`, [80] X-F6) and every section of [AR §4.4] incl. `REFS`/`PINS`/`HEADS`/`MARKERS`, `EDGE_PROPS`, `TERMS`/`POST`; the promoted-branch segment and `TOUCH` bitmap; `hist` frames with commit index; `blobs` frames and dictionary; `gitmap` pages | R1 |
| Schema as data | kinds, fields (type, lattice, class), edges (class, policy, acyclicity, cardinality) | — |
| Image format v1 | `.moi` ABNF, `.moirai-image` marker, trailer set, side-ref layout [AR §5b] (an image format bump is a new destination, never a store migration) | R3 |
| Store layout | directory contents, the `config` syntax, precedence and unknown-key rule (not the key set, [AR §13]), pointer-file format (`moiraidir:`, `store-id:`) [AR §2.14], as decided by owner decision #17; store file names from decimal numbers and fixed ASCII words ([80] X-F10); the per-OS user-scope config locations ([80] X-F11) | R2 |

**New rows (issue 2):**

| Area | Frozen content | Source |
|---|---|---|
| **`Vfs` fault model** | Part of the format specification; the in-memory `Vfs` enforces it and every crash gate runs on it: **(1)** after a crash, any subset of the 4 KiB sectors written to a file since its last successful flush may be lost (reverting to their previous content — zeros in a fresh extent), and one sector per file may be torn at 512-byte granularity; **(2)** `sync(Data)` (`NtFlushBuffersFileEx(DATA_SYNC_ONLY)`, `fdatasync`, `F_FULLFSYNC`) makes durable the file's data within its current size, and `sync(DataAndMeta)` also its size and allocation; **a create, rename or unlink is durable only after `sync_dir` of its parent, and before that any subset of the unsynced metadata operations may be lost, in any order** (the NTFS prefix rule is dropped, [80 §2.3.5]); **(3)** a failed flush leaves the content of its range — and of any unflushed range of the file — indeterminate forever: later reads may return old or new bytes, may change between reads (pages reverted, invalidated or evicted), and a later successful flush proves nothing about them (ATC'20 [08 §8.1], [80 §2.3.5]); **(4)** a read concurrent with a write of the same range may return any mix of old and new sectors; **(5)** any write — overwrites of written or zero-filled ranges included, as on copy-on-write file systems — any flush and any file creation may fail with disk-full; **(6)** any process may pause for any length of time between two `Vfs` calls; **(7)** the wall clock may step backward or forward by any amount between two calls, a monotonic clock never goes backward; a boot clock, monotonic and including suspend, serves lease deadlines, and a process may be unable to read its boot identity (Unknown-boot mode); **(8)** lock release after process death may be delayed by an unbounded amount on every OS (crash reporters keep a crashing process alive; the Windows distribution after `TerminateProcess` comes from M0 item 12); sharing violations (errors 5/32) and delete-pending files as in [AR §4.10]; **(9)** a read through a mapping of a sealed file may end the process (external truncation, media failure) — a crash at that point; **(10)** another process may truncate or rewrite a store file it has permission for; sealed files are read-only on disk; **(11)** several processes may have pending groups, any may die before a flush, the flush holder may die before, during or after its flush, with or without an error, and a failed flush may be followed by reverted, invalidated or evicted pages while others append and retry; **(12)** a read may fail (`EIO`, a checksum error): below `durable_lsn` it is corruption, above it the end of the log under the chain rule ([80 §2.3.5]) | [61 B2]; [20 §2]; [08 §8]; [80] |
| **Protocol decisions** taken in the M0 specification review under the fault model | at least: (a) adopting a record after a failed flush re-writes its bytes (or verifies them through an unbuffered read) before flushing — re-flushing alone proves nothing; (b) recovery that finds valid records of the same epoch beyond a bad one stops and reports `repair` instead of silently truncating acknowledged commits; (c) every file deletion, extent retirement or recycling is preceded by a durable `HEAD` barrier (`durable+meta` on `HEAD`, outside the writer byte, naming a state that no longer needs the file), off the commit path; (d) a lock-free reader that sees a torn `HEAD` slot uses the other slot; (e) lease TTLs, the HLC and the GC grace are specified against the monotonic clock where the fault model's clock steps would otherwise break them; (f) `ERROR_DISK_FULL` on any write aborts the command without acknowledging it and leaves only files the orphan sweep removes. The review may add others; each becomes a seeded bug (§3.1). Added for the ports: (j) the lock contract and order of [80 §2.2] (in-process ownership decided in user space first; slot < leader < maintenance < flush < writer; waits only on role bytes, only upward); (k) leaderless group commit through the flush byte with chained group validity, scan and re-write of the pending range under the writer byte, read-modify-write publishes and acknowledgement by identity, invariants I-G1–I-G6 ([80 §2.4]); (l) the `HEAD` barrier flushes outside the writer byte, only after maintenance's own `Checkpoint` is published; (m) the mapping policy and the crash-gated environment guard ([80 §2.5–§2.6]) | [61 B2]; [80], [81] |
| **Store parameters** | every threshold that switches a code path is a format-visible parameter — init-fixed ones in `HEAD`, tunable ones as `config` keys taking effect at the next checkpoint or decision point ([74 A10]) — never a compile-time constant: extent size, checkpoint thresholds (ops, bytes, body bytes), the quiet-mode cap, tiered-fold width, promotion thresholds (overlay ops incl. synced windows, overlay bytes, age), `hist` frame size, FTS tier-2 node threshold, the Kahn fallback edge count, the `suspect` closure budget, the loose/pack threshold, the idempotency and reflog retention windows. Production defaults are fixed by M0 measurements; a **test profile** sets them tiny (e.g. tier 2 at 16 nodes, checkpoint at 8 ops, 64 KiB extents, promotion at 16 ops, Kahn at 4 edges) so the reference model sees every threshold-gated path | [61 M-5] |
| **Gate-0 carrier table** | canonical item → carrier (tree path or trailer) for every hashed item, with a fixture per commit kind (ordinary, merge, sync, revert, cherry-pick, foreign, import-checkpoint), including R4's and R5's items as [40] and [50] define them | [61 M-2d] |
| **Derived-state semantics** | the persisted bitset holds only the structural predicate (named `unblocked` in [50 §3.8]); `ready` is the structural predicate plus the runtime clauses (leases, markers, `defer_until`), applied at read time and valid only at a tip; every commit's `affected` list names every node whose value of any derived predicate changed ([50] F15) | per [50] |

**Rows added by the priority audits (2026-09-26; [AR §4.6] places each; [70]–[74]):**

| Area | Frozen content | Source |
|---|---|---|
| `HEAD` | `durable_lsn`, `boot_id`, `config_gen`, flag bit3 `retired`; the boot-change recovery rule and the two-slot barrier ([AR §4.2]); the boot-identity rule and Unknown-boot mode; every publish a read-modify-write of the newest slot ([80] X-F2, X-F3) | [72 B1, M2, m6], [74 A10] |
| `LOCK` | 36 KiB, laid out anew by [80] X-F1: 256 slot records × 128 B at offset 4096 with kind, nonce, primary and alias session hashes, `ProcId`; lock bytes beyond EOF | [72 B2], [74 A01], [80] |
| Log | `RecHdr.flags` bit1 `group_end`; validity = sane length and kind ∧ epoch ∧ `lsn` = position ∧ xxh3, and per group the chain trailer matching the preceding group's ([80] X-F3, [81 B1]); record kinds `AnchorRes` (lazy), `Backup` (durable), `SessionMark` (lazy); `TreeReg` epoch and dirty-row payloads | [72 M1, M9], [70 S5, S7, S8], [73 F4] |
| Commit body | `actor u32` (also `CREATOR`); `changeset_digest`; `cs_ref` for bulk commits; the inline bound `store.commit.inline-max-bytes` | [71 RAM-B1, RAM-m6], [70 S2] |
| Segments | `seg_kind = changeset` (`cs.NNNN`); the section-entry flag `derived-optional`; `MARKERS`/`MARKERS_OLD` with the marker key `(#N, ref_id, commit)`; `LEASES` with the holder anchor, the deadline form `{wall, boot_hash, mono}` ([80] X-F2) and captured `files_owned` globs; `ALLOC` with uid and `UIDX`; `ANCHORRES`, `GLOBIDX`, the `TREES` epoch list and dirty row; `hist` frames ≤ 256 commits and ≤ 1 MiB raw | [70 S4, S5, S7, S17], [71 RAM-B1], [72 B2, M4, M7], [74 A23] |
| Ref table | `overlay_ops`, `overlay_bytes` | [70 S1] |
| Schema | the `lane` kind without a versioned dirty count | [70 S5] |
| Canonical form | item 10 hashed through `changeset_digest`; inside a uid, key classes in `.moi` line order; anchor quote/prefix/suffix only as BLAKE3-128 digests | [70 S2], [71 RAM-B1], [72 M6] |
| Image format v1 | digests on every `anchor` line, text in `full` mode verified on import, the `text-unavailable` sub-state | [72 M6] |
| `Vfs`/`ProjectFs` | the four durability classes with `sync_dir` and `sync_group` and their per-OS calls; `rename_noreplace`, `rename_replace`, `swap_dirs`; the amended fault model in both simulators; `VolumeCaps`, `OsFileId`, `JOURNALCUR` and `DIRMAP` ([80] X-F5, X-F8) | [72 M8], [80] |
| Resolver constants (R-14) | E3d exact only on a file-id, size + mtime or `oid` match; a present path with a changed file id → `ambiguous (path reused)`; the E6 window bound is a constant, not a key; per OS ([80] X-F8): `fold_v1` at Unicode 17.0.0, the twin rule, the unique-creation-time copy rule, whole-`OsFileId` identity (on Linux with the file-handle digest), denials as `Unknown` | [72 M13], [80], [81] |
| Configuration | the git-config syntax, the precedence, the unknown-key rule, the registry format — **not the key set** ([AR §13]) | [74 A10] |
| **Cross-platform** (owner decision #32) | [80 §3] X-F1–X-F12: `LOCK` v1; anchors, `ProcId`, the boot-identity rule and Unknown-boot mode; the boot-clock deadline; group commit with chained group validity; the lock contract; durability classes and fault-model amendments; the mapping policy (`total_len`) and the crash-gated environment guard; the path canonical form; the R4 tagged runtime layouts and per-OS resolver rules; named-query file names and the ref-name rule; numeric store file names; the per-OS user-scope config locations (`lock.flush-wait-ms` and the other new keys are registered in [AR §13] at M0, not frozen: the key set is not frozen); the shell transport rules | [80], [81], [X17]–[X20] |
| **Harness-agnostic interface and pure Rust** (owner decisions #43, #44) | `LEASES` fields `kind`, `role`, `run`, `anchor` (session \| session-ttl \| none) and `bound`; the X-F2 amendment (the namespaced process-lifetime identity is hashed — the Claude Code session, the Codex thread; slots taken lazily; no anchor on another thread's slot); the unhashed commit-header byte `actor_src`; the output contract's byte units, both-ends, ASCII and `--ids` page rules; one error code and two exit-5 refusal texts; the card's display spelling; the codec chosen by M0 item 6 among pure-Rust options (codec byte values, frame formats, `dict.D` as raw content or absent, or a formatted zstd dictionary if item 6 chooses an own zstd-format encoder, [90 §11.3] option (3)) | [90 §10.1] |

The protocol decisions above are restated by the audits: **(a)** adoption **re-writes** every complete record in `(durable_lsn, end]` from the read buffer, then flushes — under group commit every flush holder does this, under the writer byte, before every flush ([80 §2.4.3]) — applies each record by kind, and adopts flushed groups all or nothing — the unbuffered-read alternative is dropped ([17 §3.5], [72 M1]); **(b)** an invalid record in `(durable_lsn, committed_lsn]` is the end of the log (a lost lazy tail); one below `durable_lsn` is corruption and reports `repair` ([72 B1]); **(c)** the barrier makes sure **both** `HEAD` slots name the post-deletion state and flushes `HEAD` once, outside the writer byte ([80 §2.3.2]), and recovery rebuilds the segment set from durable `Checkpoint` records if a slot names a missing file ([72 M2]); **(g)** the first process after a boot change recovers and flushes `HEAD` once before any read; **(h)** every rename or delete a durable record depends on is followed by a directory barrier, and `file mv` is a no-replace rename followed by `durable-name` on both parents on every OS (on Windows also with `MOVEFILE_WRITE_THROUGH` until measurement 17, deferred with the rig to after the release, shows it unnecessary; [72 M8], [80 §2.3.2]); **(i)** every write is three-phase: computed before the lock, re-validated by key and committed under it ([70 S2]). New store parameters: `store.commit.inline-max-bytes`, `store.tail.max-overlay-bytes` (+ `.quiet`), `store.tail.runtime-bytes`, `store.promotion.overlay-ops`/`overlay-bytes` (replacing the ops-since-fork and 8 MiB thresholds), `store.pack-objects-max`, `store.dict.train-sample-bytes`, `store.hist-frame-bytes`; tunable parameters are `config` keys, init-fixed ones live in `HEAD` ([AR §13]).

**R4 reservations — [40 §2.11], revision 2 (which settled every change [41 §5] required before FL-0), by row:**

| # | Reservation ([40 §2.11] is authoritative) | Where in [AR] |
|---|---|---|
| R-1 | value types `path` (root sym u16 + varint-length UTF-8, exact bytes), `oid` (algo u8 + 20 or 32 B; content ids, git blob ids and git commit ids) and `pathmove` (`{hlc u64, class u8, from path, to path, git oid-or-empty}`) in the closed type set | §3.1 field block |
| R-2 | the `artifact` field set of [40 §2.2], incl. `origin_path`, `origin_pred` and `observed_blob`; status values `planned`, `removed`; merge classes `observation` (composite) and `identity` (immutable) in the schema's merge-class enum; `area` fields `root` and `path_moves` | §3.2, §5a.7 |
| R-3 | schema column `uid_derivation ∈ {random, file-key, root-key, anchor-key}` per kind or record; the length-prefixed derivations (the anchor's with its predecessor term), the predecessor order by greatest candidate uid on the registering view, the view-scoped dead-uid re-derivation rule and the edge-complete merge re-key rule (the A1 re-review, S-01, S-03) | §2.12 schema-as-data |
| R-4 | edge kind `at` (historical); an optional 128-bit discriminator in the edge key; the anchor record layout incl. `captured`; op `SetEdgeProps{src, kind, dst, disc, old, new}` for repins (`AddEdge`/`RemoveEdge` carry the anchor as props) | §3.3, §4.3 |
| R-5 | the root node: its uid derivation and its two fields. **No op, no canonical-form item, no image trailer and no commit annotation** for directory moves ([41 B4]) | §3.2 `area` |
| R-6 | `HEAD.next_anchor u32` (the reserved `_ u32` after `next_id`) | §4.2 |
| R-7 | log record kinds `FsIntent`, `FsIntentDone`, `FsIntentAborted` (durable); `FileObs`, `Pending`, `FPrint`, `JournalCursor` (was `UsnCursor`), `DirMap`, `TreeReg`, `PrefixEv`, `GitFacts` (lazy) | §4.3 |
| R-8 | segment sections `PATHIDX` (sorted (root, fold(path), path) → `#N` for present or planned nodes), `ALIASIDX`, `ANCHORS` (sorted (src#, dst#, anchor#) → record), `ANCHOR_UID`, `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT` (`oid` → blob ref), `JOURNALCUR`, `DIRMAP`, `TREES`, `PREFIXEV`, `GITRENAMES` | §4.4 |
| R-9 | blob class "fingerprint" in `blobs.NNNN`, keyed through `FPRINT` | §4.1 |
| R-10 | the byte layout of the anchor selector block as an edge-property value inside canonical-form item 10's edge class, with the discriminator in the key; no new key class | §4.6 item 10 |
| R-11 | `.moi` grammar: `anchor` lines, artifact field names, `planned`/`removed`, the block encoding of `pathmove` sets, hash-only anchors | §5b.2 rule 9 |
| R-12 | invariants I-F1…I-F14 | §3.4, §5e.8 |
| R-13 | config keys `roots.<name>`, `files.*` (incl. `files.policy.auto`, `files.hooks.evidence`, `files.hooks.edit-evidence`, `files.cloud`, `files.scratchpads`, `files.deletion-inference`, `files.mv-git`, `files.confirm-roles`, `files.portable-names`), `image.dest.<name>.anchor-text`; `files.hooks.nudge` and `files.usn` were dropped with the features they switched ([74 A13, A17]; a later E2 build brings `files.journal`, [AR §11] #41); [40 R-13] is authoritative | §4.1 `config`, §11 |
| R-14 | the resolver-version constant table (thresholds, the `is_text` rule, the case fold, the window-hash function, the never-candidate patterns, the 50 ms quiescence, the git-window bounds) as a spec appendix, incl. `fold_v1` and the per-OS rules of [80 §2.11.4] | new |
| R-15 | binding-row extension: expected git ref and base commit per designated tree; binding uniqueness (I-F12) | §5a.4 |
| R-16 | the frozen state, detail and header strings (incl. `replaced`; `spelling differs on disk`, `normalization differs on disk`, `ambiguous (normalization collision)`, `unrepresentable path`, `missing (not representable on this OS)`, [80 §2.10–§2.11]) | §7.1 output contract |
| R-17 | the `relink` provenance vocabulary, with `agent/*` distinct from `owner/*` and `confirmed/*` | new |
| R-18 | the `FILEOBS` row layout (the tagged `OsFileId` with the parent-directory id, ns timestamps with granularity, attributes, `verified_at`; [80 §2.11.2]) | §4.4 |

Section numbers inside the rows refer to [AR] after the integration of 2026-09-26. The A1 re-review of 2026-09-27 (M0 WP-80a) changed R-1, R-3, R-4, R-6, R-7, R-8, R-10, R-11, R-13, R-14, R-16 and R-17 in [40 §2.11]; only R-3 is restated above, and every chapter works from [40 §2.11], which is authoritative. Issue 2 had carried [40]'s revision-1 rows R-1…R-14 — value type `digest`, an op `PathPrefix`, a canonical-form item 11 "path-prefix moves" with the trailer `Moirai-Path-Prefix`, invariants I-F1…I-F11, fewer record kinds and sections — with a column of changes [41 §5] required; [40] revision 2 made them all, and the integration adopted its rows. **There is no canonical-form item 11 and no R4 trailer**: directory-move history is the versioned `path_moves` field ([41 B4]; [AR §4.6]). The type is named `oid` (the layout `digest` described), because [40]'s field names and git's vocabulary use it.

**R5 reservations — [50 §8.1] F1–F18 (revision 2), by row:**

| # | Reservation (summary; [50 §8.1] is authoritative) | Where |
|---|---|---|
| F1 | schema `edges` rows gain `lq_name`, `src_kinds`, `dst_kinds`, `symmetric`, `reverse_names`, `reading` | schema tables |
| F2 | schema `fields` rows gain `optional`, `default`, `index`, `sort_rank`, `coerce` | schema tables |
| F3 | schema `QUERIES` table, `Schema{weaken, query}` op, image file `schema/queries/<q>.moi` (q = hex BLAKE3-256 of the name, [80] X-F9) | schema, log, image |
| F4 | cold column `CREATOR` (actor, role) | segments |
| F5 | section-tag ranges `FCOL.<field>` and `FIDX.<field>` for indexed fields | segment header tag space |
| F6 | per-chunk cardinality in the frozen-bitset chunk index; per-bitset totals | bitsets |
| F7 | `STATS` section (degree statistics per edge kind and direction; per-field presence and distinct estimates) | segments |
| F8 | `hist` frame header `first_seq`, `last_seq`, `first_append_hlc`, `last_append_hlc`; overlay `seq → lsn` | `hist`, process |
| F9 | `append_hlc` per entry of the per-ref lsn lists in `Checkpoint` records | log |
| F10 | commit header, unhashed: `stmt_origin`, `stmt_sym`, `stmt_hash` | commit record |
| F11 | `CONFLICTS` section | segments |
| F12 | FTS `DOCLEN` cold column and a tokenizer version byte | segments |
| F13 | `PATHIDX` — R4's section (R-8), which [40] owns | segments |
| F14 | commit header, unhashed: `append_hlc`, monotonic in seq order | commit record |
| F15 | invariant: `affected` names every node whose value of any derived predicate changed, or the commit has `affected_complete = 0` | engine |
| F16 | commit header: `affected_len` widened to u32 and an unhashed `affected_complete` flag | commit record |
| F17 | store-wide `ALLOC` index `#N → (ref_id, create_seq)`, store-level runtime, rebuilt from the log | segments + tail records |
| F18 | violation classes `QueryInvalid`, `QueryCycle` and the named-query merge validator | violation codes |

Owner calls that shape these tables were decided on 2026-09-26 as recommended (§3.14): erasing history ([16 §11 Q5]; no erase — bodies may be dropped by hash without changing commit ids), the parser dependency for anchor scopes ([40 §9.2] #9: hand-written scanners), and the R4/R5 decisions listed there.

### 2.6 Extension points certified early

P10 requires that later milestones extend certified ones only through extension points certified earlier. There are five, each a table inside one implementation, never a swap point:

| Extension point | Certified in | Entry | Registered later by |
|---|---|---|---|
| **Section-producer and record-fold registry** | M1 | tag, layout class (column, sorted table, ± list, bitset from ± lists, blob class), fold over an op window, seal into a section, rebuild from the log, verify against a recomputation; for record kinds: durability class, fold target, export rule | M2 (`BM_*`, `TOPO`, `TERMS`/`POST`, `STATS`, `CREATOR`, `FCOL`/`FIDX`, `DOCLEN`, FL-3's `PATHIDX`/`ALIASIDX`/`ANCHORS`/`ANCHOR_UID`, `ALLOC`), M3 (`CONFLICTS`, frame and checkpoint timestamps), M6 (FL-4's `FILEOBS`, `PENDING`, `FSINTENT`, `FPRINT`, `JOURNALCUR`, `DIRMAP`, `TREES`, `PREFIXEV`, `GITRENAMES` and R-7's record kinds) |
| **Validator table** (I37′ order) | M2, M3 | validator id, position in the order, the rule, the violation class | M3 (merge validators, `PathClaim`), M7 (`QueryInvalid`, `QueryCycle`) |
| **Merge-rule table** | M3 | key class → typed rule → conflict class, as schema data | FL-7 in M3; M7 (query-item merge rules) |
| **Relation and built-in registry** | M7 | name, signature, view validity, the derived-state code it shares | FL-6 in M7 |
| **Named-query registry** | M7 | the standard library of [50 §4] as data | M9 (pack and brief classes, LQ-12) |

**How M1 certifies the registry without later semantics.** The storage driver drives generic physical producers of every layout class and **every reserved tag and record kind of §2.5** with synthetic payloads, so the byte-identical rebuild, the checkpoint, rollup and promotion budgets and GT1/GT3/GT4 already cover every section and record kind the frozen format contains (GT15 too, once the deferred rig runs after the release). The generic producers are permanent test infrastructure, not product code. M2, M3, M6 and M7 register the real producers and, as their own exit criterion, re-run the byte-identical rebuild, the M1 budgets and the crash gates with the real sections [61 M-1].

---

## 3. Milestones M0–M11

Conventions. **Scope** is the component at its final specification. **Not built yet** names what later milestones add and what this milestone must not anticipate with temporary code. **Exit criteria** concerning time, memory or Windows behaviour are measured on the owner's machine under the protocol of §5.1 (Defender on, idle and under the replayed 16-agent load, in agreed windows); everything else runs nightly on the same laptop in owner-granted agent-free windows (profile L, [AR §11] #34) and, with short seeds on synthetic data, in PR-level CI on hosted GitHub Actions Windows runners ([AR §11] #36; Windows only, owner decision #32). **Gates** refer to the catalogue of §3.13; a gate listed as mandatory stays mandatory on every later change. **Re-certification** (P10) means re-running an earlier milestone's byte-identical rebuild, budgets and crash gates with the real producers this milestone registers. **Size** is in [22 §7.1] units (A = 100) for the build plus the milestone's share of the test workstream; weeks convert at 5–8 units per week (§7.1: an unmeasured rate, re-measured at the M0 and M1 exits).

### 3.1 M0 — Contract and evidence (68.5–95.5 units with every delta of §7.1; 8.5–19 weeks one lane, 4.5–10.5 two lanes, plus profile L's exit windows, §7.1)

**Entry.** [40] revised in response to [41] — its blockers B1–B4 and the reservation changes of [41 §5]; [50] independently reviewed, with its edit lists for [AR] and for this file agreed; owner decision #32 decided, and every decision due before M0 decided on 2026-09-26, #34 (profile L, needed at M0 entry because provisioning the infrastructure, item 7, starts then) included (§3.14); [AR] with the normative [40], [50], this file, [80] and [90] approved by the owner as the M0 specification (the owner review of 2026-09-27, A4); the two lanes' build windows agreed with the owner at M0 start (A8). The re-review of [40] and [50] revision 2 is not an entry condition: by the owner's decision (A1) it is part of the independent specification review of item 8 and closes with zero open blocker or major findings before the format freezes. *Status on 2026-09-26: [40] revision 2 answers [41]; [50] revision 2 answers its review [51]; both edit lists were applied by the integration; neither revision has been re-reviewed yet. Status on 2026-09-27: the specification is approved (A4); the re-review runs in item 8 (A1).*

**Scope, lane A (contract and harnesses).**
1. **The on-disk format specification v1**, byte-level, covering every row of §2.5 — including the `Vfs` fault model, the protocol decisions, the store parameters and the gate-0 carrier table — with golden byte fixtures **written by hand in hex from the specification text**, by an author who writes neither the format oracle nor the product codec, for every record kind, op, value type, segment section and canonical-form case (every commit kind: ordinary, merge, sync, revert, cherry-pick, foreign, import-checkpoint); the `.moi` v1 ABNF with golden files for every node kind, a tombstone with flagged and historical edges, scalar and body conflicts, ledger lines, block strings, file nodes and anchor lines per [40], and named-query items per [50]. The **recursive-virtual-base addendum** to [AR §5a.7] (§3.4) is part of this specification. So is the **three-OS OS-layer specification** of [80 §2] with every item [80 §3] freezes (X-F1–X-F12), including the per-OS mapping tables as an appendix.
2. **The logical `Store` API**: typed commands (the semantic core of every write, VCS and maintenance verb), typed results in the `--json v1` data shape, a `state(ref)` snapshot, an injected deterministic clock.
3. **The `Vfs` seam** (with the durability classes, `sync_dir`, `sync_group`, `rename_replace`, `swap_dirs`, `LockBytes` including the flush byte and the in-process grant table, `map_sealed` and the environment guard) and its in-memory implementation enforcing the fault model, namespace item (2) included; the **crash enumerator** (at every crash point: every subset of a file's unflushed sectors while there are ≤ 12 of them, ≥ 10⁴ random subsets beyond, both `HEAD` slots exhaustively, bounded products across files, "flush error, more commits, crash" sequences, and several pending groups and flush holders with failed flushes followed by reverted, invalidated or evicted pages while appends continue (fault-model items 3, 11, 12); per-file prefixes plus one torn sector remain the fast PR tier, [72 M3]) with post-crash read-freshness and marker/lease assertions; the Windows implementation's skeleton.
4. **Harness validation.** The enumerator runs on a toy log with **≥ 12 seeded protocol bugs** — among them a checkpoint/reset race of the SQLite WAL-reset class [08 §3.1], a torn ref move (N3), a reader past `committed_lsn` (F-A1), idempotency evaluated before republish (F-B7), **adoption by re-flush after a failed flush**, **deletion of a file before a durable `HEAD` barrier** [61 B2], and the audits' additions — trusting a lazily published `committed_lsn`, a reader serving a pre-crash view, a single-slot barrier, a commit adopted without its group's markers, a skipped non-commit record, a rename without a namespace barrier, a same-epoch record at the wrong position ([72 B1, M1–M3, M8]), an intent roll-forward without the re-barrier ([40 §3.4], the A1 re-review A1P-02) — and the thirteen group-commit bugs of [80 §2.4.4], and must find every one. A harness that finds nothing proves nothing.
5. **Measurements 1–16 and 18–22** (§5.2; item 17, the rig calibration, is deferred with the rig to after the release) under the **measurement protocol** of §5.1, which M0 writes and freezes — item 7 now includes the Codex probes P1–P7, P10 and P11 of [90 §10.5], run with a test-only stub server; the layout probes (permanent micro-benchmarks); the 16-agent load fixture.
6. **FL-1, part 1** (product code at its final specification, [40] FL-1): path rules and case handling, EOL normalisation and `oid`, anchor capture and resolution, the Myers bit-parallel matcher, the histogram line diff shared with M3's diff3, the Rust/Markdown/TOML scope scanners, the gitignore matcher.
7. **Infrastructure** [61 M-7], in the laptop-only profile **L** ([AR §11] #34, decided 2026-09-26: no additional machine) with a public GitHub repository (#36; [74 A02, A06, A19]): **the moirai git repository**, public at `github.com/bluesteelll/moirai`; **every commit authored by the owner with no Claude or other AI co-author** — no `Co-Authored-By:` trailer naming Claude or another AI and no "Generated with Claude Code" line in a pull-request description (the owner's rule of 2026-09-26, [AR] binding inputs): the agents' harness attribution settings are off in the repository's configuration, and a `commit-msg` check in the local pre-merge gate and a PR CI check refuse a message or body that carries one — the rule binds from the first commit, which precedes M0 (the design documents); the attribution settings are off in the committed harness configuration (`.claude/settings.json`, approved by the owner on 2026-09-27, A2; the earlier commits carry no AI trailer) and the owner reviews each commit message until this item installs the check ([AR] binding inputs); **hosted GitHub Actions Windows runners** (free for a public repository) for the PR checks — the build, the lints (GT20 a, b, d), the cross-target type check GT20 (e), and the unit, property and differential suites with short seeds on synthetic data — never a timing, RAM, floor, crash or kill-loop gate, because the hosted images are Windows Server, not the gated Windows 11 + Defender profile; CI holds no model API key and no owner data; **owner-derived corpora and fixtures** (R4's replay corpora, the real-session LQ-Bench prompts, the register incidents, recorded HDRs, the 16-agent load fixture) live only in a gitignored local directory on the laptop — never committed, never sent to CI (#37) — with a pre-commit check that refuses its paths and any file whose hash its manifest lists; the owner reviewed what the tree publishes (the design documents quote him and describe his machine and workflow) and accepted it on 2026-09-27 (A3); **the laptop** (Ryzen 9 5900HS, 8C/16T, 16 GB, consumer NVMe without power-loss protection, the owner's Windows 11 feature version with the full build and Defender versions recorded in every measurement) runs the nightly jobs in owner-granted agent-free windows (≈ 5 a week from M1 exit; their number per week a recorded parameter), every timing, RAM and floor measurement, GT4 and GT17, and fuzzers sanitizer-off at ≤ 2 targets during agent work; the **OS-crash rig is not provisioned in M0**: the owner review of 2026-09-27 (V7, "No way to install it for now; such a deep test is postponed until the release, like mac and linux") deferred it to after the release together with the port phase — a **Windows Server 2025 Core evaluation guest** (build 26100, 180 days + one rearm, ≈ 2 GB, Defender on; one guest) in VirtualBox 7 with VMware Workstation Pro as the cross-check (the laptop runs Windows 11 Home, which has no Hyper-V; Windows Sandbox cannot keep post-crash state) with the host I/O cache off and guest flushes honoured (VirtualBox `IgnoreFlush 0`), an acknowledgement stream to the host, and bugcheck (Sysinternals NotMyFault) and hard power-off triggers, calibrated by measurement 17 with that image; the laptop's WSL2 and Memory Integrity choices (WSL2 puts VirtualBox on the slower Windows Hypervisor Platform backend, [80 §5.4]; Memory Integrity is a security setting, and the budgets are measured in the owner's state) and the guest-licence question (none is bought; if calibration failed with both hypervisors the rig would return to the owner) are deferred with it; §3.13 GT15 keeps the specification, and every format field and protocol rule the rig would test stays frozen; a sampled mutation-testing job; the compute and disk budget of §3.15. Build, CI, test and crash gates are Windows-only (owner decision #32): nothing in M0–M11 builds, runs or tests a Linux or macOS binary; the cross-target type check GT20 (e) (owner decision #44, [90 §11]), which builds no binary and runs no test, is a gate from M0 in the local pre-merge gate and PR CI; `rustup target add` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin` is part of this item.
8. **The independent specification review** by the three lenses of [20]–[22], covering the format, the fault model and the protocol decisions, the frozen query surface and the R4 reservations; it includes the re-review of [40] and [50] revision 2 (the owner review of 2026-09-27, A1) and closes with zero open blocker or major findings before the format freezes.

**Scope, lane B (oracle and evidence).**
9. **The reference model, complete** (§4): graph semantics, derived state, version control with merges and the recursive virtual base, markers, leases, idempotency, schema, the role write policy, R4 link intent with exact-evidence resolution over a simulated tree and a brute-force anchor search, **its own LQ parser, binder and nested-loop evaluator**, **an independent canonical-form encoder**, and the **format oracle** (an independent decoder of every frozen structure). Rule tables are data.
10. **LQ-Bench** [50 §7.4]: the fixture-store generator (on the model), 150 tasks × 3 phrasings with gold results, the ~30 real-session questions and the 40-task adversarial subset, the scorer, and a test harness that exposes the model through `q`/`tx`-shaped CLI and MCP-shaped tools; run as **GT13** on the model's own parser, binder and evaluator (item 9, LQ-3; the product's LQ-1 and LQ-2 stay in M7, the owner review of 2026-09-27, A6), on **Opus 5.5 only** ([AR §11] #38 (a), decided 2026-09-26; re-run when the gate model changes, [74 A14]) with the full 520 prompts for the baseline, the two gate-deciding ablations (D8, D11), the two alternative surfaces and the display-spelling ablation ([90 §8.1] L1), and a stratified 260-prompt half for the other ablations of [50 §7.4 item 7] — including BM25 against a statistics-free scorer, which decides whether `DOCLEN` stays in format v1 ([74 A15]) — plus the transport stratum in Claude Code and a scripted generic stdio client, both driven by Opus 5.5; no floor tier and no Codex arm (the `codex` client writes under the `unknown` profile until a later decision adds its model, [90 §8.2]); ≈ 53 M model tokens, mostly cached input, plus ≈ 1 M for the repeated 52-prompt sample — the neutral-API estimate, which weighs cache reads by price (≈ 80 M raw input tokens, est.), before Claude Code's per-call overhead (its system prompt and kept tool definitions), which the runner's first real calls measure, with the card's Claude token count, before any quota is requested; the ≤ 10-prompt smoke run re-issues the quota plan in raw input, cache-read and output tokens before the owner's quota ask ([50 §7.4] item 5, the A1 re-review A-M5) — **through the owner's Claude Code subscription in headless mode on the laptop — no API billing and no API key** (the owner review of 2026-09-27, V1, "I will not buy API access; only Claude Code by subscription is available"; the runner and what it changes are [90 §8.3]'s; ≈ $280 at list prices is only a reference), split across several usage windows inside M0 within the weekly limits and scheduled with the owner beside the lanes' own Opus use; if the quota is short, the prompt set shrinks by [50 §7.4] item 5's documented rule and each gate is reported with its sample size and statistical power, never skipped; the real-session prompts stay in the local corpus directory (#37) and go only to Anthropic, through Claude Code.
11. **FL-1, part 2**: sketches, token winnowing, similarity and containment — the half the model does not check (§4.2).
12. **The replay corpora** of [40 §8.3.4] on FL-1 — the five FL-1 targets of the exit criteria below; the three resolver-dependent corpora (the 34 delete-then-re-add events, the 46 worktree HEADs, the transcript census) are prepared here and first gated in M6, §3.7 (the owner review of 2026-09-27, A7): read-only walks of the owner's repositories, with git history extracted by the git CLI as a test-only data source.

**Decisions fixed at M0 exit (by measurement or benchmark).** Leader in or out of M1 (items 1–2; decided at the maximum in-lock cost item 2 sweeps, not at the toy log's own cost, and re-checked by M1's writer-wait and hold gates on the product — the A1 re-review A1P-03); T1's structure (item 14: Option A confirmed, or Option B chosen before the freeze); dictionary compression (item 6); deflate codec (item 9); lock-wait bound and lock-delay injection parameters (items 2, 12); the production value of every store parameter, with the checkpoint threshold chosen so that open at 1e6 with a full tail meets the ≤ 3 ms budget (item 10, whose replay applies records into a product-shaped overlay with the lazy-record mix at the quiet cap, loaded; M1's open gate re-checks it, A1P-03; the 4,096-op / 4 MiB default was estimated at 3–5 ms before the threshold is chosen, so it is lowered if needed, and [AR §4.7] states the gate at the chosen threshold, [61 m-9]); G15/G16 thresholds (item 3); the loose/pack threshold (item 8); the query grammar, error texts and card (GT13, including [50] D8's absent-value logic, measured by its ablation); R4's resolver constants (R-14) and anchor layout (replay corpora); `store.promotion.overlay-ops` from the daily-sync fixture (item 3); where bodies are compressed (item 6); the MCP runtime shape (item 19); the stamp route (the `mcp_tool` experiment, item 7); the default of `integrate.codex.store-writes` (probe P7 of [90 §10.5]); the codec among pure-Rust options and its dictionary form (item 6, #44, [90 §11.3]); the card's display spelling (LQ-Bench); BM25 or the statistics-free scorer (LQ-Bench).

**Not built yet.** Engine and product code other than FL-1. The M0 codec is the **test-only format oracle**; the product codec is written once, in M1, against the hex fixtures and the oracle — never a second product codec [61 m-4].

**Exit criteria.**
- The specification review — including the re-review of [40] and [50] revision 2 (A1) — closes with zero open blocker or major findings before the format freezes. The owner has **signed the model's rule tables** (merge table, status machines, delete-policy matrix, link merge rules, pack classes, and the state definition of I26′ with its marker-cache rules, [72 M4]) and **verified the GT10 core fixtures** (register incidents, the node-40 table, the [AR §7.6] walk-through).
- The three-OS OS-layer specification ([80], revision 2) passes the specification review with zero open blocker or major findings; M0 item 22 has verified the Windows boot-identity source (or Windows runs in Unknown-boot mode).
- The format oracle decodes every hand-written hex fixture and re-encodes it byte-identically; the model's canonical encoder reproduces every commit-id fixture.
- The toy-log enumeration finds all seeded bugs, including both [61 B2] scenarios.
- The model passes its fixture suite: the delete-policy matrix, the status machines, the node-40 table on one branch and across branches, the register incidents, the recursive-virtual-base cases (both sides equal → clean).
- **GT13, run on LQ-3, meets the normative gate list of [AR §7.7.5] and [50 §7.4] item 6** (the owner review of 2026-09-27, A6): first-try execution accuracy ≥ 85 % and ≥ 95 % after one retry on the literal and short phrasings and on the real-session stratum; confident-wrong ≤ 2 % on reads overall and ≤ 5 % per construct, 0 on writes; named-query use ≥ 80 % where one exists; no stratum below 75 % after one retry; the card ≤ 1,000 tokens by the maximum over the Claude and o200k tokenizers; no transport failure in the transport stratum. There is no "within 5 points of the best candidate" gate: the alternative surfaces (LQ with strict GQL spellings, the JSON IR) are ablations. A failed gate changes the card, an error text, a lint, a built-in, the compatibility table or the semantics, and the benchmark re-runs.
- **FL-1 meets the five M0 replay targets of [40 §8.3.4]** (the owner review of 2026-09-27, A7; the three resolver-dependent ones — `replaced` for the delete-then-re-add events, the 46 worktree HEADs' gate decisions, the transcript census's edited+moved — are M6 exit criteria, §3.7): every exact rename of the 201 re-bound with zero wrong; ≥ 96 % of the 1,180-citation sample resolved with zero silent wrong at the exact class; the window tie-break agrees with full-diff mapping in ≥ 99 % of duplicate-quote cases; `links mentions` finds all 188 moved-away references; the 58 dead memory paths with a unique rename chain re-bound (at M0 the chains come from item 12's test-only git CLI extraction and are re-bound through FL-1 and the model's exact-evidence resolution; the product's E6, built in M6 over M4's git API, re-runs the target at M6); the Rust scope scanner agrees with tree-sitter-rust (test-only) on ≥ 99.5 % of items. FL-1's parsers pass 24 h of fuzzing and GT16 kills ≥ 90 % of FL-1's compiling, non-equivalent mutants.
- Measurements 1–16 and 18–22 recorded under the protocol, idle and loaded (item 17, the rig calibration, is deferred with the rig); the decisions above recorded in [AR §8.2], [AR §13] and §2.5; the configuration registry of [AR §13] specified.
- [40]'s and [50]'s edit lists for [AR] applied, replacing its R4/R5 slots, so the specification of record is one document when the format freezes ([61 m-1]) — done on 2026-09-26 ([AR] §5e, §7.7); the specification review re-checks the integrated text.
- The infrastructure runs in profile L: hosted CI green on the model, the format oracle and FL-1; the nightly jobs complete in the agreed laptop windows; the commit-authorship and local-corpus checks refuse their seeded violations; the reference model stays ≤ 512 MB per case ([74 A21]); item 21 has set the two lanes' build cap and windows on the laptop (or shown that lane B must pause).
- Velocity is measured (units delivered against §7's estimate) and §7 is re-issued.

**Gates (mandatory from M0).** GT10 fixtures for what exists; harness validation by seeded bugs; GT13; GT5 and GT16 on FL-1; GT20 (b), the `Cargo.lock` dependency lint with the pure-Rust rule (a reviewed entry for every build script; no checked crate depending on a host-only crate), and GT20 (e), the cross-target type check (owner decision #44, [90 §11]), on every crate the repository holds; GT18 on the model, including the I26′ state oracle (§3.13).

**Size basis.** Lane A 28–38: format specification with every reservation, fault model, parameters, carrier table, protocol decisions and hand-written fixtures 7–9; `Vfs` simulator, enumerator and seeded bugs 5–7; measurements, probes, protocol and load fixture 2–3; infrastructure 2–3 (the OS-crash rig's provisioning and calibration, deferred with the rig, stay in these figures as contingency until the M0 re-issue); reviews 1–2; FL-1 part 1 11–14. Lane B 27–36: the reference model (≈ 8–10k lines, est.) 16–20; LQ-Bench 3–5 ([50 §8.2]: 1–1.5k lines plus the corpus); FL-1 part 2 8–11. FL-1 as a whole is [40]'s 6–8k lines at ≈ 3 units per 1k lines including tests, plus the corpus runs. The lane split keeps the model's author away from engine code and from FL-1's anchor resolver, whose states the model checks. Deltas since the pre-audit baseline (§7.1, in the calendar): audits + 4–6.5; cross-platform + 5.5–8.5, including the cross-target type check moved from M1 by owner decision #44 ([80 §5.4]); [90] + 4–6.5 (the Codex probes, the reservations, the gate's lints, the codec decision, LQ-Bench's harness; [AR §11] #38 (a) drops the Luna runner and the floor tier, ≈ 0.5–1 unit of lane B's share, which stays in the figures as contingency until the M0 re-issue).

### 3.2 M1 — Storage engine (56–74 units; 7–15 weeks)

**Depends on.** M0.

**Scope (final specification, [AR §4] and the physical half of [AR §5a.3], as settled by the M0 specification review).**
- **The Windows implementation of the OS layer** ([80 §2]): `LockBytes` over `LockFileEx` on the `LOCK` v1 bytes beyond EOF (writer, flush, maintenance, slots; one handle per role; the in-process grant table; the G1 wait with `WaitForSingleObject(2 s)` and `CancelIoEx`; exit 7 naming the holder); the durability classes (`NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)`, `FlushFileBuffers`, the directory flush) with `rename_noreplace`, `rename_replace` and `swap_dirs`; read-only whole-file `CreateFileMappingW`/`MapViewOfFile` of sealed read-only files after the `total_len` check, with the in-page-error handler; `FILE_SHARE_READ|WRITE|DELETE`; bounded retries on errors 5/32; the environment guard (NTFS allowed; ReFS, network, UNC including `\\wsl$`, FAT, exFAT and OneDrive refused); the `BootId` boot identity and the `QueryInterruptTimePrecise` boot clock. **The `Vfs` certification point**: its conformance suite under the fault model — flush semantics, errors 5/32, delete-pending, sharing modes, an AV-interference test (a process holding files without `FILE_SHARE_DELETE`), lock release delay, the `LockBytes` contract items 1–10 with the flush and slot bytes and an in-process two-client case, the in-page-error handler, the environment guard — passes first; the git object layer (M4) may start in the second lane from here.
- **The product codec** (`moirai-format`), agreeing with the hand-written hex fixtures and with the format oracle on every fixture and on every GT2 stream.
- **The multi-process protocol**: lock-free readers bounded by `committed_lsn` and stopping at an invalid record above `durable_lsn`; the **three-phase write** with **leaderless group commit** through the flush byte ([80 §2.4]) — the changeset computed against a snapshot before the lock, re-validated by key and appended under the writer byte, pending groups replayed only into a scratch layer, the pending range scanned and re-written under the writer byte and flushed once outside it, chained group validity, read-modify-write publishes and acknowledgement by identity, printing and maintenance after ([AR §4.5]); the recovery scan from `min(durable_lsn, checkpoint_lsn)` (adopt flushed groups by re-writing them, apply every record kind, republish, then idempotency — G2) **with the M0 protocol decisions** (a)–(i) of §2.5; boot-change recovery before any read; the holder-anchor session slots of `LOCK`; the compact overlay and the runtime-record index; bulk commits through `cs.NNNN` segments; `HEAD` by `pread` only, published without a flush on the commit path (1PC+C); the ref move inside the commit record with CAS and `orphans/<ref>` parking (N3, I27′); delta checkpoints under the maintenance byte with microseconds of writer byte to publish (G9); the quiet cap (G10); zero-filled extents on NTFS (G11, the Windows method of [80 §2.3.3]); epoch rules (G25); orphan sweep (G14).
- **Files**: log extents; `hist` retirement (compressed frames in the codec of M0 item 6, per-frame commit index, G3) with the active extent's `(id16 → lsn)` in the overlay (F-B2); base and tiered delta segments; bodies in the unmapped tail sealed into lazily mapped `blobs` files (G7); dictionary per the M0 decision; `gitmap` pages; **every section and record kind of §2.5 through the registry** (§2.6), exercised by the generic producers.
- **Physical branch machinery**: the ref table (`RefTable` records → `REFS`) incl. absorbed-vector fields; pins refcounted per segment file (G18); `ClientHead` records with `session:` expiry (G27); `RefUpdate` records; the per-ref chain index (G15); view construction `SEG(pin) ⊕ op windows` incl. by-reference expansion of sync windows (G17), streamed through a fixed buffer; promotion to `seg.b<ref_id>.K` (decimal, never-reused `ref_id`, never a ref name, [80] X-F10) + `TOUCH` by size or age under the maintenance byte (G16).
- **Maintenance**: the streaming rollup in a `moirai gc` process only — explicit, or the detached low-priority child spawned when deltas exceed the threshold ([71 RAM-M3]; the MCP server has no rollup entry point); resumable maintenance slices for the MCP server (M10 wires them); promotion by the overlay counters incl. synced windows and re-basing after rollup ([70 S1], [71 RAM-M1]); `MARKERS_OLD` folding ([70 S4]); GC with reachability over refs, reflog entries younger than the expiry and pins, a 60 s delete-pending grace **and the two-slot durable `HEAD` barrier before every deletion, extent retirement or recycling**; directory barriers after every rename and delete; blob GC; `gitmap` compaction; low memory priority for bulk passes.
- **Store parameters** read from `HEAD`/`config` for every threshold (§2.5), with the test profile.
- **Tools as `Store` API commands**: `init --store`, `backup` (pinned segment set, no writer byte held), `restore` (epoch re-roll), `repair --rebuild-from-log`, `doctor --fsck` and the storage half of `doctor --verify`.
- **The leader** (named pipe with DACL, keyed forwarding, G13; group commit is not a leader feature) **only if** the M0 decision requires it; then it is part of this milestone's protocol and gates.
- **The storage driver** (permanent test infrastructure): physical op streams across several refs, the generic producers, and acknowledgement reporting to the harness over a pipe before the acknowledgement is printed.

**Not built yet.** Graph semantics, version-control semantics, queries, CLI verbs, git objects, image, file-link runtime. None of these is stubbed in product code: the driver, the generic producers and the model's storage view supply the inputs.

**Exit criteria (owner's machine where timed).**
- Durable commit p50 ≤ flush floor p50 + 0.5 ms and p99 ≤ flush floor p99 + 1 ms, floors re-measured in the same run; at most one flush per durable commit — exactly one for a lone writer, ≤ 3 per 16-writer burst — and every acknowledgement after a covering flush and a passed identity check (counted).
- Open ≤ 1.5 ms at 1e5 and ≤ 3 ms at 1e6 with a full tail at the M0 checkpoint threshold, warm; bytes read on open equal at 1e4 and 1e6 apart from the tail; recovery after a kill with a full tail ≤ 10 ms.
- Writer-wait p99 ≤ 50 ms with 16 writers (G1, with group commit); writer-byte hold p99 ≤ 5 ms; reader p99 during a 16-writer burst ≤ 2× its idle value.
- Delta checkpoint ≤ 50 ms at 1e5 and ≤ 100 ms at 1e6, outside the writer byte; rollup ≤ 0.3 s at 1e5 and ≤ 3 s at 1e6; promotion ≤ 100 ms at 1e6 — all with every reserved section populated by the generic producers.
- First read of a ref forked 1k / 7k / 14k / 60k commits ago ≤ 3 / 10 / 10 / 20 ms with 50 concurrent writers (G15, G28); 50 refs × 2k ops forked 1k–60k commits apart readable within that budget.
- Private RSS of a short-lived process ≤ 4 MB at 1e4–1e6 on one ref (also with the tail at the quiet cap), + ≤ 1 MiB per additional ref read incl. synced windows; delta checkpoint and promotion ≤ 8 MB, the rollup child ≤ 24 / 24 / 32 MB, `repair` ≤ 32 MB, decoding one commit from `hist` ≤ 2 MB; zero CPU time and zero context switches over 10 minutes for an idle long-lived reader, measured from 15 s after its last request (§5.1).
- Writer-byte hold p99 ≤ 5 ms and max ≤ 20 ms with 16 fresh writers on lanes; the 14-day daily-sync fixture's first read ≤ 10 ms with an overlay ≤ 1 MiB; ≤ 10 / 14 file opens per `main` / lane read.
- `backup` ≤ 0.3 s at 1e5; `backup` → `restore` into an empty directory → `doctor --verify` (storage) clean; `repair --rebuild-from-log` reproduces byte-identical segments, **every reserved section included**.
- **GT15 is not an M1 exit criterion**: the owner review of 2026-09-27 deferred the OS-crash loop, with its ≥ 1,000 cycles, to after the release (§3.13). Crash safety at M1 exit rests on GT1's crash-point enumeration (with post-crash read freshness in every crash state), GT3, GT4, acknowledgement only after a covering flush and an identity check (counted above) and `backup`/`restore`; real power loss is a stated residual risk ([AR §10] risk 17).
- **Seeded protocol bugs in the real engine** — skip the flush; publish before the flush; adopt without CAS; adopt by re-flush only; delete before the `HEAD` barrier; truncate past valid same-epoch records; trust a lazily published `committed_lsn`; serve a pre-crash view; a single-slot barrier; adopt a commit without its group's markers; skip a non-commit record; a rename without a namespace barrier; and the thirteen group-commit bugs of [80 §2.4.4] — are each caught by GT1, GT3 or GT4 [61 M-4], [72], [81].
- Commit ids agree byte for byte between the engine and the model's independent canonical encoder on every GT2 stream.
- No `git` code and no process spawn in the engine crates (dependency and call lint).

**Gates.** GT1 over the fault model — every record kind and every boundary incl. between the two appends of a flushed group, bounded subsets of unflushed writes, torn sectors, failed-flush sequences, disk-full; ≥ 1e5 crash states; zero lost acknowledged commits, zero corrupt opens — mandatory from here. GT3 ≥ 1e6 steps with crash, fsync-error, lock-release-delay, sharing-violation, delete-pending, pause, disk-full, clock-step and mixed-sector-read injection, with pins, promotion, GC and long-lived readers, sweeping the store parameters over the test profile and the production values — mandatory, nightly afterwards. GT4 through the storage driver in its three variants of profile L (`TerminateProcess`, `NtSuspendProcess`, ±1 h clock steps; disk-full is covered by GT1's and GT3's disk-full injection and the owner-run RG7 drill, §3.13) — mandatory. GT15 — deferred to after the release (§3.13). GT5 fuzzers for records, `HEAD` and segments, 24 h each clean. GT2 at storage level: ≥ 1e6 physical ops against the model's storage view, commit ids included. GT16 on the storage crates (reported; the seeded bugs gate). GT11 rows of §5.4 marked M1. GT20 (a), (b) and (d) — the spawn lint, the dependency lint and the OS-layer lint — mandatory from here.

**Size basis.** Engine core 25 units [22 §7.1] + the physical parts of its VCS layer ≈ 5 + promotion ≈ 2 → 28–34; the registry and generic producers 2–3; the protocol decisions 1; store parameters 0.5–1 → build 32–39 (if M0 requires the leader, + 3–4). Test share 14–18: multi-process simulator ≈ 6, kill-loop harness, driver and variants 3–4, GT15 harness and runs 2–3 (deferred with the rig; kept in the figures as contingency until the M0 re-issue), fuzzers 1–2, storage differential 1–2, seeded bugs and mutation 1. Deltas since the pre-audit baseline (§7.1, in the calendar): audits + 7–11; cross-platform + 2.5–5 ([80 §5.4]; the cross-target type check moved to M0 by owner decision #44); [90] + 0.5–1 (the pure-Rust codec).

### 3.3 M2 — Graph core (31.5–41.5 units; 4–8.5 weeks)

**Depends on.** M1.

**Scope.**
- 13 kinds with `phase_state`/`return_to` and the `gates` edge; schema as data on every ref with weakening changes and **strengthening** through `migrate` and re-validation [AR §2.12]; the closed field-type set incl. R4's types (R-1); `done` as a virtual field.
- Edges, set semantics, both directions in one commit (I-P3); delete policies with X4 flagging and re-pointing, `--cascade`/`--reparent`, the `suspect` closure with its budget (a store parameter), `rm` refused under a live lease unless `--release` (I32′).
- Invariants checked on every write [AR §3.4]; I5′ with Pearce–Kelly incl. implied exogenous edges, full Kahn above the edge-count parameter.
- Derived state maintained eagerly for affected nodes; the persisted structural predicate and `ready` applied at read time (§2.5); `affected` complete for every derived predicate, or flagged incomplete ([50] F15, F16); the `ALLOC` index (F17).
- Leases with fencing tokens (I17′), run-scoped and TTL; idempotency bound to key, payload hash and branch (I14′); the change feed with `affected` sets; `settled`/`deleted`/`cleared` markers produced by the op applier on every path (CB3); status machines; `mentions` with the sigil rule; `resource` mutexes; `apply` batches with `$refs` as a `Store` API command.
- The role write-policy enforcement point (policy rows as schema data; identity plumbing in M9).
- Search: tier 1 and tier 2 (above the node-count parameter, merged at rollup).
- **FL-3** per [40] as revised after [41]: file nodes and their identity derivation, `#N` reuse, `at` edges with anchors and the discriminator, the path, alias and anchor indexes and the reverse index, the `suspect` extension, `planned`, I-F1–I-F3, I-F8, I-F9.
- **Real producers registered** for `BM_*`, `TOPO`, `TERMS`/`POST`, and [50]'s `CREATOR`, `FCOL`/`FIDX`, chunk cardinalities, `STATS`, `DOCLEN`; FL-3's sections.
- `doctor --verify` recomputation of every derived structure; a typed **read API** (rows, columns, CSR slices, bitsets, derived predicates, the change feed) that the query language compiles to. Read *verbs* are not written here: they are named queries of M7.

**Not built yet.** Branches beyond `main` at the semantic level, merges, history verbs; queries; CLI; git; image; the file-link runtime (file nodes and anchors exist as data with their invariants).

**Exit criteria (owner's machine, through the read API).** `get` ≤ 5 µs; the `ready` computation ≤ 300 µs per page of 20 at 1e5 and ≤ 3 ms at 1e6, unchanged with 50 unabsorbed markers; the blocking-task id set ≤ 300 µs at 1e5 and ≤ 3 ms at 1e6 (engine time; printing, ≈ 0.1 ms per 1,000 ids, excluded); transitive blockers ≤ 200 µs at 1e5; derived-state maintenance ≤ 1 µs per status change; FTS tier 1 ≤ 10 ms at 1e5, tier 2 ≤ 5 ms per term at 1e6; `doctor --verify` equals incremental state at 1e5 and 1e6 within ≤ 16 / 32 MB (chunked per 65,536 ids); `brief_triage`'s marker scan ≤ 50 µs with 90k inert and 100 active markers; one `#N` per uid when two lanes link one file before any merge (the `UIDX` probe, [72 M7]); FL-3 properties against the model (identity derivation, index consistency, I-F1–I-F3, I-F8, I-F9). **Re-certification**: the byte-identical rebuild, the M1 checkpoint/rollup/promotion budgets and GT1/GT3/GT4 with the real sections and graph ops.

**Gates.** GT2 semantic differential, ≥ 1e6 seeded commands incl. invalid ones and idempotent retries, zero disagreements, with the thresholds swept — mandatory from here. GT6 properties (incremental derived state == recomputation after 1e5 random ops incl. deletes and reparents; reverse CSR == inverse; `invert(changeset)`; I12-style fuzz on one ref). GT10: the delete-policy matrix and the node-40 table on `main`. GT16 ≥ 90 % in the graph crates.

**Size basis.** Graph semantics 15 [22 §7.1] + FTS tier 2 (2–3) + strengthening and `migrate` (1–2) + role policy (≈ 1) → 18–22; FL-3 6–9 ([40]: 2–3k lines); the [50] producers 1–2 → build 25–33; test 5–6 (properties, re-certification, sweeps). Deltas (§7.1): audits + 1.5–2.5.

### 3.4 M3 — Version control (38–45.5 units; 5–9 weeks) — R1

**Depends on.** M1, M2.

**The recursive virtual base** (specified at M0, implemented here). Under criss-cross — two LCAs L1, L2 of A and B that disagree on key k, A having resolved k to L1's value and B to L2's — the newest-LCA rule of issue 1's I31′ picks, say, L1 as base, sees A unchanged and takes B's value **without a conflict**: the classic hazard of choosing one base among several LCAs. Criss-cross is unreachable on the daily path into `main` (sync-first) but reachable through cross-lane and branch-of-branch merges (CM9). The rule: the LCAs are merged pairwise in generation order (ties by commit id) with the same typed rules; the result is the base; **a key whose virtual-base value is a conflict value is clean when both sides hold the same value and conflicts whenever they differ** ([61 m-3]).

**Scope.** Ref kinds and write masks (`main`, `lane/*`, `plan/*`, `merge/<dst>/from/<src>`, `import/*`, `orphans/*`, `tags/*`); `branch`/`checkout`/`--list`/`-d`/`-D` with lease release and `cleared` markers (N13d); the HEAD resolution chain (D3) and worktree bindings; reflog, `undo --expect` (N13f), `op log`/`op restore`; tags with pins; `log --graph`, `diff A..B`/`A...B`, `show @`, `blame`, whole-graph as-of with I18′; the typed three-way merge with the base at the LCA (N1) or the recursive virtual base, all rules of [AR §5a.7]; sync-first merges into `main`; merge-by-reference `sync` (G17); per-pair staging (CM5); validators in I37′ order; `resolve`, `merge --continue`/`--abort`; `merge-check`; `revert` (incl. `--mainline 1`, CL8) and `cherry-pick` with `NotFound`/`DATA` handling (I34′); absorbed vectors (CM1); marker absorption on every branch (I26′); `--across` (caller's branch + `main` by default; all refs through promoted `TOUCH` bitmaps, as a counted budget with a cursor); promotion policy; `apply` taking its branch from the run (D6); the idempotency rule across merged branches (N13e). **FL-7** per [40] as revised: the merge rules of the observation composite, path claims, the prefix history, anchor add-wins, the re-key rule for a derived identity created on one side and removed on the other [41 B1]. The [50] history-side producers: `CONFLICTS`, the `hist` frame and `Checkpoint` timestamps, the unhashed commit-header fields (F8–F11, F14). History and version views as `View`s and relations for M7.
- **The version-control test driver** (permanent test infrastructure, [61 m-8]) runs 16 simulated clients bound to different branches and simulates every input of the resolution chain in the order of record, [90 §4.1]'s Branch row ([AR §5a.4]) — explicit `--branch`/`branch` (exit 5 if it differs from the lease), the presented lease's branch, the Codex `sandboxCwd` or Claude stamp `cwd` binding, `MOIRAI_BRANCH`, the dispatch marker, `--client`/the directory binding, the git-worktree hint, `default-branch` — so the kill loop runs before the CLI exists.

**Not built yet.** The query surface over these views; CLI verbs; image; the file-link runtime (FL-7's rules are here; resolution by observation after a merge is M6).

**Exit criteria (owner's machine where timed).** The owner's two register incidents replay correctly through `sync` + `merge` [02 §7.3]; a lane completes a task through the `Store` API command behind each of the ten doors (`complete`, `set --done`, `set --status`, MCP `write` (`name: tx.complete`, or a `TX` with `SET t.done = true`), `apply`, `cherry-pick`, `revert`, `merge`, `sync`, image import's op application) and `main` never re-dispatches it, and the **I26′ state oracle** (≥ 10⁶ histories, ≥ 5 refs) agrees with every `ready` and `claim` across `reopen`, `Undelete`, `undo` of a reopen or an undelete, `op restore` in both directions, forks with `-D`, staging aborts and `TX` coalescing ([72 M4]); sync residue equivalence — `view(lane)` after sync equals the model's `state_at(sync)` with one-sided compositions and re-keys ([72 M5]); 10 synthetic lanes × 1k ops with every conflict class incl. sync-then-merge, criss-cross, branch-of-branch (CM9) and move + inheritance merge deterministically; a `plan/*` branch cannot mark work done; two lanes with staged syncs do not block a third (CM5); FL-7's merge properties hold ([40 §8.3.2] P8; the removed-then-recreated identity of [41 B1] never resurrects). Budgets: merge of a 2k-op lane vs 5k trunk ops ≤ 50 ms at 1e5, computed before the lock, ≤ 8 MB (≤ 16 MB at 1e6 with full Kahn); `sync` ≤ 40 ms at 1e5; writer-byte hold p99 ≤ 5 ms for merges and syncs, and 16 writers with a concurrent merge at p99 ≤ 50 ms; a lane read incl. the first-read overlay build on the 14-day daily-sync fixture ≤ 10 ms and ≤ `main` + 1 MiB; ≤ 2 bases mapped after a rollup; `branch`/`checkout`/`undo`/`tag` ≤ flush floor + 1 ms; as-of at a pinned set ≤ 50 ms; `log --node` ≤ 50 µs per shown edit. **Re-certification** with the history-side producers.

**Gates.** GT6 version-control properties against the model — pin ⊕ ops == replay from genesis per branch; merge determinism; disjoint keys commute; a clean merge equals sequential application through the composition and re-key functions ([72 m4]); no structural violation reaches a ref (I12 fuzz); I25′ over random DAGs with interleaved sync/merge; I26′ by its state definition through every door and every ref move; I31′ (the result is independent of LCA enumeration order; a key both LCAs agree on and neither side touched never conflicts; two sides that resolved a criss-cross differently always conflict; two sides that resolved it identically never conflict); I37′ and the merge variant of X1; `undo` versus absorbed vectors; CM1/CM5/CM9 shapes — mandatory from here. GT4 through the version-control driver with 16 bound directories running branch, commit, sync, merge and undo — mandatory from here. GT10: the node-40 table across branches, the register incidents. GT16 ≥ 90 % in the VCS and merge crates.

**Size basis.** The semantic remainder of the VCS layer ≈ 9 + merge engine 14 + cherry-pick, `op restore`, `plan/*` ≈ 3 + recursive virtual base 1–2 → 27–30 [22 §7.1]; FL-7 3–4.5 (merge rules only, [41 M9]); the [50] producers 1 → build 31–36; test 5–6 (driver, properties, re-certification). Deltas (§7.1): audits + 2–3.5.

### 3.5 M4 — Git object layer (15.5–22 units; 2–4.5 weeks; second lane from the M1 `Vfs` certification point)

**Depends on.** M0 (formats), the certified `Vfs` of M1.

**Scope.** SHA-1 and SHA-256 object model (blob, tree, commit, tag); loose objects read and written with temp-then-rename and bounded retry (G21); pack v2 + idx v2 read incl. OFS/REF delta resolution, and written (packs above the loose/pack threshold, idx written after the pack and renamed last, G23); commit-graph read **incl. split chains and generation data** [41 m16]; loose refs and `packed-refs` read and written through git's `<ref>.lock` protocol with read-after-write verification and one retry (G21, git-for-windows #6396); reftable **read**; bundles v2/v3 read and write; image-repo config written by moirai (`gc.auto = 0`, `gc.autoPackLimit = 0`, `pack.threads = 2`, `pack.windowMemory = 64m`, `refStorage = files`); tree read and path-level diff with exact-oid rename detection; generation-pruned ancestry and merge-base, including heads newer than the commit-graph (a walk over loose objects and packs) [41 B2]; deflate with the codec chosen at M0; `git` spawned only by the transport entry point and for ref updates in a reftable destination when git is present (G22).

**Not built yet.** Image semantics; R4's evidence use of this layer (M6).

**Exit criteria.** Against the git CLI as the independent oracle (GT7): every object, pack, idx and bundle written passes `git fsck --strict` and `git verify-pack`; reading agrees with `git cat-file --batch` object-for-object over the owner's repositories (read-only) and over synthetic repositories created by Git for Windows 2.54 in SHA-1 and SHA-256, after `git gc --aggressive`, with split commit-graphs, `packed-refs` and reftable; ancestry answers equal `git merge-base --is-ancestor` on ≥ 1e4 random pairs **and on the owner's trunk and active-lane heads, which are newer than the commit-graph** [M, 41 §1]; exact renames equal `git diff-tree -M100%`; refs written while a concurrent `git` process updates the same repository never corrupt and never lose an update; an injected AV rollback of a loose-ref write is detected and retried. Budgets: ancestry ≤ 1 ms per pair with a commit-graph and ≤ 5 ms without one at the owner's repository size; one checkpoint-sized pack + idx + a `packed-refs` transaction with directory barriers ≤ 50 ms for `main` + 2 lanes incl. flushes and Defender (confirmed against M0 item 8, [70 S11]); the delta-base cache ≤ 256 KiB (CLI) / 1 MiB (MCP), and a tree lookup, an ancestry walk and a 2,000-commit rename window each ≤ 1 MiB private ([71 RAM-m1]); `unknown`, never `false`, across shallow and partial clones, replace refs and grafts ([72 m7]); pack writing at ≥ the M0-measured deflate throughput minus 20 %.

**Gates.** GT7 — mandatory from here. GT5 fuzzers for loose objects, packs, idx, commit-graph, refs, reftable and bundles, 24 h each clean. GT20 with its call sites: no `git` spawn outside the four named call sites of [AR §5c] — the transport entry point (`image push/pull`) and ref updates in a reftable destination (G22), built here, `file mv --git` (M6) and `doctor lanes --refresh-graph` (M8) — and no other process spawn except the detached `moirai gc` child; GT20 (c): GT2 streams run with `git` removed from `PATH` and no `.git` present, `stale` answering `unknown` — mandatory from here. GT16 reported.

**Size basis.** The hand-written git object layer with differential tests is 9 units [22 §7.1]; SHA-256, split commit-graph chains, reftable reading and bundles bring it to [AR T10]'s 4–6k lines plus fuzzing → build 12–17; test 3–4 [61 M-8]. Deltas (§7.1): audits + 0.5–1.

### 3.6 M5 — Git image (17.5–22.5 units; 2–4.5 weeks) — R3

**Depends on.** M3, M4 (and M2's data).

**Scope.** [AR §5b] complete, driven through the `Store` API and a permanent **image test driver** (the image verbs land with the CLI in M8): tree layout; the `.moi` encoder and decoder generated from the M0 ABNF with the exporter's re-parse self-check; commit mapping with the full trailer set (CB1); two-parent `sync` commits (CM2); byte-exact bodies (CM3); tombstones with retained edges (CM4); `gitmap` and its rebuild from trailers; the unhashed side ref (CB2); export at checkpoint and commit granularity to the separate bare repository (the `refs/moirai/*` destination is not built, [74 A17]) with the durability order and the `gitmap`-vs-destination check of [AR §5b.6] ([72 M9]), packs closed every 65,536 objects; import with native verification by stated parent ids and the `verified` bit, foreign commits with deterministic ids, `Undelete`, `incr` ledgers, two-parent foreign merges through the typed three-way (I30′), `ImageParse` and violation staging on `import/<ref>`, divergent imports landing on `import/<ref>` and merged by the typed rules (`image.import-merge`, [72 M10]), import-checkpoint commits as bulk commits ([71 RAM-B1]), anchor-text digests and the `text-unavailable` sub-state ([72 M6]), markers written by imported ops (CB3); cross-store alias rules and `mentions` handling (N11, CL2); `--with-oplog`; transport through git when present; `image.allowed-remotes` ([AR §13]; decision #16). **FL-8** per [40]: `.moi` artifact fields and `anchor` lines, the prefix history as the root node's `path_moves` field (no trailer, [40] revision 2), import validation, identity derivation on import. Named-query items (F3) round-trip as schema data; their `QueryCycle` validation is registered by M7.

**Not built yet.** The image verbs (M8); the post-merge export hook (M9); `QueryCycle` (M7).

**Exit criteria.** Gate 0 over the M0 carrier table — hash reconstruction from trailers plus tree diff for every commit kind, R4's and R5's items included — passes before any corpus; gate 1 — `export → fresh import → export` byte-identical at commit granularity for 1e5 nodes and 1e5 commits in SHA-1 and SHA-256, with synced lanes, flagged tombstones, exact bodies, conflicted nodes, anchors and file nodes; gate 2 — state-identical at checkpoint granularity, dependents of flagged blockers not ready, head trees reproduced byte-identically; gate 3 — `import(export(St1)) ⊕ import(export(St2))` equals their merge, and two stores importing one bundle export identical objects including the root tree and derive the same file and anchor identities ([40 §8.3.2] P10); every failure case of [AR §5b.8] behaves as specified; `git fsck` clean. Gates 0–3 run in both anchor-text modes; a GT8 fixture exports, commits locally, imports a foreign edit and keeps every local commit, with the foreign id identical in a second store. Budgets: full export of 1e5 ≤ 3 s and ≤ 6 MB private (≤ 2 / 10 MB at 1e4 / 1e6); full import of 1e5 ≤ 7 s; import ≤ 4 MB per commit, a checkpoint-image import into a fresh store ≤ 8 / 16 / 32 MB; an incremental checkpoint export of `main` + 2 lanes ≤ 50 ms. GT15's oracle gains `git fsck --strict` on the image and `gitmap` ⊆ destination (specified here; it runs after the release with the deferred rig). **Re-certification**: GT1/GT3/GT4 with exports and imports in the streams (exports rely on pins and the delete-pending grace, G26).

**Gates.** GT8 image gates — mandatory from here; GT5 `.moi` fuzzer (block strings, `---` in bodies, CRLF, missing final LF, body conflicts, anchor lines, pathological escapes) 24 h clean; GT7 on every exported image; the state-identical check against the model's `state(ref)`; the I26′ door test through real image import (CB3); GT16.

**Size basis.** The image core is 12 units [22 §7.1]; `--with-oplog`, the second destination and the cross-store rules add 0–2; FL-8 2.5–3.5 ([40]: 0.8–1.2k lines); the verbs move to M8 (−0.5) → build 14–17; test 3–4 (round-trip corpora, driver). Deltas (§7.1): audits + 0.5–1.5 (net of the second destination's exclusion).

### 3.7 M6 — File-link runtime (31–44 units; 4–9 weeks) — R4, per [40]

**Depends on.** M1, M2, M3, M4. FL-2 may run in the second lane from M0 exit (it needs only the `Vfs` conventions).

**Scope (per [40] as revised after [41]; this file restates only the placement).** **FL-2** `ProjectFs`: the Windows implementation and a deterministic simulator of file ids, replace-by-rename, tunneling, sharing violations (32/5), the Recycle Bin, case-insensitivity, git-style checkout rewrites and cloud placeholders ([41 M8]). **FL-4**: the runtime tables and records, registered in M1's registry; `resolve` (a pure read) and settle (the write at settle points) with the evidence cascade and the exactness rules of [41 B3]; `link`, `unlink`, `file add|mv|rm|relink|revert|where` as `Store` API commands; the crash-safe intent protocol and its recovery; tree identity, the ancestry gate over M4, the freshness and write rules and the bindings ([41 M9]: they are resolve semantics, so they are here, not in a later layer). Git per-commit rename evidence (E6) over M4 as a resolver input of FL-4, with the read-path caps of [AR §5e.3]; the USN-journal reader (E2) is not built (excluded by decision with a revisit trigger, [74 A13]; its reservations stay). The priority-audit additions: `ANCHORRES` and the spawn-free edit-evidence path, `TreeReg` epochs with `FILEOBS` rows only on change, the `TREES.dirty` row, the settle CAS on `rev_seq`, `complete`'s settle as a separate commit, the E3d identity rule and the path-reuse check, streamed file reads with fixed buffers, the no-replace rename with `durable-name` on both parents in the intent protocol ([70 S5–S8], [71 RAM-M4], [72 M8, M11, M13], [80 §2.3.2]). No spawn fallback exists anywhere [41 B2]. For owner decision #32 ([80]): `VolumeCaps` as the resolver's input and the tagged `OsFileId` rows; the twin rule and the unique-creation-time copy rule; GT2's link generator sweeps the Windows, Linux and macOS capability profiles as input data, on Windows ([80 §5.5] (c)); + 1–2.5 units, in §7.

**Not built yet.** The file verbs in the CLI (M8); the link built-ins (M7); pack rendering and hooks (M9).

**Exit criteria (per [40 §8.3], as revised).** The move/edit pattern matrix of [40 §8.3.1] (rows 1–36) passes on the simulator and on real NTFS in scratch directories through a permanent file-link test driver; properties P1–P7, P9, P11, P12 and P15 of [40 §8.3.2] hold on the simulator with ground-truth identity (P8, P13 and P14 are gated in M3, P10 and P14 in M5) — in particular **zero wrong automatic re-binds** (P1) and **every anchor state agreeing with the model's brute-force search** (P11); the ambiguity fuzzers of [40 §8.3.3] surface every generated ambiguity; the replay corpora of [40 §8.3.4] re-run on the product meet the M0 targets, and the three resolver-dependent targets are met for the first time (the owner review of 2026-09-27, A7): `replaced` wherever the re-added content of the 34 delete-then-re-add events is unrelated and never for related content; every tree-gate decision for the 46 worktree HEADs equal to the brute-force oracle, the 7 the commit-graph misses included; every resolvable edit-then-move of the transcript census `moved-needs-confirm (edited+moved)`, never `missing`; crash-point enumeration of the intent protocol ([40 §8.3.5]) with sharing-violation and delay injection never loses an intent and never half-applies the graph; I-F5 (reads append nothing, counted), I-F6, I-F7 and I-F11 (no project-file handle open when a command returns, by a handle-count probe; no cloud placeholder hydrated by an automatic path) asserted; matrix rows 32–33 and P1 generators for promote-replace and delete + re-create + directory move ([72 M13]); matrix rows 34–36 on real NTFS — case twins from another OS, identical files created in one tick, the colliding checkout — for the twin rule and the unique-creation-time copy rule ([80 §2.11.4] rules 1–2); the settle-concurrency scenario (two settles, an interleaved move, no regression; [72 M11]); intent crash enumeration with fault-model item (2); the budgets of [40 §7.4] met through the driver, including a 50-link pack with 10 edited files ≤ 5 ms p50, read-path git work ≤ 10 ms p99, a `SessionStart` settle ≤ 16 KB of log when nothing changed, and ≤ 4 MB for a read verb resolving anchors in a 16 MiB file. **Re-certification** of M1–M3's rebuild, budgets and crash gates with the R4 runtime sections and records.

**Gates.** GT2 against the model's link logic — mandatory from here; GT17 pattern matrix — mandatory from here; GT10 R4 corpora; GT5 anchor-selector and fingerprint parsers; GT4 with file intents in the kill loop; GT16 ≥ 90 % in the resolver crates; GT20 (a) over every R4 path (only `file mv --git` may spawn `git`, [40 §7.4]).

**Size basis** (from [40 §8.1] at ≈ 3 units per 1k lines incl. tests): FL-2 9–12 (3–4k lines), FL-4 12–15 (4–5k), its E6 evidence input 2–3.5 (E2 excluded), the [41] changes 1.5–3 → build 25–34 ([40 §8.1]: 3–4k + 6–7.5k lines); test 4–5 (pattern matrix on NTFS, intent-protocol enumeration, corpus re-runs, re-certification). Deltas (§7.1): audits + 1–2.5 (net of E2's exclusion); cross-platform + 1–2.5.

### 3.8 M7 — Query language (51–72.5 units; 6.5–14.5 weeks) — R5, per [50]

**Depends on.** M2, M3, M4; M5 (LQ-11); M6 (FL-6). LQ-1 may be built in the second lane from M0 exit; LQ-2, LQ-4, LQ-5 and LQ-6 from M2 exit (they need only the schema module and the read API).

**Scope (per [50 §8.2]; the surface was frozen at M0).** LQ-1 front end (the full grammar incl. `TX`, spans, pretty-printer, diagnostics in text and JSON); LQ-2 binder (kind sets, schema binding, direction typing, enum coercion, absent typing, parameter typing, canonical form and hash, view-validity rules, role-policy pre-check); LQ-4 executor core; LQ-5 graph operators incl. `search()` over M2's FTS tiers; LQ-6 planner with exact counts (F6, F7), pre-flight refusal, EXPLAIN/PROFILE/check; LQ-7 transactions (statement compiler to changeset ops, `EXPECT`/`IF TIP`/`ASSERT`/`LEASE`, deferred validation, idempotency on the canonical AST, per-op role policy, `DRY`, named mutations, the JSON IR shared with `apply`); LQ-9 versioned queries (revision resolution, the as-of strategies, the derived-at-view cone, relation wrappers over M3, `conflicts`/`violations`, `across` as a counted budget with a cursor); LQ-10 the ancestry built-in over M4; the standard library of named queries and named mutations [50 §4]; LQ-11 registered in the validator and merge-rule tables; **FL-6** registered in the built-in registry.

**Not built yet.** The CLI transport and rendering (M8); the MCP tools (M10).

**Exit criteria.** Per [50 §8.3]: parse + bind ≤ 20 µs for queries ≤ 1 KB; anchored 2–3-hop patterns ≤ 0.3 ms at 1e6; each [50 §5.12] row within 2× of its estimate at 1e5 (nightly at 1e6); default budgets meet [50 §5.12]'s composition rule — a query never takes the process over its kind's RSS gate unless the pre-query baseline was already within 256 KiB of it — with `mem` from the measured headroom (default ≤ 1 MiB, agent maxima 2 MiB in the CLI and 4 MiB in MCP; as-of op caps 16,000 CLI / 100,000 MCP within `mem`); the CLI baseline at 1e5 on `main` and on a 14-day lane is measured at M0 (item 11) against the 4 MB gate (the A1 re-review S-23: the former "the CLI ≤ 4 MB private at 1e5" was withdrawn by [50]); `TX` within `wmem` (a default-cap `TX` ≤ 4 MB; a 500k-op orchestrator `TX` ≤ 16 MB, refused with E501 and a split hint once its candidate exceeds `wmem` (never a bulk commit, [AR §4.5] step 4)); writer-byte hold p99 ≤ 5 ms for `TX`; `fs` units charge git objects ([70 S6]); the canonical-AST algorithm frozen per grammar version ([72 m3]); LQ error texts ≤ 600 B, ASCII ([73 F15]); the plan-choice benchmark picks the fast plan for [15 §4.2]'s pairs at 1e5 and 1e6; the same query and data stop at the same point with the same cursor; exit code 10 for every budget cut. LQ-Bench re-run on the product executor (through the test harness) meets the M0 gates. **Re-certification**: GT8 with named queries (LQ-11); GT3 with concurrent `TX` writers — `EXPECT`/`IF TIP`/`IF TARGETS` never admit a lost update, no apply touches a target outside its `DRY` set, and a `DRY` diff equals the committed diff [50 LQ-7], [72 m1].

**Gates.** GT2 against the model's evaluator — **the generator emits ASTs, the model evaluates them, a printer feeds text to the product**, ≥ 1e6 queries across all view kinds plus `TX` blocks compared by resulting state and committed diff — mandatory from here; `parse(print(ast)) == ast`; GT9 (with two-valued logic the partition is `p` / `NOT p`; budget-stop replay; cursor pages never skip or repeat); GT5 grammar fuzzer 24 h clean; verb == named query for every standard query; canonical-hash invariance under formatting, spelling, variable names and parameter order; a static check that the read entry point cannot acquire the writer byte; GT13 on the product; GT16 ≥ 90 % in the binder and executor crates.

**Size basis.** [50 §8.2], revision 2: LQ-1, -2, -4, -5, -6, -7, -9, -10 ≈ 15–21.5k lines ≈ 45–64 units incl. tests; FL-6 ≈ 1.5; LQ-11 ≈ 1–1.5 → build 47.5–67; test 2–3 (re-certification, LQ-Bench on the product). LQ-3 (the model's own parser, binder and evaluator) and LQ-Bench are in M0. Issue 2 had carried [50]'s first-revision 14–20k lines (42–60 units, M7 46–64); the integration of 2026-09-26 raised it (§10.4). Deltas (§7.1): audits + 1.5–2.5.

### 3.9 M8 — CLI (17–25 units; 2–5 weeks) — R2 user-visible

**Depends on.** M3, M4, M5, M6, M7.

**Scope.** The output contract of [AR §7.1] as amended by [50 §6] and [40 §6.1] (ids first, deterministic order, `--ids`, `--json v1`/`--jsonl`, drop and budget footers, exit codes incl. 10); the transport rules (bare ids, queries via `-f`/heredoc/stdin, BOM stripped, no `@file`, MSYS path-mangling guard, UTF-8 output regardless of code page); every verb of C1–C7 — named queries, named mutations, **`q` and `tx`** (LQ-8), VCS rituals, `apply`, maintenance (`gc`, `quiet`, `migrate`, `backup`, `restore`, `repair`), `init` with the D4 guard, `--link`, `worktree bind`, `lane open/close`, **the image verbs** (`image export|import|push|pull|doctor|show|gc`) and **the file and link verbs** (FL-5); discovery and placement at `<git-common-dir>/moirai/` (CM6); `doctor store|lanes|agents|image|--verify|--fsck`; `check`/`stale` in-process (CM7).

**Harness-agnostic scope** ([90 §10.2]): the caller-context resolver (rights from the presented lease; the actor resolved lease-first; the binding rule for environment leases; the branch in [90 §4.1]'s order of record; harness detection) and `actor_src`; TTL renewal by lease-presenting writes; `image export --if-older` on `apply` and `run close`, best effort under a sandbox ([90 §2.5]); client profiles (`claude`, `codex`, `generic`), byte ceilings and `--ids` pages; per-harness exit-7 texts with the `result.v1` fallback; role and session leases with the minting policy (`claim --role R --run ID`, `claim --role orchestrator --session`); `moirai schema result-v1`; `apply --from jsonl:` and `codex-exec:`; GT12 shells including PowerShell 5.1 under Codex's prefix and the `cmd.exe` hook-launcher subset (+ 2–3 units, in §7).

**Not built yet.** Pack/brief, hook and MCP verbs (each arrives with its component under this frozen contract and passes GT12's contract checks).

**Exit criteria.** Golden outputs for every verb; transport tests in Git-Bash, Windows PowerShell 5.1 and PowerShell 7 with the argv tables of [40 §0.3] and [50 §6.2] as fixtures (ids, quotes, empty arguments, Cyrillic stdin, BOMs, MSYS rewriting), plus cmd for the human documentation's forms ([80 §4] T10); the `${CLAUDE_PLUGIN_DATA}` substitution in exec-form hook commands and `.mcp.json` re-checked against the then-current Claude Code (M0 item 7), deciding whether `moirai hooks install` writes the expanded absolute path ([80 §2.12]); `init` in the main checkout and a read from a `<lanes-dir>/<lane>` worktree find the same store with no pointer file; shadow detection; `check` of a commit made seconds ago correct with no spawn; the backup/restore drill through the binary; LQ-Bench through the binary. `moirai config get|set|unset|list|check` with the typed registry and `HEAD.config_gen`; `check` as a write verb; default idempotency keys and the outcome-unknown message ([72 m5]); the ASCII header `branch: R | rev N | k rows` and non-zero-exit stdout ≤ 8,000 B; `restore`'s own swap with `HEAD.retired`. Budgets: spawn-to-exit of a read verb at 1e5 ≤ empty-executable floor + 5 ms (direct); ≤ 10 / 14 / 18 file opens per command; a file-bearing header ≤ 0.5 ms with no worktree scan; private RSS ≤ 4 MB at 1e4–1e6; an uncached `check` ≤ 5 ms.

**Gates.** GT4 through the real binary: 16 CLI processes with mixed verbs across branches, file verbs and image exports included, 10,000 iterations, all three variants (§3.13) — mandatory from here; GT12 contract tests; GT5 argv/transport fuzzing; GT17 re-run through the binary; GT20 (c) — GT4 through the binary with `git` removed from `PATH` — mandatory from here.

**Size basis.** The CLI share of [22 §7.1]'s 15-unit line, thinner with named queries, 7–9; FL-5 3–4.5 ([40]: 1–1.5k lines); LQ-8 2–3 ([50]: ~1k lines + ~0.5k of LQ text); the image verbs 0.5–1 → build 12–17; test 2–3. Deltas (§7.1): audits + 1–2; [90] + 2–3.

### 3.10 M9 — Agent interface (17.5–26 units; 2–5 weeks)

**Depends on.** M5, M6, M7, M8.

**Scope.** The pack algorithm of [AR §7.4] (classes C1–C8 as named queries, byte budgets, per-class quotas, degrade before drop, `~main` rules, conflicted knowledge as one line with the base text) and `brief`; **FL-9** per [40 §6.2, §6.4] (the pack header's link segment and statuses, the `SessionStart` settle within its hard budget, the evidence hooks); hooks [AR §7.5] on the command transport, their logic shared with the `mcp_tool` handlers M10 adds (`hooks.transport`), and the git hook blocks (not installed by default); one `hooks.<hook>.enabled` key per hook ([AR §13]); `moirai hooks install` registers only the hooks enabled in config and `doctor hooks` reports any difference; the lease-role identity plumbing for the role policy ([90 §4.3]); three skills (core `moirai` ≤ 800 tokens with the reporting protocol and the file-link card, `moirai-orchestrate` ≤ 2,000 tokens with the branch ritual, the `moirai-ql` card and `reference-ql.md` (LQ-12)); `export md`, `memory-md` (a pointer line while hooks run) and `rules` (with `paths:`); `apply --from-journal`; the `SubagentStart` session marks; per-role pack budgets in bytes with the CLI ceiling; the token ledger; the import tooling for the standing rules, current pins, live lanes and open owner questions with the rule-text overlap report, and **`links import`** ([AR §11] #23: it runs at the cutover, after the owner reviews its ambiguous list; [74 A16]).

**Harness-agnostic scope** ([90 §10.2]): `moirai integrate` for `claude`, `codex` and `generic` (registry, Markdown and JSON renderers, the Claude and Codex plugins, `--check`, `--remove`, `--print`, records, `doctor agents|hooks|sandbox`); the portable skill rendering beside the plugin; the `AGENTS.md` block and the `CLAUDE.md` import; Codex hooks on the command transport; the worker-pack rule and the orchestrator-lease mint; the orchestrate skill's first step with `image export --if-older` and the merge ritual's export and `sync --check` (the hookless fallbacks of [90 §2.5]); `apply --from claude-journal:`; the ledger per harness; Tier B templates only on demand ([AR §11] #45: none built by default) (+ 4–6 units, in §7). The exit adds the GT12 golden files of every rendering, the command-transport hook fixtures in Codex as in Claude Code, and the ledger rows of [90 §9.3] for both Tier A harnesses.

**Not built yet.** MCP tools.

**Exit criteria.** Pack/brief engine time ≤ 8 ms at 1e5 and ≤ 12 ms at 1e6, a lane pack ≤ 20 ms at 1e5; brief ≤ 8,000 B; hook deltas ≤ 600 B; the command-transport hook budgets of [AR §8.3] (`SessionStart` ≤ 300 ms p50 / 500 ms p99; `SubagentStart` ≤ 150 ms p99; `UserPromptSubmit` ≤ 120 ms p99; stamp ≤ 110 ms p99) under a 16-agent burst with `hooks.transport = command`; every TOKENS row of [AR §8.3] that the CLI and the command hooks carry, on the token ledger, incl. a CLI pack at its ceiling arriving inline and whole through the then-current Bash tool and per-role pack budgets set by ≥ 20 recorded dispatches (≥ 90 % complete without `--more`, median injected tokens ≤ the M0 baseline); `SessionStart` settle ≤ 150 ms and link work in a 50-link pack ≤ 3 ms p50 warm ([40 §7.4]); hooks fail open (store missing, locked or corrupt → empty output, exit 0, inside the timeout); pack candidate-class sets equal the model's evaluation of the class queries; on at least three recorded dispatches the owner judges the pack complete against the HDR that was actually used; the hook experiment re-run on the then-current Claude Code; the `moirai-ql` card ≤ 1,000 tokens by the tokenizer and LQ-Bench on the real binary with it; an import dry-run on a copy of the owner's content produces the owner-reviewed node set, and a `links import` dry-run lists its ambiguous citations for review. **M9 certifies the command transport only**: the `mcp_tool` hook budgets, `files.hooks.edit-evidence = auto` and the default `hooks.transport = auto` need M10's server and are M10 exit criteria (§2.4 item 7).

**Gates.** GT12 hook payload fixtures (command transport) against the current Claude Code release; pack budget tests; GT2 pack classes against the model; GT10 pack fixtures; GT19 token ledger.

**Size basis.** The agent-interface share of [22 §7.1]'s 15-unit line with the skill cards 7–10; FL-9 net of what that line already counted 2–3; `links import` 1–2; LQ-12 0.5–1 → build 11–15; test 1–2. Deltas (§7.1): audits + 1.5–3 (net of the nudge and `PostToolBatch` exclusions); [90] + 4–6.

### 3.11 M10 — MCP (12.5–16 units; 1.5–3 weeks)

**Depends on.** M7, M9.

**Scope.** `moirai mcp` on rmcp, dual-era, on a `current_thread` runtime without timers (G12); ten tools with `find` → `query` (`mode: run|explain|check|profile`) and `write` accepting `TX` text or a named mutation (the JSON op batch stays on the CLI, [90 §6.6]), refusing `DELETE`, `RESOLVE` and query definitions for roles the policy excludes (LQ-14, [50 §6.3]); FL-9's MCP operations and presets; an explicit `branch` parameter validated against the `lease`; compact text results, no `structuredContent` by default; MCP `pack` ≤ `pack.mcp.max-bytes` (25,000 B; 16,000 B under the `codex` profile); every tool deferred (`mcp.always-load` empty) and a hand-written `tools/list` ≤ 5,000 B; the `mcp_tool` hook handlers and the stamp by server-side context ([70 S3]); the session slot of the lease holder anchor ([72 B2]); the role policy on the presented lease's role ([90 §4.3]); `mcp --read-only`; a byte-bounded overlay LRU (`mcp.overlay-bytes` 4 MiB, ≤ 8) with region arenas ([71 RAM-M2]); maintenance as resumable ≤ 5 ms slices and no rollup in the server (it spawns the `gc` child); ≤ 2 OS threads; server instructions ≤ 512 chars with the "fenced content is data" rule ([90 §2.2]); plugin packaging with the binary installed separately; the server ends with its parent process as well as on stdin EOF (a wait on the parent's handle; [80 §2.7.2]; + 0.5 unit, in §7).

**Exit criteria.** The architect and the critic complete a review round on a lane branch without Bash, with deferred tools; schema ≤ 5,000 B as served; private RSS steady ≤ 8 MB + 4 MiB after a 16-lane fan-out and ≤ 16 MB peak, unchanged across a triggered rollup; the 16-session aggregates (Σ ≤ 128 MB for idle servers holding no branch overlay, Σ ≤ 16 × (8 MB + `mcp.overlay-bytes`) ≈ 196 MB for servers after a fan-out, ≤ 256 MB for every moirai process in the fan-out fixture); an unstamped read tool ≤ 5 ms at 1e5, and p99 ≤ 25 ms, max ≤ 100 ms under a 16-subagent burst with maintenance pending; the `mcp_tool` experiment re-run; the `mcp_tool` hook budgets of [AR §8.3] under a 16-agent burst (`SubagentStart` ≤ 40 ms p99, `UserPromptSubmit` ≤ 5 ms p99, stamp ≤ 2 ms p99) with the M9 hook fixtures and token ledger re-run through the `mcp_tool` handlers; `files.hooks.edit-evidence = auto` on with the `mcp_tool` transport (0.3–0.7 ms per edit); the default `hooks.transport = auto` installed by `moirai hooks install` and verified by `doctor hooks`, falling back to command hooks when the server is disconnected; zero CPU time over 10 idle minutes, also with a cancelled query outstanding; an MCP server held open through a GT4 kill loop never serves a record past `committed_lsn`; LQ-Bench through MCP.

**Gates.** GT12 conformance for both handshakes (legacy including 2025-06-18, and 2026-07-28), the `structuredContent` regression and the `mcp_tool` hook behaviour, in Claude Code, Codex and a scripted generic stdio client, with the MPSP lint ([90 §10.4]); the harness-agnostic scope of [90 §10.2] — portable schemas, `_meta` context, `codex/sandbox-state-meta` and store discovery by `tree`, annotations, `clientInfo` profiles, `format: "json"`, `--tools`, lazy open, the thread-anchored lazy slot and `session-ttl` renewal, release at request end, Codex's `mcp_tool` handlers, the spawn-to-`initialize` and per-thread RAM gates (+ 3.5–5 units, in §7); GT4 with the server held open — mandatory from here; the I26′ door test re-run through `write(name)`, `write(TX)` and `complete`.

**Size basis.** MCP 5 units [22 §7.1] + the `query`/`TX` tools 1–2 → build 6–7; test ≈ 1. Deltas (§7.1): audits + 1.5–2.5; cross-platform + 0.5; [90] + 3.5–5.

### 3.12 M11 — Release hardening (13–24 units; 4–7.5 weeks, 4–8.5 in profile L)

**Depends on.** M0–M10.

**Scope.** Everything needed to meet the release gate of §6 and nothing else: the 72-hour soak (with the 16-session RAM aggregate); the synthetic campaign replay with the token ledger; the independent three-lens review of the implementation and the fixes it demands; the release build (stable install path, PE metadata; unsigned unless M0 item 11 showed that signing matters, [AR §11] #39); the operator documentation (recovery runbooks, upgrade and rollback); **the upgrade drill** to a synthetic format version 2 and its rollback drill (RG10); the cutover rehearsal on a copy of the owner's content.

**Not built.** New features. A finding that needs one is an owner decision (P8).

**Exit criteria.** The release gate (§6).

**Size basis.** Review, fixes, documentation and drills 11–20 units. **Calendar**: those units, then 14 consecutive clean nightly runs and the 72-hour soak **on the release-candidate commit** (≈ 2.5 weeks with a test host; 2.5–3.5 weeks in profile L, where the nights are completed runs in agent-free windows and the soak runs inside a 3-day agent freeze, [AR §11] #34; GT15's last 1,000 cycles on the release candidate left with the deferred rig, and the 14 runs of GT3, GT4 and GT17 still bind), so 4–6.5 weeks before the deltas; a change after that restarts the gates whose covered crates' source hashes changed ([74 A12]; §6 RG3) [61 M-8, m-6]. Deltas (§7.1): audits + 1–2; [90] + 1–2 — with them 13–24 units and 4–7.5 weeks, 4–8.5 in profile L.

### 3.13 Gate catalogue

A mandatory gate runs nightly with long seeds on the owner's laptop in its agent-free windows (profile L, [AR §11] #34) and, where it needs neither the owner's machine nor owner data, with short seeds in PR-level CI on hosted GitHub Actions Windows runners (#36) from its milestone on; the timing, RAM, floor, crash and kill-loop gates (GT11, GT4, GT17 on real NTFS; GT15 after the release) and every gate on owner-derived data (GT7 on the owner's repositories, GT10's owner-derived fixtures, GT13) run only on the laptop. In the Runs column, **CI** means those hosted PR checks — synthetic data only, no model API key — and **nightly**, **per exit**, **per change** and **release** mean runs on the laptop unless the row says otherwise; a cell that names both says which part runs where. A failure blocks the exit of that milestone and of every later one.

| Gate | What | Mandatory from | Runs |
|---|---|---|---|
| **GT1** crash-point enumeration | one process over the in-memory `Vfs` under the fault model: a crash at every write/flush/publish boundary incl. between the appends of one flushed group (N3); at each crash point **every subset** of a file's unflushed sectors while there are ≤ 12, ≥ 10⁴ random subsets beyond, both `HEAD` slots exhaustively (9 states at every barrier point), bounded cross-file products, torn sectors, "flush error, more commits, crash" sequences, and several pending groups and flush holders — failed flushes followed by reverted, invalidated or evicted pages while appends and idempotent retries continue (group commit, fault-model items 3, 11, 12); namespace operations per fault-model item (2); disk-full injection; recovery checked against the model's acknowledged-durable-effect semantics (§4.4), markers and leases included, plus **post-crash read freshness** before any writer runs; per-file prefixes are the fast PR tier ([72 B1, M1–M3]) | M1 (toy log at M0) | CI (prefix tier) + nightly (full) |
| **GT2** differential against the reference model | seeded `Store` API command streams on engine and model; exit class, result data, commit ids and `state(ref)` digests compared; thresholds swept over the test profile and production values; storage level (M1), semantics (M2), version control (M3), file links (M6), queries (M7, AST-generated), pack classes (M9); the caller-context resolver of [90 §4.1] as model data, its branch order of record included (an MCP call with `lease` and no `branch`) (M8) | M1, extended per milestone | CI + nightly |
| **GT3** multi-process simulation | N simulated processes under seeded scheduling with lock-free readers, checkpoints, GC, pins, promotion and long-lived readers; crash, fsync-error, lock-release-delay, sharing-violation, delete-pending, pause, disk-full, clock-step and mixed-sector-read injection; the model re-evaluates every read at its `seq` | M1 (≥ 1e6 steps at exit; ≥ 1e7 per night for RG3) | nightly |
| **GT4** Windows kill loop | 16 writer and reader processes at 1e4 nodes (a 1e5 variant checks presence and `doctor --verify` only), 10,000 iterations / 1 h per variant: `TerminateProcess` at random points; `NtSuspendProcess` of random processes for 1–120 s (incl. the writer-byte holder, the flush-byte holder, the maintenance-byte holder mid-checkpoint, readers across the GC grace); wall-clock steps of ±1 h. Every process reports each acknowledged **durable effect** by kind — commit, lease token, ref move, marker effect, intent outcome, `gitmap` entry, backup — to the harness over a pipe before printing it; zero lost acknowledged effects, zero corrupt opens, `doctor --verify` clean, a mid-loop `backup` restores every commit acknowledged before its `committed_lsn` (CM8); lease variants — a CLI-taken lease survives the CLI's exit, a restricted second principal yields Unknown and never an expiry, ±1 h clock steps change no expiry ([72 B2, M12]); in profile L ([AR §11] #34) GT4 has three nightly variants — `TerminateProcess`, `NtSuspendProcess` and the clock steps — and disk-full on real NTFS is not one, because attaching a small VHDX needs elevation on Windows Home and a nightly job never elevates ([74 A20]): GT1 and GT3 inject disk-full under the fault model, and the owner runs RG7's disk-full drill on an owner-created small VHDX; the model re-evaluates every read made after a recovery and a 1-in-16 sample of the others. The Unix kill loops are port gates ([80 §5.3]) | M1 (storage driver), M3 (VCS driver, 16 bound directories), M5 (exports and imports in the streams), M6 (file intents), M8 (CLI binary), M10 (MCP server held open) | nightly |
| **GT5** fuzzers | FL-1 anchor selectors and path specs (M0); records, `HEAD`, segments (M1); git objects, packs, idx, commit-graph, refs, reftable, bundles (M4); `.moi` incl. anchor lines (M5); fingerprints (M6); query grammar (M7); argv/transport (M8); MCP JSON (M10) | per component | continuous on idle cores |
| **GT6** properties | derived state == recomputation; reverse CSR == inverse; invert(changeset); pin ⊕ ops == replay; merge determinism and commutation; I12, I25′, I26′, I31′ (incl. both-sides-equal clean), I37′; CM1/CM5/CM9 shapes; X1 merge variant; FL-3/FL-7 properties | M2, M3 | CI + nightly |
| **GT7** git CLI as independent oracle | `git fsck --strict`, `verify-pack`, `cat-file --batch`, `merge-base`, `diff-tree` agreement; shallow and partial clones, replace refs and grafts in the corpus with `unknown`, never `false`, across shallow boundaries ([72 m7]) | M4, M5 | CI (synthetic repositories) + laptop (the owner's repositories, every worktree HEAD) |
| **GT8** image gates 0–3 | [AR §5b.7] over the M0 carrier table, with anchors, identity derivation and (from M7) named queries, in both anchor-text modes; import onto a diverged ref ([72 M6, M10]) | M5 | CI |
| **GT9** query metamorphic and budget replay | `p` / `NOT p` partition equality; same stop point and cursor; pages never skip or repeat | M7 | CI |
| **GT10** owner-verified fixtures | node-40 table, register incidents, the [AR §7.6] walk-through, git-side merge of counters, R4 corpora, recorded HDRs vs packs; expectations written from the specification, the core set **verified by the owner** (M0), the rest by an author who sees neither engine nor model code | from the milestone that builds each feature | CI (synthetic fixtures: the node-40 table, the walk-through, the git-side merge of counters) + laptop (register incidents, R4 corpora, recorded HDRs; [AR §11] #37) |
| **GT11** budgets | [AR §8.3] (per priority) and §5.4 (per milestone), measured under §5.1 with sample sizes tiered by duration; runs exclusively, first in the night | per row | the laptop's nightly windows (profile L) and each exit |
| **GT12** contract and harness conformance | CLI golden outputs and transport (M8); hook payload fixtures, zero-output hooks, the Bash tool's inline and failure caps against the pack ceiling, the Workflow `journal.jsonl` format (M9); the `mcp_tool` hook behaviour, MCP handshakes and `structuredContent` (M10) — re-run on every Claude Code and Codex release the owner adopts; from M8–M10 also the harness conformance of [90 §10.4] (Codex; a scripted generic stdio client for C0), with a hookless C0 fixture asserting an image export within one working session ([90 §2.5]); cmd for the documented human forms, per [80 §4] T1–T10; bash, dash and zsh in the port | M8 | CI (golden files, shell transport, stub-server handshakes) + laptop (live Claude Code and Codex conformance with the owner's installations) |
| **GT13** query accuracy (LQ-Bench) | [50 §7.4] with its gates — item 6 with [AR §7.7.5], the normative list (A6) — and ablations, run at M0 on the model's own parser, binder and evaluator (LQ-3), on Opus 5.5 through the owner's Claude Code subscription in headless mode with the Claude Code and generic-client transport arms ([AR §11] #38 (a) as amended by the owner review of 2026-09-27; [90 §8.3]); the v2 tiers of [90 §8.3] only if a later decision adds a model | M0 (on the model: surface freeze); re-run on the product (M7), the binary (M8), MCP (M10), on every card/grammar/error-text change and before release | per change |
| **GT14** soak | 72 h of mixed agent-like load with kill loops across branches, nightly image export and backup, `doctor --verify` every hour | M11 | release |
| **GT15** OS-crash loop — **deferred to after the release** by the owner review of 2026-09-27 (V7), together with the port phase; the specification is kept, and every format field and protocol rule it tests stays frozen | a Windows Server 2025 Core evaluation guest (VirtualBox 7, VMware Workstation Pro as the cross-check; host I/O cache off; guest flushes honoured; one guest on the rig host — the owner's laptop under profile L, [AR §11] #34) runs GT4's per-milestone workload with evidence-hook bursts just before each power-off; every writer streams each acknowledged durable effect to the host over a socket; the host triggers a bugcheck (NotMyFault) or a hard power-off at random; after reboot every recorded effect must be present, reads before the first writer fresh, `doctor --verify` clean, `FsIntent` outcomes equal to the file system, the image `git fsck --strict` clean with `gitmap` ⊆ destination, and the last backup restorable ([72 M12]). The rig cannot observe loss of issued-but-unflushed writes or of the drive's volatile cache: GT1 alone certifies flush placement, and the design assumes the drive honours FLUSH ([74 A18]). Linux and macOS rigs: [80 §5.3] (port phase) | after the release (≥ 1,000 cycles, then ≥ 5,000 cumulative; the oracle as extended for M5 and M6); formerly M1 | the post-release rig phase |
| **GT16** mutation testing | mutants applied only in test jobs (the laptop's windows or, on synthetic suites, the hosted runners, §3.15), never compiled into a product build; every mutant in functions changed since the previous exit plus a random pooled sample of 1,000 mutants over the semantic crates (graph, VCS and merge, link resolver, binder and executor, image codec, FL-1); the differential and property suites must kill a point estimate ≥ 90 % with a lower 95 % bound ≥ 88 % of the compiling, non-equivalent mutants (≈ 6–29 h of wall time per exit, [74 A05]); storage crates are gated by the seeded protocol bugs of M1 and report their kill rate | every exit from M0 | per exit |
| **GT17** R4 pattern matrix | [40 §8.3.1] rows 1–36 on the `ProjectFs` simulator and on real NTFS in scratch directories created by the harness, never in a user repository | M6 (driver), M8 (binary) | nightly |
| **GT18** state oracles (new) | the I26′ state oracle (≥ 10⁶ histories, ≥ 5 refs), sync residue equivalence (≥ 10⁵ cases), uid → `#N` uniqueness, settle concurrency, lease liveness (with TTL renewal by lease-presenting writes and `heartbeat`, and a hookless self-claim worked past its TTL, [90 §4.4]), lease-first branch resolution (an MCP call with `lease` and no `branch`), `TX` target integrity, verb = named mutation ([72 §8]). The model is single-process (§4.1), so at M0 these rows assert the rules over the interleavings the model enumerates — the settle CAS-drop rule, the lease TTL, renewal and `Unknown` rules, lease-first resolution — against the model's own from-scratch definitions; the concurrent variants (settle concurrency under GT3 with 16 processes, lease liveness under the kill loops) start at M2 and M6 (the A1 re-review A-m9) | M0 (model), M2, M3, M6, M7, M8 | CI + nightly |
| **GT19** token ledger (new) | every byte moirai places into an agent context, per spawn and per session, against the TOKENS rows of [AR §8.3], with the tokenizer of the model each harness runs on synthetic fixture text, per harness; shared static text on the maximum of the Claude and o200k families ([73 F8], [90 §9.4]) | M9 (hook fixtures, command transport), M10 (`mcp_tool`), M11 (campaign replay) | per exit |
| **GT20** git independence and dependency lints (new, §10.6) | (a) spawn lint: no `git` spawn outside the four call sites of [AR §5c] (`image push/pull`, a reftable ref update, `doctor lanes --refresh-graph`, `file mv --git`) and no other process spawn except the detached `moirai gc` child; (b) dependency lint: no git library (`gix*`, `git2`, `libgit2-sys`) and no embedded-database crate (SQLite bindings, `redb`, `heed`/`lmdb*`, `fjall`, `sled`, `rocksdb`, `libsql`/Turso) in any `Cargo.lock` the project keeps — it enforces owner decision 1 as well as R2; (c) the GT2 and GT4 streams pass with `git` removed from `PATH` and no `.git` present, `stale`/`check` answering `unknown`; (d) OS-layer lint: `cfg(target_os)`, `cfg(windows)`, `cfg(unix)`, `std::os::windows` and `std::os::unix` only in `moirai-os` (a source scan); `windows-sys` and `libc` declared as direct dependencies only by `moirai-os` and the binary's entry glue, third-party transitive use (tokio in the MCP front-end) only through a reviewed allow-list (`cargo metadata`); no `File::lock` and no `std::fs::rename` on store or project files ([80 §5.5]), and in product crates other than `moirai-os` no direct `std::fs` file I/O (`File::open`, `OpenOptions`, `read_dir`, `metadata`, `remove_file`, …), so every project file is read through `ProjectFs` (the A1 re-review A1P-10; test and tool crates through a reviewed allow-list); (e) cross-target type check (owner decision #44, [90 §11]): `cargo check --workspace --all-targets --locked` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` and `aarch64-apple-darwin` beside the Windows host, every target's C, C++ and assembler poisoned, the binary crate (a composition root only) and reviewed host-only crates excluded; (b) also requires a reviewed entry for every build script, forbids `links` and native build dependencies outside it, and forbids any checked crate to depend on a host-only crate, dev-dependencies included | (b) and (e) M0, (a) M1 (call sites at M4, M6, M8), (c) M4 (GT2), M8 (GT4), (d) M1 | CI and the local pre-merge gate (a, b, d, e); nightly (c) |

### 3.14 Decisions due before each milestone

Rule (P2): a decision is due before the first milestone whose format, protocol, code or spending it can change. Only what no configuration key can change later is an owner decision ([AR §11]); every operational policy is a `config` key or a policy-data row with a documented default that the reference model implements for every allowed value ([AR §13]) — 25 former owner questions became such keys (14 at the R4/R5 integration, 11 in the priority-audit pass, [74 §5.2]). Numbers refer to [AR §11], which carries the decisions and their consequences. **On 2026-09-26 the owner answered every open decision** (the answers above): #2, #34, #36 and #38 as the table states, every other one as recommended ("Everything else I approve as you wrote it"). Nothing is left open before any milestone of M0–M11; #42, #45's on-demand items and #21 (d) remain for later, with their defaults recorded. The owner review of 2026-09-27 ([AR] binding inputs) confirmed every decision and amended #34 (V7) and #38 (V1), as the rows state.

| Applies from | Decision | Decided (2026-09-26 unless stated) |
|---|---|---|
| decided 2026-09-26 | #32 Linux and macOS ([80], [AR §11], [AR §14]) | designed now, Windows built and gated in M0–M11, Linux and macOS in an unscheduled port phase |
| decided 2026-09-26 | #43 harness-agnostic agent interface ([90]) | contract C0; Tier A Claude Code and Codex |
| decided 2026-09-26 | #44 cross-target type check as a gate from M0; pure-Rust dependencies ([90 §11]) | GT20 (e) and the pure-Rust lint from M0 |
| M0 | #1 coordination liveness (`shared` field class) | isolation + markers; `shared` not built (a later choice is a format-v2 change with the RG10 procedure) |
| M0 | #2's remaining call: lanes and machines | **two lanes, both building on the laptop**, in windows agreed with the owner ("Two lanes in parallel."); M0 item 21 sets the build cap; the one-lane calendar if two do not fit (§7.1) |
| M0 | #5 branch per lane and the R1 verb set | per lane; `tag`, `undo`, `revert`, `cherry-pick`, `reflog`, `op log`/`op restore`; no `rebase` |
| M0 | #11 cross-machine writers (identity) | no; `uid` stored from M0 |
| M0 | #17 one store per repository (store layout, `#N` space, writer-lock scope) | per repository |
| M0 | #20, #21 (a)–(c), #22 ([40] #13, #3, #9) | refuse; no/no/no; hand-written scanners, tree-sitter test-only |
| M0 | #24–#29, #31 ([50] D1, D4 (location), D5 + D13, D7, D8, D11, D10) | layered `TX`; in the store; the traced subset; `ready` + `unblocked`; two-valued; Cypher/GQL bags; Lachesis |
| M0 | #33 erasing history ([16 §11 Q5]; absorbs `gc --squash-before`) | no erase; bodies droppable by hash without changing commit ids |
| M0 entry | #34 test infrastructure (money; provisioning is M0's first task, §3.1 item 7) | **profile L: no test host, no guest licence** ("For now no additional machine will be used, everything is here."); ≈ 5 agent-free nights a week from M1 exit (≈ 145–195), a 3-day agent freeze per release-candidate iteration, ≈ + 3.5 weeks at P50 against a test host (§7.1); **amended 2026-09-27** (V7): the OS-crash rig — GT15, its calibration, the guest licence, the WSL2 and Memory Integrity choices — deferred to after the release with the port phase |
| M0 | #35 recording the 16-agent load fixture (consent; resource profile only) | yes; it stays local |
| M0 | #36 repository hosting and CI (the source leaving the machine) | **public GitHub, `github.com/bluesteelll/moirai`** ("…stored in a public repository on GitHub."); hosted Windows runners for the synthetic checks; every commit authored by the owner with no AI co-author (§3.1 item 7) |
| M0 | #37 residency of owner-derived corpora and fixtures | never in the repository or on hosted runners; a gitignored local directory on the laptop |
| M0 | #38 LQ-Bench models, budget and vendors (reopened by #43, [90 §8.4]) | **(a): Opus 5.5 only** ("For now benchmarks only on Opus 5.5."); the Claude Code and generic-client transport arms; ≈ 53 M tokens (before Claude Code's per-call overhead, measured in M0's first usage window); **amended 2026-09-27** (V1): through the owner's Claude Code subscription in headless mode, no API billing and no API key ([90 §8.3]); the `codex` client `unknown` until a later decision adds its model; GT12's Codex conformance and probes P4 and P10 ([90 §10.5]) still drive the owner's Codex model, as contract tests and measurements, not accuracy benchmarks, and gate nothing on LQ accuracy ([90 §8.4]) |
| M1 | leader in or out; T1 structure | decided by M0 measurements, not by the owner |
| M4 | #3 git object I/O (code boundary; spawning git for transport is `image.transport.spawn-git`) | the in-process layer (§1.3) |
| M5 | #16 may data leave the machine (enforced by `image.allowed-remotes`) | private remote only; the list starts empty |
| M7 | #30 lease history | not stored |
| M9 | #8 prose in the repository; #15 migration and generated views (tooling; the run is at M11); #18 native Tasks; #23 `links import` | store only; as stated; ignore; `links import` built, run at the cutover after the owner reviews the ambiguous list |
| M4–M10 | #41 scope exclusions made by the design team ([AR §11]): reftable writing and transport without git (M4), the tracked-directory, orphan-branch and `refs/moirai/*` destinations (M5), R4's USN evidence E2 (M6), the move nudge and the `PostToolBatch` delta (M9), HTTP MCP (M10) | not built; reservations kept; a later "build" brings its key (`files.journal = auto\|off` — OS-neutral, since E2 is the USN journal on Windows and FSEvents on macOS — `files.hooks.nudge`, `hooks.post-tool-batch.delta`) |
| M11 | #39 code signing; #40 cutover date | unsigned at a stable path unless M0 item 11 shows that signing matters (the purchase then returns to the owner); a campaign boundary after the rehearsal |
| later: on demand, M9 at the earliest | #45 harness scope: Tier B templates, Codex cloud, a `moirai dispatch` wrapper, the plugin package and extra export formats, `codex-csv` and `--structured` ([90 §10.7]) | none built by default (confirmed): the on-demand items are later owner calls; Codex cloud out; the recipe only |
| later: port phase | #42 port-phase hardware, CI and platform coverage (money; [80 §5.4]) | default recorded: a Mac mini, free hosted Linux runners with `/dev/kvm` (the repository is public), floor-grade Linux on a dual-boot partition of the laptop unless the port buys a machine, arm64-only macOS |
| later: macOS port | #21 (d) macOS document ids | default recorded: no |

The former rows that are now configuration or policy — #4, #6, #7, #9's policy half, #10, #12, #13, #14's retention half, #19, the spawn half of #3, the definer half of #25, [40 §9.2] #1, #2, #4, #5, #6, #7, #8, #14, #15, #16 and [50] D2, D3, D6, D12 — are specified, not decided: their keys, types, defaults, scopes and reload classes are in [AR §13], and the reference model implements every allowed value from M0. [40] #10 and #11, #9's lease rule (I32′) and #12's acknowledged-means-flushed contract are design rules.

### 3.15 Owner hours and machine time

**Owner hours** (est.; excluding the supervision of the build lanes, which the rate of 5–8 units per week already assumes, [61 M-8]):

| Item | When | Hours |
|---|---|---|
| The decisions of §3.14 — answered on 2026-09-26; what remains is #42, #45's on-demand items, #21 (d) and any re-ask a measurement triggers (#39's signing; #34's rig licence, after the release); 25 former questions are documented `config` defaults that need no decision | port phase; on demand | 2–6 |
| Profile L and the public repository: granting ≈ 5 agent-free windows a week and the 3-day freezes, the owner-run VHDX setup (for measurements 18 and 22 and the RG7 drill), the review of the published tree (done and accepted on 2026-09-27, A3) | M0; weekly from M1 exit | 3–7 |
| The M0 specification review: format, fault model and protocol decisions, query surface, R4 reservations | M0 | 8–16 |
| Rule-table sign-offs (merge table, status machines, delete-policy matrix, link merge rules, pack classes) | M0 in full; 1–2 h at each later exit that changes a table | 15–25 |
| GT10 core fixtures and a sample of LQ-Bench's gold results | M0 | 10–20 |
| Pack-vs-HDR judgements on recorded dispatches | M9 | 3–6 |
| Campaign-replay sign-off (RG8), cutover rehearsal (RG11), upgrade-drill review (RG10) | M11 | 6–12 |
| Velocity reviews and the calendar re-issues | M0, M1 exits | 2–4 |
| **Total** | | **≈ 50–95** |

**Machine time** (est.):

| Resource | Use | Budget |
|---|---|---|
| Laptop, profile L ([AR §11] #34: the owner's Windows 11 laptop, Ryzen 9 5900HS, 8C/16T, 16 GB, consumer NVMe without PLP, Defender on) | **night schedule** ([74 A05]) in owner-granted agent-free windows (≈ 5 a week from M1 exit to release, ≈ 145–195 nights): GT11 exclusively first (≤ 2 h, sample sizes tiered by duration), then concurrently with CPU and RAM caps GT4 in three variants (≈ 3 h) then GT17, GT3 (≥ 1e7 steps), and fuzzers on the remaining threads; the 1e6 and 0.3–0.5 M rows weekly; the 72-hour soak inside a 3-day agent freeze per release-candidate iteration; by day the two build lanes in agreed windows ([AR §11] #2; build semaphore, `CARGO_BUILD_JOBS` cap, no `rust-analyzer` in lane worktrees), fuzzers sanitizer-off with `-rss_limit_mb=256` at ≤ 2 targets, gate jobs beside the agents ≤ 1 GB in total, everything refused below 1.5 GB free | ≈ 8 h per window; ≈ + 3.5 weeks on the two-lane release at P50 (§7.1) |
| Mutation testing | in-diff mutants + 1,000 pooled sampled mutants per exit, 4 parallel jobs in the laptop's windows, or sharded across the hosted runners (synthetic suites only, [AR §11] #36) | ≈ 6–29 h wall per exit |
| Disk ([74 A24]) | target directories, 1e6 stores, image repositories, corpora (a VM guest image only in the post-release rig phase) | ≤ 25 GB on the laptop (no VM image kept between windows), beside each lane's `target` directory; the harness refuses to start below 25 GB free |
| Fuzzing | RG5: ≈ 18 targets × 7 CPU-days | ≈ 3,000 CPU-hours cumulative: ≈ 9 window-weeks at 8 threads × 8 h × 5, plus ≤ 2 targets by day, spread over the build and off the critical path; optionally on the hosted runners ([AR §11] #36) |
| OS-crash loop | deferred to after the release by the owner review of 2026-09-27: ≥ 1,000 cycles at ≈ 3–5 min per cycle (≈ 50–83 h on one guest, ≈ 1.25–2.1 weeks at 5 laptop windows a week), then ≥ 5,000 cumulative | none in M0–M11; ≈ 250–400 guest-hours in the post-release phase |
| Owner's laptop | exit measurements and the nightly jobs (profile L), in agreed windows | 1–2 windows of 4–8 h per milestone exit; with tiered sample sizes an exit session stays ≤ 8 h (M5's export and import rows take ≈ 20 min instead of 8–28 h, [74 A04]) |
| Hosted GitHub Actions Windows runners (public repository, [AR §11] #36) | PR checks: the build, the lints, GT20 (e), the unit, property and differential suites with short seeds on synthetic data; never a timing, RAM, floor, crash or kill-loop gate; no owner data, no API key | $0 (free for a public repository) |
| Model calls | LQ-Bench as [AR §11] #38 (a) decided: 520 prompts on Opus 5.5 for the baseline, the D8 and D11 ablations, the two alternative surfaces and the display-spelling ablation, a 260-prompt stratified half for the other ablations, and the Claude Code and generic-client transport arms, ≈ 53 M tokens at M0, mostly cached input, plus ≈ 1 M for the repeated 52-prompt sample (the neutral-API estimate, before Claude Code's per-call overhead; M0's first usage window measures the overhead and re-issues the quota plan, with [50 §7.4] item 5's shrink rule as the fallback), through the owner's Claude Code subscription in headless mode across several usage windows inside M0 (no API key; ≈ $280 at list prices only as a reference); re-runs at M7, M8, M10, on surface changes (≈ 5 M) and before release (≈ 13 M) | the subscription's quota within its weekly limits (#38 (a) as amended by the owner review of 2026-09-27, V1) |

---

## 4. The Rust reference model

### 4.1 Role

The model is a small, obviously correct, **executable specification** of moirai's logical semantics. It lives in the `moirai-model` crate (`publish = false`) beside a `moirai-format-oracle` crate (an independent decoder of every frozen structure); both are dev-dependencies of the test crates only, **never linked into the binary and never a backend**. The model implements the same logical `Store` API as the engine. It replaces SQLite in both roles the oracle-first plan gave SQLite — correctness reference and conservative semantics — and adds a third: it is where every rule is written first. It is **written first and complete, at M0**, by an author separate from the engine's; LQ-Bench runs on it before the query surface freezes (§3.1), and after M0 it changes only through specification findings.

It is neither a performance reference (§5 is) nor a multi-process reference: concurrency and crash correctness come from GT1/GT3/GT4 (and GT15 after the release), which check the engine against the model's *acknowledged-commit* semantics (§4.4).

### 4.2 Scope and the deliberately different algorithm

Wherever the engine uses an optimised or incremental mechanism, the model uses the **definitional** algorithm from [AR], [40] or [50]. A disagreement is then either an engine bug or a specification ambiguity, and both are findings.

| Concern | Engine | Reference model |
|---|---|---|
| Current state | base + delta segments + overlay | `BTreeMap<#N, Node>` per state, built by folding net changesets from genesis |
| Derived state (the structural predicate, `ready`, `open_blockers(_exo)`, `is_blocker`, rollups, `suspect`, `answered`, `conflicted`) | maintained eagerly for touched nodes; frozen bitsets | **recomputed from scratch on every query**, each predicate a literal transcription of [AR §3.5] and [50 §3.8] |
| Precedence acyclicity (I5′) | Pearce–Kelly; full Kahn above the edge-count parameter | DFS over the whole combined graph with implied exogenous edges re-derived by definition |
| Branch view | `SEG(pin) ⊕ ops(main, (P, fork]) ⊕ own ops`, sync by reference, promotion | `state_at(tip)` by replaying the commit DAG from the root, memoised at tips and at every k-th commit with replay between them, ≤ 512 MB per case ([74 A21]) |
| Cross-branch exclusion (I26′) | markers as a cache — absorbed vectors, O(1) (CM1); keyed `(#N, ref_id, commit)`; recomputed on `undo`, `op restore`, `-D` | **the state definition itself** ([AR §3.4], [72 M4]): `#N` excluded on R iff some live work ref X ≠ R holds it done, cancelled or deleted at `tip(X)` by a commit that is not an ancestor-or-self of `tip(R)` — evaluated over all live refs by DFS, never from markers, so a flawed marker rule cannot be shared by engine and model |
| ahead/behind `main` | `ref_seq` arithmetic | size of the ancestor-set difference |
| LCA and the virtual base | gen-pruned bidirectional walk; pairwise typed merge of LCAs over folds | full ancestor sets; maximal common ancestors; the virtual base by materialising each LCA state and merging them with the rule table, recursively |
| Merge / sync | segment-walk folds; typed rules; staged violations | three materialised states; per-key diff; the merge table transcribed **as data**, owner-signed; validators by recomputation in I37′ order; `sync` is a merge of `main` into the lane |
| revert / cherry-pick; undo / op restore | stored before-images; `RefUpdate` + absorbed-vector restore | diff of `state_at(parent)` and `state_at(c)`; move ref pointers and emit `cleared` markers per the rules |
| Canonical op list and **commit ids** | writer coalescing; full state diff for `sync`; the product codec | `diff(state_at(first parent), state_at(c))` as a sorted key → value list (catches the CM2 class of bug), hashed by **the model's own canonical-form encoder** (≈ 300 lines), so commit ids are compared byte for byte from M1 on — an independent check of I28′ and I38′ [61 M-4] |
| Leases, fencing, idempotency, change feed, `next_id` | `LEASES`/`IDEM` sections, `HEAD.fence`, seq ring | `BTreeMap`s, counters and a `Vec` of feed entries; relevance filters by definition |
| Queries and `TX` | LQ's parser, binder, planner and vectorised executor | **its own naive parser and binder** (≈ 1–1.5k lines) and a nested-loop evaluator over the materialised state of the view; history relations by walking the DAG; `diff` by comparing two states; budgets not modelled (GT9 checks them). The generator emits ASTs; the model evaluates the AST; a printer feeds text to the product; `parse(print(ast)) == ast` is a property [61 M-4] |
| File links | the lazy cascade, runtime tables, fingerprints, anchors | link intent as data; exact-evidence resolution by definition over a simulated tree (`path → bytes`, with the creation times and file ids the `ProjectFs` simulator exposes) and **git history as abstract data** — commits with parents, committer times and `path → blob id` maps, one HEAD per simulated tree — over which the tree gate G1–G4, E6 and the writer-tree, freshness and `main`'s committed-only rules are implemented by definition, so every [40 §2.9] state has a model answer (the A1 re-review A-M4); the merge rules incl. the re-key rule and the prefix history as data, owner-signed; **a brute-force anchor resolver that enumerates every occurrence** ([40 §8.3.2] P11). The production cascade must return a subset-consistent answer — the same state or a more conservative one, never a different target. Similarity quality is checked by the GT10 corpora and GT17, not by the model |
| Packs | class quotas, degradation, rendering | candidate-class membership by evaluating the class queries (they are named queries, [50 §4.3]) |
| Image | `.moi` codec, trees, trailers | `state(ref)` for the state-identical gate, the canonical diff and commit ids for gate 0; no `.moi` bytes |
| Format | the product codec | the format oracle decodes every hand-written hex fixture and every structure the product writes in GT2 streams |

### 4.3 Out of scope

Producing on-disk bytes (the format oracle only *decodes* them), checksums, epochs and LSNs; locks, timing and processes; segments, overlays, pins, checkpoints, GC, promotion and `hist`; performance and RAM; CLI text rendering (the harness compares `--json v1` data); `.moi` bytes; the real file system; the git object layer's bytes (git history itself is model input as abstract data, §4.2). Each is covered elsewhere: hand-written fixtures and the format oracle, fuzzers, GT1/GT3/GT4 (GT15 after the release), GT7, GT8, GT17 and `doctor --verify`.

### 4.4 How engine and model are compared

1. **Generator.** A seeded, weighted mix of `Store` API commands per milestone, including invalid commands (refusals and exit classes are tested), several simulated clients with branch bindings, idempotent retries, crash-and-restart events in simulated runs, query and `TX` ASTs (M7), and file-system events from the `ProjectFs` simulator (M6). Every run draws the store parameters from the test profile or the production values (§2.5). A deterministic clock injected through the `Vfs` makes HLCs and commit ids reproducible.
2. **Lock-step execution.** Each command runs on the engine (in-memory `Vfs` in CI, real files on the laptop in the nightly windows) and on the model; compared are the exit class, the result data (`--json v1` `data` without engine-internal fields), the commit ids and, after every command, a digest of `state(ref)` for every touched ref, with a full structural comparison on mismatch and every k commands.
3. **Reads under concurrency.** Every read result names its `branch` and `rev` (seq) and, in `--json`, its `commit` ([50 §6.4]), and `rev` must be monotonic per process. GT3 re-evaluates every read on the model at its `seq`; GT4 re-evaluates every read made after a recovery and a 1-in-16 sample of the others (at 1e4 nodes; the 1e5 variant checks acknowledged-commit presence and `doctor --verify` only). A read that observed an unflushed or unpublished commit (F-A1) or went backwards is caught.
4. **Crash semantics.** For a commit in flight when a crash is injected, the model holds two candidate states, applied and not applied; after recovery the engine must equal one of them; a retry with the same idempotency key converges (I14′); an orphaned record never satisfies an idempotency lookup (I27′); **an acknowledged commit is never missing** — acknowledgements are recorded outside the store, by the harness over a pipe (GT4) or by the host over a socket (GT15, after the release), before the process prints them.
5. **Merges and recoveries.** For every merge and sync the harness compares the canonical net changeset, the set of conflict values, the set of violations, the land-or-stage decision and the staging ref, the markers written and the absorbed vector afterwards; after every GT1 recovery it compares markers and leases as well ([72 M1]).
6. **A third opinion.** At the end of each case `doctor --verify` recomputes every derived structure inside the engine; engine, model and self-check must agree.
7. **Shrinking and corpus.** Failing seeds shrink to minimal sequences and become regression fixtures; GT10 fixtures run in the same harness.
8. **Scale.** The model runs cases of ≤ 1e4 commands and ≤ 2e3 nodes. Because every threshold is a store parameter, the test profile makes every threshold-gated path — FTS tier 2, promotion, the tiered fold, `hist` retirement, checkpoints, the body threshold, the Kahn fallback, the `suspect` budget, the loose/pack threshold — reachable at model scale; nightly GT3 also sweeps the production values at 1e5 with the engine's self-check [61 M-5].

### 4.5 Guarding against a shared misunderstanding

SQLite would have been an independent implementation; the model is written from the same documents, by an agent of the same model family as the engine's author. The mitigations: **different algorithms** (§4.2); **no shared code** — the model has its own query parser and binder, its own canonical-form encoder and its own format decoder; only schema rows and the frozen specification are shared; **a separate author** — the model's author works from the specification only and never reads engine code, and vice versa (context separation between Opus agents); **owner-signed rule tables** (merge table, status machines, delete-policy matrix, link merge rules, pack classes) at M0 and at every exit that changes one — they are data, so a signature costs hours, not days; **owner-verified GT10 core fixtures**; **fully independent oracles** where they exist — the git CLI for git objects and images, the `ProjectFs` simulator's ground truth for file identity, tree-sitter (test-only) for the Rust scope scanner; **mutation testing** (GT16) and **seeded protocol bugs in the real engine** (M1) to show that the harness catches real defects, not only the toy log's; and a rule that every model–engine disagreement is triaged as a specification finding before either side changes [61 M-4].

### 4.6 How it stays small

- **Budget:** ≈ 8–10k lines at M0 (est.), all of it before the engine; after M0 it grows only through specification findings. No `unsafe`; `std` only, plus `blake3`, `sha1` and `sha2` for the hashes the specification names.
- **Asymptotics do not matter.** O(history) per query is acceptable.
- **One function per rule of [AR], [40] or [50]**, each citing its section; rules are **data tables**.
- **Coverage check at every milestone exit:** every predicate, invariant and rule of the sections a milestone implements has a model function (checked by section tags).
- **Growth review at every exit:** if the model grows past its budget it is simplified (e.g. memoisation removed), never the specification.
- **Effort** ≈ 16–20 units at M0 plus ≈ 1–2 units of maintenance inside later milestones' test shares [61 M-8].

---

## 5. Measurement and benchmark plan

### 5.1 Conditions, instruments and the measurement protocol

The owner's machine [05 §2]: Ryzen 9 5900HS, 16 GB (≈ 1.8 GB free with 16 agent processes resident [M]), consumer NVMe without power-loss protection, NTFS, Windows 11, Defender real-time on. The **measurement protocol** below is written and frozen at M0 [61 M-6]; every exit criterion that concerns time, memory or Windows behaviour is measured under it on the owner's machine, in windows agreed with the owner and never during his benchmark windows [02 §9]; nightly regression runs use the same protocol on the same machine, in owner-granted agent-free windows (profile L, [AR §11] #34).

| Element | Protocol |
|---|---|
| **Load** | "Loaded" means the **16-agent load fixture**: a recorded CPU, disk and memory profile of a real 16-agent campaign (resource usage only, no content; owner decision before M0), replayed by a load generator on the machine under test with free RAM held at ≈ 1.8 GB. "Idle" means no user process beyond the OS baseline. |
| **Sample size** | tiered by duration ([74 A04]): n ≥ 10,000 below 1 ms and ≥ 1,000 at 1–50 ms (gate on p99); ≥ 200 at 50 ms–1 s (gate on p95 and the maximum); ≥ 20 above 1 s (explicit commands; gate on the maximum); 5 repetitions below 1 s and 3 above; each gate applies to the median over the repetitions. The 1e6 and 0.3–0.5 M rows run weekly in a laptop window and at M11. |
| **Floors** | Re-measured **interleaved** with the gated operation in the same run (alternating blocks), so a slow disk day moves the floor and the operation together. |
| **Idle CPU** | Zero CPU-time delta (`GetProcessTimes`, kernel + user) **and** zero context switches (ETW) of the process over 10 minutes, measured from 15 s after its last request (a blocking-pool thread may wake once at its keep-alive, [71 RAM-m2]). |
| **RSS** | Peak private bytes (`GetProcessMemoryInfo`, `PeakPagefileUsage`) read at process exit, with the heap-only high-water mark reported beside it; for the MCP server also the **steady state** at the end of a scripted session; aggregates summed over every moirai process in the 16-session fixture; a VMMap breakdown (private, page tables, shareable) per process kind at M0 and M11; peak working set reported, not gated ([71 RAM-M2, RAM-m9]). |
| **Counts** | Flushes per commit, bytes read on open and log bytes appended by read verbs are counted by the `Vfs` instrumentation, not inferred from time. |
| **Spawn** | `hyperfine -N --warmup 5`, direct and through the agent's Git-Bash wrapper. |
| **Noise band** | For every gated quantity, the run-to-run spread of the 5 repetitions on the laptop and on the hosted runners is measured at M0 (item 16); a nightly regression beyond the band blocks; an exit decision uses the laptop's value (a hosted runner never decides a timing gate). |

### 5.2 M0 measurements and what each decides

| # | Measurement | Decides |
|---|---|---|
| 1 | open → append → data-only flush → close on a 64 MiB log file, n per §5.1 (Defender close cost, G8, U10) | whether the CLI must forward to a leader, i.e. leader in M1 (with item 2) |
| 2 | the group-commit protocol under contention: 16 writer processes through the writer and flush bytes (overlapped `LockFileEx`), last-acknowledgement p50/p99, device flushes per burst, fairness and the flush-byte hand-off latency (G1, [80 §2.4]); the toy log writes the product's `RecHdr`, chained groups and a spec-derived commit-size distribution (typical 0.3–0.6 KB, a tail up to `store.commit.inline-max-bytes`), and the run sweeps an injected in-lock CPU cost from 0 to the hold budget (p99 5 ms, max 20 ms) (the A1 re-review A1P-03) | the 2 s bounds (`lock.writer-wait-ms`, `lock.flush-wait-ms`); leader in M1 — out only if last-acknowledgement p99 ≤ 50 ms holds at the maximum in-lock cost; M1's gates re-check it |
| 3 | overlay build vs branch age: refs forked 1k / 7k / 14k / 60k commits ago with 50 concurrent writers, with and without the per-ref index (G28), and a lane synced at every `SubagentStart` for 14 days while `main` took ~500 commits a day and three lane merges ([70 S1]) | G15/G16 thresholds; `store.promotion.overlay-ops` from the 10 ms gate |
| 4 | `sync` bytes per lane per day under merge-by-reference (G28) | confirms G17 sizing |
| 5 | pinned file count and disk with 50 branches forked over 40 checkpoints (G28) | pin and GC policy |
| 6 | the codec decision of [90 §11.3]: size, encoder and decoder speed and RSS of `lz4_flex` (with and without a raw-content dictionary), `ruzstd` at its Fastest level, and the `zstd` CLI at level 1 with the same dictionary as the proxy for an own encoder, on the owner's notes and plan sections and `hist`-sized frames (U12, #44; nothing leaves the machine); bytes per token of four synthetic fixture classes for Claude (Claude Code's reported usage in headless mode, with and without the fixture — no API key exists, the owner review of 2026-09-27) and o200k and the owner's Codex model's reported usage (W-all-6, [90 §9.1]) | the codec, the dictionary and its form; token-gate conversions |
| 7 | `git` on PATH; exec-form PATH resolution of `moirai.exe` for hooks (W-all-3); whether `${CLAUDE_PLUGIN_DATA}` is substituted in an exec-form hook `command` and in `.mcp.json` (re-checked in M8; otherwise `moirai hooks install` writes the expanded absolute path, [80 §2.12]); whether `SubagentStart`/`SubagentStop`/`PostToolUse(Agent)` fire for Workflow `agent()` calls (W-all-2); the `mcp_tool` experiment — `permissionDecision` honoured, `${tool_input.idempotency_key}` substituted, `updatedInput` and whole-object substitution ([70 S3]); the Bash tool's inline and failure caps ([73 F1]); the Codex probes P1–P7, P10 and P11 of [90 §10.5] with a test-only stub server ([AR §8.2] item 7; without Codex access, which V9 asks the owner to confirm after V1, they wait, their decisions keep the documented defaults and P10 skips Luna's column) | hook design, the hook and MCP entry path (re-checked in M8), the stamp route (the Workflow experiment re-run in M9, the `mcp_tool` experiment in M10, §3.11) and `integrate.codex.store-writes` |
| 8 | loose-object create + rename under Defender, n = 1,000; full `git gc` time and peak RSS on a 1e5-commit image repository (G28) | the loose/pack threshold; `image gc` guidance |
| 9 | deflate throughput of `zlib-rs` vs `miniz_oxide` on this CPU (U33) | the git object layer's codec |
| 10 | tail replay of a full tail at candidate checkpoint thresholds (U13), decoding every record and applying it into an overlay built to the segment and runtime-table chapters, with the expected lazy-record share at the quiet cap, idle and loaded ([20 §1.3], [70 S8]; the A1 re-review A1P-03) | the checkpoint thresholds, chosen so that open at 1e6 with a full tail meets ≤ 3 ms ([61 m-9]); M1's open gate re-checks the choice |
| 11 | **physical floors**: data-only flush on a zero-filled extent vs append (G11); the directory flush (`durable-name`) and `FlushFileBuffers` (`durable+meta`); open + map of 8 sealed files; `HEAD` pread; spawn-to-exit of an empty Rust executable from the stable install path, directly and through the agent's Git-Bash wrapper, signed and unsigned; a VMMap breakdown (private, page tables, shareable) and the heap-only high-water mark per process kind ([71 RAM-m9]); the CLI's private-bytes baseline at 1e5 on `main` and on a 14-day lane against the 4 MB gate ([50 §5.12]; the A1 re-review S-23) | the floors of §5.3; #39; the CLI baseline against its gate (for WP-81a) |
| 12 | lock-release delay after `TerminateProcess` of a holder, p50/p99/max, idle and loaded (W2) | the 2 s bound; lock-delay injection parameters for GT3 and GT4 |
| 13 | BLAKE3 (feature `pure`, #44) and xxh3 throughput on this CPU, idle and loaded, through the Rust build (only measured under 97–100 % load so far [10 §0]) | hashing budgets for commits, bodies and fingerprints |
| 14 | layout probes — CSR probe, frozen-bitset AND/popcount, column scans, overlay probe at 1e5/1e6 — **plus the T1 and T2 trigger quantities**: a point read after a full tail and a delta checkpoint at 0.5 M nodes (T1), the 16-writer wait and MCP overlay catch-up (T2) [61 M-2c] | confirms the §2.5 layouts before the freeze; T1's structure (Option A or B) and the leader, decided before M1 |
| 15 | file-system costs for R4 through the `Vfs`: stat, directory enumeration with file ids, rename of one file and of a 1,000-file directory under Defender **as `file mv` performs it** — `MOVEFILE_WRITE_THROUGH` plus both parents' `FlushFileBuffers`, on the project volume, idle and loaded ([09 §8], [13 §1.4]; the A1 re-review A1P-07) — and whether a same-volume rename updates `ChangeTime` ([40 §4.3] copy rule, S-16); extended by [40 §8.3.6] with `OpenFileById` on directories, the creation-time behaviour of `mv`, `cp`, Claude Code's tools and NTFS tunneling on the owner's D: volume, directory-id stability on D:, and the attribute bits of a OneDrive placeholder read without hydration ([AR §8.2] item 15) | R4 budgets |
| 16 | **the 16-agent load fixture** recorded and validated (the replay reproduces the recorded profile), and the run-to-run noise band of every gated quantity on the laptop and on the hosted runners | the load of every "loaded" measurement; the CI noise bands |
| 17 | **OS-crash rig calibration — deferred to after the release with the rig** (the owner review of 2026-09-27, V7), on the rig host (the laptop in profile L) with the exact Server 2025 Core guest image that runs GT15 ([74 A06]): across ≥ 100 hard power-offs, a deliberately unflushed write, rename or delete is lost at least once and a barrier-protected one never (host I/O cache off, guest flushes honoured); recorded (a) guest page-cache loss seen, (b) flushed writes never lost, (c) whether loss of issued-but-unflushed writes was ever seen ([74 A18]) | that GT15 can observe lost unflushed data and names; whether `MOVEFILE_WRITE_THROUGH` (used until then) can be dropped besides the directory flush that every OS's rename point requires ([72 M8], [80 §2.3.2]); if calibration fails, the rig is reconfigured or the other hypervisor used; if both fail, the rig returns to the owner ([AR §11] #34 bought no licence) |
| 18 | flush-failure injection on a VHDX taken offline mid-flush: what Windows returns and what later reads see ([72 M1]) | fault-model item (3)'s parameters (the re-write rule is safe either way) |
| 19 | `moirai mcp`'s runtime shape over stdio: private bytes and thread count with rmcp on the configured runtime vs a hand-written synchronous JSON-RPC loop ([71 RAM-m2]), including a pending settle during a 16-subagent burst, which the shape must defer rather than sleep through ([40 §4.2]; the A1 re-review A1P-06) | the MCP front end (≤ baseline + 2 MB, ≤ 2 threads) |
| 20 | the token baseline: per-dispatch context tokens of ≥ 20 recorded dispatches (HDR, role prompt, first-turn plan reads) by the real tokenizer (counted as item 6 counts Claude tokens); the default-config token ledger of a BoykoEngine-style session ([73 F8], [74 A08]) | the M9 token gates |
| 21 | one build lane's and both lanes' peak RAM and CPU during a build-and-test cycle; the owner's intended overlap with other campaigns ([74 A03]) | the build cap and windows of the two lanes on the laptop ([AR §11] #2), or lane B's pause (the one-lane calendar); the calendar |
| 22 | Windows OS-layer conformance probes ([AR §8.2] item 22): the `LOCK` v1 byte positions and the in-process two-client case, the seal attribute, the in-page-error handler, the `total_len` check, the environment guard, the boot clock across a sleep, and the `BootId` boot identity across a clock step, a sleep, a hibernation and a reboot | the OS-layer conformance of M1; the Windows boot-identity source (else Unknown-boot mode) |

Later measurement points: M3 — as-of replay RSS at 8k and 16k ops [50 §5.10], the daily-sync fixture's overlay size; M6 — [40 §7]'s unit costs on the product; M7 — calibration of the query work unit (ns per unit); M11 — every budget of §5.4 on the release binary at 1e4 / 1e5 / 1e6 and at the owner's three-year scale of 0.3–0.5 M nodes.

### 5.3 Floor-relative gates

"Beat SQLite" is gone. Each engine budget is stated twice: as an absolute number from [AR §8.1] and as a distance from a floor of item 11 **re-measured interleaved in the same run** (§5.1), so a slow disk day cannot hide an engine regression and a fast one cannot excuse one.

| Floor (item 11) | Gate | Milestone |
|---|---|---|
| data-only flush p50 / p99 | durable commit p50 ≤ floor p50 + 0.5 ms, p99 ≤ floor p99 + 1 ms, ≤ 1 flush per durable commit (exactly one alone) | M1 |
| data-only flush p99 | 16-writer last acknowledgement p99 ≤ 50 ms (Windows); ports ≤ max(50 ms, 3 × floor p99 + 5 ms), [80 §2.4.4] | M1 |
| `HEAD` pread + map of 8 files | open ≤ floor + tail replay; ≤ 1.5 ms at 1e5, ≤ 3 ms at 1e6 | M1 |
| data-only flush p50 | `branch`/`checkout`/`undo`/`tag` ≤ floor + 1 ms | M3 |
| empty-executable spawn (direct) | read-verb spawn-to-exit at 1e5 ≤ floor + 5 ms | M8 |
| empty-executable spawn (Git-Bash) | reported, not gated (outside moirai) | M8 |

### 5.4 Absolute budgets by milestone

Each row is mandatory from its milestone and stays mandatory afterwards (GT11). **[AR §8.3] is the normative per-priority table** (speed, RAM, correctness, tokens) after the priority audits; the rows below list the same budgets by milestone and were amended to agree with it on 2026-09-26.

| Budget (source) | Target | From |
|---|---|---|
| durable commit; flushes per commit (§5.3) | floor-relative; ≤ 1 (exactly 1 alone, ≤ 3 per 16-writer burst), every acknowledgement after a covering flush and an identity check | M1 |
| open; bytes read on open (§5.3; [AR §8.1]) | ≤ 1.5 ms at 1e5, ≤ 3 ms at 1e6 with a full tail; independent of N | M1 |
| recovery after a kill with a full tail | ≤ 10 ms | M1 |
| writer-wait p99 with 16 writers (+ a concurrent merge from M3); writer-byte hold p99 and max for every verb class (plain M1, merge/sync M3, settle-bearing M6, `TX` M7); reader p99 during a writer burst | ≤ 50 ms; ≤ 5 ms and ≤ 20 ms; ≤ 2× idle | M1 |
| delta checkpoint (outside the writer byte); rollup (in the `moirai gc` child); promotion | ≤ 50 / 100 ms at 1e5 / 1e6; ≤ 0.3 / 3 s; ≤ 100 ms at 1e6; RAM ≤ 8 MB, ≤ 24 / 24 / 32 MB, ≤ 8 MB | M1 |
| first read of a ref forked 1k / 7k / 14k / 60k commits ago; of the 14-day daily-sync fixture | ≤ 3 / 10 / 10 / 20 ms; ≤ 10 ms with an overlay ≤ 1 MiB | M1 (M3 policy) |
| file opens per command; flushes per verb | ≤ 10 / 14 / 18; ≤ 1 per durable commit (exactly 1 alone, ≤ 3 per 16-writer burst); `file mv`/`rm` 2 log flushes and 2 directory flushes, counted separately (the A1 re-review A1P-07) | M1 (M8 through the binary) |
| private RSS, short-lived process (also at the quiet cap); decoding one commit from `hist`; `repair` | ≤ 4 MB at 1e4–1e6, + ≤ 1 MiB per extra ref read; ≤ 2 MB; ≤ 32 MB | M1 |
| idle CPU, any moirai process, 10 min (§5.1) | zero CPU time, zero context switches | M1 (M10 for the MCP server) |
| `backup` | ≤ 0.3 s at 1e5 | M1 |
| `get`; `ready` page; blocking-task ids; transitive blockers; the marker scan of `brief_triage`; `doctor --verify` RAM | ≤ 5 µs; ≤ 300 µs at 1e5 and ≤ 3 ms at 1e6, unchanged with 50 unabsorbed and 90k inert markers; ≤ 300 µs at 1e5 (engine time, printing excluded); ≤ 200 µs at 1e5; ≤ 50 µs; ≤ 16 / 32 MB at 1e5 / 1e6 | M2 |
| FTS tier 1; tier 2 | ≤ 10 ms at 1e5; ≤ 5 ms per term at 1e6 | M2 |
| merge of a 2k-op lane; `sync`; lane read incl. first-read overlay on the 14-day daily-sync fixture; merge and sync RAM; bases mapped after a rollup | ≤ 50 ms at 1e5; ≤ 40 ms at 1e5; ≤ 10 ms and ≤ `main` + 1 MiB; ≤ 8 MB (≤ 16 MB at 1e6 with Kahn); ≤ 2 | M3 |
| as-of at a pinned set; as-of replay | ≤ 50 ms; within `mem`, op caps 16,000 CLI / 100,000 MCP [50 §5.10] | M3 |
| ancestry per pair, incl. heads newer than the commit-graph | ≤ 1 ms with a commit-graph, ≤ 5 ms without | M4 |
| incremental checkpoint export of `main` + 2 lanes (pack + idx + `packed-refs` transaction + barriers + `gitmap`) | ≤ 50 ms (confirmed against item 8) | M4 |
| git object layer RAM: delta cache; tree lookup, ancestry, 2,000-commit rename window | ≤ 256 KiB CLI / ≤ 1 MiB MCP; ≤ 1 MiB each | M4 |
| full export of 1e5 (time, RSS); full import of 1e5; import RAM per commit / checkpoint image | ≤ 3 s, ≤ 6 MB; ≤ 7 s; ≤ 4 MB / ≤ 8, 16, 32 MB at 1e4, 1e5, 1e6 | M5 |
| R4, per [40 §7.4]: link work in a 50-link pack (all present idle / loaded; 10 edited files); `SessionStart` settle under load and its log bytes when nothing changed; read-path git work; `links check --all` at 1e4; private RSS of read verbs (16 MiB file) and hooks / of `--all` / of `--deep` at 1e5; log bytes appended by any read verb; project-file handles open when a command returns | ≤ 3 / 5 ms p50; ≤ 5 ms p50; ≤ 150 ms and ≤ 16 KB; ≤ 10 ms p99; ≤ 0.7 s warm; ≤ 4 MB / ≤ 8 MB / ≤ 16 MB; 0; 0 | M6 (through the driver), M9 (through packs and hooks) |
| R5, per [50 §8.3]: parse + bind (+ plan, first query incl. schema); anchored query; the [50 §5.12] rows; CLI RSS under default budgets; `TX` within `wmem` | ≤ 20 µs for ≤ 1 KB (≤ 200 µs); ≤ 0.3 ms at 1e6; within 2× of estimate at 1e5; [50 §5.12]'s composition rule against the process kind's gate (§3.8); default-cap `TX` ≤ 4 MB, 500k-op `TX` ≤ 16 MB | M7 |
| read-verb spawn-to-exit (§5.3); CLI RSS; uncached `check`; file-bearing header; CLI result header; non-zero-exit stdout | floor + 5 ms; ≤ 4 MB at 1e4–1e6; ≤ 5 ms; ≤ 0.5 ms with no worktree scan; per part ([AR §7.1]: base fields ≤ 60 B, the `files @` segment ≤ 80 B, extras ≤ 60 B, the reader note on line 2 ≤ 80 B; the A1 re-review A-M3); ≤ 8,000 B | M8 |
| pack/brief engine time; lane pack; brief size; hook delta; per-hook latency under a 16-agent burst; token rows | ≤ 8 ms at 1e5, ≤ 12 ms at 1e6; ≤ 20 ms at 1e5; ≤ 8,000 B; ≤ 600 B; `SessionStart` ≤ 300 ms p50 / 500 ms p99, `SubagentStart` ≤ 150 ms p99, `UserPromptSubmit` ≤ 120 ms p99, stamp ≤ 110 ms p99 (command transport); every TOKENS row of [AR §8.3] the CLI and command hooks carry, on the ledger | M9 |
| MCP private RSS steady / peak; the 16-session aggregates; `mcp_tool` hooks under a 16-agent burst; unstamped read tool, idle and under a burst with maintenance pending; instructions; schema | ≤ 8 MB + 4 MiB / ≤ 16 MB; Σ ≤ 128 MB idle (no branch overlay held), Σ ≤ 16 × (8 MB + `mcp.overlay-bytes`) after a fan-out, an idle Codex thread server ≤ 3 MB and P8's Codex leak scenario Σ ≤ 100 MB ([90 §4.5]), ≤ 256 MB all processes; `SubagentStart` ≤ 40 ms p99, `UserPromptSubmit` ≤ 5 ms p99, stamp ≤ 2 ms p99; ≤ 5 ms, p99 ≤ 25 ms, max ≤ 100 ms; ≤ 512 chars (the `codex` profile 507); ≤ 5,000 B as served | M10 |

### 5.5 No third-party reference points

*(Issue 2's "optional out-of-tree reference points" — redb 4.x and heed/LMDB run once at M1 exit for information — are deleted: the owner's decision forbids any third-party embedded database anywhere, benchmarks included, in or out of tree.)* Performance is judged only against the absolute budgets of §5.4/[AR §8.3] and the physical floors re-measured in the same run (§5.1). No embedded-database crate may appear in any `Cargo.lock` the project keeps (GT20, §3.13).

### 5.6 Removed from the old plans

- SQLite (rusqlite, WAL, `synchronous=FULL`) as "the conservative multi-process reference and the S0–S5 throw-away backend" [AR §8.2].
- "The from-scratch engine must beat them … if it cannot at S6, the trait keeps the product working" [AR §8.2].
- "`blocking --ids` ≤ 100 µs at 1e5 on the oracle" [AR §9 S1] — now measured on the engine at M2, gated at ≤ 300 µs at 1e5 in engine time ([AR §8.3], §10.6).
- "Swapped in when it beats the oracle on open time and private RSS" [AR §9 S6] — there is no swap.
- [60d]'s serialized-access contention probe and its gates (stage-S wait ≤ 250 ms, the A1 node cap) — there is no serialized stage.
- **[05 §17] item 5** ("redb 4.x, heed, and SQLite (rusqlite, WAL)" as baselines) — removed: no third-party embedded database is run for any purpose, in or out of tree (§5.5, [61 m-5]).
- Issue 1's measurement conventions: n = 200 for a p99, floors measured once at M0, an undefined "synthetic 16-agent load" and an undefined "0 % CPU" — replaced by §5.1.

---

## 6. "Complete and hardened": the release gate

The owner's workflow starts using moirai when, and only when, every criterion below holds on the release binary, installed at its stable path on the owner's machine.

| # | Criterion | Evidence |
|---|---|---|
| RG1 | **Scope complete.** M0–M10 exited with all their gates and re-certifications; every "in release" item of §1.3 built; no item deferred for schedule; [AR] amended by §9 and by [40]'s and [50]'s edit lists (applied before M0 exits, §3.1). | milestone exit records |
| RG2 | **Requirements demonstrated end to end** through the real binary, hooks and MCP server: R1 (a lane lifecycle with sync, staged violation, resolve, merge, revert of a merge, undo, tag, op restore), R2 (the full verb set in a directory outside any git repository with no `git` on PATH, image to and from a local directory included), R3 (export → fresh import → export byte-identical at commit granularity; state-identical at checkpoint granularity), R4 per [40] (a recorded move campaign replayed: explicit, raw `mv`, git-driven and directory moves; GT17 through the binary), R5 per [50] (LQ-Bench answered through the CLI and MCP within budgets). | scripted acceptance runs, logs kept |
| RG3 | **Crash safety.** GT3 nightly for ≥ 4 weeks with the last 14 consecutive nightly runs clean (≥ 1e7 steps each); GT1 exhaustive over every record kind under the fault model; GT4 clean in every variant for the last 14 nights; a **72-hour soak** (GT14) with kill loops across branches, hourly `doctor --verify`, nightly image export and backup: zero lost acknowledged commits, zero corrupt opens, zero verify findings. GT15's ≥ 5,000 cumulative OS-crash cycles (the last ≥ 1,000 on the release candidate) are not part of this gate: the owner review of 2026-09-27 deferred the rig to after the release, and real power loss is the stated residual risk of [AR §10] risk 17. **The 14 nights and the soak run on the release-candidate commit; a change restarts the gates whose covered crates' source hashes changed** — storage, graph or version-control crates restart everything; the image codec restarts GT8, GT4 with exports and GT5 `.moi`; the resolver restarts the link differential, GT17, GT4 with intents and GT5 fingerprints; the query crates restart the query differential, GT9 and GT13 ([74 A12]); M1's dependency lint proves which crates changed. In profile L ([AR §11] #34), "14 consecutive nightly runs" reads as 14 consecutive completed runs in agent-free windows, and the soak runs inside a 3-day agent freeze. Stated assumption: the drive honours FLUSH; loss of the drive's own volatile cache is covered only by the simulator. | nightly logs |
| RG4 | **Semantic correctness.** GT2 and GT6: ≥ 1e7 cumulative differential cases per component with zero open disagreements; GT16 ≥ 90 % in the semantic crates on the release candidate; GT10 fixtures all green, including the owner-verified core set and the [AR §7.6] walk-through run as a script. | CI history |
| RG5 | **Parsers.** Every GT5 fuzzer has ≥ 7 CPU-days cumulative with zero open crashes or hangs. | fuzz corpora and logs |
| RG6 | **Performance, RAM and tokens.** Every row of [AR §8.3] and §5.4 met on the release binary at 1e4, 1e5 and 1e6 and at 0.3–0.5 M nodes, idle and loaded, under §5.1 (sample sizes tiered by duration), the token rows on the campaign replay's ledger; every floor-relative gate of §5.3 met; zero idle CPU for every moirai process by §5.1's definition; no number in [AR §8] still tagged "claimed" or "pending". | the M11 measurement report |
| RG7 | **Recovery drills** on the owner's machine: backup → restore into a new directory; `repair --rebuild-from-log` after deleting every segment; image export → import into a fresh store on another path → state-identical; restore after a disk-full during a commit on a small VHDX (the command aborts unacknowledged, the next writer recovers). | drill logs |
| RG8 | **Agent fit on recorded data.** A synthetic replay of one recorded BoykoEngine campaign — orchestrator, 16 concurrent agents, lanes as branches, critic rounds through MCP by Bash-less roles, dispatcher `apply`, merges, image export, file moves, queries — runs to completion with zero invariant violations, zero double dispatch and hook outputs within caps; the owner judges the packs complete against the HDRs actually used. | replay transcript; owner sign-off |
| RG9 | **Independent review.** A final review of the implementation by the three lenses of [20]–[22] leaves zero open blocker or major findings. | review documents |
| RG10 | **Operability and upgrade.** A binary with PE metadata at a stable path (unsigned unless M0 item 11 showed that signing matters, [AR §11] #39); `doctor` covers store, lanes, image, agents, hooks, `--verify`, `--fsck`; backup-age warning (CM8); recovery runbooks; the format-version refusal tested with a newer fixture; uninstall documented; **an upgrade drill to a synthetic format version 2** [61 M-9]: (i) a derived-file layout change (a new section) applied by `repair --rebuild-from-log` to a copy of a v1 store; (ii) a log or canonical-form change applied by `image export --with-oplog` → `image import` into a v2 store, with commit ids preserved where the canonical form is unchanged and the id map documented where it is not; both verified state-identical against the model; rollback by restoring the pre-upgrade backup, also drilled. | release checklist; drill logs |
| RG11 | **Cutover rehearsed.** The import of the standing rules, current pins, live lanes and open owner questions — and `links import` over the existing citations, after the owner has reviewed its ambiguous list ([AR §11] #23) — has been run on a copy and reviewed by the owner; the generated views match the registers they replace. | rehearsal record |
| RG12 | **No known defect** of severity major or worse is open. | tracker (in moirai's own test store) |

**The ports.** The release gate is Windows' (owner decision #32). The Linux and macOS ports have their own gate, which reads RG1–RG12 per OS with the gates of [80 §5.3]; they never block this release.

**Cutover.** At a campaign boundary chosen by the owner: backup; import (the rehearsed set); hooks, skills and the MCP server installed; `OPEN-QUESTIONS.md`, `BACKLOG.md`, `MEASUREMENT-QUEUE.md` and the MEMORY.md resume block replaced by generated views; the 254 memory files kept as a read-only archive linked by `artifact` nodes. Rollback is removing the hooks and the MCP entry; nothing of the old workflow is deleted.

---

## 7. Calendar

### 7.1 Basis

- **Units** are those of [22 §7.1] (A = 100), with the additions stated per milestone in §3. R4 is sized from [40 §8.1] (22–30k lines of product code plus 10–14k of tests) and R5 from [50 §8.2] revision 2 (≈ 15–21.5k lines in M7 plus ≈ 2.5–4k in the other milestones), each at ≈ 3 units per 1k product lines including their tests [61 M-8]. Units also cover work that produces no product lines: specifications, measurements, reviews, harnesses and drills.
- **Rate** 5–8 units per week of the owner plus Opus agents. This is **the earlier plan's estimate, not a measurement** ([22 §7.3]: "S ≈ a week, M ≈ 2–3 weeks"; [60d §2.1]), and that plan's early slices were SQLite-backed, which is easier than engine work. **Velocity is measured at the M0 and M1 exits** (units delivered against this estimate) and this calendar is re-issued then.
- **Weeks per milestone** are the unit range divided by the rate; M11 adds 14 nightly runs and the 72-hour soak on the release-candidate commit (≈ 2.5 weeks with a host; 2.5–3.5 in profile L, below).
- **Two lanes, both on the owner's laptop, in profile L** ([AR §11] #2 and #34, decided 2026-09-26; [74 A02, A03]). The owner supervises a second Opus lane, and there is no other machine: both lanes build on the laptop in windows agreed with the owner (agreed at M0 start, the owner review of 2026-09-27, A8), and every nightly gate runs there in agent-free windows. Compiling and testing a 66–99k-line workspace needs est. 1–3 GB per `rustc` or test link at peak against ≈ 1.8 GB free beside a 16-agent campaign, so the lanes do not build beside a campaign; M0 item 21 measures one lane and both and sets the build cap, and a machine-wide build semaphore, no `rust-analyzer` in lane worktrees and a 1.5 GB free-RAM guard complete the mitigation ([AR §9], [AR §10] risk 40). Lane B takes M0's model, LQ-Bench and FL-1 part 2, then FL-2, the git object layer (from the M1 `Vfs` certification point), the query front end and binder, planner and executor core (LQ-1 anytime, LQ-2/4/5/6 after M2), the image (after M3 and M4), and shares M7's remainder, M8 and M9 with lane A. If item 21 shows that the laptop cannot carry two lanes even in the agreed windows, lane B pauses and the one-lane column applies.
- **P50/P90** come from a Monte Carlo over the unit ranges (uniform, independent per milestone) and one project-wide rate drawn uniformly from 5–8 units per week, scheduled as above (20,000 draws, est.). The unit ranges are treated as unbiased, which the history of this plan does not support — issue 1 was ≈ 75–100 units low [61 M-8] — so **P90 is the figure to plan against** until velocity has been measured.
- **Profile L's machine time** ([74 §2.4], est.) is placed where it falls in the schedule, not added as a flat delay: each milestone exit M0–M10 (re-certification, 24 h of fuzzing per new parser target, sampled mutation testing) takes 2–4 laptop windows instead of 1–2 host nights, + 0.2–0.5 week on the lane that owns the exit; RG3's 14 consecutive completed nights in agent-free windows, with only the soak inside a 3-day agent freeze, take 2.5–3.5 weeks instead of 2.5. Fuzzing's ≈ 3,000 CPU-hours (≈ 9 window-weeks) also run by day at ≤ 2 targets and stay off the critical path. [74 A02]'s flat + 2–4 weeks assumed half the exits on the critical path; in the two-lane schedule M0–M3 and M7–M10 always are, and one of M5 and M6, so the delay is ≈ + 3.5 weeks (P10–P90 + 3.1 to + 4.2). **The OS-crash rig's time left the calendar on 2026-09-27**: the owner review of 2026-09-27 deferred the rig to after the release (V7), so GT15's ≥ 1,000 M1-exit cycles on one laptop guest (1.25–2.1 weeks instead of a host's 0.3–0.6; with them the delay was ≈ + 5 weeks, P10–P90 + 4.3 to + 5.5, and the release 52 / 62) are no longer in the Monte Carlo; the rig's last 1,000 cycles on the release candidate ran inside RG3's 14 completed runs, which GT3, GT4, GT17 and the soak's freeze still bind, so RG3 keeps 2.5–3.5 weeks; M0's exit keeps its 2–4 windows (the calibration shared them with the 24 h FL-1 fuzzing); the GT15 harness's 2–3 units in M1 and the rig's M0 provisioning stay in the figures as contingency until the M0 re-issue (dropping them would bring the release ≈ 0.4 week earlier). Running the synthetic long seeds (GT1, GT3, fuzzing, sampled GT16) on the free hosted runners of the public repository ([AR §11] #36) would cut the per-exit share to 0–0.2 week and recover ≈ 2 weeks (P50 ≈ 48.5, P90 ≈ 58.5); it is not counted until the M0 capacity check shows that the runners carry the load.
- **Deltas since the pre-audit baseline** (re-issued 2026-09-26, [90 §10.3]). Issue 2's figures were a *pre-audit baseline*: they excluded the scope the priority audits added, [80]'s cross-platform design and, later, [90]'s harness-agnostic design. This section now includes all three, per milestone and per lane, in the table below and in the Monte Carlo: **the audits** ≈ 23–40.5 units net of the exclusions of [AR §11] #41 (est. by [90 §10.3] from [AR]'s audit entry; the table below names what lands where; `links import`'s 1–2 units stay in M9, because #23 decided that it runs, and are not subtracted); **the cross-platform design** of owner decision #32, ≈ 9.5–16.5 units in M0, M1, M6 and M10 ([80 §5.4]), its 0.5–1-unit cross-target type check moved from M1 to M0 by owner decision #44; **the harness-agnostic design and the pure-Rust rule** of owner decisions #43 and #44, ≈ 15–23.5 units in M0, M1 and M8–M11 ([90 §10.3]). Together ≈ 47.5–80.5 units and, at P50, ≈ 8 weeks on the two-lane release. The Linux and macOS port phase (≈ 52–83 units) stays outside this calendar; so do the conditional items: the leader (below), an own zstd-format dictionary encoder (+ 5–8 units in lane B before M1's exit, if M0 item 6 chooses it, [90 §11.3]) and the scope items of [AR §11] #45. The M0 and M1 exits re-issue the table with measured velocity; until then plan against P90.

| M | Audits' delta (est., net of #41) | What the audits added there (from [AR]'s Review log, audit entry) |
|---|---|---|
| M0 | 4–6.5 (lane A 3–5, lane B 1–1.5) | ≈ 20 format items in the specification, the model and the hand-written fixtures (overlay counters, `MARKERS_OLD`, the `TREES` dirty row, `ANCHORRES`, `GLOBIDX`, `cs.NNNN`, `HEAD.durable_lsn`/`boot_id`, the `LOCK` holder anchors, `RecHdr.group_end`, `ALLOC`/`UIDX`, `actor u32`, the derived-optional flag); the configuration-system specification; measurements 18–21, the daily-sync fixture and the BM25 ablation; infrastructure profiles, the night schedule and sampled mutation testing; in lane B the model's three-phase semantics, marker states and lease liveness |
| M1 | 7–11 | the three-phase write; `durable_lsn`, boot recovery, adoption by re-write and the two-slot barrier; the subset crash enumerator; holder anchors and tri-state liveness; bulk commits and changeset segments; the compact overlay, tail bounds, `wmem` and region arenas; the rollup child and streaming rollup; the configuration registry and reload |
| M2 | 1.5–2.5 | `ALLOC`/`UIDX` and the uniqueness gate; lease deadlines and the reboot rule; `TREES.dirty` and captured `files_owned` |
| M3 | 2–3.5 | overlay-driven promotion by sync; marker states and `MARKERS_OLD`; residue equivalence; the GT18 state oracles |
| M4 | 0.5–1 | byte-bounded caches and streamed objects; git work charged to `fs`; oracle-corpus additions |
| M5 | 0.5–1.5 | the durability order and verification; import onto a diverged ref; anchor-text digests; `packed-refs` transactions; bounded packs; less the second destination (A17) |
| M6 | 1–2.5 | `ANCHORRES`, settle epochs, per-directory enumeration, fixed buffers, identity checks, CAS-guarded settles; less E2 (A13) |
| M7 | 1.5–2.5 | `DRY` digests and `IF TARGETS`; `TX` computed before the lock; depth limits and heap stacks; metered git work; warm BM25 statistics |
| M8 | 1–2 | output ceilings and header rules; `moirai config`; the default idempotency key; `check` as a write verb; `backup` records |
| M9 | 1.5–3 | the token ledger; session marks; resume deltas; `export rules` with `paths:`; per-hook budgets; `apply --from-journal`; the image-export trigger; less the nudge and `PostToolBatch` (A17) |
| M10 | 1.5–2.5 | `mcp_tool` handlers and the server-side stamp; sliced server work and burst gates; the byte-bounded overlay, arenas, ≤ 2 threads and aggregate RAM gates |
| M11 | 1–2 | the widened GT15 oracles (their runs deferred with the rig; the units kept as contingency); VMMap breakdowns; durable-effect checks |

- **Lines of code** (est.): ≈ 66–99k lines of product Rust plus ≈ 38–55k of tests, of which the reference model and the format oracle are ≈ 8–10k.

| M | Pre-audit baseline (build + test) | Audits (net of #41) | Cross-platform (#32, #44) | [90] (#43, #44) | Total | Weeks (one lane) |
|---|---|---|---|---|---|---|
| M0 | 55–74 (29–39 + 26–35) | 4–6.5 | 5.5–8.5 | 4–6.5 | 68.5–95.5 | 8.5–19 |
| M1 | 46–57 (32–39 + 14–18) | 7–11 | 2.5–5 | 0.5–1 | 56–74 | 7–15 |
| M2 | 30–39 (25–33 + 5–6) | 1.5–2.5 | — | — | 31.5–41.5 | 4–8.5 |
| M3 | 36–42 (31–36 + 5–6) | 2–3.5 | — | — | 38–45.5 | 5–9 |
| M4 | 15–21 (12–17 + 3–4) | 0.5–1 | — | — | 15.5–22 | 2–4.5 |
| M5 | 17–21 (14–17 + 3–4) | 0.5–1.5 | — | — | 17.5–22.5 | 2–4.5 |
| M6 | 29–39 (25–34 + 4–5) | 1–2.5 | 1–2.5 | — | 31–44 | 4–9 |
| M7 | 49.5–70 (47.5–67 + 2–3) | 1.5–2.5 | — | — | 51–72.5 | 6.5–14.5 |
| M8 | 14–20 (12–17 + 2–3) | 1–2 | — | 2–3 | 17–25 | 2–5 |
| M9 | 12–17 (11–15 + 1–2) | 1.5–3 | — | 4–6 | 17.5–26 | 2–5 |
| M10 | 7–8 (6–7 + 1) | 1.5–2.5 | 0.5 | 3.5–5 | 12.5–16 | 1.5–3 |
| M11 | 11–20 (— + 11–20) | 1–2 | — | 1–2 | 13–24 | 4–7.5 (incl. ≈ 2.5 weeks of nights and soak; 4–8.5 in profile L) |
| **Total** | **321.5–428** | **23–40.5** | **9.5–16.5** | **15–23.5** | **369–508.5** | **48.5–104 (one lane)** |

Lanes (two-lane schedule): M0's deltas go to the lane that does the work — audits A 3–5 / B 1–1.5, cross-platform A 4–6.5 / B 1.5–2, [90] A 2.5–4 / B 1.5–2.5 — and [90]'s M8 and M9 work splits across both lanes (A 1–1.5 / B 1–1.5; A 2–3 / B 2–3); every other delta joins the milestone's existing lane. Monte Carlo (the same script, seed, 20,000 draws, rate and schedule as issue 2): units P50 ≈ 439; before profile L's machine time, one lane P50 ≈ 70, P90 ≈ 85.5 and two lanes P50 ≈ 47, P90 ≈ 57. Step by step, two lanes: the pre-audit baseline 39 / 47.5; + the audits 43 / 52.5; + the cross-platform design 45 / 54.5; + [90] 47 / 57; + profile L ([AR §11] #34) with the OS-crash rig's M1-exit cycles 52 / 62 (issued 2026-09-26); without the rig, deferred by the owner review of 2026-09-27, **50.5 / 60.5, the calendar of record**. One lane in profile L: 74.5 / 89.5 (75.5 / 91 with the rig; with a host it would have been 70 / 85.5). The same script, with profile L's delays drawn from their own seed (20260927) so that the unit and rate draws are unchanged.

If M0 puts the leader into M1, add 3–4 units (≈ 0.5–1 week) to M1 and to every later date.

### 7.2 Weeks to each capability

Bounds are "low units at 8 per week" and "high units at 5 per week", each with profile L's low or high machine time; P50/P90 as in §7.1. The calendar of record is the two-lane column in profile L ([AR §11] #2, #34), re-issued on 2026-09-27 without the OS-crash rig that the owner review of 2026-09-27 deferred (§7.1).

| Capability (milestone exit) | Two lanes, profile L: bounds | Two lanes, profile L: P50 / P90 | One lane, profile L (fallback): bounds | One lane, profile L: P50 / P90 | Two lanes with a test host (not bought): P50 / P90 |
|---|---|---|---|---|---|
| Format frozen, evidence gathered (M0) | 5–11 | 7.5 / 9 | 9–19.5 | 13 / 16 | 7 / 9 |
| Storage engine certified: protocol, crash enumeration, DST, kill loops (M1) | 12–26.5 | 17.5 / 22 | 16–35 | 23.5 / 28.5 | 17 / 21 |
| Graph core (M2) | 16–35.5 | 23.5 / 29 | 20–43.5 | 29.5 / 36 | 22.5 / 28 |
| **R1** — version control (M3) | 21–45 | 30.5 / 37 | 25–53.5 | 36 / 44 | 29 / 35.5 |
| Git object layer (M4) | 10–22 | 14.5 / 18 | 27–58 | 39.5 / 48 | 14 / 17 |
| **R3** — git image (M5) | 23.5–50 | 34 / 41 | 29.5–63 | 42.5 / 52 | 32 / 39.5 |
| **R4** — file-link runtime (M6) | 24–52 | 35 / 42.5 | 33.5–72.5 | 49 / 59.5 | 33 / 41 |
| **R5** — query language (M7) | 25.5–54.5 | 37 / 45 | 40–87.5 | 58.5 / 71.5 | 34.5 / 42.5 |
| **R2** user-visible — CLI (M8) | 27–59 | 39.5 / 48 | 42.5–93 | 62.5 / 75.5 | 37 / 45.5 |
| Agent interface (M9) | 29–63 | 42 / 51 | 45–98.5 | 66 / 80 | 39.5 / 48.5 |
| MCP (M10) | 30.5–66.5 | 44.5 / 54 | 46.5–102.5 | 68.5 / 83 | 41.5 / 51 |
| **Release gate met — first use by the owner's workflow** (M11) | **35–75** | **50.5 / 60.5** | **51–110.5** | **74.5 / 89.5** | **47 / 57** |

With a test host, one lane would have released at 48.5–104 weeks (P50 ≈ 70, P90 ≈ 85.5).

### 7.3 Comparison with the earlier plans

| Event | Oracle-first plan [AR §9] | Adoption-driven draft [60d] | Issue 1 of this file | This issue, one lane (profile L) | This issue, two lanes (profile L) |
|---|---|---|---|---|---|
| Own engine certified | 16–25 (S6 swap) | 10.5–16 (E4) | 6.5–12 (M1) | 16–35 (M1; the OS-crash loop after the release) | 12–26.5 |
| First use in the owner's workflow | 5–7 (S2, on a SQLite backend deleted at S6) | 6.5–10 (A1: serialized mode, node cap) | ≈ 23–43.5 (two lanes) | 51–110.5 | 35–75 (P50 50.5) |
| R1 | 9–13 on SQLite; 16–25 on its own engine | 14.5–22 | 13.5–24 | 25–53.5 | 21–45 |
| R3 | 11–16 via fast-import on SQLite | 16.5–25 | 17.5–33 | 29.5–63 | 23.5–50 |
| R4 / R5 | not scheduled | slots, excluded from totals | placeholders, 20–31 / 23–39 units | per [40] / [50] | per [40] / [50] |
| Total units | ≈ 118–160 without R4/R5 | ≈ 127–137 without R4/R5/v1.1 | 196–262 | 369–508.5 (321.5–428 before the deltas of §7.1) | 369–508.5 |

**Reading it.** Issue 2 is ≈ 125–165 units larger than issue 1. The difference is R4 at [40]'s size instead of a placeholder (≈ +45–60), R5 at [50]'s revised size (≈ +35–40), the complete reference model and LQ-Bench before the freeze (≈ +10), the fault model, the OS-crash loop (its harness's units kept as contingency since the rig was deferred on 2026-09-27), the section registry and mutation testing (≈ +10–15), the git object layer at its real scope (≈ +4–6), the infrastructure (≈ +2–3) and a release-hardening phase that restarts its nights after every engine fix (≈ +5–10). None of it is an interim stage, and none of it could be removed without reducing a guarantee (P8). The owner's workflow gets moirai once, complete, at 35–75 weeks with two lanes on the laptop in profile L (P50 ≈ 50.5, P90 ≈ 60.5), after two velocity measurements have re-issued this calendar; the deltas of §7.1 — the audits, the cross-platform design and the harness-agnostic design — add ≈ 47.5–80.5 units and ≈ 8 weeks at P50 to issue 2's pre-audit 28–56 weeks (P50 ≈ 39, P90 ≈ 47.5), and profile L's machine time ≈ 3.5 weeks more (≈ 5 before the OS-crash rig was deferred on 2026-09-27). (Re-run on 2026-09-26 with [50]'s revised M7, using the same Monte Carlo and schedule: the two-lane dates move only at P90, because the extra front-end and binder work falls on the second lane's slack before M5.)

---

## 8. Risks of this plan

| # | Risk | Likelihood / impact | Mitigation | Signal |
|---|---|---|---|---|
| 1 | **No use before the release.** For 35–75 weeks (two lanes in profile L; P50 ≈ 50.5) the owner's workflow gets nothing, and the value of packs, briefs and the query language is not proven in live use first — the concern behind [22 §2.7] | high / medium | validation on recorded data: LQ-Bench on Opus 5.5, the owner's Claude agent model (M0), R4's replay corpora (M0) and pattern matrix (M6), HDR-vs-pack diffs on recorded dispatches (M9), the synthetic campaign replay (M11); a demonstration on a synthetic store at every milestone exit | owner judgement at each demonstration |
| 2 | **The engine is on the critical path of everything**, and nothing is swappable ([AR] risk 5) | high / high | the format, not the engine, is the contract (frozen at M0); the engine is split along its seams (M1, M2, M3), each certified before the next; leaf components in a second lane; the test workstream from M0; no cut list — scope changes go to the owner (P8) | milestone exits against §7.2 |
| 3 | **The M0 freeze misses something** (an R4/R5 field, a layout that fails a budget, a protocol rule) | medium / medium | [40]'s revision and [50]'s review are M0 entry criteria, and the re-review of both revisions is part of the M0 specification review (A1); every piece of evidence that can change the format runs before the freeze (LQ-Bench, replay corpora, T1/T2 probes, carrier table); the independent specification review; change control: a format change after M0 re-opens M0 and re-runs every passed milestone's gates | format change requests |
| 4 | **R4/R5 designs change under review** — [41] found four blockers in [40], two of them in hashed format content; [51] found three in [50]; both designs were revised (revision 2) and integrated into [AR] on 2026-09-26, but neither revision has been re-reviewed yet | medium / medium | by the owner's decision of 2026-09-27 (A1) the re-review is part of the independent M0 specification review (§3.1 item 8), which closes with zero open blocker or major findings before the format freezes, so M0 starts and nothing is frozen before both are settled; this roadmap carries no content of its own for them (P9); their sizes are taken from the designs and re-issued with them (§10.4) | open findings at the format freeze |
| 5 | **Model and engine share a misunderstanding** (same documents, same model family) | medium / medium | §4.5: different algorithms, no shared code (own parser, binder, canonical encoder, format decoder), separate authors, owner-signed rule tables, owner-verified core fixtures, independent oracles, mutation testing, seeded real-engine bugs | model/engine disagreements; GT16 kill rates |
| 6 | **The multi-process protocol loses an acknowledged write after a process kill** ([AR] risk 1) | medium / severe | M1 certifies it with GT1/GT3/GT4 before anything builds on it; the protocol surface of branching is inside that certification; RG3 on the release-candidate commit | any GT1/GT3/GT4 failure |
| 7 | **An OS crash or power loss loses acknowledged commits** that process kills cannot reveal — adoption after a failed flush; an unflushed `HEAD` against GC and extent retirement [61 B2]; the development machine has experienced OS crashes and disk-full events | medium / severe | the fault model frozen at M0; GT1 over subsets of unflushed writes and failed-flush sequences; the protocol decisions of §2.5; acknowledgement only after a covering flush; suspend, disk-full and clock-step variants; `backup`/`restore` and the daily image export; GT15 (≥ 1,000, then ≥ 5,000 cycles) deferred to after the release by the owner review of 2026-09-27, so real power loss is a stated residual risk until it runs ([AR §10] risk 17 names the triggers that bring it forward) | any GT1 failure; a lost effect after a real OS crash or power loss |
| 8 | **The OS-crash rig is not faithful** (the hypervisor acknowledges flushes it does not persist, so GT15 passes vacuously; after the release, when the deferred rig runs) | medium / high | measurement 17 calibrates it before its first cycle; the other hypervisor as a cross-check; the simulator's fault model does not depend on the rig | calibration failures |
| 9 | **The hand-written git object layer diverges from git** (delta chains, SHA-256, split commit-graphs, reftable, packs from newer git) | medium / medium | GT7 against the git CLI on the owner's real repositories and on synthetic repositories; fuzzers; reftable writing excluded; transport stays with git | GT7 failures; a git upgrade re-runs GT7 |
| 10 | **The harnesses change during a year-long build** — Claude Code and Codex (hook fields and trust, MCP handshakes and `_meta`, truncation caps, sandbox rules, Workflow behaviour, model names) ([AR] risk 12) | high / medium | agent interface and MCP built last against the then-current harnesses; explicit parameters and leases before inferred context; the harness registry with verified versions; the hook experiments re-run in M9 and M10 in both Tier A harnesses; per-version fixtures; LQ-Bench re-run when a gate model changes ([90 §10.6]) | GT12 failures; `integrate --check` |
| 11 | **Owner decisions arrive late** — answered on 2026-09-26: every decision due before M0 is decided (§3.14); only #42 (port phase), #45's on-demand items and #21 (d) remain, and a measurement can bring back #39 (signing) or, after the release, the rig licence (#34) | low / medium | the remaining ones have recorded defaults; a re-ask arrives with its measurement and its cost | a measurement that re-opens a decision |
| 12 | **A budget cannot be met by the specified design** at a milestone exit, and reducing the guarantee is not allowed | low / high | M0 layout probes, floors and T1/T2 probes; the [AR] revisit triggers are design changes decided on measurement, followed by re-gating; the owner decides if a budget itself must change | GT11 misses |
| 13 | **The recursive virtual base adds merge-semantics risk** | low / medium | specified and reviewed at M0; the model computes it by definition; GT6 properties for I31′ incl. the both-sides-equal case | GT6 failures |
| 14 | **Test infrastructure and machine time** are under-estimated: with no test host (profile L, [AR §11] #34) every nightly gate competes for the laptop's agent-free windows | medium / medium | the M0 infrastructure deliverable in profile L; the night schedule, sampled mutation testing, tiered sample sizes and the disk budget of §3.15; the gate volumes stay (P8) and profile L's machine time is in §7's Monte Carlo (≈ + 3.5 weeks at P50; the OS-crash rig's time left with the rig); the synthetic long seeds may move to the free hosted runners ([AR §11] #36) | nightly-run completion rate; windows granted per week |
| 15 | **Estimates are low and the rate is unmeasured** | high / medium | recomputed from [40] and [50]; P90 as the planning figure; velocity measured at the M0 and M1 exits and the calendar re-issued; scope never cut without an owner decision | velocity at the M0/M1 exits |
| 16 | **M0 is large** (68.5–95.5 units with every delta of §7.1) before any engine code | medium / medium | two lanes; everything in M0 is permanent (the model, the format oracle, FL-1, the simulator, LQ-Bench, the infrastructure); it is the price of freezing the format on evidence | M0 exit date |
| 17 | **The cutover is a single step** | low / medium | rehearsed on a copy (RG11); backup first; rollback removes hooks and the MCP entry; the old files stay untouched | rehearsal findings |
| 18 | **The Linux and macOS ports** meet behaviour the frozen format or protocol did not anticipate, or the group-commit protocol, the macOS flush cost or a sandbox denial bites ([AR §10] risks 29, 31–33) | medium / high | the format and protocol frozen for all three OSes at M0 ([80 §3]); the weakest-OS fault model; the thirteen seeded group-commit bugs; Unknown-boot mode; the OS-layer lint; the port starts with probes ([80 §5.2]) | port-phase probe results; GT1 failures |
| 19 | **Two build lanes, the owner's campaigns and the gate windows share one laptop** ([AR §11] #2, #34; [AR §10] risk 40): builds page, stall or intrude on the owner's benchmark windows, and lane velocity falls below the estimate | high / medium | M0 item 21 measures a lane and both; owner-agreed build windows, never beside a 16-agent campaign; one build semaphore; `CARGO_BUILD_JOBS` capped; no `rust-analyzer` in lane worktrees; a 1.5 GB free-RAM guard; PR checks on the hosted runners; if two lanes do not fit, lane B pauses and the one-lane column of §7.2 applies | item 21; build-wait time and paging per lane; velocity per lane at the M0/M1 exits |
| 20 | **The public repository publishes what it must not** ([AR §11] #36, #37; [AR §10] risk 41): owner-derived corpora or fixtures, the owner's words, a secret, or a commit with an AI co-author | medium / high | a gitignored local directory for owner data, never committed or sent to CI; no API key in CI; the pre-commit, `commit-msg` and PR-body checks of §3.1 item 7; harness attribution off in the committed configuration (A2); the owner reviewed the published tree and accepted it on 2026-09-27 (A3) | pre-merge-gate refusals |

---

## 9. Edit list for `docs/ARCHITECTURE-RESEARCH.md`

> **Historical record** of edits applied to [AR] on 2026-09-26. The figures and decision states it quotes — the dedicated Windows 11 test host, decisions "due before M0", earlier unit ranges and calendars — are superseded by later passes and by the owner's answers of 2026-09-26 (§10.11, [90 §14.4]); [AR] and §0–§8 of this file are authoritative. The `research/design/...` paths below sit inside fenced blocks and are relative to `docs/`, where [AR] lives.

### 9.0 Conventions and coverage

Line numbers refer to [AR] as of 2026-09-26 before this list was applied (1,558 lines) and are informational; the anchors are authoritative. Each edit is a fenced block:

- `- <text>` — an exact substring of [AR] that occurs **exactly once**; it is replaced by the `+` lines that follow.
- `¶ <text>` — the one line of [AR] that **begins** with this text is replaced as a whole.
- `⇤ <text>` … `⇥ <text>` — every line from the one beginning with the first text through the one beginning with the second, inclusive, is replaced.
- `+ <text>` — replacement lines; consecutive `+` lines are joined with newlines; a bare `+` is an empty line. When a replacement keeps its anchor and adds text, the anchor is repeated in full.

Apply §9.1, §9.2 and §9.3 in order, never by blind find-and-replace: `S1`, `S2`, `S4`, `S6` and `S12` also occur as [21] finding labels, [D §12] source labels and store variables (§9.5). Every anchor was checked by script to occur exactly once in the file and at the moment it is applied.

**Coverage.** A grep of [AR] for `SQLite`, `oracle`, `S0`–`S6`, `slice`, `adopt`/`adoption`, `trait`, `throw-away`, `swap`, `v1.1`, `deferred`, `later`, `M6`, `fast-import`, `cat-file`, `merge-base` and `ImageBackend` gives the matches handled here. §9.1 covers what the owner's decisions remove directly (SQLite use, the oracle backend, the engine trait as a swap point, the S0–S6 slices, adoption milestones and early-adoption rationale) and the sections the owner's brief names (§0, §1, §2 T10/T13, §8, §9, §10, §11, the Review log). **[AR] contains no node cap**; [60d]'s A1 node cap was never merged and is not carried over. §9.2 covers what this roadmap's scope decisions change: v1.1 deferrals resolved into the release, the in-process git object layer, the recursive virtual base, the timing of the T1/T2 triggers and of the `shared` field class, and the leader's `M6` label (which would collide with milestone M6). §9.3 places R4 and R5 as **marked slots** that point to [40] and [50]; integrating those designs is a later step, required before M0 exits (§3.1). The legacy labels `M0` (measurements), `M1` (storage: "Storage engine M1", "before M1") and `M3` (merge tests) keep their meaning under the new numbering and are not edited. §9.5 lists the matches that must stay.

### 9.1 Required edits: SQLite, oracle backend, engine trait, slices, adoption; the sections the brief names

```text
E1 · header · L3
- where it changes them, it says so.*
+ where it changes them, it says so. Amended on 2026-09-26 by the owner's decisions to use no third-party database at any stage and to build every component once, to its final specification, in dependency order: §2.13 records the decisions, §9 summarises the roadmap of record [60], and the Review log's last entry lists what changed. R4 and R5 appear as marked slots that point to their designs [40] and [50].*

E2 · sources · L27
- | [30] | [research/design/30-synthesis.md](research/design/30-synthesis.md) | the synthesis this document revises |
+ | [30] | [research/design/30-synthesis.md](research/design/30-synthesis.md) | the synthesis this document revises |
+ | [40] | [research/design/40-file-links-design.md](research/design/40-file-links-design.md) | the R4 design (move-proof file links); integrated into this document in a later step (slots marked) |
+ | [41] | [research/design/41-file-links-critique.md](research/design/41-file-links-critique.md) | the adversarial review of [40]; its blockers are resolved in [40] before M0 |
+ | [50] | [research/design/50-query-language-design.md](research/design/50-query-language-design.md) | the R5 design (the query language LQ); integrated into this document in a later step (slots marked) |
+ | [60] | [research/design/60-roadmap.md](research/design/60-roadmap.md) | the roadmap of record after the owner decisions of 2026-09-26: dependency-ordered milestones M0–M11, the format-freeze list, the reference-model specification, the measurement protocol and benchmark plan, the owner-decision schedule, the release gate, the calendar and the risks |
+ | [61] | [research/design/61-roadmap-critique.md](research/design/61-roadmap-critique.md) | the adversarial review of [60]'s first issue; every finding is dispositioned in [60]'s Review log |

E3 · how to read, for implementers · L31
- §11 the questions only the owner can answer, each with a recommended default so that work can start before they are answered;
+ §11 the questions only the owner can answer, each with a recommended default and due before the first milestone whose format, protocol or code it can change ([60 §3.14]);

E4 · §0 bullet 10 · L52
¶ 10. **Build order (T13):**
+ 10. **Build order (T13), done right:** no SQLite or other third-party database at any stage, no interim or throwaway stages, no temporary modes and no use before the release gate (owner decisions of 2026-09-26, §2.13). Dependency order: **M0** contract and evidence — the on-disk format v1 frozen with every R1/R3 reservation, the R4/R5 reservations of [40]/[50], the `Vfs` fault model and store parameters for every threshold; the complete Rust reference model; the query surface frozen by an accuracy benchmark; R4's pure file libraries validated on the owner's corpora; measurements and the measurement protocol; the repository, CI, a dedicated test host and an OS-crash rig; **M1** storage engine with the final multi-process protocol, recovery, the physical branch machinery and the section registry; **M2** graph core; **M3** version control (R1); **M4** in-process git object layer; **M5** git image (R3); **M6** file-link runtime (R4); **M7** query language (R5); **M8** CLI (R2 user-visible); **M9** agent interface; **M10** MCP; **M11** release hardening to the "complete and hardened" gate. R4's graph layer and merge rules are built inside M2 and M3. Each milestone meets its final specification and passes its full gates before anything builds on it; the test workstream (fault-model simulation, crash enumeration, kill loops, an OS-crash loop, fuzzers, mutation testing, differential tests against the reference model and the git CLI) runs from M0 [60].

E5 · §0 "Why this design" · L54
- and the build re-ordered so adoption precedes the engine.
+ and the build ordered by technical dependency, each component built once to its final specification (§9, [60]).

E6 · §0 "The biggest risks" (1) · L56
- 16-writer kill loops gate the engine milestone; `moirai backup`/`restore` (S1) and the daily image export
+ 16-writer kill loops and an OS-crash loop (bugchecks and hard power-offs of a virtual machine) gate the storage milestone (M1) before anything is built on it; `moirai backup`/`restore` (M1) and the daily image export

E7 · §0 "The biggest risks" (4) · L56
- (4) Build size: two long poles (protocol and image determinism); the trait-and-oracle path keeps adoption independent of the engine.
+ (4) Build size and time to first use: two long poles (protocol and image determinism), R4 and R5 on top, and no use before the release gate (≈ 318–422 units; release at ≈ 28–56 weeks with two lanes, P50 ≈ 39, est. [60 §7]). Mitigation: dependency order with a second lane for leaf components, every decision, measurement and accuracy experiment that shapes the frozen format settled before M0 exits, the reference model as the executable specification, and validation on recorded data instead of live use [60 §8].

E8 · §1 row 1 · L64
- no `petgraph`, no async runtime in the core (§2 T10).
+ no `petgraph`, no async runtime in the core (§2 T10). No third-party database anywhere — not as a backend, a stepping stone, a test oracle or a benchmark; the differential oracle is the in-project Rust reference model (owner decision of 2026-09-26, §2.10, [60 §4]).

E9 · §2.2 T2 Rejected · L101
- *Leader in v1* [C M3]: bundles pipe security, forwarding, failover into the first adoption milestone (F-C5, [22 §3.3 issue 4]).
+ *Leader in v1 by default* [C M3]: bundles pipe security, forwarding and failover into the protocol before any measurement shows they are needed (F-C5, [22 §3.3 issue 4]).

E10 · §2.10 T10 Decision · L177
- **Storage engines are allowed as test oracles and as the S0–S5 throw-away backend behind the engine trait** (owner decision #2).
+ **No third-party embedded database is built, linked or run for any purpose, in or out of tree — not in the product, not as a temporary backend or stepping stone, not as a test oracle, not as a benchmark or informational reference point.** Owner decision of 2026-09-26 (§11 #2), verbatim translation: "No, we do not use SQLite — we build our own [engine] right away." The differential oracle is the in-project Rust reference model (§8.2, [60 §4]); performance is judged only against the absolute budgets of §8.1/§8.3 and the physical floors measured at M0; the `Cargo.lock` dependency lint (GT20) enforces the rule. *(Text as amended by the verification pass, §10.6.)*

E11 · §2.10 T10 Revisit trigger · L183
- **Revisit trigger.** Owner allows a C engine (LMDB via `heed`) as the materialized state → M0 shrinks by a third and T1 becomes Option B. Owner forbids any C code → `lz4_flex` at a worse ratio.
+ **Revisit trigger.** None for third-party storage engines (owner decision of 2026-09-26). Owner forbids any C code → `lz4_flex` at a worse ratio.

E12 · §2.13 heading · L205
¶ ### 2.13 T13 — Scope of v1
+ ### 2.13 T13 — Build order and scope

E13 · §2.13 T13, the whole body · L207–213
⇤ **Decision.** The smallest adoptable slice is S0–S2 of §9
⇥ **Revisit trigger.** Owner's first target is publishing to git
+ **Decision.** Built once, in dependency order. The owner decided on 2026-09-26 (verbatim translations): **(1)** "No, we do not use SQLite — we build our own [engine] right away." **(2)** "'So that you can start using it as early as possible' is not important — it must be done properly right away." Consequences: no SQLite or other third-party database at any stage; no interim or throwaway stages, temporary modes, node caps or reduced guarantees taken to ship sooner; no milestone ordered by time to adoption; every component is built to its final specification and passes its full gates before anything builds on it; no milestone modifies a certified one except through an extension point certified earlier (the section registry of M1, the validator and merge-rule tables of M2–M3, the built-in and named-query registries of M7); the owner's workflow starts using moirai only when the release gate ("complete and hardened", §9) is met. Milestones ([60 §3]): **M0** contract and evidence (on-disk format v1 frozen with every R1/R3/R4/R5 reservation, the `Vfs` fault model and the store parameters; the `Store` API; the complete Rust reference model; the query surface frozen by an accuracy benchmark; R4's pure file libraries validated on the owner's corpora; measurements; the repository, CI, a dedicated Windows 11 test host and an OS-crash rig); **M1** storage engine (final multi-process protocol, recovery, the physical branch machinery, the section registry); **M2** graph core (with R4's graph layer); **M3** version control, R1 (with R4's merge rules and the recursive virtual base); **M4** in-process git object layer; **M5** git image, R3; **M6** file-link runtime, R4; **M7** query language, R5; **M8** CLI, R2 user-visible; **M9** agent interface; **M10** MCP; **M11** release hardening. The release includes `tag`, `undo`, `revert` (incl. `--mainline 1`), `reflog`, `op log`/`op restore`, `cherry-pick`, `backup`/`restore`/`repair`, branch promotion, the recursive virtual merge base, FTS tier 2, schema strengthening, `--with-oplog`, SHA-256 images, bundles and in-process ancestry for `check`/`stale`. Not built, by decision rather than for schedule: `rebase --onto`, the tracked-directory and orphan-branch destinations, the leader and `watch` (unless the M0 measurements require the leader), the `shared` field class (unless the owner chooses it before M0), HTTP MCP, reftable writing, network transport without git, live cross-machine writers ([60 §1.3]).
+
+ **Chosen from.** [60]; D's component content [D §9]; the engine-first order of [A §9] and [C §9]; the split of D's M1 [22 §7.1] into storage (M1), graph core (M2) and version control (M3); the adversarial review [61] of [60]'s first issue (evidence before the freeze, a specified fault model and an OS-crash gate, certified extension points, R4 attached to its host components).
+
+ **Rejected.** *Oracle first* (S0–S5 on a throw-away SQLite backend behind an engine trait, the engine swapped in at S6; [22 §7.3] and this section's previous text): rejected by the owner — no third-party database at any stage and no throwaway stages. *Adoption-driven engine first* ([60]'s first draft: a serialized-access stage used as a task tracker under a node cap, the lock-free protocol switched on later): rejected by the owner — no temporary modes, no node caps, no milestone ordered by time to adoption. *R4 as one late milestone* ([60]'s first issue): its graph layer, merge rules and image carrier would have modified certified milestones [61 B1]; they are built inside M2, M3 and M5. *D's M1 as written* (engine + refs + branches + overlays + promotion + checkout + undo + revert + tag + GC in one milestone) [D §9]: ~40 units behind one gate (D8); split along the dependency seams, each with its own gates. *Two object backends for the image* (`git fast-import` first, a hand-written layer later, behind a trait): a throwaway stage by construction; one in-process layer is built (M4).
+
+ **Revisit trigger.** A milestone-exit measurement showing that a budget cannot be met by the specified design → the design changes (for example T2's leader or T1's Option B) and the affected milestones re-run their gates; scope is never reduced to hold a date without an owner decision recorded in §11.

E14 · §2.14 Placement · L219
- that is one reason `backup`/`restore` ship in S1
+ that is one reason `backup`/`restore` are part of the storage engine (M1)

E15 · §2.17 [22] row §2.3 · L340
- Adopted: 5-minute experiment in S0;
+ Adopted: 5-minute experiment in M0, re-run in M9 on the then-current harness;

E16 · §2.17 [22] row §2.5 · L342
- ratios measured in S0.
+ ratios measured in M0.

E17 · §2.17 [22] row §2.7 / D8 · L344
- Adopted: S0–S5 on the oracle backend behind the trait; engine at S6; M1 split.
+ Superseded by the owner decisions of 2026-09-26: no oracle backend and no use before the release gate; the value of packs, briefs and the query language is validated on recorded data instead (the query accuracy benchmark in M0, HDR-vs-pack diffs on recorded dispatches in M9, the campaign replay in M11), and D's M1 is split into M1/M2/M3 ([60]).

E18 · §2.17 [22] row §2.9 · L346
- open owner questions in S2.
+ open owner questions, with the import tooling built in M9 and run at the release cutover.

E19 · §4.6 · L648
- (fixed in the S0 format spec)
+ (fixed in the M0 format specification, which freezes every on-disk layout of §4 at M0 exit, [60 §2.5])

E20 · §5b.2 Grammar · L906
- the S4 deliverable includes an ABNF
+ the M0 format specification includes an ABNF

E21 · §5b.5 rule 8 · L958
- the S4 "reconstruct and re-hash" unit test
+ the M5 "reconstruct and re-hash" unit test (its fixtures are the M0 gate-0 carrier table, [60 §2.5])

E22 · §5b.6 import step 5 · L979
- Cross-store import is v1.1 (owner decision #11); this rule is part of its requirements and of the S4 corpus.
+ Importing another store's image is in the release and this rule is part of the M5 corpus; live cross-machine writers stay out of scope (owner decision #11).

E23 · §5b.7 · L1001
- **Gates (S4 exit).**
+ **Gates (M5 exit).**

E24 · §5b.7 gate 3 · L1001 (store variables renamed so they cannot be read as slice names)
- `import(export(S1)) ⊕ import(export(S2))`
+ `import(export(St1)) ⊕ import(export(St2))`

E25 · §7.4 · L1302
- is measured in S0 and reported in the footer
+ is measured in M0 and reported in the footer

E26 · §7.5 · L1335
- the 5-minute experiment is the first S0 task
+ the 5-minute experiment is an M0 task, re-run in M9 on the then-current harness

E27 · §8.1 workload · L1359
- (**claimed** ratio, measured in S0)
+ (**claimed** ratio, measured in M0)

E28 · §8.1 durable-commit row · L1383
- + Defender close cost **pending S0** (U10)
+ + Defender close cost **pending M0** (U10)

E29 · §8.1 CI budget gates, conditions · L1405
- **CI budget gates** (run on every PR on Windows with Defender on and under a synthetic 16-agent load, and separately on an idle machine; regressions block):
+ **CI budget gates** (measured under the M0 measurement protocol [60 §5.1] — nightly on the dedicated Windows 11 x64 test host with Defender on, idle and under the replayed 16-agent load fixture, and at every milestone exit on the owner's machine; PR-level runs on hosted runners are a secondary regression signal; regressions beyond the measured noise band block):

E30 · §8.1 CI budget gates, end · L1405
- `doctor --verify` clean after every kill-loop run.
+ `doctor --verify` clean after every kill-loop run. Each gate becomes mandatory at the milestone that builds what it measures and stays mandatory afterwards ([60 §3.13, §5.4]); two floor-relative gates are added from M1 — durable commit p50 ≤ the flush floor + 0.5 ms with the floor re-measured in the same run, and writer-byte hold p99 ≤ 5 ms — and idle CPU is measured as zero CPU time and zero context switches over 10 minutes. The R4 and R5 budget rows ([40 §7.4], [50 §8.3]) join this list when those designs are integrated (slot).

E31 · §8.2 heading · L1409
¶ **S0 measurements on the owner's machine**
+ **M0 measurements on the owner's machine** (under the M0 measurement protocol [60 §5.1]: Defender on; idle and under the replayed 16-agent load fixture; n ≥ 1,000 per p99 and ≥ 10,000 for operations under a millisecond, 5 repetitions, gated on the median p99; floors re-measured interleaved in the same run; peak private bytes at exit and peak working set via `GetProcessMemoryInfo`, flush counts, page faults; `hyperfine -N --warmup 5` for spawn-to-exit):

E32 · §8.2 item 1 · L1410
- on a 64 MiB log file, n = 200 (the Defender close cost, G8) — decides whether the CLI must forward to a leader early.
+ on a 64 MiB log file, n per the protocol (the Defender close cost, G8) — decides, before M1, whether the CLI must forward to a leader.

E33 · §8.2 item 10 and new items 11–17 · L1419
- 10. Tail replay at a full 4,096-op / 4 MiB tail (U13).
+ 10. Tail replay of a full tail at candidate checkpoint thresholds (U13) — the threshold is chosen so that open at 1e6 with a full tail meets ≤ 3 ms (§4.7 estimates 3–5 ms at a full 4 MiB tail).
+ 11. Physical floors: data-only flush on a zero-filled extent vs append (G11), open + map of 8 sealed files, `HEAD` pread, spawn-to-exit of an empty Rust executable from the stable install path, directly and through the agent's Git-Bash wrapper — the baselines of the floor-relative gates ([60 §5.3]).
+ 12. Lock-release delay after `TerminateProcess` of a holder, p50/p99/max, idle and loaded (W2) — confirms the 2 s bound and parameterises lock-delay injection in the simulator and the kill loops.
+ 13. BLAKE3 and xxh3 throughput on this CPU, idle and loaded, through the Rust build (so far measured only under 97–100 % load, [10 §0]).
+ 14. Layout probes — CSR probe, frozen-bitset AND/popcount, column scans and overlay probe at 1e5/1e6 — plus the T1 and T2 trigger quantities (a point read after a full tail and a delta checkpoint at 0.5 M nodes; the 16-writer wait and MCP overlay catch-up), so T1's structure and the leader are decided before M0 freezes the layouts.
+ 15. File-system costs for R4 through the `Vfs`: stat, directory enumeration with file ids, rename of one file and of a 1,000-file directory under Defender ([09], [13]).
+ 16. The 16-agent load fixture — a recorded CPU, disk and memory profile of a real 16-agent campaign (no content), replayed with free RAM held at ≈ 1.8 GB — and the run-to-run noise band of every gated quantity on the test host and on hosted runners ([60 §5.1]).
+ 17. Calibration of the OS-crash rig: across ≥ 100 hard power-offs of the virtual machine, a deliberately unflushed write is lost at least once and a flushed write never is (host I/O cache off, guest flushes honoured), so the OS-crash loop can observe lost unflushed data.

E34 · §8.2 "Oracles and baselines" · L1421
¶ **Oracles and baselines.**
+ **Reference model and baselines.** The differential oracle is a naive in-memory Rust reference model inside the project ([60 §4]), written first and complete at M0 by a separate author from the specification — `BTreeMap` graph, derived state recomputed from scratch, branch states by replay from genesis, merges by the §5a.7 table over materialised base and side states (the recursive virtual base by definition), marker absorption by exhaustive ancestry, its own query parser, binder and nested-loop evaluator, its own canonical-form encoder (commit ids compared byte for byte), a brute-force anchor resolver for R4 — deliberately using different algorithms from the engine and sharing no code with it; its rule tables are signed by the owner; it is test-only, never linked into the binary and never a backend. A test-only format oracle decodes every frozen structure independently of the product codec. For git objects and images the independent oracle is the git CLI; for file identity it is the ground truth of the `ProjectFs` simulator. No SQLite or other third-party embedded database is built, linked or run for any purpose — not as a backend, an oracle, a benchmark or an informational reference point, in or out of tree. Performance is judged only against the absolute budgets of §8.1/§8.3 and the physical floors of item 11 (durable commit p50 ≤ flush floor + 0.5 ms, exactly one flush per durable commit, open ≤ 3 ms at 1e6, private RSS ≤ 4 MB per CLI). *(Text as amended by the verification pass, §10.6.)*

E35 · §8.2 simulation bullet · L1426
- - **Deterministic multi-process simulation** (own workstream, ~20 % of the build):
+ - **Deterministic multi-process simulation** (own workstream, ~20 % of the build; mandatory from M1, [60 §3.13]; the in-memory `Vfs` enforces the fault model frozen at M0 — any subset of unflushed sectors lost and one sector torn, metadata operations surviving as a prefix of their issue order, a failed flush leaving its range indeterminate forever, mixed-sector concurrent reads, `ERROR_DISK_FULL`, pauses of any length and wall-clock steps — and crash states enumerate bounded subsets of unflushed writes and "flush error, more commits, crash" sequences, [60 §2.5]):

E36 · §8.2 kill-loop bullet, head · L1427
- - **Windows kill loops**: 16 writer + reader processes
+ - **Windows kill loops** (mandatory from M1 through the storage driver, from M3 across branch bindings, from M5 with image exports and imports, from M6 with file intents, from M8 through the CLI binary, from M10 with the MCP server held open; in four variants — `TerminateProcess`, `NtSuspendProcess` of random processes for 1–120 s, a small VHDX filled to disk-full, wall-clock steps of ±1 h — with every acknowledgement reported to the harness over a pipe before it is printed): 16 writer + reader processes

E37 · §8.2 kill-loop bullet, end; new bullets · L1427
- holds every commit acknowledged before the backup's `committed_lsn` (CM8).
+ holds every commit acknowledged before the backup's `committed_lsn` (CM8).
+ - **OS-crash loop** (mandatory from M1; ≥ 1,000 cycles at M1 exit, nightly afterwards, ≥ 5,000 cumulative for the release gate): a Windows 11 guest in VirtualBox or VMware Workstation (the host has no Hyper-V) with the host I/O cache off and guest flushes honoured runs the kill-loop workload; every writer streams each acknowledged commit id to the host; the host triggers a bugcheck (Sysinternals NotMyFault) or a hard power-off at random; after reboot every recorded commit must be present and `doctor --verify` clean. Loss of the drive's own volatile cache is covered only by the simulator; the design assumes the drive honours FLUSH ([60 §3.13] GT15).
+ - **Mutation testing** (at every milestone exit, CI only, never in a product build): the differential and property suites must kill ≥ 90 % of the compiling, non-equivalent mutants in the semantic crates; protocol bugs seeded into the real engine at M1 exit (skip the flush, publish before the flush, adopt without CAS, adopt by re-flush only, delete before the durable `HEAD` barrier, truncate past valid records) must each be caught ([60 §3.13] GT16).

E38 · §8.2 property-test bullet · L1428
- - **Property tests**: incremental derived state == full recompute
+ - **Property tests** (differential against the reference model of [60 §4] unless stated; every threshold is a store parameter, swept over a tiny test profile and the production values, so threshold-gated paths are compared at model scale): incremental derived state == full recompute

E39 · §9, the whole section · L1435–1450
⇤ ## 9. Roadmap
⇥ Adoption path:
+ ## 9. Roadmap
+
+ **Built once, in dependency order (owner decisions of 2026-09-26, §2.13).** No SQLite or other third-party database is used at any stage — not as a backend, a stepping stone, a test oracle or a benchmark. There are no interim or throwaway stages, no temporary modes, no node caps and no reduced guarantees taken to ship sooner. Every component is built to its final specification and passes its full gates before anything builds on it; no milestone modifies a certified one except through an extension point certified earlier (the section registry of M1, the validator and merge-rule tables of M2–M3, the built-in and named-query registries of M7); milestones are ordered by technical dependency; the owner's workflow starts using moirai only when the release gate below is met. [60] holds the full plan: the dependency graph with every edge justified, per-milestone scope, exclusions, exit criteria and gates, the format-freeze list, the reference-model specification, the measurement protocol, the owner-decision schedule, the release gate, the calendar and the risks. **R4 and R5 are slots here**: their designs are [40] (revised after its review [41]) and [50], every R4/R5 entry below points to them, and their integration into this document is a later step, required before M0 exits.
+
+ | Milestone | Scope (final specification) | Exit criteria (owner's machine under the M0 measurement protocol where time, memory or Windows behaviour is concerned) | Gates mandatory from here |
+ |---|---|---|---|
+ | **M0 — Contract and evidence** | on-disk format v1 frozen with every R1/R3 reservation, the R4/R5 reservations of [40]/[50] (§4.6 slot), the `Vfs` fault model, store parameters for every threshold, the gate-0 carrier table and the `.moi` ABNF; `Store` API; the complete Rust reference model with its own query parser and canonical-form encoder; the query surface frozen by the LQ-Bench accuracy benchmark run on the model; R4's pure file libraries built and validated on the owner's replay corpora; measurements 1–17 of §8.2 and the measurement protocol; the repository, CI, a dedicated Windows 11 x64 test host and the OS-crash rig. Entry: [40] revised after [41], [50] reviewed, the owner decisions due before M0 answered | specification review with zero open blocker or major findings; hand-written hex fixtures decoded and re-encoded by the format oracle; the crash enumerator finds ≥ 12 seeded protocol bugs, including adoption after a failed flush and deletion before a durable `HEAD`; LQ-Bench ≥ 85 % first try and ≥ 95 % after one retry with no confident-wrong write; R4 replay targets met (every exact rename re-bound, none wrong); leader, T1 structure, dictionary, codec and thresholds decided by measurement; rule tables signed by the owner; [40]/[50] integrated here | model fixtures; harness validation; LQ-Bench; fuzzing and mutation testing of the file libraries |
+ | **M1 — Storage engine** | §4 complete with the protocol of §4.5 and §6.1 as settled by the M0 specification review; refs, pins, `ClientHead`, per-ref index, view construction and promotion (the physical half of §5a.3); the section-producer and record-fold registry, certified with every reserved section and record kind; GC with a durable `HEAD` barrier before deletion; `backup`/`restore`/`repair --rebuild-from-log`; the leader only if M0 requires it | durable commit p50 ≤ flush floor + 0.5 ms with exactly one flush; open ≤ 1.5 / 3 ms at 1e5 / 1e6 with a full tail; writer-wait p99 ≤ 50 ms and writer-byte hold p99 ≤ 5 ms with 16 writers; first read of a ref forked 1k–60k commits ago ≤ 3–20 ms; RSS ≤ 4 MB; ≥ 1,000 OS-crash cycles without a lost acknowledged commit; every seeded protocol bug caught | crash enumeration under the fault model; multi-process simulation ≥ 1e6 steps; Windows kill loop, 10,000 iterations in four variants; OS-crash loop; record and segment fuzzers; storage differential incl. commit ids |
+ | **M2 — Graph core** | §3 complete: kinds, schema incl. strengthening, invariants, I5′, derived state, delete policies, leases, idempotency, markers produced by ops, change feed, FTS tiers 1–2, `doctor --verify`; R4's graph layer (per [40]); the real section producers | `get` ≤ 5 µs; `ready` page ≤ 300 µs at 1e5 with 50 stale markers; blocking ids ≤ 300 µs at 1e5 (engine time, printing excluded); derived state == recomputation at 1e5 and 1e6; M1's rebuild, budgets and crash gates re-certified with the real sections | semantic differential against the model; derived-state properties; node-40 fixtures; mutation testing |
+ | **M3 — Version control (R1)** | §5a and §5d complete incl. `op log`/`op restore` and the recursive virtual merge base; R4's merge rules (per [40]); a permanent version-control test driver | register incidents replay; the ten-door I26′ test; 10 lanes × 1k ops with every conflict class; merge ≤ 50 ms and sync ≤ 40 ms at 1e5; lane read ≤ 10 ms at a 14-day fork | version-control properties against the model; kill loop across 16 branch bindings |
+ | **M4 — Git object layer** | in-process loose objects, packs, idx, commit-graph (split chains), refs and bundles, SHA-1 and SHA-256, reftable read, ancestry (§5b.6); second lane from M1's `Vfs` certification | agreement with the git CLI on the owner's repositories and on synthetic ones; ancestry ≤ 1 ms with a commit-graph, ≤ 5 ms without | git-CLI differential; object fuzzers |
+ | **M5 — Git image (R3)** | §5b complete, driven through the `Store` API; R4's image carrier (per [40]); the verbs arrive with the CLI | §5b.7 gates 0–3; full export of 1e5 ≤ 3 s | image gates; `.moi` fuzzer |
+ | **M6 — File-link runtime (R4)** | per [40] (slot): the `ProjectFs` seam and simulator, resolver and settle points, the crash-safe file-intent protocol, the tree gate over in-process ancestry, the USN-journal reader and git evidence | per [40]: zero wrong automatic re-binds; every anchor state equal to the model's brute-force search; the move/edit pattern matrix on real NTFS | link differential against the model; pattern matrix |
+ | **M7 — Query language (R5)** | per [50] (slot): binder, executor, planner, `TX` writes, versioned queries and the standard library of named queries; R4's link built-ins | per [50]: anchored queries ≤ 0.3 ms at 1e6; LQ-Bench re-run on the product | differential against the model's evaluator; metamorphic tests; grammar fuzzer |
+ | **M8 — CLI (R2)** | §7.1 as amended by [50] and [40]; `q`/`tx`, image and file verbs; discovery and placement (§2.14); `check`/`stale` in-process | golden outputs; Git-Bash and PowerShell transport tests; read-verb spawn-to-exit ≤ floor + 5 ms | kill loop through the binary |
+ | **M9 — Agent interface** | §7.4–§7.5, skills, generated views, hooks incl. R4's accelerators, import tooling incl. `links import` | packs judged complete against recorded HDRs; hook p99 ≤ 1 s under a 16-agent burst | hook fixtures |
+ | **M10 — MCP** | §7.2 with `query` and `TX` writes | a Bash-less review round on a lane; RSS ≤ 10 MB + 1 MB × min(active branches, 8) | conformance; kill loop with the server held open |
+ | **M11 — Release hardening** | the release gate; nothing new | the release gate below | 72-hour soak; upgrade drill |
+
+ **Calendar** (est., [22 §7.1] units at 5–8 per week — the earlier plan's rate, not yet measured; velocity is measured at the M0 and M1 exits and the calendar re-issued then): ≈ 318–422 units. With two supervised agent lanes: storage engine certified at 9–19 weeks, R1 at 17.5–35, R3 at 19.5–39.5, R4 at 20–40.5, R5 at 21–42.5, **release at 28–56 weeks (P50 ≈ 39, P90 ≈ 47)**. With one lane: release at 42.5–87 weeks (P50 ≈ 59.5, P90 ≈ 72.5) [60 §7].
+
+ **Release gate ("complete and hardened", [60 §6]).** Every milestone exited with all its gates; every in-release item of [60 §1.3] built; R1–R5 demonstrated end to end through the binary, hooks and MCP server; multi-process simulation nightly for ≥ 4 weeks and the kill loops in every variant with the last 14 nights clean **on the release-candidate commit** (any change to engine code restarts the count); ≥ 5,000 cumulative OS-crash cycles without a lost acknowledged commit; a 72-hour soak with kill loops across branches without a lost acknowledged commit or a `doctor --verify` finding; ≥ 1e7 differential cases per component without an open disagreement and ≥ 90 % mutation kill rate in the semantic crates; ≥ 7 CPU-days per fuzzer; every budget of §8.1 met at 1e4, 1e5, 1e6 and the owner's 0.3–0.5 M scale with no number still "claimed" or "pending"; backup, restore, repair, image-restore and disk-full drills; an **upgrade drill** to a synthetic format version 2 (a derived-file change through `repair --rebuild-from-log`, a log or canonical-form change through `image export --with-oplog` → `image import`, both state-identical against the reference model, and rollback by restoring the pre-upgrade backup); a synthetic replay of a recorded campaign with zero invariant violations and zero double dispatch; a final three-lens review with no open blocker or major finding; the cutover rehearsed on a copy; no open defect of major severity.
+
+ **Cutover.** Once, at the release gate, at a campaign boundary: backup; the rehearsed import of the standing rules, current pins, live lanes and open owner questions (and of the existing file citations through `links import`, if the owner decides so); hooks, skills and the MCP server installed; `OPEN-QUESTIONS.md`/`BACKLOG.md`/`MEASUREMENT-QUEUE.md` and the MEMORY.md resume block replaced by generated views (`export md`), regenerated, never merged; the 254 memory files kept as a read-only archive linked by `artifact` nodes. Rollback removes the hooks and the MCP entry; nothing of the old workflow is deleted.
+
+ **Not built, by decision.** `rebase --onto`; the tracked-directory and orphan-branch image destinations; the leader, `watch` and group commit unless the M0 measurements require the leader (then M1); the `shared` field class unless the owner chooses it before M0; HTTP MCP; reftable writing; network transport without git; live cross-machine writers (decision #11); embeddings (§12); a Unix `Vfs` (Windows 11 first; the seam keeps it additive); the exclusions of [40 §1.4] and [50 §1.3].

E40 · §10 row 1 Mitigation and Signal · L1458
- DST + kill loops as the S6 gate; `doctor --verify` in CI; the oracle backend until S6 passes; `backup`/`restore` bound the blast radius | a kill-loop failure blocks the S6 swap |
+ the `Vfs` fault model frozen at M0; DST, crash enumeration and kill loops as the M1 exit gate before anything is built on the storage engine, repeated across branches (M3), with image exports (M5), with file intents (M6), through the CLI (M8) and with the MCP server (M10); the OS-crash loop from M1; `doctor --verify` in CI; `backup`/`restore` and `repair --rebuild-from-log` bound the blast radius | a DST, kill-loop or OS-crash-loop failure blocks the exit of the milestone that owns the failing component and of every later one |

E41 · §10 row 5 · L1462
¶ | 5 | The from-scratch engine takes far longer than estimated |
+ | 5 | The from-scratch engine takes far longer than estimated — and it is on the critical path to every milestone | high / high | "95 % of the effort is testing" (folklore but directionally right [04 §3.14]); redb needed years for multi-process; DoltLite ~2,000 PRs | the format, not the engine, is the contract (frozen at M0); the engine is split along its dependency seams (M1 storage, M2 graph core, M3 version control), each certified before the next; leaf components run in a second lane; the simulator, the reference model and the kill loops are built from M0; scope is never cut to hold a date without an owner decision | milestone exits against [60 §7] |

E42 · §10 row 6 Signal · L1463
- | the S0 experiment result |
+ | the M0 experiment result (re-run in M9) |

E43 · §10 row 8 Mitigation and Signal · L1465
- S0 measurement; packs via fast-import; leader forwarding as the fallback | S0 numbers
+ M0 measurement, which decides the leader before M1; packs by default in the in-process git object layer; leader forwarding only if the M0 numbers require it | M0 numbers

E44 · §10 row 13 Mitigation · L1470
- `backup`/`restore` in S1 (transaction-consistent, verified)
+ `backup`/`restore` in M1 (transaction-consistent, verified); the OS-crash loop from M1

E45 · §10 new rows after row 13 · L1470
- (CM8) | backup-age line in `doctor`/`brief` |
+ (CM8) | backup-age line in `doctor`/`brief` |
+ | 14 | No use of moirai before the release gate: the owner's workflow gets nothing for ≈ 28–56 weeks (two lanes; P50 ≈ 39), and the value of packs, briefs and the query language is not proven in live use first | high / medium | [60 §7]; [22 §2.7] made adoption-first the answer to exactly this | validation on recorded data instead of live use: the query accuracy benchmark on the owner's agent model (M0), R4's replay corpora (M0) and pattern matrix (M6), HDR-vs-pack diffs on recorded dispatches (M9), the synthetic campaign replay (M11); a demonstration on a synthetic store at every milestone exit | owner judgement at each demonstration |
+ | 15 | The M0 format freeze misses something a later milestone needs (an R4/R5 field, a layout that fails a budget, a protocol rule) | medium / medium | the format is frozen before the engine and before R4/R5 are built | [40]'s revision and [50]'s review are M0 entry criteria; the evidence that can change the format (the query accuracy benchmark, R4's replay corpora, the T1/T2 probes, the gate-0 carrier table) is gathered before the freeze; an independent specification review; change control: a format change after M0 re-opens M0 and re-runs the gates of every passed milestone; pre-release stores are regenerated, never migrated | format change requests |
+ | 16 | The reference model and the engine share a misunderstanding of the specification (no independent third-party oracle; both authors are Opus agents) | medium / medium | both are written from this document | different algorithms; no shared code (the model has its own query parser, binder, canonical-form encoder and format decoder); separate authors; owner-signed rule tables and owner-verified core fixtures (register incidents, the §5d.3 node-40 table, the §7.6 walk-through); mutation testing and seeded protocol bugs in the real engine; `doctor --verify` as a third recompute; the git CLI and the `ProjectFs` simulator's ground truth as independent oracles | model/engine disagreements, triaged as specification findings; mutation kill rates |
+ | 17 | An OS crash or power loss loses acknowledged commits that process-kill loops cannot reveal: adopting a record by re-flushing it after a failed flush; deleting or retiring files while the durable `HEAD` is an older one | medium / severe | the development machine has experienced OS crashes and disk-full events [02 §9]; fsync-failure semantics [08 §8.1]; [61 B2] | the `Vfs` fault model frozen at M0; crash enumeration over subsets of unflushed writes and failed-flush sequences; the protocol decisions of the M0 specification review (re-write or verify adopted bytes, no silent truncation past valid records, a durable `HEAD` barrier before destructive maintenance); the OS-crash loop from M1 (≥ 1,000 cycles at M1 exit, ≥ 5,000 for the release gate), calibrated so it can observe lost unflushed data | any crash-enumeration or OS-crash-loop failure |
+ | 18 | The R4 and R5 designs change under review before the freeze ([41] found four blockers in [40], two in hashed format content; [50] has no independent review yet) | high / medium | [41] | M0 cannot start before [40] is revised and [50] reviewed; this document and [60] carry no content of their own for R4/R5 (slots); their sizes come from the designs | open findings at M0 entry |
+ | 19 | The estimates are low and the rate is unmeasured | high / medium | [60]'s first issue was ≈ 75–100 units low [61 M-8]; 5–8 units per week is the earlier plan's estimate, not a measurement | recomputed from [40] and [50]; P90 used for planning; velocity measured at the M0 and M1 exits and the calendar re-issued; scope never cut without an owner decision | velocity at the M0/M1 exits |
+ | 20 | Test infrastructure and machine time: nightly simulation, kill loops, the OS-crash loop, ≈ 3,000 CPU-hours of fuzzing and a 72-hour soak cannot share the owner's only laptop | high / medium | 16 GB with ≈ 1.8 GB free under agent load [05 §2.4]; no repository or CI yet [61 M-7] | M0 provisions the repository, CI, a dedicated Windows 11 x64 test host and the OS-crash rig; hosted runners as a secondary signal; the owner's machine for exit measurements only; a compute budget per milestone ([60 §3.15]) | nightly-run completion rate |

E46 · §11 introduction · L1476
- Ordered by how much the answer changes the design.
+ Ordered by how much the answer changes the design. Each decision is due before the first milestone whose format, protocol or code it can change; the reference model carries every logical rule from M0, so most are due before M0 ([60 §3.14], which also schedules the owner calls that [40], [41] and [50] add).

E47 · §11 row 1 · L1480
- | isolation + markers for one campaign, then revisit | the `shared` class moves into S3 and T3′ becomes
+ | isolation + markers, decided before M0 (the `shared` class changes view construction and the schema's field classes, which M0 freezes); choosing it after the release is a format-version-2 change carried out with the upgrade procedure drilled at the release gate ([60 §6] RG10) | the `shared` class moves into M0–M3 and T3′ becomes

E48 · §11 row 2 · L1481
¶ | 2 | **Build order.**
+ | 2 | **Build order.** **DECIDED BY OWNER 2026-09-26: own engine first, no SQLite, no early-adoption stages.** In the owner's words (verbatim translations): "No, we do not use SQLite — we build our own [engine] right away." and "'So that you can start using it as early as possible' is not important — it must be done properly right away." Every component is built once, to its final specification, in dependency order M0–M11, and the owner's workflow starts using moirai only at the release gate (§2.13, §9, [60]); the former options — S1–S5 on a throw-away SQLite backend behind an engine trait with the engine at S6, and an adoption-driven engine-first draft — are withdrawn. Remaining owner call (due before M0): may the leaf components (the pure file libraries, the reference model, the git object layer, the query front end, R4's `ProjectFs`, the image) run in a second supervised agent lane? | one lane; two lanes | two lanes when the owner can supervise them | one lane moves the release from ≈ 28–56 weeks (P50 ≈ 39) to ≈ 42.5–87 weeks (P50 ≈ 59.5) (est., [60 §7]) |

E49 · §11 row 15 · L1494
- with generated views from S2?
+ with generated views at the release cutover?

E50 · §12 new rows · L1508
- | 4 KiB × depth per tiny commit; hardest to build; RAM tracks history size | [04 §3.2, §6], [05 §16 B] |
+ | 4 KiB × depth per tiny commit; hardest to build; RAM tracks history size | [04 §3.2, §6], [05 §16 B] |
+ | A third-party embedded database (SQLite, redb, LMDB/heed, fjall, sled, RocksDB, Turso) anywhere — product, temporary backend, stepping stone, test oracle, benchmark or informational reference point, in or out of tree | owner decision of 2026-09-26 (§2.10, §11 #2); the reference model is the oracle; performance is judged only against the budgets and the M0 physical floors; the dependency lint enforces it | [60] |
+ | Interim stages, temporary modes, node caps or "useful first slices" taken to ship sooner (a serialized-access stage, a swappable backend, "v1.1" deferrals of guarantees) | owner decision of 2026-09-26 (§2.13): every component is built once, to its final specification, before anything builds on it | [60 §1] |

E51 · Review log "Where" cells and dispositions · L1534–1552
- §5b.5 rule 8, §5b.7, §9 S4 |
+ §5b.5 rule 8, §5b.7, §9 M0, M5 |
- §5d.1, §8.2, §9 S3 |
+ §5d.1, §8.2, §9 M3 |
- §5b.2 rule 8, §5b.7, §9 S4 |
+ §5b.2 rule 8, §5b.7, §9 M5 |
- two-lanes-staged test in S3 and CI
+ two-lanes-staged test in M3 and CI
- §8.1, §9 S3 |
+ §8.1, §9 M3 |
- S1 exit criterion
+ M8 exit criterion
- §5c, §7.1, §9 S1 |
+ §5c, §7.1, §9 M8 |
- §7.6, §8.1, §9 S1 |
+ §7.6, §8.1, §9 M4, M8 |
- (epoch re-roll, G25) in S1;
+ (epoch re-roll, G25) in M1;
- §8.2, §9 S1, §10 |
+ §8.2, §9 M1, §10 |
- in the S3 property tests
+ in the M3 property tests
- F-D11, §8.2, §9 S3 |
+ F-D11, §8.2, §9 M3 |
- ABNF + golden fixtures as an S4 deliverable
+ ABNF + golden fixtures as an M0 format-specification deliverable, implemented in M5
- §5b.6 step 1, §9 S4 |
+ §5b.6 step 1, §9 M0, M5 |
- so S3 gains no merge rule from it
+ so M3 gains no merge rule from it

E52 · Review log row CL9 · L1554
- **Adopted.** Calendar: 5–7 weeks to adoption, 9–13 to R1, 11–16 to R3, 12–19 to S5, 16–25 to S6 with the test workstream in parallel; the fallback when a campaign starts before S3 stated | §9 |
+ **Adopted** then; **superseded on 2026-09-26**: the slice calendar and its campaign fallback were removed with the slices; §9 now carries the dependency-ordered calendar of [60 §7] (release gate at ≈ 28–56 weeks with two lanes, P50 ≈ 39) | §9 |

E53 · Review log, new closing entry · L1556
- (§4.2, §4.3, §4.4, §5a.1, §6.5).
+ (§4.2, §4.3, §4.4, §5a.1, §6.5).
+
+ **Owner decisions of 2026-09-26 — no third-party database; built once, in dependency order.** The owner decided (verbatim translations): "No, we do not use SQLite — we build our own [engine] right away." and "'So that you can start using it as early as possible' is not important — it must be done properly right away." This withdrew decision #2's former default (the slice plan S0–S6: S1–S5 on a throw-away SQLite backend behind an engine trait, the engine swapped in at S6) and then an adoption-driven engine-first draft (a serialized-access stage used as a tracker under a node cap, the lock-free protocol later). The plan of record is now [60] issue 2, revised after the adversarial review [61] of its first issue: no third-party database at any stage; every component built once to its final specification, in dependency order M0–M11, each passing its full gates before anything builds on it and extending certified milestones only through extension points certified earlier; the on-disk format frozen at M0 with every R1/R3 reservation, the R4/R5 reservations of [40]/[50] (a slot in §4.6), the `Vfs` fault model and store parameters for every threshold; the evidence that can change the format (the query accuracy benchmark, R4's replay corpora, the T1/T2 trigger quantities, the gate-0 carrier table) gathered before the freeze; the complete Rust reference model, written first, as the differential oracle, with its own query parser and canonical-form encoder and owner-signed rule tables; the git CLI as the independent image oracle; an OS-crash loop and mutation testing among the gates; a measurement protocol frozen at M0; the owner's workflow starts using moirai only at the release gate, which includes a format-upgrade drill. Scope consequences: branch promotion, `op log`/`op restore`, the recursive virtual merge base (amending I31′, because the newest-LCA rule can take one side's criss-cross resolution silently; a key whose virtual-base value is a conflict value is clean when both sides agree), FTS tier 2, schema strengthening, `--with-oplog`, SHA-256 images, bundles and an in-process git object layer (replacing D10's two-backend plan and the `check`/`stale` spawn of CM7) are in the release; the leader is built only if the M0 measurements require it; the `shared` field class and decisions #5, #8 and #17 are due before M0. R4 and R5 appear as marked slots (the binding inputs, §1 rows 16–17, §2.11, §4.6, §8.1, §9, §12) that point to [40] and [50]; their integration is a later step, required before M0 exits. The finding texts above, and the CM7, CL2 and CL7 dispositions, record the positions before this amendment. Edited: header, sources, how to read, §0, §1, §2.1, §2.2, §2.3, §2.7–§2.11, §2.13, §2.14, §2.15, §2.17, §3.5, §4.1, §4.4, §4.6, §4.8, §4.10, §5a.3, §5a.5, §5a.7, §5a.10, §5b.2, §5b.4–§5b.9, §5c, §6.1, §6.3, §6.5, §7.1, §7.4–§7.6, §8.1, §8.2, §9, §10, §11 (introduction, #1–#3, #5, #11, #15, #17), §12 and this log's pointers. T4–T6, T12, T14's discovery rules and T16 are unchanged.
```

### 9.2 Consequential edits from this roadmap's scope decisions

These follow from §1.3 (v1.1 items resolved into the release), §2.4 (the in-process git object layer), §3.4 (the recursive virtual base), §3.14 (the timing of the `shared` field class and the T1/T2 triggers) and the leader's `M6` label, which would collide with milestone M6.

```text
E54 · §0 bullet 2 · L44
- an opportunistic leader is a later optimisation (M6) and never required for correctness.
+ an opportunistic leader is never required for correctness and is built (in M1) only if the M0 measurements require it (§2.2).

E55 · §0 bullet 6 · L48
- The only git spawns are inside the explicit verbs `check`/`stale` (one cached `merge-base` per pair the commit-graph cannot answer, v1, CM7) and the image module.
+ `check`/`stale` answer ancestry in-process (the `ANCESTRY` cache, the commit-graph, else a generation-pruned commit walk through the git object layer, CM7); the only `git` spawns left are explicit and optional — network transport of the image, ref updates in a reftable destination, `doctor lanes --refresh-graph` — and only when git is present.

E56 · §0 bullet 7 · L49
- Export and import are hand-written at the semantic level; the git *object* writer/reader uses `git fast-import`/`git cat-file` when git is present, with a hand-written pack layer as a later "no git installed" mode.
+ Export and import are hand-written end to end: the semantic layer runs on moirai's own in-process git object layer (loose objects, packs, idx, commit-graph, refs, bundles; SHA-1 and SHA-256), so exporting and importing never need `git`; the git CLI is an independent test oracle and, when present, the network transport.

E57 · §0 "The biggest risks" (2) · L56
- and an explicit fallback (`shared` field class) after one campaign
+ and the `shared` field class as an owner decision taken before M0 (it changes view construction and the schema's field classes)

E58 · §2.1 T1 Revisit trigger · L93
- **Revisit trigger.** M1 measurement at the owner
+ **Revisit trigger.** M0 layout-probe measurement (§8.2 item 14, confirmed at M1 exit) at the owner

E59 · §2.2 T2 Decision · L97
- is milestone M6 and may never be built.
+ is built only if the M0 measurements (G8 Defender close cost; the 16-writer wait under G1) require it, and then in M1 together with the protocol it changes; it is never required for correctness.

E60 · §2.2 T2 Revisit trigger · L103
- (then bring the leader forward and make the CLI forward to it)
+ (decided by that measurement before M1, with the CLI forwarding to the leader if it is built; a change after M1 re-opens M1 and re-runs its gates)

E61 · §2.3 T3′ Decision · L107
- and is deferred to v1.1 because the owner's lanes are ~1–3k ops [D §5a.3].
+ and is part of the release (physical promotion in M1, policy in M3); below its thresholds, which are store parameters, the G15 index bounds the first-read cost [D §5a.3].

E62 · §2.3 T3′ Revisit trigger · L113
- [D §2 T3′] — never a return to planes.
+ [D §2 T3′], an owner decision due before M0 (§11 #1; chosen after the release it is a format-version-2 change carried out with the upgrade procedure drilled at the release gate) — never a return to planes.

E63 · §2.7 T7 Decision · L147
- Multiple LCAs: exactly one rule — the newest by generation number (ties by commit id) (N7, I31′);
+ Multiple LCAs: exactly one rule — the recursive virtual base: the LCAs are merged pairwise in generation order (ties by commit id) by these same typed rules and the result is the base; a key whose virtual-base value is a conflict value is clean when both sides hold the same value and conflicts whenever they differ (N7, I31′ as amended by [60 §3.4]);

E64 · §2.7 T7 Rejected · L151
- *Recursive virtual base for criss-cross in v1*: unnecessary on the daily path once merges into `main` are sync-first; scheduled for v1.1 if cross-lane merges show spurious conflicts.
+ *The newest-LCA base as the only criss-cross rule* (this section's earlier position): when two LCAs disagree on a key and the two sides resolved it differently, it takes one side's resolution without a conflict (the hazard of choosing one base among several LCAs, which git's default strategies avoid with a virtual base); criss-cross stays reachable through cross-lane and branch-of-branch merges even though merges into `main` are sync-first.

E65 · §2.8 T8 Decision · L157
- No group commit in v1; pipelined group commit only in the M6 leader (G13).
+ No group commit; pipelined group commit only in the optional leader (G13, §2.2).

E66 · §2.9 T9 Decision · L167
- optional `mcp_tool` PostToolBatch delta later.
+ optional `mcp_tool` PostToolBatch delta (in the release, off by default).

E67 · §2.10 T10 Decision, git object I/O · L177
- **Git object I/O**: v1 uses `git fast-import` (writer) and `git cat-file --batch`/`rev-list`/`diff-tree` (reader) behind an `ImageBackend` trait when git is present; a hand-written loose/pack/idx/bundle layer (with `zlib-rs` or `miniz_oxide`, measured in M0) is v1.1 for "no git installed" export/import (D10; owner decision #3).
+ **Git object I/O**: one hand-written, in-process git object layer (M4): loose objects, pack v2 + idx v2 (reading OFS/REF deltas, writing packs), commit-graph incl. split chains, loose refs and `packed-refs` with git's `.lock` protocol, reftable reading, bundles, SHA-1 and SHA-256, deflate through `zlib-rs` or `miniz_oxide` (chosen by the M0 measurement). It serves the image (R3), `check`/`stale` ancestry (CM7) and R4's tree gate and git evidence. There is no `ImageBackend` trait and no second backend; the git CLI is used only as an independent test oracle and, when present, for network transport ([60 §2.4]; owner decision #3).

E68 · §2.10 T10 Chosen from · L179
- **Chosen from.** [D §2 T10] minus the hand-written git object layer in v1; [22 §5.3, §8 T10].
+ **Chosen from.** [D §2 T10] including its hand-written git object layer ([60] reverses the D10 deferral); [22 §5.3, §8 T10].

E69 · §2.10 T10 Rejected · L181
- *Hand-written pack layer in v1* [D §5b.8]: 4–6k lines plus fuzzing for a capability R2 does not require of the image and the owner's machine (Git for Windows present) does not need yet [22 §3.4 D10].
+ *`git fast-import`/`cat-file` as the object backend* [22 §3.4 D10]: with no interim stages it would be the final design, making git a runtime dependency of R3, keeping a 74 ms [M] spawn per uncached pair in `check`/`stale` and leaving R4's tree gate without an ancestry source on the daily path [41 B2]; the in-process layer (4–6k lines plus fuzzing) is built once instead (M4).

E70 · §2.11 T11 Decision · L187
- per delta segment above ~20k nodes (v1.1);
+ per delta segment above ~20k nodes (a store parameter; in the release, M2: the owner's three-year scale of 0.3–0.5 M nodes needs it);

E71 · §2.14 ancestry for `check`/`stale` · L221
- else, in v1, **one `git merge-base --is-ancestor` spawn per uncached pair** (≈ 74 ms [M, 08 §2]) through the image module's process runner, the answer cached as a lazy fact (C's cache, G27) so every later process and every pack shares it.
+ else a generation-pruned commit walk over the repository's object store through the git object layer (in-process, no spawn; M4), the answer cached as a lazy fact (C's cache, G27) so every later process and every pack shares it.

E72 · §2.14 · L221
- v1.1 replaces the spawn with the image module's commits-only pack reader; `doctor lanes --refresh-graph` runs `git commit-graph write --reachable` explicitly (never automatically, refused in quiet mode).
+ `doctor lanes --refresh-graph` may still run `git commit-graph write --reachable` explicitly when git is present, as an accelerator only (never automatically, refused in quiet mode).

E73 · §2.14 Rejected · L225
- hence confined to the explicit `check`/`stale` verbs, cached, and replaced by the pack reader in v1.1.
+ hence no spawn at all: ancestry is answered in-process by the git object layer, and even that walk stays inside the explicit `check`/`stale` verbs and is cached.

E74 · §2.14 Rejected · L225
- *`check`/`stale` with no ancestry source for fresh commits in v1* (the synthesis's position, which silently depended on the v1.1 pack reader): pins would never be verified on the daily path (CM7).
+ *`check`/`stale` with no ancestry source for fresh commits* (the synthesis's position, which silently depended on a later pack reader): pins would never be verified on the daily path (CM7). *A `git merge-base` spawn as the permanent fallback* (this section's previous position): ~74 ms [M] per uncached pair where the in-process layer answers in milliseconds ([60 §1.3]).

E75 · §2.15 T15 Decision · L231
- Writer/reader: `git fast-import` / `git cat-file --batch` in v1 behind a trait; hand-written loose/pack layer later (D10). Hygiene when a hand-written writer exists:
+ Writer/reader: the in-process git object layer (M4; [60] reverses D10's two-backend plan). Hygiene:

E76 · §2.17 F-B5 · L267
- Adopted: `watch` leader-only (M6); no polling process ever (§6).
+ Adopted: `watch` exists only with the optional leader (§2.2); no polling process ever (§6).

E77 · §2.17 F-B6 · L268
- else (v1) one cached `merge-base` spawn per pair inside those explicit verbs, the pack reader in v1.1 (§2.14, §5c; CM7).
+ else an in-process commit walk through the git object layer inside those explicit verbs, cached (§2.14, §5c; CM7; [60]).

E78 · §2.17 F-C5 · L274
- | Adopted G13 for M6 (§6). |
+ | Adopted G13 for the optional leader (§2.2, §6). |

E79 · §2.17 F-C6 · L275
- Adopted G13: pipelined, no accumulation timer (M6).
+ Adopted G13: pipelined, no accumulation timer (optional leader, §2.2).

E80 · §2.17 F-D6 · L283
- Adopted for the hand-written writer (v1.1); in v1 `git fast-import`/`update-ref` implement git's own protocol (§5b.6).
+ Adopted in the git object layer (M4; §5b.6).

E81 · §2.17 F-D7 · L284
- Adopted: packs by default (fast-import produces packs), batched runs,
+ Adopted: packs by default above the M0 loose/pack threshold, batched runs,

E82 · §2.17 F-D13 · L290
- in v1 a cached `merge-base` spawn inside `check`/`stale` only, the pack reader in v1.1 (§2.14; CM7).
+ otherwise the git object layer's in-process commit walk inside `check`/`stale` only, cached (§2.14; CM7).

E83 · §2.17 N7 · L322
- Adopted: one rule (newest-by-gen LCA, I31′) plus sync-first merges into `main` (§5a.7). Recursive virtual base deferred to v1.1.
+ Adopted: one rule (the recursive virtual base, I31′ as amended by [60 §3.4]) plus sync-first merges into `main` (§5a.7).

E84 · §2.17 D7 · L351
- Adopted in part: `rebase --onto`, `op restore`, promotion, bundles, SHA-256, `--with-oplog`, extra destinations deferred;
+ Adopted in part: `rebase --onto` and the extra destinations are not built (scope decisions, [60 §1.3]); `op restore`, promotion, bundles, SHA-256 and `--with-oplog` are in the release ([60]);

E85 · §2.17 D10 · L353
- Adopted: fast-import/cat-file first behind a trait (owner decision #3).
+ Superseded by [60]: one in-process git object layer (M4); the git CLI is a test oracle and the network transport only (owner decision #3).

E86 · §3.5 `stale` row · L494
- else (v1) one cached `git merge-base` spawn per pair inside these two verbs (§2.14, CM7)
+ else the git object layer's in-process commit walk inside these two verbs, cached (§2.14, CM7)

E87 · §4.1 `LOCK` row · L523
- lock bytes 0 writer, 1 leader (M6), 2 maintenance,
+ lock bytes 0 writer, 1 leader (used only if the optional leader is built, §2.2), 2 maintenance,

E88 · §4.1 `seg.b` row · L528
- | `seg.b<refsym>.K` | v1.1 |
+ | `seg.b<refsym>.K` | one set per promoted branch (M1) |

E89 · §4.4 `TERMS` row · L627
- tier-2 FTS (v1.1, ≥ 20k nodes;
+ tier-2 FTS (≥ 20k nodes, a store parameter;

E90 · §4.8 text index row · L684
- per delta segment (v1.1) | segments |
+ per delta segment | segments |

E91 · §4.10 (the repair path named in §9 and §10) · L698
- `doctor --fsck` verifies column checksums and BLAKE3 footers.
+ `doctor --fsck` verifies column checksums and BLAKE3 footers. `repair --rebuild-from-log` rebuilds every segment, blob and `hist` file from the log (all are derived) and re-rolls the epoch; it is the recovery path for a segment-writer defect (M1, [60 §3.2]).

E92 · §5a.3 Promotion · L746
- - **Promotion** (v1.1): when
+ - **Promotion** (physical in M1, policy in M3; the thresholds below are store parameters): when

E93 · §5a.3 Promotion · L746
- Until promotion exists, the G15 index alone bounds cost for the owner's 1–3k-op lanes.
+ Below the thresholds, the G15 index alone bounds the cost.

E94 · §5a.5 table · L765
- | `op log` / `op restore <seq>` (v1.1) |
+ | `op log` / `op restore <seq>` |

E95 · §5a.5 table · L769
- | `rebase --onto` | **deferred** (history rewriting; the only verb that would change lane commit ids in the image) |
+ | `rebase --onto` | **not built** (a scope decision, [60 §1.3]: history rewriting; the only verb that would change lane commit ids in the image) |

E96 · §5a.7 step 1 · L784
- Multiple LCAs → **the newest by generation number, ties by lowest commit id** (I31′; git's `resolve` strategy, deterministic, may produce conflicts on keys changed between the LCAs — property-tested, recursive virtual base scheduled for v1.1 if cross-lane merges show it).
+ Multiple LCAs → **the recursive virtual base** (I31′ as amended by [60 §3.4]): the LCAs are merged pairwise in generation order (ties by lowest commit id) by these same typed rules and the result is the base; a key whose virtual-base value is a conflict value is clean when both sides hold the same value and conflicts whenever they differ, so two sides that resolved a criss-cross differently always conflict (a single chosen LCA could take one of them silently) and two sides that resolved it identically never do; property-tested.

E97 · §5a.10 table · L837
- | promotion (v1.1, 8k ops) |
+ | promotion (8k ops) |

E98 · §5b.4 side refs · L947
- (`--with-oplog`, deferred)
+ (`--with-oplog`, in the release)

E99 · §5b.6 object I/O · L962
¶ **Object I/O backends** (`ImageBackend` trait).
+ **Object I/O.** One in-process git object layer (M4, [60]) reads and writes loose objects, pack v2 + idx v2 (reading OFS/REF deltas), commit-graph files incl. split chains, loose refs and `packed-refs` through git's `<ref>.lock` protocol with read-after-write verification (G21), and bundles, in SHA-1 and SHA-256; reftable repositories are read, and ref updates in a reftable destination are delegated to `git update-ref --stdin` when git is present and refused otherwise (G22). Export writes one pack per run above the M0 loose/pack threshold (G23). There is no backend trait and no second implementation: the git CLI (`git fsck`, `cat-file`, `rev-list`, `verify-pack`, `merge-base`) is the layer's independent test oracle, and `git` is spawned for network transport only (`image push/pull`), printing the exact command when git is absent. The semantic layers (canonical `.moi` codec, tree diffing at the path level, trailers, verification, validation, `gitmap`, staging) are hand-written on top.

E100 · §5b.6 export step 4 · L969
- Update refs through the backend (fast-import's own ref update; or `git update-ref --stdin`; in a hand-written backend git's `<ref>.lock` protocol with read-after-write verification, G21)
+ Update refs through the git object layer (git's `<ref>.lock` protocol with read-after-write verification, G21; for a reftable destination `git update-ref --stdin` when git is present, otherwise refused, G22)

E101 · §5b.6 export step 5 · L970
- objects and refs are written by the backend before
+ objects and refs are written by the git object layer before

E102 · §5b.6 import step 1 · L975
- Read refs and objects through the backend;
+ Read refs and objects through the git object layer;

E103 · §5b.7 table · L994
- | only with `--with-oplog` (deferred) |
+ | only with `--with-oplog` |

E104 · §5b.7 gate 1 · L1001
- for 1e5 nodes and 1e5 commits (SHA-1; SHA-256 when supported)
+ for 1e5 nodes and 1e5 commits (SHA-1 and SHA-256)

E105 · §5b.8 destinations table · L1009
- orphan branch `moirai/image` (deferred)
+ orphan branch `moirai/image` (not built, [60 §1.3])

E106 · §5b.8 destinations table · L1010
- tracked directory `docs/moirai/` (deferred)
+ tracked directory `docs/moirai/` (not built, [60 §1.3])

E107 · §5b.8 reftable row · L1025
¶ | reftable repository |
+ | reftable repository | the git object layer reads reftable; ref updates in a reftable destination are delegated to `git update-ref --stdin` when git is present and refused otherwise (G22); `image export --create` writes `refStorage = files` |

E108 · §5b.9 heading · L1029
- fast-import zlib on one core;
+ in-process deflate on one core;

E109 · §5b.9 table · L1035
- | full export time (fast-import, pack) |
+ | full export time (one pack) |

E110 · §5b.9 table · L1037
¶ | incremental, one commit, 3 nodes |
+ | incremental, one commit, 3 nodes | ~32 KB raw / ~17 KB packed; one pack + idx written in-process with temp + rename and one ref update ≈ 5–20 ms (est.; Defender cost measured in M0 item 8) | same | same |

E111 · §5b.9 table · L1041
- | incremental import, one commit | 5–20 ms + the git spawn |
+ | incremental import, one commit | 5–20 ms |

E112 · §5c first paragraph · L1048
¶ **Runs with no `git` binary, no git library and outside any repository**:
+ **Runs with no `git` binary, no git library and outside any repository**: `init`, every read and write verb, branches, merge, sync, tags, revert, cherry-pick, undo, reflog, GC, doctor, hooks, the MCP server, packs and briefs, `image export`/`image import` to and from a local directory or bundle, and `check`/`stale` (ancestry in-process through the git object layer, CM7). Provenance fields are empty when no `.git` is discoverable; `stale` degrades to "unknown" and says so.

E113 · §5c second paragraph · L1050
¶ **Reads git *files* textually, never spawns**:
+ **Reads git *files* in-process, never spawns**: the discovery hint (`.git` file/dir → `commondir`), provenance (`HEAD`, `refs/heads/*`, `packed-refs`, ~60 lines read textually, or reftable through the git object layer), and ancestry for `stale`/`check` from commit-graph generation numbers when the graph contains both commits [04 §3.1], else a generation-pruned commit walk through the git object layer — cached as lazy facts in the log so every process shares the answers (C's cache).

E114 · §5c third paragraph · L1052
¶ **Spawns `git` only when present and only in three explicit places**:
+ **Spawns `git` only when present and only in three explicit, optional places**: network transport of the image (`image push/pull`), printing the exact command otherwise; ref updates in a reftable image destination (G22); and `doctor lanes --refresh-graph`. None is on a pack, brief, ready, blocking, get, claim, check, stale or hook path, and none is in the core crate.

E115 · §6.1 table, MCP row · L1107
- leader byte only in M6 |
+ leader byte only if the optional leader is built (§2.2) |

E116 · §6.1 table, leader row · L1108
- | leader (M6, optional) |
+ | leader (optional; built only if the M0 measurements require it, §2.2) |

E117 · §6.3 · L1122
- and, in M6, pushed by the leader to followers and to `watch`
+ and, only with the optional leader (§2.2), pushed by it to followers and to `watch`

E118 · §6.5 · L1135
- Group commit only in the M6 leader, pipelined, no accumulation timer.
+ Group commit only in the optional leader (§2.2), pipelined, no accumulation timer.

E119 · §7.1 CLI block · L1192
- moirai restore DIR --into EMPTY_DIR
+ moirai restore DIR --into EMPTY_DIR      moirai repair --rebuild-from-log

E120 · §7.1 CLI block · L1192
- # transaction-consistent copy of the store; restore re-rolls the epoch (G25)
+ # backup: transaction-consistent copy of the store; restore and repair re-roll the epoch (G25)

E121 · §7.1 example output · L1252
- via git fast-import (0.47 s)
+ via the in-process pack writer (0.41 s)

E122 · §7.5 hooks table · L1332
- | `PostToolBatch` (`mcp_tool`, optional, v1.1) |
+ | `PostToolBatch` (`mcp_tool`, optional, off by default) |

E123 · §7.6 step 7 · L1347
- (the commit-graph lacks a commit made minutes ago, so v1 spawns one `git merge-base --is-ancestor` — ~74 ms — and caches the fact; every later pack shows it without a spawn, CM7)
+ (the commit-graph lacks a commit made minutes ago, so `check` walks the new commits in-process through the git object layer — milliseconds, no spawn — and caches the fact; every later pack shows it, CM7)

E124 · §7.6 step 9 · L1349
- via fast-import, 0.5 s
+ through the git object layer, ~0.5 s

E125 · §8.1 leader row · L1372
- | Private RSS, leader (M6) |
+ | Private RSS, leader (optional, §2.2) |

E126 · §8.1 `--across` row · L1379
- promoted branches use `TOUCH` bitmaps (v1.1)
+ promoted branches use `TOUCH` bitmaps

E127 · §8.1 promotion row · L1390
- | Promotion (v1.1) |
+ | Promotion |

E128 · §8.1 image-export row · L1391
¶ | `image export`, one checkpoint (3 nodes changed) |
+ | `image export`, one checkpoint (3 nodes changed) | 5–20 ms (est.; Defender cost from M0 item 8) | same | same | in-process pack + idx + ref update; no spawn |

E129 · §8.1 `check`/`stale` row · L1394
¶ | `check #id` / `stale` (explicit verbs) |
+ | `check #id` / `stale` (explicit verbs) | ≤ 1 ms when the fact is cached or the commit-graph holds both commits; otherwise an in-process generation-pruned commit walk, ≤ 5 ms per pair at the owner's repository size (est., gated in M4) | same | same | never on the pack/brief/ready path; no spawn (CM7) |

E130 · §8.2 item 8 · L1417
- (decides the hand-written writer's loose/pack threshold and the `image gc` guidance, G28)
+ (decides the git object layer's loose/pack threshold and the `image gc` guidance, G28)

E131 · §8.2 item 9 · L1418
- (only matters for the hand-written writer, U33)
+ (chooses the git object layer's codec, U33)

E132 · §8.2 fuzzers · L1429
- pathological escapes), pack reader (v1.1).
+ pathological escapes), R4's anchor selectors and path specs (M0), the git object layer's loose/pack/idx/commit-graph/refs/bundle readers (M4), the query grammar (M7).

E133 · §10 row 2 Mitigation · L1459
- the `shared` field class as an explicit fallback after one campaign
+ the `shared` field class, decided by the owner before M0

E134 · §11 row 3 · L1482
¶ | 3 | **Git object I/O for the image.**
+ | 3 | **Git object I/O for the image, `check`/`stale` and R4.** One in-process, hand-written git object layer (loose objects, packs, idx, commit-graph, refs, bundles; SHA-1 and SHA-256; M4), or `git fast-import`/`cat-file` as the permanent backend? May `image push/pull` spawn `git`? Due before M4. | in-process layer; fast-import permanently; `gix` | in-process layer ([60] replaces the earlier default "fast-import first, hand-written later", which is a throwaway stage); the git CLI only as a test oracle; spawning `git` for network transport allowed with a printed fallback | fast-import permanently makes git a runtime dependency of R3, keeps spawns in `check`/`stale` (74 ms per pair) and leaves R4's tree gate without an ancestry source on the daily path [41 B2]; `gix` adds a large dependency with open SHA-256/reftable work [21 §5 item 12] |

E135 · §11 row 5 question · L1484
- Which R1 "ideally" verbs are v1?
+ Which R1 "ideally" verbs are in the release? Due before M0.

E136 · §11 row 5 default · L1484
- per lane; v1 = `tag`, `undo`, `revert`, `cherry-pick`, `reflog`; `rebase`, `op restore` later
+ per lane; the release has `tag`, `undo`, `revert`, `cherry-pick`, `reflog` and `op log`/`op restore`; `rebase --onto` is not built ([60 §1.3])

E137 · §11 row 11 question · L1490
- **Cross-machine sync / cloud agents writing the same graph in v1?**
+ **Cross-machine sync / cloud agents writing the same graph in the release?** Due before M0.

E138 · §11 row 11 default · L1490
- so v1.1 sync needs no migration
+ so a later cross-machine sync needs no migration of identities

E139 · §11 row 17 · L1496
- cross-project views in v1.1 through image import into a read-only aggregate store
+ cross-project views are not in the release (they would come through image import into a read-only aggregate store); due before M0, because the store layout, the `#N` space and the writer lock's scope are frozen there
```

### 9.3 R4 and R5 as marked slots

These place R4 and R5 in [AR] only as pointers to [40] and [50] (P9). They state no shape of their own; [40]'s and [50]'s edit lists replace them when those designs are integrated, which M0 requires before it exits (§3.1). *Replaced on 2026-09-26: the integration step applied [40 §8.4] and [50 §10] to [AR] (new §5e and §7.7, the §4.6 reservation table, and the sections they touch), superseding E140–E144; see [AR]'s Review log and §10.4.*

```text
E140 · binding inputs · L33
- [B §3.1] are no longer acceptable branch models.
+ [B §3.1] are no longer acceptable branch models. On 2026-09-26 the owner added **R4** (links from nodes to project files, and to places inside them, that survive moves, renames, edits and deletions) and **R5** (moirai's own query language). Their designs are [40] (reviewed in [41]) and [50]; §9 schedules them; their integration into §3–§8 is a later step, and until then §1 rows 16–17, §2.11, the reservation slot at the end of §4.6, §8.1 and §12 point to them.

E141 · §1 new rows 16–17 · L78
- | [01 §7], [01 §8.3], [02 §11–§12], [07 §8]. |
+ | [01 §7], [01 §8.3], [02 §11–§12], [07 §8]. |
+ | 16 | **Move-proof links from nodes to project files and places in them (R4)** | *Slot — integrated in a later step.* Designed in [40]: explicit file verbs plus lazy, deterministic re-binding on exact evidence; link intent versioned per branch, resolution per tree; no watcher, zero idle CPU. Reviewed in [41]. Every format field it needs is reserved in format v1 at M0 (§4.6 slot); built in M0 (pure libraries), M2 (graph layer), M3 (merge rules), M5 (image carrier), M6 (runtime) and M7–M10 (built-ins, verbs, packs, hooks, MCP) (§9). | [40], [41] |
+ | 17 | **Own query language (R5)** | *Slot — integrated in a later step.* Designed in [50]: LQ, a GQL-shaped, Cypher-tolerant read language plus guarded `TX` writes, with the engine's derived state as built-ins, revision selectors, counted budgets, and the CLI verbs as named queries. Its surface is frozen at M0 by an accuracy benchmark run on the reference model; built in M7, reaching agents through M8–M10 (§9). | [50] |

E142 · §2.11 T11 Rejected · L191
- *Text2Cypher / a query language*: ~50 % execution accuracy for GPT-4 [06 §12.2].
+ *Text2Cypher / a query language*: ~50 % execution accuracy for GPT-4 [06 §12.2]. *Superseded by owner requirement R5 (2026-09-26):* a query language is part of the release (§9 M7); its design [50] replaces this rejection when it is integrated (slot).

E143 · §4.6 format reservations for R4 and R5 (new paragraph at the end of §4.6) · L663
- and it is checked for every commit kind before any round-trip corpus runs (§5b.7 gate 0).
+ and it is checked for every commit kind before any round-trip corpus runs (§5b.7 gate 0).
+
+ **Reserved in format v1 for R4 and R5 — slot, integrated in a later step.** The on-disk format is frozen at M0 exit ([60 §2.5]) and includes, before the first byte is written, every field the R4 and R5 designs need: the reservations R-1…R-14 of [40 §2.11] (value types, the file-node field set and statuses, identity derivation, the anchor-carrying `at` edge, path-prefix history, `HEAD.next_anchor`, runtime record kinds, segment sections, the fingerprint blob class, the anchor key class of item 10, the `.moi` forms, invariants I-F1…I-F11, config keys and the resolver constants), as revised in response to [41] — whose blockers B1 and B4 change the identity derivation and whether path-prefix moves become a hashed canonical item 11 with its own trailer or versioned state — and the reservations F1–F15 of [50 §8.1] (schema columns for the query language, the `QUERIES` table, provenance and statistics sections, the `CONFLICTS` section, frame and commit-header timestamps, FTS document lengths, and the split of §3.5's `ready` into a persisted structural predicate and read-time runtime clauses). This document states no shape of its own for any of them: where the canonical form above, §4.2–§4.4 or §5b.4 gain a field, [40] and [50] are authoritative, and their text replaces this paragraph when they are integrated.

E144 · §12 query-language row · L1515
- | A query language (Cypher/GQL/Datalog) in v1 |
+ | A query language (Cypher/GQL/Datalog) in v1 — *superseded by owner requirement R5 (2026-09-26): the release includes the query language of [50] (§9 M7); this row is replaced when [50] is integrated (slot)* |
```

### 9.4 Checks after applying

All 144 edits (158 operations) were applied in order to [AR] by script; every anchor occurred exactly once both in the original and at the moment it was applied. On the result:

1. `grep -n "SQLite"` returns only design citations and the exclusion list kept by §9.5, plus the new texts that *forbid* SQLite or record its rejection (§0 bullet 10; the owner's quoted decision in §2.10, §2.13 and §11 #2; §2.13 Rejected; §8.2's reference-model paragraph; §9; §12; the Review-log entry) — nothing that uses it.
2. `grep -n -i "oracle"` returns only the reference model as the differential oracle, the git CLI, `gix` and the `ProjectFs` simulator's ground truth as test oracles, the format oracle, the statements that forbid a third-party database as an oracle (§1 row 1, §2.10, §8.2, §9, §12), §10 row 16's note that no third-party oracle exists, and rejected or superseded positions (§2.13 Rejected, the [22] §2.7/D8 disposition).
3. `grep -n -E "\bS[0-6]\b"` returns [21]'s finding labels (`S1`, `S2-B`, `S6 / N14`), [D §12 S1]/[D §12 S4] source labels, the rejected slice plan named in §2.13 Rejected, §11 #2 and the Review-log entry, and the CB1 and CL9 finding texts.
4. `grep -n -i "adopt"` returns the ledger vocabulary ("Adopted"/"adopted" dispositions in §2.17 and the Review log), design lineage ("as adopted by [D §4]"), the recovery protocol's adoption of a flushed record (§2.1, §2.17 F-A1 and N3, §4.3, §4.5, §4.10, and the new fault-model texts of §8.2, §9 and §10 row 17 that test it), and the owner decisions' own vocabulary — "no milestone ordered by time to adoption", "no early-adoption stages", the rejected adoption-driven draft (§2.13, §10 row 14, §11 #2, the Review-log entry). None schedules workflow adoption before the release gate.
5. `grep -n "v1\.1"` returns the historical Review-log rows (CM7, CL2, CL7) and the new §12 row that names "v1.1" deferrals as an anti-requirement.
6. `grep -n "\bM6\b"` returns only milestone M6 (the file-link runtime).
7. `grep -n -E "fast-import|ImageBackend|throw-away|engine trait|oracle backend|node cap"` returns only negations ("no `ImageBackend` trait", "no node caps"), rejected or superseded positions (§2.10 Rejected, §2.13, the [22] §2.7/D8 disposition, §11 #2 and #3, the Review-log entry) and the anti-requirements of §12.

### 9.5 Matches that must NOT be changed

| Where | Match | Why it stays |
|---|---|---|
| how to read (conventions) | `the SQLite session extension` | a citation of a precedent |
| §0 risks (1) | `SQLite's WAL-reset race hid for 16 years and fell to deterministic simulation in minutes [08 §3.1]` | a citation (the lesson behind the simulation gates) |
| §2.10 Decision | `Excluded from the product: SQLite, redb, LMDB/heed, fjall, sled, …` | this *is* the exclusion |
| §4.3 | `(SQLite-changeset style [04 §3.13])` | a design citation |
| §5a.7 step 8 | `(the SQLite rebaser idea [04 §3.13])` | a design citation |
| §8.2 simulation bullet | `SQLite's WAL-reset race fell to this method in ~15 minutes [08 §3.1]` | a citation |
| §10 row 1 Evidence | `SQLite WAL-reset race 16 years [08 §3.1]` | a citation; only Mitigation and Signal change |
| §1 rows 5, 16; §2.15; §5b.5; §5b.8 | `[D §12 S1 …]`, `[D §12 S4]`, `[D §12 S8–S9]` | proposal D's source labels, not slices |
| §2.7, §2.17, §3.2 | `(S12)`, `\| S1 \|`, `\| S2-B \|`, `\| S6 / N14 \|`, `S10`, `(S2-B)` | [21]'s finding labels |
| §2.2, §2.17 D8 | `[C M3]`; "M1 overloaded" in the [22] §2.7 finding text | other documents' milestones (C's M3, D's M1) |
| §2.1, §2.3, §2.6, §2.17, §7.1 example | `M0`, `M1`, `M3` in their legacy sense | they keep their meaning under the new numbering (M0 measurements, M1 storage engine, M3 merge tests) |
| §7.6 | `` `.slice()` `` | JavaScript, from the owner's scripts |
| §2.10 Rejected, §8.2 | `gix` as an optional import/read **oracle in tests** | `gix` is a git library, not a database; the git CLI is the primary oracle |
| §2.17 F-D13 | `ancestry through the pack reader` (finding text) | the finding as raised; its disposition changes (E82) |
| §2.1, §2.17, §4.2, §4.3, §4.5, §4.10 | "adopts", "adoption of a complete record", "adopt-and-republish" | recovery adoption of a flushed record — a protocol term, not workflow adoption |
| §0, §2.1, §2.17, §2.14, Review log | "as adopted by [D §4]", "Adopted" dispositions | design lineage and ledger vocabulary, not workflow adoption |
| Review log | the finding texts of CB1, CM2, CM7, CL2, CL7, CL9 and the CM7, CL2, CL7 dispositions | historical findings; only "Where" cells and the CL9 disposition change (E51, E52) and the new entry records what changed (E53) |

### 9.6 Out of scope for this edit list (belongs to [40] and [50])

- The content of R4 and R5 in [AR]: the widened `artifact` kind and the `at` edge (§3.2–§3.3), the file verbs and `q`/`tx` (§7.1), the MCP changes (§7.2), the R4 merge rows (§5a.7), the `.moi` file-node and anchor forms (§5b.2), `ready`/`unblocked` (§3.5), canonical-AST idempotency (§6.4), the R4/R5 budget rows (§8.1), the R4/R5 risks and owner decisions (§10–§11), and the replacement of the slots of §9.3. [40]'s and [50]'s edit lists carry them; applying them is an **M0 exit criterion** (§3.1), not a release-gate item ([61 m-1]). *Applied on 2026-09-26 (§10.4).*

---

## 10. Review log

### 10.1 Issue 2 (2026-09-26): disposition of the review [61]

[61] reviewed issue 1 and returned **REVISE before adopting as the roadmap of record: 2 blockers, 9 majors, 10 minors**. It confirmed that issue 1 complied with both owner decisions to the letter (no third-party database anywhere, no interim stage, no milestone ordered by time to adoption) and that its edit list was mechanically sound. Every finding is dispositioned below: **A** accepted as proposed; **A\*** accepted with the modification stated; **R** rejected with a reason. No finding is rejected.

| Id | Severity | Finding (short) | Disposition | Where |
|---|---|---|---|---|
| B1 | blocker | R4 content contradicts [40], which already existed: item 11, trailers, value type, ~12 missing reservations, placement in one late milestone, size, exit criteria, model duties, defaults, `links import` | **A\*.** §2.5 carries [40 §2.11] R-1…R-14 verbatim, with a column for the changes [41 §5] requires before FL-0; issue 1's generic annotations item 11, its three trailers, its `oid` type and its `FsIntentDone`/`FsIntentAborted` kinds are withdrawn. R4 is dissolved into its hosts: FL-0/FL-1 in M0, FL-3 in M2, FL-7 in M3, FL-8 in M5, FL-2/FL-4/FL-10 in M6 (the file-link runtime), FL-6 in M7, FL-5 in M8, FL-9 in M9/M10. `ProjectFs` is a P1 seam; the FL-1 → C2/C3 edge is added. Size, exit criteria, model duties (the brute-force anchor search, P11) and owner-call defaults (the USN journal: use one wherever one exists) come from [40]; `links import` is in M9; rule P9 forbids placeholders that contradict an existing design. **Modification:** E79 is not rewritten from [40]'s present text but replaced by a slot (E143) that points to [40] and [41], because [41 B4] proposes removing the very item 11 and trailer that [40] currently freezes — copying [40]'s text into [AR] now would repeat the error one level down; the owner's brief also asks that R4/R5 stay marked slots until their integration step, which M0 requires before it exits. | §1.2 P1, P9; §1.3; §2.1–§2.5; §3.1, §3.3, §3.4, §3.6–§3.10; §4.2; §7; E141, E143 |
| B2 | blocker | no gate exercises an OS crash or power loss; the `Vfs` fault model is unspecified; adoption after a failed flush and an unflushed `HEAD` against GC can lose acknowledged commits | **A.** The fault model is part of the format specification (§2.5 rows "`Vfs` fault model" and "Protocol decisions"); GT1 enumerates bounded subsets of unflushed writes and failed-flush sequences; the M0 harness validation seeds both scenarios; the M0 specification review decides the protocol questions (adopted bytes re-written or verified, no silent truncation past valid records, a durable `HEAD` barrier before deletion, retirement or recycling, torn-slot fallback, monotonic clocks, disk full); GT3/GT4 gain pause, disk-full (VHDX) and clock-step variants and GT3 mixed-sector reads; new GT15 OS-crash loop (VirtualBox or VMware Workstation, host cache off, guest flushes honoured, NotMyFault or hard power-off, acknowledgements streamed to the host) mandatory from M1 with ≥ 1,000 cycles at M1 exit and ≥ 5,000 for RG3; measurement 17 calibrates the rig; RG3 states the drive-cache assumption. | §2.5; §3.1; §3.2; §3.13; §5.2; §6 RG3; §8 risks 7–8; E35–E37; E45 (row 17) |
| M-1 | major | M1 cannot be final while later milestones produce its section contents | **A.** A section-producer and record-fold registry, certified in M1 with generic producers for every reserved section and record kind; later milestones register real producers without touching M1 code and re-certify M1's rebuild, budgets and crash gates as their own exit criterion (P10). | §1.2 P10; §2.6; §3.2–§3.8 |
| M-2 | major | evidence that can change the frozen format arrives after the freeze | **A.** (a) GT13 (LQ-Bench) runs at M0 on the reference model with its own parser, and the surface freezes with the R5 reservations; (b) FL-1 is built in M0 and the replay corpora run before the freeze; (c) the T1/T2 trigger quantities join item 14 and T1's trigger is re-worded as an M0 decision (E58); (d) a gate-0 carrier table with a fixture per commit kind is part of M0; the image core moves to M5, right after M3 and M4 and before the query language, with its verbs in M8. | §2.4 items 4 and 6; §2.5; §3.1; §3.6; §5.2 |
| M-3 | major | owner decisions #1, #5, #8, #17 and the R4 calls are due after the milestones they shape | **A, extended.** P2 states the rule. Because the reference model carries every logical rule from M0, every decision that changes a logical rule, a frozen byte or the frozen query surface is due **before M0** — including the R4 calls that change the model or the format, which [61] had placed before M2. | §1.2 P2; §3.14 |
| M-4 | major | oracle independence and strength | **A.** The model has its own naive query parser and binder (the generator emits ASTs, a printer feeds the product, `parse(print(ast)) == ast` is a property) and its own canonical-form encoder (commit ids compared byte for byte from M1); the owner signs the rule tables at each exit that changes one and verifies the GT10 core fixtures; GT16 mutation testing at every exit (≥ 90 % in the semantic crates); protocol bugs seeded into the real engine at M1 exit. | §3.1; §3.2; §3.13; §4.2; §4.5 |
| M-5 | major | threshold-gated code is never compared with the model | **A.** Every threshold is a format-visible store parameter (extent size included); a tiny test profile; GT2/GT3 sweep the test profile and the production values. | §2.5; §3.2; §4.4 item 8 |
| M-6 | major | the measurement protocol is undefined | **A.** §5.1 freezes it at M0: the recorded and replayed 16-agent load fixture with ≈ 1.8 GB free; n ≥ 1,000 per p99 (≥ 10,000 below a millisecond); 5 repetitions gated on the median p99; floors re-measured interleaved; idle CPU as zero CPU time and zero context switches over 10 minutes; RSS as peak private bytes at exit; a measured noise band. | §5.1–§5.3; §5.2 item 16 |
| M-7 | major | test infrastructure and machine time are not provisioned | **A.** M0 delivers the repository, CI, a dedicated Windows 11 x64 test host, the OS-crash rig, hosted Windows Server runners as a secondary signal and a mutation-testing job; the owner's laptop is used for exit measurements only; §3.15 budgets the machine time. The test host is an owner decision due before M0; without it the gate volumes stay and the calendar lengthens. | §3.1 item 7; §3.14; §3.15; §8 risk 14 |
| M-8 | major | the estimates are low (≈ +75–100 units) and the rate is unmeasured | **A.** Recomputed to ≈ 318–422 units: R4 from [40 §8.1], R5 from [50 §8.2] (42–60 units in M7 alone, above [61]'s own 33–49), the model 16–20, M4 15–21, M0 55–74, M11 11–20 plus 14 nights and the soak on the release-candidate commit after the last engine change. P50/P90 published (§7.1 method; P90 is the planning figure); velocity is measured at the M0 and M1 exits and the calendar re-issued; the owner's hours are budgeted. | §3; §3.15; §7 |
| M-9 | major | format evolution after the release is untested | **A.** RG10 includes an upgrade drill to a synthetic format v2 — (i) a derived-file change through `repair --rebuild-from-log`, (ii) a log or canonical-form change through `image export --with-oplog` → `image import`, both state-identical against the model — and a rollback drill; P4 names the procedure. | §1.2 P4; §3.12; §6 RG10 |
| m-1 | minor | edit-list misses (7 residues, the §2.13 heading) and §9.5's "RG1 before M0" | **A.** [61]'s E121–E128 are this list's E66, E98, E136, E57, E133, E60, E58 and E12; the [40]/[50] integration of [AR] is an M0 exit criterion (§3.1, §9.6), not an RG1 item. | §9; §3.1 |
| m-2 | minor | `across ≤ 8 refs` against `--across` over all refs | **A.** `across` is a counted budget with a resumable cursor over refs, and promoted refs are read through their `TOUCH` bitmaps — one mechanism for M3's verb and M7's relation; [50] owns the wording. | §1.3; §3.4; §3.8 |
| m-3 | minor | the recursive-virtual-base wording makes equal resolutions conflict | **A.** A key whose virtual-base value is a conflict value is clean when both sides hold the same value and conflicts whenever they differ; the case is an I31′ property and a row of the model's table. | §3.4; §3.13 GT6; E63; E96 |
| m-4 | minor | the M0 "reference codec" is neither product code nor oracle | **A** (the second option). The M0 codec is the test-only, permanent format oracle; the product codec is written once, in M1, against hand-written hex fixtures and the oracle. | §3.1; §3.2; §4.1 |
| m-5 | minor | [05 §17] item 5 still lists SQLite as a baseline | **A.** A supersession line in §5.6. | §5.6 |
| m-6 | minor | GT4/RG3 mechanics | **A.** Acknowledgements are reported over a pipe (GT4) or a socket (GT15) before they are printed; GT4 runs at 1e4 with the model re-evaluating every read after a recovery and a 1-in-16 sample of the others, and a 1e5 variant checks presence and `doctor --verify`; RG3's nights, last OS-crash cycles and soak run on the release-candidate commit and restart on any engine change. | §3.13; §4.4; §6 RG3 |
| m-7 | minor | the `ProjectFs` seam and the FL-1 → M3 edge are missing | **A.** | §1.2 P1; §2.3 |
| m-8 | minor | the M3 kill loop runs before discovery and the CLI exist | **A.** The version-control test driver is permanent test infrastructure and simulates every input of the resolution chain. | §3.4 |
| m-9 | minor | open ≤ 3 ms with a full tail against [AR §4.7]'s 3–5 ms | **A.** The checkpoint threshold is chosen at M0 (item 10) so that the open budget holds with a full tail. | §3.1; §3.2; §5.2 |
| m-10 | minor | choosing `shared` after the release would be an unplanned format change | **A.** Decided before M0; a later choice is a format-v2 change carried out with the RG10 procedure. | §1.3; §3.14; E47; E62 |

### 10.2 Other changes in issue 2, not raised by [61]

- **[50] now exists.** Its reservations F1–F15, its package placement (LQ-0…LQ-14), LQ-Bench (replacing issue 1's 60-question experiment), the names `ready`/`unblocked`, its arena defaults (1 MiB, agent maxima 2 MiB CLI / 4 MiB MCP), its as-of limits (8k/16k ops), GT9's two-valued partition, exit code 10 and its size are taken from it (P9), as [50 §10] asked.
- **[41] now exists.** Its blockers and its §5 reservation changes are conditions of [40]'s revision before M0; its build-order corrections — the in-process object reader as a hard dependency of the resolver, the tree gate, freshness and write rules inside FL-4, no "first complete layer", no spawn fallback — are applied in M6.
- **Milestones renumbered by the dependency order**: M5 git image (was M7), M6 file-link runtime (was M8, now R4's runtime only), M7 query language (was M5), M8 CLI (was M6). The [AR] edit list was regenerated (144 edits, 158 operations) and applied.
- **The lane plan changed**: lane B carries the model, LQ-Bench and FL-1's second half in M0, then FL-2, the git object layer, the query front end, binder, planner and executor core, and the image.

### 10.4 Integration of R4 and R5 into [AR] (2026-09-26)

[40] revision 2 (answering [41]) and [50] revision 2 (answering [51]) were integrated into [AR] (§5e, §7.7 and the sections they touch), replacing the slots of §9.3. Each design had been written against the other's previous revision, and this issue against [40]'s revision 1 and [50]'s revision 1; the contradictions were decided on the evidence (recorded in [AR]'s Review log, integration entry) and this file changed as follows:

| Change | Why | Where |
|---|---|---|
| The R4 reservation table is [40] revision 2's R-1…R-18; the "open under [41]" column is closed; `digest` → `oid`; no `PathPrefix` op, no canonical-form item 11, no `Moirai-Path-Prefix` trailer | [40] revision 2 settled [41]'s blockers (B1, B4) and every [41 §5] change; an event inside a state-diff canonical form was [41 B4]'s defect | §2.5 |
| R5 reservations F16–F18 added; F1, F2 and F15 completed | [50] revision 2 (after [51 M7, m8, m9]) | §2.5 |
| Registry and validator tables list `ALLOC`, `PREFIXEV`, `GITRENAMES`, `PathClaim`, `QueryInvalid` | the added reservations | §2.6 |
| FL-10 relabelled as E2/E6 inside FL-4 (M6); M6 exit properties split by host milestone; GT17 covers rows 1–31 | [40] revision 2 dissolved FL-10 and has 31 matrix rows and P1–P15 | §0, §2.1, §2.4, §3.7, §3.13 |
| M7 = 49.5–70 units (was 46–64): LQ core 15–21.5k lines, 45–64 units | [50] revision 2's estimate (§8.2) had not been taken over; P9 | §3.8, §7.1 |
| Calendar re-run with this issue's own Monte Carlo (20,000 draws, same seed and schedule; the M7 increase put mostly on the lane-B front end, `LQ1` 8.5–11, `LQ2` 26–38, remainder 15–21): units 321.5–428 (P50 ≈ 375); one lane 42.5–88 (P50 60, P90 73); two lanes 28–56 (P50 39, P90 47.5) | follows from the M7 change | §0, §7 |
| R4 line count 22–30k (was quoted as 21–29k) | the sum of [40 §8.1]'s rows | §7.1 |
| §3.14 lists as owner decisions only what no `config` key can change later; the operational R4/R5 calls are `config` defaults the model implements; [40] #10, #11 are design rules; [40] #12 moves to the cutover | the owner's rule of 2026-09-26 ([AR §11]) | §3.14, §3.15, §8 risk 11 |
| Status notes: M0's entry criteria (re-reviews still open), its exit criterion "edit lists applied" (done), risk 4, §9.3, §9.6 | the integration | §3.1, §8, §9 |

No milestone order, gate, budget or exclusion changed.

### 10.5 Priority audits (2026-09-26)

Five audits judged the integrated design on the owner's priorities — speed [70], minimal RAM [71], correctness [72], minimal agent tokens [73] — and on feasibility and the configuration boundary [74]; [AR]'s Review log (priority-audit entry) maps every finding to its resolution, and none is rejected. This file changed as follows:

| Change | Findings | Where |
|---|---|---|
| Format-freeze rows for every format item the fixes add (`HEAD` durable bound, boot id, `config_gen`, `retired`; `LOCK` session slots; `group_end` and the position check; `actor u32`, `changeset_digest`, `cs_ref` and bulk-commit segments; `derived-optional` sections; `MARKERS_OLD`; lease holder anchors, deadlines and captured globs; `ALLOC` uid and `UIDX`; `ANCHORRES`, `GLOBIDX`, `TREES` epochs and dirty row; new record kinds; anchor-text digests; the per-uid key order; `sync_dir`; the E3d identity constant; the config syntax, not the key set); protocol decisions (a)–(c) restated and (g)–(i) added; new store parameters | [70 S1, S2, S4, S5, S7, S8, S17], [71 RAM-B1, RAM-m6], [72 B1, B2, M1–M4, M6–M8, M13, m6], [74 A10, A23] | §2.5 |
| M0: the subset crash enumerator and its seeded bugs; measurements 1–21; infrastructure profiles H and L; the Server 2025 Core OS-crash guest; the test-host specification; LQ-Bench on one model with the BM25 ablation; new decisions fixed by measurement | [72 M3], [74 A02, A04, A06, A14, A15, A19] | §3.1, §5.2 |
| M1: three-phase write, `durable_lsn` recovery, boot-change recovery, two-slot barrier, compact overlay, bulk commits, rollup only in the `gc` child, overlay-driven promotion; RAM and hold exit criteria; more seeded bugs | [70 S1, S2, S9], [71 RAM-B1, RAM-M1, RAM-M3, RAM-M5], [72 B1, M1, M2] | §3.2 |
| M2–M10 exit criteria and scope per the audits (state oracles, residue equivalence, uid uniqueness, merge RAM, export durability and diverged imports, E2 excluded and the R4 runtime additions, `wmem` and `IF TARGETS`, the config verb and ASCII header, three skills and the token ledger, `mcp_tool` hooks, deferred tools, sliced maintenance, MCP steady/aggregate RAM) | all five audits | §3.3–§3.11 |
| Gate catalogue: GT1 full subsets, GT4/GT15 every durable effect, GT7 shallow clones, GT8 both anchor modes, GT11 exclusive and tiered, GT12 harness caps and journal, GT16 sampled, GT17 rows 1–33; new GT18 (state oracles) and GT19 (token ledger) | [72 M3, M12, m7], [73 F8], [74 A05] | §3.13 |
| §3.14 lists only real owner decisions (#32–#40 added; 25 former questions are `config` keys or policy rows, [AR §13]); §3.15 owner hours, night schedule, profile L, mutation testing, disk and model-call budgets | [74 A02–A05, A11, A14, A24, C01–C50] | §3.14, §3.15 |
| Reference model: I26′ by its state definition, not by markers; memoisation at tips and every k-th commit (≤ 512 MB per case); markers and leases compared after recovery | [72 M1, M4], [74 A21] | §4.2, §4.4 |
| Measurement protocol: sample sizes tiered by duration; idle measured from 15 s after the last request; steady-state, aggregate and heap high-water RSS | [74 A04], [71 RAM-M2, RAM-m2, RAM-m9] | §5.1 |
| §5.4 amended to agree with [AR §8.3] (hold per verb class, daily-sync fixture, file opens and flushes, RAM rows, incremental export ≤ 50 ms, per-hook latency, token rows, MCP steady and aggregate RAM) | all five audits | §5.4 |
| RG3 restarts per changed crate set; RG6 includes tokens; RG10 signing per #39 | [74 A12], [73 F8], [74 A11] | §6 |
| Calendar: machine capacity per lane and the no-purchase arithmetic; M6 sizes without E2 | [74 A02, A03, A13] | §7.1, §3.7 |
| Risks 11 and 14 updated | [74 A02, A11] | §8 |

No milestone order changed. Two scope items leave the release by decision with measured revisit triggers and kept reservations — R4's USN evidence (E2) and the `refs/moirai/*` destination, plus the move nudge and the `PostToolBatch` delta hook ([74 A13, A17]) — and `links import` is built only if owner decision #23 says it runs; the unit ranges are left as issued until velocity re-issues the calendar at the M0 exit.

### 10.6 Verification pass (2026-09-26)

A verification pass checked [AR], [40], [50] and this file against each other and against the owner's rules; [AR]'s Review log (last entry) lists every finding. Changes here: the redb/heed "optional out-of-tree reference points" are deleted (§0, §1.1, §5.5, §5.6, the §9 edit-list text) — no third-party embedded database is built, linked or run for any purpose; **GT20** (spawn lint with the four call sites of [AR §5c], `Cargo.lock` dependency lint, streams with git absent) replaces M4's two-entry-point spawn lint and is mandatory from M0/M1/M4/M8 (§3.1, §3.2, §3.5, §3.7, §3.9, §3.13); M6 is E1 and E3–E8 with E2 excluded (§0, §2.4 item 5); M9 certifies the command transport only and M10 the `mcp_tool` hook budgets, `files.hooks.edit-evidence = auto` and the default `hooks.transport = auto`, with §2.4 item 7 justifying M9 before M10 against the owner's CLI → MCP → skills naming (§3.10, §3.11, §3.13 GT12/GT19, §5.4); `blocking --ids` gated at ≤ 300 µs at 1e5, engine time (§3.3, §5.4); the post-fan-out MCP aggregate (§3.11, §5.4); decisions #32 and #34 at M0 entry, the rest before M0 exits (§3.1, §3.14); decision #41 on the design team's scope exclusions (§3.14); the calendar labelled the pre-audit baseline (§0, §7.1); the M0 threshold note no longer cites a stale §4.7 figure (§3.1); the cross-platform reports tagged [X17]–[X20] (§3.14). No milestone order changed.

### 10.7 Cross-platform design (2026-09-26)

Owner decision #32 is decided ([AR §11], [AR §14], [80] revision 2, which answers its review [81]).
- **Scope.** M0 specifies and freezes the OS layer for all three OSes (§2.5's new rows, X-F1–X-F12). M1 builds its Windows implementation, including leaderless group commit through a flush byte with chained group validity and acknowledgement by identity. The Linux and macOS implementations become the port phase ([80 §5]), which is unscheduled and outside the release gate; nothing in M0–M11 builds, runs or tests a Linux or macOS binary. §1.3's "Unix `Vfs` and `ProjectFs`: excluded" row is replaced, and group commit leaves the leader row.
- **Format and fault model.** The Log rows gain the chained group trailer; `SegHdr` gains `total_len`; `LOCK` is laid out anew; fault-model items (2), (3), (5), (7) and (8) are amended, and (9)–(12) added; restated decisions (a), (c) and (h) follow group commit and the uniform rename point.
- **Protocol decisions** (j)–(m) are added.
- **M0.** Measurements 1–22; the thirteen group-commit seeded bugs; the three-OS specification in the review; + 5–7.5 units.
- **M1.** The OS layer's Windows implementation, the `LockBytes` contract with the in-process case, GT20 (d); flush counts "≤ 1 per durable commit, ≤ 3 per 16-writer burst"; + 2.5–5 units and + 0.5–1 for the non-gating type check. *(superseded by owner decision #44: GT20 (e) is a gate from M0; the cost is in the calendar, §7.1)*
- **M6 and M10.** `VolumeCaps`, tagged ids, the twin and copy rules (+ 1–2.5); the MCP server's parent watch (+ 0.5).
- **Gates.** GT1 gains the lost-group scenario; GT20 (d); GT12 adds cmd; GT4 and GT15 name the port rigs; per-OS notes for the port in [AR §8.3].
- **Decisions.** #42 (port-phase hardware, CI and platform coverage — money) is added; #21 gains (d). The cross-target type check and the platform matrix are design defaults, not owner decisions. *(superseded by owner decision #44: GT20 (e) is a gate from M0; the cost is in the calendar, §7.1)*
- **Cost.** + 9.5–16.5 units to M0, M1, M6 and M10, not yet in §7; the port phase is ≈ 52–83 units, outside this calendar. *(superseded by owner decision #44: GT20 (e) is a gate from M0; the cost is in the calendar, §7.1)*

### 10.8 Verification pass after decision #32 (2026-09-26)

[AR]'s Review log (XV1–XV16) lists the findings. Changes here: `seg.b<ref_id>.K` (§3.2); the Log row, decision (c), the GT4 row and the flush-count row without stale terms (§2.5, §3.13, §5.4); the lease deadline `{wall, boot_hash, mono}`, and the X-F11 keys registered rather than frozen (§2.5); R-13 aligned with [40] and the prospective key `files.journal` (§2.5, §3.14); GT17 and the M6 exit on rows 1–36 (§3.7, §3.13); GT20 (d) on direct dependencies (§3.13); hosted **Windows** runners and Windows-only PR CI (§3.1); the M8 exit with cmd and the `${CLAUDE_PLUGIN_DATA}` re-check, item 7 with that check, and item 11 with [AR]'s floors (§3.9, §5.2); risk 11 "≈ 21"; the source rows [81] and [X17]–[X20].

### 10.3 Issue 1 (2026-09-26)

Replaced the oracle-first plan of [AR §9] and the adoption-driven draft [60d] after the owner decisions of 2026-09-26: twelve dependency-ordered milestones, the Rust reference model as the oracle, floor-relative benchmark gates, the release gate RG1–RG12 and an edit list for [AR]. Reviewed by [61]; superseded by issue 2.

### 10.9 Harness-agnostic design and the calendar with every delta (2026-09-26)

Owner decisions #43 and #44 are applied from [90] (revision 2, after its review [91]): M0 gains the Codex probes (inside item 7), LQ-Bench v2 (if #38, reopened, says so), the codec decision among pure-Rust options (item 6) and GT20 (e) with the pure-Rust lint, mandatory from M0 (the non-gating M1 item of [80 §5.5] is withdrawn); M1 builds the chosen codec; M8, M9, M10 and M11 gain the harness-agnostic scope of [90 §10.2]; GT12, GT13, GT19 and GT20 are extended; §2.5 gains the harness-agnostic reservations; #43 and #44 are decided, #38 reopened, #45 added. **The calendar is re-issued with every delta since the pre-audit baseline** (§7.1): the audits (est. 23–40.5 units net of #41, per milestone with its reasons), the cross-platform design (9.5–16.5) and [90] (15–23.5) — ≈ 369–508.5 units; two lanes 33–69.5 weeks (P50 ≈ 47, P90 ≈ 57); one lane 48.5–104 (P50 ≈ 70, P90 ≈ 85.5); the milestone headings of §3 and the figures of §0, §7.2, §7.3 and §8 follow. Edited: §0, §1.3, §2.3, §2.5, §3.1–§3.14, §5.2, §5.4, §7.1–§7.3, §8 and this log.

### 10.10 Verification pass after decisions #43 and #44 (2026-09-26)

[AR]'s Review log (HV1–HV22) lists the pass. Changes here: §0's pre-audit baseline ≈ 321.5–428 units (HV6); §3.4's test driver simulates [90 §4.1]'s branch order of record, and GT2/GT18 add an MCP call with `lease` and no `branch` (HV1); the ten doors and M10's gate name MCP `write(name)` and `write(TX)` (HV2); byte labels in §3.9–§3.11 and §5.4 (HV11); §2.5's `dict.D` form under option (3) (HV13); M8 and M9 scopes carry the hookless export and sync fallbacks, GT12 a hookless export fixture (HV4); GT18's lease liveness covers TTL renewal by use (HV22); superseded notes in §10.7 (HV7). The calendar is unchanged.

### 10.11 The owner's answers of 2026-09-26 on [AR §11]

The owner answered (verbatim translation, quoted in the preamble): profile L (#34: no test host, no guest licence), two lanes both building on the laptop (#2), LQ-Bench on Opus 5.5 only (#38 (a)), a public GitHub repository (#36) whose commits carry no AI co-author, and every other decision as recommended. Changes here: the preamble quotes the answers; §0's M0 row and calendar; §1.2 P6, §1.3's rows for #39, #23 and #11 and §2.5's owner calls state the decisions; §3.1's entry, item 7 (profile L on the laptop, the public repository with hosted Windows runners for synthetic checks only, the repository rule on commit authorship with its `commit-msg` and PR-body checks, the gitignored local directory for owner-derived data, the rig on the laptop in the owner's WSL2 and Memory Integrity state unless calibration fails, no licence), item 10 (LQ-Bench on Opus 5.5 with two transport arms, ≈ 53 M tokens, ≈ $280; the adversarial subset corrected to 40 tasks beside the ~30 real-session questions, as in [50 §7.4]), the exit and the size basis (the Luna runner and floor tier, ≈ 0.5–1 unit, kept as contingency); §3.10's `links import` and Tier B wording and §3.12's heading (4–8.5 weeks in profile L) and signing; §3.13's intro, GT11, GT13 and GT15 (one laptop guest); §3.14 rewritten as the decided table; §3.15's owner hours (≈ 50–95) and machine time for profile L; §4's lock-step host; §5.1 and §5.2 items 16, 17 and 21; RG3 and RG10; §7.1 (two lanes on the laptop, profile L's machine time placed in the Monte Carlo, M11's weeks), §7.2 re-issued (two lanes 35.5–76.5 weeks, P50 ≈ 52, P90 ≈ 62; one lane 51.5–112.5, 75.5 / 91; the host column for comparison), §7.3; §8 risks 1, 11 and 14, and risks 19 (two lanes on one laptop) and 20 (the public repository) added. [74 A02]'s flat + 2–4 weeks is replaced by the schedule's ≈ + 5 weeks because most exits lie on the two-lane critical path. No milestone scope, gate volume or frozen item changes.

### 10.12 Verification pass after the owner's answers (2026-09-26)

[AR]'s Review log lists the pass. Changes here: GT4 runs three variants in profile L (`TerminateProcess`, `NtSuspendProcess`, ±1 h clock steps), and disk-full is covered by GT1's and GT3's injection and the owner-run RG7 drill on an owner-created small VHDX, as [AR §11] #34 and [74 A20] state (§3.2 gates, §3.9 gates, §3.13 GT4 row, §3.15: ≈ 3 h instead of ≈ 4 h a night); §3.13's intro defines the Runs column (CI = the hosted PR checks, synthetic data only), GT7, GT10 and GT12 split into hosted and laptop parts, and GT16 names where mutants run; §3.1 item 7 binds the commit rule from the first commit, which precedes M0; §3.12's size basis gives profile L's 2.5–3.5 weeks and the totals with the deltas; §3.14's #38 row says that GT12's Codex conformance and probes P4 and P10 are contract tests and measurements, not accuracy benchmarks; §5.2 item 21 measures one lane and both; RG11 states `links import` as decided (#23); risk 16 uses 68.5–95.5 units; §9 carries a historical-record banner. The calendar is unchanged: the Monte Carlo schedules windows, not the hours inside a night.

### 10.13 Cross-document consistency pass after the Russian approval review (2026-09-27)

The Russian owner-approval description (`docs/architecture-approval-ru/`) marked the statements on which [AR], [40], [50], this file, [80] and [90] disagreed; each was checked against the texts it cites, and the outlier was aligned with the normative statement. Items fixed here: **bulk-commit-TX** — M7's exit criteria refuse a 500k-op orchestrator `TX` with E501 and a split hint once its candidate exceeds `wmem`, never a bulk commit, as [AR §4.3], [AR §4.5] step 4 and [AR §8.3]'s GT11 row state; the ≤ 16 MB figure is unchanged (§3.8); **GT18-in-M0-gates-60** — §3.1's M0 gates include GT18 on the model with the I26′ state oracle, as §3.13's catalogue row and [AR §9]'s M0 row state (§3.1); **measurement-7-row-60** — §5.2 row 7 includes the Codex probes P1–P7, P10 and P11 and decides `integrate.codex.store-writes`, and the stamp route's re-runs are the Workflow experiment in M9 and the `mcp_tool` experiment in M10, as §3.1 item 5, §3.11 and [AR §8.2] item 7 state (§5.2); **measurement-15-extension-60** — §5.2 row 15 carries [40 §8.3.6]'s extension, as [AR §8.2] item 15 does (§5.2); **stale-statements-60** — §0 says that M0 provisions CI and the test infrastructure on the existing public repository, as [AR]'s risk 20 does; §4.4 item 3 has every read result name its `branch` and `rev` and the commit id only in `--json`, the frozen envelope of [50 §6.4]; §7.1 places RG3's 14 consecutive completed nights in agent-free windows with only the soak inside a 3-day agent freeze, as [AR §9]'s calendar and machine-capacity paragraphs do. §3.1 item 7's attribution text stays as it is until the owner answers items A2 and A3 of `15-approval-checklist.md`. No line above this entry was added or removed, and no milestone order, gate threshold or calendar figure changed.

### 10.14 The owner review of 2026-09-27

The owner answered the approval checklist of the Russian description (`docs/architecture-approval-ru/15-approval-checklist.md`; items А1–А8, Б1–Б14 and В1–В10, cited as A1–A8, B1–B14 and V1–V10); [AR]'s binding inputs record the answers and its Review log entry of the same date lists the whole change. Changes here, every one in place:
- **A1** — the re-review of [40] and [50] revision 2 is part of item 8's specification review and closes before the format freezes; it is not an entry condition (§3.1 entry, item 8 and exit criteria; risks 3 and 4). **A2, A3** — the committed harness configuration and the accepted published tree (§3.1 item 7, §3.15, risk 20). **A4** — [AR] with [40], [50], this file, [80] and [90] approved as the M0 specification (§3.1 entry). **A6** — GT13 runs at M0 on LQ-3, the model's own parser, binder and evaluator; LQ-1 and LQ-2 stay in M7; the M0 exit criterion lists the normative gates of [AR §7.7.5] and [50 §7.4] item 6 — the real-session stratum, ≤ 5 % per construct, the card ≤ 1,000 tokens and lint and semantics among the allowed remedies added, the "within 5 points of the best candidate" gate removed (§2.4 item 6, §3.1 items 10 and exit criteria, §3.13 GT13). **A7** — the five FL-1 targets gate M0 exit and the three resolver-dependent ones (delete-then-re-add `replaced`, the 46 worktree HEADs, the transcript census's edited+moved) gate M6 exit (§3.1 item 12 and exit criteria, §3.7). **A8** — build windows agreed at M0 start (§3.1 entry, §7.1). **B1–B14** confirmed.
- **V1** — LQ-Bench runs through the owner's Claude Code subscription in headless mode, with no API billing and no API key; ≈ 53 M tokens at M0, mostly cached input, in several usage windows within the weekly limits; the documented shrink rule of [50 §7.4] item 5 if the quota is short; Claude token counts from Claude Code's reported usage (§3.1 item 10, §3.13 GT13, §3.14 #38, §3.15, §5.2 items 6 and 20).
- **V7** — the OS-crash rig is deferred to after the release with the port phase: GT15, measurement 17, the guest and its licence question, the WSL2 and Memory Integrity choices; the M1 exit criterion (≥ 1,000 cycles) and RG3's ≥ 5,000 cumulative cycles leave M0–M11; GT15's specification and every format field and protocol rule it tests stay; crash safety in M0–M11 rests on GT1, GT3, GT4, durable-before-acknowledge, `backup`/`restore` and the daily image export, with real power loss the residual risk of [AR §10] risk 17 (§0, §1.4, §2.3, §2.4, §2.5 item (h)'s `file mv` rule, §2.6, §3.1 item 7 and exit criteria, §3.2, §3.5, §3.12, §3.13, §3.14 #34, §3.15, §4, §5.2 item 17, §6 RG3, risks 7, 8, 11 and 14). The VHDX setup (V8) stays: it serves measurements 18 and 22 and the RG7 drill.
- **Calendar** (§0, §7.1–§7.3 including the audits table's M11 row, risk 1): the same Monte Carlo (seeds, 20,000 draws, rate, schedule, units and deltas) without the rig's machine time — GT15's M1-exit cycles on the laptop guest leave; RG3 keeps 2.5–3.5 weeks and the M0 exit its 2–4 windows. Two lanes in profile L: storage engine 12–26.5 weeks (P50 ≈ 17.5, P90 ≈ 22), release **35–75 weeks (P50 ≈ 50.5, P90 ≈ 60.5)**, was 35.5–76.5 (52 / 62); one lane 51–110.5 (74.5 / 89.5), was 51.5–112.5 (75.5 / 91); a test host unchanged at 33–69.5 (47 / 57), so profile L costs ≈ + 3.5 weeks at P50; the hosted-runner variant 48.5 / 58.5. The nights from M1 exit to release are unchanged (≈ 145–195). The GT15 harness's 2–3 units and the rig's M0 provisioning stay as contingency until the M0 re-issue.
- V2–V6, V9 and V10 remain open resource items for M0. §9 (the historical edit list) is not changed. No frozen byte changes.
- **Follow-up checks of the same day:** §3.1 item 5 names measurements 1–16 and 18–22 (item 17, the rig calibration, deferred with the rig), as the exit criteria do; the exit criterion for the 58 dead memory paths says that at M0 the rename chains come from item 12's test-only git CLI extraction and are re-bound through FL-1 and the model's exact-evidence resolution, the product's E6 re-running the target at M6; §3.8's size basis calls LQ-3 the model's own parser, binder and evaluator; the ≈ 53 M of §3.1 item 10, §3.14 #38 and §3.15 is labelled as the neutral-API estimate before Claude Code's per-call overhead, with ≈ 1 M added for the repeated 52-prompt sample, and M0's first usage window measures that overhead and re-issues the quota plan, with [50 §7.4] item 5's shrink rule as the fallback; §5.2 item 7 notes that without Codex access (V9 asks the owner to confirm after V1) the Codex probes wait, their decisions keep the documented defaults and P10 skips Luna's column ([90 §10.5]). No unit, week or gate threshold changes.

### 10.15 A1 re-review (M0 WP-80a), 2026-09-27

The owner's item A1 made the re-review of [40] and [50] revision 2 part of the M0 specification review (§3.1 item 8). The three lenses' findings and every disposition are in `docs/spec/reviews/a1-dispositions.md`; [40] and [50] record theirs, and [AR]'s Review log entry of the same date summarises the whole change. Changes here, every one in place:
- **§2.2** C6 → C7 names `link_state()`, not the withdrawn `link_status()` (A-m8). **§2.5** restates R-3's predecessor, dead-uid and re-key rules and notes the other R-rows the review changed, with [40 §2.11] authoritative (S-01, S-03).
- **§3.1** item 4 adds the seeded bug "an intent roll-forward without the re-barrier" (A1P-02); item 10 states the quota plan in raw tokens and moves the overhead measurement to the runner's first real calls, before the quota ask (A-M5); "Decisions fixed at M0 exit" decides the leader at the maximum in-lock cost and the checkpoint threshold on a product-shaped overlay, both re-checked by M1's gates (A1P-03).
- **§3.8** M7's exit replaces the withdrawn "CLI ≤ 4 MB at 1e5" by [50 §5.12]'s composition rule, with the baseline measured at M0 (S-23).
- **§3.13** GT18 states what its M0 rows assert on the single-process model and where the concurrent variants start (A-m9); GT20 (d) adds direct `std::fs` file I/O in product crates (A1P-10).
- **§4.2–§4.3** the model takes git history as abstract data and implements the tree gate, E6 and the writer-tree and freshness rules; the exclusion is "the git object layer's bytes" (A-M4). PLAN §6.2 R16 records the lane-B cost (+0.5–1 unit) for WP-99.
- **§5.2** items 2 and 10 (the in-lock cost sweep; the overlay-applying replay), 11 (the CLI baseline on `main` and a lane), 15 (the rename as the protocol performs it; `ChangeTime` on rename) and 19 (a pending settle during a burst). **§5.4** flushes per verb count log and directory flushes separately (A1P-07); the CLI result header is limited per part (A-M3); M7's RSS row follows the composition rule (S-23).
No unit, week, calendar or frozen-byte figure of this file changes except PLAN's +0.5–1 unit for A-M4, which WP-99 carries into §7.
