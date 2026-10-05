//! The enumerator's adversary: the world's seeded adversary, with one fault injected at the n-th choice of a site and
//! with a policy for the reads of poisoned sectors ([F15 §1.3]: "In an enumeration run, the crash enumerator drives the
//! same choices").
//!
//! It always asks the seeded adversary first, so the generator's stream — and with it every choice before the injection —
//! is exactly that of the discovery run; the n-th choice of a site is then the same call in both runs. It also counts
//! the choices of every site (the discovery run's counts are the injection points of every later run), notes the
//! scheduling point of the injection (from the `SystemCrash` choice every scheduling point asks), and counts the reads
//! of poisoned sectors.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use crate::adversary::{Adversary, Choice, SeededAdversary, Site};
use crate::rng::Rng;

/// How reads of a poisoned sector choose among its candidates (FM-3.2; [80 §2.3.4]: a btrfs revert, a macOS
/// invalidation, an ext4 or XFS eviction of clean-but-unwritten pages), and how a re-write fixes its unwritten
/// sub-sectors (FM-3.5).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PoisonPolicy {
    /// The world's seeded adversary decides.
    Seeded,
    /// Always the oldest candidate: the pages reverted (btrfs) or were evicted before write-back (ext4, XFS).
    Revert,
    /// Always the newest candidate: the bytes look written although the flush failed.
    Newest,
    /// Oldest and newest in turn, sub-sector by sub-sector and read by read: the pages change between reads and within
    /// one (macOS invalidation).
    Alternate,
    /// The newest candidate on the first read of each sub-sector, the oldest ever after: the new bytes stayed in pages
    /// marked clean until memory pressure (or `POSIX_FADV_DONTNEED`) evicted them (ext4, XFS).
    Evict,
}

/// One fault injected at the `nth` choice (0-based) of `site`, answered `value`; `follow` answers the next choice of
/// another site by the same process (the partial application of a failed write, FM-5.2).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Injection {
    pub(crate) site: Site,
    pub(crate) nth: u64,
    pub(crate) value: u64,
    pub(crate) follow: Option<(Site, u64)>,
}

/// What an [`EnumAdversary`] saw.
#[derive(Clone, Debug, Default)]
pub(crate) struct AdvStats {
    /// Choices per site.
    pub(crate) counts: BTreeMap<Site, u64>,
    /// The scheduling point of the injection, if it happened.
    pub(crate) injected_at: Option<u64>,
    /// The node the injected choice concerned (for a write fault, the written file), if it happened.
    pub(crate) injected_node: Option<u64>,
    /// Reads of poisoned sub-sectors.
    pub(crate) poisoned_reads: u64,
}

pub(crate) type SharedStats = Arc<Mutex<AdvStats>>;

pub(crate) fn lock_stats(s: &SharedStats) -> std::sync::MutexGuard<'_, AdvStats> {
    s.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The enumerator's adversary (see the module documentation).
pub(crate) struct EnumAdversary {
    inner: SeededAdversary,
    poison: PoisonPolicy,
    inject: Option<Injection>,
    follow: Option<(Site, u32, u64)>,
    alternate: bool,
    /// The sub-sectors (node, index) read or merged once, for [`PoisonPolicy::Evict`].
    evicted: BTreeSet<(u64, u64)>,
    last_point: u64,
    stats: SharedStats,
}

impl EnumAdversary {
    pub(crate) fn new(
        inner: SeededAdversary,
        poison: PoisonPolicy,
        inject: Option<Injection>,
        stats: SharedStats,
    ) -> EnumAdversary {
        EnumAdversary {
            inner,
            poison,
            inject,
            follow: None,
            alternate: false,
            evicted: BTreeSet::new(),
            last_point: 0,
            stats,
        }
    }
}

