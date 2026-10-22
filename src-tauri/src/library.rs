//! Lanzar juegos instalados, contar tiempo de juego y desinstalar.

use crate::store::now;
use crate::AppState;
use anyhow::Context;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

pub fn launch(app: AppHandle, state: Arc<AppState>, key: &str) -> anyhow::Result<()> {
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
        if let Some(g) = state.store.library.lock().unwrap().get_mut(&key) {
            g.playtime_secs += secs;
            g.last_played = Some(now());
        }
        let _ = state.store.save_library();
        // El perfil cuenta por juego y no se pierde al desinstalar; puede desbloquear logros.
        crate::profile::record_session(&mut state.store.profile.lock().unwrap(), &key, secs);
        crate::profile_changed(&app, &state);
        state.running.lock().unwrap().remove(&key);
        if state.running.lock().unwrap().is_empty() {
            crate::tray::set_playing(&app, None);
            crate::discord::set(true, None);
        }
        let _ = app.emit("game-state", (&key, false));
        let _ = app.emit("library-changed", ());
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
