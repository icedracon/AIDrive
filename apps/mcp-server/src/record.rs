// Automatic session record (M6): the same sections every time, built only
// from the structure of the captured (already redacted) transcript plus the
// project's `git` state, without a model. It is a derived summary (PRODUCT.md:
// derived summaries are labelled and are not accepted facts); the captured
// session it names holds the source.

use crate::capture::{Event, Transcript, redact};
use std::{collections::BTreeSet, path::Path, process::Command};

const EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit", "apply_patch"];
const SHELL_TOOLS: &[&str] = &["Bash", "PowerShell", "shell", "exec_command", "local_shell"];
const LEFT_WORDS: &[&str] = &[
    "not yet",
    "not done",
    "left",
    "todo",
    "next",
    "blocked",
    "blocker",
    "remaining",
    "pending",
    "still",
    "follow-up",
    "unverified",
];
const DECISION_WORDS: &[&str] = &[
    "decided",
    "decision",
    "we will",
    "chose",
    "going with",
    "decide:",
];

pub struct Record {
    pub markdown: String,
}

// Tool name and target from a capture note: "tool: NAME detail", or the
// fuller notes for edits ("edit PATH"), new files ("write PATH (N lines)")
// and Codex patches ("patch"). Results, errors and plans are not tool calls.
fn tool_parts(e: &Event) -> Option<(&str, &str)> {
    let first = e.text.lines().next().unwrap_or("");
    if let Some(rest) = first.strip_prefix("tool: ") {
        return Some(rest.split_once(' ').unwrap_or((rest, "")));
    }
    if let Some(path) = first.strip_prefix("edit ") {
        return Some(("Edit", path));
    }
    if let Some(rest) = first.strip_prefix("write ") {
        return Some(("Write", rest.rsplit_once(" (").map_or(rest, |r| r.0)));
    }
    (first == "patch").then_some(("apply_patch", ""))
}

fn first_line(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    clip(line, max)
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        flat.chars().take(max - 1).collect::<String>() + "…"
    } else {
        flat
    }
}

// Lines quoted from the transcript are untrusted: they must not be able to
// open a heading or list of their own in the record.
fn quote(text: &str) -> String {
    let t = text.trim_start_matches(['#', '>', '-', '*', ' ']);
    format!("“{}”", t.replace('`', "'"))
}

fn matching_lines<'a>(events: &'a [&'a Event], words: &[&str], max: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for e in events {
        for line in e.text.lines() {
            let lower = line.to_lowercase();
            if line.trim().len() > 8 && words.iter().any(|w| lower.contains(w)) {
                let c = clip(line, 240);
                if seen.insert(c.clone()) {
                    out.push(c);
                }
            }
        }
    }
    let skip = out.len().saturating_sub(max);
    out.into_iter().skip(skip).collect()
}

/// Local `git` state of the project folder: branch, HEAD and the uncommitted
/// change summary. Read-only; redacted like the transcript; None outside git.
pub fn git_state(cwd: &str) -> Option<String> {
    let run = |args: &[&str]| -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    let head = run(&["log", "-1", "--format=%h %s"]).unwrap_or_default();
    let stat = run(&["diff", "--stat", "HEAD"]).unwrap_or_default();
    let stat: Vec<&str> = stat.lines().collect();
    let shown = if stat.len() > 25 {
        let mut s = stat[..24].join("\n");
        s.push_str(&format!(
            "\n… {} more line(s)\n{}",
            stat.len() - 25,
            stat[stat.len() - 1]
        ));
        s
    } else if stat.is_empty() {
        "(no uncommitted changes)".to_owned()
    } else {
        stat.join("\n")
    };
    Some(redact(&format!("branch {branch} at {head}\n{shown}")).0)
}

