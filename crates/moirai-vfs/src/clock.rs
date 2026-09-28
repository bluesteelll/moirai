//! The three clocks, stamps, deadlines and the HLC clock rule ([OS/README §4.4], [OS/clock §2–§4, §7, §10]; X-F2).
//!
//! | Clock | Unit and origin | Guarantee | Used for |
//! |---|---|---|---|
//! | wall | ms since the Unix epoch, UTC | none: may step backward or forward between two calls ([F15 §3.7] FM-7.1) | the HLC's physical input; the wall part of a stamp; displayed times |
//! | mono | ns from an origin fixed for one boot | never decreases within a boot; comparable across the processes of a boot; may exclude suspend (FM-7.2) | in-process intervals: lock waits, retry bounds, budgets |
//! | boot | ns since the kernel booted | never decreases within a boot; includes suspend and hibernation (FM-7.3) | the boot part of a stamp: lease deadlines, cross-process grace |
//!
//! Only [`Clock`] reaches the OS; every other item here is a pure function. The mono clock is never stored; every stored
//! boot-clock value carries the writer's `boot_hash` beside it, so a reader of another boot, or in Unknown-boot mode,
//! judges it by the wall clock instead ([OS/clock §3, §4]).

use core::time::Duration;

use crate::proc::BootIdentity;

/// The clock sub-trait of `Vfs` ([OS/README §4.4]; semantics and per-OS sources [OS/clock §2]).
pub trait Clock {
    /// Wall clock: milliseconds since the Unix epoch, UTC. May step backward or forward between two calls.
    fn wall_ms(&self) -> i64;
    /// Monotonic clock in nanoseconds from an unspecified origin; never goes backward within a process (and, as
    /// [OS/clock §2] adopts, within one boot).
    fn mono_ns(&self) -> u64;
    /// Boot clock in nanoseconds since boot: monotonic and including time spent in suspend ([80 §2.7.1]).
    fn boot_ns(&self) -> u64;
}

/// The 24-byte stamp and deadline form ([OS/clock §3.1]): the lease deadline `expires = {wall, boot_hash, mono}` of
/// [80 §3.1] X-F2, "mono being the boot clock". Little-endian, packed: `wall` at 0, `boot_hash` at 8, `boot_ns` at 16.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Stamp {
    /// Wall clock in ms since the Unix epoch; a negative `wall_ms` is stored as 0.
    pub wall: u64,
    /// `boot_hash` of the writer's boot ([OS/proc §4.3]); 0 = the writer was in Unknown-boot mode.
    pub boot_hash: u64,
    /// The boot clock in ns; 0 when `boot_hash` is 0.
    pub boot_ns: u64,
}

/// The state of a deadline ([OS/clock §4.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DeadlineState {
    /// The deadline has not been reached.
    NotPassed,
    /// The deadline has passed.
    Passed,
    /// The deadline was set in another boot: every non-run-scoped lease is Dead ([AR §6.2]).
    BootChanged,
}

/// A duration in whole milliseconds, rounded up and saturating: a deadline is never earlier, and a grace never shorter,
/// than the duration asks. [OS/clock §4.2, §4.4, §4.5] treat `ttl_ms` and `ttl_ns` (and `g_ms`, `g_ns`) as "the same
/// duration in the two units" without saying how a duration that is not a whole number of milliseconds rounds; this
/// crate reads it as `ttl_ms = ⌈ttl / 1 ms⌉` and `ttl_ns = ttl` in ns, both saturating at `u64::MAX` (a spec finding of
/// WP-30, for WP-80a). The rounding is byte-visible in stored deadlines.
fn ms_ceil(d: Duration) -> u64 {
    let ms = d.as_nanos().div_ceil(1_000_000);
    u64::try_from(ms).unwrap_or(u64::MAX)
}

