//! The adversary: the component that makes every choice the fault model allows ([F15 §1.3]).
//!
//! Every freedom of [F15 §3] is one [`Site`]. At each site the simulator builds a [`Choice`] (the site, the process and
//! node it concerns, an auxiliary value and the number of allowed answers) and asks its [`Adversary`]; the answer is
//! recorded in the trace. [`SeededAdversary`] answers from a seeded generator and a table of [`FaultRates`]; the crash
//! enumerator (WP-32) implements [`Adversary`] itself to drive the same sites deterministically ([F15 §1.3]: "In an
//! enumeration run, the crash enumerator drives the same choices"). The answer's meaning per site is documented on
//! [`Site`]; an answer at or beyond the arity is reduced modulo the arity, so a faulty adversary stays deterministic.

use crate::rng::Rng;

/// One kind of adversary choice. The comment of each variant gives the arity and the meaning of the answer.
#[repr(u16)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Site {
    /// Which runnable task runs next (FM-6, FM-11). Arity: the number of runnable tasks; the answer indexes them in task
    /// order.
    Schedule = 1,
    /// A pause of the current task at this event (FM-6.1). Any `u64`: 0 no pause, else the pause in ns.
    Pause = 2,
    /// A system suspend at this event (FM-6.3). Any `u64`: 0 none, else its length in ns.
    Suspend = 3,
    /// Whether the monotonic clock counts the suspend just chosen (FM-7.2, OP-14). 2: 0 excludes, 1 includes.
    SuspendMono = 4,
    /// A wall-clock step before this reading (FM-7.1). Any `u64`: 0 none, else the step in ms as a two's-complement
    /// `i64`.
    WallStep = 5,
    /// Whether a new process can read the boot identity (FM-7.4). 2: 0 Known, 1 Unknown.
    BootMode = 6,
    /// Whether this one boot-identity read answers Unknown (FM-7.4, OP-17). 2: 0 the process's answer, 1 Unknown.
    BootRead = 7,
    /// A write's fault (FM-5.1, FM-5.5). 3: 0 none, 1 `DiskFull`, 2 `Io`.
    WriteFault = 8,
    /// How much of a failed or interrupted write applied (FM-5.2, §2.5). Any `u64`, decoded by [`PartialWrite`].
    PartialWrite = 9,
    /// A file flush's fault (FM-3.1, FM-5.3, §3.13). 4: 0 none, 1 `Io`, 2 `DiskFull`, 3 `Unsupported`.
    FlushFault = 10,
    /// A `sync_dir`'s fault (FM-3.7, FM-5.1). 4: as `FlushFault`.
    SyncDirFault = 11,
    /// A creation's fault (FM-5.4). 3: 0 none, 1 `DiskFull` with the name absent, 2 `DiskFull` leaving an empty file. A
    /// directory creation is a namespace operation, which a failure leaves unchanged (FM-5.4, NS-4): for it 2 is 1.
    CreateFault = 12,
    /// A namespace operation's fault (FM-5.1, NS-4). 3: 0 none, 1 `DiskFull`, 2 `AccessDenied`.
    NsFault = 13,
    /// A sharing violation on open, unlink or rename (FM-8.2). Any `u64`: 0 none, else the number of consecutive
    /// attempts that fail, `u64::MAX` for the whole scenario.
    Sharing = 14,
    /// A read error (FM-12.1, FM-12.2). 3: 0 none, 1 transient (this read only), 2 persistent (every later read of the
    /// range, across crashes).
    ReadFault = 15,
    /// The member of a poisoned sector's candidate set K that one sub-sector of one read returns (FM-3.2). Arity |K|.
    PoisonRead = 16,
    /// The content one sub-sector of a read shows when writes overlapped the read's interval (FM-4.1). Arity: 1 + the
    /// number of overlapping writes; 0 is the content at the read's start.
    ConcurrentRead = 17,
    /// The member of K that fixes an unwritten sub-sector when a poisoned sector is re-written (FM-3.5). Arity |K|.
    RewriteMerge = 18,
    /// Whether an unlink of an open file lingers as delete-pending (FM-8.3). 2: 0 takes effect at once, 1 lingers until
    /// the last handle closes.
    DeletePending = 19,
    /// The error of a create or rename onto a delete-pending name (FM-8.3, [OS/fs §6.4]). 3 for a create: 0
    /// `AlreadyExists`, 1 `AccessDenied`, 2 `DeletePending`; 2 for a rename (the first two).
    CreateOverPending = 20,
    /// Whether a kernel probe of a byte answers `Unknown` (§3.13, X-F4 item 6). 2: 0 the true answer, 1 `Unknown`.
    ProbeUnknown = 21,
    /// Which kernel wait obtains a freed byte (X-F4 item 10: fairness is not part of the contract). Arity: waits + 1;
    /// the last value grants none now (a spurious timeout for the waiters, §3.13).
    KernelWake = 22,
    /// The release-delay class of one byte of a dead process (FM-8.1). 3: 0 class (a) measured, 1 class (b) tail beyond
    /// every wait bound, 2 class (c) never within the scenario. `aux` is the byte kind (0 writer and other roles, 1 flush,
    /// 2 slot).
    ReleaseClass = 23,
    /// The delay of class (a) or (b), in ns (FM-8.1). Any `u64`. `aux` is `class << 8 | kind`.
    ReleaseDelay = 24,
    /// A media fault under a mapped read (FM-9.1). 2: 0 none, 1 fault: the reading process dies.
    MapFault = 25,
    /// A mapped read of an externally truncated file (FM-9.1, FM-9.2). 2: 0 the process dies, 1 zeros beyond the end.
    MapTruncated = 26,
    /// The outcome of a dead process's in-flight file flush (§2.5, FM-11.2). 3: 0 succeeded, 1 failed, 2 not performed.
    FlushAtDeath = 27,
    /// The outcome of a dead process's in-flight `sync_dir` (§2.5). 2: 0 succeeded, 1 not performed.
    SyncDirAtDeath = 28,
    /// A system crash at this scheduling point (GT3 random runs). 2: 0 no, 1 crash with a seeded plan.
    SystemCrash = 29,
    /// The grant table's wait mode for this world ([OS/lock §5.1]: "chosen per seed"). 2: 0 `CallerDriven`, 1
    /// `WaiterThread`.
    WaitMode = 30,
    /// Crash: a file's new size (FM-2.2, §2.5 step 2). Arity |H(f)|; the answer indexes H(f) in ascending order.
    CrashSize = 31,
    /// Crash: the content a dirty sector keeps (FM-1.1). Arity 1 + m: 0 the baseline, i the version vᵢ.
    CrashSector = 32,
    /// Crash: which dirty sector of a file tears (FM-1.2). Arity: dirty sectors + 1; 0 none, i the i-th dirty sector in
    /// offset order.
    CrashTorn = 33,
    /// Crash: the content one sub-sector of a torn or poisoned sector keeps (FM-1.2, FM-3.3). Arity: the candidates.
    CrashSub = 34,
    /// Crash: whether a pending namespace operation survives (FM-2.3, §2.5 step 3). 2: 0 lost, 1 survives.
    CrashOp = 35,
    /// Crash: what bytes beyond the old durable size hold (FM-2.2, OP-4). 3: 0 the resolved sectors, 1 zeros, 2
    /// garbage.
    CrashBeyond = 36,
    /// Crash: the seed of the garbage of `CrashBeyond` = 2. Any `u64`.
    CrashGarbage = 37,
    /// Crash: how a write in flight at the crash applied (§2.5 step 1). Any `u64`, decoded by [`PartialWrite`].
    CrashPartial = 38,
    /// The Windows error of one failing sharing attempt (FM-8.2, [OS/fs §6.2]). 2: 0 error 32 `SharingViolation`, 1 error
    /// 5 `AccessDenied`.
    SharingKind = 39,
    /// Whether the native directory exchange of `swap_dirs` is available on a Linux or macOS volume ([OS/fs §4.9.1]:
    /// `EINVAL`, `ENOTSUP` or `EOPNOTSUPP` select the two-rename form). 2: 0 available, 1 unsupported. Never asked for
    /// Windows, which always uses the two-rename form.
    SwapExchange = 40,
}

