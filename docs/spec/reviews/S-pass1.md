# Review pass 1, lens S: the specification

| Field | Value |
|---|---|
| Title | Independent review pass 1 of `docs/spec/` (format chapters 01–20, `os/`, `lq/`, `store-api.md` with its examples, `config.md`, `COVERAGE.md`, `HOLES.md`) and `docs/spec/rules/*.md`, lens S (semantics and correctness) |
| Status | review (pass 1); every finding awaits its disposition by the chapter owner named in "Fix" |
| Work package | WP-80 ([PLAN §3.2] item 8), lens R-REV-S. The author of this file wrote none of the reviewed text (S5) and did not read the pass-1 reviews of lenses P and A |
| Sources | Every file of `docs/spec/` and `docs/spec/rules/` (see §1 for the depth of each); `docs/spec/reviews/a1-dispositions.md` (FB-1–FB-11, FS-1–FS-4, §5 obligations, open points 1, 3, 5); `docs/spec/reviews/a1-S.md` (S-01, S-16); [21 §0] (the lens and its defect classes); design sections cited per finding |

Severity, as WP-80 defines it for this pass:

- **blocker**: two independent implementers would produce different bytes, or a rule of the design of record (including a
  signed A1 disposition) is violated;
- **major**: an ambiguity, a missing case or an inconsistency that must be fixed before the freeze;
- **minor**: editorial, or a local inconsistency with no byte or rule consequence once settled as proposed.

Defect classes follow [21 §0]: WRONG ANSWER, DATA LOSS, DIVERGENCE (two stores, two branches, the engine and the model, or
the store and its image hold different truths), UNDERSPECIFIED.

## 0. Verdict

**CHANGES REQUIRED: 17 blockers, 14 majors, 18 minors (49 findings).** Pass 1 cannot close for the format chapters, the
rule tables and [F19 §8.3] until every blocker below is disposed.

The chapters are individually careful: every offset table I recomputed sums to its stated total, every worked example I
re-derived byte by byte is correct (§5), every protocol rule of [F16] has a seeded bug, every invariant of [F13] has an
enforcement point and a gate, and `COVERAGE.md` maps all 152 rows with no UNMAPPED cell. The defects are almost all
**between** chapters that were drafted in parallel and never reconciled, and they cluster in four places:

1. **One structure, several byte layouts.** Values, edge property blocks and anchor records are laid out differently in
   [F06] (ops) and [F08] (stored rows), and [F09]'s `EDGE_PROPS` cannot hold what [F07] hashes (S1-1, S1-2, S1-3). The
   log records of [F05] and the folded rows of [F11] disagree on enumerations and fields for nine record kinds
   (S1-8), [F18] and [F11] number the link states differently (S1-9), and a binding has three encodings (S1-10). Several
   chapters record these conflicts as open points; none is resolved.
2. **Rule tables not updated after the dispositions.** `RK-006` still re-keys anchors only (FB-3, S1-14); `MR-002` and
   [F13]'s I31′ contradict [F12]'s recursive-virtual-base rules (S1-15); the I26′ definition and its marker cache differ
   between [F13 §4] and `rules/state-definition.md`, and [F11]'s `MARKERS` row fits neither completely (S1-16).
3. **The protocol needs bytes the log does not define.** [F16] appends a reservation record and a `park` ref move that
   [F05] declares invalid, so a conforming reader would exit 7 on them (S1-11).
4. **Hashed inputs that are not determined.** The scope scanners that produce `captured` are unspecified (S1-4); [F16]'s
   HLC rule makes the hashed `hlc` depend on maintenance and lazy-record timing (S1-13); the `prov` byte [F12] requires
   is absent from [F06] and [F07] (S1-5).

None of the findings needs an architectural change. Each closes inside the chapters named, and most have a fix already
proposed in some chapter's open points; the fixes below pick one alternative so that pass 2 can check it.

**Decisions lens S owes from the A1 re-review** are in §2: S-16 is decided (with a constraint added to a hole, S1-31);
S-01's residue is confirmed conditionally on S1-14; FB-11 is confirmed from the correctness side.

## 1. Method

1. Read in full: [F01]–[F12] (every line), [F13], [F16], and `a1-dispositions.md`. Read by section: [F14] (§2–§12, open
   points), [F15] (§2–§3, §5, open points), [F17] (§2–§4, Holes, open points), [F18] (§3, §4.2–§4.3, §4.10, §5), [F19]
   (§7, §8.3, §12), [F20] (§2.3, §2.7, §5.9, open points 15, 27, 30), `store-api.md` (section map, §7.2, open points),
   `config.md` (§4.2 and the key registry rows used by [F05] and [F17]), `rules/merge-table.md` (MR-001–MR-005,
   SL-027–SL-032), `rules/link-merge-rules.md` (RK rows), `rules/state-definition.md` (MF, ME rows, open points),
   `rules/delete-policy-matrix.md` (EG rows of `blocks` and `answers`), `rules/status-machines.md` (gates rows),
   `COVERAGE.md` and `HOLES.md` (in full). For the `os/` and `lq/` files and the remaining rule files I read every open-point
   list and checked, by search, every value that a format chapter shares with them (lock offsets, `ProcId`, `OsFileId`,
   `VolumeCaps`, the swap intent, probe file names, the clock of the deletion grace, the `ref_name` grammar, W10, E117, the
   idempotency-key framing, the target-set digest, commit-id text forms).
2. Parsed every JSON example of `store-api/examples/` (all 20 parse; commit ids in results are `c` + 64 hex; the
   `c<8 hex>` values in 09 and 20 are revision arguments, which [F12 §3.2] allows).
3. Scanned every `HOLE(...)` of `docs/spec/` against `HOLES.md` (60 distinct ids; all indexed, aliases included).
4. Recomputed every offset table and example listed in §5.
5. For every pair of chapters that lay out the same datum, compared the bytes field by field. This produced most
   findings.

## 2. Decisions owed by lens S

