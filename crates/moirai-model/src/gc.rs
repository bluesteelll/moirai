//! `Gc` as the model sees it ([API §8.5]; [F17 §11.2] P30, P31; OP-17-17): only reachability. After a `gc` run, the
//! commits outside the reachable set whose `append_hlc` is older than the cruft delay stop resolving, so undo, reflog
//! revisions and as-of views cannot reach them (E301). Physical GC — frames, trash, `FILEOBS`, `MARKERS_OLD` bytes — is
//! class I and outside the model ([60 §4.3]).

use crate::api::{Data, Reply, Store};
use crate::clock::Hlc;
use crate::dag::Dag;
use crate::err::{Refusal, Res};
use std::collections::BTreeSet;

/// The commits reachable at a `gc` run ([F17 §11.2]): the tips of the live refs (tags and their pins included), plus
/// the old and new tips of every reflog entry younger than `reflog_expire_ms` (the window opened by the move's HLC,
/// [API §6.2] CK-6), with all their ancestors.
// spec: [F17 §11.2]
pub fn reachable_after_gc(
    dag: &Dag,
    hlc: &Hlc,
    wall_ms: i64,
    reflog_expire_ms: u64,
) -> BTreeSet<u64> {
    let mut roots: BTreeSet<u64> = dag.live_refs().filter_map(|r| r.tip).collect();
    for r in dag.refs.values() {
        for m in &r.moves {
            if hlc.within(wall_ms, m.hlc, reflog_expire_ms) {
                roots.extend(m.old);
                roots.extend(m.new);
            }
        }
    }
    let mut out = BTreeSet::new();
    for c in roots {
        if !out.contains(&c) {
            out.extend(dag.ancestors(Some(c)));
        }
    }
    out
}

/// The commits a `gc` run drops: outside the reachable set, and older than `cruft_delay_ms` by their `append_hlc`.
// spec: [F17 §11.2]
pub fn dropped_by_gc(
    dag: &Dag,
    reachable: &BTreeSet<u64>,
    hlc: &Hlc,
    wall_ms: i64,
    cruft_delay_ms: u64,
) -> BTreeSet<u64> {
    dag.commits
        .values()
        .filter(|c| {
            !reachable.contains(&c.seq) && !hlc.within(wall_ms, c.append_hlc, cruft_delay_ms)
        })
        .map(|c| c.seq)
        .collect()
}

impl Store {
    /// `Gc` ([API §8.5]): refused in quiet mode without `force` (`quiet_mode`, exit 6, §4.3 row 8); `reflog_expire` and
    /// `cruft_delay` override P30 and P31 for this run; the dropped commits stop resolving. Result: the number of
    /// reachable commits and of commits that stopped resolving in this run. Never keyed ([API §7.1]).
    // spec: [API §8.5]
    pub fn gc(
        &mut self,
        reflog_expire_ms: Option<u64>,
        cruft_delay_ms: Option<u64>,
        force: bool,
    ) -> Res<Reply> {
        if !force && self.in_quiet_mode() {
            return Err(Refusal::new(
                "quiet_mode",
                6,
                "quiet mode: gc runs only with --force",
            ));
        }
        let expire = reflog_expire_ms.unwrap_or_else(|| self.conf.number("gc.reflog-expire"));
        let cruft = cruft_delay_ms.unwrap_or_else(|| self.conf.number("gc.cruft-delay"));
        let reachable = reachable_after_gc(&self.dag, &self.hlc, self.env.wall_ms, expire);
        let dropped: BTreeSet<u64> =
            dropped_by_gc(&self.dag, &reachable, &self.hlc, self.env.wall_ms, cruft)
                .difference(&self.pruned)
                .copied()
                .collect();
        self.pruned.extend(dropped.iter().copied());
        Ok(Reply::ok(Data::Gc(
            reachable.len() as u64,
            dropped.len() as u64,
        )))
    }

    /// Quiet mode now ([`crate::quiet::in_quiet_mode`] over the flag, `quiet.from-lane-measuring` and the lanes of
    /// `main`).
    pub fn in_quiet_mode(&self) -> bool {
        crate::quiet::in_quiet_mode(
            self.quiet,
            self.conf.flag("quiet.from-lane-measuring"),
            crate::quiet::lane_measuring(&self.dag, &self.alloc),
        )
    }
}
