//! The injected deterministic environment of the Store API ([API §6]): the wall and boot clocks with the boot
//! identity (`EnvClock`, §6.2), stamps and deadlines ([OS/clock §3]–§4), the store HLC ([API §6.2] CK-4, [OS/clock §7])
//! and the liveness-slot table (`EnvSlots`, §6.3) with the lock-anchored liveness procedure ([OS/proc §6.2]) over it.
//!
//! Every value an implementation would otherwise read from the machine comes from here (DT-3): no function of the
//! model reads the OS clock, the OS random source or the process environment.

use crate::value::blake3_128;
use std::collections::BTreeMap;

/// The boot identity of the simulated machine ([API §6.2] `boot`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Boot {
    /// A known boot, numbered from 1.
    Known(u64),
    /// Unknown-boot mode ([OS/proc §5]).
    Unknown,
}

/// A stamp: wall clock, boot hash and boot clock ([OS/clock §3.1]); a lease deadline is one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct Stamp {
    /// Wall clock in ms since the Unix epoch; a negative wall is 0.
    pub wall: u64,
    /// The writer's `boot_hash`; 0 in Unknown-boot mode.
    pub boot_hash: u64,
    /// The boot clock in ns; 0 when `boot_hash` is 0.
    pub boot_ns: u64,
}

impl Stamp {
    /// `Stamp::NEVER` ([OS/clock §4.1]): the deadline of a run-scoped lease.
    pub const NEVER: Stamp = Stamp {
        wall: u64::MAX,
        boot_hash: 0,
        boot_ns: u64::MAX,
    };
}

/// The evaluation of a deadline ([OS/clock §4.3]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Deadline {
    /// Not passed.
    NotPassed,
    /// Passed.
    Passed,
    /// Written in another boot.
    BootChanged,
}

/// `boot_id` of the simulated boot k: `BLAKE3-128(lp("moirai-boot-id-v1") ‖ lp("api-sim-boot") ‖ lp(u64le(k)))`
/// ([API §6.2] CK-2).
// spec: [API §6.2] CK-2
pub fn boot_id(k: u64) -> [u8; 16] {
    blake3_128(&[b"moirai-boot-id-v1", b"api-sim-boot", &k.to_le_bytes()])
}

/// `boot_hash(boot_id) = u64::from_le_bytes(boot_id[0..8]) | 1` ([OS/proc §4.3]).
// spec: [OS/proc §4.3]
pub fn boot_hash(id: &[u8; 16]) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&id[..8]);
    u64::from_le_bytes(b) | 1
}

/// `hlc_next(wall_ms, h) = max((max(0, wall_ms) as u64) << 16, h + 1)` ([OS/clock §7]).
// spec: [OS/clock §7]
pub fn hlc_next(wall_ms: i64, h: u64) -> u64 {
    ((wall_ms.max(0) as u64) << 16).max(h + 1)
}

/// Milliseconds of a duration, rounded up; nanoseconds, both saturating ([OS/clock §4.2]).
fn ttl_ms_ns(ttl_ms: u64) -> (u64, u64) {
    (ttl_ms, ttl_ms.saturating_mul(1_000_000))
}

/// `after(now, ttl)` ([OS/clock §4.2]).
// spec: [OS/clock §4.2]
pub fn after(now: Stamp, ttl_ms: u64) -> Stamp {
    let (ms, ns) = ttl_ms_ns(ttl_ms);
    Stamp {
        wall: now.wall.saturating_add(ms),
        boot_hash: now.boot_hash,
        boot_ns: if now.boot_hash != 0 {
            now.boot_ns.saturating_add(ns)
        } else {
            0
        },
    }
}

/// `state(d, now)` ([OS/clock §4.3]), first matching row.
// spec: [OS/clock §4.3]
pub fn deadline_state(d: Stamp, now: Stamp) -> Deadline {
    if d.boot_hash != 0 && now.boot_hash != 0 && d.boot_hash == now.boot_hash {
        if now.boot_ns >= d.boot_ns {
            Deadline::Passed
        } else {
            Deadline::NotPassed
        }
    } else if d.boot_hash != 0 && now.boot_hash != 0 {
        Deadline::BootChanged
    } else if now.wall >= d.wall {
        Deadline::Passed
    } else {
        Deadline::NotPassed
    }
}

