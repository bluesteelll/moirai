//! Host kinds and the host snapshot: the Windows build and the Defender versions recorded at the start and the end
//! of every run ([MP §2.1], [MP §2.2]).

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Where a run was made ([MP §2.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum HostKind {
    /// The owner's machine: the only host that decides a gate.
    Laptop,
    /// A GitHub-hosted Windows runner: noise bands only.
    Hosted,
}

impl HostKind {
    /// The record spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            HostKind::Laptop => "laptop",
            HostKind::Hosted => "hosted",
        }
    }

    /// The inverse of [`HostKind::as_str`].
    pub fn parse(s: &str) -> Option<HostKind> {
        match s {
            "laptop" => Some(HostKind::Laptop),
            "hosted" => Some(HostKind::Hosted),
            _ => None,
        }
    }
}

/// The Defender properties of [MP §2.2].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Defender {
    /// `AMProductVersion`.
    pub product: String,
    /// `AMEngineVersion`.
    pub engine: String,
    /// `AntivirusSignatureVersion`.
    pub signatures: String,
    /// `RealTimeProtectionEnabled`.
    pub realtime: bool,
}

/// One host snapshot ([MP §2.2]); each source is its value or the reason it could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostSnapshot {
    /// The Windows version, `<major>.<minor>.<build>[.<revision>]`.
    pub windows: Result<String, String>,
    /// The Defender properties.
    pub defender: Result<Defender, String>,
}

/// A Windows system program and its arguments ([MP §2.2]). The program is named by its path below `%SystemRoot%`, so
/// a program of the same name beside the probe binary or on `PATH` is never run in its place.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SystemCommand {
    /// The path below `%SystemRoot%`, one component per entry.
    pub path: &'static [&'static str],
    /// The arguments.
    pub args: &'static [&'static str],
}

impl SystemCommand {
    /// The program's file name, for messages.
    pub fn program(&self) -> &'static str {
        self.path.last().copied().unwrap_or("")
    }

    /// The program's full path below `system_root` (the value of `%SystemRoot%`), which must be set and absolute. An
    /// error never quotes the value: errors reach records and aggregates, which hold no paths ([MP §7.4]).
    pub fn resolve(&self, system_root: Option<&std::ffi::OsStr>) -> Result<PathBuf, String> {
        let root = system_root
            .filter(|r| !r.is_empty())
            .ok_or_else(|| format!("{}: %SystemRoot% is not set", self.program()))?;
        let root = Path::new(root);
        if !root.is_absolute() {
            return Err(format!(
                "{}: %SystemRoot% is not an absolute path",
                self.program()
            ));
        }
        Ok(self.path.iter().fold(root.to_path_buf(), |p, c| p.join(c)))
    }
}

/// `cmd.exe /d /c ver`, which prints the Windows version ([MP §2.2]).
pub const VER_COMMAND: SystemCommand = SystemCommand {
    path: &["System32", "cmd.exe"],
    args: &["/d", "/c", "ver"],
};

/// `powershell.exe … Get-MpComputerStatus …`, which prints the Defender properties as JSON ([MP §2.2]).
pub const DEFENDER_COMMAND: SystemCommand = SystemCommand {
    path: &["System32", "WindowsPowerShell", "v1.0", "powershell.exe"],
    args: &[
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-MpComputerStatus | Select-Object AMProductVersion,AMEngineVersion,AntivirusSignatureVersion,\
         RealTimeProtectionEnabled | ConvertTo-Json -Compress",
    ],
};

/// Runs a system program and returns its standard output; an error for a failed start, a non-zero exit or a program
/// that does not finish in time.
pub trait CommandRunner {
    /// Runs `cmd`.
    fn run(&mut self, cmd: &SystemCommand) -> Result<Vec<u8>, String>;
}

/// How long [`SystemRunner`] waits for a snapshot command by default before it stops it ([MP §2.2]).
pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// How often [`SystemRunner`] checks whether its child has exited.
const POLL: Duration = Duration::from_millis(10);

