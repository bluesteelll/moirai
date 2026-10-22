# Review pass 2, lens P: performance, RAM, Windows and crash safety

| Field | Value |
|---|---|
| Title | Independent review pass 2 of `docs/spec/` (format chapters 01–21, `os/`, `lq/`, `store-api.md` with its examples, `config.md`, `COVERAGE.md`, `HOLES.md`, `README.md`) and `docs/spec/rules/*.md`, lens P |
| Status | review, pass 2; every finding awaits its disposition by the owning role named in its Fix |
| Work package | WP-80 ([PLAN §3.2] item 8), role R-REV-P. The author of this file wrote none of the reviewed text (S5) and did not read the pass-2 findings of the other lenses |
| Lens | speed on the hot paths (commit path, lock holdings, open path, reads), private bytes and RAM bounds, Windows 11 behaviour (NTFS, Defender, sharing, reparse points), crash and power-loss safety; the lens of `docs/spec/reviews/P-pass1.md` |
| Reviewed | the 76 files of the pass-2 scope: every `.md` file of `docs/spec/` outside `docs/spec/reviews/` except `measurement-protocol.md`, `lq/grammar-v1.ebnf`, and the 20 examples under `docs/spec/store-api/examples/`; depth per file in §1. Read as text beside them: `fixtures/hex/INDEX.md` and `fixtures/hex/store-a/log.1.hex` |
| Base | 8bb4b6975fc808caf7fbe636556d25d4aa104c08 |
| Checked against | [AR] §4 (`LOCK`, `HEAD`, the log, the write path, maintenance, namespace durability), §5a.7, §6.5, §8.2–§8.3 (RAM and SPEED rows); [60 §2.5] (the fault model items (1)–(12)); [80 §2.2]–§2.12 and [80 §3.1] (X-F1–X-F12); [90 §10.1] and [90 §14.6]; this lens's pass-1 report (`docs/spec/reviews/P-pass1.md`) and the signed pass-1 resolutions the chapters cite |

Severity, as WP-80 defines it for the passes:

- **blocker**: two independent implementers would produce different bytes, or a rule of the design of record is
  violated;
- **major**: an ambiguity, a missing case or an inconsistency that must be fixed before the freeze;
- **minor**: editorial, or local with no byte or rule consequence.

Every line number below is at the base. Line citations name the file every time: `Fnn:<line>` for
`docs/spec/format/nn-*.md`, the repository path for every other file.

## 0. Verdict

**CHANGES REQUIRED: 1 blocker, 3 majors, 7 minors (11 findings).**

Since pass 1 the chapters this lens reads most closely have closed most of what it raised: every offset table sums to
its total (94 tables, 607 rows, §1), every re-derived example byte string matches (§6), the long maintenance holdings
now keep the tail bounded by yield checkpoints ([F16] P-98, [F17 §5.2]), and the seeded-bug catalogue gives each masked
row a written disposition (N-P7). What remains:

1. **One byte departure in the fixtures.** Store A's staged commit c11 carries `prev` deltas into
   records of `main`, which [F06 §7.3] and SL-2 forbid (P2-1). Either the fixture or the rule changes; the choice is the
   owner's.
2. **Liveness of the maintenance byte.** Nothing tells a process that a maintenance holder has stopped; a paused or
   stalled holder suspends every checkpoint, and the tail and every overlay then grow past their bound with no
   detection (P2-2).
3. **Two merge-path costs without a bound.** The walk that finds the merge M behind a conflict value ([F12 §6.5])
   has no cost bound and no budget (P2-3), and the per-merge text budget question is scheduled after the freeze
   although a budget that changes results would have to be format-fixed (P2-4).

The minors are pointers and FM citations ([F16] P-35, P-29, P-48, P-50), a restore step order that OS/fs and F15
restate against [F16] P-85, a contradiction about who runs a body purge, an unstated locator read bound, and two
reserved surfaces (the similar-records notice and the harvest cursor) without a work bound.

## 1. Method

Depth: "full" is every line; "delta" is every changed hunk since pass 1's base (aa7c67b) and every cross-reference into
it; "lens" is pass 1's "read for the lens" (each section on a hot path, a lock, a clock, a byte layout or a RAM bound).

