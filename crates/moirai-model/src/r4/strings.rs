//! The frozen strings of R-16 ([F18 §4]; [40 §2.9]) and the `relink` provenance vocabulary of R-17 ([F18 §5]; [40 §2.2]):
//! the file, anchor and link states with their codes, the severity order, the detail registry with its labels and
//! text templates, the composition of a link's detail parts, the qualified form, the header strings and the reader
//! note, and the `relink` grammar with its evidence tokens, scores, `--confirm` mapping and guesses.

use crate::r4::path::Os;
use crate::r4::text::Ratio;

/// A link state ([F18 §4.2], §4.4): the file states, `stale-anchor` (link only) and LQ's `none`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    /// 1 `ok`.
    Ok,
    /// 2 `moved-auto`.
    MovedAuto,
    /// 3 `moved-needs-confirm`.
    MovedNeedsConfirm,
    /// 4 `ambiguous`.
    Ambiguous,
    /// 5 `deleted`.
    Deleted,
    /// 6 `replaced`.
    Replaced,
    /// 7 `stale-anchor`: a link state, never stored.
    StaleAnchor,
    /// 8 `missing`.
    Missing,
    /// 9 `absent-in-tree`.
    AbsentInTree,
    /// 10 `pending`.
    Pending,
    /// 11 `planned`.
    Planned,
    /// 12 `unverified`.
    Unverified,
    /// 13 `none`: LQ only, never stored.
    None,
}

impl State {
    /// The code ([F18 §4.2], §4.4).
    pub fn code(self) -> u8 {
        match self {
            State::Ok => 1,
            State::MovedAuto => 2,
            State::MovedNeedsConfirm => 3,
            State::Ambiguous => 4,
            State::Deleted => 5,
            State::Replaced => 6,
            State::StaleAnchor => 7,
            State::Missing => 8,
            State::AbsentInTree => 9,
            State::Pending => 10,
            State::Planned => 11,
            State::Unverified => 12,
            State::None => 13,
        }
    }

    /// The frozen string.
    // spec: [F18 §4.2]; [F18 §4.4]
    pub fn name(self) -> &'static str {
        match self {
            State::Ok => "ok",
            State::MovedAuto => "moved-auto",
            State::MovedNeedsConfirm => "moved-needs-confirm",
            State::Ambiguous => "ambiguous",
            State::Deleted => "deleted",
            State::Replaced => "replaced",
            State::StaleAnchor => "stale-anchor",
            State::Missing => "missing",
            State::AbsentInTree => "absent-in-tree",
            State::Pending => "pending",
            State::Planned => "planned",
            State::Unverified => "unverified",
            State::None => "none",
        }
    }

    /// The severity rank ([F18 §4.5]): 1 is the most severe; `none` has none.
    // spec: [F18 §4.5]
    pub fn severity(self) -> Option<u8> {
        Some(match self {
            State::Missing => 1,
            State::Replaced => 2,
            State::Deleted => 3,
            State::Ambiguous => 4,
            State::MovedNeedsConfirm => 5,
            State::StaleAnchor => 6,
            State::Unverified => 7,
            State::Pending => 8,
            State::Planned => 9,
            State::AbsentInTree => 10,
            State::MovedAuto => 11,
            State::Ok => 12,
            State::None => return Option::None,
        })
    }

    /// Whether a runtime row may record the state ([F18 §4.10]; [F11 §12.5]: the states a settle decides).
    pub fn recordable(self) -> bool {
        matches!(
            self,
            State::Ok
                | State::MovedAuto
                | State::MovedNeedsConfirm
                | State::Ambiguous
                | State::Replaced
                | State::Missing
                | State::Unverified
        )
    }
}

/// The most severe of several states ([F18 §4.5]); `none` when there is nothing to summarise.
// spec: [F18 §4.5]
pub fn most_severe(states: impl IntoIterator<Item = State>) -> State {
    states
        .into_iter()
        .filter(|s| s.severity().is_some())
        .min_by_key(|s| s.severity())
        .unwrap_or(State::None)
}

