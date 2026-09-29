# Review pass 1: closure check of the blocker and major findings

| | |
|---|---|
| Title | Closure check of every blocker and major finding of [A-pass1], [P-pass1] and [S-pass1] against the current specification text, round 3 |
| Status | review, pass 1 closure, round 3; the author of this file wrote none of the reviewed text and none of the dispositions |
| Work package | WP-80 pass 1 ([PLAN §3.2] item 8), closure checker |
| Inputs | `reviews/A-pass1.md`, `reviews/P-pass1.md`, `reviews/S-pass1.md`; `reviews/pass1-dispositions-F.md`, `-M.md`, `-P.md`, `-R.md` (each with its round-1, round-2 and round-3 tables and notes); `reviews/owner-questions.md`; the round-2 version of this file; every `docs/spec/` section a fix or a round-3 edit names; `COVERAGE.md`, `HOLES.md` |
| Scope | the 40 blockers and 54 majors (94 findings); the ten contradictions of rounds 1 and 2 (NC-1 to NC-10); new cross-chapter contradictions introduced or exposed by the round-3 edits; `COVERAGE.md`; the 61 minors only by disposition and spot check (§6) |

## 1. Verdict

**Every blocker and major finding of pass 1 is closed in the text; two new cross-chapter contradictions are open.** Of
the 94 findings, 78 are **closed**, 13 **closed, owner** (the text is done and an owner question records the
design-level call) and 3 **closed, interim** (the interim scanner rule; the scanner appendix is a freeze gate). None is
open. A1-23 moves from "closed" to "closed, owner": its last cursor now has bytes (NC-10), and who appends it is the new
OQ-F-3.

The three contradictions of round 2 are closed in every chapter they touch (§4.1): **NC-8** ([LQ/errors §5.5] E409 has
the lease case, cited by [F19 §10.5], [API §9.1] and DP-005), **NC-9** ([F06 §7.10] fixes the inverse `Delete` of a
`Create` at `reason` 0, `replaced_by` 0, cited by DM-017) and **NC-10** ([F05 §9.11] `feed` 2 with `task`, [F11 §13.1]
keyed (session, agent, feed, task), cited by PT-028 and PX-011). NC-1 to NC-7 stay closed.

The round-3 edits introduced **two new contradictions**, both major, both with a one-section fix that needs no owner
answer (§4.2):

- **NC-11** — [F05 §9.11] and [F11 §13.1] state "`pack T` for agent A appends one `feed` 2 record after emitting
  (PX-011)", while [F13 §3] I-F5 and [F18 §2.5] (`pack` appends no record of any kind, durable or lazy), [API §14.1]
  (`Query` appends nothing), [API] open point 48 and PX-011 as R-MODEL reworded it in round 3 (no M0 command appends a
  pack cursor until OQ-F-3 is answered) say it does not (R-SPEC-P for [F05], R-SPEC-R for [F11]);
- **NC-12** — [F11 §14]'s worked `MARKERS` row is a `settled` entry written "by a commit's ops under a lease", "not a
  complete" (`outcome` 0), with `actor` = 5 at offset 32; since round 3, [F05 §9.5] field 9, MF-009 and [API §15.7] set
  the holder only on the entry a `complete` writes and 0 on every other, so that row's `actor` bytes must be
  `00 00 00 00` (R-SPEC-R).

The owner questions of `owner-questions.md` (OQ-R-1 to OQ-R-4, OQ-P-1 to OQ-P-3, OQ-F-1 to OQ-F-3, OQ-M-1 (a)–(p),
OQ-M-2) are unchanged in substance except for the new OQ-F-3 and OQ-M-1 (n)–(p); NC-11's fix states OQ-F-3's interim and
decides nothing.

`COVERAGE.md`: 152 rows, 152 mapped, 0 CONFLICTING, 0 UNMAPPED; no row cites the two sides of NC-11 or NC-12 against
each other (§5).

## 2. Method

1. Read the three reviews, the four disposition files with their round-1, round-2 and round-3 tables and notes,
   `owner-questions.md` and the round-2 version of this file in full. Every finding id is disposed in round 3 by every
   role that owns a part of it (R-SPEC-F lists all 155; R-SPEC-P, R-SPEC-R and R-MODEL every id with a part in their
   files), and every round-2 closure item (NC-8 to NC-10 and the three editorial residues) by its owner.
2. For each blocker and major, re-read the sections its fix names in the current text, in the chapter that owns the
   bytes or the rule and in every chapter that restates or cites them, with particular care for the sections a round-3
   edit touched ([F05 §9.5], §9.11, open point 14; [F06 §7.10], open point 35; [F09 §7.2], §14.4; [F11 §1.3], §2.7,
   §2.8, §7, §13, §14, open points 1 and 42; [F12 §2.2], §8.2, open point 14; [F16] open points 1 and 10, §17.4;
   [F19 §10.5], open point 37; [F20 §1.2], §6.1, open points 8 and 31; [LQ/errors §2.2], §5.5, §5.7, open point 17;
   [API §6.7], §9.1, §15.7, open point 48; DP-005 to DP-007, DP-010, delete-policy-matrix §9 and open points 3, 9, 14;
   DM-004, DM-017 and merge-table open point 33; MF-009; PT-027, PT-028, PX-011 and pack-classes open point 8;
   `COVERAGE.md` §2 and rows 60-I2-Gate0, 60-I2-Derived, F15; the `README.md` [F11] row). **Closed** means every part of
   the fix is in the text, or a declined part is declined with a reason that holds against the design; where a finding
   offered alternatives, one suffices.
