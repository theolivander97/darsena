//! Catálogo de juegos porteados.
//!
//! Se publica en GitHub (repo darsena-launcher) y se comprueba al abrir el launcher: si cambió, se descarga,
//! se valida, se guarda en disco y sustituye al actual sin reiniciar. Sin conexión se usa el último descargado
//! o, si no hay ninguno, el que va embebido en el binario.
//! Las fuentes solo aportan enlaces de descarga que se asocian a estas entradas.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::RwLock;

/// Catálogo publicado. raw.githubusercontent se actualiza a los pocos minutos de editar el archivo en GitHub.
pub const REMOTE_URL: &str = "https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/catalog.json";
const CACHE_FILE: &str = "catalog-remote.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGame {
    #[serde(default)]
    pub id: String,
    /// Id del juego al que pertenece este port (varios ports pueden compartirlo).
    #[serde(default)]
    pub game: String,
    /// Nombre del juego original.
    #[serde(default)]
    pub title: String,
    /// Nombre del proyecto del port.
    #[serde(default)]
    pub port: String,
    #[serde(default)]
    pub author: String,
    /// Dónde se publica el port: "github" (releases de `repo`) o "external" (solo desde fuentes).
    #[serde(default)]
    pub origin: Origin,
    /// Repositorio de GitHub "owner/name" con las releases oficiales (obligatorio si origin es "github").
    #[serde(default)]
    pub repo: String,
    /// Página del proyecto (ModDB, Discord, web…). Para ports externos es el único enlace oficial.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Proyecto de gitlab.com ("grupo/proyecto") cuyas releases tienen la build. Va con origin "external" para que
    /// las versiones antiguas de Dársena (que no conocen GitLab) lo traten como un port que necesita una fuente.
    #[serde(default)]
    pub gitlab: Option<String>,
    /// Todas las consolas de las que trae juegos, si son varias (la principal es `console`).
    #[serde(default)]
    pub consoles: Vec<String>,
    /// Uso de IA en el desarrollo del port: "none", "assisted" o "generated" (None = no se sabe).
    #[serde(default)]
    pub ai: Option<String>,
    /// Plataforma original normalizada (Nintendo 64, PlayStation…).
    #[serde(default)]
    pub console: String,
    #[serde(default)]
    pub franchise: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Regex del nombre del asset de Windows en las releases, si se conoce.
    #[serde(default)]
    pub asset: Option<String>,
    /// Nombre del ejecutable, si se conoce.
    #[serde(default)]
    pub exe: Option<String>,
    /// Rama de GitHub cuyo código es el port: proyectos sin builds en sus releases cuya aplicación está en el
    /// propio repositorio (p. ej. un instalador .bat que recompila el juego con tus datos).
    #[serde(default)]
    pub branch: Option<String>,
    /// Paso previo a jugar: hasta que exista algo de `done`, JUGAR abre `exe` para preparar el juego con tus datos.
    #[serde(default)]
    pub setup: Option<Setup>,
    /// Requisitos del sistema: { "minimum": {...}, "recommended": {...} } con os, cpu, ram, gpu, storage y other
    /// (texto, o { "es", "en" }). Pasan tal cual a la interfaz.
    #[serde(default)]
    pub requirements: Option<serde_json::Value>,
    /// Dónde guarda las partidas (para la nube): rutas relativas a la carpeta del port o con %APPDATA%,
    /// %LOCALAPPDATA%, %USERPROFILE% o %DOCUMENTS%. Vacío = se buscan carpetas "save(s)" dentro del port.
    #[serde(default)]
    pub saves: Vec<String>,
    #[serde(default)]
    pub experimental: bool,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub cover: Option<String>,
    /// Fecha (AAAA-MM-DD) en que entró al catálogo: "Añadidos recientemente".
    #[serde(default)]
    pub added_at: Option<String>,
    /// Descargas totales de sus releases en GitHub: "Más descargados".
    #[serde(default)]
    pub downloads: Option<u64>,
    /// Qué datos de juego acepta el port ("iso", "rvz", "z64", "carpeta"…). Si falta, los de su consola.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Aclaración sobre los datos (versión o región exigida, archivo concreto…).
    #[serde(default)]
    pub needs_note: Option<String>,
}