| File | Depth |
|---|---|
| COVERAGE.md | delta |
| HOLES.md | delta |
| README.md | delta |
| config.md | delta |
| format/01-conventions.md | delta |
| format/02-store-layout.md | delta |
| format/03-lock.md | full |
| format/04-head.md | full |
| format/05-log.md | full |
| format/06-commit.md | delta |
| format/07-canonical-form.md | delta |
| format/08-data-model.md | delta |
| format/09-segments.md | delta |
| format/10-sealed-files.md | delta |
| format/11-runtime-tables.md | delta |
| format/12-vcs.md | delta |
| format/13-invariants.md | delta |
| format/14-image.md | delta |
| format/15-fault-model.md | full |
| format/16-protocol.md | full |
| format/17-store-parameters.md | full |
| format/18-file-links.md | delta |
| format/19-errors-and-output.md | delta |
| format/20-r4-resolver-constants.md | delta |
| format/21-scope-scanners.md | full |
| lq/canonical-ast.md | delta |
| lq/card.md | delta |
| lq/envelope.md | delta |
| lq/errors.md | delta |
| lq/gql-spelling.md | delta |
| lq/grammar-v1.ebnf | delta |
| lq/json-ir.md | delta |
| lq/lexical.md | delta |
| lq/std.md | delta |
| os/README.md | full |
| os/clock.md | full |
| os/env.md | full |
| os/fs.md | full |
| os/lock.md | full |
| os/map.md | full |
| os/mapping-appendix.md | full |
| os/mem.md | lens |
| os/path.md | full |
| os/proc.md | full |
| os/project.md | full |
| os/shell.md | full |
| rules/README.md | delta |
| rules/delete-policy-matrix.md | delta |
| rules/link-merge-rules.md | delta |
| rules/merge-table.md | delta |
| rules/pack-classes.md | delta |
| rules/policy-keys.md | full |
| rules/role-write-policy.md | delta |
| rules/state-definition.md | delta |
| rules/status-machines.md | delta |
| store-api.md | delta |
| store-api/examples/01-init-and-clock.json | lens |
| store-api/examples/02-tx-create.json | delta |
| store-api/examples/03-set-guard.json | delta |
| store-api/examples/04-claim-complete.json | lens |
| store-api/examples/05-idempotency.json | lens |
| store-api/examples/06-rm-policy.json | lens |
| store-api/examples/07-lane-merge.json | delta |
| store-api/examples/08-merge-staged.json | delta |
| store-api/examples/09-undo.json | lens |
| store-api/examples/10-caller-context.json | lens |
| store-api/examples/11-state-snapshot.json | lens |
| store-api/examples/12-runtime-snapshot.json | delta |
| store-api/examples/13-file-mv.json | lens |
| store-api/examples/14-apply-batch.json | lens |
| store-api/examples/15-lease-clock.json | lens |
| store-api/examples/16-image.json | lens |
| store-api/examples/17-query.json | lens |
| store-api/examples/18-maintenance.json | lens |
| store-api/examples/19-schema-migrate.json | lens |
| store-api/examples/20-vcs-refs.json | lens |

Beyond its depth, every file was searched for each byte layout, offset table, constant, clock or budget that a delta
changed. Item 1a's lines ([F06]'s locator and op-byte text, F07's line, [F16]'s P-35 to P-37 area, `F17:695`) were read
at full depth, with a reader's behaviour on a torn or partial record that carries a locator or the `more` and `cont`
flags, and the read cost the locator and `prev` text imply (P2-9). In `config.md` every changed hunk was read, and the
rows of §10 that set a clock, a budget, a size or a cap were read in full against [F17] and the RAM rows.

Mechanical checks (each command and its full output are kept with the review's working evidence, outside the
repository):

- **Offset tables.** A script summed every offset table of the 76 files (each row's offset against the previous row's
  offset plus width, and the `total` row): `offsets: 94 offset tables, 607 rows checked`, no mismatch. The same script
  on a copy of [F04] with one offset changed from 1088 to 1089 reported that row and the row after it, so it can fail.