/// One row of the detail registry ([F18 §4.6]).
#[derive(Clone, Copy, Debug)]
pub struct DetailDef {
    /// The code.
    pub code: u8,
    /// The ASCII token.
    pub token: &'static str,
    /// The label of the qualified form; `None` when the detail is never principal.
    pub label: Option<&'static str>,
    /// The text template.
    pub text: &'static str,
}

/// The detail registry ([F18 §4.6]), codes 1–70; detail 40's label and text are per OS ([`trash_text`]).
// spec: [F18 §4.6]
pub const DETAILS: [DetailDef; 70] = [
    DetailDef {
        code: 1,
        token: "changed",
        label: None,
        text: "changed since <c8>",
    },
    DetailDef {
        code: 2,
        token: "spelling",
        label: Some("spelling differs on disk"),
        text: "spelling differs on disk (<path>)",
    },
    DetailDef {
        code: 3,
        token: "normalization",
        label: Some("normalization differs on disk"),
        text: "normalization differs on disk",
    },
    DetailDef {
        code: 4,
        token: "body-changed",
        label: None,
        text: "body changed since capture",
    },
    DetailDef {
        code: 5,
        token: "evidence",
        label: None,
        text: "<ev>",
    },
    DetailDef {
        code: 6,
        token: "recorded",
        label: Some("recorded"),
        text: "recorded <c8>",
    },
    DetailDef {
        code: 7,
        token: "not-recorded",
        label: Some("not yet recorded"),
        text: "not yet recorded (moirai links sync)",
    },
    DetailDef {
        code: 8,
        token: "reader-tree",
        label: Some("reader tree: not recorded"),
        text: "reader tree: not recorded",
    },
    DetailDef {
        code: 9,
        token: "uncommitted-main",
        label: Some("uncommitted in the main tree: not recorded"),
        text: "uncommitted in the main tree: not recorded",
    },
    DetailDef {
        code: 10,
        token: "identical-copy",
        label: Some("identical copy"),
        text: "identical copy",
    },
    DetailDef {
        code: 11,
        token: "file-id-edited",
        label: Some("moved and edited in place"),
        text: "moved and edited in place",
    },
    DetailDef {
        code: 12,
        token: "prefix-strong",
        label: Some("inferred directory move"),
        text: "inferred directory move",
    },
    DetailDef {
        code: 13,
        token: "git-pair",
        label: Some("git rename"),
        text: "git rename <score>",
    },
    DetailDef {
        code: 14,
        token: "edited+moved",
        label: Some("edited+moved"),
        text: "edited+moved <score>",
    },
    DetailDef {
        code: 15,
        token: "similarity",
        label: Some("similar"),
        text: "similar <score>[, runner-up <score>]",
    },
    DetailDef {
        code: 16,
        token: "weak",
        label: Some("weak"),
        text: "weak <score>",
    },
    DetailDef {
        code: 17,
        token: "split",
        label: Some("split"),
        text: "split",
    },
    DetailDef {
        code: 18,
        token: "merged",
        label: Some("merged"),
        text: "merged <score>",
    },
    DetailDef {
        code: 19,
        token: "argv",
        label: Some("seen in a shell command"),
        text: "seen in a shell command",
    },
    DetailDef {
        code: 20,
        token: "moved-differently",
        label: Some("moved differently on this line"),
        text: "moved differently on this line",
    },
    DetailDef {
        code: 21,
        token: "dir-replaced",
        label: Some("directory moved, file replaced"),
        text: "directory moved, file replaced",
    },
    DetailDef {
        code: 22,
        token: "candidates",
        label: Some("candidates"),
        text: "<n> candidates",
    },
    DetailDef {
        code: 23,
        token: "rename-over",
        label: Some("rename-over"),
        text: "rename-over",
    },
    DetailDef {
        code: 24,
        token: "swap",
        label: Some("swap"),
        text: "swap",
    },
    DetailDef {
        code: 25,
        token: "merge-conflict",
        label: Some("merge conflict"),
        text: "merge conflict: <path> | <path>",
    },
    DetailDef {
        code: 26,
        token: "case-collision",
        label: Some("case collision"),
        text: "case collision",
    },
    DetailDef {
        code: 27,
        token: "normalization-collision",
        label: Some("normalization collision"),
        text: "normalization collision",
    },
    DetailDef {
        code: 28,
        token: "path-claim",
        label: Some("PathClaim"),
        text: "PathClaim",
    },
    DetailDef {
        code: 29,
        token: "path-reused",
        label: Some("path reused"),
        text: "path reused; original at <path>",
    },
    DetailDef {
        code: 30,
        token: "removed",
        label: Some("removed"),
        text: "removed in <c8>",
    },
    DetailDef {
        code: 31,
        token: "node-deleted",
        label: Some("node deleted"),
        text: "node deleted in <c8>",
    },
    DetailDef {
        code: 32,
        token: "reason",
        label: None,
        text: "reason: <text>",
    },
    DetailDef {
        code: 33,
        token: "replaced-by",
        label: None,
        text: "replaced by #<n>",
    },
    DetailDef {
        code: 34,
        token: "unrelated",
        label: Some("unrelated content"),
        text: "re-created with unrelated content (containment <score>/<score>)",
    },
    DetailDef {
        code: 35,
        token: "git-readded",
        label: None,
        text: "deleted in git <g7>, re-added in <g7>",
    },
    DetailDef {
        code: 36,
        token: "since",
        label: None,
        text: "since <age>",
    },
    DetailDef {
        code: 37,
        token: "no-candidate",
        label: Some("no candidate"),
        text: "no candidate",
    },
    DetailDef {
        code: 38,
        token: "outside-root",
        label: Some("moved outside the root"),
        text: "moved outside the root",
    },
    DetailDef {
        code: 39,
        token: "ignored",
        label: Some("moved into ignored output"),
        text: "moved into ignored output",
    },
    DetailDef {
        code: 40,
        token: "trash",
        label: None,
        text: "",
    },
    DetailDef {
        code: 41,
        token: "never-candidate",
        label: Some("moved to a temporary or backup name"),
        text: "moved to a temporary or backup name",
    },
    DetailDef {
        code: 42,
        token: "cloud-target",
        label: Some("moved to a cloud-only file"),
        text: "moved to a cloud-only file",
    },
    DetailDef {
        code: 43,
        token: "deleted-in-git",
        label: Some("deleted in git"),
        text: "deleted in git <g7>",
    },
    DetailDef {
        code: 44,
        token: "not-representable",
        label: Some("not representable on this OS"),
        text: "not representable on this OS",
    },
    DetailDef {
        code: 45,
        token: "unrepresentable",
        label: Some("unrepresentable path"),
        text: "unrepresentable path",
    },
    DetailDef {
        code: 46,
        token: "nothing-written",
        label: None,
        text: "nothing written",
    },
    DetailDef {
        code: 47,
        token: "behind",
        label: Some("behind"),
        text: "behind",
    },
    DetailDef {
        code: 48,
        token: "diverged",
        label: Some("diverged"),
        text: "diverged",
    },
    DetailDef {
        code: 49,
        token: "observed-at",
        label: None,
        text: "observed at git <g7>",
    },
    DetailDef {
        code: 50,
        token: "not-here-yet",
        label: None,
        text: "not in this tree yet",
    },
    DetailDef {
        code: 51,
        token: "pending-age",
        label: None,
        text: "<age>",
    },
    DetailDef {
        code: 52,
        token: "plan-predates",
        label: Some("file present, tree predates the plan"),
        text: "file present, tree predates the plan",
    },
    DetailDef {
        code: 53,
        token: "budget",
        label: Some("budget"),
        text: "budget",
    },
    DetailDef {
        code: 54,
        token: "cloud-only",
        label: Some("cloud-only"),
        text: "cloud-only",
    },
    DetailDef {
        code: 55,
        token: "commit-not-here",
        label: Some("commit not in this repository"),
        text: "commit not in this repository",
    },
    DetailDef {
        code: 56,
        token: "no-tree",
        label: Some("no tree"),
        text: "no tree",
    },
    DetailDef {
        code: 57,
        token: "git",
        label: Some("git"),
        text: "git: moirai links sync | moirai check",
    },
    DetailDef {
        code: 58,
        token: "size",
        label: Some("size"),
        text: "size",
    },
    DetailDef {
        code: 59,
        token: "unreadable",
        label: Some("unreadable"),
        text: "unreadable",
    },
    DetailDef {
        code: 60,
        token: "unmapped-root",
        label: Some("unmapped root"),
        text: "unmapped root",
    },
    DetailDef {
        code: 61,
        token: "oid-algo",
        label: Some("oid algorithm differs"),
        text: "oid algorithm differs",
    },
    DetailDef {
        code: 62,
        token: "edited",
        label: Some("edited"),
        text: "edited[ <score>]",
    },
    DetailDef {
        code: 63,
        token: "edited-scope",
        label: Some("edited"),
        text: "edited (scope only)",
    },
    DetailDef {
        code: 64,
        token: "anchor-ambiguous",
        label: Some("ambiguous"),
        text: "ambiguous",
    },
    DetailDef {
        code: 65,
        token: "orphaned",
        label: Some("orphaned"),
        text: "orphaned",
    },
    DetailDef {
        code: 66,
        token: "was",
        label: None,
        text: "was: \"<quote>\"",
    },
    DetailDef {
        code: 67,
        token: "text-unavailable",
        label: None,
        text: "text-unavailable",
    },
    DetailDef {
        code: 68,
        token: "accepted-guess",
        label: None,
        text: "accepted guess <c8>",
    },
    DetailDef {
        code: 69,
        token: "confirm",
        label: None,
        text: "confirm: moirai links fix <n> --confirm",
    },
    DetailDef {
        code: 70,
        token: "glob-empty",
        label: Some("glob matches nothing"),
        text: "glob matches nothing",
    },
];

