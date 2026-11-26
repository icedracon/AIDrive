// `aidrive-mcp-server connect`: pair this machine's assistant with an
// AIDrive space without anyone copying a key. The secret is generated here
// and only its SHA-256 is sent; the owner approves a short code in the web
// app; the key is stored in a per-user profile file and the client is
// registered with its own `mcp add` command, pointing at `serve --profile`.
// When the local API runs with AIDRIVE_LOCAL_TRUST=1 the command needs no
// one: this computer claims an owner device key once (device.json) and uses
// it to create the space and issue the assistant's own key.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub api_url: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub scope: String,
    pub token: String,
}

#[derive(Serialize, Deserialize)]
struct PendingPairing {
    pairing_id: String,
    token: String,
    api_url: String,
    started_at: u64,
}

#[derive(Serialize, Deserialize)]
struct Device {
    api_url: String,
    token: String,
    owner_email: String,
}

/// Assistants AIDrive connects; each has its own profiles, bindings and key.
pub const CLIENTS: [&str; 3] = ["claude", "codex", "opencode"];

pub fn is_client(name: &str) -> bool {
    CLIENTS.contains(&name)
}

/// The client a profile belongs to: `opencode-bytetoflag` -> `opencode`.
pub fn client_of(profile: &str) -> &str {
    profile.split('-').next().unwrap_or(profile)
}

fn display_name(client: &str) -> &'static str {
    match client {
        "claude" => "Claude Code",
        "opencode" => "OpenCode",
        _ => "Codex",
    }
}

/// Clients whose sessions AIDrive captures through their hooks. OpenCode
/// exposes plugin events, but the API stores only Claude Code and Codex
/// sessions, so OpenCode keeps state with session notes and handoffs.
pub fn captures(client: &str) -> bool {
    matches!(client, "claude" | "codex")
}

pub const OPENCODE_NOT_CAPTURED: &str = "OpenCode sessions are not captured automatically yet: before stopping, the assistant writes a session note (create_file, session-<topic>-<date>.md) and calls record_handoff citing it.";

const CLIENT_ERROR: &str = "--client must be claude, codex or opencode";

pub fn profile_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("AIDrive").join("profiles")
}

fn valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

// Claude desktop is an MSIX-packaged app: what it and every program it starts
// (WSL included) write to %APPDATA%\AIDrive lands in
// %LOCALAPPDATA%\Packages\<package>\LocalCache\Roaming\AIDrive, which
// programs outside the package (OpenCode, the Codex app) never see (observed
// 2026-11-25). So settings are read from the own folder first and then from
// Claude desktop's packaged copy; the own folder wins wherever both have an
// entry. Only packages signed by Anthropic count (SECURITY-REVIEW-2026-11
// N3): any MSIX app, a sandboxed Store app included, can write its own
// LocalCache, and a profile or binding planted there would send this
// computer's sessions to a server of its choosing.
fn packaged_profile_dirs() -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    claude_package_dirs(Path::new(&local))
        .into_iter()
        .map(|p| {
            p.join("LocalCache")
                .join("Roaming")
                .join("AIDrive")
                .join("profiles")
        })
        .filter(|p| p.is_dir())
        .collect()
}

/// Anthropic's MSIX publisher id, the suffix of every package family it signs
/// (`Claude_pzs8sxrjxfjjc`). Another publisher cannot get it without
/// Anthropic's signing certificate.
pub const CLAUDE_PUBLISHER_ID: &str = "pzs8sxrjxfjjc";

/// `%LOCALAPPDATA%\Packages` folders of packages signed by Anthropic.
pub fn claude_package_dirs(local_appdata: &Path) -> Vec<PathBuf> {
    let suffix = format!("_{CLAUDE_PUBLISHER_ID}");
    fs::read_dir(local_appdata.join("Packages"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.to_ascii_lowercase().ends_with(&suffix))
        })
        .map(|e| e.path())
        .collect()
}

fn profile_dirs() -> Vec<PathBuf> {
    let own = profile_dir();
    let mut dirs = vec![own.clone()];
    dirs.extend(packaged_profile_dirs().into_iter().filter(|d| *d != own));
    dirs
}

fn read_layers<T: serde::de::DeserializeOwned>(file: &str) -> Vec<T> {
    profile_dirs()
        .iter()
        .filter_map(|d| fs::read_to_string(d.join(file)).ok())
        .filter_map(|t| serde_json::from_str(&t).ok())
        .collect()
}

/// client -> folder -> value maps, earlier layers winning per folder.
pub fn merge_layers<V>(
    layers: Vec<BTreeMap<String, BTreeMap<String, V>>>,
) -> BTreeMap<String, BTreeMap<String, V>> {
    let mut out: BTreeMap<String, BTreeMap<String, V>> = BTreeMap::new();
    for layer in layers {
        for (client, folders) in layer {
            let entry = out.entry(client).or_default();
            for (folder, value) in folders {
                entry.entry(folder).or_insert(value);
            }
        }
    }
    out
}

