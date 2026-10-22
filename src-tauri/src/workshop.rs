//! Taller: mods que sube la comunidad para los juegos del catálogo (como el Workshop de Steam).
//!
//! - Cada mod es un repositorio de GitHub con releases. Solo su dueño o un colaborador puede subirlo.
//! - Subirlo crea una discusión en la categoría "Taller" del repo darsena-launcher, con los datos en un bloque
//!   invisible <!-- darsena:mod {json} -->. No hay revisión previa: aparece en cuanto la Action lo recoge.
//! - La Action (.github/scripts/mods.mjs) escribe mods.json, el resumen público: «me gusta» (reacciones),
//!   denuncias (comentarios <!-- darsena:report reason=… -->) y los ocultos por denuncias o cerrados por el autor
//!   de Dársena. Un mod +18 lo marca su autor o dos denuncias de «contenido +18».
//! - Instalar un mod extrae su última release en la carpeta del port y apunta qué archivos puso, para quitarlo
//!   después sin tocar nada más.

use crate::reviews::gql;
use crate::submit::{normalize_repo, rest};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OWNER: &str = "theolivander97";
const REPO: &str = "darsena-launcher";
pub const CATEGORY: &str = "Taller";
const MARK: &str = "darsena:mod";
const PUBLIC_URL: &str = "https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/mods.json";
pub const REPORT_REASONS: [&str; 4] = ["nsfw", "spam", "malware", "other"];

fn token() -> anyhow::Result<String> {
    crate::auth::token().context(tr!("inicia sesión con GitHub para usar el Taller", "sign in with GitHub to use the Workshop"))
}

/// Lo que escribe el autor al subir el mod.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModMeta {
    pub repo: String,
    /// Juego del catálogo (id agrupado, p. ej. "gears-of-war-2").
    pub game: String,
    /// Port concreto para el que funciona; None = cualquiera del juego.
    #[serde(default)]
    pub port: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Contenido para adultos.
    #[serde(default)]
    pub nsfw: bool,
    /// Subcarpeta del port donde se extrae (p. ej. "mods"); vacía = la carpeta del port.
    #[serde(default)]
    pub install_dir: String,
}

/// Un mod tal como lo publica mods.json.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PublicMod {
    /// Número de la discusión: el id del mod.
    pub id: String,
    /// Id GraphQL de la discusión (para «me gusta» y denuncias).
    pub node: String,
    pub url: String,
    pub author: String,
    #[serde(default)]
    pub avatar: String,
    pub created_at: String,
    #[serde(default)]
    pub likes: u32,
    #[serde(default)]
    pub reports: u32,
    /// Oculto por denuncias, a la espera de revisión.
    #[serde(default)]
    pub hidden: bool,
    #[serde(flatten)]
    pub meta: ModMeta,
}

static PUBLIC: Mutex<Option<(Instant, Vec<PublicMod>)>> = Mutex::new(None);

/// Todos los mods publicados (incluidos los ocultos y los +18: la interfaz decide). Se cachea 10 minutos.
pub async fn public(force: bool) -> Vec<PublicMod> {
    if !force {
        if let Some((at, list)) = PUBLIC.lock().unwrap().clone() {
            if at.elapsed() < Duration::from_secs(600) {
                return list;
            }
        }
    }
    let list: Vec<PublicMod> = async {
        let text = crate::http_client()
            .get(format!("{PUBLIC_URL}?t={}", crate::store::now()))
            .timeout(Duration::from_secs(15))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let v: Value = serde_json::from_str(&text)?;
        anyhow::Ok(serde_json::from_value(v["mods"].clone()).unwrap_or_default())
    }
    .await
    .unwrap_or_default();
    *PUBLIC.lock().unwrap() = Some((Instant::now(), list.clone()));
    list
}

/// Carpeta relativa segura: sin "..", sin raíz ni unidad.
fn safe_rel(dir: &str) -> anyhow::Result<PathBuf> {
    let p = Path::new(dir.trim().trim_matches(['/', '\\']));
    anyhow::ensure!(
        p.components().all(|c| matches!(c, Component::Normal(_))),
        tr!("la carpeta de instalación no es válida", "the install folder isn't valid")
    );
    Ok(p.to_path_buf())
}

// ---------- Subir ----------

