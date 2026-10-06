//! WP-74: FL-1's Rust scope scanner (WP-63; [F21 §3]) against `moirai-tsoracle` ([F21 §3.9]; [40 §8.3.4] row 8).
//!
//! - **The repository's own Rust code** (every tracked or untracked-but-not-ignored `*.rs` file outside `fixtures/`
//!   and `testdata/` directories, which hold byte-exact test inputs, PLAN §2.2, §2.5 — [F21 §8.1]'s constructs
//!   outside the contract among them): the multiset differential of [`moirai_replay::scandiff`], with the name-path,
//!   header-line and span agreement rates, the unclaimed items, failed scans and non-text files reported beside
//!   them. The report is printed and written to `CARGO_TARGET_TMPDIR/moirai-replay-reports/scan-oracle-own.txt`; it
//!   counts but does not name the disagreements in the crates some roles must not read
//!   ([`moirai_replay::scandiff::COUNTS_ONLY`]) unless `MOIRAI_REPLAY_NAME_ITEMS=1`
//!   ([`moirai_replay::scandiff::NAME_ITEMS_ENV`]). This is WP-63's acceptance ("agreement with `moirai-tsoracle` on the repository's own Rust code"), and it is
//!   exact: in a file the oracle parses with no error, every claimed item agrees at all three levels and the scanner
//!   reports nothing else; in a file with syntax errors (a known grammar gap), every claimed item agrees and the
//!   scanner's further items are reported, not failed; no scan fails. [F21 §3.9] makes any disagreement on contract
//!   code a specification finding for chapter 21, so an accepted one is recorded there by name, never as slack
//!   here. Row 8's ≥ 99.5 % over the frozen owner snapshot is WP-76's gate. The scanner and the oracle read the same
//!   bytes: each file is read once and the oracle is given a scratch copy, so an edit made meanwhile by another
//!   session cannot make them disagree.
//! - **Generated sources of the contract** ([`gen_rust`]): three-way — the scanner and the oracle must each give
//!   exactly the items of the construction (lines, parents and pre-order included), with LF or CR LF line ends and
//!   with or without a BOM, and the oracle must parse every source with no error and claim every item. A source that
//!   uses one of the oracle's known grammar gaps (its crate documentation) is held to the construction by the
//!   scanner, and by the oracle only on the items it claims. The construction half runs everywhere; the oracle half
//!   needs the binary.
//!
//! The oracle binary is built only by the replay job, unpoisoned (`docs/m0/PLAN.md` §2.1). Where it is absent or
//! stale (the gate never builds it), the oracle halves say that they were skipped and pass.
//! [`moirai_replay::tsoracle::ENV`] makes that an error. The replay job must set it
//! (`MOIRAI_TSORACLE: target/debug/moirai-tsoracle.exe`; `pr.yml` is R-HARN's) for the differentials to run there
//! or fail the job. Each test writes its outcome line past libtest's capture (`job_log`), so every run's log shows,
//! even without `--nocapture`, whether the oracle halves ran and, for the repository, the counts and the three
//! agreement rates ([`moirai_replay::scandiff::Report::summary`]); the full report goes to the captured output and
//! the report file.

mod chunks;
mod common;
mod gen_rust;
mod scratch;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use moirai_files::scan::Lang;
use moirai_files::text::atext;
use moirai_replay::git::Git;
use moirai_replay::job_log;
use moirai_replay::scandiff::{
    Level, Report, Row, ScanSide, compare, name_items_requested, oracle_rows, scan_side,
};
use moirai_replay::tsoracle::{Located, Session, locate};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

use scratch::Scratch;

/// The workspace root: two levels above this crate's manifest directory, which cargo gives as an absolute path. It is
/// not canonicalised, which on Windows would give a verbatim (`\\?\`) path that `/` separators cannot extend.
fn workspace() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("the crate lies two levels below the workspace root")
        .to_path_buf()
}

/// The oracle binary, or `None` with the reason written to the log ([`job_log`]).
fn oracle(test: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().expect("the test executable's path");
    match locate(&exe, &workspace()).unwrap_or_else(|e| panic!("{e}")) {
        Located::Found(p) => Some(p),
        Located::Missing(p) => {
            job_log(&format!(
                "{test}: oracle half skipped: no moirai-tsoracle at {} (the replay job builds it)",
                p.display()
            ));
            None
        }
        Located::Stale { exe, why } => {
            job_log(&format!(
                "{test}: oracle half skipped: {} is stale: {why}; {} gives this work tree its own",
                exe.display(),
                why.remedy()
            ));
            None
        }
    }
}