3. Re-read every rule row a round-3 edit added or changed (DP-010, DP-005 to DP-007, DM-017, MF-009, PT-027, PT-028,
   PX-011) against the chapter that owns what it cites, and every chapter a round-3 edit made cite a rule or text
   ([LQ/errors §5.5] against [F11 §6]'s `LEASES` order, [LQ/envelope §7.4]'s `lease` object, [LQ/std §7.2] `tx.rm`,
   [LQ/grammar-v1.ebnf] and [LQ/canonical-ast] `RELEASE`, [F08 §11.3]; [F06 §7.10] against [F06 §7.4], [F07 §7.2], §13,
   [F14 §6.10]; [F05 §9.11] and [F11 §13] against [F13 §3] I-F5, [F18 §2.5], [API §14.1], [LQ/std §4.20], §5;
   [F05 §9.5] and [F11 §7] against [API §10.5], §15.7 and the store-api examples; [F20 §6.1] against [F19 §10.2]
   `anchor_spec`; [F09 §14.4] against [F16] P-87; [API §6.7] against [F05 §9.27], §10.2; [F12 §2.2] against [F16] P-70
   and [F11 §3.2]; [F11 §2.8] against [F09 §3.1]; [F20 §1.2] against [F11 §12.2], [OS/clock], [OS/project §4.2]).
