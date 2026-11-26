// OpenCode (sst/opencode) as an AIDrive client. OpenCode has no `mcp add`
// that takes a command line non-interactively, so `connect --client opencode`
// writes the `aidrive` MCP entry into the user's global opencode.json(c)
// itself. The file is JSONC that people edit by hand (comments, providers), so
// it is edited as text: only the `aidrive` entry changes, everything else
// stays byte for byte, and the old file is backed up first.
//
// Config shape (checked 2026-11-25): OpenCode 2.x documents
// `mcp.servers.<name> = {type: "local", command: [...], environment, disabled}`
// (https://opencode.ai/docs/mcp-servers/); 1.x reads `mcp.<name>` with
// `enabled` and rejects `mcp.servers` ("Invalid input mcp.servers", observed
// with opencode-cli 1.3.17). The layout follows the newest OpenCode found on
// this computer.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub const ENTRY: &str = "aidrive";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    /// OpenCode 1.x: `mcp.aidrive = {type, command, enabled}`.
    V1,
    /// OpenCode 2.x: `mcp.servers.aidrive = {type, command}`.
    V2,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// OpenCode's global config folder: `~/.config/opencode` on every platform
/// (Windows included), or `$XDG_CONFIG_HOME/opencode`.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("opencode")
}

/// The global config file to edit: an existing opencode.jsonc, else an
/// existing opencode.json, else a new opencode.jsonc.
pub fn config_path() -> PathBuf {
    let dir = config_dir();
    for name in ["opencode.jsonc", "opencode.json"] {
        if dir.join(name).is_file() {
            return dir.join(name);
        }
    }
    dir.join("opencode.jsonc")
}

fn parse_version(text: &str) -> Option<(u32, u32, u32)> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let mut parts = text[start..]
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .next()?
        .split('.')
        .map(|p| p.parse::<u32>().ok());
    Some((
        parts.next()??,
        parts.next().flatten()?,
        parts.next().flatten().unwrap_or(0),
    ))
}

fn run_version(program: &Path) -> Option<(u32, u32, u32)> {
    let out = Command::new(program).arg("--version").output().ok()?;
    out.status
        .success()
        .then(|| parse_version(&String::from_utf8_lossy(&out.stdout)))
        .flatten()
}

/// Versions of OpenCode installed here: the desktop app's bundled CLIs
/// (`%APPDATA%\ai.opencode.desktop\cli\<version>-<arch>`), the standalone
/// Windows install and `opencode` on PATH.
fn installed_versions() -> Vec<(u32, u32, u32)> {
    let mut found = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let cli = PathBuf::from(appdata)
            .join("ai.opencode.desktop")
            .join("cli");
        if let Ok(entries) = fs::read_dir(cli) {
            found.extend(
                entries
                    .flatten()
                    .filter(|e| e.path().join("opencode-cli.exe").is_file())
                    .filter_map(|e| parse_version(&e.file_name().to_string_lossy())),
            );
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let exe = PathBuf::from(local)
            .join("opencode")
            .join("opencode-cli.exe");
        if exe.is_file() {
            found.extend(run_version(&exe));
        }
    }
    let on_path = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "opencode", "--version"])
            .output()
    } else {
        Command::new("opencode").arg("--version").output()
    };
    if let Ok(out) = on_path
        && out.status.success()
    {
        found.extend(parse_version(&String::from_utf8_lossy(&out.stdout)));
    }
    found
}

/// The newest installed OpenCode decides; with none found, the shape already
/// in the file decides, and a new file gets the current (2.x) shape.
pub fn choose_layout(versions: &[(u32, u32, u32)], existing: &str) -> Layout {
    match versions.iter().max() {
        Some(v) if v.0 >= 2 => Layout::V2,
        Some(_) => Layout::V1,
        None => match lookup(existing, &["mcp", "servers"]) {
            Ok(Some(_)) => Layout::V2,
            _ if matches!(lookup(existing, &["mcp", ENTRY]), Ok(Some(_))) => Layout::V1,
            _ => Layout::V2,
        },
    }
}