pub fn load_profile(name: &str) -> Result<Profile, String> {
    if !valid_profile_name(name) {
        return Err("Profile names use letters, digits, - and _ only".into());
    }
    let text = profile_dirs()
        .iter()
        .find_map(|d| fs::read_to_string(d.join(format!("{name}.json"))).ok())
        .ok_or_else(|| {
        format!(
            "AIDrive profile '{name}' not found. Run: aidrive-mcp-server connect --client {name}"
        )
    })?;
    serde_json::from_str(&text)
        .map_err(|_| format!("AIDrive profile '{name}' is unreadable; run connect again"))
}

// Project bindings: client -> folder -> profile name. A `serve --profile
// CLIENT` process uses the profile bound to the nearest enclosing folder of its
// working directory, so one computer can keep Claude or Codex connected to a
// different space per project instead of one space per client.
pub type Bindings = BTreeMap<String, BTreeMap<String, String>>;

fn bindings_path() -> PathBuf {
    profile_dir().join("bindings.json")
}

pub fn load_bindings() -> Bindings {
    merge_layers(read_layers("bindings.json"))
}

fn save_bindings(bindings: &Bindings) -> Result<(), String> {
    let path = bindings_path();
    write_private(&path, &serde_json::to_string_pretty(bindings).unwrap())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

// Capture consent: client -> folder -> on/off, set per project by
// `connect --capture` / `--no-capture` or `capture-consent`. The hooks are
// installed per user, so this list is what decides which projects' sessions
// leave the machine. Before this list existed, binding a folder was the
// consent, so a missing file starts with every bound folder switched on.
pub type Consent = BTreeMap<String, BTreeMap<String, bool>>;

fn consent_path() -> PathBuf {
    profile_dir().join("capture-consent.json")
}

pub fn load_consent() -> Consent {
    if consent_path().is_file() {
        return merge_layers(read_layers("capture-consent.json"));
    }
    let c = consent_from_bindings(&load_bindings());
    let _ = save_consent(&c);
    c
}

pub fn consent_from_bindings(bindings: &Bindings) -> Consent {
    bindings
        .iter()
        .map(|(client, folders)| {
            (
                client.clone(),
                folders.keys().map(|f| (f.clone(), true)).collect(),
            )
        })
        .collect()
}

fn save_consent(c: &Consent) -> Result<(), String> {
    let path = consent_path();
    write_private(&path, &serde_json::to_string_pretty(c).unwrap())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Capture is allowed for the bound folder only when it was switched on.
pub fn capture_allowed(c: &Consent, client: &str, bound_folder: &str) -> bool {
    c.get(client)
        .and_then(|f| f.get(bound_folder))
        .copied()
        .unwrap_or(false)
}

pub const RETENTION: &str = "Captured sessions stay in the space until someone deletes them (web app: Sessions, Delete); they are private to you unless you share them. Only what you and the assistant said and short tool notes are sent, after local redaction; tool output, model reasoning and system instructions stay on this computer.";

pub fn set_consent(client: &str, folder: &str, on: bool) -> Result<(), String> {
    let mut c = load_consent();
    c.entry(client.to_owned())
        .or_default()
        .insert(folder.to_owned(), on);
    save_consent(&c)
}

/// `aidrive-mcp-server capture-consent --client CLIENT [--project DIR] on|off`.
pub fn consent_command(args: &[String]) -> Result<(), String> {
    let (mut client, mut project, mut state) = (None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--client" => client = it.next().cloned(),
            "--project" => project = it.next().map(PathBuf::from),
            "on" => state = Some(true),
            "off" => state = Some(false),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let usage = "usage: aidrive-mcp-server capture-consent --client claude|codex|opencode [--project DIR] on|off";
    let (client, on) = (client.ok_or(usage)?, state.ok_or(usage)?);
    if !is_client(&client) {
        return Err(CLIENT_ERROR.into());
    }
    if !captures(&client) {
        return Err(format!(
            "Not changed: capture-consent does not apply to {client}. {OPENCODE_NOT_CAPTURED}"
        ));
    }
    let project = match project {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let (_, folder) = resolve(&client, &project, &load_bindings());
    let folder =
        folder.ok_or("This folder is not bound to an AIDrive space; run connect first.")?;
    set_consent(&client, &folder, on)?;
    if on {
        println!("Automatic capture is on for {client} sessions in {folder}.\n{RETENTION}");
    } else {
        println!(
            "Automatic capture is off for {client} sessions in {folder}. Already captured sessions stay until deleted."
        );
    }
    Ok(())
}

// Windows paths compare case-insensitively and may carry the \\?\ prefix.
pub fn folder_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let text = text.trim_end_matches(['\\', '/']);
    if cfg!(windows) {
        text.replace('/', "\\").to_lowercase()
    } else {
        text.to_owned()
    }
}

/// The profile used from `cwd`: the one bound to the nearest enclosing
/// folder for this client, otherwise the client's own default profile.
pub fn resolve(client: &str, cwd: &Path, bindings: &Bindings) -> (String, Option<String>) {
    if let Some(folders) = bindings.get(client) {
        for dir in cwd.ancestors() {
            let key = folder_key(dir);
            if let Some(name) = folders.get(&key) {
                return (name.clone(), Some(key));
            }
        }
    }
    (client.to_owned(), None)
}

/// What `serve --profile NAME` uses. A client name follows the folder
/// bindings; any other name is an explicit profile, used as given, and the
/// folder returned is only the one its client binds `cwd` to (for capture).
pub fn serve_profile(name: &str, cwd: &Path, bindings: &Bindings) -> (String, Option<String>) {
    if is_client(name) {
        return resolve(name, cwd, bindings);
    }
    (name.to_owned(), resolve(client_of(name), cwd, bindings).1)
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-')
        .chars()
        .take(24)
        .collect::<String>()
        .trim_end_matches('-')
        .to_owned()
}

/// Profile name for a newly connected space: the client's default name when
/// there is no default yet or it already points at this space, otherwise
/// `CLIENT-SPACE` (plus part of the space id if that name is taken), so a
/// second project never re-binds the first.
fn profile_name_for(
    client: &str,
    space: &str,
    workspace_id: &str,
    existing: impl Fn(&str) -> Option<String>,
) -> String {
    match existing(client) {
        None => return client.to_owned(),
        Some(id) if id == workspace_id => return client.to_owned(),
        Some(_) => {}
    }
    let id8: String = workspace_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let base = match slug(space) {
        s if s.is_empty() => format!("{client}-{id8}"),
        s => format!("{client}-{s}"),
    };
    match existing(&base) {
        None => base,
        Some(id) if id == workspace_id => base,
        Some(_) => format!("{base}-{id8}"),
    }
}

/// `aidrive-mcp-server bind --client CLIENT --profile NAME [--project DIR]`.
pub fn bind(args: &[String]) -> Result<(), String> {
    let (mut client, mut name, mut project) = (None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--client" => client = it.next().cloned(),
            "--profile" => name = it.next().cloned(),
            "--project" => project = it.next().map(PathBuf::from),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let usage = "usage: aidrive-mcp-server bind --client claude|codex|opencode --profile NAME [--project DIR]";
    let (client, name) = (client.ok_or(usage)?, name.ok_or(usage)?);
    if !is_client(&client) {
        return Err(CLIENT_ERROR.into());
    }
    let project = match project {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    println!("{}", bind_folder(&client, &name, &project)?);
    Ok(())
}

/// Binds `project` to profile `name` for `client` and switches automatic
/// capture on for it: binding a folder has always been the consent to capture
/// it, and a bound folder missing from the consent list would otherwise be
/// skipped silently.
pub fn bind_folder(client: &str, name: &str, project: &Path) -> Result<String, String> {
    if !is_client(client) {
        return Err("client must be claude, codex or opencode".into());
    }
    let profile = load_profile(name)?;
    let mut bindings = load_bindings();
    let key = folder_key(project);
    bindings
        .entry(client.to_owned())
        .or_default()
        .insert(key.clone(), name.to_owned());
    save_bindings(&bindings)?;
    if !captures(client) {
        return Ok(format!(
            "{client} sessions started in {key} now use profile \"{name}\" (space \"{}\").\n{OPENCODE_NOT_CAPTURED}",
            profile.workspace_name
        ));
    }
    set_consent(client, &key, true)?;
    Ok(format!(
        "{client} sessions started in {key} now use profile \"{name}\" (space \"{}\") and are captured automatically after every turn.\n{RETENTION}\nSwitch capture off with: aidrive-mcp-server capture-consent --client {client} --project \"{key}\" off",
        profile.workspace_name
    ))
}

/// `aidrive-mcp-server profiles`: profiles and bindings, never keys.
pub fn list() -> Result<(), String> {
    let mut names: Vec<String> = profile_dirs()
        .iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|f| f.strip_suffix(".json").map(str::to_owned))
        .filter(|n| valid_profile_name(n) && n != "bindings")
        .collect();
    if names.is_empty() {
        return Err(
            "No AIDrive profiles yet. Run: aidrive-mcp-server connect --client claude".to_owned(),
        );
    }
    names.sort();
    names.dedup();
    println!("Profiles:");
    for name in &names {
        match load_profile(name) {
            Ok(p) => println!(
                "  {name}: space \"{}\" ({}), {}",
                p.workspace_name, p.scope, p.api_url
            ),
            Err(_) => println!("  {name}: not a connected profile"),
        }
    }
    println!("Project bindings:");
    let bindings = load_bindings();
    if bindings.values().all(BTreeMap::is_empty) {
        println!("  none (every session uses the client's default profile)");
    }
    for (client, folders) in &bindings {
        for (folder, name) in folders {
            println!("  {client} in {folder} -> {name}");
        }
    }
    Ok(())
}

pub(crate) fn write_private(path: &PathBuf, text: &str) -> std::io::Result<()> {
    fs::create_dir_all(path.parent().expect("profile path has a parent"))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, path)
}

pub(crate) fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|_| "No secure random source available".to_owned())?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

pub(crate) fn uuid_v4() -> Result<String, String> {
    let h = random_hex(16)?;
    let mut b: Vec<char> = h.chars().collect();
    b[12] = '4';
    b[16] = ['8', '9', 'a', 'b'][(b[16].to_digit(16).unwrap_or(0) % 4) as usize];
    let h: String = b.into_iter().collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    ))
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn open_browser(url: &str) {
    let status = if cfg!(windows) {
        Command::new("cmd").args(["/C", "start", "", url]).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else {
        Command::new("xdg-open").arg(url).status()
    };
    if status.is_err() {
        println!("Could not open a browser automatically.");
    }
}

// npm installs `claude`/`codex` as .cmd shims on Windows, which only cmd runs.
fn tool(program: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    if cfg!(windows) {
        let mut all = vec!["/C", program];
        all.extend_from_slice(args);
        Command::new("cmd").args(all).output()
    } else {
        Command::new(program).args(args).output()
    }
}

fn register(client: &str) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.to_string_lossy().to_string();
    let (program, remove, add): (&str, Vec<&str>, Vec<&str>) = match client {
        "claude" => (
            "claude",
            vec!["mcp", "remove", "--scope", "user", "aidrive"],
            vec![
                "mcp",
                "add",
                "--scope",
                "user",
                "--transport",
                "stdio",
                "aidrive",
                "--",
                &exe,
                "serve",
                "--profile",
                "claude",
            ],
        ),
        "codex" => (
            "codex",
            vec!["mcp", "remove", "aidrive"],
            vec![
                "mcp",
                "add",
                "aidrive",
                "--",
                &exe,
                "serve",
                "--profile",
                "codex",
            ],
        ),
        "opencode" => return crate::opencode::register(&exe),
        _ => return Err("unsupported client".into()),
    };
    let _ = tool(program, &remove);
    let output = tool(program, &add).map_err(|_| format!("`{program}` was not found on PATH"))?;
    if !output.status.success() {
        return Err(format!(
            "`{program} mcp add` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!(
        "{program} mcp add aidrive -- {exe} serve --profile {client}"
    ))
}

pub struct Options {
    pub client: String,
    pub name: String,
    pub api_url: String,
    pub register: bool,
    pub browser: bool,
    pub space: String,
    pub local: bool,
    pub project: PathBuf,
    // Some(true): capture this project's sessions automatically (and install
    // the client's hooks); Some(false): switch it off; None: leave as is.
    pub capture: Option<bool>,
}

fn device_path() -> PathBuf {
    profile_dir().with_file_name("device.json")
}

fn default_space(project: &Path) -> String {
    project
        .file_name()
        .map(|n| n.to_string_lossy().trim().to_owned())
        .filter(|n| !n.is_empty() && n.chars().count() <= 120)
        .unwrap_or_else(|| "My project".into())
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .map(|n| {
            n.trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(60)
                .collect::<String>()
        })
        .filter(|n| !n.is_empty())
        .map(|n| format!("Computer {n}"))
        .unwrap_or_else(|| "This computer".into())
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut client = None;
    let mut name = None;
    let mut api_url =
        std::env::var("AIDRIVE_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let mut register = true;
    let mut browser = true;
    let mut space = None;
    let mut local = true;
    let mut project = None;
    let mut capture = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--capture" => capture = Some(true),
            "--no-capture" => capture = Some(false),
            "--client" => client = it.next().cloned(),
            "--name" => name = it.next().cloned(),
            "--api" => api_url = it.next().cloned().ok_or("--api needs a URL")?,
            "--no-register" => register = false,
            "--no-browser" => browser = false,
            "--space" => space = Some(it.next().cloned().ok_or("--space needs a name")?),
            "--browser-approval" => local = false,
            "--project" => {
                project = Some(PathBuf::from(it.next().ok_or("--project needs a folder")?))
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let client = client.ok_or("usage: aidrive-mcp-server connect --client claude|codex|opencode [--space NAME] [--project DIR] [--name NAME] [--api URL] [--no-register] [--no-browser] [--browser-approval] [--capture|--no-capture]")?;
    let project = match project {
        Some(p) => p,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    if !is_client(&client) {
        return Err(CLIENT_ERROR.into());
    }
    if capture == Some(true) && !captures(&client) {
        return Err(format!(
            "Not connected: --capture is not available for {client}; run the same command without --capture. {OPENCODE_NOT_CAPTURED}"
        ));
    }
    let name = name.unwrap_or_else(|| display_name(&client).into());
    Ok(Options {
        client,
        name,
        api_url: api_url.trim_end_matches('/').to_owned(),
        register,
        browser,
        space: space
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default_space(&project)),
        local,
        project,
        capture,
    })
}

fn load_device(api_url: &str) -> Option<Device> {
    fs::read_to_string(device_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Device>(&t).ok())
        .filter(|d| d.api_url == api_url)
}

// Only a local API with AIDRIVE_LOCAL_TRUST=1 answers 201; anything else
// means "ask the owner in the browser instead".
async fn claim_device(http: &reqwest::Client, api_url: &str) -> Result<Option<Device>, String> {
    let token = format!("aidrive_device_{}", random_hex(32)?);
    let Ok(response) = http
        .post(format!("{api_url}/agent/local/claim"))
        .json(&serde_json::json!({
            "token_sha256": sha256_hex(&token),
            "label": computer_name(),
        }))
        .send()
        .await
    else {
        return Err(format!("AIDrive API is unreachable at {api_url}"));
    };
    if response.status() != reqwest::StatusCode::CREATED {
        return Ok(None);
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "Unexpected response from AIDrive".to_owned())?;
    let device = Device {
        api_url: api_url.to_owned(),
        token,
        owner_email: body["owner_email"].as_str().unwrap_or_default().to_owned(),
    };
    let path = device_path();
    write_private(&path, &serde_json::to_string_pretty(&device).unwrap())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(Some(device))
}

async fn connect_locally(
    http: &reqwest::Client,
    options: &Options,
) -> Result<Option<(Profile, String)>, String> {
    let device = match load_device(&options.api_url) {
        Some(d) => d,
        None => match claim_device(http, &options.api_url).await? {
            Some(d) => d,
            None => return Ok(None),
        },
    };
    let token = format!("aidrive_agent_{}", random_hex(32)?);
    let response = http
        .post(format!("{}/agent/local/connect", options.api_url))
        .bearer_auth(&device.token)
        .json(&serde_json::json!({
            "client_name": options.name,
            "token_sha256": sha256_hex(&token),
            "workspace_name": options.space,
        }))
        .send()
        .await
        .map_err(|_| format!("AIDrive API is unreachable at {}", options.api_url))?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let _ = fs::rename(device_path(), device_path().with_extension("revoked.json"));
        return Ok(None);
    }
    if response.status() != reqwest::StatusCode::CREATED {
        return Ok(None);
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "Unexpected response from AIDrive".to_owned())?;
    let profile = Profile {
        api_url: options.api_url.clone(),
        workspace_id: body["workspace_id"].as_str().unwrap_or_default().to_owned(),
        workspace_name: body["workspace_name"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        scope: body["scope"].as_str().unwrap_or_default().to_owned(),
        token,
    };
    let verb = if body["created_workspace"].as_bool() == Some(true) {
        "created"
    } else {
        "joined"
    };
    Ok(Some((
        profile,
        format!(
            "Connected on this computer without approval ({verb} the space; owner {}).",
            device.owner_email
        ),
    )))
}

fn finish(options: &Options, profile: &Profile, approved: &str) -> Result<(), String> {
    let name = profile_name_for(
        &options.client,
        &profile.workspace_name,
        &profile.workspace_id,
        |n| load_profile(n).ok().map(|p| p.workspace_id),
    );
    let profile_path = profile_dir().join(format!("{name}.json"));
    write_private(
        &profile_path,
        &serde_json::to_string_pretty(profile).unwrap(),
    )
    .map_err(|e| format!("cannot write {}: {e}", profile_path.display()))?;
    // Settle the one-time migration of older bindings before adding this one,
    // so a first connect without --capture does not switch capture on.
    let _ = load_consent();
    let mut bindings = load_bindings();
    let folder = folder_key(&options.project);
    bindings
        .entry(options.client.clone())
        .or_default()
        .insert(folder.clone(), name.clone());
    save_bindings(&bindings)?;
    println!(
        "{approved} Space \"{}\" ({}).",
        profile.workspace_name, profile.scope
    );
    println!(
        "Saved as profile \"{name}\"; {} sessions started in {folder} use it.",
        options.name
    );
    if name != options.client {
        println!(
            "The default profile \"{}\" still points at its own space for other folders.",
            options.client
        );
    }
    match options.capture {
        Some(true) => {
            set_consent(&options.client, &folder, true)?;
            crate::capture::install_hooks_for(&options.client, false)?;
            println!(
                "Automatic capture is on for this project.\n{RETENTION}\nSwitch it off with: aidrive-mcp-server capture-consent --client {} --project \"{folder}\" off",
                options.client
            );
        }
        Some(false) => {
            set_consent(&options.client, &folder, false)?;
            println!("Automatic capture is off for this project.");
        }
        None if !captures(&options.client) => println!("{OPENCODE_NOT_CAPTURED}"),
        None if !capture_allowed(&load_consent(), &options.client, &folder) => println!(
            "Sessions in this project are not captured automatically. To capture every turn (and give new sessions the brief), run connect again with --capture."
        ),
        None => {}
    }
    if options.register {
        match register(&options.client) {
            Ok(command) => println!(
                "Registered: {command}\nStart a new {} session to use the AIDrive tools.",
                options.name
            ),
            Err(error) => println!(
                "Saved the profile, but registration failed: {error}\nAdd it manually: aidrive-mcp-server serve --profile {}",
                options.client
            ),
        }
    }
    Ok(())
}

// `aidrive-mcp-server open`: signs the owner into the web app with a
// one-time link minted by this computer's device key, so no password.
pub async fn open(args: &[String]) -> Result<(), String> {
    let mut api_url =
        std::env::var("AIDRIVE_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let mut browser = true;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--api" => api_url = it.next().cloned().ok_or("--api needs a URL")?,
            "--no-browser" => browser = false,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let api_url = api_url.trim_end_matches('/').to_owned();
    let device = load_device(&api_url).ok_or(
        "This computer has no AIDrive device key yet. Run: aidrive-mcp-server connect --client claude",
    )?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let response = http
        .post(format!("{api_url}/agent/local/login"))
        .bearer_auth(&device.token)
        .send()
        .await
        .map_err(|_| format!("AIDrive API is unreachable at {api_url}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "AIDrive refused the sign-in link ({}).",
            response.status()
        ));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "Unexpected response from AIDrive".to_owned())?;
    let url = body["url"]
        .as_str()
        .ok_or("Unexpected response from AIDrive")?;
    println!("Open AIDrive (one-time link, 2 minutes):\n\n    {url}\n");
    if browser {
        open_browser(url);
    }
    Ok(())
}

pub async fn run(options: Options) -> Result<(), String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    if options.local
        && let Some((profile, message)) = connect_locally(&http, &options).await?
    {
        return finish(&options, &profile, &message);
    }
    let dir = profile_dir();
    let pending_path = dir.join(format!("{}.pending.json", options.client));

    // Re-running within the code's lifetime resumes the same request, so a
    // shell timeout in the calling assistant does not strand the approval.
    let pending = fs::read_to_string(&pending_path)
        .ok()
        .and_then(|t| serde_json::from_str::<PendingPairing>(&t).ok())
        .filter(|p| p.api_url == options.api_url && now().saturating_sub(p.started_at) < 540);
    let pending = match pending {
        Some(p) => p,
        None => {
            let p = PendingPairing {
                pairing_id: uuid_v4()?,
                token: format!("aidrive_agent_{}", random_hex(32)?),
                api_url: options.api_url.clone(),
                started_at: now(),
            };
            write_private(&pending_path, &serde_json::to_string(&p).unwrap())
                .map_err(|e| format!("cannot write {}: {e}", pending_path.display()))?;
            p
        }
    };

    let started: serde_json::Value = http
        .post(format!("{}/agent/pairing", options.api_url))
        .json(&serde_json::json!({
            "pairing_id": pending.pairing_id,
            "client_name": options.name,
            "token_sha256": sha256_hex(&pending.token),
        }))
        .send()
        .await
        .map_err(|_| format!("AIDrive API is unreachable at {}", options.api_url))?
        .json()
        .await
        .map_err(|_| "Unexpected response from AIDrive".to_owned())?;
    let code = started["user_code"]
        .as_str()
        .ok_or_else(|| format!("AIDrive refused the request: {started}"))?;
    let url = started["verification_url"].as_str().unwrap_or_default();
    let shown = format!("{}-{}", &code[..4], &code[4..]);
    println!(
        "To connect {} to AIDrive, approve this code in your browser:\n\n    {shown}\n\n    {url}\n",
        options.name
    );
    println!(
        "Waiting for approval (up to 10 minutes). If this stops early, run the same command again to resume."
    );
    if options.browser {
        open_browser(url);
    }

    let deadline = Instant::now() + Duration::from_secs(600);
    let status = loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let response = http
            .post(format!(
                "{}/agent/pairing/{}/status",
                options.api_url, pending.pairing_id
            ))
            .bearer_auth(&pending.token)
            .send()
            .await;
        if let Ok(response) = response
            && let Ok(body) = response.json::<serde_json::Value>().await
        {
            match body["status"].as_str() {
                Some("approved") => break body,
                Some("denied") => {
                    let _ = fs::remove_file(&pending_path);
                    return Err("The owner denied this connection.".into());
                }
                Some("expired") => {
                    let _ = fs::remove_file(&pending_path);
                    return Err("The code expired. Run the command again for a new code.".into());
                }
                _ => {}
            }
        }
        if Instant::now() > deadline {
            return Err(
                "Timed out waiting for approval. Run the command again to resume or restart."
                    .into(),
            );
        }
    };

    let profile = Profile {
        api_url: options.api_url.clone(),
        workspace_id: status["workspace_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        workspace_name: status["workspace_name"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        scope: status["scope"].as_str().unwrap_or_default().to_owned(),
        token: pending.token.clone(),
    };
    let _ = fs::remove_file(&pending_path);
    finish(
        &options,
        &profile,
        &format!(
            "Approved by {}.",
            status["approver_email"].as_str().unwrap_or("the owner")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_parse_and_validate() {
        let o = parse(&["--client".into(), "codex".into()]).unwrap();
        assert_eq!(
            (o.client.as_str(), o.name.as_str(), o.register),
            ("codex", "Codex", true)
        );
        assert!(parse(&["--client".into(), "other".into()]).is_err());
        assert!(parse(&[]).is_err());
        assert!(parse(&["--client".into(), "claude".into(), "--bogus".into()]).is_err());
        let o = parse(&["--client".into(), "claude".into(), "--capture".into()]).unwrap();
        assert_eq!(o.capture, Some(true));
        let o = parse(&["--client".into(), "codex".into(), "--no-capture".into()]).unwrap();
        assert_eq!(o.capture, Some(false));
        assert_eq!(
            parse(&["--client".into(), "codex".into()]).unwrap().capture,
            None
        );
        let o = parse(&[
            "--client".into(),
            "claude".into(),
            "--space".into(),
            " Side project ".into(),
        ])
        .unwrap();
        assert_eq!((o.space.as_str(), o.local), ("Side project", true));
        let o = parse(&[
            "--client".into(),
            "claude".into(),
            "--browser-approval".into(),
        ])
        .unwrap();
        assert!(!o.local && !o.space.is_empty());
        let o = parse(&[
            "--client".into(),
            "codex".into(),
            "--project".into(),
            "C:/work/TutorBook".into(),
        ])
        .unwrap();
        assert_eq!(
            o.space, "TutorBook",
            "the space defaults to the project folder name"
        );
    }

    #[test]
    fn capture_needs_consent_for_the_bound_folder_and_starts_from_existing_bindings() {
        let mut b = Bindings::new();
        b.entry("claude".into())
            .or_default()
            .insert(r"c:\p".into(), "claude-p".into());
        let c = consent_from_bindings(&b);
        assert!(capture_allowed(&c, "claude", r"c:\p"));
        assert!(!capture_allowed(&c, "claude", r"c:\q"));
        assert!(!capture_allowed(&c, "codex", r"c:\p"));
        let mut off = c.clone();
        off.get_mut("claude").unwrap().insert(r"c:\p".into(), false);
        assert!(!capture_allowed(&off, "claude", r"c:\p"));
    }

    #[test]
    fn sessions_use_the_profile_of_the_nearest_bound_folder() {
        let root = std::env::temp_dir().join("aidrive-bind-test");
        let (project, other) = (root.join("tutorbook"), root.join("elsewhere"));
        let mut bindings = Bindings::new();
        bindings
            .entry("claude".into())
            .or_default()
            .insert(folder_key(&project), "claude-tutorbook".into());
        let deep = project.join("apps").join("web");
        assert_eq!(
            resolve("claude", &deep, &bindings),
            ("claude-tutorbook".to_owned(), Some(folder_key(&project)))
        );
        assert_eq!(
            resolve("claude", &other, &bindings),
            ("claude".to_owned(), None)
        );
        assert_eq!(
            resolve("codex", &deep, &bindings),
            ("codex".to_owned(), None),
            "bindings are per client"
        );
        if cfg!(windows) {
            assert_eq!(
                folder_key(Path::new(r"\\?\C:\Work\TutorBook\")),
                folder_key(Path::new("c:/work/tutorbook"))
            );
        }
    }

    #[test]
    fn connecting_a_second_project_never_rebinds_the_first() {
        let existing = |defaults: Option<&'static str>, named: Option<&'static str>| {
            move |n: &str| {
                if n == "claude" {
                    defaults.map(str::to_owned)
                } else {
                    named.map(str::to_owned)
                }
            }
        };
        assert_eq!(
            profile_name_for("claude", "AIDrive launch", "ws-1", existing(None, None)),
            "claude"
        );
        assert_eq!(
            profile_name_for(
                "claude",
                "AIDrive launch",
                "ws-1",
                existing(Some("ws-1"), None)
            ),
            "claude"
        );
        assert_eq!(
            profile_name_for(
                "claude",
                "Side project",
                "ws-2",
                existing(Some("ws-1"), None)
            ),
            "claude-side-project"
        );
        assert_eq!(
            profile_name_for(
                "claude",
                "Side project",
                "ws-2abcdef99",
                existing(Some("ws-1"), Some("ws-9"))
            ),
            "claude-side-project-ws2abcde"
        );
        assert_eq!(
            profile_name_for(
                "codex",
                "Հայերեն նախագիծ",
                "1234abcd-0000",
                |n| (n == "codex").then(|| "x".to_owned())
            ),
            "codex-1234abcd"
        );
        assert!(valid_profile_name(&profile_name_for(
            "claude",
            &"x".repeat(80),
            "ws",
            existing(Some("other"), None)
        )));
    }

    #[test]
    fn opencode_is_a_client_of_its_own() {
        let o = parse(&["--client".into(), "opencode".into()]).unwrap();
        let refused = parse(&["--client".into(), "opencode".into(), "--capture".into()])
            .err()
            .unwrap();
        assert!(refused.starts_with("Not connected") && refused.contains("without --capture"));
        assert_eq!(
            (o.client.as_str(), o.name.as_str()),
            ("opencode", "OpenCode")
        );
        assert!(is_client("opencode") && !is_client("claude-aidrive-launch"));
        assert_eq!(client_of("opencode-bytetoflag"), "opencode");
        // Claude and Codex already hold profiles for the same space.
        let taken = |n: &str| {
            matches!(
                n,
                "claude" | "codex" | "codex-bytetoflag" | "claude-bytetoflag"
            )
            .then(|| "ws-btf".to_owned())
        };
        assert_eq!(
            profile_name_for("opencode", "ByteToFlag", "ws-btf", taken),
            "opencode"
        );
        let opencode_elsewhere = |n: &str| match n {
            "opencode" => Some("ws-other".to_owned()),
            "codex" | "codex-bytetoflag" | "claude" => Some("ws-btf".to_owned()),
            _ => None,
        };
        let name = profile_name_for("opencode", "ByteToFlag", "ws-btf", opencode_elsewhere);
        assert_eq!(name, "opencode-bytetoflag");
        for client in ["claude", "codex", "opencode"] {
            let name = profile_name_for(client, "ByteToFlag", "ws-btf", opencode_elsewhere);
            assert_eq!(client_of(&name), client, "{client} got {name}");
        }
    }

    #[test]
    fn only_packages_signed_by_anthropic_supply_settings() {
        let root = std::env::temp_dir().join(format!("aidrive-pkgs-{}", std::process::id()));
        for name in [
            "Claude_pzs8sxrjxfjjc",
            "Evil.StoreApp_8wekyb3d8bbwe",
            "Claude_attacker1234",
        ] {
            fs::create_dir_all(root.join("Packages").join(name)).unwrap();
        }
        let found = claude_package_dirs(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].ends_with("Claude_pzs8sxrjxfjjc"), "{found:?}");
    }

    #[test]
    fn settings_written_inside_a_packaged_app_are_read_but_never_win() {
        let own: Bindings = serde_json::from_str(
            r#"{"opencode": {"c:/b": "opencode"}, "claude": {"c:/x": "claude-x"}}"#,
        )
        .unwrap();
        let packaged: Bindings = serde_json::from_str(
            r#"{"opencode": {"c:/b": "stale", "c:/t": "opencode-t"}, "codex": {"c:/y": "codex-y"}}"#,
        )
        .unwrap();
        let merged = merge_layers(vec![own, packaged]);
        assert_eq!(merged["opencode"]["c:/b"], "opencode", "own folder wins");
        assert_eq!(merged["opencode"]["c:/t"], "opencode-t");
        assert_eq!(merged["codex"]["c:/y"], "codex-y");
        assert_eq!(merged["claude"]["c:/x"], "claude-x");
    }

    #[test]
    fn an_explicit_serve_profile_is_used_as_given() {
        let root = std::env::temp_dir().join("aidrive-serve-test");
        let mut bindings = Bindings::new();
        bindings
            .entry("claude".into())
            .or_default()
            .insert(folder_key(&root), "claude-launch".into());
        let deep = root.join("src");
        assert_eq!(
            serve_profile("claude", &deep, &bindings),
            ("claude-launch".to_owned(), Some(folder_key(&root)))
        );
        assert_eq!(
            serve_profile("claude-aidrive-launch", &deep, &bindings),
            ("claude-aidrive-launch".to_owned(), Some(folder_key(&root)))
        );
        assert_eq!(
            serve_profile("opencode-x", &deep, &bindings),
            ("opencode-x".to_owned(), None)
        );
    }

    #[test]
    fn profile_names_cannot_escape_the_profile_directory() {
        assert!(valid_profile_name("claude"));
        assert!(!valid_profile_name("../secrets"));
        assert!(!valid_profile_name("a/b"));
        assert!(!valid_profile_name(""));
    }

    #[test]
    fn generated_secrets_are_hashed_like_the_server() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let id = uuid_v4().unwrap();
        assert_eq!((id.len(), &id[14..15]), (36, "4"));
    }
}
