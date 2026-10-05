//! The pre-run guard ([MP §8]): its refusals, its command line and its output. `moirai-probes-bin guard` wires
//! [`cli`] to `moirai_os::OsMeter` and [`FsSizer`]; WP-05's nightly runner and every measurement driver run it before
//! they start anything (`docs/m0/PLAN.md` §3.2 WP-05, [60 §3.15]).

use crate::units::{GB, format_gb, parse_bytes};
use moirai_vfs::Meter;
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The default RAM floor: refuse below 1.5 GB of available physical memory ([MP §8.1]). The idle condition's floor
/// is this constant ([`crate::condition::IDLE_FLOOR`], [MP §2.3]); `--ram-floor` moves only the guard's.
pub const RAM_FLOOR: u64 = 1_500_000_000;
/// The default disk floor: refuse below 25 GB of disk headroom ([MP §8.1]).
pub const DISK_FLOOR: u64 = 25 * GB;
/// The schema name of the guard's output ([MP §8.3]).
pub const GUARD_SCHEMA: &str = "moirai-probes/guard/1";

/// Exit code: pass (and `--help`).
pub const EXIT_PASS: u8 = 0;
/// Exit code: refused.
pub const EXIT_REFUSED: u8 = 1;
/// Exit code: usage error; nothing was read.
pub const EXIT_USAGE: u8 = 2;

/// The usage text ([MP §8.2]).
pub const USAGE: &str = "\
usage: guard --volume <dir> [--dir <path> <cap>]... [--ram-floor <bytes>] [--disk-floor <bytes>]
             [--inject-available-physical <bytes>] [--inject-volume-available <bytes>]
             [--inject-dir-size <path> <bytes>]...
       guard --help
Refuses (exit 1) below the RAM floor of available physical memory (default 1.5GB) or below the disk floor of
headroom (default 25GB): the volume's available bytes minus, for each --dir, the growth left to its cap.
Byte quantities: digits with an optional fraction and unit B, KB, MB, GB, TB, KiB, MiB, GiB, TiB (25GB, 1.5GB).
The --inject-* options replace a reading, for tests only. Exit codes: 0 pass, 1 refused, 2 usage error.
See docs/spec/measurement-protocol.md section 8.
";

/// A directory whose growth up to its cap is reserved before the headroom is counted ([MP §8.1]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CountedDir {
    /// The directory.
    pub path: PathBuf,
    /// Its cap in bytes.
    pub cap: u64,
    /// An injected size, replacing the walk.
    pub injected_size: Option<u64>,
}

/// The parsed command line ([MP §8.2]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardArgs {
    /// Any existing directory on the work volume.
    pub volume: PathBuf,
    /// The counted directories, in command-line order.
    pub dirs: Vec<CountedDir>,
    /// The RAM floor.
    pub ram_floor: u64,
    /// The disk floor.
    pub disk_floor: u64,
    /// An injected available-physical reading.
    pub inject_available_physical: Option<u64>,
    /// An injected volume-available reading.
    pub inject_volume_available: Option<u64>,
}

/// What the command line asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Invocation {
    /// Run the checks.
    Check(GuardArgs),
    /// Print the usage.
    Help,
}

fn utf8<'a>(v: &'a OsStr, what: &str) -> Result<&'a str, String> {
    v.to_str()
        .ok_or_else(|| format!("{what}: '{}' is not valid Unicode", v.to_string_lossy()))
}

fn once<T>(slot: &mut Option<T>, flag: &str, v: T) -> Result<(), String> {
    if slot.replace(v).is_some() {
        return Err(format!("{flag} is given twice"));
    }
    Ok(())
}

