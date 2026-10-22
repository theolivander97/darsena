//! Ports subidos por sus autores y fichas personalizadas por el autor.
//!
//! - Solo el dueño o un colaborador de un repositorio puede subirlo. Dársena lo comprueba con su cuenta de GitHub.
//! - Cada port subido es una discusión en la categoría "Ports subidos" del repo darsena-launcher, con los datos
//!   en un bloque invisible <!-- darsena:submission {json} -->. El autor de Dársena la revisa
//!   (tools/catalog/envios.mjs): si la aprueba, el port entra en el catálogo y la discusión se cierra como resuelta;
//!   si no, se cierra con un comentario que explica por qué.
//! - La ficha de un port del catálogo puede llevar información de su autor: un `darsena.json` en la raíz de su
//!   repositorio (descripción, capturas, tráilers, web). Dársena lo lee y lo muestra, saneado y con límites.

use crate::reviews::gql;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OWNER: &str = "theolivander97";
const REPO: &str = "darsena-launcher";
pub const CATEGORY: &str = "Ports subidos";
const MARK: &str = "darsena:submission";

fn token() -> anyhow::Result<String> {
    crate::auth::token().context(tr!("inicia sesión con GitHub para subir un port", "sign in with GitHub to upload a port"))
}

async fn login() -> anyhow::Result<String> {
    Ok(crate::auth::user().await?.context(tr!("inicia sesión con GitHub para subir un port", "sign in with GitHub to upload a port"))?.login)
}

pub(crate) async fn rest(token: &str, path: &str) -> anyhow::Result<Value> {
    let res = crate::http_client()
        .get(format!("https://api.github.com/{path}"))
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(tr!("no existe en GitHub: {}", "not found on GitHub: {}", path));
    }
    Ok(res.error_for_status()?.json().await?)
}

fn in_catalog(repo: &str) -> bool {
    crate::catalog::all().iter().any(|g| g.repo.eq_ignore_ascii_case(repo))
}

// ---------- Tus repositorios ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoItem {
    pub full_name: String,
    pub description: Option<String>,
    pub stars: u64,
    pub in_catalog: bool,
}

/// Repositorios públicos tuyos y de tus organizaciones (públicas), del más reciente al más antiguo.
pub async fn my_repos() -> anyhow::Result<Vec<RepoItem>> {
    let token = token()?;
    let login = login().await?;
    let mut lists = vec![rest(&token, &format!("users/{login}/repos?type=owner&sort=pushed&per_page=100")).await?];
    if let Ok(orgs) = rest(&token, &format!("users/{login}/orgs")).await {
        for org in orgs.as_array().into_iter().flatten().filter_map(|o| o["login"].as_str()).take(10) {
            if let Ok(list) = rest(&token, &format!("orgs/{org}/repos?sort=pushed&per_page=50")).await {
                lists.push(list);
            }
        }
    }
    Ok(lists
        .iter()
        .flat_map(|l| l.as_array().cloned().unwrap_or_default())
        .filter(|r| !r["fork"].as_bool().unwrap_or(false) && !r["archived"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let full = r["full_name"].as_str()?.to_string();
            Some(RepoItem {
                in_catalog: in_catalog(&full),
                description: r["description"].as_str().map(str::to_string),
                stars: r["stargazers_count"].as_u64().unwrap_or(0),
                full_name: full,
            })
        })
        .collect())
}

// ---------- Comprobación de un repositorio ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppInRepo {
    pub branch: String,
    pub exe: String,
    #[serde(default)]
    pub setup_exe: Option<String>,
}

