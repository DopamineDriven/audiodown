#![deny(clippy::all)]

//! `@d0paminedriven/audiodown`: MP3/WAV metadata, Symphonia PCM decoding and
//! rayon waveform envelopes for Node.
//!
//! Layout follows pdfdown: this file is the Node-API surface (functions,
//! promise-returning async variants, classes), `types.rs` holds the JS
//! boundary types and `core/` is pure Rust.
//!
//! Every `*Async` function and method returns an [`AsyncBlock`]: the input is
//! snapshotted synchronously on the JS thread, the CPU work runs on tokio's
//! blocking pool via [`spawn_blocking`], and the result is converted to JS
//! values back on the JS thread when the promise resolves. Nothing blocks the
//! event loop, and nothing competes with Node's libuv pool.

use std::sync::Arc;

use napi::Env;
use napi::bindgen_prelude::*;
use napi_derive::napi;

mod core;
mod types;

pub use types::{
  AudioAnalysis, AudioContainer, AudioKind, AudioSpecs, AudioTags, DecodeOptions, DecodedAudio,
  DurationSource, Id3v2Entity, MpegChannelMode, MpegFrame, MpegLayer, MpegVersion, WaveformChannel,
  WaveformOptions, WaveformPeaks, XingHeader, XingKind,
};

use crate::core::decode::{self, DecodeConfig, Pcm};
use crate::core::metadata::{self as meta, AudioMeta};
use crate::core::waveform::{self, Resolution, Waveform, WaveformConfig};

// ── Errors ──────────────────────────────────────────────────────

fn fail(message: impl Into<String>) -> Error {
  Error::new(Status::GenericFailure, message.into())
}

fn invalid(message: impl Into<String>) -> Error {
  Error::new(Status::InvalidArg, message.into())
}

/// A JS number that must be an integer in `1..=max`. Rejects `1.5`, `NaN`
/// and `Infinity` instead of silently truncating them.
fn integer(value: f64, name: &str, max: usize) -> Result<usize> {
  if !value.is_finite() || value.fract() != 0.0 || value < 1.0 || value > max as f64 {
    return Err(invalid(format!("{name} must be an integer in 1..={max}")));
  }
  Ok(value as usize)
}

// ── Promises ────────────────────────────────────────────────────

/// Run CPU-bound `work` on tokio's blocking pool and resolve a JS promise
/// with it. `map` runs on the JS thread once the work finishes, which is where
/// typed arrays and class instances have to be created.
fn promise<V, T, W, M>(env: &Env, work: W, map: M) -> Result<AsyncBlock<T>>
where
  V: Send + 'static,
  T: ToNapiValue + 'static,
  W: FnOnce() -> Result<V> + Send + 'static,
  M: FnOnce(Env, V) -> Result<T> + 'static,
{
  AsyncBlockBuilder::build_with_map(
    env,
    async move {
      spawn_blocking(work)
        .await
        .map_err(|e| fail(format!("audiodown worker failed: {e}")))?
    },
    map,
  )
}

// ── Option resolution ───────────────────────────────────────────

fn decode_config(o: &DecodeOptions) -> Result<DecodeConfig> {
  Ok(DecodeConfig {
    gapless: o.gapless.unwrap_or(true),
    strict: o.strict.unwrap_or(true),
    max_decoded_samples: integer(
      o.max_decoded_samples
        .unwrap_or(decode::DEFAULT_MAX_DECODED_SAMPLES as f64),
      "maxDecodedSamples",
      u32::MAX as usize,
    )?,
  })
}

fn waveform_configs(o: &WaveformOptions) -> Result<(DecodeConfig, WaveformConfig)> {
  if o.peak_count.is_some() && o.samples_per_peak.is_some() {
    return Err(invalid("pass either peakCount or samplesPerPeak, not both"));
  }
  let resolution = if let Some(step) = o.samples_per_peak {
    Resolution::SamplesPerPeak(integer(step, "samplesPerPeak", u32::MAX as usize)?)
  } else {
    Resolution::PeakCount(integer(
      o.peak_count.unwrap_or(1024.0),
      "peakCount",
      waveform::MAX_PEAK_COUNT,
    )?)
  };
  Ok((
    decode_config(&o.decode_options())?,
    WaveformConfig {
      resolution,
      parallel: o.parallel.unwrap_or(true),
    },
  ))
}

