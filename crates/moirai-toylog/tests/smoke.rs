//! The toy log on the in-memory `Vfs`, without the enumerator: every scenario runs to its end, a reader sees what it
//! wrote, and the recovery finds nothing wrong.

mod common;

use moirai_toylog::Bugs;
use moirai_vfs_sim::SimWorld;
use moirai_vfs_sim::enumerate::{Ledger, Subject};

#[test]
fn every_scenario_runs_and_recovers_cleanly() {
    for sc in common::all() {
        let name = sc.name;
        let sub = common::ToySubject::new(sc, Bugs::NONE);
        let w = SimWorld::new(sub.config(1));
        let l = Ledger::new(&w);
        sub.setup(&w, &l);
        sub.workload(&w, &l);
        let rec = sub.recover(&w);
        assert!(
            rec.first_read.is_ok() && rec.state.is_ok(),
            "{name}: {rec:?}"
        );
        assert!(rec.findings.is_empty(), "{name}: {:?}", rec.findings);
        let state = rec.state.unwrap();
        assert!(!state.is_empty(), "{name}: the recovered state is empty");
        println!("{name}: {} effects", state.len());
    }
}