impl Site {
    /// The site's code in the trace.
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// The context of one choice.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Choice {
    /// The site.
    pub site: Site,
    /// The process concerned (`u32::MAX`: none).
    pub proc: u32,
    /// The node concerned (0: none), or for `Schedule` the current task.
    pub node: u64,
    /// A site-specific value (for example an offset, a sector index or a byte kind).
    pub aux: u64,
    /// The number of allowed answers; `u64::MAX` means any `u64`.
    pub arity: u64,
}

/// The component that decides every freedom of the fault model ([F15 §1.3]).
pub trait Adversary: Send {
    /// The answer at `choice`; `rng` is the world's seeded generator, which the adversary may use or ignore. Must be a
    /// deterministic function of the adversary's state, `choice` and `rng`.
    fn choose(&mut self, choice: &Choice, rng: &mut Rng) -> u64;
}

/// How much of a write applied (FM-5.2 for a failed write; §2.5 for a write in flight at a death or crash): each byte
/// of the range holds its old or its new value, in any combination.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PartialWrite {
    /// No byte changed.
    Nothing,
    /// Every byte changed.
    All,
    /// The first `n` bytes changed.
    Prefix(u64),
    /// Whole sectors of the write changed or not, by bit: sector k of the write (k = 0 for the 4 KiB sector that holds
    /// its first byte) takes its new bytes iff bit min(k, 61) of the 62-bit mask is set. The form an exhaustive
    /// enumeration of a death inside a multi-sector write iterates.
    Sectors(u64),
    /// Each byte changed or not by a generator seeded with this value.
    Mask(u64),
}

