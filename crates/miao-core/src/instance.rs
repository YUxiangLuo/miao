#[cfg_attr(not(windows), allow(dead_code))]
pub const WINDOW_TITLE: &str = "Miao";
#[cfg_attr(not(windows), allow(dead_code))]
const MUTEX_NAME: &str = "Local\\io.github.yuxiangluo.miao";

pub enum InstanceAcquire {
    Unique(InstanceGuard),
    AlreadyRunning,
    Failed,
}

/// Probe without holding the lock. Safe from an unelevated token: a high-IL
/// mutex owned by the elevated instance returns ACCESS_DENIED, which we treat
/// as already running so a second double-click does not prompt for UAC.
pub enum InstancePeek {
    None,
    AlreadyRunning,
    Failed,
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

pub fn peek_single_instance() -> InstancePeek {
    #[cfg(windows)]
    {
        windows_peek()
    }
    #[cfg(not(windows))]
    {
        InstancePeek::None
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
fn mutex_name_wide() -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(MUTEX_NAME)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(windows)]
fn windows_peek() -> InstancePeek {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND,
    };
    use windows_sys::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

    let name = mutex_name_wide();
    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
    let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
    if !handle.is_null() {
        unsafe {
            CloseHandle(handle);
        }
        return InstancePeek::AlreadyRunning;
    }
    match unsafe { GetLastError() } {
        ERROR_FILE_NOT_FOUND => InstancePeek::None,
        ERROR_ACCESS_DENIED => InstancePeek::AlreadyRunning,
        _ => InstancePeek::Failed,
    }
}

#[cfg(windows)]
fn windows_acquire() -> InstanceAcquire {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name = mutex_name_wide();
    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return InstanceAcquire::Failed;
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

    #[cfg(not(windows))]
    #[test]
    fn peek_is_none_on_unix() {
        assert!(matches!(
            super::peek_single_instance(),
            super::InstancePeek::None
        ));
        assert!(matches!(
            super::acquire_single_instance(),
            super::InstanceAcquire::Unique(_)
        ));
    }
}
