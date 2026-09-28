# 15 — The `Vfs` fault model and the durability classes

| Header | |
|---|---|
| Chapter | `docs/spec/format/15-fault-model.md`, cited as [F15 §x.y] |
| Status | draft, pass 1 pending |
| Work package | WP-16a (the fault-model part of WP-16, `docs/m0/PLAN.md` §3.2 item 1), author role R-SPEC-P |
| Freezes | The `Vfs` fault model, items (1)–(12), in the amended form; the four durability classes with `sync_dir` and `sync_group` and their per-OS calls; the semantics of `rename_noreplace`, `rename_replace` and `swap_dirs` |
| Sources (normative) | [60 §2.5]: the "`Vfs` fault model" row (items (1)–(12)), the audits' "`Vfs`/`ProjectFs`" row, the "Cross-platform" row, the protocol-decision row and its restatement paragraph (decisions (a), (c), (h)); [60 §3.1] scope items 3–5 and exit criteria; [60 §3.13] GT1, GT3, GT4, GT15; [60 §4.4] items 1 and 4; [60 §5.2] rows 12, 17, 18, 22; [80 §1] X2, X5, X8; [80 §2.1]; [80 §2.2.1] items 2, 5, 6, 9, 10; [80 §2.2.2] "Release after the holder dies" row; [80 §2.3.1]–[80 §2.3.5]; [80 §2.4.3]; [80 §2.4.4]; [80 §2.5] rules 1–6 and 8; [80 §2.6]; [80 §2.7.1]; [80 §2.11.1] `rename_noreplace` row; [80 §2.11.4] rules 6 and 7; [80 §2.12] "Store files' sharing" row; [80 §3.1] X-F2, X-F4, X-F5, X-F6; [80 §3.2] "The `Vfs` and `ProjectFs` simulators" row; [AR §2.8]; [AR §4.1] "Rules" paragraph; [AR §4.2]; [AR §4.10]; [AR §6.5]; [AR §8.2] items 12, 17, 18, 22 and the first "Correctness gates" bullet; [AR §10] risk 17; [AR §14] per-OS table; [40 §3.4], [40 §3.5], [40 §8.3.5] |
| Sources (evidence and history) | [61 B2]; [72 M1], [72 M2], [72 M3], [72 M8]; [81 B1], [81 m5], [81 m6]; [X17 §3.1], [X17 §3.4]–[X17 §3.8]; the approval checklist, item V7 (`docs/architecture-approval-ru/15-approval-checklist.md`); `docs/m0/PLAN.md` §3.2 (WP-16, WP-31, WP-32, WP-57), §3.3, §6.1 #3 and #14 |
| Consumers | WP-30 (`moirai-vfs`), WP-31 (`moirai-vfs-sim`), WP-32 (crash enumerator), WP-33 (`moirai-os`), WP-40 (toy log), the rest of WP-16 ([F16], [F17], [F13]), the FL-2 `ProjectFs` simulator (M6), M1's `Vfs` conformance suite |

**Reading guide.** §1 states scope and conventions. §2 defines the abstract machine: files, sectors, images, the
namespace, events, and the two kinds of failure. §3 states each fault-model item as rules, then what the in-memory `Vfs`
must do and what a crash gate may assume. §4 defines the durability classes and their per-OS calls. §5 defines the
namespace operations, including the three rename forms. §6 collects the crash-gate contract. §7 traces each item to its
sources. The chapter ends with its holes and its open points.

---

## 1. Scope and conventions

### 1.1 What this chapter freezes

1. **The fault model.** The fault model is the set of behaviours that a store's storage may exhibit. It is the
   union of the weakest behaviours of every file system the environment guard admits on Windows, Linux and macOS
   ([80 §1] X2, [80 §2.3.5]). The in-memory `Vfs` enforces it, and every crash gate runs on it ([60 §2.5]).
2. **The durability classes** `lazy`, `durable`, `durable+meta` and `durable-name`, and the group barrier
   `sync_group`: their guarantees, stated in terms of the model, and the calls that give them on each OS
   ([80 §2.3.1], X-F5).
3. **The namespace operations**: exclusive create, directory create, unlink, `rename_noreplace`, `rename_replace`
   and `swap_dirs`, together with the `ProjectFs` composites `durable_rename` and `durable_unlink`. For each, the
   chapter fixes what an observer sees before a crash, the possible states after a crash, and the per-OS call
   ([80 §2.3.2], [AR §14]).

### 1.2 What this chapter does not contain

| Topic | Home |
|---|---|
| Which protocol point uses which class; the protocol decisions (a)–(m); group commit; boot recovery; the `HEAD` barrier | [F16] (WP-16) |
| The record-kind durability tag, `RecHdr.flags` bit 0 `lazy` | [F05] (WP-11) |
| The `HEAD` slot layout | [F04] (WP-11) |
| `total_len` in sealed-file headers | [F09], [F10] (WP-13) |
| Signatures of `Vfs`, `ProjectFs` and `LockBytes`; the error enum; the environment guard; the mapping registry and the fault handler | [OS/fs], [OS/lock], [OS/map], [OS/env], [OS/proc], [OS/project] (WP-17) |
| The store parameters, including `store.log-extent-bytes` | [F17] (WP-16) |
| Invariants and their enforcement points | [F13] (WP-16) |

### 1.3 Normative words

- **must** and **never** state requirements.
- **may** states a behaviour the model allows. The storage (the adversary) may exhibit it; the product and the
  crash gates must tolerate it.
- The **adversary** is the component that chooses among allowed behaviours. In the in-memory `Vfs` it is a seeded
  chooser. In an enumeration run, the crash enumerator drives the same choices.
- **Widest reading.** Where a design sentence admits several readings, this chapter takes the one that allows the
  most behaviours, unless that reading would forbid a protocol the design requires. Each such choice is recorded in
  the open points.

### 1.4 Constants

The design fixes these constants. None of them is a store parameter, a configuration key or a hole.

| Name | Value | Meaning | Source |
|---|---|---|---|
| `SECTOR` | 4,096 bytes | The unit of loss after a crash. Sector k of a file covers the file offsets [4096·k, 4096·k + 4096). The grid is aligned to file offset 0 of every file. A final sector that is shorter than 4,096 bytes is still a sector. | [60 §2.5] item (1) |
| `SUBSECTOR` | 512 bytes | The unit of tearing. Sub-sector j of a sector covers [512·j, 512·j + 512) inside it, with j in 0..7. | [60 §2.5] item (1) |
| Torn sectors per file per crash | at most 1, not counting sectors in the `poisoned` or `dirty-over-poison` state (§2.2, FM-3.3) | — | [60 §2.5] item (1); FM-3.3 |

Consequence: the two 4 KiB `HEAD` slots, at file offsets 0 and 4,096 ([AR §4.2], [F04]), are two separate sectors.

### 1.5 Byte layouts

This chapter defines no on-disk structure, so it has no offset table. The one file that §5.6 requires, the swap-intent
record, gets its byte layout from WP-17 (see OP-11).

---

## 2. The abstract machine

The model is an abstract machine. The in-memory `Vfs` (WP-31) implements it. Every real `Vfs` must be **sound**
against it: every behaviour an admitted file system on a supported OS can show must also be a behaviour of the machine.
A counterexample is a specification defect. It reopens M0 ([80 §1] X8; [60] P4).

### 2.1 Actors

| Actor | Definition |
|---|---|
| **Client** | One user of the `Vfs`, for example one `LockFile` value. A process may run several clients ([80 §2.2.1] item 2). |
| **Process** | A set of clients that share one grant table and one mapping registry. A process dies as a unit. |
| **Kernel** | One boot of the operating system. It holds the page cache (represented here by the cache images), the lock state and the clocks. One store is touched by one kernel only ([80 §2.6]). |
| **External actor** | Any process that does not use moirai's `Vfs`: an antivirus scanner, an indexer, a backup tool, an editor, a user. |
| **Adversary** | The component that makes every choice this chapter allows (§1.3). |

### 2.2 Files, sectors and images

Each file f has two images:
- the **cache image**, with size `cs(f)` and bytes `C(f)`: what reads observe, apart from the exceptions in FM-3,
  FM-4, FM-9, FM-10 and FM-12;
- the **durable image**, with size `ds(f)` and bytes `D(f)`: what the device holds once every pending write-back has
  been lost.

The durable image is not directly observable. It matters only at a system crash (§2.5).

Every sector of f is in exactly one of the following states.

| State | Carried data | A read returns | After a system crash |
|---|---|---|---|
| `clean` | durable content d | d | d; the sector stays `clean` |
| `dirty` | baseline b, and the versions v₁ … vₘ written since b (vₘ is current) | vₘ | any one of {b, v₁, …, vₘ}, whole; or, for at most one sector per file, torn: each sub-sector independently from that set. The sector becomes `clean` with that content |
| `dirty-over-poison` | candidate set K, and the versions v₁ … vₘ written since the sector was poisoned | vₘ | each sub-sector independently from K ∪ {v₁, …, vₘ}, with no torn-sector bound. The sector becomes `poisoned` with candidate set K ∪ {v₁, …, vₘ} |
| `poisoned` | candidate set K | each sub-sector independently from K, chosen again at every read | each sub-sector independently from K, with no torn-sector bound. The sector stays `poisoned` with K |

Transitions:

| Event | `clean` | `dirty` | `dirty-over-poison` | `poisoned` |
|---|---|---|---|---|
| A write to the sector returns (any client, including an external actor) | → `dirty` (b = d, v₁ = new) | append a version | append a version | → `dirty-over-poison`. v₁ is the merged content: the written bytes, plus, for every unwritten byte, the bytes of one member of K chosen per sub-sector at the write |
| A successful flush of f covers the sector (§2.4) | no change | if there was no write after the flush began: → `clean` with the version current when the flush began. Otherwise → `dirty` with b := that version, and the later versions stay | same as `dirty` | no change (FM-3.4) |
| A failed flush of f (FM-3) | no change; a sector written during the flush is `dirty` by then and follows the next column | → `poisoned` with K = {b, v₁, …, vₘ} | → `poisoned` with K ∪ {v₁, …, vₘ} | no change |

