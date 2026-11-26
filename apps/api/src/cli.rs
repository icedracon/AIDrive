// `aidrive up | down | status`: one command for a person who installed the
// release archive. `up` starts this same binary in the background as the API
// with the embedded PostgreSQL and the static web app, waits until it is
// ready, and on first use connects the clients found on PATH (Claude Code,
// Codex) to a space named after the current folder. Re-running is safe.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::{self, BufRead, IsTerminal, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const COMMANDS: &[&str] = &["up", "down", "status", "open", "uninstall"];
const DEFAULT_API_PORT: u16 = 7480;
const DEFAULT_PG_PORT: u16 = 7481;
const CLIENTS: &[&str] = &["claude", "codex", "opencode"];
// OpenCode has no hooks, so AIDrive cannot capture its sessions.
const HOOKED: &[&str] = &["claude", "codex"];

const USAGE: &str = "usage:
  aidrive up [--project DIR] [--capture|--no-capture] [--no-connect]
      (AIDRIVE_CAPTURE=on|off answers the capture question in advance)
      start AIDrive (database, API, web app) and connect Claude Code, Codex
      and OpenCode to a space for DIR (default: this folder) the first time
  aidrive status     show what runs where
  aidrive open [--no-browser]
                     open the web app signed in as you (a one-time link
                     straight to your browser; --no-browser prints it)
  aidrive down       stop AIDrive (data is kept)
  aidrive uninstall  stop AIDrive and disconnect Claude Code, Codex and
                     OpenCode; the data folder and programs stay for you to delete";

#[derive(Serialize, Deserialize, Default)]
struct Config {
    api_port: u16,
    pg_port: u16,
    #[serde(default)]
    projects: Vec<String>,
    #[serde(default)]
    hooks: Vec<String>,
}

fn root() -> PathBuf {
    crate::embedded::data_root()
}

fn config_path() -> PathBuf {
    root().join("aidrive.json")
}

fn pid_path() -> PathBuf {
    root().join("api.pid")
}

fn log_path() -> PathBuf {
    root().join("logs").join("api.log")
}

fn load() -> Option<Config> {
    serde_json::from_str(&fs::read_to_string(config_path()).ok()?).ok()
}

fn save(config: &Config) -> io::Result<()> {
    fs::create_dir_all(root())?;
    let tmp = config_path().with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(config).unwrap())?;
    fs::rename(tmp, config_path())
}

fn free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn pick(start: u16, avoid: u16) -> Option<u16> {
    (start..start.saturating_add(40)).find(|p| *p != avoid && free(*p))
}

fn web_url(config: &Config) -> String {
    format!("http://localhost:{}", config.api_port)
}

// A plain HTTP/1.0 GET so the CLI needs no HTTP client; Some(status, body).
fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .ok()?;
    let mut text = String::new();
    stream.read_to_string(&mut text).ok()?;
    let status = text.split_whitespace().nth(1)?.parse().ok()?;
    let body = text.split_once("\r\n\r\n").map(|(_, b)| b.to_owned())?;
    Some((status, body))
}

fn ours(port: u16) -> bool {
    get(port, "/healthz").is_some_and(|(s, b)| s == 200 && b.contains("aidrive-api"))
}

fn ready(port: u16) -> bool {
    get(port, "/readyz").is_some_and(|(s, _)| s == 200)
}

fn exe_dir() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

fn web_dir() -> Option<PathBuf> {
    [
        exe_dir().join("web"),
        exe_dir().join("../share/aidrive/web"),
    ]
    .into_iter()
    .find(|d| d.join("index.html").is_file())
}

fn mcp_server() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "aidrive-mcp-server.exe"
    } else {
        "aidrive-mcp-server"
    };
    let local = exe_dir().join(name);
    if local.is_file() {
        return Some(local);
    }
    on_path("aidrive-mcp-server")
}

// OpenCode's desktop app keeps its CLI off PATH, so its folders count too.
fn present(client: &str) -> bool {
    if on_path(client).is_some() {
        return true;
    }
    if client != "opencode" {
        return false;
    }
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .map(|c| c.join("opencode"));
    let appdata = env::var_os("APPDATA").map(|a| PathBuf::from(a).join("ai.opencode.desktop"));
    let local = env::var_os("LOCALAPPDATA")
        .map(|l| PathBuf::from(l).join("opencode").join("opencode-cli.exe"));
    on_path("opencode-cli").is_some()
        || [config, appdata, local]
            .into_iter()
            .flatten()
            .any(|p| p.exists())
}

fn on_path(program: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .chain([String::new()])
            .collect()
    } else {
        vec![String::new()]
    };
    env::split_paths(&env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|p| p.is_file())
    })
}

