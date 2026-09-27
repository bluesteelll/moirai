# A1 re-review, lens P: performance, RAM, Windows and crash safety

| Field | Value |
|---|---|
| Title | A1 re-review of [40] revision 2 and [50] revision 2, and the §6.2 resolutions of `docs/m0/PLAN.md`, through lens P |
| Status | review, phase 0; findings open until their owners dispose of them |
| Work package | WP-80a (PLAN §3.2 item 8), role R-REV-P |
| Lens | performance, private RSS, Windows 11 behaviour (NTFS, Defender), crash and power-loss safety; the lens of [20] |
| Reviewed | [40] rev 2 (all of §0–§9 and the Review log); [50] rev 2 §3, §5, §6.3–§6.4, §8.1–§8.3, §12; PLAN §2, §3.2, §3.3, §6.1, §6.2 |
| Checked against | [41] (all); [51] (all); [AR] §4.3 (record kinds), §4.4 (R5 sections row), §4.10 (namespace durability), §5e.5, §8.3 (RAM and flush rows); [60] §3.1, §4, §5.1–§5.4; [80] §1, §2.1–§2.4.4, §2.12, §3.1, §5.5; [90] §10.1–§10.2, §11.1–§11.3; [70 S8, S9]; [20 §1.3] |
| Precedence used | [40]/[50]/[80]/[90] for their own reservations, [AR] otherwise; conflicts are recorded where they occur |

## 0. Verdict

**Changes required, small in scope.** On this lens every blocker and major of [41] and [51] is resolved by the
revisions as written (table in §1). The re-review finds **no blocker**, **four majors** and **thirteen minors**:

