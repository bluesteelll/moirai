//! The run records the session runner reads ([MP §7.1]-[MP §7.3], [MP §9.8]): which raw file is an idle run record of
//! a quantity, and whether a record is exit-grade, judged from the fields [MP §7.3] names. xtask may not depend on
//! `moirai-probes` (`xtask/Cargo.toml`), so this reads those JSON fields; the framework's full read-back of a record
//! stays the framework's.

use crate::utc;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

/// The schema of a run record ([MP §7.1]).
pub const RUN_SCHEMA: &str = "moirai-probes/run/1";

/// The names of the files directly in `dir`, a measurement's directory of raw records; a missing directory lists
/// none.
pub fn names(dir: &Path) -> Result<BTreeSet<String>, String> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => return Err(format!("{}: {e}", dir.display())),
    };
    let mut out = BTreeSet::new();
    for e in rd {
        let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
        if e.file_type().is_ok_and(|t| t.is_file()) {
            out.insert(e.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(out)
}

/// Whether `name` is the raw file name of an idle run record of quantity `q` ([MP §7.2]):
/// `<q>.idle.<YYYYMMDDTHHMMSSZ>.json`.
// spec: [MP §7.2] (raw file names)
pub fn record_name(name: &str, q: &str) -> bool {
    name.strip_prefix(q)
        .and_then(|r| r.strip_prefix(".idle."))
        .and_then(|r| r.strip_suffix(".json"))
        .is_some_and(utc::is_compact)
}

/// Judges a record: `Ok` when it is an exit-grade idle run record of measurement `n`, quantity `q` and the measured
/// commit ([MP §7.3]), otherwise every reason it is not. Exit-grade: schema `moirai-probes/run/1` with the run's
/// measurement, quantity, commit and the idle condition; valid (no reason); host kind `laptop`; both host snapshots
/// read, with Defender real-time protection on in both; and the start and end snapshots equal.
// spec: [MP §7.3] (validity and exit grade), [MP §9.8] (the record)
pub fn judge(text: &str, n: u32, q: &str, commit: &str) -> Result<(), Vec<String>> {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("not a JSON record: {e}")]),
    };
    let mut why = Vec::new();
    if v["schema"] != RUN_SCHEMA {
        why.push(format!("schema {} is not {RUN_SCHEMA}", v["schema"]));
    }
    if v["measurement"] != json!(n) {
        why.push(format!("measurement {} is not {n}", v["measurement"]));
    }
    if v["quantity"] != json!(q) {
        why.push(format!("quantity {} is not {q}", v["quantity"]));
    }
    if v["condition"] != json!({ "kind": "idle" }) {
        why.push(format!("condition {} is not idle", v["condition"]));
    }
    if v["commit"] != json!(commit) {
        why.push(format!(
            "commit {} is not the measured commit {commit}",
            v["commit"]
        ));
    }
    match v["reasons"].as_array() {
        Some(r) if r.is_empty() => {}
        Some(r) => why.push(format!(
            "not valid: {}",
            r.iter()
                .map(|x| x.as_str().unwrap_or("?"))
                .collect::<Vec<_>>()
                .join("; ")
        )),
        None => why.push("the record has no list of reasons".into()),
    }
    let host = &v["host"];
    if host["kind"] != "laptop" {
        why.push(format!("host kind {} never decides", host["kind"]));
    }
    let mut read = true;
    for when in ["start", "end"] {
        let s = &host[when];
        if s["windows"].get("ok").is_none() {
            why.push(format!("Windows version unread at the {when}"));
            read = false;
        }
        match s["defender"].get("ok") {
            None => {
                why.push(format!("Defender unread at the {when}"));
                read = false;
            }
            Some(d) if d["realtime"] != true => {
                why.push(format!("Defender real-time protection off at the {when}"))
            }
            Some(_) => {}
        }
    }
    if read && host["start"] != host["end"] {
        why.push("the Windows or Defender versions changed during the run".into());
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A synthetic exit-grade idle run record of measurement `n`, quantity `q` and `commit`, started at `started`
    /// (RFC 3339), with every field of [MP §7.1] and no samples.
    pub(crate) fn exit_grade(n: u32, q: &str, commit: &str, started: &str) -> Value {
        let snapshot = json!({
            "windows": { "ok": "10.0.1.0" },
            "defender": { "ok": { "product": "4.0.0.1", "engine": "1.0.0.1", "signatures": "1.0.0.1", "realtime": true } },
        });
        json!({
            "schema": RUN_SCHEMA,
            "measurement": n,
            "quantity": q,
            "condition": { "kind": "idle" },
            "load_replay_valid": null,
            "host": { "kind": "laptop", "start": snapshot, "end": snapshot },
            "toolchain": "rustc 1.0.0 (synthetic)",
            "commit": commit,
            "started": started,
            "ended": started,
            "timer_resolution_ns": 100,
            "plan": { "n": 20, "repetitions": 3, "block": 1 },
            "escalations": [],
            "arms": [],
            "memory": { "readings": 0, "failures": 0, "out_of_band": 0, "min": null, "max": null, "first_failure": null },
            "process": { "private_peak": 1_000_000, "heap_high_water": null },
            "reasons": [],
        })
    }

    #[test]
    fn exit_grade_follows_mp_7_3() {
        const C: &str = "0123456789abcdef0123456789abcdef01234567";
        let good = exit_grade(14, "csr.1e5", C, "2026-10-13T21:00:00Z");
        assert_eq!(judge(&good.to_string(), 14, "csr.1e5", C), Ok(()));
        let edit = |f: &dyn Fn(&mut Value)| {
            let mut v = good.clone();
            f(&mut v);
            v.to_string()
        };
        for (text, needle) in [
            (
                edit(&|v| v["reasons"] = json!(["a round-boundary reading failed"])),
                "not valid: a round-boundary reading failed",
            ),
            (
                edit(&|v| {
                    v.as_object_mut().unwrap().remove("reasons");
                }),
                "no list of reasons",
            ),
            (
                edit(&|v| v["host"]["kind"] = json!("hosted")),
                "host kind \"hosted\" never decides",
            ),
            (
                edit(&|v| v["host"]["start"]["windows"] = json!({ "error": "ver failed" })),
                "Windows version unread at the start",
            ),
            (
                edit(&|v| v["host"]["end"]["defender"] = json!({ "error": "timed out" })),
                "Defender unread at the end",
            ),
            (
                edit(&|v| {
                    v["host"]["start"]["defender"]["ok"]["realtime"] = json!(false);
                    v["host"]["end"]["defender"]["ok"]["realtime"] = json!(false);
                }),
                "Defender real-time protection off at the start",
            ),
            (
                edit(&|v| v["host"]["end"]["defender"]["ok"]["signatures"] = json!("1.0.0.2")),
                "changed during the run",
            ),
            (
                edit(&|v| v["host"]["end"]["windows"]["ok"] = json!("10.0.1.1")),
                "changed during the run",
            ),
            (
                edit(&|v| v["condition"] = json!({ "kind": "loaded", "fixture": "ab" })),
                "is not idle",
            ),
            (
                edit(&|v| v["condition"] = json!({ "kind": "synthetic", "description": "x" })),
                "is not idle",
            ),
            (
                edit(&|v| v["schema"] = json!("moirai-probes/idle/1")),
                "schema \"moirai-probes/idle/1\" is not moirai-probes/run/1",
            ),
            (
                edit(&|v| v["measurement"] = json!(6)),
                "measurement 6 is not 14",
            ),
            (
                edit(&|v| v["quantity"] = json!("csr.1e6")),
                "quantity \"csr.1e6\" is not csr.1e5",
            ),
            (
                edit(&|v| v["commit"] = json!("fedcba9876543210fedcba9876543210fedcba98")),
                "is not the measured commit",
            ),
            (good.to_string()[..200].to_string(), "not a JSON record"),
        ] {
            let why = judge(&text, 14, "csr.1e5", C).expect_err(needle);
            assert!(why.iter().any(|w| w.contains(needle)), "{needle}: {why:?}");
            // Each damage is the only reason given, except where the case damages two fields at once.
            assert!(why.len() <= 2, "{why:?}");
        }
        // The raw file names of an idle run record of a quantity ([MP §7.2]).
        assert!(record_name("csr.1e5.idle.20261013T210000Z.json", "csr.1e5"));
        for (name, q) in [
            ("csr.1e5.loaded.20261013T210000Z.json", "csr.1e5"),
            ("csr.1e5.idle.20261013T210000Z.json", "csr"),
            ("csr.1e5.idle.2026-10-13.json", "csr.1e5"),
            ("csr.1e5.idle.20261013T210000Z.json.tmp", "csr.1e5"),
            ("other.idle.20261013T210000Z.json", "csr.1e5"),
        ] {
            assert!(!record_name(name, q), "{name} {q}");
        }
    }
}