pub fn run(command: &str, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return Ok(());
    }
    match command {
        "up" => up(args),
        "down" => down(),
        "open" => open(args),
        "uninstall" => uninstall(),
        _ => status(),
    }
}

struct UpOptions {
    project: Option<PathBuf>,
    explicit_project: bool,
    capture: Option<bool>,
    connect: bool,
}

fn parse(args: &[String]) -> Result<UpOptions, String> {
    let mut o = UpOptions {
        project: None,
        explicit_project: false,
        capture: None,
        connect: true,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" => {
                o.project = Some(PathBuf::from(it.next().ok_or("--project needs a folder")?));
                o.explicit_project = true;
            }
            "--capture" => o.capture = Some(true),
            "--no-capture" => o.capture = Some(false),
            "--no-connect" => o.connect = false,
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    // Unattended installs answer the capture question in advance.
    if o.capture.is_none() {
        o.capture = match env::var("AIDRIVE_CAPTURE").as_deref() {
            Ok("on" | "1" | "yes") => Some(true),
            Ok("off" | "0" | "no") => Some(false),
            _ => None,
        };
    }
    if o.project.is_none() {
        o.project = env::current_dir().ok();
    }
    o.project = o.project.map(|p| std::path::absolute(&p).unwrap_or(p));
    Ok(o)
}

fn up(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let options = parse(args)?;
    let started = Instant::now();
    let mut config = match load() {
        Some(c) => c,
        None => {
            let api_port = pick(DEFAULT_API_PORT, 0).ok_or("no free port for the API")?;
            let pg_port = pick(DEFAULT_PG_PORT, api_port).ok_or("no free port for PostgreSQL")?;
            Config {
                api_port,
                pg_port,
                ..Config::default()
            }
        }
    };
    save(&config)?;
    if ours(config.api_port) {
        println!("AIDrive is already running.");
    } else {
        if !free(config.api_port) {
            return Err(format!(
                "port {} is used by another program; change api_port in {}",
                config.api_port,
                config_path().display()
            )
            .into());
        }
        start(&config)?;
        print!("Starting AIDrive (the first start unpacks PostgreSQL, about a minute)");
        io::stdout().flush()?;
        let deadline = Instant::now() + Duration::from_secs(300);
        while !ready(config.api_port) {
            if Instant::now() > deadline || !process_alive() {
                println!();
                return Err(
                    format!("AIDrive did not become ready; see {}", log_path().display()).into(),
                );
            }
            print!(".");
            io::stdout().flush()?;
            thread::sleep(Duration::from_millis(1000));
        }
        println!(" ready in {}s.", started.elapsed().as_secs());
    }
    if options.connect {
        connect(&mut config, &options)?;
    }
    println!(
        "\nWeb app: {}  (sign in on this computer with: aidrive open)",
        web_url(&config)
    );
    println!("Stop: aidrive down    Status: aidrive status");
    Ok(())
}

// The owner's way into the web app: aidrive-mcp-server mints a one-time
// link with this computer's device key and hands it to the browser.
fn open(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let config = load().ok_or("AIDrive is not set up yet; run: aidrive up")?;
    if !ours(config.api_port) {
        return Err("AIDrive is not running; run: aidrive up".into());
    }
    let mcp =
        mcp_server().ok_or("aidrive-mcp-server was not found next to this program or on PATH")?;
    let api = format!("http://127.0.0.1:{}", config.api_port);
    let mut forwarded = vec!["open".to_owned(), "--api".to_owned(), api];
    forwarded.extend(args.iter().filter(|a| *a == "--no-browser").cloned());
    let status = std::process::Command::new(mcp).args(&forwarded).status()?;
    if !status.success() {
        return Err("AIDrive could not open the web app; see the message above.".into());
    }
    Ok(())
}

fn start(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(log_path().parent().unwrap())?;
    let mut set: Vec<(&str, std::ffi::OsString)> = vec![
        ("AIDRIVE_DATA_DIR", root().into()),
        (
            "AIDRIVE_API_ADDR",
            format!("127.0.0.1:{}", config.api_port).into(),
        ),
        (
            "AIDRIVE_EMBEDDED_PG_PORT",
            config.pg_port.to_string().into(),
        ),
        ("AIDRIVE_LOCAL_TRUST", "1".into()),
        ("AIDRIVE_WEB_ORIGIN", web_url(config).into()),
        ("AIDRIVE_LOG_FILE", log_path().into()),
    ];
    match web_dir() {
        Some(dir) => set.push(("AIDRIVE_WEB_DIR", dir.into())),
        None => println!(
            "The web app folder (web/) was not found next to {}; only the API will run.",
            exe_dir().display()
        ),
    }
    let pid = crate::detach::spawn(
        &env::current_exe()?,
        &["serve", "--embedded-db"],
        &set,
        &["DATABASE_URL"],
    )?;
    fs::write(pid_path(), pid.to_string())?;
    Ok(())
}

