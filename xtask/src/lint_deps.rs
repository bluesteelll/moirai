//! GT20 (b), the licence lint, the GT20 (d) dependency part and the crate-kind cross-checks, as pure functions over
//! `cargo metadata` JSON and lockfile text (docs/m0/PLAN.md WP-02, §2.1, §2.4; [90 §11.2]; [AR §8.3] GT20 (b), (d);
//! [80 §5.5] (a)).
//!
//! GT20 (b):
//! - **Lockfiles.** Exactly `Cargo.lock` and `fuzz/Cargo.lock` are scanned; any other `Cargo.lock` in the repository
//!   is refused.
//! - **Base list.** No git library (`gix*`, `git2`, `libgit2-sys`), no embedded-database crate (SQLite bindings,
//!   `redb`, `heed`/`lmdb*`, `fjall`, `sled`, `rocksdb`, `libsql`/Turso and the like) and none of the crates PLAN §2.4
//!   rules out everywhere (`zstd`, `zstd-sys`) or rule 4 implies (`flate2`'s C backends) in either lockfile.
//! - **Rule 1.** Every package with a `custom-build` target in any graph of the four targets has an entry in
//!   `xtask/native-allow.toml` that covers its version and graph and asserts its resolved feature set exactly.
//! - **Rule 2.** No package outside that file declares `links` or a build dependency on a native-build crate.
//! - **Rule 3.** No checked crate depends on a host-only crate or on a composition root, dev-dependencies included.
//! - **Rule 4.** `sha1`/`sha2` never with an `asm` feature, `flate2` never with a C backend, `blake3` only with `pure`.
//!
//! Graphs: `checked` is the closure of every workspace member that is not host-only (roots included, for all four
//! targets); `host-only` is the closure of the host-only members; `fuzz` is the `fuzz/` workspace.

use crate::config::{Config, Graph, Kind};
use crate::diag::Diag;
use crate::metadata::{DepKind, Metadata, Package};
use crate::spdx;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

/// Build-script dependencies that compile or link native code (rule 2).
pub const NATIVE_BUILD: &[&str] = &[
    "cc",
    "gcc",
    "cmake",
    "bindgen",
    "pkg-config",
    "vcpkg",
    "autotools",
    "nasm-rs",
    "cxx-build",
    "cpp_build",
    "system-deps",
    "metadeps",
    "meson",
    "embed-resource",
    "winres",
    "winresource",
];

/// `(pattern, reason)`: a trailing `*` is a prefix match. Names are compared lower-case with `_` read as `-`.
pub const FORBIDDEN: &[(&str, &str)] = &[
    ("gix*", "a git library (GT20 (b))"),
    ("git2", "a git library (GT20 (b))"),
    ("git2-*", "a git library (GT20 (b))"),
    ("libgit2-sys", "a git library (GT20 (b))"),
    (
        "rusqlite",
        "an embedded database: SQLite bindings (GT20 (b))",
    ),
    (
        "libsqlite3-sys",
        "an embedded database: SQLite bindings (GT20 (b))",
    ),
    ("sqlite", "an embedded database: SQLite bindings (GT20 (b))"),
    (
        "sqlite3-sys",
        "an embedded database: SQLite bindings (GT20 (b))",
    ),
    (
        "sqlite3-src",
        "an embedded database: SQLite bindings (GT20 (b))",
    ),
    (
        "sqlx-sqlite",
        "an embedded database: SQLite bindings (GT20 (b))",
    ),
    ("redb", "an embedded database (GT20 (b))"),
    ("heed", "an embedded database: LMDB (GT20 (b))"),
    ("heed-*", "an embedded database: LMDB (GT20 (b))"),
    ("lmdb*", "an embedded database: LMDB (GT20 (b))"),
    ("fjall", "an embedded database (GT20 (b))"),
    (
        "lsm-tree",
        "an embedded database: fjall's engine (GT20 (b))",
    ),
    ("sled", "an embedded database (GT20 (b))"),
    ("rocksdb", "an embedded database (GT20 (b))"),
    ("librocksdb-sys", "an embedded database (GT20 (b))"),
    ("rust-rocksdb", "an embedded database (GT20 (b))"),
    ("libsql*", "an embedded database: libSQL/Turso (GT20 (b))"),
    ("turso*", "an embedded database: libSQL/Turso (GT20 (b))"),
    ("limbo*", "an embedded database: Turso's engine (GT20 (b))"),
    ("duckdb", "an embedded database (GT20 (b))"),
    ("libduckdb-sys", "an embedded database (GT20 (b))"),
    ("persy", "an embedded database (GT20 (b))"),
    ("sanakirja", "an embedded database (GT20 (b))"),
    ("jammdb", "an embedded database (GT20 (b))"),
    ("native_db", "an embedded database (GT20 (b))"),
    ("polodb*", "an embedded database (GT20 (b))"),
    ("surrealkv", "an embedded database (GT20 (b))"),
    ("zstd", "compiles libzstd; never used anywhere (PLAN §2.4)"),
    (
        "zstd-sys",
        "compiles libzstd; never used anywhere (PLAN §2.4)",
    ),
    (
        "zstd-safe",
        "compiles libzstd; never used anywhere (PLAN §2.4)",
    ),
    ("libz-sys", "a C zlib backend of flate2 (GT20 (b) rule 4)"),
    (
        "libz-ng-sys",
        "a C zlib-ng backend of flate2 (GT20 (b) rule 4)",
    ),
    (
        "cloudflare-zlib-sys",
        "a C zlib backend of flate2 (GT20 (b) rule 4)",
    ),
    (
        "miniz-sys",
        "a C deflate backend of flate2 (GT20 (b) rule 4)",
    ),
    ("sha1-asm", "assembly for sha1 (GT20 (b) rule 4)"),
    ("sha2-asm", "assembly for sha2 (GT20 (b) rule 4)"),
];