/// The intent-recovery strings, not link details ([F18 §4.6]): (code, token, text).
// spec: [F18 §4.6] intent-recovery strings
pub const INTENT_STRINGS: [(u8, &str, &str); 3] = [
    (71, "changed-after-move", "content changed after the move"),
    (72, "intent-both", "interrupted move: both paths present"),
    (
        73,
        "intent-neither",
        "interrupted move: neither path present",
    ),
];

/// A detail by code.
pub fn detail(code: u8) -> &'static DetailDef {
    DETAILS
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("detail code {code} is not in the registry ([F18 §4.6])"))
}

/// Detail 40's label and text on an OS ([F18 §4.9]).
// spec: [F18 §4.9]
pub fn trash_text(os: Os) -> &'static str {
    match os {
        Os::Windows => "in the Recycle Bin",
        Os::Linux | Os::Macos => "in the trash",
    }
}

/// The label of a detail on an OS.
pub fn label(code: u8, os: Os) -> Option<&'static str> {
    if code == 40 {
        Some(trash_text(os))
    } else {
        detail(code).label
    }
}

/// The parts a state's detail may hold ([F18 §4.7] rule 1): a sequence of groups, each (codes, required); a group with
/// several codes holds exactly one of them.
// spec: [F18 §4.7] rule 1
pub fn parts(state: State) -> Vec<(Vec<u8>, bool)> {
    let r = |a: u8, b: u8| (a..=b).collect::<Vec<u8>>();
    let guess = (vec![68, 69], false);
    match state {
        State::Ok => vec![
            (vec![1], false),
            (vec![2, 3], false),
            (vec![4], false),
            (vec![67], false),
            guess,
        ],
        State::MovedAuto => vec![
            (vec![5], true),
            (vec![6, 7, 8, 9], true),
            (vec![4], false),
            (vec![67], false),
            guess,
        ],
        State::MovedNeedsConfirm => vec![(r(10, 21), true), guess],
        State::Ambiguous => vec![(r(22, 29), true), guess],
        State::Deleted => vec![(vec![30, 31], true), (vec![32], false), (vec![33], false)],
        State::Replaced => vec![(vec![34], true), (vec![35], false), guess],
        State::StaleAnchor => vec![(vec![62, 63, 64, 65], true), (vec![66, 67], false), guess],
        State::Missing => vec![
            (vec![36], false),
            (r(37, 45), true),
            (vec![46], true),
            guess,
        ],
        State::AbsentInTree => vec![(vec![47, 48], true), (vec![49], false), guess],
        State::Pending => vec![(vec![50], true), (vec![51], false), guess],
        State::Planned => vec![(vec![52], false)],
        State::Unverified => vec![(r(53, 61), true), (vec![67], false), guess],
        State::None => Vec::new(),
    }
}