**D-1. S-16 (deferred to pass 1 by the dispositions).** [F20 §5.9] adopted the ChangeTime condition for copy-rule line 2
(ChangeTime clearly after V) and records its residue in its open point 15: V is a lower bound of the last verification
(A1P-04's `partial` epochs never advance it), so a creation-time-preserving copy made between V and the original's
deletion still passes line 2 and re-binds silently. The residue is not closable by any condition on V: the resolver cannot
observe the deletion time. The alternative of a1-S ("line 2 yields at most `identical copy`") removes it at the price of
every same-volume move being a proposal on Windows. **Decision:** keep [F20 §5.9] as written, and make the residue
unreachable by measurement instead of by rule: HOLE(F20-btime-ntfs) must gain the candidate "some in-scope tool copies
creation times", under which NTFS volumes take a class whose line 2 never applies (like `CopiedByClones`), and
measurement 15 must test the copying tools an agent machine actually runs (S1-31).

**D-2. S-01's accepted residue (dispositions open point 3).** Confirmed: a view-scoped dead set that lets another branch's
removed uid be live here until a merge re-keys it is visible, store-independent and resolved by the re-key; I1 still holds
because one uid keeps one `#N`. The confirmation is conditional on the re-key being edge-complete as FB-3 requires, which
the signed rule table does not yet say (S1-14).

**D-3. FB-11 (dispositions open point 1 (a)).** From the correctness side the exception is required: I1 and I-F2 need the
uid in the store-wide `ALLOC` across unmerged branches, which [50] F17's 8-byte row cannot give. Lens S asks the owner to
confirm [AR §4.4]'s widened `ALLOC` + `UIDX` as [F11 §9] specifies them.

**D-4. [F17] OP-17-15 (the `suspect` budget).** The reading of [F17 §8.2] is correct: the bitset stays complete (I9),
`affected` omits only `suspect`-only changes with `affected_complete = 0` (I42′), and the hint `SuspectBudget` of
[F19 §12.3] reports it. [AR §2.5]'s "a violation record is written" should be edited at WP-81a.

**D-5. [F12] open point 2 and [F13] OP-13-04/OP-13-05** are decided inside S1-15 and S1-16.

## 3. Findings at a glance

| Id | Sev. | Where | Finding |
|---|---|---|---|
| S1-1 | blocker | [F06 §5], [F08 §5–§6] | Two value encodings: tags, `commitref` width, `pathmove` class codes, empty values, `f64` infinities, set order |
| S1-2 | blocker | [F06 §7.5.2], [F08 §10.2], [F09 §7.2] | Two edge property blocks; `EDGE_PROPS` loses `flagged` and 16 bytes of every hashed pin |
| S1-3 | blocker | [F06 §7.5.3], [F08 §10.3] | Two anchor-record layouts; `captured` hashes `scope` bytes the two define differently |
| S1-4 | blocker | [F20] open point 30, [F08 §10.3.1] | Scope-scanner grammar unspecified: `captured`, anchor uids and commit ids undetermined |
| S1-5 | blocker | [F12 §6.3] vs [F06 §6.2], [F07 §7.3] | The `prov` byte of existence conflicts exists in one chapter only |
| S1-6 | blocker | [F07] OP5, [F14 §11.2] vs [F06 §7] | A tombstone landing from the absent state has no stored op |
| S1-7 | blocker | [F11 §10] | `CONFLICTS` sides cannot hold existence, status, body, edge or schema conflicts |
| S1-8 | blocker | [F05 §9] vs [F11 §3–§12] | Nine record kinds disagree with the rows they fold into |
| S1-9 | blocker | [F18 §4.2, §4.10] vs [F11 §12.5, §12.13] | Link-state and evidence codes differ between owner and rows |
| S1-10 | blocker | [F18 §3.2] vs [F11 §5] vs [F05 §9.3] | Three encodings of a binding; short versus full git ref |
| S1-11 | blocker | [F16] P-70, P-84 vs [F05 §7, §9.2] | Protocol records that [F05] declares invalid (kind 27, reason 5) |
| S1-12 | blocker | [F17 §2.1–§2.2] vs [F04 §4.4] | `InitParams` bytes 16–31: reserved-zero versus the store id |
| S1-13 | blocker | [F16] P-36 vs [API §6.2] CK-4 | The hashed `hlc` depends on maintenance and lazy-record timing |
| S1-14 | blocker | `rules/link-merge-rules.md` RK-006 | Re-key re-points anchors only; FB-3 and [F12 §7.6] require every added edge |
| S1-15 | blocker | `rules/merge-table.md` MR-002, [F13 §3.5] I31′ vs [F12 §5.4] | Conflict-valued base: three texts, two answers |
| S1-16 | blocker | [F13 §4] vs `rules/state-definition.md` vs [F11 §7] | Two I26′ definitions and two marker caches |
| S1-17 | blocker | [F19 §8.3] | JSON commit ids without `c`, against A-m7 and [50 §2.9] Q18 |
| S1-18 | major | [F05 §4.4] G-2, G-3, G-5; [F17 §4.4] W3 | Groups of E − 39 … E − 1 bytes can never be placed |
| S1-19 | major | [F06 §9] BK-2 vs [F09 §17.1] V-8, OP-09-03 | Two definitions of `cs_ref.b3` |
| S1-20 | major | [F09 §16.4] `PREV`, [F06 §7.3], [F16] P-34 | A bulk commit's sealed `PREV` is right only under node-granular re-validation, stated three ways |
| S1-21 | major | [F10 §4.6], OP-10-10 | `gc`'s `hist` rewrite writes a header-only commit form no chapter defines |
| S1-22 | major | [F07 §7.4] OP12, [F06 §6.3] | Node images hold no hierarchy or edges: `DeleteVsModify` towards the live side loses them |
| S1-23 | major | [F14 §11.3] OP22 | Image-only data of import-checkpoint commits has no storage: gate 2 unattainable |
| S1-24 | major | [F16] P-72, OP4 vs [F05 §2.2], [F17 §2.2] IP-6 | A crash during extent preparation makes the store refuse to open |
| S1-25 | major | [F16] P-29, P-92, OP13 | A read error above `durable_lsn` lets the next append overwrite acknowledged groups |
| S1-26 | major | [OS/fs §4.9.4] vs [F02 §3.2], [F16] P-86 | Discovery runs `swap_recover` in one file and never in two others |
| S1-27 | major | `os/` (no seam), [F08] OP41 | No cryptographically secure random source in the OS layer |
| S1-28 | major | [F20 §2.3], OP27 vs [F04 §4.4], [F17 §2.1] | The `project` root's `oid` algorithm is `init`-fixed but stored nowhere |
| S1-29 | major | [F17 §4.3] vs [F10 §4.2] | Two frame-cutting rules for `hist` |
| S1-30 | major | [F13 §6.2] vs `rules/state-definition.md` OP7, BT-006, BT-007 | `gates` in `open_blockers` deadlocks a gated task |
| S1-31 | major | [F20 §5.9], Holes `F20-btime-ntfs` | S-16 decision: the hole needs the "tools copy creation times" outcome |
| S1-32 | minor | [F07 §2.2], [F14 §6.8], [F19 §12.1] | `DATA` listed as a class that [F12 §6.1] gives no code |
| S1-33 | minor | [F13 §5] V05, V07, V12, OP-13-06 | "Class open" although [F19 §12.2] assigns 67, 68, 72 |
| S1-34 | minor | [F07] OP26, [F08 §7.2] | Empty body versus no body undecided |
| S1-35 | minor | [F10 §8], OP-10-11 | Cites an `OPS` section that [F09 §16.4] does not have |
| S1-36 | minor | [F01 §8.3] vs [F11 §8] | `IDEM` row size 40 versus 72 bytes |
| S1-37 | minor | [F05 §8.4], [F11 §2.5], `README.md` | Two `FileRef`s, two owners of `FileFamily`, `cs` owner misnamed |
| S1-38 | minor | [F02 §6.3] vs [OS/env §5], [OS/project] OP18 | `tmp/` grammar lacks the probe files and `settle.stamp` |
| S1-39 | minor | [CFG §4.2] vs [F08 §5.4.1], [F14 §2.3] | Root names may begin with a digit in one chapter only |
| S1-40 | minor | [F01 §2.5], `HOLES.md` §3–§4, rule files | Hole ids outside the id form; rule files still write decided "holes" |
| S1-41 | minor | [F16 §3], [F02 §3.3] rule 6 | Two durability points without a P-rule and a seeded bug |
| S1-42 | minor | [F03 §8.1], §8.7, §10.3 | The intent nonce is used before the holding that draws it |
| S1-43 | minor | [OS/clock §6] vs [F16] P-89, [F17 §11.4] | Two clocks for the deletion grace |
| S1-44 | minor | [F19 §7.1] vs [F05 §9.16], [API §12.4] | Exit 8 for a one-commit `file mv` with failed items |
| S1-45 | minor | [F04 §10], §5.14 vs [F16] P-88, [F11 §3.7] | Initial `HEAD` values and two homes of `next_ref_id` |
| S1-46 | minor | [F08 §8.4.6] vs `rules/delete-policy-matrix.md` EG-006, EG-009 | Delete policies the schema enumeration cannot express |
| S1-47 | minor | `os/*.md`, `rules/README.md` §1.1 | No Coverage sections; stale rule-file list |
| S1-48 | minor | `rules/link-merge-rules.md` RK-004 vs [F08 §11.2] step 4 | Two dead-uid tests |
| S1-49 | minor | [F17 §4.2] vs [F05 §2.5] EX-5 | The retirement rule omits EX-5 |

## 4. Findings

### Blockers

**S1-1 — Two stored encodings of one value (blocker, DIVERGENCE).** Where: [F06 §5.1–§5.3] and [F08 §5.1–§5.5, §6.2];
recorded, unresolved, in [F07] open point 9, [F08] open point 45, [F12] open point 27, [F14] open points 13–14, [API] open
point 33. Problem: the op values of a commit record ([F06]) and the stored values of segment rows ([F08]) use two
registries that disagree on more than numbering: (a) tags: [F06] 0 `absent`, 1 `false`, 2 `true`, 3 `int` … 14 `pathmove`;
[F08] a type byte with `type_id` 1 `bool` (value in bit 7) … 13 `pathmove`; (b) `ref`: `uvar32` versus fixed `u32`;
(c) `commit-ref`: 32 bytes versus a 16-byte `id16` — [F07 §8.1] and [F14 §5.1] need the full id, which a store cannot
rebuild from 16 bytes for a commit it does not hold; (d) empty text and empty set: "a value distinct from `absent`"
([F06 §5.2], a C-rule) versus "never stored, the field is absent" ([F08 §6.2]); an implementer following [F06] stores
`SetField(new = text "")`, one following [F08] stores no op (NF-3); (e) `f64` ±∞: valid in [F06 §5.2], refused in
[F08 §5.3] and assumed absent by [F07 §7.1]; (f) `pathmove.class`: 0–3 versus 1–4; (g) stored set order: [F06 §5.3] orders
`pathmove` by (`hlc`, `from`, `to`, `git`) with paths by (root id, bytes); [F08 §5.5] by (`hlc`, `from` text, `to` text,
`class`, `git`). Stored commit records therefore differ between implementers, and `commit-ref` values in segments lose
hashed bytes. Fix: make [F08 §5] the one owner of value bytes ([F01 §2.4] rule 4) and have [F06 §5] cite it, with
`commitref` widened to `b32` in [F08 §5.1] (keep a 16-byte index column in [F09] if wanted); one empty-value rule (absent,
as [F08 §6.2]; [F06 §5.2] drops "distinct from `absent`" and `SetField` to an empty value stores `new = absent`); `f64`
±∞ refused everywhere; one `pathmove` class numbering; one stored set order. Update the [F06 §11] example and
[F06 §10]'s V-rule list accordingly.

**S1-2 — Two edge property blocks, and a segment table that cannot hold either (blocker, DATA LOSS).** Where:
[F06 §7.5.2] (`pmask` with bits `flagged`, `pinned`, `anchor`; `pinned` `b32`), [F08 §10.2] (a block chosen by the kind's
`props`; `pinned_commit` `b16`; `flagged` as `pflags` bit 1), [F09 §7.2] (`EDGE_PROPS` rows `{edge u32, pinned_commit
b16}` "for a `cites`, `implements` or `derived_from` edge"). Problem: [F07 §8.1] hashes the `flagged` bit and the full
32-byte pinned id of every edge. `EDGE_PROPS` has no field for `flagged` at all and keeps 16 bytes of the pin, so after the
first checkpoint the state of a tombstone's retained `blocks`/`gates` edges (I39′) and of every pin to a non-local commit
is lost: `doctor --verify`, merges and exports computed from segments give another canonical state than the log.
`consumed` also carries a pin ([F08 §9.6], its open point 31) but is missing from [F09 §7.2]'s list. Fix: [F08 §10.2]
owns one block — `pflags u8` (bit 0 `has_pin`, bit 1 `flagged`), then `pinned_commit b32` when `has_pin` — and [F06 §7.5.2]
cites it; [F09 §7.2] becomes `{edge u32, pflags u8, _pad [3]u8, pinned_commit [32]u8}` (40 B), one row per edge whose
kind's `props` is `pinned` or `flagged` and whose flags are non-zero.

**S1-3 — Two anchor-record layouts (blocker, DIVERGENCE).** Where: [F06 §7.5.3] and [F08 §10.3] ([F07] open point 9,
[F08] open point 37, [F14] open points 10–12, [API] open point 32). Problem: the two records differ in (a) enumeration
codes (`akind`, `mode`, `watch` 0-based versus 1-based); (b) `scope`: a `vstr` string (`rust:struct LockFile/…`) versus
[F08 §10.3.1]'s binary value — and [F08 §11.4] hashes the scope bytes into `captured`, so the anchor uid (and every commit
id holding it) depends on which chapter an implementer follows; (c) presence: flag-driven (`hint`, `window`, `span`) versus
kind-driven; (d) `blob` with `algo` 0 for a planned target: allowed versus refused; (e) texts: `vstr` (valid UTF-8 only)
versus `vbytes` — [F14] open point 11 shows anchor texts need not be UTF-8, so [F06] cannot store them; (f) the
`text-unavailable` state: [F06]'s `aflags.text` clear versus [F08]'s `text_unavailable` bit set; (g) the uid: in [F08]'s
record, not in [F06]'s. Fix: [F08 §10.3] is the one owner (its open point 37); [F06 §7.5.3] is replaced by "the op carries
[F08 §10.3]'s record and `anchor_no`". Settle in [F08]: `blob` may have `algo` 0 for a planned target (as [F07 §8.2] and
[F14 §6.7] already allow), texts are `vbytes`, and `captured` takes [F08 §10.3.1]'s bytes, with [F14 §5.6]'s text as the
bijective image form.