- **Example bytes**, by `cargo xtask hex --digest`, each printed literal against a file derived from the rule:
  [F06 §11]'s `SetStatus` op with the known locator, `12 bytes`, equal digests; the same op with the unknown locator,
  `11 bytes`, equal digests; [F21 §2.3]'s Markdown scope, `25 bytes`, and its TOML scope, `19 bytes`, equal digests;
  [F08 §11.2]'s `uid_file` input, `46 bytes`, equal digests.
- **Computed uids**, by `cargo xtask hex`: [F14 §17.2]'s file uid gives `19143eabafc5ec1c2f8ddb102a731ae3` and the root
  uid `721354ad0f367138746bf0429056d772`, as `docs/spec/format/14-image.md:1705` and
  `docs/spec/format/14-image.md:1727` print them.
- **Store A's c11**, read as text: the `prev` values of g18 and the groups they name (P2-1).

## 2. Named checks

| Id | Check | Verdict |
|---|---|---|
| N-P1 | Does [F03] or [F16] state the maintenance byte's liveness: what a process concludes about a holder that stops, and by which probe or clock? The byte is try-only (F03:84; [F16] P-76, F16:656-661), no record names its holder (only the writer byte has `WriterDiag`, F03:231-247), and a paused holder keeps it for any duration (F15:438-441) | MISSES: P2-2 |
| N-P2 | P-35 (F16:251-256) states outcomes of the final re-check that P-37's all-unknown fallback (F16:295-299) overrides; does P-35 need a pointer to P-37? | MISSES: P2-5 |
| N-P3 | Does store A's commit c11 (`fixtures/hex/store-a/log.1.hex:2804`, `fixtures/hex/INDEX.md:98`) meet [F06 §7.3] (F06:724-726) and SL-2 (F06:777-780)? It does not: its two `prev`s name records of `main` | MISSES: P2-1 |
| N-P4 | Does the tree imply a cost bound for [F12 §6.5]'s walk from `CONFLICTS.commit` to the merge M (F12:656-664)? It does not; [F12 §5.5a]'s charging covers RS-007's origin walks only (F12:479-494) | MISSES: P2-3 |
| N-P5 | Does the specification carry [80]'s decisions (§2.2–§2.12, §3.1) without contradiction, its X-F rows mapped? `COVERAGE.md` maps X-F1–X-F12, 12 of 12, none unmapped (`docs/spec/COVERAGE.md:54`, `docs/spec/COVERAGE.md:241-256`); the lock map (F03:81-89, `docs/spec/os/lock.md:30-39`), the durability calls (F15:687-693), extent preparation (F16:117) and the mapping policy (F16:1070-1076) agree with [80]; the departures (the nine quiet bytes, F03:96-121; the restore swap; the writer's refusal on a read error, P-92) are recorded as signed pass-1 resolutions | MEETS |
| N-P6 | [90 §14.6]'s two layout items: the 32 B anchor holds the 16 B BLAKE3 session hash and its kinds (F03:437-445), the 128 B slot record (F03:290-308) and every `LOCK` offset are unchanged (F03:81-89, `docs/spec/os/lock.md:30-39`); `LEASES` rows are sorted by (`n`, `lease_id`) with `n` = 0 for a role lease (F11:535), and a Codex holder's root session is a 16 B hash, zero otherwise (F11:554) | MEETS |
| N-P7 | Every [F16 §17.3] row of a catalogue bug that another rule masks in every reachable toy state carries its written disposition: P-45 masked and carried by a unit test (F16:1191; the exit criterion at `docs/m0/PLAN.md:852` names that test, owner-accepted as OQ-A-1 (a)); P-59 and P-79 re-vehicled with their reason (F16:1205, F16:1225); P-40 and P-13 re-formed into reachable bugs (F16:1186, F16:1159); P-54, P-55 and P-61 carried through setup-injected states (F16:1200-1207); the list is [F16]'s open point 30 (F16:1455-1459). The lens rates the dispositions as sound and accepts none; acceptance is the owner's | MEETS |
| N-P8 | Does the lens want a decoder bound for [F08 §10.3] that the tree does not imply? [F08 §10.3] bounds `marker` (one line, at most 64 bytes) and a scope (1–64 segments) for every decoder, and leaves the text fields bounded by writers only (F08:1317-1325); every such field lies inside one record, which lies inside one extent, so a decoder's memory per field is already bounded. The lens wants no further decoder bound | MEETS |
| N-P9 | [F15]'s fault model is complete: every item (1)–(12) of [60 §2.5] has its FM section (`docs/spec/COVERAGE.md:108-119` map them to [F15 §3.1]–§3.12), and every behaviour a delta added to [F03], [F04], [F05], [F16] or [F17] that depends on a failure names the FM item it rests on. The first half holds; three added [F16] behaviours name none (P-29 at F16:204-210, P-48 at F16:368-370, P-50 at F16:377-381) | MISSES: P2-6 |

