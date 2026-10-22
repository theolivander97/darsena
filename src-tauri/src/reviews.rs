//! Reseñas de juegos en GitHub Discussions (repo darsena-launcher, categoría "Reseñas").
//!
//! - Una discusión por juego, titulada "Título del juego [id-del-juego]"; se crea con la primera reseña.
//! - Cada reseña es un comentario con una marca invisible: <!-- darsena:review rating=4 -->
//! - Una reseña por usuario y juego: si ya existe, se edita.
//! - Sin sesión se leen del resumen público reviews.json (lo genera una GitHub Action del repo).

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OWNER: &str = "theolivander97";
const REPO: &str = "darsena-launcher";
const CATEGORY: &str = "Reseñas";
pub const PUBLIC_URL: &str = "https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/reviews.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    #[serde(default)]
    pub id: Option<String>,
    pub user: String,
    pub avatar: String,
    pub url: String,
    pub rating: u8,
    pub text: String,
    pub date: String,
    #[serde(default)]
    pub mine: bool,
    /// Respuestas de los desarrolladores del port.
    #[serde(default)]
    pub replies: Vec<Reply>,
}

/// Respuesta de un desarrollador a una reseña.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    #[serde(default)]
    pub id: Option<String>,
    pub user: String,
    pub avatar: String,
    pub url: String,
    pub text: String,
    pub date: String,
    #[serde(default)]
    pub mine: bool,
}

const REPLY_MARK: &str = "<!-- darsena:reply -->";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameReviews {
    pub average: f32,
    pub count: usize,
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub discussion_url: Option<String>,
    /// Informes «¿Funciona en tu PC?» de la comunidad (los resume la Action en reviews.json).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
}

// ---------- Formato del comentario ----------

fn body_for(rating: u8, text: &str) -> String {
    let stars: String = (1..=5).map(|i| if i <= rating { '★' } else { '☆' }).collect();
    format!("<!-- darsena:review rating={rating} -->\n{stars} **{rating}/5**\n\n{}", text.trim())
}

fn parse_body(body: &str) -> Option<(u8, String)> {
    let re = regex::Regex::new(r"<!--\s*darsena:review rating=(\d)\s*-->").ok()?;
    let rating: u8 = re.captures(body)?[1].parse().ok()?;
    if !(1..=5).contains(&rating) {
        return None;
    }
    let text = body
        .lines()
        .filter(|l| !l.contains("darsena:review") && !(l.starts_with(['★', '☆']) && l.contains("/5**")))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    Some((rating, text))
}

fn summarize(reviews: Vec<Review>, discussion_url: Option<String>) -> GameReviews {
    let count = reviews.len();
    let average = if count == 0 { 0.0 } else { reviews.iter().map(|r| r.rating as f32).sum::<f32>() / count as f32 };
    GameReviews { average, count, reviews, discussion_url, compat: None }
}

// ---------- GraphQL ----------

pub(crate) async fn gql(token: &str, query: &str, variables: Value) -> anyhow::Result<Value> {
    let res: Value = crate::http_client()
        .post("https://api.github.com/graphql")
        .bearer_auth(token)
        .json(&json!({ "query": query, "variables": variables }))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?
        .error_for_status()?
        .json()
        .await?;
    if let Some(errors) = res.get("errors") {
        anyhow::bail!("GitHub: {}", errors[0]["message"].as_str().map(str::to_string).unwrap_or_else(|| tr!("error desconocido", "unknown error")));
    }
    Ok(res["data"].clone())
}

/// (id del repo, id de la categoría "Reseñas"), cacheados.
static IDS: Mutex<Option<(String, String)>> = Mutex::new(None);
/// id del juego -> (id de la discusión, url), cacheado durante la sesión.
static DISCUSSIONS: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);

