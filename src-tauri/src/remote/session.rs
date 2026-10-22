//! Sesión de juego remoto: captura → codificación → difusión a N invitados WebRTC.

use super::{audio, capture, encode, input};
use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use rtc::interceptor::Registry;
use rtc::media::Sample;
use rtc::media_stream::MediaStreamTrack;
use rtc::peer_connection::configuration::media_engine::{MediaEngine, MIME_TYPE_H264, MIME_TYPE_OPUS};
use rtc::peer_connection::configuration::RTCConfigurationBuilder;
use rtc::rtp_transceiver::rtp_sender::{
    RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
    RtpCodecKind,
};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};
use webrtc::media_stream::track_local::static_sample::TrackLocalStaticSample;
use webrtc::media_stream::track_local::TrackLocal;
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::media_stream::Track;
use webrtc::peer_connection::{
    register_default_interceptors, PeerConnection, PeerConnectionBuilder,
    PeerConnectionEventHandler, RTCIceGatheringState, RTCPeerConnectionState,
    RTCSessionDescription,
};

const PAYLOAD_TYPE: u8 = 102;
const AUDIO_PAYLOAD_TYPE: u8 = 111;
const DEFAULT_PORT: u16 = 47800;

#[derive(Serialize, Clone)]
pub struct RemoteInfo {
    pub port: u16,
    pub urls: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub guests: usize,
    pub audio: bool,
}

/// Fotograma codificado que se difunde a todos los invitados.
struct Frame {
    data: Bytes,
    duration: Duration,
}

struct Shared {
    frames: broadcast::Sender<Arc<Frame>>,
    audio: broadcast::Sender<Arc<audio::OpusFrame>>,
    hub: input::Hub,
    audio_ok: AtomicBool,
    next_guest: AtomicU32,
    force_key: AtomicBool,
    guests: AtomicUsize,
    stop: Arc<AtomicBool>,
    ip: String,
}

struct Running {
    shared: Arc<Shared>,
    info: RemoteInfo,
    server: tokio::task::JoinHandle<()>,
}

static CURRENT: Mutex<Option<Running>> = Mutex::new(None);

