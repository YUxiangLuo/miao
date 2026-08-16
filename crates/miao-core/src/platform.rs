/// In-app binary replacement is Unix-only. Windows cannot overwrite a running
/// exe the way Linux `exec` does; first release tells users to install a new package.
pub fn upgrade_supported() -> bool {
    !cfg!(windows)
}

/// VPS one-click deploy writes an OpenSSH askpass shell script. Hide it on Windows.
pub fn vps_supported() -> bool {
    !cfg!(windows)
}

#[cfg(test)]
mod tests {
    use super::{upgrade_supported, vps_supported};

    #[test]
    fn self_update_and_vps_follow_host_os() {
        assert_eq!(upgrade_supported(), !cfg!(windows));
        assert_eq!(vps_supported(), !cfg!(windows));
    }
}