/// Whether a detail sequence is a valid composition for a state ([F18 §4.7] rule 1): every group in order, a required
/// group present, an optional one at most once, parts 68 and 69 together.
// spec: [F18 §4.7] rule 1
pub fn valid_parts(state: State, codes: &[u8]) -> bool {
    let mut i = 0;
    for (group, required) in parts(state) {
        if group == [68, 69] {
            if codes.get(i) == Some(&68) {
                if codes.get(i + 1) != Some(&69) {
                    return false;
                }
                i += 2;
            }
            continue;
        }
        match codes.get(i) {
            Some(c) if group.contains(c) => i += 1,
            _ if required => return false,
            _ => {}
        }
    }
    i == codes.len()
}

/// The principal part of a detail sequence ([F18 §4.7] rule 2): the part from the first braced set, and for `ok` part 2
/// or 3; `None` for the states without one.
// spec: [F18 §4.7] rule 2
pub fn principal(state: State, codes: &[u8]) -> Option<u8> {
    let first: &[u8] = match state {
        State::Ok => &[2, 3],
        State::MovedAuto => &[6, 7, 8, 9],
        State::MovedNeedsConfirm => &[10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21],
        State::Ambiguous => &[22, 23, 24, 25, 26, 27, 28, 29],
        State::StaleAnchor => &[62, 63, 64, 65],
        State::Missing => &[37, 38, 39, 40, 41, 42, 43, 44, 45],
        State::AbsentInTree => &[47, 48],
        State::Planned => &[52],
        State::Unverified => &[53, 54, 55, 56, 57, 58, 59, 60, 61],
        State::Deleted | State::Replaced | State::Pending | State::None => &[],
    };
    codes.iter().copied().find(|c| first.contains(c))
}

