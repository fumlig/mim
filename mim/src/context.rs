use anyhow::Result;
use std::path::{Path, PathBuf};

const PATH_SEPARATOR: &str = "_";

pub struct Context {
    /// The .mim directory
    pub root: PathBuf,
    /// CWD relative to the root's parent (e.g. "src/lib" if root is /project/.mim and cwd is /project/src/lib)
    pub path: PathBuf,
    /// Full path to the session file
    pub session_path: PathBuf,
}

impl Context {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let root = root.unwrap_or_else(|| resolve_mim_path(&cwd));

        // Project root is the parent of .mim
        let project_root = root.parent().unwrap_or(&root);
        let path = cwd
            .strip_prefix(project_root)
            .unwrap_or(Path::new(""))
            .to_path_buf();

        let session_path = root.join("sessions").join(session_name(&path));

        Ok(Self {
            root,
            path,
            session_path,
        })
    }
}

/// Build a session filename from the relative path and current timestamp.
/// E.g. path "src/lib" -> "src_lib_2026-03-31T143000.123.jsonl"
/// Empty path -> "2026-03-31T143000.123.jsonl"
fn session_name(path: &Path) -> String {
    let ts = chrono::Local::now().format("%Y-%m-%dT%H%M%S%.3f");
    let path_part = path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, PATH_SEPARATOR);
    if path_part.is_empty() {
        format!("{ts}.jsonl")
    } else {
        format!("{path_part}{PATH_SEPARATOR}{ts}.jsonl")
    }
}

/// Walk from `start` upward looking for a `.mim` directory.
/// Returns the first one found, or `<start>/.mim` as fallback.
fn resolve_mim_path(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(".mim");
        if candidate.is_dir() {
            return candidate;
        }
        if !dir.pop() {
            break;
        }
    }
    start.join(".mim")
}
