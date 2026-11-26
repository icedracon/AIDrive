// Disk cleanup on top of the file index (migration 0026). The client measures
// sizes and free space, finds build folders in bound folders, and writes the
// brief section that asks what may go. AIDrive never deletes: a passed review
// date or a full disk is a reason to ask, not permission.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

pub const GIB: u64 = 1024 * 1024 * 1024;

/// Total bytes of a file or folder tree and its newest write, without
/// following links. Stops counting after `max_entries` entries (then the size
/// is a lower bound).
pub fn size_of(path: &Path, max_entries: usize) -> (u64, Option<SystemTime>, bool) {
    let mut total = 0u64;
    let mut newest: Option<SystemTime> = None;
    let mut seen = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        seen += 1;
        if seen > max_entries {
            return (total, newest, true);
        }
        if let Ok(m) = meta.modified() {
            newest = Some(newest.map_or(m, |n| n.max(m)));
        }
        if meta.is_dir() {
            if let Ok(entries) = fs::read_dir(&p) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    (total, newest, false)
}

/// Free and total bytes of the disk that holds `path`.
#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            dir: *const u16,
            avail: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: a NUL-terminated wide string and three valid out pointers.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
    (ok != 0).then_some((avail, total))
}

#[cfg(not(windows))]
pub fn free_space(path: &Path) -> Option<(u64, u64)> {
    let out = Command::new("df").arg("-Pk").arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let cols: Vec<&str> = text.lines().nth(1)?.split_whitespace().collect();
    let total = cols.get(1)?.parse::<u64>().ok()? * 1024;
    let avail = cols.get(3)?.parse::<u64>().ok()? * 1024;
    Some((avail, total))
}

/// Low means under 10% free or under 20 GiB free.
pub fn is_low(free: u64, total: u64) -> bool {
    free < 20 * GIB || (total > 0 && free * 10 < total)
}

/// What a folder is, if it is a known build or package folder.
pub fn build_kind(dir: &Path) -> Option<(&'static str, &'static str)> {
    let name = dir.file_name()?.to_string_lossy().to_lowercase();
    let parent = dir.parent()?;
    match name.as_str() {
        "target"
            if dir.join(".rustc_info.json").exists()
                || dir.join("CACHEDIR.TAG").exists()
                || parent.join("Cargo.toml").exists() =>
        {
            Some(("build", "Rust build output (rebuildable with cargo build)"))
        }
        "node_modules" if parent.join("package.json").exists() => {
            Some(("package", "npm packages (reinstall with npm ci)"))
        }
        ".next" if parent.join("package.json").exists() => Some((
            "build",
            "Next.js build output (rebuildable with npm run build)",
        )),
        _ => None,
    }
}

/// Build and package folders under `root` (depth-limited; never descends into
/// one it found, into .git, or through links).
pub fn find_build_dirs(
    root: &Path,
    max_depth: usize,
) -> Vec<(PathBuf, &'static str, &'static str)> {
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let p = e.path();
            if let Some((kind, purpose)) = build_kind(&p) {
                found.push((p, kind, purpose));
            } else if depth < max_depth && e.file_name() != ".git" {
                stack.push((p, depth + 1));
            }
        }
    }
    found.sort();
    found
}