/// Parses the guard's arguments, without the program name ([MP §8.2]).
pub fn parse_args(args: &[OsString]) -> Result<Invocation, String> {
    let mut volume: Option<PathBuf> = None;
    let mut dirs: Vec<CountedDir> = Vec::new();
    let mut ram_floor = None;
    let mut disk_floor = None;
    let mut inject_ram = None;
    let mut inject_volume = None;
    let mut inject_dirs: Vec<(OsString, u64)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let flag = utf8(a, "argument")?;
        let mut value = |n: &str| {
            it.next()
                .ok_or_else(|| format!("{flag} needs {n}"))
                .map(OsString::as_os_str)
        };
        let path_of = |v: &OsStr| {
            if v.is_empty() {
                Err(format!("{flag} needs a non-empty path"))
            } else {
                Ok(v.to_os_string())
            }
        };
        match flag {
            "--help" => return Ok(Invocation::Help),
            "--volume" => once(
                &mut volume,
                flag,
                PathBuf::from(path_of(value("a directory")?)?),
            )?,
            "--dir" => {
                let path = path_of(value("a path and a cap")?)?;
                let cap = parse_bytes(utf8(value("a cap after the path")?, flag)?)?;
                if dirs.iter().any(|d| d.path.as_os_str() == path) {
                    return Err(format!("--dir '{}' is given twice", path.to_string_lossy()));
                }
                dirs.push(CountedDir {
                    path: PathBuf::from(path),
                    cap,
                    injected_size: None,
                });
            }
            "--ram-floor" => once(
                &mut ram_floor,
                flag,
                parse_bytes(utf8(value("a byte quantity")?, flag)?)?,
            )?,
            "--disk-floor" => once(
                &mut disk_floor,
                flag,
                parse_bytes(utf8(value("a byte quantity")?, flag)?)?,
            )?,
            "--inject-available-physical" => once(
                &mut inject_ram,
                flag,
                parse_bytes(utf8(value("a byte quantity")?, flag)?)?,
            )?,
            "--inject-volume-available" => once(
                &mut inject_volume,
                flag,
                parse_bytes(utf8(value("a byte quantity")?, flag)?)?,
            )?,
            "--inject-dir-size" => {
                let path = path_of(value("a path and a size")?)?;
                let size = parse_bytes(utf8(value("a size after the path")?, flag)?)?;
                inject_dirs.push((path, size));
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    for (path, size) in inject_dirs {
        let d = dirs
            .iter_mut()
            .find(|d| d.path.as_os_str() == path)
            .ok_or_else(|| {
                format!(
                    "--inject-dir-size names '{}', which no --dir gives",
                    path.to_string_lossy()
                )
            })?;
        if d.injected_size.replace(size).is_some() {
            return Err(format!(
                "--inject-dir-size '{}' is given twice",
                path.to_string_lossy()
            ));
        }
    }
    Ok(Invocation::Check(GuardArgs {
        volume: volume.ok_or("--volume is required")?,
        dirs,
        ram_floor: ram_floor.unwrap_or(RAM_FLOOR),
        disk_floor: disk_floor.unwrap_or(DISK_FLOOR),
        inject_available_physical: inject_ram,
        inject_volume_available: inject_volume,
    }))
}

/// The size of a counted directory ([MP §8.1]).
pub trait DirSizer {
    /// The sum of the logical lengths of the regular files below `dir`; 0 when `dir` does not exist.
    fn size(&self, dir: &Path) -> Result<u64, String>;
}

/// [`DirSizer`] over `std::fs` ([MP §8.1]): the directory itself is resolved; below it, symbolic links and junctions
/// are not followed (`FileType::is_symlink` covers every name-surrogate reparse point on Windows, junctions
/// included); an entry that vanishes during the walk counts 0; any other error fails the size.
#[derive(Clone, Copy, Debug, Default)]
pub struct FsSizer;

/// One step of the walk ([MP §8.1]): its value, `None` for an entry that vanished, or the error text of any other
/// failure, which makes the directory unreadable.
fn tolerate<T>(r: std::io::Result<T>, p: &Path) -> Result<Option<T>, String> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

impl DirSizer for FsSizer {
    fn size(&self, dir: &Path) -> Result<u64, String> {
        let Some(meta) = tolerate(std::fs::metadata(dir), dir)? else {
            return Ok(0);
        };
        if !meta.is_dir() {
            return Ok(if meta.is_file() { meta.len() } else { 0 });
        }
        let mut total: u64 = 0;
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Some(entries) = tolerate(std::fs::read_dir(&d), &d)? else {
                continue;
            };
            for entry in entries {
                let Some(entry) = tolerate(entry, &d)? else {
                    continue;
                };
                let path = entry.path();
                let Some(ft) = tolerate(entry.file_type(), &path)? else {
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    stack.push(path);
                } else if ft.is_file()
                    && let Some(m) = tolerate(entry.metadata(), &path)?
                {
                    total = total.saturating_add(m.len());
                }
            }
        }
        Ok(total)
    }
}

/// A refusal kind ([MP §8.1]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RefusalKind {
    /// Available physical memory below the RAM floor.
    RamLow,
    /// Available physical memory could not be read.
    RamUnreadable,
    /// The disk headroom below the disk floor.
    DiskLow,
    /// The volume's free space could not be read.
    DiskUnreadable,
    /// A counted directory's size could not be read.
    DirUnreadable,
}

