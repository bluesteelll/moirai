//! The run record ([MP §7.1]), its JSON form, raw files ([MP §7.2]), validity and exit grade ([MP §7.3]).

use crate::arm::{batch_for, valid_name};
use crate::condition::{Condition, MemoryWatch};
use crate::host::{HostKind, HostRecord};
use crate::stats::{Statistic, Summary, median};
use crate::tier::{Plan, Tier};
use crate::units::Unit;
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The schema name of a run record ([MP §7.1]).
pub const RUN_SCHEMA: &str = "moirai-probes/run/1";

/// One repetition of one arm ([MP §3.4], [MP §7.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepRecord {
    /// n, min, p50, p95, p99, max.
    pub summary: Summary,
    /// The median elapsed time of a sample, for bytes and counts arms ([MP §3.3]); `None` for time arms.
    pub median_elapsed_ns: Option<u64>,
    /// The samples in sampling order; `None` in a summary record.
    pub samples: Option<Vec<u64>>,
}

/// The duration that selects a tier for one repetition ([MP §3.3]): the p50 of a time arm's samples, the median
/// elapsed time of a bytes or counts arm's samples.
pub fn rep_duration(unit: Unit, rep: &RepRecord) -> u64 {
    match unit {
        Unit::Ns => rep.summary.p50,
        _ => rep.median_elapsed_ns.unwrap_or(0),
    }
}

/// One arm of a run ([MP §7.1] `arms`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArmRecord {
    /// The arm's name.
    pub name: String,
    /// The unit of its samples.
    pub unit: Unit,
    /// Operations per sample ([MP §4.5]).
    pub batch: u32,
    /// The pilot median: per-operation time for a time arm, elapsed time per sample otherwise ([MP §3.2]).
    pub pilot_median: u64,
    /// The tier of the pilot median.
    pub pilot_tier: Tier,
    /// The gate tier ([MP §3.3]).
    pub tier: Tier,
    /// One entry per repetition.
    pub reps: Vec<RepRecord>,
}

impl ArmRecord {
    /// The duration that selects a tier for one of this arm's repetitions ([`rep_duration`]).
    pub fn rep_duration(&self, rep: &RepRecord) -> u64 {
        rep_duration(self.unit, rep)
    }

    /// The per-repetition values of one statistic, in repetition order.
    pub fn per_rep(&self, s: Statistic) -> Vec<u64> {
        self.reps.iter().map(|r| r.summary.get(s)).collect()
    }

    /// The median over repetitions of every statistic ([MP §3.4]).
    pub fn medians(&self) -> Option<Summary> {
        Summary::median_over(&self.reps.iter().map(|r| r.summary).collect::<Vec<_>>())
    }

    /// The statistics the arm is gated on ([MP §3.3], [MP §3.4]): its gate tier's for a time arm, the maximum for a
    /// bytes or counts arm.
    pub fn gated(&self) -> &'static [Statistic] {
        match self.unit {
            Unit::Ns => self.tier.gated(),
            _ => &[Statistic::Max],
        }
    }

    /// The JSON form of one entry of [MP §7.1] `arms`; samples are written when the record has them.
    fn to_json(&self) -> Value {
        let reps: Vec<Value> = self
            .reps
            .iter()
            .map(|r| {
                let s = &r.summary;
                let mut o = Map::new();
                o.insert(
                    "summary".into(),
                    json!({ "n": s.n, "min": s.min, "p50": s.p50, "p95": s.p95, "p99": s.p99, "max": s.max }),
                );
                o.insert("median_elapsed_ns".into(), json!(r.median_elapsed_ns));
                if let Some(x) = &r.samples {
                    o.insert("samples".into(), json!(x));
                }
                Value::Object(o)
            })
            .collect();
        json!({
            "name": self.name, "unit": self.unit.as_str(), "batch": self.batch,
            "pilot_median": self.pilot_median, "pilot_tier": self.pilot_tier.as_str(), "tier": self.tier.as_str(),
            "repetitions": reps,
        })
    }

    /// Writes the same JSON as [`ArmRecord::to_json`] without building it, so a raw file never holds its samples twice
    /// in memory ([MP §7.2]).
    fn write_json<W: Write + ?Sized>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(b"{\"name\":")?;
        serde_json::to_writer(&mut *w, &self.name)?;
        write!(
            w,
            ",\"unit\":\"{}\",\"batch\":{},\"pilot_median\":{},\"pilot_tier\":\"{}\",\"tier\":\"{}\",\"repetitions\":[",
            self.unit.as_str(),
            self.batch,
            self.pilot_median,
            self.pilot_tier.as_str(),
            self.tier.as_str()
        )?;
        for (i, r) in self.reps.iter().enumerate() {
            if i > 0 {
                w.write_all(b",")?;
            }
            let s = &r.summary;
            write!(
                w,
                "{{\"summary\":{{\"n\":{},\"min\":{},\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}},\"median_elapsed_ns\":",
                s.n, s.min, s.p50, s.p95, s.p99, s.max
            )?;
            match r.median_elapsed_ns {
                Some(e) => write!(w, "{e}")?,
                None => w.write_all(b"null")?,
            }
            if let Some(x) = &r.samples {
                w.write_all(b",\"samples\":[")?;
                for (j, v) in x.iter().enumerate() {
                    if j > 0 {
                        w.write_all(b",")?;
                    }
                    write!(w, "{v}")?;
                }
                w.write_all(b"]")?;
            }
            w.write_all(b"}")?;
        }
        w.write_all(b"]}")
    }
}

