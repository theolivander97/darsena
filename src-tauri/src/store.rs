//! Estado persistente: ajustes, fuentes (con caché) y biblioteca instalada.

use crate::sources::{DataFormat, DownloadLink, SourceDoc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceEntry {
    pub url: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub last_fetch: Option<u64>,
    /// Enlaces asociados a algún juego del catálogo.
    #[serde(default)]
    pub matched: usize,
    /// Enlaces que no corresponden a ningún juego del catálogo (se ignoran).
    #[serde(default)]
    pub unmatched: usize,
    #[serde(default)]
    pub error: Option<String>,
}

fn yes() -> bool {
    true
}

fn default_upload_kbps() -> u32 {
    1024
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub install_dir: String,
    #[serde(default)]
    pub sources: Vec<SourceEntry>,
    /// Compartir (sembrar) en segundo plano los ports bajados por torrent mientras Dársena está abierto.
    #[serde(default = "yes")]
    pub share_downloads: bool,
    /// Límite de subida al compartir, en KB/s (0 = sin límite).
    #[serde(default = "default_upload_kbps")]
    pub upload_limit_kbps: u32,
    /// Token opcional de GitHub: sube el límite de 60 a 5000 peticiones/hora.
    #[serde(default)]
    pub github_token: Option<String>,
    /// Origen de las portadas (URL base o carpeta). Si falta, el de por defecto.
    #[serde(default)]
    pub covers_origin: Option<String>,
    /// Idioma elegido en Ajustes ("es" o "en"); None = el del sistema.
    #[serde(default)]
    pub language: Option<String>,
    /// Juegos marcados como favoritos (ids de juego).
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Estanterías de la página principal de la biblioteca, en orden (p. ej. "installed", "console:Xbox 360").
    #[serde(default)]
    pub shelves: Vec<String>,
    /// Al cerrar la ventana, Dársena sigue en la bandeja del sistema (como Steam).
    #[serde(default = "yes")]
    pub close_to_tray: bool,
    /// Abrir Dársena (en la bandeja) al iniciar sesión en Windows.
    #[serde(default = "yes")]
    pub start_with_windows: bool,
    /// Al abrir Dársena, actualizar solos los ports instalados desde GitHub que tengan versión nueva.
    #[serde(default = "yes")]
    pub auto_update_ports: bool,
    /// Mostrar en Discord a qué estás jugando (Rich Presence).
    #[serde(default = "yes")]
    pub discord_presence: bool,
    /// Mostrar los mods +18 del Taller.
    #[serde(default)]
    pub show_nsfw: bool,
    /// Subir las partidas a la nube al cerrar un juego (con sesión de GitHub).
    #[serde(default = "yes")]
    pub cloud_saves: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGame {
    /// Id del juego en el catálogo.
    pub id: String,
    pub title: String,
    pub port: String,
    pub console: String,
    pub version: Option<String>,
    /// De dónde se descargó: "GitHub oficial" o el nombre de la fuente.
    pub origin: String,
    pub path: String,
    pub exe: Option<String>,
    pub installed_at: u64,
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default)]
    pub last_played: Option<u64>,
    /// Datos de juego instalados en este port desde tus fuentes.
    #[serde(default)]
    pub data: Vec<InstalledData>,
    /// Si se bajó por torrent y se está compartiendo: dónde está lo que se siembra.
    #[serde(default)]
    pub seed: Option<SeedInfo>,
    /// Juego que ya tenías instalado fuera de Dársena y vinculaste eligiendo su ejecutable:
    /// al quitarlo solo se olvida, sus archivos no se tocan.
    #[serde(default)]
    pub linked: bool,
    /// Mods del Taller instalados en este port.
    #[serde(default)]
    pub mods: Vec<crate::workshop::InstalledMod>,
    /// Guardado en la nube: marca de tiempo de lo último sincronizado desde este PC.
    #[serde(default)]
    pub cloud_synced: Option<u64>,
}