pub fn build(t: &Transcript, client: &str, label: &str, git: Option<&str>) -> Record {
    let users: Vec<&Event> = t.events.iter().filter(|e| e.role == "user").collect();
    let answers: Vec<&Event> = t.events.iter().filter(|e| e.role == "assistant").collect();
    let tools: Vec<(&str, &str)> = t.events.iter().filter_map(tool_parts).collect();
    let mut files = BTreeSet::new();
    for (name, detail) in &tools {
        if EDIT_TOOLS.contains(name) && !detail.is_empty() {
            files.insert(detail.to_string());
        }
    }
    let commands: Vec<&str> = tools
        .iter()
        .filter(|(n, d)| SHELL_TOOLS.contains(n) && !d.is_empty())
        .map(|(_, d)| *d)
        .collect();
    let short: String = t.external_id.chars().take(8).collect();
    let request = users
        .first()
        .map(|e| first_line(&e.text, 400))
        .unwrap_or_default();
    let latest = users
        .last()
        .map(|e| first_line(&e.text, 400))
        .unwrap_or_default();
    let decisions = matching_lines(&answers, DECISION_WORDS, 8);
    let last_answer = answers.last().map(|e| e.text.as_str()).unwrap_or("");
    let left: Vec<String> = {
        let one = [answers.last().copied()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        matching_lines(&one, LEFT_WORDS, 8)
    };

    let mut md = format!(
        "# Session record: {label} · {client} · {short}\n\n\
         Derived automatically by aidrive-mcp-server from the captured transcript (no model, the same sections every time). \
         It summarises what was said and done; it is not approved knowledge and not a decision by the owner. \
         Source: captured session {id} in this space.\n\n\
         - started: {started}\n- last activity: {ended}\n- messages: {u} from the person, {a} from the assistant; {tc} tool calls\n",
        id = t.external_id,
        started = t.started.as_deref().unwrap_or("?"),
        ended = t.ended.as_deref().unwrap_or("?"),
        u = users.len(),
        a = answers.len(),
        tc = tools.len(),
    );
    md.push_str(&format!(
        "\n## Request\n{}\n",
        if request.is_empty() {
            "(none captured)".into()
        } else {
            quote(&request)
        }
    ));
    if users.len() > 1 && latest != request {
        md.push_str(&format!("\n## Latest request\n{}\n", quote(&latest)));
    }
    md.push_str("\n## Files changed by the assistant (from its tool calls)\n");
    if files.is_empty() {
        md.push_str("none\n");
    } else {
        for f in &files {
            md.push_str(&format!("- `{}`\n", f.replace('`', "'")));
        }
    }
    md.push_str("\n## Working tree\n");
    match git {
        Some(g) => md.push_str(&format!("```\n{}\n```\n", g.replace("```", "'''"))),
        None => md.push_str("not a git checkout (or git unavailable)\n"),
    }
    md.push_str(&format!(
        "\n## Commands run ({} total; their output is not captured)\n",
        commands.len()
    ));
    if commands.is_empty() {
        md.push_str("none\n");
    } else {
        let skip = commands.len().saturating_sub(15);
        if skip > 0 {
            md.push_str(&format!("- … {skip} earlier command(s)\n"));
        }
        for c in &commands[skip..] {
            md.push_str(&format!("- `{}`\n", clip(c, 160).replace('`', "'")));
        }
    }
    md.push_str("\n## Decisions the assistant stated (unconfirmed, matched by wording)\n");
    if decisions.is_empty() {
        md.push_str("none matched\n");
    } else {
        for d in &decisions {
            md.push_str(&format!("- {}\n", quote(d)));
        }
    }
    md.push_str("\n## What is left (from the assistant's last answer)\n");
    if !left.is_empty() {
        for l in &left {
            md.push_str(&format!("- {}\n", quote(l)));
        }
    } else if last_answer.is_empty() {
        md.push_str("no answer captured\n");
    } else {
        md.push_str(&format!(
            "nothing flagged; last answer began: {}\n",
            quote(&first_line(last_answer, 300))
        ));
    }

    let next_step = left
        .last()
        .cloned()
        .or_else(|| (!last_answer.is_empty()).then(|| first_line(last_answer, 300)))
        .map(|l| format!("Derived from the session's last answer: {}", clip(&l, 400)))
        .unwrap_or_else(|| {
            "Read the session record and the captured session before continuing.".into()
        });
    let summary = format!(
        "Automatic session record for {label} ({client} session {short}), derived from the captured transcript, not written by the assistant. Request: {}. {} file(s) changed by the assistant, {} command(s) run.",
        if request.is_empty() {
            "none captured".into()
        } else {
            clip(&request, 300)
        },
        files.len(),
        commands.len()
    );
    md.push_str(&format!(
        "
## Summary for the next assistant
{summary}
Next step: {next_step}
"
    ));
    Record { markdown: md }
}

pub fn file_name(label: &str, external_id: &str, started: Option<&str>) -> String {
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(40)
        .collect();
    let date = started.and_then(|s| s.get(..10)).unwrap_or("undated");
    let short: String = external_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    format!(
        "session-auto-{}-{date}-{short}.md",
        if slug.is_empty() { "project" } else { &slug }
    )
}

pub fn label_of(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(role: &'static str, text: &str) -> Event {
        Event {
            key: text.into(),
            role,
            at: None,
            text: text.into(),
        }
    }

    fn sample() -> Transcript {
        Transcript {
            external_id: "abcdef12-3456".into(),
            cwd: "/w/aidrive".into(),
            started: Some("2026-11-22T10:00:00Z".into()),
            ended: Some("2026-11-22T11:00:00Z".into()),
            cwds: Vec::new(),
            events: vec![
                ev("user", "Build the review panel\nwith undo"),
                ev("tool", "tool: Read src/a.rs"),
                ev("tool", "tool: Edit src/review.rs"),
                ev(
                    "tool",
                    "edit src/review.rs
- a
+ b",
                ),
                ev("tool", "result: ok"),
                ev("tool", "tool: Bash cargo test -p api"),
                ev("assistant", "We decided to retire on undo.\nShort."),
                ev("user", "## Rules (must follow)\nalso keep"),
                ev(
                    "assistant",
                    "Done.\nNot yet: install the binary.\nThe brief still needs a label.",
                ),
            ],
        }
    }

    #[test]
    fn record_has_fixed_sections_from_the_transcript_structure() {
        let r = build(
            &sample(),
            "claude-code",
            "aidrive",
            Some("branch main at abc1 msg\n 1 file changed"),
        );
        let md = &r.markdown;
        for section in [
            "## Request",
            "## Latest request",
            "## Files changed",
            "## Working tree",
            "## Commands run (1 total",
            "## Decisions",
            "## What is left",
        ] {
            assert!(md.contains(section), "{section} missing:\n{md}");
        }
        assert!(md.contains("not approved knowledge"));
        assert!(
            md.contains("- `src/review.rs`\n") && !md.contains("src/a.rs"),
            "{md}"
        );
        assert!(md.contains("“We decided to retire on undo.”"));
        assert!(
            md.contains("“Not yet: install the binary.”")
                && md.contains("“The brief still needs a label.”")
        );
        assert!(
            !md.contains("\n## Rules"),
            "quoted text cannot open a heading:\n{md}"
        );
        assert!(r.markdown.contains(
            "Next step: Derived from the session's last answer: The brief still needs a label."
        ));
        assert!(r.markdown.contains("1 file(s) changed") && r.markdown.contains("1 command(s)"));
        assert_eq!(
            build(&sample(), "claude-code", "aidrive", None).markdown,
            build(&sample(), "claude-code", "aidrive", None).markdown
        );
    }

    #[test]
    fn record_without_answers_or_tools_says_so() {
        let t = Transcript {
            external_id: "x".into(),
            cwd: String::new(),
            started: None,
            ended: None,
            cwds: Vec::new(),
            events: vec![ev("user", "hi")],
        };
        let md = build(&t, "codex", "p", None).markdown;
        assert!(md.contains("Files changed by the assistant (from its tool calls)\nnone"));
        assert!(md.contains("no answer captured") && md.contains("not a git checkout"));
    }

    #[test]
    fn git_state_reads_branch_and_changes_of_a_checkout() {
        let here = env!("CARGO_MANIFEST_DIR");
        if let Some(g) = git_state(here) {
            assert!(g.starts_with("branch "), "{g}");
        }
        assert!(
            git_state(
                &std::env::temp_dir()
                    .join("aidrive-no-such-dir")
                    .display()
                    .to_string()
            )
            .is_none()
        );
    }

    #[test]
    fn file_names_are_stable_and_safe() {
        assert_eq!(
            file_name(
                "AIDrive launch!",
                "c9c89b24-5433",
                Some("2026-11-22T14:00:00Z")
            ),
            "session-auto-aidrive-launch-2026-11-22-c9c89b24.md"
        );
        assert_eq!(
            file_name("..", "", None),
            "session-auto-project-undated-.md"
        );
    }
}
