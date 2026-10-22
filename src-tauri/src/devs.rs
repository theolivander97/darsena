//! Quién es desarrollador de un port del catálogo: el dueño de su repositorio, un miembro (público) de la
//! organización dueña o alguien que ha contribuido a él. Sirve para el panel del desarrollador y para marcar
//! (y permitir) las respuestas a las reseñas.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(60 * 60);

static CONTRIBUTORS: Mutex<Option<HashMap<String, (Instant, HashSet<String>)>>> = Mutex::new(None);
static ORGS: Mutex<Option<HashMap<String, (Instant, HashSet<String>)>>> = Mutex::new(None);

async fn get_json(path: &str) -> Option<serde_json::Value> {
    let mut req = crate::http_client()
        .get(format!("https://api.github.com/{path}"))
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15));
    if let Some(t) = crate::auth::token() {
        req = req.bearer_auth(t);
    }
    req.send().await.ok()?.error_for_status().ok()?.json().await.ok()
}

fn owner_of(repo: &str) -> String {
    repo.split('/').next().unwrap_or_default().to_lowercase()
}

/// Logins (en minúsculas) del dueño y de los contribuidores de un repositorio.
pub async fn contributors(repo: &str) -> HashSet<String> {
    let key = repo.to_lowercase();
    if let Some((at, set)) = CONTRIBUTORS.lock().unwrap().as_ref().and_then(|m| m.get(&key).cloned()) {
        if at.elapsed() < TTL {
            return set;
        }
    }
    let mut set: HashSet<String> = HashSet::from([owner_of(repo)]);
    if let Some(list) = get_json(&format!("repos/{repo}/contributors?per_page=100")).await {
        set.extend(list.as_array().into_iter().flatten().filter_map(|c| c["login"].as_str()).map(str::to_lowercase));
    }
    CONTRIBUTORS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (Instant::now(), set.clone()));
    set
}

/// Organizaciones públicas de un usuario (en minúsculas).
async fn orgs(login: &str) -> HashSet<String> {
    let key = login.to_lowercase();
    if let Some((at, set)) = ORGS.lock().unwrap().as_ref().and_then(|m| m.get(&key).cloned()) {
        if at.elapsed() < TTL {
            return set;
        }
    }
    let set: HashSet<String> = get_json(&format!("users/{login}/orgs"))
        .await
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|o| o["login"].as_str())
        .map(str::to_lowercase)
        .collect();
    ORGS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (Instant::now(), set.clone()));
    set
}

fn github_ports_of_game(game_id: &str) -> Vec<&'static crate::catalog::CatalogGame> {
    crate::catalog::all()
        .iter()
        .filter(|p| p.game == game_id && p.origin == crate::catalog::Origin::Github && !p.repo.is_empty())
        .collect()
}

/// Todos los desarrolladores de los ports de un juego (logins en minúsculas).
pub async fn devs_of_game(game_id: &str) -> HashSet<String> {
    let mut all = HashSet::new();
    for p in github_ports_of_game(game_id) {
        all.extend(contributors(&p.repo).await);
    }
    all
}

/// Ports de un juego de los que `login` es desarrollador.
pub async fn my_ports_of_game(login: &str, game_id: &str) -> Vec<String> {
    let me = login.to_lowercase();
    let my_orgs = orgs(login).await;
    let mut out = Vec::new();
    for p in github_ports_of_game(game_id) {
        let owner = owner_of(&p.repo);
        if owner == me || my_orgs.contains(&owner) || contributors(&p.repo).await.contains(&me) {
            out.push(p.id.clone());
        }
    }
    out
}

/// Ports del catálogo cuyo repositorio es tuyo o de una de tus organizaciones.
pub async fn my_ports(login: &str) -> Vec<String> {
    let me = login.to_lowercase();
    let my_orgs = orgs(login).await;
    crate::catalog::all()
        .iter()
        .filter(|p| p.origin == crate::catalog::Origin::Github && !p.repo.is_empty())
        .filter(|p| {
            let owner = owner_of(&p.repo);
            owner == me || my_orgs.contains(&owner)
        })
        .map(|p| p.id.clone())
        .collect()
}
