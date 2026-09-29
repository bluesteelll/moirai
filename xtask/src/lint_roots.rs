//! The composition-root lint (docs/m0/PLAN.md §2.1 "Composition roots", §6.2 R17; [90 §11.1]; [OS/README §2.4];
//! `xtask/roots.toml`):
//! - a root holds binaries only (`bin_paths`), each target file within the line cap, and depends only on its
//!   `allowed_deps`;
//! - only a root depends on `moirai-os`: any other workspace member with a normal, build or dev dependency on it is
//!   refused ([OS/README §2.4]);
//! - a product root (`product = true`) never enables `moirai-os`'s `test-host` feature, in its dependency
//!   declaration or through its own `[features]`, and never installs a `#[global_allocator]` (`CountingAlloc` is for
//!   the probe roots, [OS/README §2.4], [OS/mem §6]).
//!
//! Live for `moirai-probes-bin`; dormant for `moirai` (`present = false`) until M8 creates it, when a root listed as
//! absent but found in the workspace is refused. A fixture workspace self-tests both (`xtask/tests/fixtures/roots/`).
//!
//! The lint also checks the workspace lint tables (docs/m0/authors.md §6 item 8): every member but `moirai-os`
//! inherits `[workspace.lints]` (`[lints] workspace = true` and nothing else), the workspace forbids `unsafe_code`,
//! and `moirai-os` denies `unsafe_code`, `unsafe_op_in_unsafe_fn` and `clippy::undocumented_unsafe_blocks`.

use crate::config::Roots;
use crate::diag::Diag;
use crate::metadata::{Metadata, Package};
use crate::paths::Pattern;
use crate::rustscan::{self, Tok};
use crate::toml::{Table, Value};
use std::path::Path;

/// A root crate's files: `(path relative to the crate directory, contents)`, `target/` and hidden entries left out.
pub type FileLister<'a> = dyn Fn(&str) -> Result<Vec<(String, Vec<u8>)>, String> + 'a;

/// The OS crate only a root may depend on, and its feature a product root may not enable ([OS/README §2.4]).
const OS_CRATE: &str = "moirai-os";
const TEST_HOST: &str = "test-host";

fn line_count(text: &[u8]) -> usize {
    text.split(|&c| c == b'\n').count() - usize::from(text.ends_with(b"\n"))
}

/// The line of the first `#[global_allocator]` attribute in a Rust source file.
fn global_allocator(src: &str) -> Option<u32> {
    rustscan::lex(src).windows(3).find_map(|w| {
        (matches!(w[0].tok, Tok::Punct(b'#'))
            && matches!(w[1].tok, Tok::Punct(b'['))
            && matches!(&w[2].tok, Tok::Ident(i) if i == "global_allocator"))
        .then_some(w[0].line)
    })
}

/// The product-root rule of [OS/README §2.4] over the root's manifest: `moirai-os`'s `test-host` feature is enabled
/// neither in the dependency declaration nor through one of the root's own features (`moirai-os/test-host`,
/// `moirai-os?/test-host`, or the same through a renamed key).
fn product_root_features(name: &str, p: &Package) -> Vec<Diag> {
    let mut out = Vec::new();
    let os_deps: Vec<_> = p
        .dependencies
        .iter()
        .filter(|d| d.name == OS_CRATE)
        .collect();
    for d in &os_deps {
        if d.features.iter().any(|f| f == TEST_HOST) {
            out.push(Diag::krate(
                "roots",
                name,
                format!(
                    "the product root {name} enables {OS_CRATE}'s {TEST_HOST} feature in its {} dependency ([OS/README §2.4])",
                    d.kind.label()
                ),
            ));
        }
    }
    for (feat, enables) in &p.features {
        for e in enables {
            let hit = os_deps.iter().any(|d| {
                e.strip_prefix(d.key())
                    .map(|r| r.strip_prefix('?').unwrap_or(r))
                    .and_then(|r| r.strip_prefix('/'))
                    == Some(TEST_HOST)
            });
            if hit {
                out.push(Diag::krate(
                    "roots",
                    name,
                    format!(
                        "the product root {name}'s feature '{feat}' enables {e} ([OS/README §2.4])"
                    ),
                ));
            }
        }
    }
    out
}

