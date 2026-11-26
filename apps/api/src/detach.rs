// Starts a background process that keeps nothing of its caller: no console,
// stdin/stdout/stderr on the null device and no inherited handles. Rust's
// std spawns Windows children with bInheritHandles=TRUE, so a background API
// started that way kept whatever pipe the caller of `aidrive up` held open
// (PowerShell `| Out-String`, CI, a wrapper script) and that caller never saw
// end-of-file. The API writes its own log (AIDRIVE_LOG_FILE) instead.
use std::{ffi::OsString, io, path::Path};

/// Returns the new process id.
pub fn spawn(
    exe: &Path,
    args: &[&str],
    set: &[(&str, OsString)],
    remove: &[&str],
) -> io::Result<u32> {
    imp::spawn(exe, args, set, remove)
}

/// Windows: runs `exe args` to completion with its output in `out` and no
/// handles inherited from this process; returns the exit code.
#[cfg(windows)]
pub fn run_to_file(exe: &Path, args: &[&str], out: &Path) -> io::Result<u32> {
    imp::run_to_file(exe, args, out)
}

#[cfg(unix)]
mod imp {
    use std::{
        ffi::OsString,
        io,
        os::unix::process::CommandExt,
        path::Path,
        process::{Command, Stdio},
    };

    pub fn spawn(
        exe: &Path,
        args: &[&str],
        set: &[(&str, OsString)],
        remove: &[&str],
    ) -> io::Result<u32> {
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        for key in remove {
            cmd.env_remove(key);
        }
        for (key, value) in set {
            cmd.env(key, value);
        }
        // Unix closes every descriptor but 0-2 on exec (CLOEXEC), so nothing
        // else of the caller's reaches the child.
        Ok(cmd.spawn()?.id())
    }
}

#[cfg(windows)]
mod imp {
    use std::{
        ffi::{OsStr, OsString, c_void},
        io,
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr,
    };