**S1-4 — The scope scanners are unspecified (blocker, UNDERSPECIFIED → DIVERGENCE).** Where: [F20] open point 30;
[F08 §10.3.1] ("what the scanners report … [F20] open point 30 leaves to review pass 1"). Problem: `scope` enters
`captured` ([F08 §11.4]), hence the anchor uid (the `at` edge key's discriminator) and canonical item 10. Which items a
Rust, Markdown or TOML scanner reports, what `name` holds (an `impl` of a generic type, a heading with inline code, a
dotted TOML key) and how Markdown numbering is stripped into `qual` decide those bytes, and no chapter fixes them. Two
engines — or the engine and the model — would derive different anchor uids for one capture. Fix: a scanner-grammar
appendix in [F20] as part of resolver version 1 (item kinds per language; the exact name text; the numbering-stripping
pattern; the TOML table path form; what a scanner does on unparsable input), with fixtures per construct, before the
freeze and before WP-63 is accepted. If it cannot be written in M0, `scope` must leave `captured` (a change of FB-4, for the
owner).

**S1-5 — The provisional side of an existence conflict is stored in one chapter only (blocker, DIVERGENCE).** Where:
[F12 §6.3] and its open point 4; [F06 §6.2] and [F07 §7.3] (`cstate`). Problem: [F12] requires a byte `prov` (0 `ours`,
1 `theirs`) after `theirs` in every `cstate` with `cs` = 1 on an existence key, part of the state and hashed; neither
[F06]'s stored `cstate` nor [F07]'s canonical `cstate` has it. Without it a `--policy` override cannot be replayed, and
[F12 §7.3] rule 4 compares a field that does not exist; with it, engines following [F12] and [F07] hash different bytes.
Fix: add `prov u8` to [F06 §6.2] and [F07 §7.3] exactly as [F12 §6.3] states, list it in [F06 §7.7] `Conflict` and in
[F11 §10] (S1-7); [F14 §6.8.1] already carries it as the node file's form.

**S1-6 — A tombstone landing from the absent state has no stored op (blocker, UNDERSPECIFIED).** Where: [F07 §10.1]
table row "absent → `deleted`" and its open point 5; [F14 §11.2] row "absent | tombstone" ("[F06] has no op yet"); [F06
§7.4], NF-1, NF-6. Problem: a lane that creates a node and later deletes it (an everyday agent pattern) and then merges
into `main` produces an item-10 entry `absent → deleted(kind, reason, replaced_by)` on `main`. [F06]'s `Delete` needs an
existing node and a before-image, and NF-1 forbids a `Create` and a `Delete` of one key; the merge commit cannot be stored,
and implementers will invent incompatible encodings. The same holds for a checkpoint import and for a sync. Fix: [F06 §7.4]
adds the form — for example `Delete` with a flag `from_absent` (bit in a new `dflags u8`), `kind`, `reason`,
`replaced_by`, an empty before-image and the tombstone's retained title — with its NF rule, [F07 §13]'s mapping row, and
[F14 §11.2]'s stored-op cell; [F12 §7.8] states that merges emit it.

**S1-7 — `CONFLICTS` rows cannot hold most conflict values (blocker, DATA LOSS).** Where: [F11 §10] (`base`, `ours`,
`theirs` "in [F08]'s value encoding; empty = absent") and its open point 31. Problem: [F08]'s value encoding covers field
values only. A `DeleteVsModify` side is an existence value with a node image, a `StatusFork` side a (status, resolution)
pair, a `TextHunk` on a body a body hash, a `FieldEdit` on an edge an edge property block or anchor record, a named-query
conflict a schema item ([F12 §6.2]). After a checkpoint these conflict values have no representation, so a staged or
landed conflict disappears from the view's state. "Empty = absent" also collides with the explicit `absent` of
[F06 §6.2]'s `kval`. Fix: store each side as [F06 §6.2]'s `kval` of the key's class (the bytes the `Conflict` op already
holds), keep `class`, and add `prov` for existence keys (S1-5); state `n` = 0 for schema keys as [F12] open point 18
proposes.

