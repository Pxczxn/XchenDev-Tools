use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn backup_path(path: &Path) -> PathBuf {
    appended_path(path, ".bak")
}

fn temp_path(path: &Path) -> PathBuf {
    appended_path(path, ".tmp")
}

fn appended_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

pub(super) fn atomic_write_with_backup(
    path: &Path,
    data: &[u8],
    backup_current: bool,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("CONFIG_WRITE_FAILED:{}", e))?;
    }

    let temp = temp_path(path);
    let backup = backup_path(path);
    let result = (|| {
        let mut file = File::create(&temp).map_err(|e| format!("CONFIG_WRITE_FAILED:{}", e))?;
        file.write_all(data)
            .map_err(|e| format!("CONFIG_WRITE_FAILED:{}", e))?;
        file.sync_all()
            .map_err(|e| format!("CONFIG_WRITE_FAILED:{}", e))?;
        drop(file);

        replace_file(&temp, path, &backup, backup_current && path.exists())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(windows)]
fn replace_file(
    temp: &Path,
    target: &Path,
    backup: &Path,
    backup_current: bool,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACE_FILE_FLAGS,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain([0]).collect()
    }

    let temp_wide = wide(temp);
    let target_wide = wide(target);

    if backup_current {
        // ReplaceFileW replaces an existing backup path with the current target on success.
        // Do not pre-delete the previous backup: if replacement fails before the API can
        // establish the new backup, the last-known-good recovery point must remain intact.
        let backup_wide = wide(backup);
        unsafe {
            ReplaceFileW(
                PCWSTR(target_wide.as_ptr()),
                PCWSTR(temp_wide.as_ptr()),
                PCWSTR(backup_wide.as_ptr()),
                REPLACE_FILE_FLAGS(0),
                None,
                None,
            )
        }
        .map_err(|e| format!("CONFIG_REPLACE_FAILED:{}", e))?;
    } else {
        unsafe {
            MoveFileExW(
                PCWSTR(temp_wide.as_ptr()),
                PCWSTR(target_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|e| format!("CONFIG_REPLACE_FAILED:{}", e))?;
    }

    Ok(())
}

#[cfg(not(windows))]
fn replace_file(
    temp: &Path,
    target: &Path,
    backup: &Path,
    backup_current: bool,
) -> Result<(), String> {
    if backup_current {
        fs::copy(target, backup).map_err(|e| format!("CONFIG_BACKUP_FAILED:{}", e))?;
    }
    fs::rename(temp, target).map_err(|e| format!("CONFIG_REPLACE_FAILED:{}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn first_write_and_replacement_keep_latest_and_backup() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.json");

        atomic_write_with_backup(&path, b"first", false).expect("first write");
        assert_eq!(fs::read(&path).expect("read first"), b"first");

        atomic_write_with_backup(&path, b"second", true).expect("replace");
        assert_eq!(fs::read(&path).expect("read second"), b"second");
        assert_eq!(fs::read(backup_path(&path)).expect("read backup"), b"first");
    }

    #[test]
    fn replacement_overwrites_existing_backup_with_previous_current() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        let backup = backup_path(&path);

        atomic_write_with_backup(&path, b"first", false).expect("first write");
        fs::write(&backup, b"older-backup").expect("seed backup");

        atomic_write_with_backup(&path, b"second", true).expect("replace with existing backup");

        assert_eq!(fs::read(&path).expect("read current"), b"second");
        assert_eq!(
            fs::read(&backup).expect("read replaced backup"),
            b"first",
            "backup must track the previous valid current config"
        );
    }
}