/// A duration in nanoseconds, saturating.
fn ns_sat(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

impl Stamp {
    /// The encoded length.
    pub const LEN: usize = 24;

    /// The never-expiring deadline of a run-scoped lease ([OS/clock §4.1]): judged by the wall clock, which never reaches
    /// `u64::MAX`, so it neither expires nor turns Dead at a boot change.
    pub const NEVER: Stamp = Stamp {
        wall: u64::MAX,
        boot_hash: 0,
        boot_ns: u64::MAX,
    };

    /// Now ([OS/clock §3.2]): the wall clock (negative values as 0) and, on a known boot, its hash and the boot clock.
    pub fn now<C: Clock + ?Sized>(clock: &C, boot: &BootIdentity) -> Stamp {
        let wall = u64::try_from(clock.wall_ms()).unwrap_or(0);
        match boot {
            BootIdentity::Known(b) => Stamp {
                wall,
                boot_hash: b.hash(),
                boot_ns: clock.boot_ns(),
            },
            BootIdentity::Unknown(_) => Stamp {
                wall,
                boot_hash: 0,
                boot_ns: 0,
            },
        }
    }

    /// The deadline `ttl` after `self` ([OS/clock §4.2]); saturating.
    pub fn after(&self, ttl: Duration) -> Stamp {
        Stamp {
            wall: self.wall.saturating_add(ms_ceil(ttl)),
            boot_hash: self.boot_hash,
            boot_ns: if self.boot_hash != 0 {
                self.boot_ns.saturating_add(ns_sat(ttl))
            } else {
                0
            },
        }
    }

    /// Evaluates the deadline `self` at `now` ([OS/clock §4.3], frozen with X-F2): by the boot clock on the same known
    /// boot, `BootChanged` across two known boots, otherwise by the wall clock.
    pub fn state(&self, now: &Stamp) -> DeadlineState {
        let (d, n) = (self, now);
        if d.boot_hash != 0 && n.boot_hash != 0 {
            if d.boot_hash == n.boot_hash {
                if n.boot_ns >= d.boot_ns {
                    DeadlineState::Passed
                } else {
                    DeadlineState::NotPassed
                }
            } else {
                DeadlineState::BootChanged
            }
        } else if n.wall >= d.wall {
            DeadlineState::Passed
        } else {
            DeadlineState::NotPassed
        }
    }

    /// The half-TTL renewal rule ([OS/clock §4.4]): `self` is the current deadline; `true` when less than half of `ttl`
    /// remains.
    pub fn due_for_renewal(&self, ttl: Duration, now: &Stamp) -> bool {
        if self.boot_hash != 0 && self.boot_hash == now.boot_hash {
            self.boot_ns.saturating_sub(now.boot_ns) < ns_sat(ttl) / 2
        } else {
            self.wall.saturating_sub(now.wall) < ms_ceil(ttl) / 2
        }
    }

    /// `true` when at least `g` has elapsed from `self` (a stamp another process wrote) to `now` ([OS/clock §4.5]): by the
    /// boot clock on one known boot; always across two known boots; otherwise by the wall clock (saturating, so a
    /// backward step yields 0, never a negative interval).
    pub fn elapsed_at_least(&self, g: Duration, now: &Stamp) -> bool {
        if self.boot_hash != 0 && now.boot_hash != 0 {
            if self.boot_hash == now.boot_hash {
                now.boot_ns.saturating_sub(self.boot_ns) >= ns_sat(g)
            } else {
                true
            }
        } else {
            now.wall.saturating_sub(self.wall) >= ms_ceil(g)
        }
    }

    /// Encodes [OS/clock §3.1].
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut b = [0u8; 24];
        b[..8].copy_from_slice(&self.wall.to_le_bytes());
        b[8..16].copy_from_slice(&self.boot_hash.to_le_bytes());
        b[16..].copy_from_slice(&self.boot_ns.to_le_bytes());
        b
    }

    /// Decodes [OS/clock §3.1]; every bit pattern is a value.
    pub fn from_bytes(b: &[u8; 24]) -> Stamp {
        let word = |at: usize| {
            let mut x = [0u8; 8];
            x.copy_from_slice(&b[at..at + 8]);
            u64::from_le_bytes(x)
        };
        Stamp {
            wall: word(0),
            boot_hash: word(8),
            boot_ns: word(16),
        }
    }
}

