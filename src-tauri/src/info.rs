//! Información de los juegos: sinopsis, tráilers y los destacados del carrusel.
//!
//! Se genera con tools/catalog/informacion.mjs y se publica junto al catálogo (info.json en el repo darsena-launcher).
//! Funciona como el catálogo: va embebido en el binario, se comprueba al abrir y, si cambió, sustituye al actual
//! sin reiniciar. Sin conexión se usa el último descargado.

use anyhow::Context;
use serde_json::Value;
use std::path::Path;
use std::sync::RwLock;

pub const REMOTE_URL: &str = "https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/info.json";
const CACHE_FILE: &str = "info-remote.json";

pub struct Info {
    value: Value,
    generated_at: String,
    hash: String,
}

static CURRENT: RwLock<Option<&'static Info>> = RwLock::new(None);

fn parse(text: &str) -> anyhow::Result<Info> {
    let value: Value = serde_json::from_str(text).context("info.json no es JSON válido")?;
    anyhow::ensure!(value["games"].is_object(), "info.json no tiene la lista \"games\"");
    Ok(Info {
        generated_at: value["generatedAt"].as_str().unwrap_or_default().to_string(),
        hash: crate::catalog::hash(text),
        value,
    })
}

fn set(i: Info) -> &'static Info {
    let leaked: &'static Info = Box::leak(Box::new(i));
    *CURRENT.write().unwrap() = Some(leaked);
    leaked
}

fn current() -> &'static Info {
    if let Some(i) = *CURRENT.read().unwrap() {
        return i;
    }
    set(parse(include_str!("../../catalog/info.json")).expect("info.json embebido inválido"))
}

/// Al arrancar: usa la última información descargada si no es más antigua que la embebida.
pub fn init(data_dir: &Path) {
    let embedded = current();
    let Ok(text) = std::fs::read_to_string(data_dir.join(CACHE_FILE)) else { return };
    if let Ok(cached) = parse(&text) {
        if cached.generated_at >= embedded.generated_at {
            set(cached);
        }
    }
}

/// Descarga la información publicada; si cambió, la valida, la guarda y pasa a ser la actual.
pub async fn refresh(data_dir: &Path) -> anyhow::Result<()> {
    let text = crate::http_client()
        .get(format!("{REMOTE_URL}?t={}", crate::store::now()))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if crate::catalog::hash(&text) == current().hash {
        return Ok(());
    }
    let new = parse(&text)?;
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(data_dir.join(CACHE_FILE), &text)?;
    set(new);
    Ok(())
}

pub fn get() -> &'static Value {
    &current().value
}
