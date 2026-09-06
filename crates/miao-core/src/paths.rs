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

/// Generated sing-box artifacts plus user preference files.
/// `profile::ResolvedProfile` selects their production ownership once. Test
/// constructors keep preferences under runtime_dir; bindings follow config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePaths {
    pub runtime_dir: PathBuf,
    pub active_config: PathBuf,
    pub config_cache: PathBuf,
    pub cache_manifest: PathBuf,
    pub sub_nodes_snapshot: PathBuf,
    pub node_bindings: PathBuf,
    pub last_proxy: PathBuf,
    pub node_select_preference: PathBuf,
    pub max_multiplier_preference: PathBuf,
}

impl RuntimePaths {
    pub fn new(runtime_dir: PathBuf, config_path: &std::path::Path) -> Self {
        let node_bindings = if is_standard_config(config_path) {
            config_path.with_file_name("node-bindings.json")
        } else {
            profile_state_dir(config_path).join("node-bindings.json")
        };
        Self {
            active_config: runtime_dir.join("config.json"),
            config_cache: runtime_dir.join("config.json.cache"),
            cache_manifest: runtime_dir.join("config-cache.manifest.json"),
            sub_nodes_snapshot: runtime_dir.join("sub-nodes.json"),
            // Bindings belong to a resolved config profile. Deriving the name
            // from the config path prevents two explicit configs in the same
            // directory (and parallel tests) from sharing node identities.
            node_bindings,
            last_proxy: runtime_dir.join(".last_proxy"),
            node_select_preference: runtime_dir.join(".node_select"),
            max_multiplier_preference: runtime_dir.join(".max_multiplier"),
            runtime_dir,
        }
    }

    pub fn with_preferences(
        mut self,
        last_proxy: PathBuf,
        node_select: PathBuf,
        max_multiplier: PathBuf,
    ) -> Self {
        self.last_proxy = last_proxy;
        self.node_select_preference = node_select;
        self.max_multiplier_preference = max_multiplier;
        self
    }
}

pub(crate) fn is_standard_config(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(CONFIG_FILENAME))
    }
    #[cfg(not(windows))]
    {
        path.file_name() == Some(std::ffi::OsStr::new(CONFIG_FILENAME))
    }
}

/// Stable, lossless identity for a resolved config path. Specify the encoding
/// explicitly so a Rust upgrade cannot change the persisted directory name.
pub(crate) fn profile_id(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hash.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            hash.update(unit.to_le_bytes());
        }
    }
    hex::encode(hash.finalize())
}

pub(crate) fn profile_state_dir(config_path: &std::path::Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(".miao-profiles")
        .join(profile_id(config_path))
}

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

pub(crate) fn absolutize(path: PathBuf) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| AppError::context("Failed to resolve current directory", e))?;
        Ok(cwd.join(path))
    }
}

pub fn resolve_default_config_path() -> ConfigPathResolution {
    let exe_dir_config = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(CONFIG_FILENAME)));
    let exe_dir_config_exists = exe_dir_config.as_deref().is_some_and(|path| path.exists());

    resolve_config_path_from_parts(exe_dir_config_exists, exe_dir_config)
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
    use std::path::PathBuf;

    use super::{resolve_config_path_from_parts, ConfigPathSource, RuntimePaths};

    #[test]
    fn persisted_profile_id_encoding_is_stable() {
        #[cfg(unix)]
        assert_eq!(
            super::profile_id(std::path::Path::new("/profiles/travel.yaml")),
            "c63470fcbe3b2d2241bbdf6030603413f3eebc98b8b7287e1f53b32e9ade5545"
        );
        #[cfg(windows)]
        {
            assert_eq!(
                super::profile_id(std::path::Path::new(r"C:\profiles\travel.yaml")),
                "0b8ad46865d2941794a15c90f2f8c780a2897c304b063d5e44e80f0ecbc03f46"
            );
            assert!(super::is_standard_config(std::path::Path::new(
                "CONFIG.YAML"
            )));
        }
    }

    #[test]
    fn runtime_bindings_follow_the_resolved_config_profile() {
        let default = RuntimePaths::new(
            PathBuf::from("/tmp/runtime"),
            std::path::Path::new("/etc/miao/config.yaml"),
        );
        assert_eq!(
            default.node_bindings,
            PathBuf::from("/etc/miao/node-bindings.json")
        );

        let profile = RuntimePaths::new(
            PathBuf::from("/tmp/runtime"),
            std::path::Path::new("/etc/miao/travel.yaml"),
        );
        assert_eq!(
            profile.node_bindings,
            super::profile_state_dir(std::path::Path::new("/etc/miao/travel.yaml"))
                .join("node-bindings.json")
        );
        assert_eq!(
            default.last_proxy,
            PathBuf::from("/tmp/runtime/.last_proxy")
        );
        assert_eq!(
            default.node_select_preference,
            PathBuf::from("/tmp/runtime/.node_select")
        );
        assert_eq!(
            default.max_multiplier_preference,
            PathBuf::from("/tmp/runtime/.max_multiplier")
        );
        let persistent = default.with_preferences(
            PathBuf::from("/etc/miao/.last_proxy"),
            PathBuf::from("/etc/miao/.node_select"),
            PathBuf::from("/etc/miao/.max_multiplier"),
        );
        assert_eq!(
            persistent.last_proxy,
            PathBuf::from("/etc/miao/.last_proxy")
        );
        assert_eq!(
            persistent.node_select_preference,
            PathBuf::from("/etc/miao/.node_select")
        );
        assert_eq!(
            persistent.max_multiplier_preference,
            PathBuf::from("/etc/miao/.max_multiplier")
        );
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