A successful flush acts on each sector according to the state it had when the flush began. A write that returns after
the flush began is not covered: the write row applies to it. A successful `sync(Data)` acts only on sectors below
`ds(f)`. A successful `sync(DataAndMeta)` acts on sectors below the new `ds(f)` (FM-2.1, FM-2.2).

**Sizes.**
- `cs(f)` changes when a write extends f, when `create_extent` runs, and when an external actor truncates f.
- The **size history** H(f) is the set containing `ds(f)` and every value `cs(f)` has taken since the last successful
  `sync(DataAndMeta)` of f began.
- Bytes at offsets ≥ `ds(f)` get no durability guarantee from `sync(Data)` (FM-2.1).

**Identity.** Every file created through the model gets a fresh identity. A rename preserves it. The in-memory `Vfs`
never reuses an identity. Identity reuse on project trees is a matter for the `ProjectFs` simulator (M6, [80 §2.11]).

### 2.3 The namespace

- Each directory has a **current namespace**, which operations change immediately, and a **durable namespace**.
- **Namespace operations** are: exclusive create of a file, directory create, unlink of a file or of an empty
  directory, `rename_noreplace`, `rename_replace`, a native `swap_dirs`, and `link` (only in the `ProjectFs` Linux
  fallback, §5.7).
- An operation has one **parent** directory, or two for a rename or exchange across directories.
- Each operation takes effect in the current namespace at one instant, its **effect instant**. For an unlink under
  delete-pending, that instant can come later than the call (FM-8.3).
- The **issue order** is the order of effect instants.
- A **pending** operation is one that has taken effect but is not yet durable (FM-2.3).

### 2.4 Events, time and coverage

- Every `Vfs` call is an interval from its **start** to its **return**.
- A write W to f is **covered** by a flush S of f if and only if W returned before S started.
- `NtFlushBuffersFileEx`, `fdatasync` and `F_FULLFSYNC` each flush the whole file ([80 §2.4.2]). So a flush covers
  every covered write to f, whichever client or process made it.
- A flush **succeeds** when it returns success, and **fails** when it returns any error.
- Every read, write and flush of one file refers to the same cache image, whichever process or client makes it. There
  is one page cache per kernel.

### 2.5 The two kinds of failure

**Process death.** This covers `TerminateProcess`, a kill, an abort, a panic and the exit 7 of a mapping fault. When a
process dies:
- every client of the process stops, and no further event of theirs occurs;
- a **write** in progress is partially applied: each byte of its range holds its old or its new value, in any
  combination;
- a **namespace operation** in progress either took effect or did not (it is atomic);
- a **flush** in progress resolves by the death, to exactly one outcome: it succeeded (FM-2), it failed (FM-3; no live
  client receives the error), or it was not performed (its sectors stay as they were);
- every lock byte a client of the process held stays held for a delay, then is released (FM-8.1). An acquisition in
  progress either was not granted, or was granted and is then released with the process's other bytes;
- the process's mappings disappear;
- **nothing else changes**: the cache images, the durable images, the pending namespace operations and every other
  process are unaffected.

**System crash.** This covers an OS crash, a bugcheck, a power loss and a hard reset. When the system crashes:
1. Every process dies, and nothing it had in progress completes.
2. For every file f: the new size is any member of H(f). Every sector below the new size resolves by the "After a system
   crash" column of §2.2, subject to the one-torn-sector bound. Bytes at offsets ≥ the old `ds(f)` and below the new
   size may hold any value (OP-4). After that, `cs(f)` = `ds(f)` = the new size, and C(f) = D(f).
3. For every directory, the adversary chooses a subset of the pending operations to survive. The new namespace is the
   durable namespace with the survivors replayed in issue order. A survivor whose precondition fails at its replay point
   is lost as well: a create over an existing name, a rename or unlink of an absent name, a no-replace rename onto an
   existing name. Files that end up with no name are gone.
4. The lock state is empty: every byte is free. The new boot has a new boot identity. The monotonic and boot clocks
   restart. Every sharing violation and every delete-pending state has ended.
5. Injected persistent read errors (FM-12.2) persist.

### 2.6 Error classes

The model reports failures through the abstract classes below. [OS/fs] names the enum and maps each OS error onto it
(OP-18).

| Class | Raised by | Source |
|---|---|---|
| `DiskFull` | any write, flush, create or namespace operation (FM-5) | [60 §2.5] (5); [80 §2.3.5] (5) |
| `Io` | a failed flush (FM-3); a failed read (FM-12); a failed write (FM-5.5) | [80 §2.3.1] error policy; [80 §2.3.5] (12) |
| `SharingViolation` | open, unlink, rename or a swap step (FM-8.2) | [60 §2.5] (8); [AR §4.10] |
| `DeletePending` | open of a name whose unlink is pending (FM-8.3) | [60 §2.5] (8); [AR §4.1] |
| `AlreadyExists` | exclusive create or a no-replace rename onto an existing name | [80 §2.3.2] |
| `NotFound` | an operation on an absent name | — |
| `CrossVolume` | a rename or swap across volumes (`EXDEV`, `ERROR_NOT_SAME_DEVICE`) | [80 §2.11.4] rule 6 |
| `Unsupported` | a class or rename form that the location cannot provide (`ENOTSUP`, a volume without `VOL_CAP_INT_RENAME_EXCL`) | [80 §2.3.1]; [80 §2.3.2] |
| `AccessDenied` | a write to a sealed (read-only) file; a denied operation | [80 §2.5] rule 2 |

---

## 3. The fault-model items

