//! The window calendar, `/private/windows.toml` (PLAN §5 V4: "A calendar in `/private/windows.toml`";
//! `docs/m0/nightly.md` §2): the agent-free and beside-agents windows the owner agreed, each with an explicit UTC
//! offset. The file is owner data and stays in the gitignored `/private/`; the tests use synthetic calendars.

use crate::toml::{self, Value};
use crate::utc;

/// The calendar's file name under `/private/`.
pub const FILE: &str = "windows.toml";
/// The longest window: profile L's 3-day agent freeze per release-candidate iteration ([60 §3.15]).
pub const MAX_WINDOW_SECS: i64 = 72 * 3_600;

/// What runs beside the nightly jobs in a window ([60 §3.15]).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum WindowKind {
    /// No agent session, build or test beside the runner (PLAN §5 V4).
    AgentFree,
    /// Agents keep working; the jobs keep to the beside-agents caps ("by day", [60 §3.15]).
    BesideAgents,
}

impl WindowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WindowKind::AgentFree => "agent-free",
            WindowKind::BesideAgents => "beside-agents",
        }
    }
}

/// One agreed window, `[start, end)` in Unix seconds.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Window {
    pub start: i64,
    pub end: i64,
    pub kind: WindowKind,
}

impl Window {
    /// `2026-10-12T20:00:00Z..2026-10-13T04:00:00Z (agent-free)`.
    pub fn describe(&self) -> String {
        format!(
            "{}..{} ({})",
            utc::rfc3339(self.start),
            utc::rfc3339(self.end),
            self.kind.as_str()
        )
    }
}

/// Parses and checks a calendar: `version = 1` and `[[window]]` entries with `start`, `end` (RFC 3339 with `Z` or an
/// offset) and `kind` (`agent-free` or `beside-agents`), and an optional `note`, which the runner never reads or
/// copies. A window is at most 72 hours long and no two overlap. The windows are returned sorted by start.
// spec: [PLAN §5 V4] (the calendar of agreed windows), [PLAN §3.2 WP-05] (the window calendar)
pub fn parse(text: &str) -> Result<Vec<Window>, String> {
    let t = toml::parse(text).map_err(|e| format!("{FILE}: {e}"))?;
    if t.get("version").and_then(Value::as_integer) != Some(1) {
        return Err(format!("{FILE}: needs 'version = 1'"));
    }
    if let Some(k) = t
        .keys()
        .find(|k| !matches!(k.as_str(), "version" | "window"))
    {
        return Err(format!("{FILE}: unknown key '{k}'"));
    }
    let entries = match t.get("window") {
        None => &[][..],
        Some(Value::Array(a)) => a.as_slice(),
        Some(_) => {
            return Err(format!(
                "{FILE}: 'window' must be an array of tables ([[window]])"
            ));
        }
    };
    let mut out = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        let what = format!("{FILE}: window {}", i + 1);
        let w = e
            .as_table()
            .ok_or_else(|| format!("{what} is not a table"))?;
        if let Some(k) = w
            .keys()
            .find(|k| !matches!(k.as_str(), "start" | "end" | "kind" | "note"))
        {
            return Err(format!("{what}: unknown key '{k}'"));
        }
        let field = |k: &str| {
            w.get(k)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{what}: needs the string '{k}'"))
        };
        if w.get("note").is_some_and(|n| n.as_str().is_none()) {
            return Err(format!("{what}: 'note' must be a string"));
        }
        let start =
            utc::parse_rfc3339(field("start")?).map_err(|e| format!("{what}: start: {e}"))?;
        let end = utc::parse_rfc3339(field("end")?).map_err(|e| format!("{what}: end: {e}"))?;
        let kind = match field("kind")? {
            "agent-free" => WindowKind::AgentFree,
            "beside-agents" => WindowKind::BesideAgents,
            o => {
                return Err(format!(
                    "{what}: kind '{o}' is not agent-free or beside-agents"
                ));
            }
        };
        if end <= start {
            return Err(format!("{what}: it ends at or before its start"));
        }
        if end - start > MAX_WINDOW_SECS {
            return Err(format!("{what}: it is longer than 72 hours"));
        }
        out.push(Window { start, end, kind });
    }
    out.sort_by_key(|w| w.start);
    if let Some(p) = out.windows(2).find(|p| p[1].start < p[0].end) {
        return Err(format!(
            "{FILE}: the windows {} and {} overlap",
            p[0].describe(),
            p[1].describe()
        ));
    }
    Ok(out)
}

/// The window that holds `now`, if any.
pub fn current(windows: &[Window], now: i64) -> Option<Window> {
    windows
        .iter()
        .copied()
        .find(|w| w.start <= now && now < w.end)
}

