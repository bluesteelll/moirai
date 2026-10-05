//! The `moirai-tsoracle` binary: command line, JSON Lines output and exit statuses (crate documentation, "Command
//! line" and "Output"). Every test waits for its child process and removes its scratch directory.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_moirai-tsoracle");

/// A scratch directory under the system temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("moirai-tsoracle-cli-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Self(dir)
    }

    fn file(&self, name: &str, content: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, content).expect("scratch file");
        path_str(&path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn path_str(p: &Path) -> String {
    p.to_str().expect("scratch paths are Unicode").to_owned()
}

/// Runs the binary with `args` and `stdin`, and waits for it.
fn run(args: &[&str], stdin: &[u8]) -> Output {
    run_in(None, args, stdin)
}

/// Runs the binary in the working directory `dir` (or this process's), and waits for it.
fn run_in(dir: Option<&Path>, args: &[&str], stdin: &[u8]) -> Output {
    let mut command = Command::new(BIN);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    {
        let mut pipe = child.stdin.take().expect("stdin is piped");
        // The binary may exit before reading stdin (usage errors); a broken pipe is then expected.
        let _ = pipe.write_all(stdin);
    }
    child.wait_with_output().expect("the binary finishes")
}

fn records(out: &Output) -> Vec<Value> {
    let text = std::str::from_utf8(&out.stdout).expect("output is UTF-8");
    assert!(
        text.is_empty() || text.ends_with('\n'),
        "output does not end with a line feed: {text:?}"
    );
    text.lines()
        .map(|l| serde_json::from_str(l).expect("each line is one JSON value"))
        .collect()
}

/// (kind, name, qual, start, end, parent) of every item of a record.
fn items(record: &Value) -> Vec<(String, String, String, u64, u64, Option<u64>)> {
    record["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().expect("kind").to_owned(),
                i["name"].as_str().expect("name").to_owned(),
                i["qual"].as_str().expect("qual").to_owned(),
                i["start"].as_u64().expect("start"),
                i["end"].as_u64().expect("end"),
                i["parent"].as_u64(),
            )
        })
        .collect()
}

fn item(
    kind: &str,
    name: &str,
    qual: &str,
    start: u64,
    end: u64,
    parent: Option<u64>,
) -> (String, String, String, u64, u64, Option<u64>) {
    (
        kind.to_owned(),
        name.to_owned(),
        qual.to_owned(),
        start,
        end,
        parent,
    )
}

#[test]
fn files_are_scanned_in_argument_order() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"mod m {\n    fn f() {}\n}\n");
    let b = s.file("b.rs", b"impl<T> From<T> for W<T> {}\n");
    let out = run(&[&b, &a], b"");
    assert!(out.status.success(), "{out:?}");
    assert!(out.stderr.is_empty());
    let recs = records(&out);
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0]["path"], b.as_str());
    assert_eq!(recs[0]["errors"], 0);
    assert_eq!(
        items(&recs[0]),
        [item("impl", "W<T>", "From<T>", 1, 1, None)]
    );
    assert_eq!(recs[1]["path"], a.as_str());
    assert_eq!(
        items(&recs[1]),
        [
            item("mod", "m", "", 1, 3, None),
            item("fn", "f", "", 2, 2, Some(0))
        ]
    );
}

#[test]
fn records_have_the_documented_key_order() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn f() {}\n");
    let out = run(&[&a], b"");
    assert!(out.status.success(), "{out:?}");
    let line = String::from_utf8(out.stdout).expect("UTF-8");
    let want_tail = r#","errors":0,"items":[{"kind":"fn","name":"f","qual":"","start":1,"end":1,"parent":null,"ok":true}]}"#;
    assert!(line.starts_with("{\"path\":"), "{line}");
    assert!(line.ends_with(&format!("{want_tail}\n")), "{line}");
}

#[test]
fn a_list_is_read_from_a_file_or_standard_input() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    let b = s.file("b.rs", b"fn b() {}\n");
    let c = s.file("c.rs", b"fn c() {}\n");
    let list = s.file("list.txt", format!("{a}\r\n\r\n{b}\n").as_bytes());

    // Positional inputs and the list keep their argument order; the list's empty line is skipped.
    let out = run(&[&c, "--files-from", &list, &c], b"");
    assert!(out.status.success(), "{out:?}");
    let paths: Vec<Value> = records(&out).iter().map(|r| r["path"].clone()).collect();
    assert_eq!(paths, [c.as_str(), a.as_str(), b.as_str(), c.as_str()]);

    let out = run(&["--files-from", "-"], format!("{b}\n{a}").as_bytes());
    assert!(out.status.success(), "{out:?}");
    let names: Vec<String> = records(&out)
        .iter()
        .map(|r| items(r)[0].1.clone())
        .collect();
    assert_eq!(names, ["b", "a"]);
}