4. Mechanical checks by script in the checker's scratch space (not committed):
   - 5,479 section citations `[Fnn §x]`, `[OS/… §x]`, `[LQ/… §x]`, `[API §x]`, `[CFG §x]`, `[RULES/… §x]` in
     `docs/spec/**` outside `reviews/` against the headings, numbered paragraphs and bold paragraphs of their targets:
     40 flagged, all the `.ebnf` annex forms (`§n`, `§P.n`, `§G.n`, `§R`, `§X`) that [LQ/grammar-v1.ebnf]'s header
     defines; rule-file citations: only the planned `[RULES/policy-keys]` and `[RULES/SIGNED]` and the corrected name in
     a [CFG] open point miss;
   - every rule-row id `XX-nnn` cited anywhere in `docs/spec/` outside `reviews/` exists in a rule file (1,953 row ids
     defined, DP-010 included; the one miss is the template row `SG-001` in [RULES/README]'s signatures table);
   - [RULES/README §7]: 111 `<!-- table: … -->` markers and 111 registry rows with the same names and files (the
     `signatures` marker is the README's template of `SIGNED.md`);
   - [F16]: P-1 to P-100 defined and no `P-n` above 100 cited; §17.4 defines L-1 to L-9, and open point 1's count
     (three toy, five GT18, L-9 at M1) matches;
   - every `HOLE(…)` id outside `reviews/` is indexed in `HOLES.md`; no hole was added, renamed or removed in round 3;
   - every "[F19 §10.2] `code`" citation names one of the 61 codes of [F19 §10.2]; the `anchor_spec` cases [F20 §6.1]
     now names (`binary`, `range`, `fffd`, `empty`, `not-found`, `no-scanner`) are rows of [F19 §10.2] and cases of
     [F19 §10.3], exit 2;
   - the tables whose bytes or meaning changed in round 3 re-summed: [F11 §13.1] `CURSORS` 56 B (`task` `u32` at 36,
     `_reserved` 33–35), [F11 §7] `MARKERS` 72 B, [F09 §7.2] `EDGE_PROPS` 40 B; [F05 §9.11]'s field order (`feed`,
     `task`, `cursor_seq`, `hlc`) matches the row, and no text cites a `Lazy` field by its old number.
5. Searched every chapter for old values of each datum round 3 changed: the `CURSORS` key without `task` or with bytes
   36–39 reserved, `feed` 1 as the only value, "the author of `commit`" as a marker's `actor`, a holder on a re-emitted
   or non-`complete` `settled` entry, E409 without the lease or replacement case, `Create` ↔ `Delete`, "recovery" as the
   parker, "the other allocation counters", "[F19] needs its text": every hit is a closed open point or a sentence that
   records the change, except the two statements of NC-11 and the worked example of NC-12.

## 3. Closure table

Status: **closed**, **closed, owner** (text done; an owner question is pending), **closed, interim** (the interim rule the
finding allows is in the text; the full fix is a freeze gate), **open**.

| Finding | Severity | Status | Note |
|---|---|---|---|
| A1-1 | blocker | closed | [F08 §5.1] is the one value registry (type 0 `absent`, `commitref` `b32`, `pathmove` classes 1–4, empty = absent); [F06 §5.1]–§5.5 cite it and define no tag; [F09 §10.1]'s promoted `commitref` element is an `id16` index key, and [F09 §10.2] makes a set row's promoted elements unique by it; the "empty is absent" rows (CS-001, RS-004 to RS-006, RS-013) agree |
| A1-2 | blocker | closed | [F06 §7.5.3] cites [F08 §10.3] byte for byte; `blob` `none` allowed for a planned target; [F14 §5.6] is the bijective text image of [F08 §10.3.1]'s bytes (written only for imported anchors while the interim rule holds); link-merge-rules §2 names `end_h` and `text_unavailable` |
| A1-3 | blocker | closed | [F09 §7.2] `EDGE_PROPS` 40 B (`pflags` bit 0 `has_pin`, bit 1 `flagged`, full 32-byte pin). Round 3: "Composition" rebuilds [F07 §8.1]'s per-key blocks exactly — a non-`at` edge's block is its row's `pflags` (`00` without a row) and the pin; an `at` CSR entry stands for one key per `ANCHORS` row, each block `04` plus that anchor record, as [F08 §10.2] requires of kind `at`. Row sizes confirmed by OQ-R-4 |
| A1-4 | blocker | closed | [F06 §7.4] op 16 `CreateDeleted`, NF-11, no inverse; [F07 §10.1], §13; [F12 §7.8]; [F14 §11.2]; [F09 §16.4]; [F06 §9] BK-5. Round 3 (NC-9): [F06 §7.10] gives the inverse `Delete` of a `Create` `reason` 0 and `replaced_by` 0, so a reverted creation leaves `deleted(kind, "", none)` ([F07 §7.2], §13; [F14 §6.10] omits the empty `field reason:` line); DM-017 and merge-table open point 33 quote it; `Undelete` → `Delete` takes the `Undelete`'s `reason`, `replaced_by` and image, as delete-policy-matrix §9 now says |
| A1-5 | blocker | closed | [F06 §4.4.15] `pruned`; [F10 §4.6] writes it; DM-016 refuses a revert or cherry-pick of a pruned commit with `commit_pruned`, exit 3, as [F06 §4.4.15], §7.10 and [F19 §10.2] do |
| A1-6 | blocker | closed | [F11 §2.9] row images; per-table field sources ([F11 §3.9] incl. reason 5 `park`; `PINS` holder 1–4; `TREES` `scope_kind` 0–2; `LEASES.session_role` from [F05 §9.4] field 27; `FSINTENT` codes; `GITRENAMES` from [F05 §9.25]). Round 3: `MARKERS.actor` takes exactly [F05 §9.5] field 9 `holder`, which (with field 11 `outcome`) is set only on the `settled` entry the commit of a `complete` writes and is 0 on every other entry, re-emits included; MF-009, [API §10.5] step 2 and [API §15.7] `markers` (`actor` null where the row's is 0) state the same set. The worked row of [F11 §14] does not follow it: NC-12 |
| A1-7 | blocker | closed | [F11 §12.5] cites [F18 §4.2] (stored 1–4, 6, 8, 12; 5, 9–11 rendered); [F18 §4.10] agrees |
| A1-8 | blocker | closed | [F11 §5] `HEADS` (161 B) embeds [F18 §3.2]'s 40-byte `BindingExt` at offset 105; [F05 §9.3] carries the row image; the full-name spelling is declined with a reason that holds (canonical item 5 hashes the short form) |
| A1-9 | blocker | closed | `prov` in [F06 §6.2], §7.7, [F07 §7.3], [F11 §10] offset 5; merge-table §2 "Values" names it |
| A1-10 | blocker | closed | [F06 §4.4.14] `ckimg`; presence bit 17 has one condition in [F06 §4.2] and [F09 §16.4] `CKIMG`; [F06 §9] BK-5, [F14 §11.3], [F10 §8] |
| A1-11 | blocker | closed | [F05 §9.2] reason 5 `park`, §9.10, §10.2, §10.3; [F16] P-70 and its bug; [F12 §8.2]; [F11 §3.9], §3.2 kind 6; [API §15.7] `moves`; VK-007 and [F13 §4.1]. Round 3: [F12 §2.2]'s ref-kind row and paragraph, §8.2, open point 14 and [API §15.7] name the parker as [F16] P-70 and [F11 §3.2] do (the first appender whose scan meets a commit whose ref CAS failed) |
| A1-12 | blocker | closed | [F05 §7] kind 27 `Reserve`, §9.27, §4.7, §10.2; [F09 §14.4] `FILES.flags` bit 0 `reserved` (round 3: a `reserved` row is no part of any view; the file enters a view through its `Commit`, which is when [F16] P-87 step 2 copies it); [F11 §9.1]; [F13 §3] I35′; [API §6.7] (round 3: a surviving reservation moves `next_id` and `next_anchor` only, as [F05 §10.2]'s `Reserve` fold; `commit_seq`, `fence`, `next_ref_id` stay), §9.10, §15.7, §16.4 |
| A1-13 | blocker | closed | [F17 §2.1] = [F04 §4.4] (`store_id` `b16` at 16–31); IP-2 non-zero, IP-1 keeps it |
| A1-14 | blocker | closed, interim | [F08 §10.3.1] and [F20 §6.1] interim scanner rule (no scope; `path::A/B` and `path#H` refused with [F19 §10.2] `anchor_spec` case `no-scanner`; imported anchors kept); [API] example 13 captures a `quote` anchor; [F18 §4.7] and [F14 §17.2] mark their `symbol` examples as imported. Scanner appendix: freeze gate (OQ-R-2) |
| A1-15 | blocker | closed | `HEAD.project_oid_algo` `u8` at 1072 ([F04 §3.1], §5.16, §7, §8.1); [F17 §2.1]–§2.2 IP-1–IP-3; [F05 §9.28]; [F20 §2.3]; [CFG §7.6] |
| A1-16 | blocker | closed | [F19 §8.3] `c` + 64 lower-case hex; [LQ/errors §5.7] and [LQ/envelope §7.3] agree |
| A1-17 | blocker | closed | [F16] P-36 = [API §6.2] CK-4 over `hlc_seq` and `hlc_commit`; the records that never advance the sequence (`Checkpoint`, `Reserve`, `Lazy`, `SessionMark`, runtime records) are the same list in [F06 §4.4.4], [F05 §10.2], [F04 §5.15], [OS/clock §7], [API §6.2] |
| A1-18 | major | closed | [OS/fs §4.9.4]: only `doctor` and `restore` run `swap_recover`; [F02] open point 17 closed |
| A1-19 | major | closed | [F12 §7.3] cites [F07 §7.3]; merge-table §2 states the same equality |
| A1-20 | major | closed | [F08 §8.3], [F06 §7.6] |
| A1-21 | major | closed | `cs_ref.b3` = `seg_digest[0..16]` in [F06 §9] BK-2, [F09 §16.4], [F10 §8]; [F09 §14.4] `digest16` and [F04 §4.1] `SegRef.blake3_16` hold the same value |
| A1-22 | major | closed | [F10 §8]: rows, `VIOLATIONS`, `CKIMG`; no op list; OP-10-11 records the earlier `OPS` wording as withdrawn |
| A1-23 | major | closed, owner | [F11 §13] `CURSORS`, `SESSMARKS`, `BACKUPS` (56 B, tags `0x020A`–`0x020C`); [F05 §7] fold targets, §10.3. Round 3 (NC-10): the per-(agent, task) pack cursor of [AR §7.4] C8 has bytes — [F05 §9.11] `feed` 2 with field 9 `task` (`#N`, never 0) and [F11 §13.1] keyed (session, agent, feed, task), `task` at offset 36; PT-028 and PX-011 cite both. Who appends it is OQ-F-3 ([40] I-F5 against PX-011); the interim (no M0 command appends one) is in [API] open point 48 and PX-011 but not yet in [F05 §9.11] and [F11 §13.1]: NC-11 |
| A1-24 | major | closed | [F05 §2.2], [F17 §2.2] IP-6, [F16] P-72; [OS/fs §4.5] re-preparation sets the length for every method |
| A1-25 | major | closed | [F04 §10] = [F16] P-88 |
| A1-26 | major | closed, owner | [F17 §8.2], OP-17-15; DS-010 and RE-010 state the same reading, and `COVERAGE.md` rows F15 and 60-I2-Derived now cite them (round 3); OQ-P-1, OQ-M-1 (k) |
| A1-27 | major | closed | [F16 §17.4] L-1–L-8; round 3 adds L-9 for [F02 §3.6]'s retired store (vehicle M1), and [F16] open point 10 now names [F02 §3.6] and [F19 §10.2] `store_retired` as the text |
| A1-28 | major | closed | [F15 §6.5] FM-3 and FM-5 rows describe the refusal path, no copy |
| A1-29 | major | closed | [F19 §4.6] rule 5: every marker ≤ 50 B; RN-008 cites it |
| A1-30 | major | closed | [LQ/envelope §3.2], §3.3, §9.3 |
| A1-31 | major | closed, owner | [LQ/envelope §6.5] cites [F19 §6.2]; OQ-F-1 |
| A1-32 | major | closed | [LQ/errors §3.3]–§3.4 |
| A1-33 | major | closed | [OS/fs §6.1] five `ProjectFs` kinds with §6.2 rows; [OS/project §2.3]; [F20 §2.4] item 3 maps each kind, which [F18 §4.6] and [OS/project §5.5] support |
| A1-34 | major | closed | [F20 §5.11.4] normative `gs` with five golden vectors (68, 85, 97, 48, 50; unchanged) |
| A1-35 | major | closed | 111 table markers, 111 registry rows; R-MODEL's parser run finds 110 data tables and 1,953 row ids with DP-010, no error |
| A1-36 | major | closed | [LQ/std §7.3]; CO-001–CO-003 |
| A1-37 | major | closed | [LQ/std §2.4], §7.3 `$ttl: text? = NULL` |
| A1-38 | major | closed, owner | [LQ/std §4.1] `priority, id`; [API §16.6]; OQ-F-2 for [50 §4.1] |
| A1-39 | major | closed | [F19 §10.2]–§10.6; the rule rows name these codes with [F19]'s exits. Round 3 (NC-8): [LQ/errors §5.5] E409 has four cases — root node (DP-010), live lease (DP-005; [AR §5d.3]'s `leased by dev#2 on lane/y (L-19)`, first lease in [F11 §6]'s `LEASES` order, help naming `RELEASE`, which [LQ/grammar-v1.ebnf] and [LQ/std §7.2] `tx.rm` have), restricted references (DP-006) and replacement (DP-007) — printed in [RULES/delete-policy-matrix §4]'s order; §5.7 `leases` key with [LQ/envelope §7.4]'s `lease` members; [F19 §10.5] lists the root-node, lease and replacement refusals; [API §9.1]'s E409 row names DP-005 to DP-007. [F20 §6.1] now names the `anchor_spec` case of each capture refusal, all rows of [F19 §10.2] |
| A1-40 | major | closed | [OS/env §5] `tmp/probe.<nonce>` (nonces a, b, c), Appendix A; [F02 §5.3], §6.3; [F16] P-79 |
| A1-41 | major | closed | [F08 §8.5] item key order; [F09 §8.3] cites it |
| P1-1 | blocker | closed | As A1-1, A1-2, A1-3, A1-9, S1-7. [F08 §5.6] gives one value per type with its stored bytes and `cv`; row 13 is a valid `pathmove`; `COVERAGE.md` 60-AR-Values and R-4 name R-FIX's fixtures |
| P1-2 | blocker | closed | As A1-6, A1-7, A1-8; R-15's fixture bytes are [F18 §3.2]'s (row offsets 105–144) |
| P1-3 | blocker | closed | As A1-11, A1-12 (with the round-3 residues listed there: the [API §6.7] counters, the parker wording, [F09 §14.4] against P-87) |
| P1-4 | blocker | closed | As A1-13, A1-15 |
| P1-5 | blocker | closed | As A1-17 |
| P1-6 | blocker | closed | As A1-21 |
| P1-7 | major | closed | [F16] P-96 (spare under `tmp/extent.<nonce>` at E/2, maintenance byte only), P-72 step 2, bug; [F02 §5.3], §6.3 word `extent`; [OS/fs §4.5]; [F17 §13.2]; [F17 §4.1] names the extent head after the pad |
| P1-8 | major | closed | Kind 28 `ExtentHead` ([F05 §4.5], §9.28; H = 138); [F16] P-85, P-97; [F04 §6], §8.1; [API §6.2] CK-6; [F10 §4.1] keeps the head, whose `chain_in` replaces the dropped pad's trailer |
| P1-9 | major | closed | [F16] P-98, P-87; [F17 §5.2], measurement 10; C-4 `2 + P14 + n_other ≤ 8`, P14 1–5 in [F17 §3], OP-17-06 and [F04] open point 4; [F10 §7.1] 1 + P14 (2 + P14 while a job yields); [OS/proc §11] |
| P1-10 | major | closed | [F03 §3], §3.1 nine quiet bytes; [OS/lock §2], §8; L-8; [F19 §10.2] `store_locked` `quiet`; PH-007 and DE-027 follow [F17 §5.3] |
| P1-11 | major | closed, owner | [CFG §10.5] `wmem` budget, `tx.wmem-max` retired (§6.4); [F17 §1.5], §4.4 W2, W4, §12 TP-3; OQ-P-2 |
| P1-12 | major | closed | [F17 §3], [CFG §5.3], §7.6: `init` refuses C-1–C-4 failures; bounded fallback; examples 01 and 02 pass `store.commit.inline-max-bytes=4KiB` |
| P1-13 | major | closed | As A1-18 |
| P1-14 | major | closed | [OS/README §4.6] `Entropy::fill_random`; [OS/README §3] lists it |
| P1-15 | major | closed | [OS/project §2.3], [OS/path §6], §8.1; [F20 §4.9]; [F18 §4.6] detail 44 |
| P1-16 | major | closed | [OS/project §6.2], §4.2 bit 14 `dir_flush_doubtful`, §9 FL-2; [OS/fs §6.2] `Unsupported` and `AccessDenied` rows; [F16] P-71, P-91; [API §12.4] step 1; [F19 §10.2] `no_dir_flush`; [F11 §12.3] bit 14, bits 15–31 reserved-zero, equal to [OS/project §4.2] |
| P1-17 | major | closed, owner | [F14 §14.1] `aliases/<h1>.moi`; OQ-R-1; measurement 8's timing for WP-81a |
| P1-18 | major | closed | [F16] P-100 with §3 row and bug; P-23; §13.5 |
| P1-19 | major | closed | [F07 §10.6]; [F17 §4.4] W1 lists `sync`; GT11's case at WP-81a |
| P1-20 | major | closed, interim | As A1-14 |
| P1-21 | major | closed, owner | MR-005 (RS-015), VA-005, VA-007, VA-008, VA-013 (classes 67, 68, 72), LM-013 (LR-006); LL-014 `derived` and unreachable ([F03 §10.3], [F11 §6]); no rule file has a `gap` basis; with DP-010 the case LM-013 needs stays unreachable under `--cascade` too; OQ-M-1 (e)–(g), (l) |
| P1-22 | major | closed | [F11 §12.8] `FPRINT.file`; [F10 §5.5]; [F10 §7.1] and [F17 §6.1] state the same page bound (1 + P14, 2 + P14 while a job yields) |
| S1-1 | blocker | closed | As A1-1; ±infinity refused everywhere; one stored set order ([F08 §5.3], §5.5); CP-003 cites [F08 §5.5]'s `pathmove` order |
| S1-2 | blocker | closed | As A1-3 (with the round-3 precision of [F09 §7.2] on `at` blocks) |
| S1-3 | blocker | closed | As A1-2 |
| S1-4 | blocker | closed, interim | As A1-14 (S1-4's own fallback, "scope leaves `captured`", is what the interim rule does until OQ-R-2 is answered) |
| S1-5 | blocker | closed | As A1-9 |
| S1-6 | blocker | closed | As A1-4 (with NC-9's bytes) |
| S1-7 | blocker | closed | [F11 §10] `CONFLICTS`: `ckey`, the [F06 §6.2] `kval` sides, `prov`, `n` = 0 for schema keys; [F11 §2.4] `RtHdr.form`; [F11 §2.8] now gives tag `0x0031` as [F09 §3.1] does |
| S1-8 | blocker | closed | As A1-6; epochs 0/1/2 of [40 §2.6] with `partial` covering brief, `--scope`, `--path` |
| S1-9 | blocker | closed | As A1-7; proposal tuple (class 1–4, evidence 13–26, rooted path, exact score); anchor state 6 never stored |
| S1-10 | blocker | closed | As A1-8 |
| S1-11 | blocker | closed | As P1-3; holder-3 merge pins in the checkpoint group ([F05 §4.7], [F16] P-81) |
| S1-12 | blocker | closed | As A1-13 |
| S1-13 | blocker | closed | As A1-17 |
| S1-14 | blocker | closed, owner | RK-004–RK-006, RK-010, RK-011; [F12 §7.6] and [F08 §11.2] cite RK-001 to RK-011; OQ-M-1 (a) |
| S1-15 | blocker | closed, owner | MR-001, MR-003, MR-004, MR-002 order, CS-006, VB-018, [F13 §3.5] I31′, [F12 §5.4]; OQ-M-1 (b) |
| S1-16 | blocker | closed, owner | [F13 §4.1]–§4.2, [F11 §7], [F05 §9.5], [F16] P-52, P-65, [API §15.7] `markers` agree; round 3 aligns the holder and outcome of an entry across [F05 §9.5] fields 9 and 11, MF-009, [F11 §7] and [API §15.7] (the [F11 §14] example is NC-12); OQ-M-1 (c), (o) |
| S1-17 | blocker | closed | As A1-16 |
| S1-18 | major | closed | [F05 §4.4] G-2–G-5 with R = H + 40 = 178; [F17 §4.4] W3, §13.2; [F17 §4.1]; [F06 §4.6] cites [F05]'s reserve |
| S1-19 | major | closed | As A1-21 |
| S1-20 | major | closed | [F16] P-34 "by key; a bulk commit by node"; [F06 §7.3]; [F09 §16.4] `PREV` |
| S1-21 | major | closed | As A1-5 |
| S1-22 | major | closed, owner | [F12 §6.5]; [F14 §6.8.1]; RS-008 and PR-013; OQ-M-1 (h) |
| S1-23 | major | closed | As A1-10; [F06 §4.4.11] zero `first`/`last` when `n_folded` = 0 ([F14] open point 19) |
| S1-24 | major | closed | As A1-24 |
| S1-25 | major | closed | [F16] P-92, P-29, P-42, P-66, P-85; [F05 §5.3] (appender, flush holder, boot recovery, `repair`); [F15] FM-12.4; [F19 §10.2] `store_io_fault`; [OS/fs §6.2] `Io`: the same four writer scans in all four chapters |
| S1-26 | major | closed | As A1-18 |
| S1-27 | major | closed | As P1-14 |
| S1-28 | major | closed | As A1-15 |
| S1-29 | major | closed | [F17 §4.3] cites [F10 §4.2] as the one frame rule |
| S1-30 | major | closed, owner | [F13 §6.2] `open_blockers` counts `blocks` only, `gated`; [F08 §3.4]; BT rows; OQ-M-1 (d) |
| S1-31 | major | closed, owner | [F20] Holes and `HOLES.md` `F20-btime-ntfs`: candidate `Absent`, measurement 15's copy paths; OQ-R-3 |

## 4. Contradictions

### 4.1 The contradictions of rounds 1 and 2

| Id | Status | Where it is closed |
|---|---|---|
| NC-1 | closed | [F11 §12.3] `flags` bit 14 `dir_flush_doubtful`, bits 15–31 reserved-zero, as [OS/project §4.2], §4.3 (unchanged in round 3) |
| NC-2 | closed | [API] example `13-file-mv.json` step n = 8 captures `docs/api.md:3` as a `quote` anchor re-derived from [F20 §2.5] and §6.1 (unchanged) |
| NC-3 | closed | [OS/fs §6.2] row `Io` = [F16] P-92 = [F05 §5.3] = [F15] FM-12.4 (unchanged) |
| NC-4 | closed | [F04] open point 4: P14 ≤ 5, as [F17 §3] C-4 and OP-17-06 (unchanged) |
| NC-5 | closed | [F08 §5.6] row 13 `project:a/` → `project:b/` with its stored bytes and `cv` (unchanged) |
| NC-6 | closed | [F06 §2.4], §4.1; [F19 §10.2] `store_corrupt` with `moirai doctor --fsck` (unchanged) |
| NC-7 | closed | [F06 §4.2] bit 17 = [F09 §16.4] `CKIMG` (unchanged) |
| NC-8 | closed | [LQ/errors §5.5] E409 lease case (`statement <i>: DELETE <id> refused: leased by <holder> on <ref> (<lease>)`, the first live task lease on the deleted set in [F11 §6]'s (`n`, `lease_id`) order, the leased node's id first when it is not the target; the other leases as detail lines; help naming `RELEASE`), with the replacement case and the root-node case; the paragraph after the table orders the four cases DP-010, DP-005, DP-006, DP-007, as [RULES/delete-policy-matrix §4]'s line order; §2.2 `<lease>`, `<holder>`; §5.7 `leases`; [F19 §10.5] rows; [API §9.1]; DP-005, DP-006, DP-007 and DP-010 cite the texts (R-SPEC-F, with R-MODEL's DP-010) |
| NC-9 | closed | [F06 §7.10]: the inverse `Delete` of a `Create` has `reason` 0 and `replaced_by` 0, so a reverted creation leaves `deleted(kind, "", none)` ([F07 §7.2], §13); [F14 §6.10] writes it with no `field reason:` line; `Delete` ↔ `Undelete` each take the other's `reason`, `replaced_by` and image; DM-017, merge-table open point 33 and delete-policy-matrix §9 state the same; [F06] open point 35 (R-SPEC-F, R-MODEL) |
| NC-10 | closed | [F05 §9.11] `Lazy` `sub` 2: `feed` 2 = the C8 pack cursor, field 9 `task` (`nodeid`, never 0), field 10 `cursor_seq` = the pack's `rev`, field 11 `hlc`; [F11 §13.1] `CURSORS` (56 B) keyed (session, agent, feed, task), `task` `u32` at 36, a `feed` 2 row with `task` 0 invalid; [F11 §1.3], §13, open point 42; [F05] open point 14; PT-028 (the row of the session the pack runs in) and PX-011 cite both (R-SPEC-P, R-SPEC-R, R-MODEL). Who appends the record is OQ-F-3; the chapters' statement of it is NC-11 |

The three editorial residues of round 2 are gone: [F16] open point 10 names [F02 §3.6] and [F19 §10.2] `store_retired`
and §17.4 L-9 seeds the loop; [API §6.7] names `next_id` and `next_anchor` as the only counters a surviving reservation
moves; [F12 §2.2], §8.2 and [API §15.7] name the parker as [F16] P-70 and [F11 §3.2] do.

### 4.2 New contradictions of round 3

Each is a datum or rule that a round-3 edit stated in one chapter differently from another. The owner of the fix is the
role that owns the text that must change.

| Id | Severity | Where | The two statements | Fix (owning role) |
|---|---|---|---|---|
| NC-11 | major | [F05 §9.11] closing paragraph and [F11 §13.1] "The pack cursor" (round 3) against [F13 §3] I-F5, [F18 §2.5], [API §14.1], [API] open point 48 and [RULES/pack-classes] PX-011 (reworded by R-MODEL in round 3) | [F05 §9.11]: "`pack T` for agent A appends one `feed` 2 record after emitting (PX-011)"; [F11 §13.1] says the same. [F18 §2.5] (I-F5): `pack` is a read verb that "appends no log record of any kind, durable or lazy", and [F13 §3] gives I-F5 the check "a read path takes no writer role and appends no record" and the trace predicate `r4::trace::if5_reads_append_nothing`; [API §14.1] maps `pack` to `Query`, which appends nothing; PX-011 now says that whether the verb or the delivering layer appends is the owner's call and that "until then no M0 command appends it, so C8 is empty in the engine and the model alike", which [API] open point 48 also states. An engine built from [F05] and [F11] appends the record, fails I-F5's trace check, and renders a non-empty C8 on the next pack while the model's stays empty (GT2); OQ-F-3 records the design conflict but the two format chapters decide it for option (a) | [F05 §9.11] and [F11 §13.1] replace the sentence with OQ-F-3's interim: the `feed` 2 record's bytes and fold are as stated; which actor appends it is [owner-questions] OQ-F-3's call; until it is answered no M0 command appends one ([API] open point 48, PX-011), so cursor(A, T) is absent and C8 empty. [F05] open point 14 and [F11] open point 42 ("which PX-011 appends") follow. No byte changes (R-SPEC-P owns [F05]; R-SPEC-R aligns [F11]; R-SPEC-F updates [API] open point 48's description of PX-011, which since round 3 no longer has `pack` append) |
| NC-12 | major | [F11 §14] worked example (informative) and [F11] open point 1 against [F05 §9.5] field 9, [F11 §7] `actor`, [RULES/state-definition] MF-009 and [API §15.7] `markers` (all round 3) | The example is a `settled` row "written by a commit's ops under a lease whose holder is actor symbol 5", "(no `complete` outcome)", "outcome 0 (none: not a complete)", with bytes `05 00 00 00` at offset 32 (`actor` = 5). Since round 3, [F05 §9.5] field 9 carries a holder only "for the entry that the commit of a `complete` writes for the task it settles … 0 for every other entry", MF-009 names "the other doors of ME-001" among the entries that carry 0, [F11 §7] takes `actor` from that field, and [API §15.7] prints `actor` null on every entry but that one. A non-`complete` settling commit therefore stores `actor` 0; the example stores 5, and an engine or fixture author following the example writes a byte the model never produces. [F11] open point 1 ("the lease holder of a leased completion, else 0") reads as the broader set as well | [F11 §14]: either keep "not a complete" and write `actor` = 0 (`00 00 00 00` at offset 32, the comment `actor = 0 (not a complete)`), or make the row a `complete`'s (`outcome` 1 `done`, byte 36 `01`, the prose "written by the commit of `complete --outcome done` under a lease whose holder is actor symbol 5"); [F11] open point 1 says "the holder of the lease a `complete` presented, else 0", as [F05 §9.5] field 9 (R-SPEC-R) |

**Editorial residues** (no byte or rule consequence; listed so pass 2 need not rediscover them):

- [API §9.1]'s E409 row names DP-005 to DP-007 but not the root-node case of DP-010 ([F08 §11.3]), which [LQ/errors
  §5.5] and [F19 §10.5] carry; R-MODEL's round-3 note (2) offers the addition (R-SPEC-F, optional).
- [RULES/delete-policy-matrix] DS-001 (step 1, "the DP rows, in order, on the candidate") runs before DS-002 (step 2,
  the deleted set), while DP-005 and the new DP-010 test the deleted set; DP-010's note says the set is defined once
  DP-004 has passed. The step table could say that step 2's set is computed inside step 1 after DP-004 (R-MODEL).
- [owner-questions] OQ-F-3 describes PX-011 as having "the `pack` verb itself append" the cursor, which was PX-011's
  round-2 wording; the question stands, and the owner should read PX-011's current interim beside it (R-SPEC-F).

**By role.** R-SPEC-P: NC-11 ([F05 §9.11], open point 14). R-SPEC-R: NC-11 ([F11 §13.1], open point 42), NC-12
([F11 §14], open point 1). R-SPEC-F: [API] open point 48's PX-011 wording (NC-11), the [API §9.1] and OQ-F-3 residues.
R-MODEL: the DS-001/DS-002 residue; no rule row changes for NC-11 or NC-12. Owner: the questions of [owner-questions];
the closures marked "closed, owner" and "closed, interim" depend on them (OQ-R-2's scanner appendix is a freeze gate,
not a pass-1 item).

## 5. `COVERAGE.md` state

- **UNMAPPED: none. CONFLICTING: none marked.** 152 rows (§3–§11): 1 + 22 + 27 + 40 + 7 + 18 + 18 + 12 + 7, all mapped;
  §2's summary matches the rows and records the round-3 check. The six rows pass 1 marked CONFLICTING carry "no longer
  CONFLICTING (round 1)" and cite sections that agree.
- Round 3's row edits are in the text: 60-I2-Gate0 cites [F06 §7.10]'s inverse `Delete` of a `Create` for its revert
  fixture (NC-9); F15 and 60-I2-Derived cite RE-010 and DS-010 beside [F13 §6.3] (A1-26).
- **NC-11, NC-12 and `COVERAGE.md`:** no row cites both sides of either. R-12 (I-F1–I-F14) cites [F18 §2] and [F13]
  for I-F5 but not [F05 §9.11] or [F11 §13.1]; 60-AR-Log-kinds cites [F05 §9] §9.1–§9.12 (so §9.11) but no I-F5
  section; 60-AU-Seg-markers cites [F11 §7] and [F05 §9.5], which agree, and not the informative [F11 §14]. The rule of
  `COVERAGE.md` §1 therefore requires no mark. R-12 is the row whose frozen item (I-F5) NC-11 touches; once NC-11 is
  fixed, a row that maps [AR §7.4]'s C8 may cite [F05 §9.11] and [F11 §13.1] beside PT-028, as R-SPEC-P and R-SPEC-R
  suggested.
- The fixture and model-function columns are empty, as expected before WP-20 and WP-90; rows 60-AR-Values, R-4, R-15
  and 60-I2-Gate0 state the cross-chapter fixtures pass 1 requires, and 60-I2-Gate0's revert fixture now has NC-9's
  bytes.

## 6. Minors

The 61 minors were not re-verified in full. Every one has a round-3 disposition from every role that owns a part of it
(R-SPEC-F: 41 verified, 20 not F; R-SPEC-P: 34 verified; R-SPEC-R: 18 verified; R-MODEL: 14 verified). Spot checks while
verifying the majors found these in the text: P1-25 and S1-37 ([F05 §8.4] `FileRefV`, named by [F11 §2.5]), P1-27 and
S1-35 ([F10 §8] with no `OPS` section), P1-31 and S1-32 (no `DATA` code; [F06 §7.10] says so), P1-32 and S1-38
([F02 §6.3] `probe` and `settle.stamp`), P1-37 ([OS/path §4.1] step 3 and §7 step 3 cite `no_canonical_path` and
`bad_path`, which exist with the stated exits), S1-33 (codes 67, 68, 72), A1-46 ([F17 §5.2] and [F09 §15.1]'s ten
sections), A1-50 ([F08 §4] cites WT-004 and GR-002, which exist), A1-57 (`HOLES.md` `CFG-codex-mcp-result` cites
PE-004).

## Coverage

| Item | Where |
|---|---|
| none | a review file specifies no [60 §2.5], R-n, F-n, X-F-n or [90 §10.1] row |

## Holes

None. This file proposes no value; the byte strings it quotes are those of [F11 §14] and the replacement bytes NC-12's
fix names follow from [F11 §7]'s layout and [F05 §9.5] field 9.

## Open points for the review

1. **Closing pass 1.** Pass 1 closes when NC-11 and NC-12 are fixed by their owning roles and the owner has answered
   [owner-questions]. A round-4 check needs to re-read only [F05 §9.11] and open point 14, [F11 §13.1] and open point 42
   against [F13 §3] I-F5, [F18 §2.5], [API §14.1], [API] open point 48 and PX-011; and [F11 §14] and open point 1
   against [F05 §9.5] field 9, MF-009 and [API §15.7].
2. **Freeze gates carried out of pass 1**, not pass-1 items: the scanner appendix of [F20] (OQ-R-2); the fixture files of
   rows 60-AR-Values, R-4, R-15 and 60-I2-Gate0 (R-FIX, WP-20); the design-text edits marked for WP-81a (M1 gates for
   rotations, GT11's 14-days-behind case, measurement 8's side-ref timing, [AR §4.4] and [AR §8.1] size rows,
   [AR §5b.1], [AR §2.5], [AR §3.5], [AR §5a.7] step 1, [AR §7.1], [50 §4.1], and I-F5 or PX-011 once OQ-F-3 is
   answered); the rule-file re-signature (OQ-M-1 (a)–(p), V3).
3. **Pass 2** should keep the practice of rounds 1 to 3 and add one step: when an owner question is opened in a round
   (as OQ-F-3 was), every chapter that the question's options would change is re-read in the same round, so that no
   chapter states one option as settled while the others state the interim (NC-11); and an informative worked example
   is re-derived whenever a round narrows the rule its bytes illustrate (NC-12).

## Round 3 follow-up

Written after this check by the roles that own the fixes of §4.2 (R-SPEC-P for [F05], R-SPEC-R for [F11], R-SPEC-F for
[API] open point 48), not by the closure checker. The dispositions are in `pass1-dispositions-P.md` §8.7 and
`pass1-dispositions-R.md` §7.7. No normative byte, record kind, field, offset, section tag, rule row, hole or
`COVERAGE.md` row changed. The one changed byte is in [F11 §14]'s informative example.

| Id | Status | Change |
|---|---|---|
| NC-11 | fixed in the text | [F05 §9.11]'s closing paragraph and [F11 §13.1] "The pack cursor" no longer say "`pack T` for agent A appends one `feed` 2 record after emitting (PX-011)". Both now state OQ-F-3's interim. The `feed` 2 record's bytes and fold are unchanged, and cursor(A, T) of PT-028 is still the `cursor_seq` of the row (session, A, 2, `#N` of T). Which actor appends the record (the `pack` verb or the layer that delivers the pack) is OQ-F-3's call. Until OQ-F-3 is answered no M0 command appends one ([API] open point 48, PX-011), so cursor(A, T) is absent, C8 is empty and `pack` stays a read that appends nothing (I-F5; [F18 §2.5], [F11 §1.2]). [F05] open point 14 and [F11] open point 42 no longer say that PX-011 "appends" the cursor. They name OQ-F-3 and the interim, and they note that OQ-F-3's options (a) and (b) keep the bytes while option (c) would withdraw `feed` 2. [API] open point 48 no longer says that PX-011 has `pack` append the cursor. It names [F05 §9.11] and [F11 §13.1] as the record and the row and OQ-F-3 as the call on the appender, and it says that PX-011 (round 3), [F05 §9.11] and [F11 §13.1] state the same interim, beside §14.1's rule that every `Query`, `pack` included, appends nothing |
| NC-12 | fixed in the text | [F11 §14]: the worked `MARKERS` row is still a `settled`, `done` entry whose commit is not a `complete` (`outcome` 0). It now stores `actor` 0: offset 32 is `00 00 00 00`, with the comment `actor = 0 (not a complete)`. The prose no longer names a lease holder with symbol 5. It says that the entry carries no holder because its commit is not a `complete` ([F05 §9.5] field 9, MF-009). No checksum, length or digest in the example covers these bytes. `RtHdr` has no checksum, the body stays 100 bytes (24 + 72 + 4; `row_size` 0x48, `heap_len` 4), and no other text or fixture quotes the old bytes. [F11] open point 1: `actor` is "the holder of the lease a `complete` presented, else 0", as [F05 §9.5] field 9 carries it. The paragraph was re-wrapped and no other word changed |

**Re-read after the edits** (open point 1's round-4 list, and a search of `docs/spec/` for PX-011, OQ-F-3, "pack
cursor", "pack-cursor" and MF-009, and for the old wordings "appends one `feed`", "PX-011 appends", "`pack` append",
"leased completion" and `actor = 5`):

- NC-11: [F05 §9.11] and open point 14 and [F11 §13.1] and open point 42 agree with the following texts: [F13 §3] I-F5,
  [F18 §2.5], [F11 §1.2], [API §14.1], [API] open points 25 and 48, PX-011, PT-028, PM-025, pack-classes open point 8
  and OQ-F-3. [LQ/std], [CFG], `COVERAGE.md` and `HOLES.md` say nothing about C8 or the pack cursor. No chapter or
  rule row outside `reviews/` now says that `pack` appends the cursor. The one sentence that still describes PX-011
  that way is the residue below.
- NC-12: [F11 §14] and open point 1 agree with the following texts: [F05 §9.5] fields 9 and 11, [F11 §7] `actor` and
  `outcome`, MF-009, [API §10.5] step 2, [API §15.7] `markers` (`actor` null where the row's is 0) and [API] open
  point 34. Every citation of MF-009 agrees.

**Editorial residue** (no byte or rule consequence): [RULES/pack-classes] open point 8 still describes the question as
"PX-011 has the pack append the cursor", which is PX-011's round-2 wording, and then states the interim. The same holds
for the OQ-F-3 residue of §4.2. The owner should read PX-011's current row beside it. The edit is R-MODEL's and falls
under the rule-file re-signature (OQ-M-1). The notes that R-SPEC-P left in round 3 (`pass1-dispositions-P.md` §8.4
item 3 and §8.6 item 1) also used that wording. §8.7 of that file supersedes them.

With these edits, open point 1's first condition for closing pass 1 holds in the text: NC-11 and NC-12 are fixed by
their owning roles. The closure checker's round-4 re-read is what confirms it. The owner's answers to [owner-questions]
remain outstanding.