async fn category_ids(token: &str) -> anyhow::Result<(String, String)> {
    let d = gql(
        token,
        "query($o:String!,$r:String!){repository(owner:$o,name:$r){id discussionCategories(first:25){nodes{id name}}}}",
        json!({ "o": OWNER, "r": REPO }),
    )
    .await?;
    let repo_id = d["repository"]["id"].as_str().context("repo")?.to_string();
    let cat = d["repository"]["discussionCategories"]["nodes"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(CATEGORY))))
        .and_then(|c| c["id"].as_str())
        .with_context(|| tr!("el Taller aún no está abierto", "the Workshop isn't open yet"))?
        .to_string();
    Ok((repo_id, cat))
}

pub async fn submit(mut m: ModMeta) -> anyhow::Result<String> {
    let token = token()?;
    let me = crate::auth::user().await?.context(tr!("inicia sesión con GitHub para usar el Taller", "sign in with GitHub to use the Workshop"))?.login;
    m.repo = normalize_repo(&m.repo).context(tr!("escribe el repositorio como usuario/repositorio", "write the repository as user/repository"))?;
    m.name = m.name.trim().chars().take(80).collect();
    m.description = m.description.trim().chars().take(3000).collect();
    m.install_dir = safe_rel(&m.install_dir)?.to_string_lossy().replace('\\', "/");
    anyhow::ensure!(!m.name.is_empty(), tr!("falta el nombre del mod", "the mod's name is missing"));
    anyhow::ensure!(crate::catalog::all().iter().any(|g| g.game == m.game), tr!("ese juego no está en el catálogo", "that game isn't in the catalog"));
    if let Some(p) = &m.port {
        anyhow::ensure!(crate::catalog::get(p).is_some_and(|g| g.game == m.game), tr!("ese port no es de este juego", "that port isn't for this game"));
    }
    // Solo su dueño o un colaborador, y con una release que instalar.
    let info = rest(&token, &format!("repos/{}", m.repo)).await?;
    m.repo = info["full_name"].as_str().unwrap_or(&m.repo).to_string();
    let owner = info["owner"]["login"].as_str().unwrap_or_default();
    let mut author_ok = owner.eq_ignore_ascii_case(&me);
    if !author_ok {
        if let Ok(list) = rest(&token, &format!("repos/{}/contributors?per_page=100", m.repo)).await {
            author_ok = list.as_array().into_iter().flatten().any(|c| c["login"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(&me)));
        }
    }
    anyhow::ensure!(
        author_ok,
        tr!("no eres el dueño ni colaborador de este repositorio: solo sus autores pueden subirlo", "you're not the owner or a contributor of this repository: only its authors can upload it")
    );
    latest_release(&m.repo, Some(&token)).await?;
    // ¿Ya está subido?
    if public(true).await.iter().any(|x| x.meta.repo.eq_ignore_ascii_case(&m.repo) && x.meta.game == m.game) {
        anyhow::bail!(tr!("este mod ya está en el Taller", "this mod is already in the Workshop"));
    }
    let (repo_id, cat) = category_ids(&token).await?;
    let game_title = crate::catalog::all().iter().find(|g| g.game == m.game).map(|g| g.title.clone()).unwrap_or_else(|| m.game.clone());
    let body = format!(
        "**{name}** · mod para {game}{nsfw}\n\n- Repositorio: https://github.com/{repo}\n- Subido por: @{me}\n\n{desc}\n\n<!-- {MARK}\n{json}\n-->",
        name = m.name,
        game = game_title,
        nsfw = if m.nsfw { " · **+18**" } else { "" },
        repo = m.repo,
        desc = m.description,
        json = serde_json::to_string_pretty(&m)?.replace("-->", "--\\u003e"),
    );
    let d = gql(
        &token,
        "mutation($r:ID!,$c:ID!,$t:String!,$b:String!){createDiscussion(input:{repositoryId:$r,categoryId:$c,title:$t,body:$b}){discussion{url}}}",
        json!({ "r": repo_id, "c": cat, "t": format!("{} — {} [{}]", m.name, game_title, m.game), "b": body }),
    )
    .await?;
    *PUBLIC.lock().unwrap() = None;
    Ok(d["createDiscussion"]["discussion"]["url"].as_str().unwrap_or_default().to_string())
}

// ---------- Me gusta y denuncias ----------

pub async fn like(node: &str) -> anyhow::Result<()> {
    let token = token()?;
    gql(&token, "mutation($s:ID!){addReaction(input:{subjectId:$s,content:THUMBS_UP}){reaction{content}}}", json!({ "s": node })).await?;
    Ok(())
}

