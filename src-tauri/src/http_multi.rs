//! Descarga HTTP rápida: varias conexiones a la vez y, si la fuente da varios mirrors del mismo archivo,
//! trozos de todos ellos en paralelo (como IDM o aria2). Muchos servidores limitan la velocidad por conexión,
//! así que repartir el archivo en trozos multiplica la velocidad.
//!
//! Si ningún mirror admite rangos (`Range`), se baja con una sola conexión, como antes.

use anyhow::Context;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// Estado compartido con la interfaz (progreso) y con pausa/reanudar.
pub struct HttpCtl {
    pub downloaded: AtomicU64,
    pub total: AtomicU64,
    pub paused: AtomicBool,
    /// Conexiones activas y mirrors en uso, para mostrarlo en Descargas.
    pub connections: AtomicUsize,
    pub mirrors: AtomicUsize,
    /// Si se bajan varios archivos seguidos (web seeds): bytes de los anteriores y total de todos (0 = un solo archivo).
    base: AtomicU64,
    grand: AtomicU64,
}

impl HttpCtl {
    pub fn new() -> Self {
        Self {
            downloaded: AtomicU64::new(0),
            total: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            mirrors: AtomicUsize::new(0),
            base: AtomicU64::new(0),
            grand: AtomicU64::new(0),
        }
    }

    /// Empieza un archivo de `file_total` bytes, contando lo ya bajado de los anteriores.
    fn begin_file(&self, file_total: u64, resumed_at: u64) {
        let grand = self.grand.load(Ordering::Relaxed);
        self.total.store(if grand > 0 { grand } else { file_total }, Ordering::Relaxed);
        self.downloaded.store(self.base.load(Ordering::Relaxed) + resumed_at, Ordering::Relaxed);
    }
}

/// Tamaño de cada trozo: pequeño para repartir bien entre conexiones, grande para no hacer miles de peticiones.
const CHUNK: u64 = 4 << 20;
const CONN_PER_MIRROR: usize = 4;
const MAX_CONN: usize = 16;
/// Errores seguidos tras los que una conexión da su mirror por perdido.
const MAX_FAILS: u32 = 4;

pub fn client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("Darsena/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        // Sin datos durante 60 s = conexión colgada: se reintenta ese trozo.
        .read_timeout(Duration::from_secs(60))
        .build()?)
}

pub fn file_name_from_url(uri: &str) -> String {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    let name = path.rsplit('/').next().unwrap_or("");
    let name = crate::sources::sanitize(name);
    if name == "juego" { "descarga.bin".into() } else { name }
}

struct Probe {
    url: String,
    total: u64,
    ranges: bool,
}

/// Pide el primer byte: dice el tamaño total y si el servidor admite descargas por trozos.
async fn probe(client: &reqwest::Client, url: &str) -> anyhow::Result<Probe> {
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, "bytes=0-0")
        .timeout(Duration::from_secs(30))
        .send()
        .await?
        .error_for_status()?;
    if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = resp
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        Ok(Probe { url: url.to_string(), total, ranges: total > 0 })
    } else {
        Ok(Probe { url: url.to_string(), total: resp.content_length().unwrap_or(0), ranges: false })
    }
}

/// Descarga el archivo de `urls` (mirrors del mismo archivo) dentro de `dir`. Devuelve la ruta del archivo.
pub async fn download(urls: &[String], dir: &Path, ctl: &HttpCtl, cancelled: &AtomicBool) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(!urls.is_empty(), tr!("no hay enlaces directos", "there are no direct links"));
    tokio::fs::create_dir_all(dir).await?;
    let file_path = dir.join(file_name_from_url(&urls[0]));
    download_to(urls, &file_path, ctl, cancelled).await?;
    Ok(file_path)
}

