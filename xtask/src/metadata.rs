//! The parts of `cargo metadata --format-version 1` that the lints read ([90 §11.2]; docs/m0/PLAN.md WP-02).
//!
//! The lints are pure functions over this model, so their seeded violations are synthetic JSON files under
//! `xtask/tests/fixtures/`, never resolved or downloaded.

use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

impl DepKind {
    fn from_json(v: &Value) -> DepKind {
        match v.as_str() {
            Some("dev") => DepKind::Dev,
            Some("build") => DepKind::Build,
            _ => DepKind::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DepKind::Normal => "normal",
            DepKind::Dev => "dev",
            DepKind::Build => "build",
        }
    }
}

/// A declared dependency of a package (`packages[].dependencies[]`).
#[derive(Clone, Debug)]
pub struct Dependency {
    /// The package name of the dependency.
    pub name: String,
    pub kind: DepKind,
    /// The key the dependency is declared under when it is renamed (`key = { package = "name" }`).
    pub rename: Option<String>,
    /// The features the declaration enables.
    pub features: Vec<String>,
}

impl Dependency {
    /// The name the declaring package uses for it (in `<key>/<feature>` feature references).
    pub fn key(&self) -> &str {
        self.rename.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug)]
pub struct Target {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: String,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub manifest_path: String,
    pub links: Option<String>,
    pub targets: Vec<Target>,
    pub dependencies: Vec<Dependency>,
    /// The package's own `[features]` table: feature name to what it enables.
    pub features: BTreeMap<String, Vec<String>>,
    /// `None`: publishable anywhere; `Some(empty)`: `publish = false`.
    pub publish: Option<Vec<String>>,
}

impl Package {
    pub fn has_build_script(&self) -> bool {
        self.targets
            .iter()
            .any(|t| t.kind.iter().any(|k| k == "custom-build"))
    }

    /// The directory holding the package's `Cargo.toml`, with `/` separators.
    pub fn dir(&self) -> String {
        let p = self.manifest_path.replace('\\', "/");
        match p.rfind('/') {
            Some(i) => p[..i].to_string(),
            None => String::new(),
        }
    }
}