/// [`CommandRunner`] over `std::process::Command`: the program is resolved below `%SystemRoot%`, and a child that has
/// not finished within `timeout` is killed and reported as the source's error ([MP §2.2]).
#[derive(Clone, Copy, Debug)]
pub struct SystemRunner {
    /// The longest wait for one command.
    pub timeout: Duration,
}

impl Default for SystemRunner {
    fn default() -> SystemRunner {
        SystemRunner {
            timeout: COMMAND_TIMEOUT,
        }
    }
}

impl CommandRunner for SystemRunner {
    fn run(&mut self, cmd: &SystemCommand) -> Result<Vec<u8>, String> {
        use std::io::Read as _;
        use std::process::{Command, Stdio};
        let program = cmd.program();
        let path = cmd.resolve(std::env::var_os("SystemRoot").as_deref())?;
        let deadline = Instant::now() + self.timeout;
        let mut child = Command::new(&path)
            .args(cmd.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{program} could not be started: {e}"))?;
        // The output is read on its own thread, so a child that fills the pipe cannot stall the wait below.
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(mut stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = tx.send(stdout.read_to_end(&mut buf).map(|_| buf));
            });
        }
        let stop = |child: &mut std::process::Child, why: String| {
            let _ = child.kill();
            let _ = child.wait();
            Err(why)
        };
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if Instant::now() >= deadline => {
                    return stop(
                        &mut child,
                        format!(
                            "{program} did not finish within {:?} and was stopped",
                            self.timeout
                        ),
                    );
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => {
                    return stop(
                        &mut child,
                        format!("{program} could not be waited for: {e}"),
                    );
                }
            }
        };
        let wait = deadline.saturating_duration_since(Instant::now()).max(POLL);
        let out = match rx.recv_timeout(wait) {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => return Err(format!("{program}: its output could not be read: {e}")),
            Err(_) => {
                return Err(format!(
                    "{program}: its output did not close within {:?}",
                    self.timeout
                ));
            }
        };
        if status.success() {
            Ok(out)
        } else {
            Err(format!("{program} exited with {status}"))
        }
    }
}

impl HostSnapshot {
    /// Takes a snapshot with `runner` ([MP §2.2]).
    pub fn take<R: CommandRunner + ?Sized>(runner: &mut R) -> HostSnapshot {
        let windows = runner.run(&VER_COMMAND).and_then(|o| parse_ver(&o));
        let defender = runner
            .run(&DEFENDER_COMMAND)
            .and_then(|o| parse_defender(&o));
        HostSnapshot { windows, defender }
    }

    /// The JSON form of [MP §7.1]: each source as `{"ok": …}` or `{"error": "…"}`.
    pub fn to_json(&self) -> Value {
        let windows = match &self.windows {
            Ok(v) => json!({ "ok": v }),
            Err(e) => json!({ "error": e }),
        };
        let defender = match &self.defender {
            Ok(d) => json!({ "ok": {
                "product": d.product,
                "engine": d.engine,
                "signatures": d.signatures,
                "realtime": d.realtime,
            } }),
            Err(e) => json!({ "error": e }),
        };
        json!({ "windows": windows, "defender": defender })
    }

    /// The inverse of [`HostSnapshot::to_json`].
    pub fn from_json(v: &Value) -> Result<HostSnapshot, String> {
        fn either<T>(
            v: Option<&Value>,
            what: &str,
            ok: impl FnOnce(&Value) -> Option<T>,
        ) -> Result<Result<T, String>, String> {
            let o = v
                .and_then(Value::as_object)
                .ok_or_else(|| format!("host snapshot: '{what}' is not an object"))?;
            match (o.get("ok"), o.get("error").and_then(Value::as_str), o.len()) {
                (Some(x), None, 1) => ok(x)
                    .map(Ok)
                    .ok_or_else(|| format!("host snapshot: malformed '{what}'")),
                (None, Some(e), 1) => Ok(Err(e.to_string())),
                _ => Err(format!(
                    "host snapshot: '{what}' needs exactly one of 'ok' and 'error'"
                )),
            }
        }
        let windows = either(v.get("windows"), "windows", |x| {
            x.as_str().map(str::to_string)
        })?;
        let defender = either(v.get("defender"), "defender", |x| {
            Some(Defender {
                product: x.get("product")?.as_str()?.to_string(),
                engine: x.get("engine")?.as_str()?.to_string(),
                signatures: x.get("signatures")?.as_str()?.to_string(),
                realtime: x.get("realtime")?.as_bool()?,
            })
        })?;
        Ok(HostSnapshot { windows, defender })
    }
}

