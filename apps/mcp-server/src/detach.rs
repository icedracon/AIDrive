// Windows: start a background program with no inherited handles. std's
// Command always passes bInheritHandles=TRUE, so the long-running watcher
// started by `install-hooks` or by the SessionStart hook (`brief --auto`)
// kept every inheritable handle of its starter, including the pipe the
// caller reads the hook's or the installer's output from; that caller never
// saw end-of-file. Same approach as aidrive-api's detach module.
use std::{
    ffi::{OsStr, c_void},
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
}

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

/// Starts `exe args` with `flags`, no inherited handles and the current
/// environment; returns the process id.
pub fn spawn(exe: &Path, args: &[&str], flags: u32) -> io::Result<u32> {
    let mut application = wide(exe.as_os_str());
    application.push(0);
    let mut line = Vec::new();
    quote(exe.as_os_str(), &mut line);
    for arg in args {
        line.push(' ' as u16);
        quote(OsStr::new(arg), &mut line);
    }
    line.push(0);
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
    // SAFETY: every pointer refers to a live NUL-terminated buffer or a struct
    // owned by this frame for the duration of the call; bInheritHandles is 0.
    let ok = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            flags,
            ptr::null(),
            ptr::null(),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: both handles were just returned by CreateProcessW.
    unsafe {
        CloseHandle(info.thread);
        CloseHandle(info.process);
    }
    Ok(info.process_id)
}

#[cfg(test)]
mod tests {
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

    // A starter holding an inheritable pipe (as a hook runner or PowerShell
    // does) must see end-of-file while the background program keeps running.
    #[test]
    fn a_piped_starter_is_not_held_open() {
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
        // The watcher's flags: DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP.
        let pid =
            super::spawn(&ping, &["-n", "30", "127.0.0.1"], 0x0000_0008 | 0x0000_0200).unwrap();
        // SAFETY: our write end, not used again.
        unsafe { CloseHandle(write) };
        let reader = read as usize;
        let (done, eof) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut byte, mut n) = (0u8, 0u32);
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
            "the background program held the starter's pipe open"
        );
        // SAFETY: the reader thread has returned.
        unsafe { CloseHandle(read) };
    }
}
