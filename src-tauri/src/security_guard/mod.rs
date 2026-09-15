use crate::domain::{ProtectionDecision, ProcessSummary};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::path::Path;

const PROTECTED_NAMES: &[&str] = &[
    "system",
    "registry",
    "csrss.exe",
    "wininit.exe",
    "services.exe",
    "lsass.exe",
    "smss.exe",
    "svchost.exe",
    "explorer.exe",
    "winlogon.exe",
    "dwm.exe",
    "fontdrvhost.exe",
    "sihost.exe",
    "taskhostw.exe",
    "runtimebroker.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "audiodg.exe",
    "msmpeng.exe",
    "securityhealthservice.exe",
    "wmiprvse.exe",
    "spoolsv.exe",
    "lsm.exe",
];

pub fn default_protected_process_names() -> Vec<String> {
    PROTECTED_NAMES
        .iter()
        .map(|name| name.to_string())
        .collect()
}

pub fn is_protected_process(summary: &ProcessSummary) -> bool {
    protection_for_process(summary).is_protected
}

pub fn is_protected_process_with_extra(summary: &ProcessSummary, extra: &[String]) -> bool {
    protection_for_process_with_extra(summary, extra).is_protected
}

pub fn protection_for_process(summary: &ProcessSummary) -> ProtectionDecision {
    decide_protection(summary, &[])
}

pub fn protection_for_process_with_extra(
    summary: &ProcessSummary,
    extra: &[String],
) -> ProtectionDecision {
    decide_protection(summary, extra)
}

fn decide_protection(summary: &ProcessSummary, extra: &[String]) -> ProtectionDecision {
    let name_lower = summary.name.to_lowercase();
    if summary.pid <= 4 || PROTECTED_NAMES.iter().any(|n| name_lower == *n) {
        return ProtectionDecision {
            is_protected: true,
            reason: Some("系统关键进程受保护".to_string()),
        };
    }
    if extra.iter().any(|n| name_lower == n.to_lowercase()) {
        return ProtectionDecision {
            is_protected: true,
            reason: Some("用户配置的保护进程".to_string()),
        };
    }
    if summary.working_directory.is_none() && summary.command_line.is_none() {
        return ProtectionDecision {
            is_protected: true,
            reason: Some("无法可靠确认进程归属".to_string()),
        };
    }
    ProtectionDecision {
        is_protected: false,
        reason: None,
    }
}

pub fn snapshot_digest(pid: u32, name: &str, cwd: Option<&str>) -> String {
    let payload = format!("{}|{}|{}", pid, name.to_lowercase(), cwd.unwrap_or(""));
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn validate_command_policy(command: &str) -> Result<(), String> {
    let dangerous = Regex::new(r"(?i)(\|\s*rm\b|&&\s*del\b|>\s*\\\\|powershell\s+-enc)").unwrap();
    if dangerous.is_match(command) {
        return Err("COMMAND_POLICY_REJECTED:命令包含未授权的危险语法".to_string());
    }
    Ok(())
}

pub fn validate_path_under_project(path: &str, project_root: &str) -> Result<(), String> {
    let path = Path::new(path);
    let root = Path::new(project_root);
    if !path.exists() {
        return Err("WORKDIR_INVALID:工作目录不存在".to_string());
    }
    if !path.starts_with(root) {
        return Err("WORKDIR_INVALID:工作目录必须在所选项目范围内".to_string());
    }
    Ok(())
}

pub fn normalize_directory(path: &str) -> Result<std::path::PathBuf, String> {
    let p = Path::new(path);
    if !p.exists() {
        return Err("DIRECTORY_INVALID:目录不存在".to_string());
    }
    fs_canonical(p)
}

fn fs_canonical(p: &Path) -> Result<std::path::PathBuf, String> {
    std::fs::canonicalize(p).map_err(|e| format!("DIRECTORY_INVALID:{}", e))
}

pub fn directory_matches_prefix(cwd: &str, root: &std::path::Path) -> Option<u8> {
    let cwd_path = fs_canonical(Path::new(cwd)).ok()?;

    if cwd_path == root {
        return Some(1);
    }

    // Use path components instead of string prefix matching. This prevents a root such as
    // `C:\work\foo` from matching the unrelated sibling `C:\work\foobar`.
    let relative = cwd_path.strip_prefix(root).ok()?;
    let depth = relative.components().count();
    if depth == 1 {
        Some(2)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn directory_match_accepts_root_and_one_child_only() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("foo");
        let child = root.join("child");
        let grandchild = child.join("grandchild");
        std::fs::create_dir_all(&grandchild).expect("dirs");
        let canonical_root = std::fs::canonicalize(&root).expect("canonical root");

        assert_eq!(
            directory_matches_prefix(root.to_str().expect("root"), &canonical_root),
            Some(1)
        );
        assert_eq!(
            directory_matches_prefix(child.to_str().expect("child"), &canonical_root),
            Some(2)
        );
        assert_eq!(
            directory_matches_prefix(grandchild.to_str().expect("grandchild"), &canonical_root),
            None
        );
    }

    #[test]
    fn directory_match_rejects_sibling_with_same_string_prefix() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("foo");
        let sibling = temp.path().join("foobar");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&sibling).expect("sibling");
        let canonical_root = std::fs::canonicalize(&root).expect("canonical root");

        assert_eq!(
            directory_matches_prefix(sibling.to_str().expect("sibling"), &canonical_root),
            None
        );
    }
}
