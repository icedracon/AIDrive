// `aidrive-mcp-server watch`: background capture. Every minute it looks at the
// Claude Code and Codex transcripts changed in the last two days, including
// sub-agent transcripts, and uploads what is new in each one whose folder is
// bound to a space with capture on. Hooks capture at the end of a turn; the
// watcher also catches a long turn while it runs and a session cut off before
// its hooks could fire (usage limit, crash, closed window). Uploads are
// idempotent per event, so hooks and the watcher overlap safely.
//
// One watcher per profile folder (per user, and per sandboxed APPDATA/HOME):
// it listens on a loopback port derived from that folder and answers only
// callers that send the secret in watch.key, which lives in the user's own
// profile. So another user, or a test install with its own profile, cannot
// stop or fake this user's watcher (found by the block 5 cold installs,
// 2026-11-26). It starts at logon (install-watch: Windows Startup folder,
// Linux systemd user service or XDG autostart) and from the SessionStart hook
// when it is not running.

use crate::capture::{self, discover, parse_claude, parse_codex};
use crate::connect::profile_dir;
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// Watchers before MCP 0.0.23 listened here and stopped on a bare "stop".
const LEGACY_PORT: u16 = 47821;
const RECENT: Duration = Duration::from_secs(48 * 3600);

fn log_path() -> PathBuf {
    profile_dir().with_file_name("watch.log")
}

fn log(line: &str) {
    let path = log_path();
    if fs::metadata(&path).is_ok_and(|m| m.len() > 1_000_000) {
        let _ = fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "{now} {line}");
    }
}

// Lines worth logging: something was uploaded or failed.
fn notable(status: &str) -> bool {
    !(status.starts_with("not captured")
        || status == "up to date"
        || status == "nothing to capture")
}

/// One pass over recent transcripts. A transcript is read again only when it
/// changed; a failed upload is retried on the next pass.
pub async fn pass(seen: &mut HashMap<PathBuf, SystemTime>) -> Vec<String> {
    let mut out = Vec::new();
    for client in ["claude", "codex"] {
        for file in discover(client) {
            let Ok(modified) = fs::metadata(&file).and_then(|m| m.modified()) else {
                continue;
            };
            if modified.elapsed().is_ok_and(|e| e > RECENT) || seen.get(&file) == Some(&modified) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            let parsed = if client == "claude" {
                parse_claude(&content)
            } else {
                parse_codex(&content)
            };
            let Some((t, _)) = parsed else {
                seen.insert(file, modified);
                continue;
            };
            match capture::sync(client, &t, "", "Watch").await {
                Ok(status) => {
                    if notable(&status) {
                        out.push(status);
                    }
                    seen.insert(file, modified);
                }
                Err(e) => out.push(format!("{client} {}: {e}", t.external_id)),
            }
        }
    }
    out
}

/// The port of this profile's watcher: AIDRIVE_WATCH_PORT, or a stable value
/// in 47000-48999 derived from the profile folder.
fn port() -> u16 {
    if let Some(p) = std::env::var("AIDRIVE_WATCH_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
    {
        return p;
    }
    port_for(&profile_dir().display().to_string())
}

fn port_for(profile: &str) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    for b in profile.to_lowercase().bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    47000 + (h % 2000) as u16
}

fn key_path() -> PathBuf {
    profile_dir().with_file_name("watch.key")
}

fn read_key() -> Option<String> {
    fs::read_to_string(key_path())
        .ok()
        .map(|k| k.trim().to_owned())
        .filter(|k| k.len() >= 32)
}

/// What the watcher answers to one request line "<verb> <key>".
#[derive(Debug, PartialEq)]
enum Reply {
    Pong,
    Bye,
    Refused,
}

fn handle(line: &str, key: &str) -> Reply {
    let mut parts = line.split_whitespace();
    let (verb, given) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if given.len() != key.len()
        || given
            .bytes()
            .zip(key.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            != 0
    {
        return Reply::Refused;
    }
    match verb {
        "ping" => Reply::Pong,
        "stop" => Reply::Bye,
        _ => Reply::Refused,
    }
}

fn ask(verb: &str) -> Option<String> {
    let key = read_key()?;
    let mut s =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port()).into(), Duration::from_millis(300))
            .ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    s.write_all(format!("{verb} {key}\n").as_bytes()).ok()?;
    let mut buf = [0u8; 16];
    let n = s.read(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf[..n]).trim().to_owned())
}