fn local_ip() -> String {
    // "Conectar" un UDP no envía nada: solo averigua qué interfaz saldría a internet.
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").map(|_| s))
        .and_then(|s| s.local_addr())
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

pub fn status() -> Option<RemoteInfo> {
    let g = CURRENT.lock().unwrap();
    g.as_ref().map(|r| RemoteInfo {
        guests: r.shared.guests.load(Ordering::Relaxed),
        audio: r.shared.audio_ok.load(Ordering::Relaxed),
        ..r.info.clone()
    })
}

fn hub() -> Option<Arc<Shared>> {
    CURRENT.lock().unwrap().as_ref().map(|r| r.shared.clone())
}

pub fn guests() -> Vec<input::GuestView> {
    hub().map(|s| s.hub.views()).unwrap_or_default()
}

pub async fn set_permissions(id: u32, perms: input::Perms) -> Result<()> {
    let s = hub().context("no hay sesión remota")?;
    let g = s.hub.set_permissions(id, perms).context("ese invitado ya no está")?;
    s.hub.notify(&g).await;
    Ok(())
}

pub fn kick(id: u32) {
    if let Some(s) = hub() {
        let g = s.hub.guests.lock().unwrap().get(&id).cloned();
        if let Some(g) = g {
            g.kick.notify_one();
        }
    }
}

pub async fn set_input_paused(paused: bool) {
    if let Some(s) = hub() {
        s.hub.set_paused(paused);
        let all: Vec<_> = s.hub.guests.lock().unwrap().values().cloned().collect();
        for g in all {
            s.hub.notify(&g).await;
        }
    }
}

pub fn stop() {
    if let Some(r) = CURRENT.lock().unwrap().take() {
        let all: Vec<_> = r.shared.hub.guests.lock().unwrap().values().cloned().collect();
        for g in all {
            r.shared.hub.release_all(&g);
        }
        r.shared.stop.store(true, Ordering::Relaxed);
        r.server.abort();
    }
}

/// Empieza a emitir la ventana `hwnd`. Resolución máxima 1280×720 (se escala).
pub async fn start(hwnd: i64, fps: u32, bitrate_kbps: u32) -> Result<RemoteInfo> {
    anyhow::ensure!(CURRENT.lock().unwrap().is_none(), "ya hay una sesión remota activa");
    let fps = fps.clamp(10, 60);
    let (sw, sh) = capture::window_size(hwnd as isize)?;
    anyhow::ensure!(sw > 0 && sh > 0, "la ventana no tiene tamaño");
    let scale = (1280.0 / sw as f64).min(720.0 / sh as f64).min(1.0);
    let (w, h) = (even((sw as f64 * scale) as u32), even((sh as f64 * scale) as u32));

    let stop = Arc::new(AtomicBool::new(false));
    let (ftx, _) = broadcast::channel::<Arc<Frame>>(8);
    let (atx, _) = broadcast::channel::<Arc<audio::OpusFrame>>(32);
    let mut pid = 0u32;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            windows::Win32::Foundation::HWND(hwnd as isize as *mut _),
            Some(&mut pid),
        );
    }
    let shared = Arc::new(Shared {
        frames: ftx,
        audio: atx,
        hub: input::Hub::new(pid),
        audio_ok: AtomicBool::new(true),
        next_guest: AtomicU32::new(1),
        force_key: AtomicBool::new(true),
        guests: AtomicUsize::new(0),
        stop: stop.clone(),
        ip: local_ip(),
    });

    // Hilo de captura → hilo de codificación (cola de 2: si se atrasa, se descartan fotogramas).
    let (raw_tx, raw_rx) = std::sync::mpsc::sync_channel::<capture::RawFrame>(2);
    {
        let stop = stop.clone();
        std::thread::Builder::new().name("remote-capture".into()).spawn(move || {
            if let Err(e) = capture::run(hwnd as isize, fps, stop.clone(), raw_tx) {
                eprintln!("[remote] captura terminada: {e:#}");
                stop.store(true, Ordering::Relaxed);
            }
        })?;
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    {
        let shared = shared.clone();
        std::thread::Builder::new().name("remote-encode".into()).spawn(move || {
            let mut enc = match encode::Encoder::new(w, h, fps, bitrate_kbps) {
                Ok(e) => {
                    let _ = ready_tx.send(Ok(()));
                    e
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("{e:#}")));
                    return;
                }
            };
            let mut last_nv12: Option<Vec<u8>> = None;
            let mut last_out = Instant::now();
            while !shared.stop.load(Ordering::Relaxed) {
                let nv12 = match raw_rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(f) => {
                        let n = enc.bgra_to_nv12(&f);
                        last_nv12 = Some(n.clone());
                        n
                    }
                    // Ventana estática: repetimos el último fotograma para que los invitados nuevos
                    // reciban imagen y el flujo no se pare.
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => match &last_nv12 {
                        Some(n) => n.clone(),
                        None => continue,
                    },
                    Err(_) => break,
                };
                if shared.force_key.swap(false, Ordering::Relaxed) {
                    enc.force_keyframe();
                }
                match enc.encode(&nv12) {
                    Ok(out) => {
                        for o in out {
                            let now = Instant::now();
                            let duration = now - last_out;
                            last_out = now;
                            let _ = shared.frames.send(Arc::new(Frame {
                                data: Bytes::from(o.data),
                                duration: duration.max(Duration::from_millis(1)),
                            }));
                        }
                    }
                    Err(e) => {
                        eprintln!("[remote] error de codificación: {e:#}");
                        break;
                    }
                }
            }
            shared.stop.store(true, Ordering::Relaxed);
        })?;
    }
    ready_rx.recv().context("el codificador no arrancó")?.map_err(anyhow::Error::msg)?;

    // Audio del proceso del juego → Opus. Si falla (Windows antiguo, proceso sin audio…) se sigue sin sonido.
    {
        let shared = shared.clone();
        std::thread::Builder::new().name("remote-audio".into()).spawn(move || {
            let tx = shared.audio.clone();
            if let Err(e) = audio::run(pid, shared.stop.clone(), |f| {
                let _ = tx.send(Arc::new(f));
            }) {
                eprintln!("[remote] sin audio: {e:#}");
                shared.audio_ok.store(false, Ordering::Relaxed);
            }
        })?;
    }

    // Servidor HTTP: página del invitado + señalización.
    let app = Router::new()
        .route("/", get(|| async { Html(include_str!("guest.html")) }))
        .route("/offer", post(offer))
        .with_state(shared.clone());
    let (listener, port) = {
        let mut found = None;
        for p in DEFAULT_PORT..DEFAULT_PORT + 20 {
            if let Ok(l) = tokio::net::TcpListener::bind(("0.0.0.0", p)).await {
                found = Some((l, p));
                break;
            }
        }
        found.context("no hay puertos libres para el servidor remoto")?
    };
    let server = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[remote] servidor HTTP terminado: {e}");
        }
    });

    let info = RemoteInfo {
        port,
        urls: vec![format!("http://{}:{port}/", shared.ip), format!("http://localhost:{port}/")],
        width: w,
        height: h,
        fps,
        bitrate_kbps,
        guests: 0,
        audio: true,
    };
    *CURRENT.lock().unwrap() = Some(Running { shared, info: info.clone(), server });
    Ok(info)
}

