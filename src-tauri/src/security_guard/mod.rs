use crate::domain::{ProtectionDecision, ProcessSummary};
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

const MAX_LAUNCH_COMMAND_LEN: usize = 4096;

/// These are shell hosts, script hosts, destructive system utilities, or cmd built-ins that
/// should not be the entry point of a saved project launch command. This is deliberately not
/// a runner allow-list: npm/maven/gradle/java/python/php/cargo and custom executables remain
/// supported. The user confirmation is still the primary authorization boundary; this policy
/// is defense in depth for imported or accidentally dangerous profiles.
const BLOCKED_LAUNCH_ENTRYPOINTS: &[&str] = &[
    "cmd",
    "cmd.exe",
    "powershell",
    "powershell.exe",
    "pwsh",
    "pwsh.exe",
    "wscript",
    "wscript.exe",
    "cscript",
    "cscript.exe",
    "mshta",
    "mshta.exe",
    "rundll32",
    "rundll32.exe",
    "regsvr32",
    "regsvr32.exe",
    "del",
    "erase",
    "rd",
    "rmdir",
    "rm",
    "format",
    "format.com",
    "diskpart",
    "diskpart.exe",
    "shutdown",
    "shutdown.exe",
    "taskkill",
    "taskkill.exe",
    "reg",
    "reg.exe",
    "sc",
    "sc.exe",
    "bcdedit",
    "bcdedit.exe",
    "wmic",
    "wmic.exe",
    "schtasks",
    "schtasks.exe",
    "net",
    "net.exe",
    "net1",
    "net1.exe",
    "start",
    "call",
    "for",
    "if",
    "%comspec%",
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

fn first_command_token(command: &str) -> Option<&str> {
    let trimmed = command.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(&rest[..end]);
    }
    trimmed.split_whitespace().next()
}

fn command_basename(token: &str) -> String {
    token
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(token)
        .trim()
        .to_lowercase()
}

fn has_unquoted_shell_control(command: &str) -> bool {
    let mut in_double_quotes = false;
    for ch in command.chars() {
        if ch == '"' {
            in_double_quotes = !in_double_quotes;
            continue;
        }
        if !in_double_quotes && matches!(ch, '&' | '|' | '>' | '<' | '^') {
            return true;
        }
    }
    // An unmatched quote changes cmd parsing in surprising ways; reject rather than guessing.
    in_double_quotes
}

pub fn validate_command_policy(command: &str) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("COMMAND_POLICY_REJECTED:启动命令不能为空".to_string());
    }
    if command.len() > MAX_LAUNCH_COMMAND_LEN {
        return Err("COMMAND_POLICY_REJECTED:启动命令过长".to_string());
    }
    if command.chars().any(char::is_control) {
        return Err("COMMAND_POLICY_REJECTED:启动命令包含控制字符".to_string());
    }
    if has_unquoted_shell_control(command) {
        return Err(
            "COMMAND_POLICY_REJECTED:启动命令不允许 shell 链接、重定向或转义控制符"
                .to_string(),
        );
    }

    let entry = first_command_token(command)
        .ok_or_else(|| "COMMAND_POLICY_REJECTED:无法识别启动命令".to_string())?;
    let entry = command_basename(entry);
    if BLOCKED_LAUNCH_ENTRYPOINTS
        .iter()
        .any(|blocked| entry.eq_ignore_ascii_case(blocked))
    {
        return Err(format!(
            "COMMAND_POLICY_REJECTED:不允许将 {} 作为项目启动入口",
            entry
        ));
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

    #[test]
    fn launch_policy_allows_common_project_commands() {
        let allowed = [
            "npm run dev",
            "pnpm dev --host 127.0.0.1",
            ".\\mvnw.cmd spring-boot:run",
            "mvn spring-boot:run -Dspring-boot.run.profiles=dev",
            ".\\gradlew.bat bootRun",
            "java -jar \"app server.jar\"",
            "python app.py",
            "php -S 127.0.0.1:8000",
            "cargo run",
        ];
        for command in allowed {
            assert!(
                validate_command_policy(command).is_ok(),
                "expected allowed command: {command}"
            );
        }
    }

    #[test]
    fn launch_policy_rejects_shell_chaining_and_redirection() {
        let rejected = [
            "npm run dev && del important.txt",
            "npm run dev | powershell -Command whoami",
            "npm run dev > output.log",
            "npm run dev ^& taskkill /PID 1 /F",
            "npm run dev\r\ndel important.txt",
        ];
        for command in rejected {
            assert!(
                validate_command_policy(command).is_err(),
                "expected rejected command: {command}"
            );
        }
    }

    #[test]
    fn launch_policy_rejects_shell_and_destructive_entrypoints() {
        let rejected = [
            "cmd /C npm run dev",
            "powershell -Command Get-Process",
            "%COMSPEC% /C dir",
            "C:\\Windows\\System32\\taskkill.exe /PID 123 /F",
            "del important.txt",
            "sc stop MySQL80",
            "start npm run dev",
        ];
        for command in rejected {
            assert!(
                validate_command_policy(command).is_err(),
                "expected rejected command: {command}"
            );
        }
    }

    #[test]
    fn launch_policy_allows_literal_metacharacters_inside_quotes() {
        assert!(validate_command_policy("node -e \"console.log('a|b&c>')\"").is_ok());
    }
}