/// Baja todos los archivos de un torrent desde sus web seeds (servidores HTTP) y comprueba las piezas.
/// Los archivos quedan donde los dejaría un cliente torrent, dentro de `dir`.
pub async fn download_plan(plan: &crate::webseed::Plan, dir: &Path, ctl: &HttpCtl, cancelled: &AtomicBool) -> anyhow::Result<()> {
    ctl.grand.store(plan.total(), Ordering::Relaxed);
    let mut done = 0;
    for (comps, len) in &plan.files {
        let target = plan.local_path(dir, comps);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if *len == 0 {
            tokio::fs::File::create(&target).await?;
            continue;
        }
        let urls: Vec<String> = plan.bases.iter().map(|b| plan.url(b, comps)).collect();
        ctl.base.store(done, Ordering::Relaxed);
        download_to(&urls, &target, ctl, cancelled).await.with_context(|| tr!("no se pudo bajar {}", "couldn't download {}", comps.join("/")))?;
        done += len;
    }
    ctl.downloaded.store(done, Ordering::Relaxed);
    // Los web seeds no garantizan nada por sí solos: las piezas se comprueban contra los hashes del torrent.
    let (plan, dir) = (plan.clone(), dir.to_path_buf());
    tokio::task::spawn_blocking(move || plan.verify(&dir)).await?
}