## 3. Findings at a glance

| Id | Sev. | Where | Finding | Owner |
|---|---|---|---|---|
| P2-1 | blocker | `fixtures/hex/store-a/log.1.hex:2865`, F06:724-726 | Store A's staged c11 carries `prev` deltas into records of `main` | R-FIX WP-20 with R-ORA WP-95, or R-SPEC-F WP-12; owner call |
| P2-2 | major | F03:84, F16:656-661, F17:358-359 | The maintenance byte has no liveness statement; a stalled holder suspends the tail bound | R-SPEC-P WP-11, WP-16; R-SPEC-F WP-18 |
| P2-3 | major | F12:656-664 | The walk from `CONFLICTS.commit` to the merge M has no cost bound | R-SPEC-F WP-12 (with R-SPEC-R WP-13 for one remedy); owner call |
| P2-4 | major | F12:1277-1283 | The per-merge text budget is decided after the freeze, though such a budget would be format-fixed | R-SPEC-F WP-12; owner call |
| P2-5 | minor | F16:251-256 | P-35 states no pointer to P-37's all-unknown fallback | R-SPEC-P WP-16 |
| P2-6 | minor | F16:204-210, F16:368-370, F16:377-381 | Three failure-dependent behaviours of [F16] name no FM item | R-SPEC-P WP-16 |
| P2-7 | minor | `docs/spec/os/fs.md:606-612`, `docs/spec/os/lock.md:389` | OS/fs, OS/lock and F15 restate restore's swap under both bytes, against P-85's order | R-SPEC-P WP-17, WP-16 |
| P2-8 | minor | F16:662-664, F16:749-754 | P-76 says no CLI process runs a purge; `BodyDrop`, a CLI verb, runs one | R-SPEC-P WP-16 |
| P2-9 | minor | F06:788-791, F06:756-758 | A known locator's read is bounded only by the extent | R-SPEC-F WP-12; owner call |
| P2-10 | minor | `docs/spec/config.md:852`, `docs/spec/lq/std.md:278-279` | The similar-records notice after every knowledge write names no budget | R-SPEC-F WP-18, WP-19 |
| P2-11 | minor | F11:1293, F11:1315-1319 | The harvest row's range list has no cap, and each read tests every range's reachability | R-SPEC-R WP-13, R-SPEC-F WP-25; owner call |

## 4. Findings by severity

### Blocker

**P2-1 — Store A's staged commit c11 carries `prev` deltas into records of `main` (blocker).**

**Where:** `fixtures/hex/store-a/log.1.hex:2804` (group g18 at lsn 9555, commit c11 on the staging ref
`merge/main/from/lane/l5np`, ref id 2, which g17 creates at `fixtures/hex/store-a/log.1.hex:2722`);
`fixtures/hex/store-a/log.1.hex:2865` (`prev` 3810, lsn 9555 − 5745) and `fixtures/hex/store-a/log.1.hex:2899` (`prev`
1478, lsn 9555 − 8077); `fixtures/hex/INDEX.md:97-98`; [F06 §7.3] at F06:724-726 and SL-2 at F06:777-780.

**Problem.** [F06 §7.3] makes `prev` per ref: 0 when the owner has no earlier op on this ref, otherwise the distance to
that ref's own newest record that touches the owner. SL-2 adds that on a lane `prev` and the locator never name a record
of another ref. c11 is the first commit on the staging ref (ref id 2 exists only from g17), so no owner has an earlier op
on that ref, and every `prev` of c11 should be 0 with no locator. Instead its two frames name lsn 5745, which is c3 on
`main` (`fixtures/hex/store-a/log.1.hex:1611`), and lsn 8077, which is c7 on `main`
(`fixtures/hex/store-a/log.1.hex:2389`). Two implementers, one following the rule and one following the fixture, write
different bytes for the same staged commit, and the fixture's bytes are hashed and pinned.

