//! Inicio de sesión con GitHub (GitHub App "Darsena Launcher", Device Flow).
//! El usuario ve un código, lo introduce en github.com/login/device y Dársena recibe su token.
//! No hay servidor ni secreto de cliente; el token se guarda en el Administrador de credenciales de Windows.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Client ID de la GitHub App (no es secreto: identifica a la app ante GitHub).
pub const CLIENT_ID: &str = "Iv23liDXJAg77jlLNeeF";
const KEYRING_SERVICE: &str = "com.darsena.app";
const KEYRING_USER: &str = "github";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct GhUser {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub html_url: String,
}

/// Usuario de la sesión actual (para no preguntar a GitHub en cada pantalla).
static USER: Mutex<Option<GhUser>> = Mutex::new(None);

fn entry() -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).context(tr!("no se pudo acceder al Administrador de credenciales", "couldn't access the Windows Credential Manager"))
}

pub fn token() -> Option<String> {
    entry().ok()?.get_password().ok()
}

/// Cuerpo application/x-www-form-urlencoded.
fn form(pairs: &[(&str, &str)]) -> String {
    let enc = |v: &str| {
        v.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

fn configured() -> anyhow::Result<()> {
    anyhow::ensure!(CLIENT_ID != "PENDIENTE", tr!("el inicio de sesión con GitHub aún no está configurado en esta versión", "GitHub sign-in isn't set up in this version yet"));
    Ok(())
}

/// Paso 1: pide a GitHub un código para que el usuario lo introduzca en github.com/login/device.
pub async fn start() -> anyhow::Result<DeviceCode> {
    configured()?;
    let res = crate::http_client()
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form(&[("client_id", CLIENT_ID)]))
        .send()
        .await
        .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?
        .error_for_status()?;
    Ok(res.json().await.context(tr!("respuesta inesperada de GitHub", "unexpected response from GitHub"))?)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

/// Paso 2: comprueba si el usuario ya autorizó. Ok(None) = todavía no (la UI vuelve a preguntar).
pub async fn poll(device_code: &str) -> anyhow::Result<Option<GhUser>> {
    configured()?;
    let res: TokenResponse = crate::http_client()
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form(&[
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]))
        .send()
        .await?
        .json()
        .await?;
    match (res.access_token, res.error.as_deref()) {
        (Some(token), _) => {
            entry()?.set_password(&token).context(tr!("no se pudo guardar la sesión", "couldn't save the session"))?;
            Ok(Some(user().await?.context(tr!("GitHub no devolvió el usuario", "GitHub didn't return the user"))?))
        }
        (None, Some("authorization_pending" | "slow_down")) => Ok(None),
        (None, Some("expired_token")) => anyhow::bail!(tr!("el código caducó; vuelve a intentarlo", "the code expired; try again")),
        (None, Some("access_denied")) => anyhow::bail!(tr!("cancelaste la autorización en GitHub", "you cancelled the authorization on GitHub")),
        (None, other) => anyhow::bail!(tr!("GitHub rechazó el inicio de sesión ({})", "GitHub rejected the sign-in ({})", other.unwrap_or("-"))),
    }
}

/// Usuario con sesión iniciada, o None. Si GitHub ya no acepta el token (revocado), se cierra la sesión.
pub async fn user() -> anyhow::Result<Option<GhUser>> {
    if let Some(u) = USER.lock().unwrap().clone() {
        return Ok(Some(u));
    }
    let Some(token) = token() else { return Ok(None) };
    let res = crate::http_client()
        .get("https://api.github.com/user")
        .bearer_auth(&token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?;
    if res.status() == reqwest::StatusCode::UNAUTHORIZED {
        logout();
        return Ok(None);
    }
    let u: GhUser = res.error_for_status()?.json().await?;
    *USER.lock().unwrap() = Some(u.clone());
    Ok(Some(u))
}

pub fn logout() {
    *USER.lock().unwrap() = None;
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
}
