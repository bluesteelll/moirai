# OS layer: `LockBytes` (`os::lock`)

| Field | Value |
|---|---|
| Title | `os::lock` — lock bytes on `LOCK`, the contract items 1–10, the in-process grant table, the lock order, bounded waits, probes and the `LOCK` identity check |
| Status | draft, pass 1 pending |
| Work package | WP-17a (role R-SPEC-P), part 1 of WP-17 |
| Sources | [80 §2.1] (`os::lock` row, "Threads" rule); [80 §2.2.1] items 1–10; [80 §2.2.2] (per-OS mapping, macOS 14 floor, 64-bit only); [80 §2.2.3] (lock map and order); [80 §2.3.5] item (8); [80 §2.4.3] (which phases take which byte); [80 §2.7.2] (slots); [80 §3.1] X-F1 (lock-byte part), X-F4; [AR §2.2] T2 (G1 wait); [AR §4.1] `LOCK` row; [AR §6.1]; [AR §13] keys `lock.writer-wait-ms`, `lock.flush-wait-ms`; [60 §2.5] protocol decision (j); [90 §4.4] (slot taken lazily, 16-byte hashes); [81] M1, m5, m12; research reports [X18 §2–§6] as cited by [80]; `docs/spec/reviews/a1-P.md` A1P-06; PLAN WP-30 acceptance (grant-table property tests) |

---

## 1. Scope

This file specifies how moirai takes, waits for, releases and probes the lock bytes of the store's `LOCK` file, on every
OS, and the Rust API that store code uses for them. It freezes X-F4 (the lock contract, items 1–10, and the lock order)
and restates the lock-byte part of X-F1.

The `LOCK` file's **data** — `LockHdr` at offset 0, `WriterDiag` at 2048, `LeaderRec` at 3072, the 256 `SlotRec`s at 4096
— and the rules for writing and reading those records are [F03] (WP-11). Liveness semantics (`Alive`/`Dead`/`Unknown`)
are [F03] and [OS/proc §6]. This file provides the operations they rest on.

---

## 2. Lock bytes

Every lock byte lies **beyond the end of `LOCK`** (a 36 KiB file, never resized): no lock ever covers a byte that holds
data, so Windows' mandatory byte-range locking never blocks a read of `LOCK`'s records, a backup, an antivirus scan or a
`doctor` dump ([80 §2.2.1] item 1, X-F1). Every lock is one byte and exclusive.

| `LockByte` | Offset in `LOCK` | Hex | Role | Held by ([80 §2.2.3]) | Operations allowed | Rank |
|---|---|---|---|---|---|---|
| `Slot(i)`, i < 256 | 2^62 + 2^16 + i | `0x4000_0000_0001_0000` + i | liveness slot i | a session's MCP server (Codex: a thread's) from the moment it knows its identity until it exits, or a CLI for the life of one file intent ([90 §4.4], [80 §2.7.2]) | try; release; probe | 0 |
| `Leader` | 2^62 + 1 | `0x4000_0000_0000_0001` | leader | the optional leader, for its lifetime (only if built) | try; release; probe | 1 |
| `Maintenance` | 2^62 + 2 | `0x4000_0000_0000_0002` | maintenance | checkpoint, promotion, rollup, GC | try; release; probe (never waited for) | 2 |
| `Flush` | 2^62 + 4 | `0x4000_0000_0000_0004` | flush | the process that flushes the log and publishes `HEAD` for everyone; boot-change recovery | try; **bounded wait**; release; probe | 3 |
| `Writer` | 2^62 + 0 | `0x4000_0000_0000_0000` | writer (innermost) | the appender (scan, re-validate, append), the flush holder (scan and re-write the pending range; publish), every publisher | try; **bounded wait**; release; probe | 4 |
| `Quiet` | 2^62 + 3 | `0x4000_0000_0000_0003` | quiet-advisory ([AR §4.1]) | as [F03] and [F16] define | probe; try; release | — (never waited) |
| reserved | 2^62 + 5 … 2^62 + 63 | `0x…0005` – `0x…003F` | reserved | never locked by moirai | none; 2^62 + 63 is probed by `foreign_lock_check` (§11) | — |

```rust
pub const ROLE_BASE: u64 = 1 << 62;                 // 0x4000_0000_0000_0000
pub const SLOT_BASE: u64 = (1 << 62) + (1 << 16);   // 0x4000_0000_0001_0000
pub const N_SLOTS: u16 = 256;                       // equals LockHdr.n_slots of [F03]
pub const FOREIGN_CHECK_BYTE: u64 = ROLE_BASE + 63; // reserved; never locked by moirai

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SlotIndex(u8);                           // 0..=255; `SlotIndex::new(u16) -> Option<SlotIndex>`

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum LockByte { Writer, Leader, Maintenance, Quiet, Flush, Slot(SlotIndex) }

impl LockByte {
    /// Offset of the byte in `LOCK` (table above).
    pub const fn offset(self) -> u64;
    /// Rank in the lock order; `None` for `Quiet`, which is never waited for.
    pub const fn rank(self) -> Option<u8>;
    /// Every byte except a slot.
    pub const fn is_role(self) -> bool;
    /// `Writer` and `Flush` only: the bytes `acquire_within` accepts.
    pub const fn waitable(self) -> bool;
}
```

The offsets are frozen by X-F1; [F03] owns them and this file restates them. A byte outside this table is never locked
by moirai.

---

## 3. The contract (X-F4 items 1–10)