/// Preparación del juego (rutas relativas a la carpeta del ejecutable del port).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Setup {
    pub exe: String,
    /// Basta con que exista uno (p. ej. el juego compilado, o el archivo que guarda otra carpeta de instalación).
    pub done: Vec<String>,
}

/// Dónde se publica la build del port.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Releases de GitHub: Dársena la descarga directamente del repo oficial.
    #[default]
    Github,
    /// No está en GitHub: solo se puede instalar con una fuente que traiga el port (torrent o mirror).
    External,
}

/// Datos que suele aceptar un port según la consola original.
/// "carpeta" = el disco o juego extraído en una carpeta.
pub fn default_needs(console: &str) -> &'static [&'static str] {
    match console {
        "Nintendo 64" => &["z64", "n64", "v64"],
        "NES" => &["nes"],
        "Super Nintendo" => &["sfc", "smc"],
        "Game Boy" => &["gb", "gbc"],
        "Game Boy Color" => &["gbc"],
        "Game Boy Advance" => &["gba"],
        "Nintendo DS" => &["nds"],
        "Nintendo 3DS" => &["3ds", "cia", "cci"],
        "Virtual Boy" => &["vb"],
        "GameCube" => &["iso", "rvz", "gcm", "ciso"],
        "Wii" => &["iso", "rvz", "wbfs"],
        "Wii U" => &["wud", "wux", "carpeta"],
        "Switch" => &["nsp", "xci"],
        "PlayStation" => &["bin/cue", "chd", "iso"],
        "PlayStation 2" => &["iso", "chd"],
        "PSP" => &["iso", "cso", "carpeta"],
        "PlayStation 3" => &["carpeta", "iso"],
        "PlayStation 4" => &["pkg", "carpeta"],
        "Xbox" => &["iso", "carpeta"],
        "Xbox 360" => &["iso", "carpeta"],
        "Sega" => &["bin/cue", "chd", "gdi", "md"],
        "Arcade" => &["zip"],
        "PC" => &["carpeta"],
        _ => &[],
    }
}