pub fn entry(exe: &str, layout: Layout) -> serde_json::Value {
    let command = serde_json::json!([exe, "serve", "--profile", "opencode"]);
    match layout {
        Layout::V1 => serde_json::json!({"type": "local", "command": command, "enabled": true}),
        Layout::V2 => serde_json::json!({"type": "local", "command": command}),
    }
}

// ---------- JSONC as text ----------

fn skip_ws(s: &[u8], mut i: usize) -> usize {
    loop {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        if s[i..].starts_with(b"//") {
            while i < s.len() && s[i] != b'\n' {
                i += 1;
            }
        } else if s[i..].starts_with(b"/*") {
            i = s[i + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .map_or(s.len(), |p| i + 2 + p + 2);
        } else {
            return i;
        }
    }
}

fn skip_string(s: &[u8], mut i: usize) -> Result<usize, String> {
    i += 1;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            b'"' => return Ok(i + 1),
            _ => i += 1,
        }
    }
    Err("unterminated string".into())
}

fn skip_value(s: &[u8], i: usize) -> Result<usize, String> {
    match s.get(i) {
        Some(b'"') => skip_string(s, i),
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            let mut j = i;
            while j < s.len() {
                j = skip_ws(s, j);
                match s.get(j) {
                    Some(b'"') => {
                        j = skip_string(s, j)?;
                        continue;
                    }
                    Some(b'{' | b'[') => depth += 1,
                    Some(b'}' | b']') => {
                        depth -= 1;
                        if depth == 0 {
                            return Ok(j + 1);
                        }
                    }
                    None => break,
                    _ => {}
                }
                j += 1;
            }
            Err("unbalanced brackets".into())
        }
        Some(_) => {
            let mut j = i;
            while j < s.len()
                && !matches!(s[j], b',' | b'}' | b']')
                && !s[j].is_ascii_whitespace()
                && !s[j..].starts_with(b"//")
                && !s[j..].starts_with(b"/*")
            {
                j += 1;
            }
            (j > i).then_some(j).ok_or_else(|| "missing value".into())
        }
        None => Err("missing value".into()),
    }
}

struct Member {
    key: String,
    start: usize,
    value_start: usize,
    value_end: usize,
}

/// Members of the object whose `{` is at `open`, and the index of its `}`.
fn members(text: &str, open: usize) -> Result<(Vec<Member>, usize), String> {
    let s = text.as_bytes();
    let mut out = Vec::new();
    let mut i = open + 1;
    loop {
        i = skip_ws(s, i);
        match s.get(i) {
            Some(b'}') => return Ok((out, i)),
            Some(b',') => i += 1,
            Some(b'"') => {
                let start = i;
                let end = skip_string(s, i)?;
                let key: String = serde_json::from_str(&text[start..end])
                    .map_err(|_| "unreadable key".to_owned())?;
                i = skip_ws(s, end);
                if s.get(i) != Some(&b':') {
                    return Err(format!("expected ':' after \"{key}\""));
                }
                let value_start = skip_ws(s, i + 1);
                let value_end = skip_value(s, value_start)?;
                out.push(Member {
                    key,
                    start,
                    value_start,
                    value_end,
                });
                i = value_end;
            }
            _ => return Err("not a JSON object".into()),
        }
    }
}

fn root(text: &str) -> Result<usize, String> {
    let i = skip_ws(text.as_bytes(), 0);
    (text.as_bytes().get(i) == Some(&b'{'))
        .then_some(i)
        .ok_or_else(|| "the config is not a JSON object".into())
}

