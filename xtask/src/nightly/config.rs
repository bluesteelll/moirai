//! `xtask/nightly.toml`: the runner's caps, guard floors, counted directories and job list (`docs/m0/nightly.md` §3).
//! Every value is operational policy with a reviewed default (AGENTS.md); the limits PLAN WP-05 states — fuzz at most
//! two targets with `-rss_limit_mb` at most 256, jobs beside agents within 1 GB, the guard's floors of 1.5 GB of RAM
//! and 25 GB of disk — are checked here, so a typo cannot widen them (a floor may be raised, never lowered).

use crate::toml::{Table, Value};

/// The file, relative to the repository root.
pub const FILE: &str = "xtask/nightly.toml";
/// The cap PLAN WP-05 sets on gate jobs beside agents ([60 §3.15] "gate jobs beside the agents ≤ 1 GB in total").
pub const BESIDE_AGENTS_LIMIT: u64 = 1_000_000_000;
/// PLAN WP-05: "fuzz ≤ 2 targets".
pub const MAX_FUZZ_TARGETS: u32 = 2;
/// PLAN WP-05: "sanitizer-off `-rss_limit_mb=256`".
pub const MAX_RSS_LIMIT_MB: u32 = 256;
/// PLAN WP-05: "refuse below 1.5 GB free RAM" ([60 §3.15]: "everything refused below 1.5 GB free").
pub const MIN_RAM_FLOOR: u64 = 1_500_000_000;
/// PLAN WP-05: "below 25 GB free disk" ([60 §3.15]: "the harness refuses to start below 25 GB free").
pub const MIN_DISK_FLOOR: u64 = 25_000_000_000;
/// The least time a fuzz job keeps between the end of its last target's `-max_total_time` and the job's deadline:
/// cargo-fuzz's start and cargo's freshness check come before libFuzzer's clock starts, and libFuzzer checks its time
/// limit only about once a second, so a target given the time up to the deadline would always be stopped there.
pub const MIN_FUZZ_GRACE_SECS: i64 = 60;

/// Parses a byte quantity: decimal digits with an optional fraction and an optional unit `B`, `KB`, `MB`, `GB`, `TB`
/// (powers of 10) or `KiB`, `MiB`, `GiB`, `TiB` (powers of 2), which must come to a whole number of bytes
/// (`docs/spec/measurement-protocol.md` §1.3, §8.2: the guard's syntax).
// spec: [MP §1.3] (KB … TB and KiB … TiB), [MP §8.2] (byte quantities)
pub fn parse_bytes(s: &str) -> Result<u64, String> {
    let bad = || {
        format!(
            "'{s}' is not a byte quantity (digits with an optional fraction and unit: 25GB, 1.5GB, 8MiB)"
        )
    };
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let mult: u128 = match unit {
        "" | "B" => 1,
        "KB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,
        "KiB" => 1 << 10,
        "MiB" => 1 << 20,
        "GiB" => 1 << 30,
        "TiB" => 1 << 40,
        _ => return Err(bad()),
    };
    let (int, frac) = num.split_once('.').unwrap_or((num, ""));
    if int.is_empty() || num.ends_with('.') || frac.contains('.') || frac.len() > 12 {
        return Err(bad());
    }
    let whole: u128 = int.parse().map_err(|_| bad())?;
    let frac_val: u128 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| bad())?
    };
    let scale = 10u128.pow(frac.len() as u32);
    let total = whole
        .checked_mul(mult)
        .and_then(|w| w.checked_add(frac_val * mult / scale))
        .ok_or_else(bad)?;
    if !(frac_val * mult).is_multiple_of(scale) {
        return Err(format!("'{s}' is not a whole number of bytes"));
    }
    u64::try_from(total).map_err(|_| bad())
}

/// A test tier (`MOIRAI_TEST_TIER`, PLAN §2.1).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Tier {
    Pr,
    Nightly,
    Exit,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Pr => "pr",
            Tier::Nightly => "nightly",
            Tier::Exit => "exit",
        }
    }
}