/// Torrent que se sigue compartiendo tras instalar un port.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SeedInfo {
    /// Archivo .torrent guardado (sirve también para magnets: no hace falta volver a pedir sus metadatos).
    pub torrent: String,
    /// Carpeta con los archivos tal cual los trae el torrent (el .zip sin extraer).
    pub dir: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledData {
    pub source_url: String,
    pub index: usize,
    pub label: String,
    pub installed_at: u64,
}

/// Datos de juego de una fuente, ofrecidos para un port concreto.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DataOption {
    pub source_name: String,
    pub source_url: String,
    pub index: usize,
    pub label: String,
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub format: DataFormat,
    pub dest: Option<String>,
    pub links: Vec<DownloadLink>,
    /// Qué contiene (según la fuente).
    pub contains: Option<String>,
    /// ¿Sirve para este port? None si la fuente no dice qué contiene.
    pub compatible: Option<bool>,
}

/// Enlace de una fuente ofrecido para un juego concreto.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceOption {
    pub source_name: String,
    pub source_url: String,
    pub index: usize,
    pub label: Option<String>,
    pub size: Option<u64>,
    pub upload_date: Option<String>,
    /// Mirrors del mismo archivo; se prueban en orden (torrents primero).
    pub links: Vec<DownloadLink>,
}