/// Where the object at `path` opens, if every step exists and is an object.
fn lookup(text: &str, path: &[&str]) -> Result<Option<usize>, String> {
    let mut open = root(text)?;
    for key in path {
        let (ms, _) = members(text, open)?;
        match ms.into_iter().rev().find(|m| m.key == *key) {
            Some(m) if text.as_bytes()[m.value_start] == b'{' => open = m.value_start,
            Some(_) => return Err(format!("\"{key}\" is not an object")),
            None => return Ok(None),
        }
    }
    Ok(Some(open))
}

fn line_indent(text: &str, at: usize) -> &str {
    let line = text[..at].rfind('\n').map_or(0, |n| n + 1);
    let rest = &text[line..];
    &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()]
}

fn render(value: &serde_json::Value, indent: &str) -> String {
    serde_json::to_string_pretty(value)
        .unwrap()
        .replace('\n', &format!("\n{indent}"))
}

/// Sets `key` in the object opening at `open` to `value`, replacing only the
/// old value or appending one member after the last.
fn set_member(
    text: &str,
    open: usize,
    key: &str,
    value: &serde_json::Value,
) -> Result<String, String> {
    let (ms, close) = members(text, open)?;
    let parent = line_indent(text, open).to_owned();
    if let Some(m) = ms.iter().rev().find(|m| m.key == key) {
        let indent = line_indent(text, m.start).to_owned();
        return Ok(format!(
            "{}{}{}",
            &text[..m.value_start],
            render(value, &indent),
            &text[m.value_end..]
        ));
    }
    let child = match ms.first() {
        Some(m) => line_indent(text, m.start).to_owned(),
        None => format!("{parent}  "),
    };
    let member = format!(
        "\n{child}{}: {}",
        serde_json::to_string(key).unwrap(),
        render(value, &child)
    );
    let s = text.as_bytes();
    Ok(match ms.last() {
        None => format!("{}{member}\n{parent}{}", &text[..open + 1], &text[close..]),
        Some(last) => {
            let after = skip_ws(s, last.value_end);
            if s[after] == b',' {
                format!("{}{member}{}", &text[..after + 1], &text[after + 1..])
            } else {
                format!(
                    "{},{member}{}",
                    &text[..last.value_end],
                    &text[last.value_end..]
                )
            }
        }
    })
}

/// Removes `key` and one adjoining comma from the object opening at `open`.
fn remove_member(text: &str, open: usize, key: &str) -> Result<String, String> {
    let (ms, _) = members(text, open)?;
    let Some(pos) = ms.iter().position(|m| m.key == key) else {
        return Ok(text.to_owned());
    };
    let m = &ms[pos];
    let s = text.as_bytes();
    let after = skip_ws(s, m.value_end);
    if s.get(after) == Some(&b',') {
        let ws = text[..m.start].trim_end_matches([' ', '\t']).len();
        let from = if text[..ws].ends_with('\n') {
            text[..ws - 1].trim_end_matches('\r').len()
        } else {
            m.start
        };
        return Ok(format!("{}{}", &text[..from], &text[after + 1..]));
    }
    let from = if pos == 0 {
        open + 1
    } else {
        ms[pos - 1].value_end
    };
    Ok(format!("{}{}", &text[..from], &text[m.value_end..]))
}