// ── Core wrappers ───────────────────────────────────────────────

fn snapshot(data: &[u8]) -> Arc<Vec<u8>> {
  Arc::new(data.to_vec())
}

fn parse_kind(data: &[u8], kind: Option<meta::AudioKind>) -> Result<AudioMeta> {
  match kind {
    Some(meta::AudioKind::Mp3) => meta::parse_mp3(data).map(AudioMeta::Mp3),
    Some(meta::AudioKind::Wav) => meta::parse_wav(data).map(AudioMeta::Wav),
    None => meta::parse_audio(data),
  }
  .map_err(fail)
}

fn parse_specs(
  data: &[u8],
  kind: Option<meta::AudioKind>,
  source: Option<String>,
) -> Result<AudioSpecs> {
  Ok(types::audio_specs(
    parse_kind(data, kind)?,
    data.len(),
    source,
  ))
}

/// Promise flavour of [`parse_specs`] over a shared snapshot.
fn parse_specs_async(
  env: &Env,
  data: Arc<Vec<u8>>,
  kind: Option<meta::AudioKind>,
  source: Option<String>,
) -> Result<AsyncBlock<AudioSpecs>> {
  let len = data.len();
  promise(
    env,
    move || parse_kind(&data, kind),
    move |_, meta| Ok(types::audio_specs(meta, len, source)),
  )
}

fn decode_pcm(data: Arc<Vec<u8>>, config: DecodeConfig) -> Result<Pcm> {
  decode::decode(data, config).map_err(fail)
}

fn decode_pcm_async(
  env: &Env,
  data: Arc<Vec<u8>>,
  config: DecodeConfig,
) -> Result<AsyncBlock<DecodedAudio>> {
  promise(
    env,
    move || decode_pcm(data, config),
    |_, pcm| Ok(pcm.into()),
  )
}

fn reduce(pcm: &Pcm, config: WaveformConfig) -> Result<Waveform> {
  waveform::peaks(pcm, config).map_err(fail)
}

fn reduce_async(
  env: &Env,
  pcm: Arc<Pcm>,
  config: WaveformConfig,
) -> Result<AsyncBlock<WaveformPeaks>> {
  promise(env, move || reduce(&pcm, config), |_, wave| Ok(wave.into()))
}

fn decode_and_reduce(data: Arc<Vec<u8>>, dc: DecodeConfig, wc: WaveformConfig) -> Result<Waveform> {
  reduce(&decode_pcm(data, dc)?, wc)
}

fn decode_and_reduce_async(
  env: &Env,
  data: Arc<Vec<u8>>,
  dc: DecodeConfig,
  wc: WaveformConfig,
) -> Result<AsyncBlock<WaveformPeaks>> {
  promise(
    env,
    move || decode_and_reduce(data, dc, wc),
    |_, wave| Ok(wave.into()),
  )
}

fn analyze(
  data: Arc<Vec<u8>>,
  dc: DecodeConfig,
  wc: WaveformConfig,
  source: Option<String>,
) -> Result<AudioAnalysis> {
  let len = data.len();
  let (meta, wave) = core::analyze(data, dc, wc).map_err(fail)?;
  Ok(AudioAnalysis {
    specs: types::audio_specs(meta, len, source),
    waveform: wave.into(),
  })
}

fn analyze_async(
  env: &Env,
  data: Arc<Vec<u8>>,
  dc: DecodeConfig,
  wc: WaveformConfig,
  source: Option<String>,
) -> Result<AsyncBlock<AudioAnalysis>> {
  let len = data.len();
  promise(
    env,
    move || core::analyze(data, dc, wc).map_err(fail),
    move |_, (meta, wave)| {
      Ok(AudioAnalysis {
        specs: types::audio_specs(meta, len, source),
        waveform: wave.into(),
      })
    },
  )
}

