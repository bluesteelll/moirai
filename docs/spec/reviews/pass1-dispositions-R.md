# Review pass 1: dispositions of R-SPEC-R

| | |
|---|---|
| Title | Dispositions by R-SPEC-R of the pass-1 findings routed to it ([P-pass1], [S-pass1], [A-pass1]) and of the items [pass1-closure] lists against its chapters |
| Status | review, pass 1, rounds 1 to 3 (round 2 in §6, round 3 in §7); dispositions await the owner's signature (WP-80, V2) |
| Work package | WP-80 pass 1, author side: R-SPEC-R (WP-13a, WP-13b, WP-14b, WP-15) |
| Files of R-SPEC-R | [F09], [F10], [F11], [F14], [F20] |

Dispositions:

- **fixed**: the change is made in the named sections. Where the fix changes bytes, codes or rules another chapter uses,
  that chapter was aligned in the same round against a fresh read of its section (cross-role edits are marked **x**
  and listed in §2).
- **fixed (R part)**: the part of the fix in R-SPEC-R's files is made; the rest lies with the role named.
- **open**: not closable in R-SPEC-R's files this round; the reason and the owing role are named.
- **rejected**: with the reason (none this round).

Summary: 48 fixed (18 blockers, 15 majors, 15 minors), 0 rejected, 5 left open. Owner questions OQ-R-1 to OQ-R-4 are in
[owner-questions]. Numbers other roles must now use are in §3.

