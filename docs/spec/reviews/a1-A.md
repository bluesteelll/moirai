# A1 re-review, lens A: agent fit, tokens and buildability

| | |
|---|---|
| Status | review, WP-80a phase 0 (pass 1 pending for the chapters that close these findings) |
| Work package | WP-80a ([docs/m0/PLAN.md](../../m0/PLAN.md) §3.2 item 8), lens R-REV-A (method of [22]) |
| Targets | [40] revision 2 (answers [41]); [50] revision 2 (answers [51]); the resolutions R1–R19 of PLAN §6.2 |
| Sources read | [40] in full (§0–§10, Review log to the owner review of 2026-09-27); [41] in full; [50] in full (§0–§12.14); [51] in full; [AR] §4.5–§4.6, §7.1–§7.2, §8.3 TOKENS rows; [60] §3.1, §3.13 GT18, §4; [90] §8, §9.1–§9.2, §10.1, §11; PLAN §1–§8 |
| Coverage rows | none: a review file owns no [60 §2.5], R-n, F-n, X-F-n or [90 §10.1] row |

Severity follows the review convention: **blocker** (the design cannot be built or frozen as written), **major** (must be closed
before the WPs that rest on it are accepted; each has a named owner below), **minor** (closed in the owning chapter's open
points or by an editorial pass).

## 1. Verdict

[40] revision 2 and [50] revision 2 resolve every blocker and every major issue of [41] and [51] on this lens. No new blocker
was found. Five new **major** findings remain (§3). Four are gaps in bytes or texts that M0 freezes: a hashed provenance
vocabulary, a hashed `hlc` value, the header template and the reference model's input. The fifth is the LQ-Bench quota plan on
the M0 critical path. Each can be closed by a decision in a named chapter (WP-12, WP-14, WP-18) or by a PLAN edit (WP-58,
WP-71b, WP-92). Nine minor findings follow. All 19 resolutions of PLAN §6.2 are **confirmed**. R10, R12, R13, R16 and R17
carry conditions (§4).

Until A-M1 to A-M5 are dispositioned, WP-80a is not closed for lens A (PLAN §3.2 acceptance: zero open blocker or major).

## 2. [41] and [51]: are the blockers and majors resolved?