Each item has the same parts:
- **Rules**, numbered FM-n.k. They are normative, in the widest reading.
- **In-memory `Vfs`**: what WP-31 must implement and inject. The enumerator (WP-32) cites these rules (S4: "Any
  enumerator change after a missed bug cites a fault-model item").
- **Crash gates**: what a gate may assume, and what it must not assume. §6 collects both lists.

The original wording of every item and each amendment are listed side by side in §7.1.

### 3.1 FM-1 — Unflushed sectors are lost in any subset, and one sector per file tears

**Rules.**
- **FM-1.1** After a system crash, each `dirty` sector of each file independently holds any one of the contents it has
  had since its durable point: its baseline, or any version written since, the latest included. "Reverting to their
  previous content" ([60 §2.5] (1)) is read as any earlier content, not only the baseline (OP-3). Background write-back
  can make an intermediate version durable.
- **FM-1.2** In each file, at most one `dirty` sector may instead be **torn**. Each of its eight sub-sectors
  independently holds one of the contents that FM-1.1 allows for that sector.
- **FM-1.3** A `clean` sector never changes at a crash.
- **FM-1.4** A lost sector reverts to content it actually held. In a log extent whose zero-fill was made durable by
  `create_extent` and the flushes that follow it, the baseline of a never-flushed sector is zeros ([60 §2.5] (1)).
  Bytes beyond the durable size are the one exception (FM-2.2).
- **FM-1.5** The surviving set is any subset. Survival follows neither the issue order nor the file order, so a later
  write may survive while an earlier one is lost, within one file or across files ([72 M3]).

**In-memory `Vfs`.**
- It keeps, for every `dirty` sector, its baseline and every version since, and never discards an intermediate
  version.
- At a crash, the adversary picks a version for every `dirty` sector and at most one torn sector per file.
- The enumeration tiers are those of §6.4.

**Crash gates.**
- A gate may assume FM-1.3 and FM-1.4, and that the two `HEAD` slots are distinct sectors (§1.4).
- A gate must not assume any order of persistence among unflushed writes, that the latest version survives, that
  sectors survive as a prefix, or that a sector survives whole.

### 3.2 FM-2 — What a flush makes durable; namespace durability

**Rules.**
- **FM-2.1** A successful `sync(Data)` of f (the `durable` class, §4) makes durable every sector of f that lies below
  `ds(f)`, except a sector that was `poisoned` when the flush began (FM-3.4). Each such sector becomes durable at least
  at the content it had when the flush began. `sync(Data)` changes neither `ds(f)` nor any name.
- **FM-2.2** A successful `sync(DataAndMeta)` of f (the `durable+meta` class) does what FM-2.1 does. It also sets
  `ds(f)` to `cs(f)` at the flush's start, together with the file's allocation, and resets H(f).
  - "Within its current size" is read as "below the durable size" (OP-4).
  - After a crash, bytes at offsets ≥ the old `ds(f)` that remain inside the file may hold any value, including stale
    blocks of other files (ext4 `data=writeback`, [X17 §3.6]).
- **FM-2.3** A namespace operation is **durable** once all three of the following hold:
  - (i) a `sync_dir` of each of its parents has succeeded, having started after the operation's effect instant;
  - (ii) the creation of each parent directory is itself durable;
  - (iii) every earlier pending operation on a name that this operation reads or writes is durable.

  Until then, a system crash may lose any subset of the pending operations, in any order (§2.5 step 3). The NTFS rule
  that metadata survives as a prefix of the issue order is dropped ([80 §2.3.5] (2)).
- **FM-2.4** A cross-directory rename or exchange has two parents. It is durable only after both have been synced
  ([80 §2.3.2] `file mv` row, [40 §3.5]; OP-5).
- **FM-2.5** Data and names are independent.
  - A file's data flush makes neither its name, its parent nor any other file durable.
  - A `sync_dir` makes no file's data durable.
  - No flag on a namespace call makes it durable in the model. This includes `MOVEFILE_WRITE_THROUGH`, which the
    Windows mapping passes (§5.8, OP-2).
- **FM-2.6** A successful `sync_group(members)` has, for each member, the effect of its class (FM-2.1, FM-2.2, FM-2.3).
  - A failed `sync_group` is a failed flush of every file member (FM-3). Every directory member's operations stay
    pending.
  - This holds even for members whose individual call succeeded before the final barrier: on macOS only the last
    `F_FULLFSYNC` gives durability ([80 §2.3.1]).
  - All members must lie on one volume ([80 §2.3.1], macOS row; OP-19).

**In-memory `Vfs`.**
- It keeps the durable namespace and, per directory, the ordered pending operations, each with its parents, its
  precondition and the parents that have synced it.
- A pending operation becomes durable only by FM-2.3.
- At a crash, it replays a subset of the pending operations as in §2.5 step 3.
- It credits nothing to a flag on a rename.

**Crash gates.**
- A gate may assume FM-2.1–FM-2.4 and FM-2.6 as guarantees.
- A gate must not assume any of the following:
  - that namespace operations persist in issue order;
  - that a data flush persists a name;
  - that `sync(Data)` persists a size;
  - that `MOVEFILE_WRITE_THROUGH` persists anything;
  - that one parent's sync makes a cross-directory rename durable.

### 3.3 FM-3 — A failed flush leaves its range indeterminate forever

This is the widest reading that `docs/m0/PLAN.md` §3.3 assigns to WP-16. It is recorded in OP-1.

**Rules.**
- **FM-3.1 Reach.** A failed flush of f poisons every sector of f that is `dirty` or `dirty-over-poison` at any
  instant between the flush's start and its return. That covers the flush's own range, "and any unflushed range of the
  file", including sectors written by other clients while the flush ran.
  - A failed flush here means `sync(Data)`, `sync(DataAndMeta)`, or `sync_group` with f as a member, returning any
    error: `Io`, `DiskFull`, `Unsupported` or another.
  - The candidate set K of a poisoned sector is its baseline, every version written since, and every content produced
    during the call.
- **FM-3.2 Reads.** Every read of a poisoned sector returns, for each sub-sector independently, the bytes of any member
  of K. The choice is made again at every read, by every client, so two consecutive reads by one client may differ.
  This covers:
  - the btrfs revert;
  - the macOS invalidation;
  - the eviction of clean-but-unwritten pages on ext4 and XFS, whether by memory pressure or by moirai's own
    `POSIX_FADV_DONTNEED` ([80 §2.3.4], [80 §2.12]);
  - Windows, whose behaviour is unknown ([X17 §3.5]).
- **FM-3.3 Crashes.** At a system crash, each sub-sector of a poisoned sector independently holds any member of K. The
  one-torn-sector bound of FM-1.2 does not apply: a failed write-back may have written any subset of any sector's
  sub-sectors. The sector stays poisoned across the crash, with the same K.
- **FM-3.4 Forever.** No later event ends the poisoning except a write to the sector (FM-3.5). A later successful flush
  of f, by any client of any process, proves nothing about a poisoned sector ([60 §2.5] (3)): the sector stays
  poisoned. Neither a process death nor a system crash ends the poisoning either.
- **FM-3.5 Ending by re-write.** A write to a poisoned sector moves it to `dirty-over-poison`.
  - The written bytes are definite.
  - Every unwritten sub-sector is fixed, at the write, to the bytes of one member of K.
  - Reads then return the current version.
  - A later successful flush makes the sector `clean` at the flushed content. This is what protocol decision (a) relies
    on: "re-write, then flush" ([80 §2.3.4], [F16]).
- **FM-3.6 Lazy records.** Lazy records published beyond `durable_lsn` may vanish with the poisoned sectors
  ([80 §2.3.5] (3)). A failed flush in any process can therefore lose any `lazy` data that no successful flush has made
  durable ([80 §2.3.1] `lazy` row).
- **FM-3.7 Scope.** Poisoning is per file and shared by every process, because there is one page cache. It covers file
  content only.
  - A failed `sync(DataAndMeta)` leaves `ds(f)` and H(f) unchanged. A later successful `sync(DataAndMeta)` makes the
    size durable.
  - A failed `sync_dir` leaves its operations pending. A later successful `sync_dir` of the same directory makes them
    durable by FM-2.3.

  Sizes and names are never poisoned (OP-6).
- **FM-3.8 Reporting.** The failed call returns its error to its caller if the caller is alive (§2.5). The model does
  not require any other client's later flush to fail. The adversary may fail any flush call, with any of the error
  classes FM-3.1 lists (FM-5.3, §3.13).
  - Every write-back failure is modelled as the failure of some flush call. A failure that no flush call reports, and
    that leaves reads returning the new bytes, is outside the model (§6.3 A-4; OP-9).
- **FM-3.9 Measurement 18.** Measurement 18 (flush failure on a VHDX taken offline mid-flush; WP-57, if the owner keeps
  the VHDX) records what Windows returns and what later reads see ([AR §8.2] item 18).
  - It is evidence for A-4 and can never narrow FM-3.1–FM-3.8.
  - If it shows a behaviour outside the model, that is a specification defect under [80 §1] X8.
  - This item has no hole. If the VHDX is deferred, no format changes (`docs/m0/PLAN.md` §6.1 #14).

**In-memory `Vfs`.**
- Flush-failure injection at every flush call, with the error class chosen.
- Tracking of K for every poisoned sector.
- A fresh draw per read and per sub-sector, and per sub-sector draws at a crash.
- Persistence of the poisoning across successful flushes and crashes.
- The `dirty-over-poison` transition on a re-write.
- The scenario the enumerator must reach: a failed flush followed by reverted, invalidated or evicted pages while at
  least three live writers keep appending and retrying ([80 §2.4.4]; §6.4).

**Crash gates.**
- A gate may assume FM-3.5: a re-written and successfully flushed sector is durable at its re-written content. A gate
  may also assume that FM-3 never changes a byte that no write has touched since the sector's last durable point, since
  every member of K agrees on such bytes.
- A gate must not assume any of the following:
  - that a successful flush after a failed one makes anything durable that was not re-written after the failure;
  - that two reads of one range agree;
  - that the one-torn-sector bound holds for poisoned sectors;
  - that poisoning ends at a crash;
  - that both `HEAD` slots stay valid after a failed `HEAD` flush (OP-1).

### 3.4 FM-4 — A read concurrent with a write

**Rules.**
- **FM-4.1** A read whose interval overlaps the interval of a write to an overlapping range returns, for each
  sub-sector independently, any content that sub-sector held during the read's interval. The write may come from any
  client of any process, or from an external actor. With several overlapping writes, any of their contents, or the
  content from before the first, may appear.
  - The design says "any mix of old and new sectors". The widest reading used here tears at sub-sector granularity,
    because concurrent page-cache copies can tear inside a sector (OP-7).
- **FM-4.2** A read that overlaps no write returns the cache content: the latest completed write, from any client of
  any process on this kernel. The exceptions are FM-3.2, FM-10 and FM-12.
- **FM-4.3** Reads are not ordered with one another. Two concurrent readers may see different mixes.

**In-memory `Vfs`.** It models reads and writes as intervals, and can pause inside a call (FM-6) so that the intervals
overlap. It draws the mix per sub-sector.

**Crash gates.**
- A gate may assume FM-4.2 (coherence).
- A gate must not assume that any read is atomic with respect to a concurrent write, in particular a `pread` of a
  `HEAD` slot or of a `LOCK` slot record ([61 B2]; [80 §3.1] X-F1, "re-reads it after probing").

### 3.5 FM-5 — Disk-full on any write, flush, creation or namespace operation

**Rules.**
- **FM-5.1** Any of the following may fail with `DiskFull`:
  - any write, including an overwrite of a written or zero-filled range, as on copy-on-write file systems;
  - any flush (`sync(Data)`, `sync(DataAndMeta)`, `sync_dir`, `sync_group`);
  - any file or directory creation, including each step of `create_extent`;
  - in the widest reading, any namespace operation: renames, a swap step, and unlink on copy-on-write file systems
    (OP-8).

  The design sources are [60 §2.5] (5), [80 §2.3.3] and [80 §2.3.5] (5).
- **FM-5.2** A failed write leaves each byte of its range at its old or its new value, in any combination. Every sector
  in which any byte changed becomes `dirty` with that content.
- **FM-5.3** A flush that fails with `DiskFull` is a failed flush (FM-3).
- **FM-5.4** A failed file creation leaves the name absent, or present as an empty file (OP-8). A failed namespace
  operation leaves the namespace unchanged.
- **FM-5.5** A write may also fail with `Io`, for example `EROFS` after an ext4 remount read-only or an XFS shutdown
  ([X17 §3.5]), with the effect of FM-5.2 (OP-8).
- **FM-5.6** Disk-full is neither persistent nor transient by rule. A later call may fail or succeed.

**In-memory `Vfs`.** An injection point at every write, flush, creation and namespace operation (WP-32: "Disk-full
injected at every write, flush and create"), with FM-5.2's partial application.

**Crash gates.**
- A gate may assume that every failure is reported to the caller: a write that returns success wrote all its bytes,
  and a create that returns success created the name.
- A gate must not assume any of the following:
  - that preallocation or zero-fill protects an overwrite;
  - that a failed write left its range unchanged;
  - that disk-full persists or clears.

### 3.6 FM-6 — Pauses of any length

**Rules.**
- **FM-6.1** Any client, any process or every process may pause for any duration. The pause may fall between any two
  events of a client, including between the start and the return of one call. A pause at the start or end of a call
  covers "between two `Vfs` calls" ([60 §2.5] (6)); a pause inside a call lets a call take any time to return.
- **FM-6.2** A paused process takes no action, and its lock bytes stay held. The kernel is not paused: the page cache
  and other processes run on, and the clocks advance by FM-7.
- **FM-6.3** A system suspend pauses every process. The boot clock advances across it. In the widest reading the
  monotonic clock may not (FM-7.2).

**In-memory `Vfs`.** A seeded scheduler that can pause any client at any event, for any duration, and simulate system
suspends. GT4 uses 1–120 s `NtSuspendProcess` pauses on real NTFS ([60 §3.13] GT4); the simulator has no upper bound.

**Crash gates.** A gate must not assume any bound on the time between two events of one client, for example:
- between acquiring a byte and using it;
- between a flush's return and the publish;
- between a reader's `HEAD` read and its segment mapping, which can span the 60 s GC grace ([61 B2]).

### 3.7 FM-7 — Clocks and the boot identity

**Rules.**
- **FM-7.1 Wall clock.** Between any two readings, by one client or by two, the wall clock may step backward or forward
  by any amount within the value domain of the wall-clock type ([OS/proc]).
- **FM-7.2 Monotonic clock.** Within one boot, readings are comparable across every client and process, and a later
  reading is never smaller. It restarts at a new boot. It may exclude time spent in system suspend (OP-14).
- **FM-7.3 Boot clock.** Within one boot, readings are comparable across every client and process, a later reading is
  never smaller, and it includes time spent in suspend. It restarts at a new boot. Lease deadlines use it
  ([80 §2.3.5] (7), [80 §2.7.1]).
- **FM-7.4 Boot identity.** The boot identity is 16 bytes. It is constant within one boot and differs from that of any
  other boot. It is invariant under wall-clock steps, suspend and hibernation (X-F2).
  - A process may be unable to read it. It then runs in Unknown-boot mode ([80 §2.7.1]).
  - The adversary fixes Known or Unknown for each process at its start. It may also make any single read return
    Unknown.
- **FM-7.5 No relation.** The model states no relation among the three clocks, and no bound on the rate of the wall
  clock.
- **FM-7.6 Injection.** Every clock reading and the boot identity reach the product through the `Vfs` seam. The
  in-memory `Vfs` derives them from the seed, which makes HLCs and commit ids reproducible ([60 §4.4] item 1).

**In-memory `Vfs`.** Seeded wall-clock steps in both directions at any point, a monotonic and a boot clock per
simulated boot, suspend events, and per-process Known or Unknown boot identity.

**Crash gates.**
- A gate may assume FM-7.2–FM-7.4.
- A gate must not assume any of the following:
  - that wall-clock order matches event order;
  - that the monotonic clock includes suspend;
  - that the boot identity is readable;
  - any relation among the clocks.

### 3.8 FM-8 — Lock release after death; sharing violations; delete-pending files

**Rules.**
- **FM-8.1 Release delay.** When a process dies, each lock byte its clients held stays held for a delay d, then is
  released. d is unbounded on every OS: a crash reporter or a debugger keeps a crashing process and its handles alive
  ([80 §2.2.2], [80 §2.3.5] (8), [81 m5]). The adversary draws d per byte from:
  - (a) the measured distribution for that byte kind — writer, flush or slot — HOLE(F15-lock-release), which is
    measurement 12;
  - (b) delays above every configured lock wait bound (`lock.writer-wait-ms`, `lock.flush-wait-ms`) and below the
    scenario horizon: the heavy tail beyond the 2 s bound;
  - (c) never within the scenario.

  During the delay, `probe` answers `Held` and acquisitions answer `Busy` or time out. A live holder's byte is released
  only by its own release call. After a system crash every byte is free.
- **FM-8.2 Sharing violations.** Any open, unlink or rename of a store or project file may fail with
  `SharingViolation` (Windows errors 5 and 32), because an external actor holds a handle without the share mode needed.
  The failure may repeat for any number of consecutive attempts, up to the whole scenario.
  - Unix has no sharing violations ([80 §2.11.4] rule 7), but the model is the union of the three OSes.
  - The design sources are [60 §2.5] (8), [AR §4.10] and [80 §2.12].
- **FM-8.3 Delete-pending.** After an unlink of a name whose file is still open by any process, a moirai mapping
  included, the name may stay occupied until the last handle closes. While it does:
  - an open of the name fails with `DeletePending`;
  - a create of, or a rename onto, the name fails with `AlreadyExists` or `AccessDenied`;
  - directory enumeration may still list the name.

  The unlink takes effect in the namespace when the last handle closes, and only a `sync_dir` that starts after that
  instant can make it durable (FM-2.3; OP-12). The design sources are [AR §4.1] rules and [80 §2.12].

**In-memory `Vfs`.**
- Per-byte release delays drawn from classes (a)–(c). Every nightly run must contain at least one death whose delay
  exceeds every configured wait bound, and at least one byte that is never released within its scenario.
- Sharing-violation injection on open, unlink and rename, with a seeded persistence.
- The delete-pending model of FM-8.3.
- Measurement 12's CDF is loaded as simulator data after WP-81a fills the hole (WP-31: "lock-release delay from
  measurement 12 plus a heavy tail").

**Crash gates.**
- A gate may assume that a byte is granted to at most one client at a time, and that a live holder's byte is released
  only by its release call.
- A gate must not assume any of the following:
  - release within any bound after a death, including the measured p99;
  - that `Held` means the holder is alive;
  - that a sharing violation clears after a given number of retries;
  - that an unlinked name is free at once.

### 3.9 FM-9 — A read through a mapping of a sealed file may end the process

**Rules.**
- **FM-9.1** A read through a mapping of a sealed file may end the reading process at that read. This happens when:
  - an external actor has truncated the file below the read's offset (FM-10); or
  - the adversary injects a media failure in that range.

  In the product, the fault handler prints one line and exits 7 ([80 §2.5] rule 6; [OS/map]). In the model it is a
  process death at that point (§2.5; [80 §2.3.5] (9)).
- **FM-9.2** After an external truncation, a mapped read at an offset at or beyond the new size may return zeros
  instead of ending the process. On Unix this happens inside the page that holds the new end of file; the model allows
  it at any such offset. A mapped read never returns an error value to its caller.
- **FM-9.3** A mapped read of a range that no external actor has modified returns the file's bytes. Sealed files are
  never written through the `Vfs` after `seal` ([80 §2.5] rules 1–3).

**In-memory `Vfs`.** `map_sealed` returns a view. Every read through the view checks for an external truncation below
its offset and for an injected media fault. On either, the process dies, with FM-8.1's release delays; the zeros of
FM-9.2 are the alternative outcome.

**Crash gates.**
- A gate may assume FM-9.3.
- A gate must not assume that a mapped read of a truncated file terminates rather than returning zeros. Readers must
  check lengths and checksums ([80 §2.5] rule 5).

### 3.10 FM-10 — Other processes may modify store files

**Rules.**
- **FM-10.1** An external actor with permission may, at any time:
  - truncate any store file to any length;
  - extend it;
  - overwrite any range of it with any bytes;
  - flush its own changes.

  Unix has no share modes ([80 §2.3.5] (10)). External writes are writes by a foreign client: FM-1, FM-3 and FM-4
  apply to them.
- **FM-10.2** Sealed files are read-only on disk: `FILE_ATTRIBUTE_READONLY` or `0444`, applied by `seal`
  ([80 §2.5] rule 2). Modifying one therefore needs deliberate action, which the model allows. The design requires
  injecting at least **external truncation of a sealed file** ([80 §2.3.5] (10)).
- **FM-10.3** An external actor may replace `LOCK` with another file under the same name. The identity check of
  [80 §2.2.1] item 9 must detect this.
- **FM-10.4** Deleting or renaming other store files is outside the model (OP-13).

**In-memory `Vfs`.** Injection of external truncation of a sealed file (required), of external rewrites and extensions
of any store file (optional, for detection tests), and of `LOCK` replacement.

**Crash gates.**
- Unless a scenario injects FM-10, a gate may assume that no store file is modified by anyone but moirai.
- Under external truncation of a sealed file, the gate asserts that the affected readers exit 7, that no acknowledged
  commit is lost, and that `doctor --fsck` names the file ([80 §2.5] rule 8).
- Under an external rewrite of any other store file, the gate asserts detection only: an error, exit 7 or a `repair`
  diagnosis, never a silently wrong result. It does not assert durability.

### 3.11 FM-11 — The group-commit crash surface

**Rules.**
- **FM-11.1** Several processes, and several clients of one process, may have appended groups that no flush has yet
  covered. The appender of a group may die before any flush ([80 §2.3.5] (11)).
- **FM-11.2** The flush holder may die before, during or after its flush, with or without an error. A flush in
  progress at its caller's death resolves by §2.5, to one of three outcomes: succeeded, failed (poisoning its sectors,
  with no live client told), or not performed.
- **FM-11.3** A failed flush may be followed by reverted, invalidated or evicted pages (FM-3.2) while other processes
  keep appending, scanning, re-writing and retrying.
- **FM-11.4** FM-11 adds no storage behaviour beyond §2.5, FM-3, FM-6 and FM-8. It names the concurrency surface the
  enumerator must cover ([80 §2.4.4]).

**In-memory `Vfs`.**
- Several clients per process through the grant table ([80 §2.2.1] item 2).
- Several processes, and death at every event, including inside a flush with each of its three outcomes.
- Seeded scheduling that lets other clients act between any two events of the flush holder.

**Crash gates.** A gate may assume nothing beyond FM-1–FM-10 and FM-12. [F16] states the protocol invariants I-G1–I-G6
that GT1 and GT3 check.

### 3.12 FM-12 — Read errors

**Rules.**
- **FM-12.1** Any `read_at` of any range of any file may fail with `Io`: `EIO`, a file-system checksum error on btrfs
  or ZFS, or a failing medium ([80 §2.3.5] (12)). A failed read returns no bytes the caller may use.
- **FM-12.2** A read error may be transient, so a later read of the range succeeds, or persistent, so every later read
  fails, across system crashes too.
- **FM-12.3** A read error never delivers wrong bytes as success. Silent corruption of durable sectors is outside the
  model (§6.3 A-5).
- **FM-12.4** Informative: the protocol treats an unreadable log range below `durable_lsn` as corruption (exit 7,
  `repair`). One at or above it ends a reader's view, but stops a writer (exit 7, nothing appended), because
  `durable_lsn` may understate the durable end after a crash and the range may hold acknowledged groups. That rule is
  [F16] P-92's (pass 1, S1-25).

**In-memory `Vfs`.** Injection of transient and persistent read errors per range, seeded. Mapped reads are covered by
FM-9.

**Crash gates.**
- A gate may assume FM-12.3.
- A gate must not assume that an unreadable range becomes readable, or that read errors occur only on one side of
  `durable_lsn`.

### 3.13 Behaviours from other frozen contracts that the in-memory `Vfs` also provides

These are not fault-model items. They are adversarial freedoms that other frozen contracts leave open, and the
simulator must exercise them.

| Behaviour | Source |
|---|---|
| `probe(byte)` may answer `Unknown` (a sandbox denial, another principal), never `Free` for a held byte | [80 §2.2.1] item 6 (X-F4) |
| Grants across processes come in any order. A waiter may time out (`Busy`) although the byte was free for a moment (a spurious timeout). Inside one process, a released byte is handed to the oldest in-process waiter | [80 §2.2.1] items 2, 10 |
| `acquire_within` returns `Granted` or `Busy`, never both. A grant that races the deadline is either returned or released | [80 §2.2.1] item 5 |
| Any non-lazy call may fail with `Unsupported`. For a file flush that is a failed flush (FM-3). For `sync_dir`, its operations stay pending (FM-3.7). Either way the product refuses the store with exit 7 | [80 §2.3.1] error policy |
| Files have identities (`identity(file)`), and `LOCK` may be replaced (FM-10.3) | [80 §2.2.1] item 9 |

**Protocol-violation detection** (proposed, OP-20). The in-memory `Vfs` reports each of the following as a harness
failure, not as storage behaviour:
- a re-entrant acquisition ([80 §2.2.1] item 3);
- a wait that breaks the lock order ([80 §2.2.3]);
- any write, flush, create or namespace call by a process after it received an error from a non-lazy class — the
  product aborts, and never retries a flush on the same handle ([80 §2.3.1]).

---

## 4. Durability classes

### 4.1 Guarantees in model terms

| Class | `Vfs` call | Guarantee (identical on every OS) |
|---|---|---|
| `lazy` | `write_at` only | Visible to every client of every process once the write returns (FM-4.2). Survives the death of any process. Becomes durable with the next successful flush of the file that covers it, made by any client (§2.4). May be lost at a system crash (FM-1) or through a failed flush in any process (FM-3.6). |
| `durable` | `sync(Data)` | FM-2.1: once the call returns success, every covered write below the durable size is on stable media, provided the drive honours FLUSH (§6.3 A-1). |
| `durable+meta` | `sync(DataAndMeta)` | FM-2.2: `durable`, plus the file's size and allocation. |
| `durable-name` | `sync_dir(dir)` | FM-2.3: each create, rename or unlink in `dir` whose effect instant came before the call started becomes durable once its other conditions hold. |
| `sync_group(members)` | `sync_group` | FM-2.6: when it returns success, every member is durable by its class. |

The record kinds carry one tag, `lazy` or `durable` (`RecHdr.flags` bit 0, [F05]). The protocol points carry the
classes above, and [F16] fixes which class each point uses ([80 §2.3.2]; X-F5).

### 4.2 Per-OS calls

These are normative, following [80 §2.3.1] and [AR §14]. Signatures are in [OS/fs].

| Class | Windows (built and gated in M0–M11) | Linux (port phase) | macOS (port phase) |
|---|---|---|---|
| `lazy` | `WriteFile` at an offset | `pwrite` | `pwrite` |
| `durable` | `NtFlushBuffersFileEx(FLUSH_FLAGS_FILE_DATA_SYNC_ONLY)` | `fdatasync` | `fcntl(F_FULLFSYNC)`. Never plain `fsync` (the data stays in the drive cache) and never `F_BARRIERFSYNC` (it orders but does not flush) |
| `durable+meta` | `FlushFileBuffers` | `fsync` | `fcntl(F_FULLFSYNC)` |
| `durable-name` (`sync_dir`) | `FlushFileBuffers` on a directory handle opened with `FILE_FLAG_BACKUP_SEMANTICS` and `GENERIC_WRITE`. A read-only directory handle fails with `ERROR_ACCESS_DENIED` ([X17 §3.4]) | `fsync` on `open(dir, O_RDONLY \| O_DIRECTORY \| O_CLOEXEC)`. On ext4 this forces a full journal commit | `fsync(dirfd)`, then a device barrier: `F_FULLFSYNC` on the dirfd, or folded into the group's last `F_FULLFSYNC` |
| `sync_group(members)` | each member flushed by its class | each member flushed by its class; each is a device flush | a plain `fsync` of each member, then **one** `F_FULLFSYNC`, last, on a member of the same volume. The man page guarantees that this persists everything `fsync`ed on that device before it |

Informative ([X17 §3.1], Rust std source): on Windows, `std::fs::File::sync_data` and `sync_all` both call
`FlushFileBuffers`. The `durable` class must therefore call `NtFlushBuffersFileEx` itself. On Apple targets both std
calls are `F_FULLFSYNC`. On Linux they are `fdatasync` and `fsync`.

### 4.3 Error policy

The design sources are [80 §2.3.1], [AR §2.8] and [AR §6.5].

- **Abort without acknowledgement.** Any error from a class other than `lazy` aborts the process without an
  acknowledgement. This includes `EIO`, `ENOSPC`, `EDQUOT`, `EROFS`, `ENOTSUP`, and the Windows equivalents, including
  `ERROR_DISK_FULL`. A flush is never retried on the same handle. The next flush holder repairs the log by re-writing
  the unflushed range ([F16], decision (a)).
- **No downgrade.** A location that cannot give a class refuses the store. This is checked:
  - at `init` and `restore`, through the probe of [80 §2.6];
  - at every open, through the classification;
  - at run time, by aborting with exit 7, "location cannot provide durable commits".

  An `ENOTSUP` from `F_FULLFSYNC` is never downgraded to `fsync`.

### 4.4 Mechanisms that are never used for durability

The design sources are [80 §2.3.1], [AR §2.8] and [AR §4.10].

| Mechanism | Why it is excluded |
|---|---|
| An `ordered` class (`F_BARRIERFSYNC`) | It exists on macOS only, and the protocol does not need it (X2) |
| Direct I/O (`O_DIRECT` + `RWF_DSYNC`) | It needs block-aligned groups, which would be a format change (X8) |
| `FILE_FLAG_WRITE_THROUGH` | It is not trusted on consumer drives ([AR §2.8]) |
| Plain `fsync` on macOS | Data stays in the drive cache |
| Any write-through flag on a rename, `MOVEFILE_WRITE_THROUGH` included | It may be passed (§5.8), but the model credits it with nothing (FM-2.5) |

---

## 5. Namespace operations

### 5.1 Common rules

- **NS-1** Every namespace operation is atomic for observers before a crash and is one operation for FM-2.3. The
  emulated `swap_dirs` (§5.6) and the `ProjectFs` Linux `link` + `unlink` fallback (§5.7) are the exceptions: each is a
  sequence of operations.
- **NS-2** A rename or exchange works within one volume. Across volumes it fails with `CrossVolume` and changes nothing.
- **NS-3** A rename preserves the file's identity (§2.2) and its content images.
- **NS-4** Every namespace operation may fail with `DiskFull` (FM-5), `SharingViolation` (FM-8.2) or `AccessDenied`.
  A failed operation leaves the namespace unchanged, except that a failed create may leave an empty file (FM-5.4).
- **NS-5** No operation of this section makes itself durable. Durability comes only from `sync_dir` of every parent
  (FM-2.3, FM-2.4). A protocol point that needs a durable name issues `durable-name` on every parent the operation
  touched ([80 §3.1] X-F5).

### 5.2 Exclusive create and directory create

- **Exclusive create** makes a new name for a new, empty file. It fails with `AlreadyExists` if the name exists. `LOCK`
  is created only this way, by `init` or `restore`: `O_CREAT | O_EXCL`, or `CREATE_NEW` ([80 §2.2.1] item 9). Which
  other protocol points use exclusive create is [F16]'s and [OS/fs]'s.
- **Directory create** is a namespace operation on the parent. Operations inside a new directory become durable only
  once its creation is durable (FM-2.3 (ii)).
- **`create_extent(path, size)`** is a composite: an exclusive create, then a size change to `size`, then zero content
  over [0, `size`). The per-file-system method ([80 §2.3.3]) does not change the model:
  - NTFS: write zeros, then `FlushFileBuffers`;
  - ext4 and XFS: `FALLOC_FL_WRITE_ZEROES` or zero writes;
  - btrfs and APFS: a sparse `ftruncate`.

  The model treats the zeros as written data, so FM-1, FM-3 and FM-5 apply to them. The extent's content is durable
  zeros and its name is durable only after the `sync(DataAndMeta)` and the `durable-name` that [F16] places before the
  first acknowledgement in the extent ([80 §2.3.2]).

### 5.3 Unlink

`unlink(path)` removes a name. It may also remove an empty directory.
- On Windows it first clears `FILE_ATTRIBUTE_READONLY` if it is set (sealed files), then calls `DeleteFileW`
  ([80 §2.5] rule 2).
- On Unix it needs only write permission on the directory.
- FM-8.3's delete-pending rule applies. The operation's effect instant is the removal of the name.
- Windows callers retry errors 5 and 32 a bounded number of times ([AR §4.10]). A violation that outlasts the retries
  fails the command.

### 5.4 `rename_noreplace(src, dst)`

- **Pre-crash semantics.**
  - If `dst` exists, the call fails with `AlreadyExists` and changes nothing.
  - Otherwise, in one step, `dst` names the file (or directory) that `src` named, and `src` no longer exists.
  - No observer ever sees `dst` replaced.
  - Of two concurrent no-replace renames onto one `dst`, at most one succeeds.
- **Crash semantics.** It is one operation with parents `parent(src)` and `parent(dst)`. It is durable by FM-2.3 and
  FM-2.4. Before that, it may be lost as a whole.
- **Directories.** A directory may be the source. Renaming it moves its subtree.
- **Unsupported locations.**
  - Store files: every file system the guard admits is expected to support the no-replace form ([80 §2.11.1],
    [80 §2.13]). If one returns `Unsupported`, the store is refused (exit 7). There is no fallback for store files.
  - Project files: see §5.7.

### 5.5 `rename_replace(src, dst)`

- **Pre-crash semantics.** In one step, `dst` names `src`'s file, and `src` no longer exists. If `dst` named a file
  before, an observer sees either the old file or the new one at `dst`, never an absent `dst`. The replaced file loses
  its name; its content remains for handles already open. This operation is for files only.
- **Crash semantics.** It is one operation whose parents are `parent(src)` and `parent(dst)` (one parent when they are
  the same directory), durable by FM-2.3 and FM-2.4. Before that, it may be lost as a whole: `dst` keeps the old file
  and `src` keeps its name.
- **Uses.** Replace-renames happen only where a point replaces a file:
  - the store `config` rewrite;
  - git's `.lock` protocol in image export (`packed-refs`, loose refs).

  The design sources are [80 §2.3.2] and [80 §3.2].
- **Never `std::fs::rename`.** It is never used on store or project files. On Unix it replaces silently, so it is
  banned ([80 §2.1]; GT20 (d)).

### 5.6 `swap_dirs(a, b)`

- **Postcondition, identical on every OS.** `a` and `b` are existing directories on one volume. Afterwards, `a` names
  the directory `b` named, and `b` names the directory `a` named. Its one use, `restore`, holds the writer and
  maintenance bytes of the store being swapped ([80 §2.3.2], [AR §4.10]).
- **Native form** (Linux `renameat2(RENAME_EXCHANGE)`; macOS `renamex_np(RENAME_SWAP)`).
  - One operation, atomic for observers and at a crash.
  - Durable after `sync_dir` of each parent.
  - Before that, it may be lost as a whole.
- **Emulated form** (Windows, and wherever the exchange returns `Unsupported`). This is a sequence guarded by an
  intent record in `parent(a)`. Each step is followed by `durable-name` on every parent it touched before the next step
  starts:
  1. Write the intent record, `durable+meta` on it, and `durable-name` on its directory.
  2. `rename_noreplace(a, t)`, where t is a temporary name in `parent(a)`.
  3. `rename_noreplace(b, a)`.
  4. `rename_noreplace(t, b)`.
  5. Unlink the intent record.
- **Crash states of the emulated form.** At most the last unsynced step is lost, so the state after a crash is the
  result of a prefix of the steps. `doctor` and `restore` complete or roll back from the intent record ([80 §2.3.2]).
- **Observers of the emulated form.** Between steps 2 and 3, `a` does not exist. A lock-free reader that discovers the
  store in that window finds no store. What it does then is [F16]'s (OP-11).
- **What is not fixed here.** The design fixes the intent record's existence and role, not its bytes or its name, nor
  the temporary name t. OP-11 assigns them. The record must identify `a`, `b`, t, the swap step reached and an
  integrity check.

### 5.7 `ProjectFs` composites and the Linux fallback

- **`durable_rename(src, dst)`** is `rename_noreplace(src, dst)`, then `sync_dir` of both parents. It returns success
  only if all three calls succeed. Otherwise the rename's durability is unknown, and the `FsIntent` recovery of
  [40 §3.4] decides. This is the `file mv` point ([80 §2.3.2], [AR §4.10]).
- **`durable_unlink(path)`** is `unlink`, then `sync_dir` of the parent. `--trash` is `rename_noreplace` into
  `<store>/trash/<intent>/`, then `sync_dir` of both parents ([40 §3.5]).
- **Linux fallback.** Where `renameat2(RENAME_NOREPLACE)` returns `EINVAL` on a project file system, a file is moved by
  `link(src, dst)` (which fails with `EEXIST` if `dst` exists, so the no-replace property holds), then `unlink(src)`.
  - These are two operations. The crash state "both names, one inode" is possible, and the `FsIntent` recovery rolls
    it forward.
  - A directory move is refused on such a file system.
  - The design sources are [80 §2.3.2] and [80 §2.11.1].
- **macOS.** A volume without `VOL_CAP_INT_RENAME_EXCL` refuses `file mv` with exit 7 ([80 §2.3.2]).

### 5.8 Per-OS calls

| Operation | Windows | Linux | macOS |
|---|---|---|---|
| `rename_noreplace` | `MoveFileExW(src, dst, MOVEFILE_WRITE_THROUGH)`: never `MOVEFILE_REPLACE_EXISTING`, never `MOVEFILE_COPY_ALLOWED` | `renameat2(…, RENAME_NOREPLACE)`; `ProjectFs` fallback §5.7 | `renamex_np(src, dst, RENAME_EXCL)` |
| `rename_replace` | `MoveFileExW(src, dst, MOVEFILE_REPLACE_EXISTING \| MOVEFILE_WRITE_THROUGH)` | `renameat` | `rename` |
| `swap_dirs` | emulated (§5.6), with each rename as above | `renameat2(…, RENAME_EXCHANGE)`; emulated on `EINVAL` | `renamex_np(a, b, RENAME_SWAP)`; emulated where unsupported |
| `unlink` | clear `FILE_ATTRIBUTE_READONLY`, then `DeleteFileW`; bounded retries on errors 5 and 32 | `unlinkat` | `unlinkat` |
| `sync_dir` | §4.2 | §4.2 | §4.2 |

**`MOVEFILE_WRITE_THROUGH`** resolves the `docs/m0/PLAN.md` §3.3 gap "without measurement 17" (OP-2).
- Every rename that the Windows `Vfs` or `ProjectFs` issues passes `MOVEFILE_WRITE_THROUGH`. It is added to, never
  used instead of, the `durable-name` on every parent that NS-5 requires.
- The model credits it with nothing (FM-2.5).
- It stays until the post-release rig calibration (measurement 17, deferred with the rig, [60 §5.2] row 17) shows it
  unnecessary. Dropping it then changes no byte and no rule of this chapter.
- The design sources are `docs/m0/PLAN.md` §6.1 #3, [60 §2.5] decision (h), [80 §2.3.1] (`durable-name` row) and
  [AR §4.10].

---

## 6. The crash-gate contract

The crash gates are:
- GT1 (crash-point enumeration, on the toy log at M0 and on the engine from M1);
- GT3 (multi-process simulation);
- GT4 (Windows kill loops on real NTFS);
- GT15 (the OS-crash loop, deferred to after the release);
- the `FsIntent` crash enumeration of [40 §8.3.5];
- M1's `Vfs` conformance suite.

A gate may rely on §6.1, must not rely on anything in §6.2, and runs under the assumptions of §6.3.

### 6.1 Guarantees a gate may assume

| # | Guarantee | Rules |
|---|---|---|
| G-1 | A successful `sync(Data)` makes durable every covered write below the durable size, except in sectors that were `poisoned` when it began | FM-2.1, FM-3.4 |
| G-2 | A successful `sync(DataAndMeta)` also makes the size and allocation durable | FM-2.2 |
| G-3 | A namespace operation becomes durable once every parent is synced after it, its parents exist durably, and its predecessors on the same names are durable | FM-2.3, FM-2.4 |
| G-4 | A `clean` sector never changes, at a crash or on a read. FM-3 never changes a byte that no write has touched since its durable point | FM-1.3, FM-3 |
| G-5 | At most one sector per file outside the `poisoned` and `dirty-over-poison` states is torn per crash, and a lost sector reverts to content it held (below the durable size) | FM-1.2, FM-1.4 |
| G-6 | A read that overlaps no write, of a range that is not poisoned, not externally modified and not in error, returns the latest completed write of any client of any process | FM-4.2 |
| G-7 | A re-written poisoned sector that a successful flush then covers is durable at its re-written content | FM-3.5 |
| G-8 | Every failure is reported to its caller: a write, create or flush that returns success did all it claims | FM-5, FM-3.8 |
| G-9 | A lock byte is granted to at most one client at a time, and a live holder's byte is released only by its release | FM-8.1 |
| G-10 | Within one boot the monotonic and boot clocks never decrease, and the boot identity is constant and distinct across boots | FM-7.2–FM-7.4 |
| G-11 | The death of a process changes no cache or durable image, apart from its partial in-flight write and in-flight flush (§2.5) | §2.5 |
| G-12 | A failed read never delivers wrong bytes, and a mapped read of an unmodified sealed file returns its bytes | FM-12.3, FM-9.3 |
| G-13 | Without an injected FM-10 event, no store file is modified by anyone but moirai | FM-10 |
| G-14 | Harness-side records (acknowledgement logs over a pipe or socket, the reference model's state) are outside the fault model and are never lost | [60 §4.4] item 4 |

### 6.2 What no gate may assume

| # | Not assumed | Rules |
|---|---|---|
| N-1 | Any persistence order among unflushed writes or among pending namespace operations | FM-1.5, FM-2.3 |
| N-2 | That the latest version of a dirty sector survives, or that sectors survive whole | FM-1.1, FM-1.2 |
| N-3 | That anything that was not re-written is durable after a failed flush, even after a later successful flush; that poisoning ends at a crash | FM-3.4, FM-3.3 |
| N-4 | That two reads of one range agree, or that any read is atomic against a concurrent write | FM-3.2, FM-4.1 |
| N-5 | That `sync(Data)` persists a size, or that a data flush persists a name | FM-2.1, FM-2.5 |
| N-6 | That `MOVEFILE_WRITE_THROUGH` or any write-through flag persists anything | FM-2.5, §4.4 |
| N-7 | That preallocation protects an overwrite from disk-full, or that a failed write left its range unchanged | FM-5 |
| N-8 | Any bound on a pause, or on the lock-release delay after a death | FM-6, FM-8.1 |
| N-9 | That wall-clock order is event order, that the monotonic clock counts suspend, or that the boot identity is readable | FM-7 |
| N-10 | That a sharing violation or a delete-pending name clears within a given number of attempts | FM-8.2, FM-8.3 |
| N-11 | That a mapped read of a truncated file terminates rather than returning zeros | FM-9.2 |
| N-12 | That at most one `HEAD` slot is invalid after a failed `HEAD` flush | FM-3.3, OP-1 |

### 6.3 Assumptions outside the model

| # | Assumption | Status |
|---|---|---|
| A-1 | The drive honours FLUSH | Assumed by design ([80 §2.3.1] `durable` row; [60 §3.13] GT15; [AR §10] risk 17). `doctor` warns about the Windows setting "turn off write-cache buffer flushing" ([80 §2.6]) |
| A-2 | The store lies on a file system that the environment guard admits: NTFS in M0–M11 ([80 §2.6]) | Enforced by the guard. On a refused file system the model does not hold |
| A-3 | One kernel touches the store | Enforced by the guard: cross-kernel sharing is refused ([80 §2.6]) |
| A-4 | Every write-back failure of a sector is reported by some flush call on that file, or leaves later reads returning the sector's durable content. A failure that no flush reports and that leaves reads returning the new bytes is outside the model | Documented for Linux (errseq) and macOS (invalidation) ([X17 §3.5]). **Unknown on Windows**: measurement 18 is the evidence, and if the VHDX is deferred it stays unverified ([AR §10] risk 17; OP-9) |
| A-5 | Durable sectors do not change silently. Media corruption surfaces as a read error (FM-12) or is caught by moirai's checksums | By design. Checksums detect a change; the model does not inject silent changes except through FM-10 |
| A-6 | The outcome of a dead process's in-flight flush resolves no later than its death (§2.5) | Model simplification (OP-10) |
| A-7 | Values drawn from the random source ([OS/README §4.6]) do not repeat, except where the owning chapter draws again on a repeat (a `tmp/` name that exists, a re-rolled epoch equal to the previous one, a random uid `UIDX` holds). A repeated store id, slot nonce or leader nonce is outside the model | By design: 8- and 16-byte draws from a cryptographically secure source. The in-memory `Vfs` repeats a value only when a test scripts it ([OS/README §4.6], "Simulator form") |

### 6.4 Enumeration obligations

These come from [60 §3.1] item 3, [60 §3.13] GT1 and `docs/m0/PLAN.md` WP-32, and apply per item. The numbers are
fixed by the design and are not holes.

| Dimension | Obligation | Items |
|---|---|---|
| Crash points | A system crash at every `Vfs` call boundary: every write, flush, publish, create, rename and unlink, including between the appends of one flushed group | all |
| Sector subsets | Every subset (each `dirty` sector at its baseline or at its newest version) while a file has ≤ 12 dirty sectors (4,096 states). At least 10⁴ random states beyond, sampling intermediate versions (FM-1.1) and poisoned sub-sector mixes (FM-3.3). The fast PR tier is per-file prefixes plus one torn sector | FM-1, FM-3 |
| Torn sector | One torn sector per file, with sub-sector mixes | FM-1.2 |
| `HEAD` slots | Both slots exhaustively at every barrier point: {old, new, torn} × {old, new, torn}, where "old" ranges over every version since the slot's durable point. The 9 states of [60 §3.13] are the minimum (OP-3) | FM-1, FM-3 |
| Cross-file | Bounded products across files | FM-1 |
| Namespace | Every subset of the pending namespace operations lost, replayed by §2.5 step 3, exhaustively while there are ≤ 12, and at least 10⁴ random subsets beyond | FM-2 |
| Disk-full | Injected at every write, flush, create and namespace operation | FM-5 |
| Failed flushes | "Flush error, more commits, crash" sequences. Several pending groups and flush holders, with failed flushes followed by per-read reverts, invalidations and evictions while at least three live writers append and retry idempotently | FM-3, FM-11 |
| Process death | At every event, including inside a flush with each of its three outcomes | §2.5, FM-11 |
| Reads | Read-error injection, transient and persistent. Mapping-fault deaths. External truncation of a sealed file | FM-12, FM-9, FM-10 |
| Locks and time | Lock-release delays by FM-8.1. Pauses, wall-clock steps and suspends (GT3) | FM-6–FM-8 |

**Determinism.** Every adversary choice comes from a seeded generator and is recorded in a replayable trace. The same
seed and the same sequence of calls, with their clients and schedule, give a byte-identical trace (WP-31 acceptance).
The random draws of `Entropy` ([OS/README §4.6]) come from the same seed, one stream per simulated process, like the
clock readings of FM-7.6.

### 6.5 Applicability to the `ProjectFs` simulator

The amended fault model applies in both simulators ([60 §2.5], audits' `Vfs`/`ProjectFs` row). The `ProjectFs`
simulator arrives with FL-2 (M6).

| Item | `Vfs` simulator (store files) | `ProjectFs` simulator (project trees) |
|---|---|---|
| FM-1 | yes | yes, for project files that other tools write: after a crash their content may revert, and R4 re-observes it |
| FM-2 | yes | yes: `file mv`, `file rm`, `--trash` ([40 §8.3.5]) |
| FM-3 | yes | only its namespace reading: a failed `sync_dir` of a project directory leaves its renames and unlinks pending (FM-3.7). `ProjectFs` flushes no file content: `file mv` never copies, and a cross-volume move is refused before its intent ([OS/project §6.4], [F16] P-83; FS-4, pass 1, A1-28) |
| FM-4 | yes | yes: reads concurrent with other tools' writes |
| FM-5 | yes | yes: renames, unlinks and trash moves (there are no copies, [OS/project §6.4]) |
| FM-6, FM-7 | yes | yes |
| FM-8 | yes | FM-8.2 and FM-8.3 yes. They are central to R4 ([40 §8.3.1] row 21). The lock delay applies through `LOCK` |
| FM-9 | yes | no: project files are never mapped |
| FM-10 | yes, as an injected fault | yes, as the normal case: other processes edit project files, and that is not a fault |
| FM-11 | yes | no: it concerns the log. The `FsIntent` groups are log groups |
| FM-12 | yes | yes: a read error makes a source `Unknown` or `unverified`, never absent ([80 §2.11.4] rule 9) |

---

## 7. Traceability

### 7.1 Each item: original text, amendment, and this chapter's reading

| Item | [60 §2.5] as issued | Amendment ([80 §2.3.5]) | Reading here |
|---|---|---|---|
| (1) | Any subset of unflushed 4 KiB sectors lost, reverting to previous content; one sector per file torn at 512 B | — | FM-1. Previous content means any earlier version (OP-3) |
| (2) | `sync(Data)` / `sync(DataAndMeta)`; metadata operations survive as a prefix of the issue order | Replaced: durable only after `sync_dir` of the parent; any subset lost, in any order; the prefix rule dropped | FM-2. Both parents for cross-directory operations; dependencies (OP-5); the durable size (OP-4) |
| (3) | A failed flush leaves its range indeterminate forever | Widened: old or new bytes, changing between reads, the whole unflushed range of the file, lazy records vanish | FM-3, widest reading (OP-1). File content only (OP-6) |
| (4) | Concurrent read and write: any mix of old and new sectors | — | FM-4, at sub-sector granularity (OP-7) |
| (5) | Allocating or extending writes may fail with disk-full | Widened: any write, any flush, any creation | FM-5, plus every namespace operation and `Io` write errors (OP-8) |
| (6) | Pause for any length between two calls | — | FM-6, including inside a call |
| (7) | Wall-clock steps; a monotonic clock never goes back | Extended: a boot clock, and an unreadable boot identity | FM-7 |
| (8) | Lock-release delay; sharing violations; delete-pending | Widened: unbounded on every OS; a heavy tail | FM-8. HOLE(F15-lock-release) |
| (9) | — | New: a mapped read may end the process | FM-9, with zeros beyond a truncated end as the alternative outcome (FM-9.2) |
| (10) | — | New: another process may truncate or rewrite store files | FM-10, plus `LOCK` replacement (from X-F4) |
| (11) | — | New: the group-commit crash surface | FM-11, with three outcomes for a flush at death |
| (12) | — | New: read errors | FM-12 |

**Precedence.**
- [80] amends [60 §2.5] for its own reservation (X-F5), so the amended text governs.
- [X17 §3.8.3] item 4 ("immediate on Unix") is superseded by [80 §2.3.5] (8) ("unbounded on every OS"), after [81] m5.
- Where [AR], [60] and [80] disagree about `MOVEFILE_WRITE_THROUGH` (OP-2), this chapter records the conflict and
  takes the reading that satisfies every source.
- For the emulated `swap_dirs` (OP-11), this chapter keeps the exchange postcondition that X2 requires (one semantics
  on every OS) and records the conflict with [80 §2.3.2]'s "two renames".

### 7.2 Coverage rows

| Item | Where |
|---|---|
| [60 §2.5] `Vfs` fault model, items (1)–(12) | [F15 §3.1]–[F15 §3.12] (one section per item) |
| [60 §2.5] audits' `Vfs`/`ProjectFs` row: the four durability classes with `sync_dir`, `sync_group` and their per-OS calls | [F15 §4] |
| [60 §2.5] audits' `Vfs`/`ProjectFs` row: `rename_noreplace`, `rename_replace`, `swap_dirs` | [F15 §5] |
| [60 §2.5] audits' `Vfs`/`ProjectFs` row: the amended fault model in both simulators | [F15 §3], [F15 §6.5] |
| [60 §2.5] decision (h), the Windows call mapping (`MOVEFILE_WRITE_THROUGH` besides the directory flush) | [F15 §5.8]. The protocol points are in [F16] |
| X-F5: the per-OS durability calls, no downgrade, no direct I/O, `lazy` lost after a failed flush, fault-model items (2), (3), (5), (7)–(12) | [F15 §3], [F15 §4], [F15 §5]. The record-kind tag is in [F05]; the protocol-point classes and the `HEAD` barrier are in [F16] |
| X-F2 (part): the boot clock, and the boot identity with Unknown-boot mode as a fault surface | [F15 §3.7] |
| X-F4 (part): release after death, the probe's `Unknown`, unfair grants, the bound of item 5 as adversary freedoms | [F15 §3.8], [F15 §3.13] |
| X-F6 (part): rule 6's fault-handler exit as fault-model item (9) | [F15 §3.9]. The policy is in [OS/map] |

---

## Holes

| id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| HOLE(F15-lock-release) | The distribution of the lock-release delay after `TerminateProcess` of a holder, per byte kind (writer, flush, slot), idle and loaded: p50, p99, max, and the empirical CDF the in-memory `Vfs` samples as class (a) of FM-8.1 | Measurement 12 (WP-52), filled by WP-81a | Prior evidence: ≤ 32 ms observed, p99 1–8 ms after `TerminateProcess` ([80 §2.2.2], [M, X18 §5]) | The simulator's delay law must stochastically dominate the loaded CDF for each byte kind, and classes (b) and (c) of FM-8.1 always stay in addition. A value never removes the unbounded tail |

No other value in this chapter depends on a measurement:
- Item (3) keeps its widest reading whatever measurement 18 shows (FM-3.9).
- Measurement 17 is deferred, and `MOVEFILE_WRITE_THROUGH` stays (§5.8).
- Measurement 22 decides whether Windows runs in Unknown-boot mode, and the model already contains both modes (FM-7.4).

## Open points for the review

| # | Point | Resolution taken here | What the review must confirm, and the consequences |
|---|---|---|---|
| OP-1 | **The widest reading of item (3)** (`docs/m0/PLAN.md` §3.3, gap assigned to WP-16) | FM-3.1–FM-3.8. The reach is every sector dirty during the failed call. Reads draw per sub-sector and per read. The one-torn-sector bound does not apply. Poisoning survives later successful flushes and system crashes, and only a re-write ends it | **For [F16]:** a failed `HEAD` flush (barrier or boot-change recovery) can leave **both** slots failing validation on some reads and after a crash. [80 §2.3.2]'s argument "at most one slot can be torn" covers FM-1 only. A slot the barrier did not re-write may also still be poisoned from an earlier failed flush. [F16] must say what a process does when no slot validates. Proposal: the barrier and boot-change recovery write **both** slots, a read-modify-write of the newest valid slot into each, before their `HEAD` flush, which ends any poisoning by FM-3.5; and a process that finds no valid slot exits 7 for `repair`, which recovers the epoch from the log's records and the chain seed XXH3-64(epoch). The log needs nothing new: decision (a)'s re-write under the writer byte, with the chain and the identity check, already covers FM-3 |
| OP-2 | **`MOVEFILE_WRITE_THROUGH` without measurement 17** (`docs/m0/PLAN.md` §3.3, WP-16) | Every Windows rename issued through `Vfs` or `ProjectFs` passes the flag, alongside the mandatory `durable-name`. The model credits it with nothing | **Conflict recorded.** [60 §2.5] (h), [AR §4.10] and `docs/m0/PLAN.md` §6.1 #3 name `file mv` only. The approval checklist item V7 names `file mv` and the export renames. [80 §2.3.1] says "on the rename" in the `durable-name` row. Taking every rename satisfies all of them. Cost: measurement 8 (WP-54) should time the loose-object renames with the flag. [F16] may narrow the flag to the points the owner named without affecting this chapter's model |
| OP-3 | "Reverting to their previous content" in item (1) | Any version the sector has had since its durable point (FM-1.1) | The exhaustive tier enumerates {baseline, newest}, and the random tier samples intermediate versions (§6.4). [60 §3.13]'s "9 `HEAD` states" is then a lower bound. The review confirms that the tiering is enough |
| OP-4 | "Within its current size" in item (2) | Below the durable size. After a crash, bytes beyond the old durable size may hold any value (FM-2.2) | Harmless to the protocol: log extents are pre-sized by `durable+meta`, and sealed files use `durable+meta`. [F16] must never rely on `sync(Data)` alone after extending a file |
| OP-5 | Namespace durability with two parents and with dependencies | FM-2.3 (i)–(iii), FM-2.4 | **For [F16]:** [80 §2.3.2]'s store-`config` row writes the new text to `<store>/tmp/` and then `rename_replace` onto `config` with one `durable-name`. Under FM-2.4 both `tmp/` and the store directory must be synced, or the temporary file must live in the store directory. The same holds for a bulk commit's `cs.NNNN` if its temporary name is in `tmp/`. [AR §4.1] also says "segments under construction use a temp name", while [80 §2.3.2] says sealed files are created under their final number; [F16] should reconcile this |
| OP-6 | Does item (3) apply to names and sizes? | No. Poisoning is per file content (FM-3.7). A failed `sync_dir` leaves its operations pending, and a later successful one makes them durable | If the review extends poisoning to namespace operations, the `FsIntent` recovery of [40 §3.4] must redo a rename it observed, not trust it, and the model gains a poisoned state for pending operations |
| OP-7 | Item (4) granularity ("any mix of old and new sectors") | Sub-sector (512 B) mixing (FM-4.1) | Concurrent page-cache copies can tear inside a sector. Every protocol read unit is checksummed, so this costs nothing |
| OP-8 | Item (5) scope | Any namespace operation may fail with `DiskFull` (unlink on copy-on-write file systems included). A failed create may leave an empty file. A failed write leaves any byte combination. `Io` write errors (`EROFS`) behave like disk-full (FM-5.4, FM-5.5) | The protocol response is the same (abort, no acknowledgement; decision (f)). [F16] must tolerate an empty file left under a name it tried to create, for example a sealed-file number or an extent number |
| OP-9 | Unreported write-back failures | Outside the model as assumption A-4. Every modelled write-back failure is the failure of a flush call | Windows' behaviour is unknown ([X17 §3.5]). Measurement 18 is the only planned evidence. If the owner defers the VHDX (`docs/m0/PLAN.md` §6.1 #14), A-4 stays unverified on Windows and should join [AR §10] risk 17's residual list |
| OP-10 | The outcome of a flush in progress at its caller's death | It resolves by the death to success, failure or not performed (§2.5; assumption A-6) | A real kernel may complete or fail the in-flight write-back later. The simplification is sound as long as the next flush holder's re-write re-dirties the range before its own flush. The review confirms this |
| OP-11 | `swap_dirs` emulation | The exchange postcondition, identical on every OS. The emulation is **three** renames guarded by an intent record (§5.6) | **Conflict recorded:** [80 §2.3.2] says "two renames". Two renames can install the new store and move the old one aside, but they cannot give the exchange postcondition of the native form. The intent record's bytes, its file name and the temporary name are not fixed by any design document. Owner: WP-17 ([OS/fs]) with WP-10 ([F02]), because the files sit beside the store. **For [F16]:** lock-free readers can find no store at `a` between steps 2 and 3. Proposal: a reader that finds no store while a swap-intent record exists in `parent(a)` retries discovery a bounded number of times, then exits 7 "store unavailable" |
| OP-12 | Delete-pending names (FM-8.3) | An unlink takes effect in the namespace only when the last handle closes. A `sync_dir` before that instant does not make it durable | **For [F16]:** `file rm` of a project file that an editor holds open, and GC of a mapped segment. Windows 11 NTFS may use POSIX delete semantics by default [I, unverified]; measurement 15 or 22 could record which. The model keeps the lingering form |
| OP-13 | FM-10's scope | Truncation, extension, rewrite, and `LOCK` replacement are in the model. Deletion or renaming of other store files by other processes is outside it | Consistent with [80 §2.3.5] (10) and [80 §2.2.1] item 9. `doctor` detects a missing store file. The review confirms |
| OP-14 | The monotonic clock and suspend | It may exclude suspend (FM-7.2) | **For [F16]:** decision (e) specifies lease TTLs, the HLC and the GC grace "against the monotonic clock". Wherever elapsed time across a suspend matters, [F16] must use the boot clock, as the lease deadlines already do ([80 §2.7.1]) |
| OP-15 | Poisoning across system crashes | It persists (FM-3.3), following the literal "forever" | Reads of an unacknowledged tail during recovery after a reboot may vary between reads. The re-write rule handles the log. `HEAD` is covered by OP-1 |
| OP-16 | Cross-references to the OS chapters | [OS/fs], [OS/lock], [OS/map], [OS/proc], [OS/env] and [OS/project] are assumed as WP-17's file names | These are provisional until WP-17 lands. The review fixes the citations |
| OP-17 | Boot identity readability per call | The adversary fixes Known or Unknown per process at its start, and may also make any single read Unknown (FM-7.4) | Harmless if [OS/proc] caches the value per process. It keeps the product correct if it does not |
| OP-18 | The error classes of §2.6 | Proposed as the abstract classes. [OS/fs] owns the enum | WP-17 and WP-30 adopt these names or map them one to one |
| OP-19 | The single-volume precondition of `sync_group` | All members on one volume (FM-2.6) | Implied by [80 §2.3.1]'s macOS row. Harmless on Windows and Linux |
| OP-20 | Protocol-violation detection in the in-memory `Vfs` (§3.13) | Proposed as a WP-31 obligation, beyond [60]'s text | It turns the error policy of [80 §2.3.1] and the lock-order rule into harness checks. The review confirms it, or moves it to the toy-log assertions (WP-40) |