Round 3 (§7): 8 findings fixed in their residue (closure NC-10's `CURSORS` row among them), 0 rejected, 1 part left
open (the [F05 §9.11] record of NC-10, R-SPEC-P's); every other R-owned finding re-verified with no change.
Round 3 follow-up (§7.7): closure NC-11 is fixed in [F11 §13.1] and open point 42, and NC-12 in the [F11 §14] example
(`actor` 0) and open point 1.

Round 2 (§6): 17 findings fixed or their residue fixed (P1-16 closes, and with it closure NC-1), 0 rejected, 0 pass-1
items left open; 51 R-owned findings re-verified in the text with no change. The five round-1 "open" rows are closed:
A1-15, P1-4, S1-28 and P1-8 by R-SPEC-P's round-1 records ([F04 §5.16], [F05 §9.28]) with R-SPEC-R's citations
reviewed, P1-16 by this round's [F11 §12.3].

## 1. Dispositions

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| A1-3 | blocker | 1 | fixed | [F09 §7.2] `EDGE_PROPS` is `{edge u32, pflags u8, _reserved [3], pinned_commit [32]}` = 40 B: [F08 §10.2]'s `pflags` (bit 0 `has_pin`, bit 1 `flagged`; `anchor` never stored here) and the full pin; one row per edge whose block without its anchor is not `00`, which covers every `pinned`-props kind (`consumed` included) and every `flagged`-props kind; the composition rule rebuilds [F07 §8.1]'s hashed bytes. [F09 §3.1] (40 B), V-11, OP-09-28; [F14] open point 13 closed |
| S1-2 | blocker | 1 | fixed | As A1-3; the row is exactly S1-2's proposal |
| P1-1 | blocker | 1 | fixed (R part) | [F09 §7.2] (A1-3) and [F11 §10] (S1-7). The cross-chapter byte fixtures per value type and anchor mode for `COVERAGE.md` rows 60-AR-Values and R-4 are R-SPEC-F's (WP-21 fixtures) |
| S1-7 | blocker | 1 | fixed | [F11 §10] `CONFLICTS`: `key` = [F06 §6.1]'s `ckey`; each side = [F06 §6.2]'s `kval` of the key's class, exactly as the `Conflict` op holds it (existence with node image, status pair, body hash, edge block with anchor, schema item); `prov` at offset 5 for existence keys, 0 otherwise; `class` = [F12 §6.1]'s code; no empty-slice rule (every `kval` is ≥ 1 byte). Schema keys have `n` = 0 and fold with `SCHEMA` ([F11 §10], [F09 §8.2]). [F11] open point 31 closed |
| S1-5 | blocker | 1 | fixed (R part) | [F11 §10] `prov`; the [F06]/[F07]/[F12] parts were R-SPEC-F's (closed) |
| A1-6 | blocker | 1 | fixed | [F11] open point 33 adopted as [F11 §2.9] "Row images": `FileObs`, `Pending`, `DirMap`, `PrefixEv`, `AnchorRes` rows and the `ClientHead` set record carry the [F11] row image; event records keep [F05]'s fields and each table names the record field behind every row field. Per table: `REFS` [F11 §3.9] field sources and the partial-upsert rule; `PINS` `PinHolder.kind` 1 fork-base, 2 promotion-base, 3 merge, 4 tag (= [F05 §9.8]'s values, now owned by [F11 §4]); `LEASES.flags.session_role` from new [F05 §9.4] field 27 `lflags` **x**; `MARKERS` byte 25 renamed `status`, byte 36 `outcome` (MF-009, [F05 §9.5] field 11), row still 72 B; `TREES` epochs per [40 §2.6] (0 full-tree, 1 lane-owned, 2 partial), `SensEntry` as [F11]'s, `journal_vol` dropped from [F05], `git` flag carried in `tflags` bit 3, sources per `sub`; `FILEOBS`, `PENDING`, `ANCHORRES` per S1-9; `FSINTENT` items carry root-qualified paths, `itflags`, the [F05] item outcomes and abort reasons (owned by [F11 §12.7]), flags `recursive`, `recovered`; `GITRENAMES` aligned with [F05 §9.25] facts (`vbytes` paths, group blob ids, author time, 98 B). [F05] aligned **x**: §8.5, §8.7, §9.3, §9.4, §9.8, §9.10, §9.15–§9.19, §9.22–§9.24, §9.26, §10.3, open points 9, 12, 14 |
| S1-8 | blocker | 1 | fixed | As A1-6. The epoch enumeration is decided by the design of record: [40 §2.6] fixes `scope_kind` 0 full-tree, 1 lane-owned, 2 partial, and FB-1 makes `partial` cover the brief, `--scope` and `--path`; its digest is zero, since a partial epoch is never decidable ([F11 §12.4]) |
| P1-2 | blocker | 1 | fixed (R part) | As A1-6: paths of `FSINTENT`, `PENDING` and `FILEOBS` proposals carry their root ([F11 §2.3] `HeapRef` type `path`); `RefTable` is a partial upsert ([F11 §3.9], [F05 §9.10] **x**, new fields 20–23 and `eflags` bit 2); the binding per A1-8. The full git-ref spelling P1-2 prefers was not taken by [F18 §3.2] rule 1 (R-SPEC-F's reason); row R-15's fixture is R-SPEC-F's |
| A1-7 | blocker | 1 | fixed | [F11 §12.5] `FILEOBS.state` cites [F18 §4.2]'s codes: stored states 1, 2, 3, 4, 6, 8, 12 (5, 9, 10, 11 computed at render; 7, 13 link-only); details are [F18 §4.6] codes with their slot values (`Detail`), the moved-auto target with its exact token; [F11] open point 40 |
| S1-9 | blocker | 1 | fixed | As A1-7; proposals are [F18 §4.10]'s tuple (class 1–4, evidence 13–26, path with root, exact score); `PENDING.evidence` is a `u8` token code (an exact token allowed, [F20 §5.6]); `ANCHORRES.state` [F18 §4.3] 1–5, `detail` 62/63, `unverified` never stored. [F18 §4.10]'s parenthetical aligned **x** (a `PENDING` row may hold an exact token) |
| A1-8 | blocker | 1 | fixed | [F11 §5] `HEADS` embeds [F18 §3.2]'s 40-byte `BindingExt` at offset 105 in place of the `binding`/`designated` flags, the `OidSlot` base and the full-name `git_ref`; row 161 B; every kind-1 row is a binding row ([F18 §3.1]); I-F12 over `binding.designated`. [F05 §9.3] `ClientHead` carries the row image **x**; [API §11.4] wording aligned **x**. [F11] open point 12 |
| S1-10 | blocker | 1 | fixed | As A1-8, with [F18 §3.2] rule 1's short ref form |
| A1-10 | blocker | 1 | fixed | [F09 §16.4] `CKIMG` (tag `0x00C2`, [F09 §3.1]): the [F06 §4.4.14] entries of a bulk import-checkpoint, sorted by `id`, not row-scoped, kept with the `cs.<n>`; [F14 §11.3] row and open point 22 closed; [F10 §8] |
| S1-23 | major | 1 | fixed | As A1-10 |
| S1-6 | blocker | 1 | fixed (R part) | [F14 §11.2] absent → tombstone cell: `CreateDeleted` (NF-11) with the retained title and `AddEdge` ops; [F14] open point 25 closed; [F09 §16.4] table row for the absent-to-deleted touched row. [F06]/[F07]/[F12] parts were R-SPEC-F's (A1-4 closed) |
| A1-14 | blocker | 1 | fixed (interim rule; appendix open) | [F20 §6.1] step 8 and "The interim scanner rule": no capture records a scope, rung 2 skipped, `captured` takes `lp("")` ([F08 §10.3.1]); closure open point 2 — the `symbol` and `heading` forms, whose header line, quote, hint and span hash a scanner decides, are refused until the appendix exists; imported ones resolve without scanner steps ([F20 §6.5]). [F20] open point 30 states the appendix's required content; OQ-R-2. The normative scanner appendix itself is not written this round (a three-language grammar with fixtures; R-SPEC-R with WP-63, before the freeze) |
| S1-4 | blocker | 1 | fixed (interim rule; appendix open) | As A1-14 |
| P1-20 | major | 1 | fixed (interim rule; appendix open) | As A1-14 (P1-20's interim "the anchor is a `quote` anchor" is realised by refusing the scanner forms and keeping `path:L-M` and quote-file forms) |
| A1-2 | blocker | 1 | fixed (residue) | [F14 §5.6]: the scope text is the bijective image of [F08 §10.3.1]'s bytes, which the one record holds; [F14 §6.7] (`text_unavailable`, presence), §11.3 anchor-text row; [F14] open points 10–12 closed (S1-3's residue too) |
| P1-6 | blocker | 1 | fixed (R part) | [F10 §8] and [F09 §16.4]: `cs_ref.b3` = `seg_digest[0..16]`; [F09] OP-09-03 closed |
| A1-15 | blocker | 1 | open | Needs the `init`-fixed field in `InitParams` ([F04 §4.4], [F17 §2.1]; R-SPEC-P), whose 32-byte block has no spare byte. [F20 §2.3] now states what the field holds (one `algo` byte, 1 or 2, recorded at `init`, kept by `restore`/`repair`, never recomputed) and [F20] open point 27 cites its name and offset once they exist |
| P1-4 | blocker | 1 | open (R part) | As A1-15 |
| A1-22 | major | 1 | fixed | [F10 §8]: a `cs` file keeps its rows, `VIOLATIONS` and `CKIMG` for history; a bulk commit stores no op list and is read as a state delta ([F06 §9] BK-5); OP-10-11; [F09] OP-09-15 |
| S1-19 | major | 1 | fixed | [F10 §8] `b3` = `seg_digest[0..16]` ([F06 §9] BK-2); [F09 §16.4], OP-09-03 closed |
| A1-21 | major | 1 | fixed (R part) | As S1-19 |
| S1-20 | major | 1 | fixed (R part) | [F09 §16.4] `PREV` cites [F16] P-34's node-granular re-validation, as [F06 §7.3] and BK-5 do; OP-09-15. P-34's own title and wording ("by key") are R-SPEC-P's |
| A1-34 | major | 1 | fixed | [F20 §5.11.4] defines the git pair score normatively (span hash, CR-before-LF skip for text, 64-byte chunks, modulus 107,927, `⌊100 × Σ min / max len⌋`, links 0) with five golden vectors; constants `GS_CHUNK`, `GS_HASHBASE`, `GS_TEXT_PREFIX` in [F20 §7]. Checked: an implementation of the definition agrees with git 2.54.0 on the vectors and on 60 random edited pairs (`git diff --no-index -M1%`, `core.autocrlf=false`). No [F01 §2.2] reference needed; [F20] open points 36, 39 |
| P1-15 | major | 1 | fixed (R part) | [F20 §4.9]: every segment of every path is tested with `representable_here` before any OS call; a node's own path → `missing (not representable on this OS)` (detail 44), a candidate is dropped, `--allow-nonportable` never relaxes it; [F20 §4], §5.4, open point 38. The `ProjectFs` check ([OS/path], [OS/project], [OS/fs]) is R-SPEC-P's |
| P1-17 | major | 1 | fixed (pending owner) | [F14 §14.1]: the side ref holds `aliases/<h1>.moi` (256 blobs by the uid's first byte); a run rewrites only touched prefixes; `aliases-file` needs ≥ 1 row; §12.4, §16, open point 32. The [AR §5b.1] change is OQ-R-1; measurement 8's side-ref timing is for WP-81a ([60 §5.2]) |
| P1-22 | major | 1 | fixed | [F11 §12.8] `FPRINT` carries the `blobs` file number (54 B) and is rewritten when a file is replaced; [F10 §5.5] one-file lookup; [F10 §7.1] a pair's `gitmap` pages tiered by `store.fold-width` (≤ 1 + P14 pages); [F09] OP-09-14 adopted, §15.2; [F10] OP-10-18 |
| S1-29 | major | 1 | fixed | [F17 §4.3] cites [F10 §4.2] as the one frame rule and keeps only the values **x**; [F10 §4.2], OP-10-03 closed |
| S1-31 | major | 1 | fixed | [F20] Holes `F20-btime-ntfs`: candidate `Absent` (a measured tool copies creation times: line 2 never applies on NTFS, the value ReFS already takes) and measurement 15's added copy paths (`robocopy /COPY:DAT`, `Copy-Item`, Explorer, archive extraction, `git checkout`); ChangeTime condition kept; [F20] open point 15; OQ-R-3 for the unmeasured-tool residue |
| A1-23 | major | 1 | fixed | [F11 §13] adds `CURSORS` (`Lazy` cursors), `SESSMARKS` (`SessionMark`) and `BACKUPS` (`Backup`), 56 B rows, tags `0x020A`–`0x020C` ([F09 §3.1], §14.5), retention decided in [F11] open point 22; [F11] open point 39; [F05 §10.3] and open point 14 cite them **x**. The worked example moved to [F11 §14] (no chapter cited §13) |
| A1-41 | major | 1 | fixed | [F09 §8.3] cites [F08 §8.5]'s item key order explicitly |
| S1-22 | major | 1 | fixed (R part) | [F14 §6.8.1]: the image holds no hierarchy or edge key; a take restores them by [F12 §6.5]. [RULES/merge-table] RS-008 is R-MODEL's |
| S1-28 | major | 1 | open (R part) | As A1-15 |
| P1-8 | major | 1 | open (R part) | [F10] OP-10-19: retirement keeps every record but the rotation pad, so an anchor record is kept once [F05]/[F16] (R-SPEC-P) define it |
| P1-16 | major | 1 | open (R part) | The `VolumeCaps` "directory flush" bit: [OS/project §4.2] owns the values and the probe (R-SPEC-P, [F11] open point 21); [F11 §12.3] restates the bit once it is defined |
| P1-27 | minor | 1 | fixed | As A1-22 |
| S1-35 | minor | 1 | fixed | As A1-22 |
| A1-43 | minor | 1 | fixed (R part) | [F14 §6.8]: `DATA` is not a class; an importer reads it as `ImageParse` |
| S1-32 | minor | 1 | fixed (R part) | As A1-43 |
| P1-31 | minor | 1 | fixed (R part) | As A1-43 ([F14 §6.8]) |
| A1-44 | minor | 1 | fixed (R part) | [F14] open point 36 closed ([F18 §2.2] already fixed); the [F13 §3.9] part is R-SPEC-P's |
| A1-45 | minor | 1 | fixed | [F20 §1.5]: the reasons render as [F18 §4.6] details 53–59, `unreadable` = 59; [F20] open point 22 |
| A1-46 | minor | 1 | fixed (R part) | [F17 §5.4] names [F09 §15.1]'s ten runtime-window sections, `ANCESTRY` included **x**; [F17 §4.3] per S1-29 **x**. The [F04 §10] part is R-SPEC-P's |
| P1-25 | minor | 1 | fixed (R part) | [F11 §2.5] is the one owner of `FileFamily` and names the two `FileRef` encodings; the `FileRefV` rename in [F05 §8.4] is R-SPEC-P's |
| S1-37 | minor | 1 | fixed (R part) | As P1-25; [F11] open point 11 |
| P1-28 | minor | 1 | fixed | [F11] open point 14: the 72-byte slot is kept (every field is needed); [AR §4.4] and [AR §8.1] size rows are updated at WP-81a (OQ-R-4) |
| S1-36 | minor | 1 | fixed (R part) | As P1-28; the [F01 §8.3] part was R-SPEC-F's |
| P1-30 | minor | 1 | fixed | As S1-29 |
| S1-45 | minor | 1 | fixed (R part) | [F11 §3.7]: `REFS.aux` equals the fold's `next_ref_id` at the segment's bound and never exceeds `HEAD.next_ref_id`; [F04 §10] is R-SPEC-P's |
| S1-34 | minor | 1 | fixed (R part) | [F14 §6.9]: an empty body is no body ([F08 §7.2]); the empty-body form never occurs natively |
| S1-38, S1-39, P1-18, P1-44 | minor, major | 1 | no R change | Checked: [F20 §5.12.1] already uses `tmp/settle.stamp`, which [F02 §6.3] now admits; [F14 §2.3]'s root-name rule agrees with [CFG §4.2] after R-SPEC-F's fix; P1-18's loose-object sequence and P1-44's windows are [F16]'s and [API]'s |

## 2. Cross-role edits made this round (each after a fresh read of the section)

- **[F05] (R-SPEC-P):** §8.5 row-batch body = [F11] row image for `FileObs`, `Pending`, `DirMap`, `PrefixEv`,
  `AnchorRes`; §8.7 owners of the codes; §9.3 `ClientHead` = op + `HEADS` row image (remove: kind, key, hlc; a session
  head expires after `idempotency.retention`, [F11 §5.2], where the earlier text said `idempotency.default-window`);
  §9.4 field 27 `lflags` (`session_role`); §9.8 holder cites [F11 §4]; §9.10 partial upsert, `eflags` bit 2 `pinned`,
  fields 20–23; §9.15–§9.17 codes cite [F11 §12.7]; §9.18, §9.19, §9.22, §9.24, §9.26 replaced by row-image citations;
  §9.23 `tflags` bit 3 `git`, `journal_vol` removed (fields renumbered 10–15), `sens` and `epoch` cite [F11 §12.4];
  §10.3 `RefTable` and the new tables; open points 9, 12, 14.
- **[F17] (R-SPEC-P):** §4.3 cites [F10 §4.2]; §5.4 names [F09 §15.1]'s ten sections.
- **[F18] (R-SPEC-F):** §4.10, the proposal paragraph: `PENDING` may store an exact evidence token (codes 1–12).
- **[API] (R-SPEC-F):** §11.4 `WorktreeBind`: "a `ClientHead` record setting a `HEADS` row whose `BindingExt` is
  `designated` …" instead of "with the `binding` flag".

## 3. Numbers other chapters must use

- [F11] row sizes: `HEADS` 161, `PENDING` 123, `FPRINT` 54, `GITRENAMES` 98, `CURSORS`/`SESSMARKS`/`BACKUPS` 56;
  `CONFLICTS` 56 and `MARKERS` 72 unchanged in size. Tags `0x020A` `CURSORS`, `0x020B` `SESSMARKS`, `0x020C`
  `BACKUPS`; `0x00C2` `CKIMG`; `EDGE_PROPS` 40 B.
- Codes owned by [F11]: `PinHolder.kind` 1–4; settle epoch `scope_kind` 0–2 ([40 §2.6]); intent item outcomes 1–5 and
  abort reasons 1–5 ([F11 §12.7]); `MARKERS.status` 1–2 and `outcome` 0–3.
- `gs` (the git pair score) is [F20 §5.11.4]'s function; [F18 §5.2] token 16's score is `gs / 100`.

## 4. Notes for R-SPEC-F (`COVERAGE.md`, `HOLES.md`, `README.md`)

1. `COVERAGE.md` rows R-4 and 60-AR-Values: the [F09 §7.2] part is fixed; lift CONFLICTING if nothing else remains
   (the fixtures of P1-1 stay with WP-21).
2. Rows R-15, R-7, R-18: [F11 §5] embeds `BindingExt`, [F05 §9.3] carries the `HEADS` row image, and the record/row
   alignment of [F11 §2.9] is done; R-15's fixture recompute is R-SPEC-F's.
3. The rows [pass1-closure] §7 lists as citing contradicting sections are resolved on the [F05]/[F09]/[F10]/[F11]/[F17]
   side: 60-AR-Seg-runtime, 60-AU-Log-TreeReg, 60-AU-Seg-leases, 90-LEASES, 60-AU-Seg-r4rt, 60-AU-Commit-csref,
   60-AU-Seg-hist, F11. 60-AR-HEAD-params (A1-15), 60-AR-Log-kinds and 60-I2-PD(e)/F14 remain R-SPEC-P's.
4. New sections that rows may cite: [F11 §2.9] (row images), §3.9 (`REFS` field sources), §13 (`CURSORS`, `SESSMARKS`,
   `BACKUPS`: the [72 M9] backup and [73 F4] session-mark rows); [F09 §16.4] `CKIMG`; [F20 §4.9] (representability),
   §5.11.4 (normative `gs`).
5. `HOLES.md` row `F20-btime-ntfs`: add the candidate `Absent` and measurement 15's added copy paths ([F20] Holes);
   the constraint now reads "`TunneledNotCopied` only if no measured tool gives a new file its source's creation time;
   if one does, `Absent`".
6. `README.md`: [F11]'s row may name the session and backup tables and the row images.
7. Please review the [F18 §4.10] and [API §11.4] alignments of §2.

## 5. Notes for R-SPEC-P

1. Please review the [F05] and [F17] edits of §2. [F05 §7]'s fold-target cells may cite [F11 §13] by name; [F05 §8.4]'s
   `FileRefV` rename (P1-25) and [F17 §6.1]'s mention that P14 also bounds `gitmap` pages ([F10 §7.1]) are yours.
2. Still owed for findings whose [F09]/[F10]/[F20] side is done: [F16] P-34 "by node" (S1-20); the `project` oid
   algorithm field in `InitParams` (A1-15, S1-28, P1-4); the anchor record of P1-8, which [F10] OP-10-19 will keep; the
   `VolumeCaps` directory-flush bit (P1-16), which [F11 §12.3] will restate; the `ProjectFs` name check (P1-15).

## 6. Round 2

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1] whose fix lands in [F09], [F10], [F11], [F14] or [F20], at
every severity, and every item [pass1-closure] (round 1) lists against these chapters: its open finding P1-16 with
contradiction NC-1, the editorial residues of [F10] OP-10-19 and [F09] OP-09-17, and the review requests of
`pass1-dispositions-P.md` §5 and `pass1-dispositions-F.md` ("Notes for other roles"). Cross-role edits are marked **x**.

### 6.1 Dispositions

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| P1-16 | major | 2 | fixed (R part; closes the finding and closure NC-1) | [F11 §12.3] `flags`: bit 14 `dir_flush_doubtful` (set by file-system class, never by a probe, as [OS/project §4.2], §4.3), bits 15–31 reserved-zero, so a `TREES` row written on a network, `\\wsl$`, NFS, CIFS, FUSE or 9p volume is valid in both chapters. [F11] open point 21 re-checked field by field: `OsFileId`, `FsTime`, `FileAttrs`, the `VolumeCaps` snapshot and `JOURNALCUR` agree byte for byte with [OS/project §3.1], §3.3, §3.4, §4.2, §7.1. Open point 41 |
| P1-8 | major | 2 | fixed (residue) | [F10 §4.1] names what retirement keeps: every extent's first record, its `ExtentHead` (kind 28, [F05 §4.5], §9.28), whose `chain_in` is the chain value at the extent's first byte, so the value a dropped rotation pad's trailer held survives in the next extent's head; [F10] OP-10-19 closed (the closure's editorial residue) |
| P1-3 | blocker | 2 | fixed (R part) | The `Reserve` record (kind 27) now reaches every R structure it touches: [F09 §16.4] "Ids" cites [F05 §9.27] and [F16] P-84, [F09] OP-09-17 closed and retitled (closure residue); [F11 §9.1] states the `ALLOC` rows of reserved ids (holes in a segment that covers the record but not its bulk `Commit`; filled once by the fold that covers the `Commit`, the layer's range extended down to them; unused or abandoned ids stay holes, never reused), [F11 §1.3] `ALLOC` row; [F09 §14.4] `FILES.flags` bit 0 `reserved` keeps a folded reservation's `cs` and `blobs` numbers named for [F16] P-77 condition 2 until the `Commit` folds or `gc` releases them (skipped by readers, P-87's copy and `doctor --fsck`); [F10 §2.1], §5.4, §8 name the reservation as the file's claim before its `Commit`. **x** [F13 §3] I35′ enforcement: "an `ALLOC` row that binds a uid is never rewritten; only a reserved id's hole is filled" (was "`ALLOC` rows are never rewritten", which the fill would have contradicted) |
| S1-11 | blocker | 2 | fixed (R part) | As P1-3 |
| A1-12 | blocker | 2 | fixed (R part) | As P1-3 |
| A1-11 | blocker | 2 | fixed (R part) | [F11 §3.9]: the field sources of `RefUpdate` reason 5 `park` ([F05 §9.2]): a creating park (`old` zero) writes the create fields of `orphans/<R>` (kind 6, no fork, counters 0, empty absorbed vector); every park writes `tip`, `tip_lsn`, `gen`; a park is no commit landing on the ref and changes no counter. [F11 §3.2] kind 6 names its writer ([F16] P-70) instead of "recovery" |
| P1-9 | major | 2 | fixed (review of R-SPEC-P's [F10 §7.1] edit) | Kept, and worded as [F16] P-98 states it: a yield checkpoint merges no `gitmap` page except the one page per pair that the same holding's earlier yield wrote, which it replaces and releases; hence 2 + P14 while a long job yields |
| P1-1 | blocker | 2 | fixed (review of R-SPEC-F's [F09 §10.1] edit) | Kept (the promoted `commitref` element is an `id16` index key, the value confirmed against the field block, as [F08 §5.1] says). Added in [F09 §10.2]: a set row's promoted elements are unique by their `id16`, so two full ids sharing one are stored once (the field block holds both); without it "unique" had two readings |
| P1-25 | minor | 2 | fixed | [F11 §2.5] names [F05 §8.4]'s record form `FileRefV` (R-SPEC-P made the rename in round 1) instead of asking for it |
| S1-37 | minor | 2 | fixed | As P1-25 |
| P1-22 | major | 2 | fixed (residue) | [F10] OP-10-18 closed: [F17 §6.1] now states that P14 bounds a pair's `gitmap` pages (1 + P14, 2 + P14 while a long job yields) |
| S1-23 | major | 2 | fixed (residue) | [F14] open point 19: [F06 §4.4.11] now allows all-zero `ckpt.first`/`last` exactly when `n_folded` = 0; the request is recorded as done |
| A1-14 | blocker | 2 | fixed (residue of the interim rule) | [F20 §6.4] "Same-kind headers": finding item headers needs a scanner, so while §6.1's interim scanner rule holds the restriction does not apply and an imported `symbol` or `heading` anchor's header quote is matched as a quote (§6.1's bullet said so only implicitly). [F14 §5.6]: no capture records a scope under the rule, so the scope text is written only for imported anchors. [F14 §17.2]: the example's `symbol` anchor is held by import while the rule holds (the counterpart of the closure's [F18 §4.7] residue, which is R-SPEC-F's). The scanner appendix stays the freeze gate of OQ-R-2 |
| S1-4 | blocker | 2 | fixed (residue) | As A1-14 |
| P1-20 | major | 2 | fixed (residue) | As A1-14 |
| P1-38 | minor | 2 | fixed (R part) | [F20 §2.4] item 3 maps the `ProjectFs` kinds: `CloudOnly`, including a file that became a placeholder between stat and open, which [OS/project §5.5] step 3 now re-checks through the handle, gives `Unavailable(cloud-only)` (detail 54), not `unreadable` (59); `IsSymlink` is read by `read_link` and hashed by §2.3; `IsDirectory`, `OutsideRoot` and `Stale` are read errors (`unreadable`). Two implementers mapped `CloudOnly` differently before |
| A1-33 | major | 2 | fixed (R part) | As P1-38: [F20 §2.4] uses the five kinds [OS/fs §6.1] added |
| A1-2, A1-3, A1-4, A1-5, A1-6, A1-7, A1-8, A1-9, A1-10, A1-15, P1-2, P1-4, P1-6, S1-2, S1-3, S1-5, S1-6, S1-7, S1-8, S1-9, S1-10, S1-16 | blocker | 2 | verified, no change | Closed in [pass1-closure] round 1; re-read where a round-2 edit touched their sections ([F09 §7.2], §13.3, §16.4; [F10 §4.6], §8; [F11 §3], §7, §10, §12; [F14 §5.6], §6.8.1, §11.2, §11.3; [F20 §2.3]). A1-15, P1-4, S1-28 (round-1 "open" rows): [F20 §2.3] and open point 27 cite `HEAD.project_oid_algo` at slot offset 1072, which [F04 §3.1], §5.16 and [F17 §2.2] IP-1–IP-3 define (R-SPEC-P's round-1 cross edit, reviewed and kept) |
| A1-21, A1-22, A1-23, A1-34, A1-41, P1-15, P1-17, S1-19, S1-20, S1-21, S1-22, S1-28, S1-29, S1-31 | major | 2 | verified, no change | As above. P1-17 and S1-31 stay "closed, owner" (OQ-R-1, OQ-R-3) |
| A1-43, A1-44, A1-45, A1-46, P1-27, P1-28, P1-30, P1-31, S1-32, S1-34, S1-35, S1-36, S1-38, S1-39, S1-45 | minor | 2 | verified, no change | [F14 §6.8] (`DATA` is `ImageParse`; the class list equals [F12 §6.1]), §6.9, §2.3 `rootname` = [F08 §5.4.1] = [CFG §4.2], open point 36; [F20 §1.5] (details 53–59), §5.12.1 (`settle.stamp` is [F02 §6.3]'s fixed name); [F10 §4.2], §8, OP-10-03, OP-10-11; [F11 §3.7], §8, open point 14; [F17 §4.3] cites [F10 §4.2] and [F17 §5.4] lists [F09 §15.1]'s ten runtime-window sections |

Rejected: none. Left open: none among the pass-1 items of these chapters. Carried out of pass 1, not open items: the
scanner appendix of [F20] (freeze gate, OQ-R-2) and the owner confirmations OQ-R-1, OQ-R-3 and OQ-R-4.

### 6.2 Other round-2 edits in R-SPEC-R's chapters (no finding of their own)

- [F11 §2.4]: the `RtHdr.form` of the versioned sections `CONFLICTS` and `GLOBIDX` (1 in a base, 2 in a delta,
  promoted-branch or changeset segment), which the chapter never stated although it fixes a byte; [F09] OP-09-13 closed.
- [F20] open point 37 closed: §2.3, §2.4, §5.1, §5.10, §5.11.4, §5.19 and the closed `unverified` set re-checked
  against [40] as committed; they agree (detail 59 stays with the owner through [F18] open point 12).
- [F20] Coverage row "[40] R-4 and R-10": names §2.7.3, §2.8 and §6.1 as the inputs of R-FIX's per-kind anchor fixtures
  of `COVERAGE.md` row R-4 (the request in `pass1-dispositions-F.md`), with no scope captured under the interim rule.
- New open points recording the round: [F09] OP-09-29, [F11] open point 41.

### 6.3 Cross-role edit of this round (after a fresh read of the section)

- **[F13 §3] (R-SPEC-P):** row I35′, enforcement point EP-W8: "(C: an `ALLOC` row that binds a uid is never rewritten;
  only the hole of an id reserved by a `Reserve` record is filled, once, by the fold of the bulk commit that uses it,
  [F11 §9.1])". Please review.

### 6.4 Numbers and rules other chapters must use

- `FILES.flags` bit 0 `reserved` ([F09 §14.4]); bits 1–7 reserved-zero. The row is otherwise unchanged (56 B).
- `ALLOC`: reserved ids are holes until the fold of their bulk `Commit`; a hole is filled at most once ([F11 §9.1]).
- `VolumeCaps` snapshot: bit 14 `dir_flush_doubtful`, bits 15–31 reserved-zero, in [F11 §12.3] as in [OS/project §4.2].
- `RtHdr.form` of `CONFLICTS` and `GLOBIDX`: 1 in a base segment, 2 in every other graph segment kind ([F11 §2.4]).

### 6.5 Notes for R-SPEC-F (`COVERAGE.md`, `HOLES.md`, `README.md`)

1. Rows 60-AU-Vfs-projfs and X-F8 no longer cite contradicting sections: [F11 §12.3] and [OS/project §4.2] agree on
   `VolumeCaps` (closure §5, NC-1), so neither needs CONFLICTING.
2. Rows 60-AU-Seg-alloc and F17 may add [F05 §9.27] (`Reserve`) beside [F05 §10.3], since reserved ids reach `ALLOC`
   through it ([F11 §9.1]); rows R-9 and 60-AR-Log-kinds may cite [F09 §14.4]'s `reserved` flag.
3. Row R-4: [F20]'s Coverage now names the window and span-hash definitions R-FIX's anchor fixtures take (§2.7.3, §2.8,
   §6.1).
4. `HOLES.md`: no hole added, renamed or removed by R-SPEC-R this round. `README.md`: no change needed.
5. [F18 §4.7]'s example list (the closure's third editorial residue) is yours; [F14 §17.2] now states the same point for
   its own example.

### 6.6 Notes for R-SPEC-P

1. Please review the [F13] I35′ wording of §6.3.
2. [F16] P-87 step 2 copies "every sealed file S's view names (… its `FILES` registry …)": a `FILES` row with the
   `reserved` flag names no content and may name no file yet ([F09 §14.4]); P-87 may say so where it lists the files.

## 7. Round 3

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1] whose fix lands in [F09], [F10], [F11], [F14] or [F20], at
every severity, and every item [pass1-closure] (round 2) lists against these chapters: contradiction NC-10 (the
`CURSORS` row), its editorial residues and its open point 1, and the review request of `pass1-dispositions-P.md` §7.5.
Besides NC-10, the checks of [A-pass1]'s open point 2 were run on the R chapters: each table this role owns was compared
field by field and value by value with the record or chapter that restates or feeds it, every section citation of the
five chapters was resolved against its target's headings (none missing; the [LQ/envelope] and [LQ/std] hits are
numbered paragraphs), every offset table re-summed, every `HOLE(…)` id found in `HOLES.md`, and the record-kind numbers
27 and 28 and the tags `0x0201`–`0x021A`, `0x0031`, `0x0084` compared with [F05 §7] and [F09 §3.1]. Cross-role edits:
none this round.

### 7.1 Dispositions

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-10 (closure §4.2) | major | 3 | fixed (R part); the [F05 §9.11] record is R-SPEC-P's | [F11 §13.1] `CURSORS` holds the per-(agent, task) pack cursor of [AR §7.4] C8 that [RULES/pack-classes] PX-011 appends and PT-028 reads: `feed` 2 = the pack cursor; `task` `u32` at offset 36 (the task's `#N`, taken from the record's `task`, a `nodeid`; 0 for `feed` 1; a `feed` 2 row with `task` 0 is invalid); `cursor_seq` holds the pack's `rev` (PT-032) for `feed` 2; `_reserved` shrinks to bytes 33–35, so the row stays 56 B and the tag `0x020A` is unchanged. Key (session, agent, feed, task), `feed` and `task` compared numerically ([F11 §2.2]); each row field named after its record field. The lookup: cursor(A, T) is the row (session, A, 2, `#N` of T) of the session the pack runs in; absent → C8 empty (PM-025); retention as before (a dropped cursor leaves C8 empty). `#N` rather than the 16-byte uid because it is store-wide and never reused ([F11 §9]) and fits the row, as `SESSMARKS` holds rule `#N`s and [F05 §9.4] a lease's task. Also [F11 §13] intro, §1.3 table-map row, open point 42. Left for R-SPEC-P (§7.4 item 1): [F05 §9.11] `feed` 2 and the `task` field |
| A1-23 | major | 3 | fixed (residue) | As NC-10: the pack cursor, the last cursor that A1-23's record kinds fold without a row, now has one; [F11 §2.7]'s full-validation list names §13 (it said §3–§12 although §13 was added in round 1) |
| A1-6 | blocker | 3 | fixed (residue) | [F11 §7] `MARKERS.actor` now takes exactly its record source, [F05 §9.5] field 9 `holder`: the lease holder of a `settled` entry (MF-009, the `holder` of [AR §5d.1]), 0 when none and for the other kinds, whose entries carry no holder. The row said "the author of `commit`" for an unleased completion and for `deleted` rows, a value no `Marker` entry carries, so replay could not fill it and the engine's and the model's `actor` ([API §15.7] `markers`) could differ. [F11] open point 1 and the §14 example's wording follow |
| S1-16 | blocker | 3 | fixed (residue) | As A1-6 (the `MARKERS` row against [F05 §9.5]) |
| P1-3 | blocker | 3 | fixed (residue; review of [F16] P-87 per `pass1-dispositions-P.md` §7.5 item 2) | P-87 step 2 agrees with [F09 §14.4]: a reservation's `cs` and `blobs` files are copied only when the bulk `Commit` lies below S.`committed_lsn`. [F09 §14.4] now says so explicitly: a `reserved` row is skipped, and the file enters a view through the `Commit` that names it once that record lies in the view's log range, which is when P-87 copies it |
| A1-3 | blocker | 3 | fixed (residue, precision) | [F09 §7.2] "Composition" stated one block per CSR entry with "the anchor records of its `ANCHORS` rows". An `at` CSR entry stands for one edge key per `ANCHORS` row of (src, dst) (disc = the anchor's uid, §13.4), and each key's block is `pflags` = `04` followed by that one anchor record ([F08 §10.2]: an `at` edge admits `anchor` alone); a non-`at` edge's block is its row's `pflags` (`00` without a row) and the pin. No byte changes; the text now rebuilds [F07 §8.1]'s per-key blocks unambiguously |
| S1-2 | blocker | 3 | fixed (residue, precision) | As A1-3 |
| A1-39 | major | 3 | fixed (residue) | [F20 §6.1] cited "[F19]" for its capture refusals; it now names [F19 §10.2] `anchor_spec` and the case of each: `binary` (a span on non-text content), `range` (step 1), `fffd`, `empty` and `not-found` (quote input, step 3), `no-scanner` (the interim scanner rule). [F20] open point 31 ("[F19] needs the refusal code") is closed: the `binary` case exists with exit 2 |
| A1-2, A1-4, A1-5, A1-7, A1-8, A1-9, A1-10, A1-11, A1-12, A1-14, A1-15, P1-1, P1-2, P1-4, P1-6, S1-3, S1-4, S1-5, S1-6, S1-7, S1-8, S1-9, S1-10, S1-11 | blocker | 3 | verified, no change | Closed in [pass1-closure] round 2; re-read against the current text of the chapters they cite: [F08 §5.1], §8.5 (the item key order [F09 §8.3] cites), §10.2, §10.3; [F07 §8.1], §8.2; [F05 §7], §9.2, §9.4 field 27, §9.5, §9.27, §9.28; [F04 §3.1] offset 1072; [F18 §4.6] details 53–59; [F14 §6.10] (a tombstone with an empty reason omits the line, which fits any value NC-9 gives the inverse `Delete`). A1-14's interim rule and OQ-R-2 unchanged |
| A1-21, A1-22, A1-33, A1-34, A1-41, P1-8, P1-9, P1-15, P1-16, P1-17, P1-20, P1-22, S1-19, S1-20, S1-21, S1-22, S1-23, S1-28, S1-29, S1-31 | major | 3 | verified, no change | As above. P1-17 and S1-31 stay "closed, owner" (OQ-R-1, OQ-R-3); P1-20 "closed, interim" |
| A1-43, A1-44, A1-45, A1-46, P1-25, P1-27, P1-28, P1-30, P1-31, P1-38, S1-32, S1-34, S1-35, S1-36, S1-37, S1-38, S1-39, S1-45 | minor | 3 | verified, no change | Re-read in [F09 §14.4], [F10 §4.2], §8, [F11 §2.5], §3.7, §8, [F14 §2.3], §6.8, §6.9, [F20 §1.5], §2.4, §5.12.1 |

Rejected: none. Left open: the [F05 §9.11] half of NC-10, which the closure assigns to R-SPEC-P (the record's `feed` 2
and `task` field; §7.4 item 1 gives the bytes [F11 §13.1] expects). Carried out of pass 1, unchanged: the scanner
appendix of [F20] (freeze gate, OQ-R-2) and the owner confirmations OQ-R-1, OQ-R-3 and OQ-R-4. No owner question added.

### 7.2 Other round-3 edits in R-SPEC-R's chapters (no finding of their own)

- [F11 §2.8]: the column "Proposed tag" is "Tag" (the values are [F09 §3.1]'s adopted ones), and `CONFLICTS` and
  `GLOBIDX` name their tags `0x0031` and `0x0084` instead of "in [F09]'s versioned range".
- [F20 §1.2] states the granularity G as [F11 §12.2] defines it (`max(10^gran, VolumeCaps.mtime_granularity_ns)`);
  [F11] open point 20, which asked for that citation, is closed.
- [F20] open point 8 closed: [F07 §8.2] field 11 carries W as `lp(W)` and [F14 §6.7] writes base64url(W) without
  padding, as the point asked.
- [F09], [F10]: 14 citations of `os/` files written as code spans are plain citations, the form of [F01 §2.2].

### 7.3 Numbers and rules other chapters must use

- `CURSORS` (56 B, tag `0x020A`): `feed` 1 change feed, 2 pack cursor; `task` `u32` at offset 36 (`#N`; 0 for `feed` 1,
  never 0 for `feed` 2); key (session, agent, feed, task); `cursor_seq` = the pack's `rev` for `feed` 2.
- `MARKERS.actor` = [F05 §9.5] field 9 `holder` of a `settled` entry; 0 otherwise.

### 7.4 Notes for R-SPEC-P

1. **NC-10, the record** ([F05 §9.11]). [F11 §13.1] expects: field 8 `feed` gains value 2 = the pack cursor of
   [AR §7.4] C8 ([RULES/pack-classes] PX-011); a new field `task`, `nodeid`, present when `sub` = 2 and `feed` = 2,
   not 0, the `#N` of the task the pack was built for (placed after `feed`, with `cursor_seq` and `hlc` renumbered, or
   appended; the order is yours); `cursor_seq` holds the pack's `rev` for `feed` 2. The §9.11 closing sentence and
   [F05] open point 14 ("one row per (session, agent)") may then say "(session, agent, feed, task)". If you choose
   another encoding (for example the 16-byte uid), tell R-SPEC-R: the row has 3 spare bytes only, so a uid would grow
   it to 72 B.
2. [F05 §9.5] field 9 `holder` is now the sole source of [F11 §7] `actor`. Its text ("the lease holder; 0 when none")
   covers any leased write that settles a task, while MF-009 speaks of records "written by `complete`"; if the two are
   meant to differ, one of [F05] or [RULES/state-definition] should say which (R-MODEL is told the same, §7.6).
3. [F16] P-87 step 2 re-read against [F09 §14.4] and kept (§7.1, P1-3).

### 7.5 Notes for R-SPEC-F (`COVERAGE.md`, `HOLES.md`, `README.md`)

1. `COVERAGE.md`: no row cites [F11 §13.1] or [AR §7.4] C8; none needs a change for NC-10. A row that later maps
   [AR §7.4]'s pack classes may cite [F11 §13.1] for C8's cursor.
2. `HOLES.md`: no hole added, renamed or removed by R-SPEC-R this round. `README.md`: the [F11] row may say "change-feed
   and pack cursors" beside `SESSMARKS`; optional.

### 7.6 Notes for R-MODEL

1. [RULES/pack-classes] open point 8's review note ("none carries T, so PX-011's record has no bytes yet") is answered
   on the row side: [F11 §13.1] `feed` 2 with the task's `#N`; PT-028's cursor(A, T) is the row of the session the pack
   runs in. Once [F05 §9.11] has the record field, PX-011 and PT-028 may cite both.
2. MF-009 ("for `settled` records written by `complete`: the lease holder") and [F05 §9.5] field 9 (any lease holder of
   the settling write) should state the same set of writes; [F11 §7] `actor` follows [F05] (§7.4 item 2).

### 7.7 Round 3 follow-up (closure NC-11, NC-12)

[pass1-closure] (round 3) §4.2 lists NC-11 against [F11 §13.1] and open point 42, and NC-12 against [F11 §14] and open
point 1. The following were re-read in full before the edits: [F11 §1.2], §2.1, §2.3, §7, §13 and §14, and open
points 1 and 42; [F05 §9.5] fields 9 and 11, §9.11 and open point 14; [F13 §3] I-F5; [F18 §2.5]; [API §10.5],
§14.1, §15.7 `markers`, and open points 25, 34 and 48; [RULES/state-definition] MF-009; [RULES/pack-classes] PX-011,
PT-028, PM-025 and open point 8; and [owner-questions] OQ-F-3. After the edits, every citation of PX-011, OQ-F-3, the
pack cursor and MF-009 in `docs/spec/` was re-read ([pass1-closure] "Round 3 follow-up").

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-11 (closure §4.2, round 3) | major | 3, follow-up | fixed (R part; [F05 §9.11] and open point 14 are R-SPEC-P's, `pass1-dispositions-P.md` §8.7) | [F11 §13.1] "The pack cursor" said "`pack T` for agent A appends one `feed` 2 record after emitting (PX-011)". It now says that a `feed` 2 row is the cursor of a pack of T for agent A, and cursor(A, T) of PT-028 and its absence (C8 empty, PM-025) are unchanged. The record's bytes and fold are [F05 §9.11]'s. Which actor appends the record (the `pack` verb or the layer that delivers the pack) is OQ-F-3's call. Until OQ-F-3 is answered no M0 command appends one ([API] open point 48, PX-011), so cursor(A, T) is absent, C8 is empty and `pack` stays a read that appends nothing (I-F5, §1.2). Open point 42 no longer says that PX-011 "appends" the cursor. It names OQ-F-3 and the interim (no `feed` 2 row exists), and it notes that OQ-F-3's options (a) and (b) change no byte of the row while option (c) would withdraw `feed` 2. The row layout, key, tag `0x020A` and retention are unchanged, and the paragraph and open point 42 were re-wrapped to 120 columns |
| NC-12 (closure §4.2, round 3) | major | 3, follow-up | fixed | [F11 §14]: the worked `MARKERS` row is still a `settled`, `done` entry whose commit is not a `complete` (`outcome` 0, byte 36 `00`). It now stores `actor` 0: offset 32 `00 00 00 00`, comment `actor = 0 (not a complete)`. The prose no longer says that the row was written "under a lease whose holder is actor symbol 5". It says that the commit is not a `complete`, so the entry carries no holder and `actor` is 0 ([F05 §9.5] field 9, MF-009). The closure's other option, a `complete`'s row with `outcome` 1, is not taken. The example keeps the case it states, and a non-`complete` entry with holder 0 is what every entry except one per `complete` carries. No checksum, length or digest in the example covers the changed bytes. `RtHdr` carries none, and the body stays 100 bytes (24 + 72 + 4; `n_rows` 1, `row_size` 0x48, `heap_len` 4). No other text or fixture quotes the old bytes. Open point 1: `actor` is "the holder of the lease a `complete` presented, else 0, as [F05 §9.5] field 9 carries it" (was "the lease holder of a leased completion, else 0"). The point was re-wrapped to 120 columns, and no other word changed |
| A1-6 | blocker | 3, follow-up | fixed (residue) | As NC-12. §7.1's A1-6 row said that "the §14 example's wording" followed the round-3 `actor` rule, but its bytes and prose still named holder 5 for a non-`complete` entry. They now follow the rule |
| S1-16 | blocker | 3, follow-up | fixed (residue) | As A1-6 |
| A1-23 | major | 3, follow-up | fixed (residue) | As NC-11: [F11 §13.1] now states the interim that [API] open point 48 and PX-011 state. A1-23 stays "closed, owner" (OQ-F-3) |

Rejected: none. Left open: none. Numbers other chapters must use are unchanged (§7.3). [F11 §7] `actor` ("the lease
holder of the completion, [F05 §9.5] field 9") agrees, because it takes field 9 exactly, so it was kept as written.