/// Ejecutables de Windows en la raíz del repositorio: el de jugar y el de instalar (por su nombre).
async fn app_in_repo(token: &str, repo: &str, branch: &str) -> Option<AppInRepo> {
    let list = rest(token, &format!("repos/{repo}/contents/?ref={branch}")).await.ok()?;
    let files: Vec<String> = list
        .as_array()?
        .iter()
        .filter(|f| f["type"] == "file")
        .filter_map(|f| f["name"].as_str().map(str::to_string))
        .filter(|n| [".exe", ".bat", ".cmd"].iter().any(|e| n.to_ascii_lowercase().ends_with(e)))
        .collect();
    let has = |words: &[&str], n: &str| words.iter().any(|w| n.to_ascii_lowercase().contains(w));
    let setup = files.iter().find(|n| has(&["install", "setup", "build"], n)).cloned();
    let exe = files
        .iter()
        .find(|n| has(&["play", "launch", "run", "start"], n))
        .or_else(|| files.iter().find(|n| Some(*n) != setup.as_ref()))
        .cloned()?;
    Some(AppInRepo { branch: branch.to_string(), exe, setup_exe: setup })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCheck {
    pub full_name: String,
    pub description: Option<String>,
    pub html_url: String,
    pub stars: u64,
    /// Eres su dueño o has contribuido a él.
    pub author_ok: bool,
    pub in_catalog: bool,
    /// Última versión con una build de Windows: (versión, archivo).
    pub build: Option<(String, String)>,
    /// Sin build en sus releases: la aplicación está en el propio repositorio (rama, ejecutable para jugar y,
    /// si lo hay, el que prepara el juego la primera vez). Dársena instala entonces el código.
    pub app_in_repo: Option<AppInRepo>,
    /// Ya tiene un darsena.json válido en su raíz.
    pub has_darsena_json: bool,
    pub problems: Vec<String>,
}

/// Normaliza "https://github.com/a/b", "github.com/a/b.git" o "a/b" a "a/b".
pub fn normalize_repo(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/').trim_end_matches(".git");
    let s = s.split("github.com/").last().unwrap_or(s);
    let mut parts = s.split('/').filter(|p| !p.is_empty());
    let (owner, name) = (parts.next()?, parts.next()?);
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    (ok(owner) && ok(name)).then(|| format!("{owner}/{name}"))
}

pub async fn check_repo(input: &str) -> anyhow::Result<RepoCheck> {
    let token = token()?;
    let login = login().await?;
    let repo = normalize_repo(input).context(tr!("escribe el repositorio como usuario/repositorio", "write the repository as user/repository"))?;
    let info = rest(&token, &format!("repos/{repo}")).await?;
    let full = info["full_name"].as_str().unwrap_or(&repo).to_string();
    let owner = info["owner"]["login"].as_str().unwrap_or_default();
    let mut author_ok = owner.eq_ignore_ascii_case(&login);
    if !author_ok {
        if let Ok(list) = rest(&token, &format!("repos/{full}/contributors?per_page=100")).await {
            author_ok = list.as_array().into_iter().flatten().any(|c| c["login"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(&login)));
        }
    }
    let releases = rest(&token, &format!("repos/{full}/releases?per_page=10")).await.unwrap_or(Value::Null);
    let build = releases.as_array().into_iter().flatten().filter(|r| !r["draft"].as_bool().unwrap_or(false)).find_map(|r| {
        let asset = r["assets"]
            .as_array()?
            .iter()
            .filter_map(|a| a["name"].as_str())
            .filter_map(|n| crate::github::score(n, None).map(|s| (s, n)))
            .max_by_key(|(s, _)| *s)?;
        Some((r["tag_name"].as_str().unwrap_or_default().to_string(), asset.1.to_string()))
    });
    let app_in_repo = match build {
        Some(_) => None,
        None => app_in_repo(&token, &full, info["default_branch"].as_str().unwrap_or("main")).await,
    };
    let has_darsena_json = author_file(&full).await.is_some();
    let in_cat = in_catalog(&full);
    let mut problems = Vec::new();
    if !author_ok {
        problems.push(tr!(
            "No eres el dueño ni colaborador de este repositorio: solo sus autores pueden subirlo.",
            "You're not the owner or a contributor of this repository: only its authors can upload it."
        ));
    }
    if in_cat {
        problems.push(tr!("Este port ya está en el catálogo de Dársena.", "This port is already in Dársena's catalog."));
    }
    if build.is_none() && app_in_repo.is_none() {
        problems.push(tr!(
            "No hay nada que instalar: ni una release con una build de Windows (.zip o .exe) ni un ejecutable (.exe o .bat) en la raíz del repositorio.",
            "There's nothing to install: neither a release with a Windows build (.zip or .exe) nor an executable (.exe or .bat) in the repository root."
        ));
    }
    if info["archived"].as_bool().unwrap_or(false) {
        problems.push(tr!("El repositorio está archivado.", "The repository is archived."));
    }
    Ok(RepoCheck {
        full_name: full,
        description: info["description"].as_str().map(str::to_string),
        html_url: info["html_url"].as_str().unwrap_or_default().to_string(),
        stars: info["stargazers_count"].as_u64().unwrap_or(0),
        author_ok,
        in_catalog: in_cat,
        build,
        app_in_repo,
        has_darsena_json,
        problems,
    })
}

// ---------- Enviar ----------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub repo: String,
    pub title: String,
    pub console: String,
    #[serde(default)]
    pub franchise: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub needs_note: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub experimental: bool,
    /// Uso de IA: "none", "assisted" o "generated".
    #[serde(default)]
    pub ai: Option<String>,
    #[serde(default)]
    pub synopsis: Option<String>,
    #[serde(default)]
    pub trailer: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Lo rellena Dársena al comprobar el repositorio (no el autor).
    #[serde(default)]
    pub app_in_repo: Option<AppInRepo>,
}

