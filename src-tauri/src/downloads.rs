//! Gestor de descargas: torrents (librqbit) y descargas directas multiconexión (http_multi),
//! a la vez cuando una fuente da ambos; pausa/reanudar/cancelar, extracción de .zip y alta en la biblioteca.

use crate::catalog::CatalogGame;
use crate::github;
use crate::http_multi::{self, HttpCtl};
use crate::webseed;
use crate::sources::{DownloadLink, LinkKind, PLATFORM};
use crate::sources::DataFormat;
use crate::store::{now, DataOption, InstalledData, InstalledGame, SeedInfo, SourceOption, Store};
use crate::AppState;
use anyhow::Context;
use librqbit::{
    AddTorrent, AddTorrentOptions, ListenerMode, ListenerOptions, ManagedTorrent, Session,
    SessionOptions, TorrentStatsState,
};
use serde::Serialize;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Starting,
    Downloading,
    Paused,
    Extracting,
    Completed,
    Error,
}

/// Hueco para el torrent de una carrera: se rellena cuando el magnet consigue su información.
type TorrentSlot = Arc<Mutex<Option<Arc<ManagedTorrent>>>>;

#[derive(Clone)]
enum Engine {
    Pending,
    Torrent(Arc<ManagedTorrent>),
    Http(Arc<HttpCtl>),
}

struct Task {
    title: String,
    kind: LinkKind,
    status: Status,
    error: Option<String>,
    engine: Engine,
    cancelled: Arc<AtomicBool>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub key: String,
    pub title: String,
    pub kind: LinkKind,
    pub status: Status,
    pub downloaded: u64,
    pub total: u64,
    pub speed_bps: f64,
    pub eta_secs: Option<u64>,
    pub error: Option<String>,
    /// Cómo se está bajando: "MAGNET", "HTTP · 8 conexiones · 2 mirrors", "TORRENT + HTTP"…
    pub via: String,
}

fn http_via(c: &HttpCtl) -> String {
    let (n, m) = (c.connections.load(Ordering::Relaxed), c.mirrors.load(Ordering::Relaxed));
    let mut v = "HTTP".to_string();
    if n > 1 {
        v += &format!(" · {n} conexiones");
    }
    if m > 1 {
        v += &format!(" · {m} mirrors");
    }
    v
}

#[derive(Default)]
pub struct DownloadManager {
    session: tokio::sync::OnceCell<Arc<Session>>,
    tasks: Mutex<HashMap<String, Task>>,
    /// Último (bytes, instante, velocidad suavizada) por tarea, para calcular velocidad.
    speed: Mutex<HashMap<String, (u64, Instant, f64)>>,
    /// Ports que se están compartiendo en segundo plano: id del port -> torrent.
    seeding: Mutex<HashMap<String, Arc<ManagedTorrent>>>,
}

/// Lo que se está compartiendo, para Ajustes.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharingStats {
    pub ports: usize,
    pub uploaded_bytes: u64,
    pub peers: usize,
}

fn upload_bps(kbps: u32) -> Option<NonZeroU32> {
    NonZeroU32::new(kbps.saturating_mul(1024))
}

/// Trackers públicos que se añaden a todos los torrents (también a los magnets sin `&tr=`):
/// encuentran pares en segundos, mientras que solo con DHT puede tardar minutos.
const PUBLIC_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "https://tracker.tamersunion.org:443/announce",
];

/// Cuando una fuente da torrent y enlace directo, primero se prueba el torrent solo durante este plazo. Si no ha bajado
/// `RACE_MIN_BYTES` (≈1 MB/s) se corta, se borra lo que llevaba y se sigue solo con la descarga directa. Nunca se bajan
/// las dos a la vez: cada una reserva el archivo entero en disco y se duplicaría el espacio.
const RACE_PROBE: Duration = Duration::from_secs(8);
const RACE_MIN_BYTES: u64 = 8 << 20;

/// Si un torrent no consigue su información o no empieza a bajar en este tiempo, se pasa al siguiente mirror.
const TORRENT_STALL: Duration = Duration::from_secs(60);

impl DownloadManager {
    async fn session(&self, store: &Store) -> anyhow::Result<Arc<Session>> {
        let data_dir = store.data_dir();
        let upload_kbps = store.settings.lock().unwrap().upload_limit_kbps;
        self.session
            .get_or_try_init(|| async {
                let opts = SessionOptions {
                    // Escuchar conexiones entrantes y abrir puerto por UPnP: más pares = más velocidad.
                    // uTP aún está marcado como inestable en librqbit, así que solo TCP.
                    listen: Some(ListenerOptions {
                        mode: ListenerMode::TcpOnly,
                        enable_upnp_port_forwarding: true,
                        ..Default::default()
                    }),
                    trackers: PUBLIC_TRACKERS.iter().filter_map(|t| t.parse().ok()).collect(),
                    // Límite de subida de Ajustes: compartir no debe saturar la conexión de nadie.
                    ratelimits: librqbit::limits::LimitsConfig { upload_bps: upload_bps(upload_kbps), download_bps: None },
                    ..Default::default()
                };
                Session::new_with_opts(data_dir.join("torrent-tmp"), opts).await
            })
            .await
            .cloned()
    }

    /// Cambia el límite de subida al momento (KB/s, 0 = sin límite).
    pub fn set_upload_limit(&self, kbps: u32) {
        if let Some(s) = self.session.get() {
            s.ratelimits.set_upload_bps(upload_bps(kbps));
        }
    }

    /// Deja de compartir un port (sin borrar sus archivos: eso lo hace quien llama).
    pub async fn stop_seeding(&self, key: &str) {
        let handle = self.seeding.lock().unwrap().remove(key);
        if let (Some(h), Some(s)) = (handle, self.session.get()) {
            let _ = s.delete(h.id().into(), false).await;
        }
    }

    pub async fn stop_all_seeding(&self) {
        let keys: Vec<String> = self.seeding.lock().unwrap().keys().cloned().collect();
        for k in keys {
            self.stop_seeding(&k).await;
        }
    }

    pub fn sharing_stats(&self) -> SharingStats {
        let seeding = self.seeding.lock().unwrap();
        let mut out = SharingStats { ports: seeding.len(), uploaded_bytes: 0, peers: 0 };
        for h in seeding.values() {
            let st = h.stats();
            out.uploaded_bytes += st.uploaded_bytes;
            out.peers += st.live.as_ref().map(|l| l.snapshot.peer_stats.live as usize).unwrap_or(0);
        }
        out
    }

