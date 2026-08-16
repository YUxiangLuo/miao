use tracing::error;

pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        nix::unistd::Uid::effective().is_root()
    }
    #[cfg(windows)]
    {
        windows_is_elevated()
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_is_elevated() -> bool {
    use windows_sys::Win32::UI::Shell::IsUserAnAdmin;
    unsafe { IsUserAnAdmin() != 0 }
}

#[cfg(windows)]
fn relaunch_elevated() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let args = std::env::args_os()
        .skip(1)
        .map(|arg| {
            let text = arg.to_string_lossy();
            if text.contains(char::is_whitespace) {
                format!("\"{text}\"")
            } else {
                text.into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let exe_wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let operation: Vec<u16> = "runas\0".encode_utf16().collect();
    let params: Vec<u16> = args.encode_utf16().chain(Some(0)).collect();

    let status = unsafe {
        ShellExecuteW(
            0 as HWND,
            operation.as_ptr(),
            exe_wide.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    let code = status as usize;
    if code <= 32 {
        return Err(format!("ShellExecuteW runas failed with status {code}"));
    }
    Ok(())
}

pub fn require_privileges() {
    if is_elevated() {
        return;
    }

    #[cfg(windows)]
    {
        match relaunch_elevated() {
            Ok(()) => std::process::exit(0),
            Err(err) => {
                error!(error = %err, "TUN requires administrator privileges");
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(windows))]
    {
        error!("This application must be run as root");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::is_elevated;

    #[test]
    fn is_elevated_returns_a_boolean() {
        let _ = is_elevated();
    }
}