    type Handle = *mut c_void;

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        reserved2_len: u16,
        reserved2: *mut u8,
        std_input: Handle,
        std_output: Handle,
        std_error: Handle,
    }

    #[repr(C)]
    struct ProcessInformation {
        process: Handle,
        thread: Handle,
        process_id: u32,
        thread_id: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateProcessW(
            application: *const u16,
            command_line: *mut u16,
            process_attributes: *const c_void,
            thread_attributes: *const c_void,
            inherit_handles: i32,
            creation_flags: u32,
            environment: *const c_void,
            current_directory: *const u16,
            startup_info: *const StartupInfoW,
            process_information: *mut ProcessInformation,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn GetExitCodeProcess(handle: Handle, code: *mut u32) -> i32;
    }

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    // A hidden console of its own (not DETACHED_PROCESS): the console programs
    // the API starts (cmd, pg_ctl, postgres) share it instead of each opening
    // a visible window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().collect()
    }

    // The quoting CommandLineToArgvW expects.
    fn quote(arg: &OsStr, out: &mut Vec<u16>) {
        let text = wide(arg);
        let needs = text.is_empty()
            || text
                .iter()
                .any(|&c| c == ' ' as u16 || c == '\t' as u16 || c == '"' as u16);
        if !needs {
            out.extend(text);
            return;
        }
        out.push('"' as u16);
        let mut backslashes = 0;
        for c in text {
            if c == '\\' as u16 {
                backslashes += 1;
                continue;
            }
            let n = if c == '"' as u16 {
                backslashes * 2 + 1
            } else {
                backslashes
            };
            out.extend(std::iter::repeat_n('\\' as u16, n));
            backslashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n('\\' as u16, backslashes * 2));
        out.push('"' as u16);
    }

    pub(super) fn command_line(exe: &Path, args: &[&str]) -> Vec<u16> {
        let mut line = Vec::new();
        quote(exe.as_os_str(), &mut line);
        for arg in args {
            line.push(' ' as u16);
            quote(OsStr::new(arg), &mut line);
        }
        line.push(0);
        line
    }

    // This process's environment with the changes, sorted case-insensitively
    // as CreateProcessW expects, as a double-NUL-terminated UTF-16 block.
    fn environment(set: &[(&str, OsString)], remove: &[&str]) -> Vec<u16> {
        let mut vars: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(k, _)| {
                let k = k.to_string_lossy();
                !remove.iter().any(|r| r.eq_ignore_ascii_case(&k))
                    && !set.iter().any(|(s, _)| s.eq_ignore_ascii_case(&k))
            })
            .collect();
        vars.extend(set.iter().map(|(k, v)| (OsString::from(k), v.clone())));
        vars.sort_by_key(|(k, _)| k.to_string_lossy().to_uppercase());
        let mut block = Vec::new();
        for (k, v) in vars {
            block.extend(wide(&k));
            block.push('=' as u16);
            block.extend(wide(&v));
            block.push(0);
        }
        block.push(0);
        block
    }

    fn create(
        application: Option<&[u16]>,
        line: &mut [u16],
        flags: u32,
        env: Option<&[u16]>,
    ) -> io::Result<ProcessInformation> {
        let startup = StartupInfoW {
            cb: size_of::<StartupInfoW>() as u32,
            reserved: ptr::null_mut(),
            desktop: ptr::null_mut(),
            title: ptr::null_mut(),
            x: 0,
            y: 0,
            x_size: 0,
            y_size: 0,
            x_count_chars: 0,
            y_count_chars: 0,
            fill_attribute: 0,
            flags: 0,
            show_window: 0,
            reserved2_len: 0,
            reserved2: ptr::null_mut(),
            std_input: ptr::null_mut(),
            std_output: ptr::null_mut(),
            std_error: ptr::null_mut(),
        };
        let mut info = ProcessInformation {
            process: ptr::null_mut(),
            thread: ptr::null_mut(),
            process_id: 0,
            thread_id: 0,
        };
        // SAFETY: every pointer refers to a live, NUL-terminated buffer or a
        // struct owned by the caller for the duration of the call.
        // bInheritHandles is FALSE: the child gets none of our handles.
        let ok = unsafe {
            CreateProcessW(
                application.map_or(ptr::null(), <[u16]>::as_ptr),
                line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                flags,
                env.map_or(ptr::null(), |e| e.as_ptr().cast()),
                ptr::null(),
                &startup,
                &mut info,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    pub fn spawn(
        exe: &Path,
        args: &[&str],
        set: &[(&str, OsString)],
        remove: &[&str],
    ) -> io::Result<u32> {
        let mut application = wide(exe.as_os_str());
        application.push(0);
        let mut line = command_line(exe, args);
        let env = environment(set, remove);
        let info = create(
            Some(&application),
            &mut line,
            CREATE_NEW_PROCESS_GROUP | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
            Some(&env),
        )?;
        // SAFETY: both handles were just returned by CreateProcessW.
        unsafe {
            CloseHandle(info.thread);
            CloseHandle(info.process);
        }
        Ok(info.process_id)
    }

    // `cmd /c "<exe> <args> > <out> 2>&1"` with no inherited handles: the
    // output reaches the file through cmd's own handle, and nothing the
    // program starts in the background can hold our caller's pipes.
    pub fn run_to_file(exe: &Path, args: &[&str], out: &Path) -> io::Result<u32> {
        let mut inner = command_line(exe, args);
        inner.pop();
        let mut text = String::from_utf16_lossy(&inner);
        text.push_str(" > ");
        let mut target = Vec::new();
        quote(out.as_os_str(), &mut target);
        text.push_str(&String::from_utf16_lossy(&target));
        text.push_str(" 2>&1");
        let comspec = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        let mut application = wide(&comspec);
        application.push(0);
        let mut line = wide(OsStr::new(&format!("cmd.exe /d /s /c \"{text}\"")));
        line.push(0);
        let info = create(Some(&application), &mut line, CREATE_NO_WINDOW, None)?;
        let mut code = 1u32;
        // SAFETY: the process handle is valid until closed below.
        unsafe {
            WaitForSingleObject(info.process, u32::MAX);
            GetExitCodeProcess(info.process, &mut code);
            CloseHandle(info.thread);
            CloseHandle(info.process);
        }
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn command_line_quotes_like_argv() {
        let line = super::imp::command_line(
            std::path::Path::new(r"C:\Program Files\AIDrive\aidrive.exe"),
            &["serve", "a b", r#"say "hi""#, r"trail\"],
        );
        let text = String::from_utf16(&line[..line.len() - 1]).unwrap();
        assert_eq!(
            text,
            r#""C:\Program Files\AIDrive\aidrive.exe" serve "a b" "say \"hi\"" trail\"#
        );
    }

    #[cfg(windows)]
    #[test]
    fn run_to_file_captures_output_and_exit_code() {
        let out = std::env::temp_dir().join(format!("aidrive detach {}.txt", std::process::id()));
        let system = std::env::var_os("SystemRoot").unwrap();
        let whoami = std::path::Path::new(&system)
            .join("System32")
            .join("whoami.exe");
        assert_eq!(super::run_to_file(&whoami, &[], &out).unwrap(), 0);
        assert!(!std::fs::read_to_string(&out).unwrap().trim().is_empty());
        assert_ne!(
            super::run_to_file(&whoami, &["/no-such-flag"], &out).unwrap(),
            0
        );
        let _ = std::fs::remove_file(&out);
    }

    // The regression: a caller holding an inheritable pipe (as PowerShell and
    // CI do for a child's output) must see end-of-file once it closes its own
    // write end, even while the background process keeps running.
    #[cfg(windows)]
    #[test]
    fn a_piped_caller_is_not_held_open() {
        use std::{ffi::c_void, ptr, sync::mpsc, time::Duration};
        #[repr(C)]
        struct SecurityAttributes {
            len: u32,
            descriptor: *mut c_void,
            inherit: i32,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreatePipe(
                read: *mut *mut c_void,
                write: *mut *mut c_void,
                attributes: *const SecurityAttributes,
                size: u32,
            ) -> i32;
            fn ReadFile(
                file: *mut c_void,
                buffer: *mut u8,
                len: u32,
                read: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let attributes = SecurityAttributes {
            len: size_of::<SecurityAttributes>() as u32,
            descriptor: ptr::null_mut(),
            inherit: 1,
        };
        let (mut read, mut write): (*mut c_void, *mut c_void) = (ptr::null_mut(), ptr::null_mut());
        // SAFETY: out-pointers to locals; both ends are closed below.
        assert_ne!(
            unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) },
            0
        );
        let system = std::env::var_os("SystemRoot").unwrap();
        let ping = std::path::Path::new(&system)
            .join("System32")
            .join("PING.EXE");
        let pid = super::spawn(&ping, &["-n", "30", "127.0.0.1"], &[], &[]).unwrap();
        // SAFETY: our write end, not used again.
        unsafe { CloseHandle(write) };
        let reader = read as usize;
        let (done, eof) = mpsc::channel();
        std::thread::spawn(move || {
            let mut byte = 0u8;
            let mut n = 0u32;
            // SAFETY: the read end stays open until this call returns.
            let ok =
                unsafe { ReadFile(reader as *mut c_void, &mut byte, 1, &mut n, ptr::null_mut()) };
            let _ = done.send(ok == 0 || n == 0);
        });
        let result = eof.recv_timeout(Duration::from_secs(10));
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
        assert_eq!(
            result,
            Ok(true),
            "the background process held the caller's pipe open"
        );
        // SAFETY: the reader thread has returned.
        unsafe { CloseHandle(read) };
    }

    #[cfg(unix)]
    #[test]
    fn a_piped_caller_is_not_held_open() {
        use std::{io::Read, sync::mpsc, time::Duration};
        let (mut reader, writer) = std::io::pipe().unwrap();
        let pid = super::spawn(std::path::Path::new("/bin/sleep"), &["30"], &[], &[]).unwrap();
        drop(writer);
        let (done, eof) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1];
            let _ = done.send(reader.read(&mut buf).map(|n| n == 0).unwrap_or(true));
        });
        let result = eof.recv_timeout(Duration::from_secs(10));
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
        assert_eq!(result, Ok(true));
    }
}
