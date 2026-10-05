//! A ref move that no commit carries ([F05 §9.2] reasons 3 `undo` and 4 `op restore`): the `RefUpdate` with its
//! reflog entry and the marker events it implies ([RULES/state-definition] ME-006, ME-013; VR-005), in one ref group.
//! WP-91's `Undo` and `OpRestore` compose their commands from it; the model's own suites use it to reach the
//! backward moves of GT18 ([AR §8.2]: "`undo` of a reopen or an undelete", "`op restore` in both directions").

use crate::api::{MarkerOut, Store};
use crate::dag::{MoveReason, RefMove};
use crate::markers::{Cause, Group};

impl Store {
    /// Moves the live ref `name` to `new` (a commit of the DAG, or none) by `undo` or `op restore`: one `RefUpdate`
    /// whose HLC opens the ref group, then the group's `Marker` record ([F05 §4.7]). Returns the listed marker entries
    /// ([API §10.8]). Panics when the ref is not live or `reason` is not a backward move: callers check first.
    // spec: [RULES/state-definition] ME-006
    pub fn move_ref(
        &mut self,
        name: &str,
        new: Option<u64>,
        reason: MoveReason,
        actor: &str,
    ) -> Vec<MarkerOut> {
        let cause = match reason {
            MoveReason::Undo => Cause::Undo,
            MoveReason::OpRestore => Cause::OpRestore,
            other => panic!("{other:?} is not a ref move without a commit"),
        };
        let hlc = self.hlc.record(self.env.wall_ms);
        let r = self.dag.live_mut(name).expect("a live ref");
        let old = r.tip;
        r.tip = new;
        r.moves.push(RefMove {
            old,
            new,
            reason,
            actor: actor.to_string(),
            hlc,
        });
        let r = r.clone();
        let entries = self.markers.ref_moved(
            &self.dag,
            &r,
            old,
            new,
            cause,
            Group::Move(hlc),
            &mut self.hlc,
            self.env.wall_ms,
        );
        // [LQ/std §2.15]: the moved ref, its new tip (absent for a deletion), the record's actor.
        self.feed.event(
            self.commit_seq,
            name,
            new,
            None,
            reason.name(),
            "ref",
            name.to_string(),
            actor,
            None,
        );
        self.feed_markers(&entries, None);
        self.listed(&entries)
    }
}
