//! Findings of the lints, with the crate and path they concern, so `xtask gate --branch` can reduce the ones a role
//! may not read (docs/m0/PLAN.md §3.1 "The gate worktree").

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diag {
    /// The lint's short name, as `xtask gate` reports it.
    pub lint: &'static str,
    pub krate: Option<String>,
    /// Repository-relative, with `/`.
    pub path: Option<String>,
    pub line: Option<u32>,
    pub message: String,
}

impl Diag {
    pub fn new(lint: &'static str, message: impl Into<String>) -> Diag {
        Diag {
            lint,
            krate: None,
            path: None,
            line: None,
            message: message.into(),
        }
    }

    pub fn krate(lint: &'static str, krate: &str, message: impl Into<String>) -> Diag {
        Diag {
            krate: Some(krate.to_string()),
            ..Diag::new(lint, message)
        }
    }

    pub fn path(lint: &'static str, path: &str, message: impl Into<String>) -> Diag {
        Diag {
            path: Some(path.to_string()),
            ..Diag::new(lint, message)
        }
    }

    pub fn at(
        lint: &'static str,
        krate: &str,
        path: &str,
        line: u32,
        message: impl Into<String>,
    ) -> Diag {
        Diag {
            krate: Some(krate.to_string()),
            path: Some(path.to_string()),
            line: Some(line),
            ..Diag::new(lint, message)
        }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] ", self.lint)?;
        match (&self.path, self.line, &self.krate) {
            (Some(p), Some(l), _) => write!(f, "{p}:{l}: ")?,
            (Some(p), None, _) => write!(f, "{p}: ")?,
            (None, _, Some(k)) => write!(f, "crate {k}: ")?,
            _ => {}
        }
        f.write_str(&self.message)
    }
}