impl RefusalKind {
    /// The output spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            RefusalKind::RamLow => "ram-low",
            RefusalKind::RamUnreadable => "ram-unreadable",
            RefusalKind::DiskLow => "disk-low",
            RefusalKind::DiskUnreadable => "disk-unreadable",
            RefusalKind::DirUnreadable => "dir-unreadable",
        }
    }
}

/// One refusal and its text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The kind.
    pub kind: RefusalKind,
    /// What was found.
    pub detail: String,
}

/// One counted directory in the report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirReport {
    /// The directory.
    pub path: PathBuf,
    /// Its cap.
    pub cap: u64,
    /// Its size, if read.
    pub size: Option<u64>,
    /// max(0, cap − size), if the size was read.
    pub reserve: Option<u64>,
    /// Whether its size exceeds its cap.
    pub over_cap: bool,
    /// Whether the size was injected.
    pub injected: bool,
    /// Why the size could not be read.
    pub error: Option<String>,
}

/// Everything the guard read and decided ([MP §8.3]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardReport {
    /// Available physical memory, if read.
    pub ram_available: Option<u64>,
    /// The RAM floor used.
    pub ram_floor: u64,
    /// Whether the RAM reading was injected.
    pub ram_injected: bool,
    /// Why it could not be read.
    pub ram_error: Option<String>,
    /// The work volume's directory.
    pub volume: PathBuf,
    /// The volume's available bytes, if read.
    pub volume_available: Option<u64>,
    /// Σ reserve, if every directory was read.
    pub reserved: Option<u64>,
    /// available − reserved, floored at 0, if both are known.
    pub headroom: Option<u64>,
    /// The disk floor used.
    pub disk_floor: u64,
    /// Whether the volume reading was injected.
    pub disk_injected: bool,
    /// Why the volume could not be read.
    pub disk_error: Option<String>,
    /// The counted directories.
    pub dirs: Vec<DirReport>,
    /// The refusals; empty when the guard passes.
    pub refusals: Vec<Refusal>,
}