const FLATE2_C: &[&str] = &[
    "zlib",
    "zlib-default",
    "zlib-ng",
    "zlib-ng-compat",
    "cloudflare_zlib",
    "miniz-sys",
];

pub fn norm(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

pub fn forbidden(name: &str) -> Option<&'static str> {
    let n = norm(name);
    FORBIDDEN.iter().find_map(|(pat, why)| {
        let p = norm(pat);
        let hit = match p.strip_suffix('*') {
            Some(prefix) => n.starts_with(prefix),
            None => n == p,
        };
        hit.then_some(*why)
    })
}

/// One `[[package]]` of a lockfile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockPkg {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
}

pub fn parse_lockfile(text: &str) -> Result<Vec<LockPkg>, String> {
    let t = crate::toml::parse(text).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    if let Some(pkgs) = t.get("package").and_then(crate::toml::Value::as_array) {
        for p in pkgs {
            let name = p
                .get_path(&["name"])
                .and_then(|v| v.as_str())
                .ok_or("a [[package]] without a name")?;
            let version = p
                .get_path(&["version"])
                .and_then(|v| v.as_str())
                .ok_or("a [[package]] without a version")?;
            out.push(LockPkg {
                name: name.to_string(),
                version: version.to_string(),
                source: p
                    .get_path(&["source"])
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }
    Ok(out)
}

pub struct DepInputs<'a> {
    /// `(target triple, cargo metadata --filter-platform <triple>)` for each of the four targets.
    pub targets: &'a [(String, Metadata)],
    /// Unfiltered metadata of the main workspace.
    pub full: &'a Metadata,
    /// Unfiltered metadata of the `fuzz/` workspace, when it exists.
    pub fuzz: Option<&'a Metadata>,
    /// `(repository-relative path, text)` of `Cargo.lock` and `fuzz/Cargo.lock` as found.
    pub lockfiles: &'a [(String, String)],
    /// Every file named `Cargo.lock` in the repository (tracked or not ignored), repository-relative.
    pub found_lockfiles: &'a [String],
    pub config: &'a Config,
}

/// The package ids of each graph of one metadata document.
pub struct Graphs {
    pub checked: HashSet<String>,
    pub host_only: HashSet<String>,
}

pub fn graphs(md: &Metadata, host_only: &crate::config::HostOnly) -> Graphs {
    let host_ids: HashSet<String> = md
        .workspace_members
        .iter()
        .filter(|id| md.package(id).is_some_and(|p| host_only.contains(&p.name)))
        .cloned()
        .collect();
    let closure = |starts: Vec<String>, stop: &HashSet<String>| {
        let mut seen: HashSet<String> = starts.iter().cloned().collect();
        let mut q: VecDeque<String> = starts.into_iter().collect();
        while let Some(id) = q.pop_front() {
            let member = md.is_member(&id);
            let Some(node) = md.node(&id) else { continue };
            for d in &node.deps {
                let follow = member || d.kinds.iter().any(|k| *k != DepKind::Dev);
                if follow && !stop.contains(&d.pkg) && seen.insert(d.pkg.clone()) {
                    q.push_back(d.pkg.clone());
                }
            }
        }
        seen
    };
    let checked_starts: Vec<String> = md
        .workspace_members
        .iter()
        .filter(|id| !host_ids.contains(*id))
        .cloned()
        .collect();
    let checked = closure(checked_starts, &host_ids);
    let host_only = closure(host_ids.iter().cloned().collect(), &HashSet::new());
    Graphs { checked, host_only }
}

/// Collects findings that repeat across targets into one line naming the targets.
#[derive(Default)]
struct Collect {
    by_key: BTreeMap<(String, String), (Diag, BTreeSet<String>)>,
}

impl Collect {
    fn add(&mut self, d: Diag, target: &str) {
        let key = (d.message.clone(), d.path.clone().unwrap_or_default());
        self.by_key
            .entry(key)
            .or_insert_with(|| (d, BTreeSet::new()))
            .1
            .insert(target.to_string());
    }

    fn finish(self, all_targets: usize) -> Vec<Diag> {
        self.by_key
            .into_values()
            .map(|(mut d, ts)| {
                if !ts.is_empty()
                    && !(ts.len() == all_targets && all_targets > 1)
                    && !ts.contains("")
                {
                    d.message = format!(
                        "{} [target {}]",
                        d.message,
                        ts.into_iter().collect::<Vec<_>>().join(", ")
                    );
                }
                d
            })
            .collect()
    }
}

fn pkg_label(p: &Package) -> String {
    format!("{} {}", p.name, p.version)
}

/// Rules 1, 2 and 4 over one graph of one metadata document.
fn check_graph(
    md: &Metadata,
    ids: &HashSet<String>,
    graph: Graph,
    cfg: &Config,
    target: &str,
    c: &mut Collect,
) {
    let mut sorted: Vec<&String> = ids.iter().collect();
    sorted.sort();
    for id in sorted {
        let Some(p) = md.package(id) else { continue };
        let entry = cfg.native.find(&p.name, &p.version, graph);
        let lint = "gt20-b";
        let d = |msg: String| Diag::krate(lint, &p.name, msg);
        // Rule 1.
        if p.has_build_script() {
            match entry {
                None => {
                    let mut resolved: Vec<&str> =
                        md.features(id).iter().map(String::as_str).collect();
                    resolved.sort_unstable();
                    c.add(
                        d(format!(
                            "{} has a build script and no entry in xtask/native-allow.toml (rule 1; {} graph; resolved features [{}])",
                            pkg_label(p),
                            graph.label(),
                            resolved.join(", ")
                        )),
                        target,
                    )
                }
                Some(e) => {
                    let mut resolved: Vec<&str> =
                        md.features(id).iter().map(String::as_str).collect();
                    resolved.sort_unstable();
                    let mut asserted: Vec<&str> = e.features.iter().map(String::as_str).collect();
                    asserted.sort_unstable();
                    if resolved != asserted {
                        c.add(
                            d(format!(
                                "{} resolves with features [{}], but xtask/native-allow.toml asserts [{}] (rule 1)",
                                pkg_label(p),
                                resolved.join(", "),
                                asserted.join(", ")
                            )),
                            target,
                        );
                    }
                }
            }
        }
        if let Some(e) = entry {
            if !e.graphs.contains(&graph) {
                c.add(
                    d(format!(
                        "{} is in the {} graph, but its xtask/native-allow.toml entry (versions {}) allows only [{}]",
                        pkg_label(p),
                        graph.label(),
                        e.versions_raw,
                        e.graphs.iter().map(|g| g.label()).collect::<Vec<_>>().join(", ")
                    )),
                    target,
                );
            }
            if e.links != p.links {
                c.add(
                    d(format!(
                        "{} declares links = {:?}, but its xtask/native-allow.toml entry says {:?} (rule 2)",
                        pkg_label(p),
                        p.links,
                        e.links
                    )),
                    target,
                );
            }
        } else {
            // Rule 2.
            if let Some(l) = &p.links {
                c.add(
                    d(format!(
                        "{} declares links = \"{l}\" and has no entry in xtask/native-allow.toml (rule 2)",
                        pkg_label(p)
                    )),
                    target,
                );
            }
            for dep in &p.dependencies {
                if dep.kind == DepKind::Build && NATIVE_BUILD.contains(&norm(&dep.name).as_str()) {
                    c.add(
                        d(format!(
                            "{} has a build dependency on {} and no entry in xtask/native-allow.toml (rule 2)",
                            pkg_label(p),
                            dep.name
                        )),
                        target,
                    );
                }
            }
        }
        // Rule 4.
        let feats = md.features(id);
        match norm(&p.name).as_str() {
            "sha1" | "sha2" => {
                for f in feats.iter().filter(|f| f.starts_with("asm")) {
                    c.add(
                        d(format!(
                            "{} resolves with the '{f}' feature (rule 4: sha1/sha2 never asm)",
                            pkg_label(p)
                        )),
                        target,
                    );
                }
            }
            "flate2" => {
                for f in feats.iter().filter(|f| FLATE2_C.contains(&f.as_str())) {
                    c.add(
                        d(format!(
                            "{} resolves with the C backend '{f}' (rule 4)",
                            pkg_label(p)
                        )),
                        target,
                    );
                }
            }
            "blake3" if !feats.iter().any(|f| f == "pure") => {
                c.add(
                    d(format!(
                        "{} resolves without the 'pure' feature (rule 4: blake3 only with pure)",
                        pkg_label(p)
                    )),
                    target,
                );
            }
            _ => {}
        }
    }
}

/// GT20 (b): the lockfile set, the base list and rules 1 to 4.
pub fn gt20b(inp: &DepInputs<'_>) -> Vec<Diag> {
    let mut out = Vec::new();
    let cfg = inp.config;
    // Lockfile set.
    for f in inp.found_lockfiles {
        if !crate::config::LOCKFILES.contains(&f.as_str()) {
            out.push(Diag::path(
                "gt20-b",
                f,
                "a Cargo.lock outside the two the lints scan (Cargo.lock, fuzz/Cargo.lock): remove it or make its workspace part of one of them",
            ));
        }
    }
    if !inp.lockfiles.iter().any(|(p, _)| p == "Cargo.lock") {
        out.push(Diag::path(
            "gt20-b",
            "Cargo.lock",
            "missing: every gate runs --locked (PLAN §2.1)",
        ));
    }
    // Base list over the lockfile text.
    for (path, text) in inp.lockfiles {
        match parse_lockfile(text) {
            Err(e) => out.push(Diag::path(
                "gt20-b",
                path,
                format!("unreadable lockfile: {e}"),
            )),
            Ok(pkgs) => {
                for p in &pkgs {
                    if let Some(why) = forbidden(&p.name) {
                        out.push(Diag::path(
                            "gt20-b",
                            path,
                            format!("{} {}: {why}", p.name, p.version),
                        ));
                    }
                }
            }
        }
    }
    // Rules 1, 2, 4 per target and graph.
    let mut c = Collect::default();
    for (target, md) in inp.targets {
        let g = graphs(md, &cfg.host_only);
        check_graph(md, &g.checked, Graph::Checked, cfg, target, &mut c);
        let host_only_only: HashSet<String> = g.host_only.difference(&g.checked).cloned().collect();
        check_graph(md, &host_only_only, Graph::HostOnly, cfg, target, &mut c);
    }
    if let Some(fz) = inp.fuzz {
        let all: HashSet<String> = fz.packages.iter().map(|p| p.id.clone()).collect();
        check_graph(fz, &all, Graph::Fuzz, cfg, "", &mut c);
    }
    out.extend(c.finish(inp.targets.len()));
    // Rule 3.
    let md = inp.full;
    for m in md.members() {
        if cfg.host_only.contains(&m.name) {
            continue;
        }
        for dep in &m.dependencies {
            let is_member = md.member_named(&dep.name).is_some();
            if !is_member {
                continue;
            }
            if cfg.host_only.contains(&dep.name) {
                out.push(Diag::krate(
                    "gt20-b",
                    &m.name,
                    format!(
                        "{} has a {} dependency on the host-only crate {} (rule 3)",
                        m.name,
                        dep.kind.label(),
                        dep.name
                    ),
                ));
            }
            if cfg.roots.roots.iter().any(|r| r.name == dep.name) {
                out.push(Diag::krate(
                    "gt20-b",
                    &m.name,
                    format!(
                        "{} has a {} dependency on the composition root {} (rule 3)",
                        m.name,
                        dep.kind.label(),
                        dep.name
                    ),
                ));
            }
        }
    }
    out
}

/// The licence lint over both lockfiles (PLAN §2.4).
pub fn licences(inp: &DepInputs<'_>) -> Vec<Diag> {
    let mut out = Vec::new();
    for (path, text) in inp.lockfiles {
        let md = if path == "Cargo.lock" {
            Some(inp.full)
        } else {
            inp.fuzz
        };
        let Some(md) = md else {
            out.push(Diag::path("licence", path, "no cargo metadata for this lockfile's workspace: licences cannot be checked (fails closed)"));
            continue;
        };
        let allowed = inp.config.licences.allowed_in(path);
        let pkgs = match parse_lockfile(text) {
            Ok(p) => p,
            Err(e) => {
                out.push(Diag::path(
                    "licence",
                    path,
                    format!("unreadable lockfile: {e}"),
                ));
                continue;
            }
        };
        for lp in &pkgs {
            let p = md.packages.iter().find(|p| {
                p.name == lp.name
                    && p.version == lp.version
                    && match (&p.source, &lp.source) {
                        (Some(a), Some(b)) => a == b,
                        _ => true,
                    }
            });
            let label = format!("{} {}", lp.name, lp.version);
            let Some(p) = p else {
                out.push(Diag::path(
                    "licence",
                    path,
                    format!("{label}: not in cargo metadata, licence unknown (fails closed)"),
                ));
                continue;
            };
            match &p.license {
                None if p.license_file.is_some() => out.push(Diag::krate(
                    "licence",
                    &p.name,
                    format!(
                        "{label} ({path}): only a license-file, no SPDX licence (fails closed)"
                    ),
                )),
                None => out.push(Diag::krate(
                    "licence",
                    &p.name,
                    format!("{label} ({path}): no licence (fails closed)"),
                )),
                Some(expr) => match spdx::parse(expr) {
                    Err(e) => out.push(Diag::krate(
                        "licence",
                        &p.name,
                        format!("{label} ({path}): {e}"),
                    )),
                    Ok(e) => {
                        if let Err(bad) = spdx::check(&e, &allowed) {
                            out.push(Diag::krate(
                                "licence",
                                &p.name,
                                format!(
                                    "{label} ({path}): licence '{expr}' is not allowed in {path} by xtask/licence-allow.toml (failing terms: {})",
                                    bad.join(", ")
                                ),
                            ));
                        }
                    }
                },
            }
        }
    }
    out
}

/// GT20 (d), dependency part: direct dependencies on an OS-binding crate ([`crate::config::OSDEPS`]: `windows-sys`,
/// `libc` and their kin) only in `moirai-os`; third-party paths to them only through `[[transitive]]` entries of
/// `xtask/osdeps-allow.toml`. An edge between two OS-binding crates (`windows-sys -> windows-link`, `nix -> libc`)
/// is inside the binding family and needs no entry.
pub fn osdeps(inp: &DepInputs<'_>) -> Vec<Diag> {
    let cfg = inp.config;
    let mut out = Vec::new();
    for m in inp.full.members() {
        if m.name == "moirai-os" || cfg.osdeps.exempt(&m.name, "direct-osdep") {
            continue;
        }
        for dep in &m.dependencies {
            let n = norm(&dep.name);
            if crate::config::is_osdep(&n) {
                out.push(Diag::krate(
                    "gt20-d",
                    &m.name,
                    format!(
                        "{} declares a direct {} dependency on {} (only moirai-os may, PLAN §2.1)",
                        m.name,
                        dep.kind.label(),
                        dep.name
                    ),
                ));
            }
        }
    }
    let mut c = Collect::default();
    for (target, md) in inp.targets {
        let g = graphs(md, &cfg.host_only);
        let mut ids: Vec<&String> = g.checked.union(&g.host_only).collect();
        ids.sort();
        for id in ids {
            if md.is_member(id) {
                continue;
            }
            let (Some(p), Some(node)) = (md.package(id), md.node(id)) else {
                continue;
            };
            for d in &node.deps {
                let Some(dp) = md.package(&d.pkg) else {
                    continue;
                };
                let n = norm(&dp.name);
                if crate::config::is_osdep(&n)
                    && !crate::config::is_osdep(&norm(&p.name))
                    && !cfg.osdeps.transitive_ok(&p.name, &p.version, &n)
                {
                    c.add(
                        Diag::krate(
                            "gt20-d",
                            &p.name,
                            format!(
                                "third-party path {} -> {} has no [[transitive]] entry in xtask/osdeps-allow.toml ([80 §5.5] (a))",
                                pkg_label(p),
                                dp.name
                            ),
                        ),
                        target,
                    );
                }
            }
        }
    }
    out.extend(c.finish(inp.targets.len()));
    out
}

/// Cross-checks `xtask/crates.toml` with the members, `host-only.toml`, `roots.toml` and `osdeps-allow.toml`, and
/// that every member is `publish = false`.
pub fn crate_kinds(inp: &DepInputs<'_>) -> Vec<Diag> {
    let cfg = inp.config;
    let md = inp.full;
    let mut out = Vec::new();
    let names: Vec<&str> = md.members().map(|p| p.name.as_str()).collect();
    for n in &names {
        match cfg.kinds.kind(n) {
            None => out.push(Diag::krate(
                "crates",
                n,
                format!("workspace member {n} has no entry in xtask/crates.toml"),
            )),
            Some(k) => {
                if (k == Kind::HostOnly) != cfg.host_only.contains(n) {
                    out.push(Diag::krate(
                        "crates",
                        n,
                        format!("{n}: xtask/crates.toml says '{}', which disagrees with xtask/host-only.toml", k.label()),
                    ));
                }
                if k == Kind::Product && cfg.roots.contains(n) {
                    out.push(Diag::krate(
                        "crates",
                        n,
                        format!("{n} is a product crate and cannot be a composition root"),
                    ));
                }
                if k == Kind::Product && cfg.osdeps.crates.iter().any(|c| c.name == *n) {
                    out.push(Diag::krate("crates", n, format!("{n} is a product crate and cannot be exempted in xtask/osdeps-allow.toml")));
                }
            }
        }
    }
    for (n, _) in &cfg.kinds.crates {
        if !names.contains(&n.as_str()) {
            out.push(Diag::new(
                "crates",
                format!("xtask/crates.toml lists {n}, which is not a workspace member"),
            ));
        }
    }
    for h in &cfg.host_only.crates {
        if !names.contains(&h.as_str()) {
            out.push(Diag::new(
                "crates",
                format!("xtask/host-only.toml lists {h}, which is not a workspace member"),
            ));
        }
    }
    for p in md.members() {
        if p.publish.as_ref().is_none_or(|v| !v.is_empty()) {
            out.push(Diag::krate(
                "crates",
                &p.name,
                format!("{} is not publish = false (PLAN §2.1)", p.name),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::tests::synth;
    use std::path::Path;

    fn fixture(name: &str) -> String {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/deps")
            .join(name);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    fn config() -> Config {
        let mut c = Config::default();
        let t = |s: &str| crate::toml::parse(s).unwrap();
        c.host_only = crate::config::HostOnly::from_table(&t(
            "version = 1\n[[crate]]\nname = \"oracle\"\nreason = \"C\"\n",
        ))
        .unwrap();
        c.roots = crate::config::Roots::from_table(&t(
            "version = 1\n[[root]]\nname = \"root\"\npresent = true\nproduct = false\nbin_paths = [\"src/bin/*.rs\"]\nmax_lines = 200\nallowed_deps = []\nwindows_check = \"x\"\n",
        ))
        .unwrap();
        c.native = crate::config::NativeAllow::from_table(&t(
            "version = 1\n[[package]]\nname = \"blake3\"\nversions = \"1.8\"\ngraphs = [\"checked\"]\nfeatures = [\"pure\", \"std\"]\nscript = \"s\"\nreason = \"r\"\n[[package]]\nname = \"tsc\"\nversions = \"*\"\ngraphs = [\"host-only\"]\nfeatures = []\nlinks = \"tsc\"\nscript = \"s\"\nreason = \"r\"\n",
        ))
        .unwrap();
        c.licences = crate::config::LicenceAllow::from_table(&t(&std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("licence-allow.toml"),
        )
        .unwrap()))
        .unwrap();
        c
    }

    fn run_b(md: &Metadata, lock: &str, found: &[&str]) -> Vec<Diag> {
        let cfg = config();
        let targets = vec![("x86_64-pc-windows-msvc".to_string(), md.clone())];
        let lockfiles = vec![("Cargo.lock".to_string(), lock.to_string())];
        let found: Vec<String> = found.iter().map(|s| s.to_string()).collect();
        gt20b(&DepInputs {
            targets: &targets,
            full: md,
            fuzz: None,
            lockfiles: &lockfiles,
            found_lockfiles: &found,
            config: &cfg,
        })
    }

    fn clean_md() -> Metadata {
        synth(
            &["app", "oracle"],
            &[
                (
                    "app",
                    "0.0.0",
                    None,
                    Some("Apache-2.0"),
                    false,
                    None,
                    &[("blake3", "normal")],
                ),
                (
                    "oracle",
                    "0.0.0",
                    None,
                    Some("Apache-2.0"),
                    false,
                    None,
                    &[("tsc", "normal"), ("app", "normal")],
                ),
                (
                    "blake3",
                    "1.8.7",
                    Some("registry+r"),
                    Some("CC0-1.0 OR Apache-2.0"),
                    true,
                    None,
                    &[("cc", "build")],
                ),
                (
                    "cc",
                    "1.2.0",
                    Some("registry+r"),
                    Some("MIT OR Apache-2.0"),
                    false,
                    None,
                    &[],
                ),
                (
                    "tsc",
                    "0.1.0",
                    Some("registry+r"),
                    Some("MIT"),
                    true,
                    Some("tsc"),
                    &[("cc", "build")],
                ),
            ],
            &[("blake3", &["pure", "std"])],
        )
    }

    #[test]
    fn clean_graph_passes() {
        let d = run_b(&clean_md(), "version = 4\n", &["Cargo.lock"]);
        assert!(d.is_empty(), "{d:#?}");
    }

    #[test]
    fn seeded_git_library_in_each_lockfile() {
        let d = run_b(
            &clean_md(),
            &fixture("git-library.lock.txt"),
            &["Cargo.lock"],
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("gix-hash") && x.message.contains("git library")),
            "{d:#?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("libgit2-sys")),
            "{d:#?}"
        );
        let cfg = config();
        let md = clean_md();
        let targets = vec![("t".to_string(), md.clone())];
        let lockfiles = vec![
            ("Cargo.lock".to_string(), "version = 4\n".to_string()),
            ("fuzz/Cargo.lock".to_string(), fixture("fuzz-git2.lock.txt")),
        ];
        let found = vec!["Cargo.lock".to_string(), "fuzz/Cargo.lock".to_string()];
        let d = gt20b(&DepInputs {
            targets: &targets,
            full: &md,
            fuzz: None,
            lockfiles: &lockfiles,
            found_lockfiles: &found,
            config: &cfg,
        });
        assert!(d.iter().any(|x| x.path.as_deref() == Some("fuzz/Cargo.lock") && x.message.contains("git2")), "{d:#?}");
    }

    #[test]
    fn seeded_embedded_database() {
        let d = run_b(
            &clean_md(),
            &fixture("embedded-db.lock.txt"),
            &["Cargo.lock"],
        );
        for n in [
            "rusqlite",
            "libsqlite3-sys",
            "redb",
            "heed",
            "lmdb-master-sys",
            "fjall",
            "sled",
            "librocksdb-sys",
            "libsql",
            "turso_core",
        ] {
            assert!(
                d.iter().any(|x| x.message.starts_with(&format!("{n} "))
                    && x.message.contains("embedded database")),
                "{n}: {d:#?}"
            );
        }
        assert!(!d.iter().any(|x| x.message.starts_with("serde ")));
    }

    #[test]
    fn seeded_unlisted_lockfile() {
        let d = run_b(
            &clean_md(),
            "version = 4\n",
            &["Cargo.lock", "crates/x/Cargo.lock"],
        );
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].path.as_deref(), Some("crates/x/Cargo.lock"));
    }

    #[test]
    fn seeded_blake3_without_pure() {
        let md = Metadata::from_json(&fixture("blake3-no-pure.json")).unwrap();
        let d = run_b(&md, "version = 4\n", &["Cargo.lock"]);
        assert!(
            d.iter()
                .any(|x| x.message.contains("without the 'pure' feature")),
            "{d:#?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("asserts [pure, std]")),
            "{d:#?}"
        );
    }

    #[test]
    fn seeded_sha2_with_asm() {
        let md = Metadata::from_json(&fixture("sha2-asm.json")).unwrap();
        let d = run_b(&md, "version = 4\n", &["Cargo.lock"]);
        assert!(
            d.iter().any(|x| x.message.contains("'asm' feature")),
            "{d:#?}"
        );
    }

    #[test]
    fn seeded_checked_to_host_only_and_root_edges() {
        let md = Metadata::from_json(&fixture("checked-to-host-only.json")).unwrap();
        let d = run_b(&md, "version = 4\n", &["Cargo.lock"]);
        assert!(
            d.iter().any(|x| x
                .message
                .contains("dev dependency on the host-only crate oracle")),
            "{d:#?}"
        );
        let md = Metadata::from_json(&fixture("checked-to-root.json")).unwrap();
        let d = run_b(&md, "version = 4\n", &["Cargo.lock"]);
        assert!(
            d.iter().any(|x| x
                .message
                .contains("dependency on the composition root root")),
            "{d:#?}"
        );
    }

    #[test]
    fn rules_one_and_two() {
        let md = synth(
            &["app"],
            &[
                (
                    "app",
                    "0.0.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("nat", "normal"), ("lnk", "normal")],
                ),
                (
                    "nat",
                    "1.0.0",
                    Some("registry+r"),
                    Some("MIT"),
                    true,
                    None,
                    &[("cc", "build")],
                ),
                (
                    "lnk",
                    "1.0.0",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    Some("foo"),
                    &[],
                ),
                (
                    "cc",
                    "1.2.0",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[],
                ),
            ],
            &[],
        );
        let d = run_b(&md, "version = 4\n", &["Cargo.lock"]);
        assert!(
            d.iter().any(|x| x
                .message
                .contains("nat 1.0.0 has a build script and no entry")),
            "{d:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("nat 1.0.0 has a build dependency on cc")),
            "{d:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("lnk 1.0.0 declares links")),
            "{d:#?}"
        );
        // A host-only entry used in the checked graph.
        let md = synth(
            &["app"],
            &[
                (
                    "app",
                    "0.0.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("tsc", "normal")],
                ),
                (
                    "tsc",
                    "0.1.0",
                    Some("registry+r"),
                    Some("MIT"),
                    true,
                    Some("tsc"),
                    &[],
                ),
            ],
            &[],
        );
        let d = run_b(&md, "version = 4\n", &["Cargo.lock"]);
        assert!(
            d.iter()
                .any(|x| x.message.contains("is in the checked graph")),
            "{d:#?}"
        );
    }

    fn run_lic(md: &Metadata, lock: &str, fuzz: Option<(&Metadata, &str)>) -> Vec<Diag> {
        let cfg = config();
        let mut lockfiles = vec![("Cargo.lock".to_string(), lock.to_string())];
        if let Some((_, l)) = fuzz {
            lockfiles.push(("fuzz/Cargo.lock".to_string(), l.to_string()));
        }
        licences(&DepInputs {
            targets: &[],
            full: md,
            fuzz: fuzz.map(|f| f.0),
            lockfiles: &lockfiles,
            found_lockfiles: &[],
            config: &cfg,
        })
    }

    #[test]
    fn seeded_disallowed_and_licence_file_only() {
        let md = Metadata::from_json(&fixture("licences.json")).unwrap();
        let lock = fixture("licences.lock.txt");
        let d = run_lic(&md, &lock, None);
        assert!(d.iter().any(|x| x.message.contains("gplcrate 1.0.0") && x.message.contains("GPL-3.0-only")), "{d:#?}");
        assert!(
            d.iter().any(|x| x.message.contains("filecrate 1.0.0")
                && x.message.contains("only a license-file")),
            "{d:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("nolicence 1.0.0") && x.message.contains("no licence")),
            "{d:#?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("ghost 9.9.9")
                && x.message.contains("not in cargo metadata")),
            "{d:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("fuzzonly 1.0.0") && x.message.contains("NCSA")),
            "{d:#?}"
        );
        assert!(!d.iter().any(|x| x.message.contains("okcrate")), "{d:#?}");
        // NCSA is allowed in fuzz/Cargo.lock only.
        let fz = synth(
            &["fz"],
            &[
                ("fz", "0.0.0", None, Some("Apache-2.0"), false, None, &[]),
                (
                    "libfuzzer-sys",
                    "0.4.10",
                    Some("registry+r"),
                    Some("(MIT OR Apache-2.0) AND NCSA"),
                    true,
                    None,
                    &[],
                ),
            ],
            &[],
        );
        let flock = "version = 4\n[[package]]\nname = \"fz\"\nversion = \"0.0.0\"\n[[package]]\nname = \"libfuzzer-sys\"\nversion = \"0.4.10\"\nsource = \"registry+r\"\n";
        let d = run_lic(
            &synth(
                &["a"],
                &[("a", "0.0.0", None, Some("MIT"), false, None, &[])],
                &[],
            ),
            "version = 4\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n",
            Some((&fz, flock)),
        );
        assert!(d.is_empty(), "{d:#?}");
    }

    #[test]
    fn osdeps_direct_and_transitive() {
        let md = synth(
            &["moirai-os", "moirai-files", "moirai-diff", "app"],
            &[
                (
                    "moirai-os",
                    "0.0.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("windows-sys", "normal")],
                ),
                (
                    "moirai-files",
                    "0.0.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("libc", "dev")],
                ),
                (
                    "moirai-diff",
                    "0.0.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("rustix", "normal")],
                ),
                (
                    "app",
                    "0.0.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("tp", "normal"), ("tp2", "normal")],
                ),
                (
                    "tp",
                    "1.0.0",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[("windows-sys", "normal")],
                ),
                (
                    "windows-sys",
                    "0.61.2",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[("windows-link", "normal")],
                ),
                (
                    "libc",
                    "0.2.0",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[],
                ),
                (
                    "tp2",
                    "1.0.0",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[("winapi", "normal")],
                ),
                (
                    "winapi",
                    "0.3.9",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[],
                ),
                (
                    "windows-link",
                    "0.2.1",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[],
                ),
                (
                    "rustix",
                    "1.0.0",
                    Some("registry+r"),
                    Some("MIT"),
                    false,
                    None,
                    &[],
                ),
            ],
            &[],
        );
        let cfg = config();
        let targets = vec![("t".to_string(), md.clone())];
        let d = osdeps(&DepInputs {
            targets: &targets,
            full: &md,
            fuzz: None,
            lockfiles: &[],
            found_lockfiles: &[],
            config: &cfg,
        });
        assert!(
            d.iter().any(|x| x
                .message
                .contains("moirai-files declares a direct dev dependency on libc")),
            "{d:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.message.contains("tp 1.0.0 -> windows-sys")),
            "{d:#?}"
        );
        assert!(
            !d.iter().any(|x| x.message.contains("moirai-os declares")),
            "{d:#?}"
        );
        // The other OS-binding crates count too; an edge inside the binding family needs no entry.
        assert!(
            d.iter()
                .any(|x| x.message.contains("moirai-diff declares a direct")
                    && x.message.contains("on rustix")),
            "{d:#?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("tp2 1.0.0 -> winapi")),
            "{d:#?}"
        );
        assert!(
            !d.iter().any(|x| x.message.contains("-> windows-link")),
            "{d:#?}"
        );
    }

    #[test]
    fn forbidden_names() {
        assert!(forbidden("gix").is_some());
        assert!(forbidden("gix-features").is_some());
        assert!(forbidden("Git2").is_some());
        assert!(forbidden("lmdb_rkv_sys").is_some());
        assert!(forbidden("zstd-sys").is_some());
        assert!(forbidden("github").is_none());
        assert!(forbidden("serde").is_none());
        assert!(forbidden("sledgehammer").is_none());
    }
}
