//! Perfil del jugador: tiempo por juego, logros del launcher y reseñas escritas.
//!
//! Se guarda en profile.json (sobrevive a desinstalar ports) y, con sesión de GitHub, se sincroniza con un Gist
//! público del usuario ("darsena-profile.json"). Así cualquiera puede ver su perfil desde sus reseñas.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Duration;

pub const GIST_FILE: &str = "darsena-profile.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameStats {
    #[serde(default)]
    pub playtime_secs: u64,
    #[serde(default)]
    pub sessions: u32,
    #[serde(default)]
    pub first_played: Option<u64>,
    #[serde(default)]
    pub last_played: Option<u64>,
    /// Sesión más larga, en segundos.
    #[serde(default)]
    pub longest_session: u64,
    /// Ports con los que se ha jugado.
    #[serde(default)]
    pub ports: BTreeSet<String>,
    /// Segundos jugados por día (días desde 1970), solo los últimos 30: para "las últimas dos semanas".
    #[serde(default)]
    pub days: BTreeMap<u64, u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub updated_at: u64,
    /// Id del juego -> estadísticas.
    #[serde(default)]
    pub games: BTreeMap<String, GameStats>,
    /// Id del logro -> fecha en que se desbloqueó.
    #[serde(default)]
    pub achievements: BTreeMap<String, u64>,
    /// Juegos reseñados.
    #[serde(default)]
    pub reviewed: BTreeSet<String>,
    /// Gist donde se sincroniza (solo local).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gist_id: Option<String>,
}

fn one() -> u32 {
    1
}

impl Default for Profile {
    fn default() -> Self {
        Self { schema_version: 1, updated_at: 0, games: BTreeMap::new(), achievements: BTreeMap::new(), reviewed: BTreeSet::new(), gist_id: None }
    }
}

// ---------- Logros ----------

pub struct AchievementDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// Nombre y descripción en inglés.
    pub en: (&'static str, &'static str),
    pub icon: &'static str,
    check: fn(&Ctx) -> bool,
}

impl AchievementDef {
    pub fn title(&self) -> &'static str {
        if crate::i18n::en() { self.en.0 } else { self.name }
    }
    pub fn text(&self) -> &'static str {
        if crate::i18n::en() { self.en.1 } else { self.description }
    }
}

/// Lo que miran los logros.
struct Ctx<'a> {
    p: &'a Profile,
    owned: usize,
}

impl Ctx<'_> {
    fn max_game_hours(&self) -> f64 {
        self.p.games.values().map(|g| g.playtime_secs).max().unwrap_or(0) as f64 / 3600.0
    }
    fn total_hours(&self) -> f64 {
        self.p.games.values().map(|g| g.playtime_secs).sum::<u64>() as f64 / 3600.0
    }
    fn played(&self) -> usize {
        self.p.games.values().filter(|g| g.sessions > 0).count()
    }
    fn consoles(&self) -> usize {
        let played: HashSet<&str> = self.p.games.iter().filter(|(_, g)| g.sessions > 0).map(|(id, _)| id.as_str()).collect();
        crate::catalog::all().iter().filter(|c| played.contains(c.game.as_str())).map(|c| c.console.as_str()).collect::<HashSet<_>>().len()
    }
    fn played_experimental(&self) -> bool {
        self.p.games.values().flat_map(|g| g.ports.iter()).any(|id| crate::catalog::get(id).is_some_and(|c| c.experimental))
    }
    fn longest_session_hours(&self) -> f64 {
        self.p.games.values().map(|g| g.longest_session).max().unwrap_or(0) as f64 / 3600.0
    }
}

