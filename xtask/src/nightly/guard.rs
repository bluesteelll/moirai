//! The runner's use of `moirai-probes-bin guard` ([MP §8], `docs/spec/measurement-protocol.md` §8): the pre-check's
//! arguments (the RAM and disk floors and the four counted directories with their caps), the watchdog's (RAM only),
//! and the parse of the guard's one-line JSON output (`moirai-probes/guard/1`) and exit code.

use serde_json::Value;
use std::path::Path;

/// The schema of the guard's output ([MP §8.3]).
pub const SCHEMA: &str = "moirai-probes/guard/1";
/// The longest a guard run may take: its walk of the counted directories ([MP §8.1]).
pub const TIMEOUT_SECS: u64 = 600;

/// One refusal as the guard reports it ([MP §8.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// `ram-low`, `disk-low`, `ram-unreadable`, `disk-unreadable` or `dir-unreadable`.
    pub kind: String,
    pub detail: String,
}

/// What a guard run decided.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    /// Exit code 0 and verdict `pass`.
    pub pass: bool,
    pub refusals: Vec<Refusal>,
    /// The available physical memory it read (or was given), if any.
    pub ram_available: Option<u64>,
    /// The guard's whole output object, kept in the run record.
    pub json: Value,
}

/// The pre-check's arguments ([MP §8.2]): `--volume` the target root, one `--dir <path> <cap>` per counted directory,
/// the two floors, then the injected readings `nightly check` passes through (never a real run's, [MP §8.2]).
// spec: [PLAN §3.2 WP-05] (refuse below 1.5 GB free RAM or below 25 GB free disk counted after the two lane
// directories and the fuzz and mutants directories), [MP §8.1], [MP §8.2]
pub fn pre_check_args(
    root: &Path,
    dirs: &[(std::path::PathBuf, u64)],
    ram_floor: u64,
    disk_floor: u64,
    inject: &[String],
) -> Vec<String> {
    let mut a = vec!["--volume".to_string(), root.to_string_lossy().into_owned()];
    for (d, cap) in dirs {
        a.extend([
            "--dir".to_string(),
            d.to_string_lossy().into_owned(),
            cap.to_string(),
        ]);
    }
    a.extend([
        "--ram-floor".to_string(),
        ram_floor.to_string(),
        "--disk-floor".to_string(),
        disk_floor.to_string(),
    ]);
    a.extend(inject.iter().cloned());
    a
}

/// The watchdog's arguments: the RAM floor alone (no counted directory, disk floor 0), so it reads one value and
/// walks nothing ([60 §3.15]: "everything refused below 1.5 GB free").
// spec: [60 §3.15] (everything refused below 1.5 GB free), [MP §8.1]
pub fn watch_args(root: &Path, ram_floor: u64) -> Vec<String> {
    vec![
        "--volume".to_string(),
        root.to_string_lossy().into_owned(),
        "--ram-floor".to_string(),
        ram_floor.to_string(),
        "--disk-floor".to_string(),
        "0".to_string(),
    ]
}