fn pcm_from_js(samples: &[f32], sample_rate: f64, channels: f64, max: usize) -> Result<Pcm> {
  let sample_rate = integer(sample_rate, "sampleRate", u32::MAX as usize)? as u32;
  let channels = integer(channels, "channels", 64)?;
  if samples.len() > max {
    return Err(invalid("PCM exceeds maxDecodedSamples"));
  }
  if samples.is_empty() || !samples.len().is_multiple_of(channels) {
    return Err(invalid("PCM must contain complete sample frames"));
  }
  Ok(Pcm {
    samples: samples.to_vec(),
    sample_rate,
    channels,
    gapless_enabled: false,
    skipped_packets: 0,
  })
}

// ── Standalone functions ────────────────────────────────────────

/// Cheap magic sniff: `'mp3'`, `'wav'` or `null`.
#[napi]
pub fn sniff_audio(data: &[u8]) -> Option<AudioKind> {
  meta::sniff(data).map(Into::into)
}

#[napi]
pub fn parse_audio(data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
  parse_specs(data, None, source)
}

#[napi]
pub fn parse_audio_async(
  env: &Env,
  data: &[u8],
  source: Option<String>,
) -> Result<AsyncBlock<AudioSpecs>> {
  parse_specs_async(env, snapshot(data), None, source)
}

#[napi(ts_return_type = "Mp3Meta")]
pub fn parse_mp3(data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
  parse_specs(data, Some(meta::AudioKind::Mp3), source)
}

#[napi(ts_return_type = "Promise<Mp3Meta>")]
pub fn parse_mp3_async(
  env: &Env,
  data: &[u8],
  source: Option<String>,
) -> Result<AsyncBlock<AudioSpecs>> {
  parse_specs_async(env, snapshot(data), Some(meta::AudioKind::Mp3), source)
}

#[napi(ts_return_type = "WavMeta")]
pub fn parse_wav(data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
  parse_specs(data, Some(meta::AudioKind::Wav), source)
}

#[napi(ts_return_type = "Promise<WavMeta>")]
pub fn parse_wav_async(
  env: &Env,
  data: &[u8],
  source: Option<String>,
) -> Result<AsyncBlock<AudioSpecs>> {
  parse_specs_async(env, snapshot(data), Some(meta::AudioKind::Wav), source)
}

#[napi]
pub fn decode_audio(data: &[u8], options: Option<DecodeOptions>) -> Result<DecodedAudio> {
  let config = decode_config(&options.unwrap_or_default())?;
  decode_pcm(snapshot(data), config).map(Into::into)
}

#[napi]
pub fn decode_audio_async(
  env: &Env,
  data: &[u8],
  options: Option<DecodeOptions>,
) -> Result<AsyncBlock<DecodedAudio>> {
  let config = decode_config(&options.unwrap_or_default())?;
  decode_pcm_async(env, snapshot(data), config)
}

#[napi]
pub fn waveform_peaks(data: &[u8], options: Option<WaveformOptions>) -> Result<WaveformPeaks> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  decode_and_reduce(snapshot(data), dc, wc).map(Into::into)
}

#[napi]
pub fn waveform_peaks_async(
  env: &Env,
  data: &[u8],
  options: Option<WaveformOptions>,
) -> Result<AsyncBlock<WaveformPeaks>> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  decode_and_reduce_async(env, snapshot(data), dc, wc)
}

#[napi]
pub fn waveform_from_pcm(
  samples: &[f32],
  sample_rate: f64,
  channels: f64,
  options: Option<WaveformOptions>,
) -> Result<WaveformPeaks> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  let pcm = pcm_from_js(samples, sample_rate, channels, dc.max_decoded_samples)?;
  reduce(&pcm, wc).map(Into::into)
}

#[napi]
pub fn waveform_from_pcm_async(
  env: &Env,
  samples: &[f32],
  sample_rate: f64,
  channels: f64,
  options: Option<WaveformOptions>,
) -> Result<AsyncBlock<WaveformPeaks>> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  let pcm = pcm_from_js(samples, sample_rate, channels, dc.max_decoded_samples)?;
  reduce_async(env, Arc::new(pcm), wc)
}

#[napi]
pub fn analyze_audio(
  data: &[u8],
  options: Option<WaveformOptions>,
  source: Option<String>,
) -> Result<AudioAnalysis> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  analyze(snapshot(data), dc, wc, source)
}