const PREFIX_BIT: u64 = 1 << 63;
const SECTORS_BIT: u64 = 1 << 62;
const SECTORS_MASK: u64 = SECTORS_BIT - 1;

impl PartialWrite {
    /// The choice value that encodes `self`: 0 `Nothing`, 1 `All`, `2^63 | n` `Prefix(n)` (n < 2^63), `2^62 | m`
    /// `Sectors(m)` (m < 2^62), any other value `Mask(value)`. `Mask` seeds 0, 1 and those with bit 62 or 63 set are
    /// therefore not encodable; the decoder never needs them.
    pub fn to_choice(self) -> u64 {
        match self {
            PartialWrite::Nothing => 0,
            PartialWrite::All => 1,
            PartialWrite::Prefix(n) => PREFIX_BIT | (n & !PREFIX_BIT),
            PartialWrite::Sectors(m) => SECTORS_BIT | (m & SECTORS_MASK),
            PartialWrite::Mask(seed) => (seed & SECTORS_MASK).max(2),
        }
    }

    /// Decodes a choice value (see [`PartialWrite::to_choice`]).
    pub fn from_choice(v: u64) -> PartialWrite {
        match v {
            0 => PartialWrite::Nothing,
            1 => PartialWrite::All,
            v if v & PREFIX_BIT != 0 => PartialWrite::Prefix(v & !PREFIX_BIT),
            v if v & SECTORS_BIT != 0 => PartialWrite::Sectors(v & SECTORS_MASK),
            v => PartialWrite::Mask(v),
        }
    }

    /// Whether byte `i` of a write at file offset `offset` takes its new value, for a generator `mask` built by
    /// [`PartialWrite::mask_rng`].
    pub(crate) fn applies(self, offset: u64, i: u64, mask: &mut Option<Rng>) -> bool {
        match self {
            PartialWrite::Nothing => false,
            PartialWrite::All => true,
            PartialWrite::Prefix(n) => i < n,
            PartialWrite::Sectors(m) => {
                let k = (offset + i) / crate::content::SECTOR - offset / crate::content::SECTOR;
                m & (1 << k.min(61)) != 0
            }
            PartialWrite::Mask(_) => mask.as_mut().is_some_and(|r| r.next_u64() & 1 == 1),
        }
    }