pub fn human(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else {
        format!("{} MB", bytes / (1024 * 1024))
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    Some((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_owned(),
    ))
}

/// Hints for "is it still in use?": recent writes, uncommitted work, whether
/// the branch is already in main. Hints only; the assistant and the owner decide.
pub fn in_use_hints(path: &Path, worktree: Option<&str>, newest: Option<SystemTime>) -> String {
    let mut hints = Vec::new();
    if let Some(age) = newest.and_then(|n| n.elapsed().ok()) {
        let hours = age.as_secs() / 3600;
        hints.push(if hours < 3 {
            "written in the last 3 hours (likely in use)".to_owned()
        } else if hours < 48 {
            format!("last written {hours} h ago")
        } else {
            format!("last written {} days ago", hours / 24)
        });
    }
    if let Some(w) = worktree.map(Path::new) {
        if !w.exists() {
            hints.push("its worktree is gone".into());
        } else {
            if let Some((_, s)) = git(w, &["status", "--porcelain", "--untracked-files=no"])
                && !s.is_empty()
            {
                hints.push("its worktree has uncommitted changes".into());
            }
            if let Some((merged, _)) = git(w, &["merge-base", "--is-ancestor", "HEAD", "main"]) {
                hints.push(
                    if merged {
                        "its branch is already in main"
                    } else {
                        "its branch is not in main yet"
                    }
                    .into(),
                );
            }
        }
    }
    if !path.exists() {
        hints.push("already gone".into());
    }
    hints.join("; ")
}

/// The brief section shown when the disk is low: the biggest open registered
/// items on this machine, what deleting them would free, and the question.
pub fn low_disk_section(free: u64, total: u64, open: &[serde_json::Value], top: usize) -> String {
    let mut items: Vec<&serde_json::Value> = open
        .iter()
        .filter(|a| a["size_bytes"].as_u64().is_some_and(|b| b > 0))
        .collect();
    items.sort_by_key(|a| std::cmp::Reverse(a["size_bytes"].as_u64().unwrap_or(0)));
    let mut out = format!(
        "Free space is low: {} free of {}. Registered items on this computer, biggest first. For each: if it is still used, extend it (resolve_file extend, keep_days) or keep it; if not, ask the owner before deleting anything that is not yours, delete it, then resolve_file removed. Nothing is deleted automatically.\n",
        human(free),
        human(total)
    );
    let mut sum = 0u64;
    for a in items.iter().take(top) {
        let size = a["size_bytes"].as_u64().unwrap_or(0);
        sum += size;
        let path = a["path"].as_str().unwrap_or("");
        let worktree = a["worktree"].as_str();
        let (_, newest, _) = size_of(Path::new(path), 1);
        out.push_str(&format!(
            "- {} {} [{}] {} (review {}; {}; id {})\n",
            human(size),
            path,
            a["kind"].as_str().unwrap_or("file"),
            a["purpose"].as_str().unwrap_or(""),
            a["keep_until"].as_str().unwrap_or("none"),
            in_use_hints(Path::new(path), worktree, newest),
            a["id"].as_str().unwrap_or("")
        ));
    }
    if items.is_empty() {
        out.push_str("- nothing with a known size is registered on this computer; large folders may be unregistered.\n");
    } else {
        out.push_str(&format!(
            "Removing all listed would free about {}.\n",
            human(sum)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_disk_threshold() {
        assert!(is_low(5 * GIB, 1000 * GIB));
        assert!(is_low(50 * GIB, 1000 * GIB), "under 10%");
        assert!(!is_low(150 * GIB, 1000 * GIB));
    }

    #[test]
    fn finds_build_folders_and_measures_them() {
        let root = std::env::temp_dir().join(format!("aidrive-disk-{}", std::process::id()));
        let crate_dir = root.join("proj");
        fs::create_dir_all(crate_dir.join("target/debug")).unwrap();
        fs::write(crate_dir.join("Cargo.toml"), "[package]").unwrap();
        fs::write(crate_dir.join("target/debug/blob"), vec![0u8; 4096]).unwrap();
        fs::create_dir_all(root.join("web/node_modules/x")).unwrap();
        fs::write(root.join("web/package.json"), "{}").unwrap();
        fs::create_dir_all(root.join("other/target")).unwrap(); // not a Rust target: no Cargo.toml
        let found = find_build_dirs(&root, 4);
        let names: Vec<String> = found
            .iter()
            .map(|(p, k, _)| format!("{}:{k}", p.strip_prefix(&root).unwrap().display()))
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(
            names.iter().any(|n| n.ends_with("target:build"))
                && names.iter().any(|n| n.ends_with("node_modules:package")),
            "{names:?}"
        );
        let (size, newest, capped) = size_of(&crate_dir.join("target"), 1000);
        assert_eq!((size, capped), (4096, false));
        assert!(newest.is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn low_disk_section_lists_biggest_first_with_total() {
        let open = vec![
            serde_json::json!({"id": "a", "path": "/nowhere/a", "kind": "build", "purpose": "Rust build output", "size_bytes": 2 * GIB, "keep_until": "2026-12-01T00:00:00Z"}),
            serde_json::json!({"id": "b", "path": "/nowhere/b", "kind": "package", "purpose": "npm packages", "size_bytes": 5 * GIB}),
        ];
        let text = low_disk_section(3 * GIB, 900 * GIB, &open, 10);
        let (ib, ia) = (
            text.find("5.0 GB /nowhere/b").unwrap(),
            text.find("2.0 GB /nowhere/a").unwrap(),
        );
        assert!(ib < ia, "{text}");
        assert!(
            text.contains("about 7.0 GB")
                && text.contains("Nothing is deleted automatically")
                && text.contains("already gone"),
            "{text}"
        );
    }

    #[test]
    fn free_space_reads_this_disk() {
        let (free, total) = free_space(&std::env::temp_dir()).expect("free space");
        assert!(total > 0 && free <= total);
    }
}