/// One edge of the resolved graph.
#[derive(Clone, Debug)]
pub struct NodeDep {
    pub pkg: String,
    pub kinds: Vec<DepKind>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub deps: Vec<NodeDep>,
    pub features: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub packages: Vec<Package>,
    pub workspace_members: Vec<String>,
    pub workspace_root: String,
    pub nodes: Vec<Node>,
    by_id: HashMap<String, usize>,
    node_by_id: HashMap<String, usize>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

/// A JSON array of strings (anything else reads as empty).
fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

impl Metadata {
    /// Reads `cargo metadata --format-version 1` output.
    pub fn from_json(text: &str) -> Result<Metadata, String> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("cargo metadata JSON: {e}"))?;
        Self::from_value(&v)
    }

    pub fn from_value(v: &Value) -> Result<Metadata, String> {
        let mut md = Metadata {
            workspace_root: s(v, "workspace_root")
                .unwrap_or_default()
                .replace('\\', "/"),
            ..Metadata::default()
        };
        let pkgs = v
            .get("packages")
            .and_then(Value::as_array)
            .ok_or("cargo metadata: no packages array")?;
        for p in pkgs {
            let id = s(p, "id").ok_or("cargo metadata: package without id")?;
            let targets = p
                .get("targets")
                .and_then(Value::as_array)
                .map(|ts| {
                    ts.iter()
                        .map(|t| Target {
                            name: s(t, "name").unwrap_or_default(),
                            kind: t
                                .get("kind")
                                .and_then(Value::as_array)
                                .map(|k| {
                                    k.iter()
                                        .filter_map(|x| x.as_str().map(str::to_string))
                                        .collect()
                                })
                                .unwrap_or_default(),
                            src_path: s(t, "src_path").unwrap_or_default().replace('\\', "/"),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let dependencies = p
                .get("dependencies")
                .and_then(Value::as_array)
                .map(|ds| {
                    ds.iter()
                        .map(|d| Dependency {
                            name: s(d, "name").unwrap_or_default(),
                            kind: DepKind::from_json(d.get("kind").unwrap_or(&Value::Null)),
                            rename: s(d, "rename"),
                            features: strings(d.get("features")),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let features = p
                .get("features")
                .and_then(Value::as_object)
                .map(|f| {
                    f.iter()
                        .map(|(k, v)| (k.clone(), strings(Some(v))))
                        .collect()
                })
                .unwrap_or_default();
            let publish = p
                .get("publish")
                .filter(|x| x.is_array())
                .map(|x| strings(Some(x)));
            md.by_id.insert(id.clone(), md.packages.len());
            md.packages.push(Package {
                id,
                name: s(p, "name").unwrap_or_default(),
                version: s(p, "version").unwrap_or_default(),
                source: s(p, "source"),
                license: s(p, "license"),
                license_file: s(p, "license_file"),
                manifest_path: s(p, "manifest_path").unwrap_or_default().replace('\\', "/"),
                links: s(p, "links"),
                targets,
                dependencies,
                features,
                publish,
            });
        }
        md.workspace_members = strings(v.get("workspace_members"));
        if let Some(nodes) = v
            .get("resolve")
            .and_then(|r| r.get("nodes"))
            .and_then(Value::as_array)
        {
            for n in nodes {
                let id = s(n, "id").unwrap_or_default();
                let deps = n
                    .get("deps")
                    .and_then(Value::as_array)
                    .map(|ds| {
                        ds.iter()
                            .map(|d| NodeDep {
                                pkg: s(d, "pkg").unwrap_or_default(),
                                kinds: d
                                    .get("dep_kinds")
                                    .and_then(Value::as_array)
                                    .map(|ks| {
                                        ks.iter()
                                            .map(|k| {
                                                DepKind::from_json(
                                                    k.get("kind").unwrap_or(&Value::Null),
                                                )
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_else(|| vec![DepKind::Normal]),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let features = strings(n.get("features"));
                md.node_by_id.insert(id.clone(), md.nodes.len());
                md.nodes.push(Node { deps, features });
            }
        }
        Ok(md)
    }

    pub fn package(&self, id: &str) -> Option<&Package> {
        self.by_id.get(id).map(|&i| &self.packages[i])
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.node_by_id.get(id).map(|&i| &self.nodes[i])
    }

    pub fn is_member(&self, id: &str) -> bool {
        self.workspace_members.iter().any(|m| m == id)
    }

    pub fn members(&self) -> impl Iterator<Item = &Package> {
        self.workspace_members
            .iter()
            .filter_map(|id| self.package(id))
    }

    pub fn member_named(&self, name: &str) -> Option<&Package> {
        self.members().find(|p| p.name == name)
    }

    /// The resolved features of a package in this graph (empty when it is not resolved).
    pub fn features(&self, id: &str) -> &[String] {
        self.node(id).map(|n| n.features.as_slice()).unwrap_or(&[])
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// `(name, version, source, license, build_script, links, deps as (name, kind))`.
    pub type SynthPkg<'a> = (
        &'a str,
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        bool,
        Option<&'a str>,
        &'a [(&'a str, &'a str)],
    );

    /// Builds synthetic `cargo metadata` JSON for tests.
    pub fn synth(
        members: &[&str],
        pkgs: &[SynthPkg<'_>],
        features: &[(&str, &[&str])],
    ) -> Metadata {
        let id = |n: &str, v: &str| format!("{n} {v}");
        let find_ver = |n: &str| {
            pkgs.iter()
                .find(|p| p.0 == n)
                .map(|p| p.1)
                .unwrap_or("0.0.0")
        };
        let packages: Vec<Value> = pkgs
            .iter()
            .map(|(n, v, src, lic, bs, links, deps)| {
                let mut targets = vec![json!({"name": n, "kind": ["lib"], "src_path": format!("/ws/{n}/src/lib.rs")})];
                if *bs {
                    targets.push(json!({"name": "build-script-build", "kind": ["custom-build"], "src_path": format!("/ws/{n}/build.rs")}));
                }
                json!({
                    "id": id(n, v), "name": n, "version": v, "source": src, "license": lic, "license_file": null,
                    "manifest_path": format!("/ws/{n}/Cargo.toml"), "links": links, "targets": targets,
                    "publish": if src.is_none() { json!([]) } else { Value::Null },
                    "dependencies": deps.iter().map(|(d, k)| json!({"name": d, "kind": if *k == "normal" { Value::Null } else { json!(k) }, "optional": false, "target": null})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let nodes: Vec<Value> = pkgs
            .iter()
            .map(|(n, v, _, _, _, _, deps)| {
                let feats = features.iter().find(|f| f.0 == *n).map(|f| f.1.to_vec()).unwrap_or_default();
                json!({
                    "id": id(n, v),
                    "features": feats,
                    "deps": deps.iter().map(|(d, k)| json!({"name": d, "pkg": id(d, find_ver(d)), "dep_kinds": [{"kind": if *k == "normal" { Value::Null } else { json!(k) }, "target": null}]})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let v = json!({
            "packages": packages,
            "workspace_members": members.iter().map(|m| id(m, find_ver(m))).collect::<Vec<_>>(),
            "workspace_root": "/ws",
            "resolve": {"nodes": nodes, "root": null},
        });
        Metadata::from_value(&v).unwrap()
    }

    #[test]
    fn reads_synthetic_metadata() {
        let md = synth(
            &["a"],
            &[
                (
                    "a",
                    "0.1.0",
                    None,
                    Some("MIT"),
                    false,
                    None,
                    &[("b", "normal"), ("c", "build")],
                ),
                (
                    "b",
                    "1.0.0",
                    Some("registry+x"),
                    Some("MIT"),
                    true,
                    Some("b"),
                    &[],
                ),
                ("c", "2.0.0", Some("registry+x"), None, false, None, &[]),
            ],
            &[("b", &["std"])],
        );
        assert_eq!(md.members().count(), 1);
        let b = md.package("b 1.0.0").unwrap();
        assert!(b.has_build_script());
        assert_eq!(b.links.as_deref(), Some("b"));
        assert_eq!(md.features("b 1.0.0"), &["std".to_string()]);
        let a = md.node("a 0.1.0").unwrap();
        assert_eq!(a.deps[1].kinds, vec![DepKind::Build]);
        assert_eq!(md.member_named("a").unwrap().dir(), "/ws/a");
        assert_eq!(md.member_named("a").unwrap().publish, Some(vec![]));
    }

    #[test]
    fn reads_declared_features_and_renames() {
        let md = Metadata::from_json(
            r#"{"packages": [{"id": "r 0.0.0", "name": "r", "version": "0.0.0", "manifest_path": "C:\\ws\\r\\Cargo.toml",
                "publish": null, "features": {"probe": ["os?/test-host", "dep:os"], "default": []},
                "dependencies": [{"name": "moirai-os", "rename": "os", "kind": null, "features": ["test-host"]},
                                 {"name": "x", "kind": "dev"}]}],
                "workspace_members": ["r 0.0.0"], "workspace_root": "C:\\ws"}"#,
        )
        .unwrap();
        let r = md.member_named("r").unwrap();
        assert_eq!(r.publish, None);
        assert_eq!(r.dir(), "C:/ws/r");
        assert_eq!(r.features["probe"], vec!["os?/test-host", "dep:os"]);
        assert!(r.features["default"].is_empty());
        assert_eq!(r.dependencies[0].key(), "os");
        assert_eq!(r.dependencies[0].features, vec!["test-host"]);
        assert_eq!(r.dependencies[1].key(), "x");
        assert_eq!(r.dependencies[1].kind, DepKind::Dev);
        assert!(r.dependencies[1].features.is_empty());
        assert!(md.nodes.is_empty());
    }
}