pub fn running() -> bool {
    ask("ping").is_some_and(|r| r == "pong")
}

pub fn stop() -> bool {
    ask("stop").is_some_and(|r| r == "bye")
}

/// Stops a watcher from before MCP 0.0.23 (bare "stop" on 47821), used once
/// when installing or removing so an update does not leave two running.
fn stop_legacy() {
    if let Ok(mut s) = TcpStream::connect_timeout(
        &([127, 0, 0, 1], LEGACY_PORT).into(),
        Duration::from_millis(300),
    ) {
        let _ = s.write_all(b"stop\n");
    }
}

/// Starts the watcher in the background when it is not running. Never fails.
pub fn ensure_running() {
    if running() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    #[cfg(windows)]
    {
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW, and
        // no inherited handles (crate::detach). Leave the hook's job object
        // when allowed, so the watcher outlives the session that started it;
        // otherwise start it inside the job.
        let flags = 0x0000_0008 | 0x0000_0200 | 0x0800_0000;
        if crate::detach::spawn(&exe, &["watch"], 0x0100_0000 | flags).is_err() {
            let _ = crate::detach::spawn(&exe, &["watch"], flags);
        }
    }
    #[cfg(not(windows))]
    {
        // Unix closes every other descriptor on exec (CLOEXEC).
        let _ = std::process::Command::new(exe)
            .arg("watch")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

pub async fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--stop") {
        println!(
            "{}",
            if stop() {
                "AIDrive watch stopped."
            } else {
                "AIDrive watch is not running."
            }
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--status") {
        println!(
            "AIDrive watch is {}. Log: {}",
            if running() { "running" } else { "not running" },
            log_path().display()
        );
        return Ok(());
    }
    let interval = args
        .iter()
        .position(|a| a == "--interval")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(15, 3600);
    let mut seen = HashMap::new();
    if args.iter().any(|a| a == "--once") {
        let lines = pass(&mut seen).await;
        for l in &lines {
            println!("{l}");
        }
        if lines.is_empty() {
            println!("Nothing new to capture.");
        }
        return Ok(());
    }
    if running() {
        return Err("AIDrive watch is already running".to_owned());
    }
    let listener = TcpListener::bind(("127.0.0.1", port())).map_err(|_| {
        format!(
            "port {} is taken; set AIDRIVE_WATCH_PORT to another free port",
            port()
        )
    })?;
    let key = match read_key() {
        Some(k) => k,
        None => {
            let k = crate::connect::random_hex(32)?;
            crate::connect::write_private(&key_path(), &k)
                .map_err(|e| format!("cannot write watch.key: {e}"))?;
            k
        }
    };
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 128];
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let Ok(n) = stream.read(&mut buf) else {
                continue;
            };
            match handle(&String::from_utf8_lossy(&buf[..n]), &key) {
                Reply::Pong => {
                    let _ = stream.write_all(b"pong\n");
                }
                Reply::Bye => {
                    let _ = stream.write_all(b"bye\n");
                    log("stopped");
                    std::process::exit(0);
                }
                Reply::Refused => {
                    let _ = stream.write_all(b"no\n");
                }
            }
        }
    });
    log(&format!("started, every {interval}s"));
    let mut last_index: Option<std::time::Instant> = None;
    loop {
        for line in pass(&mut seen).await {
            log(&line);
        }
        if last_index.is_none_or(|t| t.elapsed() >= Duration::from_secs(3600)) {
            last_index = Some(std::time::Instant::now());
            for line in index_build_dirs().await {
                log(&line);
            }
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
    }
}

