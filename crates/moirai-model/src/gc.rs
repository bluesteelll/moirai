//! `Gc` as the model sees it ([API §8.5]; [F17 §11.2] P30, P31; OP-17-17): reachability and the runtime drops the
//! snapshot shows. After a `gc` run, the commits outside the reachable set whose `append_hlc` is older than the cruft
//! delay stop resolving, so undo, reflog revisions and as-of views cannot reach them (E301); the ref moves older than
//! the run's reflog window are no longer held ([F12 §3.5]; [API §11.11]); after the run's inertness move the
//! `MARKERS_OLD` rows older than that window are dropped ([F11 §7] "Retention"), then the `REFS` rows of expired
//! deleted refs with their entries in the other refs' absorbed vectors ([F11 §3.8]). Both drops are visible in
//! [API §15.7] `markers`, `refs` and `absorbed`, and neither depends on when earlier folds ran. Physical GC — frames,
//! trash, `FILEOBS`, expired `IDEM` entries — is class I and outside the model ([60 §4.3]).

use crate::api::{Data, Reply, Store};
use crate::clock::Hlc;
use crate::dag::Dag;
use crate::err::{Refusal, Res};
use crate::idem;
use crate::markers::Markers;
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

/// The deleted refs whose `REFS` row expires at a `gc` run ([F11 §3.8]), read after the run's inertness move and
/// `MARKERS_OLD` retention: (1) the deleting `RefUpdate` has an `hlc` older than the run's reflog window; (2) no row of
/// `MARKERS` or `MARKERS_OLD` names the ref as its origin ref (VR-006); (3) no `IDEM` entry bound to the ref is inside
/// its lookup window ([F17 §11.1]).
// spec: [F11 §3.8]
// rule: VR-006
pub fn expired_refs(
    dag: &Dag,
    markers: &Markers,
    table: &idem::Table,
    windows: idem::Windows,
    hlc: &Hlc,
    wall_ms: i64,
    reflog_expire_ms: u64,
) -> Vec<u32> {
    dag.refs
        .values()
        .filter(|r| r.deleted)
        .filter(|r| {
            r.moves
                .last()
                .is_none_or(|m| !hlc.within(wall_ms, m.hlc, reflog_expire_ms))
        })
        .filter(|r| !markers.rows().any(|m| m.key.1 == r.id))
        .filter(|r| {
            !table
                .entries
                .values()
                .any(|e| e.ref_id == r.id && hlc.within(wall_ms, e.append_hlc, windows.of(e)))
        })
        .map(|r| r.id)
        .collect()
}

impl Store {
    /// `Gc` ([API §8.5]): refused in quiet mode without `force` (`quiet_mode`, exit 6, §4.3 row 8); `reflog_expire` and
    /// `cruft_delay` override P30 and P31 for this run. In the run's order: the dropped commits stop resolving; the
    /// moves older than the reflog window are no longer held ([F12 §3.5]: `OpRestore`'s second E301, `Undo`'s move count
    /// and the reflog suffixes read only held moves); the checkpoint fold's inertness move (ME-012), then its
    /// `MARKERS_OLD` retention ([F11 §7]); then the expired deleted refs' rows and their absorbed entries ([F11 §3.8]).
    /// Result: the number of reachable commits and of commits that stopped resolving in this run. Never keyed
    /// ([API §7.1]).
    // spec: [API §8.5]
    // spec: [F11 §7]
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
        let (hlc, wall) = (&self.hlc, self.env.wall_ms);
        for r in self.dag.refs.values_mut() {
            let kept = r.moves.iter().position(|m| hlc.within(wall, m.hlc, expire));
            let cut = kept.unwrap_or(r.moves.len());
            if cut > 0 {
                self.moves_dropped.insert(r.id, r.moves[cut - 1].clone());
                r.moves.drain(..cut);
            }
        }
        self.markers.fold_inert(&self.dag);
        self.markers
            .old
            .retain(|_, m| hlc.within(wall, m.hlc, expire));
        let expired = expired_refs(
            &self.dag,
            &self.markers,
            &self.idem,
            self.windows(),
            &self.hlc,
            self.env.wall_ms,
            expire,
        );
        for id in expired {
            self.dag.refs.remove(&id);
            self.markers.absorbed.remove(&id);
            for v in self.markers.absorbed.values_mut() {
                v.remove(&id);
            }
        }
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