    /// The per-byte generator of `Mask`.
    pub(crate) fn mask_rng(self) -> Option<Rng> {
        match self {
            PartialWrite::Mask(seed) => Some(Rng::new(seed)),
            _ => None,
        }
    }
}

/// The per-million rates at which [`SeededAdversary`] injects each fault. All zero by default: a fault-free world, in
/// which only the schedule, the crash resolution and the per-OS alternatives (delete-pending lingering, the zeros of a
/// truncated mapping) are drawn.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct FaultRates {
    /// A pause at a scheduling point (FM-6).
    pub pause: u32,
    /// A system suspend at a scheduling point (FM-6.3).
    pub suspend: u32,
    /// A wall-clock step before a wall reading (FM-7.1).
    pub wall_step: u32,
    /// A process that cannot read its boot identity (FM-7.4).
    pub unknown_boot: u32,
    /// A single boot-identity read that answers Unknown (FM-7.4).
    pub unknown_boot_read: u32,
    /// A write error (FM-5).
    pub write_fault: u32,
    /// A file flush error (FM-3, FM-5.3).
    pub flush_fault: u32,
    /// A `sync_dir` error (FM-3.7).
    pub sync_dir_fault: u32,
    /// A creation error (FM-5.4).
    pub create_fault: u32,
    /// A namespace-operation error (FM-5.1).
    pub ns_fault: u32,
    /// A sharing violation starting at an open, unlink or rename (FM-8.2).
    pub sharing: u32,
    /// A read error (FM-12).
    pub read_fault: u32,
    /// A probe answering `Unknown` (§3.13).
    pub probe_unknown: u32,
    /// A freed byte granted to no kernel waiter at once (§3.13 spurious timeouts).
    pub spurious_wake: u32,
    /// A media fault under a mapped read (FM-9.1).
    pub map_fault: u32,
    /// A system crash at a scheduling point.
    pub system_crash: u32,
    /// A torn sector per file at a seeded crash (FM-1.2).
    pub torn: u32,
    /// A Linux or macOS volume without the native directory exchange ([OS/fs §4.9.1]).
    pub no_exchange: u32,
}

impl FaultRates {
    /// Moderate rates for random multi-process runs (GT3): every fault occurs, none dominates.
    pub fn adversarial() -> FaultRates {
        FaultRates {
            pause: 20_000,
            suspend: 1_000,
            wall_step: 10_000,
            unknown_boot: 100_000,
            unknown_boot_read: 5_000,
            write_fault: 2_000,
            flush_fault: 5_000,
            sync_dir_fault: 5_000,
            create_fault: 2_000,
            ns_fault: 2_000,
            sharing: 5_000,
            read_fault: 2_000,
            probe_unknown: 20_000,
            spurious_wake: 50_000,
            map_fault: 1_000,
            system_crash: 0,
            torn: 500_000,
            no_exchange: 250_000,
        }
    }
}