#[napi]
pub fn analyze_audio_async(
  env: &Env,
  data: &[u8],
  options: Option<WaveformOptions>,
  source: Option<String>,
) -> Result<AsyncBlock<AudioAnalysis>> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  analyze_async(env, snapshot(data), dc, wc, source)
}

// ── AudioService: stateless service, same shape as the TS package ──

/// Stateless service. Construct once, call many; every method is also
/// available as a standalone function. Optional constructor defaults are
/// merged under per-call options.
#[napi]
pub struct AudioService {
  defaults: WaveformOptions,
}

#[napi]
impl AudioService {
  #[napi(constructor)]
  pub fn new(defaults: Option<WaveformOptions>) -> Result<Self> {
    let defaults = defaults.unwrap_or_default();
    waveform_configs(&defaults)?;
    Ok(AudioService { defaults })
  }

  #[napi(getter)]
  pub fn defaults(&self) -> WaveformOptions {
    self.defaults.clone()
  }

  #[napi]
  pub fn sniff_audio(&self, data: &[u8]) -> Option<AudioKind> {
    sniff_audio(data)
  }

  #[napi]
  pub fn parse_audio(&self, data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
    parse_audio(data, source)
  }

  #[napi]
  pub fn parse_audio_async(
    &self,
    env: &Env,
    data: &[u8],
    source: Option<String>,
  ) -> Result<AsyncBlock<AudioSpecs>> {
    parse_audio_async(env, data, source)
  }

  #[napi(ts_return_type = "Mp3Meta")]
  pub fn parse_mp3(&self, data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
    parse_mp3(data, source)
  }

  #[napi(ts_return_type = "Promise<Mp3Meta>")]
  pub fn parse_mp3_async(
    &self,
    env: &Env,
    data: &[u8],
    source: Option<String>,
  ) -> Result<AsyncBlock<AudioSpecs>> {
    parse_mp3_async(env, data, source)
  }

  #[napi(ts_return_type = "WavMeta")]
  pub fn parse_wav(&self, data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
    parse_wav(data, source)
  }

  #[napi(ts_return_type = "Promise<WavMeta>")]
  pub fn parse_wav_async(
    &self,
    env: &Env,
    data: &[u8],
    source: Option<String>,
  ) -> Result<AsyncBlock<AudioSpecs>> {
    parse_wav_async(env, data, source)
  }

  #[napi]
  pub fn decode_audio(&self, data: &[u8], options: Option<DecodeOptions>) -> Result<DecodedAudio> {
    decode_audio(data, Some(self.defaults.merged_decode(options)))
  }

  #[napi]
  pub fn decode_audio_async(
    &self,
    env: &Env,
    data: &[u8],
    options: Option<DecodeOptions>,
  ) -> Result<AsyncBlock<DecodedAudio>> {
    decode_audio_async(env, data, Some(self.defaults.merged_decode(options)))
  }

  #[napi]
  pub fn waveform_peaks(
    &self,
    data: &[u8],
    options: Option<WaveformOptions>,
  ) -> Result<WaveformPeaks> {
    waveform_peaks(data, Some(self.defaults.merged(options)))
  }

  #[napi]
  pub fn waveform_peaks_async(
    &self,
    env: &Env,
    data: &[u8],
    options: Option<WaveformOptions>,
  ) -> Result<AsyncBlock<WaveformPeaks>> {
    waveform_peaks_async(env, data, Some(self.defaults.merged(options)))
  }

  #[napi]
  pub fn waveform_from_pcm(
    &self,
    samples: &[f32],
    sample_rate: f64,
    channels: f64,
    options: Option<WaveformOptions>,
  ) -> Result<WaveformPeaks> {
    waveform_from_pcm(
      samples,
      sample_rate,
      channels,
      Some(self.defaults.merged(options)),
    )
  }

  #[napi]
  pub fn waveform_from_pcm_async(
    &self,
    env: &Env,
    samples: &[f32],
    sample_rate: f64,
    channels: f64,
    options: Option<WaveformOptions>,
  ) -> Result<AsyncBlock<WaveformPeaks>> {
    waveform_from_pcm_async(
      env,
      samples,
      sample_rate,
      channels,
      Some(self.defaults.merged(options)),
    )
  }

