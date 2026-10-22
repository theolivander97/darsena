#[macro_use]
mod i18n;
mod auth;
mod catalog;
mod cloud;
mod comunidad;
mod covers;
mod devs;
mod discord;
mod drives;
mod downloads;
mod github;
mod hardware;
mod info;
mod inicio;
mod http_multi;
mod library;
mod procs;
mod profile;
#[cfg(feature = "remote")]
pub mod remote;
mod reviews;
mod sources;
mod submit;
mod webseed;
mod workshop;
mod store;
mod tray;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

pub struct AppState {
    pub store: store::Store,
    pub dl: downloads::DownloadManager,
    pub running: Mutex<HashSet<String>>,
}

/// Cliente HTTP compartido: reutiliza conexiones TLS entre peticiones.
pub fn http_client() -> &'static reqwest::Client {
    static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
        reqwest::Client::builder()
            .user_agent(concat!("Darsena/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("cliente HTTP")
    });
    &CLIENT
}

type S<'a> = State<'a, Arc<AppState>>;
type R<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

// ---------- Juego remoto (opcional: `--features remote`, ver docs/juego-remoto.md) ----------

#[cfg(feature = "remote")]
#[tauri::command]
fn remote_list_windows() -> Vec<remote::WindowInfo> {
    remote::list_windows()
}

#[cfg(feature = "remote")]
#[tauri::command]
async fn remote_start(hwnd: i64, fps: Option<u32>, bitrate_kbps: Option<u32>) -> R<remote::RemoteInfo> {
    remote::start(hwnd, fps.unwrap_or(30), bitrate_kbps.unwrap_or(6000)).await.map_err(err)
}

#[cfg(feature = "remote")]
#[tauri::command]
fn remote_stop() {
    remote::stop();
}

#[cfg(feature = "remote")]
#[tauri::command]
fn remote_status() -> Option<remote::RemoteInfo> {
    remote::status()
}

#[cfg(feature = "remote")]
#[tauri::command]
fn remote_guests() -> Vec<remote::GuestView> {
    remote::guests()
}

#[cfg(feature = "remote")]
#[tauri::command]
async fn remote_set_permissions(id: u32, pad: bool, keyboard: bool, mouse: bool) -> R<()> {
    remote::set_permissions(id, remote::Perms { pad, keyboard, mouse }).await.map_err(err)
}

#[cfg(feature = "remote")]
#[tauri::command]
fn remote_kick(id: u32) {
    remote::kick(id);
}

#[cfg(feature = "remote")]
#[tauri::command]
async fn remote_pause_input(paused: bool) {
    remote::set_input_paused(paused).await;
}

// ---------- Ajustes ----------

#[tauri::command]
fn get_settings(state: S) -> store::Settings {
    state.store.settings.lock().unwrap().clone()
}

/// Idioma de la interfaz: `preference` es lo elegido en Ajustes (None = el del sistema) y `effective`,
/// el idioma que resulta, que también usan los mensajes de Rust.
#[tauri::command]
fn set_language(app: AppHandle, state: S, preference: Option<String>, effective: String) -> R<()> {
    i18n::set(&effective);
    tray::refresh_menu(&app);
    let mut settings = state.store.settings.lock().unwrap();
    if settings.language != preference {
        settings.language = preference;
        drop(settings);
        state.store.save_settings().map_err(err)?;
    }
    Ok(())
}

/// Marca o desmarca un juego como favorito.
#[tauri::command]
fn set_favorite(state: S, game: String, value: bool) -> R<()> {
    {
        let mut s = state.store.settings.lock().unwrap();
        s.favorites.retain(|g| g != &game);
        if value {
            s.favorites.push(game);
        }
    }
    state.store.save_settings().map_err(err)
}

/// Estanterías de la página principal de la biblioteca.
#[tauri::command]
fn set_shelves(state: S, shelves: Vec<String>) -> R<()> {
    state.store.settings.lock().unwrap().shelves = shelves;
    state.store.save_settings().map_err(err)
}

/// Discos del equipo con su espacio libre (para Ajustes → Bibliotecas).
#[tauri::command]
fn list_drives() -> Vec<drives::Drive> {
    drives::list()
}

/// Crea la carpeta DarsenaLibrary en la raíz de un disco ("D:\\") y la usa para instalar a partir de ahora.
#[tauri::command]
fn use_library_drive(state: S, root: String) -> R<String> {
    let ok = root.len() == 3 && root.as_bytes()[0].is_ascii_alphabetic() && root.ends_with(":\\");
    if !ok {
        return Err(tr!("disco no válido", "invalid drive"));
    }
    let path = PathBuf::from(&root).join("DarsenaLibrary");
    std::fs::create_dir_all(&path).map_err(|e| tr!("no se pudo crear {}: {e}", "couldn't create {}: {e}", path.display()))?;
    let path = path.to_string_lossy().into_owned();
    state.store.settings.lock().unwrap().install_dir = path.clone();
    state.store.save_settings().map_err(err)?;
    Ok(path)
}