/// Comments and trailing commas removed: plain JSON for checking.
pub fn strip_jsonc(text: &str) -> String {
    let s = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'"' {
            let end = skip_string(s, i).unwrap_or(s.len());
            out.push_str(&text[i..end]);
            i = end;
        } else if s[i..].starts_with(b"//") || s[i..].starts_with(b"/*") {
            let end = skip_ws(s, i);
            out.push(' ');
            i = end;
        } else if s[i] == b',' {
            let next = skip_ws(s, i + 1);
            if !matches!(s.get(next), Some(b'}' | b']')) {
                out.push(',');
            }
            i += 1;
        } else {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

pub fn parse_jsonc(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&strip_jsonc(text)).map_err(|e| format!("not valid JSONC: {e}"))
}

/// The config text with the AIDrive entry set for `layout` and any entry of
/// the other layout removed (OpenCode 1.x rejects `mcp.servers`, and 2.x would
/// keep a stale `mcp.aidrive` beside the real one).
pub fn merge(text: &str, value: &serde_json::Value, layout: Layout) -> Result<String, String> {
    let text = if text.trim().is_empty() {
        "{\n  \"$schema\": \"https://opencode.ai/config.json\"\n}\n".to_owned()
    } else {
        text.to_owned()
    };
    parse_jsonc(&text)?;
    let mut text = text;
    let wrap = |key: &str, v: serde_json::Value| serde_json::json!({ key: v });
    let mcp = match lookup(&text, &["mcp"])? {
        Some(o) => o,
        None => {
            text = set_member(&text, root(&text)?, "mcp", &serde_json::json!({}))?;
            lookup(&text, &["mcp"])?.unwrap()
        }
    };
    match layout {
        Layout::V1 => {
            if let Some(servers) = lookup(&text, &["mcp", "servers"])? {
                text = remove_member(&text, servers, ENTRY)?;
                let servers = lookup(&text, &["mcp", "servers"])?.unwrap();
                if members(&text, servers)?.0.is_empty() {
                    text = remove_member(&text, lookup(&text, &["mcp"])?.unwrap(), "servers")?;
                }
            }
            let mcp = lookup(&text, &["mcp"])?.unwrap();
            text = set_member(&text, mcp, ENTRY, value)?;
        }
        Layout::V2 => {
            if members(&text, mcp)?.0.iter().any(|m| m.key == ENTRY) {
                text = remove_member(&text, mcp, ENTRY)?;
            }
            text = match lookup(&text, &["mcp", "servers"])? {
                Some(servers) => set_member(&text, servers, ENTRY, value)?,
                None => set_member(
                    &text,
                    lookup(&text, &["mcp"])?.unwrap(),
                    "servers",
                    &wrap(ENTRY, value.clone()),
                )?,
            };
        }
    }
    let check = parse_jsonc(&text)?;
    let at = match layout {
        Layout::V1 => &check["mcp"][ENTRY],
        Layout::V2 => &check["mcp"]["servers"][ENTRY],
    };
    if at != value {
        return Err("the merged config did not come out as expected; not changed".into());
    }
    Ok(text)
}

/// The path of `exe` as programs outside an MSIX-packaged app see it. A
/// packaged app (the Claude desktop app is one) sees its private copy of
/// `%LOCALAPPDATA%\X` at `%LOCALAPPDATA%\X`, but the file really lives in
/// `%LOCALAPPDATA%\Packages\<package>\LocalCache\Local\X`, where OpenCode has
/// to be pointed (observed 2026-11-25: "is not recognized as an internal or
/// external command"). A packaged copy counts only when it is the same file
/// as `exe` (same size and modification time), so a real install wins, and
/// only in a package signed by Anthropic (SECURITY-REVIEW-2026-11 N3).
pub fn outside_path(exe: &Path, local_appdata: &Path) -> PathBuf {
    let Ok(rel) = exe.strip_prefix(local_appdata) else {
        return exe.to_owned();
    };
    let Ok(meta) = fs::metadata(exe) else {
        return exe.to_owned();
    };
    crate::connect::claude_package_dirs(local_appdata)
        .into_iter()
        .map(|p| p.join("LocalCache").join("Local").join(rel))
        .find(|c| {
            fs::metadata(c)
                .is_ok_and(|m| m.len() == meta.len() && m.modified().ok() == meta.modified().ok())
        })
        .unwrap_or_else(|| exe.to_owned())
}

/// Writes the AIDrive entry into OpenCode's global config, backing the old
/// file up beside it. Returns a line saying what was done.
pub fn register(exe: &str) -> Result<String, String> {
    let outside = match std::env::var_os("LOCALAPPDATA") {
        Some(local) => outside_path(Path::new(exe), Path::new(&local)),
        None => PathBuf::from(exe),
    };
    let exe = outside.display().to_string();
    let exe = exe.as_str();
    let path = config_path();
    let original = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let layout = choose_layout(&installed_versions(), &original);
    let merged = merge(&original, &entry(exe, layout), layout)
        .map_err(|e| format!("{}: {e}; not changed", path.display()))?;
    let shape = match layout {
        Layout::V1 => "mcp.aidrive (OpenCode 1.x)",
        Layout::V2 => "mcp.servers.aidrive (OpenCode 2.x)",
    };
    if merged == original {
        return Ok(format!("{} already has {shape}", path.display()));
    }
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    if !original.is_empty() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup = path.with_extension(format!(
            "{}.aidrive-{stamp}.bak",
            path.extension().and_then(|e| e.to_str()).unwrap_or("json")
        ));
        fs::copy(&path, &backup).map_err(|e| format!("cannot back up {}: {e}", path.display()))?;
    }
    let tmp = path.with_extension("aidrive.tmp");
    fs::write(&tmp, &merged).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(format!(
        "wrote {shape} into {} (previous file backed up beside it)",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = r#"{
  // my providers, keep them
  "$schema": "https://opencode.ai/config.json",
  "disabled_providers": ["localai", "git"], /* inline */
  "provider": {
    "localai": {
      "name": "localai // not a comment",
      "options": { "baseURL": "http://localhost:20128/v1" },
    },
  },
}
"#;

    fn exe() -> serde_json::Value {
        entry(r"C:\bin\aidrive-mcp-server.exe", Layout::V2)
    }

    #[test]
    fn merge_adds_the_entry_and_keeps_comments_and_other_keys() {
        let out = merge(OWNER, &exe(), Layout::V2).unwrap();
        for kept in [
            "// my providers, keep them",
            "/* inline */",
            "\"name\": \"localai // not a comment\"",
            "\"baseURL\": \"http://localhost:20128/v1\"",
        ] {
            assert!(out.contains(kept), "{kept} lost:\n{out}");
        }
        let v = parse_jsonc(&out).unwrap();
        assert_eq!(v["mcp"]["servers"]["aidrive"], exe());
        assert_eq!(
            v["provider"]["localai"]["options"]["baseURL"],
            "http://localhost:20128/v1"
        );
        assert_eq!(merge(&out, &exe(), Layout::V2).unwrap(), out, "idempotent");
    }

    #[test]
    fn merge_replaces_a_stale_entry_without_touching_other_servers() {
        let text = r#"{
  "mcp": {
    "servers": {
      "github": { "type": "remote", "url": "https://x" },
      "aidrive": {
        "type": "local",
        "command": ["old.exe", "serve", "--profile", "codex-bytetoflag"]
      }
    }
  },
  "provider": {}
}"#;
        let out = merge(text, &exe(), Layout::V2).unwrap();
        let v = parse_jsonc(&out).unwrap();
        assert_eq!(v["mcp"]["servers"]["aidrive"]["command"][3], "opencode");
        assert_eq!(v["mcp"]["servers"]["github"]["url"], "https://x");
        assert!(!out.contains("codex-bytetoflag"));
    }

    #[test]
    fn opencode_1x_gets_the_old_shape_and_loses_the_rejected_servers_key() {
        let text = r#"{
  "mcp": {
    "servers": {
      "aidrive": { "type": "local", "command": ["x"] }
    }
  },
  "provider": { "a": {} }
}"#;
        let v1 = entry("C:/b/a.exe", Layout::V1);
        let out = merge(text, &v1, Layout::V1).unwrap();
        let v = parse_jsonc(&out).unwrap();
        assert_eq!(v["mcp"]["aidrive"], v1);
        assert_eq!(v["mcp"]["aidrive"]["enabled"], true);
        assert!(v["mcp"].get("servers").is_none(), "{out}");
        assert!(v["provider"]["a"].is_object());
        let back = merge(&out, &exe(), Layout::V2).unwrap();
        let v = parse_jsonc(&back).unwrap();
        assert!(v["mcp"].get("aidrive").is_none(), "{back}");
        assert_eq!(v["mcp"]["servers"]["aidrive"], exe());
    }

    #[test]
    fn merge_creates_a_config_and_refuses_broken_ones() {
        let out = merge("", &exe(), Layout::V2).unwrap();
        assert_eq!(
            parse_jsonc(&out).unwrap()["mcp"]["servers"]["aidrive"],
            exe()
        );
        let out = merge("{}", &exe(), Layout::V1).unwrap();
        assert!(parse_jsonc(&out).unwrap()["mcp"]["aidrive"].is_object());
        assert!(merge("{ \"a\": ", &exe(), Layout::V2).is_err());
        assert!(merge("[1]", &exe(), Layout::V2).is_err());
        assert!(merge(r#"{"mcp": 3}"#, &exe(), Layout::V2).is_err());
    }

    #[test]
    fn a_packaged_apps_private_copy_is_written_as_its_real_path() {
        let root = std::env::temp_dir().join(format!("aidrive-msix-{}", std::process::id()));
        let exe = root.join("AIDrive").join("bin").join("a.exe");
        let real = root
            .join("Packages")
            .join("Claude_pzs8sxrjxfjjc")
            .join("LocalCache")
            .join("Local")
            .join("AIDrive")
            .join("bin")
            .join("a.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"same").unwrap();
        assert_eq!(outside_path(&exe, &root), exe, "no packaged copy");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, b"different!").unwrap();
        assert_eq!(outside_path(&exe, &root), exe, "another file wins nothing");
        fs::copy(&exe, &real).unwrap();
        let t = fs::metadata(&exe).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&real)
            .unwrap()
            .set_modified(t)
            .unwrap();
        assert_eq!(outside_path(&exe, &root), real);
        let elsewhere = std::env::temp_dir().join("x.exe");
        assert_eq!(outside_path(&elsewhere, &root), elsewhere);
        let _ = fs::remove_dir_all(&root);
    }

    // SECURITY-REVIEW-2026-11 N3: any MSIX app, a sandboxed Store app
    // included, can write its own LocalCache. A same-size, same-time copy
    // there became the command OpenCode runs with the user's full rights.
    #[test]
    fn only_claude_desktops_package_can_supply_the_command() {
        let root = std::env::temp_dir().join(format!("aidrive-msix-evil-{}", std::process::id()));
        let exe = root.join("AIDrive").join("bin").join("a.exe");
        let planted = root
            .join("Packages")
            .join("Evil.StoreApp_8wekyb3d8bbwe")
            .join("LocalCache")
            .join("Local")
            .join("AIDrive")
            .join("bin")
            .join("a.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::create_dir_all(planted.parent().unwrap()).unwrap();
        fs::write(&exe, b"same").unwrap();
        fs::copy(&exe, &planted).unwrap();
        let t = fs::metadata(&exe).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(&planted)
            .unwrap()
            .set_modified(t)
            .unwrap();
        assert_eq!(outside_path(&exe, &root), exe);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn layout_follows_the_newest_install_then_the_file() {
        assert_eq!(choose_layout(&[(1, 3, 17), (2, 4, 1)], "{}"), Layout::V2);
        assert_eq!(choose_layout(&[(1, 3, 17)], "{}"), Layout::V1);
        assert_eq!(choose_layout(&[], r#"{"mcp":{"aidrive":{}}}"#), Layout::V1);
        assert_eq!(choose_layout(&[], "{}"), Layout::V2);
        assert_eq!(parse_version("2.4.1-x64"), Some((2, 4, 1)));
        assert_eq!(parse_version("opencode 1.3.17\n"), Some((1, 3, 17)));
        assert_eq!(parse_version("local"), None);
    }
}