/// Whether `s` is a dotted Windows version: three or four non-empty decimal fields.
fn is_dotted_version(s: &str) -> bool {
    let fields: Vec<&str> = s.split('.').collect();
    (3..=4).contains(&fields.len())
        && fields
            .iter()
            .all(|f| !f.is_empty() && f.len() <= 10 && f.bytes().all(|b| b.is_ascii_digit()))
}

/// The Windows version from `ver`'s output ([MP §2.2]): the last space-separated token inside the last `[`…`]` pair.
/// The output is in the console code page; only the ASCII version token is read, so any display language works
/// ("Microsoft Windows [Version 10.0.26200.6584]", or the same with a localised word).
pub fn parse_ver(out: &[u8]) -> Result<String, String> {
    let open = out
        .iter()
        .rposition(|&b| b == b'[')
        .ok_or("ver: no '[' in the output")?;
    let close = out[open..]
        .iter()
        .position(|&b| b == b']')
        .ok_or("ver: no ']' after the last '['")?;
    let inside = &out[open + 1..open + close];
    let token = inside
        .split(|&b| b == b' ' || b == b'\t')
        .rfind(|t| !t.is_empty())
        .ok_or("ver: nothing inside the brackets")?;
    let token = std::str::from_utf8(token).map_err(|_| "ver: the version token is not ASCII")?;
    if is_dotted_version(token) {
        Ok(token.to_string())
    } else {
        Err(format!("ver: '{token}' is not a dotted version"))
    }
}

/// The Defender properties from `Get-MpComputerStatus … | ConvertTo-Json -Compress` ([MP §2.2]). A UTF-8 byte-order
/// mark and surrounding whitespace are ignored.
pub fn parse_defender(out: &[u8]) -> Result<Defender, String> {
    let body = out.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(out);
    let text = std::str::from_utf8(body).map_err(|_| "Defender: the output is not UTF-8")?;
    let v: Value = serde_json::from_str(text.trim())
        .map_err(|e| format!("Defender: the output is not JSON: {e}"))?;
    let s = |k: &str| -> Result<String, String> {
        match v.get(k) {
            Some(Value::String(x)) if !x.is_empty() => Ok(x.clone()),
            _ => Err(format!(
                "Defender: '{k}' is missing or not a non-empty string"
            )),
        }
    };
    Ok(Defender {
        product: s("AMProductVersion")?,
        engine: s("AMEngineVersion")?,
        signatures: s("AntivirusSignatureVersion")?,
        realtime: v
            .get("RealTimeProtectionEnabled")
            .and_then(Value::as_bool)
            .ok_or("Defender: 'RealTimeProtectionEnabled' is missing or not a boolean")?,
    })
}

/// The host record of a run: its kind and the snapshots at its start and end ([MP §2.2], [MP §7.1] `host`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostRecord {
    /// The host kind.
    pub kind: HostKind,
    /// The snapshot taken before the pilot.
    pub start: HostSnapshot,
    /// The snapshot taken after the last repetition.
    pub end: HostSnapshot,
}

impl HostRecord {
    /// Why the host disqualifies the run from deciding anything ([MP §7.3]); empty when it qualifies.
    pub fn disqualifications(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.kind != HostKind::Laptop {
            out.push(format!("host kind {} never decides", self.kind.as_str()));
        }
        for (when, s) in [("start", &self.start), ("end", &self.end)] {
            if let Err(e) = &s.windows {
                out.push(format!("Windows version unread at the {when}: {e}"));
            }
            match &s.defender {
                Err(e) => out.push(format!("Defender unread at the {when}: {e}")),
                Ok(d) if !d.realtime => {
                    out.push(format!("Defender real-time protection off at the {when}"))
                }
                Ok(_) => {}
            }
        }
        if self.start.windows.is_ok()
            && self.start.defender.is_ok()
            && self.end.windows.is_ok()
            && self.end.defender.is_ok()
            && self.start != self.end
        {
            out.push("the Windows or Defender versions changed during the run".to_string());
        }
        out
    }