    fn update<F: FnOnce(&mut Task)>(&self, key: &str, f: F) {
        if let Some(t) = self.tasks.lock().unwrap().get_mut(key) {
            f(t);
        }
    }

    fn engine(&self, key: &str) -> Option<(Engine, Status)> {
        self.tasks.lock().unwrap().get(key).map(|t| (t.engine.clone(), t.status))
    }

    pub fn views(&self) -> Vec<TaskView> {
        let tasks = self.tasks.lock().unwrap();
        let mut speeds = self.speed.lock().unwrap();
        let mut out: Vec<TaskView> = tasks
            .iter()
            .map(|(key, t)| {
                let kind = format!("{:?}", t.kind).to_uppercase();
                let (downloaded, total, via) = match &t.engine {
                    Engine::Torrent(h) => {
                        let s = h.stats();
                        (s.progress_bytes, s.total_bytes, kind)
                    }
                    Engine::Http(c) => (c.downloaded.load(Ordering::Relaxed), c.total.load(Ordering::Relaxed), http_via(c)),
                    Engine::Pending => (0, 0, kind),
                };
                let entry = speeds.entry(key.clone()).or_insert((downloaded, Instant::now(), 0.0));
                let dt = entry.1.elapsed().as_secs_f64();
                if dt >= 0.5 {
                    let inst = downloaded.saturating_sub(entry.0) as f64 / dt;
                    // Media móvil desde 0: evita picos falsos en la primera muestra.
                    entry.2 = entry.2 * 0.6 + inst * 0.4;
                    *entry = (downloaded, Instant::now(), entry.2);
                }
                let speed = if t.status == Status::Downloading { entry.2 } else { 0.0 };
                let eta = (speed > 1.0 && total > downloaded)
                    .then(|| ((total - downloaded) as f64 / speed) as u64);
                TaskView {
                    key: key.clone(),
                    title: t.title.clone(),
                    kind: t.kind,
                    status: t.status,
                    downloaded,
                    total,
                    speed_bps: speed,
                    eta_secs: eta,
                    error: t.error.clone(),
                    via,
                }
            })
            .collect();
        speeds.retain(|k, _| tasks.contains_key(k));
        out.sort_by(|a, b| a.title.cmp(&b.title));
        out
    }

    pub fn is_active(&self, key: &str) -> bool {
        self.tasks
            .lock()
            .unwrap()
            .get(key)
            .is_some_and(|t| !matches!(t.status, Status::Completed | Status::Error))
    }

    pub async fn pause(&self, key: &str) -> anyhow::Result<()> {
        let (engine, status) = self.engine(key).context(tr!("descarga no encontrada", "download not found"))?;
        if status != Status::Downloading {
            return Ok(());
        }
        match engine {
            Engine::Torrent(h) => {
                if let Some(s) = self.session.get() {
                    s.pause(&h).await?;
                }
            }
            Engine::Http(c) => c.paused.store(true, Ordering::SeqCst),
            Engine::Pending => anyhow::bail!(tr!("todavía conectando, espera un momento", "still connecting, wait a moment")),
        }
        self.update(key, |t| t.status = Status::Paused);
        Ok(())
    }

    pub async fn resume(&self, key: &str) -> anyhow::Result<()> {
        let (engine, status) = self.engine(key).context(tr!("descarga no encontrada", "download not found"))?;
        if status != Status::Paused {
            return Ok(());
        }
        match engine {
            Engine::Torrent(h) => {
                if let Some(s) = self.session.get() {
                    s.unpause(&h).await?;
                }
            }
            Engine::Http(c) => c.paused.store(false, Ordering::SeqCst),
            Engine::Pending => {}
        }
        self.update(key, |t| t.status = Status::Downloading);
        Ok(())
    }