async fn download_to(urls: &[String], file_path: &Path, ctl: &HttpCtl, cancelled: &AtomicBool) -> anyhow::Result<()> {
    let client = client()?;
    let file_path = file_path.to_path_buf();

    let probes = futures_util::future::join_all(urls.iter().map(|u| probe(&client, u))).await;
    let mut ok: Vec<Probe> = Vec::new();
    let mut first_err = None;
    for p in probes {
        match p {
            Ok(p) => ok.push(p),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    if ok.is_empty() {
        return Err(first_err.unwrap_or_else(|| anyhow::anyhow!(tr!("ningún enlace respondió", "no link responded"))).context(tr!("no se pudo conectar con los mirrors", "couldn't connect to the mirrors")));
    }
    // Mirrors que sirven por trozos y coinciden en tamaño con el primero de ellos (mismo archivo).
    let total = ok.iter().find(|p| p.ranges).map(|p| p.total).unwrap_or(0);
    let mirrors: Vec<String> = ok.iter().filter(|p| p.ranges && p.total == total).map(|p| p.url.clone()).collect();
    if mirrors.is_empty() || total < 2 * CHUNK {
        // Sin rangos (o archivo pequeño): una sola conexión al primer mirror que respondió.
        ctl.mirrors.store(1, Ordering::Relaxed);
        ctl.connections.store(1, Ordering::Relaxed);
        return single(&client, &ok[0].url, &file_path, ctl, cancelled).await;
    }

    ctl.begin_file(total, 0);
    ctl.mirrors.store(mirrors.len(), Ordering::Relaxed);
    {
        let f = std::fs::File::create(&file_path).with_context(|| tr!("no se pudo crear {}", "couldn't create {}", file_path.display()))?;
        f.set_len(total)?;
    }
    let queue: Mutex<VecDeque<(u64, u64)>> = Mutex::new(
        (0..total).step_by(CHUNK as usize).map(|s| (s, (s + CHUNK).min(total) - 1)).collect(),
    );
    let chunks = queue.lock().unwrap().len();
    let per_mirror = CONN_PER_MIRROR.min((MAX_CONN / mirrors.len()).max(1));
    let workers: Vec<&String> = mirrors.iter().flat_map(|m| std::iter::repeat(m).take(per_mirror)).take(chunks).collect();
    ctl.connections.store(workers.len(), Ordering::Relaxed);

    let last_err: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let in_flight = AtomicUsize::new(0);
    futures_util::future::join_all(workers.into_iter().map(|url| {
        worker(&client, url, &file_path, &queue, &in_flight, ctl, cancelled, &last_err)
    }))
    .await;
    ctl.connections.store(0, Ordering::Relaxed);

    if cancelled.load(Ordering::SeqCst) {
        anyhow::bail!("cancelado");
    }
    if !queue.lock().unwrap().is_empty() {
        let e = last_err.lock().unwrap().take();
        return Err(e.unwrap_or_else(|| anyhow::anyhow!(tr!("error desconocido", "unknown error"))).context(tr!("los mirrors dejaron de responder", "the mirrors stopped responding")));
    }
    Ok(())
}

/// Una conexión: va cogiendo trozos de la cola hasta que se acaben o su mirror falle demasiadas veces.
#[allow(clippy::too_many_arguments)]
async fn worker(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    queue: &Mutex<VecDeque<(u64, u64)>>,
    // Trozos que otras conexiones tienen a medias: si fallan, vuelven a la cola y hay que seguir.
    in_flight: &AtomicUsize,
    ctl: &HttpCtl,
    cancelled: &AtomicBool,
    last_err: &Mutex<Option<anyhow::Error>>,
) {
    let mut fails = 0;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return;
        }
        if ctl.paused.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }
        let next = {
            let mut q = queue.lock().unwrap();
            let n = q.pop_front();
            if n.is_some() {
                in_flight.fetch_add(1, Ordering::SeqCst);
            }
            n
        };
        let Some((start, end)) = next else {
            if in_flight.load(Ordering::SeqCst) == 0 {
                ctl.connections.fetch_sub(1, Ordering::Relaxed);
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        };
        let mut written = 0;
        let r = fetch_range(client, url, path, start, end, ctl, cancelled, &mut written).await;
        // Lo que falte de este trozo (por error, pausa o cancelación) vuelve a la cola.
        {
            let mut q = queue.lock().unwrap();
            if start + written <= end {
                q.push_front((start + written, end));
            }
            in_flight.fetch_sub(1, Ordering::SeqCst);
        }
        match r {
            Ok(()) => fails = 0,
            Err(e) => {
                fails += 1;
                *last_err.lock().unwrap() = Some(e);
                if fails >= MAX_FAILS {
                    ctl.connections.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
                tokio::time::sleep(Duration::from_secs(fails as u64)).await;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_range(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    start: u64,
    end: u64,
    ctl: &HttpCtl,
    cancelled: &AtomicBool,
    written: &mut u64,
) -> anyhow::Result<()> {
    let resp = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
        .send()
        .await?
        .error_for_status()?;
    anyhow::ensure!(resp.status() == reqwest::StatusCode::PARTIAL_CONTENT, tr!("el mirror dejó de admitir descargas por trozos", "the mirror stopped supporting ranged downloads"));
    let mut file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context(tr!("conexión interrumpida", "connection interrupted"))?;
        // Nunca escribir fuera del trozo pedido (un servidor podría mandar de más).
        let room = (end + 1 - start - *written) as usize;
        let data = &chunk[..chunk.len().min(room)];
        file.write_all(data).await?;
        *written += data.len() as u64;
        ctl.downloaded.fetch_add(data.len() as u64, Ordering::Relaxed);
        if *written > end - start || cancelled.load(Ordering::SeqCst) || ctl.paused.load(Ordering::SeqCst) {
            break;
        }
    }
    file.flush().await?;
    Ok(())
}

/// Una sola conexión, reanudando con Range tras una pausa si el servidor lo admite.
async fn single(client: &reqwest::Client, uri: &str, file_path: &Path, ctl: &HttpCtl, cancelled: &AtomicBool) -> anyhow::Result<()> {
    'outer: loop {
        while ctl.paused.load(Ordering::SeqCst) {
            if cancelled.load(Ordering::SeqCst) {
                anyhow::bail!("cancelado");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let have = tokio::fs::metadata(file_path).await.map(|m| m.len()).unwrap_or(0);
        let mut req = client.get(uri);
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let resp = req.send().await?.error_for_status()?;
        let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let base = if resumed { have } else { 0 };
        ctl.begin_file(base + resp.content_length().unwrap_or(0), base);

        let mut file = if resumed {
            tokio::fs::OpenOptions::new().append(true).open(file_path).await?
        } else {
            tokio::fs::File::create(file_path).await?
        };
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context(tr!("conexión interrumpida", "connection interrupted"))?;
            file.write_all(&chunk).await?;
            ctl.downloaded.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            if cancelled.load(Ordering::SeqCst) {
                anyhow::bail!("cancelado");
            }
            if ctl.paused.load(Ordering::SeqCst) {
                file.flush().await?;
                continue 'outer;
            }
        }
        file.flush().await?;
        return Ok(());
    }
}