  #[napi]
  pub fn analyze_audio(
    &self,
    data: &[u8],
    options: Option<WaveformOptions>,
    source: Option<String>,
  ) -> Result<AudioAnalysis> {
    analyze_audio(data, Some(self.defaults.merged(options)), source)
  }

  #[napi]
  pub fn analyze_audio_async(
    &self,
    env: &Env,
    data: &[u8],
    options: Option<WaveformOptions>,
    source: Option<String>,
  ) -> Result<AsyncBlock<AudioAnalysis>> {
    analyze_audio_async(env, data, Some(self.defaults.merged(options)), source)
  }

  /// Snapshot a buffer once and reuse it across metadata, decode, waveform
  /// and analyze calls. Inherits this service's defaults.
  #[napi]
  pub fn open(&self, data: &[u8], source: Option<String>) -> Result<AudioDown> {
    AudioDown::from_bytes(snapshot(data), source, self.defaults.clone())
  }
}

// ── AudioDown: parse once, call many (PdfDown-style handle) ─────

/// One audio buffer, copied exactly once into shared memory. Sync methods
/// borrow it; async methods share it with the worker through `Arc`.
#[napi]
pub struct AudioDown {
  data: Arc<Vec<u8>>,
  kind: meta::AudioKind,
  source: Option<String>,
  defaults: WaveformOptions,
}

impl AudioDown {
  fn from_bytes(
    data: Arc<Vec<u8>>,
    source: Option<String>,
    defaults: WaveformOptions,
  ) -> Result<Self> {
    let kind = meta::sniff(&data).ok_or_else(|| fail("unrecognized audio magic"))?;
    Ok(AudioDown {
      data,
      kind,
      source,
      defaults,
    })
  }
}

#[napi]
impl AudioDown {
  #[napi(constructor)]
  pub fn new(data: &[u8], source: Option<String>) -> Result<Self> {
    Self::from_bytes(snapshot(data), source, WaveformOptions::default())
  }

  #[napi(getter)]
  pub fn kind(&self) -> AudioKind {
    self.kind.into()
  }

  #[napi(getter)]
  pub fn byte_size(&self) -> f64 {
    self.data.len() as f64
  }

  #[napi(getter)]
  pub fn source(&self) -> Option<String> {
    self.source.clone()
  }

  #[napi]
  pub fn metadata(&self) -> Result<AudioSpecs> {
    parse_specs(&self.data, Some(self.kind), self.source.clone())
  }

  #[napi]
  pub fn metadata_async(&self, env: &Env) -> Result<AsyncBlock<AudioSpecs>> {
    parse_specs_async(
      env,
      Arc::clone(&self.data),
      Some(self.kind),
      self.source.clone(),
    )
  }

  #[napi]
  pub fn decode(&self, options: Option<DecodeOptions>) -> Result<DecodedAudio> {
    let config = decode_config(&self.defaults.merged_decode(options))?;
    decode_pcm(Arc::clone(&self.data), config).map(Into::into)
  }

  #[napi]
  pub fn decode_async(
    &self,
    env: &Env,
    options: Option<DecodeOptions>,
  ) -> Result<AsyncBlock<DecodedAudio>> {
    let config = decode_config(&self.defaults.merged_decode(options))?;
    decode_pcm_async(env, Arc::clone(&self.data), config)
  }