**S1-8 — Log records and the rows they fold into disagree (blocker, DIVERGENCE).** Where: [F05 §9] against [F11 §3–§12];
[F11] open point 33 proposes the alignment; nothing aligned. Problem, per kind:
- `TreeReg` epochs ([F05 §9.23] vs [F11 §12.4]): `scope_kind` 0 full-tree / 1 lane-owned / 2 `partial` versus 1 / 2 /
  3 `brief`. FB-1 says "`partial` (the brief, `--scope`, `--path`)"; [F11] has no value for `--scope` or `--path`
  settles and defines a digest only for `brief`. The sensitivity map: [F05] `(dir vstr, sflags)` with bit 0
  `case_sensitive`; [F11] `(equiv, path vstr)` with bit 0 `case_insensitive` — the bit's meaning is inverted.
  [F05]'s `journal_vol` has no row field; [F11]'s `dirty_present` and `git` flags have no record field.
- `FileObs` ([F05 §9.18] vs [F11 §12.5]): proposals `(path pathv, evidence u8, score u16 in 1/10,000)` versus
  `(class u8, path vstr, evidence vstr, score_num u64, score_den u64)`; `state_path` versus a `detail` string.
- `Pending`: `evidence u8` (a class) versus `class u8` plus an `evidence` token text.
- `AnchorRes`: `score u16` versus an exact rational; no `detail` byte in the record.
- `FsIntent`/`FsIntentDone`: item paths as `pathv` (root and text) versus `vstr` (root lost, although intents can touch
  named roots); per-item outcomes 1 done, 2 busy, 3 destination exists, 4 source missing, 5 other, versus 0 open, 1 done,
  2 aborted, 3 ambiguous, 4 missing.
- `Lease`: [F11 §6]'s flag `session_role` is carried by no field of the `Lease` record.
- `Marker`: [F11 §7]'s `origin` distinguishes `reattributed` from `branch-delete` and stores `orig_ref_id`; [F05 §9.5]'s
  `cause` 4 covers both and carries no original ref; [F05]'s `outcome` (`done`/`failed`/`abandoned`) has no row field.
- `RefTable`: "a `RefTable` entry replaces the ref's entry" ([F05 §10.3]) but `RefEntry` lacks [F11 §3.1]'s `fork_lsn`,
  `fork_ref_id`, `ops_total`, `bytes_total`, `trunk_mark_*`, `moves` and the `pinned` flag, so a replay after a
  `RefUpdate` group leaves them undefined.

Fix: adopt [F11] open point 33 — for every folded kind, the record's row part is byte-identical to the [F11] row (fixed
part plus heap slices as a sequence), with [F11] owning the row and [F05] the record frame — or, kind by kind, change
[F05] to [F11]'s enumerations and fields. Decide the epoch enumeration together with FB-1 (`partial` covers brief,
`--scope` and `--path`, with one digest rule each, or a single non-covering value) and keep [F20 §5.9]'s "a partial epoch
never counts".

**S1-9 — Link-state and evidence codes (blocker, DIVERGENCE).** Where: [F18 §4.2], §4.3, §4.10, §5.2 against [F11 §12.5]
and §12.13; [F05 §8.7]. Problem: [F18] owns the codes ([F05 §8.7] says so) and states that rows use them: file states
1 `ok` … 5 `deleted`, 6 `replaced`, 8 `missing`, 9–12. [F11 §12.5] stores 5 `replaced`, 6 `missing`. Proposals: [F18
§4.10] stores the evidence as a §5.2 token code (`u8`), [F11] as a class byte plus a token text, [F05] as a class byte
owned by [F20]. [F18 §4.10] lets `ANCHORRES` store anchor state 6 `unverified`; [F11 §12.13] says it is never stored.
Fix: [F11] uses [F18]'s codes verbatim (cite, do not restate); a proposal row is `(class u8 per [F20 §1.5], evidence u8
per [F18 §5.2], path, score)`, applied also in [F05] (S1-8); [F18 §4.10] states that anchor state 6 is never stored
(it depends on budgets, as [F11] argues).

**S1-10 — Three encodings of a binding (blocker, DIVERGENCE).** Where: [F18 §3.2] (`BindingExt`, 40 B: `bflags`,
`base_algo`, `expected_ref` as a `git-branch` symbol in **short** form, `base` in the fixed 32-byte slot) and §3.7 ("every
binding row carries exactly one `BindingExt`"); [F11 §5] (`HEADS`: flag bits, `base_commit` as a 33-byte `OidSlot`,
`git_ref` as heap text holding the **full** ref, "for example `refs/heads/u/l5np`"); [F05 §9.3] (`ClientHead`:
`expected_git_ref vstr` of unstated form, `base_commit oidv`). Problem: one R-15 datum has three layouts and two text
forms; the designated tree's HEAD check compares against the stored form. Fix: [F18 §3.2] owns the bytes; [F11 §5]
embeds `BindingExt` in place of `base_commit` and `git_ref` (or states that its fields are `BindingExt`'s), [F05 §9.3]
carries the 40 bytes, and every chapter uses the short form of [F18 §3.2] rule 1.

**S1-11 — The protocol appends records that the log format declares invalid (blocker).** Where: [F16] P-84 and open point
5 (a durable reservation record, "kind 27 `Reserve`" requested), P-70 and open point 16 (`RefUpdate` reason 5 `park`),
open point 15 (merge pins in the checkpoint group); [F05 §7] ("0 and 27–255 are invalid"), §9.2 (reasons 1–4), §4.7;
[F12 §8.2] and its open point 14. Problem: a record of kind 27 fails record validity ([F05 §5.2] check 3): below
`durable_lsn` the store is corrupt (exit 7), above it the valid log ends there, every later group becomes invisible and
the next append overwrites it ([F05 §5.3]). A `RefUpdate` with reason 5 is a malformed payload, corruption wherever it
lies ([F05 §5.4]). Every store that ran a bulk commit or parked a commit would lose data or become unreadable; conversely an
implementer who follows [F05] cannot implement P-70 or P-84.
Fix: [F05] adds kind 27 `Reserve` (durable; payload: the reserved `#N` range, `aN` range, `cs` and `blobs` file numbers,
schema ids, and symbols through the `SymDefs` block; `HEAD` fold: `next_id`, `next_anchor`, `next_file_no`), reason 5
`park` in §9.2 (fields `old`, `new`; no absorbed vector), the checkpoint group's merge pins in §4.7, and updates the
registry sentence to "0 and 28–255".