/// Once an hour: register build and package folders found in folders bound
/// to a space (with capture on), with their measured size, as "refresh"
/// entries: a known folder only updates its size, so review dates and
/// decisions stay. New folders get a 7-day review date. Nothing is deleted.
pub async fn index_build_dirs() -> Vec<String> {
    use crate::connect::{capture_allowed, load_bindings, load_consent, load_profile};
    let bindings = load_bindings();
    let consent = load_consent();
    let mut targets: Vec<(String, String)> = Vec::new();
    for (client, folders) in &bindings {
        for (folder, profile) in folders {
            if capture_allowed(&consent, client, folder)
                && !targets.iter().any(|(f, _)| f == folder)
            {
                targets.push((folder.clone(), profile.clone()));
            }
        }
    }
    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(h) => h,
        Err(_) => return vec![],
    };
    let host = crate::index::host();
    let mut out = Vec::new();
    for (folder, name) in targets {
        let Ok(profile) = load_profile(&name) else {
            continue;
        };
        let root = PathBuf::from(&folder);
        let found = tokio::task::spawn_blocking(move || {
            crate::disk::find_build_dirs(&root, 6)
                .into_iter()
                .map(|(p, k, purpose)| {
                    let (size, _, _) = crate::disk::size_of(&p, 3_000_000);
                    (p, k, purpose, size)
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let (mut ok, mut total) = (0, 0u64);
        for (path, kind, purpose, size) in &found {
            let (abs, worktree, branch) = crate::index::locate(&path.display().to_string());
            let body = serde_json::json!({
                "path": abs, "host": host, "worktree": worktree, "branch": branch, "purpose": purpose,
                "keep_days": 7, "kind": kind, "size_bytes": size, "session": "aidrive watch", "refresh": true,
            });
            let sent = http
                .post(format!(
                    "{}/agent/workspaces/{}/artifacts",
                    profile.api_url, profile.workspace_id
                ))
                .bearer_auth(&profile.token)
                .json(&body)
                .send()
                .await;
            if sent.is_ok_and(|r| r.status().is_success()) {
                ok += 1;
                total += size;
            }
        }
        if !found.is_empty() {
            out.push(format!(
                "indexed {ok}/{} build or package folder(s) under {folder} ({}) into \"{}\"",
                found.len(),
                crate::disk::human(total),
                profile.workspace_name
            ));
        }
    }
    out
}

fn startup_script() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| {
        PathBuf::from(a)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
            .join("aidrive-watch.vbs")
    })
}

// `install-watch [--remove]`: start the watcher at logon (Windows Startup
// folder, hidden window) and now.
// Silent: no dialog when the binary is missing or mid-update.
fn startup_vbs(exe: &str) -> String {
    format!(
        "On Error Resume Next\r\nexe = \"{exe}\"\r\nIf CreateObject(\"Scripting.FileSystemObject\").FileExists(exe) Then CreateObject(\"WScript.Shell\").Run \"\"\"\" & exe & \"\"\" watch\", 0, False\r\n"
    )
}

pub fn install(args: &[String]) -> Result<(), String> {
    let remove = args.iter().any(|a| a == "--remove");
    let script = startup_script().filter(|_| cfg!(windows));
    if remove {
        if let Some(s) = &script {
            let _ = fs::remove_file(s);
        }
        #[cfg(not(windows))]
        linux_autostart(None);
        stop();
        stop_legacy();
        println!(
            "AIDrive watch removed from logon and stopped. Hooks still capture at the end of each turn."
        );
        return Ok(());
    }
    // Point logon at the installed binary, never at a build output that a
    // rebuild replaces (that left a dialog at logon, 2026-11-24).
    let installed = std::env::var_os("LOCALAPPDATA")
        .map(|d| {
            PathBuf::from(d)
                .join("AIDrive")
                .join("bin")
                .join("aidrive-mcp-server.exe")
        })
        .filter(|p| p.exists());
    let exe = match installed {
        Some(p) => p,
        None => std::env::current_exe().map_err(|e| e.to_string())?,
    };
    match script {
        Some(s) => {
            let body = startup_vbs(&exe.display().to_string());
            if let Some(parent) = s.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
            fs::write(&s, body).map_err(|e| format!("cannot write {}: {e}", s.display()))?;
            println!("AIDrive watch starts at logon ({}).", s.display());
        }
        None => {
            #[cfg(not(windows))]
            println!("{}", linux_autostart(Some(&exe)));
            #[cfg(windows)]
            println!(
                "Add \"{} watch\" to your login items to start it at logon.",
                exe.display()
            );
        }
    }
    stop_legacy();
    ensure_running();
    println!(
        "It captures every bound session, sub-agents included, once a minute. Status: aidrive-mcp-server watch --status; log: {}",
        log_path().display()
    );
    Ok(())
}

// Used by linux_autostart (not built on Windows) and the tests.
#[cfg_attr(windows, allow(dead_code))]
fn systemd_unit(exe: &str) -> String {
    format!(
        "[Unit]\nDescription=AIDrive background capture\n\n[Service]\nExecStart=\"{exe}\" watch\nRestart=on-failure\nRestartSec=30\n\n[Install]\nWantedBy=default.target\n"
    )
}

// Used by linux_autostart (not built on Windows) and the tests.
#[cfg_attr(windows, allow(dead_code))]
fn xdg_desktop(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=AIDrive watch\nExec=\"{exe}\" watch\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
    )
}