/// `due_for_renewal(d, ttl, now)`: more than half the TTL has elapsed ([OS/clock §4.4]).
// spec: [OS/clock §4.4]
pub fn due_for_renewal(d: Stamp, ttl_ms: u64, now: Stamp) -> bool {
    let (ms, ns) = ttl_ms_ns(ttl_ms);
    if d.boot_hash != 0 && d.boot_hash == now.boot_hash {
        d.boot_ns.saturating_sub(now.boot_ns) < ns / 2
    } else {
        d.wall.saturating_sub(now.wall) < ms / 2
    }
}

/// One held liveness slot ([API §6.3]): the session identity and its `/clear` alias.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slot {
    /// The primary identity `claude:<id>` or `codex:<id>`.
    pub identity: String,
    /// The alias identity, never matched for an anchor.
    pub alias: Option<String>,
}

/// The answer of the lock-anchored liveness procedure ([OS/proc §6.2]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Liveness {
    /// The anchor names a held slot.
    Alive,
    /// No held slot names it, or the boot differs.
    Dead,
    /// `LOCK` cannot be read.
    Unknown,
}

/// The injected environment ([API §6.1]).
#[derive(Clone, Debug)]
pub struct Env {
    /// `wall_ms`.
    pub wall_ms: i64,
    /// The boot number k of the machine; the boot the clock reads is [`Env::boot`].
    pub boot_no: u64,
    /// Unknown-boot mode.
    pub unknown: bool,
    /// `boot_ns`.
    pub boot_ns: u64,
    /// The held slots by primary identity, at most 256.
    pub slots: BTreeMap<String, Slot>,
    /// Whether `LOCK` is readable.
    pub readable: bool,
}

impl Default for Env {
    /// The start of a stream ([API §6.2]): `wall_ms` = 1,790,000,000,000, `Known(1)`, `boot_ns` = 10^9; no slot.
    fn default() -> Env {
        Env {
            wall_ms: 1_790_000_000_000,
            boot_no: 1,
            unknown: false,
            boot_ns: 1_000_000_000,
            slots: BTreeMap::new(),
            readable: true,
        }
    }
}

/// The arguments of `EnvClock` ([API §6.2]), applied in this order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvClock {
    /// `set_wall_ms`.
    pub set_wall_ms: Option<i64>,
    /// `step_ms`, signed.
    pub step_ms: Option<i64>,
    /// `advance_ms` ≥ 0.
    pub advance_ms: Option<u64>,
    /// `reboot`.
    pub reboot: bool,
    /// `boot_mode`: `Some(true)` for `"unknown"`, `Some(false)` for `"known"`.
    pub boot_unknown: Option<bool>,
}

/// The arguments of `EnvSlots` ([API §6.3]), applied in this order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvSlots {
    /// `release`.
    pub release: Vec<String>,
    /// `hold`.
    pub hold: Vec<String>,
    /// `alias`: (primary, alias) pairs.
    pub alias: Vec<(String, String)>,
    /// `readable`.
    pub readable: Option<bool>,
}

/// The data of an `EnvSlots` result: `held`, `unheld`, `readable` ([API §6.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotsResult {
    /// The held identities, bytewise.
    pub held: Vec<String>,
    /// The identities a `hold` could not seat (SL-4).
    pub unheld: Vec<String>,
    /// Whether `LOCK` is readable.
    pub readable: bool,
}

/// The slot capacity of `LOCK` ([API §6.3] SL-4).
pub const SLOTS: usize = 256;

impl Env {
    /// The boot the clock reads: `Known(k)`, or `Unknown` in Unknown-boot mode.
    pub fn boot(&self) -> Boot {
        if self.unknown {
            Boot::Unknown
        } else {
            Boot::Known(self.boot_no)
        }
    }

    /// The current `boot_hash`, 0 in Unknown-boot mode.
    pub fn boot_hash(&self) -> u64 {
        match self.boot() {
            Boot::Known(k) => boot_hash(&boot_id(k)),
            Boot::Unknown => 0,
        }
    }