async fn ids(token: &str) -> anyhow::Result<(String, String)> {
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
        .with_context(|| tr!("la subida de ports aún no está abierta (falta la categoría \"{CATEGORY}\")", "port uploads aren't open yet (the \"{CATEGORY}\" category is missing)"))?
        .to_string();
    Ok((repo_id, cat))
}

/// Discusiones de envíos (las 100 más recientes).
async fn submissions(token: &str) -> anyhow::Result<Vec<Value>> {
    let (_, cat) = ids(token).await?;
    let d = gql(
        token,
        "query($o:String!,$r:String!,$c:ID!){repository(owner:$o,name:$r){discussions(first:100,categoryId:$c,orderBy:{field:CREATED_AT,direction:DESC}){nodes{title url closed stateReason createdAt body author{login} comments(last:1){nodes{body author{login}}}}}}}",
        json!({ "o": OWNER, "r": REPO, "c": cat }),
    )
    .await?;
    Ok(d["repository"]["discussions"]["nodes"].as_array().cloned().unwrap_or_default())
}

fn submission_of(body: &str) -> Option<Submission> {
    let start = body.find(&format!("<!-- {MARK}"))? + MARK.len() + 5;
    let end = body[start..].find("-->")? + start;
    serde_json::from_str(body[start..end].trim()).ok()
}