/// Linux logon start: a systemd user service when systemd is available,
/// otherwise an XDG autostart entry. None removes both.
#[cfg(not(windows))]
fn linux_autostart(exe: Option<&std::path::Path>) -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let unit = config
        .join("systemd")
        .join("user")
        .join("aidrive-watch.service");
    let desktop = config.join("autostart").join("aidrive-watch.desktop");
    let systemctl = |args: &[&str]| {
        std::process::Command::new("systemctl")
            .arg("--user")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let Some(exe) = exe else {
        if unit.exists() {
            systemctl(&["disable", "aidrive-watch.service"]);
            let _ = fs::remove_file(&unit);
            systemctl(&["daemon-reload"]);
        }
        let _ = fs::remove_file(&desktop);
        return String::new();
    };
    let exe = exe.display().to_string();
    if let Some(dir) = unit.parent()
        && fs::create_dir_all(dir).is_ok()
        && fs::write(&unit, systemd_unit(&exe)).is_ok()
        && systemctl(&["daemon-reload"])
        && systemctl(&["enable", "aidrive-watch.service"])
    {
        return format!(
            "AIDrive watch starts at logon (systemd user service {}).",
            unit.display()
        );
    }
    let _ = fs::remove_file(&unit);
    match desktop.parent().map(fs::create_dir_all) {
        Some(Ok(())) if fs::write(&desktop, xdg_desktop(&exe)).is_ok() => {
            format!(
                "AIDrive watch starts at desktop logon ({}).",
                desktop.display()
            )
        }
        _ => format!("Add \"{exe} watch\" to your login items to start it at logon."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_port_is_per_profile_and_in_range() {
        let a = port_for("C:\\Users\\USER\\AppData\\Roaming\\AIDrive\\profiles");
        let b = port_for("C:\\Users\\USER\\AppData\\Roaming\\AIDrive\\profiles-other");
        assert!((47000..49000).contains(&a) && (47000..49000).contains(&b));
        assert_ne!(a, b);
        assert_eq!(
            a,
            port_for("c:\\users\\USER\\appdata\\roaming\\aidrive\\profiles"),
            "case-insensitive"
        );
    }

    #[test]
    fn only_the_key_holder_can_ping_or_stop() {
        let key = "a".repeat(64);
        assert_eq!(handle(&format!("ping {key}\n"), &key), Reply::Pong);
        assert_eq!(handle(&format!("stop {key}"), &key), Reply::Bye);
        assert_eq!(
            handle("stop\n", &key),
            Reply::Refused,
            "a bare stop (the old protocol) is refused"
        );
        assert_eq!(
            handle(&format!("stop {}", "b".repeat(64)), &key),
            Reply::Refused
        );
        assert_eq!(handle(&format!("halt {key}"), &key), Reply::Refused);
    }

    #[test]
    fn linux_logon_entries_run_the_watcher() {
        assert!(
            systemd_unit("/opt/aidrive/aidrive-mcp-server")
                .contains("ExecStart=\"/opt/aidrive/aidrive-mcp-server\" watch")
        );
        assert!(xdg_desktop("/opt/a").contains("Exec=\"/opt/a\" watch"));
    }

    #[test]
    fn logon_script_is_silent_and_checks_the_binary() {
        let v = startup_vbs("C:\\A\\aidrive-mcp-server.exe");
        assert!(v.starts_with("On Error Resume Next\r\n"));
        assert!(v.contains("FileExists(exe) Then"));
        assert!(v.contains("Run \"\"\"\" & exe & \"\"\" watch\", 0, False"));
    }

    #[test]
    fn only_uploads_and_failures_are_logged() {
        assert!(!notable("up to date"));
        assert!(!notable("nothing to capture"));
        assert!(!notable(
            "not captured: C:\\x is not inside a folder bound to an AIDrive space"
        ));
        assert!(notable(
            "captured 3 new event(s) of session s into \"A\" (3 stored)"
        ));
    }
}
