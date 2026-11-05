//! Lanzar juegos instalados, contar tiempo de juego y desinstalar.

use crate::store::now;
use crate::AppState;
use anyhow::Context;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn launch(app: AppHandle, state: Arc<AppState>, key: &str) -> anyhow::Result<()> {
    // Juego de navegador: se juega sin añadirlo a la biblioteca; su página se abre en una ventana propia.
    if let Some(g) = crate::catalog::get(key).filter(|g| g.web.is_some()) {
        return launch_web(app, state, key, &g.title, g.web.as_deref().unwrap_or_default());
    }
    let game = state
        .store
        .library
        .lock()
        .unwrap()
        .get(key)
        .cloned()
        .context(tr!("el juego no está instalado", "the game isn't installed"))?;
    if !state.running.lock().unwrap().insert(key.to_string()) {
        anyhow::bail!(tr!("{} ya está en ejecución", "{} is already running", game.title));
    }

    let root = PathBuf::from(&game.path);
    let result = (|| {
        let exe_rel = game
            .exe
            .as_deref()
            .context(tr!("no se encontró el ejecutable; ábrelo desde la carpeta del juego", "the executable wasn't found; open it from the game folder"))?;
        let exe = root.join(exe_rel);
        anyhow::ensure!(exe.is_file(), tr!("no existe {}", "{} doesn't exist", exe.display()));
        let cwd = exe.parent().map(|p| p.to_path_buf()).unwrap_or(root.clone());
        // Sin preparar todavía (p. ej. recompilar el juego con tus datos): se abre su instalador en su lugar.
        let setup = crate::catalog::get(key).and_then(|g| g.setup.clone()).filter(|s| !s.done.iter().any(|d| cwd.join(d).exists()));
        let preparing = setup.is_some();
        let exe = setup.map(|s| cwd.join(s.exe)).unwrap_or(exe);
        std::process::Command::new(&exe)
            .current_dir(cwd)
            .spawn()
            .map(|c| (c, preparing))
            .with_context(|| tr!("no se pudo iniciar {}", "couldn't start {}", exe.display()))
    })();
    let (mut child, preparing) = match result {
        Ok(c) => c,
        Err(e) => {
            state.running.lock().unwrap().remove(key);
            return Err(e);
        }
    };

    let _ = app.emit("game-state", (key, true));
    crate::tray::set_playing(&app, Some(&game.title));
    crate::discord::set(state.store.settings.lock().unwrap().discord_presence, Some(&game.title));
    let key = key.to_string();
    // Carpetas cuyos programas cuentan como "el juego": la del port y, si la instaló Dársena, la del juego
    // (con Datos). Un juego vinculado a mano solo usa la carpeta de su ejecutable.
    let mut dirs = vec![root.clone()];
    if !game.linked {
        if let Some(parent) = root.parent() {
            dirs.push(parent.to_path_buf());
        }
    }
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        // El tiempo de preparación no cuenta como tiempo de juego.
        let tracked = track(&mut child, &dirs, started);
        let secs = if preparing { 0 } else { tracked };
        end_session(&app, &state, &key, secs);
    });
    Ok(())
}

/// Cierra una sesión de juego: suma el tiempo a la biblioteca y al perfil (que puede desbloquear logros) y avisa a la interfaz.
fn end_session(app: &AppHandle, state: &Arc<AppState>, key: &str, secs: u64) {
    if let Some(g) = state.store.library.lock().unwrap().get_mut(key) {
        g.playtime_secs += secs;
        g.last_played = Some(now());
    }
    let _ = state.store.save_library();
    // El perfil cuenta por juego y no se pierde al desinstalar.
    crate::profile::record_session(&mut state.store.profile.lock().unwrap(), key, secs);
    crate::profile_changed(app, state);
    state.running.lock().unwrap().remove(key);
    if state.running.lock().unwrap().is_empty() {
        crate::tray::set_playing(app, None);
        crate::discord::set(true, None);
    }
    let _ = app.emit("game-state", (key, false));
    let _ = app.emit("library-changed", ());
}

