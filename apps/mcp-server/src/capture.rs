// `aidrive-mcp-server capture`: import Claude Code / Codex session transcripts
// for one project folder into AIDrive (research/session-memory.md).
// What leaves the machine: what the user and assistant said, each tool call
// with its key input (edits as before/after text, new files with their first
// lines, commands, plans), and the first lines of each tool result, all
// redacted before upload. Model reasoning and system/developer instructions
// stay local. Sub-agent transcripts are captured as their own sessions. Transcript formats are the
// clients' internal files, so parsing is defensive and skips what it does not
// recognise.

use crate::connect::{
    Profile, capture_allowed, load_bindings, load_consent, load_profile, profile_dir, resolve,
    uuid_v4,
};
use crate::record;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_TEXT: usize = 4000;
const BATCH: usize = 200;
const MAX_BATCH_BYTES: usize = 1024 * 1024;
const CAPTURE_VERSION: &str = "v4";

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub key: String,
    pub role: &'static str,
    pub at: Option<String>,
    pub text: String,
}

#[derive(Debug)]
pub struct Transcript {
    pub external_id: String,
    pub cwd: String,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub events: Vec<Event>,
    // Every working directory the session ran in, with how many transcript
    // lines ran there. A session that started elsewhere and moved into a
    // bound folder belongs to the folder where most of its lines ran.
    pub cwds: Vec<(String, usize)>,
}

fn count_cwd(cwds: &mut Vec<(String, usize)>, cwd: &str) {
    if cwd.is_empty() {
        return;
    }
    match cwds.iter_mut().find(|(c, _)| c == cwd) {
        Some((_, n)) => *n += 1,
        None => cwds.push((cwd.to_owned(), 1)),
    }
}

// ---------- redaction ----------

const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "accesskey",
    "access_key",
    "private_key",
    "privatekey",
    "client_secret",
    "credential",
    "authorization",
    "auth",
    "cookie",
    "session_id",
    "sessionid",
    "secret_key_base",
    "signing_key",
    "encryption_key",
];
const TOKEN_PREFIXES: &[&str] = &[
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxs-",
    "npm_",
    "AKIA",
    "ASIA",
    "AIza",
    "aidrive_agent_",
    "aidrive_device_",
    "hf_",
    "xapp-",
    "xoxe-",
    "pypi-",
    "shpat_",
    "glptt-",
    "dop_v1_",
    "whsec_",
    "GOCSPX-",
    "ya29.",
    "AGE-SECRET-KEY-",
];

fn is_sensitive_key(key: &str) -> bool {
    let k = key
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .to_ascii_lowercase();
    // Names like PGPASSWORD glue the label to a prefix; only unambiguous
    // labels are matched that way (not "token" or "auth").
    const GLUED: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "apikey",
        "accesskey",
        "privatekey",
    ];
    !k.is_empty()
        && (SENSITIVE_KEYS
            .iter()
            .any(|s| k == *s || k.ends_with(&format!("_{s}")) || k.ends_with(&format!("-{s}")))
            || GLUED.iter().any(|s| k.ends_with(s)))
}

// One `key=value` / `key:value` pair. Returns the replacement (if any) and
// whether the value is expected in the next word (`"password": "x"`).
fn redact_pair(seg: &str) -> (Option<String>, bool) {
    let Some(pos) = seg.find(['=', ':']).filter(|&p| p > 0) else {
        return (looks_like_token(seg).then(|| "[REDACTED]".into()), false);
    };
    let (key, value) = (&seg[..pos], &seg[pos + 1..]);
    let sep = &seg[pos..pos + 1];
    let bare = value.trim_matches(['"', '\'', '\\']);
    if let Some(rest) = value.strip_prefix("//") {
        // scheme://user:password@host
        let replaced = rest.find('@').and_then(|at| {
            let creds = &rest[..at];
            creds
                .find(':')
                .map(|c| format!("{key}://{}:[REDACTED]{}", &creds[..c], &rest[at..]))
        });
        return (replaced, false);
    }
    if is_sensitive_key(key) {
        if bare.is_empty() {
            return (None, true);
        }
        return (Some(format!("{key}{sep}[REDACTED]")), false);
    }
    // DATABASE_URL=postgres://user:password@host
    if value.contains("://")
        && let (Some(url), _) = redact_pair(value)
    {
        return (Some(format!("{key}{sep}{url}")), false);
    }
    if looks_like_token(bare) {
        return (Some(format!("{key}{sep}[REDACTED]")), false);
    }
    (None, false)
}

fn looks_like_token(core: &str) -> bool {
    if TOKEN_PREFIXES
        .iter()
        .any(|p| core.starts_with(p) && core.len() >= p.len() + 12)
    {
        return true;
    }
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && core.starts_with("eyJ")
        && core.len() >= 30
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

fn strip_blocks(text: &str, open: &str, close: &str, replacement: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut count = 0;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        match rest[start..].find(close) {
            Some(end) => {
                out.push_str(replacement);
                rest = &rest[start + end + close.len()..];
                count += 1;
            }
            None => {
                out.push_str(replacement);
                rest = "";
                count += 1;
            }
        }
    }
    out.push_str(rest);
    (out, count)
}

/// Returns the redacted text and how many values were replaced. Labelled
/// secrets, bearer tokens, URL credentials, private keys and well-known token
/// shapes are caught; an unlabelled secret with no recognisable shape is not.
pub fn redact(text: &str) -> (String, usize) {
    let (text, mut count) = strip_blocks(text, "<private>", "</private>", "[PRIVATE]");
    let (text, pem) = strip_blocks(&text, "-----BEGIN", "-----END", "[REDACTED KEY]");
    count += pem;
    let mut out = String::with_capacity(text.len());
    let mut redact_next = false;
    // `password = value`: a label, then a lone `=` or `:`, then the value.
    let mut after_label = false;
    // `curl -u user:password`
    let mut after_user_flag = false;
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end_matches(char::is_whitespace);
        let space = &piece[word.len()..];
        if word.is_empty() {
            out.push_str(piece);
            continue;
        }
        let lead = word.len()
            - word
                .trim_start_matches(['"', '\'', '(', '[', '{', '`'])
                .len();
        // Trailing punctuation is measured after the leading part, so a word
        // made only of punctuation (`'`, `",`) cannot be counted twice.
        let trail = word.len()
            - lead
            - word[lead..]
                .trim_end_matches(['"', '\'', ')', ']', '}', ',', ';', '`', '.'])
                .len();
        let core = &word[lead..word.len() - trail];
        let was_label = std::mem::take(&mut after_label);
        let was_user_flag = std::mem::take(&mut after_user_flag);
        // The scheme word keeps a pending redaction for the value after it.
        let replaced: Option<String> =
            if core.eq_ignore_ascii_case("bearer") || core.eq_ignore_ascii_case("basic") {
                redact_next = true;
                None
            } else if redact_next && !core.is_empty() {
                redact_next = false;
                Some("[REDACTED]".into())
            } else if was_label && (core == "=" || core == ":") {
                redact_next = true;
                None
            } else if was_user_flag && core.contains(':') && !core.contains("://") {
                core.split_once(':')
                    .map(|(user, _)| format!("{user}:[REDACTED]"))
            } else if core == "-u" || core == "--user" {
                after_user_flag = true;
                None
            } else if core.contains(['=', ':']) && !core.contains("://") {
                // A word may hold several pairs: `{email:'x',password:'y'}`, `a=1&token=2`.
                let mut rebuilt = String::with_capacity(core.len());
                let mut changed = false;
                let mut start = 0;
                for (i, c) in core
                    .char_indices()
                    .chain(std::iter::once((core.len(), ',')))
                {
                    if c == ',' || c == '&' || c == ';' {
                        let seg = &core[start..i];
                        let (replacement, pending) = redact_pair(seg);
                        if pending {
                            redact_next = true;
                        }
                        match replacement {
                            Some(r) => {
                                rebuilt.push_str(&r);
                                changed = true;
                            }
                            None => rebuilt.push_str(seg),
                        }
                        if i < core.len() {
                            rebuilt.push(c);
                        }
                        start = i + c.len_utf8();
                    }
                }
                changed.then_some(rebuilt)
            } else if core.contains("://") {
                redact_pair(core).0
            } else if looks_like_token(core) {
                Some("[REDACTED]".into())
            } else {
                if is_sensitive_key(core) && core.len() < 40 {
                    // `"password": "value"` — the label and value are separate words.
                    redact_next = word.ends_with(':') || word.ends_with("\":");
                    after_label = !redact_next;
                }
                None
            };
        match replaced {
            Some(r) => {
                count += 1;
                out.push_str(&word[..lead]);
                out.push_str(&r);
                out.push_str(&word[word.len() - trail..]);
            }
            None => out.push_str(word),
        }
        out.push_str(space);
    }
    (out, count)
}

