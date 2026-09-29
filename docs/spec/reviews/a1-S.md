# A1 re-review, lens S: [40] and [50] revision 2, and the plan's resolutions R1–R19

| Field | Value |
|---|---|
| Title | A1 re-review of the file-link design [40] rev 2 and the query-language design [50] rev 2, lens S (semantics and correctness); confirmation of `docs/m0/PLAN.md` §6.2 R1–R19 |
| Status | draft, pass 1 pending (phase-0 review; every finding awaits its disposition in the chapter named in "Closes in") |
| Work package | WP-80a (`docs/m0/PLAN.md` §3.2 item 8), lens R-REV-S. The author of this file wrote none of the reviewed text (S5) and did not read the other lenses' reviews |
| Sources | [40] `docs/research/design/40-file-links-design.md` §0.1–§0.3, §1.2–§1.4, §2.1–§2.11, §3.1–§3.8, §4.1–§4.8, §5.1–§5.8, §6.1–§6.5, §7.4, §8.1–§8.3, §9.2, Review log (all entries). [41] `41-file-links-critique.md` §0–§6. [50] `50-query-language-design.md` §0.2–§0.4, §1.3, §2.1–§2.9, §3.1–§3.10, §4.1–§4.4, §5.1–§5.12, §6.1–§6.6, §7.2, §7.4, §8.1–§8.3, §9.2, §10.3, §12.1–§12.14. [51] `51-query-language-critique.md` §0–§8. [AR] `docs/ARCHITECTURE-RESEARCH.md` §3.1–§3.5, §4.2–§4.6, §5a.5, §5a.7–§5a.8, §5b.1–§5b.7, §5e.1–§5e.9, §7.1. [60] `60-roadmap.md` §2.5, §2.6, §3.1, §3.5, §3.7, §3.8, §3.13 (GT18, GT20). [80] `80-cross-platform-design.md` §2.1 (module table), §2.10 P11, §3 X-F9. [90] `90-harness-agnostic-design.md` §10.1. [21] `21-critique-semantics-correctness.md` §0 (the lens and its defect classes). `docs/m0/PLAN.md` §2.1–§2.4, §3.1–§3.3, §6.2 |

Severity: **blocker** (format v1 or the frozen surface cannot be frozen as written, no known fix inside the design), **major**
(a reachable wrong answer, lost write or cross-store divergence in frozen bytes or frozen semantics; the fix is known and
must be decided before the chapter it names is accepted), **minor** (underspecified or inconsistent text that a chapter
author must settle; no wrong answer if settled as proposed). Defect classes follow [21 §0]: WRONG ANSWER, DATA LOSS,
DIVERGENCE (two stores, two branches or the store and its image hold different truths with no conflict record),
UNDERSPECIFIED.

## 0. Verdict

**CHANGES REQUIRED — no blocker, 5 major, 19 minor.** Every blocker of [41] (B1–B4) and of [51] (B1–B3) has a resolution in
the revisions, and every major issue of both reviews is resolved in a form that holds under this lens. Two of the blocker
resolutions leave residues that break exactly the property they were meant to establish: derived file uids are not
store-independent and the dual-creation re-key re-points too little (S-01, residue of [41 B1]); the portable form of
named queries misses store-local constants spelled without `#` or `s` (S-02, residue of [51 B1]). The revisions also
introduce three new major defects in frozen bytes or frozen semantics: two different captures from one node into one
file derive the same anchor uid (S-03); the `end` quote of a range anchor is hashed as text, so a hash-only image cannot
reproduce commit ids (S-04); `link_state(n)` is absent for a node without anchors, so `link_state(t) <> 'ok'` silently
selects every unlinked node (S-05).