/// The first window that starts after `now`, if any.
pub fn next(windows: &[Window], now: i64) -> Option<Window> {
    windows.iter().copied().find(|w| w.start > now)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    pub(crate) const SYNTHETIC: &str = r#"
version = 1
[[window]]
start = "2026-10-13T23:00:00+03:00"
end = "2026-10-14T07:00:00+03:00"
kind = "agent-free"
note = "synthetic"
[[window]]
start = "2026-10-12T09:00:00Z"
end = "2026-10-12T12:00Z"
kind = "beside-agents"
"#;

    #[test]
    fn a_synthetic_calendar() {
        let w = parse(SYNTHETIC).unwrap();
        assert_eq!(w.len(), 2);
        // Sorted by start; offsets applied.
        assert_eq!(w[0].kind, WindowKind::BesideAgents);
        assert_eq!(utc::rfc3339(w[1].start), "2026-10-13T20:00:00Z");
        assert_eq!(utc::rfc3339(w[1].end), "2026-10-14T04:00:00Z");
        let at = |s: &str| utc::parse_rfc3339(s).unwrap();
        assert_eq!(current(&w, at("2026-10-13T20:00:00Z")), Some(w[1]));
        assert_eq!(current(&w, at("2026-10-14T03:59:59Z")), Some(w[1]));
        assert_eq!(
            current(&w, at("2026-10-14T04:00:00Z")),
            None,
            "the end is outside"
        );
        assert_eq!(current(&w, at("2026-10-12T11:00:00Z")), Some(w[0]));
        assert_eq!(current(&w, at("2026-10-12T12:00:00Z")), None);
        assert_eq!(next(&w, at("2026-10-12T12:00:00Z")), Some(w[1]));
        assert_eq!(next(&w, at("2026-10-14T00:00:00Z")), None);
        assert_eq!(
            w[1].describe(),
            "2026-10-13T20:00:00Z..2026-10-14T04:00:00Z (agent-free)"
        );
        assert_eq!(parse("version = 1\n"), Ok(Vec::new()));
    }

    #[test]
    fn malformed_calendars_are_refused() {
        let one = |start: &str, end: &str, kind: &str, extra: &str| {
            format!(
                "version = 1\n[[window]]\nstart = \"{start}\"\nend = \"{end}\"\nkind = \"{kind}\"\n{extra}"
            )
        };
        for (text, needle) in [
            ("version = 2\n".to_string(), "version = 1"),
            ("version = 1\nfoo = 1\n".to_string(), "unknown key 'foo'"),
            ("version = 1\nwindow = 3\n".to_string(), "array of tables"),
            (one("2026-10-12T22:00:00Z", "2026-10-12T21:00:00Z", "agent-free", ""), "at or before"),
            (one("2026-10-12T22:00:00Z", "2026-10-15T22:00:01Z", "agent-free", ""), "longer than 72 hours"),
            (one("2026-10-12T22:00:00", "2026-10-13T06:00:00Z", "agent-free", ""), "start:"),
            (one("2026-10-12T22:00:00Z", "2026-10-13T06:00:00Z", "night", ""), "kind 'night'"),
            (one("2026-10-12T22:00:00Z", "2026-10-13T06:00:00Z", "agent-free", "owner = \"x\"\n"), "unknown key 'owner'"),
            (one("2026-10-12T22:00:00Z", "2026-10-13T06:00:00Z", "agent-free", "note = 3\n"), "'note' must be a string"),
            ("version = 1\n[[window]]\nstart = \"2026-10-12T22:00:00Z\"\nkind = \"agent-free\"\n".to_string(), "needs the string 'end'"),
            (
                one("2026-10-12T22:00:00Z", "2026-10-13T06:00:00Z", "agent-free", "")
                    + "[[window]]\nstart = \"2026-10-13T05:00:00Z\"\nend = \"2026-10-13T08:00:00Z\"\nkind = \"beside-agents\"\n",
                "overlap",
            ),
        ] {
            let e = parse(&text).unwrap_err();
            assert!(e.contains(needle), "{text}\n{e}");
        }
    }

    proptest! {
        #[test]
        fn the_current_window_holds_now(start in 1_700_000_000i64..1_900_000_000, len in 1i64..=MAX_WINDOW_SECS, at in 0i64..=2 * MAX_WINDOW_SECS) {
            let text = format!(
                "version = 1\n[[window]]\nstart = \"{}\"\nend = \"{}\"\nkind = \"agent-free\"\n",
                utc::rfc3339(start), utc::rfc3339(start + len)
            );
            let w = parse(&text).unwrap();
            let now = start - MAX_WINDOW_SECS / 2 + at;
            prop_assert_eq!(current(&w, now).is_some(), start <= now && now < start + len);
        }
    }
}