// ---------- WebRTC por invitado ----------

#[derive(Clone)]
struct Handler {
    gather_tx: mpsc::UnboundedSender<()>,
    state_tx: mpsc::UnboundedSender<RTCPeerConnectionState>,
    shared: Arc<Shared>,
    guest: Arc<input::Guest>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gather_tx.send(());
        }
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let _ = self.state_tx.send(state);
    }
    async fn on_data_channel(&self, dc: Arc<dyn DataChannel>) {
        let (shared, guest) = (self.shared.clone(), self.guest.clone());
        tokio::spawn(async move {
            let label = dc.label().await.unwrap_or_default();
            while let Some(ev) = dc.poll().await {
                match ev {
                    DataChannelEvent::OnOpen => {
                        if label == "ctl" {
                            *guest.ctl.lock().unwrap() = Some(dc.clone());
                            shared.hub.notify(&guest).await;
                        }
                    }
                    DataChannelEvent::OnMessage(m) => {
                        let Ok(msg) = serde_json::from_slice::<input::Msg>(&m.data) else { continue };
                        if shared.hub.handle(&guest, msg) {
                            shared.hub.notify(&guest).await;
                        }
                    }
                    DataChannelEvent::OnClose => break,
                    _ => {}
                }
            }
        });
    }
}

async fn offer(
    State(shared): State<Arc<Shared>>,
    Json(offer): Json<RTCSessionDescription>,
) -> impl IntoResponse {
    match handle_offer(shared, offer).await {
        Ok(answer) => Json(answer).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
    }
}

fn new_track(codec: &RTCRtpCodec, kind: RtpCodecKind, ssrc: u32, name: &str) -> Result<Arc<TrackLocalStaticSample>> {
    Ok(Arc::new(TrackLocalStaticSample::new(
        Instant::now(),
        MediaStreamTrack::new(
            "darsena-stream".into(),
            format!("darsena-{name}"),
            format!("darsena-{name}"),
            kind,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters { ssrc: Some(ssrc), ..Default::default() },
                codec: codec.clone(),
                ..Default::default()
            }],
        ),
    )?))
}

async fn negotiated_pt(sender: &Arc<dyn webrtc::rtp_transceiver::RtpSender>, default: u8) -> u8 {
    match sender.get_parameters().await {
        Ok(p) => p.rtp_parameters.codecs.first().map(|c| c.payload_type).unwrap_or(default),
        Err(_) => default,
    }
}

