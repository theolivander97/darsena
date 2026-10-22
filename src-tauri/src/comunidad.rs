//! Enlace al servidor de Discord de Dársena.
//!
//! Las invitaciones se caen o se renuevan, así que el enlace no va fijo en la aplicación: se lee de discord.json
//! en el repo de datos (https://github.com/theolivander97/darsena-launcher). Para cambiarlo basta con editar ese
//! archivo, sin sacar una versión nueva. Sin conexión se usa el último enlace guardado, o el que trae la versión.

use std::path::Path;

/// Enlace que trae esta versión, por si nunca se ha podido descargar el publicado.
const DEFAULT_INVITE: &str = "https://discord.gg/NheqrHfAZ";
const REMOTE_URL: &str = "https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/discord.json";
const CACHE_FILE: &str = "discord-link.txt";

/// Solo se aceptan invitaciones de Discord: el archivo es público y no debe poder mandar a otro sitio.
fn valid(link: &str) -> bool {
    let rest = link
        .strip_prefix("https://discord.gg/")
        .or_else(|| link.strip_prefix("https://discord.com/invite/"));
    match rest {
        Some(code) => !code.is_empty() && code.len() <= 40 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        None => false,
    }
}

async fn download() -> anyhow::Result<String> {
    let value: serde_json::Value = crate::http_client()
        .get(format!("{REMOTE_URL}?t={}", crate::store::now()))
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let link = value["invite"].as_str().unwrap_or_default().trim().to_string();
    anyhow::ensure!(valid(&link), "discord.json no tiene una invitación válida");
    Ok(link)
}

/// Enlace vigente: el publicado si se puede descargar; si no, el último guardado; si no, el de fábrica.
pub async fn invite(data_dir: &Path) -> String {
    if let Ok(link) = download().await {
        let _ = std::fs::write(data_dir.join(CACHE_FILE), &link);
        return link;
    }
    match std::fs::read_to_string(data_dir.join(CACHE_FILE)) {
        Ok(cached) if valid(cached.trim()) => cached.trim().to_string(),
        _ => DEFAULT_INVITE.to_string(),
    }
}
