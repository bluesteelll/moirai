//! The reviewed data files of `xtask/` (docs/m0/PLAN.md §2.1, §2.4, §2.5): `host-only.toml`, `roots.toml`,
//! `native-allow.toml`, `osdeps-allow.toml`, `licence-allow.toml`, `crates.toml` and `roles.toml`.
//!
//! Each loader checks `version = 1` and every required field, and refuses unknown `graphs`, `scans`, `kind` and
//! lockfile values, so a typo in a reviewed file fails the gate instead of silently widening it.

use crate::semver::VersionReq;
use crate::toml::{self, Table, Value};
use std::path::Path;

pub fn read_toml(path: &Path) -> Result<Table, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn check_version(t: &Table, file: &str) -> Result<(), String> {
    match t.get("version").and_then(Value::as_integer) {
        Some(1) => Ok(()),
        Some(v) => Err(format!("{file}: unsupported version {v} (expected 1)")),
        None => Err(format!("{file}: missing 'version = 1'")),
    }
}

fn entries<'t>(t: &'t Table, key: &str, file: &str) -> Result<&'t [Value], String> {
    match t.get(key) {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(format!("{file}: '{key}' must be an array of tables")),
    }
}

fn req_str(e: &Value, key: &str, what: &str) -> Result<String, String> {
    e.get_path(&[key])
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{what}: missing string '{key}'"))
}

