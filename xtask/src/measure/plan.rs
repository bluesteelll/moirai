//! The measurement night's plan file (`xtask/measure-night.toml`, [MP §9.8]): where the release binaries and the
//! scratch directory are, the settle, the finish margin, the smoke budget and the guard's floors, and the measurements
//! in run order with their drivers, budgets and quantities. Every value is operational policy with a reviewed default
//! (AGENTS.md); the guard's floors of [MP §8.1] may be raised, never lowered.

use crate::nightly::calendar::MAX_WINDOW_SECS;
use crate::nightly::config::{MIN_DISK_FLOOR, MIN_RAM_FLOOR, parse_bytes};
use crate::toml::{self, Table, Value};

/// The binaries of `moirai-probes-bin` that are tools, not measurement drivers: the four that `noise.yml` lists, and
/// `aggregate`, which reads the records after the night.
pub const TOOLS: &[&str] = &["guard", "loadgen", "noise", "empty", "aggregate"];
/// The largest minutes value: the longest window ([`MAX_WINDOW_SECS`], 72 hours).
const MAX_MINUTES: i64 = MAX_WINDOW_SECS / 60;

/// One measurement of the plan, in run order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Measurement {
    /// The row number of [60 §5.2]: the record's `measurement` and the directory `/private/measurements/<n>/`.
    pub n: u32,
    /// The binary of `moirai-probes-bin` that makes its runs.
    pub driver: String,
    pub title: String,
    /// The budget of one run.
    pub run_budget_secs: i64,
    /// The budget of every run and retry of the measurement together.
    pub budget_secs: i64,
    /// Its quantities ([MP §4.1]), one run each, in order; empty until the work that builds the driver fills it.
    pub quantities: Vec<String>,
}

/// The plan file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plan {
    /// The directory of the release `moirai-probes-bin` binaries, relative to the target root unless absolute.
    pub bins: String,
    /// The drivers' working directories and the session's own files, relative to the target root unless absolute.
    pub scratch: String,
    /// The wait after the pre-checks, before the first run.
    pub settle_secs: i64,
    /// Every run ends this long before the window does.
    pub finish_margin_secs: i64,
    /// The budget of each `--smoke` run.
    pub smoke_budget_secs: i64,
    /// The guard's floors ([MP §8.1]).
    pub ram_floor: u64,
    pub disk_floor: u64,
    pub measurements: Vec<Measurement>,
}

fn known(t: &Table, keys: &[&str], what: &str) -> Result<(), String> {
    match t.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) => Err(format!("[{what}] has the unknown key '{k}'")),
        None => Ok(()),
    }
}

fn string(t: &Table, key: &str, what: &str) -> Result<String, String> {
    match t.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(format!("[{what}] needs the non-empty string '{key}'")),
    }
}

/// A minutes value from `min` to [`MAX_MINUTES`], in seconds.
fn minutes(t: &Table, key: &str, what: &str, min: i64) -> Result<i64, String> {
    match t.get(key).and_then(Value::as_integer) {
        Some(v) if (min..=MAX_MINUTES).contains(&v) => Ok(v * 60),
        Some(v) => Err(format!(
            "[{what}] {key} = {v} is outside {min}..={MAX_MINUTES}"
        )),
        None => Err(format!("[{what}] needs the integer '{key}'")),
    }
}

/// A floor of `[session]`: a byte quantity no lower than [MP §8.1]'s.
// spec: [MP §8.1] (the RAM floor 1.5 GB, the disk floor 25 GB), [MP §9.8] (the plan file)
fn floor(t: &Table, key: &str, min: u64, label: &str) -> Result<u64, String> {
    let s = string(t, key, "session")?;
    let v = parse_bytes(&s).map_err(|e| format!("[session] {key}: {e}"))?;
    if v < min {
        return Err(format!(
            "[session] {key} = \"{s}\" is below {label} ([MP §8.1]); a floor may be raised, never lowered"
        ));
    }
    Ok(v)
}

/// A quantity name of [MP §4.1]: 1 to 64 bytes of `[a-z0-9._-]`, starting with a letter or digit.
// spec: [MP §4.1] (arm and quantity names)
pub fn quantity_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
        })
}