/// Abre un juego de navegador en su propia ventana. La ventana no tiene acceso a Dársena (ver capabilities) y solo
/// puede navegar por la web del juego: los enlaces a otros sitios se abren en tu navegador. El tiempo cuenta mientras
/// la ventana esté abierta.
fn launch_web(app: AppHandle, state: Arc<AppState>, key: &str, title: &str, url: &str) -> anyhow::Result<()> {
    let label: String = format!("web-{}", key.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>());
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let parsed: tauri::Url = url.parse().context("la dirección del juego no es válida")?;
    anyhow::ensure!(parsed.scheme() == "https", tr!("la página del juego debe ser https", "the game's page must be https"));
    let host = parsed.host_str().context("la dirección del juego no tiene servidor")?.to_string();
    if !state.running.lock().unwrap().insert(key.to_string()) {
        anyhow::bail!(tr!("{} ya está en ejecución", "{} is already running", title));
    }
    // La ventana se crea fuera del hilo del comando: en Windows, crearla desde un comando síncrono bloquea la aplicación.
    let title = title.to_string();
    let key = key.to_string();
    std::thread::spawn(move || {
        let built = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
            .title(&title)
            .inner_size(1280.0, 720.0)
            .min_inner_size(640.0, 400.0)
            .center()
            .on_navigation(move |u| {
                if !matches!(u.scheme(), "http" | "https") {
                    return true;
                }
                let same = u.scheme() == "https" && u.host_str().is_some_and(|h| h == host || h.ends_with(&format!(".{host}")));
                if !same {
                    let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
                }
                same
            })
            .build();
        let win = match built {
            Ok(w) => w,
            Err(e) => {
                state.running.lock().unwrap().remove(&key);
                use tauri_plugin_dialog::DialogExt;
                app.dialog().message(format!("{e}")).title("Dársena").show(|_| {});
                return;
            }
        };
        let _ = app.emit("game-state", (&key, true));
        crate::tray::set_playing(&app, Some(&title));
        crate::discord::set(state.store.settings.lock().unwrap().discord_presence, Some(&title));
        let (started, app2) = (std::time::Instant::now(), app.clone());
        win.on_window_event(move |ev| {
            if let tauri::WindowEvent::Destroyed = ev {
                end_session(&app2, &state, &key, started.elapsed().as_secs());
            }
        });
    });
    Ok(())
}

/// Espera a que termine el juego y devuelve los segundos jugados.
///
/// Muchos ports se abren con un launcher que arranca el juego y se cierra enseguida. Por eso no basta con
/// esperar al proceso lanzado: se sigue a sus descendientes y a cualquier programa que se ejecute desde la
/// carpeta del juego, y la sesión acaba cuando ya no queda ninguno.
fn track(child: &mut std::process::Child, dirs: &[PathBuf], started: std::time::Instant) -> u64 {
    use std::collections::HashSet;
    const POLL: std::time::Duration = std::time::Duration::from_secs(2);
    // Rutas en minúsculas y terminadas en separador, para comparar sin distinguir mayúsculas.
    // Una carpeta demasiado general (la raíz de un disco) no sirve para reconocer el juego.
    let prefixes: Vec<String> = dirs
        .iter()
        .map(|d| d.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase() + "\\")
        .filter(|d| d.len() > 4)
        .collect();
    let mut tracked: HashSet<u32> = HashSet::from([child.id()]);
    let mut last_alive = started.elapsed();
    let mut misses = 0;
    loop {
        let launcher_alive = matches!(child.try_wait(), Ok(None));
        let procs = crate::procs::list(!prefixes.is_empty());
        let live: HashSet<u32> = procs.iter().map(|p| p.pid).collect();
        // Descendientes: los hijos de lo que ya seguimos (el juego que abre el launcher, y los suyos).
        loop {
            let new: Vec<u32> = procs
                .iter()
                .filter(|p| tracked.contains(&p.parent) && !tracked.contains(&p.pid))
                .map(|p| p.pid)
                .collect();
            if new.is_empty() {
                break;
            }
            tracked.extend(new);
        }
        tracked.retain(|pid| live.contains(pid));
        let in_folder = procs.iter().any(|p| {
            p.path.as_ref().is_some_and(|path| {
                let path = path.to_lowercase();
                prefixes.iter().any(|d| path.starts_with(d))
            })
        });
        if launcher_alive || !tracked.is_empty() || in_folder {
            last_alive = started.elapsed();
            misses = 0;
        } else {
            // Tres comprobaciones seguidas sin nada (6 s): entre que el launcher se cierra y el juego aparece
            // puede pasar un momento.
            misses += 1;
            if misses >= 3 {
                break;
            }
        }
        std::thread::sleep(POLL);
    }
    last_alive.as_secs()
}

pub fn uninstall(state: &AppState, key: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !state.running.lock().unwrap().contains(key),
        tr!("cierra el juego antes de desinstalarlo", "close the game before uninstalling it")
    );
    let game = state
        .store
        .library
        .lock()
        .unwrap()
        .remove(key)
        .context(tr!("el juego no está instalado", "the game isn't installed"))?;
    state.store.save_library()?;
    // Un juego vinculado (instalado fuera de Dársena) solo se olvida: sus archivos son tuyos.
    if game.linked {
        return Ok(());
    }
    // Lo que se compartía por torrent (la sesión ya lo dejó de sembrar antes de llamar aquí).
    if let Some(seed) = &game.seed {
        let _ = std::fs::remove_dir_all(&seed.dir);
        let _ = std::fs::remove_file(&seed.torrent);
        if let Some(shared) = PathBuf::from(&seed.dir).parent() {
            let _ = std::fs::remove_dir(shared);
        }
    }
    let path = PathBuf::from(&game.path);
    if path.exists() {
        std::fs::remove_dir_all(&path)
            .with_context(|| tr!("no se pudo borrar {}", "couldn't delete {}", path.display()))?;
    }
    // La carpeta del juego solo se borra si queda vacía: la carpeta Datos (tus datos) se conserva.
    if let Some(game_dir) = path.parent() {
        let _ = std::fs::remove_dir(game_dir.join(".compartido"));
        let _ = std::fs::remove_dir(game_dir.join("Datos"));
        let _ = std::fs::remove_dir(game_dir);
    }
    Ok(())
}