    /// The stamp `now` over (`wall_ms`, `boot`, `boot_ns`) ([API §6.2] CK-3, [OS/clock §3.2]).
    // spec: [API §6.2] CK-3
    pub fn now(&self) -> Stamp {
        let known = !self.unknown;
        Stamp {
            wall: self.wall_ms.max(0) as u64,
            boot_hash: self.boot_hash(),
            boot_ns: if known { self.boot_ns } else { 0 },
        }
    }

    /// The wall clock in whole seconds, for `defer_until ≤ now()` ([API §6.2] CK-7).
    // spec: [API §6.2] CK-7
    pub fn now_s(&self) -> i64 {
        self.wall_ms.div_euclid(1000)
    }

    /// Applies `EnvClock` ([API §6.2]).
    // spec: [API §6.2]
    pub fn clock(&mut self, a: &EnvClock) {
        if let Some(w) = a.set_wall_ms {
            self.wall_ms = w;
        }
        if let Some(s) = a.step_ms {
            self.wall_ms = self.wall_ms.saturating_add(s);
        }
        if let Some(d) = a.advance_ms {
            self.wall_ms = self.wall_ms.saturating_add(d as i64);
            if !self.unknown {
                self.boot_ns = self.boot_ns.saturating_add(d.saturating_mul(1_000_000));
            }
        }
        if a.reboot {
            self.boot_no += 1;
            self.boot_ns = 1_000_000_000;
            self.slots.clear();
        }
        if let Some(u) = a.boot_unknown {
            self.unknown = u;
        }
    }

    /// Applies `EnvSlots` ([API §6.3]): `release`, `hold`, `alias`, `readable`; a `hold` beyond 256 slots is unheld
    /// (SL-4).
    // spec: [API §6.3] SL-4
    pub fn slots(&mut self, a: &EnvSlots) -> SlotsResult {
        for r in &a.release {
            self.slots.remove(r);
        }
        let mut unheld = Vec::new();
        for h in &a.hold {
            if self.slots.contains_key(h) {
                continue;
            }
            if self.slots.len() >= SLOTS {
                unheld.push(h.clone());
            } else {
                self.slots.insert(
                    h.clone(),
                    Slot {
                        identity: h.clone(),
                        alias: None,
                    },
                );
            }
        }
        for (p, al) in &a.alias {
            if let Some(s) = self.slots.get_mut(p) {
                s.alias = Some(al.clone());
            }
        }
        if let Some(r) = a.readable {
            self.readable = r;
        }
        unheld.sort();
        SlotsResult {
            held: self.slots.keys().cloned().collect(),
            unheld,
            readable: self.readable,
        }
    }

    /// Ends every simulated process: the slot table is emptied ([API §6.7] `between`).
    pub fn crash(&mut self) {
        self.slots.clear();
    }

    /// Whether a session holds a slot now.
    pub fn holds_slot(&self, session: &str) -> bool {
        self.slots.contains_key(session)
    }

    /// The liveness procedure of [OS/proc §6.2] for a `session` or `session-ttl` anchor (steps B–D) over this table
    /// ([API §6.3] SL-2): step B with the current boot, step C with `readable`, step D matching the anchor's session
    /// against the primary identities of held slots, never the alias.
    // spec: [API §6.3] SL-2
    // spec: [OS/proc §6.2]
    pub fn anchor_liveness(&self, session: &str, anchor_boot_hash: u64) -> Liveness {
        if let Boot::Known(k) = self.boot()
            && anchor_boot_hash != 0
            && anchor_boot_hash != boot_hash(&boot_id(k))
        {
            return Liveness::Dead;
        }
        if !self.readable {
            return Liveness::Unknown;
        }
        if self.slots.contains_key(session) {
            Liveness::Alive
        } else {
            Liveness::Dead
        }
    }
}

/// The store's HLC sequence ([API §6.2] CK-4): the greatest value the sequence produced (`hlc_seq`) and the greatest
/// `hlc` of any commit the store holds (`hlc_commit`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hlc {
    /// `HEAD.hlc_seq`.
    pub seq: u64,
    /// `HEAD.hlc_commit`.
    pub commit: u64,
}