/// The lock-release delay law of FM-8.1: classes (a), (b) and (c).
///
/// Class (a) is the measured distribution per byte kind, HOLE(F15-lock-release), which measurement 12 (WP-52) decides and
/// WP-81a fills; it is a test parameter here, given as an empirical CDF. Until the fill, [`ReleaseDelayLaw::default`]
/// uses the prior evidence the hole row cites ([80 §2.2.2]: ≤ 32 ms observed, p99 1–8 ms after `TerminateProcess`) as a
/// piecewise-linear CDF that stochastically dominates it: p50 1 ms, p99 8 ms, max 32 ms. Classes (b) and (c) always stay
/// in addition ([F15 §3.8], the hole's constraint: "a value never removes the unbounded tail"; [OS/lock §7.3]: "plus a
/// heavy tail beyond the 2 s bound"): the default law draws class (b) for 2 % and class (c) for 0.5 % of the bytes a
/// dead process held, and [`ReleaseDelayLaw::adversarial`] 20 % and 5 %. A death plan
/// ([`crate::DeathPlan::release_class`]) forces a class; [`crate::RunReport::release_classes`] counts the classes drawn, so
/// the nightly harness can check the obligation of [F15 §3.8] (at least one delay beyond every wait bound and one byte
/// never released per run).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseDelayLaw {
    /// Class (a) per byte kind — index 0 the writer byte (and the other role bytes), 1 the flush byte, 2 the slot bytes —
    /// as CDF points `(quantile in per-million, delay in ns)`, ascending in both, ending at quantile 1,000,000.
    pub measured: [Vec<(u32, u64)>; 3],
    /// The largest configured lock wait bound (`lock.writer-wait-ms`, `lock.flush-wait-ms`), in ns; class (b) delays lie
    /// strictly above it.
    pub wait_bound_ns: u64,
    /// The scenario horizon in ns; class (b) delays lie below it.
    pub horizon_ns: u64,
    /// The per-million probability of class (b).
    pub p_tail: u32,
    /// The per-million probability of class (c).
    pub p_never: u32,
}

impl Default for ReleaseDelayLaw {
    fn default() -> ReleaseDelayLaw {
        let prior = vec![
            (0, 0),
            (500_000, 1_000_000),
            (990_000, 8_000_000),
            (1_000_000, 32_000_000),
        ];
        ReleaseDelayLaw {
            measured: [prior.clone(), prior.clone(), prior],
            wait_bound_ns: 2_000_000_000,
            horizon_ns: 600_000_000_000,
            p_tail: 20_000,
            p_never: 5_000,
        }
    }
}

impl ReleaseDelayLaw {
    /// The law of random multi-process runs (GT3), with [`FaultRates::adversarial`]: the default class (a) with class (b)
    /// at 20 % and class (c) at 5 %, so that a run with a handful of deaths draws every class.
    pub fn adversarial() -> ReleaseDelayLaw {
        ReleaseDelayLaw {
            p_tail: 200_000,
            p_never: 50_000,
            ..ReleaseDelayLaw::default()
        }
    }

    /// Samples class (a) for byte kind `kind` at the per-million quantile `u` by linear interpolation.
    pub fn measured_at(&self, kind: usize, u: u32) -> u64 {
        let cdf = &self.measured[kind.min(2)];
        let mut prev = (0u32, 0u64);
        for &(q, ns) in cdf {
            if u <= q {
                if q == prev.0 {
                    return ns;
                }
                let span = u64::from(q - prev.0);
                let into = u64::from(u - prev.0);
                let delta = ns.saturating_sub(prev.1);
                return prev.1 + (u128::from(delta) * u128::from(into) / u128::from(span)) as u64;
            }
            prev = (q, ns);
        }
        prev.1
    }
}

/// The default adversary: seeded, with [`FaultRates`] and a [`ReleaseDelayLaw`].
#[derive(Clone, Debug)]
pub struct SeededAdversary {
    /// Fault rates.
    pub rates: FaultRates,
    /// Lock-release delays.
    pub law: ReleaseDelayLaw,
    /// The longest pause the seeded draw produces, in ns. FM-6 bounds nothing: a queued choice or another adversary may
    /// pause for any `u64` duration; the seeded draw is log-uniform up to this value, so short and very long pauses are
    /// both frequent.
    pub pause_max_ns: u64,
    /// The longest suspend the seeded draw produces, in ns (log-uniform as well).
    pub suspend_max_ns: u64,
}

impl SeededAdversary {
    /// An adversary with these rates and law, pauses drawn up to one day (well beyond GT4's 1–120 s and every lease and
    /// grace period of the store parameters, `docs/spec/format/17-store-parameters.md`) and suspends up to one week.
    pub fn new(rates: FaultRates, law: ReleaseDelayLaw) -> SeededAdversary {
        SeededAdversary {
            rates,
            law,
            pause_max_ns: 86_400_000_000_000,
            suspend_max_ns: 604_800_000_000_000,
        }
    }
}

