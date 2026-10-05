//! Quiet mode ([AR §6.6]; [API §8.3], §4.3 row 8; [CFG §10.3] `quiet.from-lane-measuring`; [RULES/status-machines]
//! DE-027, DE-028): whether the store is in quiet mode, and which commands it refuses without `force`.

use crate::api::AllocTable;
use crate::dag::Dag;

/// Whether a live `lane` node on `main`'s tip has the status `measuring` ([RULES/status-machines] ST-053, DE-027).
pub fn lane_measuring(dag: &Dag, alloc: &AllocTable) -> bool {
    let tip = dag.live("main").and_then(|r| r.tip);
    dag.state_at(tip, alloc)
        .nodes
        .values()
        .any(|x| x.live() && x.kind == "lane" && x.status == "measuring")
}

/// Quiet mode ([AR §6.6]): the explicit flag (`HEAD.flags.quiet`, `Quiet`), or, while `quiet.from-lane-measuring` is
/// true, a lane with status `measuring`. "The explicit flag wins" is read as: the flag set is quiet whatever the
/// lanes say; with one bit there is no explicit "off" that could override a measuring lane.
// spec: [AR §6.6]
// spec: [CFG §10.3] quiet.from-lane-measuring
// rule: DE-027, DE-028
pub fn in_quiet_mode(flag: bool, from_lane_measuring: bool, lane_measuring: bool) -> bool {
    flag || (from_lane_measuring && lane_measuring)
}

/// The commands quiet mode refuses without `force` ([API §8.3]): `Gc`, `ImageExport`, `ImageImport`, and `LinksSync`
/// with `deep` or `all`. The model runs `Gc` at M0; the image commands are M5's and the link commands group F's
/// ([API §12]).
pub const REFUSED: [&str; 4] = ["Gc", "ImageExport", "ImageImport", "LinksSync-deep"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_or_a_measuring_lane_under_the_key() {
        assert!(in_quiet_mode(true, false, false));
        assert!(in_quiet_mode(false, true, true));
        assert!(!in_quiet_mode(false, false, true));
        assert!(!in_quiet_mode(false, true, false));
    }
}