/// PowerShell 5.1's `Out-File -Encoding utf8` starts a list with a BOM; it is skipped, from a file and from standard
/// input alike.
#[test]
fn a_bom_at_the_start_of_a_list_is_skipped() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    let b = s.file("b.rs", b"fn b() {}\n");
    let text = format!("\u{FEFF}{a}\r\n{b}\r\n");
    let list = s.file("list.txt", text.as_bytes());
    for out in [
        run(&["--files-from", &list], b""),
        run(&["--files-from", "-"], text.as_bytes()),
    ] {
        assert!(out.status.success(), "{out:?}");
        let paths: Vec<Value> = records(&out).iter().map(|r| r["path"].clone()).collect();
        assert_eq!(paths, [a.as_str(), b.as_str()]);
    }
}

/// Each record is flushed as soon as its input is scanned: a caller that sends one path and waits for its record,
/// with the list still open, receives it.
#[test]
fn each_record_arrives_before_the_list_ends() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    let mut child = Command::new(BIN)
        .args(["--files-from", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the binary starts");
    let mut list = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        let mut first = String::new();
        let got = stdout.read_line(&mut first).map(|_| first);
        let _ = tx.send(got);
        // Drain the rest, so the child never blocks on a full pipe.
        let mut rest = String::new();
        let _ = stdout.read_to_string(&mut rest);
        rest
    });
    writeln!(list, "{a}").expect("the path is sent");
    let first = rx.recv_timeout(Duration::from_secs(30));
    // Closing the list ends the run whether or not the record arrived.
    drop(list);
    let status = child.wait().expect("the binary finishes");
    let rest = reader.join().expect("the reader thread finishes");
    let first = first
        .expect("the record arrives while the list is open")
        .expect("stdout is readable");
    let record: Value = serde_json::from_str(&first).expect("one JSON record");
    assert_eq!(record["path"], a.as_str());
    assert!(status.success());
    assert!(rest.is_empty(), "{rest:?}");
}

#[test]
fn a_source_is_read_from_standard_input() {
    let out = run(&["-"], b"\xEF\xBB\xBFstruct S;\r\nenum E { A }\r\n");
    assert!(out.status.success(), "{out:?}");
    let recs = records(&out);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["path"], "-");
    assert_eq!(
        items(&recs[0]),
        [
            item("struct", "S", "", 1, 1, None),
            item("enum", "E", "", 2, 2, None)
        ]
    );
}

#[test]
fn after_double_dash_every_argument_is_a_file() {
    let s = Scratch::new();
    let odd = s.file("-odd.rs", b"fn odd() {}\n");
    let out = run_in(Some(&s.0), &["--", "-odd.rs"], b"");
    assert!(out.status.success(), "{out:?}");
    let recs = records(&out);
    assert_eq!(recs[0]["path"], "-odd.rs");
    assert_eq!(items(&recs[0])[0].1, "odd");
    assert!(odd.ends_with("-odd.rs"));
}

/// After `--`, `-` is a file named `-`, not standard input.
#[test]
fn after_double_dash_a_dash_is_a_file_named_dash() {
    let s = Scratch::new();
    s.file("-", b"fn from_file() {}\n");
    let out = run_in(Some(&s.0), &["--", "-"], b"fn from_stdin() {}\n");
    assert!(out.status.success(), "{out:?}");
    let recs = records(&out);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["path"], "-");
    assert_eq!(items(&recs[0]), [item("fn", "from_file", "", 1, 1, None)]);
    // Before `--`, the same `-` reads standard input.
    let out = run_in(Some(&s.0), &["-"], b"fn from_stdin() {}\n");
    assert!(out.status.success(), "{out:?}");
    assert_eq!(items(&records(&out)[0])[0].1, "from_stdin");
}

