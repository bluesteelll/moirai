# Review pass 1: dispositions of R-SPEC-P

| | |
|---|---|
| Title | Dispositions by R-SPEC-P of the pass-1 findings routed to it ([P-pass1], [S-pass1], [A-pass1]) and of the items [pass1-closure] lists against its chapters |
| Status | review, pass 1, rounds 1 to 3 (round 2 in §7, round 3 in §8); dispositions await the owner's signature (WP-80, V2) |
| Work package | WP-80 pass 1, author side: R-SPEC-P (WP-11, WP-16, WP-17) |
| Files of R-SPEC-P | [F03], [F04], [F05], [F13], [F15], [F16], [F17], `os/*` |

Dispositions:

- **fixed**: the change is made in the named sections. Where the fix changes bytes, codes or rules another chapter uses,
  that chapter was aligned in the same round against a fresh read of its section (cross-role edits are marked **x** and
  listed in §2).
- **fixed (P part)**: the part of the fix in R-SPEC-P's files is made; the rest lies with the role named.
- **verified**: the fix was made by another role in R-SPEC-P's file (a cross-role edit); R-SPEC-P re-read it and keeps it.
- **rejected**: with the reason (none this round).

Summary: 76 findings disposed this round — 69 fixed (11 blockers, 28 majors, 30 minors; 16 of them "P part"), 7 verified
(5 blockers, 1 major, 1 minor: cross-role edits of R-SPEC-R and R-MODEL kept), 0 rejected, 0 left open; the two
dispositions of the earlier round (S1-27, S1-47) stand.
Three owner questions are raised (OQ-P-1 to OQ-P-3 in [owner-questions]). Numbers other roles must use are in §3.