pub async fn submit(mut s: Submission) -> anyhow::Result<String> {
    let token = token()?;
    let check = check_repo(&s.repo).await?;
    if let Some(p) = check.problems.first() {
        anyhow::bail!("{p}");
    }
    s.repo = check.full_name.clone();
    s.app_in_repo = check.app_in_repo.clone();
    s.title = s.title.trim().to_string();
    anyhow::ensure!(!s.title.is_empty(), tr!("falta el nombre del juego", "the game's name is missing"));
    anyhow::ensure!(!s.console.trim().is_empty(), tr!("falta la consola original", "the original console is missing"));
    let clip = |o: &mut Option<String>, n: usize| {
        *o = o.take().map(|t| t.trim().chars().take(n).collect::<String>()).filter(|t| !t.is_empty());
    };
    clip(&mut s.synopsis, 2000);
    clip(&mut s.needs_note, 300);
    clip(&mut s.notes, 2000);
    clip(&mut s.trailer, 200);
    s.ai = s.ai.filter(|a| ["none", "assisted", "generated"].contains(&a.as_str()));
    // ¿Ya hay un envío pendiente de este repo?
    for d in submissions(&token).await? {
        let same = submission_of(d["body"].as_str().unwrap_or_default()).is_some_and(|x| x.repo.eq_ignore_ascii_case(&s.repo));
        if same && !d["closed"].as_bool().unwrap_or(false) {
            anyhow::bail!(tr!("este port ya está subido y pendiente de revisión: {}", "this port is already uploaded and awaiting review: {}", d["url"].as_str().unwrap_or_default()));
        }
    }
    let (repo_id, cat) = ids(&token).await?;
    let me = login().await?;
    let list = |v: &[String]| if v.is_empty() { "—".to_string() } else { v.join(", ") };
    let body = format!(
        "**{title}** · {console}\n\n\
         - Repositorio: https://github.com/{repo}\n- Subido por: @{me}\n- Última build: {build}\n- Datos que pide: {needs}\n\
         - Plataformas: {platforms}\n- Experimental: {exp}\n\n{synopsis}{notes}\
         <!-- {MARK}\n{json}\n-->",
        title = s.title,
        console = s.console,
        repo = s.repo,
        build = check.build.as_ref().map(|(v, a)| format!("{v} ({a})")).or_else(|| check.app_in_repo.as_ref().map(|a| format!(
            "sin build en releases: se instala el código de `{}` y se juega con `{}`{}",
            a.branch,
            a.exe,
            a.setup_exe.as_ref().map(|s| format!(" (prepara con `{s}`)")).unwrap_or_default()
        ))).unwrap_or_default(),
        needs = list(&s.needs),
        platforms = list(&s.platforms),
        exp = if s.experimental { "sí" } else { "no" },
        synopsis = s.synopsis.as_ref().map(|t| format!("### Sinopsis\n\n{t}\n\n")).unwrap_or_default(),
        notes = s.notes.as_ref().map(|t| format!("### Notas para el revisor\n\n{t}\n\n")).unwrap_or_default(),
        json = serde_json::to_string_pretty(&s)?.replace("-->", "--\\u003e"),
    );
    let d = gql(
        &token,
        "mutation($r:ID!,$c:ID!,$t:String!,$b:String!){createDiscussion(input:{repositoryId:$r,categoryId:$c,title:$t,body:$b}){discussion{url}}}",
        json!({ "r": repo_id, "c": cat, "t": format!("{} — {}", s.title, s.repo), "b": body }),
    )
    .await?;
    Ok(d["createDiscussion"]["discussion"]["url"].as_str().unwrap_or_default().to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MySubmission {
    pub title: String,
    pub repo: String,
    pub url: String,
    /// "pending", "approved" o "rejected".
    pub status: &'static str,
    /// Último comentario del revisor (el motivo si se rechazó).
    pub reply: Option<String>,
    pub created_at: String,
}

pub async fn mine() -> anyhow::Result<Vec<MySubmission>> {
    let token = token()?;
    let me = login().await?;
    Ok(submissions(&token)
        .await?
        .into_iter()
        .filter(|d| d["author"]["login"].as_str().is_some_and(|l| l.eq_ignore_ascii_case(&me)))
        .filter_map(|d| {
            let sub = submission_of(d["body"].as_str()?)?;
            let closed = d["closed"].as_bool().unwrap_or(false);
            let status = if !closed { "pending" } else if d["stateReason"].as_str() == Some("RESOLVED") { "approved" } else { "rejected" };
            let last = &d["comments"]["nodes"][0];
            let reply = last["author"]["login"].as_str().filter(|l| l.eq_ignore_ascii_case(OWNER)).and(last["body"].as_str()).map(str::to_string);
            Some(MySubmission {
                title: sub.title,
                repo: sub.repo,
                url: d["url"].as_str().unwrap_or_default().to_string(),
                status,
                reply,
                created_at: d["createdAt"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// Rama principal de un repositorio (para abrir el editor de GitHub en el sitio correcto).
pub async fn default_branch(repo: &str) -> anyhow::Result<String> {
    let token = token()?;
    let info = rest(&token, &format!("repos/{repo}")).await?;
    Ok(info["default_branch"].as_str().unwrap_or("main").to_string())
}

// ---------- darsena.json: la ficha que escribe el autor ----------

static AUTHOR_CACHE: Mutex<Option<HashMap<String, (Instant, Option<Value>)>>> = Mutex::new(None);
const AUTHOR_TTL: Duration = Duration::from_secs(30 * 60);

fn https(v: &Value) -> Option<String> {
    v.as_str().filter(|u| u.starts_with("https://") && u.len() < 500 && !u.contains(['"', '<', '>', ' '])).map(str::to_string)
}

/// Id de YouTube a partir del id o de una URL (watch?v=, youtu.be/, /embed/, /shorts/).
pub fn youtube_id(v: &str) -> Option<String> {
    let v = v.trim();
    let id = if v.len() == 11 { v } else {
        let after = ["v=", "youtu.be/", "/embed/", "/shorts/"].iter().find_map(|k| v.split(k).nth(1))?;
        after.get(..11)?
    };
    id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_').then(|| id.to_string())
}

/// Texto en un idioma: "texto" o { "es": "...", "en": "..." }.
fn text(v: &Value, max: usize) -> Option<Value> {
    let clip = |s: &str| s.trim().chars().take(max).collect::<String>();
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(json!({ "es": clip(s), "en": clip(s) })),
        Value::Object(m) => {
            let es = m.get("es").and_then(Value::as_str).map(clip);
            let en = m.get("en").and_then(Value::as_str).map(clip);
            (es.is_some() || en.is_some()).then(|| json!({ "es": es.clone().or(en.clone()), "en": en.or(es) }))
        }
        _ => None,
    }
}

const REQ_KEYS: [&str; 6] = ["os", "cpu", "ram", "gpu", "storage", "other"];

/// Solo los campos conocidos, con límites: lo que el autor no debería poder colar, no pasa.
fn sanitize(raw: &Value) -> Option<Value> {
    let mut out = serde_json::Map::new();
    if let Some(t) = raw.get("description").and_then(|v| text(v, 3000)) {
        out.insert("description".into(), t);
    }
    if let Some(t) = raw.get("needsNote").and_then(|v| text(v, 300)) {
        out.insert("needsNote".into(), t);
    }
    let shots: Vec<String> = raw.get("screenshots").and_then(Value::as_array).into_iter().flatten().filter_map(https).take(8).collect();
    if !shots.is_empty() {
        out.insert("screenshots".into(), json!(shots));
    }
    let trailers: Vec<String> = raw
        .get("trailers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().and_then(youtube_id))
        .take(3)
        .collect();
    if !trailers.is_empty() {
        out.insert("trailers".into(), json!(trailers));
    }
    if let Some(w) = raw.get("website").and_then(https) {
        out.insert("website".into(), json!(w));
    }
    // Enlaces para apoyar al autor (Ko-fi, Patreon, GitHub Sponsors, PayPal…).
    let support: Vec<String> = raw.get("support").and_then(Value::as_array).into_iter().flatten().filter_map(https).take(4).collect();
    if !support.is_empty() {
        out.insert("support".into(), json!(support));
    }
    // Ficha decorada: imagen de fondo grande, logo del juego y sinopsis propia.
    for key in ["background", "logo"] {
        if let Some(u) = raw.get(key).and_then(https) {
            out.insert(key.into(), json!(u));
        }
    }
    if let Some(t) = raw.get("synopsis").and_then(|v| text(v, 2000)) {
        out.insert("synopsis".into(), t);
    }
    // Requisitos del sistema: solo los apartados y claves conocidos, texto corto.
    let mut reqs = serde_json::Map::new();
    for level in ["minimum", "recommended"] {
        let set: serde_json::Map<String, Value> = REQ_KEYS
            .iter()
            .filter_map(|k| raw.get("requirements")?.get(level)?.get(*k).and_then(|v| text(v, 200)).map(|t| (k.to_string(), t)))
            .collect();
        if !set.is_empty() {
            reqs.insert(level.into(), Value::Object(set));
        }
    }
    if !reqs.is_empty() {
        out.insert("requirements".into(), Value::Object(reqs));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// darsena.json de un repositorio (rama por defecto), saneado. None si no tiene o no es válido.
async fn author_file(repo: &str) -> Option<Value> {
    if let Some((at, v)) = AUTHOR_CACHE.lock().unwrap().as_ref().and_then(|c| c.get(repo).cloned()) {
        if at.elapsed() < AUTHOR_TTL {
            return v;
        }
    }
    let res = crate::http_client()
        .get(format!("https://raw.githubusercontent.com/{repo}/HEAD/darsena.json"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    let value = if res.status().is_success() {
        let text = res.text().await.ok()?;
        (text.len() < 64_000).then(|| serde_json::from_str::<Value>(&text).ok()).flatten().and_then(|v| sanitize(&v))
    } else {
        None
    };
    AUTHOR_CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(repo.to_string(), (Instant::now(), value.clone()));
    value
}

/// Información del autor para la ficha de un port del catálogo (si no está bloqueada en info.json).
pub async fn author_info(port_id: &str) -> Option<Value> {
    let port = crate::catalog::get(port_id)?;
    if port.origin == crate::catalog::Origin::External || port.repo.is_empty() {
        return None;
    }
    let blocked = crate::info::get()["blockedAuthorInfo"].as_array().is_some_and(|b| b.iter().any(|x| x.as_str() == Some(port_id)));
    if blocked {
        return None;
    }
    author_file(&port.repo).await
}
