//! Guardado en la nube: las partidas de cada juego en un Gist **privado** del usuario ("darsena-saves").
//!
//! - Dónde guarda cada port: el campo `saves` del catálogo (rutas relativas a la carpeta del port, o con
//!   %APPDATA%, %LOCALAPPDATA%, %USERPROFILE% o %DOCUMENTS%) o, si no lo dice, las carpetas "save", "saves",
//!   "savedata" o "saved" que haya dentro del port.
//! - El Gist tiene un índice (darsena-saves.json: cuándo se subió cada juego y desde qué PC) y un archivo por juego
//!   con sus partidas en un .zip en base64.
//! - Antes de restaurar, las partidas de este PC se copian a <port>/.darsena-copias/<fecha>.zip: nunca se pierde nada.

use crate::store::InstalledGame;
use anyhow::Context;
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const INDEX: &str = "darsena-saves.json";
const MAX_ZIP: usize = 25 << 20;

fn api(req: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    req.header("Accept", "application/vnd.github+json").timeout(Duration::from_secs(60)).bearer_auth(token)
}

fn machine() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

// ---------- Dónde están las partidas ----------

/// Raíz de una ruta del catálogo: (etiqueta para el zip, carpeta real).
fn expand(spec: &str, port_dir: &Path) -> Option<(String, PathBuf)> {
    let spec = spec.replace('\\', "/");
    for (var, label) in [("%APPDATA%", "APPDATA"), ("%LOCALAPPDATA%", "LOCALAPPDATA"), ("%USERPROFILE%", "USERPROFILE"), ("%DOCUMENTS%", "DOCUMENTS")] {
        if let Some(rest) = spec.strip_prefix(var) {
            let base = if label == "DOCUMENTS" {
                PathBuf::from(std::env::var("USERPROFILE").ok()?).join("Documents")
            } else {
                PathBuf::from(std::env::var(label).ok()?)
            };
            let rest = rest.trim_start_matches('/');
            return Some((format!("{label}/{rest}"), base.join(rest)));
        }
    }
    let rel = Path::new(&spec);
    (rel.is_relative() && !spec.contains("..")).then(|| (format!("port/{spec}"), port_dir.join(rel)))
}

/// Carpetas de partidas de un port instalado: (etiqueta, carpeta). Solo las que existen.
pub fn save_dirs(inst: &InstalledGame) -> Vec<(String, PathBuf)> {
    let port_dir = PathBuf::from(&inst.path);
    let from_catalog: Vec<String> = crate::catalog::get(&inst.id).map(|g| g.saves.clone()).unwrap_or_default();
    let mut out: Vec<(String, PathBuf)> = from_catalog.iter().filter_map(|s| expand(s, &port_dir)).filter(|(_, p)| p.is_dir()).collect();
    if from_catalog.is_empty() {
        // Sin datos del catálogo: carpetas con nombre de partidas dentro del port (hasta 3 niveles).
        fn walk(dir: &Path, root: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_lowercase();
                if ["save", "saves", "savedata", "saved", "savegames", "savegame"].contains(&name.as_str()) {
                    let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                    out.push((format!("port/{rel}"), p));
                } else if depth < 3 && !name.starts_with('.') {
                    walk(&p, root, depth + 1, out);
                }
            }
        }
        walk(&port_dir, &port_dir, 1, &mut out);
    }
    out
}

fn files_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files_in(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Última modificación de las partidas de un port en este PC.
pub fn local_modified_of(inst: &InstalledGame) -> u64 {
    local_modified(&save_dirs(inst))
}

/// Última modificación (segundos Unix) de las partidas locales; 0 si no hay.
fn local_modified(dirs: &[(String, PathBuf)]) -> u64 {
    let mut files = Vec::new();
    for (_, d) in dirs {
        files_in(d, &mut files);
    }
    files
        .iter()
        .filter_map(|f| f.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs()))
        .max()
        .unwrap_or(0)
}