impl Adversary for EnumAdversary {
    fn choose(&mut self, c: &Choice, rng: &mut Rng) -> u64 {
        let seeded = self.inner.choose(c, rng);
        if c.site == Site::SystemCrash {
            self.last_point = c.aux;
        }
        let mut st = lock_stats(&self.stats);
        let n = st.counts.entry(c.site).or_insert(0);
        let nth = *n;
        *n += 1;
        if let Some(inj) = self.inject
            && inj.site == c.site
            && inj.nth == nth
        {
            st.injected_at = Some(self.last_point);
            st.injected_node = Some(c.node);
            self.follow = inj.follow.map(|(s, v)| (s, c.proc, v));
            return inj.value;
        }
        if let Some((s, p, v)) = self.follow
            && s == c.site
            && p == c.proc
        {
            self.follow = None;
            return v;
        }
        match c.site {
            Site::PoisonRead | Site::RewriteMerge => {
                if c.site == Site::PoisonRead {
                    st.poisoned_reads += 1;
                }
                let last = c.arity.saturating_sub(1);
                match self.poison {
                    PoisonPolicy::Seeded => seeded,
                    PoisonPolicy::Revert => 0,
                    PoisonPolicy::Newest => last,
                    PoisonPolicy::Alternate => {
                        self.alternate = !self.alternate;
                        if self.alternate { 0 } else { last }
                    }
                    PoisonPolicy::Evict => {
                        if self.evicted.insert((c.node, c.aux)) {
                            last
                        } else {
                            0
                        }
                    }
                }
            }
            _ => seeded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adversary::{FaultRates, ReleaseDelayLaw};

    fn choice(site: Site, proc: u32, arity: u64) -> Choice {
        Choice {
            site,
            proc,
            node: 1,
            aux: 0,
            arity,
        }
    }

    #[test]
    fn injects_at_the_nth_choice_and_keeps_the_stream() {
        let stats = SharedStats::default();
        let seeded = || SeededAdversary::new(FaultRates::default(), ReleaseDelayLaw::default());
        let inj = Injection {
            site: Site::WriteFault,
            nth: 1,
            value: 1,
            follow: Some((Site::PartialWrite, 7)),
        };
        let mut a = EnumAdversary::new(seeded(), PoisonPolicy::Seeded, Some(inj), stats.clone());
        let mut b = seeded();
        let (mut ra, mut rb) = (Rng::new(5), Rng::new(5));
        let w = choice(Site::WriteFault, 3, 3);
        assert_eq!(a.choose(&w, &mut ra), b.choose(&w, &mut rb));
        assert_eq!(a.choose(&w, &mut ra), 1, "the second write fails");
        b.choose(&w, &mut rb);
        assert_eq!(
            a.choose(&choice(Site::PartialWrite, 3, u64::MAX), &mut ra),
            7
        );
        b.choose(&choice(Site::PartialWrite, 3, u64::MAX), &mut rb);
        assert_eq!(ra, rb, "the generator stream is the seeded adversary's");
        assert_eq!(lock_stats(&stats).counts[&Site::WriteFault], 2);
        assert_eq!(
            lock_stats(&stats).injected_node,
            Some(1),
            "the written file's node"
        );
    }

    #[test]
    fn poison_policies_revert_renew_and_alternate() {
        let stats = SharedStats::default();
        let seeded = || SeededAdversary::new(FaultRates::default(), ReleaseDelayLaw::default());
        let mut rng = Rng::new(1);
        let p = choice(Site::PoisonRead, 0, 3);
        let mut rev = EnumAdversary::new(seeded(), PoisonPolicy::Revert, None, stats.clone());
        let mut new = EnumAdversary::new(seeded(), PoisonPolicy::Newest, None, stats.clone());
        let mut alt = EnumAdversary::new(seeded(), PoisonPolicy::Alternate, None, stats.clone());
        assert_eq!(rev.choose(&p, &mut rng), 0);
        assert_eq!(new.choose(&p, &mut rng), 2);
        let seq: Vec<u64> = (0..4).map(|_| alt.choose(&p, &mut rng)).collect();
        assert_eq!(seq, [0, 2, 0, 2]);
        let mut ev = EnumAdversary::new(seeded(), PoisonPolicy::Evict, None, stats.clone());
        let other = Choice { aux: 1, ..p };
        let seq: Vec<u64> = [p, p, other, p, other]
            .iter()
            .map(|c| ev.choose(c, &mut rng))
            .collect();
        assert_eq!(
            seq,
            [2, 0, 2, 0, 0],
            "new once per sub-sector, then evicted"
        );
        assert_eq!(lock_stats(&stats).poisoned_reads, 11);
    }
}
