//! Detecta si la ventana activa ocupa todo el monitor (juego, video,
//! presentación) para que Canela se aparte y no consuma recursos.
//! Sólo en Windows; en otros sistemas siempre devuelve `false`.

#[cfg(windows)]
pub fn foreground_is_fullscreen() -> bool {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect,
        GetWindowThreadProcessId,
    };

    // SAFETY: llamadas de sólo lectura a la API Win32 con búferes propios.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() || hwnd == GetShellWindow() || hwnd == GetDesktopWindow() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == std::process::id() {
            return false;
        }
        // El escritorio (fondo de pantalla) también "ocupa" todo el monitor.
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..n.max(0) as usize]);
        if class == "WorkerW" || class == "Progman" {
            return false;
        }
        let mut r: RECT = zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return false;
        }
        let m = info.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}

#[cfg(not(windows))]
pub fn foreground_is_fullscreen() -> bool {
    false
}