/// The qualified form ([F18 §4.7] rule 3): `<state> (<label>)`, or `<state>` without a principal part.
// spec: [F18 §4.7] rule 3
pub fn qualified(state: State, codes: &[u8], os: Os) -> String {
    match principal(state, codes).and_then(|c| label(c, os)) {
        Some(l) => format!("{} ({l})", state.name()),
        None => state.name().to_string(),
    }
}

/// The header's `files` field for a tree that is not eligible ([F18 §4.8] item 1).
// spec: [F18 §4.8] item 1
pub const NO_TREE: &str = "files: no tree bound";

/// A slot value cut from the left to at most `max` bytes: `...` and the longest suffix of at most `max − 3` bytes that
/// starts at a scalar-value boundary ([F18 §4.8] item 2).
fn cut_left(v: &str, max: usize) -> String {
    if v.len() <= max {
        return v.to_string();
    }
    let keep = max - 3;
    let mut start = v.len() - keep;
    while !v.is_char_boundary(start) {
        start += 1;
    }
    format!("...{}", &v[start..])
}

/// `<here>` of the reader note ([F18 §4.8] item 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Here {
    /// The branch T's HEAD names, in the short form.
    Ref(String),
    /// A detached HEAD at a commit (hex).
    Detached(String),
    /// No readable git HEAD.
    NoGit,
}

/// `<there>` of the reader note ([F18 §4.8] item 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum There {
    /// The binding's expected ref.
    Ref(String),
    /// Another tree's display label.
    Tree(String),
    /// The branch has no designated tree.
    BoundTree,
    /// The designated tree has git and no expected ref.
    GitLine,
}