**Evidence.** The three group lines and the two `prev` lines cited above, read as text; `fixtures/hex/INDEX.md:97-98`
names g17 as the creation of the staging ref and g18 as c11 on it. Nothing in [F06 §7.3] or SL-2 makes a staging ref an
exception.

**Fix.** Two remedies; the review chooses neither:
- (fixture) R-FIX, WP-20: set c11's `prev`s to 0, so their locators vanish and store A's bytes ripple again from g18 on,
  with the oracle's matching change (R-ORA, WP-95), in their own landing;
- (rule) R-SPEC-F, WP-12: a staging-ref exception in [F06 §7.3] (a staging ref's first commit chains to the
  destination's records), stated with its locator and SL-2 consequences.

The rule remedy makes a format rule: owner call.

**Disposition:** open.

### Majors

**P2-2 — The maintenance byte has no liveness statement; a stalled holder suspends the tail bound (major).**

**Where:** [F03 §3] (F03:84); `WriterDiag`, writer only (F03:231-247); [F16] P-76 (F16:656-661); [F17 §5.2]
(F17:358-359, F17:366-374); [F17 §10] (F17:583); [F15] FM-6 (F15:438-441) and FM-8.1 (F15:488-499, F15:537); [OS/lock]
`probe` (`docs/spec/os/lock.md:103-105`); [F19 §10.2] `maintenance_busy` (F19:741).

**Problem.** The maintenance byte is only tried, never waited on, and has no bound (F17:583). When C1 holds and the byte
is busy, a process does nothing because "the holder keeps the tail bounded itself" (F17:358-359), and [F17 §5.2]'s bound
(F17:366-374) holds only while the holder runs its steps. The fault model lets the holder pause for any duration with its
bytes held (F15:438-441), and lets a dead holder's byte stay held, class (c), for the whole scenario (F15:488-499); a
gate may not read `Held` as "alive" (F15:537). So a paused, stalled or dying holder suspends every checkpoint of the
store: the tail and every process's overlay grow past P09 (P10 in quiet mode) with no detection, no probe, no clock and
no diagnostic, while every other process skips its checkpoint by the rule above. No text says what a process concludes
about such a holder. The diagnostic path is missing too: the kernel never reveals a holder ([OS/lock §3] item 6,
`docs/spec/os/lock.md:103-105`), `WriterDiag` serves only the writer byte (F03:231-247), and yet `maintenance_busy`'s
help says that `moirai doctor agents` names the holder (F19:741), with no record to name it from.

**Evidence.** The cited lines. [OS/proc §11]'s argument that lowered priority never lengthens the tail
(`docs/spec/os/proc.md:439-441`) assumes the job keeps running its steps; it says nothing of a job that stops.

**Fix.** R-SPEC-P, WP-11 for [F03] and WP-16 for [F16] and [F17]; R-SPEC-F, WP-18 for [F19]'s help text. Two remedies;
the review chooses neither:
- (a) a maintenance-holder record in `LOCK`'s reserved region (the holder's `ProcId`, its job and the hlc of its last
  step), written at each step boundary and read by a process that finds the byte busy while C1 holds, and by `doctor`,
  with a staleness bound past which the process reports the stall: adds a protocol decision, and the bound and the
  bytes are an owner call;
- (b) no new bytes: state the residual in [F17 §5.2] (a stalled maintenance holder suspends the bound for as long as it
  holds the byte), report it (a warning when C1 holds and the byte has stayed busy past a configured bound), and drop
  "names the holder" from F19:741: the bound is an owner call.

**Disposition:** open.

**P2-3 — The walk from `CONFLICTS.commit` to the merge M has no cost bound (major).**

**Where:** [F12 §6.5], the restore of a side's hierarchy key and out-edges from the merge M (F12:656-664); [F12 §5.5a]
(F12:479-494); `CONFLICTS.commit` (F11:730).

