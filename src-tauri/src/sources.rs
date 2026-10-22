//! Fuentes: documentos JSON con enlaces de descarga, que añade cada usuario.
//! - `downloads`: builds de ports (mirrors de las releases).
//! - `data`: datos de juego de tus propias copias (ROM, ISO, default.xex…), desde donde los tengas subidos.
//! No definen juegos: cada enlace se asocia a un juego del catálogo por su id o por título.

use crate::catalog;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceDoc {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub downloads: Vec<SourceLink>,
    #[serde(default)]
    pub data: Vec<DataEntry>,
}

/// Formato del archivo de datos: se copia tal cual o se extrae.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DataFormat {
    #[default]
    File,
    Zip,
}

/// Datos de juego (de tu copia) para colocar dentro de la carpeta de un port instalado.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DataEntry {
    /// Id del juego del catálogo (el que agrupa los ports, p. ej. "star-fox-64").
    #[serde(default)]
    pub game: Option<String>,
    /// Título del juego, usado si no hay `game`.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    /// Mirrors del mismo archivo, se prueban en orden.
    #[serde(default)]
    pub uris: Vec<String>,
    /// Huella SHA-1 esperada: se comprueba tras descargar.
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// Qué hay dentro: "iso", "rvz", "z64", "carpeta"… Se compara con lo que acepta cada port (`needs`).
    #[serde(default)]
    pub contains: Option<String>,
    #[serde(default)]
    pub format: DataFormat,
    /// Destino dentro de la carpeta Datos del juego: archivo ("oot.z64") o carpeta ("game"). Por defecto, la raíz.
    #[serde(default)]
    pub dest: Option<String>,
    /// Si solo vale para ciertos ports del juego (ids de port).
    #[serde(default)]
    pub ports: Vec<String>,
}

impl DataEntry {
    /// Id del juego del catálogo al que pertenecen estos datos.
    pub fn game_id(&self) -> Option<String> {
        if let Some(g) = self.game.as_deref().filter(|g| catalog::all().iter().any(|p| p.game == *g)) {
            return Some(g.to_string());
        }
        catalog::match_game(None, self.title.as_deref()).map(|p| p.game.clone())
    }

    pub fn resolve(&self) -> Vec<DownloadLink> {
        resolve_uris(self.uri.as_deref(), None, &self.uris)
    }
}

/// URIs utilizables (uri + uris), ordenadas torrents primero.
fn resolve_uris(uri: Option<&str>, kind: Option<LinkKind>, uris: &[String]) -> Vec<DownloadLink> {
    let mut out: Vec<DownloadLink> = uri
        .into_iter()
        .map(|u| (u, kind))
        .chain(uris.iter().map(|u| (u.as_str(), None)))
        .filter_map(|(u, k)| Some(DownloadLink { kind: k.or_else(|| kind_of(u))?, uri: u.to_string() }))
        .collect();
    out.sort_by_key(|l| l.kind as u8);
    out
}

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    Magnet,
    Torrent,
    Http,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceLink {
    /// Id del juego en el catálogo (preferido).
    #[serde(default)]
    pub game_id: Option<String>,
    /// Título del juego, usado si no hay gameId.
    #[serde(default)]
    pub title: Option<String>,
    /// Si falta, se deduce de la URI.
    #[serde(rename = "type", default)]
    pub kind: Option<LinkKind>,
    #[serde(default)]
    pub uri: Option<String>,
    /// Formato de Hydra: varias URIs alternativas para el mismo archivo.
    #[serde(default)]
    pub uris: Vec<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub upload_date: Option<String>,
}

/// Enlace ya resuelto y listo para descargar.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadLink {
    pub kind: LinkKind,
    pub uri: String,
}

pub const PLATFORM: &str = if cfg!(target_os = "windows") {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

pub fn kind_of(uri: &str) -> Option<LinkKind> {
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with("magnet:") {
        Some(LinkKind::Magnet)
    } else if lower.split(['?', '#']).next().is_some_and(|p| p.ends_with(".torrent")) {
        Some(LinkKind::Torrent)
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(LinkKind::Http)
    } else {
        None
    }
}

impl SourceLink {
    pub fn for_this_platform(&self) -> bool {
        self.platform.as_deref().map_or(true, |p| p.eq_ignore_ascii_case(PLATFORM))
    }

    pub fn game(&self) -> Option<&'static catalog::CatalogGame> {
        catalog::match_game(self.game_id.as_deref(), self.title.as_deref())
    }

    /// Todas las URIs utilizables de este enlace, ordenadas torrents primero.
    pub fn resolve(&self) -> Vec<DownloadLink> {
        resolve_uris(self.uri.as_deref(), self.kind, &self.uris)
    }
}

/// Nombre seguro para usar como carpeta o archivo.
pub fn sanitize(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let out = out.trim_matches('.').to_string();
    if out.is_empty() { "juego".into() } else { out }
}

/// Descarga y valida una fuente. Acepta http(s) o una ruta local (útil para probar fuentes propias).
pub async fn fetch(url: &str) -> anyhow::Result<SourceDoc> {
    let text = if url.starts_with("http://") || url.starts_with("https://") {
        crate::http_client()
            .get(url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?
    } else {
        let path = url.strip_prefix("file:///").unwrap_or(url);
        tokio::fs::read_to_string(path).await?
    };
    let doc: SourceDoc = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!(tr!("JSON de fuente inválido: {e}", "invalid source JSON: {e}")))?;
    if doc.schema_version > SCHEMA_VERSION {
        anyhow::bail!(tr!(
            "la fuente usa schemaVersion {} y este launcher solo entiende hasta {}",
            "the source uses schemaVersion {} and this launcher only understands up to {}",
            doc.schema_version,
            SCHEMA_VERSION
        ));
    }
    Ok(doc)
}