/// The reader note on line 2 ([F18 §4.8] item 2): `reading only: tree on <here>, branch expects <there>`, each slot at
/// most 20 bytes (a tree label at most 15), cut from the left.
// spec: [F18 §4.8] item 2
pub fn reader_note(here: &Here, there: &There) -> String {
    let h = match here {
        Here::Ref(r) => cut_left(r, 20),
        Here::Detached(c) => format!("detached {}", &c[..7.min(c.len())]),
        Here::NoGit => "no git".into(),
    };
    let t = match there {
        There::Ref(r) => cut_left(r, 20),
        There::Tree(l) => format!("tree {}", cut_left(l, 15)),
        There::BoundTree => "a bound tree".into(),
        There::GitLine => "a git line".into(),
    };
    format!("reading only: tree on {h}, branch expects {t}")
}

/// A `relink` evidence token ([F18 §5.2]): (code, token, scored).
// spec: [F18 §5.2]
pub const EVIDENCE: [(u8, &str, bool); 28] = [
    (1, "intent", false),
    (2, "intent-recovered", false),
    (3, "file-id", false),
    (4, "dir-id", false),
    (5, "oid+ctime", false),
    (6, "prefix", false),
    (7, "pending", false),
    (8, "r100", false),
    (9, "case", false),
    (10, "move", false),
    (11, "usn", false),
    (12, "fsevents", false),
    (13, "identical-copy", false),
    (14, "file-id-edited", false),
    (15, "prefix-strong", false),
    (16, "git-pair", true),
    (17, "edited+moved", true),
    (18, "similarity", true),
    (19, "weak", true),
    (20, "split", false),
    (21, "merged", true),
    (22, "argv", false),
    (23, "tie", false),
    (24, "rename-over", false),
    (25, "swap", false),
    (26, "path-reused", false),
    (27, "manual", false),
    (28, "replacement", false),
];

/// An evidence token by code.
pub fn evidence_token(code: u8) -> &'static str {
    EVIDENCE
        .iter()
        .find(|e| e.0 == code)
        .map(|e| e.1)
        .unwrap_or_else(|| panic!("evidence code {code} is not in [F18 §5.2]"))
}

/// A score in two decimals, half-even ([F18 §5.3]).
// spec: [F18 §5.3]
pub fn score_text(r: Ratio) -> String {
    let r = r.min(Ratio::int(1));
    let x = r.num * 100;
    let (q, rem) = (x / r.den, x % r.den);
    let k = match (rem * 2).cmp(&r.den) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q % 2),
    };
    if k >= 100 {
        "1.00".into()
    } else {
        format!("0.{k:02}")
    }
}

/// Whether a text is a valid `relink` value ([F18 §5.1]).
// spec: [F18 §5.1]
pub fn relink_valid(v: &str) -> bool {
    const LAZY: [&str; 5] = ["file-id", "dir-id", "oid+ctime", "prefix", "pending"];
    const GIT: [&str; 2] = ["r100", "case"];
    const HOOK: [&str; 2] = ["file-id", "move"];
    const UNSCORED: [&str; 9] = [
        "identical-copy",
        "file-id-edited",
        "prefix-strong",
        "split",
        "argv",
        "tie",
        "rename-over",
        "swap",
        "path-reused",
    ];
    const SCORED: [&str; 5] = ["git-pair", "edited+moved", "similarity", "weak", "merged"];
    let score_ok = |s: &str| {
        s == "1.00"
            || (s.len() == 4 && s.starts_with("0.") && s[2..].bytes().all(|b| b.is_ascii_digit()))
    };
    let Some((how, rest)) = v.split_once('/') else {
        return false;
    };
    match how {
        "explicit" => rest == "intent" || rest == "intent-recovered",
        "lazy" => LAZY.contains(&rest),
        "git" => GIT.contains(&rest),
        "hook" => HOOK.contains(&rest),
        "journal" => rest == "usn" || rest == "fsevents",
        "merge-observation" => LAZY.contains(&rest) || GIT.contains(&rest) || HOOK.contains(&rest),
        "merge-compose" => rest == "prefix",
        "owner" | "agent" | "policy" | "confirmed" => {
            if UNSCORED.contains(&rest) || rest == "manual" || rest == "replacement" {
                return true;
            }
            match rest.split_once('/') {
                Some((ev, s)) => SCORED.contains(&ev) && score_ok(s),
                None => false,
            }
        }
        _ => false,
    }
}