    /// Cancela una descarga en curso o quita de la lista una terminada/fallida.
    pub fn cancel(&self, key: &str) {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(t) = tasks.get(key) {
            if matches!(t.status, Status::Completed | Status::Error) {
                tasks.remove(key);
            } else {
                // El bucle de descarga ve la bandera, limpia los archivos y quita la tarea.
                t.cancelled.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// Emite el estado de las descargas a la UI dos veces por segundo.
pub fn spawn_ticker(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut was_empty = true;
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let views = state.dl.views();
            if views.is_empty() && was_empty {
                continue;
            }
            was_empty = views.is_empty();
            let _ = app.emit("downloads", &views);
        }
    });
}

/// Qué descargar: la build oficial de GitHub o una opción de una fuente.
pub enum Choice {
    Official,
    Source(SourceOption),
}

pub fn start(
    app: AppHandle,
    state: Arc<AppState>,
    game: &'static CatalogGame,
    choice: Choice,
    install_root: PathBuf,
    // Datos de tus fuentes (url de la fuente, índice) a descargar en cuanto termine el port.
    then_data: Option<(String, usize, Option<bool>)>,
) -> anyhow::Result<()> {
    if state.dl.is_active(&game.id) {
        anyhow::bail!(tr!("este juego ya se está descargando", "this game is already downloading"));
    }
    let kind = match &choice {
        Choice::Official => LinkKind::Http,
        Choice::Source(o) => o.links.first().context(tr!("la opción no tiene enlaces", "this option has no links"))?.kind,
    };
    let dest = port_dir(&install_root, game);
    let cancelled = Arc::new(AtomicBool::new(false));
    state.dl.tasks.lock().unwrap().insert(
        game.id.clone(),
        Task {
            title: game.title.clone(),
            kind,
            status: Status::Starting,
            error: None,
            engine: Engine::Pending,
            cancelled: cancelled.clone(),
        },
    );

    tauri::async_runtime::spawn(async move {
        let key = game.id.clone();
        let result = run(&state, game, choice, &dest, &cancelled).await;
        if cancelled.load(Ordering::SeqCst) {
            let _ = tokio::fs::remove_dir_all(&dest).await;
            let _ = tokio::fs::remove_dir_all(seed_dir_of(&dest, game)).await;
            state.dl.tasks.lock().unwrap().remove(&key);
            return;
        }
        match result {
            Ok(()) => {
                state.dl.update(&key, |t| t.status = Status::Completed);
                let _ = app.emit("library-changed", ());
                if let Some((source_url, index, extract)) = then_data {
                    let installed = state.store.library.lock().unwrap().get(&key).cloned();
                    let opt = state
                        .store
                        .data_options(&key)
                        .into_iter()
                        .find(|o| o.source_url == source_url && o.index == index)
                        .map(|mut o| {
                            apply_extract(&mut o, extract);
                            o
                        });
                    if let (Some(installed), Some(opt)) = (installed, opt) {
                        if let Err(e) = start_data(app.clone(), state.clone(), installed, opt) {
                            state.dl.update(&key, |t| t.error = Some(tr!("los datos no se pudieron descargar: {e:#}", "the data couldn't be downloaded: {e:#}")));
                        }
                    }
                }
            }
            Err(e) => state.dl.update(&key, |t| {
                t.status = Status::Error;
                t.error = Some(format!("{e:#}"));
            }),
        }
    });
    Ok(())
}

async fn run(
    state: &Arc<AppState>,
    game: &'static CatalogGame,
    choice: Choice,
    dest: &Path,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let key = game.id.as_str();
    let mut expected_sha256 = None;
    let (links, origin, version) = match choice {
        Choice::Official => {
            let token = state.store.settings.lock().unwrap().github_token.clone();
            let build = github::latest_build(game, token.as_deref()).await?;
            expected_sha256 = build.sha256.clone().map(|h| (crate::http_multi::file_name_from_url(&build.url), h));
            let link = DownloadLink { kind: LinkKind::Http, uri: build.url };
            (vec![link], "GitHub oficial".to_string(), Some(build.version))
        }
        Choice::Source(o) => (o.links, o.source_name, o.label),
    };

    // Si se comparte, el torrent baja a .compartido/<port> (se siembra desde ahí) y se copia al port.
    let share = state.store.settings.lock().unwrap().share_downloads;
    let seed_dir = seed_dir_of(dest, game);
    state.dl.stop_seeding(key).await;
    let _ = tokio::fs::remove_dir_all(&seed_dir).await;
    let seeding = fetch(state, key, &links, dest, cancelled, share.then_some(seed_dir.as_path())).await?;
    verify_sha256(dest, expected_sha256).await?;
    // Un torrent suele traer una carpeta raíz (y archive.org, archivos de metadatos): se deja el port suelto.
    let d = dest.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || webseed::tidy_layout(&d)).await;

    state.dl.update(key, |t| t.status = Status::Extracting);
    let dest2 = dest.to_path_buf();
    let mut installed =
        tokio::task::spawn_blocking(move || finalize(&dest2, game, origin, version)).await??;
    if let Some(handle) = seeding {
        let torrent_file = seed_dir.with_extension("torrent");
        match handle.with_metadata(|m| m.torrent_bytes.clone()).map(|b| std::fs::write(&torrent_file, &b)) {
            Ok(Ok(())) => {
                installed.seed = Some(SeedInfo {
                    torrent: torrent_file.to_string_lossy().into_owned(),
                    dir: seed_dir.to_string_lossy().into_owned(),
                });
                state.dl.seeding.lock().unwrap().insert(key.to_string(), handle);
            }
            // Sin .torrent no se podría reanudar al reiniciar: se deja de compartir.
            _ => {
                if let Some(s) = state.dl.session.get() {
                    let _ = s.delete(handle.id().into(), false).await;
                }
                let _ = tokio::fs::remove_dir_all(&seed_dir).await;
            }
        }
    }
    state
        .store
        .library
        .lock()
        .unwrap()
        .insert(installed.id.clone(), installed);
    state.store.save_library()?;
    Ok(())
}

/// Comprueba la build oficial contra el SHA-256 que GitHub registró al subirla: si alguien la cambiara o la
/// descarga llegara dañada, no se instala.
async fn verify_sha256(dir: &Path, expected: Option<(String, String)>) -> anyhow::Result<()> {
    let Some((name, want)) = expected else { return Ok(()) };
    let path = dir.join(&name);
    if !path.is_file() {
        return Ok(());
    }
    let got = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        use sha2::{Digest, Sha256};
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)?;
        Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })
    .await??;
    anyhow::ensure!(
        got == want,
        tr!(
            "la descarga no coincide con la que publicó el autor en GitHub (SHA-256): está dañada o la han cambiado",
            "the download doesn't match the one the author published on GitHub (SHA-256): it's damaged or was altered"
        )
    );
    Ok(())
}

// ---------- Actualizar un port instalado ----------

/// Baja la última versión oficial en una carpeta aparte y la copia encima del port: se sobrescriben los archivos
/// del juego, pero lo que el port ha creado (partidas, configuración, mods) se queda.
pub fn start_update(app: AppHandle, state: Arc<AppState>, game: &'static CatalogGame) -> anyhow::Result<()> {
    if state.dl.is_active(&game.id) {
        anyhow::bail!(tr!("este juego ya se está descargando", "this game is already downloading"));
    }
    let installed = state
        .store
        .library
        .lock()
        .unwrap()
        .get(&game.id)
        .cloned()
        .context(tr!("el juego no está instalado", "the game isn't installed"))?;
    anyhow::ensure!(
        !installed.linked,
        tr!("los juegos vinculados a mano se actualizan por su cuenta", "manually linked games update on their own")
    );
    anyhow::ensure!(
        !state.running.lock().unwrap().contains(&game.id),
        tr!("cierra el juego antes de actualizarlo", "close the game before updating it")
    );
    let cancelled = Arc::new(AtomicBool::new(false));
    state.dl.tasks.lock().unwrap().insert(
        game.id.clone(),
        Task {
            title: game.title.clone(),
            kind: LinkKind::Http,
            status: Status::Starting,
            error: None,
            engine: Engine::Pending,
            cancelled: cancelled.clone(),
        },
    );
    tauri::async_runtime::spawn(async move {
        let key = game.id.clone();
        let port = PathBuf::from(&installed.path);
        let name = port.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let staging = port.with_file_name(format!("{name}.actualizacion"));
        let result = run_update(&state, game, &installed, &port, &staging, &cancelled).await;
        let _ = tokio::fs::remove_dir_all(&staging).await;
        if cancelled.load(Ordering::SeqCst) {
            state.dl.tasks.lock().unwrap().remove(&key);
            return;
        }
        match result {
            Ok(()) => {
                state.dl.update(&key, |t| t.status = Status::Completed);
                let _ = app.emit("library-changed", ());
            }
            Err(e) => state.dl.update(&key, |t| {
                t.status = Status::Error;
                t.error = Some(format!("{e:#}"));
            }),
        }
    });
    Ok(())
}