**Problem.** A `Resolve` that takes a `live` side restores the node's hierarchy key and out-edges from that side's state
at M, the merge that made the conflict value. A reader finds M by starting at the commit `CONFLICTS.commit` names and
moving to a parent whose state still holds an equal conflict value until no parent does (F12:658-662). Every commit
between M and that start holds the value, carried merges included (F12:662-664), so the walk visits each of them and
reads the key's state at each one; its length is the conflict's age in commits, which nothing bounds. [F12 §5.5a]
charges RS-007's origin walks to the merge's work budget and allows a per-node index for them (F12:486-494), but says
nothing of this walk, which runs inside a `Resolve`, not a merge. So the cost of a resolve is unbounded and unbudgeted,
and two implementations may refuse or not refuse the same old conflict. The tree implies no bound: [F17] and [F05] name
none for it.

**Evidence.** The cited lines. [F07]'s "introducing commit" wording at F07:1229 is a different, known defect (§5).

**Fix.** R-SPEC-F, WP-12. Two remedies; the review chooses neither:
- (a) the `CONFLICTS` row records M itself (a byte change in [F11 §10] before the freeze, R-SPEC-R WP-13 with
  R-SPEC-F WP-12), so no walk is needed;
- (b) the walk is charged to the `Resolve`'s work budget like RS-007's walks, with [F12 §5.5a]'s per-node index named
  for it and its cost measured on a long-lived lane.

The bound, and the byte change of (a), are an owner call.

**Disposition:** open.

**P2-4 — The per-merge text budget is decided after the freeze, though such a budget would be format-fixed (major).**

**Where:** [F12] open point 29 (F12:1277-1283); [F12 §7.5]'s measurement (F12:862-866).

**Problem.** One adversarial text key costs about 32 ms of diff3 at the 65,536-byte bound (F12:862-866), against
≤ 50 ms for the whole merge of a 2k-op lane at 1e5 (GT11), so two such keys exceed the gate. Open point 29 leaves the
choice to be decided "before GT11's merge row is written" (F12:1279-1280), which falls after the `format-v1` tag (the milestone of that row starts at the tag, `docs/m0/PLAN.md:343`), and itself says that
a budget that changes the merge result would have to be a format-fixed rule, since a merged text enters commit ids
(F12:1281-1283). A rule that must be format-fixed cannot be decided after the freeze, so the open point's schedule
contradicts its own condition.

**Evidence.** The cited lines.

**Fix.** R-SPEC-F, WP-12; owner call. Two remedies; the review chooses neither:
- (a) decide now that GT11 measures workloads only (the ordinary 2k-op lane, about 78 µs per text key) and that an
  adversarial text lies outside the gate, which needs no format rule;
- (b) state a per-merge text budget as a format-fixed rule before the tag, with its value and its effect on the result.

**Disposition:** open.

### Minors

**P2-5 — P-35 states no pointer to P-37's all-unknown fallback (minor).**

**Where:** [F16] P-35 (F16:251-256) and P-37 (F16:295-299).

**Problem.** P-35 says a commit that exceeds the inline bound on its final encoding re-runs on the bulk path or is
refused with E501, and that a group no extent can hold is refused with `commit_too_large`. P-37 then says that when the
re-check fails with the known locators and passes with every locator unknown, the appender writes every locator unknown
instead, so a locator never refuses a commit. A reader of P-35 alone takes its outcomes as final.

**Fix.** R-SPEC-P, WP-16: P-35 names P-37's fallback as the step that runs first.

**Disposition:** open.

**P2-6 — Three failure-dependent behaviours of [F16] name no FM item (minor).**

**Where:** [F16] P-29 (F16:204-210, a lazy tail lost after a crash or a failed flush), P-48 (F16:368-370, a publisher's
scan after a failed flush), P-50 (F16:377-381, `HEAD` reverting to an older slot after a crash).

**Problem.** Each behaviour was added since pass 1 and rests on a fault of [F15] (lost sectors after a crash, FM-1; a
failed flush, FM-3), yet none cites it, while [F16]'s neighbouring rules do (F16:309, F16:335). A reader cannot check
the behaviour against the fault's exact guarantee.

**Fix.** R-SPEC-P, WP-16: cite FM-1 and FM-3 (with their items) at the three rules.

**Disposition:** open.

**P2-7 — OS/fs, OS/lock and F15 restate restore's swap under both bytes, against P-85's order (minor).**

