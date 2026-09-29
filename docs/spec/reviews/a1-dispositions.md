# A1 re-review: dispositions

| Field | Value |
|---|---|
| Title | Dispositions of the A1 re-review of [40] and [50] revision 2, and the outcome of PLAN §6.2's resolutions R1–R19 |
| Status | draft, pass 1 pending (the owner signs the dispositions in WP-80, V2) |
| Work package | WP-80a (`docs/m0/PLAN.md` §3.2 item 8), disposition by R-SPEC (design editor) |
| Findings disposed | `docs/spec/reviews/a1-P.md` (lens P: A1P-01…A1P-17), `a1-S.md` (lens S: S-01…S-24), `a1-A.md` (lens A: A-M1…A-M5, A-m1…A-m9) |
| Sources edited | [40] `docs/research/design/40-file-links-design.md` (§0.1, §0.2, §1.2, §1.3, §2.2–§2.11, §3.1, §3.4–§3.8, §4.2, §4.3, §4.6, §5.1, §5.5, §5.7, §6.3, §6.5, §7.2, §7.3, §8.3.2, §8.3.5, §9.1, §9.2, Review log "A1 re-review (M0 WP-80a), 2026-09-27"); [50] `50-query-language-design.md` (header, §2.2, §2.6, §2.8, §2.9, §3.10, §4.1, §4.4, §5.2, §5.5, §5.9, §5.10, §5.12, §6.4, §7.4, §8.1, §8.2, §10.3, §12.15); [AR] `docs/ARCHITECTURE-RESEARCH.md` (header, §4.3–§4.6, §4.10, §5a.7, §5b.2, §5e.2–§5e.8, §7.1, §7.2, §7.7.5, §8.2, §8.3, §11 #38, Review log "A1 re-review (M0 WP-80a), 2026-09-27"); [60] `60-roadmap.md` (header, §2.2, §2.5, §3.1, §3.8, §3.13, §4.2, §4.3, §5.2, §5.4, §10.15); PLAN §6.2 |
| Precedence applied | [40]/[50]/[80]/[90] for their own reservations, [AR] otherwise; every conflict is recorded in §6 |

## 0. Summary

55 findings: **14 majors, all fixed**; **41 minors: 38 fixed, 3 deferred to WP-80 pass 1** (S-16, S-22, A-m1); **none
rejected**. No blocker was raised. After these dispositions WP-80a has **zero open blocker or major findings**, the
condition PLAN §3.2 sets for accepting WP-11–15, WP-19, WP-33, WP-61–64 and WP-90–93.

All nineteen resolutions of PLAN §6.2 are **confirmed**; none is overturned. R2, R10, R16 and R18 carry amendments that
lenses P and A asked for, now written into PLAN §6.2; the other conditions are listed with their owners in §3.

**Frozen-byte and frozen-rule changes** are listed in §2 for the owner. They all land before the format freeze; none
touches a byte already written anywhere.

## 1. Dispositions

"Fixed" means the design text now carries the fix (the "Where" column); "deferred" means the finding stays open for WP-80
pass 1 with the named owner; the chapter owners of §5 transcribe every fixed rule.

| Id | Lens | Severity | Disposition | Where fixed | Note |
|---|---|---|---|---|---|
| A1P-01 | P | major | fixed (preferred fix) | [40 §3.1, §3.4 steps 1 and 3, recovery table, §4.6, §9.1 risk 10]; [AR §4.10, §5e.5] | A cross-volume file move is refused (exit 7, "move it with a raw mv; links re-bind by evidence"); the copy path is deleted, so `ProjectFs` needs no copy operation and R1/R2's "complete trait" holds. [80 §2.11.4] rule 6 still describes the copy: §4 |
| A1P-02 | P | major | fixed | [40 §3.4 step 5, §3.5 step 4, §8.3.5]; [AR §4.10, §5e.5, §8.2 mutation list, §8.3 seeded bugs]; [60 §3.1 item 4] | Every roll-forward first calls `sync_dir` on both parents; a protocol point for chapter 16 beside [80 §2.3.2]; seeded bug "roll-forward without the re-barrier" (WP-40); WP-32 enumerates rename → crash → recovery → loss of unsynced names. `ProjectFs::sync_dir` already in [AR §4.10] |
| A1P-03 | P | major | fixed | [60 §3.1 decisions paragraph, §5.2 items 2 and 10]; [AR §8.2 items 2 and 10]; PLAN §6.2 R10 | Toy log with product `RecHdr`, chained groups, spec commit sizes; measurement 2 sweeps the in-lock cost up to the hold budget; WP-53d applies records into a product-shaped overlay with the lazy mix; M1's gates re-check both. Chapter 17 states the constraints (WP-16) |
| A1P-04 | P | major | fixed | [40 §2.6 `TREES`, `FILEOBS.verified_at`, §4.2, R-7, R-8]; [AR §4.3, §4.4, §5e.3] | Epoch `{scope_kind u8, _ [3], scope_ref u32, scope digest [16], hlc u64}` (32 B; 48 B in `TreeReg` with the tree key); `partial` (the brief, `--scope`, `--path`) never advances `verified_at`; newest per (tree, scope_kind, scope_ref) kept. Frozen-byte change (§2) |
| A1P-05 | P | minor | fixed | [40 §2.5]; [AR §5e.2] | One handle; pass 2 checks the normalised length and unchanged size and mtime; one retry, then `unverified`. WP-62 adds the race test (§4) |
| A1P-06 | P | minor | fixed | [40 §4.2]; [AR §5e.3, §8.2 item 19]; [60 §5.2 item 19] | Hook and server settles never sleep; lock waits bounded by the remaining cap; measurement 19 with a pending settle and a burst |
| A1P-07 | P | minor | fixed | [40 §3.4 cost, §7.2]; [AR §8.2 item 15, §8.3 flushes row]; [60 §5.2 item 15, §5.4] | "2 log flushes and 2 directory flushes", counted separately; measurement 15 renames with the protocol's flags; WP-81a re-derives [40 §7.2] |
| A1P-08 | P | minor | fixed | [50 §5.10, §5.12]; [40 §7.3] | [AR §8.3]'s figures replace [50]'s stale ones (conflict: [AR] wins, §6); `mem` headroom from the process kind's gate (CLI/hook 4 MB + 1 MiB per extra ref; MCP 16 MB); ≤ 0.65 MB. The gate becomes a named parameter in chapter 17 or `config.md` (WP-16/WP-18) |
| A1P-09 | P | minor | fixed | PLAN §6.2 R2; [50 §5.10] | `Meter` gains `private_now` and `private_peak` (WP-17, WP-30) |
| A1P-10 | P | minor | fixed | [AR §8.3 GT20 (d)]; [60 §3.13 GT20 (d)]; PLAN §6.2 R18 | Direct `std::fs` file I/O refused in product crates other than `moirai-os`; WP-62's reader takes a byte source; the narrowing of [80 §2.1] recorded in [AR] |
| A1P-11 | P | minor | fixed | [50 §8.1 F17] | [AR §4.4]'s widened `ALLOC` + `UIDX`; conflict recorded (§6); owner confirms the exception in pass 1 |
| A1P-12 | P | minor | fixed | [40 §2.7, R-4, R-6]; [AR §4.3] | The `AddEdge` that creates an anchor carries its `aN` (unhashed); recovery derives `next_anchor`; a known anchor uid reuses its `aN`. Small frozen-byte change (§2) |
| A1P-13 | P | minor | fixed | [50 §3.10 item 10, §5.10]; [AR §4.5 step 7] | `tx.max-work-in-lock` calibrated at M7 under "cap × ns/unit ≤ hold p99 − plain append cost"; 5e5 provisional; chapter 17 lists the key with the constraint (§7) |
| A1P-14 | P | minor | fixed (by S-01) | [40 §2.3] | The dead-uid check reads the registering view only: one lookup per step, no per-ref probe |
| A1P-15 | P | minor | fixed | [40 §4.2, R-13]; [50 §5.10]; [AR §5e.3] | `fs` units are the budget of every read path; `files.read-budget-ms` is only the safety net, reported like E503 (WP-18) |
| A1P-16 | P | minor | fixed | [40 §2.6, §7.3]; [AR §4.4] | `ANCHORRES` kept for current or last-observed content of a live file node (≈ 12 MB at 1e5); `GITFACTS` within bound trees' E6 windows (WP-13) |
| A1P-17 | P | minor | fixed | [AR §4.10]; PLAN §6.2 R2 | No `journal_since` in the M0 trait; `JOURNALCUR`/`JournalCursor` stay reserved. [80 §2.1] still lists it: §4 |
| S-01 | S | major | fixed | [40 §0.1 item 4, §2.3, §5.5 re-key row and convergence paragraph, R-3, §8.3.2 P13, §9.1 risk 4]; [AR §4.6 reservations, §5a.7 R4 row, §5e.2, §5e.6]; [60 §2.5 R-3] | (A) the dead set is the registering view's (option 1); (B) predecessor = greatest candidate uid, tombstones not candidates (their uids stay dead through the view rule); `created` provenance only; (C) the re-key re-points every edge side S added since the LCA, recorded in a sync's residue; (D) convergence claim stated with its condition; P13 gains three cases. Frozen rule change (§2); residue in §6 |
| S-02 | S | major | fixed | [50 §4.4]; [AR §5b.2 rule 9] | Rewrite on the bound AST by type (`#u:`, full commit ids, quoted outside revision positions); anchor handles under E117; validation by re-binding; the two-store test covers every spelling (WP-19, WP-15, WP-13) |
| S-03 | S | major | fixed | [40 §2.7, I-F2, R-3, R-4, §5.7]; [AR §4.6, §5b.2, §5e.2, §5e.8] | `captured` includes widened prefix/suffix and a `lines` window; anchor uid gains `lp(pred or empty)`, re-derived on a collision on (src, dst); `pred` stored. Frozen-byte change (§2) |
| S-04 | S | major | fixed | [40 §2.7 `end`, R-10, R-11, §5.7]; [AR §4.6 item 10, Not hashed, §5b.2 rule 9] | `end` is exact text only and enters item 10 as `end_h`; every anchor line carries `end_h`; text in `full` mode verified; `text-unavailable` covers it; gate 0 and GT8 include a range anchor (WP-21). Frozen-byte change (§2) |
| S-05 | S | major | fixed | [40 §2.9, §6.5, R-16]; [50 §2.6, §5.2 W10, §7.4 item 2]; [AR §5e.2] | `link_state(n)` = `none` without `AT` edges; `a.state` = `unresolved` for an unresolved file; W10 warns on `<>`/`NOT IN` over the node form and counts `none` rows. Totality alone would not fix `<> 'ok'` (`none <> 'ok'` is true), hence the warning; §6 |
| S-06 | S | minor | fixed | [50 §3.10 item 3, §5.9 step 4] | [AR §4.5] step 7's trigger (markers and leases) adopted; runtime predicates stated (WP-19, WP-16) |
| S-07 | S | minor | fixed | [40 §4.3 step 0, §6.5]; [50 §3.8 unchanged] | LQ built-ins: E302 with the `--tree` hint; `unverified (no tree)` only in packs, briefs and `links check`, which render it themselves |
| S-08 | S | minor | fixed | [40 §2.9, §4.3, R-16]; [AR §5e.2, §5e.3]; [50 §10.3 note] | `unverified` details closed: `budget`, `cloud-only`, `commit not in this repository`, `no tree`, `git`, `size`; examples non-normative; header grammar from [AR §7.1] (WP-14, WP-18) |
| S-09 | S | minor | fixed | [50 §8.1 F16, F17] | `affected_complete u8` and the widened `ALLOC`/`UIDX`, both [AR]'s; conflicts recorded (§6) |
| S-10 | S | minor | fixed | [50 §4.4] | Both sides bound against the merge result's schema; equal hashes land dst's text (WP-19, WP-12) |
| S-11 | S | minor | fixed | [50 §2.2 rule 6] | LQ's `ref_name` is the store's ref-name grammar; segments matching `c[0-9a-f]{7,64}` or `s[0-9]+` refused; literals first in revision positions (WP-12, WP-19). [80] X-F9 already says "LQ `ref_word` stays as is" |
| S-12 | S | minor | fixed | [40 §8.3.2 P11 and the model paragraph] | The conservativeness order defined; P11 is subset-consistency (WP-77, WP-92) |
| S-13 | S | minor | fixed | [40 §1.2, DR8, I-F10]; [AR §5e.3, §5e.8] | Inputs: versioned link, tree snapshot with git objects, the tree's runtime rows, resolver version, budget; caches output-neutral; `unverified` the only budget-dependent output |
| S-14 | S | minor | fixed | [40 I-F14]; [AR §5e.8] | `revert`, `undo`, `cherry-pick`, `op restore` of the removing commit are explicit, recorded doors |
| S-15 | S | minor | fixed | [40 §8.3.2 P8] | Commutativity up to the listed directional rules |
| S-16 | S | minor | **deferred** (pass 1; WP-14b with WP-55) | measurement input added: [AR §8.2 item 15], [60 §5.2 item 15] | Not unambiguous: `verified_at` is a lower bound of F's last sighting (more so after A1P-04's `partial` epochs), so a `ChangeTime > verified_at` condition protects only partly; lens S decides in pass 1 between that condition and "line 2 yields at most `identical copy`" |
| S-17 | S | minor | fixed | [40 §4.3 "Clock domains", R-14] | ns everywhere, HLC → `(hlc >> 16) × 10^6`, committer time × 10^9, conservative side by a skew margin: a named hole of chapter 20 (§7) |
| S-18 | S | minor | fixed | [40 §2.5, R-14]; [AR §5e.2] | Per-root, `init`-fixed: `project` = the store repository's object format (SHA-1 without git), other roots SHA-1; different algorithms never compare equal |
| S-19 | S | minor | fixed | [40 R-1, I-F8]; [AR §4.6 item 10] | Roots of `path`/`pathmove` values encoded by name in canonical values; `path` root = the `root` field (WP-12, WP-14) |
| S-20 | S | minor | fixed (with A-M2) | [40 §2.4, R-1] | See A-M2 |
| S-21 | S | minor | fixed | [50 §2.8 departures table, §7.4 item 2] | Walks reusing a fixed part's edge; undirected and mixed-kind patterns over DAG kinds are cyclic; LQ-Bench tags both |
| S-22 | S | minor | **deferred** (conditional; WP-19/WP-13 after WP-72) | [50 §5.5] states the obligation | Moot if the statistics-free scorer wins the ablation; if BM25 stays, F12 fixes the f64 formula, summation order, rounding and tie before the freeze |
| S-23 | S | minor | fixed | [60 §3.8 exit, §5.2 item 11, §5.4 R5 row]; [AR §8.2 item 11]; [50 §5.12] | The withdrawn "CLI ≤ 4 MB at 1e5" replaced by the composition rule; measurement 11 reports the baseline on `main` and on a lane |
| S-24 | S | minor | fixed | [50 §2.9 Q18] | 64 hex digits; see A-m7 for the JSON rule |
| A-M1 | A | major | fixed | [40 §2.2 (closed vocabulary table), §3.6, §3.7, §4.3, §9.2 decision 1, R-17]; [50 §4.1 `links_guesses`]; [AR §5e.5] | Closed ASCII grammar: `how` ∈ {`explicit`, `lazy`, `git`, `hook`, `journal` (reserved), `owner`, `agent`, `policy`, `confirmed`, `merge-observation`, `merge-compose`}; evidence tokens per E-source and proposal class; `manual`, `replacement`; score two decimals, half-even, only on scored tokens; `--confirm` rewrites `agent`/`policy` to `confirmed`; `links_guesses` and `[accepted guess]` cover `agent/*` and `policy/*`. `policy` is new (policy B's automatic strong re-binds). Frozen hashed-value set (§2). WP-21 adds one fixture per value |
| A-M2 | A | major | fixed | [40 §2.4, R-1] | `pathmove.hlc` = the writer's HLC at candidate computation (phase 1); a re-parent never changes it; never compared with the commit's `hlc`; the Store API's injected clock lets the model derive it (WP-12, WP-25) |
| A-M3 | A | major | fixed | [AR §7.1 header rules and examples, §5e.4, §8.3 TOKENS]; [50 §6.4, §2.9 examples]; [40 §2.9, §4.6, §5.1]; [60 §5.4] | Tree display label (path relative to the main worktree's parent, else the last two components; ≤ 20 B, git branch ≤ 16 B); the reader note in R-16's ASCII spelling on line 2 (≤ 80 B); limits per part: base ≤ 60 B, `files @` ≤ 80 B, extras ≤ 60 B, `dropped`/`more` ≤ 30 B. A restated TOKENS gate: owner confirms (§6). WP-18 freezes, WP-22/WP-71a re-count |
| A-M4 | A | major | fixed | [60 §4.2, §4.3]; [40 §8.3.2 model paragraph]; PLAN §6.2 R16 | The model takes git history as abstract data and implements G1–G4, E6, the writer-tree, freshness and committed-only rules; WP-70 generates every [40 §2.9] state; +0.5–1 u lane B for WP-99 |
| A-M5 | A | major | fixed | [50 §7.4 item 5]; [60 §3.1 item 10]; [AR §7.7.5, §8.3 TOKENS LQ-Bench row, §11 #38] | Raw ≈ 80 M input tokens (est.) beside the price-weighted ≈ 53 M; the runner's first real calls record input, cache-read, cache-write and output tokens of an empty turn and the card's Claude tokens, before any quota ask; the smoke run re-issues the plan and the V9 ask quotes it. [90 §8.3] wording at WP-81a, as the finding assigns (§4) |
| A-m1 | A | minor | **deferred** (pass 1; WP-14 R-16 before M9) | — | Two valid fixes (cap the `was:` quote and shorten `pending`/`diverged`, or restate the gate as ≤ 50 B median, ≤ 90 B max); the choice belongs with R-16's strings |
| A-m2 | A | minor | fixed | [50 §2.6, §6.4]; [40 §6.5] | Column `next`; the shape says "the next command (evidence or settle, never an accept)" (WP-19) |
| A-m3 | A | minor | fixed | [40 §3.8 preamble]; [50 §10.3 note] | [40 §3.8] illustrative; [AR §7.1] and [50 §6.4] normative (WP-18, WP-22) |
| A-m4 | A | minor | fixed | [40 §0.2, §3.4, §3.8 example]; [AR §5e.5] | Agent text: raw-`mv` fallback or "ask the orchestrator"; bind line only for orchestrator and owner (WP-18) |
| A-m5 | A | minor | fixed | [40 §4.2]; [50 §4.1, §5.10] | Verb-invoked `links check` runs at the orchestrator's `fs` ceiling for every role; `--budget fs=` lowers it; `--budget-ms` is the wall-clock safety net (WP-18, WP-19) |
| A-m6 | A | minor | fixed | [40 §6.3]; [AR §7.2] | `quote` and `end` string parameters; U+FFFD refused |
| A-m7 | A | minor | fixed (varied) | [50 §2.9 Q18] | One rule, but `c` + 64 lower-case hex in JSON (as Q1, Q6 and the `.moi` already use, and pasteable as a revspec), not the finding's prefix-less form; text keeps `c<8 hex>` (WP-18) |
| A-m8 | A | minor | fixed | [40 Review log: superseded note, end marker moved]; [60 §2.2]; [50 §8.2 LQ-8, §10.3] | Stale `link_status`, preset count, end marker; LQ-8's test crosses the 24,000-B page and checks the stderr footer and exit 10 |
| A-m9 | A | minor | fixed | [60 §3.13 GT18] | M0 rows assert CAS-drop, TTL, renewal and `Unknown` rules over enumerated interleavings; concurrent variants from M2/M6 (WP-94) |

## 2. Frozen-byte and frozen-rule changes (for the owner)

Each change lands before `format-v1`; the chapters named write the bytes. **None alters a byte or rule of a structure that
has been written anywhere; each alters what M0 will freeze.**

| # | Change | Finding | Bytes or rule before → after | Chapter |
|---|---|---|---|---|
| FB-1 | Settle epoch layout and retention | A1P-04 | `{scope digest [16], hlc u64}` (24 B), unbounded → `{scope_kind u8, _ [3], scope_ref u32, scope digest [16], hlc u64}` (32 B in `TREES`; `TreeReg` record 48 B with the 16 B tree key), one per (tree, scope_kind, scope_ref) | 05, 11 (WP-11, WP-13) |
| FB-2 | File-uid predecessor and dead set | S-01 | predecessor by greatest (generation, commit id) among removed, deleted and aliased nodes; dead = removed/deleted on some branch head of the store → predecessor = greatest candidate uid among the view's removed and aliased nodes; dead = any node of the registering view. **Derived uid values change** for re-registered paths | 08 (WP-14), R-3 |
| FB-3 | Merge re-key scope | S-01 | anchors re-pointed → every edge the re-keyed side added since the LCA, recorded in a sync's residue | 12-vcs (WP-12) |
| FB-4 | Anchor capture digest and uid | S-03 | `captured` over (file uid, kind, scope, quote, end, occurrence) → adds widened prefix, suffix and a `lines` window; anchor uid gains `lp(pred or empty)`; the anchor record and the anchor line gain `pred` (0 or 16 B) | 08, 18 (WP-14), 07 (WP-12), 14 (WP-15) |
| FB-5 | `end` in canonical item 10 | S-04 | `end` text hashed → `end_h` (BLAKE3-128) only; anchor lines carry `end_h` | 07 (WP-12), 14 (WP-15) |
| FB-6 | `relink` value set | A-M1 | open examples → closed ASCII grammar with a new `how` token `policy` and a fixed two-decimal score | 18 (WP-14) |
| FB-7 | `aN` in the log | A1P-12 | not carried → the creating `AddEdge` carries `aN` (store-local, unhashed) | 06 (WP-12), 05 (WP-11) |
| FB-8 | `path` roots in canonical values | S-19 | unstated → encoded by root name | 07 (WP-12) |
| FB-9 | `pathmove.hlc` | A-M2, S-20 | "the adding commit's hlc" → the writer's HLC at candidate computation (same field, definition fixed) | 06/07 (WP-12) |
| FB-10 | `oid` algorithm | S-18 | "per the repository" → per root, `init`-fixed | 20 (WP-14b) |
| FB-11 | F16, F17 layouts | S-09, A1P-11 | [50]'s flag bit and 8 B `ALLOC` → [AR]'s `affected_complete u8` and 24 B `ALLOC` + `UIDX` (already [AR]'s; the exception to "[50] for its own reservations" needs the owner's confirmation) | 06 (WP-12), 11 (WP-13) |
| FS-1 | Frozen strings and codes | S-05, S-08 | + `none`, `unresolved`, the closed `unverified` detail set; + warning W10 | 18 (WP-14), `lq/errors.md` (WP-19) |
| FS-2 | Named-query stored text | S-02 | lexical rewrite → rewrite by bound type; E117 also refuses anchor handles | `lq/` (WP-19), 14 (WP-15) |
| FS-3 | CLI header limits | A-M3 | ≤ 60/100 B (≤ 90/130 B) combined → per part: base ≤ 60, `files @` ≤ 80, extras ≤ 60, `dropped`/`more` ≤ 30, reader note on line 2 ≤ 80 | 19 (WP-18) |
| FS-4 | Cross-volume `file mv` | A1P-01 | copy → flush → verify → delete → refused (exit 7); an explicit-verb capability is removed | 16 (WP-16), 18 |

## 3. PLAN §6.2 resolutions R1–R19

P, S and A give each lens's verdict (c = confirm, c+ = confirm with conditions or amendments). No lens overturned any
resolution.

| # | P | S | A | Outcome | Conditions and their owners |
|---|---|---|---|---|---|
| R1 | c+ | c+ | c+ | confirmed; owner day-1 confirmation pending | WP-33 builds `fs`, `lock`, `map`, `env`, `proc`, `mem` first (they gate WP-50, WP-52), the `ProjectFs` half after; `CountingAlloc` as `#[global_allocator]` only in probe roots; WP-33 accepted only after WP-80 pass 1 of `os/`; M1's `Vfs` certification and M6's `ProjectFs` conformance not waived; each measurement records the `moirai-os` commit and a changed call path re-runs it (WP-33, WP-50, WP-80) |
| R2 | c+ | c+ | c | **amended** in PLAN §6.2 | `ProjectFs` has `sync_dir`, no `journal_since`, no copy op; `Meter` gains `private_now`/`private_peak`; the grant table's move to `moirai-vfs` recorded (WP-17, WP-30) |
| R3 | c | c+ | c | confirmed with condition | Every checksum defined over on-disk bytes; E3's wording says content digests over decompressed bytes are verified at M0 for codec-`none` fixtures and for real-codec frames from M1 (WP-10–13, WP-95, PLAN E3 text) |
| R4 | c | c+ | c+ | confirmed with conditions | The `--json v1` data shape is total (multiplicity and order included); WP-25 fixes field names and order and lists every excluded field with a reason |
| R5 | c | c+ | c+ | confirmed with conditions | Known-answer vectors for xxh3-64, BLAKE3, SHA-1 and SHA-256 as fixtures (WP-21); the model builds `fold_v1` once per process (WP-90); [60 §4.6]'s list amended at WP-99 |
| R6 | c | c+ | c+ | confirmed with conditions | "Matches the UCD test data" = `NormalizationTest.txt` for NFD, `CaseFolding.txt` C and F exhaustively, agreement with the model's derivation over every scalar value, CCC and Hangul in the tables (WP-61); the UCD download is in the day-1 bundle |
| R7 | c | c | c+ | confirmed with condition | tsoracle's grammar version pinned in `tools.md` (WP-06) |
| R8 | c | c | c | confirmed | — |
| R9 | c | c | c | confirmed | — |
| R10 | c+ | c+ | c+ | **amended** in PLAN §6.2 | A1P-03's vehicle rules; each decision names the product re-run that re-validates it; M1's exit re-runs measurements 1, 2 and T2 on `moirai-store` and revisits the leader and the lock waits if a value moves (WP-40, WP-52, WP-53d, WP-16, M1 plan) |
| R11 | c+ | c | c | confirmed with condition | Measurement 19 includes a pending settle during a subagent burst (WP-56; [60 §5.2] item 19) |
| R12 | c | c+ | c+ | confirmed with conditions | GT18 at M0 detects incremental-versus-definition divergence only; the definitions rest on the V3 signatures and GT10, and the from-scratch functions are written from the signed rule text; WP-94 states what each M0 row asserts ([60 §3.13]) and reports the wall time per 10⁵ histories in its first nightly |
| R13 | c | c+ | c+ | confirmed with conditions | The spec text (`lq/envelope.md`, `19-errors-and-output.md`, R-16) wins over the goldens, which are reviewed in pass 2 before they bind M7/M8; the renderer implements A-M3's header rules (WP-71a) |
| R14 | c | c+ | c+ | confirmed with conditions | Result files carry every field [50 §7.4] item 8 puts on `measurement` nodes: metric, value, unit, model id, Claude Code version, card and grammar versions, environment, commit (WP-72) |
| R15 | c | c | c+ | confirmed with conditions | WP-53 and WP-54 run only in machine-wide agent-free windows, loaded rows on the replayed fixture; WP-60, WP-61b and WP-63 yield to lane B's critical path as WP-66/67 do (PLAN §4) |
| R16 | c | c+ | c+ | **amended** in PLAN §6.2 | A-M4's git-history input for WP-92 and WP-70 (+0.5–1 u lane B); the M0 pass is evidence for FL-1 and the model, not for FL-4, and the exit text says so; row 1 counts as an oracle check; renames matched by git's blob ids, never moirai `oid`s |
| R17 | c | c | c+ | confirmed; owner day-1 confirmation pending | `roots.toml` reviewed; `moirai-probes-bin`'s line cap given as a number (WP-01, WP-02) |
| R18 | c+ | c | c+ | **amended** in PLAN §6.2 | A1P-10's `std::fs` scan; tool crates that need `std::os::windows` get `osdeps-allow.toml` entries (WP-02) |
| R19 | c | c | c | confirmed; owner day-1 confirmation pending | The owner confirms the listed licences as "similar" (day-1 bundle item 7) |

## 4. Edits outside this work package's files

These texts now disagree with the design of record as amended. Each is superseded by the [AR]/[40]/[50] text named, and
is corrected with the owner's review at the step named; the chapters follow the amended text meanwhile.

| Text | What it still says | Superseded by | Corrected at |
|---|---|---|---|
| [80 §2.1] `os::project` | lists `journal_since`; omits `sync_dir` | [AR §4.10] (A1P-02, A1P-17) | WP-99 |
| [80 §2.1] boundary rules | `File::lock` and `std::fs::rename` "never used anywhere" | [AR §8.3] GT20 (d): product crates (A1P-10) | WP-99 |
| [80 §2.3.2] protocol-point table | no row for the recovery re-barrier | [40 §3.4] step 5 (A1P-02); chapter 16 lists it | WP-16 transcribes; [80] at WP-99 |
| [80 §2.11.4] rule 6 | "a file is copied → flushed → `oid` verified → deleted" | [40 §3.4] (A1P-01); the file-move protocol is [40]'s reservation | WP-99 |
| [80 §5.5], [90 §6.3] | combined header limits (≤ 60/100 B, ≤ 90/130 B) | [AR §7.1] per-part limits (A-M3) | WP-81a / WP-99 |
| [90 §8.3] quota paragraph and cost table | "M0's first usage window measures that per-call overhead" | [50 §7.4] item 5 (A-M5) | WP-81a, as the finding assigns |
| [60 §2.5] R-rows other than R-3 | pre-review R-1, R-4, R-6–R-8, R-10, R-11, R-13, R-14, R-16, R-17 text | [40 §2.11] (authoritative; PLAN §3.3 already routes chapters there) | WP-99 |
| PLAN §2.2 `moirai-vfs` and `moirai-os` rows | "the complete `ProjectFs`"; `Meter` without private bytes | PLAN §6.2 R2 as amended | plan author |
| PLAN WP-40, WP-52, WP-53d | vehicle rules | PLAN §6.2 R10 as amended | plan author |
| PLAN WP-32 | enumeration list | add "rename → crash → recovery → loss of unsynced names" (A1P-02) | plan author |
| PLAN WP-55 | rename "without the protocol's flags" | [60 §5.2] item 15 (A1P-07; `ChangeTime` for S-16) | plan author |
| PLAN WP-56 | measurement 19 inputs | [60 §5.2] item 19 (A1P-06) | plan author |
| PLAN WP-58, WP-71b, §5 V9 ask | overhead measured in WP-72's window; "≈ 53 M" in the ask | [50 §7.4] item 5 (A-M5): first real calls; smoke run re-issues the plan; the ask quotes it | plan author |
| PLAN WP-62 | acceptance | a writer racing the two passes (A1P-05); a caller-supplied byte source (A1P-10) | plan author |
| PLAN WP-70, WP-92 | model scope | PLAN §6.2 R16 as amended (A-M4) | plan author |
| PLAN WP-94 | GT18 rows | [60 §3.13] GT18 (A-m9) | plan author |

## 5. Chapter obligations created by the dispositions

| Chapter (WP) | Must specify |
|---|---|
| 05-log, 03/04 (WP-11) | `TreeReg` epoch payload (FB-1); `next_anchor` among the counters recovery derives (FB-7) |
| 06, 07, 12 (WP-12) | `aN` in the creating `AddEdge` (FB-7); `end_h`, anchor `pred`, `path` roots by name, the closed `relink` bytes and `pathmove.hlc` in item 10 (FB-4, FB-5, FB-6, FB-8, FB-9); the edge-complete re-key and its residue (FB-3); the ref-name grammar and refused literal-shaped segments (S-11); dst's text on equal named-query hashes (S-10); `affected_complete u8` (FB-11) |
| 09, 10, 11 (WP-13) | `TREES` epochs and retention (FB-1); `ANCHORRES` and `GITFACTS` retention and sizes (A1P-16); `ALLOC` + `UIDX` (FB-11); F3 portable text rule (FS-2) |
| 08, 18 (WP-14) | the uid derivations and predecessor rule (FB-2, FB-4); R-16's closed set with `none`, `unresolved` and the details (FS-1); R-17 (FB-6); I-F2, I-F8, I-F10, I-F14 as amended; the `policy` how token |
| 20 (WP-14b) | the clock-domain conversion and the skew-margin hole (S-17); the per-root `oid` algorithm (FB-10); S-16's decision in pass 1 |
| 14-image (WP-15) | anchor lines with `end_h`, `end`, `pred` (FB-4, FB-5); the query file's re-binding check (FS-2) |
| 15, 16, 17, 13 (WP-16) | the recovery re-barrier protocol point and its seeded bug (A1P-02); the refusal of cross-volume moves (FS-4); the never-sleep rule for hook and server settles (A1P-06); in chapter 17 the hole constraints of the leader and the checkpoint thresholds (A1P-03), `tx.max-work-in-lock` with its constraint (A1P-13) and the per-process-kind RSS gate (A1P-08), unless WP-18 places the latter in `config.md` |
| `os/` (WP-17) | `ProjectFs` with `sync_dir`, without `journal_since` or a copy op; `Meter` with `private_now`/`private_peak` |
| `config.md`, 19 (WP-18) | `fs` units as the read budget and `files.read-budget-ms` as a safety net (A1P-15, A-m5); header rules (FS-3); exit-5 texts (A-m4); JSON commit ids as `c` + 64 hex (A-m7) |
| `lq/` (WP-19) | the portable rewrite and E117's scope (FS-2); W10 and the `none`/`unresolved` values (FS-1); `links()` yields `next`; the `TX` re-validation trigger (S-06); the departures (S-21); W10 beside the chapter's W09 |
| fixtures (WP-21, WP-22, WP-71a) | a range anchor in both anchor-text modes (S-04); one `relink` case per value (A-M1); header goldens re-counted per part (A-M3) |

## Holes

The dispositions introduce one new hole and add constraints to three existing ones. No other value is decided here.

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| r4-clock-skew | the skew margin of [40 §4.3]'s clock-domain comparisons (chapter 20) | measurement 15 (file-system timestamp behaviour on the project volume, WP-55) and measurement 22 (the boot clock across sleep and clock steps, WP-52) | ≥ the measured file-system timestamp granularity plus the observed wall-clock step; the design gives no figure | every comparison stays on the conservative side of [40 §4.3]; a larger margin only turns re-binds into proposals, never the reverse |
| F17-ckpt-ops, F17-ckpt-bytes, F17-tail-overlay (existing, chapter 17) | checkpoint thresholds | measurement 10 (WP-53d) | as chapter 17 lists | open ≤ 3 ms at 1e6, loaded, measured **with records applied into a product-shaped overlay and the lazy-record share at the quiet cap** (A1P-03); M1's open gate re-checks |
| leader in or out (existing, WP-81a) | whether M1 builds the leader | measurements 1, 2 and T2 (WP-52, WP-53e) | out; in | out only if last-acknowledgement p99 ≤ 50 ms **at the maximum injected in-lock cost** (p99 5 ms, max 20 ms) (A1P-03); M1's gates re-check |
| `tx.max-work-in-lock` value (not an M0 hole) | the in-lock re-evaluation cap | M7's work-unit calibration | 5e5 units provisional | cap × calibrated ns per unit ≤ the hold p99 budget − the plain append cost (A1P-13); chapter 17 records the constraint |

## Open points for the review

1. **Owner confirmations.** (a) FB-11: [AR] wins over [50]'s own F16/F17 reservations, an exception to the precedence
   rule, because F17's row predates the audit amendment [72 M7] that [50 §5.9] already relies on (lens S asks the owner to
   confirm in pass 1). (b) FS-3 restates a TOKENS gate: the typical file-bearing header stays ≈ 100 B ([AR §7.1]'s example
   with a 10-byte label is 98 B), but the worst case per part is larger than the former combined limit, which no real
   worktree path could meet. (c) FS-4 removes an explicit-verb capability (lens P open point 3): lens A confirms the agent
   text "move it with a raw mv; links re-bind by evidence" in pass 1. (d) R1, R17 and R19 still need the day-1 bundle.
2. **Conflicts recorded, with the precedence applied.** `TX` re-validation: [50 §3.10] vs [AR §4.5] step 7 → [AR]
   (protocol, not an [50] reservation; S-06). RSS figures: [50 §5.12] vs [AR §8.3] → [AR] (A1P-08). `ProjectFs` surface:
   [80 §2.1] vs [AR §4.10] → [AR] (not an X-F item; A1P-02, A1P-17). Cross-volume file moves: [80 §2.11.4] rule 6 vs
   [40 §3.4] → [40] (the file-move protocol and R-14 are [40]'s reservations, and rule 6 defers to [40 §3.4] by its own
   words; A1P-01). No tree: [50 §3.8] (E302) vs [40 §4.3] (`unverified (no tree)`) → both, by context (S-07).
3. **S-01's accepted residue.** With the view-scoped dead set, a uid that an unmerged branch of the same store removed or
   deleted can be live on another branch and share its `#N` until a merge or sync re-keys it; a `deleted_elsewhere` flag
   may then show on the live file node. This is visible, store-independent and resolved by the re-key, whereas revision
   2's store-wide rule made two stores derive different uids for the same file. Lens S confirms in pass 1.
4. **S-05's design choice.** Totality alone (`none`) would not have fixed the finding, because `'none' <> 'ok'` is true;
   the warning W10 makes the construct hedged rather than silent. The alternative the finding offered (an error on
   comparing an absent built-in) was not taken, because it would be a data-dependent error. W10 is numbered after the
   error chapter's W09 (`lq/errors.md`), whose "a later version adds W10+" sentence WP-19 revises.
5. **A1P-04 against S-16.** A `partial` epoch never advancing `verified_at` is conservative for the copy rule's line 3,
   which alone reads `verified_at` today. S-16's candidate fix would read it in line 2 as well, where a lower bound is not
   conservative; pass 1 must decide S-16 with that in view (lens P open point 1, lens S).
6. **The `policy` how token** (A-M1) is a new frozen value introduced by this disposition for policy B's automatic strong
   re-binds, which [40 §9.2] decision 1 already rendered like accepted guesses without a provenance of their own.
7. **Not a finding, for the owner** (lens S open point 4): in `hash-only` mode an anchor's `scope` (Rust item paths,
   Markdown heading paths) still travels as text, because the `text-unavailable` sub-state resolves by scope. If a
   hash-only destination must carry no document text at all, `scope` needs the digest treatment of S-04, and such anchors
   would resolve by hint and window only. No change is made without the owner's decision.
8. **PLAN §3.3 gaps.** None is assigned to WP-80a; none is resolved here. **Coverage rows.** This file specifies no
   structure and adds no `COVERAGE.md` row; the rows whose chapters must carry the dispositions are R-1, R-3, R-4, R-6,
   R-7, R-8, R-10, R-11, R-12, R-13, R-14, R-16, R-17, R-18, F3, F16, F17, X-F5 (the recovery re-barrier point) and X-F9
   (the ref-name rule).