/// `--confirm` ([F18 §5.5]): `agent/…` and `policy/…` become `confirmed/…`; anything else is refused.
// spec: [F18 §5.5]
pub fn confirm(v: Option<&str>) -> Option<String> {
    let v = v?;
    let (how, rest) = v.split_once('/')?;
    matches!(how, "agent" | "policy").then(|| format!("confirmed/{rest}"))
}

/// Whether a `relink` value is a guess: its `how` is `agent` or `policy` ([F18 §5.6]).
// spec: [F18 §5.6]
pub fn is_guess(v: Option<&str>) -> bool {
    v.and_then(|v| v.split_once('/'))
        .is_some_and(|(how, _)| how == "agent" || how == "policy")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rows of a Markdown table of [F18] that starts after `heading`, as trimmed cells (`\|` kept as `|`).
    fn f18_rows(heading: &str) -> Vec<Vec<String>> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/format/18-file-links.md");
        let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let start = text
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} not in [F18]"));
        let mut rows = Vec::new();
        let mut in_table = false;
        for line in text[start..].lines().skip(1) {
            if line.starts_with('|') {
                in_table = true;
                let cells: Vec<String> = line
                    .replace("\\|", "\u{1}")
                    .split('|')
                    .map(|c| c.trim().replace('\u{1}', "|"))
                    .collect();
                rows.push(cells[1..cells.len() - 1].to_vec());
            } else if in_table {
                break;
            }
        }
        rows
    }

    /// The first code span of a cell (the template), without its back-quotes.
    fn unquote(c: &str) -> &str {
        match c.find('`') {
            Some(i) => {
                let rest = &c[i + 1..];
                &rest[..rest.find('`').unwrap_or(rest.len())]
            }
            None => c,
        }
    }

    /// The registry transcribed here equals [F18 §4.6]'s table: every code, token, label and text template.
    #[test]
    fn the_detail_registry_equals_f18_4_6() {
        let rows: Vec<Vec<String>> = f18_rows("### 4.6 The detail registry")
            .into_iter()
            .filter(|r| r.len() == 5 && r[0].parse::<u8>().is_ok())
            .collect();
        assert_eq!(rows.len(), 70, "codes 1-70");
        for r in &rows {
            let code: u8 = r[0].parse().unwrap();
            let d = detail(code);
            assert_eq!(d.token, r[1].trim_matches('`'), "token of {code}");
            let label = if r[3] == "—" || r[3].starts_with("per OS") {
                None
            } else {
                Some(r[3].trim_matches('`'))
            };
            if code != 40 {
                assert_eq!(d.label, label, "label of {code}");
                assert_eq!(d.text, unquote(&r[4]), "text of {code}");
            }
        }
    }

    /// The evidence tokens transcribed here equal [F18 §5.2]'s table.
    #[test]
    fn the_evidence_tokens_equal_f18_5_2() {
        let rows: Vec<Vec<String>> = f18_rows("### 5.2 Evidence tokens")
            .into_iter()
            .filter(|r| r.len() == 5 && r[0].parse::<u8>().is_ok())
            .collect();
        assert_eq!(rows.len(), 28);
        for r in &rows {
            let code: u8 = r[0].parse().unwrap();
            let e = EVIDENCE.iter().find(|e| e.0 == code).unwrap();
            assert_eq!(e.1, r[1].trim_matches('`'), "token of {code}");
            assert_eq!(e.2, r[2] == "yes", "scored of {code}");
        }
    }

    #[test]
    fn states_codes_and_severity() {
        assert_eq!(State::Replaced.code(), 6);
        assert_eq!(State::Missing.code(), 8);
        assert_eq!(
            most_severe([State::Ok, State::Deleted, State::Replaced]),
            State::Replaced
        );
        assert_eq!(most_severe([]), State::None);
        for (i, d) in DETAILS.iter().enumerate() {
            assert_eq!(usize::from(d.code), i + 1);
        }
    }

    #[test]
    fn compositions_and_qualified_forms() {
        assert!(valid_parts(State::Missing, &[36, 37, 46]));
        assert!(valid_parts(State::Missing, &[38, 46, 68, 69]));
        assert!(!valid_parts(State::Missing, &[37]));
        assert!(valid_parts(State::MovedAuto, &[5, 7]));
        assert!(!valid_parts(State::MovedAuto, &[7]));
        assert!(valid_parts(State::Ok, &[]));
        assert!(valid_parts(State::Ok, &[1, 2]));
        assert!(!valid_parts(State::Ok, &[2, 1]));
        assert_eq!(
            qualified(State::Ambiguous, &[27], Os::Windows),
            "ambiguous (normalization collision)"
        );
        assert_eq!(
            qualified(State::Missing, &[44, 46], Os::Windows),
            "missing (not representable on this OS)"
        );
        assert_eq!(
            qualified(State::Ok, &[1, 2], Os::Linux),
            "ok (spelling differs on disk)"
        );
        assert_eq!(qualified(State::Ok, &[1], Os::Linux), "ok");
        assert_eq!(
            qualified(State::AbsentInTree, &[47, 49], Os::Linux),
            "absent-in-tree (behind)"
        );
        assert_eq!(
            qualified(State::Missing, &[40, 46], Os::Windows),
            "missing (in the Recycle Bin)"
        );
        assert_eq!(
            qualified(State::MovedAuto, &[5, 7], Os::Windows),
            "moved-auto (not yet recorded)"
        );
        assert_eq!(qualified(State::Deleted, &[30], Os::Windows), "deleted");
    }

    #[test]
    fn reader_note_cuts_slots() {
        assert_eq!(
            reader_note(&Here::Ref("u/other".into()), &There::Ref("u/l5np".into())),
            "reading only: tree on u/other, branch expects u/l5np"
        );
        let n = reader_note(
            &Here::Ref("feature/a-very-long-branch-name".into()),
            &There::Tree("lanes/some-very-long-label".into()),
        );
        assert!(n.len() <= 79, "{n}");
        assert!(n.contains("..."));
        assert_eq!(
            reader_note(&Here::Detached("7c1e0a4d99".into()), &There::GitLine),
            "reading only: tree on detached 7c1e0a4, branch expects a git line"
        );
    }

    #[test]
    fn relink_grammar_scores_and_confirm() {
        for v in [
            "explicit/intent",
            "lazy/oid+ctime",
            "git/r100",
            "hook/move",
            "agent/similarity/0.81",
            "confirmed/edited+moved/0.81",
            "merge-observation/oid+ctime",
            "merge-compose/prefix",
            "owner/manual",
            "policy/git-pair/1.00",
        ] {
            assert!(relink_valid(v), "{v}");
        }
        for v in [
            "",
            "lazy/r100",
            "agent/similarity",
            "agent/split/0.50",
            "agent/similarity/0.8",
            "x/y",
        ] {
            assert!(!relink_valid(v), "{v}");
        }
        assert_eq!(score_text(Ratio::new(81, 100)), "0.81");
        assert_eq!(
            score_text(Ratio::new(1, 8)),
            "0.12",
            "0.125 rounds half-even to 0.12"
        );
        assert_eq!(
            score_text(Ratio::new(3, 8)),
            "0.38",
            "0.375 rounds half-even to 0.38"
        );
        assert_eq!(score_text(Ratio::new(1, 1)), "1.00");
        assert_eq!(score_text(Ratio::new(7, 100_000)), "0.00");
        assert_eq!(
            confirm(Some("agent/manual")).as_deref(),
            Some("confirmed/manual")
        );
        assert_eq!(confirm(Some("owner/manual")), None);
        assert!(is_guess(Some("policy/argv")));
        assert!(!is_guess(Some("confirmed/argv")));
    }
}