**Where:** `docs/spec/os/fs.md:416`, `docs/spec/os/fs.md:513`, `docs/spec/os/fs.md:606-612`;
`docs/spec/os/lock.md:389`; F15:808-809; against [F16] P-85 step 2 (F16:901-904) and F02:258-261.

**Problem.** [F16] P-85 makes `retired` durable under both bytes, releases both and closes every handle into the store
before the swap, because a Windows directory rename fails while a handle inside is open (F16:901-904; [OS/fs]'s own open
point 6, `docs/spec/os/fs.md:886`). [OS/fs]'s operation table (`docs/spec/os/fs.md:416`), its swap section
(`docs/spec/os/fs.md:513`), [OS/lock]'s sequence table ("try `Maintenance` → wait `Writer` → swap → release both",
`docs/spec/os/lock.md:389`) and [F15]'s swap postcondition (F15:808-809) still say the swap runs under the writer and
maintenance bytes. The new accepted-limit paragraph (`docs/spec/os/fs.md:606-610`) and the next one
(`docs/spec/os/fs.md:612`) say the old store gets `HEAD.retired` after `Completed`, while P-85 and F02:258-261 set it
before the swap intent exists.

**Fix.** R-SPEC-P, WP-17 for `os/fs.md` and `os/lock.md`, WP-16 for [F15]: state P-85's order (retired durable, both
bytes released, handles closed, then the swap) or cite P-85 instead of restating it.

**Disposition:** open.

**P2-8 — P-76 says no CLI process runs a purge; `BodyDrop`, a CLI verb, runs one (minor).**

**Where:** [F16 §12] P-76's paragraph (F16:662-664, again at F16:1527) and P-101's `BodyDrop` run (F16:749-754).

**Problem.** P-76 calls a purge "a long job of a rollup's size … which no CLI, hook or MCP process runs", while P-101
has the `BodyDrop` command, which is the CLI verb `moirai body drop`, run a pending purge after its acknowledgement.
The intent is that no automatic trigger runs one; the sentence says more.

**Fix.** R-SPEC-P, WP-16: say that no automatic trigger and no hook or MCP process runs a purge, and name `BodyDrop`,
`gc` and `backup` as the three explicit runs.

**Disposition:** open.

**P2-9 — A known locator's read is bounded only by the extent (minor).**

**Where:** [F06 §7.3.1] "Reading" (F06:788-791) and LV-1 (F06:756-758); C-1 (F17:213).

**Problem.** A reader reads the locator's `prev_len` bytes when they lie inside one extent, so a read is "never longer
than E − 32 bytes" (F06:791): up to 64 MiB at the production extent size for one hop of `prev`. LV-1 bounds
`prev_len` only by `prev`. A writer's prev run lies inside one inline record, which C-1 bounds by P05 ≤ P01 / 8
(F17:213), so a valid writer never needs more, but a reader that meets a corrupt or hostile `prev_len` reads up to the
extent, on a read path the lens wants short.

**Fix.** R-SPEC-F, WP-12: a validity bound on `prev_len` in LV-1 (for example `prev_len` ≤ P05), so a reader treats a
longer locator as unknown. The bound narrows which locators are valid: owner call.

**Disposition:** open.

**P2-10 — The similar-records notice after every knowledge write names no budget (minor).**

**Where:** `knowledge.similar-notice` (`docs/spec/config.md:852`); `similar()`'s work (`docs/spec/lq/std.md:278-279`).

**Problem.** From its milestone on, every `Tx`, `Mutation` or `Apply` that creates a rule, decision, note or finding, or
changes the title, the abstract or the kind's text field of one, runs `similar()` on the new tip, outside the writer byte, and "a budget cut gives no notice"
(`docs/spec/config.md:852`). Neither text names the budget: its class, its value, or the key that sets it, though
`similar()` counts one work unit per compared term over every candidate of the kind (`docs/spec/lq/std.md:278-279`).
The cost lands on the commit path of every knowledge write.

**Fix.** R-SPEC-F, WP-18 (and WP-19 for [LQ/std §2.16]): name the budget of the notice's computation (a class of
[LQ/std] or a work value) before the surface is reserved further.

**Disposition:** open.

**P2-11 — The harvest row's range list has no cap, and each read tests every range's reachability (minor).**