pub static ACHIEVEMENTS: &[AchievementDef] = &[
    AchievementDef { id: "zarpamos", name: "Zarpamos", description: "Juega a tu primer port.", en: ("Setting Sail", "Play your first port."), icon: "⚓", check: |c| c.played() >= 1 },
    AchievementDef { id: "calentando", name: "Calentando motores", description: "Juega 1 hora a un mismo juego.", en: ("Warming Up", "Play the same game for 1 hour."), icon: "⏱️", check: |c| c.max_game_hours() >= 1.0 },
    AchievementDef { id: "enganchado", name: "Enganchado", description: "Juega 10 horas a un mismo juego.", en: ("Hooked", "Play the same game for 10 hours."), icon: "🎣", check: |c| c.max_game_hours() >= 10.0 },
    AchievementDef { id: "devoto", name: "Devoto", description: "Juega 50 horas a un mismo juego.", en: ("Devoted", "Play the same game for 50 hours."), icon: "🔥", check: |c| c.max_game_hours() >= 50.0 },
    AchievementDef { id: "maraton", name: "Maratón", description: "Juega 4 horas seguidas.", en: ("Marathon", "Play for 4 hours straight."), icon: "🏃", check: |c| c.longest_session_hours() >= 4.0 },
    AchievementDef { id: "marinero", name: "Marinero", description: "Acumula 25 horas de juego.", en: ("Sailor", "Reach 25 hours of playtime."), icon: "⛵", check: |c| c.total_hours() >= 25.0 },
    AchievementDef { id: "lobo-de-mar", name: "Lobo de mar", description: "Acumula 100 horas de juego.", en: ("Old Sea Dog", "Reach 100 hours of playtime."), icon: "🐺", check: |c| c.total_hours() >= 100.0 },
    AchievementDef { id: "explorador", name: "Explorador", description: "Juega a 5 juegos distintos.", en: ("Explorer", "Play 5 different games."), icon: "🧭", check: |c| c.played() >= 5 },
    AchievementDef { id: "trotamundos", name: "Trotamundos", description: "Juega a 20 juegos distintos.", en: ("Globetrotter", "Play 20 different games."), icon: "🌍", check: |c| c.played() >= 20 },
    AchievementDef { id: "multiplataforma", name: "Multiplataforma", description: "Juega a ports de 3 consolas distintas.", en: ("Multiplatform", "Play ports from 3 different consoles."), icon: "🎮", check: |c| c.consoles() >= 3 },
    AchievementDef { id: "museo", name: "Museo del videojuego", description: "Juega a ports de 6 consolas distintas.", en: ("Video Game Museum", "Play ports from 6 different consoles."), icon: "🏛️", check: |c| c.consoles() >= 6 },
    AchievementDef { id: "coleccionista", name: "Coleccionista", description: "Ten 10 juegos en tu biblioteca.", en: ("Collector", "Have 10 games in your library."), icon: "📚", check: |c| c.owned >= 10 },
    AchievementDef { id: "acaparador", name: "Acaparador", description: "Ten 50 juegos en tu biblioteca.", en: ("Hoarder", "Have 50 games in your library."), icon: "🗄️", check: |c| c.owned >= 50 },
    AchievementDef { id: "conejillo", name: "Conejillo de indias", description: "Juega a un port experimental.", en: ("Guinea Pig", "Play an experimental port."), icon: "🧪", check: |c| c.played_experimental() },
    AchievementDef { id: "critico", name: "Crítico", description: "Escribe tu primera reseña.", en: ("Critic", "Write your first review."), icon: "✍️", check: |c| !c.p.reviewed.is_empty() },
    AchievementDef { id: "critico-experto", name: "Crítico experto", description: "Escribe reseñas de 5 juegos.", en: ("Expert Critic", "Review 5 games."), icon: "🏅", check: |c| c.p.reviewed.len() >= 5 },
];

/// Desbloquea los logros que se cumplan y devuelve los nuevos.
pub fn evaluate(p: &mut Profile, owned: usize) -> Vec<&'static AchievementDef> {
    let now = crate::store::now();
    let new: Vec<_> = {
        let ctx = Ctx { p, owned };
        ACHIEVEMENTS.iter().filter(|a| !ctx.p.achievements.contains_key(a.id) && (a.check)(&ctx)).collect()
    };
    for a in &new {
        p.achievements.insert(a.id.to_string(), now);
    }
    if !new.is_empty() {
        p.updated_at = now;
    }
    new
}