None of the findings needs an architectural change. Each closes inside a named M0 chapter before format v1 freezes, and
the revisions are otherwise consistent with [AR], [60], [80] and [90] (the conflicts found are recorded under "Open
points"). **M0 can build on them**: the OS layer, the `Vfs` seam and simulator, the toy log, the model core, LQ-3's
lexer and parser, FL-1's path, fold, `oid`, diff, scanner and similarity work can proceed now; the WPs listed in §6 must
carry the dispositions of the findings they depend on before acceptance.

**R1–R19: all 19 confirmed; 11 of them with conditions (§5).**

## 1. Method

1. Read [40] rev 2 and [50] rev 2 in full, [41] and [51] in full, and every section of [AR], [60], [80] and [90] listed
   in the header.
2. For every blocker and major issue of [41] and [51], checked that the revision's text produces the fixed behaviour,
   by tracing the reviewer's own scenario and one adjacent scenario through the revised rules (§2, §3).
3. Traced new scenarios against the three properties this lens owns: no silent wrong answer, no lost acknowledged write,
   no divergence between stores, branches or store and image (§4).
4. Checked cross-document consistency of every frozen item the two revisions own (R-1…R-18, F1–F18) against [AR §3–§5],
   [AR §7.1], [60 §2.5], [80 §3] and [90 §10.1].
5. Judged each PLAN §6.2 resolution against the design documents and against what M0's evidence can support (§5).

## 2. [41] resolutions under this lens

| [41] issue | Resolution in [40] rev 2 | Verdict |
|---|---|---|
| B1 derived-uid resurrection, machine dependence | exact-byte `origin_path`; predecessor by (generation, commit id); dead-uid re-derivation; merge re-key; `StatusFork` never resolved by presence; stored inputs (§2.3, §5.5, I-F14) | **resolved with residue**: machine independence holds; store independence and the re-key's scope do not (S-01) |
| B2 no ancestry source | in-process git reader of [60] M4 as a hard dependency; [60 §3.5] carries split chains, generation data and heads beyond the graph | resolved |
| B3 exact `oid` re-binds to copies | copy rule; E7 never on a first settle; basename tie-break removed; never-candidate list; I-F13 | resolved; one narrow residue for creation-time-preserving copies (S-16, minor, resolver constant) |
| B4 `PathPrefix` event in a state-diff canonical form | `path_moves` as a versioned add-wins set field; no item 11, no trailer ([AR §4.6]) | resolved; the entry's `hlc` needs a precise definition (S-20) |
| M1 case-only renames | `ok (spelling differs on disk)`; git's HEAD spelling; `git/case` | resolved |
| M2 wrong git line; two trees per branch | writer-tree predicate, I-F12, R-15 | resolved |
| M3 ancestry after patch integration | gate G1–G4, freshness rule, `observed_blob`, per-link escalation | resolved |
| M4 verbs in unbound trees | refuse (owner decision #13 / [AR §11] #20) | resolved |
| M5 path reuse renders `ok` | `replaced` state; creation-time condition rejected with evidence | resolved |
| M6 printed fixes accept guesses | `--expect`, `agent/*` until `--confirm`, `--repin --at` | resolved |
| M7 directory moves | E3d, `PREFIXEV` across passes, all-missing pack gate | resolved |
| M8 cloud roots | attribute gate, `unverified (cloud-only)`, `files.cloud` | resolved |
| M9 interim modes | FL-4 depends on M4; gate, freshness and bindings in M6 | resolved |
| M10 atomic-save races | quiescence re-check, p + suffix rule | resolved |
| M11 stale E3 after edits | measured 5.0 %; E8; hook `auto` | resolved (changed, justified) |
| M12 import without tree or objects | eligibility, `files: no tree bound`, bounded E6 | resolved; the LQ side disagrees on the value (S-07) |
| m1–m18 | adopted or changed as the Review log says | resolved; [41 m4]'s intent ("derive from the capture-time selectors") is only partly met (S-03) |

## 3. [51] resolutions under this lens

| [51] issue | Resolution in [50] rev 2 | Verdict |
|---|---|---|
| B1 store-local data in named queries | portable form: `#N` → `#u:`, `s<seq>`/prefixes → full ids, reflog refused (E117) | **resolved with residue** (S-02) |
| B2 R4 part contradicts [40] | [40 §6.5] adopted; "reads never write" with an `fs` budget; one binding per anchor; verb-only ops; MCP `query` | resolved; two value-level disagreements remain (S-05, S-07) |
| B3 set semantics | Cypher/GQL bags; endpoint pairs in quantified parts with N08; D11 | resolved; the interaction of distinct edges with quantified parts is unstated (S-21) |
| M1 revision lexing | `ref_seg = word(.word)*`; revision mode only in revision positions; token/AST fixtures | resolved; ref names of literal shape remain ambiguous (S-11) |
| M2 BFS hop bounds | walk bounds, layered frontier | resolved (S-21 for cyclic patterns) |
| M3 direction typing | reverse aliases, reading echo, N07, symmetric kinds | resolved |
| M4 Cypher-tolerance gaps | pattern predicates, E118, E102, coercions, W07 | resolved |
| M5 cursors vs runtime state | pinned and live cursors | resolved |
| M6 v1 grammar size | requirement trace, D13 | resolved |
| M7 other-branch ids; `affected`; reservations | `ALLOC` + N06; F16; F18 | resolved; F17's layout disagrees with [AR] (S-09) |
| M8 runtime anchors; RSS composition | `RuntimeScan`; one `mem` budget; composition rule | resolved in [50]; [60 §3.8] still states the withdrawn claim (S-23) |
| M9 `--ids` and pipes | byte page, stderr footer, exit 10 | resolved |
| M10 LQ-Bench cannot fix semantics | ablations, semantic remedies, real-session stratum | resolved |
| m1–m11 | adopted | resolved; m8's merge rule needs a tie-break (S-10); m11's parity needs fixed arithmetic (S-22) |

## 4. Findings

### 4.1 Summary

| Id | Severity | Class | Where | Closes in |
|---|---|---|---|---|
| S-01 | major | DIVERGENCE, WRONG ANSWER | [40 §2.3, §5.5, I-F14, R-3]; [AR §5e.2, §5a.7 R4 rows, §5b.5 rule 7] | WP-14, WP-12; WP-62, WP-92 |
| S-02 | major | DIVERGENCE (R3) | [50 §4.4, §2.2 rules 5–6, §3.2, F3]; [AR §5b.5 rule 7] | WP-19, WP-15, WP-13; WP-93a |
| S-03 | major | DATA LOSS | [40 §2.7, I-F3, R-3, R-4]; [AR §5e.2] | WP-14, WP-12; WP-64, WP-92 |
| S-04 | major | DIVERGENCE (R3) | [40 R-10, R-11, §2.7, §5.7]; [AR §4.6 item 10, §5b.2 rule 9] | WP-12, WP-15; WP-21 |
| S-05 | major | WRONG ANSWER | [50 §2.6, §3.3]; [40 §6.5] | WP-19, WP-14; WP-93b, WP-70 |
| S-06 | minor | UNDERSPECIFIED | [50 §3.10 item 3, §5.9 step 4] vs [AR §4.5 step 7] | WP-19, WP-16 |
| S-07 | minor | UNDERSPECIFIED | [50 §3.8, §6.3] vs [40 §4.3 step 0, §5.1], [AR §5e.4, §7.1] | WP-19, WP-14 |
| S-08 | minor | UNDERSPECIFIED | [40 §2.9, §3.8, R-16]; [AR §5e.3, §7.1]; [50 §10.3] | WP-14, WP-18 |
| S-09 | minor | UNDERSPECIFIED | [50 §8.1 F16, F17] vs [AR §4.3, §4.4] | WP-13, WP-12 |
| S-10 | minor | DIVERGENCE | [50 §4.4, §5.3, F3]; [AR §5a.7 R5 row] | WP-19, WP-12 |
| S-11 | minor | UNDERSPECIFIED | [50 §2.2 rule 6, §2.3 §5]; [80 §2.10 P11 (b), X-F9] | WP-12, WP-19 |
| S-12 | minor | UNDERSPECIFIED | [40 §8.3.2 P11 and the paragraph below it]; PLAN WP-77 | WP-14b, WP-16 (13-invariants); WP-92, WP-77 |
| S-13 | minor | UNDERSPECIFIED | [40 I-F10, P3]; [AR §5e.3, §5e.8] | WP-14 |
| S-14 | minor | UNDERSPECIFIED | [40 I-F14, §5.6] | WP-14, WP-12 |
| S-15 | minor | UNDERSPECIFIED | [40 §5.5 composite row 2, §8.3.2 P8]; [AR §5a.7] | WP-12, WP-14; WP-91, WP-92 |
| S-16 | minor | WRONG ANSWER (narrow) | [40 §4.3 copy rule line 2]; [80 §2.11.4 rule 1] | WP-14b, WP-55 |
| S-17 | minor | UNDERSPECIFIED | [40 §4.3 copy rule, E7, G4 window; §3.2 `planned`] | WP-14b |
| S-18 | minor | UNDERSPECIFIED | [40 §2.5 `oid`]; [AR §5e.2] | WP-14b |
| S-19 | minor | DIVERGENCE | [40 R-1]; [AR §3.1 `path`, §4.6 "Not hashed"] | WP-12 |
| S-20 | minor | UNDERSPECIFIED | [40 §2.4 `pathmove.hlc`] vs [AR §4.5 step 7] | WP-14, WP-12 |
| S-21 | minor | UNDERSPECIFIED | [50 §3.4 items 2 and 4, §2.8 departures, §3.7 item 2] | WP-19, WP-70 |
| S-22 | minor | UNDERSPECIFIED | [50 §5.5 "Ranking statistics", F12] | WP-19, WP-13 |
| S-23 | minor | UNDERSPECIFIED | [60 §3.8 exit] vs [50 §5.12 "RSS composition"] | WP-52, WP-99 |
| S-24 | minor | UNDERSPECIFIED | [50 §2.9 Q18 JSON] | WP-71a, WP-22 |

### 4.2 Major findings

#### S-01 — Derived file uids are not store-independent, and the re-key re-points only anchors (major; residue of [41 B1])

**Where.** [40 §2.3] bullets "The predecessor" and "Dead uids are never re-created"; [40 §5.5] existence rows; I-F14;
R-3. [AR §5e.2] ("chosen by the greatest (generation, commit id) — store-independent"), [AR §5a.7] R4 existence row,
[AR §5b.5] rule 7.

**Problem, four traces.**

- **A: the dead set depends on which refs a store holds.** Store P holds `lane/x`, on which file node
  U = uid(project, `docs/a.md`, ∅) was created after the fork and later `file rm`'d; `lane/x` is not merged. On `main`,
  an agent links `docs/a.md` for the first time. §2.3: U is "known to the store as removed … on some branch head and live
  on none", so registration derives U′ = uid(project, `docs/a.md`, U). Store Q — a colleague's store that imported only
  `main`, or P itself after `branch -D lane/x` — registers the same committed file on the same `main` state and derives
  U. When one store's image is imported into the other and merged, both sides created a live node at one exact path,
  neither side removed the other's uid, so no re-key applies and both land as `PathClaim`. §2.3's "gets the same uid …
  in another store", [AR §5b.5] rule 7 and [AR §5e.2]'s "store-independent" do not hold. "Branch head" is also undefined
  (tags, `merge/*`, `import/*`, `orphans/*`, a deleted ref still in the reflog), so the model (WP-92) and FL-1 (WP-62)
  cannot implement one rule.
- **B: (generation, commit id) is store-independent only at commit granularity.** On P's `main`, F1 is `removed` at `p`
  by commit c1; F2, registered at `p` later with predecessor F1, moves away by commit c2, so `p ∈ F2.aliases`. The next
  registration at `p` picks F2 (greatest (gen, id)) and derives uid(p, F2). P exports `main` at checkpoint granularity,
  the default ([AR §5b.7] gate 2); Q imports it into a fresh store, where c1 and c2 are folded into one import-checkpoint
  commit X. Both candidates' last change is X: a tie that §2.3 does not break, and any tie-break by state (uid order)
  may pick F1 and derive uid(p, F1) ≠ uid(p, F2) for the same file, path and state. The dual-creation `created` rule
  (least (generation, commit id)) has the same dependence.
- **C: the re-key moves too little.** On lane S, after `main` removed the old U, an unrelated file is registered at the
  same path and derives U again (lane S never saw the removal); a run on S records `produced → U`, a note on S mentions
  `#N_U`, and the artifact carries `implements → #decision`. Merging S into `main`, §5.5 re-keys S's node to U′ "with S's
  fields" and re-points "S's anchors" only. The `produced`, `mentions` and the artifact's out-edges added on S since the
  LCA stay on U, which on `main` is the removed old file: the run now claims to have produced the removed file (I14's
  read-back check runs against the wrong node) and the note's source turns `suspect` for no reason. WRONG ANSWER, silent.
- **D: the convergence claim is conditional.** "Re-keyed … to the uid a registration after the removal derives" holds
  only when U is the merged view's greatest predecessor at `origin_path`; if another node vacated that path later, a
  later registration derives uid(p, that node) ≠ U′ and the store holds two nodes for one file.

**Fix.**
1. Make registration's dead set a function of the registering view V (uids removed or engine-deleted **on V**). If the
   store-wide rule is kept for `#N` and marker hygiene, define its input exactly (live refs of kinds `main`, `work` and
   `plan`; tags, staging, import and orphan refs and deleted refs excluded), state that the uid is then a function of
   (V, that dead set), and add a merge rule that **unifies** — instead of raising `PathClaim` — two live nodes at one exact
   path, both created since the LCA, when one uid equals uid(root, `origin_path`, other uid): the derived one survives,
   with the same re-pointing as the re-key.
2. Order predecessor candidates by a key carried in versioned state at every image granularity (for example the greatest
   candidate uid, or a per-root versioned "vacated by" record), never by commit generation or id. Do the same for the
   dual-creation `created`, or declare `created` informational where checkpoint import has folded the creating commits.
3. The re-key re-points every edge incident to U that side S added since the LCA — `at` with its anchors, `produced`,
   `consumed`, `mentions`, `replaced_by` refs and the artifact's own out-edges — not only anchors; the sync residue
   carries it ([AR §4.6] already lists re-keys in residues).
4. Restate the convergence claim with its condition, and add P13 cases: a store that never held the removing branch;
   registration after a checkpoint-granularity import at a path with ≥ 2 predecessors; non-anchor edges on the re-keyed
   side.

**Closes in.** WP-14 (`08-data-model.md` uid derivations; `18-file-links.md` I-F2, I-F14), WP-12 (`12-vcs.md` merge rows);
gates WP-62 (uid derivations, predecessor order) and WP-92 (model R4).

#### S-02 — The portable form misses store-local constants spelled without `#` or `s` (major; residue of [51 B1])

**Where.** [50 §4.4] "Portable form" and "Image"; §2.2 rules 5–6; §3.2 "Type-directed coercion" and "Ids and numbers";
§2.8 (`(t:Task {id: 40})`); §4.2 (`unlink` expansion); F3. [AR §5b.5] rule 7, §5b.2 rule 9.

**Problem.** §4.4 rewrites "every `#N`" and "every `s<seq>` and every commit prefix", and the exporter asserts "no `#`
followed by a digit outside string literals and comments". But the binder, not the lexer, decides what a constant means
(§3.2), and several store-local constants carry no `#` or `s`:
- `TX { DEFINE QUERY hot() AS { MATCH (t:task {id: 40}) RETURN t } }` on store P, where #40 has uid V: §3.2 makes the
  integer a node, §4.4 does not rewrite it, the exporter's check passes, and store Q binds `40` to its own #40 — a
  different node, since Q's alias map gave V another number. The same for `WHERE t = 40`, `t IN [40, 41]`, `id(t) = 40`
  and an integer default of a `node` parameter (`param_decl` admits a `literal`).
- `WHERE t.rev = 4466`, `t.updated > 4400` and an integer default of a `rev` parameter: §2.2 rule 6 makes the integer a
  store sequence number, which §4.4 rewrites only when spelled `s4466`.
- Anchor handles: `WHERE a.anchor = 'a17'`. `aN` comes from `HEAD.next_anchor` and is store-local ([40] R-6); §4.2's own
  `unlink` expansion compares it, so agents will copy the form into definitions.
- Conversely, a back-quoted identifier containing `#1` fails the exporter's character check although it is portable.

The commit id stays reproducible (the text is copied verbatim), but the definition's meaning changes between stores —
the R3 break [51 B1] was about.

**Fix.** Rewrite on the bound AST, by type: every node-typed constant, whatever its spelling, becomes `#u:<32 hex>`;
every revision-typed constant becomes a full `c<64 hex>` id; an anchor handle in a definition is E117 (or a new
anchor-uid literal, if a definition needs one); the stored text is the author's text with exactly those tokens replaced.
The exporter and the importer check a definition by re-binding it and asserting that its bound AST holds no store-local
constant, not by a character pattern. F3's ABNF comment, E117's text and the two-store property test (§8.3) cover these
spellings.

**Closes in.** WP-19 (`lq/canonical-ast.md`, `lq/errors.md`), WP-15 (the `moirai-query 1` ABNF), WP-13 (F3's item);
gates WP-93a.

#### S-03 — Two different captures from one node into one file derive the same anchor uid (major; new, and residue of [41 m4])

**Where.** [40 §2.7] (`captured`, anchor uid, "Capture de-duplication", capture step 4), I-F3, R-3, R-4; [AR §5e.2].

**Problem.** `captured` = BLAKE3-128(file uid, kind, scope, `quote.exact`, `end.exact`, occurrence). It leaves out
`prefix` and `suffix`, although capture step 4 makes a duplicated quote unique by widening exactly those, and records
`occurrence` only when context and scope are still not unique.
- A developer links `#51 --at scripts/build.py:10` and `#51 --at scripts/build.py:42`; both lines read `return result`.
  Python has no scope scanner ([40 §2.7.1]), so scope is empty; widened prefix/suffix make each capture unique, so neither
  records `occurrence`. Both captures have the same `captured`, hence one anchor uid A. The second capture's current
  selectors (prefix, suffix, hint, window) differ from the first's, so de-duplication does not reuse the first anchor,
  and a second anchor with uid A on (#51, file) violates I-F3 ("anchor uids are unique per (src, dst)"). The edge key
  (src, `at`, dst, disc = A) holds one anchor: the second `link` is refused with no defined error, overwrites the first,
  or is a no-op — an acknowledged citation that does not land (DATA LOSS). Two `lines` anchors in one scope, and any
  heading or symbol duplicated inside one scope, fall into the same case.
- After a repin: a17 repinned from Q1 to Q2 keeps its `captured` by design; a later fresh capture of Q1 (the edit was
  reverted) derives a17's uid while a17's current selectors are Q2, with the same outcome.

**Fix.** Put into `captured` every selector capture used to make the anchor unique — `prefix.exact` and `suffix.exact`
as widened, and for `lines` the window — and give the anchor uid a predecessor term,
`anchor uid = BLAKE3-128(lp("moirai-anchor-v1") ‖ lp(src uid) ‖ lp(captured) ‖ lp(pred or empty))`, where `pred` is
empty unless the derived uid already exists on (src, dst) with different current selectors, in which case capture
re-derives with that uid as `pred` until the result is free (the file uid's shape: deterministic on a view, and two such
anchors merge add-wins by uid). Both are R-3/R-4 bytes and must be decided before FL-0.

**Closes in.** WP-14 (`18-file-links.md` anchor record; `08-data-model.md` uid derivations), WP-12 (anchor props in
item 10); gates WP-64 and WP-92.

#### S-04 — The `end` quote of a range anchor enters the canonical form as text (major; new)

**Where.** [40] R-10 (digest-only list), R-11, §2.7 (`end`; authoring form `path:L-M` → `range` for spans over 4 lines or
128 B), §5.7 "Anchor text"; [AR §4.6] item 10, [AR §5b.2] rule 9, [AR] I28′, I29′; [60 §2.5] audits' canonical-form row.

**Problem.** R-10 makes quote, prefix and suffix digest-only; `end`, the second quote of every `range` anchor, is part of
the selector block and therefore enters item 10 as text, and [AR §5b.2] rule 9's anchor line has neither `end` nor
`end_h`. A destination with `image.dest.<name>.anchor-text = hash-only` must then either carry the `end` text — the
source excerpt hash-only mode exists to keep out ([72 M6], [40 §9.2] #15) — or omit it, in which case the importer cannot
rebuild item 10, every commit that touches a range anchor fails its `Moirai-Commit` check and is demoted (I29′), and
I28′'s "the moirai commit id is a function of moirai data only" fails. If `end` carries its own prefix/suffix
(W3C RangeSelector), the same holds for them. PLAN §3.3 gives the anchor line's `end` field to WP-15 but not its
canonical treatment.

**Fix.** `end` (and any end prefix/suffix) enter item 10 only as BLAKE3-128 digests (`end_h`, …), carried on every anchor
line; the text appears only in `full` mode and is verified against its digest on import (`ImageParse` on a mismatch);
the `text-unavailable` sub-state covers `end`; gate 0 and GT8 in both anchor-text modes include a range anchor.

**Closes in.** WP-12 (`07-canonical-form.md`, R-10's selector block), WP-15 (`14-image.md` anchor line); fixtures in
WP-21 (`moi/`, `carrier/`).

#### S-05 — `link_state(n)` is absent for a node without anchors, so `<> 'ok'` selects every unlinked node (major; new)

**Where.** [50 §2.6] (`link_state(x)`, "or absent when it has none"; `a.state`), §3.3 (absent: `x <> v` is true), §7.4
item 2 (adversarial set); [40 §6.5], §4.5 (the anchor cascade runs only on a resolved file).

**Problem.** "Which tasks have broken links?" is naturally written `MATCH (t:task) WHERE link_state(t) <> 'ok' RETURN t`.
For every task with no `AT` edge `link_state(t)` is absent, and `absent <> 'ok'` is true, so the result lists every
unlinked task beside the broken ones, with exit 0 and no warning (W01 covers ordered comparisons only). The same holds
for `link_state(t) NOT IN […]` and for `a.state <> 'fresh'` when the file did not resolve (`missing`, `replaced`), where
the anchor cascade never runs. This is the valid-but-wrong class [51 L2] targets, introduced by the revision, and it
contradicts §0.4's "none is silent".

**Fix.** Make the built-ins total: `link_state(n)` over a node with no `AT` edge returns a frozen string (for example
`none`), and `a.state` over an unresolved file returns a frozen string (for example `unresolved`) — both added to R-16's
strings — or make any comparison of these built-ins with an absent operand an error that names `IS NULL`. Add the
construct to LQ-Bench's adversarial tags.

**Closes in.** WP-19 (`lq/std.md`, `lq/errors.md`), WP-14 (R-16 strings in `18-file-links.md`); gates WP-93b; WP-70
adds the adversarial task.

### 4.3 Minor findings

**S-06 — `TX` re-validation trigger narrower than [AR].** [50 §3.10] item 3 and §5.9 step 4 re-evaluate `MATCH` targets
under the lock only "whenever a commit landed since touched a kind, field or edge kind the `MATCH` reads". [AR §4.5]
step 7 also re-validates when a node, edge, **marker or lease** the candidate read changed. A `MATCH … WHERE t.ready`
(or `t.claimed`, `t.settled_elsewhere`) target is invalidated by a `Lease` record or by a marker from another ref, which
is not a commit touching a kind or field; read literally, [50] lets the block commit on a stale runtime predicate while
§3.10 claims `EXPECT` "always holds against the branch tip". *Fix:* WP-19 and WP-16 take [AR §4.5] step 7's trigger
(which wins: `TX` re-validation is protocol, not an [50] reservation), stated for runtime predicates explicitly.

**S-07 — No resolvable tree: error or value?** [50 §3.8] and §6.3: tree-derived built-ins with no resolvable tree are
E302. [40 §4.3] step 0, §5.1 and [AR §5e.4]/§7.1: an ineligible tree gives `files: no tree bound` and every link
`unverified (no tree)`. [50]'s own reason for E302 at past views (a value satisfies `<> 'ok'`) applies here too: after an
image import on a machine without the files, `links_broken` would list every link. *Fix:* one rule — for LQ built-ins an
ineligible or absent tree is E302 with the `--tree` hint; `unverified (no tree)` is a rendering of packs and `links
check` only.

**S-08 — The frozen string set is not closed.** `unverified` details differ: [40 §2.9] lists `budget`, `cloud-only`,
`commit not in this repository`, `no tree`; [40 §4.3] and [AR §5e.3] add `git: moirai links sync | moirai check`;
[AR §5e.3] adds `size`; [AR §8.3] writes `unverified (git)`. [40 §3.8]'s example headers omit `<n> rows` and `live`,
which [AR §7.1] freezes, and render commits as `c4472`, where the envelope uses `c<8 hex>`; [50 §10.3] says [40]'s header
"already matches". *Fix:* WP-14 enumerates the closed set of state, detail and header strings (R-16); WP-18 takes the
header grammar from [AR §7.1]; examples are non-normative.

**S-09 — Layout conflicts in F16 and F17.** [50 §8.1] F17: `ALLOC` = `#N → (ref_id u32, create_seq u32)`, 8 B per id;
[AR §4.4]: `ALLOC` = `#N → (uid, ref_id, create_seq)` plus `UIDX` (uid → `#N`), widened by [72 M7], and [50 §5.9] step 5
itself relies on `UIDX`. The precedence rule of this review round ("[50] for its own reservations") would select the
narrow form and break I1/I-F2's `#N` reuse. F16 says "a flag bit `affected_complete`", [AR §4.3] an `affected_complete u8`.
*Fix:* WP-13 takes [AR §4.4]'s widened `ALLOC` and `UIDX` (a later audit amendment of F17), WP-12 [AR §4.3]'s header
field; both record the conflict in their open points.

**S-10 — Named-query merge is not fully deterministic.** [50 §4.4]: a definition changed on both sides "to the same
canonical-AST hash is not a conflict", but the two texts can differ in bytes, and which text lands is not stated. The
canonical AST is the **bound** AST, and binding rewrites reverse aliases through F1, which is schema data that can
differ per branch; the hash then depends on which schema the merge binds against. *Fix:* WP-19 defines the canonical AST
over the portable parse with the grammar version's alias table (frozen per grammar version, [72 m3]), or binds against
the merge result's schema; WP-12's merge row takes dst's text (or the bytewise smaller) when hashes are equal.

**S-11 — Ref-name rule not unique.** [50 §2.2] rule 6: `ref_word = [a-z0-9_][a-z0-9_-]*`, lower-case ASCII only.
[80 §2.10] P11 (b) and X-F9 contemplate `lane/Foo` beside `lane/foo`, NFC input and fold-equal refusal, and say "LQ
`ref_word` stays as is". A ref segment of literal shape (`c0ffee12`, `s4400`) is a valid `ref_word` and a valid
`commit_lit`/`seq_lit` in a revision position, with no precedence. *Fix:* WP-12 freezes one ref-name grammar equal to
LQ's `ref_name` (P11 (b)'s fold rule then holds trivially) and refuses a ref segment matching `c[0-9a-f]{7,64}` or
`s[0-9]+`; WP-19 states that a revision token is a commit or sequence literal first.

**S-12 — "Subset-consistent" has no order.** [40 §8.3.2] P11 says every anchor state "agrees" with the model's
brute-force search; the paragraph below it and PLAN WP-77 say "subset-consistent: the same state, or a more
conservative one, never a different target". No order over {`fresh`, `moved`, `edited`, `ambiguous`, `orphaned`} × span
is defined, so WP-77 cannot decide pass or fail. *Fix:* define it where P11's oracle is specified: a state with a span is
never more conservative than one with a different span; `ambiguous` and `orphaned` are more conservative than any
span-bearing state; `edited` with span S is more conservative than `moved`/`fresh` with span S; P11 is subset-consistency.

**S-13 — I-F10's inputs are incomplete.** "`resolve(link, tree snapshot, resolver version)` is a pure function" (I-F10,
P3), but the cascade also reads the tree's runtime rows (`FILEOBS.verified_at`, creation time, `last_oid`, `PENDING`,
`FSINTENT`, `TREES` last settle and first-settle flag), cached git facts, and the `fs` budget, whose exhaustion yields
`unverified`. *Fix:* restate I-F10 over (versioned link, tree snapshot with its git objects, the tree's runtime rows,
resolver version, budget); caches are output-neutral; `unverified` is the only budget-dependent output. The model (WP-92)
takes the same tuple as input.

**S-14 — I-F14 versus history verbs.** I-F14 lets only `links fix --restore` and `Undelete` bring a removed or deleted
derived uid back; `revert`, `undo`, `cherry-pick` and `op restore` of the commit that set `removed` also do, because they re-apply
field and status values through the ordinary rules ([40 §5.6], [AR §5a.5]). *Fix:* list them as explicit, recorded doors in I-F14, or
make them stage; the model follows.

**S-15 — P8's "commutativity" versus directional rules.** [40 §5.5]: "both changed, same path → take dst's composite";
[AR §5a.7]: owner-authority fields are directional too. merge(A into B) and merge(B into A) then differ in `oid`,
`observed_git` and `relink`. *Fix:* define P8's commutativity as equality up to the listed directional rules, or pick the
same-path composite by a state order (for example the greater `observed_git` generation, then bytes).

**S-16 — Copy-rule residue for creation-time-preserving copies.** Copy-rule line 2 makes a near (E4) equal-`oid`
candidate exact when its creation time equals `FILEOBS.creation` and is unique; line 3 rejects a pre-existing copy only
when its creation time **differs**. A same-name copy in the parent directory made earlier by a tool that preserves
creation times, followed by deletion of the linked file, passes line 2 and re-binds silently to the pre-existing copy
([40] names the tool risk [I] but not this case). *Fix:* line 2 also requires evidence that q appeared after
`FILEOBS.verified_at` — its change time (Windows `ChangeTime`, ctime on Unix) after it — provided measurement 15 shows a
same-volume rename updates it (WP-55 adds the row); otherwise line 2 yields at most `identical copy`. R-14 is a
resolver-version constant, so the format is not reopened.

**S-17 — Clock domains in comparisons.** File-system timestamps, HLCs and git committer times are compared with no stated
conversion: copy-rule line 3 (`q.creation < FILEOBS.verified_at`, an HLC), E7 ("later than T's last settle", an HLC),
G4's window ("committer time ≥ the observation's hlc − 1 day"), `planned` binding ("creation time later than the
planning commit's time"). *Fix:* chapter 20 defines one conversion (HLC → ns as `(hlc >> 16) × 10^6`) and one skew margin
(a HOLE decided by measurement 15 and measurement 22's clock rows), each comparison taking the conservative side.

**S-18 — `oid` algorithm selection.** "SHA-256 when the repository's objectFormat is sha256" leaves named roots, `abs`
roots, stores without git, and roots in repositories of different formats undefined, while the copy rule and
`last_oid` compare `oid`s. *Fix:* chapter 20 makes the algorithm a per-root, init-fixed parameter (a git root: its
`objectFormat`; otherwise SHA-1), recorded in the `oid` tag; `oid`s of different algorithms never compare equal.

**S-19 — Canonical encoding of `path` values.** R-1 defines `path` as root symbol u16 + UTF-8 bytes; symbol numbers are
store-local and excluded from hashes ([AR §4.6] "Not hashed"). Item 10 must encode the root of every `path` and
`pathmove` value by name, and an artifact's `path` root must equal its `root` field (an invariant, or drop the
redundancy). *Fix:* WP-12's value encodings; WP-14 adds the invariant.

**S-20 — `pathmove.hlc` is not "the hlc of the commit that adds the entry".** [AR §4.5] step 7 re-parents a candidate
under the lock with a new header `hlc` over an unchanged `changeset_digest`, so a `path_moves` entry written in the
candidate keeps the candidate's HLC. *Fix:* define the entry's `hlc` as the writer's HLC at candidate computation, used
only for ordering; the importer and the model never check it against the commit's `hlc`.

**S-21 — Quantified parts versus distinct edges and cyclic patterns.** [50 §3.4] item 2 forbids two edge patterns of one
`MATCH` to bind the same edge, but item 4's endpoint-pair semantics binds no edge inside a quantified part, so a walk may
reuse the fixed part's edge (Cypher forbids it). §2.8's departure row says walks differ from Cypher only on "`RELATES`,
`CITES`, `MENTIONS` and similar historical kinds"; an undirected pattern or a mixed-kind alternation over DAG kinds is
cyclic too. *Fix:* state both in the departures table of the `lq/` spec; LQ-Bench's adversarial set tags them.

**S-22 — Ranking parity needs fixed arithmetic.** "Both tiers therefore return identical rankings on every view"
([50 §5.5]) holds only if the BM25 computation (f64 formula, summation order, rounding, tie by id) is specified, since
the tiers accumulate the same statistics in different orders. *Fix:* specify it if the ablation keeps BM25; moot if the
statistics-free scorer wins and `DOCLEN` leaves F12.

**S-23 — [60 §3.8] keeps a claim [50] withdrew.** M7's exit reads "default budgets keep the CLI ≤ 4 MB private at 1e5";
[50 §5.12] withdrew "≤ 4 MB at 1e5" for a composition rule and notes that [AR]'s own baseline (4 MB on `main` at 1e5, +1 MB
on a lane) already exceeds the gate. *Fix:* WP-52 (measurement 11) reports the CLI baseline at 1e5 on `main` and on a
14-day lane against the 4 MB gate and drafts the decision for WP-81a; WP-99's re-issue aligns [60 §3.8] with [50 §5.12].

**S-24 — A malformed example id.** [50 §2.9] Q18's JSON `changed_by.commit` is `c` + 63 hex digits (Q1 and Q6 carry 64).
*Fix:* goldens (WP-71a) and fixtures (WP-22) are authored from the spec text, never copied from examples.

## 5. PLAN §6.2 resolutions R1–R19

| # | Verdict | Reason under this lens | Conditions |
|---|---|---|---|
| R1 | **confirm** | Measurements 1, 2, 11, 12, 15 and 22 decide frozen values; taking them on the shipped calls rather than throwaway probes is what makes them evidence. [60 §3.1] "Not built yet" changes, so the owner confirms (day 1) | (a) WP-33 is accepted only after WP-80 pass 1 of the `os/` chapters, not only WP-80a; (b) M1's `Vfs` certification and M6's `ProjectFs` conformance (simulator, GT17) are not waived; (c) each measurement records the `moirai-os` commit it ran on, and a change to a measured call path before `format-v1` re-runs that measurement or records why not |
| R2 | **confirm** | One trait crate shared by simulator and OS layer means the in-process grant table the simulator exercises is the one that ships ([80 §2.2]); the complete `ProjectFs` from WP-30 avoids an interim trait | the frozen signatures include what [50 §5.10] and [AR §4.5] step 4 need for `mem` and `wmem` (the process's own private bytes, [80 §2.1] `os::mem::private_now`), so no later addition becomes an interim form |
| R3 | **confirm** | An independent decoder plus a test-only re-encoder is what E3 needs; the M1 codec decoder is [90 §10.2]'s plan | E3 at M0 covers structure and checksums over stored bytes only: content digests over decompressed bytes (for example `BLOBTAB`'s `blake3_16`, `SetBody`'s hash) are verified for codec-`none` fixtures at M0 and for real-codec frames when the oracle gains its decoder at M1; E3's wording says so |
| R4 | **confirm** | No shared type keeps S2's independence; `--json v1` is the frozen contract both sides must meet | the JSON data shape is total — every value the engine computes that a comparison needs, including row multiplicity and order, is in it — and WP-25 lists the excluded fields with a reason each |
| R5 | **confirm** | `span_hash` is xxh3-64 inside hashed anchor props, so the model needs xxh3; a hash crate is not a semantic dependency | known-answer vectors for xxh3-64, BLAKE3 and SHA-1/SHA-256 from their specifications are fixtures, so a crate regression cannot pass silently in both model and product |
| R6 | **confirm** | Generated, committed, pinned tables with an independent derivation in the model give two implementations of `fold_v1 = NFD(full_casefold(NFD(x)))` | "matches the UCD test data" is defined: `NormalizationTest.txt` for NFD, `CaseFolding.txt` statuses C and F (not S, not T) exhaustively, and agreement with the model's derivation over every scalar value; canonical combining classes and Hangul decomposition are in the tables |
| R7 | **confirm** | Host-only oracle and a separate fuzz workspace keep C out of every checked graph without weakening GT5 | — |
| R8 | **confirm** | Enforcing GT20 (d) earlier than [60] requires is strictly safer | — |
| R9 | **confirm** | No product binary exists at M0; a dormant lint self-tested on a fixture workspace is not an interim mode | — |
| R10 | **confirm** | The toy log implements the frozen group-commit protocol, so its measurements answer the protocol's questions; micro-benchmarks written from the spec are re-run on the product in M1–M3 | each decision drawn from the toy log or a micro-benchmark names the product re-run that re-validates it (M1 or M3 exit) |
| R11 | **confirm** | A permanent stub is a fixture, not a throwaway stage | — |
| R12 | **confirm** | Comparing the model's incremental and marker-cache rules with its own from-scratch definitions is what GT18 on the model can check at M0 | GT18 at M0 detects incremental-versus-definition divergence only; the definitions' correctness rests on the owner's signatures (V3) and GT10, and the from-scratch functions are written from the signed rule text |
| R13 | **confirm** | Text rendering at M0 must exist for LQ-Bench; binding M7/M8 to its outputs keeps one rendering | the spec text (`lq/envelope.md`, R-16 strings) wins over the goldens; goldens are reviewed against it in WP-80 pass 2 before they bind M7/M8 |
| R14 | **confirm** | No store exists at M0; files imported later are the same data | the result files carry every field [50 §7.4] item 8 puts on `measurement` nodes (model id, card version, Claude Code version, environment), so the import is lossless |
| R15 | **confirm** | S2 is a rule on roles, not lanes; none of the moved WPs is R-MODEL's, and R-MODEL may read no product crate (`moirai-diff` included) | — |
| R16 | **confirm** | At M0 no resolver exists; running rows 1, 3 and 4 through the model with git renames as data tests FL-1's functions and the model's rules | (a) the M0 pass of these rows is evidence for FL-1 and the model, not for FL-4, and the exit text says so; (b) row 1 at M0 is largely a plumbing check, because git's renames are both the evidence and the ground truth; (c) rename matching uses git's blob ids from the extraction, never moirai `oid`s ([41 m1]) |
| R17 | **confirm** | A Windows-only composition root outside the cross-target set keeps GT20 (e) meaningful for every shared crate; the owner confirms the amendment of [90 §11.1] | — |
| R18 | **confirm** | The scopes are at least as strict as GT20 (a) and (d) of [60 §3.13]; moving FL-1's git differentials to `moirai-replay` keeps [40 §7.4]'s "no R4 path spawns" true from the first product crate | — |
| R19 | **confirm** | Outside this lens; the listed licences are permissive and Apache-2.0-compatible, and the owner confirms them as AGENTS.md's "similar" licences | — |

## 6. What each finding gates in M0

| Finding | WPs whose acceptance waits for its disposition | Work that may proceed meanwhile |
|---|---|---|
| S-01 | WP-14 (`08`, `18`), WP-12 (`12-vcs`), WP-62 (uid derivations), WP-92 | path rules, `fold_v1`, `oid`, readers (WP-61, WP-62's reader and `oid` parts) |
| S-02 | WP-19, WP-15, WP-13 (F3), WP-93a | LQ-3's lexer and parser; the grammar fixtures (WP-22 `lq/`) |
| S-03 | WP-14 (`18`), WP-12, WP-64, WP-92 | anchor resolution parts that do not derive uids |
| S-04 | WP-12 (`07`), WP-15, WP-21 (`moi/`, `carrier/`) | `canonical/` fixtures without range anchors |
| S-05 | WP-19, WP-14 (R-16), WP-93b | WP-93a |
| S-06 … S-24 | the chapter named in "Closes in"; none blocks code that does not implement the named rule | everything else |

## Holes

None. This review fixes no value; the values its fixes mention (the skew margin of S-17, the change-time behaviour of
S-16) are holes of chapter 20, decided by measurements 15 and 22 (WP-55, WP-52), and are listed there when WP-14b takes
the dispositions.

## Open points for the review

1. **PLAN §3.3 gaps.** PLAN §3.3 assigns no gap to WP-80a; none is resolved here.
2. **Coverage rows.** This review specifies no structure, so it contributes no row to `COVERAGE.md`. The findings name the
   R-, F- and [60 §2.5] rows whose chapters must take a disposition: R-3, R-4, R-10, R-11, R-12 (I-F3, I-F10, I-F14),
   R-14, R-16 (S-01, S-03, S-04, S-05, S-08, S-12, S-13, S-14, S-16–S-18); F3, F16, F17 (S-02, S-09, S-10); R-1 (S-19);
   X-F9 (S-11).
3. **Conflicts recorded, with the precedence applied.**
   - `ALLOC`/`UIDX` layout: [50] F17 versus [AR §4.4] (S-09). The general rule ("[50] for its own reservations") would
     pick [50]; this review recommends [AR]'s layout, because it is an audit amendment of F17 that [50] §5.9 already
     relies on. The owner-facing disposition in WP-80 pass 1 should confirm the exception.
   - `affected_complete`: flag bit ([50] F16) versus byte ([AR §4.3]) (S-09); [AR] recommended, since the commit header's
     layout is [AR]'s.
   - `TX` re-validation trigger: [50 §3.10] versus [AR §4.5] step 7 (S-06); [AR] wins (protocol, not a reservation).
   - Tree-derived value with no tree: [50 §3.8] (E302) versus [40 §4.3]/[AR §5e.4] (`unverified (no tree)`) (S-07); this
     review recommends E302 in LQ and `unverified (no tree)` in rendering.
   - Header and state strings: [40 §3.8]/§2.9 versus [AR §7.1]/§5e.3 (S-08); [AR §7.1] for the header, WP-14's closed
     list for the strings.
   - CLI RSS at 1e5: [60 §3.8] versus [50 §5.12] (S-23); left to measurement 11 and WP-99.
4. **Not a finding, for the owner's attention.** In `hash-only` mode an anchor's `scope` (Rust item paths, Markdown
   heading paths) still travels as text, because the `text-unavailable` sub-state resolves by scope. Heading paths are
   prose from the owner's documents; if hash-only destinations must carry no document text at all, `scope` needs the
   same digest treatment as S-04 and `text-unavailable` anchors resolve by hint and window only.
5. **For the other lenses.** S-16's fix depends on whether an NTFS same-volume rename updates `ChangeTime`, and S-17's
   skew margin on clock behaviour across a sleep; both belong in measurement 15's and measurement 22's rows (lens P).
