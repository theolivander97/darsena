//! Arrancar Dársena al iniciar sesión en Windows (queda en la bandeja, sin abrir la ventana).
//!
//! Se registra en HKCU\...\Run, que no necesita permisos de administrador y es lo que Windows enseña
//! en «Aplicaciones de inicio». Solo se toca en la versión instalada, no al desarrollar.

/// Argumento con el que Windows lanza Dársena al iniciar sesión.
pub const ARG: &str = "--inicio-windows";

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE: &str = "Darsena";

/// Pone o quita la entrada de inicio según el ajuste.
pub fn apply(enabled: bool) {
    if cfg!(debug_assertions) {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let Ok(exe) = std::env::current_exe() else { return };
        let mut cmd = std::process::Command::new("reg");
        if enabled {
            cmd.args(["add", RUN_KEY, "/v", VALUE, "/t", "REG_SZ", "/d", &format!("\"{}\" {ARG}", exe.display()), "/f"]);
        } else {
            cmd.args(["delete", RUN_KEY, "/v", VALUE, "/f"]);
        }
        let _ = cmd.creation_flags(0x0800_0000).output();
    }
    #[cfg(not(windows))]
    let _ = enabled;
}

/// ¿Se abrió porque Windows acaba de iniciar sesión?
pub fn launched_by_windows<S: AsRef<str>>(args: &[S]) -> bool {
    args.iter().any(|a| a.as_ref() == ARG)
}
