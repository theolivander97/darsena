//! Procesos en ejecución (id, padre y ruta del ejecutable), para seguir a un juego aunque lo abra un launcher.

pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    /// Ruta completa del ejecutable, si se puede leer (los procesos del sistema no la dan).
    pub path: Option<String>,
}

#[cfg(windows)]
mod win {
    #[repr(C)]
    pub struct ProcessEntry32W {
        pub size: u32,
        pub usage: u32,
        pub pid: u32,
        pub heap: usize,
        pub module: u32,
        pub threads: u32,
        pub parent: u32,
        pub pri: i32,
        pub flags: u32,
        pub exe: [u16; 260],
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        pub fn Process32FirstW(snap: isize, entry: *mut ProcessEntry32W) -> i32;
        pub fn Process32NextW(snap: isize, entry: *mut ProcessEntry32W) -> i32;
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        pub fn QueryFullProcessImageNameW(process: isize, flags: u32, name: *mut u16, size: *mut u32) -> i32;
        pub fn CloseHandle(handle: isize) -> i32;
    }
}

#[cfg(windows)]
fn image_path(pid: u32) -> Option<String> {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let h = win::OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h == 0 {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = win::QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        win::CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Todos los procesos del equipo. `with_paths`: leer también la ruta de cada uno (más lento).
#[cfg(windows)]
pub fn list(with_paths: bool) -> Vec<Proc> {
    const TH32CS_SNAPPROCESS: u32 = 2;
    const INVALID: isize = -1;
    let mut out = Vec::new();
    unsafe {
        let snap = win::CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID {
            return out;
        }
        let mut e: win::ProcessEntry32W = std::mem::zeroed();
        e.size = std::mem::size_of::<win::ProcessEntry32W>() as u32;
        let mut ok = win::Process32FirstW(snap, &mut e);
        while ok != 0 {
            out.push(Proc { pid: e.pid, parent: e.parent, path: None });
            ok = win::Process32NextW(snap, &mut e);
        }
        win::CloseHandle(snap);
    }
    if with_paths {
        for p in &mut out {
            p.path = image_path(p.pid);
        }
    }
    out
}

#[cfg(not(windows))]
pub fn list(_with_paths: bool) -> Vec<Proc> {
    Vec::new()
}