/// ¿Unos datos que contienen `contains` sirven para un port que acepta `needs`?
pub fn data_compatible(needs: &[String], contains: &str) -> bool {
    let c = contains.trim().trim_start_matches('.').to_lowercase();
    needs.iter().any(|n| {
        let n = n.to_lowercase();
        n == c || n.split('/').any(|part| part == c) || c.split('/').any(|part| part == n)
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    #[serde(default)]
    generated_at: String,
    games: Vec<CatalogGame>,
}

pub struct Catalog {
    pub games: Vec<CatalogGame>,
    pub generated_at: String,
    /// Hash del texto: si el remoto tiene el mismo, no hay nada que actualizar.
    hash: String,
}

/// Resultado de comprobar el catálogo remoto, para la ventana de arranque.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogUpdate {
    pub updated: bool,
    /// Títulos de los ports nuevos respecto al catálogo anterior.
    pub added: Vec<String>,
    pub ports: usize,
    pub games: usize,
}

static CURRENT: RwLock<Option<&'static Catalog>> = RwLock::new(None);

pub fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn slug(s: &str) -> String {
    let n: String = s
        .split('·')
        .next()
        .unwrap_or(s)
        .chars()
        .map(|c| {
            let c = normalize(&c.to_string()).chars().next().unwrap_or(' ');
            if c.is_ascii_alphanumeric() { c } else { '-' }
        })
        .collect();
    n.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-")
}

/// Parsea y normaliza un catálogo. Las entradas incompletas se descartan en vez de romper todo
/// (el catálogo remoto se puede editar a mano desde la web de GitHub).
fn parse(text: &str) -> anyhow::Result<Catalog> {
    let file: CatalogFile = serde_json::from_str(text).context(tr!("el catálogo no es JSON válido", "the catalog isn't valid JSON"))?;
    let mut seen = HashSet::new();
    let games: Vec<CatalogGame> = file
        .games
        .into_iter()
        .filter_map(|mut g| {
            if g.title.trim().is_empty() || g.console.trim().is_empty() {
                return None;
            }
            match g.origin {
                Origin::Github => {
                    let (author, port) = g.repo.split_once('/')?;
                    if g.id.is_empty() {
                        g.id = slug(&g.repo).to_lowercase();
                    }
                    if g.author.is_empty() {
                        g.author = author.to_string();
                    }
                    if g.port.is_empty() {
                        g.port = port.to_string();
                    }
                }
                Origin::External => {
                    // Sin repo: el port necesita al menos un nombre de proyecto.
                    if g.port.trim().is_empty() {
                        return None;
                    }
                    if g.id.is_empty() {
                        g.id = slug(&format!("{} {}", g.author, g.port)).to_lowercase();
                    }
                    if g.author.trim().is_empty() {
                        g.author = "autor desconocido".into();
                    }
                }
            }
            if g.game.is_empty() {
                g.game = slug(&g.title);
            }
            if g.platforms.is_empty() {
                g.platforms = vec!["Windows".into()];
            }
            if g.needs.is_empty() {
                g.needs = default_needs(&g.console).iter().map(|s| s.to_string()).collect();
            }
            seen.insert(g.id.clone()).then_some(g)
        })
        .collect();
    anyhow::ensure!(!games.is_empty(), tr!("el catálogo no tiene ningún juego válido", "the catalog has no valid games"));
    Ok(Catalog { games, generated_at: file.generated_at, hash: hash(text) })
}

fn set(c: Catalog) -> &'static Catalog {
    let leaked: &'static Catalog = Box::leak(Box::new(c));
    *CURRENT.write().unwrap() = Some(leaked);
    leaked
}

fn current() -> &'static Catalog {
    if let Some(c) = *CURRENT.read().unwrap() {
        return c;
    }
    set(parse(include_str!("../../catalog/catalog.json")).expect("catalog.json embebido inválido"))
}

/// Al arrancar: usa el último catálogo descargado si no es más antiguo que el embebido.
pub fn init(data_dir: &Path) {
    let embedded = current();
    let Ok(text) = std::fs::read_to_string(data_dir.join(CACHE_FILE)) else { return };
    match parse(&text) {
        Ok(cached) if cached.generated_at >= embedded.generated_at => {
            set(cached);
        }
        _ => {}
    }
}

/// Descarga el catálogo de GitHub; si cambió, lo valida, lo guarda y pasa a ser el actual.
pub async fn check_update(data_dir: &Path) -> anyhow::Result<CatalogUpdate> {
    // El parámetro evita que una caché intermedia devuelva una versión vieja.
    let url = format!("{REMOTE_URL}?t={}", crate::store::now());
    let text = crate::http_client()
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?
        .error_for_status()
        .context(tr!("GitHub no devolvió el catálogo", "GitHub didn't return the catalog"))?
        .text()
        .await?;

    let old = current();
    let summary = |c: &Catalog, updated: bool, added: Vec<String>| CatalogUpdate {
        updated,
        added,
        ports: c.games.len(),
        games: c.games.iter().map(|g| g.game.as_str()).collect::<HashSet<_>>().len(),
    };
    if hash(&text) == old.hash {
        return Ok(summary(old, false, Vec::new()));
    }

    let new = parse(&text)?;
    let old_ids: HashSet<&str> = old.games.iter().map(|g| g.id.as_str()).collect();
    let added = new
        .games
        .iter()
        .filter(|g| !old_ids.contains(g.id.as_str()))
        .map(|g| g.title.clone())
        .collect();
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(data_dir.join(CACHE_FILE), &text)?;
    let new = set(new);
    Ok(summary(new, true, added))
}