/// A syntax error is a count in the record, not a failure; it takes out of the claim only the item it touches.
#[test]
fn syntax_errors_are_reported_not_fatal() {
    let s = Scratch::new();
    let bad = s.file(
        "bad.rs",
        b"fn ok() {}\nfn bad(x: Box<dyn 'a + Send>) {}\nfn also_ok() {}\n",
    );
    let out = run(&[&bad], b"");
    assert!(out.status.success(), "{out:?}");
    let recs = records(&out);
    assert!(recs[0]["errors"].as_u64().expect("errors") > 0);
    let flags: Vec<(String, bool)> = recs[0]["items"]
        .as_array()
        .expect("items is an array")
        .iter()
        .map(|i| {
            (
                i["name"].as_str().expect("name").to_owned(),
                i["ok"].as_bool().expect("ok is a boolean"),
            )
        })
        .collect();
    assert_eq!(
        flags,
        [
            ("ok".to_owned(), true),
            ("bad".to_owned(), false),
            ("also_ok".to_owned(), true)
        ]
    );
}

#[test]
fn an_unreadable_input_stops_the_run_with_status_1() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    let missing = path_str(&s.0.join("missing.rs"));
    let out = run(&[&a, &missing, &a], b"");
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    // The record written before the failure is complete; nothing after it is written.
    assert_eq!(records(&out).len(), 1);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("missing.rs"), "{err}");
}

/// A LIST that cannot be opened stops the run with status 1; the records written before it stay.
#[test]
fn a_missing_list_stops_the_run_with_status_1() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    let list = path_str(&s.0.join("no-such-list.txt"));
    let out = run(&[&a, "--files-from", &list, &a], b"");
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let recs = records(&out);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["path"], a.as_str());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("no-such-list.txt"), "{err}");
}

/// A LIST that is not UTF-8 stops the run with status 1 at the line that is not; the paths before it are scanned.
#[test]
fn a_list_that_is_not_utf8_stops_the_run_with_status_1() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    let b = s.file("b.rs", b"fn b() {}\n");
    let mut text = format!("{b}\n").into_bytes();
    text.extend_from_slice(b"\xFF\xFE\n");
    let list = s.file("list.txt", &text);
    for out in [
        run(&[&a, "--files-from", &list, &a], b""),
        run(&[&a, "--files-from", "-", &a], &text),
    ] {
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let paths: Vec<Value> = records(&out).iter().map(|r| r["path"].clone()).collect();
        assert_eq!(paths, [a.as_str(), b.as_str()]);
        assert!(!out.stderr.is_empty());
    }
    // A list of the bytes FF FE alone writes no record from the list.
    let only = s.file("only.txt", b"\xFF\xFE");
    let out = run(&["--files-from", &only], b"");
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
}

#[test]
fn usage_errors_exit_2_and_write_no_record() {
    let s = Scratch::new();
    let a = s.file("a.rs", b"fn a() {}\n");
    for args in [
        &[][..],
        &["--bogus"][..],
        &[a.as_str(), "--files-from"][..],
        &["-", "--files-from", "-"][..],
        &["-", "-"][..],
        &[a.as_str(), "--version"][..],
        &["--version", a.as_str()][..],
        &["--help", a.as_str()][..],
        &["-h", a.as_str()][..],
        &[a.as_str(), "--help"][..],
    ] {
        let out = run(args, b"");
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("usage:"),
            "{args:?}"
        );
    }
}

#[test]
fn version_prints_the_pins() {
    let out = run(&["--version"], b"");
    assert!(out.status.success(), "{out:?}");
    let recs = records(&out);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0]["oracle"], "moirai-tsoracle");
    assert_eq!(recs[0]["format"], 3);
    assert_eq!(recs[0]["tree_sitter"], "0.27.0");
    assert_eq!(recs[0]["tree_sitter_rust"], "0.24.2");
    assert!(recs[0]["language_abi"].as_u64().expect("abi") >= 13);
    // Both of the binary's units were compiled in this work tree, as this test was. A failure here means another
    // work tree sharing the target directory compiled them: `cargo clean -p moirai-tsoracle` recompiles them here.
    for key in ["manifest_dir", "bin_manifest_dir"] {
        assert_eq!(
            recs[0][key],
            env!("CARGO_MANIFEST_DIR"),
            "{key}: the binary under test was compiled in another work tree"
        );
    }
    assert_eq!(recs[0].as_object().map(serde_json::Map::len), Some(7));
}

#[test]
fn help_prints_the_usage() {
    for flag in ["--help", "-h"] {
        let out = run(&[flag], b"");
        assert!(out.status.success(), "{flag}: {out:?}");
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("usage:"));
    }
}