async fn run_update(
    state: &Arc<AppState>,
    game: &'static CatalogGame,
    installed: &InstalledGame,
    port: &Path,
    staging: &Path,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let key = game.id.as_str();
    let token = state.store.settings.lock().unwrap().github_token.clone();
    let build = github::latest_build(game, token.as_deref()).await?;
    let expected = build.sha256.clone().map(|h| (crate::http_multi::file_name_from_url(&build.url), h));
    let links = vec![DownloadLink { kind: LinkKind::Http, uri: build.url.clone() }];
    let _ = tokio::fs::remove_dir_all(staging).await;
    fetch(state, key, &links, staging, cancelled, None).await?;
    verify_sha256(staging, expected).await?;
    let s = staging.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || webseed::tidy_layout(&s)).await;
    state.dl.update(key, |t| t.status = Status::Extracting);
    let (s, version) = (staging.to_path_buf(), Some(build.version.clone()));
    let fresh = tokio::task::spawn_blocking(move || finalize(&s, game, "GitHub oficial".into(), version)).await??;
    copy_contents(staging, port).await?;
    // Lo que se compartía era la versión anterior: se deja de compartir.
    state.dl.stop_seeding(key).await;
    if let Some(seed) = &installed.seed {
        let _ = tokio::fs::remove_dir_all(&seed.dir).await;
        let _ = tokio::fs::remove_file(&seed.torrent).await;
    }
    {
        let mut lib = state.store.library.lock().unwrap();
        if let Some(g) = lib.get_mut(key) {
            g.version = fresh.version;
            g.origin = "GitHub oficial".into();
            g.seed = None;
            // El ejecutable elegido se respeta mientras siga existiendo.
            if !g.exe.as_ref().is_some_and(|e| port.join(e).is_file()) {
                g.exe = fresh.exe;
            }
        }
    }
    state.store.save_library()?;
    Ok(())
}

async fn run_torrent(
    state: &Arc<AppState>,
    key: &str,
    uri: &str,
    dest: &Path,
    cancelled: &AtomicBool,
    allow_seed: bool,
    // ¿Hay otro mirror detrás? Entonces no se espera indefinidamente a un torrent sin pares.
    has_fallback: bool,
    // En una carrera torrent/HTTP el torrent no se registra como motor de la tarea: va en este hueco.
    slot: Option<&TorrentSlot>,
) -> anyhow::Result<Option<Arc<ManagedTorrent>>> {
    let session = state.dl.session(&state.store).await?;
    let add = session.add_torrent(
        AddTorrent::from_url(uri),
        Some(AddTorrentOptions {
            output_folder: Some(dest.to_string_lossy().into_owned()),
            overwrite: true,
            ..Default::default()
        }),
    );
    // Un magnet primero tiene que conseguir la información del torrent de algún par.
    let added = if has_fallback {
        tokio::time::timeout(TORRENT_STALL, add)
            .await
            .map_err(|_| anyhow::anyhow!(tr!("el torrent no encontró pares en 1 minuto", "the torrent found no peers in 1 minute")))?
    } else {
        add.await
    };
    let handle = added
        .context(tr!("no se pudo añadir el torrent", "couldn't add the torrent"))?
        .into_handle()
        .context(tr!("respuesta inesperada del motor torrent", "unexpected response from the torrent engine"))?;
    match slot {
        Some(slot) => *slot.lock().unwrap() = Some(handle.clone()),
        None => state.dl.update(key, |t| {
            t.engine = Engine::Torrent(handle.clone());
            t.status = Status::Downloading;
        }),
    }

    // Tiempo esperando sin recibir ni un byte (no cuenta mientras está en pausa).
    let mut waiting = Duration::ZERO;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = session.delete(handle.id().into(), true).await;
            anyhow::bail!("cancelado");
        }
        let st = handle.stats();
        if st.finished {
            break;
        }
        if has_fallback && st.progress_bytes == 0 && !matches!(st.state, TorrentStatsState::Paused) {
            waiting += Duration::from_millis(500);
            if waiting >= TORRENT_STALL {
                let _ = session.delete(handle.id().into(), true).await;
                anyhow::bail!(tr!("el torrent no empezó a descargar en 1 minuto", "the torrent didn't start downloading in 1 minute"));
            }
        }
        if matches!(st.state, TorrentStatsState::Error) {
            let _ = session.delete(handle.id().into(), false).await;
            anyhow::bail!(tr!("error del torrent: {}", "torrent error: {}", st.error.unwrap_or_default()));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Compartiendo: el torrent sigue en la sesión y se devuelve para registrarlo.
    if allow_seed {
        return Ok(Some(handle));
    }
    let _ = session.delete(handle.id().into(), false).await;
    Ok(None)
}

