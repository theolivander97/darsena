//! Portadas: base de datos propia (carpeta `covers/` del proyecto, publicable en un CDN).
//! La UI pide `cover://localhost/<id>?v=<hash>`; se sirve desde la caché en disco y solo la
//! primera vez se descarga del origen. El hash cambia si cambia la imagen, así que nunca queda vieja.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tauri::http::{header, Request, Response, StatusCode};

/// Origen por defecto en release: carpeta `covers/` del repositorio publicada por jsDelivr.
/// Se puede cambiar en Ajustes (URL o carpeta local).
pub const DEFAULT_REMOTE: &str = "https://cdn.jsdelivr.net/gh/theolivander97/portlauncher-covers@main";
/// Los logos van en su propio repo: jsDelivr no sirve repos de más de 50 MB y el de portadas está al límite.
pub const LOGOS_REMOTE: &str = "https://cdn.jsdelivr.net/gh/theolivander97/darsena-logos@main";
const LOGOS_CACHE: &str = "logos-index.json";

/// id -> versión del logo.
static LOGOS: RwLock<Option<&'static HashMap<String, String>>> = RwLock::new(None);

fn parse_logos(text: &str) -> Option<HashMap<String, String>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(v["logos"].as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
}

fn set_logos(map: HashMap<String, String>) {
    *LOGOS.write().unwrap() = Some(Box::leak(Box::new(map)));
}

/// Índice para la interfaz: portadas y heroes, con la versión del logo de cada juego si la hay.
pub fn index_with_logos() -> HashMap<String, Versions> {
    let mut out = index().clone();
    if let Some(logos) = *LOGOS.read().unwrap() {
        for (id, v) in logos {
            out.entry(id.clone()).or_insert(Versions { v: None, h: None, l: None }).l = Some(v.clone());
        }
    }
    out
}

/// Descarga el índice de logos publicado (y lo guarda para la próxima vez sin red).
pub async fn refresh_logos(data_dir: &Path) -> anyhow::Result<()> {
    let text = crate::http_client()
        .get(format!("{LOGOS_REMOTE}/index.json"))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let map = parse_logos(&text).ok_or_else(|| anyhow::anyhow!("logos/index.json"))?;
    tokio::fs::write(data_dir.join(LOGOS_CACHE), &text).await?;
    set_logos(map);
    Ok(())
}

#[derive(Deserialize)]
struct IndexFile {
    covers: HashMap<String, Versions>,
}

/// Versiones (hash del contenido) de la portada y del hero panorámico de un juego.
/// Sin versión = no tiene esa imagen.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Versions {
    #[serde(default)]
    pub v: Option<String>,
    #[serde(default)]
    pub h: Option<String>,
    /// Logo del juego (su nombre con la tipografía oficial, fondo transparente).
    #[serde(default)]
    pub l: Option<String>,
}

const INDEX_CACHE: &str = "covers-index.json";

fn parse_index(text: &str) -> Option<HashMap<String, Versions>> {
    let f: IndexFile = serde_json::from_str(text).ok()?;
    Some(f.covers.into_iter().filter(|(_, e)| e.v.is_some() || e.h.is_some() || e.l.is_some()).collect())
}

/// id -> versión. El embebido sirve sin red; al arrancar se cambia por el último publicado,
/// para que los juegos nuevos del catálogo remoto tengan portada sin actualizar la app.
static INDEX: RwLock<Option<&'static HashMap<String, Versions>>> = RwLock::new(None);

fn set_index(map: HashMap<String, Versions>) {
    *INDEX.write().unwrap() = Some(Box::leak(Box::new(map)));
}

pub fn index() -> &'static HashMap<String, Versions> {
    if let Some(i) = *INDEX.read().unwrap() {
        return i;
    }
    set_index(parse_index(include_str!("../../covers/index.json")).unwrap_or_default());
    index()
}

/// Al arrancar: usa el último índice descargado, si lo hay.
pub fn init(data_dir: &Path) {
    let cached = std::fs::read_to_string(data_dir.join(INDEX_CACHE)).ok().and_then(|t| parse_index(&t));
    if let Some(map) = cached.filter(|m| m.len() >= index().len()) {
        set_index(map);
    }
    if let Some(map) = std::fs::read_to_string(data_dir.join(LOGOS_CACHE)).ok().and_then(|t| parse_logos(&t)) {
        set_logos(map);
    }
}