#[tauri::command]
fn set_install_dir(state: S, path: String) -> R<()> {
    std::fs::create_dir_all(&path).map_err(|e| tr!("carpeta no válida: {e}", "invalid folder: {e}"))?;
    state.store.settings.lock().unwrap().install_dir = path;
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn set_covers_origin(state: S, origin: String) -> R<()> {
    let origin = origin.trim().to_string();
    state.store.settings.lock().unwrap().covers_origin = (!origin.is_empty()).then_some(origin);
    state.store.save_settings().map_err(err)
}

/// Compartir en segundo plano lo descargado por torrent. Al desactivarlo se para y se borran las copias.
#[tauri::command]
async fn set_share_downloads(state: S<'_>, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().share_downloads = value;
    state.store.save_settings().map_err(err)?;
    if value {
        downloads::resume_seeding(state.inner().clone()).await;
        return Ok(());
    }
    state.dl.stop_all_seeding().await;
    let seeds: Vec<store::SeedInfo> = {
        let mut lib = state.store.library.lock().unwrap();
        lib.values_mut().filter_map(|g| g.seed.take()).collect()
    };
    for s in seeds {
        let _ = std::fs::remove_dir_all(&s.dir);
        let _ = std::fs::remove_file(&s.torrent);
    }
    state.store.save_library().map_err(err)
}

#[tauri::command]
fn set_upload_limit(state: S, kbps: u32) -> R<()> {
    state.store.settings.lock().unwrap().upload_limit_kbps = kbps;
    state.dl.set_upload_limit(kbps);
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn get_sharing_stats(state: S) -> downloads::SharingStats {
    state.dl.sharing_stats()
}

#[tauri::command]
fn set_github_token(state: S, token: String) -> R<()> {
    let token = token.trim().to_string();
    state.store.settings.lock().unwrap().github_token = (!token.is_empty()).then_some(token);
    state.store.save_settings().map_err(err)
}

// ---------- Catálogo ----------

#[tauri::command]
fn get_catalog() -> &'static [catalog::CatalogGame] {
    catalog::all()
}

// ---------- Actualizaciones (ventana de arranque) ----------

/// Comprueba el catálogo publicado en GitHub y, de paso, el índice de portadas del CDN.
#[tauri::command]
async fn check_catalog_update(state: S<'_>) -> R<catalog::CatalogUpdate> {
    let dir = state.store.data_dir().to_path_buf();
    let res = catalog::check_update(&dir).await.map_err(err)?;
    // Si falla el índice de portadas no pasa nada: se siguen usando las que ya había.
    let _ = covers::refresh_index(&dir).await;
    let _ = covers::refresh_logos(&dir).await;
    // Sinopsis y tráilers: igual, si falla se siguen usando los que ya había.
    let _ = info::refresh(&dir).await;
    Ok(res)
}

/// Invitación al servidor de Discord de Dársena (se lee de discord.json, así que se puede cambiar sin versión nueva).
#[tauri::command]
async fn get_discord_link(state: S<'_>) -> R<String> {
    let dir = state.store.data_dir().to_path_buf();
    Ok(comunidad::invite(&dir).await)
}

/// Sinopsis, tráilers y destacados de los juegos (generado por tools/catalog/informacion.mjs).
#[tauri::command]
fn get_game_info() -> &'static serde_json::Value {
    info::get()
}

#[tauri::command]
fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateInfo {
    version: String,
    current: String,
    notes: Option<String>,
}

/// ¿Hay una versión nueva del launcher en la última release de GitHub?
#[tauri::command]
async fn check_app_update(app: AppHandle) -> R<Option<AppUpdateInfo>> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| AppUpdateInfo { version: u.version.clone(), current: u.current_version.clone(), notes: u.body.clone() }))
}

/// Descarga la versión nueva (verificando su firma), la instala y reinicia el launcher.
#[tauri::command]
async fn install_app_update(app: AppHandle) -> R<()> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or(tr!("no hay ninguna actualización", "there's no update"))?;
    let progress = app.clone();
    let mut done: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                done += chunk as u64;
                let _ = progress.emit("app-update-progress", (done, total));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;
    app.restart()
}

/// Qué juegos tienen portada y su versión (para invalidar la caché).
#[tauri::command]
fn get_cover_index() -> HashMap<String, covers::Versions> {
    covers::index_with_logos()
}

/// Cuántas opciones de descarga aportan las fuentes activas a cada juego.
#[tauri::command]
fn get_source_counts(state: S) -> HashMap<&'static str, usize> {
    state.store.source_options().into_iter().map(|(k, v)| (k, v.len())).collect()
}

#[tauri::command]
fn get_source_options(state: S, id: String) -> Vec<store::SourceOption> {
    state.store.source_options().remove(id.as_str()).unwrap_or_default()
}

#[tauri::command]
async fn get_official_build(state: S<'_>, id: String) -> R<github::OfficialBuild> {
    let game = catalog::get(&id).ok_or(tr!("juego no encontrado en el catálogo", "game not found in the catalog"))?;
    let token = state.store.settings.lock().unwrap().github_token.clone();
    github::latest_build(game, token.as_deref()).await.map_err(err)
}

