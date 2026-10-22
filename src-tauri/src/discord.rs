//! Discord Rich Presence: «Jugando a X» en el perfil de Discord mientras hay un juego abierto.
//! Habla con Discord por su tubería local (IPC), sin dependencias. Si Discord no está abierto no pasa nada.

use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

/// Id de la aplicación «Dársena» creada en discord.com/developers (es público, no es un secreto).
/// Vacío = la función está desactivada.
const APP_ID: &str = "";

/// Qué mostrar: título del juego y cuándo empezó (segundos Unix), o nada.
type Activity = Option<(String, u64)>;

static TX: OnceLock<Sender<Activity>> = OnceLock::new();

/// Muestra (o quita, con None) la actividad. Se llama al abrir y al cerrar un juego.
pub fn set(enabled: bool, game: Option<&str>) {
    if APP_ID.is_empty() {
        return;
    }
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<Activity>();
        std::thread::spawn(move || {
            let mut conn: Option<std::fs::File> = None;
            while let Ok(mut act) = rx.recv() {
                // Si llegaron más cambios mientras tanto, solo importa el último.
                while let Ok(newer) = rx.try_recv() {
                    act = newer;
                }
                for _ in 0..2 {
                    if conn.is_none() {
                        conn = connect();
                    }
                    let Some(f) = conn.as_mut() else { break };
                    if send_activity(f, &act).is_ok() {
                        break;
                    }
                    conn = None;
                }
            }
        });
        tx
    });
    let act = if enabled {
        game.map(|g| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            (g.to_string(), now)
        })
    } else {
        None
    };
    let _ = tx.send(act);
}

#[cfg(windows)]
fn connect() -> Option<std::fs::File> {
    for i in 0..10 {
        let Ok(mut f) = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{i}")) else { continue };
        let hello = serde_json::json!({ "v": 1, "client_id": APP_ID }).to_string();
        if write_frame(&mut f, 0, &hello).is_ok() && read_frame(&mut f).is_ok() {
            return Some(f);
        }
    }
    None
}

#[cfg(not(windows))]
fn connect() -> Option<std::fs::File> {
    None
}

fn send_activity(f: &mut std::fs::File, act: &Activity) -> std::io::Result<()> {
    let activity = act.as_ref().map(|(title, start)| {
        serde_json::json!({
            "details": tr!("Jugando a {}", "Playing {}", title),
            "state": "Dársena",
            "timestamps": { "start": start },
            "assets": { "large_image": "darsena", "large_text": "Dársena" },
        })
    });
    let msg = serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": std::process::id(), "activity": activity },
        "nonce": format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)),
    });
    write_frame(f, 1, &msg.to_string())?;
    read_frame(f).map(|_| ())
}

fn write_frame(f: &mut std::fs::File, op: u32, json: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut buf = Vec::with_capacity(8 + json.len());
    buf.extend_from_slice(&op.to_le_bytes());
    buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
    buf.extend_from_slice(json.as_bytes());
    f.write_all(&buf)
}

fn read_frame(f: &mut std::fs::File) -> std::io::Result<String> {
    use std::io::Read;
    let mut head = [0u8; 8];
    f.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::other("respuesta demasiado grande"));
    }
    let mut body = vec![0u8; len];
    f.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}