| Issue | Resolved? | Evidence in the revision | Residue (this review) |
|---|---|---|---|
| [41] B1 derived uids resurrect; machine-dependent | yes | exact-byte `origin_path`, predecessor by (generation, commit id), dead-uid re-derivation, merge re-key, `origin_*` stored for verification ([40 §2.3, §5.5], I-F14, P13) | — |
| [41] B2 no ancestry source; spawn fallbacks | yes | in-process git reader of [60] M4 is a hard dependency; read-path git caps in `fs` units; no spawn except `file mv --git` ([40 §4.3, §5.2, §7.4]) | — |
| [41] B3 exact `oid` re-binds to copies | yes | copy rule with unique creation time, never-candidate list, E7 never on a first settle, I-F13 ([40 §4.3, §4.4]) | — |
| [41] B4 `PathPrefix` event in a state-diff form | yes | `path_moves` versioned set on a root node; no op, no item 11, no trailer ([40 §2.4, §5.7]; [AR §4.6]) | A-M2: the entry's `hlc` definition conflicts with the O(1) re-parent |
| [41] M1 case-only renames | yes | `ok (spelling differs on disk)`; recorded only from τ(HEAD) or `file mv` | — |
| [41] M2 wrong git line; two trees per branch | yes | writer tree = designated tree on its expected ref; I-F12; R-15 | A-M3: the `reading only` note has two spellings |
| [41] M3 ancestry after patch integration | yes | gate G1–G4 on τ(HEAD) first; `observed_blob`; per-link marking | A-M4: the reference model has no git input for G1–G4 |
| [41] M4 verbs in unbound trees | yes (refuse, owner decision #20) | exit 5 outside the writer tree | A-m4: the exit-5 hint sends agents to an orchestrator ritual |
| [41] M5 path reuse renders `ok` | yes | `replaced` state | — |
| [41] M6 printed fixes accept guesses | yes | evidence command, `--expect`, `[accepted guess]`, `--confirm`, `--repin --at` | A-M1 (the provenance vocabulary behind `[accepted guess]` is open); A-m2 (`fix` column) |
| [41] M7 directory moves; prefix never fires | yes | E3d by parent-directory id; `PREFIXEV` across passes; all-missing pack gate | — |
| [41] M8 cloud roots | yes | attribute gate, `unverified (cloud-only)`, `--allow-hydrate`, `files.cloud` | — |
| [41] M9 interim modes | yes | gate, freshness and write rule in M6 itself; no spawn backend; DR13 | — |
| [41] M10 atomic-save races | yes | 50 ms quiescence re-check; p + suffix rule | — |
| [41] M11 stale E3 after agent edits | yes (changed) | E8 proposal; the edit-evidence hook `auto` (on with `mcp_tool` handlers) | — |
| [41] M12 import without files or objects | yes (changed) | tree eligibility; `files: no tree bound` | A-M3 (header byte limits) |
| [51] B1 store-local data in named queries | yes (varied) | portable form: `#N` → `#u:`, `s<seq>` → full commit id, reflog refused (E117); hash over the portable form | — |
| [51] B2 R4 parts contradict [40] | yes | [40 §6.5] is LQ's R4 part; live resolution charged to `fs`; one binding per anchor | A-m2; A-m5 (the `links check` budget) |
| [51] B3 set semantics change counts | yes | Cypher/GQL bags; endpoint pairs in quantified parts with N08; D11 | — |
| [51] M1 revision lexing | yes | revision mode only in revision positions; token/AST fixtures | — |
| [51] M2 BFS hop bounds | yes | walk lengths; layered frontier | — |
| [51] M3 direction blind on same-kind edges | yes | reverse aliases, reading echo, N07, symmetric kinds | — |
| [51] M4 Cypher-tolerance gaps | yes | pattern predicates, E118, E102, W07, coercions | — |
| [51] M5 cursors vs runtime state | yes | pinned and live cursors, W06 | — |
| [51] M6 v1 grammar too large | yes | requirement trace (§2.3.1); untraced productions are E004 | — |
| [51] M7 ids from other branches; `affected` completeness | yes | `ALLOC` (F17) and N06; `affected_complete` + u32 (F16); F18 | — |
| [51] M8 runtime anchors; RSS composition | yes | `RuntimeScan`, one `mem` budget from the measured headroom | (the baseline itself against the gate is an M0 measurement; outside this lens) |
| [51] M9 `--ids` in pipes | yes (paged in bytes by [90 §2.1]) | no row cap; byte page; footer on stderr, exit 10 | A-m8: LQ-8's test threshold is stale |
| [51] M10 LQ-Bench cannot fix semantics | yes | ablations for the silent-drift choices; semantic remedies; real-session stratum; per-construct gate | A-M4, A-M5 |

## 3. Findings

### 3.1 Major

**A-M1. The `relink` provenance vocabulary (R-17) is an open set, and `relink` is a hashed field.**
- *Where:* [40 §2.2] (the `relink` row), §2.11 R-17, §3.6, §3.7 (`--accept`, `--to`, `--accept-replacement`, `--confirm`), §4.3 "What settle writes" (`<trigger>/<evidence>`), §4.6 (`hook/argv`), §9.2 decision 1 (policy B); [50 §4.1] `links_guesses`.
- *Problem:* R-17 freezes "the `relink` provenance vocabulary of §2.2", but §2.2 lists examples of a grammar `how/evidence[/score]`, not a closed set. Values the text itself produces are missing from the list:
  - `owner|agent/replacement` (§3.7);
  - what `--confirm` makes of `agent/manual` or `agent/replacement` (only `confirmed/similarity/0.81` is listed);
  - the provenance of `--to PATH` ("as for `--accept`", but a manual path has no evidence or score);
  - `hook/argv` (§4.6), a prefix re-bind through E5 (no `lazy/prefix`), and an automatic strong re-bind under policy B;
  - the `<evidence>` tokens after `owner/` and `agent/` (`similarity` appears; `identical-copy`, `edited+moved`, `split`, `merged` and a git pair do not);
  - the decimal form of `<score>` (digits, rounding).

  `relink` belongs to the observation composite, a versioned field inside canonical item 10 ([AR §4.6]), so its exact bytes enter
  `commit_id`. The model (WP-92), the hex and canonical fixtures (WP-21) and the M6 engine must produce the same bytes. The
  `links_guesses` query (`relink STARTS WITH 'agent/'`) and the `[accepted guess]` marker also depend on the set.
- *Fix (WP-14, `18-file-links.md` R-17; recorded in its open points):* freeze a closed ASCII grammar:
  - a `how` set (`explicit`, `lazy`, `git`, `hook`, `journal`, `owner`, `agent`, `confirmed`, `merge-observation`, `merge-compose`);
  - one evidence token per E-source and per proposal class;
  - `manual` and `replacement` as tokens;
  - the score as exactly two decimals, rounded half-even, with no score when the evidence carries none.

  State which values `--confirm` maps and to what, and that `links_guesses` and `[accepted guess]` cover every `agent/*` value.
  WP-21's `r4/` fixtures carry one canonical case per value.

**A-M2. `pathmove.hlc` is defined as the adding commit's hlc, but a re-parent under the lock changes that hlc and keeps the changeset digest.**
- *Where:* [40 §2.4] ("`hlc` is the hlc of the commit that adds the entry. It is part of the value"), R-1; [AR §4.5] step 7 (a re-parent gets a new `hlc` and a header hash "over the unchanged `changeset_digest`").
- *Problem:* a `file mv` of a directory, a `links fix --prefix` or a settle that adds a `committed` or `observed` entry is
  computed in phase 1. With 16 writers it is often re-parented in phase 2. Then either the stored entry no longer equals "the hlc
  of the commit that adds it", or the digest must be recomputed, which contradicts the O(1) re-parent. The reference model
  never re-parents, so the two readings give different commit ids for the same history in GT2 from M1 on.
- *Fix (WP-12, `06-commit.md`/`07-canonical-form.md` with the R-1 value encoding; recorded in open points):* choose one rule:
  - the entry carries the hlc assigned when the adding commit's candidate was computed (phase 1), and a re-parent never changes
    it. Ordering needs only a stored deterministic value, which this gives;
  - or a re-parent recomputes the digest whenever the changeset adds a `pathmove` entry.

  Either way, the Store API's injected clock (WP-25) makes the model derive the same value.

**A-M3. The header template cannot meet the frozen header byte limits: `<tree>` has no display rule.**
- *Where:* [AR §7.1] (template and example), §8.3 TOKENS row "CLI result header ≤ 60 B without `files @`, ≤ 100 B with"; [50 §6.4]; [40 §2.9] and §5.1 (the `reading only` note); [50 §2.9] example headers.
- *Problem:*
  - [AR §7.1]'s own example `branch: lane/l5np | rev 4473 | live | 42 rows | files @ <lanes-dir>/l5np (u/l5np 7c1e0a, dirty 3 4m ago)` is 104 B with a 16-byte placeholder for the tree. The fixed parts alone take 88 B, leaving 12 B for the tree. A real absolute worktree path gives ≈ 110–140 B.
  - The reader-tree note adds ≈ 45 B. It is spelled `reading only: tree on <ref>, branch expects <ref>` in [40 §2.9], which R-16 freezes, and `· reading only: branch lane/l5np expects u/l5np` in [40 §5.1], which uses a non-ASCII `·`.
  - [50 §2.9]'s headers with view extras are 72 B (staged), 77 B (search), 79 B (diff) and 110 B (as-of with recomputation), all against ≤ 60 B.

  WP-18 freezes "header limits" at M0, and the M8 goldens gate them, so as written either the goldens fail or the contract
  changes after the freeze. The header is the first line of every result. It is paid in every agent context, and it is the D2/D3
  branch safety signal.
- *Fix (WP-18, `format/19-errors-and-output.md`; recorded in open points):* freeze three things:
  - a tree display label: the tree's path relative to the parent directory of the main worktree, or its registered short name. The full canonical path goes only to `--json` `tree` and `file where`;
  - one ASCII spelling of the reader note (R-16's), on line 2 or inside an explicit limit;
  - the fields each limit covers. The base fields (branch, rev, view flag, rows, files) are counted; view extras (search, diff range, composite parts, recomputation) get their own counted budget or a second line.

  Re-count every example of [AR §7.1], [40 §3.8] and [50 §2.9] in the goldens (WP-22's `lq/`, WP-71a's renderer).

**A-M4. The reference model has no git input, but LQ-Bench's link states and R16 need one.**
- *Where:* [60 §4.2] (file links resolve "over a simulated tree (`path → bytes` …)"), §4.3 (out of scope: "the git object layer"); [50 §7.4] item 1 (fixture links "in every [40] state"), §8.2 LQ-3 ("link states from a model of [40]'s resolver over a fixture tree"); [40 §4.3] G1–G4, E6, §5.3 (freshness, `main`'s committed-only rule); PLAN WP-92, R16.
- *Problem:* several [40] states need HEAD trees, ancestry and per-commit renames:
  - `pending`, `absent-in-tree (behind|diverged)` and `unverified (commit not in this repository)`;
  - every E6 re-bind (`git/r100`, `git/case`);
  - the freshness rule and `main`'s committed-only rule.

  As scoped, WP-92 builds a model without the gate. GT13's file-link stratum then has no gold result for those states. R16's
  rows 1 and 4 (git renames "as data") have no specified model input. The model is "complete at M0" and afterwards changes only
  through specification findings ([60 §4.1]), so it would be incomplete for R4 when GT2 needs it at M6.
- *Fix (PLAN WP-92 and WP-70; [60 §4.3] wording at WP-99):*
  - The model takes git history as abstract data: commits with parents, committer times and `path → blob id` maps, and one HEAD per simulated tree.
  - It implements G1–G4, E6 and the writer-tree and freshness rules by definition.
  - [60 §4.3]'s exclusion reads "the git object layer's bytes".
  - WP-70 generates every [40] state from that data.
  - Record the +0.5–1 u in lane B (the critical lane) for WP-99.

**A-M5. The LQ-Bench quota plan does not reproduce from its inputs, and its main unknown is measured late on the critical path.**
- *Where:* [50 §7.4] item 5; [90 §8.3] (plan of record and cost table); [60 §3.1] item 10; PLAN WP-58, WP-71b, WP-72, §5 (the V9 ask).
- *Problem:*
  - The plan is 9.5 full-run equivalents of 520 prompts at ≈ 2.3 calls and ≈ 7k input tokens per call. That is 4,940 × 2.3 × 7,000 ≈ **79.5 M raw input tokens**, against the stated ≈ 45 M (Opus) and ≈ 53 M in all. The 45 M appears to count cache reads at a list-price weight, a unit the subscription quota need not use.
  - Neither figure includes Claude Code's own system prompt or the definitions of the tools it keeps, which repeat in every call.
  - The plan measures that overhead in "M0's first usage window", which is WP-72's window: weeks 6–10, on the critical path, after the owner has been asked for 53 M (V9).
  - The card (3,013 B) is ≈ 840–1,120 Claude tokens at [90 §9.1]'s 3.6 (prose) to 2.69 (code) bytes per token, which straddles the ≤ 1,000-token gate. It is also first measured in WP-72.

  A 2–4× quota miss discovered at WP-72 costs whole quota windows on the path that sets M0's exit.
- *Fix (PLAN WP-58, WP-71b; [50 §7.4]/[90 §8.3] wording at WP-81a):*
  - WP-58's first real calls record the input, cache-read and cache-write tokens of an empty benchmark turn, with the card appended and the benchmark server attached. Built-in tools are removed where the pinned Claude Code allows it, not only denied.
  - The same calls record the card's Claude token count by the with/without delta.
  - WP-71b's smoke run re-issues the quota plan in raw and cache-read tokens, and the V9 ask quotes that figure.
  - A card over the gate is then fixed before the baseline runs.

### 3.2 Minor

| # | Where | Problem | Fix |
|---|---|---|---|
| A-m1 | [40 §6.2] marker table; [AR §8.3] TOKENS "Link marker ≤ 50 B per non-`ok` link" | Five of the ten markers exceed the gate by design: accepted guess +65 B, ambiguous +40–70, stale-anchor +50–90 (the `was:` quote), pending +60, diverged +60 | Before M9: cap the `was:` quote (e.g. 24 B plus `...`) and shorten `pending`/`diverged`, or restate the gate (≤ 50 B median, ≤ 90 B max). Record in WP-14's open points, since detail strings are R-16 |
| A-m2 | [50 §2.6] `links()` yields `fix`; [50 §6.4] `links` shape "the one-command fix"; [40 §6.1] JSON `next`; [40 §6.2]/[41 M6] | A column named `fix` invites an agent to run it, and the shape text reads as an accept command, which [41 M6] removed | Rename the column `next` and word the shape as "the next command (evidence or settle, never an accept)". WP-19 (`lq/std.md`) freezes it |
| A-m3 | [40 §3.8] examples; §3.7 `[accepted guess c… · confirm: …]` | They predate the frozen envelope: no `<n> rows` or `live`; writes lack `-> new \| committed`; long `-> verify: moirai file where …` markers instead of `\| verify #N` plus the legend line; non-ASCII `…`/`·` | WP-18/WP-22 take [AR §7.1] and [50 §6.4] as normative for layout and treat [40 §3.8] as illustrative; record in open points |
| A-m4 | [40 §3.8] `error[not_writer_tree]` hint | It prints `moirai worktree bind <dir> lane/x` to the agent. Binding is an orchestrator ritual ([AR §7.1]); following it gives exit 5 again (I-F12), demotes the lane's tree with `--replace`, or records the move on another branch | The agent-facing text names only the raw-`mv` fallback ("links re-bind when the code reaches …") or "ask the orchestrator"; the bind line is printed for the orchestrator and owner roles. WP-18 owns exit texts |
| A-m5 | [40 §4.2] (`links check`: "default none for explicit"); [50 §4.1] (`links_broken`, class `medium`), §5.10 (`fs` 400 default, 10,000 agent maximum) | `links check` renders `std.links_broken`, so an agent's `links check --all` at 1e4 files and 3e4 anchors (≥ 4e4 `fs` units) ends at exit 10, and [40 §7.4]'s `--all` gate is not reachable by an agent. `--budget-ms` and `--budget fs=` are two unreconciled controls | WP-19/WP-18 state the verb's budget: verb-invoked `links check`/`sync` use the `files.*` time budgets with an `fs` ceiling at the orchestrator's level, and `--budget-ms` maps onto `--budget` |
| A-m6 | [40 §6.3] and [AR §7.2] `write` named mutation behind `link --at` (`node, spec, watch, planned`) | There is no quote-text parameter, so Bash-less roles (architect, critic, researcher) cannot create the `--quote-file` form, which the skill teaches for Cyrillic headings | Add `quote` and `end` string parameters (JSON strings carry no shell hazard); keep the U+FFFD refusal |
| A-m7 | [50 §2.9] JSON examples | `commit` is 65 characters (`c` + 64 hex) in Q1 and Q6, and 64 in Q18's `changed_by.commit` | WP-18 freezes one rule: JSON carries 64 lower-case hex without the display prefix; text uses `c<8 hex>` |
| A-m8 | [40 §6.5] ("Revision 2 of this design had specified a cached `link_status`"); [40] Review log, "Other corrections" bullet 2; the `*End of 40-file-links-design.md.*` line before five more log entries; [60 §2.2] C6 → C7 edge (`link_status()`); [50 §10.3] (four presets, six exist); [50 §8.2] LQ-8 gate ("`--ids` pipe test with > 500 ids") | Stale text after the integration; the LQ-8 threshold predates the 24,000-B `--ids` page | Chapter authors follow the integration entries. WP-81a/WP-99 correct the text with owner review. LQ-8's test crosses the byte page (> 24,000 B of ids) and checks the stderr footer and exit 10 |
| A-m9 | [60 §3.13] GT18 row "M0 (model)"; PLAN WP-94 | Settle concurrency and lease liveness are listed for the model at M0, but the model is single-process ([60 §4.1]) | WP-94 states what each M0 row asserts (e.g. the CAS-drop and TTL rules over interleavings the model enumerates) and that the concurrent variants start at M2/M6 |

## 4. PLAN §6.2 resolutions R1–R19

| # | Verdict | Reason (lens A) | Condition |
|---|---|---|---|
| R1 | **confirm** | Measurements 1, 2, 11, 12, 15 and 22 decide product parameters (lock waits, the `BootId` source, the environment guard, R4 copy-rule facts). Run on a probe OS layer, they would rest on throwaway code, which the owner ruled out. Building `moirai-os` for Windows once, to final spec, moves 3.5–5 u from M1/FL-2 without adding any | WP-17's `os/` spec, the complete `ProjectFs` included, has passed WP-80 pass 1 before WP-33 is accepted. The owner confirms the scope change on day 1 |
| R2 | **confirm** | One seam crate lets the simulator, `moirai-os` and the probes share one trait set with no interim form. `Meter` is not probe-only: the product reads private bytes for LQ's `mem`/`wmem` defaults ([50 §5.10]) and free space for its guards | — |
| R3 | **confirm** | The codec is undecided until WP-81a, so opaque payloads keep the oracle from writing a codec twice. Structure and checksums still get byte-exact re-encoding; the codec decoder joins at M1 ([90 §10.2]) | — |
| R4 | **confirm** | A shared Rust type would couple model and engine and break S2. Comparison over `--json v1` data is the surface agents see anyway | WP-25 fixes every result field name and order, and the excluded fields |
| R5 | **confirm** | `span_hash` is xxh3-64 by definition. A leaf hash crate is not shared logic, and the model otherwise needs its own xxh3 | [60 §4.6]'s list is amended in the WP-99 re-issue |
| R6 | **confirm** | Committed generated tables give a reviewable diff and no runtime Unicode dependency. The model's independent derivation from the UCD text is the cross-check | UCD 17.0.0 download in the day-1 bundle |
| R7 | **confirm** | It keeps C out of every checked graph while keeping the tree-sitter oracle and libFuzzer ([90 §11.1]) | tsoracle's grammar version is pinned in `tools.md` |
| R8 | **confirm** | M0 already writes the only OS crate, so enforcing (d) now is only a lint and prevents rework at M1 | — |
| R9 | **confirm** | [60 §3.1] builds no product binary at M0; a skipped check that self-tests on a fixture workspace fails closed once `moirai` appears | — |
| R10 | **confirm** | The toy log runs the frozen group-commit protocol through the real writer and flush bytes, the closest vehicle before M1 | M1's exit re-runs measurements 1, 2 and T2 on `moirai-store` and revisits "leader in or out" and the lock waits if a value moves. PLAN states re-runs only for the WP-53 micro-benchmarks |
| R11 | **confirm** | One hand-written dual-era loop serves the Codex probes, measurement 19 and LQ-Bench's server; permanent, so not throwaway | — |
| R12 | **confirm** | Before any engine exists, checking the model's marker-cache rules against its own state definition is the only way to test the rule tables the owner signs | A-m9 |
| R13 | **confirm** | GT13 must show agents the exact texts M7/M8 will print, or it measures another surface, so a renderer is needed at M0 and its goldens must bind | Goldens trace to `lq/envelope.md`, `19-errors-and-output.md` and R-FIX's `fixtures/lq`; a renderer–spec disagreement is a spec finding. It implements A-M3's header rule |
| R14 | **confirm** | Files at M0 and import at M2 keep the data without an engine | WP-72's result schema carries what a `measurement` node needs: metric, value, unit, model id, Claude Code version, card version, grammar version, commit |
| R15 | **confirm** | It balances the lanes (42.5–59.25 u and 41.75–59.75 u). Lanes are calendars, not roles, so S2 is unaffected | Lane B holds the critical path (R-MODEL → R-BENCH → GT13), so WP-60, WP-61b and WP-63 yield to it as WP-66/67 do |
| R16 | **confirm** | It fills a real gap in [40 §8.3.4]. At M0, rows 1, 3 and 4 test the model's exact-evidence rules and FL-1's `oid` and path functions against real history, and the product re-runs all eight at M6 | Depends on A-M4 (the model's git input). Row 1 with renames given as data is near-tautological apart from identical-blob groups; count it as an oracle check, not as FL-1 evidence |
| R17 | **confirm** | Stricter than listing the probes as host-only: the root keeps GT20 (b) on all four targets plus a Windows `-p` check. It is the minimal amendment of [90 §11.1] | `roots.toml` is reviewed; `moirai-probes-bin`'s line cap is given as a number; the owner confirms the amendment on day 1 |
| R18 | **confirm** | With FL-1's git differentials in `moirai-replay`, product crates spawn nothing at M0, so the scopes cost nothing and match [60 §3.13] | Tool crates that need `std::os::windows` (e.g. process-creation flags in `moirai-tokcount`) get `osdeps-allow.toml` entries |
| R19 | **confirm** | Every listed licence permits redistribution under Apache-2.0. NCSA stays inside `fuzz/`, outside every shipped graph | Shown to the owner (day-1 bundle item 7) |

## 5. Holes

None. This review fixes no value.

## 6. Open points for the review

1. A-M1 to A-M5 need a disposition from their owners (WP-14, WP-12, WP-18, R-HARN-M/R-BENCH for PLAN, R-MODEL for PLAN) before
   WP-80a closes. A-M4 and A-M5 are PLAN edits, which R-SPEC makes with the owner, because PLAN.md is outside every role's
   write set.
2. R1 and R17 still need the owner's day-1 confirmation (PLAN §8 item 5). This lens confirms both on their merits.
3. The ≈ 53 M quota figure in the V9 ask should not go to the owner before A-M5's measurement, or it should go with its unit
   and the missing overhead stated.