**Where:** [F11 §13.5] `ranges` (F11:1293) and its rules (F11:1315-1319); [API §8.8] (`docs/spec/store-api.md:915`,
`docs/spec/store-api.md:923`).

**Problem.** A `HARVEST` row appends a 40-byte range for every harvested batch and keeps it until a forget; nothing caps
the list. Liveness is computed at each read from each range's commit and the live refs, and every `Apply` with a
`harvest` member checks overlap against the live ranges of its row (`docs/spec/store-api.md:923`). So one transcript's
row grows without limit, and each `harvest()` read and each such `Apply` costs one reachability test per range. The
bytes are reserved before the freeze; a cap, if any, belongs with them.

**Fix.** R-SPEC-R, WP-13 for [F11], with R-SPEC-F, WP-25 for [API §8.8]: a cap on a row's ranges (a refusal, or the
merge of adjacent live ranges), or a statement that the cost is accepted. The cap's value is an owner call.

**Disposition:** open.

## 5. Known open findings met

Cited, not raised again:

- [F07]'s "the conflict's introducing commit" (F07:1229) against [F12 §6.5], RS-008 and [F12]'s open point 5 (c): the
  wording defect is known; P2-3 is a different defect (the walk's cost).
- [F17]'s "(§4.5)" pointer at F17:809-810, the broken line at F17:14, and the test value 4 at F17:556-557.
- [RULES/merge-table]'s cost note on the replay start (`docs/spec/rules/merge-table.md:1107-1108`): RS-007's replay is not bounded by
  [AR §5a.7] step 3's daily-path bound, and the merge's work budget must name it and the engine measure it. The lens
  agrees; [F12 §5.5a] carries the same charge for the origin walks.
- The status rows: most files under `docs/spec/` still read "draft, pass 1 pending".
- The old Klotho byte names in the fixtures, in their known state.

## 6. Checked and found correct

Pass 1's list holds for unchanged text and is not repeated. New or changed text checked here:

- Every offset table of the 76 files sums to its total (94 tables, 607 rows), the two-slot `HEAD` included
  (F04:41-78).
- The extent head: its group length H = 138 and the rotation reserve R = H + 40 = 178 (F05:191) agree with its record
  table (F05:1119-1136).
- [F06 §11]'s worked example: the `SetStatus` op with the known locator (F06:1327) and with the unknown locator
  (F06:1337) encode as printed, and the payload lengths 208 and 207 follow from them.
- [F21 §2.3]'s two scope byte strings (25 and 19 bytes, F21:124-126); [F08 §11.2]'s 46-byte `uid_file` input; [F14 §17.2]'s
  two computed uids (`docs/spec/format/14-image.md:1705`, `docs/spec/format/14-image.md:1727`).
- `BodyDrop`'s cap of 2,048 hashes (`docs/spec/store-api.md:851`): 32,768 bytes of hashes keep the record within the
  group bound of [F17 §4.4] at the test profile's 64 KiB extents, which F05:1191-1193 put at about 4,000 hashes.
- [F16] P-98 and [F17 §5.2]'s yield checkpoints bound the tail during a long maintenance holding while the job runs
  (F17:366-374); the residual case is P2-2.
- [RULES/policy-keys]'s value sets for the store parameters match [F17 §3]'s ranges, and [CFG §10]'s clock, budget,
  size and cap defaults match [F17] and the RAM rows.
- [F21]'s scanners keep bounded state and run in one pass without recursion; the lens raises no blocker or major on
  [F21].

## 7. Open points for the disposition

1. P2-1: the fixture remedy or the rule remedy (owner call).
2. P2-2: remedy (a), with new `LOCK` bytes and a protocol decision, or remedy (b), with a reported residual; and the
   staleness bound either way.
3. P2-3 and P2-4: the bound for the walk to M, and whether GT11 excludes adversarial texts or a per-merge text budget is
   fixed before the tag.
4. P2-9 and P2-11: whether the locator read and the harvest row get validity or size caps before the freeze.
5. Not raised again: the disk growth of a pending purge, which spec sync 3 decided, and `DOCLEN`'s fallback, which
   tokenises the whole view when the section is absent and waits for its hole (`F09-doclen`).
