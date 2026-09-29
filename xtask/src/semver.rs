//! Semantic versions and Cargo-style version requirements, for the `versions` fields of the allow-lists
//! (`xtask/native-allow.toml`, `xtask/osdeps-allow.toml`).
//!
//! A requirement is a comma-separated list of comparators, all of which must hold: `=`, `>`, `>=`, `<`, `<=`, `~`,
//! `^` or a bare version (caret), with partial versions and `*` wildcards, as Cargo reads them. A pre-release version
//! satisfies a comparator only when that comparator names the same `major.minor.patch` with a pre-release.

use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(str::to_string).collect()),
            None => (s, Vec::new()),
        };
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        if it.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    fn cmp_core(&self, o: &Version) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(o.major, o.minor, o.patch))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        self.cmp_core(o)
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let ord = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if ord != Ordering::Equal {
                            return ord;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
    Wildcard,
}

#[derive(Clone, Debug)]
struct Comparator {
    op: Op,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<String>,
}

/// A parsed version requirement.
#[derive(Clone, Debug)]
pub struct VersionReq {
    comparators: Vec<Comparator>,
}

impl VersionReq {
    pub fn parse(s: &str) -> Result<VersionReq, String> {
        let s = s.trim();
        if s.is_empty() || s == "*" {
            return Ok(VersionReq {
                comparators: Vec::new(),
            });
        }
        let mut comparators = Vec::new();
        for part in s.split(',') {
            comparators.push(
                parse_comparator(part.trim())
                    .ok_or_else(|| format!("invalid version requirement '{s}'"))?,
            );
        }
        Ok(VersionReq { comparators })
    }

    /// Whether some version satisfies both requirements. Each comparator admits one interval of versions, so the
    /// release versions both admit form an interval whose least element is the largest of the comparators' least
    /// release versions (or 0.0.0): testing those candidates decides the question for releases. A pre-release is
    /// found only when a comparator names it.
    pub fn intersects(&self, other: &VersionReq) -> bool {
        let mut candidates = vec![Version {
            major: 0,
            minor: 0,
            patch: 0,
            pre: Vec::new(),
        }];
        for c in self.comparators.iter().chain(&other.comparators) {
            candidates.extend(c.least_releases());
            if !c.pre.is_empty() {
                candidates.push(c.lower());
            }
        }
        candidates
            .iter()
            .any(|v| self.matches(v) && other.matches(v))
    }

    pub fn matches(&self, v: &Version) -> bool {
        if !self.comparators.iter().all(|c| c.matches(v)) {
            return false;
        }
        if v.pre.is_empty() {
            return true;
        }
        // A pre-release matches only through a comparator that names the same version with a pre-release.
        self.comparators.iter().any(|c| {
            !c.pre.is_empty()
                && c.major == v.major
                && c.minor == Some(v.minor)
                && c.patch == Some(v.patch)
        })
    }
}

fn parse_comparator(s: &str) -> Option<Comparator> {
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (Op::GreaterEq, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (Op::LessEq, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (Op::Greater, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (Op::Less, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (Op::Exact, r)
    } else if let Some(r) = s.strip_prefix('~') {
        (Op::Tilde, r)
    } else if let Some(r) = s.strip_prefix('^') {
        (Op::Caret, r)
    } else {
        (Op::Caret, s)
    };
    let rest = rest.trim();
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_string).collect()),
        None => (rest, Vec::new()),
    };
    let mut parts = core.split('.');
    let wild = |p: &str| p == "*" || p == "x" || p == "X";
    let major_s = parts.next()?;
    if wild(major_s) {
        return Some(Comparator {
            op: Op::Wildcard,
            major: 0,
            minor: None,
            patch: None,
            pre,
        });
    }
    let major = major_s.parse().ok()?;
    let mut minor = None;
    let mut patch = None;
    let mut op = op;
    if let Some(m) = parts.next() {
        if wild(m) {
            op = if op == Op::Caret { Op::Wildcard } else { op };
        } else {
            minor = Some(m.parse().ok()?);
            if let Some(p) = parts.next() {
                if wild(p) {
                    op = if op == Op::Caret { Op::Wildcard } else { op };
                } else {
                    patch = Some(p.parse().ok()?);
                }
            }
        }
    }
    if parts.next().is_some() {
        return None;
    }
    Some(Comparator {
        op,
        major,
        minor,
        patch,
        pre,
    })
}

impl Comparator {
    fn lower(&self) -> Version {
        Version {
            major: self.major,
            minor: self.minor.unwrap_or(0),
            patch: self.patch.unwrap_or(0),
            pre: self.pre.clone(),
        }
    }

    /// Release versions among which is the least release this comparator admits (when it has a lower bound): its
    /// own version, and the next patch, minor and major after it (for `>`).
    fn least_releases(&self) -> [Version; 4] {
        let (ma, mi, pa) = (self.major, self.minor.unwrap_or(0), self.patch.unwrap_or(0));
        let v = |major, minor, patch| Version {
            major,
            minor,
            patch,
            pre: Vec::new(),
        };
        [
            v(ma, mi, pa),
            v(ma, mi, pa.saturating_add(1)),
            v(ma, mi.saturating_add(1), 0),
            v(ma.saturating_add(1), 0, 0),
        ]
    }