/// The probe process's own readings at the end of the run ([MP §4.6]).
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct ProcessReadings {
    /// `Meter::private_peak`, if it could be read: the process's lifetime peak, so one process per run.
    pub private_peak: Option<u64>,
    /// The `CountingAlloc` high-water mark since the runner reset it before the pilot, if the binary installed it.
    pub heap_high_water: Option<u64>,
}

/// One run of one quantity under one condition ([MP §7.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunRecord {
    /// The measurement's row number.
    pub measurement: u32,
    /// The quantity's name.
    pub quantity: String,
    /// The condition.
    pub condition: Condition,
    /// The replay verdict of a loaded run ([MP §2.3] (a), (b)); `None` until the driver sets it.
    pub load_replay_valid: Option<bool>,
    /// The host kind and snapshots.
    pub host: HostRecord,
    /// The `rustc` version of the measured binaries.
    pub toolchain: String,
    /// The measured commit.
    pub commit: String,
    /// The start, RFC 3339 UTC with whole seconds.
    pub started: String,
    /// The end, likewise.
    pub ended: String,
    /// The timer's resolution ([MP §4.4]).
    pub timer_resolution_ns: u64,
    /// The plan of the recorded attempt.
    pub plan: Plan,
    /// The plans of discarded under-sampled attempts ([MP §3.3]).
    pub escalations: Vec<Plan>,
    /// The arms.
    pub arms: Vec<ArmRecord>,
    /// The round-boundary readings.
    pub memory: MemoryWatch,
    /// The probe process's readings.
    pub process: ProcessReadings,
    /// Why the run is invalid; empty for a valid run ([MP §7.3]).
    pub reasons: Vec<String>,
}

/// The tier-check reasons of [MP §3.3]: an arm repetition whose median duration falls in a tier the plan does not
/// cover. Empty for every run the runner records; a hand-edited or truncated record can fail it.
pub fn tier_reasons(plan: &Plan, arms: &[ArmRecord]) -> Vec<String> {
    let mut out = Vec::new();
    for a in arms {
        for (r, rep) in a.reps.iter().enumerate() {
            let t = Tier::of_duration(a.rep_duration(rep));
            if !plan.covers(t) {
                out.push(format!(
                    "arm {} repetition {r} is under-sampled: its median falls in {} and the plan takes {} × {}",
                    a.name,
                    t.as_str(),
                    plan.n,
                    plan.repetitions
                ));
            }
        }
    }
    out
}

/// The gate tier of an arm ([MP §3.3]): the tier of the median over repetitions of its per-repetition durations.
pub fn gate_tier(arm: &ArmRecord) -> Tier {
    let mut d: Vec<u64> = arm.reps.iter().map(|r| arm.rep_duration(r)).collect();
    Tier::of_duration(median(&mut d).unwrap_or(arm.pilot_median))
}

/// The fields a run record and an idle observation share ([MP §7.1]), borrowed from either: what they measure,
/// where, and when.
pub(crate) struct Header<'a> {
    pub measurement: u32,
    pub quantity: &'a str,
    pub condition: &'a Condition,
    pub load_replay_valid: Option<bool>,
    pub host: &'a HostRecord,
    pub toolchain: &'a str,
    pub commit: &'a str,
    pub started: &'a str,
    pub ended: &'a str,
}

/// The header fields as read from JSON.
pub(crate) struct OwnedHeader {
    pub measurement: u32,
    pub quantity: String,
    pub condition: Condition,
    pub load_replay_valid: Option<bool>,
    pub host: HostRecord,
    pub toolchain: String,
    pub commit: String,
    pub started: String,
    pub ended: String,
}

/// Why a measurement number, quantity name, condition and host kind break the protocol, if they do ([MP §2.1],
/// [MP §2.3], [MP §4.1]): the run is refused before its pilot, and a record carrying them when it is read back.
pub(crate) fn spec_problem(
    measurement: u32,
    quantity: &str,
    condition: &Condition,
    host: HostKind,
) -> Option<String> {
    if measurement == 0 {
        return Some("the measurement number must be at least 1".into());
    }
    if !valid_name(quantity) {
        return Some(format!("'{quantity}' is not a valid quantity name"));
    }
    if let Some(p) = condition.problem() {
        return Some(p);
    }
    if host == HostKind::Hosted && matches!(condition, Condition::Loaded { .. }) {
        return Some(
            "a hosted runner runs idle or under synthetic load, never loaded ([MP §2.1])".into(),
        );
    }
    None
}