/// What a job runs (`docs/m0/nightly.md` §5).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobKind {
    /// `cargo test --locked --no-fail-fast -p …` in the build lane's target directory at a tier.
    CargoTest {
        packages: Vec<String>,
        tier: Tier,
        /// Arguments for the test binaries (after `--`), such as test-name filters.
        args: Vec<String>,
    },
    /// One shard of cargo-mutants over the packages, rotating from run to run (GT16's sample).
    Mutants {
        packages: Vec<String>,
        shards: u32,
        tier: Tier,
    },
    /// At most two libFuzzer targets of `fuzz/`, sanitizer off, rotating from run to run (GT5).
    Fuzz {
        max_targets: u32,
        rss_limit_mb: u32,
        /// The time kept between the end of the last target's `-max_total_time` and the job's deadline.
        grace_secs: i64,
    },
}

impl JobKind {
    pub fn label(&self) -> &'static str {
        match self {
            JobKind::CargoTest { .. } => "cargo-test",
            JobKind::Mutants { .. } => "mutants",
            JobKind::Fuzz { .. } => "fuzz",
        }
    }
}

/// One job of the list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Job {
    /// `[a-z0-9-]+`, unique; the stem of the job's log file.
    pub name: String,
    pub title: String,
    pub kind: JobKind,
    /// The RAM the job may use; a window whose budget is smaller skips it.
    pub ram_budget: u64,
    /// The job starts only if this much time is left before the deadline.
    pub min_secs: i64,
    /// The longest the job may run; 0 lets it run to the deadline.
    pub max_secs: i64,
}

/// The caps of one window kind (`docs/m0/nightly.md` §3).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct KindCaps {
    /// `CARGO_BUILD_JOBS` for every cargo the runner starts; a job that runs `parallel` cargo processes side by side
    /// gives each its share, so the job's total stays within it.
    pub build_jobs: u32,
    /// `RUST_TEST_THREADS` for every test the runner starts, shared the same way.
    pub test_threads: u32,
    /// The most processes of one job side by side (cargo-mutants `--jobs`); at most `build_jobs`.
    pub parallel: u32,
    /// The largest RAM budget a job may declare in this kind of window.
    pub ram_budget: u64,
}

/// `xtask/nightly.toml`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// The lane (`xtask/roles.toml` `[lanes]`) whose target directory the runner builds in.
    pub lane: String,
    /// Every job ends this long before its window does.
    pub finish_margin_secs: i64,
    /// The RAM watchdog's period while a job runs.
    pub watch_secs: u64,
    /// A job log longer than this keeps its first quarter and its last three quarters.
    pub log_cap: u64,
    /// The raw result directories kept in `/private/nightly/`; 0 keeps every one.
    pub keep_runs: u32,
    /// The guard's floors ([MP §8.1]).
    pub ram_floor: u64,
    pub disk_floor: u64,
    /// The counted directories under the target root and their caps, in file order ([MP §8.1], PLAN §2.1).
    pub caps: Vec<(String, u64)>,
    pub agent_free: KindCaps,
    pub beside_agents: KindCaps,
    pub jobs: Vec<Job>,
}

fn table<'t>(t: &'t Table, key: &str, what: &str) -> Result<&'t Table, String> {
    t.get(key)
        .and_then(Value::as_table)
        .ok_or_else(|| format!("{FILE}: missing table [{what}]"))
}

fn known(t: &Table, keys: &[&str], what: &str) -> Result<(), String> {
    match t.keys().find(|k| !keys.contains(&k.as_str())) {
        Some(k) => Err(format!("{FILE}: [{what}] has the unknown key '{k}'")),
        None => Ok(()),
    }
}

fn string(t: &Table, key: &str, what: &str) -> Result<String, String> {
    t.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{FILE}: [{what}] needs the string '{key}'"))
}