/// A driver's name as `noise.yml` checks it: 1 to 64 bytes of `[a-z0-9-]`, starting with a letter or digit.
fn driver_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn measurement(v: &Value, i: usize) -> Result<Measurement, String> {
    let what = format!("measurement {}", i + 1);
    let t = v
        .as_table()
        .ok_or_else(|| format!("[{what}] is not a table"))?;
    known(
        t,
        &[
            "n",
            "driver",
            "title",
            "run-budget-minutes",
            "budget-minutes",
            "quantities",
        ],
        &what,
    )?;
    let n = match t.get("n").and_then(Value::as_integer) {
        Some(n) if (1..=i64::from(u32::MAX)).contains(&n) => n as u32,
        Some(n) => {
            return Err(format!(
                "[{what}] n = {n} is not a measurement number (at least 1)"
            ));
        }
        None => return Err(format!("[{what}] needs the integer 'n'")),
    };
    let what = format!("measurement {n}");
    let driver = string(t, "driver", &what)?;
    if !driver_name(&driver) {
        return Err(format!(
            "[{what}] driver '{driver}' must be 1 to 64 bytes of a-z, 0-9 and '-', starting with a letter or digit"
        ));
    }
    if TOOLS.contains(&driver.as_str()) {
        return Err(format!(
            "[{what}] '{driver}' is a tool of moirai-probes-bin, not a measurement driver"
        ));
    }
    let run_budget_secs = minutes(t, "run-budget-minutes", &what, 1)?;
    let budget_secs = minutes(t, "budget-minutes", &what, 1)?;
    if run_budget_secs > budget_secs {
        return Err(format!(
            "[{what}] run-budget-minutes {} exceeds budget-minutes {}: the measurement's budget bounds every run and retry ([MP §9.8])",
            run_budget_secs / 60,
            budget_secs / 60
        ));
    }
    let quantities = match t.get("quantities") {
        Some(Value::Array(a)) => a
            .iter()
            .map(|q| {
                q.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("[{what}] 'quantities' must hold strings"))
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(format!("[{what}] needs the list of strings 'quantities'")),
    };
    for (j, q) in quantities.iter().enumerate() {
        if !quantity_name(q) {
            return Err(format!(
                "[{what}] quantity '{q}' is not a name of [MP §4.1] (1 to 64 bytes of a-z, 0-9, '.', '_' and '-', starting with a letter or digit)"
            ));
        }
        if quantities[..j].contains(q) {
            return Err(format!("[{what}] lists the quantity '{q}' twice"));
        }
    }
    Ok(Measurement {
        n,
        driver,
        title: string(t, "title", &what)?,
        run_budget_secs,
        budget_secs,
        quantities,
    })
}

impl Plan {
    /// Parses and checks a plan file ([MP §9.8]): every key is required and an unknown key is refused.
    // spec: [MP §9.8] (the plan file)
    pub fn parse(text: &str) -> Result<Plan, String> {
        let t = toml::parse(text).map_err(|e| e.to_string())?;
        if t.get("version").and_then(Value::as_integer) != Some(1) {
            return Err("needs 'version = 1'".into());
        }
        known(&t, &["version", "session", "measurement"], "top level")?;
        let s = t
            .get("session")
            .and_then(Value::as_table)
            .ok_or("missing table [session]")?;
        known(
            s,
            &[
                "bins",
                "scratch",
                "settle-minutes",
                "finish-margin-minutes",
                "smoke-budget-minutes",
                "ram-floor",
                "disk-floor",
            ],
            "session",
        )?;
        let entries = match t.get("measurement") {
            Some(Value::Array(a)) => a.as_slice(),
            None => &[],
            Some(_) => {
                return Err("'measurement' must be an array of tables ([[measurement]])".into());
            }
        };
        if entries.is_empty() {
            return Err("needs at least one [[measurement]]".into());
        }
        let mut measurements: Vec<Measurement> = Vec::with_capacity(entries.len());
        for (i, v) in entries.iter().enumerate() {
            let m = measurement(v, i)?;
            if measurements.iter().any(|x| x.n == m.n) {
                return Err(format!("two measurements have n = {}", m.n));
            }
            measurements.push(m);
        }
        Ok(Plan {
            bins: string(s, "bins", "session")?,
            scratch: string(s, "scratch", "session")?,
            settle_secs: minutes(s, "settle-minutes", "session", 0)?,
            finish_margin_secs: minutes(s, "finish-margin-minutes", "session", 1)?,
            smoke_budget_secs: minutes(s, "smoke-budget-minutes", "session", 1)?,
            ram_floor: floor(s, "ram-floor", MIN_RAM_FLOOR, "1.5 GB")?,
            disk_floor: floor(s, "disk-floor", MIN_DISK_FLOOR, "25 GB")?,
            measurements,
        })
    }

    /// The measurements whose quantity list is still empty: the plan is incomplete while there is one ([MP §9.8]).
    pub fn incomplete(&self) -> Vec<&Measurement> {
        self.measurements
            .iter()
            .filter(|m| m.quantities.is_empty())
            .collect()
    }

    /// The number of runs: one per quantity.
    pub fn runs(&self) -> usize {
        self.measurements.iter().map(|m| m.quantities.len()).sum()
    }

    /// The measurements' budgets together.
    pub fn budgets_secs(&self) -> i64 {
        self.measurements.iter().map(|m| m.budget_secs).sum()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_repository_plan_loads() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let p =
            Plan::parse(&std::fs::read_to_string(repo.join("xtask/measure-night.toml")).unwrap())
                .unwrap();
        let col =
            |f: &dyn Fn(&Measurement) -> i64| p.measurements.iter().map(f).collect::<Vec<_>>();
        // The night's order: the short canary, the measurements that decide bytes, the leader, the hot keys.
        assert_eq!(col(&|m| i64::from(m.n)), [1, 14, 6, 2, 10]);
        assert_eq!(
            p.measurements
                .iter()
                .map(|m| m.driver.as_str())
                .collect::<Vec<_>>(),
            ["m1", "m14", "m6", "m2", "m10"]
        );
        assert_eq!(col(&|m| m.run_budget_secs / 60), [20, 90, 45, 70, 45]);
        assert_eq!(col(&|m| m.budget_secs / 60), [25, 120, 55, 85, 160]);
        assert_eq!((p.ram_floor, p.disk_floor), (1_500_000_000, 25_000_000_000));
        assert_eq!(
            (p.settle_secs, p.finish_margin_secs, p.smoke_budget_secs),
            (15 * 60, 10 * 60, 10 * 60)
        );
        assert_eq!(
            (p.bins.as_str(), p.scratch.as_str()),
            ("laneA/release", "measure")
        );
        // The settle, every measurement's budget and the finish margin fit an 8-hour window, so a hang costs only its
        // own measurement.
        assert!(p.settle_secs + p.budgets_secs() + p.finish_margin_secs <= 8 * 3_600);
        // The driver units fill the quantity lists; every name listed is one of [MP §4.1].
        for m in &p.measurements {
            assert!(m.quantities.iter().all(|q| quantity_name(q)), "{m:?}");
        }
    }

    /// A plan with one measurement, with `session` and `measurement` lines replaced or added.
    pub(crate) fn text(session: &str, measurement: &str) -> String {
        format!(
            "version = 1\n[session]\nbins = \"bins\"\nscratch = \"scratch\"\nsettle-minutes = 15\n\
             finish-margin-minutes = 10\nsmoke-budget-minutes = 10\n{session}\n[[measurement]]\nn = 1\n\
             driver = \"m1\"\ntitle = \"t\"\nrun-budget-minutes = 20\nbudget-minutes = 25\n{measurement}\n"
        )
    }

    const FLOORS: &str = "ram-floor = \"1.5GB\"\ndisk-floor = \"25GB\"";

    #[test]
    fn malformed_plans_are_refused() {
        let ok = text(FLOORS, "quantities = [\"a.b\", \"c-1\"]");
        let p = Plan::parse(&ok).unwrap();
        assert_eq!(p.runs(), 2);
        assert!(p.incomplete().is_empty());
        // An empty list loads; the plan is then incomplete.
        let p = Plan::parse(&text(FLOORS, "quantities = []")).unwrap();
        assert_eq!(p.incomplete().len(), 1);
        let q = "quantities = [\"a\"]";
        for (t, needle) in [
            (ok.replace("version = 1", "version = 2"), "version = 1"),
            (
                format!("extra = 1\n{ok}"),
                "[top level] has the unknown key 'extra'",
            ),
            (
                text(&format!("{FLOORS}\nwatch = 1"), q),
                "[session] has the unknown key 'watch'",
            ),
            (
                text(FLOORS, &format!("{q}\nturn = 1")),
                "[measurement 1] has the unknown key 'turn'",
            ),
            (
                text("ram-floor = \"1GB\"\ndisk-floor = \"25GB\"", q),
                "ram-floor = \"1GB\" is below 1.5 GB",
            ),
            (
                text("ram-floor = \"1.5GB\"\ndisk-floor = \"24GB\"", q),
                "disk-floor = \"24GB\" is below 25 GB",
            ),
            (
                text("ram-floor = \"1.5 GB\"\ndisk-floor = \"25GB\"", q),
                "not a byte quantity",
            ),
            (
                text("disk-floor = \"25GB\"", q),
                "needs the non-empty string 'ram-floor'",
            ),
            (
                ok.replace("run-budget-minutes = 20", "run-budget-minutes = 0"),
                "run-budget-minutes = 0 is outside 1..=4320",
            ),
            (
                ok.replace("budget-minutes = 25", "budget-minutes = 4321"),
                "budget-minutes = 4321 is outside",
            ),
            (
                ok.replace("smoke-budget-minutes = 10", "smoke-budget-minutes = 0"),
                "smoke-budget-minutes = 0",
            ),
            (
                ok.replace("settle-minutes = 15", "settle-minutes = -1"),
                "settle-minutes = -1",
            ),
            (
                ok.replace("budget-minutes = 25", "budget-minutes = 19"),
                "run-budget-minutes 20 exceeds budget-minutes 19",
            ),
            (
                ok.replace("\nn = 1\n", "\nn = 0\n"),
                "n = 0 is not a measurement number",
            ),
            (
                ok.clone()
                    + "[[measurement]]\nn = 1\ndriver = \"m2\"\ntitle = \"t\"\nrun-budget-minutes = 1\nbudget-minutes = 1\nquantities = []\n",
                "two measurements have n = 1",
            ),
            (
                ok.replace("driver = \"m1\"", "driver = \"guard\""),
                "'guard' is a tool of moirai-probes-bin",
            ),
            (
                ok.replace("driver = \"m1\"", "driver = \"aggregate\""),
                "'aggregate' is a tool of moirai-probes-bin",
            ),
            (
                ok.replace("driver = \"m1\"", "driver = \"M1\""),
                "driver 'M1' must be",
            ),
            (
                ok.replace("driver = \"m1\"", "driver = \"-m1\""),
                "driver '-m1' must be",
            ),
            (
                text(FLOORS, "quantities = [\"Big\"]"),
                "quantity 'Big' is not a name",
            ),
            (
                text(FLOORS, "quantities = [\".a\"]"),
                "quantity '.a' is not a name",
            ),
            (
                text(FLOORS, &format!("quantities = [\"{}\"]", "a".repeat(65))),
                "is not a name",
            ),
            (
                text(FLOORS, "quantities = [\"a\", \"a\"]"),
                "lists the quantity 'a' twice",
            ),
            (
                text(FLOORS, "quantities = \"a\""),
                "needs the list of strings 'quantities'",
            ),
            (text(FLOORS, "quantities = [1]"), "must hold strings"),
            (
                ok.replace("title = \"t\"", "title = \"\""),
                "needs the non-empty string 'title'",
            ),
            (
                ok.split("[[measurement]]").next().unwrap().to_string(),
                "needs at least one [[measurement]]",
            ),
            (
                "version = 1\nmeasurement = 3\n[session]\n".to_string(),
                "array of tables",
            ),
            (
                "version = 1\n[[measurement]]\nn = 1\n".to_string(),
                "missing table [session]",
            ),
        ] {
            let e = Plan::parse(&t).unwrap_err();
            assert!(e.contains(needle), "{t}\n{e}");
        }
    }
}
