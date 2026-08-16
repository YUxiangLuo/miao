use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::OnceLock;

static ACTIVE_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn set_active_log_path(path: PathBuf) -> Result<(), PathBuf> {
    ACTIVE_LOG_PATH.set(path)
}

pub fn active_log_path() -> Option<&'static PathBuf> {
    ACTIVE_LOG_PATH.get()
}

use crate::error::{AppError, AppResult};

pub const CONFIG_FILENAME: &str = "config.yaml";
pub const ETC_CONFIG_PATH: &str = "/etc/miao/config.yaml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigPathSource {
    Explicit,
    ExecutableDirExisting,
    EtcDefault,
}

#[derive(Clone, Debug)]
pub struct ConfigPathResolution {
    pub path: PathBuf,
    pub source: ConfigPathSource,
}

fn absolutize(path: PathBuf) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| AppError::context("Failed to resolve current directory", e))?;
        Ok(cwd.join(path))
    }
}

fn config_arg_from(args: impl IntoIterator<Item = OsString>) -> AppResult<Option<PathBuf>> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--config" {
            let value = args
                .next()
                .ok_or_else(|| AppError::message("--config requires a path"))?;
            return Ok(Some(PathBuf::from(value)));
        }

        if let Some(value) = arg.to_str().and_then(|arg| arg.strip_prefix("--config=")) {
            if value.is_empty() {
                return Err(AppError::message("--config requires a path"));
            }
            return Ok(Some(PathBuf::from(value)));
        }
    }

    Ok(None)
}

pub fn resolve_config_path() -> AppResult<ConfigPathResolution> {
    if let Some(path) = config_arg_from(std::env::args_os().skip(1))? {
        return Ok(ConfigPathResolution {
            path: absolutize(path)?,
            source: ConfigPathSource::Explicit,
        });
    }

    let exe_dir_config = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(CONFIG_FILENAME)));
    let exe_dir_config_exists = exe_dir_config.as_deref().is_some_and(|path| path.exists());

    Ok(resolve_config_path_from_parts(
        exe_dir_config_exists,
        exe_dir_config,
    ))
}

fn resolve_config_path_from_parts(
    exe_dir_config_exists: bool,
    exe_dir_config: Option<PathBuf>,
) -> ConfigPathResolution {
    if exe_dir_config_exists {
        if let Some(path) = exe_dir_config {
            return ConfigPathResolution {
                path,
                source: ConfigPathSource::ExecutableDirExisting,
            };
        }
    }

    ConfigPathResolution {
        path: platform_default_config_path(),
        source: ConfigPathSource::EtcDefault,
    }
}

/// Windows app-data folder. Must not be `$LOCALAPPDATA\Miao` — that is the
/// current-user NSIS install dir on a case-insensitive volume.
pub const WINDOWS_APP_DIR: &str = "io.github.yuxiangluo.miao";

pub fn platform_data_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
            .join(WINDOWS_APP_DIR)
    } else {
        PathBuf::from("/etc/miao")
    }
}

pub fn platform_default_config_path() -> PathBuf {
    if cfg!(windows) {
        platform_data_dir().join(CONFIG_FILENAME)
    } else {
        PathBuf::from(ETC_CONFIG_PATH)
    }
}

pub fn default_log_path() -> PathBuf {
    platform_data_dir().join("miao.log")
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{config_arg_from, resolve_config_path_from_parts, ConfigPathSource};

    #[test]
    fn config_arg_parses_separate_value() {
        let args = vec![OsString::from("--config"), OsString::from("/tmp/miao.yaml")];

        let parsed = config_arg_from(args).unwrap();

        assert_eq!(parsed, Some(PathBuf::from("/tmp/miao.yaml")));
    }

    #[test]
    fn config_arg_parses_equals_value() {
        let args = vec![OsString::from("--config=/tmp/miao.yaml")];

        let parsed = config_arg_from(args).unwrap();

        assert_eq!(parsed, Some(PathBuf::from("/tmp/miao.yaml")));
    }

    #[test]
    fn config_arg_rejects_missing_value() {
        let args = vec![OsString::from("--config")];

        let err = config_arg_from(args).unwrap_err();

        assert_eq!(err.to_string(), "--config requires a path");
    }

    #[test]
    fn executable_directory_config_is_compatible() {
        let resolution =
            resolve_config_path_from_parts(true, Some(PathBuf::from("/opt/miao/config.yaml")));

        assert_eq!(resolution.path, PathBuf::from("/opt/miao/config.yaml"));
        assert_eq!(resolution.source, ConfigPathSource::ExecutableDirExisting);
    }

    #[test]
    fn falls_back_to_etc_default_when_executable_directory_config_is_absent() {
        let resolution =
            resolve_config_path_from_parts(false, Some(PathBuf::from("/opt/miao/config.yaml")));

        assert_eq!(resolution.path, super::platform_default_config_path());
        assert_eq!(resolution.source, ConfigPathSource::EtcDefault);
    }

    #[cfg(not(windows))]
    #[test]
    fn platform_default_config_is_etc_on_unix() {
        assert_eq!(
            super::platform_default_config_path(),
            PathBuf::from(super::ETC_CONFIG_PATH)
        );
        assert_eq!(
            super::default_log_path(),
            PathBuf::from("/etc/miao/miao.log")
        );
    }

    #[cfg(windows)]
    #[test]
    fn platform_paths_live_under_local_app_data_on_windows() {
        let data_dir = super::platform_data_dir();
        assert!(data_dir.ends_with(super::WINDOWS_APP_DIR));
        assert!(!data_dir.ends_with("miao"));
        assert_eq!(super::default_log_path(), data_dir.join("miao.log"));
        assert_eq!(
            super::platform_default_config_path(),
            data_dir.join(super::CONFIG_FILENAME)
        );
    }
}