fn bytes(t: &Table, key: &str, what: &str) -> Result<u64, String> {
    parse_bytes(&string(t, key, what)?).map_err(|e| format!("{FILE}: [{what}] {key}: {e}"))
}

fn int(t: &Table, key: &str, what: &str, min: i64, max: i64) -> Result<i64, String> {
    match t.get(key).and_then(Value::as_integer) {
        Some(v) if (min..=max).contains(&v) => Ok(v),
        Some(v) => Err(format!(
            "{FILE}: [{what}] {key} = {v} is outside {min}..={max}"
        )),
        None => Err(format!("{FILE}: [{what}] needs the integer '{key}'")),
    }
}

fn strings(t: &Table, key: &str, what: &str, required: bool) -> Result<Vec<String>, String> {
    match t.get(key) {
        None if !required => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{FILE}: [{what}] '{key}' must hold strings"))
            })
            .collect(),
        _ => Err(format!(
            "{FILE}: [{what}] needs the list of strings '{key}'"
        )),
    }
}

fn tier(t: &Table, what: &str) -> Result<Tier, String> {
    match string(t, "tier", what)?.as_str() {
        "pr" => Ok(Tier::Pr),
        "nightly" => Ok(Tier::Nightly),
        "exit" => Ok(Tier::Exit),
        o => Err(format!(
            "{FILE}: [{what}] tier '{o}' is not pr, nightly or exit"
        )),
    }
}

fn kind_caps(t: &Table, key: &str) -> Result<KindCaps, String> {
    let what = format!("window.{key}");
    let k = t
        .get("window")
        .and_then(|w| w.get_path(&[key]))
        .and_then(Value::as_table)
        .ok_or_else(|| format!("{FILE}: missing table [{what}]"))?;
    known(
        k,
        &["build-jobs", "test-threads", "parallel", "ram-budget"],
        &what,
    )?;
    let caps = KindCaps {
        build_jobs: int(k, "build-jobs", &what, 1, 64)? as u32,
        test_threads: int(k, "test-threads", &what, 1, 64)? as u32,
        parallel: int(k, "parallel", &what, 1, 16)? as u32,
        ram_budget: bytes(k, "ram-budget", &what)?,
    };
    if caps.parallel > caps.build_jobs {
        return Err(format!(
            "{FILE}: [{what}] parallel {} exceeds build-jobs {}: each of a job's processes gets a share of build-jobs (PLAN WP-05: the CARGO_BUILD_JOBS cap)",
            caps.parallel, caps.build_jobs
        ));
    }
    Ok(caps)
}

/// A floor of `[guard]`: a byte quantity no lower than PLAN WP-05's.
// spec: [PLAN §3.2 WP-05] (refuse below 1.5 GB free RAM, or below 25 GB free disk)
fn floor(t: &Table, key: &str, min: u64, plan: &str) -> Result<u64, String> {
    let v = bytes(t, key, "guard")?;
    if v < min {
        return Err(format!(
            "{FILE}: [guard] {key} is below {plan} (PLAN WP-05); a floor may be raised, never lowered"
        ));
    }
    Ok(v)
}

fn package_list(j: &Table, what: &str) -> Result<Vec<String>, String> {
    let p = strings(j, "packages", what, true)?;
    if p.is_empty() {
        return Err(format!("{FILE}: [{what}] packages is empty"));
    }
    Ok(p)
}