  #[napi]
  pub fn waveform(&self, options: Option<WaveformOptions>) -> Result<WaveformPeaks> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    decode_and_reduce(Arc::clone(&self.data), dc, wc).map(Into::into)
  }

  #[napi]
  pub fn waveform_async(
    &self,
    env: &Env,
    options: Option<WaveformOptions>,
  ) -> Result<AsyncBlock<WaveformPeaks>> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    decode_and_reduce_async(env, Arc::clone(&self.data), dc, wc)
  }

  #[napi]
  pub fn analyze(&self, options: Option<WaveformOptions>) -> Result<AudioAnalysis> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    analyze(Arc::clone(&self.data), dc, wc, self.source.clone())
  }

  #[napi]
  pub fn analyze_async(
    &self,
    env: &Env,
    options: Option<WaveformOptions>,
  ) -> Result<AsyncBlock<AudioAnalysis>> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    analyze_async(env, Arc::clone(&self.data), dc, wc, self.source.clone())
  }

  /// Decode once into an `AudioPcm` handle, then reduce it at any number of
  /// resolutions without the PCM ever crossing into JS.
  #[napi]
  pub fn pcm(&self, options: Option<DecodeOptions>) -> Result<AudioPcm> {
    let config = decode_config(&self.defaults.merged_decode(options))?;
    Ok(AudioPcm {
      pcm: Arc::new(decode_pcm(Arc::clone(&self.data), config)?),
      defaults: self.defaults.clone(),
    })
  }

  #[napi]
  pub fn pcm_async(
    &self,
    env: &Env,
    options: Option<DecodeOptions>,
  ) -> Result<AsyncBlock<AudioPcm>> {
    let config = decode_config(&self.defaults.merged_decode(options))?;
    let data = Arc::clone(&self.data);
    let defaults = self.defaults.clone();
    promise(
      env,
      move || decode_pcm(data, config),
      move |_, pcm| {
        Ok(AudioPcm {
          pcm: Arc::new(pcm),
          defaults,
        })
      },
    )
  }
}

// ── AudioPcm: decoded PCM kept in Rust ──────────────────────────

/// Interleaved `f32` PCM held in shared Rust memory. Build one from
/// `AudioDown.pcm()` / `pcmAsync()` or from your own `Float32Array`.
#[napi]
pub struct AudioPcm {
  pcm: Arc<Pcm>,
  defaults: WaveformOptions,
}

#[napi]
impl AudioPcm {
  #[napi(constructor)]
  pub fn new(
    samples: &[f32],
    sample_rate: f64,
    channels: f64,
    defaults: Option<WaveformOptions>,
  ) -> Result<Self> {
    let defaults = defaults.unwrap_or_default();
    let (dc, _) = waveform_configs(&defaults)?;
    let pcm = pcm_from_js(samples, sample_rate, channels, dc.max_decoded_samples)?;
    // The reducer rejects non-finite input on every call; fail once, up front.
    if pcm.samples.iter().any(|s| !s.is_finite()) {
      return Err(invalid("PCM contains non-finite samples"));
    }
    Ok(AudioPcm {
      pcm: Arc::new(pcm),
      defaults,
    })
  }

  #[napi(getter)]
  pub fn sample_rate(&self) -> u32 {
    self.pcm.sample_rate
  }

  #[napi(getter)]
  pub fn channels(&self) -> u32 {
    self.pcm.channels as u32
  }

  /// Frames per channel.
  #[napi(getter)]
  pub fn frame_count(&self) -> u32 {
    u32::try_from(self.pcm.frames()).unwrap_or(u32::MAX)
  }

  #[napi(getter)]
  pub fn duration_sec(&self) -> f64 {
    self.pcm.duration_sec()
  }

  #[napi(getter)]
  pub fn gapless_enabled(&self) -> bool {
    self.pcm.gapless_enabled
  }

  #[napi(getter)]
  pub fn skipped_packets(&self) -> u32 {
    self.pcm.skipped_packets
  }

  /// Copy of the interleaved samples.
  #[napi]
  pub fn samples(&self) -> Float32Array {
    Float32Array::new(self.pcm.samples.clone())
  }

  /// The same data as a `DecodedAudio` object (copies the samples).
  #[napi]
  pub fn decoded(&self) -> DecodedAudio {
    Pcm::clone(&self.pcm).into()
  }

  #[napi]
  pub fn waveform(&self, options: Option<WaveformOptions>) -> Result<WaveformPeaks> {
    let (_, wc) = waveform_configs(&self.defaults.merged(options))?;
    reduce(&self.pcm, wc).map(Into::into)
  }

  #[napi]
  pub fn waveform_async(
    &self,
    env: &Env,
    options: Option<WaveformOptions>,
  ) -> Result<AsyncBlock<WaveformPeaks>> {
    let (_, wc) = waveform_configs(&self.defaults.merged(options))?;
    reduce_async(env, Arc::clone(&self.pcm), wc)
  }
}
