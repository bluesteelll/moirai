//! The processes resident beside the night's runs ([MP §9.8]): the image names of the interactive sessions, which the
//! session record lists at the session's start and end, so that what stayed resident in the idle baseline of
//! [MP §2.3] is on record. Only image names are read: no command line, user name or host name (`tasklist` without
//! `/V`).

use crate::nightly::exec::{Cmd, Host, RealHost};
use std::path::Path;

/// How long `tasklist` may run (seconds), as [MP §2.2] gives each system program it runs.
const TIMEOUT_SECS: u64 = 30;
/// The program, below `%SystemRoot%`.
const TASKLIST: &str = "tasklist.exe";

/// The sorted, unique image names of the processes outside session 0 (the services' session) now, without `tasklist`
/// itself: `%SystemRoot%\System32\tasklist.exe /FO CSV /NH`, named by its full path as [MP §2.2] requires of system
/// programs.
// spec: [MP §9.8] (the session record: the resident processes), [MP §2.2] (system programs by their full paths)
pub fn read() -> Result<Vec<String>, String> {
    let root = std::env::var_os("SystemRoot")
        .filter(|r| !r.is_empty())
        .ok_or("%SystemRoot% is not set")?;
    let dir = Path::new(&root).join("System32");
    let cmd = Cmd::new(dir.join(TASKLIST).to_string_lossy(), &dir).args(["/FO", "CSV", "/NH"]);
    let o = RealHost::new().capture(&cmd, TIMEOUT_SECS)?;
    if o.exit != Some(0) {
        return Err(format!(
            "{} exited with {:?}: {}",
            cmd.line(),
            o.exit,
            o.stderr.trim()
        ));
    }
    parse(&o.stdout)
}

/// The quoted fields of one CSV line (`""` inside a field is one `"`); `None` when a field is not quoted or a quote
/// is not closed.
fn fields(line: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut it = line.chars().peekable();
    loop {
        if it.next()? != '"' {
            return None;
        }
        let mut f = String::new();
        loop {
            match it.next()? {
                '"' if it.peek() == Some(&'"') => {
                    it.next();
                    f.push('"');
                }
                '"' => break,
                c => f.push(c),
            }
        }
        out.push(f);
        match it.next() {
            None => return Some(out),
            Some(',') => {}
            Some(_) => return None,
        }
    }
}

/// The image names of `tasklist /FO CSV /NH`'s output whose session number is not 0, sorted and unique, without
/// `tasklist.exe`. A line is `"<image>","<PID>","<session name>","<session#>","<mem usage>"`; only the first and the
/// fourth fields are read, since the fifth is written in the display language and the console's code page. Output
/// with no line, or a line without four quoted fields, an image name or a session number, is an error; lines that
/// are all in session 0 give an empty list.
fn parse(stdout: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut lines = 0;
    for line in stdout.lines().map(|l| l.trim_end_matches('\r')) {
        if line.is_empty() {
            continue;
        }
        lines += 1;
        let bad = || {
            format!(
                "tasklist printed a line that is not \"<image>\",\"<PID>\",\"<session name>\",\"<session#>\",…: {line}"
            )
        };
        let f = fields(line).filter(|f| f.len() >= 4).ok_or_else(bad)?;
        let session: u32 = f[3].trim().parse().map_err(|_| bad())?;
        if f[0].is_empty() {
            return Err(bad());
        }
        if session != 0 && !f[0].eq_ignore_ascii_case(TASKLIST) {
            names.push(f[0].clone());
        }
    }
    if lines == 0 {
        return Err("tasklist printed no process".into());
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interactive_sessions_image_names_are_kept() {
        // Session 0 (the services) is left out, and so is tasklist itself; the rest is sorted and unique. The fifth
        // field is never read: here it holds a localized number and a character the console's code page lost.
        let out = "\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"\r\n\
                   \"svchost.exe\",\"1200\",\"Services\",\"0\",\"12,000 K\"\r\n\
                   \"explorer.exe\",\"6000\",\"Console\",\"1\",\"120 000 K\"\r\n\
                   \"agent-app.exe\",\"7000\",\"Console\",\"1\",\"300\u{fffd}000 K\"\r\n\
                   \"explorer.exe\",\"6001\",\"Console\",\"1\",\"1 K\"\r\n\
                   \"a \"\"quoted\"\", comma.exe\",\"6002\",\"RDP-Tcp#0\",\"2\",\"1 K\"\r\n\
                   \"TASKLIST.EXE\",\"8000\",\"Console\",\"1\",\"9 K\"\r\n";
        assert_eq!(
            parse(out),
            Ok(vec![
                "a \"quoted\", comma.exe".to_string(),
                "agent-app.exe".to_string(),
                "explorer.exe".to_string(),
            ])
        );
        // Every process in session 0: no resident, and no error.
        assert_eq!(
            parse("\"svchost.exe\",\"1200\",\"Services\",\"0\",\"12 K\"\n"),
            Ok(Vec::new())
        );
        for bad in [
            "",
            "\r\n\r\n",
            "INFO: No tasks are running which match the specified criteria.\r\n",
            "\"a.exe\",\"1\",\"Console\"\r\n",
            "\"a.exe\",\"1\",\"Console\",\"x\",\"1 K\"\r\n",
            "\"\",\"1\",\"Console\",\"1\",\"1 K\"\r\n",
            "\"a.exe\",\"1\",\"Console\",\"1\",\"1 K\r\n",
            "\"a.exe\",1,\"Console\",\"1\",\"1 K\"\r\n",
            "\"explorer.exe\",\"6000\",\"Console\",\"1\",\"1 K\"\r\n\"a.exe\"\r\n",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}: {:?}", parse(bad));
        }
    }

    /// The real `tasklist` of this machine: it runs, and its output parses. A host whose processes all run in session
    /// 0 (a service's job) gives an empty list. A host without `%SystemRoot%\System32\tasklist.exe` (not Windows) has
    /// no list to read, and `read` says so. The host is told apart while the test runs, by the program's file, since
    /// GT20 (d) refuses an OS-specific attribute outside `moirai-os`, tests included.
    #[test]
    fn tasklist_lists_this_machine() {
        let tasklist = std::env::var_os("SystemRoot")
            .filter(|r| !r.is_empty())
            .is_some_and(|r| Path::new(&r).join("System32").join(TASKLIST).is_file());
        if !tasklist {
            assert!(read().is_err(), "{:?}", read());
            return;
        }
        let names = read().unwrap();
        assert!(
            !names.iter().any(|n| n.eq_ignore_ascii_case(TASKLIST)),
            "{names:?}"
        );
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(names, sorted);
    }
}