impl Hlc {
    /// The HLC of the next semantic durable record: `hlc_next(wall_ms, hlc_seq)`, which raises the sequence (CK-4).
    // spec: [API §6.2] CK-4
    pub fn record(&mut self, wall_ms: i64) -> u64 {
        let h = hlc_next(wall_ms, self.seq);
        self.seq = h;
        h
    }

    /// The HLC of a local commit: drawn from `max(hlc_seq, hlc_commit)`; it is also the commit's `append_hlc` (CK-4).
    // spec: [API §6.2] CK-4
    pub fn commit(&mut self, wall_ms: i64) -> u64 {
        let h = hlc_next(wall_ms, self.seq.max(self.commit));
        self.seq = h;
        self.commit = self.commit.max(h);
        h
    }

    /// `now_ms` of a retention window: `max(wall_ms, h >> 16)` over the greatest HLC seen (CK-6).
    // spec: [API §6.2] CK-6
    pub fn now_ms(&self, wall_ms: i64) -> u64 {
        (wall_ms.max(0) as u64).max(self.seq.max(self.commit) >> 16)
    }

    /// Whether a window of `window_ms` opened by the HLC `t` is still open: `now_ms − (t >> 16) < window_ms` (CK-6).
    // spec: [API §6.2] CK-6
    pub fn within(&self, wall_ms: i64, t: u64, window_ms: u64) -> bool {
        self.now_ms(wall_ms).saturating_sub(t >> 16) < window_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_starts_at_the_initial_clock() {
        let e = Env::default();
        let now = e.now();
        assert_eq!(now.wall, 1_790_000_000_000);
        assert_eq!(now.boot_ns, 1_000_000_000);
        assert_ne!(now.boot_hash & 1, 0);
    }

    #[test]
    fn a_wall_step_changes_no_expiry_but_elapsed_time_does() {
        let mut e = Env::default();
        let d = after(e.now(), 15 * 60_000);
        e.clock(&EnvClock {
            step_ms: Some(3_600_000),
            ..Default::default()
        });
        assert_eq!(deadline_state(d, e.now()), Deadline::NotPassed);
        e.clock(&EnvClock {
            advance_ms: Some(15 * 60_000),
            ..Default::default()
        });
        assert_eq!(deadline_state(d, e.now()), Deadline::Passed);
        e.clock(&EnvClock {
            reboot: true,
            ..Default::default()
        });
        assert_eq!(deadline_state(d, e.now()), Deadline::BootChanged);
        assert_eq!(deadline_state(Stamp::NEVER, e.now()), Deadline::NotPassed);
    }

    #[test]
    fn the_hlc_never_decreases() {
        let mut h = Hlc::default();
        let a = h.commit(1000);
        let b = h.record(10);
        let c = h.commit(999);
        assert!(a < b && b < c);
        assert_eq!(a, 1000 << 16);
    }

    #[test]
    fn renewal_is_due_after_half_the_ttl() {
        let mut e = Env::default();
        let d = after(e.now(), 1000);
        assert!(!due_for_renewal(d, 1000, e.now()));
        e.clock(&EnvClock {
            advance_ms: Some(501),
            ..Default::default()
        });
        assert!(due_for_renewal(d, 1000, e.now()));
    }

    #[test]
    fn slots_hold_release_and_judge_anchors() {
        let mut e = Env::default();
        let r = e.slots(&EnvSlots {
            hold: vec!["claude:s1".into()],
            alias: vec![("claude:s1".into(), "claude:s2".into())],
            ..Default::default()
        });
        assert_eq!(r.held, vec!["claude:s1".to_string()]);
        let bh = e.boot_hash();
        assert_eq!(e.anchor_liveness("claude:s1", bh), Liveness::Alive);
        assert_eq!(
            e.anchor_liveness("claude:s2", bh),
            Liveness::Dead,
            "never the alias"
        );
        e.slots(&EnvSlots {
            readable: Some(false),
            ..Default::default()
        });
        assert_eq!(e.anchor_liveness("claude:s1", bh), Liveness::Unknown);
        e.clock(&EnvClock {
            reboot: true,
            ..Default::default()
        });
        assert_eq!(e.anchor_liveness("claude:s1", bh), Liveness::Dead);
    }
}
