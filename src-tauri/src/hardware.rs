//! «¿Funciona en tu PC?»: qué procesador, memoria y gráfica tiene el equipo. La comparación con los requisitos
//! de cada juego la hace la interfaz; aquí solo se lee el hardware (una vez por sesión).

use serde::Serialize;
use std::sync::OnceLock;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub cpu: String,
    /// Hilos lógicos.
    pub threads: usize,
    pub avx2: bool,
    pub ram_gb: f64,
    /// Gráficas (la dedicada suele ir primero; la interfaz se queda con la más potente).
    pub gpus: Vec<String>,
    /// Versión de Vulkan del controlador ("1.3"), si hay.
    pub vulkan: Option<String>,
}

static HW: OnceLock<Hardware> = OnceLock::new();

pub fn get() -> Hardware {
    HW.get_or_init(detect).clone()
}

#[cfg(windows)]
#[repr(C)]
struct MemoryStatusEx {
    length: u32,
    load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page: u64,
    avail_page: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_ext: u64,
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(status: *mut MemoryStatusEx) -> i32;
    fn LoadLibraryW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> isize;
}

#[cfg(windows)]
fn ram_gb() -> f64 {
    let mut s = MemoryStatusEx { length: std::mem::size_of::<MemoryStatusEx>() as u32, load: 0, total_phys: 0, avail_phys: 0, total_page: 0, avail_page: 0, total_virtual: 0, avail_virtual: 0, avail_ext: 0 };
    if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
        return 0.0;
    }
    s.total_phys as f64 / (1u64 << 30) as f64
}

/// Versión de Vulkan que admite el controlador: vkEnumerateInstanceVersion de vulkan-1.dll (sin crear nada).
#[cfg(windows)]
fn vulkan() -> Option<String> {
    let name: Vec<u16> = "vulkan-1.dll\0".encode_utf16().collect();
    let module = unsafe { LoadLibraryW(name.as_ptr()) };
    if module == 0 {
        return None;
    }
    let f = unsafe { GetProcAddress(module, b"vkEnumerateInstanceVersion\0".as_ptr()) };
    if f == 0 {
        return Some("1.0".into());
    }
    let enumerate: extern "system" fn(*mut u32) -> i32 = unsafe { std::mem::transmute(f) };
    let mut v = 0u32;
    (enumerate(&mut v) == 0).then(|| format!("{}.{}", (v >> 22) & 0x7f, (v >> 12) & 0x3ff))
}

/// Nombres del procesador y las gráficas, por WMI (PowerShell sin ventana).
#[cfg(windows)]
fn names() -> (String, Vec<String>) {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name); '---'; Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
        ])
        .creation_flags(0x0800_0000)
        .output();
    let Ok(out) = out else { return (String::new(), Vec::new()) };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut parts = text.split("---");
    let cpu = parts.next().unwrap_or_default().trim().to_string();
    let gpus = parts
        .next()
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        // Adaptadores que no son una gráfica de verdad.
        .filter(|l| !l.is_empty() && !l.contains("Basic Display") && !l.contains("Remote") && !l.contains("Virtual") && !l.contains("Parsec"))
        .collect();
    (cpu, gpus)
}

#[cfg(windows)]
fn detect() -> Hardware {
    let (cpu, gpus) = names();
    Hardware {
        cpu,
        threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        avx2: std::arch::is_x86_feature_detected!("avx2"),
        ram_gb: ram_gb(),
        gpus,
        vulkan: vulkan(),
    }
}

#[cfg(not(windows))]
fn detect() -> Hardware {
    Hardware { threads: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0), ..Default::default() }
}