// Runs an aidrive-mcp-server command and prints its output afterwards.
// install-hooks starts the watcher; on Windows a child inherits every
// inheritable handle of its parent, including pipes our own caller handed us
// (PowerShell passes all of its inheritable handles to native programs), and
// the long-running watcher would keep them open: the caller of `aidrive up`
// would never see end-of-file. So on Windows the helper starts with no
// inherited handles and writes to a file.
fn helper(exe: &Path, args: &[&str]) -> io::Result<bool> {
    #[cfg(windows)]
    {
        let logs = log_path().parent().unwrap().to_path_buf();
        fs::create_dir_all(&logs)?;
        // A watcher started by install-hooks keeps its helper file open, so
        // it is deleted on a later run instead.
        for old in fs::read_dir(&logs)?.flatten() {
            let name = old.file_name().to_string_lossy().into_owned();
            if name.starts_with("helper-") && name.ends_with(".out") {
                let _ = fs::remove_file(old.path());
            }
        }
        static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let out = logs.join(format!("helper-{}-{call}.out", std::process::id()));
        let code = crate::detach::run_to_file(exe, args, &out)?;
        if let Ok(text) = fs::read_to_string(&out) {
            print!("{text}");
        }
        let _ = fs::remove_file(&out);
        Ok(code == 0)
    }
    #[cfg(not(windows))]
    {
        Ok(Command::new(exe)
            .args(args)
            .stdin(Stdio::null())
            .status()?
            .success())
    }
}

fn pid() -> Option<u32> {
    fs::read_to_string(pid_path()).ok()?.trim().parse().ok()
}

fn process_alive() -> bool {
    let Some(pid) = pid() else { return false };
    if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
    } else {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

fn ask(question: &str) -> Option<bool> {
    if !io::stdin().is_terminal() {
        return None;
    }
    print!("{question} [Y/n] ");
    io::stdout().flush().ok()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).ok()?;
    let answer = line.trim().to_ascii_lowercase();
    Some(answer.is_empty() || answer.starts_with('y'))
}

fn home() -> Option<PathBuf> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

// A space named after the home folder or a drive root is almost never meant.
fn plausible_project(dir: &Path) -> bool {
    let same = |a: &Path, b: &Path| {
        let (a, b) = (a.display().to_string(), b.display().to_string());
        let (a, b) = (
            a.trim_end_matches(['/', '\\']),
            b.trim_end_matches(['/', '\\']),
        );
        if cfg!(windows) {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    dir.parent().is_some() && !home().is_some_and(|h| same(&h, dir))
}

fn connect(config: &mut Config, options: &UpOptions) -> Result<(), Box<dyn std::error::Error>> {
    let clients: Vec<&str> = CLIENTS.iter().copied().filter(|c| present(c)).collect();
    if clients.is_empty() {
        println!(
            "\nNo Claude Code (claude), Codex (codex) or OpenCode was found, so nothing was connected. Install one, then run `aidrive up` again in your project folder."
        );
        return Ok(());
    }
    let Some(mcp) = mcp_server() else {
        println!(
            "\naidrive-mcp-server was not found next to this program or on PATH; nothing was connected."
        );
        return Ok(());
    };
    let Some(project) = options.project.clone() else {
        return Ok(());
    };
    let key = project.display().to_string();
    if config.projects.contains(&key) && !options.explicit_project {
        println!("\n{key} is already connected.");
        return Ok(());
    }
    if !options.explicit_project && !plausible_project(&project) {
        println!(
            "\n{key} looks like your home folder, so no space was created. Run `aidrive up` inside your project folder (or `aidrive up --project DIR`)."
        );
        return Ok(());
    }
    let capture = if !clients.iter().any(|c| HOOKED.contains(c)) {
        false
    } else {
        options
            .capture
        .or_else(|| {
            ask(&format!("\nSave every Claude Code / Codex session in {key} to AIDrive automatically (kept on this computer, redacted, private to you)?"))
        })
        .unwrap_or_else(|| {
            println!("\nSession capture stays off (no terminal to ask). Switch it on later with: aidrive up --project \"{key}\" --capture");
            false
        })
    };
    let api = format!("http://127.0.0.1:{}", config.api_port);
    for client in &clients {
        let hooked = HOOKED.contains(client);
        if hooked && !config.hooks.iter().any(|h| h == client) {
            let ok = helper(&mcp, &["install-hooks", "--client", client])?;
            if ok {
                config.hooks.push(client.to_string());
            }
        }
        let mut args = vec![
            "connect",
            "--client",
            client,
            "--api",
            &api,
            "--no-browser",
            "--project",
            &key,
        ];
        if hooked {
            args.push(if capture { "--capture" } else { "--no-capture" });
        }
        println!();
        if !helper(&mcp, &args)? {
            return Err(format!("connecting {client} failed (see above)").into());
        }
    }
    if !config.projects.contains(&key) {
        config.projects.push(key);
    }
    save(config)?;
    Ok(())
}

fn pg_ctl() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "pg_ctl.exe"
    } else {
        "pg_ctl"
    };
    fs::read_dir(root().join("postgres").join("server"))
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join("bin").join(name))
        .find(|p| p.is_file())
}

