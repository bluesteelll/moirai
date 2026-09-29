//! Repository path patterns, as docs/m0/authors.md §3 defines them ("Matching"): paths are repository-relative with
//! `/`; `dir/**` is everything under `dir/`; `*` matches within one path component; `{a,b}` lists alternatives; a
//! module path ending in `src/<m>` stands for both `src/<m>.rs` and `src/<m>/**`. The most specific pattern is the one
//! with the longest literal prefix, counted in path components; a full file name is more specific than any pattern
//! that contains it.

/// One compiled pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    pub raw: String,
    /// The brace-expanded, module-expanded alternatives, each split into components.
    alts: Vec<Vec<String>>,
    /// Leading components with no `*` or `{`.
    pub literal_components: usize,
    /// No wildcard, no alternatives, not a module path and not `dir/**`.
    pub full_file: bool,
}

impl Pattern {
    pub fn new(raw: &str) -> Pattern {
        let raw = raw
            .trim()
            .trim_start_matches("./")
            .trim_start_matches('/')
            .to_string();
        let comps: Vec<&str> = raw.split('/').collect();
        let literal_components = comps
            .iter()
            .take_while(|c| !c.contains('*') && !c.contains('{'))
            .count();
        let mut alts = Vec::new();
        let mut module = false;
        for e in expand_braces(&raw) {
            let parts: Vec<String> = e.split('/').map(str::to_string).collect();
            let n = parts.len();
            let is_module = n >= 2
                && parts[n - 2] == "src"
                && !parts[n - 1].contains('.')
                && !parts[n - 1].contains('*')
                && !parts[n - 1].is_empty();
            if is_module {
                module = true;
                let mut f = parts.clone();
                f[n - 1] = format!("{}.rs", parts[n - 1]);
                alts.push(f);
                let mut d = parts;
                d.push("**".to_string());
                alts.push(d);
            } else {
                alts.push(parts);
            }
        }
        let full_file = !raw.contains('*') && !raw.contains('{') && !module;
        Pattern {
            raw,
            alts,
            literal_components,
            full_file,
        }
    }

    /// Specificity: longer literal prefix first, then full file names over patterns.
    pub fn specificity(&self) -> (usize, bool) {
        (self.literal_components, self.full_file)
    }

    pub fn matches(&self, path: &str) -> bool {
        let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        self.alts.iter().any(|a| match_comps(a, &comps))
    }

    /// The concrete glob forms (braces and module paths expanded), e.g. `src/bug.rs` and `src/bug/**`.
    pub fn globs(&self) -> Vec<String> {
        self.alts.iter().map(|a| a.join("/")).collect()
    }

    /// True when every path under the directory `dir` matches (used to reduce whole crates).
    pub fn covers_dir(&self, dir: &str) -> bool {
        self.matches(&format!("{}/{}", dir.trim_end_matches('/'), "\u{0}probe"))
            && self
                .alts
                .iter()
                .any(|a| a.last().map(String::as_str) == Some("**"))
    }
}

fn match_comps(pat: &[String], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((p, rest)) if p == "**" => {
            // `dir/**` needs at least one component below `dir`; `**` elsewhere matches zero or more.
            let min = usize::from(rest.is_empty());
            (min..=path.len()).any(|k| match_comps(rest, &path[k..]))
        }
        Some((p, rest)) => match path.split_first() {
            Some((c, prest)) => {
                glob_component(p.as_bytes(), c.as_bytes()) && match_comps(rest, prest)
            }
            None => false,
        },
    }
}

/// `*` matches any run of characters within one component.
fn glob_component(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|k| glob_component(rest, &s[k..])),
        Some((c, rest)) => s.first() == Some(c) && glob_component(rest, &s[1..]),
    }
}

/// Expands every `{a,b,...}` group (groups do not nest in authors.md).
pub fn expand_braces(s: &str) -> Vec<String> {
    let Some(open) = s.find('{') else {
        return vec![s.to_string()];
    };
    let Some(close_rel) = s[open..].find('}') else {
        return vec![s.to_string()];
    };
    let close = open + close_rel;
    let (pre, inner, post) = (&s[..open], &s[open + 1..close], &s[close + 1..]);
    let mut out = Vec::new();
    for alt in inner.split(',') {
        for tail in expand_braces(post) {
            out.push(format!("{pre}{alt}{tail}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_rules() {
        assert!(Pattern::new("xtask/**").matches("xtask/src/main.rs"));
        assert!(!Pattern::new("xtask/**").matches("xtask"));
        assert!(Pattern::new("Cargo.toml").matches("Cargo.toml"));
        assert!(!Pattern::new("Cargo.toml").matches("xtask/Cargo.toml"));
        let m = Pattern::new("xtask/src/ucd");
        assert!(m.matches("xtask/src/ucd.rs"));
        assert!(m.matches("xtask/src/ucd/tables.rs"));
        assert!(!m.matches("xtask/src/ucdx.rs"));
        let f = Pattern::new("crates/moirai-files/src/{path,fold,anchor}");
        assert!(f.matches("crates/moirai-files/src/fold.rs"));
        assert!(f.matches("crates/moirai-files/src/anchor/resolve.rs"));
        assert!(!f.matches("crates/moirai-files/src/lib.rs"));
        let z = Pattern::new("fuzz/fuzz_targets/{anchor,path,spec}_*.rs");
        assert!(z.matches("fuzz/fuzz_targets/anchor_selector.rs"));
        assert!(!z.matches("fuzz/fuzz_targets/scan_rust.rs"));
        assert!(Pattern::new("docs/**/x.md").matches("docs/x.md"));
        assert!(Pattern::new("docs/**/x.md").matches("docs/a/b/x.md"));
    }

    #[test]
    fn specificity_rules() {
        let files = Pattern::new("crates/moirai-files/**");
        let module = Pattern::new("crates/moirai-files/src/{scan,ignore,sketch}");
        assert!(module.specificity() > files.specificity());
        let fuzz = Pattern::new("fuzz/**");
        let toml = Pattern::new("fuzz/Cargo.toml");
        assert!(toml.specificity() > fuzz.specificity());
        assert!(toml.full_file);
        assert!(!module.full_file);
        let targets = Pattern::new("fuzz/fuzz_targets/{anchor,path,spec}_*.rs");
        assert_eq!(targets.literal_components, 2);
        assert!(Pattern::new("crates/moirai-model/**").covers_dir("crates/moirai-model"));
        assert!(
            !Pattern::new("crates/moirai-toylog/src/{bug,bugs}").covers_dir("crates/moirai-toylog")
        );
    }
}
