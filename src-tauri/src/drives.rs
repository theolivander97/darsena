//! Discos del equipo (C:, D:…) con su espacio libre, para elegir dónde va la DarsenaLibrary.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    /// "C:\\"
    pub root: String,
    /// Nombre del volumen ("Windows", "Datos"…), si tiene.
    pub label: String,
    pub total: u64,
    pub free: u64,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetDiskFreeSpaceExW(dir: *const u16, free_to_caller: *mut u64, total: *mut u64, total_free: *mut u64) -> i32;
    fn GetVolumeInformationW(
        root: *const u16, name: *mut u16, name_len: u32, serial: *mut u32, max_comp: *mut u32, flags: *mut u32,
        fs_name: *mut u16, fs_len: u32,
    ) -> i32;
}

/// Discos fijos y extraíbles (sin lectores de CD ni unidades de red).
#[cfg(windows)]
pub fn list() -> Vec<Drive> {
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
        if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
            continue;
        }
        let (mut free, mut total, mut _all_free) = (0u64, 0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut _all_free) } == 0 {
            continue;
        }
        let mut name = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(wide.as_ptr(), name.as_mut_ptr(), name.len() as u32, std::ptr::null_mut(),
                std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0)
        };
        let label = if ok != 0 {
            String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(0)])
        } else {
            String::new()
        };
        out.push(Drive { root, label, total, free });
    }
    out
}

#[cfg(not(windows))]
pub fn list() -> Vec<Drive> {
    Vec::new()
}