/// A duration in `1..=max` (ns) whose logarithm is uniform: every order of magnitude is as likely as any other.
fn log_uniform(rng: &mut Rng, max: u64) -> u64 {
    let max = max.max(1);
    let bits = 64 - max.leading_zeros();
    let b = 1 + rng.below(u64::from(bits)) as u32;
    let top = if b >= 64 { u64::MAX } else { (1u64 << b) - 1 };
    (1 + rng.below(top)).min(max)
}

/// A wall-clock step: small, large or extreme, forward or backward ("any amount", FM-7.1).
fn wall_step(rng: &mut Rng) -> u64 {
    let magnitude: i64 = match rng.below(3) {
        0 => 1 + rng.below(1_000) as i64,
        1 => 1_000 + rng.below(86_400_000) as i64,
        _ => 86_400_000 + rng.below(3_155_760_000_000) as i64,
    };
    let step = if rng.below(2) == 0 {
        magnitude
    } else {
        -magnitude
    };
    step as u64
}

impl Adversary for SeededAdversary {
    fn choose(&mut self, c: &Choice, rng: &mut Rng) -> u64 {
        let r = &self.rates;
        let fault = |rng: &mut Rng, rate: u32, kinds: u64| {
            if rng.chance(rate) {
                1 + rng.below(kinds)
            } else {
                0
            }
        };
        match c.site {
            Site::Pause => {
                if rng.chance(r.pause) {
                    log_uniform(rng, self.pause_max_ns)
                } else {
                    0
                }
            }
            Site::Suspend => {
                if rng.chance(r.suspend) {
                    log_uniform(rng, self.suspend_max_ns)
                } else {
                    0
                }
            }
            Site::WallStep => {
                if rng.chance(r.wall_step) {
                    wall_step(rng)
                } else {
                    0
                }
            }
            Site::BootMode => u64::from(rng.chance(r.unknown_boot)),
            Site::BootRead => u64::from(rng.chance(r.unknown_boot_read)),
            Site::WriteFault => fault(rng, r.write_fault, 2),
            Site::FlushFault => fault(rng, r.flush_fault, 3),
            Site::SyncDirFault => fault(rng, r.sync_dir_fault, 3),
            Site::CreateFault => fault(rng, r.create_fault, 2),
            Site::NsFault => fault(rng, r.ns_fault, 2),
            Site::ReadFault => fault(rng, r.read_fault, 2),
            Site::Sharing => {
                if rng.chance(r.sharing) {
                    if rng.below(16) == 0 {
                        u64::MAX
                    } else {
                        1 + rng.below(8)
                    }
                } else {
                    0
                }
            }
            Site::ProbeUnknown => u64::from(rng.chance(r.probe_unknown)),
            Site::KernelWake => {
                let waits = c.arity - 1;
                if rng.chance(r.spurious_wake) {
                    waits
                } else {
                    rng.below(waits.max(1))
                }
            }
            Site::ReleaseClass => {
                let u = rng.below(1_000_000);
                if u < u64::from(self.law.p_never) {
                    2
                } else if u < u64::from(self.law.p_never) + u64::from(self.law.p_tail) {
                    1
                } else {
                    0
                }
            }
            Site::ReleaseDelay => {
                let class = c.aux >> 8;
                let kind = (c.aux & 0xFF) as usize;
                if class == 1 {
                    let lo = self.law.wait_bound_ns.saturating_add(1);
                    let hi = self.law.horizon_ns.max(lo.saturating_add(1));
                    lo + rng.below(hi - lo)
                } else {
                    self.law.measured_at(kind, rng.below(1_000_001) as u32)
                }
            }
            Site::MapFault => u64::from(rng.chance(r.map_fault)),
            Site::SwapExchange => u64::from(rng.chance(r.no_exchange)),
            Site::SystemCrash => u64::from(rng.chance(r.system_crash)),
            Site::CrashTorn => {
                if rng.chance(r.torn) {
                    1 + rng.below(c.arity - 1)
                } else {
                    0
                }
            }
            Site::PartialWrite
            | Site::CrashPartial
            | Site::CrashGarbage
            | Site::Schedule
            | Site::SuspendMono
            | Site::PoisonRead
            | Site::ConcurrentRead
            | Site::RewriteMerge
            | Site::DeletePending
            | Site::CreateOverPending
            | Site::MapTruncated
            | Site::FlushAtDeath
            | Site::SyncDirAtDeath
            | Site::WaitMode
            | Site::CrashSize
            | Site::CrashSector
            | Site::CrashSub
            | Site::CrashOp
            | Site::CrashBeyond
            | Site::SharingKind => rng.below(c.arity),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_write_codes_round_trip() {
        for p in [
            PartialWrite::Nothing,
            PartialWrite::All,
            PartialWrite::Prefix(0),
            PartialWrite::Prefix(4096),
            PartialWrite::Sectors(0b1011),
            PartialWrite::Sectors(0),
            PartialWrite::Mask(12345),
        ] {
            assert_eq!(PartialWrite::from_choice(p.to_choice()), p);
        }
        // Sector k of a write at offset 4000: bytes 0..96 lie in its sector 0, bytes 96.. in sector 1.
        let pw = PartialWrite::Sectors(0b10);
        assert!(!pw.applies(4000, 95, &mut None));
        assert!(pw.applies(4000, 96, &mut None));
        assert!(!PartialWrite::Sectors(1 << 61).applies(0, 60 * 4096, &mut None));
        assert!(PartialWrite::Sectors(1 << 61).applies(0, 70 * 4096, &mut None));
    }

    #[test]
    fn every_release_class_is_drawn_by_the_default_and_adversarial_laws() {
        for law in [ReleaseDelayLaw::default(), ReleaseDelayLaw::adversarial()] {
            assert!(law.p_tail > 0 && law.p_never > 0);
            let mut adv = SeededAdversary::new(FaultRates::default(), law.clone());
            let mut rng = Rng::new(9);
            let mut seen = [0u32; 3];
            for _ in 0..20_000 {
                let c = Choice {
                    site: Site::ReleaseClass,
                    proc: 0,
                    node: 1,
                    aux: 0,
                    arity: 3,
                };
                seen[adv.choose(&c, &mut rng) as usize] += 1;
            }
            assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
            let tail = Choice {
                site: Site::ReleaseDelay,
                proc: 0,
                node: 1,
                aux: 1 << 8,
                arity: u64::MAX,
            };
            let d = adv.choose(&tail, &mut rng);
            assert!(d > law.wait_bound_ns && d < law.horizon_ns);
        }
    }

    #[test]
    fn default_release_law_interpolates_the_prior_evidence() {
        let law = ReleaseDelayLaw::default();
        assert_eq!(law.measured_at(0, 0), 0);
        assert_eq!(law.measured_at(0, 500_000), 1_000_000);
        assert_eq!(law.measured_at(1, 990_000), 8_000_000);
        assert_eq!(law.measured_at(2, 1_000_000), 32_000_000);
        assert_eq!(law.measured_at(0, 250_000), 500_000);
    }

    #[test]
    fn fault_free_rates_inject_nothing() {
        let mut adv = SeededAdversary::new(FaultRates::default(), ReleaseDelayLaw::default());
        let mut rng = Rng::new(3);
        for site in [
            Site::WriteFault,
            Site::FlushFault,
            Site::Pause,
            Site::Sharing,
        ] {
            let c = Choice {
                site,
                proc: 0,
                node: 1,
                aux: 0,
                arity: u64::MAX,
            };
            assert_eq!(adv.choose(&c, &mut rng), 0);
        }
    }
}