    /// The JSON form ([MP §7.1]).
    pub fn to_json(&self) -> Value {
        json!({ "kind": self.kind.as_str(), "start": self.start.to_json(), "end": self.end.to_json() })
    }

    /// The inverse of [`HostRecord::to_json`].
    pub fn from_json(v: &Value) -> Result<HostRecord, String> {
        Ok(HostRecord {
            kind: v
                .get("kind")
                .and_then(Value::as_str)
                .and_then(HostKind::parse)
                .ok_or("host: unknown 'kind'")?,
            start: HostSnapshot::from_json(v.get("start").ok_or("host: no 'start'")?)?,
            end: HostSnapshot::from_json(v.get("end").ok_or("host: no 'end'")?)?,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const DEFENDER_JSON: &str = r#"{"AMProductVersion":"4.18.25080.5","AMEngineVersion":"1.1.25080.4","AntivirusSignatureVersion":"1.437.82.0","RealTimeProtectionEnabled":true}"#;

    /// Replays canned outputs per program name.
    pub(crate) struct Canned {
        pub ver: Result<Vec<u8>, String>,
        pub defender: Result<Vec<u8>, String>,
        pub calls: Vec<String>,
    }

    impl Canned {
        pub(crate) fn good() -> Canned {
            Canned {
                ver: Ok(b"\r\nMicrosoft Windows [Version 10.0.26200.6584]\r\n".to_vec()),
                defender: Ok(DEFENDER_JSON.as_bytes().to_vec()),
                calls: Vec::new(),
            }
        }
    }

    impl CommandRunner for Canned {
        fn run(&mut self, cmd: &SystemCommand) -> Result<Vec<u8>, String> {
            self.calls
                .push(format!("{} {}", cmd.path.join("\\"), cmd.args.join(" ")));
            match cmd.program() {
                "cmd.exe" => self.ver.clone(),
                "powershell.exe" => self.defender.clone(),
                p => Err(format!("unexpected program {p}")),
            }
        }
    }

    #[test]
    fn system_programs_resolve_below_system_root() {
        let root = std::env::temp_dir();
        let p = VER_COMMAND.resolve(Some(root.as_os_str())).unwrap();
        assert_eq!(p, root.join("System32").join("cmd.exe"));
        let p = DEFENDER_COMMAND.resolve(Some(root.as_os_str())).unwrap();
        assert!(p.starts_with(&root) && p.ends_with("WindowsPowerShell/v1.0/powershell.exe"));
        assert_eq!(DEFENDER_COMMAND.program(), "powershell.exe");
        for bad in [
            None,
            Some(std::ffi::OsStr::new("")),
            Some(std::ffi::OsStr::new("Windows")),
        ] {
            assert!(VER_COMMAND.resolve(bad).is_err(), "{bad:?}");
        }
    }

    /// The real runner, where `%SystemRoot%` exists (Windows): `ver` runs from System32, and a child that outlives
    /// the timeout is stopped and reported.
    #[test]
    fn system_runner_runs_and_bounds_system_programs() {
        if std::env::var_os("SystemRoot").is_none() {
            assert!(SystemRunner::default().run(&VER_COMMAND).is_err());
            return;
        }
        let out = SystemRunner::default().run(&VER_COMMAND).unwrap();
        assert!(parse_ver(&out).is_ok(), "{}", String::from_utf8_lossy(&out));
        let slow = SystemCommand {
            path: &["System32", "PING.EXE"],
            args: &["-n", "30", "127.0.0.1"],
        };
        let started = Instant::now();
        let e = SystemRunner {
            timeout: Duration::from_millis(200),
        }
        .run(&slow)
        .unwrap_err();
        assert!(e.contains("did not finish within 200ms"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let missing = SystemCommand {
            path: &["System32", "no-such-program-moirai.exe"],
            args: &[],
        };
        assert!(
            SystemRunner::default()
                .run(&missing)
                .unwrap_err()
                .contains("could not be started")
        );
    }

    #[test]
    fn ver_in_any_language() {
        assert_eq!(
            parse_ver(b"\r\nMicrosoft Windows [Version 10.0.26200.6584]\r\n").as_deref(),
            Ok("10.0.26200.6584")
        );
        // "Версия" in code page 866, as a Russian console prints it.
        let mut ru = b"\r\nMicrosoft Windows [".to_vec();
        ru.extend_from_slice(&[0x82, 0xA5, 0xE0, 0xE1, 0xA8, 0xEF]);
        ru.extend_from_slice(b" 10.0.19045.3803]\r\n");
        assert_eq!(parse_ver(&ru).as_deref(), Ok("10.0.19045.3803"));
        assert_eq!(
            parse_ver(b"[Version 10.0.22631]").as_deref(),
            Ok("10.0.22631")
        );
        for bad in [
            &b"Microsoft Windows"[..],
            b"[Version 10.0]",
            b"[Version x.y.z]",
            b"[ ]",
            b"[Version 10.0.1",
        ] {
            assert!(
                parse_ver(bad).is_err(),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
    }

    #[test]
    fn defender_properties() {
        let d = parse_defender(DEFENDER_JSON.as_bytes()).unwrap();
        assert_eq!(
            (
                d.product.as_str(),
                d.engine.as_str(),
                d.signatures.as_str(),
                d.realtime
            ),
            ("4.18.25080.5", "1.1.25080.4", "1.437.82.0", true)
        );
        let mut bom = b"\xEF\xBB\xBF  ".to_vec();
        bom.extend_from_slice(DEFENDER_JSON.as_bytes());
        bom.extend_from_slice(b"\r\n");
        assert_eq!(parse_defender(&bom), Ok(d));
        assert!(parse_defender(b"").is_err());
        assert!(parse_defender(b"{\"AMProductVersion\":\"1\"}").is_err());
        assert!(
            parse_defender(
                br#"{"AMProductVersion":"1","AMEngineVersion":"2","AntivirusSignatureVersion":"3","RealTimeProtectionEnabled":"True"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn snapshots_and_disqualifications() {
        let mut c = Canned::good();
        let s = HostSnapshot::take(&mut c);
        assert_eq!(c.calls.len(), 2);
        assert!(c.calls[0].starts_with("System32\\cmd.exe /d /c ver"));
        assert!(c.calls[1].starts_with("System32\\WindowsPowerShell\\v1.0\\powershell.exe "));
        assert!(c.calls[1].contains("Get-MpComputerStatus"));
        assert_eq!(HostSnapshot::from_json(&s.to_json()), Ok(s.clone()));
        let rec = HostRecord {
            kind: HostKind::Laptop,
            start: s.clone(),
            end: s.clone(),
        };
        assert!(rec.disqualifications().is_empty());
        assert_eq!(HostRecord::from_json(&rec.to_json()), Ok(rec.clone()));

        let hosted = HostRecord {
            kind: HostKind::Hosted,
            ..rec.clone()
        };
        assert_eq!(hosted.disqualifications().len(), 1);

        let mut updated = s.clone();
        if let Ok(d) = &mut updated.defender {
            d.signatures = "1.437.83.0".into();
        }
        let changed = HostRecord {
            end: updated,
            ..rec.clone()
        };
        assert!(changed.disqualifications()[0].contains("changed during the run"));

        let mut off = Canned::good();
        off.defender = Ok(DEFENDER_JSON.replace("true", "false").into_bytes());
        off.ver = Err("cmd.exe exited with exit code: 1".into());
        let bad = HostSnapshot::take(&mut off);
        assert!(bad.windows.is_err());
        let r = HostRecord {
            kind: HostKind::Laptop,
            start: bad.clone(),
            end: s,
        };
        let d = r.disqualifications();
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(HostSnapshot::from_json(&bad.to_json()), Ok(bad));
        assert!(
            HostSnapshot::from_json(
                &json!({"windows": {"ok": "1", "error": "x"}, "defender": {"error": "y"}})
            )
            .is_err()
        );
        for k in [HostKind::Laptop, HostKind::Hosted] {
            assert_eq!(HostKind::parse(k.as_str()), Some(k));
        }
    }
}