pub fn check_roots(roots: &Roots, md: &Metadata, list_files: &FileLister<'_>) -> Vec<Diag> {
    let mut out = Vec::new();
    // Only a root depends on moirai-os.
    for m in md.members() {
        if m.name == OS_CRATE || roots.contains(&m.name) {
            continue;
        }
        for d in m.dependencies.iter().filter(|d| d.name == OS_CRATE) {
            out.push(Diag::krate(
                "roots",
                &m.name,
                format!(
                    "{} has a {} dependency on {OS_CRATE}: only a composition root of xtask/roots.toml may depend on it (PLAN §2.1, [OS/README §2.4])",
                    m.name,
                    d.kind.label()
                ),
            ));
        }
    }
    for r in &roots.roots {
        let member = md.member_named(&r.name);
        if !r.present {
            if member.is_some() {
                out.push(Diag::krate(
                    "roots",
                    &r.name,
                    format!("{} is a workspace member but xtask/roots.toml says present = false: set present = true", r.name),
                ));
            }
            continue;
        }
        let Some(p) = member else {
            out.push(Diag::krate(
                "roots",
                &r.name,
                format!(
                    "xtask/roots.toml lists {} as present, but it is not a workspace member",
                    r.name
                ),
            ));
            continue;
        };
        let dir = p.dir();
        let pats: Vec<Pattern> = r.bin_paths.iter().map(|b| Pattern::new(b)).collect();
        let mut target_files = Vec::new();
        for t in &p.targets {
            let rel = t
                .src_path
                .strip_prefix(&format!("{dir}/"))
                .unwrap_or(&t.src_path)
                .to_string();
            if t.kind != ["bin"] {
                out.push(Diag::krate(
                    "roots",
                    &r.name,
                    format!(
                        "target '{}' ({}) of {}: a composition root holds binaries only",
                        t.name,
                        t.kind.join(", "),
                        r.name
                    ),
                ));
            } else if !pats.iter().any(|pt| pt.matches(&rel)) {
                out.push(Diag::krate(
                    "roots",
                    &r.name,
                    format!(
                        "binary '{}' at {rel} is outside the allowed paths [{}]",
                        t.name,
                        r.bin_paths.join(", ")
                    ),
                ));
            }
            target_files.push(rel);
        }
        if r.product {
            out.extend(product_root_features(&r.name, p));
        }
        match list_files(&dir) {
            Err(e) => out.push(Diag::krate(
                "roots",
                &r.name,
                format!("cannot list {}: {e}", r.name),
            )),
            Ok(files) => {
                for (rel, text) in files {
                    let repo_path = format!("{}/{rel}", rel_to_ws(md, &dir));
                    if rel == "Cargo.toml" {
                        continue;
                    }
                    if r.product
                        && rel.ends_with(".rs")
                        && let Some(line) = global_allocator(&String::from_utf8_lossy(&text))
                    {
                        out.push(Diag {
                            line: Some(line),
                            ..Diag::path(
                                "roots",
                                &repo_path,
                                format!(
                                    "the product root {} installs a #[global_allocator]: CountingAlloc is for the probe roots only ([OS/README §2.4], [OS/mem §6])",
                                    r.name
                                ),
                            )
                        });
                    }
                    let lines = line_count(&text);
                    if !target_files.contains(&rel) {
                        out.push(Diag::path(
                            "roots",
                            &repo_path,
                            format!(
                                "{}: a composition root holds only its binaries' files [{}]",
                                r.name,
                                r.bin_paths.join(", ")
                            ),
                        ));
                    } else if lines > r.max_lines {
                        out.push(Diag::path(
                            "roots",
                            &repo_path,
                            format!(
                                "{lines} lines, over the cap of {} for {}",
                                r.max_lines, r.name
                            ),
                        ));
                    }
                }
            }
        }
        for d in &p.dependencies {
            if !r.allowed_deps.contains(&d.name) {
                out.push(Diag::krate(
                    "roots",
                    &r.name,
                    format!(
                        "{} has a {} dependency on {}, outside its allowed [{}]",
                        r.name,
                        d.kind.label(),
                        d.name,
                        r.allowed_deps.join(", ")
                    ),
                ));
            }
        }
    }
    out
}

fn rel_to_ws(md: &Metadata, dir: &str) -> String {
    dir.strip_prefix(&format!("{}/", md.workspace_root))
        .unwrap_or(dir)
        .to_string()
}