fn zip_dirs(dirs: &[(String, PathBuf)]) -> anyhow::Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let mut raw = 0usize;
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (label, dir) in dirs {
            let mut files = Vec::new();
            files_in(dir, &mut files);
            for f in files {
                let rel = f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().replace('\\', "/");
                z.start_file(format!("{label}/{rel}"), opts)?;
                let mut data = Vec::new();
                std::fs::File::open(&f)?.read_to_end(&mut data)?;
                z.write_all(&data)?;
                raw += data.len();
                anyhow::ensure!(raw < MAX_ZIP * 8, tr!("las partidas ocupan demasiado para la nube (máximo 25 MB)", "the saves are too big for the cloud (25 MB max)"));
            }
        }
        z.finish()?;
    }
    let zip = buf.into_inner();
    anyhow::ensure!(zip.len() < MAX_ZIP, tr!("las partidas ocupan demasiado para la nube (máximo 25 MB)", "the saves are too big for the cloud (25 MB max)"));
    Ok(zip)
}

// ---------- base64 (sin dependencias) ----------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn b64_decode(text: &str) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = B64.iter().position(|b| *b == c).context("base64 no válido")? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Ok(out)
}

// ---------- Gist ----------

/// (id del Gist, índice). Busca el Gist privado entre los del usuario.
async fn find(token: &str) -> anyhow::Result<Option<(String, Value)>> {
    for page in 1..=5 {
        let list: Vec<Value> = api(crate::http_client().get(format!("https://api.github.com/gists?per_page=100&page={page}")), token)
            .send()
            .await
            .context(tr!("no se pudo conectar con GitHub", "couldn't connect to GitHub"))?
            .error_for_status()?
            .json()
            .await?;
        for g in &list {
            if let Some(f) = g["files"].get(INDEX) {
                let raw = f["raw_url"].as_str().unwrap_or_default();
                let index: Value = crate::http_client().get(raw).bearer_auth(token).send().await?.error_for_status()?.json().await.unwrap_or(json!({}));
                return Ok(Some((g["id"].as_str().unwrap_or_default().to_string(), index)));
            }
        }
        if list.len() < 100 {
            break;
        }
    }
    Ok(None)
}