/// Suma una partida terminada al perfil.
pub fn record_session(p: &mut Profile, port_id: &str, secs: u64) {
    // Un juego añadido a mano (fuera del catálogo) es su propio juego.
    let game = match crate::catalog::get(port_id) {
        Some(port) => port.game.clone(),
        None if port_id.starts_with(crate::CUSTOM_PREFIX) => port_id.to_string(),
        None => return,
    };
    let now = crate::store::now();
    let g = p.games.entry(game).or_default();
    g.playtime_secs += secs;
    g.sessions += 1;
    g.first_played.get_or_insert(now);
    g.last_played = Some(now);
    g.longest_session = g.longest_session.max(secs);
    g.ports.insert(port_id.to_string());
    let today = now / 86_400;
    *g.days.entry(today).or_default() += secs;
    g.days.retain(|d, _| *d + 30 > today);
    p.updated_at = now;
}

/// Segundos jugados a un juego en los últimos 14 días.
fn last_two_weeks(g: &GameStats) -> u64 {
    let today = crate::store::now() / 86_400;
    g.days.iter().filter(|(d, _)| **d + 14 > today).map(|(_, s)| s).sum()
}

/// Junta dos perfiles (el local y el del Gist, p. ej. de otro PC): se queda con lo mayor de cada cosa.
pub fn merge(local: &mut Profile, remote: Profile) {
    for (id, r) in remote.games {
        let l = local.games.entry(id).or_default();
        l.playtime_secs = l.playtime_secs.max(r.playtime_secs);
        l.sessions = l.sessions.max(r.sessions);
        l.first_played = match (l.first_played, r.first_played) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        l.last_played = l.last_played.max(r.last_played);
        l.longest_session = l.longest_session.max(r.longest_session);
        l.ports.extend(r.ports);
        for (d, s) in r.days {
            let e = l.days.entry(d).or_default();
            *e = (*e).max(s);
        }
    }
    for (id, at) in remote.achievements {
        let e = local.achievements.entry(id).or_insert(at);
        *e = (*e).min(at);
    }
    local.reviewed.extend(remote.reviewed);
    local.updated_at = local.updated_at.max(remote.updated_at);
}

// ---------- Vista para la interfaz ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub game: String,
    pub title: String,
    pub console: String,
    pub playtime_secs: u64,
    pub sessions: u32,
    pub last_played: Option<u64>,
    pub last_two_weeks_secs: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementView {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon: &'static str,
    pub unlocked_at: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub login: Option<String>,
    pub avatar: Option<String>,
    pub total_secs: u64,
    pub games_played: usize,
    pub reviews: usize,
    pub games: Vec<GameView>,
    pub achievements: Vec<AchievementView>,
    pub gist_url: Option<String>,
    pub updated_at: u64,
}

pub fn view(p: &Profile, login: Option<String>, avatar: Option<String>, gist_url: Option<String>) -> ProfileView {
    let mut games: Vec<GameView> = p
        .games
        .iter()
        .filter(|(_, g)| g.sessions > 0)
        .map(|(id, g)| GameView {
            game: id.clone(),
            title: crate::catalog::game_title(id),
            console: crate::catalog::all().iter().find(|c| &c.game == id).map(|c| c.console.clone()).unwrap_or_default(),
            playtime_secs: g.playtime_secs,
            sessions: g.sessions,
            last_played: g.last_played,
            last_two_weeks_secs: last_two_weeks(g),
        })
        .collect();
    games.sort_by(|a, b| b.playtime_secs.cmp(&a.playtime_secs).then(b.last_played.cmp(&a.last_played)));
    ProfileView {
        login,
        avatar,
        total_secs: p.games.values().map(|g| g.playtime_secs).sum(),
        games_played: games.len(),
        reviews: p.reviewed.len(),
        games,
        achievements: ACHIEVEMENTS
            .iter()
            .map(|a| AchievementView { id: a.id, name: a.title(), description: a.text(), icon: a.icon, unlocked_at: p.achievements.get(a.id).copied() })
            .collect(),
        gist_url,
        updated_at: p.updated_at,
    }
}

// ---------- Gist ----------

fn api(req: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    let req = req.header("Accept", "application/vnd.github+json").timeout(Duration::from_secs(20));
    match token {
        Some(t) => req.bearer_auth(t),
        None => req,
    }
}