fn job(j: &Table, i: usize) -> Result<Job, String> {
    let what = format!("job {}", i + 1);
    let name = string(j, "name", &what)?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(format!(
            "{FILE}: [{what}] name '{name}' must be lower-case letters, digits and '-'"
        ));
    }
    let what = format!("job {name}");
    let common = [
        "name",
        "kind",
        "title",
        "ram-budget",
        "min-minutes",
        "max-minutes",
    ];
    let kind = match string(j, "kind", &what)?.as_str() {
        "cargo-test" => {
            known(
                j,
                &[&common[..], &["packages", "tier", "args"]].concat(),
                &what,
            )?;
            JobKind::CargoTest {
                packages: package_list(j, &what)?,
                tier: tier(j, &what)?,
                args: strings(j, "args", &what, false)?,
            }
        }
        "mutants" => {
            known(
                j,
                &[&common[..], &["packages", "shards", "tier"]].concat(),
                &what,
            )?;
            JobKind::Mutants {
                packages: package_list(j, &what)?,
                shards: int(j, "shards", &what, 1, 10_000)? as u32,
                tier: tier(j, &what)?,
            }
        }
        "fuzz" => {
            known(
                j,
                &[
                    &common[..],
                    &["max-targets", "rss-limit-mb", "grace-seconds"],
                ]
                .concat(),
                &what,
            )?;
            JobKind::Fuzz {
                max_targets: int(j, "max-targets", &what, 1, i64::from(MAX_FUZZ_TARGETS)).map_err(
                    |e| format!("{e} (PLAN WP-05: fuzz at most {MAX_FUZZ_TARGETS} targets)"),
                )? as u32,
                rss_limit_mb: int(j, "rss-limit-mb", &what, 1, i64::from(MAX_RSS_LIMIT_MB))
                    .map_err(|e| format!("{e} (PLAN WP-05: -rss_limit_mb=256)"))?
                    as u32,
                grace_secs: int(j, "grace-seconds", &what, MIN_FUZZ_GRACE_SECS, 3_600)?,
            }
        }
        o => {
            return Err(format!(
                "{FILE}: [{what}] kind '{o}' is not cargo-test, mutants or fuzz"
            ));
        }
    };
    let min = int(j, "min-minutes", &what, 1, 7 * 24 * 60)?;
    let max = int(j, "max-minutes", &what, 0, 7 * 24 * 60)?;
    if max != 0 && max < min {
        return Err(format!(
            "{FILE}: [{what}] max-minutes {max} is below min-minutes {min}"
        ));
    }
    Ok(Job {
        name,
        title: string(j, "title", &what)?,
        kind,
        ram_budget: bytes(j, "ram-budget", &what)?,
        min_secs: min * 60,
        max_secs: max * 60,
    })
}