async fn handle_offer(shared: Arc<Shared>, offer: RTCSessionDescription) -> Result<RTCSessionDescription> {
    let vcodec = RTCRtpCodec {
        mime_type: MIME_TYPE_H264.to_owned(),
        clock_rate: 90000,
        channels: 0,
        sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".to_owned(),
        rtcp_feedback: vec![],
    };
    let acodec = RTCRtpCodec {
        mime_type: MIME_TYPE_OPUS.to_owned(),
        clock_rate: 48000,
        channels: 2,
        sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
        rtcp_feedback: vec![],
    };
    let mut media = MediaEngine::default();
    media.register_codec(
        RTCRtpCodecParameters { rtp_codec: vcodec.clone(), payload_type: PAYLOAD_TYPE, ..Default::default() },
        RtpCodecKind::Video,
    )?;
    media.register_codec(
        RTCRtpCodecParameters { rtp_codec: acodec.clone(), payload_type: AUDIO_PAYLOAD_TYPE, ..Default::default() },
        RtpCodecKind::Audio,
    )?;
    let registry = register_default_interceptors(Registry::new(), &mut media)?;

    let guest = Arc::new(input::Guest::new(shared.next_guest.fetch_add(1, Ordering::Relaxed)));
    let (gather_tx, mut gather_rx) = mpsc::unbounded_channel();
    let (state_tx, mut state_rx) = mpsc::unbounded_channel();
    let pc = PeerConnectionBuilder::new()
        .with_configuration(RTCConfigurationBuilder::new().build()) // LAN: sin STUN/TURN
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_handler(Arc::new(Handler {
            gather_tx,
            state_tx,
            shared: shared.clone(),
            guest: guest.clone(),
        }))
        .with_udp_addrs(vec![format!("{}:0", shared.ip)])
        .build()
        .await?;

    let vssrc = rand::random::<u32>();
    let assrc = rand::random::<u32>();
    let vtrack = new_track(&vcodec, RtpCodecKind::Video, vssrc, "video")?;
    let atrack = new_track(&acodec, RtpCodecKind::Audio, assrc, "audio")?;
    let vsender = pc.add_track(vtrack.clone() as Arc<dyn TrackLocal>).await?;
    let asender = pc.add_track(atrack.clone() as Arc<dyn TrackLocal>).await?;

    pc.set_remote_description(offer).await?;
    let answer = pc.create_answer(None).await?;
    pc.set_local_description(answer).await?;
    // ICE sin trickle: esperamos a tener todos los candidatos locales (en LAN es instantáneo).
    let _ = tokio::time::timeout(Duration::from_secs(5), gather_rx.recv()).await;
    let local = pc.local_description().await.context("sin descripción local")?;

    tokio::spawn(async move {
        // Esperar a conectar; mientras, `pc` se mantiene vivo en esta tarea.
        let connected = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(s) = state_rx.recv().await {
                if s == RTCPeerConnectionState::Connected {
                    return true;
                }
                if matches!(s, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
                    return false;
                }
            }
            false
        })
        .await
        .unwrap_or(false);
        if connected {
            shared.guests.fetch_add(1, Ordering::Relaxed);
            shared.hub.guests.lock().unwrap().insert(guest.id, guest.clone());
            let vpt = negotiated_pt(&vsender, PAYLOAD_TYPE).await;
            let apt = negotiated_pt(&asender, AUDIO_PAYLOAD_TYPE).await;
            let vssrc = vtrack.ssrcs().await.first().copied().unwrap_or(vssrc);
            let assrc = atrack.ssrcs().await.first().copied().unwrap_or(assrc);
            let mut frames = shared.frames.subscribe();
            let mut aframes = shared.audio.subscribe();
            shared.force_key.store(true, Ordering::Relaxed); // el invitado nuevo necesita un IDR
            let mut last_key_req = Instant::now();
            loop {
                tokio::select! {
                    f = frames.recv() => match f {
                        Ok(f) => {
                            let sample = Sample {
                                data: f.data.clone(),
                                duration: f.duration,
                                ..Sample::new(Instant::now())
                            };
                            if vtrack.sample_writer(vssrc, vpt).write_sample(&sample).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // Nos quedamos atrás: pedimos un IDR para resincronizar.
                            shared.force_key.store(true, Ordering::Relaxed);
                        }
                        Err(_) => break,
                    },
                    a = aframes.recv() => {
                        if let Ok(a) = a {
                            let sample = Sample {
                                data: Bytes::from(a.data.clone()),
                                duration: Duration::from_millis(20),
                                ..Sample::new(Instant::now())
                            };
                            let _ = atrack.sample_writer(assrc, apt).write_sample(&sample).await;
                        }
                    },
                    s = state_rx.recv() => match s {
                        Some(RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed
                            | RTCPeerConnectionState::Disconnected) | None => break,
                        _ => {}
                    },
                    _ = guest.kick.notified() => break,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                }
                if shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                // Prototipo: sin lectura de PLI/NACK, así que un IDR cada 2 s repara pérdidas.
                if last_key_req.elapsed() > Duration::from_secs(2) {
                    shared.force_key.store(true, Ordering::Relaxed);
                    last_key_req = Instant::now();
                }
            }
            shared.hub.release_all(&guest);
            shared.hub.guests.lock().unwrap().remove(&guest.id);
            shared.guests.fetch_sub(1, Ordering::Relaxed);
        }
        let _ = pc.close().await;
    });

    Ok(local)
}