/// Busca el Gist del perfil entre los Gists de un usuario: (id, url, url del archivo).
async fn find_gist(login: Option<&str>, token: Option<&str>) -> anyhow::Result<Option<(String, String, String)>> {
    for page in 1..=5 {
        let url = match login {
            Some(l) => format!("https://api.github.com/users/{l}/gists?per_page=100&page={page}"),
            None => format!("https://api.github.com/gists?per_page=100&page={page}"),
        };
        let list: Vec<serde_json::Value> = api(crate::http_client().get(url), token)
            .send()
            .await
            .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?
            .error_for_status()?
            .json()
            .await?;
        for g in &list {
            if let Some(f) = g["files"].get(GIST_FILE) {
                return Ok(Some((
                    g["id"].as_str().unwrap_or_default().into(),
                    g["html_url"].as_str().unwrap_or_default().into(),
                    f["raw_url"].as_str().unwrap_or_default().into(),
                )));
            }
        }
        if list.len() < 100 {
            break;
        }
    }
    Ok(None)
}

async fn read_raw(raw_url: &str) -> anyhow::Result<Profile> {
    let text = crate::http_client().get(raw_url).timeout(Duration::from_secs(20)).send().await?.error_for_status()?.text().await?;
    serde_json::from_str(&text).context(tr!("el perfil del Gist no es válido", "the Gist profile isn't valid"))
}

/// Trae el perfil del Gist del usuario (si existe), lo junta con el local y sube el resultado.
/// Devuelve el perfil sincronizado y la url del Gist.
pub async fn sync(token: &str, mut local: Profile) -> anyhow::Result<(Profile, String)> {
    let found = find_gist(None, Some(token)).await?;
    if let Some((_, _, raw)) = &found {
        if let Ok(remote) = read_raw(raw).await {
            merge(&mut local, remote);
        }
    }
    let mut public = local.clone();
    public.gist_id = None;
    let content = serde_json::to_string_pretty(&public)?;
    let files = json!({ GIST_FILE: { "content": content } });
    let res: serde_json::Value = match &found {
        Some((id, _, _)) => api(crate::http_client().patch(format!("https://api.github.com/gists/{id}")), Some(token))
            .json(&json!({ "files": files }))
            .send()
            .await?
            .error_for_status()
            .context(tr!("no se pudo actualizar el Gist del perfil", "couldn't update the profile Gist"))?
            .json()
            .await?,
        None => api(crate::http_client().post("https://api.github.com/gists"), Some(token))
            .json(&json!({ "description": "Perfil de Dársena (tiempo de juego y logros)", "public": true, "files": files }))
            .send()
            .await?
            .error_for_status()
            .context(tr!("no se pudo crear el Gist del perfil", "couldn't create the profile Gist"))?
            .json()
            .await?,
    };
    local.gist_id = res["id"].as_str().map(String::from);
    Ok((local, res["html_url"].as_str().unwrap_or_default().to_string()))
}

/// Perfil público de otro usuario (desde su Gist). None si no usa Dársena o no lo ha sincronizado.
pub async fn of_user(login: &str, token: Option<&str>) -> anyhow::Result<Option<(Profile, String)>> {
    anyhow::ensure!(login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), tr!("usuario no válido", "invalid user"));
    let Some((_, url, raw)) = find_gist(Some(login), token).await? else { return Ok(None) };
    Ok(Some((read_raw(&raw).await?, url)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mezcla_y_logros() {
        let mut a = Profile::default();
        record_session(&mut a, "harbourmasters-starship", 2 * 3600);
        let mut b = Profile::default();
        record_session(&mut b, "harbourmasters-starship", 5 * 3600);
        b.reviewed.insert("star-fox-64".into());
        merge(&mut a, b);
        let g = &a.games["star-fox-64"];
        assert_eq!((g.playtime_secs, g.longest_session), (5 * 3600, 5 * 3600));
        let new: Vec<_> = evaluate(&mut a, 1).iter().map(|x| x.id).collect();
        assert!(new.contains(&"zarpamos") && new.contains(&"maraton") && new.contains(&"critico"));
        assert!(evaluate(&mut a, 1).is_empty());
    }
}