fn opt_str(e: &Value, key: &str) -> Option<String> {
    e.get_path(&[key])
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn str_list(e: &Value, key: &str, what: &str) -> Result<Vec<String>, String> {
    match e.get_path(&[key]) {
        None => Err(format!("{what}: missing list '{key}'")),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{what}: '{key}' must hold strings"))
            })
            .collect(),
        Some(_) => Err(format!("{what}: '{key}' must be a list of strings")),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// host-only.toml

#[derive(Clone, Debug, Default)]
pub struct HostOnly {
    pub crates: Vec<String>,
}

impl HostOnly {
    pub fn from_table(t: &Table) -> Result<HostOnly, String> {
        const F: &str = "xtask/host-only.toml";
        check_version(t, F)?;
        let mut crates = Vec::new();
        for (i, e) in entries(t, "crate", F)?.iter().enumerate() {
            let what = format!("{F}: crate entry {}", i + 1);
            let name = req_str(e, "name", &what)?;
            req_str(e, "reason", &what)?;
            crates.push(name);
        }
        Ok(HostOnly { crates })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.crates.iter().any(|c| c == name)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// roots.toml

#[derive(Clone, Debug)]
pub struct Root {
    pub name: String,
    pub present: bool,
    /// A product root ([OS/README §2.4]): it may not enable `moirai-os`'s `test-host` feature or install a
    /// `#[global_allocator]` (`CountingAlloc` is for probe roots only, [OS/mem §6]).
    pub product: bool,
    pub bin_paths: Vec<String>,
    pub max_lines: usize,
    pub allowed_deps: Vec<String>,
    pub windows_check: String,
}

#[derive(Clone, Debug, Default)]
pub struct Roots {
    pub roots: Vec<Root>,
}

impl Roots {
    pub fn from_table(t: &Table) -> Result<Roots, String> {
        const F: &str = "xtask/roots.toml";
        check_version(t, F)?;
        let mut roots = Vec::new();
        for (i, e) in entries(t, "root", F)?.iter().enumerate() {
            let what = format!("{F}: root entry {}", i + 1);
            let max_lines = e
                .get_path(&["max_lines"])
                .and_then(Value::as_integer)
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("{what}: missing positive integer 'max_lines'"))?;
            roots.push(Root {
                name: req_str(e, "name", &what)?,
                present: e
                    .get_path(&["present"])
                    .and_then(Value::as_bool)
                    .ok_or_else(|| format!("{what}: missing boolean 'present'"))?,
                product: e
                    .get_path(&["product"])
                    .and_then(Value::as_bool)
                    .ok_or_else(|| format!("{what}: missing boolean 'product'"))?,
                bin_paths: str_list(e, "bin_paths", &what)?,
                max_lines: usize::try_from(max_lines)
                    .map_err(|_| format!("{what}: 'max_lines' too large"))?,
                allowed_deps: str_list(e, "allowed_deps", &what)?,
                windows_check: req_str(e, "windows_check", &what)?,
            });
        }
        Ok(Roots { roots })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.roots.iter().any(|r| r.name == name)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// native-allow.toml

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Graph {
    Checked,
    HostOnly,
    Fuzz,
}

impl Graph {
    pub fn parse(s: &str) -> Option<Graph> {
        match s {
            "checked" => Some(Graph::Checked),
            "host-only" => Some(Graph::HostOnly),
            "fuzz" => Some(Graph::Fuzz),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Graph::Checked => "checked",
            Graph::HostOnly => "host-only",
            Graph::Fuzz => "fuzz",
        }
    }
}

#[derive(Clone, Debug)]
pub struct NativeEntry {
    pub name: String,
    pub versions_raw: String,
    pub versions: VersionReq,
    pub graphs: Vec<Graph>,
    pub features: Vec<String>,
    pub links: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct NativeAllow {
    pub entries: Vec<NativeEntry>,
}

impl NativeAllow {
    pub fn from_table(t: &Table) -> Result<NativeAllow, String> {
        const F: &str = "xtask/native-allow.toml";
        check_version(t, F)?;
        let mut out = Vec::new();
        for (i, e) in entries(t, "package", F)?.iter().enumerate() {
            let what = format!("{F}: package entry {}", i + 1);
            let name = req_str(e, "name", &what)?;
            let what = format!("{what} ('{name}')");
            let versions_raw = req_str(e, "versions", &what)?;
            let versions = VersionReq::parse(&versions_raw).map_err(|m| format!("{what}: {m}"))?;
            let graphs = str_list(e, "graphs", &what)?
                .iter()
                .map(|g| Graph::parse(g).ok_or_else(|| format!("{what}: unknown graph '{g}'")))
                .collect::<Result<Vec<_>, _>>()?;
            if graphs.is_empty() {
                return Err(format!("{what}: 'graphs' is empty"));
            }
            let features = str_list(e, "features", &what)?;
            req_str(e, "script", &what)?;
            req_str(e, "reason", &what)?;
            // One entry per (name, version, graph): a second one would never be consulted by [`NativeAllow::find`],
            // so its reviewed script and reason would be dead text.
            if let Some((j, prev)) =
                out.iter()
                    .enumerate()
                    .find(|(_, p): &(usize, &NativeEntry)| {
                        p.name == name
                            && p.graphs.iter().any(|g| graphs.contains(g))
                            && p.versions.intersects(&versions)
                    })
            {
                return Err(format!(
                    "{what}: overlaps package entry {} ('{}' {}, graphs {}): both cover a version of the same graph; give each graph and version one entry",
                    j + 1,
                    prev.name,
                    prev.versions_raw,
                    prev.graphs
                        .iter()
                        .map(|g| g.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            out.push(NativeEntry {
                name,
                versions_raw,
                versions,
                graphs,
                features,
                links: opt_str(e, "links"),
            });
        }
        Ok(NativeAllow { entries: out })
    }

    /// The entry covering `name` at `version` in `graph`, if any; the loader refuses two entries that cover one
    /// version of one graph, so there is at most one. A package whose resolved features differ between graphs (libc
    /// in the checked and the fuzz graphs) has one entry per graph; an entry that covers the version but not the
    /// graph is returned when no other does, so the lint reports the graph it lacks.
    pub fn find(&self, name: &str, version: &str, graph: Graph) -> Option<&NativeEntry> {
        let v = crate::semver::Version::parse(version)?;
        let mut covering = self
            .entries
            .iter()
            .filter(|e| e.name == name && e.versions.matches(&v));
        let first = covering.clone().next();
        covering.find(|e| e.graphs.contains(&graph)).or(first)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// osdeps-allow.toml

pub const SCANS: &[&str] = &["cfg-os", "std-os", "direct-osdep"];

/// The OS-binding crates of the GT20 (d) dependency rules: [80 §5.5] names `windows-sys` and `libc`; the rule's intent,
/// keeping the OS surface inside `moirai-os`, covers every crate that binds an OS API directly: the other Windows
/// binding families (`windows`, `windows-core`, `windows-targets`, `windows-link`, `winapi`) and the Unix wrappers
/// (`nix`, `rustix`). Package names are compared with `_` read as `-`.
pub const OSDEPS: &[&str] = &[
    "windows-sys",
    "windows",
    "windows-core",
    "windows-targets",
    "windows-link",
    "winapi",
    "libc",
    "nix",
    "rustix",
];

/// Whether a (normalised) package name is one of [`OSDEPS`].
pub fn is_osdep(name: &str) -> bool {
    OSDEPS.contains(&name)
}

#[derive(Clone, Debug)]
pub struct OsdepsCrate {
    pub name: String,
    pub scans: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct OsdepsTransitive {
    pub krate: String,
    pub versions: VersionReq,
    pub osdep: String,
}

#[derive(Clone, Debug, Default)]
pub struct OsdepsAllow {
    pub crates: Vec<OsdepsCrate>,
    pub transitive: Vec<OsdepsTransitive>,
}

impl OsdepsAllow {
    pub fn from_table(t: &Table) -> Result<OsdepsAllow, String> {
        const F: &str = "xtask/osdeps-allow.toml";
        check_version(t, F)?;
        let mut crates = Vec::new();
        for (i, e) in entries(t, "crate", F)?.iter().enumerate() {
            let what = format!("{F}: crate entry {}", i + 1);
            let name = req_str(e, "name", &what)?;
            let scans = str_list(e, "scans", &what)?;
            for s in &scans {
                if !SCANS.contains(&s.as_str()) {
                    return Err(format!(
                        "{what}: unknown scan '{s}' (known: {})",
                        SCANS.join(", ")
                    ));
                }
            }
            req_str(e, "reason", &what)?;
            crates.push(OsdepsCrate { name, scans });
        }
        let mut transitive = Vec::new();
        for (i, e) in entries(t, "transitive", F)?.iter().enumerate() {
            let what = format!("{F}: transitive entry {}", i + 1);
            let krate = req_str(e, "crate", &what)?;
            let versions = VersionReq::parse(&req_str(e, "versions", &what)?)
                .map_err(|m| format!("{what}: {m}"))?;
            let osdep = req_str(e, "osdep", &what)?;
            if !is_osdep(&osdep) {
                return Err(format!(
                    "{what}: 'osdep' must be one of {}",
                    OSDEPS.join(", ")
                ));
            }
            req_str(e, "reason", &what)?;
            transitive.push(OsdepsTransitive {
                krate,
                versions,
                osdep,
            });
        }
        Ok(OsdepsAllow { crates, transitive })
    }

    pub fn exempt(&self, krate: &str, scan: &str) -> bool {
        self.crates
            .iter()
            .any(|c| c.name == krate && c.scans.iter().any(|s| s == scan))
    }

    pub fn transitive_ok(&self, krate: &str, version: &str, osdep: &str) -> bool {
        let Some(v) = crate::semver::Version::parse(version) else {
            return false;
        };
        self.transitive
            .iter()
            .any(|t| t.krate == krate && t.osdep == osdep && t.versions.matches(&v))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// licence-allow.toml

pub const LOCKFILES: &[&str] = &["Cargo.lock", "fuzz/Cargo.lock"];

#[derive(Clone, Debug)]
pub struct LicenceEntry {
    pub spdx: String,
    pub lockfiles: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct LicenceAllow {
    pub entries: Vec<LicenceEntry>,
}

impl LicenceAllow {
    pub fn from_table(t: &Table) -> Result<LicenceAllow, String> {
        const F: &str = "xtask/licence-allow.toml";
        check_version(t, F)?;
        let mut out = Vec::new();
        for (i, e) in entries(t, "licence", F)?.iter().enumerate() {
            let what = format!("{F}: licence entry {}", i + 1);
            let spdx = req_str(e, "spdx", &what)?;
            let lockfiles = str_list(e, "lockfiles", &what)?;
            for l in &lockfiles {
                if !LOCKFILES.contains(&l.as_str()) {
                    return Err(format!("{what}: unknown lockfile '{l}'"));
                }
            }
            let basis = req_str(e, "basis", &what)?;
            if basis != "named" && basis != "similar" {
                return Err(format!("{what}: 'basis' must be \"named\" or \"similar\""));
            }
            out.push(LicenceEntry { spdx, lockfiles });
        }
        Ok(LicenceAllow { entries: out })
    }

    /// The licences allowed in one lockfile.
    pub fn allowed_in(&self, lockfile: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.lockfiles.iter().any(|l| l == lockfile))
            .map(|e| e.spdx.as_str())
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// crates.toml

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Product,
    TestOnly,
    Tool,
    HostOnly,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        match s {
            "product" => Some(Kind::Product),
            "test-only" => Some(Kind::TestOnly),
            "tool" => Some(Kind::Tool),
            "host-only" => Some(Kind::HostOnly),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Product => "product",
            Kind::TestOnly => "test-only",
            Kind::Tool => "tool",
            Kind::HostOnly => "host-only",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CrateKinds {
    pub crates: Vec<(String, Kind)>,
}

impl CrateKinds {
    pub fn from_table(t: &Table) -> Result<CrateKinds, String> {
        const F: &str = "xtask/crates.toml";
        check_version(t, F)?;
        let mut crates = Vec::new();
        for (i, e) in entries(t, "crate", F)?.iter().enumerate() {
            let what = format!("{F}: crate entry {}", i + 1);
            let name = req_str(e, "name", &what)?;
            let k = req_str(e, "kind", &what)?;
            let kind = Kind::parse(&k).ok_or_else(|| format!("{what}: unknown kind '{k}'"))?;
            if crates.iter().any(|(n, _)| *n == name) {
                return Err(format!("{what}: '{name}' is listed twice"));
            }
            crates.push((name, kind));
        }
        Ok(CrateKinds { crates })
    }

    pub fn kind(&self, name: &str) -> Option<Kind> {
        self.crates.iter().find(|(n, _)| n == name).map(|(_, k)| *k)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// roles.toml

#[derive(Clone, Debug)]
pub struct Role {
    /// The worktree and branch name (`m0/<name>`), lower case.
    pub name: String,
    /// The name used in PLAN §3.1 and authors.md, e.g. `R-HARN-I`.
    pub title: String,
    pub lane: String,
    pub deny_read: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Roles {
    pub lanes: Vec<(String, String)>,
    pub roles: Vec<Role>,
}

impl Roles {
    pub fn from_table(t: &Table) -> Result<Roles, String> {
        const F: &str = "xtask/roles.toml";
        check_version(t, F)?;
        let mut lanes = Vec::new();
        match t.get("lanes") {
            Some(Value::Table(l)) => {
                for (k, v) in l {
                    let dir = v
                        .as_str()
                        .ok_or_else(|| format!("{F}: lanes.{k} must be a string"))?;
                    lanes.push((k.clone(), dir.to_string()));
                }
            }
            _ => return Err(format!("{F}: missing [lanes] table")),
        }
        let mut roles = Vec::new();
        for (i, e) in entries(t, "role", F)?.iter().enumerate() {
            let what = format!("{F}: role entry {}", i + 1);
            let name = req_str(e, "name", &what)?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            {
                return Err(format!(
                    "{what}: role name '{name}' must be lower-case letters, digits and '-'"
                ));
            }
            let lane = req_str(e, "lane", &what)?;
            if !lanes.iter().any(|(l, _)| *l == lane) {
                return Err(format!("{what}: unknown lane '{lane}'"));
            }
            req_str(e, "reason", &what)?;
            roles.push(Role {
                name,
                title: req_str(e, "title", &what)?,
                lane,
                deny_read: str_list(e, "deny_read", &what)?,
            });
        }
        Ok(Roles { lanes, roles })
    }

    pub fn role(&self, name: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.name == name)
    }

    pub fn lane_dir(&self, lane: &str) -> Option<&str> {
        self.lanes
            .iter()
            .find(|(l, _)| l == lane)
            .map(|(_, d)| d.as_str())
    }
}

/// Every reviewed file, loaded together.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub host_only: HostOnly,
    pub roots: Roots,
    pub native: NativeAllow,
    pub osdeps: OsdepsAllow,
    pub licences: LicenceAllow,
    pub kinds: CrateKinds,
    pub roles: Roles,
}

impl Config {
    pub fn load(repo: &Path) -> Result<Config, String> {
        let x = repo.join("xtask");
        Ok(Config {
            host_only: HostOnly::from_table(&read_toml(&x.join("host-only.toml"))?)?,
            roots: Roots::from_table(&read_toml(&x.join("roots.toml"))?)?,
            native: NativeAllow::from_table(&read_toml(&x.join("native-allow.toml"))?)?,
            osdeps: OsdepsAllow::from_table(&read_toml(&x.join("osdeps-allow.toml"))?)?,
            licences: LicenceAllow::from_table(&read_toml(&x.join("licence-allow.toml"))?)?,
            kinds: CrateKinds::from_table(&read_toml(&x.join("crates.toml"))?)?,
            roles: Roles::from_table(&read_toml(&x.join("roles.toml"))?)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    #[test]
    fn repository_files_load() {
        let c = Config::load(&repo()).unwrap();
        assert!(c.host_only.contains("moirai-tsoracle"));
        assert!(c.roots.contains("moirai-probes-bin"));
        assert!(c.roles.role("r-harn-i").is_some());
        assert_eq!(c.kinds.kind("moirai-os"), Some(Kind::Product));
        assert!(c.licences.allowed_in("fuzz/Cargo.lock").contains(&"NCSA"));
        assert!(!c.licences.allowed_in("Cargo.lock").contains(&"NCSA"));
    }

    #[test]
    fn native_entries_per_graph() {
        let entry = |graph: &str, features: &str| {
            format!(
                "[[package]]\nname = \"libc\"\nversions = \"=0.2.1\"\ngraphs = [\"{graph}\"]\nfeatures = [{features}]\nscript = \"s\"\nreason = \"r\"\n"
            )
        };
        let text = format!(
            "version = 1\n{}{}",
            entry("checked", ""),
            entry("fuzz", "\"std\"")
        );
        let n = NativeAllow::from_table(&toml::parse(&text).unwrap()).unwrap();
        let features = |g| n.find("libc", "0.2.1", g).unwrap().features.clone();
        assert!(features(Graph::Checked).is_empty());
        assert_eq!(features(Graph::Fuzz), vec!["std".to_string()]);
        // No entry lists the graph: the first covering entry, so the lint names the graph it lacks.
        assert_eq!(
            n.find("libc", "0.2.1", Graph::HostOnly).unwrap().graphs,
            vec![Graph::Checked]
        );
        assert!(n.find("libc", "0.2.2", Graph::Checked).is_none());
    }

    #[test]
    fn refuses_entries_that_cover_the_same_version_and_graph() {
        let entry = |versions: &str, graphs: &str| {
            format!(
                "[[package]]\nname = \"libc\"\nversions = \"{versions}\"\ngraphs = [{graphs}]\nfeatures = []\nscript = \"s\"\nreason = \"r\"\n"
            )
        };
        let load = |a: String, b: String| {
            NativeAllow::from_table(&toml::parse(&format!("version = 1\n{a}{b}")).unwrap())
        };
        // The same pin in a shared graph, and a range that contains the other entry's pin.
        for (a, b) in [
            (
                entry("=0.2.1", "\"checked\", \"fuzz\""),
                entry("=0.2.1", "\"fuzz\""),
            ),
            (entry("=0.2.1", "\"fuzz\""), entry("0.2", "\"fuzz\"")),
        ] {
            let e = load(a, b).unwrap_err();
            assert!(
                e.contains("package entry 2") && e.contains("overlaps package entry 1"),
                "{e}"
            );
        }
        // Disjoint graphs, or disjoint versions of one graph, are separate entries.
        load(entry("=0.2.1", "\"checked\""), entry("=0.2.1", "\"fuzz\"")).unwrap();
        load(entry("=0.2.1", "\"fuzz\""), entry("=0.2.2", "\"fuzz\"")).unwrap();
    }

    #[test]
    fn refuses_bad_entries() {
        let t = toml::parse("version = 1\n[[package]]\nname = \"x\"\nversions = \"1\"\ngraphs = [\"nope\"]\nfeatures = []\nscript = \"s\"\nreason = \"r\"\n").unwrap();
        assert!(
            NativeAllow::from_table(&t)
                .unwrap_err()
                .contains("unknown graph")
        );
        let t = toml::parse("version = 2\n").unwrap();
        assert!(HostOnly::from_table(&t).is_err());
        let t = toml::parse(
            "version = 1\n[[crate]]\nname = \"a\"\nscans = [\"cfg\"]\nreason = \"r\"\n",
        )
        .unwrap();
        assert!(OsdepsAllow::from_table(&t).is_err());
        let t = toml::parse("version = 1\n[[licence]]\nspdx = \"MIT\"\nlockfiles = [\"other/Cargo.lock\"]\nbasis = \"named\"\n").unwrap();
        assert!(LicenceAllow::from_table(&t).is_err());
    }
}