async fn ids(token: &str) -> anyhow::Result<(String, String)> {
    if let Some(ids) = IDS.lock().unwrap().clone() {
        return Ok(ids);
    }
    let d = gql(
        token,
        "query($o:String!,$r:String!){repository(owner:$o,name:$r){id discussionCategories(first:25){nodes{id name}}}}",
        json!({ "o": OWNER, "r": REPO }),
    )
    .await?;
    let repo_id = d["repository"]["id"].as_str().context(tr!("no se encontró el repo de reseñas", "the reviews repo wasn't found"))?.to_string();
    let cat_id = d["repository"]["discussionCategories"]["nodes"]
        .as_array()
        .and_then(|cats| cats.iter().find(|c| c["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(CATEGORY))))
        .and_then(|c| c["id"].as_str())
        .with_context(|| tr!("falta la categoría \"{CATEGORY}\" en las Discussions del repo", "the \"{CATEGORY}\" category is missing from the repo's Discussions"))?
        .to_string();
    *IDS.lock().unwrap() = Some((repo_id.clone(), cat_id.clone()));
    Ok((repo_id, cat_id))
}

async fn find_discussion(token: &str, game_id: &str) -> anyhow::Result<Option<(String, String)>> {
    if let Some(found) = DISCUSSIONS.lock().unwrap().as_ref().and_then(|m| m.get(game_id).cloned()) {
        return Ok(Some(found));
    }
    let (_, cat_id) = ids(token).await?;
    let suffix = format!("[{game_id}]");
    let mut after: Option<String> = None;
    loop {
        let d = gql(
            token,
            "query($o:String!,$r:String!,$c:ID!,$a:String){repository(owner:$o,name:$r){discussions(first:100,after:$a,categoryId:$c){pageInfo{hasNextPage endCursor} nodes{id title url}}}}",
            json!({ "o": OWNER, "r": REPO, "c": cat_id, "a": after }),
        )
        .await?;
        let page = &d["repository"]["discussions"];
        let mut map = DISCUSSIONS.lock().unwrap();
        let map = map.get_or_insert_with(HashMap::new);
        for n in page["nodes"].as_array().into_iter().flatten() {
            let title = n["title"].as_str().unwrap_or("");
            if let (Some(start), Some(end)) = (title.rfind('['), title.rfind(']')) {
                if start < end {
                    let id = &title[start + 1..end];
                    map.insert(id.to_string(), (n["id"].as_str().unwrap_or("").into(), n["url"].as_str().unwrap_or("").into()));
                }
            }
            if title.ends_with(&suffix) {
                return Ok(map.get(game_id).cloned());
            }
        }
        if !page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false) {
            return Ok(None);
        }
        after = page["pageInfo"]["endCursor"].as_str().map(String::from);
    }
}

/// Reseñas de un juego leídas en vivo de GitHub (con sesión iniciada).
pub async fn live(token: &str, game_id: &str) -> anyhow::Result<GameReviews> {
    let Some((disc_id, url)) = find_discussion(token, game_id).await? else {
        return Ok(GameReviews::default());
    };
    let d = gql(
        token,
        "query($id:ID!){node(id:$id){... on Discussion{comments(last:100){nodes{id body createdAt isMinimized viewerDidAuthor author{login avatarUrl url} replies(first:20){nodes{id body createdAt isMinimized viewerDidAuthor author{login avatarUrl url}}}}}}}}",
        json!({ "id": disc_id }),
    )
    .await?;
    // Solo cuentan las respuestas de quien desarrolla alguno de los ports del juego.
    let devs = crate::devs::devs_of_game(game_id).await;
    let mut reviews: Vec<Review> = d["node"]["comments"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| !c["isMinimized"].as_bool().unwrap_or(false))
        .filter_map(|c| {
            let (rating, text) = parse_body(c["body"].as_str()?)?;
            Some(Review {
                id: c["id"].as_str().map(String::from),
                user: c["author"]["login"].as_str().unwrap_or("ghost").into(),
                avatar: c["author"]["avatarUrl"].as_str().unwrap_or("").into(),
                url: c["author"]["url"].as_str().unwrap_or("").into(),
                rating,
                text,
                date: c["createdAt"].as_str().unwrap_or("").into(),
                mine: c["viewerDidAuthor"].as_bool().unwrap_or(false),
                replies: c["replies"]["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|r| !r["isMinimized"].as_bool().unwrap_or(false))
                    .filter(|r| r["author"]["login"].as_str().is_some_and(|l| devs.contains(&l.to_lowercase())))
                    .filter_map(|r| {
                        Some(Reply {
                            id: r["id"].as_str().map(String::from),
                            user: r["author"]["login"].as_str()?.into(),
                            avatar: r["author"]["avatarUrl"].as_str().unwrap_or("").into(),
                            url: r["author"]["url"].as_str().unwrap_or("").into(),
                            text: r["body"].as_str()?.replace(REPLY_MARK, "").trim().to_string(),
                            date: r["createdAt"].as_str().unwrap_or("").into(),
                            mine: r["viewerDidAuthor"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect(),
            })
        })
        .collect();
    // Si alguien dejó varias, cuenta solo la última de cada usuario.
    reviews.reverse();
    let mut seen = std::collections::HashSet::new();
    reviews.retain(|r| seen.insert(r.user.clone()));
    Ok(summarize(reviews, Some(url)))
}

/// Publica o edita la reseña del usuario para un juego.
pub async fn submit(token: &str, game_id: &str, game_title: &str, rating: u8, text: &str) -> anyhow::Result<GameReviews> {
    anyhow::ensure!((1..=5).contains(&rating), tr!("la nota debe ser de 1 a 5 estrellas", "the rating must be 1 to 5 stars"));
    anyhow::ensure!(text.chars().count() <= 4000, tr!("la reseña es demasiado larga (máximo 4000 caracteres)", "the review is too long (4000 characters max)"));
    let body = body_for(rating, text);

    let disc_id = discussion_for(token, game_id, game_title).await?;
    let mine = live(token, game_id).await?.reviews.into_iter().find(|r| r.mine).and_then(|r| r.id);
    match mine {
        Some(comment_id) => {
            gql(token, "mutation($id:ID!,$b:String!){updateDiscussionComment(input:{commentId:$id,body:$b}){comment{id}}}", json!({ "id": comment_id, "b": body })).await?;
        }
        None => {
            gql(token, "mutation($d:ID!,$b:String!){addDiscussionComment(input:{discussionId:$d,body:$b}){comment{id}}}", json!({ "d": disc_id, "b": body })).await?;
        }
    }
    live(token, game_id).await
}

/// La discusión de reseñas del juego (la crea si aún no existe).
async fn discussion_for(token: &str, game_id: &str, game_title: &str) -> anyhow::Result<String> {
    Ok(match find_discussion(token, game_id).await? {
        Some((id, _)) => id,
        None => {
            let (repo_id, cat_id) = ids(token).await?;
            let d = gql(
                token,
                "mutation($r:ID!,$c:ID!,$t:String!,$b:String!){createDiscussion(input:{repositoryId:$r,categoryId:$c,title:$t,body:$b}){discussion{id url}}}",
                json!({
                    "r": repo_id, "c": cat_id,
                    "t": format!("{game_title} [{game_id}]"),
                    "b": format!("Reseñas de **{game_title}** escritas desde Dársena.\n\nCada comentario es una reseña con su nota de 1 a 5 estrellas."),
                }),
            )
            .await?;
            let disc = &d["createDiscussion"]["discussion"];
            let (id, url) = (disc["id"].as_str().context(tr!("no se pudo crear la discusión", "couldn't create the discussion"))?.to_string(), disc["url"].as_str().unwrap_or("").to_string());
            DISCUSSIONS.lock().unwrap().get_or_insert_with(HashMap::new).insert(game_id.into(), (id.clone(), url));
            id
        }
    })
}

const COMPAT_MARK: &str = "darsena:compat";

/// «¿Qué tal te funcionó?»: un comentario por usuario en la discusión del juego, con su resultado y su hardware.
/// Si ya había informado de ese juego, se sustituye.
pub async fn report_compat(token: &str, game_id: &str, game_title: &str, result: &str, port: &str, hw: &crate::hardware::Hardware) -> anyhow::Result<()> {
    anyhow::ensure!(["good", "ok", "bad"].contains(&result), "resultado no válido");
    let label = match result {
        "good" => "Funciona bien",
        "ok" => "Funciona con problemas",
        _ => "No funciona bien",
    };
    let data = json!({
        "result": result,
        "port": port,
        "cpu": hw.cpu,
        "gpu": hw.gpus.first().cloned().unwrap_or_default(),
        "ram": hw.ram_gb.round() as u64,
    });
    let body = format!(
        "<!-- {COMPAT_MARK} {} -->\n**{label}** en mi PC · {} · {} · {} GB",
        data.to_string().replace("-->", ""),
        data["cpu"].as_str().unwrap_or("?"),
        data["gpu"].as_str().unwrap_or("?"),
        data["ram"]
    );
    let disc_id = discussion_for(token, game_id, game_title).await?;
    let d = gql(
        token,
        "query($id:ID!){node(id:$id){... on Discussion{comments(last:100){nodes{id body viewerDidAuthor}}}}}",
        json!({ "id": disc_id }),
    )
    .await?;
    let mine = d["node"]["comments"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["viewerDidAuthor"].as_bool().unwrap_or(false) && c["body"].as_str().is_some_and(|b| b.contains(COMPAT_MARK)))
        .and_then(|c| c["id"].as_str().map(str::to_string));
    match mine {
        Some(id) => gql(token, "mutation($id:ID!,$b:String!){updateDiscussionComment(input:{commentId:$id,body:$b}){comment{id}}}", json!({ "id": id, "b": body })).await?,
        None => gql(token, "mutation($d:ID!,$b:String!){addDiscussionComment(input:{discussionId:$d,body:$b}){comment{id}}}", json!({ "d": disc_id, "b": body })).await?,
    };
    Ok(())
}

/// Responde (o edita tu respuesta) a una reseña. Solo pueden los desarrolladores de algún port del juego.
pub async fn reply(token: &str, login: &str, game_id: &str, review_id: &str, text: &str) -> anyhow::Result<GameReviews> {
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), tr!("escribe tu respuesta", "write your reply"));
    anyhow::ensure!(text.chars().count() <= 4000, tr!("la respuesta es demasiado larga (máximo 4000 caracteres)", "the reply is too long (4000 characters max)"));
    anyhow::ensure!(
        !crate::devs::my_ports_of_game(login, game_id).await.is_empty(),
        tr!("solo los desarrolladores de este port pueden responder a sus reseñas", "only this port's developers can reply to its reviews")
    );
    let (disc_id, _) = find_discussion(token, game_id).await?.context(tr!("la reseña ya no existe", "the review no longer exists"))?;
    let body = format!("{REPLY_MARK}\n{text}");
    let current = live(token, game_id).await?;
    let mine = current.reviews.iter().find(|r| r.id.as_deref() == Some(review_id)).and_then(|r| r.replies.iter().find(|x| x.mine)).and_then(|x| x.id.clone());
    match mine {
        Some(id) => {
            gql(token, "mutation($id:ID!,$b:String!){updateDiscussionComment(input:{commentId:$id,body:$b}){comment{id}}}", json!({ "id": id, "b": body })).await?;
        }
        None => {
            gql(
                token,
                "mutation($d:ID!,$r:ID!,$b:String!){addDiscussionComment(input:{discussionId:$d,replyToId:$r,body:$b}){comment{id}}}",
                json!({ "d": disc_id, "r": review_id, "b": body }),
            )
            .await?;
        }
    }
    live(token, game_id).await
}

pub async fn delete_mine(token: &str, game_id: &str) -> anyhow::Result<GameReviews> {
    let mine = live(token, game_id).await?.reviews.into_iter().find(|r| r.mine).and_then(|r| r.id);
    if let Some(id) = mine {
        gql(token, "mutation($id:ID!){deleteDiscussionComment(input:{id:$id}){comment{id}}}", json!({ "id": id })).await?;
    }
    live(token, game_id).await
}

// ---------- Resumen público (sin sesión) ----------

static PUBLIC: Mutex<Option<(Instant, HashMap<String, GameReviews>)>> = Mutex::new(None);

/// reviews.json del repo: id del juego -> reseñas. Se cachea 10 minutos.
pub async fn public() -> HashMap<String, GameReviews> {
    if let Some((at, map)) = PUBLIC.lock().unwrap().clone() {
        if at.elapsed() < Duration::from_secs(600) {
            return map;
        }
    }
    let map: HashMap<String, GameReviews> = async {
        let text = crate::http_client()
            .get(format!("{PUBLIC_URL}?t={}", crate::store::now()))
            .timeout(Duration::from_secs(15))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        anyhow::Ok(serde_json::from_str::<Value>(&text)?
            .get("games")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default())
    }
    .await
    .unwrap_or_default();
    *PUBLIC.lock().unwrap() = Some((Instant::now(), map.clone()));
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formato_de_resena() {
        let body = body_for(4, "  Muy buen port.\nCorre perfecto.  ");
        assert_eq!(parse_body(&body), Some((4, "Muy buen port.\nCorre perfecto.".into())));
        assert_eq!(parse_body("comentario normal"), None);
        assert_eq!(parse_body("<!-- darsena:review rating=9 -->"), None);
    }
}