/// The next HLC after `last` from the wall clock ([OS/clock §7]): `max(max(0, wall_ms) << 16, last + 1)`, where `last` is
/// the greatest `hlc` (for `append_hlc`: the greatest `append_hlc`) in the log as scanned under the writer byte. The HLC
/// never decreases whatever the wall clock does; saturating (a wall value of 2^48 ms or more is taken as the largest
/// representable millisecond).
pub fn hlc_next(wall_ms: i64, last: u64) -> u64 {
    let ms = u64::try_from(wall_ms).unwrap_or(0).min(u64::MAX >> 16);
    (ms << 16).max(last.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::{BootId, UnknownBoot};
    use proptest::prelude::*;

    struct Fixed {
        wall: i64,
        boot: u64,
    }

    impl Clock for Fixed {
        fn wall_ms(&self) -> i64 {
            self.wall
        }
        fn mono_ns(&self) -> u64 {
            self.boot / 2
        }
        fn boot_ns(&self) -> u64 {
            self.boot
        }
    }

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn now_and_after() {
        let known = BootIdentity::Known(BootId([2; 16]));
        let s = Stamp::now(
            &Fixed {
                wall: -5,
                boot: 1_000,
            },
            &known,
        );
        assert_eq!(
            s,
            Stamp {
                wall: 0,
                boot_hash: BootId([2; 16]).hash(),
                boot_ns: 1_000
            }
        );
        let u = Stamp::now(
            &Fixed {
                wall: 77,
                boot: 1_000,
            },
            &BootIdentity::Unknown(UnknownBoot::Denied),
        );
        assert_eq!(
            u,
            Stamp {
                wall: 77,
                boot_hash: 0,
                boot_ns: 0
            }
        );
        assert_eq!(s.after(MIN).boot_ns, 1_000 + 60_000_000_000);
        assert_eq!(
            u.after(MIN),
            Stamp {
                wall: 60_077,
                boot_hash: 0,
                boot_ns: 0
            }
        );
        assert_eq!(
            u.after(Duration::from_nanos(1)).wall,
            78,
            "a partial millisecond rounds up"
        );
        // Row 3 of §4.3 judges an unknown-boot deadline by the wall clock alone, so the boot part is 0 (§4.2).
        let never = Stamp::NEVER.after(MIN);
        assert_eq!(
            never,
            Stamp {
                wall: u64::MAX,
                boot_hash: 0,
                boot_ns: 0
            }
        );
        assert_eq!(
            never.state(&Stamp {
                wall: u64::MAX - 1,
                boot_hash: 0x11,
                boot_ns: 5
            }),
            DeadlineState::NotPassed
        );
    }

    #[test]
    fn deadline_rows() {
        let a = 0x11;
        let b = 0x23;
        let d = Stamp {
            wall: 1_000,
            boot_hash: a,
            boot_ns: 500,
        };
        // Row 1: the same known boot decides by the boot clock, whatever the wall clock says.
        assert_eq!(
            d.state(&Stamp {
                wall: 0,
                boot_hash: a,
                boot_ns: 500
            }),
            DeadlineState::Passed
        );
        assert_eq!(
            d.state(&Stamp {
                wall: 9_999,
                boot_hash: a,
                boot_ns: 499
            }),
            DeadlineState::NotPassed
        );
        // Row 2: another known boot.
        assert_eq!(
            d.state(&Stamp {
                wall: 0,
                boot_hash: b,
                boot_ns: 0
            }),
            DeadlineState::BootChanged
        );
        // Row 3: either side unknown decides by the wall clock.
        assert_eq!(
            d.state(&Stamp {
                wall: 1_000,
                boot_hash: 0,
                boot_ns: 0
            }),
            DeadlineState::Passed
        );
        assert_eq!(
            d.state(&Stamp {
                wall: 999,
                boot_hash: 0,
                boot_ns: 0
            }),
            DeadlineState::NotPassed
        );
        let du = Stamp {
            wall: 1_000,
            boot_hash: 0,
            boot_ns: 0,
        };
        assert_eq!(
            du.state(&Stamp {
                wall: 1_000,
                boot_hash: a,
                boot_ns: 0
            }),
            DeadlineState::Passed
        );
        // The never-expiring deadline.
        assert_eq!(
            Stamp::NEVER.state(&Stamp {
                wall: u64::MAX - 1,
                boot_hash: b,
                boot_ns: u64::MAX
            }),
            DeadlineState::NotPassed
        );
    }

    #[test]
    fn renewal_and_grace() {
        let a = 0x11;
        let now = Stamp {
            wall: 10_000,
            boot_hash: a,
            boot_ns: 1_000_000_000,
        };
        let d = now.after(MIN);
        assert!(!d.due_for_renewal(MIN, &now));
        let later = Stamp {
            wall: 10_000,
            boot_hash: a,
            boot_ns: 1_000_000_000 + 31_000_000_000,
        };
        assert!(
            d.due_for_renewal(MIN, &later),
            "a wall step changes nothing on a known boot"
        );
        let du = Stamp {
            wall: 70_000,
            boot_hash: 0,
            boot_ns: 0,
        };
        assert!(!du.due_for_renewal(
            MIN,
            &Stamp {
                wall: 40_000,
                boot_hash: 0,
                boot_ns: 0
            }
        ));
        assert!(du.due_for_renewal(
            MIN,
            &Stamp {
                wall: 40_001,
                boot_hash: 0,
                boot_ns: 0
            }
        ));
        assert!(!Stamp::NEVER.due_for_renewal(MIN, &now));
        // Grace.
        let start = Stamp {
            wall: 5_000,
            boot_hash: a,
            boot_ns: 0,
        };
        assert!(!start.elapsed_at_least(
            MIN,
            &Stamp {
                wall: 999_999,
                boot_hash: a,
                boot_ns: 59_999_999_999
            }
        ));
        assert!(start.elapsed_at_least(
            MIN,
            &Stamp {
                wall: 0,
                boot_hash: a,
                boot_ns: 60_000_000_000
            }
        ));
        assert!(
            start.elapsed_at_least(
                MIN,
                &Stamp {
                    wall: 0,
                    boot_hash: 0x99,
                    boot_ns: 0
                }
            ),
            "another boot"
        );
        let su = Stamp {
            wall: 5_000,
            boot_hash: 0,
            boot_ns: 0,
        };
        assert!(!su.elapsed_at_least(
            MIN,
            &Stamp {
                wall: 64_999,
                boot_hash: a,
                boot_ns: 0
            }
        ));
        assert!(su.elapsed_at_least(
            MIN,
            &Stamp {
                wall: 65_000,
                boot_hash: a,
                boot_ns: 0
            }
        ));
        assert!(
            !su.elapsed_at_least(
                MIN,
                &Stamp {
                    wall: 0,
                    boot_hash: 0,
                    boot_ns: 0
                }
            ),
            "a backward step"
        );
    }

    #[test]
    fn hlc_rule() {
        assert_eq!(hlc_next(1, 0), 1 << 16);
        assert_eq!(
            hlc_next(1, 1 << 16),
            (1 << 16) + 1,
            "a stalled or backward clock advances the counter"
        );
        assert_eq!(hlc_next(-7, 5), 6);
        assert_eq!(hlc_next(i64::MAX, 0), (u64::MAX >> 16) << 16);
        assert_eq!(hlc_next(0, u64::MAX), u64::MAX);
    }

    proptest! {
        #![proptest_config(crate::testing::proptest_config())]

        #[test]
        fn stamp_round_trips(wall in any::<u64>(), boot_hash in any::<u64>(), boot_ns in any::<u64>()) {
            let s = Stamp { wall, boot_hash, boot_ns };
            prop_assert_eq!(Stamp::from_bytes(&s.to_bytes()), s);
        }

        /// The HLC never decreases, whatever the wall clock does.
        #[test]
        fn hlc_is_monotonic(walls in proptest::collection::vec(any::<i64>(), 1..40)) {
            let mut last = 0u64;
            for w in walls {
                let next = hlc_next(w, last);
                prop_assert!(next > last || last == u64::MAX);
                last = next;
            }
        }

        /// A deadline on a known boot is immune to wall steps.
        #[test]
        fn wall_steps_do_not_move_known_boot_deadlines(wall_a in any::<u64>(), wall_b in any::<u64>(),
                                                       start in 0u64..1 << 40, ttl_ms in 0u64..1 << 12, elapsed in 0u64..1 << 33) {
            let h = 0x55;
            let d = Stamp { wall: wall_a, boot_hash: h, boot_ns: start }.after(Duration::from_millis(ttl_ms));
            let now = Stamp { wall: wall_b, boot_hash: h, boot_ns: start + elapsed };
            let expected = if elapsed >= ttl_ms * 1_000_000 { DeadlineState::Passed } else { DeadlineState::NotPassed };
            prop_assert_eq!(d.state(&now), expected);
        }
    }
}