- A1P-01 and A1P-02 are crash-safety holes in the `file mv`/`file rm` protocol that [40] rev 2 and [80 §2.3.2]'s frozen
  table of protocol points leave open: the cross-volume copy path has no namespace ordering (a power loss can lose the
  user's file), and intent recovery rolls forward without re-establishing the directory barrier.
- A1P-04 is a frozen runtime format item ([70 S8]: "the epoch semantics of `TreeReg`/`TREES` are frozen items") that cannot
  be laid out as written: an epoch's "scope digest" cannot decide which files it covered, and the epoch list has no bound.
- A1P-03 is a PLAN item (resolution R10): two M0 exit decisions (leader in or out; checkpoint thresholds) are taken on
  vehicles that can be optimistic, and nothing states how the decision stays valid for the product.

Each major has a one-paragraph fix that changes no approved architecture. **R1–R19 of PLAN §6.2 are all confirmed**;
R1, R2, R10 and R18 carry amendments or conditions (§3). Chapters WP-11 to WP-16 and WP-33, WP-61–64 may build on the
revisions once A1P-01, A1P-02 and A1P-04 are disposed of in chapters 16, 11 and 17 and in `os/`.

## 1. Resolution check of [41] and [51] (lens P)

"Resolved" means the revision's text closes the issue on this lens; a residual names the finding that carries it.

| Issue | P-lens verdict | Residual |
|---|---|---|
| [41] B1 derived uids | resolved; exact-byte derivation adds no hot-path cost (`fold_v1` only in `PATHIDX` ordering) | A1P-14 (cost of the dead-uid cross-head check) |
| [41] B2 no ancestry source; spawn fallbacks | resolved: in-process reader ([60] M4), read-path caps (1 uncached pair, 32 E6 commits, `fs` units), `GITFACTS` written only at settles, no spawn on any resolution path | the settle-path E6 window (≤ 2,000 commits, R-14) has a RAM row at M4 ([60 §5.4]) but no time row; an uncached window inside `SessionStart`'s 150 ms ends `unverified`, which is safe. Recommend an M4 time row; not an M0 item |
| [41] B3 copies re-bound | resolved; creation time comes from the settle's enumeration (`FileIdExtdDirectoryInfo`), no extra I/O | — |
| [41] B4 `PathPrefix` event | resolved (state field); a set add on the root node is commutative, so no hot CAS on that node | — |
| [41] M1, M2, M3, M4, M5, M6, M9, M12 | resolved; outside this lens beyond cost, which the revision states | — |
| [41] M7 directory moves slow | resolved: E3d, one `OpenFileById` per moved directory (0.17–0.40 ms [M]), all-missing pack gate ≤ 5 ms p50 | — |
| [41] M8 OneDrive | resolved: attribute gate from enumeration, no content open, no hydration; row 28 asserts it | — |
| [41] M10 atomic-save races | resolved (50 ms quiescence) | A1P-06 (the wait on hook and MCP paths) |
| [41] M11 stale E3 after edits | changed and acceptable: edit-evidence hook `auto` (spawn-free `mcp_tool`, 0.3–0.7 ms), off under command hooks (25–73 ms spawn) | — |
| [51] B1, B3, M1–M4, M6, M7, M10 | resolved; outside this lens beyond cost | A1P-11 (F17's stale size) |
| [51] B2 R4 purity | resolved: reads stat and read in the resolved tree, charged to `fs`, live, excluded from determinism | A1P-15 (two budgets for the same work) |
| [51] M5 cursors | resolved (live keyset cursors) | — |
| [51] M8 engine and RSS | resolved in mechanism: `RuntimeScan` (`brief_triage` ≤ 50 µs), one `mem` budget covering every visited set, `ValueJoin`, lower-bound pre-flight, resumable plans listed | A1P-08 (stale RSS figures, per-kind gate), A1P-13 (in-lock work cap) |
| [51] M9 `--ids` | resolved (byte page, stderr footer, exit 10) | — |

## 2. Findings

Severity: **blocker** freezes a wrong byte or rule or loses acknowledged data by design; **major** leaves a frozen item
unimplementable, a crash-safety hole in the protocol text, or an M0 decision on an invalid basis; **minor** is a
consistency, budget or wording fix that a chapter author can apply.

### A1P-01 — major — cross-volume `file mv` of a file has no namespace ordering and no `ProjectFs` operation

- **Where.** [40 §3.4] step 3, fourth bullet ("copy → flush → verify the destination `oid` → delete the source");
  [80 §2.3.2] (frozen table of protocol points: no row for this path); [80 §2.1] `os::project` surface (no create,
  write or copy operation); PLAN R1/R2 ("the complete `ProjectFs` trait from WP-30").
- **Problem.** Fault-model item (2) as amended ([80 §2.3.5]) lets any subset of unsynced metadata operations be lost, in
  any order. The copy path creates the destination and unlinks the source with no `durable-name` between them. A power
  loss after the unlink became durable and before the destination's directory entry did leaves neither name: the user's
  only copy of a project file is gone, while no commit records anything (the intent recovers to "neither present →
  `missing`"). This is the one place moirai deletes a user file after copying it. Separately, `ProjectFs` as listed in
  [80 §2.1] cannot perform a copy at all, so a "complete" trait frozen at M0 (R1, R2) would have to change in M6.
- **Fix.** Preferred: refuse a cross-volume file move exactly as a cross-volume directory move is refused (`EXDEV` or a
  different `VolumeKey` → exit 7, "move it with a raw mv; links re-bind by evidence"), and delete the copy path from
  [40 §3.4]. It is rare (a volume mounted inside a tree), and the lazy path still handles a raw move. Alternative, if the
  explicit verb must support it: add a protocol point to chapter 16 and [80 §2.3.2] — create the destination no-replace,
  write, `durable+meta`, `durable-name` of the destination's parent, verify the `oid` by re-reading, then `unlink` the
  source and `durable-name` its parent, all before the commit — add a `ProjectFs` operation for it, a recovery row
  ("both present, destination `oid` = intent `oid`" → finish the unlink, roll forward), and a WP-40 seeded bug ("source
  unlinked before the destination name is durable"). Owners: WP-16, WP-17, WP-30.

### A1P-02 — major — intent recovery rolls forward without re-establishing the namespace barrier

- **Where.** [40 §3.4] step 5 (recovery table), [40 §3.5] step 4; [80 §2.3.2]; [AR §4.10] "Namespace durability";
  [80 §2.1] `os::project` (lists `durable_rename`/`durable_unlink`, but no `sync_dir`, although [AR §4.10] says
  `ProjectFs` exposes `sync_dir`).
- **Problem.** A CLI killed after `MoveFileExW` (or `DeleteFileW`, or the trash rename) and before `durable-name` of one or
  both parents leaves the new names only in the OS cache. The next writer's open finds the intent's anchor Dead, sees
  "source absent, destination present", and writes the roll-forward commit and `FsIntentDone` durably. A later bugcheck
  or power loss can drop the unsynced rename while the commit survives: the graph records a move or removal the disk does
  not have, and "an intent is never lost" fails silently until a `doctor` run after the boot change. [AR §4.10]'s rule
  ("every … rename or delete a durable record depends on is followed by `durable-name`") is phrased for the process that
  issues the operation; the recovery table that chapter 16 will transcribe does not say it, and `MOVEFILE_WRITE_THROUGH`
  cannot be relied on for a same-volume rename (that is what the deferred item 17 was to measure).
- **Fix.** Every roll-forward row first calls `sync_dir` on both parents (for `--trash` the source directory and the
  trash directory; for a plain `rm` the parent), which is idempotent, then commits. Make it a protocol point in chapter 16
  and a row of [80 §2.3.2]'s table; `ProjectFs` exposes `sync_dir`; WP-40 seeds "roll-forward without the re-barrier";
  WP-32 enumerates a crash between the rename and each parent flush, then recovery, then loss of the unsynced namespace
  operations. Owners: WP-16, WP-17, WP-40.

### A1P-03 — major — M0 decisions on vehicles that can be optimistic (PLAN R10)

- **Where.** PLAN §6.2 R10; WP-40, WP-52 (measurements 1, 2, 12), WP-53d (measurement 10), WP-53e (measurement 14, T2);
  [60 §3.1] "Decisions fixed at M0 exit"; [60 §5.2] items 1, 2, 10, 14; [80 §2.4.3] "Costs".
- **Problem.** "Leader in or out of M1" rests on the 16-writer burst run on `moirai-toylog`, and the checkpoint threshold
  (open ≤ 3 ms at 1e6 with a full tail) on WP-53d's "log-tail record decoder". Neither vehicle is required to reproduce the
  product's costs:
  - the toy log's work under the writer byte (scan, re-validation, append) and its record sizes are its own; the product
    holds the byte for re-validation of a `TX` up to `tx.max-work-in-lock` and must still meet hold p99 ≤ 5 ms and
    writer-wait p99 ≤ 50 ms ([60 §5.4]);
  - a decoder alone measures half of tail replay: [20 §1.3] puts 0.5–1.5 ms of a 3–5 ms full-tail open in decode **and
    overlay inserts**, and [70 S8] shows the tail's content is dominated by lazy runtime records (`FileObs`, `AnchorRes`,
    `TreeReg`, heartbeats) whose share decides how often the threshold trips.

  If M1 then misses either gate, the leader (3–4 units) or a lower threshold returns after the freeze, and an init-fixed
  threshold cannot be lowered without a format event.
- **Fix.** (a) The toy log writes the product's `RecHdr`, chained groups and a commit-size distribution taken from the
  spec (typical 0.3–0.6 KB, a tail of large inline commits up to `store.commit.inline-max-bytes`); measurement 2 sweeps an
  injected in-lock CPU cost from 0 to the hold budget (p99 5 ms, max 20 ms), and the leader decision must hold at the
  budget, not at the toy's cost. (b) WP-53d applies every decoded record into an overlay built to chapters 09 and 11 (the
  sorted patch lists and id maps the product will use), with a tail mix that includes the expected lazy-record share at
  the quiet cap. (c) Chapter 17 states both as named-hole constraints ("threshold such that a full-tail open with the
  overlay probe, loaded, is ≤ 3 ms at 1e6"; "leader out only if last-ack p99 ≤ 50 ms at the maximum in-lock cost"), and
  WP-81a records that M1's gates re-check them. Owners: PLAN (R10 text), WP-40, WP-52, WP-53d, WP-16.

### A1P-04 — major — `TREES` settle epochs cannot decide coverage and are unbounded

- **Where.** [40 §2.6] `TREES` row (the settle epoch list `{scope digest, hlc}`, 24 B per epoch) and `FILEOBS.verified_at`
  ("max(row, the newest `TREES` epoch whose scope covered F and saw it unchanged)"); R-7 (`TreeReg`), R-8 (`TREES`),
  R-18; [AR §4.3] `TreeReg`; [70 S8] ("the epoch semantics of `TreeReg`/`TREES` are frozen items").
- **Problem.** A digest cannot be inverted, so "the epoch's scope covered F" is undecidable without recomputing the scope
  as of the epoch; a `SessionStart` scope ("the brief's items and the bound lane's `files_owned`") depends on graph state
  that has moved since. The list also has no retention rule: every settle on every tree (46 trees, every start and
  compaction) appends one, so `TREES` rows and the `TreeReg` share of the tail grow with use — [70 S8]'s tail problem in a
  slower form. Chapter 11 cannot give a byte layout for this as written.
- **Fix.** Freeze an epoch as `{tree key [16], scope_kind u8 (full-tree | lane-owned | brief), _ [3], scope_ref u32 (the
  lane's ref_id, else 0), scope digest [16], hlc u64}`. Coverage is decidable for `full-tree` (every file of the tree) and
  `lane-owned` (F matches the lane's `files_owned` globs whose digest equals the epoch's); a `brief` epoch never advances
  `verified_at`. This is conservative: a smaller `verified_at` only turns some "copy, never a candidate" outcomes of the
  copy rule into `identical copy` proposals, never into an automatic re-bind. Retain only the newest epoch per (tree,
  scope_kind, scope_ref); a checkpoint folds the rest away. Chapter 17 states the byte cost per settle. Owners: WP-13
  (chapter 11), WP-11 (`TreeReg` payload), WP-16 (chapter 17), WP-14b (copy rule wording).

### A1P-05 — minor — the two-pass `oid` reader is not safe against a concurrent in-place writer

- **Where.** [40 §2.5] "Streaming, bounded memory"; PLAN WP-62; chapter 20.
- **Problem.** Pass 1 fixes the normalised length that the `blob <len>\0` header commits to; pass 2 streams the bytes. An
  in-place writer (append, `Set-Content`, an editor that does not rename) between the passes yields a wrong `oid` and a
  sketch of different content, which settles and captures then write into versioned data (`oid`, anchor `blob`). A
  replace-by-rename writer is harmless only if both passes use one handle.
- **Fix.** Both passes use one handle; pass 2 checks that it hashed exactly the pass-1 normalised length and that the
  handle's size and last-write time are unchanged (`GetFileInformationByHandle` before and after); on a mismatch retry
  once, then the answer is `unverified`. Add a WP-62 test with a writer racing the two passes.

### A1P-06 — minor — settles on hook and MCP paths must never sleep or wait past their cap

- **Where.** [40 §4.2] (`SessionStart` settle, `links sync` through MCP `write`, `complete`'s settle), [40 §6.4]
  (`hooks.transport = auto` runs hooks as `mcp_tool` handlers on the session's server); [50 §5.10] (server work in ≤ 5 ms
  slices); [AR §2.2] (no timers); [60 §5.4] M10 row (read tool p99 ≤ 25 ms, max ≤ 100 ms under a burst with maintenance
  pending); PLAN WP-56 (measurement 19).
- **Problem.** A writing settle waits ≥ 50 ms for quiescence and then takes the writer byte. On the session's
  `current_thread` server (or a hand-written synchronous loop, if measurement 19 picks one) a literal 50 ms wait blocks
  every subagent of the session and breaks the M10 gate; a `SessionStart` settle that waits for the writer byte with
  `lock.writer-wait-ms` (2 s) cannot honour its 150 ms hard cap.
- **Fix.** State as a rule in chapter 16 and [40 §4.2]: on hook and server paths a settle never sleeps — re-binds whose
  quiescence window has not elapsed when the slice ends are left to the next settle (or re-checked on the next call); every
  lock wait on these paths is `acquire_within(min(lock.writer-wait-ms, remaining cap))`, and `Busy` drops the commit, not
  the evidence. Add "a pending settle and a 16-subagent burst" to measurement 19's decision inputs.

### A1P-07 — minor — `file mv`/`rm` cost and flush accounting omit the directory barrier and write-through

- **Where.** [40 §3.4] "Cost: two flushes (~4 ms) plus the rename"; [40 §7.2] `file mv` rows; [AR §8.3] and [60 §5.4]
  "flushes per verb … `file mv`/`rm` 2"; PLAN WP-55 (measurement 15), WP-50 (measurement protocol).
- **Problem.** Since [72 M8] and the owner decision of 2026-09-27 (PLAN §6.1 #3), a Windows `file mv` issues two log
  flushes, a `MoveFileExW` with `MOVEFILE_WRITE_THROUGH`, and two directory flushes; the gate "2 flushes" is ambiguous
  once the `Vfs` counter sees `sync_dir`, and the cost of write-through on a same-volume rename under Defender is
  unmeasured. Measurement 15 as planned renames without the protocol's flags.
- **Fix.** The `Vfs`/`ProjectFs` counters report log data flushes and `sync_dir` calls separately, and the gate reads
  "2 log flushes and 2 directory flushes"; measurement 15 measures the rename of one file and of a 1,000-file directory as
  the protocol performs it (`MOVEFILE_WRITE_THROUGH` plus both parents' `FlushFileBuffers`) on the project volume, idle
  and loaded; [40 §7.2]'s 8–15 ms is re-derived from it at WP-81a.

### A1P-08 — minor — [50]'s RSS composition text is stale, and the headroom rule ignores the lane allowance

- **Where.** [50 §5.10] ("The gate is ≤ 4 MB for a CLI or hook process at 1e4–1e6"), [50 §5.12] "RSS composition";
  [AR §8.3] RAM rows; [60 §5.4] ("≤ 4 MB at 1e4–1e6, + ≤ 1 MiB per extra ref read"); [40 §7.3].
- **Problem.** [50 §5.12] still quotes a 1e6 CLI baseline of 3–6 MB and "4 MB on `main` at 1e5 already exceeds the gate",
  and adds `mem` to a baseline that [AR §8.3] already defines to include it (est. 1.5–3.6 MB at 1e5, 1.6–4.1 MB at 1e6,
  `mem` ≤ 1 MiB included). `mem = min(1 MiB, gate − private bytes)` against a flat 4 MB takes the lane's 1 MiB allowance
  away from lane queries, so they hit the 256 KiB floor and E502 where `main` passes. [40 §7.3]'s "+ ≤ 0.5 MB" for the
  read buffer and line-hash array is 128 KiB + 512 KiB = 640 KiB.
- **Fix.** Chapter 17 (or `config.md`) defines the RSS gate per process kind as a named parameter — CLI or hook 4 MB,
  plus 1 MiB per extra ref read; MCP server 16 MB peak — and `mem`'s headroom uses the kind's gate; [50 §5.12]'s paragraph
  is replaced by [AR §8.3]'s figures (record the conflict: [AR] wins, it is not a [50] reservation); [40 §7.3] reads
  "≤ 0.65 MB".

### A1P-09 — minor — the `Meter` trait lacks the private-bytes reads the product needs (R2)

- **Where.** PLAN §2.2 (`Meter`: free space, available physical memory, child peak, heap high-water); [80 §2.1] `os::mem`
  (`private_peak()`, `private_now()`); [50 §5.10] (every query reads its private bytes at start).
- **Problem.** The query engine (M7) sets `mem` from `private_now()`; it is target-independent code and may reach the OS
  only through a seam. Without these reads in the M0 trait, M7 changes the seam, which R2 ("no interim form") excludes.
- **Fix.** `Meter` gains `private_now` and `private_peak` (process private bytes, the gated quantity) beside the heap
  high-water mark; WP-17 and WP-30 carry them.

### A1P-10 — minor — the GT20 (d) scope misses direct file I/O in product crates (R18)

- **Where.** PLAN §2.1 GT20 scopes and R18; [80 §2.1] ("Every store file and every project file goes through `Vfs` or
  `ProjectFs`"; `File::lock` and `std::fs::rename` "never used anywhere"); [80 §5.5] (a); PLAN WP-62 (`moirai-files` has no
  `moirai-vfs` dependency).
- **Problem.** The planned source scan bans `cfg(...)`, `std::os::*`, `File::lock` and `std::fs::rename`, but not
  `std::fs::File::open`, `OpenOptions`, `read_dir`, `metadata` or `remove_file` in product crates. FL-1's reader could then
  open project files itself, bypassing I-F11's cloud-placeholder gate, `O_NOFOLLOW`, `O_NOATIME`, the macOS dataless check
  and the enumeration-supplied attributes. The narrowing of "never used anywhere" to product crates is also unrecorded.
- **Fix.** The (d) scan also refuses those `std::fs` entry points in product crates other than `moirai-os`; WP-62's reader
  consumes a caller-supplied byte source (in M6, `ProjectFs::read_for_hash`) and opens nothing. Record the narrowing of
  `File::lock`/`std::fs::rename` to product crates as an amendment of [80 §2.1] in `authors.md` or chapter `os/`.

### A1P-11 — minor — F17's layout in [50 §8.1] is stale against [AR §4.4]

- **Where.** [50 §8.1] F17 (`#N → (ref_id u32, create_seq u32)`, 8 B per id); [50 §5.9] step 5 (already names `UIDX`);
  [AR §4.4] R5 sections row (`ALLOC` = `(uid, ref_id, create_seq)` and `UIDX`, widened by [72 M7], 24 B per node).
- **Problem.** The task's precedence gives [50] authority over its own reservations; applied literally it would freeze
  the 8-byte row and drop [72 M7]'s uid, which I1's uniqueness across unmerged branches needs. RAM: 24 MB instead of 8 MB
  at 1e6, cold pages only.
- **Fix.** Chapter 11 follows [AR §4.4]'s widened `ALLOC` plus `UIDX` and records the conflict (the F17 row predates the
  audit; [50 §5.9] already depends on `UIDX`). Owner: WP-13.

### A1P-12 — minor — R-6 `next_anchor` needs its log side

- **Where.** [40] R-6; [80 §2.4.3] phase 2a step 3 and the publish fold ("`next_id`, `next_anchor`, `fence`,
  `commit_seq` advance from the scanned groups exactly as recovery derives them").
- **Problem.** `HEAD` is not flushed on the commit path, so `next_anchor` must be re-derivable from records, as `next_id`
  is from `Create` ops. [40] allocates `aN` but does not say that a record carries it.
- **Fix.** Chapter 06 encodes each allocated `aN` (store-local, unhashed) in the op that creates the anchor, and chapter 05
  lists `next_anchor` among the counters recovery derives. Owners: WP-11, WP-12.

### A1P-13 — minor — the in-lock `TX` work cap can exceed the writer-byte hold gate

- **Where.** [50 §3.10] item 10 and §5.10 (`tx.max-work-in-lock` 5e5 units, "≈ 2–10 ms, est."); [60 §5.4] (writer-byte hold
  p99 ≤ 5 ms, max ≤ 20 ms, `TX` from M7); [80 §2.4.4].
- **Problem.** At its upper estimate the default cap alone is twice the p99 hold budget; under a burst of `TX` writers
  whose `MATCH` targets were touched, every holder may spend it.
- **Fix.** Chapter 17 lists `tx.max-work-in-lock` with the constraint "cap × calibrated ns per unit ≤ hold p99 budget
  minus the plain append cost", its value set at M7's work-unit calibration; A1P-03 (a) sweeps the same in-lock cost at M0.

### A1P-14 — minor — the dead-uid registration check has no stated cost

- **Where.** [40 §2.3] ("known to the store as removed or deleted on some branch head and live on none").
- **Problem.** Deciding "live on none" needs the uid's state at every live branch head, i.e. a view probe per ref (first
  read 3–20 ms and up to 1 MiB each, [60 §5.4]). Unstated, it can make `link` capture cost O(refs) overlay builds.
- **Fix.** State the probe order — the creating ref from `ALLOC` first, which settles the common case (the uid is live
  there) in one probe — and give `link`/`file add` a latency and RSS row in [40 §7.2] ("+ ≤ 1 MiB per extra ref read" as
  in [60 §5.4]). Owner: WP-14 (chapter 18).

### A1P-15 — minor — two budgets govern read-path file work

- **Where.** [40] R-13 (`files.read-budget-ms`, a time budget; the read cap of 20 ms) and [40 §4.2]; [50 §5.10] (`fs`
  units, count-based, default 400).
- **Problem.** A `pack` is a named query; which limit cuts its link work, and which one makes the cut deterministic for the
  same tree state, is not specified. A time cut under the 16-agent load turns links `unverified` non-deterministically.
- **Fix.** `config.md` states that `fs` units are the budget for every read (CLI verbs, packs, `q`, MCP), and that
  `files.read-budget-ms` is only the wall-clock safety net (reported like E503, never the primary cut). Owner: WP-18.

### A1P-16 — minor — `ANCHORRES` and `GITFACTS` have no retention rule

- **Where.** [40 §2.6] (`ANCHORRES` keyed (anchor uid, file `oid`, resolver version), "lazy (derivable)"; `GITFACTS`
  "dropped by `gc`"); [40 §7.3] (no `ANCHORRES` row).
- **Problem.** Every edit of a linked file adds rows for all its anchors under a new `oid`; rows for superseded content are
  never needed again, yet nothing drops them, so the section grows with edits (the census has ≈ 24k edits).
- **Fix.** The fold keeps an `ANCHORRES` row only while its `oid` is the current or last-observed content of a live file
  node (the `FPRINT` retention rule of [40 §2.5]); `GITFACTS` rows are kept for commits reachable from a bound tree's
  HEAD within the E6 window; both stated in chapter 11, with sizes in [40 §7.3]'s table. Owner: WP-13.

### A1P-17 — minor — a "complete" `ProjectFs` must not carry a method whose feature is excluded

- **Where.** [80 §2.1] `os::project` (`journal_since`); [40 §4.7] and [74 A13], [AR §11] #41 (E2 not built); [90 §11.1]
  ("no stub, no interim code"); PLAN R1, R2.
- **Problem.** Freezing `journal_since` in the M0 trait forces either a Windows USN implementation of an excluded feature
  or a stub, which the rules forbid.
- **Fix.** The M0 trait omits `journal_since`; the format keeps `JOURNALCUR`/`JournalCursor` reserved (R-7, R-8), so a
  later E2 adds the method additively. Owners: WP-17, WP-30.

## 3. PLAN §6.2 resolutions R1–R19

| # | Decision | Reason (lens P) |
|---|---|---|
| R1 | **confirm, with conditions** | Measurements 1, 2, 11, 12, 15 and 22 are only valid on the product's own OS functions; measuring stand-ins would have to be repeated at M1. Conditions: (1) WP-33 builds `fs`, `lock`, `map`, `env`, `proc`, `mem` first, because they gate WP-50 and WP-52 (chain 3 of PLAN §4); the `ProjectFs` half serves only WP-55 at M0 and follows; (2) `CountingAlloc` is installed as `#[global_allocator]` only by probe roots, never by `moirai`, so the product pays no atomic per allocation (its RSS gate is read through `private_now`); (3) the trait is complete in the sense of A1P-01, A1P-02 and A1P-17 |
| R2 | **confirm, with amendments** | One trait crate shared by the simulator and `moirai-os` lets the simulator exercise the real grant table (the in-process two-client case of X-F4), and generics keep the commit path free of dynamic dispatch (X4). Amendments: `Meter` gains `private_now`/`private_peak` (A1P-09); `ProjectFs` gains `sync_dir` and loses `journal_since` (A1P-02, A1P-17). Record that the grant table moves from `moirai-os` ([80 §2.1]'s "target-independent part") to `moirai-vfs` |
| R3 | **confirm** | Decode plus a test-only re-encoder gives byte-identical E3 without a second product codec; opaque compressed payloads are sound because frame checksums are over stored bytes. Chapters must define every checksum over on-disk bytes so the M0 oracle can verify it; digests over decompressed content are checked from M1, when the oracle gains its codec decoder ([90 §10.2]) |
| R4 | **confirm** | No lens-P content; comparing `--json v1` data keeps the model free of engine types |
| R5 | **confirm** | xxh3 is needed for `span_hash`. Note: the model's `fold_v1` from the UCD text should be built once per process, so the GT18 volumes (≥ 10⁶ histories nightly) do not re-parse the UCD files per case |
| R6 | **confirm** | Generated, committed tables: no Unicode crate, deterministic, small in the binary |
| R7 | **confirm** | Keeps C out of every checked graph. The fuzz caps (`-rss_limit_mb=256`, ≤ 2 targets during agent work) fit the laptop's ≈ 1.8 GB free; the libFuzzer-on-MSVC fallback correctly goes to the owner (WP-06) |
| R8 | **confirm** | Earlier enforcement than [80 §5.5] (M1) is safe and prevents OS code leaking into FL-1 from its first line; scope per R18 |
| R9 | **confirm** | Nothing at M0 needs the product binary; measurement 11's empty executable is `moirai-probes-bin`'s `empty` |
| R10 | **confirm, with conditions** | The toy log on the real Windows `Vfs` is the right vehicle for measurements 1, 2, 12 and T2, and permanent micro-benchmarks from the spec are the right vehicle for 3, 4, 5, 10 and 14. Conditions of A1P-03: the toy log's record sizes, chaining and injected in-lock cost span the product's budget; WP-53d applies records into a spec-shaped overlay with the expected lazy-record mix; chapter 17 states the decision constraints as hole constraints |
| R11 | **confirm** | One permanent hand-written loop serves the probes, LQ-Bench and measurement 19's baseline. Condition: measurement 19 exercises the loop with a pending settle and a subagent burst (A1P-06), since the runtime shape must allow non-blocking deferral |
| R12 | **confirm** | Incremental against from-scratch definitions is the only GT18 possible before the engine. Note: WP-94 reports the wall time per 10⁵ histories in its first nightly so that the ≥ 10⁶ nightly target is fitted to the agreed windows beside GT1 and fuzzing (WP-05's job list) |
| R13 | **confirm** | The renderer is where the byte limits (header ≤ 60/100 B, 8,000-B pages, ≤ 600-B errors) are first checked |
| R14 | **confirm** | No lens-P content |
| R15 | **confirm** | Moving WP-53/54/58/60/61b/63/65 to lane B balances the lanes without touching S2. Note: WP-53 and WP-54 are measurements; they run only in machine-wide agent-free windows, never beside lane A builds, and their "loaded" rows use the replayed fixture (WP-51), not live lane activity |
| R16 | **confirm** | No lens-P content |
| R17 | **confirm** | A Windows-only root is the only way to wire `moirai-os`, which exports nothing on the cross targets, into probes that stay generic and checked; the probes then measure the same code paths as the product |
| R18 | **confirm, with amendment** | Scopes are right for spawn and `cfg`. Amendment of A1P-10: add direct `std::fs` file I/O in product crates other than `moirai-os`, and record the narrowing of [80 §2.1]'s "never used anywhere" to product crates |
| R19 | **confirm** | No lens-P content; the listed licences are permissive (xxhash-rust is BSL-1.0) and the owner confirms them on day 1 |

## 4. Other checks made (no finding)

- **The owner decision on `MOVEFILE_WRITE_THROUGH`** (PLAN §6.1 #3) is written consistently in [40 §3.4, §3.5, §8.3.5],
  [80 §2.3.1–§2.3.2] and [AR §4.10]: write-through plus the directory flush on both parents, the flush never removable.
- **Group commit and hooks.** [40 §2.6]'s lazy evidence records follow [80 §2.4.3] phase 2b's exception for best-effort
  appends; an evidence hook drops its record when the writer byte is busy ([40 §6.4]), so hooks never wait on it.
- **Zero idle CPU.** No timer, watcher or thread outlives a command in [40] or [50]; the scoped worker threads of
  `--deep`/`--all` and the Unix lock-waiter thread are bounded by their command ([40 §4.8], [80 §2.1]).
- **Project-file handles.** I-F11 (full sharing, closed before the next file, never mapped or locked) and the attribute
  gate for cloud placeholders cover the Windows sharing-violation and hydration hazards of [41 M8, M10].
- **Read-path RAM in [40].** The two-pass reader, the capped line-hash array and ≤ 2 content readers bound a 16 MiB file
  to a fixed buffer (arithmetic corrected in A1P-08).

## 5. Holes

None. This file is a review; it introduces no value decided by a measurement.

## 6. Open points for the review

| # | Point | For |
|---|---|---|
| 1 | A1P-04's coverage rule changes what `verified_at` means for brief-scoped settles; lens S confirms that the conservative reading of the copy rule holds | R-REV-S |
| 2 | A1P-11 is a precedence conflict ([50] F17 against [AR §4.4] after [72 M7]); lens S confirms [AR] wins | R-REV-S |
| 3 | A1P-01's preferred fix removes an explicit-verb capability (cross-volume file moves); lens A confirms the agent-facing text ("move it with a raw mv") | R-REV-A |
| 4 | A1P-03 changes PLAN R10's text and WP-40/WP-53d acceptance; the plan author applies it before WP-40 and WP-53d start | PLAN |
