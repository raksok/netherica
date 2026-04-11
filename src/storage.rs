use crate::error::{AppError, AppResult};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataRootSource {
    ExecutableDirectory,
    OsUserDataFallback,
}

#[derive(Debug, Clone)]
pub struct DataDirectory {
    pub root: PathBuf,
    pub archive: PathBuf,
    pub reports: PathBuf,
    pub root_source: DataRootSource,
}

impl DataDirectory {
    /// Resolves the data directory using the following priority:
    /// 1. The directory of the current executable (if writable).
    /// 2. The OS user data directory (fallback).
    pub fn resolve() -> AppResult<Self> {
        Self::resolve_with(Self::find_writable_root, Self::find_user_data_dir)
    }

    pub fn used_fallback(&self) -> bool {
        self.root_source == DataRootSource::OsUserDataFallback
    }

    fn resolve_with<WF, UF>(writable_root: WF, user_data_root: UF) -> AppResult<Self>
    where
        WF: FnOnce() -> AppResult<PathBuf>,
        UF: FnOnce() -> AppResult<PathBuf>,
    {
        let (root, root_source) = match writable_root() {
            Ok(path) => (path, DataRootSource::ExecutableDirectory),
            Err(_) => (user_data_root()?, DataRootSource::OsUserDataFallback),
        };

        let archive = root.join("archive");
        let reports = root.join("reports");

        // Ensure subdirectories exist
        fs::create_dir_all(&archive).map_err(AppError::IoError)?;
        fs::create_dir_all(&reports).map_err(AppError::IoError)?;

        Ok(Self {
            root,
            archive,
            reports,
            root_source,
        })
    }

    /// Attempts to find a writable directory relative to the executable.
    fn find_writable_root() -> AppResult<PathBuf> {
        let exe_dir = std::env::current_exe()
            .map_err(|e| AppError::InternalError(format!("Failed to get current exe path: {}", e)))?
            .parent()
            .ok_or_else(|| {
                AppError::InternalError("Failed to find executable parent directory".to_string())
            })?
            .to_path_buf();

        // Check if we can actually write to this directory
        let metadata = fs::metadata(&exe_dir).map_err(AppError::IoError)?;
        if metadata.permissions().readonly() {
            return Err(AppError::IoError(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Executable directory is read-only",
            )));
        }

        Ok(exe_dir)
    }

    /// Fallback to OS-specific user data directory.
    fn find_user_data_dir() -> AppResult<PathBuf> {
        dirs::data_dir()
            .map(|p| p.join("netherica"))
            .ok_or_else(|| {
                AppError::InternalError("Could not determine OS user data directory".to_string())
            })
            .and_then(|p| {
                if p.exists() {
                    Ok(p)
                } else {
                    fs::create_dir_all(&p).map_err(AppError::IoError)?;
                    Ok(p)
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_prefers_executable_dir_when_writable() {
        let base = std::env::temp_dir().join("netherica_storage_exe_root");
        let data = DataDirectory::resolve_with(|| Ok(base.clone()), || unreachable!())
            .expect("should resolve using writable executable directory");

        assert_eq!(data.root, base);
        assert_eq!(data.root_source, DataRootSource::ExecutableDirectory);
        assert!(!data.used_fallback());
    }

    #[test]
    fn resolve_reports_fallback_when_executable_dir_unwritable() {
        let fallback = std::env::temp_dir().join("netherica_storage_user_data");
        let data = DataDirectory::resolve_with(
            || {
                Err(AppError::IoError(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "not writable",
                )))
            },
            || Ok(fallback.clone()),
        )
        .expect("should resolve using user data fallback");

        assert_eq!(data.root, fallback);
        assert_eq!(data.root_source, DataRootSource::OsUserDataFallback);
        assert!(data.used_fallback());
    }
}