/// Descarga el índice publicado en el CDN. Devuelve cuántas portadas tiene.
pub async fn refresh_index(data_dir: &Path) -> anyhow::Result<usize> {
    let text = crate::http_client()
        .get(format!("{DEFAULT_REMOTE}/index.json"))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let map = parse_index(&text).ok_or_else(|| anyhow::anyhow!(tr!("índice de portadas inválido", "invalid cover index")))?;
    let n = map.len();
    tokio::fs::write(data_dir.join(INDEX_CACHE), &text).await?;
    set_index(map);
    Ok(n)
}

/// Origen por defecto: en desarrollo, la carpeta `covers/` del proyecto.
pub fn default_origin() -> String {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("covers")
            .to_string_lossy()
            .into_owned()
    } else {
        DEFAULT_REMOTE.to_string()
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 120 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn respond(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "image/webp")
        // La versión va en la URL: se puede cachear para siempre.
        .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap()
}

/// `file` es la ruta dentro del origen: "<id>.webp" o "heroes/<id>.webp".
async fn fetch_origin(origin: &str, file: &str) -> anyhow::Result<Vec<u8>> {
    if origin.starts_with("http://") || origin.starts_with("https://") {
        let url = format!("{}/{file}", origin.trim_end_matches('/'));
        Ok(crate::http_client()
            .get(url)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .to_vec())
    } else {
        Ok(tokio::fs::read(Path::new(origin).join(file)).await?)
    }
}

/// Sirve una portada (`/<id>`), un hero (`/hero/<id>`) o un logo (`/logo/<id>`): caché en disco -> origen (y guarda en caché).
pub async fn serve(cache_dir: PathBuf, origin: String, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = req.uri();
    let path = uri.path().trim_start_matches('/');
    // Los logos siempre salen de su repo (o, en desarrollo, de la carpeta logos/ del proyecto).
    let origin = if path.starts_with("logo/") {
        if cfg!(debug_assertions) {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("logos").to_string_lossy().into_owned()
        } else {
            LOGOS_REMOTE.to_string()
        }
    } else {
        origin
    };
    let (id, file, cache_dir) = if let Some(id) = path.strip_prefix("hero/") {
        (id, format!("heroes/{id}.webp"), cache_dir.join("heroes"))
    } else if let Some(id) = path.strip_prefix("logo/") {
        (id, format!("{id}.webp"), cache_dir.join("logos"))
    } else {
        (path, format!("{path}.webp"), cache_dir)
    };
    let v = uri
        .query()
        .and_then(|q| q.split('&').find_map(|p| p.strip_prefix("v=")))
        .unwrap_or("0");
    if !valid_id(id) || !valid_id(v) {
        return respond(StatusCode::BAD_REQUEST, Vec::new());
    }

    let cached = cache_dir.join(format!("{id}-{v}.webp"));
    if let Ok(bytes) = tokio::fs::read(&cached).await {
        return respond(StatusCode::OK, bytes);
    }
    match fetch_origin(&origin, &file).await {
        Ok(bytes) => {
            let _ = tokio::fs::create_dir_all(&cache_dir).await;
            // Borra versiones anteriores de esta portada.
            if let Ok(mut rd) = tokio::fs::read_dir(&cache_dir).await {
                while let Ok(Some(e)) = rd.next_entry().await {
                    let name = e.file_name().to_string_lossy().into_owned();
                    // "<id>-<v>.webp" exacto: la versión no lleva guiones, así "foo" no borra "foo-bar-…".
                    let old_version = name
                        .strip_prefix(&format!("{id}-"))
                        .and_then(|r| r.strip_suffix(".webp"))
                        .is_some_and(|ver| !ver.contains('-'));
                    if old_version {
                        let _ = tokio::fs::remove_file(e.path()).await;
                    }
                }
            }
            let _ = tokio::fs::write(&cached, &bytes).await;
            respond(StatusCode::OK, bytes)
        }
        Err(_) => respond(StatusCode::NOT_FOUND, Vec::new()),
    }
}
