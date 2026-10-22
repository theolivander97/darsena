//! Codificador H.264 de baja latencia con Media Foundation.
//!
//! F0 usa el MFT software de Microsoft (síncrono, siempre presente). La conversión BGRA→NV12
//! se hace en CPU. F3: MFT de hardware (NVENC/AMF/QSV) y entrada directa de texturas D3D11.

use super::capture::RawFrame;
use anyhow::{bail, Context, Result};
use windows::core::{Interface, GUID};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VARIANT;

/// Un fotograma H.264 en Annex B listo para empaquetar en RTP.
pub struct Encoded {
    pub data: Vec<u8>,
    pub keyframe: bool,
}

pub struct Encoder {
    mft: IMFTransform,
    codec: Option<ICodecAPI>,
    pub width: u32,
    pub height: u32,
    fps: u32,
    frame_no: i64,
    provides_samples: bool,
    out_size: u32,
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn set_codec_u32(api: &ICodecAPI, key: &GUID, v: u32) {
    // Los parámetros opcionales fallan en algunos codificadores: se ignoran.
    unsafe {
        let _ = api.SetValue(key, &VARIANT::from(v));
    }
}

impl Encoder {
    /// `width` y `height` deben ser pares.
    pub fn new(width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL).context("MFStartup")?;

            let mft: IMFTransform =
                CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER)
                    .context("no hay codificador H.264 en este Windows (¿edición N?)")?;

            // Salida primero: H.264 Baseline (lo decodifica cualquier navegador por hardware).
            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_kbps * 1000)?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))?;
            out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
            mft.SetOutputType(0, &out, 0).context("SetOutputType")?;

            let inp = MFCreateMediaType()?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            inp.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))?;
            inp.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
            inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
            inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            mft.SetInputType(0, &inp, 0).context("SetInputType")?;

            let codec = mft.cast::<ICodecAPI>().ok();
            if let Some(api) = &codec {
                set_codec_u32(api, &CODECAPI_AVLowLatencyMode, 1);
                set_codec_u32(api, &CODECAPI_AVEncCommonRateControlMode, eAVEncCommonRateControlMode_CBR.0 as u32);
                set_codec_u32(api, &CODECAPI_AVEncCommonMeanBitRate, bitrate_kbps * 1000);
                set_codec_u32(api, &CODECAPI_AVEncMPVGOPSize, u32::MAX / 2);
                set_codec_u32(api, &CODECAPI_AVEncMPVDefaultBPictureCount, 0);
            }

            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            let info = mft.GetOutputStreamInfo(0)?;
            Ok(Self {
                mft,
                codec,
                width,
                height,
                fps,
                frame_no: 0,
                provides_samples: info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0,
                out_size: info.cbSize.max(1 << 20),
            })
        }
    }

    pub fn force_keyframe(&self) {
        if let Some(api) = &self.codec {
            set_codec_u32(api, &CODECAPI_AVEncVideoForceKeyFrame, 1);
        }
    }

    /// Convierte BGRA (cualquier tamaño, vecino más cercano) a NV12 del tamaño del codificador.
    pub fn bgra_to_nv12(&self, f: &RawFrame) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let (sw, sh) = (f.width as usize, f.height as usize);
        let mut out = vec![0u8; w * h * 3 / 2];
        let (yp, uv) = out.split_at_mut(w * h);
        for y in 0..h {
            let sy = y * sh / h;
            let srow = &f.bgra[sy * sw * 4..(sy + 1) * sw * 4];
            for x in 0..w {
                let sx = x * sw / w;
                let p = &srow[sx * 4..sx * 4 + 4];
                let (b, g, r) = (p[0] as i32, p[1] as i32, p[2] as i32);
                yp[y * w + x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
                if y % 2 == 0 && x % 2 == 0 {
                    let i = (y / 2) * w + x;
                    uv[i] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                    uv[i + 1] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                }
            }
        }
        out
    }

    /// Codifica un fotograma NV12. Devuelve 0 o más fotogramas H.264.
    pub fn encode(&mut self, nv12: &[u8]) -> Result<Vec<Encoded>> {
        unsafe {
            let buf = MFCreateMemoryBuffer(nv12.len() as u32)?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None)?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), p, nv12.len());
            buf.Unlock()?;
            buf.SetCurrentLength(nv12.len() as u32)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buf)?;
            let dur = 10_000_000i64 / self.fps as i64;
            sample.SetSampleTime(self.frame_no * dur)?;
            sample.SetSampleDuration(dur)?;
            self.frame_no += 1;
            self.mft.ProcessInput(0, &sample, 0)?;

            let mut result = Vec::new();
            loop {
                let own = if self.provides_samples {
                    None
                } else {
                    let s = MFCreateSample()?;
                    s.AddBuffer(&MFCreateMemoryBuffer(self.out_size)?)?;
                    Some(s)
                };
                let mut out = [MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: std::mem::ManuallyDrop::new(own),
                    dwStatus: 0,
                    pEvents: std::mem::ManuallyDrop::new(None),
                }];
                let mut status = 0u32;
                let r = self.mft.ProcessOutput(0, &mut out, &mut status);
                let sample = std::mem::ManuallyDrop::take(&mut out[0].pSample);
                let _events = std::mem::ManuallyDrop::take(&mut out[0].pEvents);
                match r {
                    Ok(()) => {
                        let s = sample.context("el codificador no devolvió muestra")?;
                        let b = s.ConvertToContiguousBuffer()?;
                        let mut p = std::ptr::null_mut();
                        let mut len = 0u32;
                        b.Lock(&mut p, None, Some(&mut len))?;
                        let data = std::slice::from_raw_parts(p, len as usize).to_vec();
                        b.Unlock()?;
                        let keyframe = s.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) == 1;
                        result.push(Encoded { data, keyframe });
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => break,
                    Err(e) => bail!("ProcessOutput: {e}"),
                }
            }
            Ok(result)
        }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = MFShutdown();
        }
    }
}