**S1-12 — `InitParams` bytes 16–31 (blocker).** Where: [F17 §2.1] (`_reserved`, reserved-zero, "tested by IP-2") and
IP-2; [F04 §4.4] (`store_id` there, "never all zero") and its open point 2 ("WP-16c edits [F17 §2.1] to match"). Problem:
the edit was not made. A reader that applies [F17]'s IP-2 finds non-zero reserved bytes in every store and exits 7; one
that applies [F04 §7] check 5 requires them non-zero. Fix: [F17 §2.1] names bytes 16–31 `store_id` (`b16`), IP-2 checks it
is not all zero, IP-1 keeps it across `restore` and `repair`, and the §2.1 table cites [F02 §4].

**S1-13 — The hashed `hlc` depends on timing outside the model (blocker, DIVERGENCE).** Where: [F16] P-36 (every
append-time HLC, `Checkpoint.append_hlc` and `Lazy.hlc` included, advances one `h_last`); [API §6.2] CK-4 and its open
point 39; [F17 §1.5] SP-1. Problem: a commit's `hlc` is canonical item 3. Under P-36 a class-I checkpoint, a runtime-only
fold or a hook's lazy evidence record appended between two commits in one millisecond, or while the wall clock lags the HLC,
raises the counter of the next commit's `hlc`, so commit ids depend on maintenance timing (SP-1 violated), differ between
the engine and the model (GT2), and differ between a run and its replay after a crash that lost a lazy record. Fix: P-36's
`h_last` is the greatest HLC of the semantic durable records ([API §6.2] CK-4's list); other records carry
`max(wall << 16, h_last)` without advancing it. I43′ stays strict for commits in `seq` order, which is all [F05 §9.9]'s
`dhlc` needs; [OS/clock §7] is restated to match.

**S1-14 — The re-key of the signed rule table is anchors-only (blocker, design rule violated).** Where:
`rules/link-merge-rules.md` RK-006 ("repoint-anchors": "Every `at` edge on S whose destination is U gets destination
uid′"); `a1-dispositions.md` FB-3; [F12 §7.6] step 3 (every edge S added since B whose destination is U, and `replaced_by`
and `ref`-typed values equal to U; "RK-006 … superseded by FB-3; R-MODEL amends RK-006"). Problem: the reference model is
written from the signed rule text; with RK-006 as it stands it leaves non-`at` edges (`produced`, `consumed`, `implements`,
`mentions`, `cites` …) on U after a dual creation, which is S-01's WRONG ANSWER, and GT2 would compare it against an
engine following [F12]. Fix: RK-006 becomes "every edge key present in S and absent in B whose destination is U, every kind,
anchors included; every `deleted(replaced_by = U)` and every `ref`-typed value equal to U that S set since B" (FB-3 plus
[F12] open point 25), RK-010 records the keys in a sync's residue, the owner re-signs (V3), and P13 of [40 §8.3.2] gains a
non-anchor edge case.