pub struct Store {
    dir: PathBuf,
    pub settings: Mutex<Settings>,
    /// URL de fuente -> documento descargado (caché para funcionar sin conexión).
    pub sources: Mutex<HashMap<String, SourceDoc>>,
    /// Ports instalados: id del port -> instalación.
    pub library: Mutex<HashMap<String, InstalledGame>>,
    /// Juegos añadidos a la biblioteca (instalados o no): id del juego -> fecha en que se añadió.
    pub owned: Mutex<HashMap<String, u64>>,
    /// Perfil del jugador: tiempo por juego y logros (se conserva al desinstalar).
    pub profile: Mutex<crate::profile::Profile>,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Escritura atómica: archivo temporal + rename, para no corromper datos si se cierra a medias.
fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl Store {
    pub fn load(dir: PathBuf, default_install_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let settings = read_json(&dir.join("settings.json")).unwrap_or(Settings {
            install_dir: default_install_dir.to_string_lossy().into_owned(),
            sources: Vec::new(),
            share_downloads: true,
            upload_limit_kbps: default_upload_kbps(),
            github_token: None,
            covers_origin: None,
            language: None,
            favorites: Vec::new(),
            shelves: Vec::new(),
            close_to_tray: true,
            start_with_windows: true,
            auto_update_ports: true,
            discord_presence: true,
            show_nsfw: false,
            cloud_saves: true,
        });
        let sources = read_json(&dir.join("sources-cache.json")).unwrap_or_default();
        let library: HashMap<String, InstalledGame> = read_json(&dir.join("library.json")).unwrap_or_default();
        let mut owned: HashMap<String, u64> = read_json(&dir.join("owned.json")).unwrap_or_default();
        // Todo juego con un port instalado está en la biblioteca (también para datos de versiones anteriores).
        for g in library.values() {
            if let Some(c) = crate::catalog::get(&g.id) {
                owned.entry(c.game.clone()).or_insert(g.installed_at);
            }
        }
        // Perfil: si aún no existe, se crea con el tiempo de juego que ya guardaba la biblioteca.
        let profile = read_json(&dir.join("profile.json")).unwrap_or_else(|| {
            let mut p = crate::profile::Profile::default();
            for g in library.values().filter(|g| g.playtime_secs > 0) {
                let Some(c) = crate::catalog::get(&g.id) else { continue };
                let s = p.games.entry(c.game.clone()).or_default();
                s.playtime_secs += g.playtime_secs;
                s.sessions += 1;
                s.first_played = s.first_played.or(g.last_played);
                s.last_played = s.last_played.max(g.last_played);
                s.ports.insert(g.id.clone());
            }
            p
        });
        Ok(Self {
            dir,
            settings: Mutex::new(settings),
            sources: Mutex::new(sources),
            library: Mutex::new(library),
            owned: Mutex::new(owned),
            profile: Mutex::new(profile),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.dir
    }

    pub fn save_settings(&self) -> anyhow::Result<()> {
        write_json(&self.dir.join("settings.json"), &*self.settings.lock().unwrap())
    }

    pub fn save_sources(&self) -> anyhow::Result<()> {
        write_json(&self.dir.join("sources-cache.json"), &*self.sources.lock().unwrap())
    }

    pub fn save_library(&self) -> anyhow::Result<()> {
        write_json(&self.dir.join("library.json"), &*self.library.lock().unwrap())
    }

    pub fn save_profile(&self) -> anyhow::Result<()> {
        write_json(&self.dir.join("profile.json"), &*self.profile.lock().unwrap())
    }

    pub fn save_owned(&self) -> anyhow::Result<()> {
        write_json(&self.dir.join("owned.json"), &*self.owned.lock().unwrap())
    }

    /// Datos de juego de las fuentes activas que sirven para un port (por su juego y, si se indica, por port).
    pub fn data_options(&self, port_id: &str) -> Vec<DataOption> {
        let Some(port) = crate::catalog::get(port_id) else { return Vec::new() };
        let settings = self.settings.lock().unwrap();
        let docs = self.sources.lock().unwrap();
        let mut out = Vec::new();
        for src in settings.sources.iter().filter(|s| s.enabled) {
            let Some(doc) = docs.get(&src.url) else { continue };
            for (i, d) in doc.data.iter().enumerate() {
                if d.game_id().as_deref() != Some(port.game.as_str()) {
                    continue;
                }
                if !d.ports.is_empty() && !d.ports.iter().any(|p| p == port_id) {
                    continue;
                }
                let links = d.resolve();
                if links.is_empty() {
                    continue;
                }
                out.push(DataOption {
                    source_name: doc.name.clone(),
                    source_url: src.url.clone(),
                    index: i,
                    label: d.label.clone().unwrap_or_else(|| tr!("Datos del juego", "Game data").into()),
                    size: d.size,
                    sha1: d.sha1.clone(),
                    format: d.format,
                    dest: d.dest.clone(),
                    links,
                    contains: d.contains.clone(),
                    compatible: d.contains.as_deref().map(|c| crate::catalog::data_compatible(&port.needs, c)),
                });
            }
        }
        // Primero los compatibles, luego los desconocidos y al final los que no sirven.
        out.sort_by_key(|o| match o.compatible { Some(true) => 0, None => 1, Some(false) => 2 });
        out
    }

    /// Enlaces de las fuentes activas para cada juego del catálogo: id -> opciones.
    pub fn source_options(&self) -> HashMap<&'static str, Vec<SourceOption>> {
        let settings = self.settings.lock().unwrap();
        let docs = self.sources.lock().unwrap();
        let mut out: HashMap<&'static str, Vec<SourceOption>> = HashMap::new();
        for src in settings.sources.iter().filter(|s| s.enabled) {
            let Some(doc) = docs.get(&src.url) else { continue };
            for (i, l) in doc.downloads.iter().enumerate() {
                if !l.for_this_platform() {
                    continue;
                }
                let Some(game) = l.game() else { continue };
                let links = l.resolve();
                if links.is_empty() {
                    continue;
                }
                out.entry(game.id.as_str()).or_default().push(SourceOption {
                    source_name: doc.name.clone(),
                    source_url: src.url.clone(),
                    index: i,
                    label: l.label.clone(),
                    size: l.size,
                    upload_date: l.upload_date.clone(),
                    links,
                });
            }
        }
        out
    }
}