/// Whether a listed path lies in a directory of byte-exact test inputs (`fixtures/` at the root, or any `testdata/`
/// directory; PLAN §2.2, §2.5), which may hold Rust outside the contract on purpose ([F21 §8.1]).
fn test_input(rel: &str) -> bool {
    rel.starts_with("fixtures/") || rel.starts_with("testdata/") || rel.contains("/testdata/")
}

#[test]
fn the_repository_rust_code_agrees_with_the_oracle() {
    const TEST: &str = "the_repository_rust_code_agrees_with_the_oracle";
    let Some(exe) = oracle(TEST) else { return };
    let root = workspace();
    let scratch = Scratch::new("scan-own");
    let git = Git::isolated(scratch.path()).expect("the isolated git home");
    let listed = git.ls_files(&root, ".rs").expect("git ls-files");
    let (inputs, files): (Vec<String>, Vec<String>) =
        listed.into_iter().partition(|rel| test_input(rel));
    assert!(files.len() > 100, "only {} Rust files listed", files.len());
    let copy = scratch.path().join("own.rs");
    let copy_text = copy.to_str().expect("a UTF-8 path").to_string();
    let mut session = Session::start(&exe, scratch.path()).expect("the oracle starts");
    let mut report = Report::default();
    let mut vanished = Vec::new();
    for rel in &files {
        let bytes = match std::fs::read(root.join(rel)) {
            Ok(b) => b,
            // Another session removed it after the listing.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                vanished.push(rel.as_str());
                continue;
            }
            Err(e) => panic!("{rel}: {e}"),
        };
        std::fs::write(&copy, &bytes).unwrap_or_else(|e| panic!("{}: {e}", copy.display()));
        let record = session
            .scan(&copy_text)
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        report.add(compare(rel, &scan_side(&bytes), &record));
    }
    session.finish().expect("the oracle exits cleanly");

    let body = if name_items_requested() {
        report.named().to_string()
    } else {
        report.to_string()
    };
    let text = format!(
        "scope-scanner differential over the repository's own Rust code ([F21 §3.9]; [40 §8.3.4] row 8)\n\
         {} files listed by git, {} of them test inputs left out, {} removed while the test ran\n{body}",
        files.len() + inputs.len(),
        inputs.len(),
        vanished.len()
    );
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("moirai-replay-reports");
    std::fs::create_dir_all(&dir).expect("the report directory");
    let file = dir.join("scan-oracle-own.txt");
    std::fs::write(&file, &text).expect("the report is written");
    eprintln!("{text}");
    job_log(&format!(
        "{TEST}: {} files listed by git, {} of them test inputs left out; full report in {}\n{}",
        files.len() + inputs.len(),
        inputs.len(),
        file.display(),
        report.summary()
    ));

    // Every entry of `disagreements` is a failed scan, a non-text file or a file that disagrees; a file with syntax
    // errors may disagree only by extra scanner items.
    let failing: Vec<&str> = report
        .disagreements
        .iter()
        .filter(|d| d.oracle_errors == 0 || !d.claimed_agree())
        .map(|d| d.path.as_str())
        .collect();
    assert!(
        failing.is_empty(),
        "{} files disagree with the oracle: {}\n{text}",
        failing.len(),
        failing.join(", ")
    );
    assert_eq!(
        report.files as usize + vanished.len(),
        files.len(),
        "{text}"
    );
    for level in Level::ALL {
        assert_eq!(
            report.agree[level as usize],
            report.claimed,
            "{}\n{text}",
            level.name()
        );
    }
}

/// The bytes of a rendered source with its line ends and BOM.
fn bytes_of(text: &str, crlf: bool, bom: bool) -> Vec<u8> {
    let mut b = Vec::new();
    if bom {
        b.extend_from_slice(b"\xef\xbb\xbf");
    }
    if crlf {
        b.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    } else {
        b.extend_from_slice(text.as_bytes());
    }
    b
}

fn first_difference(got: &[Row], want: &[Row]) -> String {
    let at = got
        .iter()
        .zip(want)
        .position(|(g, w)| g != w)
        .unwrap_or(got.len().min(want.len()));
    format!(
        "{} items, {} expected; first difference at #{at}: got {:?}, expected {:?}",
        got.len(),
        want.len(),
        got.get(at).map(ToString::to_string),
        want.get(at).map(ToString::to_string)
    )
}