/// Lists a crate directory's files with their contents (`target/` and hidden entries left out); a root holds a
/// few small files, so each is read whole.
pub fn list_crate_files(dir: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![(Path::new(dir).to_path_buf(), String::new())];
    while let Some((abs, rel)) = stack.pop() {
        let rd = std::fs::read_dir(&abs).map_err(|e| format!("{}: {e}", abs.display()))?;
        for e in rd {
            let e = e.map_err(|e| e.to_string())?;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || (rel.is_empty() && name == "target") {
                continue;
            }
            let r = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let ft = e.file_type().map_err(|e| e.to_string())?;
            if ft.is_dir() {
                stack.push((e.path(), r));
            } else {
                let text = std::fs::read(e.path()).map_err(|e| e.to_string())?;
                out.push((r, text));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The lint tables (authors.md §6 item 8). `manifests` maps a member name to its parsed `Cargo.toml`.
pub fn check_lint_tables(
    root_manifest: &Table,
    manifests: &[(String, String, Table)],
) -> Vec<Diag> {
    let mut out = Vec::new();
    let forbid = Value::Table(root_manifest.clone());
    if forbid
        .get_path(&["workspace", "lints", "rust", "unsafe_code"])
        .and_then(Value::as_str)
        != Some("forbid")
    {
        out.push(Diag::path(
            "lints",
            "Cargo.toml",
            "[workspace.lints.rust] must set unsafe_code = \"forbid\" (PLAN §2.1)",
        ));
    }
    for (name, path, t) in manifests {
        let lints = t.get("lints").and_then(Value::as_table);
        if name == "moirai-os" {
            let v = Value::Table(t.clone());
            let want = [
                (["lints", "rust", "unsafe_code"], "deny"),
                (["lints", "rust", "unsafe_op_in_unsafe_fn"], "deny"),
                (["lints", "clippy", "undocumented_unsafe_blocks"], "deny"),
            ];
            for (p, val) in want {
                if v.get_path(&p).and_then(Value::as_str) != Some(val) {
                    out.push(Diag::path(
                        "lints",
                        path,
                        format!(
                            "moirai-os must set {} = \"{val}\" (PLAN §2.1)",
                            p[1..].join(".")
                        ),
                    ));
                }
            }
            continue;
        }
        let ok = lints.is_some_and(|l| {
            l.len() == 1 && l.get("workspace").and_then(Value::as_bool) == Some(true)
        });
        if !ok {
            out.push(Diag::path(
                "lints",
                path,
                format!("{name} must inherit the workspace lint table: [lints] workspace = true and nothing else (authors.md §6 item 8)"),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture_dir(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/roots")
            .join(name)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Metadata for a fixture workspace whose members live under `xtask/tests/fixtures/roots/`.
    /// `(name, fixture directory, targets as (kind, path), dependencies as (spec, kind))`; a dependency spec is
    /// `[key=]name[[feature,..]]`, and a dependency of kind `feature` is one of the package's own features instead,
    /// written `feature=enabled,..`.
    type Member<'a> = (
        &'a str,
        &'a str,
        &'a [(&'a str, &'a str)],
        &'a [(&'a str, &'a str)],
    );

    fn dep_json(spec: &str, kind: &str) -> serde_json::Value {
        let (key, rest) = match spec.split_once('=') {
            Some((k, r)) => (Some(k), r),
            None => (None, spec),
        };
        let (name, feats) = match rest.split_once('[') {
            Some((n, f)) => (n, f.trim_end_matches(']').split(',').collect::<Vec<_>>()),
            None => (rest, Vec::new()),
        };
        let kind = if kind == "normal" {
            serde_json::Value::Null
        } else {
            json!(kind)
        };
        json!({"name": name, "rename": key, "features": feats, "kind": kind})
    }

    fn md(members: &[Member<'_>]) -> Metadata {
        let ws = fixture_dir("");
        let packages: Vec<_> = members
            .iter()
            .map(|(name, dir, targets, deps)| {
                let d = fixture_dir(dir);
                let features: serde_json::Map<String, serde_json::Value> = deps
                    .iter()
                    .filter(|(_, k)| *k == "feature")
                    .filter_map(|(s, _)| s.split_once('='))
                    .map(|(f, e)| (f.to_string(), json!(e.split(',').collect::<Vec<_>>())))
                    .collect();
                json!({
                    "id": format!("{name} 0.0.0"), "name": name, "version": "0.0.0", "source": null,
                    "manifest_path": format!("{d}/Cargo.toml"), "publish": [], "features": features,
                    "targets": targets.iter().map(|(k, p)| json!({"name": name, "kind": [k], "src_path": format!("{d}/{p}")})).collect::<Vec<_>>(),
                    "dependencies": deps.iter().filter(|(_, k)| *k != "feature").map(|(s, k)| dep_json(s, k)).collect::<Vec<_>>(),
                })
            })
            .collect();
        Metadata::from_value(&json!({
            "packages": packages,
            "workspace_members": members.iter().map(|m| format!("{} 0.0.0", m.0)).collect::<Vec<_>>(),
            "workspace_root": ws.trim_end_matches('/'),
        }))
        .unwrap()
    }

    fn roots(present_moirai: bool) -> Roots {
        let text =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("roots.toml"))
                .unwrap();
        let text = if present_moirai {
            text.replace("present = false", "present = true")
        } else {
            text
        };
        Roots::from_table(&crate::toml::parse(&text).unwrap()).unwrap()
    }

    #[test]
    fn live_probes_bin_root_passes_and_is_bounded() {
        let m = md(&[(
            "moirai-probes-bin",
            "probes-bin-ok",
            &[("bin", "src/bin/empty.rs"), ("bin", "src/bin/guard.rs")],
            &[("moirai-probes", "normal"), ("moirai-os", "normal")],
        )]);
        assert!(check_roots(&roots(false), &m, &list_crate_files).is_empty());
        let bad = md(&[(
            "moirai-probes-bin",
            "probes-bin-bad",
            &[
                ("bin", "src/bin/empty.rs"),
                ("lib", "src/lib.rs"),
                ("bin", "src/main.rs"),
            ],
            &[
                ("moirai-probes", "normal"),
                ("serde_json", "normal"),
                ("moirai-files", "dev"),
            ],
        )]);
        let d = check_roots(&roots(false), &bad, &list_crate_files);
        let msgs: Vec<&str> = d.iter().map(|x| x.message.as_str()).collect();
        assert!(
            msgs.iter().any(|m| m.contains("holds binaries only")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("outside the allowed paths")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("dependency on serde_json")),
            "{msgs:#?}"
        );
        assert!(
            msgs.iter()
                .any(|m| m.contains("dev dependency on moirai-files")),
            "{msgs:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.path.as_deref() == Some("probes-bin-bad/src/bin/helper/mod.rs")),
            "{d:#?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("over the cap of 200")),
            "{d:#?}"
        );
    }

    #[test]
    fn dormant_moirai_root() {
        // Absent and listed absent: nothing to check.
        let none = md(&[]);
        let d = check_roots(&roots(false), &none, &list_crate_files);
        assert!(
            d.iter()
                .all(|x| x.krate.as_deref() == Some("moirai-probes-bin")),
            "{d:#?}"
        );
        // Created but still listed absent: refused.
        let created = md(&[(
            "moirai",
            "moirai-ok",
            &[("bin", "src/main.rs")],
            &[("moirai-app", "normal"), ("moirai-os", "normal")],
        )]);
        let d = check_roots(&roots(false), &created, &list_crate_files);
        assert!(
            d.iter().any(|x| x.message.contains("present = false")),
            "{d:#?}"
        );
        // Switched on: the fixture that follows [90 §11.1] passes; the one that does not is refused.
        let d = check_roots(&roots(true), &created, &list_crate_files);
        assert!(
            d.iter().all(|x| x.krate.as_deref() != Some("moirai")),
            "{d:#?}"
        );
        let bad = md(&[(
            "moirai",
            "moirai-bad",
            &[("bin", "src/main.rs")],
            &[("moirai-app", "normal"), ("moirai-store", "normal")],
        )]);
        let d = check_roots(&roots(true), &bad, &list_crate_files);
        assert!(
            d.iter()
                .any(|x| x.message.contains("dependency on moirai-store")),
            "{d:#?}"
        );
        assert!(
            d.iter()
                .any(|x| x.path.as_deref() == Some("moirai-bad/src/cli.rs")),
            "{d:#?}"
        );
        assert!(
            d.iter().any(|x| x.message.contains("over the cap of 200")),
            "{d:#?}"
        );
    }

    #[test]
    fn only_roots_depend_on_moirai_os() {
        let m = md(&[
            (
                "moirai-probes-bin",
                "probes-bin-ok",
                &[("bin", "src/bin/empty.rs"), ("bin", "src/bin/guard.rs")],
                &[("moirai-os[test-host]", "normal")],
            ),
            ("moirai-os", "probes-bin-ok", &[], &[]),
            (
                "moirai-probes",
                "probes-bin-ok",
                &[],
                &[("moirai-vfs", "normal")],
            ),
        ]);
        assert!(check_roots(&roots(false), &m, &list_crate_files).is_empty());
        for kind in ["normal", "dev", "build"] {
            let m = md(&[
                (
                    "moirai-probes-bin",
                    "probes-bin-ok",
                    &[("bin", "src/bin/empty.rs"), ("bin", "src/bin/guard.rs")],
                    &[],
                ),
                (
                    "moirai-toylog",
                    "probes-bin-ok",
                    &[],
                    &[("os=moirai-os", kind)],
                ),
            ]);
            let d = check_roots(&roots(false), &m, &list_crate_files);
            assert_eq!(d.len(), 1, "{d:#?}");
            assert!(
                d[0].message.contains(&format!(
                    "moirai-toylog has a {kind} dependency on moirai-os"
                )),
                "{d:#?}"
            );
        }
    }

    #[test]
    fn product_root_never_enables_test_host_or_an_allocator() {
        // The probe root may enable test-host and install CountingAlloc; the product root may do neither. (The
        // absent probe root's own finding is left out.)
        let moirai_only = |d: Vec<Diag>| -> Vec<Diag> {
            d.into_iter()
                .filter(|x| x.krate.as_deref() != Some("moirai-probes-bin"))
                .collect()
        };
        let ok = md(&[(
            "moirai",
            "moirai-ok",
            &[("bin", "src/main.rs")],
            &[
                ("moirai-app", "normal"),
                ("moirai-os", "normal"),
                ("trace=moirai-app/trace", "feature"),
            ],
        )]);
        assert!(moirai_only(check_roots(&roots(true), &ok, &list_crate_files)).is_empty());
        let direct = md(&[(
            "moirai",
            "moirai-ok",
            &[("bin", "src/main.rs")],
            &[("moirai-app", "normal"), ("moirai-os[test-host]", "normal")],
        )]);
        let d = moirai_only(check_roots(&roots(true), &direct, &list_crate_files));
        assert_eq!(d.len(), 1, "{d:#?}");
        assert!(
            d[0].message
                .contains("enables moirai-os's test-host feature")
        );
        let forwarded = md(&[(
            "moirai",
            "moirai-ok",
            &[("bin", "src/main.rs")],
            &[
                ("moirai-app", "normal"),
                ("os=moirai-os", "normal"),
                ("probe=os?/test-host", "feature"),
                ("other=moirai-os/test-host", "feature"),
            ],
        )]);
        let d = moirai_only(check_roots(&roots(true), &forwarded, &list_crate_files));
        assert_eq!(d.len(), 1, "{d:#?}");
        assert!(
            d[0].message
                .contains("feature 'probe' enables os?/test-host")
        );
        // `#[global_allocator]` in the product root's files (the moirai-bad fixture's main.rs).
        let bad = md(&[(
            "moirai",
            "moirai-bad",
            &[("bin", "src/main.rs")],
            &[("moirai-app", "normal"), ("moirai-os", "normal")],
        )]);
        let d = check_roots(&roots(true), &bad, &list_crate_files);
        assert!(
            d.iter()
                .any(|x| x.path.as_deref() == Some("moirai-bad/src/main.rs")
                    && x.line == Some(5)
                    && x.message.contains("#[global_allocator]")),
            "{d:#?}"
        );
        assert_eq!(
            global_allocator("// #[global_allocator]\nconst S: &str = \"#[global_allocator]\";\n"),
            None
        );
    }

    #[test]
    fn lint_tables() {
        let t = |s: &str| crate::toml::parse(s).unwrap();
        let root = t("[workspace.lints.rust]\nunsafe_code = \"forbid\"\n");
        let good = vec![
            (
                "a".to_string(),
                "crates/a/Cargo.toml".to_string(),
                t("[lints]\nworkspace = true\n"),
            ),
            (
                "moirai-os".to_string(),
                "crates/moirai-os/Cargo.toml".to_string(),
                t(
                    "[lints.rust]\nunsafe_code = \"deny\"\nunsafe_op_in_unsafe_fn = \"deny\"\n[lints.clippy]\nundocumented_unsafe_blocks = \"deny\"\n",
                ),
            ),
        ];
        assert!(check_lint_tables(&root, &good).is_empty());
        let bad = vec![
            (
                "a".to_string(),
                "crates/a/Cargo.toml".to_string(),
                t("[lints.rust]\nunsafe_code = \"allow\"\n"),
            ),
            (
                "b".to_string(),
                "crates/b/Cargo.toml".to_string(),
                t("[package]\nname = \"b\"\n"),
            ),
            (
                "moirai-os".to_string(),
                "crates/moirai-os/Cargo.toml".to_string(),
                t("[lints]\nworkspace = true\n"),
            ),
        ];
        let d = check_lint_tables(&t("[workspace]\n"), &bad);
        assert_eq!(d.len(), 1 + 2 + 3, "{d:#?}");
    }
}