impl Header<'_> {
    /// The header's rules ([MP §7.3]).
    pub fn check(&self) -> Result<(), String> {
        if let Some(p) = spec_problem(
            self.measurement,
            self.quantity,
            self.condition,
            self.host.kind,
        ) {
            return Err(p);
        }
        if self.load_replay_valid.is_some() && !matches!(self.condition, Condition::Loaded { .. }) {
            return Err("only a loaded run carries a replay verdict".into());
        }
        for s in [self.started, self.ended] {
            compact_stamp(s).ok_or_else(|| format!("'{s}' is not an RFC 3339 UTC stamp"))?;
        }
        Ok(())
    }

    /// Whether the run is valid ([MP §7.3]): no reason, and a loaded run's replay verdict is `true`.
    pub fn valid(&self, reasons: &[String]) -> bool {
        reasons.is_empty()
            && (!matches!(self.condition, Condition::Loaded { .. })
                || self.load_replay_valid == Some(true))
    }

    /// Why the run may not decide anything ([MP §7.3]); empty for an exit-grade run.
    pub fn disqualifications(&self, reasons: &[String]) -> Vec<String> {
        let mut out = reasons.to_vec();
        if matches!(self.condition, Condition::Loaded { .. })
            && self.load_replay_valid != Some(true)
        {
            out.push(match self.load_replay_valid {
                None => "the load replay verdict is not recorded".to_string(),
                _ => "the load replay was not valid".to_string(),
            });
        }
        if matches!(self.condition, Condition::Synthetic { .. }) {
            out.push("a synthetic run never decides".to_string());
        }
        out.extend(self.host.disqualifications());
        out
    }

    /// The JSON fields of the header, beside `schema` ([MP §7.1]).
    pub fn json_fields(&self, schema: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("schema".into(), json!(schema));
        m.insert("measurement".into(), json!(self.measurement));
        m.insert("quantity".into(), json!(self.quantity));
        m.insert("condition".into(), self.condition.to_json());
        m.insert("load_replay_valid".into(), json!(self.load_replay_valid));
        m.insert("host".into(), self.host.to_json());
        m.insert("toolchain".into(), json!(self.toolchain));
        m.insert("commit".into(), json!(self.commit));
        m.insert("started".into(), json!(self.started));
        m.insert("ended".into(), json!(self.ended));
        m
    }

    /// The raw file name of [MP §7.2]: `<quantity>.<condition kind>.<YYYYMMDDTHHMMSSZ>.json`.
    pub fn raw_file_name(&self) -> Result<String, String> {
        let stamp = compact_stamp(self.started)
            .ok_or_else(|| format!("'{}' is not an RFC 3339 UTC stamp", self.started))?;
        Ok(format!(
            "{}.{}.{stamp}.json",
            self.quantity,
            self.condition.kind().as_str()
        ))
    }
}

impl OwnedHeader {
    /// Reads the header of a record whose schema is `schema`.
    pub fn from_json(v: &Value, schema: &str) -> Result<OwnedHeader, String> {
        let found = v.get("schema").and_then(Value::as_str);
        if found != Some(schema) {
            return Err(format!("not a {schema} record (schema {found:?})"));
        }
        Ok(OwnedHeader {
            measurement: u32_of(v, "measurement")?,
            quantity: str_of(v, "quantity")?,
            condition: Condition::from_json(v.get("condition").ok_or("no 'condition'")?)?,
            load_replay_valid: match v.get("load_replay_valid") {
                Some(Value::Null) | None => None,
                Some(Value::Bool(b)) => Some(*b),
                _ => return Err("'load_replay_valid' is not a boolean or null".into()),
            },
            host: HostRecord::from_json(v.get("host").ok_or("no 'host'")?)?,
            toolchain: str_of(v, "toolchain")?,
            commit: str_of(v, "commit")?,
            started: str_of(v, "started")?,
            ended: str_of(v, "ended")?,
        })
    }
}

