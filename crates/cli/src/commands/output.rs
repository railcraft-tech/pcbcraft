//! What one command returns. Callers print it or hand it to MCP.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl CommandOutput {
    pub fn text(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            code: 0,
        }
    }

    pub fn note(stderr: impl Into<String>) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            code: 0,
        }
    }

    pub fn status(code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: stderr.into(),
            code,
        }
    }

    pub fn error(code: i32, stderr: impl Into<String>) -> Self {
        Self::status(code, "", stderr)
    }

    pub fn from_result(result: anyhow::Result<Self>) -> Self {
        match result {
            Ok(output) => output,
            Err(err) => Self::error(1, format!("pcbcraft: {err:#}\n")),
        }
    }
}

/// Join `path` onto `cwd` when it is relative. `-` stays `-` so stdout outputs still work.
pub fn place(cwd: &Path, path: &Path) -> PathBuf {
    if path == Path::new("-") || path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// A fresh directory for one command. Parallel calls do not share it.
pub fn scratch(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("pcbcraft-{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).ok();
    path
}