#[tauri::command]
fn open_url(url: String) -> R<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(tr!("URL no permitida", "URL not allowed").into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ---------- Fuentes ----------

/// Enlaces (de ports y de datos) que corresponden o no a algún juego del catálogo.
fn count_links(doc: &sources::SourceDoc) -> (usize, usize) {
    let ports = doc.downloads.iter().map(|l| l.game().is_some());
    let data = doc.data.iter().map(|d| d.game_id().is_some());
    ports.chain(data).fold((0, 0), |(m, u), ok| if ok { (m + 1, u) } else { (m, u + 1) })
}

#[tauri::command]
async fn add_source(state: S<'_>, url: String) -> R<store::SourceEntry> {
    let url = url.trim().to_string();
    if state.store.settings.lock().unwrap().sources.iter().any(|s| s.url == url) {
        return Err(tr!("esa fuente ya está añadida", "that source is already added").into());
    }
    let doc = sources::fetch(&url).await.map_err(err)?;
    let (matched, unmatched) = count_links(&doc);
    let entry = store::SourceEntry {
        url: url.clone(),
        name: doc.name.clone(),
        enabled: true,
        last_fetch: Some(store::now()),
        matched,
        unmatched,
        error: None,
    };
    state.store.sources.lock().unwrap().insert(url, doc);
    state.store.settings.lock().unwrap().sources.push(entry.clone());
    state.store.save_sources().map_err(err)?;
    state.store.save_settings().map_err(err)?;
    Ok(entry)
}

#[tauri::command]
fn remove_source(state: S, url: String) -> R<()> {
    state.store.settings.lock().unwrap().sources.retain(|s| s.url != url);
    state.store.sources.lock().unwrap().remove(&url);
    state.store.save_sources().map_err(err)?;
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn toggle_source(state: S, url: String, enabled: bool) -> R<()> {
    if let Some(s) = state.store.settings.lock().unwrap().sources.iter_mut().find(|s| s.url == url) {
        s.enabled = enabled;
    }
    state.store.save_settings().map_err(err)
}

/// Vuelve a descargar todas las fuentes en paralelo. Si una falla se conserva la caché anterior.
#[tauri::command]
async fn refresh_sources(state: S<'_>) -> R<Vec<store::SourceEntry>> {
    let urls: Vec<String> = state.store.settings.lock().unwrap().sources.iter().map(|s| s.url.clone()).collect();
    let results = futures_util::future::join_all(urls.iter().map(|u| sources::fetch(u))).await;
    {
        let mut settings = state.store.settings.lock().unwrap();
        let mut docs = state.store.sources.lock().unwrap();
        for (url, res) in urls.iter().zip(results) {
            let Some(entry) = settings.sources.iter_mut().find(|s| &s.url == url) else { continue };
            match res {
                Ok(doc) => {
                    (entry.matched, entry.unmatched) = count_links(&doc);
                    entry.name = doc.name.clone();
                    entry.last_fetch = Some(store::now());
                    entry.error = None;
                    docs.insert(url.clone(), doc);
                }
                Err(e) => entry.error = Some(format!("{e:#}")),
            }
        }
    }
    state.store.save_sources().map_err(err)?;
    state.store.save_settings().map_err(err)?;
    Ok(state.store.settings.lock().unwrap().sources.clone())
}

// ---------- Biblioteca ----------

#[tauri::command]
fn get_library(state: S) -> Vec<store::InstalledGame> {
    let mut games: Vec<_> = state.store.library.lock().unwrap().values().cloned().collect();
    games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    games
}

/// Juegos añadidos a la biblioteca: id del juego -> fecha en que se añadió.
#[tauri::command]
fn get_owned(state: S) -> HashMap<String, u64> {
    state.store.owned.lock().unwrap().clone()
}

#[tauri::command]
fn add_to_library(app: AppHandle, state: S, game: String) -> R<()> {
    if !catalog::all().iter().any(|p| p.game == game) {
        return Err(tr!("juego no encontrado en el catálogo", "game not found in the catalog").into());
    }
    state.store.owned.lock().unwrap().entry(game).or_insert_with(store::now);
    state.store.save_owned().map_err(err)?;
    profile_changed(&app, state.inner());
    Ok(())
}

/// Quita un juego de la biblioteca. Solo si no tiene ningún port instalado.
#[tauri::command]
fn remove_from_library(state: S, game: String) -> R<()> {
    let installed = state
        .store
        .library
        .lock()
        .unwrap()
        .keys()
        .any(|id| catalog::get(id).is_some_and(|p| p.game == game));
    if installed {
        return Err(tr!("desinstala primero el juego para quitarlo de la biblioteca", "uninstall the game first to remove it from the library").into());
    }
    state.store.owned.lock().unwrap().remove(&game);
    state.store.save_owned().map_err(err)
}

#[tauri::command]
fn get_running(state: S) -> Vec<String> {
    state.running.lock().unwrap().iter().cloned().collect()
}

// ---------- Envío de ports por sus autores ----------

#[tauri::command]
async fn my_repos() -> R<Vec<submit::RepoItem>> {
    submit::my_repos().await.map_err(err)
}

#[tauri::command]
async fn check_repo(repo: String) -> R<submit::RepoCheck> {
    submit::check_repo(&repo).await.map_err(err)
}

#[tauri::command]
async fn submit_port(submission: submit::Submission) -> R<String> {
    submit::submit(submission).await.map_err(err)
}

#[tauri::command]
async fn my_submissions() -> R<Vec<submit::MySubmission>> {
    submit::mine().await.map_err(err)
}

/// Lo que el autor de un port cuenta de él en el darsena.json de su repositorio.
#[tauri::command]
async fn get_author_info(id: String) -> Option<serde_json::Value> {
    submit::author_info(&id).await
}

/// Crea en el escritorio un acceso directo al juego. Devuelve dónde quedó.
#[tauri::command]
fn create_shortcut(app: AppHandle, state: S, id: String) -> R<String> {
    tray::create_shortcut(&app, state.inner(), &id).map(|p| p.to_string_lossy().into_owned()).map_err(err)
}

/// Hardware del equipo (procesador, memoria, gráficas, Vulkan) para «¿Funciona en tu PC?».
#[tauri::command]
async fn get_hardware() -> hardware::Hardware {
    tokio::task::spawn_blocking(hardware::get).await.unwrap_or_default()
}

/// «¿Qué tal te funcionó?» de un juego, con el hardware de este PC.
#[tauri::command]
async fn report_compat(game: String, result: String, port: String) -> R<()> {
    let token = need_token()?;
    let hw = tokio::task::spawn_blocking(hardware::get).await.unwrap_or_default();
    reviews::report_compat(&token, &game, &catalog::game_title(&game), &result, &port, &hw).await.map_err(err)
}

/// Informes de compatibilidad de la comunidad de un juego (del resumen público).
#[tauri::command]
async fn get_compat(game: String) -> Option<serde_json::Value> {
    reviews::public().await.get(&game).and_then(|r| r.compat.clone())
}

// ---------- Guardado en la nube ----------

fn installed(state: &S, id: &str) -> R<store::InstalledGame> {
    state.store.library.lock().unwrap().get(id).cloned().ok_or(tr!("el juego no está instalado", "the game isn't installed"))
}

fn set_synced(state: &S, id: &str, at: u64) -> R<()> {
    if let Some(g) = state.store.library.lock().unwrap().get_mut(id) {
        g.cloud_synced = Some(at);
    }
    state.store.save_library().map_err(err)
}

#[tauri::command]
async fn cloud_status(state: S<'_>, id: String) -> R<cloud::CloudStatus> {
    let token = need_token()?;
    cloud::status(&token, &installed(&state, &id)?).await.map_err(err)
}

#[tauri::command]
async fn cloud_upload(state: S<'_>, id: String) -> R<()> {
    let token = need_token()?;
    let at = cloud::upload(&token, &installed(&state, &id)?).await.map_err(err)?;
    set_synced(&state, &id, at)
}

#[tauri::command]
async fn cloud_restore(state: S<'_>, id: String) -> R<()> {
    let token = need_token()?;
    let inst = installed(&state, &id)?;
    let at = cloud::restore(&token, &inst).await.map_err(err)?;
    // Lo recién escrito cuenta como sincronizado (si no, parecería más nuevo que la nube).
    let local = tokio::task::spawn_blocking(move || cloud::local_modified_of(&inst)).await.unwrap_or(0);
    set_synced(&state, &id, at.max(local))
}

#[tauri::command]
fn set_cloud_saves(state: S, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().cloud_saves = value;
    state.store.save_settings().map_err(err)
}

/// Pantalla grande: la ventana a pantalla completa (y de vuelta).
#[tauri::command]
fn set_fullscreen(app: AppHandle, value: bool) -> R<()> {
    let w = app.get_webview_window("main").ok_or("ventana")?;
    w.set_fullscreen(value).map_err(|e| e.to_string())
}

// ---------- Taller ----------

#[tauri::command]
async fn list_mods(force: Option<bool>) -> R<Vec<workshop::PublicMod>> {
    Ok(workshop::public(force.unwrap_or(false)).await)
}

#[tauri::command]
async fn submit_mod(meta: workshop::ModMeta) -> R<String> {
    workshop::submit(meta).await.map_err(err)
}

#[tauri::command]
async fn like_mod(node: String) -> R<()> {
    workshop::like(&node).await.map_err(err)
}

#[tauri::command]
async fn report_mod(node: String, reason: String, text: String) -> R<()> {
    workshop::report(&node, &reason, &text).await.map_err(err)
}

/// Instala un mod del Taller en un port instalado.
#[tauri::command]
async fn install_mod(app: AppHandle, state: S<'_>, port_id: String, mod_id: String) -> R<()> {
    let m = workshop::public(false)
        .await
        .into_iter()
        .find(|m| m.id == mod_id)
        .ok_or(tr!("ese mod ya no está en el Taller", "that mod is no longer in the Workshop"))?;
    let dir = {
        let lib = state.store.library.lock().unwrap();
        let g = lib.get(&port_id).ok_or(tr!("el port no está instalado", "the port isn't installed"))?;
        if let Some(old) = g.mods.iter().find(|x| x.id == mod_id) {
            workshop::uninstall(old, std::path::Path::new(&g.path));
        }
        std::path::PathBuf::from(&g.path)
    };
    let installed = workshop::install(&m, &dir).await.map_err(err)?;
    if let Some(g) = state.store.library.lock().unwrap().get_mut(&port_id) {
        g.mods.retain(|x| x.id != mod_id);
        g.mods.push(installed);
    }
    state.store.save_library().map_err(err)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
fn uninstall_mod(app: AppHandle, state: S, port_id: String, mod_id: String) -> R<()> {
    if let Some(g) = state.store.library.lock().unwrap().get_mut(&port_id) {
        if let Some(i) = g.mods.iter().position(|x| x.id == mod_id) {
            let m = g.mods.remove(i);
            workshop::uninstall(&m, std::path::Path::new(&g.path));
        }
    }
    state.store.save_library().map_err(err)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
fn set_show_nsfw(state: S, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().show_nsfw = value;
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn set_discord_presence(state: S, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().discord_presence = value;
    if !value {
        discord::set(false, None);
    }
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn set_start_with_windows(state: S, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().start_with_windows = value;
    inicio::apply(value);
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn set_close_to_tray(state: S, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().close_to_tray = value;
    state.store.save_settings().map_err(err)
}

/// Actualiza un port instalado a su última versión oficial (conserva partidas y configuración).
#[tauri::command]
fn update_game(app: AppHandle, state: S, id: String) -> R<()> {
    let game = catalog::get(&id).ok_or(tr!("juego no encontrado en el catálogo", "game not found in the catalog"))?;
    downloads::start_update(app, state.inner().clone(), game).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortUpdate {
    id: String,
    installed: Option<String>,
    latest: String,
}

/// Ports instalados desde GitHub con una versión más nueva publicada.
#[tauri::command]
async fn port_updates(state: S<'_>) -> R<Vec<PortUpdate>> {
    let installed: Vec<store::InstalledGame> = state
        .store
        .library
        .lock()
        .unwrap()
        .values()
        .filter(|g| !g.linked && g.origin == "GitHub oficial")
        .cloned()
        .collect();
    let token = state.store.settings.lock().unwrap().github_token.clone();
    let mut out = Vec::new();
    for g in installed {
        let Some(port) = catalog::get(&g.id) else { continue };
        if let Ok(b) = github::latest_build(port, token.as_deref()).await {
            if g.version.as_deref() != Some(b.version.as_str()) {
                out.push(PortUpdate { id: g.id.clone(), installed: g.version.clone(), latest: b.version });
            }
        }
    }
    Ok(out)
}

#[tauri::command]
fn set_auto_update_ports(state: S, value: bool) -> R<()> {
    state.store.settings.lock().unwrap().auto_update_ports = value;
    state.store.save_settings().map_err(err)
}

#[tauri::command]
fn launch_game(app: AppHandle, state: S, id: String) -> R<()> {
    library::launch(app, state.inner().clone(), &id).map_err(err)
}

#[tauri::command]
async fn uninstall_game(state: S<'_>, id: String) -> R<()> {
    // Primero se deja de compartir: la sesión de torrents tiene abiertos esos archivos.
    state.dl.stop_seeding(&id).await;
    let st = state.inner().clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || library::uninstall(&st, &id2))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)?;
    // Un juego añadido a mano solo existe en tu biblioteca: al quitarlo, desaparece del todo.
    if id.starts_with(CUSTOM_PREFIX) && state.store.owned.lock().unwrap().remove(&id).is_some() {
        state.store.save_owned().map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
fn open_game_folder(state: S, id: String) -> R<()> {
    let path = state
        .store
        .library
        .lock()
        .unwrap()
        .get(&id)
        .map(|g| g.path.clone())
        .ok_or(tr!("el juego no está instalado", "the game isn't installed"))?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

/// Vincula un port que ya tienes instalado fuera de Dársena: basta con elegir su ejecutable.
/// Queda como instalado (se puede jugar y cuenta el tiempo), y al quitarlo no se borra nada.
#[tauri::command]
fn locate_game(app: AppHandle, state: S, id: String, exe: String) -> R<()> {
    let game = catalog::get(&id).ok_or(tr!("juego no encontrado en el catálogo", "game not found in the catalog"))?;
    if state.store.library.lock().unwrap().contains_key(&id) {
        return Err(tr!("el juego ya está instalado", "the game is already installed"));
    }
    let exe = PathBuf::from(&exe);
    if !exe.is_file() {
        return Err(tr!("no existe {}", "{} doesn't exist", exe.display()));
    }
    let dir = exe.parent().ok_or(tr!("ruta no válida", "invalid path"))?;
    let installed = store::InstalledGame {
        id: id.clone(),
        title: game.title.clone(),
        port: game.port.clone(),
        console: game.console.clone(),
        version: None,
        origin: "Vinculado a mano".into(),
        path: dir.to_string_lossy().into_owned(),
        exe: exe.file_name().map(|f| f.to_string_lossy().into_owned()),
        installed_at: store::now(),
        playtime_secs: 0,
        last_played: None,
        data: Vec::new(),
        seed: None,
        linked: true,
        mods: Vec::new(),
        cloud_synced: None,
    };
    state.store.library.lock().unwrap().insert(id, installed);
    state.store.save_library().map_err(err)?;
    state.store.owned.lock().unwrap().entry(game.game.clone()).or_insert_with(store::now);
    state.store.save_owned().map_err(err)?;
    let _ = app.emit("library-changed", ());
    profile_changed(&app, state.inner());
    Ok(())
}

/// Prefijo de los juegos añadidos a mano que no están en el catálogo.
pub const CUSTOM_PREFIX: &str = "custom-";

/// Añade a la biblioteca un juego que no está en el catálogo: cualquier ejecutable, con el nombre que elijas.
#[tauri::command]
fn add_custom_game(app: AppHandle, state: S, name: String, exe: String) -> R<String> {
    let name = name.trim().chars().take(80).collect::<String>();
    if name.is_empty() {
        return Err(tr!("escribe el nombre del juego", "write the game's name"));
    }
    let exe = PathBuf::from(&exe);
    if !exe.is_file() {
        return Err(tr!("no existe {}", "{} doesn't exist", exe.display()));
    }
    let dir = exe.parent().ok_or(tr!("ruta no válida", "invalid path"))?;
    let base = format!("{CUSTOM_PREFIX}{}", catalog::slug(&name));
    let id = {
        let lib = state.store.library.lock().unwrap();
        (1..).map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") }).find(|id| !lib.contains_key(id)).unwrap()
    };
    let installed = store::InstalledGame {
        id: id.clone(),
        title: name,
        port: tr!("Juego propio", "Own game"),
        console: "PC".into(),
        version: None,
        origin: "Añadido a mano".into(),
        path: dir.to_string_lossy().into_owned(),
        exe: exe.file_name().map(|f| f.to_string_lossy().into_owned()),
        installed_at: store::now(),
        playtime_secs: 0,
        last_played: None,
        data: Vec::new(),
        seed: None,
        linked: true,
        mods: Vec::new(),
        cloud_synced: None,
    };
    state.store.library.lock().unwrap().insert(id.clone(), installed);
    state.store.save_library().map_err(err)?;
    state.store.owned.lock().unwrap().entry(id.clone()).or_insert_with(store::now);
    state.store.save_owned().map_err(err)?;
    let _ = app.emit("library-changed", ());
    Ok(id)
}

/// Fija a mano el ejecutable cuando no se detectó solo. Debe estar dentro de la carpeta del juego.
#[tauri::command]
fn set_game_exe(state: S, id: String, exe: String) -> R<()> {
    let mut lib = state.store.library.lock().unwrap();
    let game = lib.get_mut(&id).ok_or(tr!("el juego no está instalado", "the game isn't installed"))?;
    let root = PathBuf::from(&game.path);
    let rel = PathBuf::from(&exe)
        .strip_prefix(&root)
        .map_err(|_| tr!("el ejecutable debe estar dentro de la carpeta del juego", "the executable must be inside the game folder").to_string())?
        .to_string_lossy()
        .into_owned();
    game.exe = Some(rel);
    drop(lib);
    state.store.save_library().map_err(err)
}

// ---------- Descargas ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourcePick {
    source_url: String,
    index: usize,
    /// Casilla «extraer el .zip» (solo para datos): None = lo que diga la fuente.
    #[serde(default)]
    extract: Option<bool>,
}

#[tauri::command]
fn install_game(
    app: AppHandle,
    state: S,
    id: String,
    source: Option<SourcePick>,
    install_dir: Option<String>,
    // Datos de tus fuentes a descargar justo después del port.
    data: Option<SourcePick>,
) -> R<()> {
    if state.store.library.lock().unwrap().contains_key(&id) {
        return Err(tr!("el juego ya está instalado", "the game is already installed").into());
    }
    let game = catalog::get(&id).ok_or(tr!("juego no encontrado en el catálogo", "game not found in the catalog"))?;
    // Instalar un port añade su juego a la biblioteca (y ahí sigue aunque se desinstale).
    state.store.owned.lock().unwrap().entry(game.game.clone()).or_insert_with(store::now);
    state.store.save_owned().map_err(err)?;
    let choice = match source {
        None => downloads::Choice::Official,
        Some(pick) => {
            let opt = state
                .store
                .source_options()
                .remove(id.as_str())
                .unwrap_or_default()
                .into_iter()
                .find(|o| o.source_url == pick.source_url && o.index == pick.index)
                .ok_or(tr!("esa opción de descarga ya no existe; actualiza las fuentes", "that download option no longer exists; refresh your sources"))?;
            downloads::Choice::Source(opt)
        }
    };
    let root = PathBuf::from(
        install_dir
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| state.store.settings.lock().unwrap().install_dir.clone()),
    );
    std::fs::create_dir_all(&root).map_err(|e| tr!("no se pudo crear {}: {e}", "couldn't create {}: {e}", root.display()))?;
    let then_data = data.map(|d| (d.source_url, d.index, d.extract));
    downloads::start(app, state.inner().clone(), game, choice, root, then_data).map_err(err)
}

/// Abre la carpeta Datos del juego de un port instalado (la crea si aún no existe).
#[tauri::command]
fn open_data_folder(state: S, id: String) -> R<()> {
    let installed = state.store.library.lock().unwrap().get(&id).cloned().ok_or(tr!("el juego no está instalado", "the game isn't installed"))?;
    let dir = downloads::data_dir_of(&installed);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(&dir, None::<&str>).map_err(|e| e.to_string())
}

/// Datos de juego disponibles en tus fuentes para un port.
#[tauri::command]
fn get_data_options(state: S, id: String) -> Vec<store::DataOption> {
    state.store.data_options(&id)
}

/// Descarga datos de juego de una fuente y los coloca en la carpeta del port instalado.
#[tauri::command]
fn install_data(app: AppHandle, state: S, id: String, source_url: String, index: usize, extract: Option<bool>) -> R<()> {
    let installed = state
        .store
        .library
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or(tr!("instala primero el juego", "install the game first"))?;
    let mut opt = state
        .store
        .data_options(&id)
        .into_iter()
        .find(|o| o.source_url == source_url && o.index == index)
        .ok_or(tr!("esos datos ya no están en tus fuentes; actualízalas", "that data is no longer in your sources; refresh them"))?;
    downloads::apply_extract(&mut opt, extract);
    downloads::start_data(app, state.inner().clone(), installed, opt).map_err(err)
}

#[tauri::command]
fn list_downloads(state: S) -> Vec<downloads::TaskView> {
    state.dl.views()
}

#[tauri::command]
async fn pause_download(state: S<'_>, id: String) -> R<()> {
    state.dl.pause(&id).await.map_err(err)
}

#[tauri::command]
async fn resume_download(state: S<'_>, id: String) -> R<()> {
    state.dl.resume(&id).await.map_err(err)
}

#[tauri::command]
fn cancel_download(state: S, id: String) {
    state.dl.cancel(&id);
}

// ---------- Sesión de GitHub ----------

#[tauri::command]
async fn auth_start() -> R<auth::DeviceCode> {
    auth::start().await.map_err(err)
}

#[tauri::command]
async fn auth_poll(device_code: String) -> R<Option<auth::GhUser>> {
    auth::poll(&device_code).await.map_err(err)
}

#[tauri::command]
async fn auth_user() -> R<Option<auth::GhUser>> {
    auth::user().await.map_err(err)
}

#[tauri::command]
fn auth_logout() {
    auth::logout();
}

// ---------- Reseñas ----------

fn need_token() -> R<String> {
    auth::token().ok_or_else(|| tr!("inicia sesión con GitHub para escribir reseñas", "sign in with GitHub to write reviews").to_string())
}

/// Reseñas de un juego: en vivo si hay sesión (para ver y editar la propia), si no del resumen público.
#[tauri::command]
async fn get_reviews(game: String) -> R<reviews::GameReviews> {
    if let Some(token) = auth::token() {
        if let Ok(r) = reviews::live(&token, &game).await {
            return Ok(r);
        }
    }
    Ok(reviews::public().await.remove(&game).unwrap_or_default())
}

#[derive(Serialize)]
struct ReviewSummary {
    average: f32,
    count: usize,
}

/// Nota media y número de reseñas de cada juego (para las tarjetas), del resumen público.
#[tauri::command]
async fn get_review_summaries() -> HashMap<String, ReviewSummary> {
    reviews::public()
        .await
        .into_iter()
        .map(|(id, r)| (id, ReviewSummary { average: r.average, count: r.count }))
        .collect()
}

#[derive(Serialize)]
struct RecentReview {
    game: String,
    user: String,
    avatar: String,
    rating: u8,
    text: String,
    date: String,
}

/// Las reseñas más recientes de todos los juegos (para la pestaña Comunidad).
#[tauri::command]
async fn recent_reviews() -> Vec<RecentReview> {
    let mut all: Vec<RecentReview> = reviews::public()
        .await
        .into_iter()
        .flat_map(|(game, r)| {
            r.reviews.into_iter().map(move |x| RecentReview { game: game.clone(), user: x.user, avatar: x.avatar, rating: x.rating, text: x.text, date: x.date })
        })
        .collect();
    all.sort_by(|a, b| b.date.cmp(&a.date));
    all.truncate(20);
    all
}

#[tauri::command]
async fn submit_review(app: AppHandle, state: S<'_>, game: String, rating: u8, text: String) -> R<reviews::GameReviews> {
    let token = need_token()?;
    let res = reviews::submit(&token, &game, &catalog::game_title(&game), rating, &text).await.map_err(err)?;
    state.store.profile.lock().unwrap().reviewed.insert(game);
    profile_changed(&app, state.inner());
    Ok(res)
}

#[tauri::command]
async fn delete_review(game: String) -> R<reviews::GameReviews> {
    let token = need_token()?;
    reviews::delete_mine(&token, &game).await.map_err(err)
}

async fn my_login() -> R<String> {
    Ok(auth::user().await.map_err(err)?.ok_or_else(|| tr!("inicia sesión con GitHub", "sign in with GitHub"))?.login)
}

/// Responde a una reseña como desarrollador del port.
#[tauri::command]
async fn reply_review(game: String, review: String, text: String) -> R<reviews::GameReviews> {
    let token = need_token()?;
    let login = my_login().await?;
    reviews::reply(&token, &login, &game, &review, &text).await.map_err(err)
}

/// Ports de un juego de los que eres desarrollador (vacío si no eres de ninguno o no has iniciado sesión).
#[tauri::command]
async fn my_ports_of_game(game: String) -> Vec<String> {
    match my_login().await {
        Ok(login) => devs::my_ports_of_game(&login, &game).await,
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
async fn repo_default_branch(repo: String) -> R<String> {
    submit::default_branch(&repo).await.map_err(err)
}

/// Ports del catálogo que son tuyos (para el panel del desarrollador).
#[tauri::command]
async fn my_dev_ports() -> R<Vec<String>> {
    Ok(devs::my_ports(&my_login().await?).await)
}

// ---------- Perfil y logros ----------

/// Tras cambiar el perfil: desbloquea logros, guarda, avisa a la interfaz y sincroniza con el Gist.
pub fn profile_changed(app: &AppHandle, state: &Arc<AppState>) {
    let owned = state.store.owned.lock().unwrap().len();
    let new = profile::evaluate(&mut state.store.profile.lock().unwrap(), owned);
    let _ = state.store.save_profile();
    if !new.is_empty() {
        let list: Vec<_> = new
            .iter()
            .map(|a| serde_json::json!({ "id": a.id, "name": a.title(), "icon": a.icon, "description": a.text() }))
            .collect();
        let _ = app.emit("achievements", list);
    }
    let _ = app.emit("profile-changed", ());
    if let Some(token) = auth::token() {
        let (app, state) = (app.clone(), state.clone());
        tauri::async_runtime::spawn(async move {
            let _ = sync_profile_with(&app, &state, &token).await;
        });
    }
}

/// Junta el perfil local con el del Gist del usuario y sube el resultado (de uno en uno: evita Gists duplicados).
async fn sync_profile_with(app: &AppHandle, state: &Arc<AppState>, token: &str) -> anyhow::Result<()> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = LOCK.lock().await;
    let local = state.store.profile.lock().unwrap().clone();
    let (synced, _) = profile::sync(token, local).await?;
    let owned = state.store.owned.lock().unwrap().len();
    let new = {
        let mut p = state.store.profile.lock().unwrap();
        p.gist_id = synced.gist_id.clone();
        // Lo jugado mientras se sincronizaba se conserva: la mezcla se queda con lo mayor.
        profile::merge(&mut p, synced);
        profile::evaluate(&mut p, owned)
    };
    state.store.save_profile()?;
    if !new.is_empty() {
        let list: Vec<_> = new
            .iter()
            .map(|a| serde_json::json!({ "id": a.id, "name": a.title(), "icon": a.icon, "description": a.text() }))
            .collect();
        let _ = app.emit("achievements", list);
    }
    let _ = app.emit("profile-changed", ());
    Ok(())
}

fn gist_url(p: &profile::Profile) -> Option<String> {
    p.gist_id.as_ref().map(|id| format!("https://gist.github.com/{id}"))
}

/// Tu perfil (local, con tu usuario de GitHub si hay sesión).
#[tauri::command]
async fn get_profile(state: S<'_>) -> R<profile::ProfileView> {
    let user = auth::user().await.ok().flatten();
    let p = state.store.profile.lock().unwrap().clone();
    Ok(profile::view(&p, user.as_ref().map(|u| u.login.clone()), user.map(|u| u.avatar_url), gist_url(&p)))
}

/// Sincroniza ya tu perfil con tu Gist (al iniciar sesión o al abrir el perfil).
#[tauri::command]
async fn sync_profile(app: AppHandle, state: S<'_>) -> R<profile::ProfileView> {
    let token = auth::token().ok_or_else(|| tr!("inicia sesión con GitHub para sincronizar tu perfil", "sign in with GitHub to sync your profile").to_string())?;
    sync_profile_with(&app, state.inner(), &token).await.map_err(err)?;
    get_profile(state).await
}

/// Perfil público de otro usuario de Dársena (desde su Gist). None si no tiene.
#[tauri::command]
async fn get_user_profile(login: String) -> R<Option<profile::ProfileView>> {
    let found = profile::of_user(&login, auth::token().as_deref()).await.map_err(err)?;
    Ok(found.map(|(p, url)| profile::view(&p, Some(login.clone()), Some(format!("https://github.com/{login}.png")), Some(url))))
}

/// El launcher se llamaba PortLauncher: si existe su carpeta de datos y aún no la de Dársena,
/// se mueve tal cual (ajustes, fuentes, biblioteca y caché de portadas).
fn migrate_old_data(data_dir: &std::path::Path) {
    if data_dir.exists() {
        return;
    }
    let Some(old) = data_dir.parent().map(|p| p.join("com.portlauncher.app")) else { return };
    if old.is_dir() {
        let _ = std::fs::rename(&old, data_dir);
    }
}

/// Lista de comandos de la app. `$extra` son los comandos opcionales (juego remoto), para no duplicar la lista.
macro_rules! app_handler {
    ($($extra:ident),* $(,)?) => {
        tauri::generate_handler![
            get_settings,
            set_language,
            set_favorite,
            create_shortcut,
            update_game,
            port_updates,
            set_auto_update_ports,
            my_repos,
            check_repo,
            submit_port,
            my_submissions,
            get_author_info,
            reply_review,
            my_ports_of_game,
            my_dev_ports,
            repo_default_branch,
            set_close_to_tray,
            set_start_with_windows,
            set_discord_presence,
            list_mods,
            get_hardware,
            get_discord_link,
            set_fullscreen,
            cloud_status,
            cloud_upload,
            cloud_restore,
            set_cloud_saves,
            add_custom_game,
            report_compat,
            get_compat,
            recent_reviews,
            submit_mod,
            like_mod,
            report_mod,
            install_mod,
            uninstall_mod,
            set_show_nsfw,
            locate_game,
            set_shelves,
            set_install_dir,
            list_drives,
            use_library_drive,
            set_share_downloads,
            set_upload_limit,
            get_sharing_stats,
            set_github_token,
            get_catalog,
            get_game_info,
            check_catalog_update,
            get_app_version,
            check_app_update,
            install_app_update,
            get_cover_index,
            set_covers_origin,
            get_source_counts,
            get_source_options,
            get_official_build,
            open_url,
            add_source,
            remove_source,
            toggle_source,
            refresh_sources,
            get_library,
            get_running,
            get_owned,
            add_to_library,
            remove_from_library,
            launch_game,
            uninstall_game,
            open_game_folder,
            set_game_exe,
            install_game,
            get_data_options,
            open_data_folder,
            install_data,
            list_downloads,
            pause_download,
            resume_download,
            cancel_download,
            auth_start,
            auth_poll,
            auth_user,
            auth_logout,
            get_reviews,
            get_review_summaries,
            submit_review,
            delete_review,
            get_profile,
            sync_profile,
            get_user_profile,
            $($extra),*
        ]
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Idioma guardado en Ajustes (si se eligió uno), para los textos que Rust muestra antes que la interfaz.
fn state_lang(app: &AppHandle) -> Option<String> {
    app.try_state::<Arc<AppState>>()?.store.settings.lock().unwrap().language.clone()
}

pub fn run() {
    let builder = tauri::Builder::default()
        // Una sola Dársena: si ya está abierta, un acceso directo le pasa el juego a abrir (o la trae al frente).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| match tray::launch_arg(&args) {
            Some(id) => tray::launch_from_shortcut(app, &id),
            None => tray::show_main(app),
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("cover", |ctx, req, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                let Some(state) = app.try_state::<Arc<AppState>>() else {
                    responder.respond(tauri::http::Response::builder().status(503).body(Vec::new()).unwrap());
                    return;
                };
                let cache = state.store.data_dir().join("covers-cache");
                let origin = state
                    .store
                    .settings
                    .lock()
                    .unwrap()
                    .covers_origin
                    .clone()
                    .unwrap_or_else(covers::default_origin);
                responder.respond(covers::serve(cache, origin, req).await);
            });
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            migrate_old_data(&data_dir);
            // Último catálogo e índice de portadas descargados (si los hay) en vez de los embebidos.
            catalog::init(&data_dir);
            info::init(&data_dir);
            covers::init(&data_dir);
            // Biblioteca por defecto: <unidad del usuario>:\DarsenaLibrary (se cambia en Ajustes).
            let default_games = app
                .path()
                .home_dir()
                .ok()
                .and_then(|h| h.ancestors().last().map(|root| root.join("DarsenaLibrary")))
                .unwrap_or_else(|| data_dir.join("DarsenaLibrary"));
            let state = Arc::new(AppState {
                store: store::Store::load(data_dir, default_games)?,
                dl: downloads::DownloadManager::default(),
                running: Mutex::new(HashSet::new()),
            });
            app.manage(state.clone());
            // Gancho de pruebas del juego remoto (F0): DARSENA_REMOTE_TEST="parte del título".
            #[cfg(feature = "remote")]
            if let Ok(needle) = std::env::var("DARSENA_REMOTE_TEST") {
                tauri::async_runtime::spawn(async move {
                    let needle = needle.to_lowercase();
                    let msg = match remote::list_windows().iter().find(|w| w.title.to_lowercase().contains(&needle)) {
                        None => "no hay ventana con ese título".to_string(),
                        Some(w) => match remote::start(w.hwnd, 30, 6000).await {
                            Ok(i) => format!("OK {} {}x{}", i.urls.join(" "), i.width, i.height),
                            Err(e) => format!("error: {e:#}"),
                        },
                    };
                    let _ = std::fs::write(std::env::temp_dir().join("darsena_remote_test.txt"), msg);
                });
            }
            // Vuelve a compartir lo que se bajó por torrent (en segundo plano: no retrasa el arranque).
            tauri::async_runtime::spawn(downloads::resume_seeding(state.clone()));
            downloads::spawn_ticker(app.handle().clone(), state);
            if let Some(lang) = state_lang(app.handle()) {
                i18n::set(&lang);
            }
            tray::create(app.handle())?;
            // Abierta desde un acceso directo: se lanza el juego y Dársena se queda en la bandeja.
            // Si no, se enseña la ventana (empieza oculta para no parpadear en ese caso).
            let args: Vec<String> = std::env::args().collect();
            inicio::apply(app.state::<Arc<AppState>>().store.settings.lock().unwrap().start_with_windows);
            match tray::launch_arg(&args) {
                Some(id) => tray::launch_from_shortcut(app.handle(), &id),
                // Al iniciar Windows, Dársena se queda en la bandeja sin abrir la ventana.
                None if inicio::launched_by_windows(&args) => {}
                None => tray::show_main(app.handle()),
            }
            Ok(())
        })
        // Cerrar la ventana la esconde en la bandeja (si así está en Ajustes, o si hay un juego abierto,
        // para no perder su tiempo de juego). Para salir del todo: «Salir» en el menú de la bandeja.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<Arc<AppState>>() {
                    let keep = state.store.settings.lock().unwrap().close_to_tray || !state.running.lock().unwrap().is_empty();
                    if keep {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        });
    #[cfg(feature = "remote")]
    let builder = builder.invoke_handler(app_handler!(
        remote_list_windows,
        remote_start,
        remote_stop,
        remote_status,
        remote_guests,
        remote_set_permissions,
        remote_kick,
        remote_pause_input,
    ));
    #[cfg(not(feature = "remote"))]
    let builder = builder.invoke_handler(app_handler!());
    builder
        .run(tauri::generate_context!())
        .expect("error al iniciar Dársena");
}
