//! Descarga oficial: busca en las releases de GitHub del port el asset para esta plataforma.
//! Una sola petición por juego y caché en memoria (sin token, GitHub permite 60 peticiones/hora).

use crate::catalog::CatalogGame;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    /// Título y notas de la release tal cual las escribió el autor (Markdown).
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
    /// "sha256:…": la suma que GitHub calcula al subir el archivo.
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OfficialBuild {
    pub version: String,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub asset_name: String,
    pub size: u64,
    pub url: String,
    /// SHA-256 del archivo según GitHub (en hexadecimal), para comprobar la descarga.
    pub sha256: Option<String>,
    /// Título y notas de la release (Markdown) y su página en GitHub: para las Novedades de la biblioteca.
    pub name: Option<String>,
    pub notes: Option<String>,
    pub page: Option<String>,
}

static CACHE: Mutex<Option<HashMap<String, (Instant, OfficialBuild)>>> = Mutex::new(None);
const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

const REJECT: &[&str] = &[
    "linux", "macos", "osx", "darwin", "-mac", "_mac", ".mac", "android", ".apk", "appimage",
    "-ios", "_ios", "arm64", "aarch64", "source", "-src", "_src", "debug", "symbols", ".pdb", "flatpak",
    "switch", "vita", ".dmg", ".deb", ".rpm", ".tar", "sha256", ".sig", ".asc",
];

/// Puntúa un asset como build de Windows; None si no sirve.
pub(crate) fn score(name: &str, pattern: Option<&regex::Regex>) -> Option<i32> {
    let n = name.to_ascii_lowercase();
    let ext_ok = n.ends_with(".zip") || n.ends_with(".exe");
    if !ext_ok || REJECT.iter().any(|r| n.contains(r)) {
        return None;
    }
    if pattern.is_some_and(|p| p.is_match(name)) {
        return Some(1000);
    }
    let mut s = 0;
    if ["win", "windows", "win64", "win32"].iter().any(|w| n.contains(w)) {
        s += 10;
    }
    if ["x64", "x86_64", "amd64"].iter().any(|w| n.contains(w)) {
        s += 5;
    }
    if n.ends_with(".zip") {
        s += 3;
    }
    if n.contains("portable") {
        s += 2;
    }
    if n.contains("setup") || n.contains("installer") {
        s -= 5;
    }
    Some(s)
}

/// Última build de Windows de un proyecto de GitLab (sus releases enlazan los archivos como "links").
async fn gitlab_build(project: &str) -> anyhow::Result<OfficialBuild> {
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/releases?per_page=10",
        project.replace('/', "%2F")
    );
    let resp = crate::http_client().get(url).timeout(Duration::from_secs(20)).send().await
        .context(tr!("no se pudo contactar con GitLab", "couldn't reach GitLab"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(tr!("el proyecto {} ya no existe en GitLab", "the project {} no longer exists on GitLab", project));
    }
    let releases: Vec<serde_json::Value> = resp.error_for_status()?.json().await?;
    releases
        .iter()
        .filter(|r| !r["upcoming_release"].as_bool().unwrap_or(false))
        .find_map(|r| {
            let best = r["assets"]["links"]
                .as_array()?
                .iter()
                .filter_map(|l| {
                    let name = l["name"].as_str()?;
                    let url = l["direct_asset_url"].as_str().or(l["url"].as_str())?;
                    score(name, None).map(|s| (s, name, url))
                })
                .max_by_key(|(s, _, _)| *s)?;
            Some(OfficialBuild {
                version: r["tag_name"].as_str().unwrap_or_default().to_string(),
                prerelease: false,
                published_at: r["released_at"].as_str().map(str::to_string),
                asset_name: best.1.to_string(),
                size: 0,
                url: best.2.to_string(),
                sha256: None,
                name: r["name"].as_str().map(str::to_string).filter(|n| !n.trim().is_empty()),
                notes: r["description"].as_str().map(str::to_string).filter(|n| !n.trim().is_empty()),
                page: r["_links"]["self"].as_str().map(str::to_string),
            })
        })
        .with_context(|| tr!("las releases de {} no tienen una build para Windows", "{}'s releases have no Windows build", project))
}