/// Creates `<private_root>/measurements/<measurement>/<name>` ([MP §7.2]) and writes `body` and a line feed to it
/// through a buffer; an existing file is never overwritten, and a file whose writing fails is removed.
pub(crate) fn write_raw_file(
    private_root: &Path,
    measurement: u32,
    name: &str,
    body: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()>,
) -> std::io::Result<PathBuf> {
    let dir = private_root
        .join("measurements")
        .join(measurement.to_string());
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    fn finish(
        file: std::fs::File,
        body: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut w = std::io::BufWriter::new(file);
        body(&mut w)?;
        w.write_all(b"\n")?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    }
    match finish(file, body) {
        Ok(()) => Ok(path),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

impl RunRecord {
    /// The shared fields.
    pub(crate) fn header(&self) -> Header<'_> {
        Header {
            measurement: self.measurement,
            quantity: &self.quantity,
            condition: &self.condition,
            load_replay_valid: self.load_replay_valid,
            host: &self.host,
            toolchain: &self.toolchain,
            commit: &self.commit,
            started: &self.started,
            ended: &self.ended,
        }
    }

    /// Whether the run is valid ([MP §7.3]): no reason, and a loaded run's replay verdict is `true`.
    pub fn valid(&self) -> bool {
        self.header().valid(&self.reasons)
    }

    /// Why the run may not decide anything ([MP §7.3]); empty for an exit-grade run.
    pub fn disqualifications(&self) -> Vec<String> {
        self.header().disqualifications(&self.reasons)
    }

    /// Whether the run is exit-grade ([MP §7.3]).
    pub fn exit_grade(&self) -> bool {
        self.disqualifications().is_empty()
    }

    /// Records the loaded run's replay verdict ([MP §2.3]); ignored for other conditions.
    pub fn set_load_replay(&mut self, valid: bool) {
        if matches!(self.condition, Condition::Loaded { .. }) {
            self.load_replay_valid = Some(valid);
        }
    }

    /// The arm named `name`.
    pub fn arm(&self, name: &str) -> Option<&ArmRecord> {
        self.arms.iter().find(|a| a.name == name)
    }

    /// The same record without samples: a summary record ([MP §7.1]). The samples are not copied.
    pub fn summary_record(&self) -> RunRecord {
        RunRecord {
            measurement: self.measurement,
            quantity: self.quantity.clone(),
            condition: self.condition.clone(),
            load_replay_valid: self.load_replay_valid,
            host: self.host.clone(),
            toolchain: self.toolchain.clone(),
            commit: self.commit.clone(),
            started: self.started.clone(),
            ended: self.ended.clone(),
            timer_resolution_ns: self.timer_resolution_ns,
            plan: self.plan,
            escalations: self.escalations.clone(),
            arms: self
                .arms
                .iter()
                .map(|a| ArmRecord {
                    name: a.name.clone(),
                    unit: a.unit,
                    batch: a.batch,
                    pilot_median: a.pilot_median,
                    pilot_tier: a.pilot_tier,
                    tier: a.tier,
                    reps: a
                        .reps
                        .iter()
                        .map(|r| RepRecord {
                            summary: r.summary,
                            median_elapsed_ns: r.median_elapsed_ns,
                            samples: None,
                        })
                        .collect(),
                })
                .collect(),
            memory: self.memory.clone(),
            process: self.process,
            reasons: self.reasons.clone(),
        }
    }

    /// Every field of [MP §7.1] but `arms`.
    fn head_json(&self) -> Map<String, Value> {
        let plan = |p: &Plan| json!({ "n": p.n, "repetitions": p.repetitions, "block": p.block });
        let mut m = self.header().json_fields(RUN_SCHEMA);
        m.insert(
            "timer_resolution_ns".into(),
            json!(self.timer_resolution_ns),
        );
        m.insert("plan".into(), plan(&self.plan));
        m.insert(
            "escalations".into(),
            Value::Array(self.escalations.iter().map(plan).collect()),
        );
        m.insert("memory".into(), self.memory.to_json());
        m.insert(
            "process".into(),
            json!({ "private_peak": self.process.private_peak,
                    "heap_high_water": self.process.heap_high_water }),
        );
        m.insert("reasons".into(), json!(self.reasons));
        m
    }

    /// The JSON form of [MP §7.1]; samples are written when the record has them.
    pub fn to_json(&self) -> Value {
        let mut m = self.head_json();
        m.insert(
            "arms".into(),
            Value::Array(self.arms.iter().map(ArmRecord::to_json).collect()),
        );
        Value::Object(m)
    }

    /// Writes [`RunRecord::to_json`]'s JSON to `w` without building it: only the fields other than the arms are held
    /// as a JSON tree, and the samples are streamed ([MP §7.2]).
    pub fn write_json<W: Write + ?Sized>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(b"{")?;
        for (k, v) in &self.head_json() {
            serde_json::to_writer(&mut *w, k)?;
            w.write_all(b":")?;
            serde_json::to_writer(&mut *w, v)?;
            w.write_all(b",")?;
        }
        w.write_all(b"\"arms\":[")?;
        for (i, a) in self.arms.iter().enumerate() {
            if i > 0 {
                w.write_all(b",")?;
            }
            a.write_json(w)?;
        }
        w.write_all(b"]}")
    }

    /// Reads a record written by [`RunRecord::to_json`] or [`RunRecord::write_json`], and checks it against the
    /// protocol ([MP §7.1], [MP §7.3]); a summary record (no samples) is accepted.
    pub fn from_json(v: &Value) -> Result<RunRecord, String> {
        let h = OwnedHeader::from_json(v, RUN_SCHEMA)?;
        let rec = RunRecord {
            measurement: h.measurement,
            quantity: h.quantity,
            condition: h.condition,
            load_replay_valid: h.load_replay_valid,
            host: h.host,
            toolchain: h.toolchain,
            commit: h.commit,
            started: h.started,
            ended: h.ended,
            timer_resolution_ns: u64_of(v, "timer_resolution_ns")?,
            plan: plan_of(v.get("plan").ok_or("no 'plan'")?)?,
            escalations: array_of(v, "escalations")?
                .iter()
                .map(plan_of)
                .collect::<Result<_, _>>()?,
            arms: array_of(v, "arms")?
                .iter()
                .map(arm_of)
                .collect::<Result<_, _>>()?,
            memory: MemoryWatch::from_json(v.get("memory").ok_or("no 'memory'")?)?,
            process: {
                let p = v.get("process").ok_or("no 'process'")?;
                ProcessReadings {
                    private_peak: opt_u64_of(p, "private_peak")?,
                    heap_high_water: opt_u64_of(p, "heap_high_water")?,
                }
            },
            reasons: strings_of(v, "reasons")?,
        };
        rec.check()?;
        Ok(rec)
    }

    /// The protocol's rules for a record ([MP §3], [MP §4], [MP §7.1], [MP §7.3]). A record the runner made passes;
    /// a truncated or edited one that breaks the protocol is refused.
    pub fn check(&self) -> Result<(), String> {
        self.header().check()?;
        if self.timer_resolution_ns == 0 {
            return Err("the timer resolution must be positive ([MP §4.4])".into());
        }
        let final_tier = self
            .plan
            .tier()
            .ok_or_else(|| format!("the plan {:?} is not a tier plan ([MP §3.1])", self.plan))?;
        let mut previous: Option<&Plan> = None;
        for e in &self.escalations {
            if e.tier().is_none() {
                return Err(format!(
                    "the escalation {e:?} is not a tier plan ([MP §3.3])"
                ));
            }
            if e.covers(final_tier) {
                return Err(format!(
                    "the escalation {e:?} already covers the final plan ([MP §3.3])"
                ));
            }
            if previous.is_some_and(|p| e.n <= p.n) {
                return Err("the escalations do not rise strictly ([MP §3.3])".into());
            }
            previous = Some(e);
        }
        if self.arms.is_empty() {
            return Err("a run has at least one arm".into());
        }
        let first_plan = self.escalations.first().unwrap_or(&self.plan);
        let with_samples = self
            .arms
            .iter()
            .flat_map(|a| &a.reps)
            .filter(|r| r.samples.is_some())
            .count();
        if with_samples != 0 && with_samples != self.arms.len() * self.plan.repetitions as usize {
            return Err("samples are recorded on every repetition or on none ([MP §7.1])".into());
        }
        let mut scratch = Vec::new();
        for (i, a) in self.arms.iter().enumerate() {
            if !valid_name(&a.name) || self.arms[..i].iter().any(|b| b.name == a.name) {
                return Err(format!("arm name '{}' is invalid or repeated", a.name));
            }
            let batch = match a.unit {
                Unit::Ns => batch_for(a.pilot_median, self.timer_resolution_ns),
                _ => 1,
            };
            if a.batch != batch {
                return Err(format!(
                    "arm {}: batch {} where [MP §4.5] gives {batch}",
                    a.name, a.batch
                ));
            }
            if a.pilot_tier != Tier::of_duration(a.pilot_median) {
                return Err(format!(
                    "arm {}: the pilot tier does not match the pilot median",
                    a.name
                ));
            }
            if !first_plan.covers(a.pilot_tier) {
                return Err(format!(
                    "arm {}: the first attempt's plan does not cover its pilot tier ([MP §3.2])",
                    a.name
                ));
            }
            if a.reps.len() != self.plan.repetitions as usize {
                return Err(format!(
                    "arm {}: {} repetitions, the plan takes {}",
                    a.name,
                    a.reps.len(),
                    self.plan.repetitions
                ));
            }
            if a.tier != gate_tier(a) {
                return Err(format!(
                    "arm {}: the gate tier does not match the repetitions",
                    a.name
                ));
            }
            for rep in &a.reps {
                let s = &rep.summary;
                if s.n != u64::from(self.plan.n) {
                    return Err(format!(
                        "arm {}: a repetition has {} samples, the plan takes {}",
                        a.name, s.n, self.plan.n
                    ));
                }
                if !(s.min <= s.p50 && s.p50 <= s.p95 && s.p95 <= s.p99 && s.p99 <= s.max) {
                    return Err(format!("arm {}: a summary is not ordered", a.name));
                }
                if rep.median_elapsed_ns.is_some() != (a.unit != Unit::Ns) {
                    return Err(format!(
                        "arm {}: 'median_elapsed_ns' is set exactly for bytes and counts arms",
                        a.name
                    ));
                }
                if let Some(x) = &rep.samples {
                    scratch.clear();
                    scratch.extend_from_slice(x);
                    if Summary::of(&mut scratch) != Some(rep.summary) {
                        return Err(format!(
                            "arm {}: a summary does not match its samples",
                            a.name
                        ));
                    }
                }
            }
        }
        let readings = u64::from(self.plan.repetitions) * (u64::from(self.plan.rounds()) + 1);
        self.memory.check(&self.condition, readings)?;
        let required = self
            .memory
            .reasons(&self.condition)
            .into_iter()
            .chain(tier_reasons(&self.plan, &self.arms));
        for r in required {
            if !self.reasons.contains(&r) {
                return Err(format!("the record omits the reason '{r}'"));
            }
        }
        Ok(())
    }

    /// The raw file name of [MP §7.2]: `<quantity>.<condition kind>.<YYYYMMDDTHHMMSSZ>.json`.
    pub fn raw_file_name(&self) -> Result<String, String> {
        self.header().raw_file_name()
    }

    /// Writes the record to `<private_root>/measurements/<n>/<raw file name>` ([MP §7.2]), creating the directories,
    /// streaming its samples ([`RunRecord::write_json`]); an existing file is never overwritten. `private_root` is
    /// the main worktree's `private/` directory.
    pub fn write_raw(&self, private_root: &Path) -> std::io::Result<PathBuf> {
        let name = self.raw_file_name().map_err(std::io::Error::other)?;
        write_raw_file(private_root, self.measurement, &name, |w| {
            self.write_json(w)
        })
    }
}