/// Runs the checks of [MP §8.1].
pub fn evaluate<M: Meter + ?Sized, S: DirSizer + ?Sized>(
    args: &GuardArgs,
    meter: &M,
    sizer: &S,
) -> GuardReport {
    let mut refusals = Vec::new();

    let (ram_available, ram_error) = match args.inject_available_physical {
        Some(v) => (Some(v), None),
        None => match meter.available_physical() {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        },
    };
    match (ram_available, &ram_error) {
        (Some(v), _) if v < args.ram_floor => refusals.push(Refusal {
            kind: RefusalKind::RamLow,
            detail: format!(
                "available physical memory {} is below the floor {}",
                format_gb(v),
                format_gb(args.ram_floor)
            ),
        }),
        (None, Some(e)) => refusals.push(Refusal {
            kind: RefusalKind::RamUnreadable,
            detail: format!("available physical memory could not be read: {e}"),
        }),
        _ => {}
    }

    let (volume_available, disk_error) = match args.inject_volume_available {
        Some(v) => (Some(v), None),
        None => match meter.free_space(&args.volume) {
            Ok(f) => (Some(f.available), None),
            Err(e) => (None, Some(e.to_string())),
        },
    };
    if let Some(e) = &disk_error {
        refusals.push(Refusal {
            kind: RefusalKind::DiskUnreadable,
            detail: format!(
                "the free space of the volume of '{}' could not be read: {e}",
                args.volume.display()
            ),
        });
    }

    let dirs: Vec<DirReport> = args
        .dirs
        .iter()
        .map(|d| {
            let (size, error) = match d.injected_size {
                Some(s) => (Some(s), None),
                None => match sizer.size(&d.path) {
                    Ok(s) => (Some(s), None),
                    Err(e) => (None, Some(e)),
                },
            };
            DirReport {
                path: d.path.clone(),
                cap: d.cap,
                size,
                reserve: size.map(|s| d.cap.saturating_sub(s)),
                over_cap: size.is_some_and(|s| s > d.cap),
                injected: d.injected_size.is_some(),
                error,
            }
        })
        .collect();
    for d in dirs.iter().filter(|d| d.error.is_some()) {
        refusals.push(Refusal {
            kind: RefusalKind::DirUnreadable,
            detail: format!(
                "the size of '{}' could not be read: {}",
                d.path.display(),
                d.error.as_deref().unwrap_or("")
            ),
        });
    }
    let reserved = dirs
        .iter()
        .try_fold(0u64, |acc, d| d.reserve.map(|r| acc.saturating_add(r)));
    let headroom = volume_available
        .zip(reserved)
        .map(|(a, r)| a.saturating_sub(r));
    if let Some(h) = headroom
        && h < args.disk_floor
    {
        refusals.push(Refusal {
            kind: RefusalKind::DiskLow,
            detail: format!(
                "disk headroom {} (available {} minus {} reserved for the counted directories) is below the floor {}",
                format_gb(h),
                format_gb(volume_available.unwrap_or(0)),
                format_gb(reserved.unwrap_or(0)),
                format_gb(args.disk_floor)
            ),
        });
    }

    GuardReport {
        ram_available,
        ram_floor: args.ram_floor,
        ram_injected: args.inject_available_physical.is_some(),
        ram_error,
        volume: args.volume.clone(),
        volume_available,
        reserved,
        headroom,
        disk_floor: args.disk_floor,
        disk_injected: args.inject_volume_available.is_some(),
        disk_error,
        dirs,
        refusals,
    }
}

impl GuardReport {
    /// Whether the guard passes.
    pub fn passed(&self) -> bool {
        self.refusals.is_empty()
    }

    /// The output object of [MP §8.3].
    pub fn to_json(&self) -> Value {
        json!({
            "schema": GUARD_SCHEMA,
            "verdict": if self.passed() { "pass" } else { "refuse" },
            "ram": {
                "available": self.ram_available,
                "floor": self.ram_floor,
                "injected": self.ram_injected,
                "error": self.ram_error,
            },
            "disk": {
                "volume": self.volume.to_string_lossy(),
                "available": self.volume_available,
                "reserved": self.reserved,
                "headroom": self.headroom,
                "floor": self.disk_floor,
                "injected": self.disk_injected,
                "error": self.disk_error,
            },
            "dirs": self.dirs.iter().map(|d| json!({
                "path": d.path.to_string_lossy(),
                "cap": d.cap,
                "size": d.size,
                "reserve": d.reserve,
                "over_cap": d.over_cap,
                "injected": d.injected,
                "error": d.error,
            })).collect::<Vec<_>>(),
            "refusals": self.refusals.iter().map(|r| json!({
                "kind": r.kind.as_str(),
                "detail": r.detail,
            })).collect::<Vec<_>>(),
        })
    }
}