Round 3 (§8): the closure's two items against these chapters are fixed — NC-10 (the pack-cursor record of [F05 §9.11])
and the editorial residue of [F16] open point 10 — with the residues of A1-6, S1-16 (the `Marker` holder and outcome of
[F05 §9.5] against MF-009), A1-23 and A1-27; 81 findings re-verified with no change; 0 rejected; 0 left open.
Round 3 follow-up (§8.7): closure NC-11 is fixed in [F05 §9.11] and open point 14 (OQ-F-3's interim), with no byte
change.

## 1. Dispositions

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| S1-27 | major | 0 | fixed | (earlier round) [OS/README §4.6] `Entropy::fill_random`; see the first version of this file |
| S1-47 | minor | 0 | fixed | (earlier round) Coverage, Holes and Open points in every `os/` file; [RULES/README §1.1] |
| A1-11 | blocker | 1 | fixed | [F05 §9.2] reason 5 `park` (`old` zero when the record creates `orphans/<R>`, no absorbed vector), §9.10 partial-upsert fields of a park, §10.2 `next_ref_id` for reasons 1 and 5, §10.3 the move; [F16] P-70 cites the bytes, its seeded bug names them (§17.3). [F12 §8.2]'s citation now resolves |
| A1-12 | blocker | 1 | fixed | [F05 §7] kind 27 `Reserve` (durable), §9.27 payload (ref, `hlc`, `cs` and `blobs` numbers, `#N` and `aN` ranges, `SchemaRes` list; symbols in its `SymDefs` block), §4.7 reservation group, §10.2 fold (`next_id`, `next_anchor`, `next_file_no`), §10.3 schema-id map; registry range 0 and 29–255 invalid (§3.1, §7). [F16] P-6, P-31, P-78, P-84 (producer's release check; `gc` releases the files of a reservation whose commit never landed after `gc.cruft-delay`), open point 5 closed; [F04 §5.13] cites P-78 |
| P1-3 | blocker | 1 | fixed | As A1-11 and A1-12; the seeded bugs of P-70 and P-84 name `RefUpdate` reason 5 and `Reserve` kind 27 ([F16 §17.3]) |
| S1-11 | blocker | 1 | fixed | As P1-3; the checkpoint group carries the holder-3 merge pins ([F05 §4.7], [F16] P-81, open point 15 closed) |
| A1-13 | blocker | 1 | fixed | [F17 §2.1] bytes 16–31 are `store_id` (`b16`), the layout of [F04 §4.4]; IP-2 checks it non-zero, IP-1 keeps it through `restore` and `repair`; [F04 §4.4] and open point 2 updated |
| S1-12 | blocker | 1 | fixed | As A1-13 |
| P1-4 | blocker | 1 | fixed | As A1-13 and A1-15; `COVERAGE.md` row 60-AR-HEAD-params can lose its CONFLICTING mark (§4) |
| A1-15 | blocker | 1 | fixed | The `project` root's algorithm is the init-fixed `HEAD.project_oid_algo`, `u8` at slot offset 1072 ([F04 §3.1], §5.16, §6, §7 check 5, §8.1 IP-3), because the 32-byte block has no spare byte; [F17 §2.1]–§2.2 list it beside the block under IP-1–IP-3; the extent head carries it ([F05 §9.28]). **x** [F20 §2.3] and open point 27 cite the name and offset; **x** [CFG §7.6] records it at `init` from `extensions.objectFormat` (1 without a repository) |
| S1-28 | major | 1 | fixed | As A1-15 |
| A1-17 | blocker | 1 | fixed | [F16] P-36 is [API §6.2] CK-4: two maxima `hlc_seq` (the semantic durable records) and `hlc_commit` (every commit's `hlc`); semantic records draw from and raise `hlc_seq`, a local commit draws from both; `Checkpoint`, `Reserve`, `Lazy`, `SessionMark` and runtime rows carry a value and raise nothing. The maxima live in `HEAD` ([F04 §5.15], offsets 1080, 1088) and are folded by [F05 §10.2], so an append needs no scan below `committed_lsn`; they survive epoch re-rolls in the extent head (P-75, [F05 §9.28]). [OS/clock §7] and §10 restated; [F16] P-50, P-89, open point 18; [F13] OP-13-10 (strict) |
| S1-13 | blocker | 1 | fixed | As A1-17 |
| P1-5 | blocker | 1 | fixed | As A1-17; P-36's seeded bug is "a `Checkpoint` advances the sequence" ([F16 §17.3]) |
| A1-6 | blocker | 1 | verified | R-SPEC-R's cross-role edits of [F05] (§8.5 row images, §8.7, §9.3, §9.4 field 27, §9.8, §9.10 fields 20–23, §9.15–§9.19, §9.22–§9.24, §9.26, §10.3, open points 9, 12, 14) re-read and kept. This round adds the [F11 §13] fold targets to [F05 §7] (A1-23) and `FileRefV` (P1-25) |
| S1-8 | blocker | 1 | verified | As A1-6 |
| P1-2 | blocker | 1 | verified | As A1-6 (the [F05] part: roots in `FsIntent`, `Pending` and `FileObs` paths; `RefTable` partial upsert) |
| A1-8 | blocker | 1 | verified | [F05 §9.3] `ClientHead` carries the `HEADS` row image with [F18 §3.2]'s `BindingExt` (R-SPEC-R's edit, kept) |
| S1-10 | blocker | 1 | verified | As A1-8 |
| A1-18 | major | 1 | fixed | [OS/fs §4.9.4]: `swap_recover` is run only by `doctor` and by `restore` for its own failed swap; discovery only probes and retries ([F16] P-86, [F02 §3.2]) |
| P1-13 | major | 1 | fixed | As A1-18 |
| S1-26 | major | 1 | fixed | As A1-18 |
| A1-23 | major | 1 | fixed | [F05 §7] fold targets of `Lazy`, `Backup` and `SessionMark` name [F11 §13] `CURSORS`, `BACKUPS`, `SESSMARKS` (R-SPEC-R added the tables and the §10.3 text) |
| A1-24 | major | 1 | fixed | The length rule applies at or below the extent holding the end of the valid log, and to a longer file anywhere; a shorter file beyond is an interrupted preparation, a full-length one a spare ([F05 §2.2], [F17 §2.2] IP-6, [F16] P-72, open point 4); [OS/fs §4.5] `recycle_extent` sets the length to `len` for every method |
| S1-24 | major | 1 | fixed | As A1-24 |
| A1-25 | major | 1 | fixed | [F04 §10] follows [F16] P-88: `next_ref_id` 1, `refs_lsn` the `main` group's `RefTable`, `heads_lsn` with a binding, `committed_lsn` = `durable_lsn` = the end of the `main` group, the HLC maxima, `project_oid_algo`; open point 11 closed; [F16] open point 14 closed |
| A1-26 | major | 1 | fixed | [F17 §8.2] and OP-17-15 state lens S's D-4 reading as confirmed (hint `SuspectBudget` of [F19 §12.3]) and list the owner's sign-off as OQ-P-1; the [F13] part (OP-13-05) was R-MODEL's and is closed |
| A1-27 | major | 1 | fixed | [F16 §17.4] seeded bugs L-1–L-8 for [F03 §8.4] SR-2 and SR-5, [F03 §8.6], [F03 §10.3], [OS/proc §6.2], [OS/lock §5.4] I-L4/I-L2 and I-L6, and [F03 §3.1] rule 2; GT18 carries five, the toy log three; open point 1 counts them |
| A1-28 | major | 1 | fixed | [F15 §6.5]: FM-3 applies to `ProjectFs` only as a failed `sync_dir` (no file-content flush: no copy path); FM-5 lists renames, unlinks and trash moves |
| A1-33 | major | 1 | fixed | [OS/fs §6.1] `VfsErrorKind` adds `CloudOnly`, `IsSymlink`, `IsDirectory`, `OutsideRoot`, `Stale` (ProjectFs only), with mapping rows in §6.2; [OS/project §2.3] and open point 6 |
| A1-40 | major | 1 | fixed | [OS/env §5]: the probe files are `tmp/probe.<nonce>` with three fresh nonces; open point 5; [F16] P-79 |
| P1-7 | major | 1 | fixed | New [F16] P-96: maintenance prepares the spare `log.<n+1>` at E/2 under `tmp/extent.<nonce>` and renames it into place, under the maintenance byte only (a paused preparer never writes a name an appender uses); P-72 step 2 re-issues `durable+meta` and `durable-name` on a spare (sub-millisecond) and prepares under the flush byte only as the fallback; P-1, P-8, P-74, P-76, P-79, open point 3; seeded bug of P-96 (§17.3). [F17 §13.2] (the E/2 point is a protocol constant), Holes `F17-lock-flush` (measurement 2 includes rotations); [OS/fs §4.5]. **x** [F02 §5.3] and §6.3 `tmp/` word `extent`. M1's gates: for WP-81a ([60 §5.2] item 2) |
| P1-8 | major | 1 | fixed | New record kind 28 `ExtentHead` ([F05 §9.28]), the first group of every extent (138 B) and the epoch-start group (replacing the 40-byte `Noop`): `chain_in`, `epoch_lsn`, `init`, `project_oid_algo`, the `quiet`/`readonly` flags, the counters and the HLC maxima as of its append; [F05 §2.4], §2.7, §4.4, §4.5, §4.7, §5.1, §7, §9.12, §10.2, open point 21; [F04 §6], §8.1, §10; [F16] P-72, P-75, P-85 (the slot-less `repair` algorithm), P-88, P-97 with its seeded bug, open point 26. `hist` keeps the heads ([F10 §4.1]; R-SPEC-R can close OP-10-19). **x** [API §6.2] CK-6 names the carrier |
| P1-9 | major | 1 | fixed | New [F16] P-98: a long job (rollup, GC rewrite, `backup` copy) keeps the maintenance byte, works in steps of one file, and at each step boundary runs a yield checkpoint when C1 holds (no promotion, retirement, rollup or GC; releases only its own earlier yield deltas); a rollup re-folds the yield window over its new base. [F17 §5.2] states the bound, measurement 10 checks it; C-4 becomes `2 + P14 + n_other ≤ 8` (P14 ≤ 5); [F17 §6.1], OP-17-26; [F04 §4.1]. **x** [F10 §7.1] gitmap-page bound 2 + P14 during a long job |
| P1-10 | major | 1 | fixed | [F03 §3], §3.1: nine quiet bytes (`ROLE_BASE` + 3 and + 5 … + 12); each requester holds its own; the decider probes all nine; open point 9. [OS/lock §2] (`LockByte::Quiet(QuietIndex)`), §8, open point 11; [F17 §5.3]; [F16 §17.4] L-8. **x** [F19 §10.2] `store_locked` names the `quiet` lock (all nine busy). Code follow-up for WP-30 |
| P1-11 | major | 1 | fixed | [F17 §1.5], §4.4 W2 (the `wmem` budget keys of [CFG §10.5], `max(256 KiB, min(requested, headroom))`), W4, §12 TP-3, §13.1, §13.2, OP-17-11: `tx.wmem-max` gone. Residue: an agent `TX` is bounded by P05 (≈ 4,400–5,500 ops) below `tx.max-ops` = 10,000: owner question OQ-P-2, OP-17-25 |
| P1-12 | major | 1 | fixed | [F17 §3]: `init` refuses a combination that fails C-1–C-4 (exit 2); a tunable's fallback is min(production value, the largest admissible under the recorded init-fixed values) ([CFG §5.3]); TP-2 passes the whole profile to `init` |
| P1-15 | major | 1 | fixed | [OS/project §2.3] the Windows name check: every segment of every path through `representable_here` before any OS call, `InvalidName`, never relaxed by `--allow-nonportable`; §9 simulator, open point 22; [OS/path §6], §8.1; [OS/mapping-appendix §2.1]. [F20 §4.9] and [F18 §4.6] were aligned by their roles |
| P1-16 | major | 1 | fixed | [OS/project §6.2]: the plan step's `sync_dir` turns `Unsupported`/`AccessDenied` into `no_dir_flush` with nothing changed; recovery's re-barrier on such a volume leaves the intent open for `doctor` ([F16] P-71, P-91); `VolumeCaps` bit 14 `dir_flush_doubtful` by file-system class ([OS/project §4.2], §4.3); FL-2 profiles for both paths (§9), open point 23. [F11 §12.3]'s restatement of the bit is R-SPEC-R's (§4) |
| P1-18 | major | 1 | fixed | New [F16] P-100 (loose objects: `durable+meta`, `rename_noreplace` into `objects/<xx>/`, `durable-name` on `objects/<xx>/` and a new `objects/`, all before the ref's `.lock` step) with its seeded bug; §3 row, §13.5; P-23 names the pack path; [OS/fs §4.4.6] |
| P1-19 | major | 1 | fixed (P part) | [F17 §4.4] W1 lists `sync` (with `merge --continue`, `revert`, `cherry-pick`, [API §9.10]); W2 charges and spills the item-10 sorts ([F07 §10.6]). GT11's 14-days-behind case is [60 §3.13]'s, for WP-81a |
| P1-21 | major | 1 | fixed (P part) | [F13 §5] V05 `DepthExceeded` 67, V07 `Cardinality` 68, V12 `PlanMask` 72 with [F19 §12.2] and [F12 §7.9]; OP-13-06 closed. The merge-table rows are R-MODEL's |
| P1-22 | major | 1 | fixed (P part) | [F17 §6.1]: P14 also bounds a pair's `gitmap` pages ([F10 §7.1]); the rest was R-SPEC-R's |
| S1-18 | major | 1 | fixed | [F05 §4.4] G-2 bounds an ordinary group at `E − R`, R = H + 40 = 178 B (the extent head included), G-3 places the head first, G-5 and open point 1 corrected; [F17 §4.4] W3 and §13.2 use R |
| S1-20 | major | 1 | fixed | [F16] P-34 "re-validation by key; a bulk commit by node": every node owning a row of `cs.<n>` counts as read and written, any intervening touch re-runs phase 1; §17.3 row renamed |
| S1-25 | major | 1 | fixed | [F16] P-92: a read error at or above `durable_lsn` ends a reader's view but stops a writer's scan (appender, flush holder, boot recovery, `repair`) with exit 7 `store_io_fault`; P-29, P-42, P-66, P-85, open points 13 and 29, seeded bug; [F05 §5.3], open point 22; [F15] FM-12.4. **x** [F19 §10.2] `store_io_fault` lists the trigger |
| S1-29 | major | 1 | verified | R-SPEC-R's edit of [F17 §4.3] (cites [F10 §4.2] as the one frame rule) re-read and kept |
| P1-24 | minor | 1 | fixed | [F03 §6.3] WD-1 states the cost (one 512-byte cached write per holding, three per durable commit) and that measurement 2 includes it; written every time, because WD-4 needs it most for a holder that dies early |
| P1-25 | minor | 1 | fixed | [F05 §8.4] `FileRefV`, the record form; §9.8, §9.9 use it; the family values cite [F11 §2.5] |
| S1-37 | minor | 1 | fixed (P part) | As P1-25; [F05] no longer restates `FileFamily` |
| P1-33 | minor | 1 | verified | R-MODEL's renames in [OS/lock], [OS/fs], [F03], [F16] checked: every hole id in R-SPEC-P's files has the `<part>-<name>` form |
| A1-42 | minor | 1 | fixed (P part) | As P1-33 |
| S1-40 | minor | 1 | fixed (P part) | As P1-33 |
| P1-34 | minor | 1 | fixed | [OS/clock §4.5], §6: the GC deletion grace is measured on the HLC from the releasing `Checkpoint` ([F17 §11.4], [F16] P-89); [F16] open point 8 closed |
| S1-43 | minor | 1 | fixed | As P1-34 |
| P1-35 | minor | 1 | fixed | [OS/README §4.2]: the cross-volume `file mv` is a refusal ([OS/project §6.4], [F16] P-83); open point 5 closed |
| A1-47 | minor | 1 | fixed | [OS/README §1.3] and §3 name [OS/proc §11]–§13 and [OS/shell §10]; open point 6 closed; [OS/fs §6.4] and [OS/map §10] cite `gc.delete-grace` instead of 60 s |
| P1-36 | minor | 1 | fixed | [OS/mem §3]: the Linux `smaps_rollup` cost and a per-request cache if the port measures it above budget; §6.2: `ACTIVE` stored once after a relaxed load |
| P1-37 | minor | 1 | fixed (P part) | [OS/path §4.1] step 3: `GetFinalPathNameByHandleW` failures map to `NotFound`/`Unsupported`, no invented spelling, exit 7 and a `doctor` text; §7 step 3 refuses drive-relative `X:rel` and the device forms with exit 2. The exit-7 code and text are R-SPEC-F's (§4) |
| P1-38 | minor | 1 | fixed | [OS/project §5.5] step 3 re-checks the placeholder attributes through the handle (`FileAttributeTagInfo`) before the first read; [OS/mapping-appendix §2.1]; open point 24 |
| P1-39 | minor | 1 | fixed (P part) | [OS/shell §5.2] step 2 reads stdin and `-f` incrementally against `input.max-bytes` ([CFG §10.5]), exit 2 beyond it |
| P1-40 | minor | 1 | fixed | [OS/map §10]: the mapping bound (current sets and pinned sets served, plus the current request's sealed files, unmapped at request end); `RegistryFull` falls back to positional reads; §7, open point 3 |
| P1-43 | minor | 1 | fixed | [F16] P-87: `backup` is a long job that yields by P-98 (as P1-9) |
| P1-44 | minor | 1 | fixed | [F16] P-89: `now` = max(`wall_ms` << 16, `hlc_seq`, `hlc_commit`), carried across epoch re-rolls by P-75 |
| P1-32 | minor | 1 | fixed (P part) | As A1-40; [F16] P-79 lets the sweep remove `probe.<nonce>` and `settle.stamp` |
| S1-38 | minor | 1 | fixed (P part) | As P1-32; [OS/project §5.9] and open point 18 closed |
| S1-41 | minor | 1 | fixed | New [F16] P-99 (the `init --link` pointer file durable before success) with its seeded bug; §3's `LOCK` row states that records need no P-rule ([F03 §12]'s argument) and points to §17.4. **x** [F02 §3.3] rule 6 cites P-99 |
| S1-42 | minor | 1 | fixed | [F03 §8.1] `nonce`: an intent holder draws it before choosing its slot (the search starts at `nonce mod 256`) and keeps it for the holding; §10.3 follows |
| S1-45 | minor | 1 | fixed (P part) | [F04 §10] takes P-88's values and states that `REFS.aux` ([F11 §3.7], R-SPEC-R) never exceeds `HEAD.next_ref_id` |
| S1-49 | minor | 1 | fixed | [F17 §4.2] cites EX-4 and EX-5; [F05] open point 3 |
| S1-33 | minor | 1 | fixed | As P1-21 |
| A1-44 | minor | 1 | fixed (P part) | [F13 §3.9] names [F18 §2.1]–§2.14 as the statements of record, lists the four differences, and lets [F18] win |
| A1-46 | minor | 1 | fixed (P part) | [F04 §10] "every other field equal (the two checksums differ)"; the [F17 §4.3] and §5.4 parts were R-SPEC-R's (verified) |
| A1-49 | minor | 1 | fixed | `total` rows in [OS/clock §3.1], [OS/proc §3.1], [OS/project §3.1], §3.3, §4.2, §7.1, [OS/fs §2.6], §4.9.3 |
| A1-59 | minor | 1 | fixed (P part) | [OS/project §6.4] quotes the fix line of [F19 §10.2] `cross_volume` ("… or show a proposal to confirm") |
| A1-60 | minor | 1 | fixed | [OS/path §8.2] device names add `CONIN$`, `CONOUT$`, `COM¹`–`COM³`, `LPT¹`–`LPT³`; open point 3 |
| P1-26 | minor | 1 | fixed (P part) | [F16] P-37 states the O(`cs_bytes`) in-lock re-serialisation and measurement 2's sweep up to P05 ([F06] open point 16 is R-SPEC-F's) |
| P1-31 | minor | 1 | fixed (P part) | [F16] P-76, P-85, P-87 use `maintenance_busy`. [F19 §10.2]'s row says "whose bounded wait … timed out" and `(waited <t> ms)`, but the byte is only tried ([F16] P-1, P-76): R-SPEC-F's to reword (§4) |

## 2. Cross-role edits made this round (each after a fresh read of the section)

- **[F02] (R-SPEC-F):** §5.3 table and §6.3 `tmp-word`: the word `extent` (P1-7); §3.3 rule 6 cites [F16] P-99 (S1-41).
- **[F10] (R-SPEC-R):** §7.1 "Page bound": 2 + P14 pages while a long job yields (P1-9).
- **[F19] (R-SPEC-F):** §10.2 `store_io_fault` trigger and the §10 lookup row for a writer's read error (S1-25);
  `store_locked` `<lock>` `quiet` and its JSON value (P1-10).
- **[F20] (R-SPEC-R):** §2.3 and open point 27 name `HEAD.project_oid_algo` at offset 1072 (A1-15).
- **[API] (R-SPEC-F):** §6.2 CK-6: h lives in `HEAD.hlc_seq`/`hlc_commit` and crosses re-rolls in the epoch-start extent
  head (P1-8, P1-44).
- **[CFG] (R-SPEC-F):** §7.6 bullet: `init` records `project_oid_algo` from the repository's object format (A1-15).

## 3. Numbers other chapters must use

- Record kinds: 27 `Reserve` ([F05 §9.27]), 28 `ExtentHead` ([F05 §9.28]); 0 and 29–255 invalid. `RefUpdate` reason 5
  `park`. The epoch-start group is an `ExtentHead` of 138 B (payload 98 B); the rotation reserve R = 178 B; the largest
  ordinary group is `E − 178` bytes.
- `HEAD` slot: 1072 `project_oid_algo` `u8`, 1073 `_pad1` [7], 1080 `hlc_seq` `u64`, 1088 `hlc_commit` `u64`, 1096
  `_reserved1` [2984]; `InitParams` bytes 16–31 are `store_id`.
- `LOCK`: quiet bytes at `ROLE_BASE` + 3 and + 5 … + 12 (nine); reserved + 13 … + 63.
- `VolumeCaps` snapshot flag bit 14 `dir_flush_doubtful`; `VfsErrorKind` gains five `ProjectFs` kinds.
- `store.fold-width` (P14) range 1–5 (C-4 is `2 + P14 + n_other ≤ 8`).
- [F16] rules P-96 (spare extent), P-97 (extent heads), P-98 (long holdings yield), P-99 (`init --link` pointer), P-100
  (loose objects); §17.4 bugs L-1–L-8.
- `tmp/` word `extent` ([F02 §5.3]).

## 4. Notes for R-SPEC-F (`COVERAGE.md`, `HOLES.md`, `README.md`, [F19], [CFG])

1. `COVERAGE.md` rows that can lose CONFLICTING or the §7 "contradicting" note: 60-AR-HEAD-params ([F04 §4.4] = [F17 §2.1];
   add [F04 §5.16] for `project_oid_algo`); 60-AR-Log-kinds (add [F05 §9.2] reason 5, §9.27 `Reserve`, §9.28
   `ExtentHead`); 60-I2-PD(e) and F14 ([F16] P-36 = [API §6.2] CK-4; [F04 §5.15]); 60-AU-Vfs-renames ([OS/fs §4.9.4]).
   Rows that may cite new sections: 60-AR-LOCK ([F03 §3.1] the nine quiet bytes); X-F3 ([F05 §4.5] extent heads,
   [F16] P-96, P-97); 60-AR-HEAD ([F04 §5.15]); X-F5 and 60-PA-(h) ([F16] P-100 loose objects).
2. `HOLES.md`: no hole added, renamed or removed by R-SPEC-P this round. `F17-lock-flush`'s constraint now names
   rotations in measurement 2 ([F17] Holes).
3. [F19 §10.2] `maintenance_busy`: the maintenance byte is only tried ([F16] P-1, P-76; [OS/lock §6]), so "whose bounded
   wait … timed out" and `(waited <t> ms)` / `waited_ms` should become "that finds the maintenance byte held" (P1-31).
4. [F19]: an exit-7 code and text for a root that cannot be canonicalised ([OS/path §4.1] step 3, P1-37), and the exit-2
   `usage` case for a drive-relative or device-form path argument ([OS/path §7] step 3).
5. [CFG §10.5] says a `TX` near the default `tx.max-ops` "needs `--budget wmem=4MiB`"; W1/W4 refuse any agent changeset
   above P05 first ([F17 §4.4]), so the sentence overstates what the raise buys; see OQ-P-2.
6. `README.md`: [F05]'s row may name kinds 27 and 28; [F16]'s row the rule range P-1…P-100.

## 5. Notes for R-SPEC-R

1. [F10] OP-10-19 can close: the anchor record is [F05 §9.28] `ExtentHead`, kept by [F10 §4.1]'s rule.
2. [F11 §12.3]: the `VolumeCaps` snapshot has flag bit 14 `dir_flush_doubtful` ([OS/project §4.2]); [F11] open point 21.
3. [F09] OP-09-17 and [F11 §9.1]: the reservation record is [F05 §9.27]; ids of an unused reservation stay skipped.
4. Please review the [F10 §7.1] and [F20 §2.3] alignments of §2.

## 6. Notes for WP-81a and the owner

- M1's writer-hold and last-acknowledgement gates and measurement 2's workload include extent rotations (P1-7;
  [60 §5.2] item 2). GT11 covers a `sync` of a lane 14 days behind (P1-19; [60 §3.13]). Measurement 10 records a rollup's
  and a GC's step durations and the overlay high-water mark while they run (P1-9).
- Owner questions OQ-P-1 (the `suspect` budget), OQ-P-2 (a default-cap `TX` against P05) and OQ-P-3 (confirmation of the
  pass-1 additions: quiet bytes, `HEAD` fields, kinds 27 and 28, spare extents, yields, writer read-error refusal).
- Code follow-ups for WP-30 (the Rust agents are not touched by this round): `LockByte::Quiet(QuietIndex)`; the five
  `VfsErrorKind` values; `VolumeCaps::dir_flush_doubtful`; `hlc_next(wall_ms, h)` over the two maxima.

## 7. Round 2

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1], at every severity, whose fix lands in [F03], [F04], [F05],
[F13], [F15], [F16], [F17] or `os/*`, and every item [pass1-closure] (round 1) lists against these chapters: its
contradictions NC-3 and NC-4 (§4), the `COVERAGE.md` row 60-I2-FM(12) that waits on NC-3 (§5), and NC-6, whose rule
[F05 §5.4] owns. Also the requests of `pass1-dispositions-R.md` §6.6 and `pass1-dispositions-M.md` ("Notes for other
roles"). Every section named below was re-read in its current text, after the other roles' round-1 and round-2 edits.
Following the closure's open point 3, each datum a round-1 fix changed (the read-error rule, P14's range, the extent
head and rotation reserve, the probe names, the new [F19] codes, the runtime window, the reservation record) was searched
in every chapter of R-SPEC-P for a restatement that still carried the old value; the hits are the residues fixed below.

**Round 2 summary.** 85 findings re-checked: 13 fixed (residues of findings closed in round 1: 3 blockers, 6 majors,
4 minors), 72 verified with no change (16 blockers, 26 majors, 30 minors), 0 rejected, 0 left open. Closure
contradictions closed: NC-3, NC-4, and NC-6 (a cross-role edit, **x**). No hole, record kind, offset, enumeration value or
section tag changed; no new owner question (OQ-P-1 to OQ-P-3 stand; OQ-M-2 is recorded in [F13] OP-13-09).

### 7.1 Dispositions

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| S1-25 | major | 2 | fixed (residue; closes closure NC-3) | [OS/fs §6.2] row `Io`, "Callers' reaction": judged by position and by caller as [F16] P-92 states — below `durable_lsn` corruption (`store_corrupt`); at or above it a reader's view ends, and a writer's scan (appender, flush holder, boot recovery, `repair`) appends nothing and exits 7 `store_io_fault`. The old cell ("end of log above it") was the rule S1-25 removed. [F05 §5.3] now names `repair`'s scan and the code, as P-92 and [F15] FM-12.4 do |
| P1-9 | major | 2 | fixed (residue; closes closure NC-4) | [F04] open point 4: `segments` lists `main`'s base, its deltas, the one yield delta of [F16] P-98 and the dictionary, so C-4 (`2 + P14 + n_other ≤ 8`) gives P14 ≤ 5, the range of [F17 §3] (was "P14 ≤ 6"). [F17] OP-17-06 said "the range 1–6 of P14" and "[F04] decides": now closed with the same statement. P1-9 also named [OS/proc §11]: its bulk-pass paragraph now says that lowered priority lengthens a long job but never the tail, because the job yields by P-98 ([F17 §5.2]) |
| P1-16 | major | 2 | fixed (P residue) | P1-16 named [OS/fs §6.2]: its `Unsupported` and `AccessDenied` rows now give the `ProjectFs::sync_dir` reactions of [OS/project §6.2] and [F16] P-71 (plan step: exit 7 `no_dir_flush`, nothing changed; recovery's re-barrier: the intent stays open for `doctor`) beside the store's. [OS/project §2.3]'s kind table names the `sync_dir` cases of both kinds. [F11 §12.3]'s bit 14 is R-SPEC-R's round-2 edit (closure NC-1), re-read: byte for byte equal to [OS/project §4.2] |
| S1-18 | major | 2 | fixed (residue) | [F17 §4.1] "Decision points" still said "[F05] gives the padding rule when fewer bytes remain than a minimal `Noop` group needs", a case [F05 §4.4] G-4 excludes since S1-18; it now says the pad is followed by the next extent's head (G-3, §4.5) and that G-4 always leaves room for the pad |
| P1-8 | major | 2 | fixed (residue) | As S1-18 (the extent head in the rotation of [F17 §4.1]) |
| A1-40 | major | 2 | fixed (residue) | [OS/env] Appendix A row "Lock probe" still named `tmp/probe`; it now names `tmp/probe.a` on all three OSes, as §5 step 4 does |
| P1-32 | minor | 2 | fixed (residue) | As A1-40 |
| S1-38 | minor | 2 | fixed (residue) | As A1-40 |
| P1-37 | minor | 2 | fixed (residue) | [F19 §10.2] now has the codes that round 1 asked for; [OS/path §4.1] step 3 names exit 7 `no_canonical_path`, and §7 step 3 names exit 2 `bad_path` with `rule` `drive-relative` or `device`, both citing [F19 §10.2] |
| A1-46 | minor | 2 | fixed (residue) | [F17 §5.2] still listed four runtime-only fold targets (`FILEOBS`, `FPRINT`, `GITRENAMES`, `ANCHORRES`) beside §5.4's ten; it now cites §5.4 and [F09 §15.1]. [F05] open point 6 records that [F17 §5.4] lists `ANCESTRY` |
| P1-3 | blocker | 2 | fixed (residue) | For `pass1-dispositions-R.md` §6.6 item 2: [F16] P-87 step 2 copies a reservation's `cs.<n>` and `blobs.<n>` only when its bulk `Commit` lies below S.`committed_lsn`, and leaves them out otherwise, whether the reservation is in the log or its `FILES` row carries [F09 §14.4]'s `reserved` flag (which names no content). [F05 §9.27] and [F16] P-84 cite [F09 §16.4] "Ids" instead of the closed OP-09-17. [F05] open point 8 records that [F09 §14.4] `FILES` and `SegHdr.rt_upto_lsn` answer its requests |
| S1-11 | blocker | 2 | fixed (residue) | As P1-3 |
| A1-12 | blocker | 2 | fixed (residue) | As P1-3 |
| NC-6 (closure) | major | 2 | fixed **x** | [F06 §2.4] (R-SPEC-F) said a V-rule break makes the record invalid "with [F05]'s consequence (the end of the log above `durable_lsn`, corruption below it)"; [F05 §5.4], the single owner of the rule, makes a malformed payload of a valid record corrupt wherever it lies. The bullet now cites [F05 §5.4] (exit 7, `moirai doctor --fsck`). Smallest edit, after a fresh read of §2.4; §4.1's "finds the record invalid" is read through §2.4 and needs no change |
| A1-6, A1-8, A1-11, A1-13, A1-15, A1-17, P1-2, P1-4, P1-5, P1-6, S1-8, S1-10, S1-12, S1-13, S1-15, S1-16 | blocker | 2 | verified, no change | Re-read after the round-2 edits of R-SPEC-R and R-MODEL: [F05 §9.2], §9.3, §9.5, §9.10 against [F11 §3.9] (the park's field sources, round 2), §7, §9.27, §9.28; [F04 §3.1], §4.1 (`SegRef.blake3_16` is the header digest, as BK-2's `b3`), §4.4, §5.15, §5.16, §10; [F17 §2.1]–§2.2; [F16] P-36, P-52, P-65, P-70, P-84; [OS/clock §7]; [F13 §3.5] I31′, §4.1–§4.2. R-SPEC-R's **x** edit of [F13 §3] I35′, EP-W8 ("an `ALLOC` row that binds a uid is never rewritten; only the hole of an id reserved by a `Reserve` record is filled, once", `pass1-dispositions-R.md` §6.3) is kept: it agrees with [F05 §9.27] and [F11 §9.1], and I35′ binds a `#N` to at most one uid either way |
| A1-18, A1-23, A1-24, A1-25, A1-26, A1-27, A1-28, A1-33, P1-7, P1-10, P1-11, P1-12, P1-13, P1-14, P1-15, P1-18, P1-19, P1-21, P1-22, S1-20, S1-24, S1-26, S1-27, S1-28, S1-29, S1-30 | major | 2 | verified, no change | [OS/fs §4.5], §4.9.4, §6.1; [OS/project §2.3], §4.2, §6.2; [OS/README §4.6]; [F03 §3.1]; [OS/lock §2], §8; [F05 §2.2], §7; [F04 §10]; [F15 §6.5]; [F16] P-34, P-72, P-96–P-100, §17.4 L-1–L-8; [F17 §3], §4.4 W1–W4, §5.2, §6.1, §8.2; [F13 §5], §6.2. A1-26 and P1-11 stay "closed, owner" (OQ-P-1, OQ-P-2) |
| P1-24, P1-25, P1-26, P1-30, P1-31, P1-33, P1-34, P1-35, P1-36, P1-38, P1-39, P1-40, P1-43, P1-44, S1-33, S1-37, S1-40, S1-41, S1-42, S1-43, S1-44, S1-45, S1-47, S1-49, A1-42, A1-44, A1-47, A1-49, A1-59, A1-60 | minor | 2 | verified, no change | The round-1 text is present: [F03 §6.3] WD-1, §8.1; [F05 §8.4] `FileRefV`, §9.16 (exit 8 as [F19 §7.1]); [F16] P-37, P-76, P-85, P-87, P-89, P-99; [F17 §4.2], §4.3, §5.4; [F13 §3.9], §5; [F04 §10]; [OS/clock §4.5], §6; [OS/README §1.3], §3, §4.2; [OS/mem §3], §6.2; [OS/project §5.5], §6.4; [OS/mapping-appendix §2.1]; [OS/shell §5.2]; [OS/map §10]; [OS/path §8.2]; every `HOLE(…)` of these chapters has the `<part>-<name>` form and is indexed in `HOLES.md`; every offset table of [F03], [F04], [F05], [F17] and `os/*` re-summed by script to its `total` row (A1-49) |

Rejected: none. Left open: none among the pass-1 items of these chapters.

### 7.2 Other round-2 edits in R-SPEC-P's chapters (no finding of their own)

- [F13] OP-13-09: [RULES/status-machines] GD-005 reads I13's "different actor" differently from the draft's proposal; the
  choice is the owner's (OQ-M-2, raised by R-MODEL), and the open point now says so instead of claiming that the rule
  table carries the draft's reading. The I13 row states no reading, so no invariant text changed.
- [F17 §11.4] "Other deletion paths" states what [F16] open point 7 asked: numbered orphans are deleted by claim without
  the grace (P-78, P-79), and a `tmp/` entry by its age against P34 (P-79). [F16] open points 6 and 7 are marked done.
- [F04] open point 8 cites [F19 §10.2] `readonly_flag`; [F05] open point 10 records that [F16] P-71 adopts the abort
  reasons 4 and 5.

### 7.3 Cross-role edit of this round (after a fresh read of the section)

- **[F06 §2.4] (R-SPEC-F):** the V-rule bullet cites [F05 §5.4] (closure NC-6). Please review.

### 7.4 Notes for R-SPEC-F (`COVERAGE.md`, `HOLES.md`, `README.md`)

1. Row 60-I2-FM(12) no longer cites contradicting sections: [OS/fs §6.2]'s `Io` row now states [F16] P-92 and
   [F15 §3.12] FM-12.4 (closure §5, NC-3); it needs no CONFLICTING mark.
2. Rows that may cite the round-2 text: 60-AR-HEAD-params may add [F04] open point 4 for P14 ≤ 5; X-F8 and
   60-AU-Vfs-projfs may add [OS/fs §6.2] for the `ProjectFs` `sync_dir` reactions (P1-16).
3. `HOLES.md`: no hole added, renamed or removed by R-SPEC-P this round. `README.md`: no change needed.
4. Not a pass-1 finding, recorded for the owning chapters: [F16] open point 10 still asks [F19] for the text a process
   prints when discovery finds a store with `HEAD.retired` set and no swap intent (only `doctor` clears it). No [F19] code
   covers that case today.

### 7.5 Notes for R-SPEC-R

1. The [F13] I35′ edit of `pass1-dispositions-R.md` §6.3 is kept (§7.1).
2. [F16] P-87 now handles `FILES` rows with the `reserved` flag as §6.6 item 2 asked; please check it against
   [F09 §14.4].

## 8. Round 3

Scope: every finding of [P-pass1], [S-pass1] and [A-pass1], at every severity, whose fix lands in [F03], [F04], [F05],
[F13], [F15], [F16], [F17] or `os/*`; the items [pass1-closure] (round 2) lists against these chapters — NC-10 (the
record, §4.2) and the editorial residue of [F16] open point 10 — and the notes the other roles left for R-SPEC-P
(`pass1-dispositions-R.md` §7.4, `pass1-dispositions-F.md` "Notes for other roles", `pass1-dispositions-M.md` round 2
"Notes for other roles"). As the closure's open point 3 asks, every rule row or chapter that a round-3 edit cites was
re-read for the exact case: [RULES/pack-classes] PT-001, PT-028, PT-032, PX-011, PM-025 and open point 8, and
[F11 §13.1] (R-SPEC-R's round-3 row) for NC-10; [RULES/state-definition] MF-009, ME-001, ME-003, ME-006, ME-007,
[F11 §7] `actor` and `outcome`, and [API §10.5] for the `Marker` fields; [F02 §3.6] and [F19 §10.2] `store_retired` for
[F16] open point 10. No other role edited a file of R-SPEC-P after round 2 except [F13 §4.1] (R-MODEL's VK-007
citation, reviewed in §8.2). Mechanical checks over every file of R-SPEC-P, by script in the scratch space (not
committed): every `[Fnn §x]`, `[OS/…]`, `[LQ/…]`, `[API]`, `[CFG]` and `[RULES/…]` citation resolves to a heading of its
target (the three misses are numbered paragraphs of [LQ/errors §5] and [LQ/envelope §2]); every rule-row id cited exists
(1,952 defined); every offset table sums to its `total` row; every `HOLE(…)` id is indexed in `HOLES.md`.

**Round 3 summary.** 86 items: 5 fixed (the closure's NC-10; the residues of A1-6, S1-16, A1-23 and A1-27), 81 verified
with no change (17 blockers, 30 majors, 34 minors), 0 rejected, 0 left open. One cross-role edit (**x**, [F11 §13.1], one
cell, no byte change). No record kind, offset, section tag or hole changed; the `Lazy` record gains the value `feed` 2
and the field `task`, whose bytes [F11 §13.1] already expected; [F16 §17.4] gains L-9 (P-1 to P-100 unchanged). No new
owner question (OQ-P-1 to OQ-P-3 stand).

### 8.1 Dispositions

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-10 (closure §4.2) | major | 3 | fixed (the record; with R-SPEC-R's round-3 row NC-10 is closed) | [F05 §9.11] `Lazy` `sub` 2: field 8 `feed` gains 2 = the pack cursor of [AR §7.4] C8 ([RULES/pack-classes] PX-011), other values invalid; new field 9 `task` (`nodeid`, present when `feed` = 2, never 0) = the `#N` of the pack's target T (PT-001: any kind, usually a task); field 10 `cursor_seq` holds the pack's `rev` for `feed` 2 (PT-032); `hlc` is field 11. A cursor replaces the row (session, agent, `feed`, `task`) of [F11 §13.1] `CURSORS`, `task` 0 for `feed` 1; cursor(A, T) of PT-028 is the `cursor_seq` of the row (session, A, 2, `#N` of T) of the session the pack runs in, and C8 is empty when it is absent. The pack cursor is a `cursor` for `durability.lazy-kinds` (§6.1). A `feed` 1 record has no `task`, so no existing byte changes. [F05] open point 14 records it. The record matches [F11 §13.1] field by field (`task` `u32` at offset 36 from the `nodeid`, `cursor_seq`, `hlc`), as `pass1-dispositions-R.md` §7.4 item 1 asked. R-MODEL's round-2 proposal (the task's 16-byte uid in the record) is not taken: the row has 4 spare bytes, a `#N` is store-wide and never reused ([F11 §9]), and a lease names its task the same way ([F05 §9.4] field 5); PX-011 and PT-028 need no change |
| A1-23 | major | 3 | fixed (residue) | As NC-10: the last cursor that A1-23's fold targets lacked (R-MODEL's round-2 residue) now has record bytes |
| A1-6 | blocker | 3 | fixed (residue; `pass1-dispositions-R.md` §7.4 item 2) | [F05 §9.5] field 9 `holder` said "the lease holder; 0 when none", which left open whose lease a `settled` entry written by a merge, a fork, an `undo` or a re-emit names, while [F11 §7] `actor` (R-SPEC-R, round 3) takes exactly this field and [API §15.7] prints it. The field now states MF-009's set: for the entry that the commit of a `complete` writes for the task it settles (ME-001), the holder of the task lease that `complete` presented and released ([API §10.5] step 2); 0 for every other entry, re-emits (ME-003, ME-006, ME-007) and `cancelled` holds included. Field 11 `outcome` names the same entry set (1–3 from `--outcome`, 0 otherwise). [F11 §7] and its open point 1 already read it so; no byte changes |
| S1-16 | blocker | 3 | fixed (residue) | As A1-6 (the `MARKERS` row against [F05 §9.5]) |
| A1-27 | major | 3 | fixed (residue; the closure's §4.2 editorial residue, [F16] open point 10) | [F16] open point 10 ended "[F19] needs its text"; since round 2 [F02 §3.6] states what a process does with a store left `retired` and no swap intent (P-86's probe delays, then exit 7) and [F19 §10.2] `store_retired` is its text. Open point 10 now says so. The rule stays [F02 §3.6]'s (P-86 lends it only its delays), and, as R-SPEC-F's note offered, it gets a seeded bug where A1-27 put the bugs of protocol rules owned by other chapters: [F16 §17.4] L-9 ("a process that finds `retired` set re-runs discovery without a bound and loops", detected by avail, vehicle M1 beside P-85 and P-86); §17.4's lead-in and open point 1's count (nine bugs, L-9 at M1) follow. P-1 to P-100 are unchanged |
| A1-8, A1-11, A1-12, A1-13, A1-15, A1-17, P1-2, P1-3, P1-4, P1-5, P1-6, S1-8, S1-10, S1-11, S1-12, S1-13, S1-15 | blocker | 3 | verified, no change | Re-read against the current text of the chapters that restate them, the other roles' round-2 and round-3 edits included: [F05 §7] (kinds 27 and 28; 0 and 29–255 invalid), §9.2 reason 5 against [F11 §3.9] and [API §15.7] `moves`, §9.3 against [F11 §5] and [F18 §3.2], §9.27 against [F09 §14.4] (a `reserved` row is skipped, R-SPEC-R round 3) and [F11 §9.1], §9.28, §10.2; [F04 §3.1] (1072 `project_oid_algo`; 1080 and 1088 the HLC maxima), §4.4 = [F17 §2.1], §5.15 = [F16] P-36 = [OS/clock §7] = [F06 §4.4.4] = [API §6.2] CK-4 (one list of the records that never advance the sequence, `SessionMark` included), §10; [F16] P-70, P-84, P-87; [F13 §3.5] I31′; `SegRef.blake3_16` = BK-2's `b3` |
| A1-18, A1-24, A1-25, A1-26, A1-28, A1-33, A1-40, P1-7, P1-8, P1-9, P1-10, P1-11, P1-12, P1-13, P1-14, P1-15, P1-16, P1-18, P1-19, P1-21, P1-22, S1-18, S1-20, S1-24, S1-25, S1-26, S1-27, S1-28, S1-29, S1-30 | major | 3 | verified, no change | [OS/fs §4.5], §4.9.4, §6.1, §6.2 (the `Io`, `Unsupported` and `AccessDenied` rows as [F16] P-92 and P-71 and [F19 §10.2] `store_io_fault` and `no_dir_flush`); [OS/project §2.3], §4.2 = [F11 §12.3] bit 14, §6.2; [OS/env §5] and Appendix A; [F03 §3.1] = [F19 §10.2] `store_locked` `quiet` (`lock.writer-wait-ms`); [F05 §2.2], §4.4 (R = 178), §5.3; [F04 §10] = [F16] P-88; [F15 §6.5]; [F16] P-34, P-72, P-92, P-96 to P-100, §17.4; [F17 §3] C-4 (P14 1 to 5) = [F04] open point 4 = [F10 §7.1] (1 + P14, 2 + P14 while a job yields), §4.1, §4.3 (cites [F10 §4.2]), §4.4 W1–W4, §5.2, §6.1, §8.2; [F13 §5], §6.2. A1-26 and P1-11 stay "closed, owner" (OQ-P-1, OQ-P-2) |
| P1-24, P1-25, P1-26, P1-30, P1-31, P1-32, P1-33, P1-34, P1-35, P1-36, P1-37, P1-38, P1-39, P1-40, P1-43, P1-44, S1-33, S1-37, S1-38, S1-40, S1-41, S1-42, S1-43, S1-44, S1-45, S1-47, S1-49, A1-42, A1-44, A1-46, A1-47, A1-49, A1-59, A1-60 | minor | 3 | verified, no change | Checked by search in the current text: [F03 §6.3] WD-1 (the cost, measurement 2), §8.1 and §8.7 (`nonce mod 256`); [F04 §10] (the `xxh3_128` values differ with `slot_seq`; `REFS.aux` ≤ `next_ref_id`); [F05 §8.4] `FileRefV`, §9.16 (exit 8, as [F19 §7.1] row 8 reads); [F13 §3.9] ([F18 §2] wins), §5 (67, 68, 72); [F16] P-37 (O(`cs_bytes`)), P-85 (`maintenance_busy`; the byte is only tried, as [F19 §10.2] now says), P-87 (yields by P-98), P-89, P-99 (= [F02 §3.3] rule 6); [F17 §4.2] (EX-4, EX-5), §4.3, §5.4 (ten sections, `ANCESTRY` included); [OS/clock §4.5], §6 (the grace on the HLC); [OS/README §1.3], §3, §4.2, open point 5; [OS/mem §3], §6.2; [OS/path §4.1] step 3 (`no_canonical_path`), §7 step 3 (`bad_path` `drive-relative` or `device`), §8.2 (`CONIN$`, `CONOUT$`, the superscript digits); [OS/project §5.5] step 3 and [OS/mapping-appendix §2.1] (`FileAttributeTagInfo`), [OS/project §6.4] (A1-59's help line = [F19 §10.2] `cross_volume`); [OS/shell §5.2] (`input.max-bytes`); [OS/map §10] (`RegistryFull` falls back to positional reads); [OS/fs §6.4] and [OS/map §10] (`gc.delete-grace`); every `os/` file ends with Coverage, Holes and Open points; hole ids and offset totals by script (above) |

Rejected: none. Left open: none among the pass-1 items of these chapters. With the owner, unchanged: OQ-P-1 (A1-26),
OQ-P-2 (P1-11), OQ-P-3 (confirmation of the pass-1 additions).

### 8.2 Review of the other roles' edits in R-SPEC-P's chapters

- **[F13 §4.1] (R-MODEL, round 2):** "Refs of kinds `plan`, `merge` (staging), `import`, `tag` and `orphans` are never in
  `W` (VK-002 to VK-005, VK-007)". Kept: VK-007 exists in [RULES/state-definition] (`orphans`, holds no, basis
  derived) and agrees with [F05 §9.2] reason 5 and [F16] P-70, which park a commit on `orphans/<R>` ([F12 §2.2] kind 6).

### 8.3 Cross-role edit of this round (after a fresh read of the section)

- **[F11 §13.1] (R-SPEC-R), `CURSORS` offset 36 `task`:** "the `#N` of the task T the pack was built for" now reads "the
  `#N` of the node T the pack was built for (…; T is [RULES/pack-classes] PT-001's target, of any kind, usually a
  task)". PT-001 is the one owner of T and admits any node kind (the critic's plan pack targets a doc, pack-classes open
  point 5); no byte, key or validity rule changes. Please review.

### 8.4 Notes for R-SPEC-F (`COVERAGE.md`, `HOLES.md`, `README.md`, [API])

1. `COVERAGE.md`: no row cites [F05 §9.11], [F05 §9.5] fields 9 and 11, or [F16 §17.4] against a section that
   contradicts it; none needs a change. A row that later maps [AR §7.4]'s pack classes may cite [F05 §9.11] beside
   [F11 §13.1] for C8's cursor.
2. `HOLES.md`: no hole added, renamed or removed by R-SPEC-P this round. `README.md`: no change needed (the [F16] row
   names P-1–P-100, which are unchanged; §17.4 now holds L-1 to L-9).
3. [API]: `pack` maps to `Query` ([API §18]), and [API] open point 25 says the lazy records that hooks append
   (`SessionMark`, cursors, evidence rows) change no compared state. PX-011's pack cursor is appended by `pack` itself,
   after emitting, and a later `pack`'s C8 reads it (PT-028, PM-025). If `pack` results enter GT2 streams, [API] should
   say whether the model's `Query` for `pack` appends the `feed` 2 cursor of [F05 §9.11] or GT2 compares C8 only where
   both sides start without one. Not a pass-1 finding; recorded for the owning chapter.

### 8.5 Notes for R-SPEC-R

1. NC-10: [F05 §9.11] carries `feed` 2 and `task` exactly as `pass1-dispositions-R.md` §7.4 item 1 expects (field order
   `feed`, `task`, `cursor_seq`, `hlc`; `task` a `nodeid`, never 0 for `feed` 2). Please review the one-cell alignment of
   [F11 §13.1] (§8.3).
2. [F05 §9.5] field 9 now names MF-009's set of writes, which [F11 §7] `actor` and its open point 1 already follow (§8.1,
   A1-6).

### 8.6 Notes for R-MODEL

1. [F05 §9.11] now holds the pack-cursor record that PX-011 appends and PT-028 reads ([F11 §13.1] is the row):
   pack-classes open point 8's request is answered, and PX-011 and PT-028 may cite both. T is held as its `#N`, not its
   uid (§8.1, NC-10).
2. [F05 §9.5] fields 9 and 11 now read MF-009 literally: `holder` and `outcome` are set only on the `settled` entry that
   a `complete`'s commit writes (ME-001) and are 0 on every other entry, a re-emit of a marker that a `complete` once
   wrote included (ME-003, ME-006, ME-007), which therefore resets `MARKERS.actor` and `outcome` to 0. If MF-009 means a
   re-emit to keep the holder and outcome of the marker it re-emits, MF-009 should say so and [F05 §9.5] will carry them.

### 8.7 Round 3 follow-up (closure NC-11)

[pass1-closure] (round 3) §4.2 lists NC-11 against [F05 §9.11] and open point 14. The following were re-read in full
before the edit: [F05 §9.11] and open point 14; [F11 §13.1] and open point 42; [F13 §3] I-F5; [F18 §2.5]; [API §14.1]
and open points 25 and 48; [RULES/pack-classes] PX-011, PT-028, PM-025 and open point 8; and [owner-questions] OQ-F-3.
After the edit, every citation of PX-011, OQ-F-3 and the pack cursor in `docs/spec/` was re-read ([pass1-closure]
"Round 3 follow-up").

| Finding | Severity | Round | Disposition | Where / reason |
|---|---|---|---|---|
| NC-11 (closure §4.2, round 3) | major | 3, follow-up | fixed (P part; [F11 §13.1] and open point 42 are R-SPEC-R's, `pass1-dispositions-R.md` §7.7; [API] open point 48 is R-SPEC-F's text, aligned in the same follow-up) | [F05 §9.11]'s closing paragraph said "`pack T` for agent A appends one `feed` 2 record after emitting (PX-011)". That sentence decided OQ-F-3 for option (a), against I-F5 ([F13 §3], [F18 §2.5]), [API §14.1], [API] open point 48 and PX-011's round-3 row. The paragraph now states OQ-F-3's interim. A `feed` 2 record is the cursor of a pack of T for agent A, and cursor(A, T) of PT-028 is the `cursor_seq` of the row (session, A, 2, `#N` of T) of the session the pack runs in (unchanged). Which actor appends it (the `pack` verb or the layer that delivers the pack) is OQ-F-3's call. Until OQ-F-3 is answered no M0 command appends one ([API] open point 48, PX-011), so cursor(A, T) is absent, C8 is empty and `pack` stays a read verb that appends nothing (I-F5, [F18 §2.5]). Open point 14 no longer says that PX-011 "appends" the cursor. It names OQ-F-3 and the interim, and it notes that OQ-F-3's options (a) and (b) keep the record's bytes and fold while option (c) would withdraw `feed` 2. No field, value, fold rule or durability class changed, and §9.11's table is unchanged |
| A1-23 | major | 3, follow-up | fixed (residue) | As NC-11: [F05 §9.11] now states the interim that [API] open point 48 and PX-011 state. A1-23 stays "closed, owner" (OQ-F-3) |

Rejected: none. Left open: none. OQ-F-3 stays with the owner. §8.4 item 3 and §8.6 item 1 described PX-011 as having
`pack` itself append the cursor, which is its round-2 wording. This row supersedes them.
