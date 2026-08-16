#[cfg_attr(not(windows), allow(dead_code))]
pub const WINDOW_TITLE: &str = "Miao";
/// TUN 是整机全局资源：优先用 Global 命名空间，防止快速用户切换后在另一个
/// 会话里再跑一个实例抢 Wintun。非提权令牌可能无权创建/打开 Global 对象，
/// 因此保留 Local 兜底。
#[cfg_attr(not(windows), allow(dead_code))]
const MUTEX_GLOBAL_NAME: &str = "Global\\io.github.yuxiangluo.miao";
#[cfg_attr(not(windows), allow(dead_code))]
const MUTEX_LOCAL_NAME: &str = "Local\\io.github.yuxiangluo.miao";

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
fn mutex_name_wide(name: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(windows)]
enum MutexOpen {
    Exists,
    Missing,
    /// ACCESS_DENIED：一个高完整性级别的实例持有了它，视为已在运行。
    Denied,
    Failed,
}

#[cfg(windows)]
fn open_mutex(name: &str) -> MutexOpen {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND,
    };
    use windows_sys::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};

    let name = mutex_name_wide(name);
    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
    let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
    if !handle.is_null() {
        unsafe {
            CloseHandle(handle);
        }
        return MutexOpen::Exists;
    }
    match unsafe { GetLastError() } {
        ERROR_FILE_NOT_FOUND => MutexOpen::Missing,
        ERROR_ACCESS_DENIED => MutexOpen::Denied,
        _ => MutexOpen::Failed,
    }
}

#[cfg(windows)]
fn windows_peek() -> InstancePeek {
    let mut saw_failure = false;
    for name in [MUTEX_GLOBAL_NAME, MUTEX_LOCAL_NAME] {
        match open_mutex(name) {
            MutexOpen::Exists | MutexOpen::Denied => return InstancePeek::AlreadyRunning,
            MutexOpen::Missing => {}
            MutexOpen::Failed => saw_failure = true,
        }
    }
    if saw_failure {
        InstancePeek::Failed
    } else {
        InstancePeek::None
    }
}

#[cfg(windows)]
enum MutexCreate {
    Acquired(InstanceGuard),
    AlreadyRunning,
    /// 创建失败（如无权限创建 Global 对象），可尝试下一个命名空间。
    Failed,
}

#[cfg(windows)]
fn create_mutex(name: &str) -> MutexCreate {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name = mutex_name_wide(name);
    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return MutexCreate::Failed;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(handle);
        }
        return MutexCreate::AlreadyRunning;
    }
    MutexCreate::Acquired(InstanceGuard { handle })
}

#[cfg(windows)]
fn windows_acquire() -> InstanceAcquire {
    for name in [MUTEX_GLOBAL_NAME, MUTEX_LOCAL_NAME] {
        match create_mutex(name) {
            MutexCreate::Acquired(guard) => return InstanceAcquire::Unique(guard),
            MutexCreate::AlreadyRunning => return InstanceAcquire::AlreadyRunning,
            MutexCreate::Failed => {}
        }
    }
    InstanceAcquire::Failed
}

/// 仅凭窗口标题找人会误中同名窗口；枚举窗口后按进程镜像名（miao.exe）
/// 校验。未提权令牌对提权进程做 PROCESS_QUERY_LIMITED_INFORMATION 通常
/// 仍被允许；校验不了时退回第一个标题匹配，保持旧行为。
#[cfg(windows)]
struct FindContext {
    verified: windows_sys::Win32::Foundation::HWND,
    fallback: windows_sys::Win32::Foundation::HWND,
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_miao_process_image(image: &str) -> bool {
    image
        .rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("miao.exe"))
}

#[cfg(windows)]
unsafe extern "system" fn enum_find_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId};

    let context = &mut *(lparam as *mut FindContext);
    if !context.verified.is_null() {
        return 0;
    }

    // 托盘隐藏窗口也要能找到，不按可见性过滤
    let mut title = [0u16; 256];
    let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
    if len <= 0 || String::from_utf16_lossy(&title[..len as usize]) != WINDOW_TITLE {
        return 1;
    }
    if context.fallback.is_null() {
        context.fallback = hwnd;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return 1;
    }
    let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if process.is_null() {
        return 1;
    }
    let mut image = [0u16; 1024];
    let mut size = image.len() as u32;
    let queried = QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut size);
    CloseHandle(process);
    if queried != 0 && is_miao_process_image(&String::from_utf16_lossy(&image[..size as usize])) {
        context.verified = hwnd;
        return 0;
    }
    1
}

#[cfg(windows)]
fn windows_focus_existing() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let mut context = FindContext {
        verified: std::ptr::null_mut(),
        fallback: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(
            Some(enum_find_window_proc),
            (&mut context as *mut FindContext) as windows_sys::Win32::Foundation::LPARAM,
        );
    }

    let hwnd = if !context.verified.is_null() {
        context.verified
    } else {
        context.fallback
    };
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
    fn mutex_names_cover_global_and_local_namespaces() {
        assert!(super::MUTEX_GLOBAL_NAME.starts_with("Global\\"));
        assert!(super::MUTEX_LOCAL_NAME.starts_with("Local\\"));
        assert!(super::MUTEX_GLOBAL_NAME.contains("miao"));
        assert_eq!(super::WINDOW_TITLE, "Miao");
    }

    #[test]
    fn process_image_matches_only_miao_exe() {
        use super::is_miao_process_image;

        assert!(is_miao_process_image(r"C:\Users\Jane\miao.exe"));
        assert!(is_miao_process_image("miao.exe"));
        assert!(is_miao_process_image(r"C:\Miao\MIAO.EXE"));
        assert!(!is_miao_process_image(r"C:\tools\miaow.exe"));
        assert!(!is_miao_process_image(r"C:\tools\miao.exe.bak"));
        assert!(!is_miao_process_image(r"C:\Windows\notepad.exe"));
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