impl Config {
    /// Reads the parsed file; `lanes` are the lane names and directories of `xtask/roles.toml`, which the counted
    /// directories must include (with `fuzz` and `mutants`, `docs/m0/tools.md` §12).
    // spec: [PLAN §3.2 WP-05] (the caps, the floors, the counted directories, the job list), [PLAN §2.1] (target
    // directories)
    pub fn from_table(t: &Table, lanes: &[(String, String)]) -> Result<Config, String> {
        match t.get("version").and_then(Value::as_integer) {
            Some(1) => {}
            _ => return Err(format!("{FILE}: needs 'version = 1'")),
        }
        known(
            t,
            &["version", "run", "guard", "window", "job"],
            "top level",
        )?;
        let run = table(t, "run", "run")?;
        known(
            run,
            &[
                "lane",
                "finish-margin-minutes",
                "watch-seconds",
                "log-cap",
                "keep-runs",
            ],
            "run",
        )?;
        let lane = string(run, "lane", "run")?;
        if !lanes.iter().any(|(l, _)| *l == lane) {
            return Err(format!(
                "{FILE}: [run] lane '{lane}' is not a lane of xtask/roles.toml"
            ));
        }
        let guard = table(t, "guard", "guard")?;
        known(guard, &["ram-floor", "disk-floor", "caps"], "guard")?;
        let caps_t = guard
            .get("caps")
            .and_then(Value::as_table)
            .ok_or_else(|| format!("{FILE}: missing table [guard.caps]"))?;
        let mut caps = Vec::new();
        for (k, v) in caps_t {
            let cap = v
                .as_str()
                .ok_or_else(|| format!("{FILE}: [guard.caps] {k} must be a byte quantity string"))
                .and_then(|s| {
                    parse_bytes(s).map_err(|e| format!("{FILE}: [guard.caps] {k}: {e}"))
                })?;
            caps.push((k.clone(), cap));
        }
        let mut want: Vec<String> = lanes.iter().map(|(_, d)| d.clone()).collect();
        want.extend(["fuzz".to_string(), "mutants".to_string()]);
        for w in &want {
            if !caps.iter().any(|(k, _)| k == w) {
                return Err(format!(
                    "{FILE}: [guard.caps] lacks '{w}': the guard counts both lane directories and the fuzz and mutants directories (PLAN WP-05)"
                ));
            }
        }
        if let Some((k, _)) = caps.iter().find(|(k, _)| !want.contains(k)) {
            return Err(format!(
                "{FILE}: [guard.caps] '{k}' is not a lane, fuzz or mutants directory"
            ));
        }
        let beside_agents = kind_caps(t, "beside-agents")?;
        if beside_agents.ram_budget > BESIDE_AGENTS_LIMIT {
            return Err(format!(
                "{FILE}: [window.beside-agents] ram-budget exceeds 1 GB (PLAN WP-05: gate jobs beside agents stay within 1 GB)"
            ));
        }
        let window = t
            .get("window")
            .and_then(Value::as_table)
            .ok_or_else(|| format!("{FILE}: missing table [window]"))?;
        known(window, &["agent-free", "beside-agents"], "window")?;
        let jobs_v = match t.get("job") {
            Some(Value::Array(a)) => a.as_slice(),
            None => &[],
            Some(_) => {
                return Err(format!(
                    "{FILE}: 'job' must be an array of tables ([[job]])"
                ));
            }
        };
        let mut jobs: Vec<Job> = Vec::new();
        for (i, v) in jobs_v.iter().enumerate() {
            let j = v
                .as_table()
                .ok_or_else(|| format!("{FILE}: job {} is not a table", i + 1))?;
            let job = job(j, i)?;
            if jobs.iter().any(|x| x.name == job.name) {
                return Err(format!("{FILE}: two jobs are named '{}'", job.name));
            }
            jobs.push(job);
        }
        Ok(Config {
            lane,
            finish_margin_secs: int(run, "finish-margin-minutes", "run", 1, 24 * 60)? * 60,
            watch_secs: int(run, "watch-seconds", "run", 5, 3_600)? as u64,
            log_cap: {
                let c = bytes(run, "log-cap", "run")?;
                if c < 4_096 {
                    return Err(format!("{FILE}: [run] log-cap must be at least 4KiB"));
                }
                c
            },
            keep_runs: int(run, "keep-runs", "run", 0, 100_000)? as u32,
            ram_floor: floor(guard, "ram-floor", MIN_RAM_FLOOR, "1.5 GB")?,
            disk_floor: floor(guard, "disk-floor", MIN_DISK_FLOOR, "25 GB")?,
            caps,
            agent_free: kind_caps(t, "agent-free")?,
            beside_agents,
            jobs,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    pub(crate) fn lanes() -> Vec<(String, String)> {
        vec![("A".into(), "laneA".into()), ("B".into(), "laneB".into())]
    }

    pub(crate) const SAMPLE: &str = r#"
version = 1
[run]
lane = "A"
finish-margin-minutes = 10
watch-seconds = 30
log-cap = "8MiB"
keep-runs = 30
[guard]
ram-floor = "1.5GB"
disk-floor = "25GB"
[guard.caps]
laneA = "40GB"
laneB = "40GB"
fuzz = "10GB"
mutants = "20GB"
[window.agent-free]
build-jobs = 6
test-threads = 4
parallel = 2
ram-budget = "6GB"
[window.beside-agents]
build-jobs = 2
test-threads = 2
parallel = 1
ram-budget = "1GB"
[[job]]
name = "gt1"
kind = "cargo-test"
title = "GT1 full"
packages = ["moirai-vfs-sim", "moirai-toylog"]
tier = "nightly"
ram-budget = "1GB"
min-minutes = 30
max-minutes = 240
[[job]]
name = "gt16"
kind = "mutants"
title = "GT16 sample"
packages = ["moirai-files", "moirai-diff"]
shards = 32
tier = "pr"
ram-budget = "2GB"
min-minutes = 30
max-minutes = 180
[[job]]
name = "fuzz"
kind = "fuzz"
title = "GT5"
max-targets = 2
rss-limit-mb = 256
grace-seconds = 120
ram-budget = "768MB"
min-minutes = 15
max-minutes = 0
"#;

    pub(crate) fn sample() -> Config {
        Config::from_table(&crate::toml::parse(SAMPLE).unwrap(), &lanes()).unwrap()
    }

    fn with(edit: &str, by: &str) -> Result<Config, String> {
        assert!(SAMPLE.contains(edit), "{edit}");
        Config::from_table(
            &crate::toml::parse(&SAMPLE.replacen(edit, by, 1)).unwrap(),
            &lanes(),
        )
    }

    #[test]
    fn byte_quantities() {
        assert_eq!(parse_bytes("25GB"), Ok(25_000_000_000));
        assert_eq!(parse_bytes("1.5GB"), Ok(1_500_000_000));
        assert_eq!(parse_bytes("8MiB"), Ok(8 << 20));
        assert_eq!(parse_bytes("768MB"), Ok(768_000_000));
        assert_eq!(parse_bytes("1500000000"), Ok(1_500_000_000));
        assert_eq!(parse_bytes("0B"), Ok(0));
        assert_eq!(parse_bytes("0.5KiB"), Ok(512));
        for bad in [
            "",
            "GB",
            "1.GB",
            ".5GB",
            "1.2.3GB",
            "1gb",
            "1 GB",
            "-1GB",
            "1.0000001B",
            "99999999999TB",
            "1.5",
        ] {
            assert!(parse_bytes(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_sample_loads() {
        let c = sample();
        assert_eq!(c.lane, "A");
        assert_eq!(
            (c.finish_margin_secs, c.watch_secs, c.log_cap, c.keep_runs),
            (600, 30, 8 << 20, 30)
        );
        assert_eq!((c.ram_floor, c.disk_floor), (1_500_000_000, 25_000_000_000));
        assert_eq!(c.caps.len(), 4);
        assert_eq!(c.beside_agents.ram_budget, BESIDE_AGENTS_LIMIT);
        assert_eq!(c.jobs.len(), 3);
        assert_eq!(c.jobs[0].kind.label(), "cargo-test");
        assert_eq!((c.jobs[0].min_secs, c.jobs[0].max_secs), (1_800, 14_400));
        assert_eq!(
            c.jobs[2].kind,
            JobKind::Fuzz {
                max_targets: 2,
                rss_limit_mb: 256,
                grace_secs: 120
            }
        );
        assert_eq!(c.jobs[2].max_secs, 0);
    }

    #[test]
    fn the_repository_file_loads() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let roles = crate::config::Roles::from_table(
            &crate::config::read_toml(&repo.join("xtask/roles.toml")).unwrap(),
        )
        .unwrap();
        let c = Config::from_table(
            &crate::config::read_toml(&repo.join(FILE)).unwrap(),
            &roles.lanes,
        )
        .unwrap();
        assert!(
            c.jobs
                .iter()
                .any(|j| matches!(j.kind, JobKind::Fuzz { .. }))
        );
        assert!(
            c.jobs
                .iter()
                .any(|j| matches!(j.kind, JobKind::Mutants { .. }))
        );
        assert!(c.beside_agents.ram_budget <= BESIDE_AGENTS_LIMIT);
    }

    #[test]
    fn the_plan_limits_cannot_be_widened() {
        let e = with("max-targets = 2", "max-targets = 3").unwrap_err();
        assert!(e.contains("PLAN WP-05: fuzz at most 2 targets"), "{e}");
        let e = with("rss-limit-mb = 256", "rss-limit-mb = 512").unwrap_err();
        assert!(e.contains("rss_limit_mb=256"), "{e}");
        let e = with(
            "parallel = 1\nram-budget = \"1GB\"",
            "parallel = 1\nram-budget = \"1.1GB\"",
        )
        .unwrap_err();
        assert!(e.contains("within 1 GB"), "{e}");
        let e = with("ram-floor = \"1.5GB\"", "ram-floor = \"0\"").unwrap_err();
        assert!(e.contains("ram-floor is below 1.5 GB"), "{e}");
        let e = with("disk-floor = \"25GB\"", "disk-floor = \"24.9GB\"").unwrap_err();
        assert!(e.contains("disk-floor is below 25 GB"), "{e}");
        // A floor may be raised.
        let c = with("disk-floor = \"25GB\"", "disk-floor = \"30GB\"").unwrap();
        assert_eq!(c.disk_floor, 30_000_000_000);
        let e = with("grace-seconds = 120", "grace-seconds = 59").unwrap_err();
        assert!(e.contains("outside 60..=3600"), "{e}");
    }

    #[test]
    fn malformed_files_are_refused() {
        for (edit, by, needle) in [
            ("version = 1", "version = 2", "version = 1"),
            ("lane = \"A\"", "lane = \"C\"", "not a lane"),
            ("laneB = \"40GB\"\n", "", "lacks 'laneB'"),
            (
                "mutants = \"20GB\"",
                "mutants = \"20GB\"\nother = \"1GB\"",
                "not a lane, fuzz or mutants",
            ),
            (
                "keep-runs = 30",
                "keep-runs = 30\nfoo = 1",
                "unknown key 'foo'",
            ),
            ("log-cap = \"8MiB\"", "log-cap = \"1KiB\"", "at least 4KiB"),
            (
                "log-cap = \"8MiB\"",
                "log-cap = \"8 MiB\"",
                "not a byte quantity",
            ),
            (
                "watch-seconds = 30",
                "watch-seconds = 1",
                "outside 5..=3600",
            ),
            (
                "name = \"gt16\"",
                "name = \"gt1\"",
                "two jobs are named 'gt1'",
            ),
            ("name = \"gt16\"", "name = \"GT16\"", "lower-case"),
            ("kind = \"mutants\"", "kind = \"bench\"", "kind 'bench'"),
            ("tier = \"pr\"", "tier = \"weekly\"", "tier 'weekly'"),
            ("max-minutes = 180", "max-minutes = 20", "below min-minutes"),
            (
                "packages = [\"moirai-files\", \"moirai-diff\"]",
                "packages = []",
                "packages is empty",
            ),
            ("shards = 32", "shards = 0", "outside 1..=10000"),
            (
                "ram-budget = \"6GB\"",
                "ram-budget = 6",
                "needs the string 'ram-budget'",
            ),
            (
                "max-targets = 2",
                "max-targets = 2\nargs = []",
                "unknown key 'args'",
            ),
            (
                "parallel = 2",
                "parallel = 7",
                "parallel 7 exceeds build-jobs 6",
            ),
            (
                "grace-seconds = 120\n",
                "",
                "needs the integer 'grace-seconds'",
            ),
        ] {
            let e = with(edit, by).unwrap_err();
            assert!(e.contains(needle), "{edit} -> {by}: {e}");
        }
    }

    proptest! {
        #[test]
        fn whole_quantities_parse_exactly(n in 0u64..1_000_000, unit in 0usize..9) {
            let (u, m) = [("B", 1u64), ("KB", 1_000), ("MB", 1_000_000), ("GB", 1_000_000_000), ("TB", 1_000_000_000_000),
                ("KiB", 1 << 10), ("MiB", 1 << 20), ("GiB", 1 << 30), ("TiB", 1 << 40)][unit];
            prop_assert_eq!(parse_bytes(&format!("{n}{u}")), Ok(n * m));
        }
    }
}