/// Reads a guard run: exit 0 with verdict `pass`, or exit 1 with verdict `refuse` and its refusals ([MP §8.3]).
/// Exit 2 (a usage error), another exit, no exit code, or output that is not the guard's object is an error: the
/// runner then refuses, failing closed as the guard does.
// spec: [MP §8.3] (the output object and its agreement with the exit code), [MP §8.1] (fail closed)
pub fn parse(exit: Option<i32>, stdout: &str, stderr: &str) -> Result<Verdict, String> {
    let line = stdout.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let first_err = || stderr.lines().next().unwrap_or("").trim().to_string();
    let json: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            return Err(format!(
                "the guard printed no JSON line (exit {}; {})",
                exit.map_or("none".to_string(), |c| c.to_string()),
                first_err()
            ));
        }
    };
    if json["schema"] != SCHEMA {
        return Err(format!("the guard's output is not {SCHEMA}"));
    }
    let verdict = json["verdict"].as_str().unwrap_or("");
    let refusals: Vec<Refusal> = json["refusals"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| Refusal {
                    kind: r["kind"].as_str().unwrap_or("").to_string(),
                    detail: r["detail"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let pass = match (exit, verdict) {
        (Some(0), "pass") if refusals.is_empty() => true,
        (Some(1), "refuse") if !refusals.is_empty() => false,
        _ => {
            return Err(format!(
                "the guard's exit code {} and verdict '{verdict}' with {} refusals do not agree",
                exit.map_or("none".to_string(), |c| c.to_string()),
                refusals.len()
            ));
        }
    };
    Ok(Verdict {
        pass,
        refusals,
        ram_available: json["ram"]["available"].as_u64(),
        json,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A guard output line as `moirai-probes-bin guard` prints it.
    pub(crate) fn line(ram: u64, refusals: &[(&str, &str)]) -> String {
        let verdict = if refusals.is_empty() {
            "pass"
        } else {
            "refuse"
        };
        let r: Vec<Value> = refusals
            .iter()
            .map(|(k, d)| serde_json::json!({"kind": k, "detail": d}))
            .collect();
        serde_json::json!({
            "schema": SCHEMA, "verdict": verdict,
            "ram": {"available": ram, "floor": 1_500_000_000u64, "injected": false, "error": null},
            "disk": {"volume": "D:/moirai-target", "available": 90_000_000_000u64, "reserved": 0, "headroom": 90_000_000_000u64,
                     "floor": 25_000_000_000u64, "injected": false, "error": null},
            "dirs": [], "refusals": r,
        })
        .to_string()
    }

    #[test]
    fn arguments() {
        let root = PathBuf::from("D:/moirai-target");
        let dirs = vec![
            (root.join("laneA"), 40_000_000_000),
            (root.join("fuzz"), 10_000_000_000),
        ];
        let a = pre_check_args(
            &root,
            &dirs,
            1_500_000_000,
            25_000_000_000,
            &["--inject-available-physical".into(), "1GB".into()],
        );
        assert_eq!(a[..2], ["--volume", "D:/moirai-target"]);
        assert_eq!(
            a[2..5],
            [
                "--dir",
                &*root.join("laneA").to_string_lossy(),
                "40000000000"
            ]
        );
        assert_eq!(
            a[8..12],
            ["--ram-floor", "1500000000", "--disk-floor", "25000000000"]
        );
        assert_eq!(a[12..], ["--inject-available-physical", "1GB"]);
        assert_eq!(
            watch_args(&root, 7)[2..],
            ["--ram-floor", "7", "--disk-floor", "0"]
        );
    }

    #[test]
    fn verdicts() {
        let v = parse(Some(0), &format!("{}\n", line(8_000_000_000, &[])), "").unwrap();
        assert!(v.pass && v.refusals.is_empty());
        assert_eq!(v.ram_available, Some(8_000_000_000));
        let out = line(
            1_000_000_000,
            &[(
                "ram-low",
                "available physical memory 1.000 GB is below the floor 1.500 GB",
            )],
        );
        let v = parse(Some(1), &out, "guard: refused: ram-low: ...").unwrap();
        assert!(!v.pass);
        assert_eq!(v.refusals[0].kind, "ram-low");
        assert_eq!(v.json["schema"], SCHEMA);
    }

    #[test]
    fn anything_else_fails_closed() {
        let pass = line(8_000_000_000, &[]);
        let refuse = line(1, &[("ram-low", "x")]);
        for (exit, out, err, needle) in [
            (
                Some(2),
                "",
                "guard: --volume needs a directory",
                "printed no JSON line (exit 2; guard: --volume needs a directory)",
            ),
            (None, "", "", "exit none"),
            (Some(1), pass.as_str(), "", "do not agree"),
            (Some(0), refuse.as_str(), "", "do not agree"),
            (Some(3), pass.as_str(), "", "do not agree"),
            (
                Some(0),
                r#"{"schema":"other/1","verdict":"pass","refusals":[]}"#,
                "",
                "is not moirai-probes/guard/1",
            ),
            (Some(0), "not json", "", "printed no JSON line"),
        ] {
            let e = parse(exit, out, err).unwrap_err();
            assert!(e.contains(needle), "{e}");
        }
    }
}
