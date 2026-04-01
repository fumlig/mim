use anyhow::Result;
use std::path::{Path, PathBuf};

const MIM_DIR: &str = ".mim";
const PATH_SEPARATOR: &str = "_";

pub struct Context {
    /// Absolute path to the project root (the directory containing .mim)
    pub root: PathBuf,
    /// Absolute path to the working directory
    pub cwd: PathBuf,
    /// Absolute path to the session file
    pub session_path: PathBuf,
}

impl Context {
    pub fn new(root: Option<PathBuf>) -> Result<Self> {
        let cwd = canonicalize_or(&std::env::current_dir()?);

        let root = match root {
            Some(explicit) => {
                let mim_dir = canonicalize_or(&explicit);
                let project_root = mim_dir.parent().unwrap_or(&mim_dir).to_path_buf();
                anyhow::ensure!(
                    cwd.starts_with(&project_root),
                    "mim path {} is not an ancestor of current directory {}",
                    mim_dir.display(),
                    cwd.display(),
                );
                project_root
            }
            None => resolve_project_root(&cwd),
        };

        // Relative path from project root to cwd, used only for session naming
        let rel_path = cwd
            .strip_prefix(&root)
            .unwrap_or(Path::new(""))
            .to_path_buf();

        let session_path = root
            .join(MIM_DIR)
            .join("sessions")
            .join(session_name(&rel_path));

        Ok(Self {
            root,
            cwd,
            session_path,
        })
    }

    /// Absolute path to the .mim directory
    pub fn mim_dir(&self) -> PathBuf {
        self.root.join(MIM_DIR)
    }

    /// Absolute path to the sessions directory
    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join(MIM_DIR).join("sessions")
    }
}

/// Build a session filename from the relative path and current timestamp.
/// E.g. path "src/lib" -> "src_lib_2026-03-31T143000.123.jsonl"
/// Empty path -> "2026-03-31T143000.123.jsonl"
fn session_name(rel_path: &Path) -> String {
    let ts = chrono::Local::now().format("%Y-%m-%dT%H%M%S%.3f");
    let path_part = rel_path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, PATH_SEPARATOR);
    if path_part.is_empty() {
        format!("{ts}.jsonl")
    } else {
        format!("{path_part}{PATH_SEPARATOR}{ts}.jsonl")
    }
}

/// Best-effort canonicalize: resolve symlinks and `..` if the path exists,
/// otherwise return the original path.
fn canonicalize_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Walk from `start` upward looking for a directory containing `.mim`.
/// Returns the first project root found, or `start` itself as fallback.
fn resolve_project_root(start: &Path) -> PathBuf {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(MIM_DIR).is_dir() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }
    start.to_path_buf()
}