/// El código de una rama como build: la versión es el último commit.
async fn branch_build(repo: &str, branch: &str, token: Option<&str>) -> anyhow::Result<OfficialBuild> {
    let mut req = crate::http_client()
        .get(format!("https://api.github.com/repos/{repo}/commits/{branch}"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20));
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(t.trim());
    }
    let resp = req.send().await.context(tr!("no se pudo contactar con GitHub", "couldn't reach GitHub"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(tr!("el repositorio {} ya no existe en GitHub", "the repository {} no longer exists on GitHub", repo));
    }
    let c: serde_json::Value = resp.error_for_status()?.json().await?;
    let sha = c["sha"].as_str().context("commit")?;
    let short = &sha[..sha.len().min(7)];
    let name = repo.rsplit('/').next().unwrap_or(repo);
    Ok(OfficialBuild {
        version: short.to_string(),
        prerelease: false,
        published_at: c["commit"]["committer"]["date"].as_str().map(str::to_string),
        asset_name: format!("{name}-{short}.zip"),
        size: 0,
        // Termina en .zip: así se guarda con esa extensión y Dársena lo extrae.
        url: format!("https://github.com/{repo}/archive/{sha}.zip"),
        sha256: None,
        name: None,
        notes: c["commit"]["message"].as_str().map(str::to_string),
        page: c["html_url"].as_str().map(str::to_string),
    })
}

pub async fn latest_build(game: &CatalogGame, token: Option<&str>) -> anyhow::Result<OfficialBuild> {
    if let Some(branch) = game.branch.as_deref() {
        let key = format!("{}@{branch}", game.repo);
        if let Some((at, b)) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key)) {
            if at.elapsed() < CACHE_TTL {
                return Ok(b.clone());
            }
        }
        let build = branch_build(&game.repo, branch, token).await?;
        CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (Instant::now(), build.clone()));
        return Ok(build);
    }
    if let Some(project) = game.gitlab.as_deref() {
        if let Some((at, b)) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(project)) {
            if at.elapsed() < CACHE_TTL {
                return Ok(b.clone());
            }
        }
        let build = gitlab_build(project).await?;
        CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(project.to_string(), (Instant::now(), build.clone()));
        return Ok(build);
    }
    if game.origin == crate::catalog::Origin::External {
        anyhow::bail!(tr!("este port no se publica en GitHub: se instala desde una fuente que lo incluya", "this port isn't published on GitHub: install it from a source that includes it"));
    }
    if let Some((at, b)) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&game.repo)) {
        if at.elapsed() < CACHE_TTL {
            return Ok(b.clone());
        }
    }

    let mut req = crate::http_client()
        .get(format!("https://api.github.com/repos/{}/releases?per_page=10", game.repo))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20));
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(t.trim());
    }
    let resp = req.send().await.context(tr!("no se pudo contactar con GitHub", "couldn't reach GitHub"))?;
    if resp.status() == reqwest::StatusCode::FORBIDDEN || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!(tr!("límite de peticiones de GitHub alcanzado; añade un token de GitHub en Ajustes o espera un rato", "GitHub request limit reached; add a GitHub token in Settings or wait a while"));
    }
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(tr!("el repositorio {} ya no existe en GitHub", "the repository {} no longer exists on GitHub", game.repo));
    }
    let releases: Vec<Release> = resp.error_for_status()?.json().await?;

    let pattern = game.asset.as_deref().and_then(|p| regex::Regex::new(p).ok());
    let build = releases
        .iter()
        .filter(|r| !r.draft)
        .find_map(|r| {
            let best = r
                .assets
                .iter()
                .filter_map(|a| score(&a.name, pattern.as_ref()).map(|s| (s, a)))
                .max_by_key(|(s, _)| *s)?;
            Some(OfficialBuild {
                version: r.tag_name.clone(),
                prerelease: r.prerelease,
                published_at: r.published_at.clone(),
                asset_name: best.1.name.clone(),
                size: best.1.size,
                url: best.1.browser_download_url.clone(),
                sha256: best.1.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase),
                name: r.name.clone().filter(|n| !n.trim().is_empty()),
                notes: r.body.clone().filter(|b| !b.trim().is_empty()),
                page: r.html_url.clone(),
            })
        })
        .with_context(|| {
            tr!("las releases de {} no tienen una build para Windows; revisa el proyecto en GitHub", "{}'s releases have no Windows build; check the project on GitHub", game.repo)
        })?;

    CACHE
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(game.repo.clone(), (Instant::now(), build.clone()));
    Ok(build)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puntua_assets() {
        assert!(score("Game-Linux-x64.zip", None).is_none());
        assert!(score("Game-macOS.zip", None).is_none());
        assert!(score("source.tar.gz", None).is_none());
        assert!(score("Game-Win64.zip", None) > score("Game.zip", None));
        assert!(score("Game-Win64-setup.exe", None) < score("Game-Win64.zip", None));
    }

    #[tokio::test]
    #[ignore = "usa red; ejecutar con --ignored"]
    async fn resuelve_releases_reales() {
        for id in ["harbourmasters-shipwright", "zelda64recomp-zelda64recomp", "perfect-dark-pc-port-perfect-dark"] {
            let g = crate::catalog::get(id).unwrap();
            let b = latest_build(g, None).await.unwrap();
            println!("{id}: {} -> {} ({} MB)", b.version, b.asset_name, b.size / 1_000_000);
        }
    }
}