/// The guard's command line ([MP §8.2], [MP §8.3]): parses `args` (without the program name), runs the checks, writes
/// the JSON line to `out` and one line per refusal to `err`, and returns the exit code.
pub fn cli<M: Meter + ?Sized, S: DirSizer + ?Sized>(
    args: &[OsString],
    meter: &M,
    sizer: &S,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let parsed = match parse_args(args) {
        Ok(Invocation::Help) => {
            let _ = out.write_all(USAGE.as_bytes());
            return EXIT_PASS;
        }
        Ok(Invocation::Check(a)) => a,
        Err(e) => {
            let _ = writeln!(err, "guard: {e}");
            let _ = err.write_all(USAGE.as_bytes());
            return EXIT_USAGE;
        }
    };
    let report = evaluate(&parsed, meter, sizer);
    let _ = writeln!(out, "{}", report.to_json());
    for r in &report.refusals {
        let _ = writeln!(err, "guard: refused: {}: {}", r.kind.as_str(), r.detail);
    }
    if report.passed() {
        EXIT_PASS
    } else {
        EXIT_REFUSED
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{FakeMeter, meter_err, scratch_dir};
    use moirai_vfs::{FreeSpace, OsCode, VfsError, VfsErrorKind};
    use proptest::prelude::*;
    use std::collections::HashMap;

    struct FakeSizer(HashMap<PathBuf, Result<u64, String>>);

    impl DirSizer for FakeSizer {
        fn size(&self, dir: &Path) -> Result<u64, String> {
            self.0.get(dir).cloned().unwrap_or(Ok(0))
        }
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn meter(ram: u64, disk: u64) -> FakeMeter {
        let mut m = FakeMeter::new(ram);
        m.free = Ok(FreeSpace {
            available: disk,
            total: 512 * GB,
        });
        m
    }

    fn check(args: &[&str]) -> GuardArgs {
        match parse_args(&os(args)).unwrap() {
            Invocation::Check(a) => a,
            Invocation::Help => panic!("help"),
        }
    }

    /// The four counted directories of WP-05 (`docs/m0/tools.md` §12) with illustrative caps.
    fn four() -> Vec<&'static str> {
        vec![
            "--volume",
            "D:/moirai-target",
            "--dir",
            "D:/moirai-target/laneA",
            "30GB",
            "--dir",
            "D:/moirai-target/laneB",
            "30GB",
            "--dir",
            "D:/moirai-target/fuzz",
            "10GB",
            "--dir",
            "D:/moirai-target/mutants",
            "10GB",
        ]
    }

    const NAMES: [&str; 4] = [
        "D:/moirai-target/laneA",
        "D:/moirai-target/laneB",
        "D:/moirai-target/fuzz",
        "D:/moirai-target/mutants",
    ];

    #[test]
    fn parses_the_command_line() {
        let a = check(&four());
        assert_eq!(a.volume, PathBuf::from("D:/moirai-target"));
        assert_eq!(a.dirs.len(), 4);
        assert_eq!(a.dirs[0].cap, 30 * GB);
        assert_eq!((a.ram_floor, a.disk_floor), (RAM_FLOOR, DISK_FLOOR));
        let mut v = four();
        v.extend([
            "--ram-floor",
            "2GB",
            "--disk-floor",
            "40GiB",
            "--inject-available-physical",
            "1.4GB",
        ]);
        v.extend([
            "--inject-volume-available",
            "60GB",
            "--inject-dir-size",
            "D:/moirai-target/fuzz",
            "2GB",
        ]);
        let a = check(&v);
        assert_eq!((a.ram_floor, a.disk_floor), (2 * GB, 40 << 30));
        assert_eq!(
            (a.inject_available_physical, a.inject_volume_available),
            (Some(1_400_000_000), Some(60 * GB))
        );
        assert_eq!(a.dirs[2].injected_size, Some(2 * GB));
        assert_eq!(
            parse_args(&os(&["--dir", "x", "1", "--help"])),
            Ok(Invocation::Help)
        );
        for bad in [
            &[][..],
            &["--volume"],
            &["--volume", "a", "--volume", "b"],
            &["--volume", "a", "--dir", "x"],
            &["--volume", "a", "--dir", "x", "ten"],
            &["--volume", "a", "--dir", "x", "1", "--dir", "x", "2"],
            &["--volume", "a", "--inject-dir-size", "x", "1"],
            &[
                "--volume",
                "a",
                "--dir",
                "x",
                "1",
                "--inject-dir-size",
                "x",
                "1",
                "--inject-dir-size",
                "x",
                "2",
            ],
            &["--volume", "a", "--ram-floor", "1GB", "--ram-floor", "2GB"],
            &["--volume", "a", "--frobnicate"],
            &["--volume", ""],
            &["--volume", "a", "--dir", "", "1GB"],
            &[
                "--volume",
                "a",
                "--dir",
                "x",
                "1",
                "--inject-dir-size",
                "",
                "1",
            ],
            &["-h"],
        ] {
            assert!(parse_args(&os(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn passes_with_room() {
        let m = meter(8 * GB, 100 * GB);
        let mut sizes = HashMap::new();
        sizes.insert(PathBuf::from(NAMES[0]), Ok(12 * GB));
        sizes.insert(PathBuf::from(NAMES[1]), Ok(35 * GB));
        let r = evaluate(&check(&four()), &m, &FakeSizer(sizes));
        // Reserved: 18 + 0 (over its cap) + 10 + 10 = 38 GB; headroom 62 GB.
        assert_eq!(r.reserved, Some(38 * GB));
        assert_eq!(r.headroom, Some(62 * GB));
        assert!(r.passed(), "{:?}", r.refusals);
        assert!(r.dirs[1].over_cap && r.dirs[1].reserve == Some(0));
        let v = r.to_json();
        assert_eq!(v["verdict"], "pass");
        assert_eq!(v["dirs"][0]["reserve"], json!(18 * GB));
        assert_eq!(v["ram"]["floor"], json!(RAM_FLOOR));
    }

    #[test]
    fn refuses_low_ram_and_low_disk() {
        let sizer = FakeSizer(HashMap::new());
        let r = evaluate(&check(&four()), &meter(RAM_FLOOR - 1, 200 * GB), &sizer);
        assert_eq!(r.refusals.len(), 1);
        assert_eq!(r.refusals[0].kind, RefusalKind::RamLow);
        assert!(r.refusals[0].detail.contains("1.500 GB"));
        assert!(evaluate(&check(&four()), &meter(RAM_FLOOR, 200 * GB), &sizer).passed());
        // Empty directories reserve their whole caps: 80 GB of 104 GB leaves 24 GB.
        let r = evaluate(&check(&four()), &meter(8 * GB, 104 * GB), &sizer);
        assert_eq!(r.headroom, Some(24 * GB));
        assert_eq!(
            r.refusals.iter().map(|x| x.kind).collect::<Vec<_>>(),
            [RefusalKind::DiskLow]
        );
        assert!(evaluate(&check(&four()), &meter(8 * GB, 105 * GB), &sizer).passed());
        let r = evaluate(&check(&four()), &meter(8 * GB, 10 * GB), &sizer);
        assert_eq!(r.headroom, Some(0));
    }

    #[test]
    fn fails_closed_on_unreadable_readings() {
        let mut m = FakeMeter::scripted(vec![Err(meter_err("GlobalMemoryStatusEx"))]);
        m.free = Err(VfsError::new(
            VfsErrorKind::NotFound,
            OsCode(3),
            "GetDiskFreeSpaceExW",
        ));
        let mut sizes = HashMap::new();
        sizes.insert(PathBuf::from(NAMES[2]), Err("access denied".to_string()));
        let r = evaluate(&check(&four()), &m, &FakeSizer(sizes));
        let kinds: Vec<_> = r.refusals.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                RefusalKind::RamUnreadable,
                RefusalKind::DiskUnreadable,
                RefusalKind::DirUnreadable
            ]
        );
        assert_eq!((r.reserved, r.headroom), (None, None));
        assert_eq!(r.to_json()["verdict"], "refuse");
    }

    #[test]
    fn injected_values_replace_readings() {
        let m = FakeMeter::scripted(vec![Err(meter_err("never read"))]);
        let mut v = four();
        v.extend([
            "--inject-available-physical",
            "1GB",
            "--inject-volume-available",
            "200GB",
        ]);
        for d in NAMES {
            v.extend(["--inject-dir-size", d, "0"]);
        }
        let sizer = FakeSizer(
            NAMES
                .iter()
                .map(|d| (PathBuf::from(d), Err("never walked".to_string())))
                .collect(),
        );
        let r = evaluate(&check(&v), &m, &sizer);
        assert_eq!(m.avail_calls.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(
            r.refusals.iter().map(|x| x.kind).collect::<Vec<_>>(),
            [RefusalKind::RamLow]
        );
        assert!(r.ram_injected && r.disk_injected && r.dirs.iter().all(|d| d.injected));
        assert_eq!(r.headroom, Some(120 * GB));
        let j = r.to_json();
        assert_eq!(j["ram"]["injected"], true);
        assert_eq!(j["dirs"][3]["injected"], true);
    }

    #[test]
    fn cli_exit_codes_and_output() {
        let sizer = FakeSizer(HashMap::new());
        let run = |args: &[&str], m: &FakeMeter| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = cli(&os(args), m, &sizer, &mut out, &mut err);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let (code, out, err) = run(&four(), &meter(8 * GB, 200 * GB));
        assert_eq!((code, err.as_str()), (EXIT_PASS, ""));
        assert_eq!(out.lines().count(), 1);
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            (v["schema"].as_str(), v["verdict"].as_str()),
            (Some(GUARD_SCHEMA), Some("pass"))
        );
        let (code, out, err) = run(&four(), &meter(GB, 10 * GB));
        assert_eq!(code, EXIT_REFUSED);
        assert!(out.contains("\"refuse\""));
        assert_eq!(err.lines().count(), 2);
        assert!(err.starts_with("guard: refused: ram-low: "));
        let (code, out, err) = run(&["--volume"], &meter(8 * GB, 200 * GB));
        assert_eq!((code, out.as_str()), (EXIT_USAGE, ""));
        assert!(err.starts_with("guard: --volume needs a directory\nusage: guard"));
        let (code, out, _) = run(&["--help"], &meter(8 * GB, 200 * GB));
        assert_eq!(code, EXIT_PASS);
        assert_eq!(out, USAGE);
    }

    #[test]
    fn fs_sizer_walks_regular_files() {
        let root = scratch_dir("sizer");
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("top.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(root.join("a/one"), vec![0u8; 24]).unwrap();
        std::fs::write(root.join("a/b/c/deep"), vec![0u8; 4096]).unwrap();
        std::fs::write(root.join("a/b/empty"), b"").unwrap();
        assert_eq!(FsSizer.size(&root), Ok(1000 + 24 + 4096));
        assert_eq!(FsSizer.size(&root.join("a/b")), Ok(4096));
        assert_eq!(FsSizer.size(&root.join("top.bin")), Ok(1000));
        assert_eq!(FsSizer.size(&root.join("missing")), Ok(0));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Makes `link` a directory junction (Windows, `mklink /J`, no privilege needed) or a symbolic link (elsewhere)
    /// to `target`; `false` when neither tool is available.
    fn make_dir_link(target: &Path, link: &Path) -> bool {
        use std::process::{Command, Stdio};
        let quiet = |c: &mut Command| {
            c.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        quiet(
            Command::new("cmd")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(link)
                .arg(target),
        ) || quiet(Command::new("ln").arg("-s").arg(target).arg(link))
    }

    #[test]
    fn fs_sizer_does_not_follow_links_below_the_directory() {
        let root = scratch_dir("sizer-links");
        let outside = scratch_dir("sizer-outside");
        std::fs::write(outside.join("big"), vec![0u8; 50_000]).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/own"), vec![0u8; 10]).unwrap();
        let link = root.join("sub").join("link");
        if make_dir_link(&outside, &link) {
            assert!(std::fs::symlink_metadata(&link).is_ok());
            assert_eq!(
                FsSizer.size(&root),
                Ok(10),
                "a junction or link below the counted directory is not followed"
            );
            // The counted directory itself is resolved, even when it is a link.
            assert_eq!(FsSizer.size(&link), Ok(50_000));
            let _ = std::fs::remove_dir(&link).or_else(|_| std::fs::remove_file(&link));
        }
        assert_eq!(FsSizer.size(&outside), Ok(50_000));
        std::fs::remove_dir_all(&root).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn fs_sizer_tolerates_only_vanished_entries() {
        let p = Path::new("lane/target");
        assert_eq!(tolerate(Ok(5u8), p), Ok(Some(5)));
        let gone = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            tolerate::<u8>(Err(gone), p),
            Ok(None),
            "a vanished entry counts 0"
        );
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let e = tolerate::<u8>(Err(denied), p).unwrap_err();
        assert!(e.starts_with(&format!("{}: ", p.display())), "{e}");
        // A path the OS cannot even name fails the walk, and the guard refuses it as dir-unreadable.
        let bad = PathBuf::from("lane\0target");
        assert!(FsSizer.size(&bad).is_err());
        let args = GuardArgs {
            volume: PathBuf::from("v"),
            dirs: vec![CountedDir {
                path: bad,
                cap: GB,
                injected_size: None,
            }],
            ram_floor: 0,
            disk_floor: 0,
            inject_available_physical: Some(GB),
            inject_volume_available: Some(100 * GB),
        };
        let r = evaluate(&args, &FakeMeter::new(0), &FsSizer);
        assert_eq!(
            r.refusals.iter().map(|x| x.kind).collect::<Vec<_>>(),
            [RefusalKind::DirUnreadable]
        );
        assert_eq!((r.reserved, r.headroom), (None, None));
    }

    proptest! {
        #[test]
        fn headroom_formula(
            avail in any::<u64>(),
            dirs in proptest::collection::vec((0u64..=100 * GB, 0u64..=100 * GB), 0..5),
            floor in 0u64..=200 * GB,
        ) {
            let args = GuardArgs {
                volume: PathBuf::from("v"),
                dirs: dirs.iter().enumerate().map(|(i, &(cap, size))| CountedDir {
                    path: PathBuf::from(format!("d{i}")), cap, injected_size: Some(size),
                }).collect(),
                ram_floor: 0,
                disk_floor: floor,
                inject_available_physical: Some(1),
                inject_volume_available: Some(avail),
            };
            let r = evaluate(&args, &FakeMeter::new(0), &FakeSizer(HashMap::new()));
            let reserved: u64 = dirs.iter().map(|&(c, s)| c.saturating_sub(s)).sum();
            let headroom = avail.saturating_sub(reserved);
            prop_assert_eq!(r.reserved, Some(reserved));
            prop_assert_eq!(r.headroom, Some(headroom));
            prop_assert_eq!(r.passed(), headroom >= floor);
        }
    }
}