fn down() -> Result<(), Box<dyn std::error::Error>> {
    let Some(config) = load() else {
        println!("AIDrive is not set up here ({}).", root().display());
        return Ok(());
    };
    if let Some(pid) = pid().filter(|_| process_alive()) {
        let pid = pid.to_string();
        if cfg!(windows) {
            Command::new("taskkill")
                .args(["/PID", &pid, "/F"])
                .stdout(Stdio::null())
                .status()?;
        } else {
            Command::new("kill").args(["-TERM", &pid]).status()?;
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while process_alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(300));
        }
    }
    // Windows has no SIGTERM for a background process, and an API that was
    // killed leaves PostgreSQL running: stop the cluster of our data folder.
    let data = root().join("postgres").join("data");
    if data.join("postmaster.pid").is_file()
        && let Some(pg_ctl) = pg_ctl()
    {
        let _ = Command::new(pg_ctl)
            .args(["stop", "-m", "fast", "-w", "-D"])
            .arg(&data)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = fs::remove_file(pid_path());
    if ours(config.api_port) {
        return Err(format!("something still answers on port {}", config.api_port).into());
    }
    println!("AIDrive stopped. Your data stays in {}.", root().display());
    Ok(())
}

fn uninstall() -> Result<(), Box<dyn std::error::Error>> {
    down()?;
    if let Some(mcp) = mcp_server() {
        for client in HOOKED {
            let _ = helper(&mcp, &["install-hooks", "--client", client, "--remove"]);
        }
        let _ = helper(&mcp, &["install-watch", "--remove"]);
    }
    let unregister: [(&str, &[&str]); 2] = [
        ("claude", &["mcp", "remove", "--scope", "user", "aidrive"]),
        ("codex", &["mcp", "remove", "aidrive"]),
    ];
    for (client, args) in unregister {
        if let Some(program) = on_path(client) {
            let ok = Command::new(program)
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                println!("Removed the aidrive MCP server from {client}.");
            }
        }
    }
    if present("opencode") {
        println!(
            "OpenCode: delete the \"aidrive\" entry under \"mcp\" in ~/.config/opencode/opencode.jsonc (or opencode.json)."
        );
    }
    if let Some(mut config) = load() {
        config.projects.clear();
        config.hooks.clear();
        save(&config)?;
    }
    println!(
        "\nAIDrive is disconnected. To remove everything, delete these folders:\n  {}  (programs)\n  {}  (your AIDrive data: spaces, files, captured sessions)\n  {}  (connection profiles)",
        exe_dir().display(),
        root().display(),
        profiles_dir().display()
    );
    Ok(())
}

// Same lookup as aidrive-mcp-server's profile folder.
fn profiles_dir() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default()
        .join("AIDrive")
}

fn status() -> Result<(), Box<dyn std::error::Error>> {
    let Some(config) = load() else {
        println!("AIDrive is not set up yet. Run: aidrive up");
        return Ok(());
    };
    let state = if ready(config.api_port) {
        "running, ready"
    } else if ours(config.api_port) {
        "running, not ready (see the log)"
    } else {
        "stopped"
    };
    let pg = if root()
        .join("postgres")
        .join("data")
        .join("postmaster.pid")
        .is_file()
    {
        "running"
    } else {
        "stopped"
    };
    println!("AIDrive: {state}");
    println!(
        "Web app and API: {}  (API also at http://127.0.0.1:{})",
        web_url(&config),
        config.api_port
    );
    println!("PostgreSQL: {pg} on 127.0.0.1:{}", config.pg_port);
    println!("Data: {}", root().display());
    println!("Log: {}", log_path().display());
    if config.projects.is_empty() {
        println!("Connected folders: none yet (run `aidrive up` in your project folder)");
    } else {
        println!("Connected folders: {}", config.projects.join(", "));
    }
    let found: Vec<&str> = CLIENTS.iter().copied().filter(|c| present(c)).collect();
    println!(
        "Clients found: {}",
        if found.is_empty() {
            "none".into()
        } else {
            found.join(", ")
        }
    );
    Ok(())
}