Identical on every OS. "Client" is one `Locks::Client` value ([80 §2.1]'s `LockFile`): the unit that holds grants.

1. **Lock bytes carry no data.** Every lock byte lies beyond the end of `LOCK` (§2); every lock is one byte and
   exclusive. *Enforced by:* `LockByte::offset`, the only source of offsets; the `LOCK` size check of [F03].
2. **Ownership is decided in user space first.** A process-global grant table (§5) records, per lock byte, the client that
   holds it and the in-process clients that wait for it.
   - A second in-process client asking for a byte the process holds gets `Busy` from `try_acquire`, or waits in-process
     for the holder's release from `acquire_within`, **before any kernel call**, identically on every OS. The kernel never
     sees two in-process acquisitions of one byte, so neither Windows' non-reentrant refusal nor the OFD merge (whose
     `F_UNLCK` would drop another client's grant) ever occurs ([81] M1).
   - The kernel lock is taken on one handle or OFD per role per process, opened lazily and reused across grants (a
     Windows open is costly); every slot grant has its own handle or OFD (§9.2).
   - Release always unlocks in the kernel; in-process waiters then compete through a fresh kernel acquisition like any
     other process.
   - Waiter joining: at most one kernel acquisition per byte per process is in flight (a try, or a wait); a grant a kernel
     wait obtains is handed to exactly one waiting client, the oldest in the in-process queue whose deadline has not
     passed; the others keep waiting on the table (§5.4 I-L2, I-L4). [80] states this for the Unix waiter thread; this
     file applies it on every OS (open point 3).
   - Two clients in one process therefore conflict identically on every OS; the simulator and tests run several clients
     in one process, and M1's `Vfs` conformance suite has an in-process two-client case.
   *Enforced by:* the `GrantTable` (§5), property-tested (§5.5).
3. **Non-reentrant.** A client that acquires a byte it already holds commits a programming error, asserted in user
   space: the call panics. *Enforced by:* `GrantTable::begin_try`/`begin_wait` (I-L3).
4. **Fixed lock order** (§6): slot < leader < maintenance < flush < writer. Neither OFD locks nor `LockFileEx` detect
   deadlocks. *Enforced by:* `GrantTable::begin_wait` (I-L5).
5. **`acquire_within(byte, T)`** returns `Granted` or `Busy`, never both. A grant that races the deadline is either
   returned or released, never leaked. *Enforced by:* §5.3 transitions T4–T6 (I-L6); §7.
6. **`probe(byte)`** returns `Held`, `Free` or `Unknown`. The kernel never reveals the holder (`l_pid` is −1 for OFD;
   Windows has no query); holder identity comes only from moirai's records in `LOCK` ([F03]). An `EPERM`, `EACCES` or
   `EBADF` from the probe — a sandbox rule, a read-only descriptor — and every other probe error is `Unknown`, never
   `Free` ([81] m12). *Enforced by:* §8.
7. **No spawn while holding a role byte.** Every handle and descriptor is `O_CLOEXEC` or non-inheritable. An inherited
   OFD would keep a lock alive in a child that outlives its parent. *Enforced by:* §9.2 open flags; `os::spawn` asserts
   `Locks::holds_any_role() == false` ([OS/README §5.2]).
8. **Never `flock` or `File::lock` a store file.** `doctor` warns when `LOCK` carries a foreign lock (§11). *Enforced
   by:* GT20 (d) in product crates ([OS/README §2.5]); §11.
9. **`LOCK` identity.** `LOCK` is created only by `init` or `restore` (`create_new`: `O_CREAT | O_EXCL`, `FILE_CREATE`)
   and never deleted by moirai. After opening it, a process compares the handle's identity with a fresh query of the
   path; a mismatch means the file was replaced: close and retry once, then refuse with exit 7 (the Beads #2933 class,
   [X18 §4]). *Enforced by:* §9.1.
10. **Fairness is not part of the contract** ([81] m5). Windows grants roughly in arrival order; Unix wakes every waiter at
    once. The only guarantees are item 5's bound and exit 7 naming the holder; a spurious timeout is an exit 7 the caller
    retries. Lock release after the holder's death may lag by an unbounded amount on every OS (fault-model item (8):
    crash reporters and debuggers keep a crashing process and its handles alive; ≤ 32 ms observed on Windows after
    `TerminateProcess` [M, X18 §5]).

---

## 4. The `Locks` API

