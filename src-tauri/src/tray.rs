//! Segundo plano: icono en la bandeja del sistema, abrir juegos desde accesos directos (`--launch <port>`)
//! y accesos directos en el escritorio.

use crate::AppState;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";

/// Id del port que pide abrir un acceso directo: `darsena.exe --launch <id>`.
pub fn launch_arg<S: AsRef<str>>(args: &[S]) -> Option<String> {
    let i = args.iter().position(|a| a.as_ref() == "--launch")?;
    args.get(i + 1).map(|s| s.as_ref().to_string()).filter(|s| !s.is_empty())
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", tr!("Abrir Dársena", "Open Dársena"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr!("Salir", "Quit"), true, None::<&str>)?;
    Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit])
}

/// Vuelve a poner el menú de la bandeja en el idioma actual.
pub fn refresh_menu(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Icono de la bandeja con su menú. Un clic abre la ventana.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Dársena")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Texto del icono de la bandeja: qué juego está abierto.
pub fn set_playing(app: &AppHandle, title: Option<&str>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let text = match title {
            Some(t) => tr!("Dársena · Jugando a {}", "Dársena · Playing {}", t),
            None => "Dársena".to_string(),
        };
        let _ = tray.set_tooltip(Some(text));
    }
}

/// Abre un juego pedido por un acceso directo. Si falla, se enseña la ventana con el error.
pub fn launch_from_shortcut(app: &AppHandle, id: &str) {
    let Some(state) = app.try_state::<Arc<AppState>>() else { return };
    if let Err(e) = crate::library::launch(app.clone(), state.inner().clone(), id) {
        show_main(app);
        use tauri_plugin_dialog::DialogExt;
        app.dialog().message(format!("{e:#}")).title("Dársena").show(|_| {});
    }
}

/// Crea en el escritorio un acceso directo que abre el juego con Dársena en segundo plano.
pub fn create_shortcut(app: &AppHandle, state: &AppState, id: &str) -> anyhow::Result<PathBuf> {
    let game = state
        .store
        .library
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!(tr!("el juego no está instalado", "the game isn't installed")))?;
    let title = crate::catalog::get(id).map(|g| crate::catalog::game_title(&g.game)).unwrap_or(game.title.clone());
    let desktop = app.path().desktop_dir()?;
    let name: String = title.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) { ' ' } else { c }).collect();
    let lnk = desktop.join(format!("{}.lnk", name.split_whitespace().collect::<Vec<_>>().join(" ")));
    let target = std::env::current_exe()?;
    // El icono del acceso directo es el del propio juego (si su ejecutable es un .exe).
    let exe = game.exe.as_deref().map(|e| PathBuf::from(&game.path).join(e));
    let icon = exe.filter(|e| e.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) && e.is_file()).unwrap_or(target.clone());

    // WScript.Shell crea el .lnk; los valores van por variables de entorno para no tener que escaparlos.
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:DS_LNK); \
        $s.TargetPath = $env:DS_TARGET; $s.Arguments = $env:DS_ARGS; $s.IconLocation = $env:DS_ICON + ',0'; \
        $s.WorkingDirectory = $env:DS_DIR; $s.Description = $env:DS_DESC; $s.Save()";
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("DS_LNK", &lnk)
        .env("DS_TARGET", &target)
        .env("DS_ARGS", format!("--launch {id}"))
        .env("DS_ICON", &icon)
        .env("DS_DIR", target.parent().unwrap_or(&target))
        .env("DS_DESC", tr!("Jugar a {} con Dársena", "Play {} with Dársena", title));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // sin ventana de consola
    }
    let out = cmd.output()?;
    anyhow::ensure!(out.status.success() && lnk.is_file(), tr!("no se pudo crear el acceso directo: {}", "couldn't create the shortcut: {}", String::from_utf8_lossy(&out.stderr).trim()));
    Ok(lnk)
}