fn clean(text: &str) -> Option<(String, usize)> {
    let (text, _) = strip_blocks(text, "<system-reminder>", "</system-reminder>", "");
    // PostgreSQL text cannot hold NUL; one in a tool result refused the batch.
    let text = text.replace('\0', "");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let (mut redacted, n) = redact(text);
    if redacted.chars().count() > MAX_TEXT {
        redacted = redacted.chars().take(MAX_TEXT - 1).collect::<String>() + "…";
    }
    Some((redacted, n))
}

fn clean_dialogue(text: &str) -> Option<(String, usize)> {
    let (text, _) = strip_blocks(text, "<system-reminder>", "</system-reminder>", "");
    let text = text.replace('\0', "");
    (!text.trim().is_empty()).then(|| redact(&text))
}

fn cursor_key(profile: &str, t: &Transcript) -> String {
    format!(
        "{CAPTURE_VERSION}:{profile}:{}:{}",
        t.external_id,
        segment_id(t)
    )
}

fn segment_id(t: &Transcript) -> String {
    use sha2::{Digest, Sha256};
    let Some(first) = t.events.first() else {
        return "empty".into();
    };
    if let Some(rest) = first.key.strip_prefix("sg:")
        && let Some((id, _)) = rest.split_once(':')
        && id.len() == 64
        && id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return id.to_owned();
    }
    let seed = serde_json::json!([first.key, first.at, first.role, first.text]);
    format!("{:x}", Sha256::digest(seed.to_string().as_bytes()))
}

fn namespace_codex_segment(t: &mut Transcript) {
    let id = segment_id(t);
    for event in &mut t.events {
        let base = event.key.strip_suffix("#out").unwrap_or(&event.key);
        if base
            .strip_prefix('L')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            event.key = format!("sg:{id}:{}", event.key);
        }
    }
}

// ---------- parsers ----------

fn short(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        text.chars().take(max - 1).collect::<String>() + "…"
    } else {
        text.to_owned()
    }
}

// A plan as one line per step: Claude Code desktop update_plan {items},
// TodoWrite {todos}, Codex update_plan {plan}.
fn plan_note(input: &serde_json::Value) -> Option<String> {
    let steps = ["items", "todos", "plan"]
        .iter()
        .find_map(|k| input.get(*k).and_then(|v| v.as_array()))?;
    let lines: Vec<String> = steps
        .iter()
        .map(|s| {
            let text = ["content", "step", "activeForm"]
                .iter()
                .find_map(|k| s.get(*k).and_then(|v| v.as_str()))
                .unwrap_or("");
            format!(
                "[{}] {}",
                s["status"].as_str().unwrap_or("?"),
                short(text, 200)
            )
        })
        .collect();
    Some(format!("plan:\n{}", lines.join("\n")))
}

fn tool_note(name: &str, input: &serde_json::Value) -> String {
    let field = |k: &str| input.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let base = name.rsplit("__").next().unwrap_or(name);
    match base {
        "Edit" | "MultiEdit" => {
            let mut note = format!("edit {}", field("file_path"));
            let edits: Vec<&serde_json::Value> = match input["edits"].as_array() {
                Some(a) => a.iter().collect(),
                None => vec![input],
            };
            for e in edits.iter().take(5) {
                let (old, new) = (
                    e["old_string"].as_str().unwrap_or(""),
                    e["new_string"].as_str().unwrap_or(""),
                );
                if !old.is_empty() || !new.is_empty() {
                    note.push_str(&format!("\n- {}\n+ {}", short(old, 500), short(new, 1000)));
                }
            }
            return note;
        }
        "Write" => {
            let content = field("content");
            return format!(
                "write {} ({} lines)\n{}",
                field("file_path"),
                content.lines().count(),
                short(content, 1500)
            )
            .trim_end()
            .to_owned();
        }
        "apply_patch" => return format!("patch\n{}", short(field("patch"), 2500)),
        _ => {}
    }
    if let Some(plan) = plan_note(input) {
        return plan;
    }
    if input["advance"] == true {
        return "plan: next step".into();
    }
    let command = match &input["command"] {
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        v => v.as_str().unwrap_or("").to_owned(),
    };
    let detail = if command.is_empty() {
        [
            "file_path",
            "path",
            "pattern",
            "query",
            "url",
            "description",
        ]
        .iter()
        .find_map(|k| input.get(*k).and_then(|v| v.as_str()))
        .map(|s| short(s, 300))
        .unwrap_or_default()
    } else {
        short(&command, 600)
    };
    format!("tool: {name} {detail}").trim().to_owned()
}

// The first lines of what a tool returned; errors are marked.
fn result_note(content: &serde_json::Value, error: bool) -> String {
    let text = match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if text.trim().is_empty() {
        return String::new();
    }
    let label = if error { "error" } else { "result" };
    format!("{label}: {}", short(text.trim(), 600))
}

pub fn parse_claude(content: &str) -> Option<(Transcript, usize)> {
    let mut t = Transcript {
        external_id: String::new(),
        cwd: String::new(),
        started: None,
        ended: None,
        events: Vec::new(),
        cwds: Vec::new(),
    };
    let mut redactions = 0;
    // A sub-agent transcript (its own file, every line a sidechain with an
    // agentId) becomes its own session "<parent>/agent-<id>"; sidechain lines
    // inside a main transcript stay out of the main session.
    let mut subagent: Option<bool> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = v["type"].as_str().unwrap_or("");
        if kind != "user" && kind != "assistant" {
            continue;
        }
        count_cwd(&mut t.cwds, v["cwd"].as_str().unwrap_or(""));
        let side = v["isSidechain"] == true;
        let sub = *subagent.get_or_insert(side && v["agentId"].is_string());
        if side != sub {
            continue;
        }
        if t.external_id.is_empty() {
            let session = v["sessionId"].as_str().unwrap_or("");
            t.external_id = if sub && !session.is_empty() {
                format!("{session}/agent-{}", v["agentId"].as_str().unwrap_or(""))
            } else {
                session.to_owned()
            };
        }
        if t.cwd.is_empty() {
            t.cwd = v["cwd"].as_str().unwrap_or("").to_owned();
        }
        let at = v["timestamp"].as_str().map(str::to_owned);
        if t.started.is_none() {
            t.started = at.clone();
        }
        t.ended = at.clone().or(t.ended);
        let uuid = v["uuid"].as_str().unwrap_or("");
        let role = if kind == "user" { "user" } else { "assistant" };
        let content = &v["message"]["content"];
        let blocks: Vec<serde_json::Value> = match content {
            serde_json::Value::String(s) => vec![serde_json::json!({"type": "text", "text": s})],
            serde_json::Value::Array(a) => a.clone(),
            _ => continue,
        };
        for (i, block) in blocks.iter().enumerate() {
            let (text, event_role) = match block["type"].as_str() {
                Some("text") => (block["text"].as_str().unwrap_or("").to_owned(), role),
                Some("tool_use") => (
                    tool_note(block["name"].as_str().unwrap_or("tool"), &block["input"]),
                    "tool",
                ),
                Some("tool_result") => (
                    result_note(&block["content"], block["is_error"] == true),
                    "tool",
                ),
                // thinking, images and anything unknown stay local.
                _ => continue,
            };
            if let Some((text, n)) = if event_role == "tool" {
                clean(&text)
            } else {
                clean_dialogue(&text)
            } {
                redactions += n;
                t.events.push(Event {
                    key: format!("{uuid}#{i}"),
                    role: event_role,
                    at: at.clone(),
                    text,
                });
            }
        }
    }
    (!t.external_id.is_empty()).then_some((t, redactions))
}