```rust
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum LockMode {
    /// Readers, hooks and CLIs that only read `LOCK`'s records and probe bytes. No acquisition.
    Probe,
    /// Writers, maintenance, MCP servers, `init`/`restore` after creation, `file mv`/`file rm`.
    Acquire,
}

pub trait Locks: VfsTypes {
    /// One client instance ([80 §2.1] `LockFile`). Dropping it releases every grant it holds (kernel unlock) and
    /// leaves every queue it waits in.
    type Client: Send;

    /// Opens `LOCK` in the store root, checks its identity (§9.1) and registers a client in the process's grant table
    /// for that `LOCK` (§5.1). `LOCK` absent → `NoLockFile`; never creates it.
    fn lock_client(&self, store: &Self::Root, mode: LockMode) -> Result<Self::Client, LockError>;

    /// The client's data handle on `LOCK` (read-only in `Probe` mode), for the records of [F03].
    fn lock_data<'a>(&self, client: &'a Self::Client) -> &'a Self::File;

    /// Never waits. `Busy` without a kernel call if the byte is held or being acquired in this process.
    fn try_acquire(&self, client: &mut Self::Client, byte: LockByte) -> Result<Acquired, LockError>;

    /// Bounded blocking wait (G1): `Writer` or `Flush` only, lock order checked (§6). `within_ms = 0` is a try.
    fn acquire_within(&self, client: &mut Self::Client, byte: LockByte, within_ms: u32) -> Result<Acquired, LockError>;

    /// Always unlocks in the kernel. `grant` must belong to `client`.
    fn release(&self, client: &mut Self::Client, grant: Grant);

    fn probe(&self, client: &Self::Client, byte: LockByte) -> ProbeResult;
    fn holds(&self, client: &Self::Client, byte: LockByte) -> bool;

    /// Process-wide: does any client of any grant table in this process hold a role byte? (§3 item 7.)
    fn holds_any_role(&self) -> bool;

    /// `doctor` only: is a lock that moirai never takes present on `LOCK` (§11)?
    fn foreign_lock_check(&self, client: &Self::Client) -> ProbeResult;
}

#[must_use]
pub enum Acquired { Granted(Grant), Busy }

/// Proof of one held byte. Not `Clone`, not `Copy`; consumed by `release`.
#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub struct Grant { /* private: byte: LockByte, client: ClientId, table: TableId */ }
impl Grant { pub fn byte(&self) -> LockByte; }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ProbeResult { Held, Free, Unknown }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LockError {
    /// `LOCK` does not exist: the store is not initialised or is damaged. Exit 7.
    NoLockFile,
    /// §9.1 failed twice. Exit 7.
    IdentityMismatch,
    /// `LOCK` cannot be opened for writing (a sandbox, another principal): writers exit 7 with the texts of [90 §5.3].
    AccessDenied { os: OsCode },
    /// The kernel refuses byte-range locks here (`EINVAL`, `ENOTSUP`, `EOPNOTSUPP`; Windows `ERROR_NOT_SUPPORTED`,
    /// `ERROR_INVALID_FUNCTION`): the store is refused, exit 7 ([80 §2.2.2] "Unsupported").
    Unsupported { os: OsCode },
    /// Any other failure (including a waiter thread that could not be started). Exit 7.
    Io(VfsError),
}
```

Programming errors — reentrant acquisition (item 3), a wait on a byte that is not waitable, a wait that breaks the order
(item 4), an acquisition through a `Probe`-mode client, releasing a grant through another client — panic in every build.
A product process aborts on panic (exit 1, "internal").

Callers choose `within_ms`: `lock.writer-wait-ms` for the writer byte (default `HOLE(lock-writer-wait-ms)`, registered
as 2,000) and `lock.flush-wait-ms` for the flush byte (default `HOLE(lock-flush-wait-ms)`, registered as 2,000)
([AR §13], [CFG]); on hook and MCP-server paths `min(key, remaining cap)` (A1P-06). A `Busy` from the writer byte is exit 7
naming the holder from `WriterDiag` ([F03]) and its anchor's liveness; a `Busy` from the flush byte is exit 7 with outcome
`pending` ([80 §2.4.3] phase 2b step 2). Texts are [F19].

---

## 5. The in-process grant table

### 5.1 One table per `LOCK` per process

- The `GrantTable` type lives in `moirai-vfs` and is **pure**: no I/O, no clock, no thread, no blocking. Its methods
  take the current monotonic time where they need it and return *steps* that tell the caller which kernel operation to
  perform ([OS/README §2.1]).
- `moirai-os` keeps a process-global registry `FileIdentity of LOCK → (Mutex<GrantTable>, Condvar, kernel handles)`. The
  first `lock_client` for a `LOCK` creates the entry; every later client of the same `LOCK` in the process joins it,
  whichever root value it came through. The entry's role and probe handles are closed when its last client is dropped.
- `moirai-vfs-sim` keeps one registry **per simulated process**, so simulated processes stay independent while the
  clients inside one simulated process share a table. The simulator runs both wait modes of §5.2 (chosen per seed), so
  GT1 and GT3 cover the Windows and the Unix hand-off paths.

### 5.2 State

```rust
pub struct GrantTable { /* private */ }

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WaitMode {
    /// Windows: the kernel wait is an overlapped request driven by the thread of the waiting client ("driver").
    CallerDriven,
    /// Linux, macOS: the kernel wait runs in a waiter thread that owns its own OFD.
    WaiterThread,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ClientId(u64);   // never reused within a table
```

Per byte `b` the table keeps:

| Field | Values | Meaning |
|---|---|---|
| `holder` | `None` \| `Some(ClientId)` | the client of this process that holds `b` |
| `kernel` | `Idle` \| `Trying(ClientId)` \| `Held(Owner)` \| `Waiting(Driver)` | the kernel state of `b` for this process. `Owner` = `RoleHandle` \| `SlotHandle(ClientId)` \| `WaiterHandle`; `Driver` = `Caller(ClientId)` (mode `CallerDriven`) \| `Thread` (mode `WaiterThread`) |
| `queue` | ordered list of `(ClientId, seq u64, deadline_ns u64)` | in-process clients waiting for `b`, oldest first; `seq` is the table's arrival counter |

Per client the table keeps the set of bytes it holds. The table also keeps the arrival counter.

### 5.3 Transitions

The driver (the `Locks` implementation) calls these methods under the table's mutex and performs the returned step
outside it, then reports the result. `now` is `Clock::mono_ns()`.

