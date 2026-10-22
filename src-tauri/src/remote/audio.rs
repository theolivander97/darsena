//! Audio del juego: WASAPI loopback por proceso (Windows 10 2004+) → Opus 48 kHz estéreo, tramas de 20 ms.
//! Solo se captura el audio del proceso del juego (y su árbol), no Discord ni notificaciones.

use anyhow::{bail, Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{CoInitializeEx, BLOB, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

const RATE: u32 = 48_000;
const CHANNELS: u32 = 2;
const FRAME_SAMPLES: usize = 960; // 20 ms por canal

/// Una trama Opus de 20 ms.
pub struct OpusFrame {
    pub data: Vec<u8>,
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Done(mpsc::Sender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Done_Impl {
    fn ActivateCompleted(
        &self,
        _op: windows::core::Ref<'_, IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
}

fn activate_process_loopback(pid: u32) -> Result<IAudioClient> {
    unsafe {
        let mut params = AUDIOCLIENT_ACTIVATION_PARAMS::default();
        params.ActivationType = AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK;
        params.Anonymous.ProcessLoopbackParams = AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
            TargetProcessId: pid,
            ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
        };
        // PROPVARIANT de tipo BLOB que apunta a `params`. No se libera con PropVariantClear.
        let pv = std::mem::ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: &mut params as *mut _ as *mut u8,
                        },
                    },
                }),
            },
        });

        let (tx, rx) = mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Done(tx).into();
        let op = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*pv as *const PROPVARIANT),
            &handler,
        )
        .context("ActivateAudioInterfaceAsync")?;
        rx.recv_timeout(Duration::from_secs(5)).context("la activación del audio no respondió")?;
        let mut hr = windows::core::HRESULT(0);
        let mut unk: Option<windows::core::IUnknown> = None;
        op.GetActivateResult(&mut hr, &mut unk)?;
        hr.ok().context("el audio por proceso no está disponible")?;
        Ok(unk.context("sin IAudioClient")?.cast::<IAudioClient>()?)
    }
}

/// Bucle de captura de audio; bloquea hasta que `stop` sea true. Cada 20 ms entrega una trama Opus
/// (silencio incluido, para que el reloj RTP no se descuadre).
pub fn run(pid: u32, stop: Arc<AtomicBool>, mut out: impl FnMut(OpusFrame)) -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let client = activate_process_loopback(pid)?;

        let fmt = WAVEFORMATEX {
            wFormatTag: 1, // WAVE_FORMAT_PCM
            nChannels: CHANNELS as u16,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * CHANNELS * 2,
            nBlockAlign: (CHANNELS * 2) as u16,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                2_000_000, // 200 ms
                0,
                &fmt,
                None,
            )
            .context("Initialize del audio")?;
        let event: HANDLE = CreateEventW(None, false, false, PCWSTR::null())?;
        client.SetEventHandle(event)?;
        let cap: IAudioCaptureClient = client.GetService()?;
        client.Start()?;

        let mut enc = opus::Encoder::new(RATE, opus::Channels::Stereo, opus::Application::LowDelay)
            .map_err(|e| anyhow::anyhow!("Opus: {e}"))?;
        let _ = enc.set_bitrate(opus::Bitrate::Bits(96_000));
        let _ = enc.set_inband_fec(true);

        let mut pending: Vec<i16> = Vec::with_capacity(FRAME_SAMPLES * 2 * 8);
        let mut next = Instant::now() + Duration::from_millis(20);
        let mut buf = vec![0u8; 4000];
        let result = (|| -> Result<()> {
            while !stop.load(Ordering::Relaxed) {
                let wait = next.saturating_duration_since(Instant::now()).as_millis() as u32;
                let _ = WaitForSingleObject(event, wait.max(1)) == WAIT_OBJECT_0;
                // Vaciar todo lo disponible.
                loop {
                    let n = cap.GetNextPacketSize()?;
                    if n == 0 {
                        break;
                    }
                    let mut data = std::ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    cap.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let samples = frames as usize * CHANNELS as usize;
                    if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                        pending.extend(std::iter::repeat(0i16).take(samples));
                    } else {
                        pending.extend_from_slice(std::slice::from_raw_parts(data as *const i16, samples));
                    }
                    cap.ReleaseBuffer(frames)?;
                }
                // Reloj de 20 ms: una trama por tic; si falta audio se rellena con silencio.
                let now = Instant::now();
                while now >= next {
                    let need = FRAME_SAMPLES * CHANNELS as usize;
                    let mut frame: Vec<i16> = if pending.len() >= need {
                        pending.drain(..need).collect()
                    } else {
                        let mut f: Vec<i16> = pending.drain(..).collect();
                        f.resize(need, 0);
                        f
                    };
                    // Si nos atrasamos mucho (>100 ms acumulados) descartamos para no sumar latencia.
                    if pending.len() > need * 5 {
                        pending.drain(..pending.len() - need * 2);
                    }
                    let n = enc.encode(&frame, &mut buf).map_err(|e| anyhow::anyhow!("Opus: {e}"))?;
                    out(OpusFrame { data: buf[..n].to_vec() });
                    frame.clear();
                    next += Duration::from_millis(20);
                }
            }
            Ok(())
        })();
        let _ = client.Stop();
        let _ = CloseHandle(event);
        if let Err(e) = &result {
            bail!("{e:#}");
        }
        Ok(())
    }
}