pub async fn report(node: &str, reason: &str, text: &str) -> anyhow::Result<()> {
    let token = token()?;
    anyhow::ensure!(REPORT_REASONS.contains(&reason), "motivo no válido");
    let label = match reason {
        "nsfw" => "contenido +18 sin marcar",
        "spam" => "spam o engañoso",
        "malware" => "malware o archivos peligrosos",
        _ => "otro motivo",
    };
    let text: String = text.trim().chars().take(1000).collect();
    let body = format!("<!-- darsena:report reason={reason} -->\n**Denuncia:** {label}\n\n{text}");
    gql(
        &token,
        "mutation($d:ID!,$b:String!){addDiscussionComment(input:{discussionId:$d,body:$b}){comment{id}}}",
        json!({ "d": node, "b": body }),
    )
    .await?;
    Ok(())
}

// ---------- Instalar ----------

/// (versión, url del .zip, hay que quitar la carpeta raíz) de la última release del repo.
async fn latest_release(repo: &str, token: Option<&str>) -> anyhow::Result<(String, String, bool)> {
    let mut req = crate::http_client()
        .get(format!("https://api.github.com/repos/{repo}/releases?per_page=10"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let list: Vec<Value> = req.send().await.context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?.error_for_status()?.json().await?;
    let r = list
        .iter()
        .find(|r| !r["draft"].as_bool().unwrap_or(false))
        .context(tr!("el repositorio no tiene ninguna release: publica una con el mod", "the repository has no releases: publish one with the mod"))?;
    let version = r["tag_name"].as_str().unwrap_or_default().to_string();
    let zip = r["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str().is_some_and(|n| n.to_ascii_lowercase().ends_with(".zip")))
        .and_then(|a| a["browser_download_url"].as_str());
    Ok(match zip {
        Some(u) => (version, u.to_string(), false),
        // Sin .zip adjunto: el código de la release (GitHub lo mete en una carpeta "repo-version/").
        None => (version, r["zipball_url"].as_str().context("zipball")?.to_string(), true),
    })
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Archivos que puso, relativos a la carpeta del port.
    pub files: Vec<String>,
    pub installed_at: u64,
}

/// Descarga y extrae el mod en la carpeta del port. Devuelve lo que hay que apuntar en la biblioteca.
pub async fn install(m: &PublicMod, port_dir: &Path) -> anyhow::Result<InstalledMod> {
    let token = crate::auth::token();
    let (version, url, strip_root) = latest_release(&m.meta.repo, token.as_deref()).await?;
    let tmp = std::env::temp_dir().join(format!("darsena-mod-{}.zip", m.id));
    {
        use std::io::Write;
        let mut res = crate::http_client().get(&url).send().await?.error_for_status()?;
        let mut f = std::fs::File::create(&tmp)?;
        let mut total = 0u64;
        while let Some(chunk) = res.chunk().await? {
            total += chunk.len() as u64;
            anyhow::ensure!(total < 4 << 30, tr!("el mod es demasiado grande", "the mod is too big"));
            f.write_all(&chunk)?;
        }
    }
    let base = safe_rel(&m.meta.install_dir)?;
    let result = (|| -> anyhow::Result<Vec<String>> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(&tmp)?).context(tr!("el archivo del mod no es un .zip válido", "the mod file isn't a valid .zip"))?;
        let mut files = Vec::new();
        for i in 0..z.len() {
            let mut entry = z.by_index(i)?;
            let Some(name) = entry.enclosed_name() else { continue };
            let name: PathBuf = if strip_root { name.components().skip(1).collect() } else { name };
            if name.as_os_str().is_empty() {
                continue;
            }
            let rel = base.join(&name);
            let out = port_dir.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::io::copy(&mut entry, &mut std::fs::File::create(&out)?)?;
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
        Ok(files)
    })();
    let _ = std::fs::remove_file(&tmp);
    let files = result?;
    anyhow::ensure!(!files.is_empty(), tr!("el mod no trae ningún archivo", "the mod has no files"));
    Ok(InstalledMod { id: m.id.clone(), name: m.meta.name.clone(), version, files, installed_at: crate::store::now() })
}

/// Quita los archivos que puso el mod y las carpetas que queden vacías.
pub fn uninstall(m: &InstalledMod, port_dir: &Path) {
    let mut dirs = std::collections::BTreeSet::new();
    for f in &m.files {
        let Ok(rel) = safe_rel(f) else { continue };
        let path = port_dir.join(&rel);
        let _ = std::fs::remove_file(&path);
        let mut p = rel.parent();
        while let Some(d) = p.filter(|d| !d.as_os_str().is_empty()) {
            dirs.insert(d.to_path_buf());
            p = d.parent();
        }
    }
    // De la más profunda a la menos: solo se borran si quedaron vacías.
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(port_dir.join(d));
    }
}