/// Baja una opción con todos sus enlaces:
/// - torrents (magnet o .torrent): uno tras otro, pasando al siguiente si uno no tiene pares;
/// - directos: todos a la vez, por trozos y con varias conexiones (http_multi);
/// - si hay de los dos tipos, torrent y directos compiten y gana el primero que termine.
async fn fetch(
    state: &Arc<AppState>,
    key: &str,
    links: &[DownloadLink],
    dest: &Path,
    cancelled: &AtomicBool,
    // Carpeta desde la que compartir si lo baja un torrent (None = no compartir).
    seed_dir: Option<&Path>,
) -> anyhow::Result<Option<Arc<ManagedTorrent>>> {
    let torrents: Vec<&DownloadLink> = links.iter().filter(|l| l.kind != LinkKind::Http).collect();
    let https: Vec<String> = links.iter().filter(|l| l.kind == LinkKind::Http).map(|l| l.uri.clone()).collect();
    // Torrents sin pares (como los de archive.org): si el .torrent lista web seeds y la fuente no da enlaces
    // directos propios, se bajan de esos servidores HTTP mientras el torrent sigue buscando pares.
    let plan = if https.is_empty() && !torrents.is_empty() { web_seed_plan(&torrents).await } else { None };
    match (torrents.is_empty(), https.is_empty()) {
        (true, true) => anyhow::bail!(tr!("la opción no tiene enlaces", "this option has no links")),
        (false, true) if plan.is_some() => {
            race(state, key, &torrents, HttpSide::WebSeed(plan.unwrap()), dest, cancelled, seed_dir).await
        }
        (false, true) => match seed_dir {
            Some(sd) => {
                let handle = run_torrents(state, key, &torrents, sd, cancelled, true, None).await?;
                copy_contents(sd, dest).await?;
                Ok(handle)
            }
            None => run_torrents(state, key, &torrents, dest, cancelled, false, None).await,
        },
        (true, false) => {
            let ctl = Arc::new(HttpCtl::new());
            state.dl.update(key, |t| {
                t.kind = LinkKind::Http;
                t.engine = Engine::Http(ctl.clone());
                t.status = Status::Downloading;
            });
            http_multi::download(&https, dest, &ctl, cancelled).await.map(|_| None)
        }
        (false, false) => race(state, key, &torrents, HttpSide::Mirrors(&https), dest, cancelled, seed_dir).await,
    }
}

/// Lado HTTP de una carrera: mirrors directos de la fuente o los web seeds de un .torrent.
enum HttpSide<'a> {
    Mirrors(&'a [String]),
    WebSeed(webseed::Plan),
}

/// Descarga el primer `.torrent` (por http) de la lista y mira si trae web seeds utilizables.
async fn web_seed_plan(torrents: &[&DownloadLink]) -> Option<webseed::Plan> {
    for t in torrents.iter().filter(|t| t.uri.starts_with("http://") || t.uri.starts_with("https://")) {
        let res = crate::http_client().get(&t.uri).timeout(Duration::from_secs(20)).send().await.ok()?;
        if !res.status().is_success() || res.content_length().is_some_and(|n| n > (8 << 20)) {
            continue;
        }
        if let Some(plan) = res.bytes().await.ok().and_then(|b| webseed::plan_from_torrent(&b)) {
            return Some(plan);
        }
    }
    None
}

/// Carpeta desde la que se comparte un port: <Juego>\.compartido\<id del port>.
pub fn seed_dir_of(port_dir: &Path, game: &CatalogGame) -> PathBuf {
    port_dir.parent().unwrap_or(port_dir).join(".compartido").join(crate::sources::sanitize(&game.id))
}

/// Copia el contenido de una carpeta en otra (lo que se comparte se queda donde está).
async fn copy_contents(from: &Path, to: &Path) -> anyhow::Result<()> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_dir(&from, &to)).await?
}