| # | Method | Precondition (panic otherwise) | Effect and returned step |
|---|---|---|---|
| T0 | `register() → ClientId`; `unregister(c) → Vec<Step>` | — | `unregister` emits `KernelUnlock` for every byte `c` holds and removes `c` from every queue (I-L9) |
| T1 | `begin_try(c, b) → TryStep` | `c` does not hold `b` | if `holder ≠ None`, or `kernel ≠ Idle`, or `queue` is not empty: **`Busy`** (no kernel call). Otherwise `kernel := Trying(c)` and **`KernelTry { handle }`**, `handle` = `RoleHandle` for a role byte, `NewSlotHandle` for a slot |
| T2 | `end_try(c, b, r) → (Outcome, Option<Step>)` | `kernel = Trying(c)` | `r = Granted`: `holder := c`, `kernel := Held(owner)`, outcome **`Granted`**. `r = Busy` or error: `kernel := Idle`, outcome **`Busy`** or the error; then if `queue` is not empty, the step **`StartWait`** for the oldest queued client (T3's slow path) |
| T3 | `begin_wait(c, b, deadline) → WaitStep` | `b.waitable()`; `c` does not hold `b`; `c` holds no byte of rank ≥ `rank(b)` (§6); client in `Acquire` mode | if `holder = None`, `kernel = Idle`, `queue` empty: `kernel := Trying(c)`, step **`KernelTry`** (then T3b). Otherwise: enqueue `c` with the next `seq`, step **`WaitInTable`** (block on the condvar until notified or `deadline`) |
| T3b | `end_wait_try(c, b, r, deadline) → WaitStep` | `kernel = Trying(c)` | `r = Granted`: `holder := c`, `kernel := Held(RoleHandle)`, **`Granted`**. `r = Busy`: enqueue `c`; `CallerDriven`: `kernel := Waiting(Caller(c))`, **`DriveKernelWait`**; `WaiterThread`: `kernel := Waiting(Thread)`, **`StartWaiterThread`**. `r` = error: `kernel := Idle`, the error, and **`StartWait`** for the oldest queued client if any. A queued client told **`StartWait`** (by T2, T3b or T7) performs T3's `KernelTry` and continues with T3b, keeping its place and deadline |
| T4 | `kernel_granted(b, now) → GrantStep` | `kernel = Waiting(_)` | let `o` = the oldest queued client with `deadline > now`. If `o` exists: remove it, `holder := o`, `kernel := Held(RoleHandle` for `CallerDriven`, `WaiterHandle` for `WaiterThread)`, step **`HandTo(o)`** (notify `o`, which returns `Granted`). Otherwise **`ReleaseNow`**: unlock through the owning handle, `kernel := Idle` |
| T5 | `deadline_passed(c, b) → DeadlineStep` | `c` was waiting for `b` | if `holder = c` (the grant arrived first): **`AlreadyGranted`** (`c` returns `Granted`). Else remove `c` from `queue`; if `kernel = Waiting(Caller(c))`: **`CancelKernelWait`** (the driver cancels, settles, reports T4 or T6, then returns `Busy`); else **`Busy`**. In mode `WaiterThread` an emptied queue leaves the waiter *abandoned*: it keeps waiting, and T4 then releases the byte at once |
| T6 | `kernel_cancelled(b) → CancelStep` | `kernel = Waiting(Caller(_))` | `kernel := Idle`; if `queue` is not empty, **`NewDriver(o)`** for the oldest queued client `o` (it issues a fresh kernel wait: `kernel := Waiting(Caller(o))`); else **`Idle`** |
| T7 | `release(c, b) → ReleaseStep` | `holder = c` | `holder := None`; step **`KernelUnlock { owner }`**, after which `kernel := Idle`; then, if `queue` is not empty, **`StartWait`** for the oldest queued client, which competes through a fresh kernel acquisition (T3's `KernelTry`) like any other process |
| T8 | `probe_step(b) → ProbeStep` | — | if `holder ≠ None`: **`Held`** (no kernel call). Else **`KernelProbe`** (§8) |
| T9 | `holds(c, b)`, `holds_any_role()` | — | queries |

### 5.4 Invariants

The property tests of WP-30 check each on random interleavings of T0–T9 with random deadlines, kernel answers and both
wait modes.

| Id | Invariant |
|---|---|
| I-L1 | At most one client holds a byte (`holder` is a single value), and `holder ≠ None` ⇔ `kernel = Held(_)`. |
| I-L2 | Single flight: at most one kernel acquisition (`Trying` or `Waiting`) per byte per table, and none while the byte is held. |
| I-L3 | Non-reentrancy: T1 and T3 by a client that holds the byte panic. |
| I-L4 | Oldest first: a kernel grant obtained while clients queue goes to the queued client with the smallest `seq` whose deadline has not passed. |
| I-L5 | Order: T3 requires a waitable byte and that the client holds no byte of equal or higher rank; violations panic. |
| I-L6 | Exactly one outcome per request: every T1 or T3 ends, for its client, in exactly one of `Granted`, `Busy` or an error; after every step no byte is `Held` without a holder (no leaked grant). |
| I-L7 | Release unlocks: T7 always emits `KernelUnlock`; a byte never passes from one holder to another without a kernel unlock and a fresh kernel acquisition. |
| I-L8 | Probe short-circuit: T8 on a byte held in this table returns `Held` without a kernel call. |
| I-L9 | No leak on drop: T0's `unregister` releases every held byte and empties the client's queue entries. |
| I-L10 | Waitable set: T3 on `Leader`, `Maintenance`, `Quiet` or a slot panics. |

### 5.5 Required property tests (WP-30 acceptance)

1. Non-reentrancy (I-L3) — a client's second acquisition of a held byte panics, on every byte kind.
2. Oldest-first grant (I-L4) — with n queued clients and random deadlines, the grant goes to the oldest unexpired one.
3. Order (I-L5, I-L10) — every forbidden wait panics; every allowed one proceeds: for example `Flush` then `Writer`,
   `Maintenance` then `Flush`, a slot then `Writer`; never `Writer` then `Flush`.
4. Single flight (I-L2) and exclusivity (I-L1) over random interleavings in both wait modes.
5. Exactly one outcome (I-L6) under grants racing deadlines (T4 against T5), including the `CancelKernelWait` path.

---

## 6. Lock order and waits

**Order:** `Slot` (0) < `Leader` (1) < `Maintenance` (2) < `Flush` (3) < `Writer` (4) ([80 §2.2.3], X-F4).

1. **Only role bytes are ever waited for, and only `Writer` and `Flush` in practice**: `Maintenance` is try-only
   ("maintenance never waits for it"), `Leader` is try-only, slots are try-only, `Quiet` is probed or tried.
2. **A client may wait on a byte only if it holds no byte of equal or higher rank.** Waits therefore go only upward: a
   client holding `Flush` may wait for `Writer`; a client holding `Writer` waits for nothing.
3. **Tries and probes are allowed in every state** (they never block), except the reentrant try of item 3 of §3.
4. **The writer byte is innermost.** Nobody holding it waits for any lock, flushes, sleeps for a sharing-violation retry
   ([OS/fs §6.3]) or spawns: the appender releases it before it waits for the flush byte; the flush holder releases it
   before it flushes ([80 §2.2.3], §2.4.3).
5. **One client per store per product process.** Product code opens one `Locks::Client` per store per process (the CLI,
   the MCP server, the `gc` child). The order is checked per client; a thread never blocks in one client's
   `acquire_within` while another client it drives holds a byte (tests that run several clients give each its own
   thread or simulated process).

The acquisition sequences of the frozen protocol ([80 §2.4.3], [F16]) all respect the order:

| Sequence | Bytes, in order |
|---|---|
| Phase 2a (append) | wait `Writer` → release |
| Phase 2b (flush and publish) | wait `Flush` → wait `Writer` → release `Writer` → flush → wait `Writer` → release `Writer` → release `Flush` |
| Boot-change recovery | wait `Flush` → wait `Writer` → release `Writer` → flush → wait `Writer` → release `Writer` → `durable+meta` on `HEAD` → release `Flush` |
| Maintenance (checkpoint, promotion, rollup, GC) | try `Maintenance` → its durable records by phases 2a/2b → barrier: wait `Writer` for a no-op publish, release, flush `HEAD` outside it → release `Maintenance` |
| `restore` swap | try `Maintenance` → wait `Writer` → swap → release both |
| Session server | try `Slot(i)` (the next slot on `Busy`), lazily at the first call carrying its identity ([90 §4.4]) → held to exit |
| `file mv`, `file rm` | try `Slot(i)` for the intent → phases 2a/2b for `FsIntent` and the commit → release the slot |

---

## 7. Bounded waits per OS

### 7.1 Windows (`CallerDriven`)

Every kernel operation on a role byte uses the table's role handle, opened with `FILE_FLAG_OVERLAPPED` semantics (§9.2)
and carrying one manual-reset event created with it. `low(b)` and `high(b)` are the low and high 32 bits of the offset
(`high(2^62) = 0x4000_0000`).

| Step | Calls |
|---|---|
| `KernelTry` | `LockFileEx(h, LOCKFILE_EXCLUSIVE_LOCK \| LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &ov)` with `ov.Offset = low(b)`, `ov.OffsetHigh = high(b)`. `TRUE` → granted. `FALSE` with 33 `ERROR_LOCK_VIOLATION` → busy. `FALSE` with 997 `ERROR_IO_PENDING` → `GetOverlappedResult(h, &ov, &n, TRUE)`: `TRUE` → granted, error 33 → busy [I; WP-33 test]. Any other error → `LockError` (§4) |
| `DriveKernelWait` (the driver's thread) | `ResetEvent`; `LockFileEx(h, LOCKFILE_EXCLUSIVE_LOCK, 0, 1, 0, &ov)` with `ov.hEvent`. `TRUE` → report T4. `FALSE` with 997 → `WaitForSingleObject(event, remaining ms of the driver's deadline)`: `WAIT_OBJECT_0` → `GetOverlappedResult(…, FALSE)` → report T4; `WAIT_TIMEOUT` → T5 |
| `CancelKernelWait` | `CancelIoEx(h, &ov)`; `GetOverlappedResult(h, &ov, &n, TRUE)`: `TRUE` → the grant raced the cancel: report T4 (it goes to the next unexpired client or is released); `FALSE` with 995 `ERROR_OPERATION_ABORTED` → report T6 [M, X18 §5] |
| `WaitInTable` | the client's thread waits on the table's condvar until notified or its deadline |
| `KernelUnlock` | `UnlockFile(h, low(b), high(b), 1, 0)` on the owning handle |
| Slot `KernelTry` | as `KernelTry`, on a new slot handle opened for this grant; closed at release or on busy |

### 7.2 Linux and macOS (`WaiterThread`)

`fl(b, type)` is `struct flock { l_type = type, l_whence = SEEK_SET, l_start = b, l_len = 1, l_pid = 0 }` (64-bit `off_t`
only: moirai supports 64-bit targets only, which avoids the 32-bit `off_t` problem at 2^62 [80 §2.2.2]).

| Step | Calls |
|---|---|
| `KernelTry` | `fcntl(role_fd, F_OFD_SETLK, fl(b, F_WRLCK))`: 0 → granted; `EAGAIN` or `EACCES` → busy; `EINTR` → repeat; `EINVAL`, `ENOTSUP`, `EOPNOTSUPP` → `LockError::Unsupported`; other → `LockError::Io` |
| `StartWaiterThread` | spawn one thread (stack 64 KiB, name `moirai-lockwait`); it opens **its own OFD** on `LOCK` (`openat(store_fd, "LOCK", O_RDWR \| O_CLOEXEC)`, identity-checked, §9.1) and blocks in `fcntl(own_fd, F_OFD_SETLKW, fl(b, F_WRLCK))`, repeating on `EINTR`. On success it takes the table's mutex and reports T4: `HandTo(o)` → the grant is owned by the waiter's OFD (`WaiterHandle`) and `o` is notified; `ReleaseNow` → `F_UNLCK` on its OFD and close. Then the thread exits. A thread that cannot be started is `LockError::Io` for the client that asked |
| `WaitInTable` | `Condvar::wait_timeout` until notified or the client's deadline |
| `KernelUnlock` | `fcntl(owner_fd, F_OFD_SETLK, fl(b, F_UNLCK))`; a `WaiterHandle` OFD is then closed (Unix opens are cheap) |
| Slot `KernelTry` | as `KernelTry`, on a new OFD opened for this grant; closed at release or on busy |

- **Why the waiter owns a separate OFD.** OFD locks of one description merge; a waiter on a shared description that
  unlocked "its" byte after being abandoned could drop a later grant made through the same description ([X18 §3]).
- **Abandoned waiter.** A waiter whose clients all timed out stays parked until it obtains the byte, then releases it at
  once (T4 `ReleaseNow`); it lives at most as long as another process holds that role byte, seconds for `Writer` and
  `Flush` ([80 §2.1]). The idle-thread rule counts it while it exists.
- **Never used:** `F_OFD_SETLKWTIMEOUT` (93 on macOS): its bound applies to each sleep of the kernel's retry loop, not to
  the call [S, X18 §2.3]; a timer signal interrupting `F_OFD_SETLKW` (racy, and a timer); polling `F_OFD_SETLK` with
  sleeps (G1's convoy); process-associated `F_SETLK`/`F_SETLKW` (released when any descriptor of the file closes).

### 7.3 Simulator

`moirai-vfs-sim` implements the kernel side as a per-store map `byte → (simulated process, handle)` with FIFO or random
wake-up (both are allowed by item 10), the release delay after a simulated process's death drawn from measurement 12's
distribution `HOLE(lock-release-delay)` plus a heavy tail beyond the 2 s bound (fault-model item (8), [F15]), and the
same step protocol as §7.1 or
§7.2 according to the seed's `WaitMode`.

---

## 8. Probes

`probe(client, b)`:

1. **In-process first.** If any client of the table holds `b`, the answer is `Held` without a kernel call (T8, I-L8).
2. **Kernel probe through a dedicated probe handle** — one per table, opened lazily, which never holds a lock between
   probes (a probe on a handle that holds `b` would report it free on Unix, where the OFD's own lock does not conflict):

| | Windows | Linux | macOS |
|---|---|---|---|
| Probe handle | `LOCK` opened with `GENERIC_READ \| GENERIC_WRITE`, falling back to `GENERIC_READ` on error 5 (`LockFileEx` needs one of the two); overlapped | `openat(store_fd, "LOCK", O_RDWR \| O_CLOEXEC)`, falling back to `O_RDONLY` on `EACCES` | as Linux |
| Call | `LockFileEx(p, EXCLUSIVE \| FAIL_IMMEDIATELY, b)`; `TRUE` → `UnlockFile(p, b)` → **`Free`**; 33 → **`Held`** | `fcntl(p, F_OFD_GETLK, fl(b, F_WRLCK))`; `l_type == F_UNLCK` → **`Free`**; otherwise **`Held`** | `fcntl(p, F_OFD_GETLK (92), fl(b, F_WRLCK))`; same reading |
| Side effect | holds `b` for a few µs; a concurrent try of `b` by any process, this one included, may see `Busy` in that window, which item 10 tolerates | none | none |
| Errors | the handle cannot be opened, or any other error → **`Unknown`** | any error (`EPERM`, `EACCES`, `EBADF` from a sandbox or a read-only descriptor included) → **`Unknown`** | same as Linux |

3. `Unknown` never ends a lease or recovers an intent; only a deadline, the run scope or an explicit `reclaim` does
   ([AR §6.2]).
4. Readers of slot records accept a record only if its checksum is valid, and re-read it after probing (seqlock style,
   X-F1); the procedure is [F03]'s.
5. Cost: 2–9 µs on Windows [M, X18 §5]; one `fcntl` on Unix (unmeasured).

---

## 9. `LOCK` identity and handles

### 9.1 Identity check (item 9)

`lock_client(store, mode)`:

1. Open the **data handle**: `open(store, "LOCK", Read)` in `Probe` mode, `ReadWrite` in `Acquire` mode ([OS/fs §4.2]).
   `NotFound` → `NoLockFile`. `AccessDenied` in `Acquire` mode → `LockError::AccessDenied` (the sandbox exit 7 of
   [90 §5.3]).
2. Compare `identity(data handle)` with `path_identity(store, "LOCK")` ([OS/fs §4.10]). On a mismatch close, reopen once
   and compare again; a second mismatch is `IdentityMismatch` (exit 7).
3. Join or create the process's table for that identity (§5.1).
4. **Every later handle** the client or its table opens on `LOCK` — role handles, slot handles, the probe handle, a
   waiter's OFD — is checked the same way against the data handle's identity when it is opened; a mismatch is
   `IdentityMismatch`.

### 9.2 Handle inventory

| Handle | Count | Opened | Flags | Closed |
|---|---|---|---|---|
| data | 1 per client | `lock_client` | [OS/fs §5] (synchronous) | client drop |
| role handle | ≤ 1 per role byte per table | first kernel try of that role | Windows: `GENERIC_READ \| GENERIC_WRITE`, share read/write/delete, open existing, overlapped (no `FILE_SYNCHRONOUS_IO_*`), relative to the store root; Unix: `O_RDWR \| O_CLOEXEC` (`F_WRLCK` needs write access, else `EBADF`) | last client of the table dropped |
| slot handle | 1 per slot grant | T1 for a slot | as a role handle | release, or busy |
| probe handle | ≤ 1 per table | first kernel probe | §8 | last client of the table dropped |
| waiter OFD (Unix) | 1 per waiter thread | the waiter thread | `O_RDWR \| O_CLOEXEC` | after its grant is released, or at once if abandoned |

Every handle is non-inheritable (item 7). No handle ever locks a byte of `LOCK`'s data (item 1). On Unix every open of
`LOCK` is the relative open of [OS/fs §5.2] (`openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` on Linux,
`O_NOFOLLOW_ANY` on macOS); this file's tables abbreviate it as `openat(store_fd, "LOCK", …)`.

---

## 10. Liveness slots at the lock level

- Slot `i` is `Slot(SlotIndex(i))`, offset `SLOT_BASE + i`, `i < 256` ([80] X-F1).
- Slots are only ever **tried**. A busy slot moves the caller to the next one; the starting slot (`hash(session) mod 256`
  for a server, `hash(nonce) mod 256` for an intent), the probe order and the behaviour when all are busy are
  [OS/proc §6.4]'s ([80 §2.7.2]); the liveness decision over slot probes is [OS/proc §6.2]'s.
- A slot may be taken at any point of a process's life, lazily at the first call that carries the identity it tracks
  ([90 §4.4]); being a try, it needs no position in the lock order and may be taken while other bytes are held.
- The holder writes its `SlotRec` at `4096 + 128 × i` right after the grant and before it serves anything; [F03] owns the
  record and its checksum.
- A slot is released explicitly (`FsIntentDone`/`Aborted` for an intent) or by process death; after death the byte reads
  `Held` for the release lag (item 10), which makes a dead holder look `Alive` for that long — an error in the safe
  direction ([80 §2.7.2]).

---

## 11. Foreign locks (item 8)

moirai never calls `flock` or `File::lock` on a store file (GT20 (d) in product crates). `foreign_lock_check` lets
`doctor` warn when another tool holds such a lock on `LOCK`; it is never a refusal ([80 §2.2.1] item 8):

| Windows | Linux | macOS |
|---|---|---|
| probe `FOREIGN_CHECK_BYTE` (2^62 + 63, never locked by moirai) through §8: `Held` means a foreign range lock covers it (for example a whole-range `File::lock`) | `flock(fd, LOCK_EX \| LOCK_NB)` on a fresh `O_RDONLY \| O_CLOEXEC` descriptor: `EWOULDBLOCK` → `Held`; success → `flock(fd, LOCK_UN)`, `Free`. Then probe `FOREIGN_CHECK_BYTE` as on Windows (a foreign OFD or POSIX range lock); `Held` from either → `Held` | probe `FOREIGN_CHECK_BYTE` only: flock and OFD locks share one list per vnode on macOS, so a foreign `flock` shows there, and an `flock` probe by `doctor` would itself collide with moirai's own byte locks |

`Held` produces a `doctor` warning (text in [F19]); `Unknown` produces "not checked".

---

## Appendix A. Per-OS mapping

| Item | Windows 11 (built from M0) | Linux ≥ 5.10, 64-bit (port; OFD since 3.15) | macOS ≥ 14 (port) |
|---|---|---|---|
| Open `LOCK` for locks | `NtCreateFile` relative to the store root: `FILE_GENERIC_READ \| FILE_GENERIC_WRITE`, share read/write/delete, `FILE_OPEN`, `FILE_NON_DIRECTORY_FILE`, no synchronous-I/O option (overlapped) | `openat(store_fd, "LOCK", O_RDWR \| O_CLOEXEC)` | as Linux; XNU checks `FWRITE` for `F_WRLCK` |
| Try-acquire (after the table) | `LockFileEx(EXCLUSIVE \| FAIL_IMMEDIATELY, off b, len 1)`; 33 → busy | `fcntl(F_OFD_SETLK = 37, fl(b, F_WRLCK))`; `EAGAIN`/`EACCES` → busy | `fcntl(F_OFD_SETLK = 90, …)`; same errors |
| Bounded wait | overlapped `LockFileEx` + `WaitForSingleObject(T)` + `CancelIoEx` + `GetOverlappedResult(TRUE)`: granted, or 995 → busy | waiter thread in `fcntl(F_OFD_SETLKW = 38, …)` on its own OFD, `EINTR` repeated; condvar deadline; hand-off under the mutex; abandoned grant released at once | as Linux with `F_OFD_SETLKW = 91`; `F_OFD_SETLKWTIMEOUT` (93) never used |
| Release | `UnlockFile(h, low(b), high(b), 1, 0)` | `fcntl(F_OFD_SETLK, fl(b, F_UNLCK))` | same |
| Probe | try + `UnlockFile` on the probe handle | `fcntl(F_OFD_GETLK = 36, fl(b, F_WRLCK))`; `l_type != F_UNLCK` → held | `fcntl(F_OFD_GETLK = 92, …)` |
| Holder from the kernel | none | `l_pid = −1` for OFD | `l_pid = −1` |
| Release after the holder dies | by the OS; ≤ 32 ms observed after `TerminateProcess` (p99 1–8 ms) [M, X18 §5]; unbounded while a crash reporter holds the process | at the last close of the OFD at process exit; unbounded while `systemd-coredump` or a debugger holds the process | same; ReportCrash may hold a crashing process for seconds [I] |
| Unsupported | a UNC path or `DRIVE_REMOTE` is refused by the guard before any lock ([OS/env]); `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_FUNCTION` → `Unsupported` | `EINVAL`, `ENOTSUP`, `EOPNOTSUPP` from `F_OFD_*` → `Unsupported` (exit 7) | same, plus volumes without `MNT_LOCAL` refused by the guard |
| Version floor that shapes this code | — | kernel ≥ 3.15 for OFD (floor 5.10, [80 §2.13]) | macOS 14: the first public SDK declaring `F_OFD_*`; linked with minimum OS 14 and checked at open ([OS/env §6]); the private range 10.13–13 is never used ([80] X9) |
| Waiter thread | none (the caller's thread drives the overlapped wait) | 64 KiB stack, one per pending wait per role per process | same |
| Foreign-lock check (§11) | probe of 2^62 + 63 | `flock(LOCK_EX \| LOCK_NB)` probe and probe of 2^62 + 63 | probe of 2^62 + 63 |
| Constants not in `libc` | — | — | `F_OFD_SETLKW` and friends are declared locally in `src/macos/` if the pinned `libc` lacks them ([X18 §2.3]: `libc` 0.2.189 exports `F_OFD_SETLK`, `F_OFD_SETLKW`, `F_OFD_GETLK` for Apple) |

---

## Holes

| Id | What | Decided by | Candidates | Constraint the value must meet |
|---|---|---|---|---|
| `lock-writer-wait-ms` (referenced; the key `lock.writer-wait-ms` is registered in [CFG], which owns the hole) | default bound of the writer-byte wait | measurements 2 and 12 (WP-52) → WP-81a | 2,000 ms (the registered default, [AR §13]) | > the measured p99 writer-byte wait of the 16-writer burst plus the p99 release lag after `TerminateProcess`, loaded; the 16-writer last-acknowledgement gate (p99 ≤ 50 ms) is met far below it |
| `lock-flush-wait-ms` (referenced; key `lock.flush-wait-ms`, [CFG]) | default bound of the flush-byte wait | measurement 2 (WP-52) → WP-81a | 2,000 ms ([AR §13], [80] X-F11) | > the measured p99 flush-byte hand-off latency under the 16-writer burst, loaded; a timeout is exit 7 with outcome `pending` |
| `lock-release-delay` (referenced; the simulator's injection parameters, [F15]) | distribution of lock release after `TerminateProcess` for writer, flush and slot bytes | measurement 12 (WP-52) | measured p50/p99/max plus a heavy tail beyond 2 s | covers the measured maximum; the tail exceeds the wait bounds above so GT1/GT3 exercise the timeout paths |

## Open points for the review

| # | Point | Resolution in this file | For |
|---|---|---|---|
| 1 | [80 §2.2.1] specifies behaviour, not a structure, for the grant table; the simulator and `moirai-os` must share it | a pure state machine (§5) in `moirai-vfs` whose steps each implementation performs; both wait modes selectable, and the simulator runs both | R-REV-P, WP-30, WP-31 |
| 2 | [X18 §6.1] item 2 (one owner handle per grant) conflicts with [80 §2.2.1] item 2 (one handle per role per process plus the grant table) | [80] wins (its own reservation, and revision 2 answered [81] M1); slot grants keep a handle each, as [80] says | R-REV-P |
| 3 | [80 §2.2.1] states waiter joining for Unix only | single flight (one kernel acquisition per byte per process) applies on every OS, which is what "the kernel never sees two in-process acquisitions of one byte" requires; on Windows the oldest waiting client drives the one overlapped request and hands over on timeout (T5, T6) | R-REV-P |
| 4 | The lock order of [80 §2.2.3] is phrased per process | checked per client, with the rule "one client per store per product process" (§6 item 5) so that per-client and per-process coincide in the product; tests with several clients use separate threads or simulated processes | R-REV-P |
| 5 | Only `Writer` and `Flush` are waitable | `acquire_within` panics on any other byte; [80 §2.2.3] makes every other byte try- or probe-only | R-REV-P |
| 6 | The Unix waiter thread's OFD after a hand-off | the grant stays owned by the waiter's OFD until release, which unlocks through it and closes it; one waiter thread per pending wait, exiting after its grant is handed on or released ([80 §2.1] bounds its life the same way) | R-REV-P |
| 7 | Windows probes momentarily hold the byte | not serialized with in-process tries: a try that meets the probe window sees `Busy`, which item 10 already tolerates for cross-process probes | R-REV-P |
| 8 | [80 §2.2.1] item 8 names only the Unix `flock` probe | `foreign_lock_check` per OS (§11): the reserved byte 2^62 + 63 on Windows and macOS (a `flock` probe on macOS would collide with moirai's own OFD locks), both checks on Linux | R-REV-P |
| 9 | A Windows `LockFileEx` try with `FAIL_IMMEDIATELY` on an overlapped handle may complete as pending | settled with `GetOverlappedResult(TRUE)` (§7.1) [I]; WP-33 tests both completions | WP-33 |
| 10 | Which bytes `restore` holds and in which order | try `Maintenance`, then wait `Writer` (§6); [F16] owns the sequence | WP-16 |