**S1-15 — Merging over a conflict-valued base: three texts, two answers (blocker, WRONG ANSWER).** Where:
`rules/merge-table.md` MR-002 ("conflicts whenever they differ, even when one side still holds the base's conflict value");
[F13 §3.5] I31′ (the same literal); [F12 §5.4] RVB-1–RVB-4 and its open point 2 (a side that still holds the base's
conflict value has not touched the key since the base, so the other side's value lands). Problem: under MR-002 a lane that
never touched k conflicts with a lane that resolved it — a conflict on a key untouched on one side since the base, which
I25′ forbids and which MR-004 does not raise at a real LCA. The model (from MR-002) and an engine (from [F12]) diverge on
merge commits. **Decision (lens S):** [F12 §5.4] is correct. Fix: reorder the conflicted-key rows as [F12] open point 2
states (MR-001 same → clean; MR-003/MR-004 one side untouched → that side's value; MR-002 only when both sides changed and
differ, class per RVB-4), restate I31′ in [F13 §3.5] as "conflicts when both sides changed it and differ", mark [60 §3.4]
and [AR §5a.7] step 1 for WP-81a, add VBC-3 to GT6, and have the owner re-sign the table.

**S1-16 — Two definitions of I26′ and two marker caches (blocker, WRONG ANSWER).** Where: [F13 §4.1] (the setter is the
oldest commit of the run on X's **first-parent** chain; for a merge into `main` the merge commit, open point OP-13-04),
§4.2 MC-1–MC-7 and open point OP-13-05 (a known case where cache and definition disagree: `reopen` on a parent ref);
`rules/state-definition.md` OR rows and open point 1 (the **origin**: follow the parents, first parent first, while the
hold is unchanged — the lane commit, not the merge commit), MF-006 `holders`, MF-007 `nonlinear`, ME-001–ME-012 and open
points 3–5; [F11 §7] (`MARKERS` row without a holder set or a `nonlinear` flag). Problem: the two readings exclude
differently (the rule table's scenario S8: after `lane/b` merged `lane/a` directly and reopened `#89` on purpose, [F13]
keeps `#89` excluded on `lane/b` because of `main`'s merge commit; the rule table does not). The owner is to sign "the I26′
state definition with its marker-cache rules" once, and GT18's oracle can implement only one. [F13]'s cache also fails its
own OP-13-05 case; the rule table's holder sets fix it (scenario S7). **Decision (lens S):** adopt the rule table's origin
reading and holder-set cache: it is a pure function of states ([72 M4] fix 1), it removes OP-13-05, and it does not let a
merge commit override a deliberate reopen. Fix: [F13 §4.1] cites the OR rows for the setter; §4.2 is replaced by the MF/ME
rows (or cites them); [F11 §7] adds the holder set (a heap slice or bitset over live `ref_id`s), the `nonlinear` flag and
the key of MF-003/MF-004; [F05 §9.5] carries what the fold cannot recompute; OP-13-04 and OP-13-05 close.

**S1-17 — JSON commit ids (blocker, disposition violated).** Where: [F19 §8.3] ("64 lower-case hexadecimal digits without
the `c` prefix (review A-m7)") and its open point 24; `a1-dispositions.md` A-m7 ("`c` + 64 lower-case hex in JSON", fixed
in [50 §2.9] Q18); [LQ/envelope §7.3], [LQ/errors §5.7], [F12 §3.8], [API §5.1] and [API] open point 1 follow the
disposition. Fix: [F19 §8.3] writes `c` + 64 lower-case hex digits and cites [50 §2.9] Q18.

### Majors

**S1-18 — Rotation cannot place groups of length E − 39 to E − 1 (major, UNDERSPECIFIED).** Where: [F05 §4.4] G-2 ("a
group is at most E bytes"), G-3, G-5 ("a group of any length up to E always fits the extent it goes to"; "the rotation
reserve … is therefore 0"); [F17 §4.4] W3. Problem: at a fresh extent r = E. A group with E − 40 < g < E satisfies neither
`g = r` nor `g ≤ r − 40`, so G-3 pads the whole fresh extent and moves on — forever. W3 with reserve 0 admits such a group,
and [F16] P-35 would then loop or fail differently per implementation. Fix: G-2 bounds a group at E − 40 bytes (or at
E − 40 or exactly E), [F05] open point 1 and G-5 are corrected, and [F17 §4.4] W3's reserve is 40.

**S1-19 — Two definitions of `cs_ref.b3` (major).** Where: [F06 §9] BK-2 (BLAKE3-128 of the whole file) against [F09
§2.3], V-8 and `FILES.digest16` (`seg_digest[0..16]`) and OP-09-03; [F10 §8] repeats both. Problem: V-8 checks the header
against "a commit's `cs_ref`", which it cannot do without reading the file under BK-2; the two chapters give two values for
one hashed-nowhere but stored field. Fix: adopt OP-09-03 (`b3` = `seg_digest[0..16]`), edit BK-2 and [F10 §8].

**S1-20 — A bulk commit's `PREV` is correct only under node-granular re-validation, which is not stated (major, WRONG
ANSWER in history).** Where: [F09 §16.4] `PREV` (absolute lsns sealed before the commit is appended); [F06 §7.3] ("`prev`
depends only on the owner's history on the ref, which is unchanged whenever a re-parent is allowed ([AR §4.5] step 7
re-validates **by key**)"); [F16] P-34 (titled "re-validation by key", but its text re-parents only if "no **node**, edge,
marker or lease it read or wrote changed"). Problem: under key-granular re-validation, a commit that landed on the same
ref between the sealing and the append and changed another key of a node the bulk commit touches allows the O(1)
re-parent, yet it is now that node's newest earlier op; the sealed `PREV` skips it, so per-node history, `blame` and as-of
reverse application miss a commit. Inline commits are safe because [F06 §7.3] re-serialises `prev` under the writer byte;
a bulk commit cannot. Which granularity holds is stated three ways. Fix: [F16] P-34 states that every node owning a row of
a bulk changeset counts as read and written at node granularity (any intervening commit touching it forces a re-run of
phase 1, which re-streams the file), [F06 §7.3] says "by node" instead of "by key", and [F09 §16.4] cites P-34.

**S1-21 — `gc`'s `hist` rewrite uses an undefined record form (major, UNDERSPECIFIED).** Where: [F10 §4.6] ("replaced by
[F06]'s header-only form"), OP-10-10; [F06] defines none. Problem: after `gc` two implementations write different
`hist` bytes, and a decoder cannot tell a pruned commit from an empty one (`n_ops` = 0 with a non-empty
`changeset_digest` passes every V-rule). Fix: [F06] defines the header-only form (for example `presence` bit 16 `pruned`,
`n_ops` = `n_bodies` = 0, the stored `changeset_digest` kept), its V-rule, and how history and `revert` refuse on it.

**S1-22 — Node images lose hierarchy and edges (major, DATA LOSS).** Where: [F07 §7.4] and its open point 12; [F06 §6.3];
[F12 §6.5] ("a `live` existence side restores the node's value keys from its node image"). Problem: a `DeleteVsModify`
under `delete-wins` resolved `--take` towards the live side restores the value keys but not the node's `parent`/`order`
nor the out-edges the delete policies removed, so the "restored" node differs from the side it was taken from; no
chapter states that this is intended. Fix: either add the hierarchy key and the out-edges to the snapshot ([F06 §6.3],
[F07 §7.4], [F14 §6.8.1]), or have [F12 §6.5] and `rules/merge-table.md` state that a take restores value keys only and
emit the hierarchy and edge ops of the live side in the resolving commit.

**S1-23 — Image-only data of import-checkpoint commits has no storage (major).** Where: [F14 §11.3] last row and open
point 22 (per node file: `created:`, `updated:`, `deleted:` values and ledger lines "not yet in [F06]"); [F14] open point
19 (zero ids in `ckpt.first`/`ckpt.last`). Problem: without them a re-export rewrites provenance and ledgers with the
importing store's ids, and gate 2's byte-identical head trees ([AR §5b.7]) fail by construction. Fix: [F06] adds an
unhashed group to an import-checkpoint commit (or a record kind) holding, per node file brought in, its three provenance
values and its ledger lines; [F09 §16.4] carries it for bulk checkpoints; [F06 §4.4.11] allows zero `first`/`last`.

**S1-24 — A crash during extent preparation bricks the store (major, availability).** Where: [F16] P-72 and open point 4
(a shorter file beyond the valid log is an interrupted preparation, re-prepared); [F05 §2.2] and [F17 §2.2] IP-6 ("an
extent of another length … exit 7"). Problem: `DiskFull` or a crash during `create_extent` ([F15] FM-5.4) leaves such a
file; a process following [F05] refuses the store permanently. Fix: [F05 §2.2] and IP-6 apply the length rule only to
extents at or below the one that holds the end of the valid log, as [F16] open point 4 proposes; [OS/fs §4.5] states that
re-preparation sets the length to E for every method.

**S1-25 — An unreadable acknowledged group can be overwritten (major, DATA LOSS).** Where: [F16] P-29, P-92 and open point
13; [F15] FM-12. Problem: after an OS crash `durable_lsn` in `HEAD` understates the durable end ([72 B1]). A persistent
read error inside acknowledged groups above it ends the valid log for the writer's scan (P-92), and the next append at E_v
overwrites them. FM-12's persistent read errors are injected by the in-memory `Vfs`, so GT1 would report an I-G1
violation that the protocol permits. Fix: distinguish readers from writers: a reader may end its view (P-58), but a writer
whose scan (P-29) meets a read error at or above `durable_lsn` appends nothing and exits 7 `store_io_fault` naming
`moirai repair` (X5: refuse rather than lose); record the residual only if the owner prefers availability.

**S1-26 — Who runs `swap_recover` (major).** Where: [OS/fs §4.9.4] ("run by `doctor` (and by store discovery when it finds
`<a>.swap`)"); [F02 §3.2] and its open point 17, [F16] P-86 (discovery never runs it; retries at 10, 20 and 40 ms, then
exit 7 `swap_in_progress`). Problem: discovery runs in lock-free readers that cannot tell a crashed swap from a running
one; recovering a running swap undoes it halfway. Fix: delete the parenthesis in [OS/fs §4.9.4] and cite [F16] P-86.

**S1-27 — No random source in the OS layer (major, UNDERSPECIFIED).** Where: [F02 §4], §5.3 (store id, `tmp/` nonces),
[F03 §7.1], §8.1 (leader and slot nonces), [F04 §5.3] (epoch), [F08 §2.2] (random uids) all require "the OS's
cryptographically secure random source"; [F08] open point 41; no `os/` file names a call, while [API §6.4] injects entropy
for the model. Problem: X3 puts every OS call behind `moirai-os`, and the simulator must inject the stream; the seam is
missing. Fix: [OS/README] adds a seam (for example `Env::fill_random(&mut [u8])`) with its per-OS calls in
[OS/mapping-appendix] and its simulator form, and the format chapters cite it.

**S1-28 — The `project` root's `oid` algorithm is stored nowhere (major).** Where: [F20 §2.3] (A(`project`) is `init`-fixed:
the repository's object format read at `init`) and its open point 27; [F04 §4.4], [F17 §2.1] (the 32-byte `InitParams`
block is full once S1-12 is fixed). Problem: every later process must recompute A(`project`) from the repository, which a
store without git at `init` and a repository added later would answer differently; `oid` comparisons across such a change
become "content unknown" silently. Fix: record `project_oid_algo u8` in `HEAD` (for example at slot offset 1072, kept in
`HEAD` like `init`, with IP-1/IP-2/IP-3 applied), list it in [F17 §3] as an `init`-fixed parameter, and have [F20 §2.3]
cite it.

**S1-29 — Two frame-cutting rules for `hist` (major, DIVERGENCE).** Where: [F17 §4.3] ("a frame is closed when it holds P03
commits, or when adding the next commit would take its raw bytes above P04") against [F10 §4.2] (every record's bytes count
toward P04; only commits toward P03; the split rule for any record above P04) and OP-10-03. Problem: the two produce
different frames, hence different `hist` bytes. Fix: [F17 §4.3] cites [F10 §4.2] and drops its own wording.

**S1-30 — `gates` in `open_blockers` deadlocks a gated task (major, WRONG ANSWER).** Where: [F13 §6.2] (`open_blockers`
counts `blocks` and `gates` in-edges whose source is not done or accepted; `unblocked` requires 0) and [F08 §3.4];
`rules/state-definition.md` open point 7, BT-006, BT-007 (`gates` counts only in the completion guard); [AR §3.3] X5
("`gates` constrains `complete` … never `claim`"). Problem: a task gated by an `open` verdict (for example a
`fail_fixable` one awaiting the fix) is never `ready`, so no one can claim the work that would let the verdict be accepted.
Fix: adopt the rule table's reading: `open_blockers` counts `blocks` only, `gates` enter the completion guard (`gated`,
[RULES/status-machines] GD-002); update [F13 §6.2], [F08 §3.4] and, at WP-81a, [AR §3.5].

**S1-31 — S-16: the btime hole must include the outcome that closes the residue (major).** Where: [F20 §5.9] line 2,
open point 15; Holes `F20-btime-ntfs` and `F20-ctime-rename`; decision D-1. Problem: [F20]'s residue (a
creation-time-preserving copy made between V and the original's deletion re-binds silently) is closed only if no tool in
use copies creation times on NTFS, but `F20-btime-ntfs`'s candidates are `TunneledNotCopied` and `Unforgeable` only, and
measurement 15's tool list ([40 §8.3.6]) names no copying tool that preserves creation times. Fix: add the candidate
"copied by some in-scope tool" to `F20-btime-ntfs`, under which [F20 §5.9] line 2 never applies on NTFS (as for
`CopiedByClones`); add to measurement 15's list the copy paths an agent machine runs (`robocopy /COPY:DAT`, PowerShell
`Copy-Item`, Explorer copy and paste, archive extraction, `git checkout` of a moved file); keep the ChangeTime condition;
and record in [F20] open point 15 that with the constraint satisfied the residue is unreachable, otherwise the owner signs
it as a known risk.