pub(crate) fn u64_of(v: &Value, k: &str) -> Result<u64, String> {
    v.get(k)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("'{k}' is not an unsigned integer"))
}

pub(crate) fn u32_of(v: &Value, k: &str) -> Result<u32, String> {
    u32::try_from(u64_of(v, k)?).map_err(|_| format!("'{k}' is out of range"))
}

fn opt_u64_of(v: &Value, k: &str) -> Result<Option<u64>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => x
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("'{k}' is not an unsigned integer or null")),
    }
}

fn str_of(v: &Value, k: &str) -> Result<String, String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("'{k}' is not a string"))
}

pub(crate) fn array_of<'a>(v: &'a Value, k: &str) -> Result<&'a Vec<Value>, String> {
    v.get(k)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("'{k}' is not an array"))
}

pub(crate) fn strings_of(v: &Value, k: &str) -> Result<Vec<String>, String> {
    array_of(v, k)?
        .iter()
        .map(|r| {
            r.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("an entry of '{k}' is not a string"))
        })
        .collect()
}

fn plan_of(v: &Value) -> Result<Plan, String> {
    Ok(Plan {
        n: u32_of(v, "n")?,
        repetitions: u32_of(v, "repetitions")?,
        block: u32_of(v, "block")?,
    })
}

