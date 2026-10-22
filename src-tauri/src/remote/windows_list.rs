//! Lista de ventanas capturables.

use serde::Serialize;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Serialize, Clone)]
pub struct WindowInfo {
    pub hwnd: i64,
    pub title: String,
    pub pid: u32,
    pub width: i32,
    pub height: i32,
}

unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return true.into();
    }
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let len = GetWindowTextLengthW(hwnd);
    if len == 0 {
        return true.into();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, &mut buf);
    let title = String::from_utf16_lossy(&buf[..n as usize]);
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w < 64 || h < 64 {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    out.push(WindowInfo { hwnd: hwnd.0 as i64, title, pid, width: w, height: h });
    true.into()
}

pub fn list_windows() -> Vec<WindowInfo> {
    let mut v: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut v as *mut _ as isize));
    }
    v
}
