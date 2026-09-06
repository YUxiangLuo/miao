//! CLI-only launch configuration. Desktop/SDK callers keep using RuntimeOptions.
use std::ffi::OsString;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::models::{NodeSelect, Region, StableConfig};
use crate::runtime::RuntimeOptions;
use crate::validation::Validator;

pub(crate) const HELP: &str = "Usage:
  miao [--config PATH]
  miao --sub URL [HK|JP|TW|SG|US]
  miao --help
  miao --version

Options:
  --config PATH   Use a configuration file (also --config=PATH).
  --sub URL       Use one Clash YAML subscription (also --sub=URL).
                  Mutually exclusive with --config. Uses a temporary profile;
                  existing configuration and preferences are not changed.
  HK|JP|TW|SG|US  Select the fastest node in that region (case-insensitive).
                  Only valid with --sub; omitted means manual selection.
  -h, --help      Show this help without starting the proxy.
  -V, --version   Show the version without starting the proxy.

Examples:
  sudo ./miao --sub 'https://example.com/sub?token=xxx' JP
  sudo ./miao --config /etc/miao/config.yaml

The temporary profile is deleted on normal exit; panel edits are temporary too.
Quote subscription URLs containing shell characters such as '&'.
Stop any existing miao instance before starting another (shared TUN/API ports).";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Run(LaunchSource),
    Help,
    Version,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LaunchSource {
    Default,
    Config(PathBuf),
    Subscription {
        url: String,
        node_select: NodeSelect,
    },
}

fn required_value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> AppResult<OsString> {
    let value = args
        .next()
        .ok_or_else(|| AppError::message(format!("{flag} requires a value")))?;
    if value.is_empty() || value.to_str().is_some_and(|value| value.starts_with('-')) {
        return Err(AppError::message(format!("{flag} requires a value")));
    }
    Ok(value)
}

/// Parse before checking privileges or touching configuration/runtime files.
/// OsString preserves non-UTF-8 config paths when passed as a separate value.
pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> AppResult<Command> {
    let mut args = args.into_iter();
    let mut config = None;
    let mut subscription = None;
    let mut region = None;
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            return Ok(Command::Help);
        }
        if arg == "--version" || arg == "-V" {
            return Ok(Command::Version);
        }
        let text = arg.to_str();
        if arg == "--config" || text.is_some_and(|arg| arg.starts_with("--config=")) {
            if config.is_some() {
                return Err(AppError::message("--config may only be supplied once"));
            }
            let value = match text.and_then(|arg| arg.strip_prefix("--config=")) {
                Some(value) => OsString::from(value),
                None => required_value(&mut args, "--config")?,
            };
            if value.is_empty() {
                return Err(AppError::message("--config requires a path"));
            }
            config = Some(PathBuf::from(value));
        } else if arg == "--sub" || text.is_some_and(|arg| arg.starts_with("--sub=")) {
            if subscription.is_some() {
                return Err(AppError::message("--sub may only be supplied once"));
            }
            let value = match text.and_then(|arg| arg.strip_prefix("--sub=")) {
                Some(value) => OsString::from(value),
                None => required_value(&mut args, "--sub")?,
            };
            subscription = Some(
                value
                    .into_string()
                    .map_err(|_| AppError::message("--sub URL must be valid UTF-8"))?,
            );
        } else if text.is_some_and(|arg| arg.starts_with('-')) {
            return Err(AppError::message(format!(
                "Unknown option: {} (see --help)",
                arg.to_string_lossy()
            )));
        } else {
            if region.is_some() {
                return Err(AppError::message(
                    "Only one region may be supplied (HK/JP/TW/SG/US)",
                ));
            }
            region = Some(
                arg.into_string()
                    .map_err(|_| AppError::message("Region must be HK/JP/TW/SG/US"))?,
            );
        }
    }

    if config.is_some() && subscription.is_some() {
        return Err(AppError::message(
            "--sub and --config are mutually exclusive",
        ));
    }
    if let Some(url) = subscription {
        Validator::subscription_url(&url)
            .map_err(|err| AppError::message(format!("--sub: {err}")))?;
        let node_select = match region.as_deref().map(str::to_ascii_uppercase).as_deref() {
            None => NodeSelect::Manual,
            Some("HK") => NodeSelect::Fastest(Region::Hk),
            Some("JP") => NodeSelect::Fastest(Region::Jp),
            Some("TW") => NodeSelect::Fastest(Region::Tw),
            Some("SG") => NodeSelect::Fastest(Region::Sg),
            Some("US") => NodeSelect::Fastest(Region::Us),
            Some(_) => return Err(AppError::message("Unsupported region; use HK/JP/TW/SG/US")),
        };
        return Ok(Command::Run(LaunchSource::Subscription {
            url,
            node_select,
        }));
    }
    if region.is_some() {
        return Err(AppError::message(
            "A region is only valid with --sub (see --help)",
        ));
    }
    Ok(Command::Run(match config {
        Some(path) => LaunchSource::Config(path),
        None => LaunchSource::Default,
    }))
}

/// Shells supporting --config can reuse the CLI parser after removing their
/// own flags (e.g. --minimized). SDK startup never implicitly reads argv.
pub fn config_path_from_args(
    args: impl IntoIterator<Item = OsString>,
) -> AppResult<Option<PathBuf>> {
    match parse(args)? {
        Command::Run(LaunchSource::Default) => Ok(None),
        Command::Run(LaunchSource::Config(path)) => Ok(Some(crate::paths::absolutize(path)?)),
        _ => Err(AppError::message(
            "This shell supports --config PATH; use the CLI for --sub/--help/--version",
        )),
    }
}

/// Own the temporary directory until handed to runtime::spawn_prepared, which
/// transfers it to AppState. Failed preparation/startup drops it normally;
/// SIGKILL/power loss can still leave files behind.
pub(crate) struct PreparedLaunch {
    pub options: RuntimeOptions,
    pub(crate) profile: Option<tempfile::TempDir>,
}

pub(crate) fn prepare(source: LaunchSource) -> AppResult<PreparedLaunch> {
    let mut options = RuntimeOptions {
        open_browser: true,
        install_tracing: true,
        ..RuntimeOptions::default()
    };
    let profile = match source {
        LaunchSource::Default => None,
        LaunchSource::Config(path) => {
            options.config_path = Some(crate::paths::absolutize(path)?);
            None
        }
        LaunchSource::Subscription { url, node_select } => {
            // Create privately from the outset, before writing the URL/token.
            // Passing runtime_dir also isolates .last_proxy/.node_select/
            // .max_multiplier; node bindings follow the temporary config path.
            let mut builder = tempfile::Builder::new();
            builder.prefix("miao-cli-");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                builder.permissions(std::fs::Permissions::from_mode(0o700));
            }
            let profile = builder.tempdir()?;
            let config_path = profile.path().join("config.yaml");
            let config = StableConfig {
                subs: vec![url],
                node_select,
                ..StableConfig::default()
            };
            std::fs::write(&config_path, yaml_serde::to_string(&config)?)?;
            options.config_path = Some(config_path);
            options.runtime_dir = Some(profile.path().join("runtime"));
            options.volatile_path = Some(profile.path().join("volatile.yaml"));
            if cfg!(windows) {
                options.log_path = Some(profile.path().join("miao.log"));
            }
            Some(profile)
        }
    };
    Ok(PreparedLaunch { options, profile })
}

#[cfg(test)]
mod tests;