fn tier_of(v: &Value, k: &str) -> Result<Tier, String> {
    v.get(k)
        .and_then(Value::as_str)
        .and_then(Tier::parse)
        .ok_or_else(|| format!("'{k}' is not a tier"))
}

fn arm_of(v: &Value) -> Result<ArmRecord, String> {
    let reps = array_of(v, "repetitions")?
        .iter()
        .map(|r| {
            let s = r.get("summary").ok_or("no 'summary'")?;
            Ok(RepRecord {
                summary: Summary {
                    n: u64_of(s, "n")?,
                    min: u64_of(s, "min")?,
                    p50: u64_of(s, "p50")?,
                    p95: u64_of(s, "p95")?,
                    p99: u64_of(s, "p99")?,
                    max: u64_of(s, "max")?,
                },
                median_elapsed_ns: opt_u64_of(r, "median_elapsed_ns")?,
                samples: match r.get("samples") {
                    None => None,
                    Some(x) => Some(
                        x.as_array()
                            .ok_or("'samples' is not an array")?
                            .iter()
                            .map(|e| e.as_u64().ok_or("a sample is not an unsigned integer"))
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ArmRecord {
        name: str_of(v, "name")?,
        unit: v
            .get("unit")
            .and_then(Value::as_str)
            .and_then(Unit::parse)
            .ok_or("'unit' is not a unit")?,
        batch: u32_of(v, "batch")?,
        pilot_median: u64_of(v, "pilot_median")?,
        pilot_tier: tier_of(v, "pilot_tier")?,
        tier: tier_of(v, "tier")?,
        reps,
    })
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// A time as RFC 3339 UTC with whole seconds, `YYYY-MM-DDTHH:MM:SSZ` ([MP §7.1]); times before 1970 read as 1970.
pub fn utc_stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3_600,
        s / 60 % 60,
        s % 60
    )
}

/// `YYYY-MM-DDTHH:MM:SSZ` → `YYYYMMDDTHHMMSSZ` ([MP §7.2]); `None` for any other form.
pub fn compact_stamp(stamp: &str) -> Option<String> {
    let b = stamp.as_bytes();
    let digits_at = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let shape = b.len() == 20
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && b[19] == b'Z'
        && digits_at.iter().all(|&i| b[i].is_ascii_digit());
    shape.then(|| {
        let mut s = String::with_capacity(16);
        for (i, &c) in b.iter().enumerate() {
            if !matches!(i, 4 | 7 | 13 | 16) {
                s.push(c as char);
            }
        }
        s
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::condition::IDLE_FLOOR;
    use crate::host::HostSnapshot;
    use crate::host::tests::Canned;
    use crate::testkit::{meter_err, scratch_dir};
    use std::time::Duration;

    /// A small, valid, exit-grade record with two arms and samples.
    pub(crate) fn sample_record() -> RunRecord {
        let snap = HostSnapshot::take(&mut Canned::good());
        let plan = Plan::new(20, 3);
        let arm = |name: &str, base: u64| {
            let reps = (0..3)
                .map(|r| {
                    let samples: Vec<u64> = (0..20).map(|i| base + i * 1_000_000 + r).collect();
                    let mut sorted = samples.clone();
                    RepRecord {
                        summary: Summary::of(&mut sorted).unwrap(),
                        median_elapsed_ns: None,
                        samples: Some(samples),
                    }
                })
                .collect();
            let mut a = ArmRecord {
                name: name.into(),
                unit: Unit::Ns,
                batch: 1,
                pilot_median: base + 10_000_000,
                pilot_tier: Tier::of_duration(base + 10_000_000),
                tier: Tier::T4,
                reps,
            };
            a.tier = gate_tier(&a);
            a
        };
        RunRecord {
            measurement: 11,
            quantity: "spawn.empty".into(),
            condition: Condition::Idle,
            load_replay_valid: None,
            host: HostRecord {
                kind: HostKind::Laptop,
                start: snap.clone(),
                end: snap,
            },
            toolchain: "1.98.1".into(),
            commit: "0123456789abcdef0123456789abcdef01234567".into(),
            started: "2026-10-04T09:30:00Z".into(),
            ended: "2026-10-04T09:41:07Z".into(),
            timer_resolution_ns: 100,
            plan,
            escalations: vec![],
            arms: vec![arm("op", 2_000_000_000), arm("floor", 1_500_000_000)],
            memory: MemoryWatch {
                readings: 63,
                min: Some(9_000_000_000),
                max: Some(9_100_000_000),
                ..Default::default()
            },
            process: ProcessReadings {
                private_peak: Some(3_100_000),
                heap_high_water: None,
            },
            reasons: vec![],
        }
    }

    #[test]
    fn json_round_trip_and_summary_records() {
        let r = sample_record();
        assert_eq!(r.check(), Ok(()));
        assert!(r.valid() && r.exit_grade(), "{:?}", r.disqualifications());
        let back = RunRecord::from_json(&r.to_json()).unwrap();
        assert_eq!(back, r);
        let s = r.summary_record();
        assert!(
            s.to_json()["arms"][0]["repetitions"][0]
                .get("samples")
                .is_none()
        );
        assert_eq!(RunRecord::from_json(&s.to_json()), Ok(s));
        assert_eq!(r.arm("floor").unwrap().tier, Tier::T4);
        assert_eq!(r.arm("op").unwrap().gated(), &[Statistic::Max]);
    }

    #[test]
    fn streamed_json_equals_the_tree() {
        let mut r = sample_record();
        r.arms[0].name = "op \"quoted\" \\ é".into();
        r.arms[1].reps[0].samples = Some(vec![]);
        for rec in [sample_record(), sample_record().summary_record(), r] {
            let mut buf = Vec::new();
            rec.write_json(&mut buf).unwrap();
            let parsed: Value = serde_json::from_slice(&buf).unwrap();
            assert_eq!(parsed, rec.to_json());
        }
    }

    #[test]
    fn a_failed_reading_round_trips() {
        let mut r = sample_record();
        r.memory
            .observe(&r.condition.clone(), Err(meter_err("GlobalMemoryStatusEx")));
        r.memory.readings -= 1;
        r.memory.failures = 1;
        assert!(r.check().is_err(), "the failure's reason is required");
        r.reasons = r.memory.reasons(&r.condition);
        assert_eq!(r.check(), Ok(()));
        let back = RunRecord::from_json(&r.to_json()).unwrap();
        assert_eq!(back, r);
        assert_eq!(
            back.memory.first_failure.as_deref(),
            Some(meter_err("GlobalMemoryStatusEx").to_string().as_str())
        );
        assert!(!back.valid());
    }

    #[test]
    fn from_json_refuses_broken_records() {
        let r = sample_record();
        // Each case: the substring its refusal must contain, and the edit.
        let mut cases: Vec<(&str, Value)> = Vec::new();
        let mut edit = |expect: &'static str, f: &dyn Fn(&mut Value)| {
            let mut v = r.to_json();
            f(&mut v);
            cases.push((expect, v));
        };
        edit("not a moirai-probes/run/1 record", &|v| {
            v["schema"] = json!("moirai-probes/run/2");
        });
        edit("does not match its samples", &|v| {
            v["arms"][0]["repetitions"][0]["samples"][0] = json!(1);
        });
        edit("is not a tier plan", &|v| v["plan"]["block"] = json!(2));
        edit("invalid or repeated", &|v| {
            v["arms"][1]["name"] = json!("op")
        });
        edit("only a loaded run carries", &|v| {
            v["load_replay_valid"] = json!(true);
        });
        edit("RFC 3339", &|v| v["started"] = json!("2026-10-04 09:30:00"));
        edit("gate tier does not match", &|v| {
            v["arms"][0]["tier"] = json!("t1")
        });
        edit("'median_elapsed_ns' is set exactly", &|v| {
            v["arms"][0]["repetitions"][0]["median_elapsed_ns"] = json!(5);
        });
        edit("BLAKE3-256", &|v| {
            v["condition"] = json!({"kind": "loaded", "fixture": "x"});
        });
        edit("hosted runner", &|v| {
            v["condition"] = json!({"kind": "loaded", "fixture": "0f".repeat(32)});
            v["host"]["kind"] = json!("hosted");
        });
        // The memory readings ([MP §7.3]). The review's case: 40 failures and 23 readings out of band, no reason.
        edit("omits the reason '40 of 63", &|v| {
            v["memory"]["failures"] = json!(40);
            v["memory"]["out_of_band"] = json!(23);
            v["memory"]["min"] = json!(1_000_000_000);
            v["memory"]["max"] = json!(1_400_000_000);
            v["memory"]["first_failure"] = json!("GlobalMemoryStatusEx failed");
        });
        edit("omits the reason '1 of 63", &|v| {
            v["memory"]["out_of_band"] = json!(1);
            v["memory"]["min"] = json!(IDLE_FLOOR - 1);
        });
        edit("more failed and out-of-band readings than readings", &|v| {
            v["memory"]["failures"] = json!(60);
            v["memory"]["out_of_band"] = json!(10);
            v["memory"]["first_failure"] = json!("x");
        });
        edit("62 readings, the protocol takes 63", &|v| {
            v["memory"]["readings"] = json!(62);
        });
        edit("0 readings, the protocol takes 63", &|v| {
            v["memory"]["readings"] = json!(0);
            v["memory"]["min"] = json!(null);
            v["memory"]["max"] = json!(null);
        });
        edit("contradicts", &|v| {
            v["memory"]["min"] = json!(IDLE_FLOOR - 1)
        });
        edit("'first_failure' is set exactly", &|v| {
            v["memory"]["first_failure"] = json!("x");
        });
        // The timer, the plans and the escalations.
        edit("timer resolution must be positive", &|v| {
            v["timer_resolution_ns"] = json!(0);
        });
        edit("is not a tier plan", &|v| {
            v["plan"] = json!({"n": 20, "repetitions": 5, "block": 1});
        });
        edit("escalation Plan { n: 500", &|v| {
            v["escalations"] = json!([{"n": 500, "repetitions": 5, "block": 25}]);
        });
        edit("already covers the final plan", &|v| {
            v["escalations"] = json!([{"n": 20, "repetitions": 3, "block": 1}]);
        });
        // The batch and the samples.
        edit("batch 2 where [MP §4.5] gives 1", &|v| {
            v["arms"][0]["batch"] = json!(2);
        });
        edit("every repetition or on none", &|v| {
            v["arms"][1]["repetitions"][2]
                .as_object_mut()
                .unwrap()
                .remove("samples");
        });
        edit("is not ordered", &|v| {
            for a in 0..2 {
                for rep in v["arms"][a]["repetitions"].as_array_mut().unwrap() {
                    rep.as_object_mut().unwrap().remove("samples");
                }
            }
            v["arms"][0]["repetitions"][0]["summary"]["p95"] = json!(1);
        });
        edit("an entry of 'reasons' is not a string", &|v| {
            v["reasons"] = json!([1]);
        });
        for (expect, v) in cases {
            let e = RunRecord::from_json(&v).unwrap_err();
            assert!(e.contains(expect), "expected '{expect}', got '{e}'");
        }
    }

    #[test]
    fn escalations_rise_strictly_below_the_final_plan() {
        let mut r = sample_record();
        // A t1 run whose first attempts were 20 × 3 and 200 × 5.
        let plan = Plan::new(10_000, 5);
        for a in &mut r.arms {
            a.pilot_median = 2_000_000_000;
            a.pilot_tier = Tier::T4;
            a.reps = (0..5)
                .map(|_| {
                    let mut s: Vec<u64> = (0..10_000).collect();
                    RepRecord {
                        summary: Summary::of(&mut s).unwrap(),
                        median_elapsed_ns: None,
                        samples: None,
                    }
                })
                .collect();
            a.tier = gate_tier(a);
        }
        r.plan = plan;
        r.memory.readings = 5 * 101;
        r.escalations = vec![Plan::new(20, 3), Plan::new(200, 5)];
        assert_eq!(r.check(), Ok(()));
        r.escalations = vec![Plan::new(200, 5), Plan::new(20, 3)];
        assert!(r.check().is_err(), "falling escalations");
        r.escalations = vec![Plan::new(200, 5), Plan::new(200, 5)];
        assert!(r.check().is_err(), "repeated escalation");
        // The first attempt covers every pilot tier: a t1 pilot cannot start at 20 × 3.
        r.escalations = vec![Plan::new(20, 3)];
        r.arms[0].pilot_median = 500;
        r.arms[0].pilot_tier = Tier::T1;
        r.arms[0].batch = batch_for(500, r.timer_resolution_ns);
        assert!(r.check().unwrap_err().contains("first attempt"));
    }

    #[test]
    fn validity_and_exit_grade() {
        let mut r = sample_record();
        r.condition = Condition::Loaded {
            fixture: "0f".repeat(32),
        };
        assert!(!r.valid());
        assert!(
            r.disqualifications()
                .iter()
                .any(|d| d.contains("not recorded"))
        );
        r.set_load_replay(false);
        assert!(!r.valid());
        r.set_load_replay(true);
        assert!(r.valid() && r.exit_grade());
        r.reasons.push("x".into());
        assert!(!r.valid() && !r.exit_grade());
        let mut s = sample_record();
        s.set_load_replay(true);
        assert_eq!(s.load_replay_valid, None);
        s.condition = Condition::Synthetic {
            description: "cpu stress".into(),
        };
        assert!(s.valid() && !s.exit_grade());
    }

    #[test]
    fn tier_check_reasons() {
        let mut r = sample_record();
        // A 1 ms-scale repetition in a 20 × 3 plan is under-sampled.
        let a = &mut r.arms[0];
        let samples: Vec<u64> = (0..20).map(|i| 1_000_000 + i).collect();
        let mut sorted = samples.clone();
        a.reps[0] = RepRecord {
            summary: Summary::of(&mut sorted).unwrap(),
            median_elapsed_ns: None,
            samples: Some(samples),
        };
        a.tier = gate_tier(a);
        let reasons = tier_reasons(&r.plan, &r.arms);
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert!(reasons[0].contains("t2"));
        assert!(r.check().is_err(), "a record must carry its tier reasons");
        r.reasons = reasons;
        assert_eq!(r.check(), Ok(()));
        assert!(!r.valid());
    }

    #[test]
    fn stamps() {
        assert_eq!(utc_stamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(1_791_106_200)),
            "2026-10-04T09:30:00Z"
        );
        assert_eq!(
            utc_stamp(UNIX_EPOCH + Duration::from_secs(4_102_444_799)),
            "2099-12-31T23:59:59Z"
        );
        assert_eq!(
            compact_stamp("2026-10-04T09:30:00Z").as_deref(),
            Some("20261004T093000Z")
        );
        for bad in [
            "2026-10-04T09:30:00",
            "2026-10-04 09:30:00Z",
            "2026-1a-04T09:30:00Z",
            "",
        ] {
            assert_eq!(compact_stamp(bad), None);
        }
        let now = utc_stamp(SystemTime::now());
        assert!(compact_stamp(&now).is_some(), "{now}");
    }

    #[test]
    fn raw_files_are_never_overwritten() {
        let root = scratch_dir("raw");
        let r = sample_record();
        assert_eq!(
            r.raw_file_name().as_deref(),
            Ok("spawn.empty.idle.20261004T093000Z.json")
        );
        let p = r.write_raw(&root).unwrap();
        assert!(p.ends_with("measurements/11/spawn.empty.idle.20261004T093000Z.json"));
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.ends_with("}\n"));
        let back = RunRecord::from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(back, r);
        assert!(r.write_raw(&root).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), text, "kept as it was");
        // A body that fails leaves no file behind.
        let e = write_raw_file(&root, 11, "broken.json", |_| {
            Err(std::io::Error::other("disk full"))
        });
        assert!(e.is_err());
        assert!(!root.join("measurements/11/broken.json").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