fn file_of(id: &str) -> String {
    format!("{id}.zip.b64")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStatus {
    /// Carpetas de partidas encontradas en este PC.
    pub dirs: Vec<String>,
    pub local_modified: u64,
    /// Cuándo se subió a la nube y desde qué PC (0 = nunca).
    pub remote_updated: u64,
    pub remote_machine: Option<String>,
    /// Lo último que se sincronizó desde este PC.
    pub last_sync: u64,
    /// "none" (nada en ningún sitio), "synced", "upload" (lo de aquí es más nuevo), "download" (lo de la nube es más
    /// nuevo y aquí no se ha jugado desde la última sincronización) o "conflict" (han cambiado los dos).
    pub action: &'static str,
}

pub async fn status(token: &str, inst: &InstalledGame) -> anyhow::Result<CloudStatus> {
    let dirs = save_dirs(inst);
    let local = local_modified(&dirs);
    let index = find(token).await?.map(|(_, i)| i).unwrap_or(json!({}));
    let entry = &index["games"][&inst.id];
    let remote = entry["updated"].as_u64().unwrap_or(0);
    let last = inst.cloud_synced.unwrap_or(0);
    let action = if remote == 0 && local == 0 {
        "none"
    } else if remote == 0 {
        "upload"
    } else if local == 0 {
        "download"
    } else if last == 0 || (remote > last && local > last) {
        // Nunca se sincronizó desde este PC y hay partidas en los dos sitios, o han cambiado los dos.
        "conflict"
    } else if remote > last {
        "download"
    } else if local > last {
        "upload"
    } else {
        "synced"
    };
    Ok(CloudStatus {
        dirs: dirs.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect(),
        local_modified: local,
        remote_updated: remote,
        remote_machine: entry["machine"].as_str().map(str::to_string),
        last_sync: last,
        action,
    })
}

/// Sube las partidas de este PC. Devuelve la marca de tiempo con la que quedan en la nube.
pub async fn upload(token: &str, inst: &InstalledGame) -> anyhow::Result<u64> {
    let dirs = save_dirs(inst);
    anyhow::ensure!(!dirs.is_empty(), tr!("no se encontraron partidas de este juego", "no saves were found for this game"));
    let modified = local_modified(&dirs);
    let zip = tokio::task::spawn_blocking(move || zip_dirs(&dirs)).await??;
    let now = crate::store::now().max(modified);
    let found = find(token).await?;
    let mut index = found.as_ref().map(|(_, i)| i.clone()).unwrap_or(json!({}));
    if !index["games"].is_object() {
        index["games"] = json!({});
    }
    index["games"][&inst.id] = json!({ "title": inst.title, "updated": now, "machine": machine(), "size": zip.len() });
    let files = json!({
        INDEX: { "content": serde_json::to_string_pretty(&index)? },
        file_of(&inst.id): { "content": b64_encode(&zip) },
    });
    let req = match &found {
        Some((id, _)) => api(crate::http_client().patch(format!("https://api.github.com/gists/{id}")), token).json(&json!({ "files": files })),
        None => api(crate::http_client().post("https://api.github.com/gists"), token).json(&json!({
            "description": "Partidas guardadas de Dársena (privado)",
            "public": false,
            "files": files,
        })),
    };
    req.send().await?.error_for_status().context(tr!("no se pudieron subir las partidas", "couldn't upload the saves"))?;
    Ok(now)
}

/// Copia de seguridad de las partidas de este PC antes de sustituirlas.
fn backup(inst: &InstalledGame, dirs: &[(String, PathBuf)]) -> anyhow::Result<()> {
    if local_modified(dirs) == 0 {
        return Ok(());
    }
    let zip = zip_dirs(dirs)?;
    let dir = PathBuf::from(&inst.path).join(".darsena-copias");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("partidas-{}.zip", crate::store::now())), zip)?;
    Ok(())
}

/// Trae las partidas de la nube (con copia de seguridad de las de aquí). Devuelve su marca de tiempo.
pub async fn restore(token: &str, inst: &InstalledGame) -> anyhow::Result<u64> {
    let (gist, index) = find(token).await?.context(tr!("no tienes partidas en la nube", "you have no saves in the cloud"))?;
    let updated = index["games"][&inst.id]["updated"].as_u64().context(tr!("este juego no tiene partidas en la nube", "this game has no saves in the cloud"))?;
    let g: Value = api(crate::http_client().get(format!("https://api.github.com/gists/{gist}")), token).send().await?.error_for_status()?.json().await?;
    let raw = g["files"][file_of(&inst.id)]["raw_url"].as_str().context("archivo de partidas")?;
    let text = crate::http_client().get(raw).bearer_auth(token).timeout(Duration::from_secs(120)).send().await?.error_for_status()?.text().await?;
    let zip = b64_decode(&text)?;
    let inst = inst.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let port_dir = PathBuf::from(&inst.path);
        backup(&inst, &save_dirs(&inst))?;
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip))?;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            let Some(name) = e.enclosed_name() else { continue };
            let name = name.to_string_lossy().replace('\\', "/");
            // "port/<ruta>" o "APPDATA/<ruta>"…: se vuelve a la carpeta real de este PC.
            let (root, rest) = name.split_once('/').unwrap_or((&name, ""));
            let base = match root {
                "port" => port_dir.clone(),
                "DOCUMENTS" => PathBuf::from(std::env::var("USERPROFILE")?).join("Documents"),
                "APPDATA" | "LOCALAPPDATA" | "USERPROFILE" => PathBuf::from(std::env::var(root)?),
                _ => continue,
            };
            let out = base.join(rest);
            if e.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::io::copy(&mut e, &mut std::fs::File::create(&out)?)?;
        }
        Ok(())
    })
    .await??;
    Ok(updated)
}