fn copy_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).with_context(|| tr!("no se pudo copiar {}", "couldn't copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Al abrir Dársena: vuelve a compartir los ports instalados por torrent (comprueba los archivos y siembra).
pub async fn resume_seeding(state: Arc<AppState>) {
    if !state.store.settings.lock().unwrap().share_downloads {
        return;
    }
    let seeds: Vec<(String, SeedInfo)> = state
        .store
        .library
        .lock()
        .unwrap()
        .values()
        .filter_map(|g| g.seed.clone().map(|s| (g.id.clone(), s)))
        .collect();
    if seeds.is_empty() {
        return;
    }
    let Ok(session) = state.dl.session(&state.store).await else { return };
    for (id, seed) in seeds {
        // Si borraron la carpeta compartida no se vuelve a bajar nada: simplemente no se comparte.
        let (Ok(bytes), true) = (tokio::fs::read(&seed.torrent).await, Path::new(&seed.dir).is_dir()) else { continue };
        let added = session
            .add_torrent(
                AddTorrent::from_bytes(bytes),
                Some(AddTorrentOptions { output_folder: Some(seed.dir.clone()), overwrite: true, ..Default::default() }),
            )
            .await;
        if let Ok(Some(h)) = added.map(|r| r.into_handle()) {
            state.dl.seeding.lock().unwrap().insert(id, h);
        }
    }
}

async fn run_torrents(
    state: &Arc<AppState>,
    key: &str,
    torrents: &[&DownloadLink],
    dest: &Path,
    cancelled: &AtomicBool,
    allow_seed: bool,
    slot: Option<&TorrentSlot>,
) -> anyhow::Result<Option<Arc<ManagedTorrent>>> {
    let mut last_err = None;
    for (i, link) in torrents.iter().enumerate() {
        if slot.is_none() {
            state.dl.update(key, |t| {
                t.kind = link.kind;
                t.status = Status::Starting;
                t.engine = Engine::Pending;
            });
        }
        // En una carrera nunca se abandona el último torrent: los directos ya van en paralelo.
        let has_fallback = i + 1 < torrents.len();
        match run_torrent(state, key, &link.uri, dest, cancelled, allow_seed, has_fallback, slot).await {
            Ok(h) => return Ok(h),
            Err(e) => {
                if cancelled.load(Ordering::SeqCst) {
                    anyhow::bail!("cancelado");
                }
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!(tr!("sin torrents", "no torrents"))))
}

/// Torrent y descarga directa a la vez, cada uno en su carpeta. Al terminar uno se cancela el otro
/// y lo descargado pasa a `dest`.
async fn race(
    state: &Arc<AppState>,
    key: &str,
    torrents: &[&DownloadLink],
    side: HttpSide<'_>,
    dest: &Path,
    cancelled: &AtomicBool,
    seed_dir: Option<&Path>,
) -> anyhow::Result<Option<Arc<ManagedTorrent>>> {
    // Si se comparte, el torrent baja directamente a la carpeta compartida.
    let tdir = seed_dir.map(Path::to_path_buf).unwrap_or_else(|| dest.join(".torrent"));
    let hdir = dest.join(".directo");
    let share = seed_dir.is_some();

    // Fase 1: solo el torrent, con un plazo para demostrar que baja a buen ritmo.
    let slot: TorrentSlot = Arc::new(Mutex::new(None));
    state.dl.update(key, |t| {
        t.kind = torrents[0].kind;
        t.engine = Engine::Pending;
        t.status = Status::Starting;
    });
    let t_cancel = AtomicBool::new(false);
    let tf = run_torrents(state, key, torrents, &tdir, &t_cancel, share, Some(&slot));
    tokio::pin!(tf);
    let started = Instant::now();
    let (mut committed, mut shown) = (false, false);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let phase1 = loop {
        tokio::select! {
            r = &mut tf => break Some(r),
            _ = tick.tick() => {
                if cancelled.load(Ordering::SeqCst) {
                    t_cancel.store(true, Ordering::SeqCst);
                    break None;
                }
                let handle = slot.lock().unwrap().clone();
                if let (Some(h), false) = (&handle, shown) {
                    shown = true;
                    state.dl.update(key, |t| {
                        t.engine = Engine::Torrent(h.clone());
                        t.status = Status::Downloading;
                    });
                }
                let paused = state.dl.engine(key).is_some_and(|(_, st)| st == Status::Paused);
                if !committed && !paused && started.elapsed() >= RACE_PROBE {
                    let got = handle.as_ref().map(|h| h.stats().progress_bytes).unwrap_or(0);
                    if got >= RACE_MIN_BYTES {
                        committed = true; // va bien: sigue solo el torrent hasta el final
                    } else {
                        t_cancel.store(true, Ordering::SeqCst);
                        break None; // sin pares o muy lento: pasa a la descarga directa
                    }
                }
            }
        }
    };
    let torrent_done = phase1.is_some();
    if !torrent_done {
        // Se espera un poco a que el torrent suelte los archivos antes de borrarlos.
        let _ = tokio::time::timeout(Duration::from_secs(8), &mut tf).await;
    }
    let mut torrent_err = None;
    match phase1 {
        Some(Ok(handle)) if !cancelled.load(Ordering::SeqCst) => return finish_torrent(&tdir, dest, handle).await,
        Some(Err(e)) => torrent_err = Some(e),
        _ => {}
    }
    drop_torrent(state, &slot, &tdir, seed_dir.is_some()).await;
    if cancelled.load(Ordering::SeqCst) {
        anyhow::bail!("cancelado");
    }

    // Fase 2: descarga directa, ya sola.
    let ctl = Arc::new(HttpCtl::new());
    state.dl.update(key, |t| {
        t.kind = LinkKind::Http;
        t.engine = Engine::Http(ctl.clone());
        t.status = Status::Downloading;
    });
    let h_cancel = AtomicBool::new(false);
    let hf = async {
        match &side {
            HttpSide::Mirrors(urls) => http_multi::download(urls, &hdir, &ctl, &h_cancel).await.map(|_| ()),
            HttpSide::WebSeed(plan) => http_multi::download_plan(plan, &hdir, &ctl, &h_cancel).await,
        }
    };
    tokio::pin!(hf);
    let h_res = loop {
        tokio::select! {
            r = &mut hf => break r,
            _ = tick.tick() => {
                if cancelled.load(Ordering::SeqCst) { h_cancel.store(true, Ordering::SeqCst); }
            }
        }
    };
    let moved = match &h_res {
        Ok(()) if !cancelled.load(Ordering::SeqCst) => Some(move_contents(&hdir, dest).await),
        _ => None,
    };
    remove_dir_retry(&hdir).await;
    if cancelled.load(Ordering::SeqCst) {
        anyhow::bail!("cancelado");
    }
    if let Some(moved) = moved {
        return moved.map(|()| None);
    }
    let he = h_res.err().map(|e| format!("{e:#}")).unwrap_or_default();

    // Fase 3: si el torrent solo se cortó por lento (no falló), se le da otra oportunidad, ahora sin plazo.
    match torrent_err {
        None => {
            let handle = run_torrents(state, key, torrents, &tdir, cancelled, share, None).await;
            match handle {
                Ok(h) => finish_torrent(&tdir, dest, h).await,
                Err(e) => {
                    remove_dir_retry(&tdir).await;
                    Err(anyhow::anyhow!(tr!("falló la descarga directa ({he}) y también el torrent ({e:#})", "the direct download failed ({he}) and so did the torrent ({e:#})")))
                }
            }
        }
        Some(te) => Err(anyhow::anyhow!(tr!("falló el torrent ({te:#}) y también la descarga directa ({he})", "the torrent failed ({te:#}) and so did the direct download ({he})"))),
    }
}

/// El torrent terminó: sus archivos pasan al port (si se comparte, se copian y el torrent sigue sembrando).
async fn finish_torrent(tdir: &Path, dest: &Path, handle: Option<Arc<ManagedTorrent>>) -> anyhow::Result<Option<Arc<ManagedTorrent>>> {
    if handle.is_some() {
        copy_contents(tdir, dest).await?;
    } else {
        move_contents(tdir, dest).await?;
        remove_dir_retry(tdir).await;
    }
    Ok(handle)
}

/// Quita de la sesión el torrent a medias, con sus archivos, y borra su carpeta (y `.compartido` si queda vacía).
async fn drop_torrent(state: &Arc<AppState>, slot: &TorrentSlot, tdir: &Path, shared: bool) {
    let taken = slot.lock().unwrap().take();
    if let (Some(h), Some(sess)) = (taken, state.dl.session.get()) {
        let _ = sess.delete(h.id().into(), true).await;
    }
    remove_dir_retry(tdir).await;
    if shared {
        if let Some(parent) = tdir.parent() {
            let _ = tokio::fs::remove_dir(parent).await;
        }
    }
}

/// Borra una carpeta aunque Windows tarde unos instantes en soltar los archivos (el torrent los acaba de cerrar).
async fn remove_dir_retry(p: &Path) {
    for i in 0..10u64 {
        if tokio::fs::remove_dir_all(p).await.is_ok() || !p.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(300 * (i + 1))).await;
    }
}

/// Mueve el contenido de una carpeta a otra (misma unidad: es un renombrado, instantáneo).
async fn move_contents(from: &Path, to: &Path) -> anyhow::Result<()> {
    let mut rd = tokio::fs::read_dir(from).await?;
    while let Some(entry) = rd.next_entry().await? {
        let target = to.join(entry.file_name());
        if tokio::fs::metadata(&target).await.is_ok() {
            let _ = tokio::fs::remove_dir_all(&target).await;
            let _ = tokio::fs::remove_file(&target).await;
        }
        tokio::fs::rename(entry.path(), &target)
            .await
            .with_context(|| tr!("no se pudo mover {}", "couldn't move {}", entry.path().display()))?;
    }
    Ok(())
}

/// Extrae archivos .zip de la raíz, localiza el ejecutable y construye la entrada de biblioteca.
fn finalize(
    dest: &Path,
    game: &CatalogGame,
    origin: String,
    version: Option<String>,
) -> anyhow::Result<InstalledGame> {
    for entry in std::fs::read_dir(dest)? {
        let path = entry?.path();
        let is_zip = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
        if path.is_file() && is_zip {
            let file = std::fs::File::open(&path)?;
            zip::ZipArchive::new(file)
                .and_then(|mut z| z.extract(dest))
                .with_context(|| tr!("no se pudo extraer {}", "couldn't extract {}", path.display()))?;
            std::fs::remove_file(&path)?;
        }
    }
    // El código de una rama viene dentro de "repo-<commit>/": se deja suelto para que una actualización
    // (otro commit) caiga en las mismas rutas y no en una carpeta nueva.
    if game.branch.is_some() {
        crate::webseed::tidy_layout(dest)?;
    }

    Ok(InstalledGame {
        id: game.id.clone(),
        title: game.title.clone(),
        port: game.port.clone(),
        console: game.console.clone(),
        version,
        origin,
        path: dest.to_string_lossy().into_owned(),
        exe: resolve_exe(dest, game),
        installed_at: now(),
        playtime_secs: 0,
        last_played: None,
        data: Vec::new(),
        seed: None,
        linked: false,
        mods: Vec::new(),
        cloud_synced: None,
    })
}


// ---------- Datos de juego (de tus fuentes) ----------

pub fn data_key(port_id: &str) -> String {
    format!("data:{port_id}")
}

/// Nombre de carpeta válido en Windows: "Dragon Ball Z: Budokai" -> "Dragon Ball Z - Budokai".
pub fn windows_name(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            ':' => out.push_str(" -"),
            '<' | '>' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let out = out.trim_end_matches(['.', ' ']).to_string();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3"];
    if out.is_empty() { "Juego".into() } else if reserved.contains(&out.to_uppercase().as_str()) { format!("_{out}") } else { out }
}

/// Biblioteca\<Juego>
pub fn game_dir(library_root: &Path, game: &CatalogGame) -> PathBuf {
    library_root.join(windows_name(&crate::catalog::game_title(&game.game)))
}

/// Biblioteca\<Juego>\<Proyecto del port>
pub fn port_dir(library_root: &Path, game: &CatalogGame) -> PathBuf {
    game_dir(library_root, game).join(windows_name(&game.port))
}

/// Biblioteca\<Juego>\Datos: compartida por todos los ports del juego.
pub fn data_dir_of(installed: &InstalledGame) -> PathBuf {
    let port = PathBuf::from(&installed.path);
    port.parent().map(|p| p.join("Datos")).unwrap_or_else(|| port.join("Datos"))
}

/// Descarga datos de juego de una fuente y los coloca en la carpeta Datos del juego.
pub fn start_data(app: AppHandle, state: Arc<AppState>, installed: InstalledGame, opt: DataOption) -> anyhow::Result<()> {
    let key = data_key(&installed.id);
    if state.dl.is_active(&key) {
        anyhow::bail!(tr!("ya se están descargando datos para este juego", "data for this game is already downloading"));
    }
    let root = data_dir_of(&installed);
    std::fs::create_dir_all(&root).with_context(|| tr!("no se pudo crear {}", "couldn't create {}", root.display()))?;
    // Se valida antes de descargar nada: el destino debe quedar dentro de la carpeta Datos.
    let target = data_target(&root, opt.dest.as_deref())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    state.dl.tasks.lock().unwrap().insert(
        key.clone(),
        Task {
            title: tr!("Datos · {}", "Data · {}", installed.title),
            kind: opt.links.first().context(tr!("la fuente no tiene enlaces", "the source has no links"))?.kind,
            status: Status::Starting,
            error: None,
            engine: Engine::Pending,
            cancelled: cancelled.clone(),
        },
    );

    tauri::async_runtime::spawn(async move {
        let tmp = root.join(".descargando");
        let result = run_data(&state, &key, &opt, &tmp, &root, &target, &cancelled).await;
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        if cancelled.load(Ordering::SeqCst) {
            state.dl.tasks.lock().unwrap().remove(&key);
            return;
        }
        match result {
            Ok(()) => {
                if let Some(g) = state.store.library.lock().unwrap().get_mut(&installed.id) {
                    g.data.retain(|d| !(d.source_url == opt.source_url && d.index == opt.index));
                    g.data.push(InstalledData {
                        source_url: opt.source_url.clone(),
                        index: opt.index,
                        label: opt.label.clone(),
                        installed_at: now(),
                    });
                }
                let _ = state.store.save_library();
                state.dl.update(&key, |t| t.status = Status::Completed);
                let _ = app.emit("library-changed", ());
            }
            Err(e) => state.dl.update(&key, |t| {
                t.status = Status::Error;
                t.error = Some(format!("{e:#}"));
            }),
        }
    });
    Ok(())
}

async fn run_data(
    state: &Arc<AppState>,
    key: &str,
    opt: &DataOption,
    tmp: &Path,
    root: &Path,
    target: &Path,
    cancelled: &Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let _ = tokio::fs::remove_dir_all(tmp).await;
    fetch(state, key, &opt.links, tmp, cancelled, None).await?;
    state.dl.update(key, |t| t.status = Status::Extracting);
    let (tmp, root, target, opt) = (tmp.to_path_buf(), root.to_path_buf(), target.to_path_buf(), opt.clone());
    tokio::task::spawn_blocking(move || place_data(&tmp, &root, &target, &opt)).await?
}

/// Ruta de destino dentro de la carpeta del port. Rechaza rutas absolutas o con "..".
fn data_target(root: &Path, dest: Option<&str>) -> anyhow::Result<PathBuf> {
    let rel = Path::new(dest.unwrap_or("").trim_start_matches(['/', '\\']));
    let safe = rel.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
    anyhow::ensure!(safe, tr!("destino no permitido en la fuente: {:?} (debe ser una ruta dentro del juego)", "destination not allowed in the source: {:?} (it must be a path inside the game)", dest.unwrap_or("")));
    Ok(root.join(rel))
}

fn sha1_of(path: &Path) -> anyhow::Result<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Casilla «extraer el .zip» del diálogo: cambia cómo se colocan los datos sin tocar la fuente.
/// - `Some(true)`: se extrae el .zip aunque la fuente lo marque como archivo («dest» de tipo `x.zip` pasa a ser la raíz).
/// - `Some(false)`: el .zip se guarda tal cual en la carpeta Datos (o en la carpeta «dest», si la fuente da una).
/// - `None`: se respeta lo que diga la fuente.
pub fn apply_extract(opt: &mut DataOption, extract: Option<bool>) {
    let Some(extract) = extract else { return };
    let dest_is_zip_name = opt.dest.as_deref().is_some_and(|d| d.to_lowercase().ends_with(".zip"));
    if extract && opt.format == DataFormat::File {
        opt.format = DataFormat::Zip;
        if dest_is_zip_name {
            opt.dest = None;
        }
    } else if !extract && opt.format == DataFormat::Zip {
        opt.format = DataFormat::File;
    }
}

/// Coloca lo descargado: comprueba el SHA-1, extrae los zip o mueve los archivos al destino.
fn place_data(tmp: &Path, root: &Path, target: &Path, opt: &DataOption) -> anyhow::Result<()> {
    let mut files = Vec::new();
    walk(tmp, 16, &mut files);
    anyhow::ensure!(!files.is_empty(), tr!("la descarga no contiene archivos", "the download contains no files"));

    if let Some(expected) = opt.sha1.as_deref().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
        // Con varios archivos (torrent de carpeta) basta con que alguno coincida.
        let ok = files.iter().any(|f| sha1_of(f).is_ok_and(|h| h == expected));
        anyhow::ensure!(ok, tr!("el SHA-1 no coincide con el de la fuente: el archivo está dañado o es otra versión", "the SHA-1 doesn't match the source's: the file is damaged or a different version"));
    }

    let dest_is_file = opt.format == DataFormat::File
        && files.len() == 1
        && opt.dest.as_deref().is_some_and(|d| !d.ends_with(['/', '\\']) && Path::new(d).extension().is_some())
        && !target.is_dir();

    match opt.format {
        DataFormat::Zip => {
            std::fs::create_dir_all(target)?;
            let zips: Vec<&PathBuf> = files.iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))).collect();
            anyhow::ensure!(!zips.is_empty(), tr!("la fuente indica formato zip pero no se descargó ningún .zip", "the source says zip format but no .zip was downloaded"));
            for z in zips {
                zip::ZipArchive::new(std::fs::File::open(z)?)
                    .and_then(|mut a| a.extract(target))
                    .with_context(|| tr!("no se pudo extraer {}", "couldn't extract {}", z.display()))?;
            }
        }
        DataFormat::File if dest_is_file => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::remove_file(target);
            std::fs::rename(&files[0], target)?;
        }
        DataFormat::File => {
            // Uno o varios archivos dentro de la carpeta destino, conservando la estructura del torrent.
            for f in &files {
                let rel = f.strip_prefix(tmp).unwrap_or(f);
                let to = target.join(rel);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let _ = std::fs::remove_file(&to);
                std::fs::rename(f, &to)?;
            }
        }
    }
    anyhow::ensure!(target.starts_with(root), tr!("destino fuera de la carpeta del juego", "destination outside the game folder"));
    Ok(())
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, depth - 1, out);
            }
        } else {
            out.push(p);
        }
    }
}