#[test]
fn generated_rust_gives_its_construction_in_the_scanner_and_the_oracle() {
    const TEST: &str = "generated_rust_gives_its_construction_in_the_scanner_and_the_oracle";
    let scratch = Scratch::new("scan-generated");
    let session = oracle(TEST)
        .map(|exe| RefCell::new(Session::start(&exe, scratch.path()).expect("the oracle starts")));
    let file = scratch.path().join("gen.rs");
    let file_text = file.to_str().expect("a UTF-8 path").to_string();
    // Sources, items by construction, and sources that use a grammar gap of the oracle.
    let counts = RefCell::new((0u64, 0u64, 0u64));
    let cases = (
        gen_rust::source(),
        any::<bool>(),
        prop::bool::weighted(0.2),
        prop::collection::vec(any::<usize>(), 0..6),
    );
    common::runner(TEST, 64)
        .run(&cases, |((nodes, shebang, inner), crlf, bom, cuts)| {
            let (text, want, gap) = gen_rust::render(&nodes, shebang, inner);
            let bytes = bytes_of(&text, crlf, bom);
            let ctx = || format!("source (crlf {crlf}, bom {bom}):\n{text}");
            match scan_side(&bytes) {
                ScanSide::Items(got) if got == want => {
                    // Fed as a reader feeds it, in chunks that may end inside a token or a UTF-8 sequence.
                    let t = atext(&bytes).unwrap_or_default();
                    let chunked = chunks::scan_chunked(Lang::Rust, &t, &cuts);
                    if chunked.as_ref() != Some(&want) {
                        return Err(TestCaseError::fail(format!(
                            "scanner fed in chunks ending at {cuts:?} (modulo {}): {}\n{}",
                            t.len() + 1,
                            chunked.map_or_else(
                                || "the scan failed".into(),
                                |c| first_difference(&c, &want)
                            ),
                            ctx()
                        )));
                    }
                }
                other => {
                    let msg = match &other {
                        ScanSide::Items(got) => first_difference(got, &want),
                        _ => format!("{other:?}"),
                    };
                    return Err(TestCaseError::fail(format!("scanner: {msg}\n{}", ctx())));
                }
            }
            if let Some(s) = &session {
                std::fs::write(&file, &bytes).map_err(|e| TestCaseError::fail(e.to_string()))?;
                let rec = s
                    .borrow_mut()
                    .scan(&file_text)
                    .map_err(|e| TestCaseError::fail(e.to_string()))?;
                if gap && rec.errors > 0 {
                    // Outside the grammar, the oracle vouches only for the items it claims: those must be the
                    // construction's (lines and name paths), and the ones error recovery lost are not held against it.
                    let d = compare("gen.rs", &ScanSide::Items(want.clone()), &rec);
                    if !d.claimed_agree() {
                        return Err(TestCaseError::fail(format!(
                            "the oracle's claim on a source with a grammar gap: {d:?}\n{}",
                            ctx()
                        )));
                    }
                    let mut c = counts.borrow_mut();
                    c.0 += 1;
                    c.1 += want.len() as u64;
                    c.2 += 1;
                    return Ok(());
                }
                // With no errors, every item is claimed (`parse_record` refuses a record that says otherwise).
                let rows = oracle_rows(&rec);
                if rec.errors != 0 {
                    return Err(TestCaseError::fail(format!(
                        "the oracle parsed a generated source with {} errors\n{}",
                        rec.errors,
                        ctx()
                    )));
                }
                let got: Vec<Row> = rows.into_iter().map(|(r, _)| r).collect();
                if got != want {
                    return Err(TestCaseError::fail(format!(
                        "oracle: {}\n{}",
                        first_difference(&got, &want),
                        ctx()
                    )));
                }
                let d = compare("gen.rs", &ScanSide::Items(want.clone()), &rec);
                if !d.agrees() {
                    return Err(TestCaseError::fail(format!("differential: {d:?}")));
                }
            }
            let mut c = counts.borrow_mut();
            c.0 += 1;
            c.1 += want.len() as u64;
            c.2 += u64::from(gap);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
    let three_way = session.is_some();
    if let Some(s) = session {
        s.into_inner().finish().expect("the oracle exits cleanly");
    }
    let (sources, items, gaps) = counts.into_inner();
    job_log(&format!(
        "{TEST}: {sources} sources, {items} items by construction, {gaps} sources with a grammar gap of the oracle; {}",
        if three_way {
            "the scanner and the oracle checked"
        } else {
            "the scanner checked, the oracle half skipped"
        }
    ));
    assert!(items > sources, "the generator produces items");
    assert!(
        gaps > 0 && gaps < sources,
        "the generator writes grammar gaps in some sources only"
    );
}
