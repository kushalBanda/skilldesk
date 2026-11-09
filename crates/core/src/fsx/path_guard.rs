//! Centralized path safety helpers.
//!
//! `sanitize_name` strips characters that are unsafe as filesystem names,
//! collapses dot sequences, and caps length — use for any caller-supplied
//! skill/directory name before joining it to a base path.
//!
//! `is_path_safe` canonicalizes both inputs and verifies the target stays
//! inside the base directory. Call before any write/delete/copy that
//! consumes a path derived from untrusted input.

use std::path::{Component, Path, PathBuf};

/// Maximum filesystem name length we'll allow. Matches the lowest common
/// denominator (most modern filesystems allow 255 bytes).
const MAX_NAME_LEN: usize = 200;

/// Fallback used when sanitization strips everything out.
const FALLBACK_NAME: &str = "unnamed";

/// Sanitize a filesystem name.
///
/// - Strips path separators and Windows-forbidden characters
/// - Removes null bytes and other control characters
/// - Collapses `..` sequences (no traversal)
/// - Trims leading/trailing dots and whitespace
/// - Truncates to MAX_NAME_LEN bytes
/// - Returns FALLBACK_NAME when input collapses to empty
pub fn sanitize_name(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        let drop = matches!(
            ch,
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        ) || ch.is_control();
        if !drop {
            out.push(ch);
        }
    }

    // Collapse any run of dots into a single dot so ".." cannot survive,
    // even if other characters were stripped between dots.
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_dot = false;
    for ch in out.chars() {
        if ch == '.' {
            if !prev_dot {
                collapsed.push('.');
            }
            prev_dot = true;
        } else {
            collapsed.push(ch);
            prev_dot = false;
        }
    }

    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c.is_whitespace());

    let truncated = if trimmed.len() > MAX_NAME_LEN {
        // Cut on a char boundary at or below the byte limit.
        let mut end = MAX_NAME_LEN;
        while !trimmed.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &trimmed[..end]
    } else {
        trimmed
    };

    if truncated.is_empty() {
        FALLBACK_NAME.to_string()
    } else {
        truncated.to_string()
    }
}

/// Verify that `target` resolves to a location inside `base`.
///
/// Both paths are canonicalized when possible so symlinks and `..` segments
/// are resolved before comparison. When canonicalize fails (e.g. target does
/// not yet exist), falls back to a normalized path comparison.
///
/// Returns `false` when the relationship cannot be established safely —
/// callers should treat that as a refusal.
pub fn is_path_safe(base: &Path, target: &Path) -> bool {
    let base_resolved = base.canonicalize().unwrap_or_else(|_| normalize(base));
    let target_resolved = target
        .canonicalize()
        .unwrap_or_else(|_| resolve_non_existing(target));

    target_resolved.starts_with(&base_resolved)
}

/// Resolve a path that may not exist yet by walking from the nearest
/// existing ancestor.
fn resolve_non_existing(path: &Path) -> PathBuf {
    for anc in path.ancestors() {
        if let Ok(real) = anc.canonicalize() {
            let mut out = real;
            if let Ok(rest) = path.strip_prefix(anc) {
                out.push(rest);
            }
            return normalize(&out);
        }
    }
    normalize(path)
}

/// Resolve `.` and `..` segments without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_guard_rejects_escape() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        assert!(!is_path_safe(&base, &base.join("..").join("outside")));
        std::os::unix::fs::symlink(&outside, base.join("escape")).unwrap();
        assert!(!is_path_safe(&base, &base.join("escape")));
        assert!(is_path_safe(&base, &base.join("new").join("skill")));
        assert_eq!(sanitize_name("../../.."), FALLBACK_NAME);
        assert_eq!(sanitize_name("../etc/passwd"), "etcpasswd");
    }
}