    fn matches(&self, v: &Version) -> bool {
        let prefix = |v: &Version| {
            v.major == self.major
                && self.minor.is_none_or(|m| v.minor == m)
                && self.patch.is_none_or(|p| v.patch == p)
        };
        match self.op {
            Op::Wildcard => {
                if self.major == 0
                    && self.minor.is_none()
                    && self.patch.is_none()
                    && self.pre.is_empty()
                {
                    // `*`
                    return true;
                }
                prefix(v)
            }
            Op::Exact => {
                if self.patch.is_some() {
                    v.cmp(&self.lower()) == Ordering::Equal
                } else {
                    prefix(v)
                }
            }
            Op::Greater => {
                if self.patch.is_some() {
                    *v > self.lower()
                } else {
                    !prefix(v) && v.cmp_core(&self.lower()) == Ordering::Greater
                }
            }
            Op::GreaterEq => *v >= self.lower(),
            Op::Less => *v < self.lower(),
            Op::LessEq => {
                if self.patch.is_some() {
                    *v <= self.lower()
                } else {
                    prefix(v) || *v < self.lower()
                }
            }
            Op::Tilde => {
                if *v < self.lower() {
                    return false;
                }
                match self.minor {
                    Some(m) => v.major == self.major && v.minor == m,
                    None => v.major == self.major,
                }
            }
            Op::Caret => {
                if *v < self.lower() {
                    return false;
                }
                if self.major > 0 || self.minor.is_none() {
                    return v.major == self.major;
                }
                let minor = self.minor.unwrap_or(0);
                if minor > 0 || self.patch.is_none() {
                    return v.major == 0 && v.minor == minor;
                }
                v.major == 0 && v.minor == 0 && Some(v.patch) == self.patch
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(req: &str, v: &str) -> bool {
        VersionReq::parse(req)
            .unwrap()
            .matches(&Version::parse(v).unwrap())
    }

    #[test]
    fn caret_and_ranges() {
        assert!(m("1.8", "1.8.7"));
        assert!(m("1.8", "1.9.0"));
        assert!(!m("1.8", "2.0.0"));
        assert!(!m("1.8", "1.7.9"));
        assert!(m("0.8.18", "0.8.20"));
        assert!(!m("0.8.18", "0.9.0"));
        assert!(m("^0.0.3", "0.0.3"));
        assert!(!m("^0.0.3", "0.0.4"));
        assert!(m(">=1.2, <2", "1.99.0"));
        assert!(!m(">=1.2, <2", "2.0.0"));
        assert!(m("~1.2", "1.2.9"));
        assert!(!m("~1.2", "1.3.0"));
        assert!(m("=1.2.3", "1.2.3"));
        assert!(!m("=1.2.3", "1.2.4"));
        assert!(m("*", "7.0.0"));
        assert!(m("1.*", "1.4.0"));
        assert!(!m("1.*", "2.0.0"));
        assert!(m("<=1.2", "1.2.7"));
        assert!(!m(">1.2", "1.2.7"));
        assert!(m(">1.2", "1.3.0"));
    }

    #[test]
    fn intersections() {
        let x = |a: &str, b: &str| {
            let (a, b) = (VersionReq::parse(a).unwrap(), VersionReq::parse(b).unwrap());
            let r = a.intersects(&b);
            assert_eq!(r, b.intersects(&a), "symmetric");
            r
        };
        assert!(x("=0.2.189", "=0.2.189"));
        assert!(!x("=0.2.189", "=0.2.190"));
        assert!(x("=0.2.189", "0.2"));
        assert!(!x("=0.3.4", "0.4"));
        assert!(x(">1.2.3", "<1.2.5"));
        assert!(!x(">1.2.3", "<=1.2.3"));
        assert!(x(">1.2", "~1.3"));
        assert!(!x(">1.2", "~1.2"));
        assert!(x(">1", "2.0.0"));
        assert!(!x(">=1.5, <2", "^2"));
        assert!(x("*", "=9.9.9"));
        assert!(!x("^0.0.3", "^0.0.4"));
        assert!(x(">=1.1.0-alpha.1", "=1.1.0-alpha.2"));
    }

    #[test]
    fn prereleases() {
        assert!(!m("1.0", "1.1.0-alpha.1"));
        assert!(m(">=1.1.0-alpha.1", "1.1.0-alpha.2"));
        assert!(Version::parse("1.0.0-alpha").unwrap() < Version::parse("1.0.0").unwrap());
        assert!(
            Version::parse("1.0.0-alpha.2").unwrap() < Version::parse("1.0.0-alpha.10").unwrap()
        );
        assert!(Version::parse("1.0.0+build").is_some());
        assert!(Version::parse("1.0").is_none());
        assert!(VersionReq::parse("abc").is_err());
    }
}