fn codex_instructions(text: &str) -> bool {
    let t = text.trim_start();
    [
        "<environment_context>",
        "<user_instructions>",
        "<permissions",
        "# AGENTS.md",
        "<INSTRUCTIONS>",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

pub fn parse_codex(content: &str) -> Option<(Transcript, usize)> {
    let mut t = Transcript {
        external_id: String::new(),
        cwd: String::new(),
        started: None,
        ended: None,
        events: Vec::new(),
        cwds: Vec::new(),
    };
    let mut redactions = 0;
    let mut internal = false;
    for (line_no, line) in content.lines().enumerate() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let p = &v["payload"];
        let at = v["timestamp"].as_str().map(str::to_owned);
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => {
                t.external_id = p["id"]
                    .as_str()
                    .or(p["session_id"].as_str())
                    .unwrap_or("")
                    .to_owned();
                t.cwd = p["cwd"].as_str().unwrap_or("").to_owned();
                count_cwd(&mut t.cwds, &t.cwd.clone());
                // Codex records its own reviewer/sub-agent threads as sessions;
                // only threads the user started are conversation.
                internal = p["thread_source"].as_str().is_some_and(|s| s != "user")
                    || p["source"].get("subagent").is_some();
                t.started = p["timestamp"].as_str().map(str::to_owned).or(at.clone());
            }
            (Some("turn_context"), _) => {
                count_cwd(&mut t.cwds, p["cwd"].as_str().unwrap_or(""));
            }
            (Some("response_item"), Some("message")) => {
                let role = match p["role"].as_str() {
                    Some("user") => "user",
                    Some("assistant") => "assistant",
                    _ => continue, // developer/system instructions stay local
                };
                let text: String = p["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                if role == "user" && codex_instructions(&text) {
                    continue;
                }
                if let Some((text, n)) = clean_dialogue(&text) {
                    redactions += n;
                    let key = p["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("L{line_no}"));
                    t.events.push(Event {
                        key,
                        role,
                        at: at.clone(),
                        text,
                    });
                    t.ended = at.clone().or(t.ended.clone());
                }
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call")) => {
                let name = p["name"].as_str().unwrap_or("tool");
                let args = p["arguments"]
                    .as_str()
                    .and_then(|a| serde_json::from_str(a).ok())
                    .or_else(|| p["input"].as_str().map(|i| serde_json::json!({"patch": i})))
                    .unwrap_or(serde_json::Value::Null);
                if let Some((text, n)) = clean(&tool_note(name, &args)) {
                    redactions += n;
                    let key = p["call_id"]
                        .as_str()
                        .or(p["id"].as_str())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("L{line_no}"));
                    t.events.push(Event {
                        key,
                        role: "tool",
                        at: at.clone(),
                        text,
                    });
                }
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                let output = match &p["output"] {
                    serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
                        .ok()
                        .and_then(|o| o["output"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| s.clone()),
                    o => o["output"].as_str().unwrap_or("").to_owned(),
                };
                let note = result_note(&serde_json::Value::String(output), false);
                if let Some((text, n)) = clean(&note) {
                    redactions += n;
                    let call = p["call_id"].as_str().map(str::to_owned);
                    t.events.push(Event {
                        key: format!("{}#out", call.unwrap_or_else(|| format!("L{line_no}"))),
                        role: "tool",
                        at: at.clone(),
                        text,
                    });
                }
            }
            _ => {}
        }
    }
    if !t.external_id.is_empty() && !internal {
        namespace_codex_segment(&mut t);
        Some((t, redactions))
    } else {
        None
    }
}

// ---------- discovery ----------

fn normalize(path: &str) -> String {
    let mut p = path.replace('\\', "/").trim_end_matches('/').to_owned();
    // Git Bash and WSL spell C:\x as /c/x and /mnt/c/x.
    let b = p.as_bytes();
    for prefix in ["/mnt/", "/"] {
        let n = prefix.len();
        if cfg!(windows)
            && p.starts_with(prefix)
            && b.len() > n
            && b[n].is_ascii_alphabetic()
            && (b.len() == n + 1 || b[n + 1] == b'/')
        {
            p = format!("{}:{}", &p[n..n + 1], &p[n + 1..]);
            break;
        }
    }
    if cfg!(windows) { p.to_lowercase() } else { p }
}

pub fn within(cwd: &str, project: &str) -> bool {
    let (c, p) = (normalize(cwd), normalize(project));
    !p.is_empty() && (c == p || c.starts_with(&format!("{p}/")))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out, depth + 1);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn discover(client: &str) -> Vec<PathBuf> {
    let root = match client {
        "claude" => home().join(".claude").join("projects"),
        _ => home().join(".codex").join("sessions"),
    };
    let mut files = Vec::new();
    walk(&root, &mut files, 0);
    files.sort();
    files
}

// ---------- upload ----------

pub struct Options {
    pub client: String,
    pub project: String,
    pub profile: String,
    pub dry_run: bool,
    pub hook: bool,
    // Automatic capture from a Claude Code hook: the space comes from the
    // folder binding of the session's working directory, a folder that is not
    // bound is never captured (binding a folder is the consent), only events
    // not yet uploaded are sent, and every failure exits quietly so the hook
    // can never block or slow the session.
    pub auto: bool,
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut client = None;
    let mut project = None;
    let mut profile = None;
    let mut dry_run = false;
    let mut hook = false;
    let mut auto = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--client" => client = it.next().cloned(),
            "--project" => project = it.next().cloned(),
            "--profile" => profile = it.next().cloned(),
            "--dry-run" => dry_run = true,
            "--hook" => hook = true,
            "--auto" => {
                auto = true;
                hook = true;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let usage = "usage: aidrive-mcp-server capture --client claude|codex --project <folder> [--profile NAME] [--dry-run] [--hook] | capture --client claude --auto (hook input on stdin)";
    let client = client.ok_or(usage)?;
    if !matches!(client.as_str(), "claude" | "codex") {
        return Err("--client must be claude or codex".into());
    }
    if project.is_none() && !auto {
        return Err(usage.into());
    }
    Ok(Options {
        profile: profile.unwrap_or_else(|| client.clone()),
        project: project.unwrap_or_default(),
        client,
        dry_run,
        hook,
        auto,
    })
}

// Events already uploaded per (profile, session), so a hook that runs after
// every turn sends only the new tail. Ordinals are positions in the parsed
// transcript, which only grows, so the tail keeps its ordinals.
fn cursor_path() -> PathBuf {
    profile_dir().with_file_name("capture-cursor.json")
}

fn load_cursors() -> serde_json::Map<String, serde_json::Value> {
    fs::read_to_string(cursor_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_cursors(c: &serde_json::Map<String, serde_json::Value>) {
    let path = cursor_path();
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, serde_json::Value::Object(c.clone()).to_string()).is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

// The folder a session mostly ran in: lines are grouped by the bound folder
// their working directory falls under (or none), and the largest group wins.
// Returns its most frequent directory, or None when that group is unbound.
pub fn main_cwd(
    client: &str,
    t: &Transcript,
    bindings: &crate::connect::Bindings,
) -> Option<String> {
    let mut groups: Vec<(Option<String>, usize, String, usize)> = Vec::new();
    for (cwd, n) in &t.cwds {
        let path = normalize(cwd);
        let (_, folder) = resolve(client, Path::new(&path), bindings);
        match groups.iter_mut().find(|g| g.0 == folder) {
            Some(g) => {
                g.1 += n;
                if *n > g.3 {
                    g.2 = path;
                    g.3 = *n;
                }
            }
            None => groups.push((folder, *n, path, *n)),
        }
    }
    groups
        .into_iter()
        .max_by_key(|g| g.1)
        .filter(|g| g.0.is_some())
        .map(|g| g.2)
}

/// Automatic capture for one hook call. Returns a one-line status for logs.
// The transcript a hook refers to. Claude Code passes `transcript_path`;
// Codex hook input is not documented here, so it uses `transcript_path` when
// present and otherwise the newest rollout under ~/.codex/sessions whose
// working directory is inside the bound folder.
fn hook_transcript(client: &str, v: &serde_json::Value) -> Result<Transcript, String> {
    let parse = |content: &str| match client {
        "claude" => parse_claude(content),
        _ => parse_codex(content),
    };
    if let Some(path) = v["transcript_path"].as_str() {
        let content =
            fs::read_to_string(path).map_err(|e| format!("cannot read transcript: {e}"))?;
        return parse(&content)
            .map(|(t, _)| t)
            .ok_or_else(|| "transcript has no session yet".into());
    }
    let cwd = v["cwd"]
        .as_str()
        .ok_or("hook input has no transcript_path or cwd")?;
    let mut files = discover(client);
    files.sort_by_key(|f| fs::metadata(f).and_then(|m| m.modified()).ok());
    for f in files.iter().rev().take(40) {
        if let Some((t, _)) = fs::read_to_string(f).ok().as_deref().and_then(parse)
            && within(&t.cwd, cwd)
        {
            return Ok(t);
        }
    }
    Err("no transcript found for this folder".into())
}

pub async fn run_auto(input: &str, client: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(input).map_err(|_| "hook input is not JSON".to_owned())?;
    let t = hook_transcript(client, &v)?;
    let event = v["hook_event_name"].as_str().unwrap_or("Stop");
    sync(client, &t, v["cwd"].as_str().unwrap_or(""), event).await
}

// Uploads what is new in one transcript to the space its folder is bound to,
// and refreshes the session record when due. Shared by the hooks and `watch`;
// uploads are idempotent per event key, so the two may overlap safely.
pub async fn sync(
    client: &str,
    t: &Transcript,
    fallback_cwd: &str,
    event: &str,
) -> Result<String, String> {
    let bindings = load_bindings();
    let cwd = main_cwd(client, t, &bindings).unwrap_or_else(|| {
        if t.cwd.is_empty() {
            fallback_cwd.to_owned()
        } else {
            t.cwd.clone()
        }
    });
    let (name, folder) = resolve(client, Path::new(&cwd), &bindings);
    let Some(folder) = folder else {
        return Ok(format!(
            "not captured: {cwd} is not inside a folder bound to an AIDrive space"
        ));
    };
    if !capture_allowed(&load_consent(), client, &folder) {
        return Ok(format!(
            "not captured: automatic capture is off for {folder} (aidrive-mcp-server capture-consent --client {client} on)"
        ));
    }
    if !within(&cwd, &folder) || t.events.is_empty() {
        return Ok("nothing to capture".into());
    }
    let profile = load_profile(&name)?;
    // Revisit historical truncations once; only advance after every fragment succeeds.
    let key = cursor_key(&name, t);
    let mut cursors = load_cursors();
    let done = cursors.get(&key).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let label = record::label_of(&cwd);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let mut status = if done >= t.events.len() {
        "up to date".to_owned()
    } else {
        let inserted = upload_from(&http, &profile, client, "hook", &label, t, done).await?;
        cursors.insert(key.clone(), serde_json::Value::from(t.events.len()));
        save_cursors(&cursors);
        format!(
            "captured {} new event(s) of session {} into \"{}\" ({inserted} stored)",
            t.events.len() - done,
            t.external_id,
            profile.workspace_name
        )
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let segment = segment_id(t);
    let rkey = format!("record:v4:{name}:{}:{segment}", t.external_id);
    let prior = cursors
        .get(&rkey)
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let closing = matches!(event, "SessionEnd" | "PreCompact");
    // Sub-agent work is in its events; the record belongs to the main session.
    if !t.external_id.contains("/agent-") && record_due(&prior, t.events.len(), now, closing) {
        let client_label = if client == "claude" {
            "claude-code"
        } else {
            "codex"
        };
        let rec = record::build(t, client_label, &label, record::git_state(&cwd).as_deref());
        let record_id = format!("{}-segment-{}", t.external_id, &segment[..12]);
        let name = record::file_name(&label, &record_id, t.started.as_deref());
        match write_record(&http, &profile, &prior, &name, &rec.markdown).await {
            Ok((file_id, version_id)) => {
                let entry = serde_json::json!({"file_id": file_id, "version_id": version_id, "events": t.events.len(), "at": now});
                status.push_str(&format!("; session record {name} updated"));
                // No handoff draft at session end: the record is the session's
                // state, and drafts filled the owner's queue (2026-11-24).
                cursors.insert(rkey, entry);
                save_cursors(&cursors);
            }
            Err(e) => status.push_str(&format!("; session record skipped: {e}")),
        }
    }
    Ok(status)
}

// The record is rewritten at most every 5 minutes while the session runs (so
// a session cut off by a usage limit still leaves a recent one), and always
// before compaction and at the end, but only when new events arrived.
pub fn record_due(prior: &serde_json::Value, events: usize, now: u64, closing: bool) -> bool {
    let seen = prior["events"].as_u64().unwrap_or(0) as usize;
    let at = prior["at"].as_u64().unwrap_or(0);
    events > seen && (closing || prior.is_null() || now.saturating_sub(at) >= 5 * 60)
}

async fn write_record(
    http: &reqwest::Client,
    p: &Profile,
    prior: &serde_json::Value,
    name: &str,
    content: &str,
) -> Result<(String, String), String> {
    let base = format!("{}/agent/workspaces/{}/files", p.api_url, p.workspace_id);
    let parse = |v: serde_json::Value| -> Result<(String, String), String> {
        match (v["file_id"].as_str(), v["version_id"].as_str()) {
            (Some(f), Some(ver)) => Ok((f.to_owned(), ver.to_owned())),
            _ => Err(format!("AIDrive refused the record: {v}")),
        }
    };
    let send = |req: reqwest::RequestBuilder| async move {
        req.bearer_auth(&p.token)
            .send()
            .await
            .map_err(|_| "AIDrive API is unreachable".to_owned())?
            .json::<serde_json::Value>()
            .await
            .map_err(|_| "unexpected response".to_owned())
    };
    if let Some(file_id) = prior["file_id"].as_str() {
        // Someone may have edited the record; then build on their version.
        let current = send(http.get(format!("{base}/{file_id}"))).await?;
        let base_version = current["version_id"]
            .as_str()
            .or(prior["version_id"].as_str())
            .unwrap_or_default()
            .to_owned();
        if current["content"].as_str() == Some(content) {
            return Ok((file_id.to_owned(), base_version));
        }
        let body = serde_json::json!({"request_id": uuid_v4()?, "base_version_id": base_version, "content": content});
        return parse(send(http.put(format!("{base}/{file_id}")).json(&body)).await?);
    }
    let body = serde_json::json!({"request_id": uuid_v4()?, "name": name, "content": content});
    let created = send(http.post(&base).json(&body)).await?;
    if created["error"] == "name_taken" || created["error"] == "duplicate_name" {
        return Err(format!("a file named {name} already exists"));
    }
    parse(created)
}

async fn upload(
    http: &reqwest::Client,
    p: &Profile,
    client: &str,
    origin: &str,
    label: &str,
    t: &Transcript,
) -> Result<usize, String> {
    upload_from(http, p, client, origin, label, t, 0).await
}

async fn upload_from(
    http: &reqwest::Client,
    p: &Profile,
    client: &str,
    origin: &str,
    label: &str,
    t: &Transcript,
    skip: usize,
) -> Result<usize, String> {
    let base = format!("{}/agent/workspaces/{}/sessions", p.api_url, p.workspace_id);
    let session: serde_json::Value = http
        .post(&base)
        .bearer_auth(&p.token)
        .json(&serde_json::json!({
            "client": if client == "claude" { "claude-code" } else { "codex" },
            "external_id": t.external_id,
            "label": label,
            "origin": origin,
            "started_at": t.started,
            "ended_at": t.ended,
        }))
        .send()
        .await
        .map_err(|_| "AIDrive API is unreachable".to_owned())?
        .json()
        .await
        .map_err(|_| "unexpected response".to_owned())?;
    let sid = session["session_id"]
        .as_str()
        .ok_or_else(|| format!("AIDrive refused the session: {session}"))?;
    let mut inserted = 0;
    let mut pending = Vec::new();
    let mut bytes = 32;
    for (ordinal, e) in t.events.iter().enumerate().skip(skip) {
        let count = e.text.chars().count().div_ceil(MAX_TEXT);
        let digest = if e.role != "tool" {
            use sha2::{Digest, Sha256};
            Some(format!("{:x}", Sha256::digest(e.text.as_bytes())))
        } else {
            None
        };
        let legacy_digest = digest.as_ref().map(|_| {
            use sha2::{Digest, Sha256};
            format!(
                "{:x}",
                Sha256::digest(short(e.text.trim(), MAX_TEXT).as_bytes())
            )
        });
        let mut chars = e.text.chars();
        for index in 0..count {
            let text: String = chars.by_ref().take(MAX_TEXT).collect();
            let mut fragment = serde_json::json!({"key": e.key, "ordinal": ordinal,
                "role": e.role, "at": e.at, "text": text});
            if let Some(digest) = &digest {
                fragment["chunk_index"] = index.into();
                fragment["chunk_count"] = count.into();
                fragment["content_sha256"] = digest.clone().into();
                fragment["legacy_sha256"] = legacy_digest.as_deref().unwrap().into();
            }
            let size = serde_json::to_vec(&fragment)
                .map_err(|_| "cannot encode capture")?
                .len()
                + 1;
            if !pending.is_empty() && (pending.len() == BATCH || bytes + size > MAX_BATCH_BYTES) {
                inserted +=
                    send_events(http, p, &base, sid, std::mem::take(&mut pending), false).await?;
                bytes = 32;
            }
            if size + 32 > MAX_BATCH_BYTES {
                return Err("capture fragment exceeds request limit".into());
            }
            pending.push(fragment);
            bytes += size;
        }
    }
    if !pending.is_empty() {
        inserted += send_events(http, p, &base, sid, pending, true).await?;
    }
    Ok(inserted)
}

async fn send_events(
    http: &reqwest::Client,
    p: &Profile,
    base: &str,
    sid: &str,
    events: Vec<serde_json::Value>,
    complete: bool,
) -> Result<usize, String> {
    let response = http
        .post(format!("{base}/{sid}/events"))
        .bearer_auth(&p.token)
        .json(&serde_json::json!({"events": events, "complete": complete}))
        .send()
        .await
        .map_err(|_| "AIDrive API is unreachable")?;
    if !response.status().is_success() {
        return Err(format!(
            "AIDrive refused capture (HTTP {}); cursor unchanged",
            response.status()
        ));
    }
    let body: serde_json::Value = response.json().await.map_err(|_| "unexpected response")?;
    body["inserted"]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| "unexpected capture response".into())
}

pub async fn run(o: Options) -> Result<(), String> {
    if o.auto {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        println!("{}", run_auto(&input, &o.client).await?);
        return Ok(());
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let profile = if o.dry_run {
        None
    } else {
        Some(load_profile(&o.profile)?)
    };

    // Hook mode: Claude Code passes JSON with the transcript path on stdin.
    let (files, origin): (Vec<PathBuf>, &str) = if o.hook {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        let v: serde_json::Value =
            serde_json::from_str(&input).map_err(|_| "hook input is not JSON".to_owned())?;
        let path = v["transcript_path"]
            .as_str()
            .ok_or("hook input has no transcript_path")?;
        (vec![PathBuf::from(path)], "hook")
    } else {
        (discover(&o.client), "import")
    };

    let label = Path::new(&o.project)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let (mut sessions, mut events, mut redactions, mut uploaded) = (0, 0, 0, 0);
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        let parsed = if o.client == "claude" {
            parse_claude(&content)
        } else {
            parse_codex(&content)
        };
        let Some((t, n)) = parsed else { continue };
        // Every mode, including hooks, only ever uploads sessions that ran
        // inside the chosen project folder.
        if !within(&t.cwd, &o.project) || t.events.is_empty() {
            continue;
        }
        let label = if o.hook {
            Path::new(&t.cwd)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".into())
        } else {
            label.clone()
        };
        sessions += 1;
        events += t.events.len();
        redactions += n;
        if let Some(p) = &profile {
            uploaded += upload(&http, p, &o.client, origin, &label, &t).await?;
        }
    }
    if o.dry_run {
        println!(
            "Dry run: {sessions} {} session(s) in this folder, {events} event(s), {redactions} value(s) would be redacted. Nothing was uploaded.",
            o.client
        );
    } else {
        println!(
            "Captured {sessions} session(s): {events} event(s) found, {uploaded} new, {redactions} value(s) redacted."
        );
    }
    Ok(())
}

// `install-hooks [--remove]`: Claude Code hooks for automatic session state.
// SessionStart prints the AIDrive brief into the new session (also after
// compaction or resume); Stop (after every turn), PreCompact and SessionEnd
// capture the transcript tail. Only folders bound to a space are affected.
// Idempotent; other hooks are kept; the previous file is backed up once.
const HOOK_MARK: &str = "aidrive-mcp-server";

pub fn claude_settings_path() -> PathBuf {
    home().join(".claude").join("settings.json")
}

pub fn codex_hooks_path() -> PathBuf {
    home().join(".codex").join("hooks.json")
}

// Every event an AIDrive hook may sit on; removal looks at all of them.
const HOOK_EVENTS: [&str; 7] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "PreCompact",
    "SessionEnd",
];

// The send_message tool of an AIDrive server, whatever the client named it.
pub const SEND_MATCHER: &str = "mcp__.*aidrive.*__.*";

// Claude Code (documented in code.claude.com/docs/en/hooks): SessionStart
// brief; capture after every turn, before compaction and at session end;
// inbox: register the chat and inject messages for it at session start,
// prompt submit and after tools, mark the sending chat before send_message,
// and after every turn wait in the background for a message (asyncRewake:
// exit 2 wakes the idle chat). Codex (learn.chatgpt.com/docs/hooks,
// ~/.codex/hooks.json): the same without PreCompact/SessionEnd; its Stop
// inbox hook continues the turn with the message instead of waiting.
pub fn client_hooks(exe: &str, client: &str) -> Vec<(&'static str, serde_json::Value)> {
    let command = |rest: &str| format!("\"{exe}\" {rest}");
    let hook = |rest: &str, timeout: u64| serde_json::json!({"hooks": [{"type": "command", "command": command(rest), "timeout": timeout}]});
    let capture = format!("capture --client {client} --auto");
    let brief = format!("brief --auto --client {client}");
    let inbox = format!("inbox --client {client} --auto");
    let mut send = hook(&inbox, 5);
    send["matcher"] = SEND_MATCHER.into();
    let mut list = vec![
        ("SessionStart", hook(&brief, 20)),
        ("SessionStart", hook(&inbox, 5)),
        ("UserPromptSubmit", hook(&inbox, 5)),
        ("PreToolUse", send),
        ("PostToolUse", hook(&inbox, 5)),
        ("Stop", hook(&capture, 20)),
    ];
    if client == "claude" {
        list.push((
            "Stop",
            serde_json::json!({"hooks": [{"type": "command", "command": command(&format!("{inbox} --wait")),
                "asyncRewake": true, "timeout": crate::inbox::WAIT_HOOK_TIMEOUT}]}),
        ));
        list.push(("PreCompact", hook(&capture, 30)));
        list.push(("SessionEnd", hook(&capture, 30)));
        list.push(("SessionEnd", hook(&inbox, 5)));
    } else {
        list.push(("Stop", hook(&inbox, 10)));
    }
    list
}

pub fn set_client_hooks(settings: &mut serde_json::Value, exe: &str, client: &str, remove: bool) {
    if !settings.is_object() {
        *settings = serde_json::json!({});
    }
    let hooks = settings
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let hooks = hooks.as_object_mut().unwrap();
    let wanted = if remove {
        Vec::new()
    } else {
        client_hooks(exe, client)
    };
    for event in HOOK_EVENTS {
        let ours: Vec<serde_json::Value> = wanted
            .iter()
            .filter(|(e, _)| *e == event)
            .map(|(_, g)| g.clone())
            .collect();
        let list = hooks.entry(event).or_insert_with(|| serde_json::json!([]));
        if !list.is_array() {
            *list = serde_json::json!([]);
        }
        let list = list.as_array_mut().unwrap();
        list.retain(|group| {
            !group["hooks"].as_array().is_some_and(|hs| {
                hs.iter()
                    .any(|h| h["command"].as_str().is_some_and(|c| c.contains(HOOK_MARK)))
            })
        });
        list.extend(ours);
        if list.is_empty() {
            hooks.remove(event);
        }
    }
}

/// `aidrive-mcp-server install-hooks [--client claude|codex] [--remove]`.
pub fn install_hooks(args: &[String]) -> Result<(), String> {
    let mut remove = false;
    let mut client = "claude".to_owned();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--remove" => remove = true,
            "--client" => client = it.next().cloned().unwrap_or_default(),
            other => {
                return Err(format!(
                    "unknown argument {other}; usage: aidrive-mcp-server install-hooks [--client claude|codex] [--remove]"
                ));
            }
        }
    }
    if !matches!(client.as_str(), "claude" | "codex") {
        return Err("--client must be claude or codex".into());
    }
    install_hooks_for(&client, remove)
}

pub fn install_hooks_for(client: &str, remove: bool) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .display()
        .to_string()
        .replace('\\', "/");
    let path = if client == "claude" {
        claude_settings_path()
    } else {
        codex_hooks_path()
    };
    let original = fs::read_to_string(&path).unwrap_or_else(|_| "{}".into());
    let mut settings: serde_json::Value = serde_json::from_str(&original)
        .map_err(|_| format!("{} is not valid JSON; not changed", path.display()))?;
    let backup = path.with_extension("json.aidrive-backup");
    if path.exists() && !backup.exists() {
        fs::copy(&path, &backup).map_err(|e| format!("cannot back up {}: {e}", path.display()))?;
    }
    set_client_hooks(&mut settings, &exe, client, remove);
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&settings).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    if remove {
        println!("Removed the AIDrive hooks from {}.", path.display());
    } else if client == "claude" {
        println!(
            "Installed AIDrive hooks in {} (SessionStart brief; capture after every turn, before compaction and at session end; chat inbox: messages for a chat are pushed into it and wake it when idle).\nOnly projects with capture switched on are captured (connect --capture, or capture-consent); messages reach chats in any bound folder. New Claude Code sessions pick this up.",
            path.display()
        );
    } else {
        println!(
            "Installed AIDrive hooks in {} (SessionStart brief; capture after every turn; chat inbox: messages for a thread are added at its start, on each prompt and after tools, and at the end of a turn Codex answers them). Codex asks you to trust new hooks before it runs them (/hooks). An idle Codex thread receives its messages on its next prompt. Only projects with capture switched on are captured.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_dialogue_keeps_long_bilingual_text_and_redacts_before_boundaries() {
        let input = format!(
            "  {}password=boundary_fixture {}\n  ",
            "ա English ".repeat(500),
            "Վերջ answer ".repeat(2000)
        );
        let expected = redact(&input).0;
        let codex = [
            serde_json::json!({"type":"session_meta","payload":{"id":"synthetic-long","cwd":"/synthetic"}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","id":"u","content":[{"text":input}]}}),
            serde_json::json!({"type":"response_item","payload":{"type":"message","role":"assistant","id":"a","content":[{"text":input}]}}),
        ].into_iter().map(|v| v.to_string()).collect::<Vec<_>>().join("\n");
        let (t, n) = parse_codex(&codex).unwrap();
        assert_eq!(n, 2);
        assert_eq!(t.events.len(), 2);
        for e in t.events {
            assert_eq!(e.text, expected);
            assert!(!e.text.contains("boundary_fixture"));
        }
        for role in ["user", "assistant"] {
            let claude = serde_json::json!({"type":role,"sessionId":"synthetic-long","uuid":role,"cwd":"/synthetic","message":{"content":input}}).to_string();
            assert_eq!(parse_claude(&claude).unwrap().0.events[0].text, expected);
        }
        assert!(clean(&input).unwrap().0.chars().count() <= 4000);
        assert_ne!(CAPTURE_VERSION, "v3");
    }

    #[test]
    fn resumed_codex_segments_have_independent_cursors_keys_and_local_ordinals() {
        fn parsed(at: &str, prompt: &str, tail: &str) -> Transcript {
            let meta = serde_json::json!({"type":"session_meta","payload":{"id":"same-session","cwd":"/synthetic"}});
            let first = serde_json::json!({"type":"response_item","timestamp":at,"payload":{"type":"message","role":"user","content":[{"text":prompt}]}});
            let mut lines = vec![meta.to_string(), first.to_string()];
            if !tail.is_empty() {
                lines.push(serde_json::json!({"type":"response_item","timestamp":at,"payload":{"type":"message","role":"assistant","content":[{"text":tail}]}}).to_string());
            }
            parse_codex(&lines.join("\n")).unwrap().0
        }
        let first = parsed("2026-11-24T10:00:00Z", "first prompt", "first answer");
        let second = parsed("2026-11-25T10:00:00Z", "second prompt", "second answer");
        let third = parsed("2026-11-26T10:00:00Z", "third prompt", "");
        assert_eq!(first.external_id, third.external_id);
        let ids = [
            cursor_key("p", &first),
            cursor_key("p", &second),
            cursor_key("p", &third),
        ];
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
        assert_ne!(first.events[0].key, second.events[0].key);
        let appended = parsed("2026-11-26T10:00:00Z", "third prompt", "new answer");
        assert_eq!(cursor_key("p", &third), cursor_key("p", &appended));
        assert_eq!(third.events[0].key, appended.events[0].key);
        assert_eq!(
            first.events,
            parsed("2026-11-24T10:00:00Z", "first prompt", "first answer").events
        );
        let mut replay = first;
        namespace_codex_segment(&mut replay);
        assert_eq!(
            replay.events[0].key,
            parsed("2026-11-24T10:00:00Z", "first prompt", "first answer").events[0].key
        );
        // Appending to a different segment never changes an earlier segment's cursor or order.
        assert_eq!(ids[1], cursor_key("p", &second));
        assert_eq!(second.events[0].role, "user");
    }

    #[test]
    fn full_redaction_covers_secrets_straddling_the_old_limit() {
        for prefix in [3985, 3990, 3999] {
            for secret in [
                "password=synthetic_boundary_value",
                "Authorization: Bearer synthetic_boundary_value",
                "sk-syntheticboundaryvalue1234",
                "<private>synthetic_boundary_value</private>",
            ] {
                let raw = format!(
                    "{} {secret} {}",
                    "ա".repeat(prefix),
                    "English final ".repeat(500)
                );
                let cleaned = clean_dialogue(&raw).unwrap().0;
                let chunks: Vec<String> = cleaned
                    .chars()
                    .collect::<Vec<_>>()
                    .chunks(MAX_TEXT)
                    .map(|c| c.iter().collect())
                    .collect();
                assert!(chunks.len() > 1);
                assert_eq!(chunks.concat(), cleaned);
                assert!(!cleaned.contains("synthetic_boundary_value"));
                assert!(!cleaned.contains("sk-syntheticboundaryvalue1234"));
                assert!(cleaned.ends_with("English final "));
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires synthetic AIDRIVE_CAPTURE_TEST_* API fixture"]
    async fn lossless_capture_against_synthetic_api() {
        let api_url = std::env::var("AIDRIVE_CAPTURE_TEST_API").unwrap();
        assert!(
            api_url.ends_with(":7480"),
            "synthetic API must use port 7480"
        );
        let p = Profile {
            api_url,
            workspace_id: std::env::var("AIDRIVE_CAPTURE_TEST_WORKSPACE").unwrap(),
            workspace_name: "Synthetic".into(),
            scope: "files:write".into(),
            token: std::env::var("AIDRIVE_CAPTURE_TEST_TOKEN").unwrap(),
        };
        let raw = format!(
            "{}password=synthetic_boundary_value {}",
            "ա English ".repeat(401),
            "Հայերեն English final\n".repeat(110_000)
        );
        let expected = clean_dialogue(&raw).unwrap().0;
        assert!(expected.len() > 2 * 1024 * 1024);
        let t = Transcript {
            external_id: "synthetic-lossless-v3".into(),
            cwd: "/synthetic".into(),
            started: None,
            ended: None,
            cwds: vec![],
            events: vec![
                Event {
                    key: "synthetic-u".into(),
                    role: "user",
                    at: None,
                    text: expected.clone(),
                },
                Event {
                    key: "synthetic-a".into(),
                    role: "assistant",
                    at: None,
                    text: expected.clone(),
                },
            ],
        };
        let http = reqwest::Client::new();
        upload(&http, &p, "codex", "import", "Synthetic", &t)
            .await
            .unwrap();
        assert_eq!(
            upload(
                &reqwest::Client::new(),
                &p,
                "codex",
                "import",
                "Synthetic",
                &t
            )
            .await
            .unwrap(),
            0
        );
        let sessions: serde_json::Value = http
            .get(format!(
                "{}/agent/workspaces/{}/sessions",
                p.api_url, p.workspace_id
            ))
            .bearer_auth(&p.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let session = sessions
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["label"] == "Synthetic")
            .unwrap();
        assert_eq!(session["event_count"], 2);
        let events: serde_json::Value = http
            .get(format!(
                "{}/agent/workspaces/{}/sessions/{}/events?around=0&radius=2",
                p.api_url,
                p.workspace_id,
                session["id"].as_str().unwrap()
            ))
            .bearer_auth(&p.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(events.as_array().unwrap().len(), 2);
        for event in events.as_array().unwrap() {
            assert!(event["text"].as_str().unwrap() == expected);
        }
    }

    #[test]
    fn codex_hooks_use_only_the_events_codex_was_seen_to_run() {
        let mut v = serde_json::json!({"hooks": {"PostToolUse": [{"matcher": "x", "hooks": [{"type": "command", "command": "other"}]}]}});
        set_client_hooks(&mut v, "C:/b/aidrive-mcp-server.exe", "codex", false);
        let hooks = v["hooks"].as_object().unwrap();
        assert!(
            hooks.contains_key("SessionStart")
                && hooks.contains_key("Stop")
                && hooks.contains_key("PostToolUse")
        );
        assert!(!hooks.contains_key("SessionEnd") && !hooks.contains_key("PreCompact"));
        assert!(
            v["hooks"]["Stop"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .ends_with("capture --client codex --auto")
        );
        let stop_inbox = &v["hooks"]["Stop"][1]["hooks"][0];
        assert!(
            stop_inbox["command"]
                .as_str()
                .unwrap()
                .ends_with("inbox --client codex --auto")
                && stop_inbox.get("asyncRewake").is_none(),
            "Codex continues the turn instead of waiting: {stop_inbox}"
        );
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], SEND_MATCHER);
        assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 2);
        set_client_hooks(&mut v, "C:/b/aidrive-mcp-server.exe", "codex", true);
        assert_eq!(
            v["hooks"].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["PostToolUse"]
        );
    }

    #[test]
    fn records_are_rewritten_on_new_events_at_most_every_fifteen_minutes() {
        let none = serde_json::Value::Null;
        assert!(record_due(&none, 3, 1000, false));
        let prior = serde_json::json!({"events": 3, "at": 1000});
        assert!(!record_due(&prior, 3, 99999, true), "no new events");
        assert!(!record_due(&prior, 5, 1000 + 60, false), "too soon");
        assert!(
            record_due(&prior, 5, 1000 + 60, true),
            "closing events always write"
        );
        assert!(record_due(&prior, 5, 1000 + 900, false));
    }

    #[test]
    fn redaction_catches_labelled_and_shaped_secrets() {
        let cases = [
            ("password=hunter2 rest", "password=[REDACTED] rest"),
            ("export API_KEY=abc123def", "export API_KEY=[REDACTED]"),
            (
                "Authorization: Bearer abc.def.ghi",
                "Authorization: Bearer [REDACTED]",
            ),
            ("{\"token\": \"s3cr3t\"}", "{\"token\": \"[REDACTED]\"}"),
            (
                "db postgres://app:pa55@localhost/db",
                "db postgres://app:[REDACTED]@localhost/db",
            ),
            ("key sk-abcdefghijklmnop1234 end", "key [REDACTED] end"),
            ("aidrive_agent_0123456789abcdef0123", "[REDACTED]"),
            (
                "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJlX3Rlc3Q",
                "jwt [REDACTED]",
            ),
            ("a <private>my note</private> b", "a [PRIVATE] b"),
            (
                "PGPASSWORD=\\\"$original_password\\\" psql",
                "PGPASSWORD=[REDACTED]\" psql",
            ),
            (
                "{email:'u@x.test',password:'fixture pass'}",
                "{email:'u@x.test',password:[REDACTED] pass'}",
            ),
            ("url?a=1&token=abc123&b=2", "url?a=1&token=[REDACTED]&b=2"),
        ];
        for (input, expected) in cases {
            assert_eq!(redact(input).0, expected, "{input}");
        }
        let pem = "x -----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY----- y";
        assert!(!redact(pem).0.contains("abc"));
    }

    // SECURITY-REVIEW-2026-11 F2: each of these reached AIDrive unredacted.
    #[test]
    fn redaction_covers_device_keys_env_urls_and_spaced_labels() {
        let device = format!("aidrive_device_{}", "a".repeat(64));
        assert_eq!(redact(&device).0, "[REDACTED]");
        let cases = [
            (
                "DATABASE_URL=postgres://app:pa55word@localhost/db",
                "DATABASE_URL=postgres://app:[REDACTED]@localhost/db",
            ),
            ("password = hunter2 ok", "password = [REDACTED] ok"),
            ("token : abc123def ok", "token : [REDACTED] ok"),
            ("SECRET_KEY_BASE=abcdef0123", "SECRET_KEY_BASE=[REDACTED]"),
            ("hf_abcdefghijklmnopqrstuvwxyz", "[REDACTED]"),
            ("GOCSPX-abcdefghijklmnopqrstuv", "[REDACTED]"),
            ("ya29.a0AfH6SMBabcdefghijklmnop", "[REDACTED]"),
            (
                "curl -u admin:hunter2 http://x",
                "curl -u admin:[REDACTED] http://x",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(redact(input).0, expected, "{input}");
        }
    }

    #[test]
    fn redaction_leaves_ordinary_text_alone() {
        for text in [
            "Use git commit 3f9a2c1 and read README.md",
            "token budget of 300 is enough",
            "git push -u origin main",
            "let a = b; x : y",
            "Աուդիտորիան է solo founders",
            "quote ' and \", or ( ) [] {} ` . alone",
        ] {
            assert_eq!(redact(text).0, text);
        }
    }

    #[test]
    fn claude_parser_keeps_dialogue_tools_and_results_but_not_thinking() {
        let lines = [
            r#"{"type":"user","sessionId":"s1","cwd":"C:\\proj","uuid":"u1","timestamp":"2026-11-19T10:00:00Z","message":{"role":"user","content":"Plan the pilot<system-reminder>internal</system-reminder>"}}"#,
            r#"{"type":"assistant","sessionId":"s1","cwd":"C:\\proj","uuid":"a1","timestamp":"2026-11-19T10:01:00Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"private"},{"type":"text","text":"Interview five users."},{"type":"tool_use","name":"Edit","input":{"file_path":"plan.md"}}]}}"#,
            r#"{"type":"user","sessionId":"s1","cwd":"C:\\proj","uuid":"u2","timestamp":"2026-11-19T10:02:00Z","message":{"role":"user","content":[{"type":"tool_result","content":"file contents"}]}}"#,
            r#"{"type":"assistant","isSidechain":true,"sessionId":"s1","uuid":"x","message":{"content":"subagent"}}"#,
        ]
        .join("\n");
        let (t, _) = parse_claude(&lines).unwrap();
        let texts: Vec<_> = t.events.iter().map(|e| (e.role, e.text.as_str())).collect();
        assert_eq!(
            texts,
            vec![
                ("user", "Plan the pilot"),
                ("assistant", "Interview five users."),
                ("tool", "edit plan.md"),
                ("tool", "result: file contents")
            ]
        );
        assert_eq!(
            (t.external_id.as_str(), t.started.as_deref()),
            ("s1", Some("2026-11-19T10:00:00Z"))
        );
    }

    #[test]
    fn codex_parser_skips_instructions_and_reasoning() {
        let lines = [
            r#"{"type":"session_meta","timestamp":"2026-11-19T10:00:00Z","payload":{"id":"r1","cwd":"C:\\proj"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"system rules"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>cwd</environment_context>"}]}}"#,
            r#"{"type":"response_item","timestamp":"2026-11-19T10:01:00Z","payload":{"type":"message","role":"user","id":"m1","content":[{"type":"input_text","text":"Continue the handoff"}]}}"#,
            r#"{"type":"response_item","payload":{"type":"reasoning","summary":[]}}"#,
            r#"{"type":"response_item","timestamp":"2026-11-19T10:02:00Z","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":"{\"command\":\"cargo test\"}"}}"#,
        ]
        .join("\n");
        let (t, _) = parse_codex(&lines).unwrap();
        let texts: Vec<_> = t.events.iter().map(|e| (e.role, e.text.as_str())).collect();
        assert_eq!(
            texts,
            vec![
                ("user", "Continue the handoff"),
                ("tool", "tool: shell cargo test")
            ]
        );
    }

    #[test]
    fn claude_subagent_transcript_is_its_own_session() {
        let lines = [
            r#"{"type":"user","isSidechain":true,"agentId":"a7","sessionId":"s1","cwd":"C:\\proj","uuid":"u1","message":{"role":"user","content":"Write the eval set"}}"#,
            r#"{"type":"assistant","isSidechain":true,"agentId":"a7","sessionId":"s1","uuid":"a1","message":{"content":[{"type":"text","text":"Done."}]}}"#,
        ]
        .join("\n");
        let (t, _) = parse_claude(&lines).unwrap();
        assert_eq!(t.external_id, "s1/agent-a7");
        assert_eq!(t.cwd, "C:\\proj");
        assert_eq!(t.events.len(), 2);
    }

    #[test]
    fn tool_notes_carry_edits_new_files_and_plans() {
        let edit = tool_note(
            "Edit",
            &serde_json::json!({"file_path": "a.rs", "old_string": "x = 1", "new_string": "x = 2"}),
        );
        assert_eq!(edit, "edit a.rs\n- x = 1\n+ x = 2");
        let write = tool_note(
            "Write",
            &serde_json::json!({"file_path": "n.md", "content": "one\ntwo"}),
        );
        assert_eq!(write, "write n.md (2 lines)\none\ntwo");
        let plan = tool_note(
            "mcp__ccd_turn__update_plan",
            &serde_json::json!({"items": [{"content": "Build", "status": "in_progress"}, {"content": "Test", "status": "pending"}]}),
        );
        assert_eq!(plan, "plan:\n[in_progress] Build\n[pending] Test");
        let codex = tool_note(
            "update_plan",
            &serde_json::json!({"plan": [{"step": "Ship", "status": "completed"}]}),
        );
        assert_eq!(codex, "plan:\n[completed] Ship");
        assert_eq!(
            tool_note("shell", &serde_json::json!({"command": ["cargo", "test"]})),
            "tool: shell cargo test"
        );
        assert_eq!(result_note(&serde_json::json!("boom"), true), "error: boom");
    }

    #[test]
    fn a_session_belongs_to_the_bound_folder_where_most_of_it_ran() {
        let bound = if cfg!(windows) {
            "C:\\w\\aidrive"
        } else {
            "/w/aidrive"
        };
        let mut folders = std::collections::BTreeMap::new();
        folders.insert(crate::connect::folder_key(Path::new(bound)), "p".to_owned());
        let mut bindings = crate::connect::Bindings::new();
        bindings.insert("claude".into(), folders);
        let mut t = Transcript {
            external_id: "s".into(),
            cwd: if cfg!(windows) {
                "C:\\w".into()
            } else {
                "/w".into()
            },
            started: None,
            ended: None,
            events: Vec::new(),
            cwds: Vec::new(),
        };
        let home = t.cwd.clone();
        t.cwds = vec![
            (home.clone(), 5),
            (format!("{bound}{}src", std::path::MAIN_SEPARATOR), 4),
            (bound.to_owned(), 3),
        ];
        let main = main_cwd("claude", &t, &bindings).expect("mostly in the bound folder");
        assert!(within(&main, bound), "{main}");
        t.cwds = vec![(home, 9), (bound.to_owned(), 3)];
        assert_eq!(
            main_cwd("claude", &t, &bindings),
            None,
            "mostly outside: not captured"
        );
        assert!(clean("a\0b").is_some_and(|(c, _)| c == "ab"));
        if cfg!(windows) {
            assert_eq!(normalize("/c/Users/USER"), "c:/users/USER");
            assert_eq!(normalize("/mnt/c/Users/USER"), "c:/users/USER");
        }
    }

    #[test]
    fn codex_internal_review_threads_are_skipped() {
        let review = [
            r#"{"type":"session_meta","payload":{"id":"g1","cwd":"/p","thread_source":"guardian_review","source":{"subagent":{"other":"guardian"}}}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","id":"m","content":[{"type":"input_text","text":"assess this action"}]}}"#,
        ]
        .join("\n");
        assert!(parse_codex(&review).is_none());
        let user = review.replace(
            r#""thread_source":"guardian_review","source":{"subagent":{"other":"guardian"}}"#,
            r#""thread_source":"user","source":"vscode""#,
        );
        assert!(parse_codex(&user).is_some());
    }

    #[test]
    fn hooks_install_is_idempotent_keeps_other_hooks_and_removes_cleanly() {
        let mut settings = serde_json::json!({"model": "x", "hooks": {"PostToolUse": [{"matcher": "m", "hooks": [{"type": "command", "command": "other"}]}],
            "Stop": [{"hooks": [{"type": "command", "command": "keep-me"}]}]}});
        set_client_hooks(
            &mut settings,
            "C:/bin/aidrive-mcp-server.exe",
            "claude",
            false,
        );
        set_client_hooks(
            &mut settings,
            "C:/bin/aidrive-mcp-server.exe",
            "claude",
            false,
        );
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(
            stop.len(),
            3,
            "installed once next to the existing Stop hook: {settings}"
        );
        let wait = &stop[2]["hooks"][0];
        assert!(
            wait["command"]
                .as_str()
                .unwrap()
                .ends_with("inbox --client claude --auto --wait")
                && wait["asyncRewake"] == true
                && wait["timeout"].as_u64() > Some(crate::inbox::WAIT_SECONDS),
            "the waiter wakes the idle chat and ends before its hook timeout: {wait}"
        );
        for event in ["UserPromptSubmit", "PreToolUse", "SessionEnd"] {
            assert!(
                settings["hooks"][event]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|g| g["hooks"][0]["command"]
                        .as_str()
                        .unwrap()
                        .contains("inbox --client claude")),
                "{event}"
            );
        }
        assert!(
            stop[1]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .ends_with("capture --client claude --auto")
        );
        assert!(
            settings["hooks"]["SessionStart"][0]["hooks"][0]["command"]
                .as_str()
                .unwrap()
                .ends_with("brief --auto --client claude")
        );
        assert_eq!(
            settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "other"
        );
        assert_eq!(settings["model"], "x");
        set_client_hooks(
            &mut settings,
            "C:/bin/aidrive-mcp-server.exe",
            "claude",
            true,
        );
        assert_eq!(
            settings["hooks"]["PostToolUse"].as_array().unwrap().len(),
            1
        );
        set_client_hooks(
            &mut settings,
            "C:/bin/aidrive-mcp-server.exe",
            "claude",
            false,
        );
        set_client_hooks(
            &mut settings,
            "C:/bin/aidrive-mcp-server.exe",
            "claude",
            true,
        );
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(
            settings["hooks"].get("SessionStart").is_none()
                && settings["hooks"].get("PreCompact").is_none()
        );
    }

    #[test]
    fn auto_mode_needs_no_project_and_implies_hook() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let o = parse(&args(&["--client", "claude", "--auto"])).unwrap();
        assert!(o.auto && o.hook);
    }

    #[test]
    fn hook_mode_requires_a_project_folder() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse(&args(&["--client", "claude", "--hook"])).is_err());
        assert!(parse(&args(&["--client", "claude", "--hook", "--project", "/p"])).is_ok());
    }

    #[test]
    fn project_scoping_matches_folder_and_children_only() {
        assert!(within("C:\\Work\\aidrive", "C:/work/aidrive") || !cfg!(windows));
        assert!(within("/w/aidrive/apps", "/w/aidrive"));
        assert!(!within("/w/aidrive-old", "/w/aidrive"));
        assert!(!within("/w", "/w/aidrive"));
    }
}