### Minors

**S1-32 — `DATA` in class lists (minor).** [F07 §2.2] (canonical names), [F14 §6.8] (accepted `class=` values) and
[F19 §12.1] (code range 1–63) list `DATA`; [F12 §6.1] gives it no code and its open point 5 asks [F19] to drop it. An
importer would accept `class=DATA` that no store can hold. Fix: remove `DATA` from all three lists.

**S1-33 — Violation classes "open" in [F13] (minor).** [F13 §5] V05, V07 and V12 say "class open (OP-13-06)", while [F19
§12.2] assigns `DepthExceeded` 67, `Cardinality` 68 and `PlanMask` 72 and [F12 §7.9] gives their keys. Fix: [F13] cites
them and closes OP-13-06.

**S1-34 — Empty body (minor).** [F07] open point 26, [F14 §6.9]: whether an empty body is a body is undecided; [F08 §7.2]
is silent. Fix: [F08 §7.2] states "an empty body is no body" (as for empty text, S1-1), and [F06 §7.4] `SetBody` to empty
stores `new` absent.

**S1-35 — The `OPS` of a changeset segment (minor).** [F10 §8] ("its `OPS` stay the commit's op list") and OP-10-11 cite a
section [F09 §16.4] does not have (OP-09-15: a bulk commit is a state delta). Fix: [F10] reads "its rows and `VIOLATIONS`".

**S1-36 — `IDEM` row size (minor).** [F01 §8.3] restates [AR §4.4]'s 40-byte row "leaving 4 bytes for … `branch_sym`";
[F11 §8] makes it 72 bytes (open points 13–14). Fix: [F01 §8.3] cites [F11 §8] and keeps only the symbol-width argument.

**S1-37 — File references and families (minor).** [F05 §8.4] (`FileRef`, variable) and [F11 §2.5] (`FileRef`, 9 bytes)
share a name; both define `FileFamily`; `README.md` says [F10] holds the `cs` files while [F02 §5.1] and [F10 §2.1] give
the layout to [F09 §16.4]. Fix: one owner of `FileFamily` ([F11 §2.5], cited by [F05] and [F09]); rename [F05]'s form
`FileRefV`; correct the README row.

**S1-38 — `tmp/` names used but not in the grammar (minor).** [F02 §6.3]'s `tmp-word` set is `cs`, `config`, `head`,
`sort`; [OS/env §5] writes `tmp/probe`, `tmp/probe.1`, `tmp/probe.2`, and [OS/project] open point 18 and [F20 §5.12.1]
use `settle.stamp`. The orphan sweep never touches foreign entries ([F02 §5.6], [F16] P-79), so leftovers accumulate. Fix:
[F02 §6.3] adds `probe` (with a nonce) and the fixed name `settle.stamp`; [OS/env §5] uses `probe.<nonce>`.

**S1-39 — Root-name grammar (minor).** [CFG §4.2] accepts a root name beginning with a digit; [F08 §5.4.1] and [F14 §2.3]
`rootname` require a letter, so `roots.2d` would configure a root whose paths cannot be stored or exported. Fix: [CFG §4.2]
uses [F08 §5.4.1]'s grammar ([F08] open point 38).