pub fn all() -> &'static [CatalogGame] {
    &current().games
}

pub fn get(id: &str) -> Option<&'static CatalogGame> {
    all().iter().find(|g| g.id == id)
}

/// Título de un juego: el más corto de sus ports ("Diablo" antes que "Diablo (1996)"), como en la interfaz.
pub fn game_title(game_id: &str) -> String {
    all()
        .iter()
        .filter(|p| p.game == game_id)
        .map(|p| p.title.as_str())
        .min_by_key(|t| t.chars().count())
        .unwrap_or(game_id)
        .to_string()
}

/// Normaliza títulos para emparejar enlaces de fuentes con juegos del catálogo.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter_map(|c| {
            let c = match c {
                'á' | 'à' | 'ä' | 'â' | 'Á' => 'a',
                'é' | 'è' | 'ë' | 'ê' | 'É' => 'e',
                'í' | 'ì' | 'ï' | 'î' | 'Í' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' | 'Ó' => 'o',
                'ú' | 'ù' | 'ü' | 'û' | 'Ú' => 'u',
                'ñ' | 'Ñ' => 'n',
                c => c,
            };
            c.is_alphanumeric().then(|| c.to_ascii_lowercase())
        })
        .collect()
}

/// Busca el juego del catálogo al que corresponde un enlace de fuente.
pub fn match_game(game_id: Option<&str>, title: Option<&str>) -> Option<&'static CatalogGame> {
    if let Some(g) = game_id.and_then(get) {
        return Some(g);
    }
    let t = normalize(title?);
    if t.is_empty() {
        return None;
    }
    all().iter().find(|g| {
        normalize(&g.title) == t
            || normalize(&g.port) == t
            || g.aliases.iter().any(|a| normalize(a) == t)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carga_y_empareja() {
        assert!(all().len() > 400);
        assert_eq!(match_game(None, Some("Star Fox 64")).unwrap().id, "harbourmasters-starship");
        assert_eq!(match_game(Some("harbourmasters-shipwright"), None).unwrap().port, "Shipwright");
        assert!(match_game(None, Some("Juego inexistente")).is_none());
    }

    #[test]
    fn compatibilidad_de_datos() {
        let needs: Vec<String> = default_needs("PlayStation").iter().map(|s| s.to_string()).collect();
        assert!(data_compatible(&needs, "chd"));
        assert!(data_compatible(&needs, ".bin"));
        assert!(data_compatible(&needs, "bin/cue"));
        assert!(!data_compatible(&needs, "rvz"));
        assert_eq!(get("harbourmasters-starship").unwrap().needs, vec!["z64", "n64", "v64"]);
    }

    #[test]
    fn entrada_minima_editada_a_mano() {
        let c = parse(r#"{"games":[{"title":"Juego Nuevo","repo":"Autor/JuegoNuevoRecomp","console":"Nintendo 64"},{"title":"Sin repo"}]}"#).unwrap();
        assert_eq!(c.games.len(), 1);
        let g = &c.games[0];
        assert_eq!((g.id.as_str(), g.author.as_str(), g.port.as_str(), g.game.as_str()),
                   ("autor-juegonuevorecomp", "Autor", "JuegoNuevoRecomp", "juego-nuevo"));
        assert_eq!(g.origin, Origin::Github);
    }

    #[test]
    fn port_externo_sin_repo() {
        let c = parse(r#"{"games":[
            {"title":"Juego X","port":"Juego X PC","author":"Alguien","origin":"external","console":"Xbox 360","homepage":"https://www.moddb.com/x"},
            {"title":"Sin nombre de port","origin":"external","console":"Xbox 360"}]}"#).unwrap();
        assert_eq!(c.games.len(), 1);
        let g = &c.games[0];
        assert_eq!((g.id.as_str(), g.origin), ("alguien-juego-x-pc", Origin::External));
    }
}