/// Devuelve la ruta del ejecutable relativa a `dest`.
pub fn resolve_exe(dest: &Path, game: &CatalogGame) -> Option<String> {
    let rel = |p: &Path| p.strip_prefix(dest).ok().map(|r| r.to_string_lossy().into_owned());
    let mut files = Vec::new();
    walk(dest, 4, &mut files);

    // Nombre conocido del catálogo: el zip pudo traer una carpeta raíz extra, se busca por nombre.
    if let Some(want) = game.exe.as_deref().and_then(|e| Path::new(e).file_name()) {
        let want = want.to_string_lossy().to_lowercase();
        if let Some(f) = files
            .iter()
            .find(|f| f.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == want))
        {
            return rel(f);
        }
    }

    if PLATFORM != "windows" {
        return None;
    }
    let skip = ["unins", "setup", "redist", "crash", "update", "dxsetup", "vc_", "installer"];
    let hints: Vec<String> = [&game.port, &game.title, &game.id]
        .iter()
        .map(|s| crate::catalog::normalize(s))
        .collect();
    let mut exes: Vec<&PathBuf> = files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")))
        .filter(|f| {
            let n = f.file_name().unwrap().to_string_lossy().to_lowercase();
            !skip.iter().any(|s| n.contains(s))
        })
        .collect();
    exes.sort_by_key(|f| {
        let n = crate::catalog::normalize(&f.file_stem().unwrap().to_string_lossy());
        let hinted = !n.is_empty() && hints.iter().any(|h| h.contains(&n) || n.contains(h.as_str()));
        let depth = f.strip_prefix(dest).map(|r| r.components().count()).unwrap_or(9);
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        (!hinted, depth, std::cmp::Reverse(size))
    });
    exes.first().and_then(|f| rel(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flujo completo sin UI: resolver release oficial, descargar, extraer y detectar el ejecutable.
    #[tokio::test]
    #[ignore = "usa red; ejecutar con --ignored"]
    async fn instala_starship_desde_github() {
        let game = crate::catalog::get("harbourmasters-starship").expect("en catálogo");
        let build = github::latest_build(game, None).await.expect("release");
        println!("release {} -> {} ({} bytes)", build.version, build.asset_name, build.size);

        let dest = std::env::temp_dir().join("darsena-test-starship");
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        let bytes = crate::http_client().get(&build.url).send().await.unwrap().bytes().await.unwrap();
        std::fs::write(dest.join(http_multi::file_name_from_url(&build.url)), &bytes).unwrap();

        let installed = finalize(&dest, game, "GitHub oficial".into(), Some(build.version)).unwrap();
        println!("exe detectado: {:?}", installed.exe);
        assert!(installed.exe.is_some_and(|e| e.to_lowercase().ends_with(".exe")));
        let _ = std::fs::remove_dir_all(&dest);
    }
}