**S1-40 — Hole ids (minor).** Ids outside [F01 §2.5]'s form remain (`share-retry-ms`, the aliases of `HOLES.md` §4), and
`rules/pack-classes.md` and `rules/role-write-policy.md` still write `HOLE(pack-digest-param)`, `HOLE(exit5-codes)` and
`HOLE(unknown-model-write-code)` for decided naming choices, which [F01 §2.5] does not allow. Fix: rename per [F01] open
point 15 (`OS-share-retry-ms`, …) and replace the three decided holes by their values (`HOLES.md` §3).

**S1-41 — Durability points without a seeded bug (minor).** [F16 §3]'s rows for the `init --link` pointer file ([F02 §3.3]
rule 6) and for `LOCK` records have no P-number, so E4's "one bug per rule" does not reach them. Fix: give the pointer-file
point a P-rule and a seeded bug (success reported before `durable-name`), and state that `LOCK` records are covered by
[F03 §12]'s argument with no protocol point.

**S1-42 — The intent nonce's order (minor).** [F03 §8.1] draws `nonce` "at the start of the holding", while §8.7 starts the
slot search at `nonce mod 256` and §10.3 takes the slot "with a fresh nonce". Fix: §8.1 says the intent holder draws its
nonce before choosing the slot and keeps it for the holding.

**S1-43 — The clock of the deletion grace (minor).** [OS/clock §6] measures the GC grace by the stamp form of §4.5; [F16]
P-89 and [F17 §11.4] by the HLC ([F16] open point 8). Fix: [OS/clock §6]'s row cites [F17 §11.4].

**S1-44 — Exit 8 for `file mv` (minor).** [F05 §9.16] and [API §12.4] step 5 make a one-commit `file mv` with failed items
exit 8; [F19 §7.1] defines 8 as "several independent commits committed some and not others". Fix: [F19 §7.1] adds "or a
file verb whose commit recorded failed items".

**S1-45 — Initial `HEAD` and `next_ref_id` (minor).** [F04 §10] gives `next_ref_id` 0 and zero table pointers; [F16] P-88
writes the `main` group, so `next_ref_id` = 1 and `refs_lsn` (and `heads_lsn` with a binding) are non-zero ([F16] open point
14). `next_ref_id` also lives in both `HEAD` ([F04 §5.14]) and `REFS.aux` ([F11 §3.7]). Fix: [F04 §10] takes P-88's values;
[F11 §3.7] states that `REFS.aux` equals the covering slot's `next_ref_id` at the segment's bound.

**S1-46 — Delete policies the schema cannot express (minor).** `rules/delete-policy-matrix.md` EG-006 (re-point `blocks` to
a replacement when the destination is deleted) and EG-009 (drop, not flag, when a finished blocker is deleted) have no value
in [F08 §8.4.6]'s `on_dst`/`on_src` enumerations (`blocks`: `drop` / `repoint-or-flag`). Fix: extend the enumerations (for
example `drop-or-repoint`, `repoint-or-flag-unfinished`) or state in [F08 §9.6] that the rule table refines them.

**S1-47 — Coverage sections and the rule-file list (minor).** No `os/` file ends with a Coverage section ([F01 §2.3];
`COVERAGE.md` open point 1); `rules/README.md` §1.1 lists `delete-policy.md` and `i26-state.md` and omits
`role-write-policy.md` (`COVERAGE.md` open point 6). Fix: as those open points propose.

**S1-48 — Two dead-uid tests (minor).** `rules/link-merge-rules.md` RK-004 re-derives while uid′ is "deleted, or live with
status `removed`" in B, o or t; [F08 §11.2] step 4 while u is the uid of **any** node of the view (live at another path
included). Fix: RK-004 uses [F08 §11.2]'s test over B, O and T.

**S1-49 — The retirement rule (minor).** [F17 §4.2] retires an extent when every record lies before `checkpoint_lsn`; [F05
§2.5] EX-5 also needs `checkpoint_lsn` > n·E. Fix: [F17 §4.2] cites EX-4 and EX-5 ([F05] open point 3).

## 5. Checked and found correct

These were recomputed or cross-checked and need no change; pass 2 need not repeat them unless the text changes.

- [F01]: the varint and zigzag tables, the `fstr<16>` and `lp()` examples, the `hlc` example (`03 00 00 6C 50 C4 A0 01`),
  the XXH3-128 storage example.
- [F02]: the 78-byte pointer-file example; the store-name grammar examples.
- [F03]: `LockHdr` (64), `WriterDiag` (512), `LeaderRec` (512), `SlotRec` (128), `ProcId` (32), `ParentRec` (16), `Anchor`
  (32); the `role_base`/`slot_base` byte strings; identical `ProcId` in [OS/proc §3.1]; lock offsets identical in [OS/lock].
- [F04]: every `HeadSlot` offset to 4,096; `SegRef` 29; the entry-offset formulas; `committed_lsn` = 40 after the epoch-start
  group of [F05 §4.5].
- [F05]: the embedded sizes (`OsFileId` 57, `VolumeCaps` 16, `JournalCursorRow` 41, `SettleEpoch` 32); symbol-class codes
  identical in [F09 §14.1].
- [F06]: the 205-byte example (presence `0x7043`, varints `B4 24`, `F7 22`, `C0 25`), the header-part bound 659 + 10A + M +
  5F.
- [F07]: the 43-byte entry, the 74-byte digest input, the 196-byte C, the `path`, set, counter-delta and `pathmove`
  encodings, and the BLAKE3 digest prefix of the empty string.
- [F08]: `NodeHdr` (60), `Creator` (6), `KindSet` (33), the 12-byte field block, the 46-byte file-uid input.
- [F09]: `SegHdr` (120), `SecEnt` (32), the fixed-table widths; [F10]: `DictHdr` (32), `GitmapHdr` (48), `FrameHdr` (88),
  `BlobIdx` (36); [F11]: the 88-byte `MARKERS` example.
- [F16]: I-G1–I-G6 map to rules; the thirteen group-commit bugs of [80 §2.4.4] and the fourteen of [60 §3.1] item 4 each
  appear in §17.3 (G12 split across P-50 and P-62); every P-rule has a seeded bug; P-36 makes `append_hlc` strictly
  increasing, which [F05 §9.9]'s unsigned `dhlc` requires (keep that property for commits when fixing S1-13).
- The A1 obligations of `a1-dispositions.md` §5 other than those above are met: FB-4, FB-5 ([F06 §7.5.3], [F07 §8.2],
  [F14 §6.7]), FB-6 ([F18 §5]), FB-7 ([F06 §7.5.1]), FB-8, FB-9 ([F07 §7.1]), FB-10 ([F20 §2.3]), FB-11 ([F06 §4.4.13],
  [F11 §9]), FS-1 and FS-2 ([LQ/errors] W10 and E117, [LQ/std §2.8], [F14 §7.2.2]), FS-4 ([F16] P-83); A1P-02's re-barrier
  (P-19); S-06 (P-34); S-11 ([F12 §2.1] and [LQ/grammar-v1.ebnf] `ref_name` agree).
- `COVERAGE.md`: 152 rows, none UNMAPPED. Several rows are mapped to chapters whose text contradicts another chapter (R-4,
  R-8, R-15, R-16, R-18, F11, [60 §2.5] "Ops and values" and "Segments"); they stay mapped, and the findings above are what
  must change.
- `HOLES.md`: every `HOLE(...)` in `docs/spec/` appears in it, aliases included.
- The idempotency-key framing agrees in [F06 §4.4.7], [API §7.2] and [LQ/canonical-ast] C-10; the `IF TARGETS` digest is
  defined in [LQ/envelope §9.4].
</content>
</invoke>
