#[cfg_attr(not(windows), allow(dead_code))]
pub const WINDOW_TITLE: &str = "Miao";
#[cfg_attr(not(windows), allow(dead_code))]
const MUTEX_NAME: &str = "Local\\io.github.yuxiangluo.miao";

pub enum InstanceAcquire {
    Unique(InstanceGuard),
    AlreadyRunning,
}

pub struct InstanceGuard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        if !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

/// Windows desktop only. Linux CLI stays multi-instance (port bind is the lock).
pub fn acquire_single_instance() -> InstanceAcquire {
    #[cfg(windows)]
    {
        windows_acquire()
    }
    #[cfg(not(windows))]
    {
        InstanceAcquire::Unique(InstanceGuard {})
    }
}

pub fn focus_existing_window() -> bool {
    #[cfg(windows)]
    {
        windows_focus_existing()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn windows_acquire() -> InstanceAcquire {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = std::ffi::OsStr::new(MUTEX_NAME)
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return InstanceAcquire::Unique(InstanceGuard { handle });
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return InstanceAcquire::AlreadyRunning;
    }
    InstanceAcquire::Unique(InstanceGuard { handle })
}

#[cfg(windows)]
fn windows_focus_existing() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let title: Vec<u16> = format!("{WINDOW_TITLE}\0").encode_utf16().collect();
    let hwnd = unsafe { FindWindowW(std::ptr::null(), title.as_ptr()) };
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn mutex_name_is_local_namespace() {
        assert!(super::MUTEX_NAME.starts_with("Local\\"));
        assert!(super::MUTEX_NAME.contains("miao"));
        assert_eq!(super::WINDOW_TITLE, "Miao");
    }
}
