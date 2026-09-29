//! `os::spawn` on Windows: the detached, low-priority `moirai gc` child and `enter_background` ([OS/proc §11];
//! [80 §2.12] "Detached `moirai gc --rollup` child", "Bulk passes").
//!
//! The one process a product crate spawns ([60 §3.5] M4 gate); [OS/proc §11] names this module as the GT20 (a) spawn
//! lint's one allowed site. Contract: no role byte held (asserted), no inheritable handle, standard handles not set,
//! detached from the console and the process group, broken away from the parent's job where the job allows it, and
//! created below normal priority; the child's first action is [`enter_background`].

#![allow(unsafe_code)]

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use moirai_vfs::{OsCode, VfsError, VfsErrorKind};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
use windows_sys::Win32::System::Threading::{
    BELOW_NORMAL_PRIORITY_CLASS, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP,
    CreateProcessW, DETACHED_PROCESS, GetCurrentProcess, MEMORY_PRIORITY_INFORMATION,
    MEMORY_PRIORITY_LOW, PROCESS_INFORMATION, PROCESS_MODE_BACKGROUND_BEGIN, ProcessMemoryPriority,
    STARTUPINFOW, SetPriorityClass, SetProcessInformation,
};

use super::sys::{self, Domain, last_error};

/// Appends one argument quoted by the Microsoft C runtime's rules, so that the child's `argv` holds it unchanged: an
/// argument without space, tab, newline, vertical tab or `"` (and not empty) is written as it is; otherwise it is
/// enclosed in `"`, with `2n + 1` backslashes before an embedded `"`, `2n` before the closing quote, `n` elsewhere.
pub(crate) fn push_quoted(cmd: &mut Vec<u16>, arg: &str) {
    let special = |c: char| matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '"');
    if !arg.is_empty() && !arg.contains(special) {
        cmd.extend(arg.encode_utf16());
        return;
    }
    cmd.push(u16::from(b'"'));
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                cmd.extend(core::iter::repeat_n(u16::from(b'\\'), 2 * backslashes + 1));
                cmd.push(u16::from(b'"'));
                backslashes = 0;
            }
            c => {
                cmd.extend(core::iter::repeat_n(u16::from(b'\\'), backslashes));
                backslashes = 0;
                let mut b = [0u16; 2];
                cmd.extend_from_slice(c.encode_utf16(&mut b));
            }
        }
    }
    cmd.extend(core::iter::repeat_n(u16::from(b'\\'), 2 * backslashes));
    cmd.push(u16::from(b'"'));
}

/// The command line: the executable in quotes (argv\[0\] takes no escapes) followed by the quoted arguments.
pub(crate) fn command_line(exe: &[u16], args: &[&str]) -> Vec<u16> {
    let mut cmd =
        Vec::with_capacity(exe.len() + 3 + args.iter().map(|a| a.len() + 3).sum::<usize>());
    cmd.push(u16::from(b'"'));
    cmd.extend_from_slice(exe);
    cmd.push(u16::from(b'"'));
    for a in args {
        cmd.push(u16::from(b' '));
        push_quoted(&mut cmd, a);
    }
    cmd.push(0);
    cmd
}

/// Spawns the detached `gc` child and returns its pid ([OS/proc §11]): `CreateProcessW(exe, command line, NULL, NULL,
/// FALSE, BELOW_NORMAL_PRIORITY_CLASS | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB, NULL,
/// cwd, …)`; on `ERROR_ACCESS_DENIED` (a job that forbids breakaway) the same call without breakaway; both handles are
/// closed at once. Panics if any lock client of this process holds a role byte ([OS/README §5.2] item 2).
pub(crate) fn spawn_gc_child(exe: &Path, args: &[&str], cwd: &Path) -> Result<u32, VfsError> {
    assert!(
        !super::lock::holds_any_role(),
        "os::spawn: a process spawn while a role byte is held ([OS/lock §3] item 7)"
    );
    let bad = || VfsError::new(VfsErrorKind::InvalidName, OsCode::NONE, "CreateProcessW");
    let exe = std::path::absolute(exe).map_err(|_| bad())?;
    let cwd = std::path::absolute(cwd).map_err(|_| bad())?;
    let exe_w: Vec<u16> = exe.as_os_str().encode_wide().collect();
    let app = sys::with_nul(&exe_w);
    let dir = sys::wide_z(cwd.as_os_str());
    let base = BELOW_NORMAL_PRIORITY_CLASS | DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    for flags in [base | CREATE_BREAKAWAY_FROM_JOB, base] {
        let mut cmd = command_line(&exe_w, args);
        let si = STARTUPINFOW {
            cb: core::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();
        // SAFETY: `app`, `dir` are NUL-terminated; `cmd` is a writable NUL-terminated buffer (the call may modify it);
        // `si` and `pi` are live locals; no security attributes, no inherited handles, the parent's environment.
        let ok = unsafe {
            CreateProcessW(
                app.as_ptr(),
                cmd.as_mut_ptr(),
                core::ptr::null(),
                core::ptr::null(),
                0,
                flags,
                core::ptr::null(),
                dir.as_ptr(),
                &si,
                &mut pi,
            )
        };
        if ok != 0 {
            // SAFETY: both handles were just returned by `CreateProcessW` and are closed exactly once; nothing waits.
            unsafe {
                CloseHandle(pi.hThread);
                CloseHandle(pi.hProcess);
            }
            return Ok(pi.dwProcessId);
        }
        let e = last_error();
        if e != ERROR_ACCESS_DENIED || flags & CREATE_BREAKAWAY_FROM_JOB == 0 {
            return Err(sys::error(e, Domain::Store, "CreateProcessW"));
        }
    }
    Err(sys::error(
        ERROR_ACCESS_DENIED,
        Domain::Store,
        "CreateProcessW",
    ))
}

/// Lowers this process's CPU, I/O and memory priority ([OS/proc §11]): `SetPriorityClass(GetCurrentProcess(),
/// PROCESS_MODE_BACKGROUND_BEGIN)`, then `SetProcessInformation(ProcessMemoryPriority, MEMORY_PRIORITY_LOW)`. A hint:
/// failures are ignored.
pub(crate) fn enter_background() {
    // SAFETY: the pseudo-handle of the current process is always valid; the mode flag takes no other argument.
    unsafe { SetPriorityClass(GetCurrentProcess(), PROCESS_MODE_BACKGROUND_BEGIN) };
    let info = MEMORY_PRIORITY_INFORMATION {
        MemoryPriority: MEMORY_PRIORITY_LOW,
    };
    // SAFETY: `info` is a live `MEMORY_PRIORITY_INFORMATION` of the size passed.
    unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessMemoryPriority,
            (&info as *const MEMORY_PRIORITY_INFORMATION).cast(),
            core::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// The Microsoft C runtime's parsing of one quoted-or-bare argument sequence (argv\[1..\]), used as the reference.
    fn parse_args(s: &str) -> Vec<String> {
        let c: Vec<char> = s.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            while i < c.len() && (c[i] == ' ' || c[i] == '\t') {
                i += 1;
            }
            if i >= c.len() {
                return out;
            }
            let mut arg = String::new();
            let mut quoted = false;
            while i < c.len() && (quoted || !(c[i] == ' ' || c[i] == '\t')) {
                if c[i] == '\\' {
                    let mut n = 0;
                    while i < c.len() && c[i] == '\\' {
                        n += 1;
                        i += 1;
                    }
                    if i < c.len() && c[i] == '"' {
                        arg.extend(core::iter::repeat_n('\\', n / 2));
                        if n % 2 == 1 {
                            arg.push('"');
                            i += 1;
                        }
                    } else {
                        arg.extend(core::iter::repeat_n('\\', n));
                    }
                } else if c[i] == '"' {
                    if quoted && i + 1 < c.len() && c[i + 1] == '"' {
                        arg.push('"');
                        i += 2;
                    } else {
                        quoted = !quoted;
                        i += 1;
                    }
                } else {
                    arg.push(c[i]);
                    i += 1;
                }
            }
            out.push(arg);
        }
    }

    #[test]
    fn quoting_examples() {
        let q = |a: &str| {
            let mut v = Vec::new();
            push_quoted(&mut v, a);
            String::from_utf16(&v).unwrap()
        };
        assert_eq!(q("gc"), "gc");
        assert_eq!(q("--rollup"), "--rollup");
        assert_eq!(q(""), "\"\"");
        assert_eq!(q("a b"), "\"a b\"");
        assert_eq!(q("C:\\dir with space\\"), "\"C:\\dir with space\\\\\"");
        assert_eq!(q("say \"hi\""), "\"say \\\"hi\\\"\"");
        let cmd = command_line(
            &"C:\\m\\moirai.exe".encode_utf16().collect::<Vec<_>>(),
            &["gc", "--rollup"],
        );
        assert_eq!(
            String::from_utf16(&cmd[..cmd.len() - 1]).unwrap(),
            "\"C:\\m\\moirai.exe\" gc --rollup"
        );
    }

    proptest! {
        #![proptest_config(crate::windows::testing::proptest_config())]

        /// Quoting then parsing by the runtime's rules gives the arguments back.
        #[test]
        fn quoting_round_trips(args in proptest::collection::vec("[a-z\\\\\" \t]{0,8}", 0..5)) {
            let mut v: Vec<u16> = Vec::new();
            for (i, a) in args.iter().enumerate() {
                if i > 0 { v.push(u16::from(b' ')); }
                push_quoted(&mut v, a);
            }
            prop_assert_eq!(parse_args(&String::from_utf16(&v).unwrap()), args);
        }
    }
}
