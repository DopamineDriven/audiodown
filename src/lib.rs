#![deny(clippy::all)]

//! `@d0paminedriven/audiodown`: MP3/WAV metadata, Symphonia PCM decoding and
//! rayon waveform envelopes for Node.
//!
//! Layout follows pdfdown: this file is the Node-API surface (functions,
//! `AsyncTask` impls, classes), `types.rs` holds the JS boundary types and
//! `core/` is pure Rust.

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::{Env, Task};
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

fn decode_pcm(data: Arc<Vec<u8>>, config: DecodeConfig) -> Result<Pcm> {
  decode::decode(data, config).map_err(fail)
}

fn reduce(pcm: &Pcm, config: WaveformConfig) -> Result<Waveform> {
  waveform::peaks(pcm, config).map_err(fail)
}

fn decode_and_reduce(data: Arc<Vec<u8>>, dc: DecodeConfig, wc: WaveformConfig) -> Result<Waveform> {
  reduce(&decode_pcm(data, dc)?, wc)
}

fn analyze(
  data: Arc<Vec<u8>>,
  dc: DecodeConfig,
  wc: WaveformConfig,
) -> Result<(AudioMeta, usize, Waveform)> {
  let len = data.len();
  let (meta, wave) = core::analyze(data, dc, wc).map_err(fail)?;
  Ok((meta, len, wave))
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

// ── Tasks (libuv thread pool) ───────────────────────────────────

pub struct ParseTask {
  data: Arc<Vec<u8>>,
  kind: Option<meta::AudioKind>,
  source: Option<String>,
}

#[napi]
impl Task for ParseTask {
  type Output = AudioMeta;
  type JsValue = AudioSpecs;

  fn compute(&mut self) -> Result<Self::Output> {
    parse_kind(&self.data, self.kind)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(types::audio_specs(
      output,
      self.data.len(),
      self.source.take(),
    ))
  }
}

pub struct DecodeTask {
  data: Arc<Vec<u8>>,
  config: DecodeConfig,
}

#[napi]
impl Task for DecodeTask {
  type Output = Pcm;
  type JsValue = DecodedAudio;

  fn compute(&mut self) -> Result<Self::Output> {
    decode_pcm(Arc::clone(&self.data), self.config)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(output.into())
  }
}

pub struct WaveformTask {
  data: Arc<Vec<u8>>,
  dc: DecodeConfig,
  wc: WaveformConfig,
}

#[napi]
impl Task for WaveformTask {
  type Output = Waveform;
  type JsValue = WaveformPeaks;

  fn compute(&mut self) -> Result<Self::Output> {
    decode_and_reduce(Arc::clone(&self.data), self.dc, self.wc)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(output.into())
  }
}

pub struct PcmWaveformTask {
  pcm: Arc<Pcm>,
  config: WaveformConfig,
}

#[napi]
impl Task for PcmWaveformTask {
  type Output = Waveform;
  type JsValue = WaveformPeaks;

  fn compute(&mut self) -> Result<Self::Output> {
    reduce(&self.pcm, self.config)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(output.into())
  }
}

pub struct AnalyzeTask {
  data: Arc<Vec<u8>>,
  dc: DecodeConfig,
  wc: WaveformConfig,
  source: Option<String>,
}

#[napi]
impl Task for AnalyzeTask {
  type Output = (AudioMeta, usize, Waveform);
  type JsValue = AudioAnalysis;

  fn compute(&mut self) -> Result<Self::Output> {
    analyze(Arc::clone(&self.data), self.dc, self.wc)
  }

  fn resolve(&mut self, _env: Env, (meta, len, wave): Self::Output) -> Result<Self::JsValue> {
    Ok(AudioAnalysis {
      specs: types::audio_specs(meta, len, self.source.take()),
      waveform: wave.into(),
    })
  }
}

pub struct PcmTask {
  data: Arc<Vec<u8>>,
  config: DecodeConfig,
  defaults: WaveformOptions,
}

#[napi]
impl Task for PcmTask {
  type Output = Pcm;
  type JsValue = AudioPcm;

  fn compute(&mut self) -> Result<Self::Output> {
    decode_pcm(Arc::clone(&self.data), self.config)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(AudioPcm {
      pcm: Arc::new(output),
      defaults: self.defaults.clone(),
    })
  }
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
pub fn parse_audio_async(data: &[u8], source: Option<String>) -> AsyncTask<ParseTask> {
  AsyncTask::new(ParseTask {
    data: snapshot(data),
    kind: None,
    source,
  })
}

#[napi(ts_return_type = "Mp3Meta")]
pub fn parse_mp3(data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
  parse_specs(data, Some(meta::AudioKind::Mp3), source)
}

#[napi(ts_return_type = "Promise<Mp3Meta>")]
pub fn parse_mp3_async(data: &[u8], source: Option<String>) -> AsyncTask<ParseTask> {
  AsyncTask::new(ParseTask {
    data: snapshot(data),
    kind: Some(meta::AudioKind::Mp3),
    source,
  })
}

#[napi(ts_return_type = "WavMeta")]
pub fn parse_wav(data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
  parse_specs(data, Some(meta::AudioKind::Wav), source)
}

#[napi(ts_return_type = "Promise<WavMeta>")]
pub fn parse_wav_async(data: &[u8], source: Option<String>) -> AsyncTask<ParseTask> {
  AsyncTask::new(ParseTask {
    data: snapshot(data),
    kind: Some(meta::AudioKind::Wav),
    source,
  })
}

#[napi]
pub fn decode_audio(data: &[u8], options: Option<DecodeOptions>) -> Result<DecodedAudio> {
  let config = decode_config(&options.unwrap_or_default())?;
  decode_pcm(snapshot(data), config).map(Into::into)
}

#[napi]
pub fn decode_audio_async(
  data: &[u8],
  options: Option<DecodeOptions>,
) -> Result<AsyncTask<DecodeTask>> {
  Ok(AsyncTask::new(DecodeTask {
    data: snapshot(data),
    config: decode_config(&options.unwrap_or_default())?,
  }))
}

#[napi]
pub fn waveform_peaks(data: &[u8], options: Option<WaveformOptions>) -> Result<WaveformPeaks> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  decode_and_reduce(snapshot(data), dc, wc).map(Into::into)
}

#[napi]
pub fn waveform_peaks_async(
  data: &[u8],
  options: Option<WaveformOptions>,
) -> Result<AsyncTask<WaveformTask>> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  Ok(AsyncTask::new(WaveformTask {
    data: snapshot(data),
    dc,
    wc,
  }))
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
  samples: &[f32],
  sample_rate: f64,
  channels: f64,
  options: Option<WaveformOptions>,
) -> Result<AsyncTask<PcmWaveformTask>> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  Ok(AsyncTask::new(PcmWaveformTask {
    pcm: Arc::new(pcm_from_js(
      samples,
      sample_rate,
      channels,
      dc.max_decoded_samples,
    )?),
    config: wc,
  }))
}

#[napi]
pub fn analyze_audio(
  data: &[u8],
  options: Option<WaveformOptions>,
  source: Option<String>,
) -> Result<AudioAnalysis> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  let (meta, len, wave) = analyze(snapshot(data), dc, wc)?;
  Ok(AudioAnalysis {
    specs: types::audio_specs(meta, len, source),
    waveform: wave.into(),
  })
}

#[napi]
pub fn analyze_audio_async(
  data: &[u8],
  options: Option<WaveformOptions>,
  source: Option<String>,
) -> Result<AsyncTask<AnalyzeTask>> {
  let (dc, wc) = waveform_configs(&options.unwrap_or_default())?;
  Ok(AsyncTask::new(AnalyzeTask {
    data: snapshot(data),
    dc,
    wc,
    source,
  }))
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
  pub fn parse_audio_async(&self, data: &[u8], source: Option<String>) -> AsyncTask<ParseTask> {
    parse_audio_async(data, source)
  }

  #[napi(ts_return_type = "Mp3Meta")]
  pub fn parse_mp3(&self, data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
    parse_mp3(data, source)
  }

  #[napi(ts_return_type = "Promise<Mp3Meta>")]
  pub fn parse_mp3_async(&self, data: &[u8], source: Option<String>) -> AsyncTask<ParseTask> {
    parse_mp3_async(data, source)
  }

  #[napi(ts_return_type = "WavMeta")]
  pub fn parse_wav(&self, data: &[u8], source: Option<String>) -> Result<AudioSpecs> {
    parse_wav(data, source)
  }

  #[napi(ts_return_type = "Promise<WavMeta>")]
  pub fn parse_wav_async(&self, data: &[u8], source: Option<String>) -> AsyncTask<ParseTask> {
    parse_wav_async(data, source)
  }

  #[napi]
  pub fn decode_audio(&self, data: &[u8], options: Option<DecodeOptions>) -> Result<DecodedAudio> {
    decode_audio(data, Some(self.defaults.merged_decode(options)))
  }

  #[napi]
  pub fn decode_audio_async(
    &self,
    data: &[u8],
    options: Option<DecodeOptions>,
  ) -> Result<AsyncTask<DecodeTask>> {
    decode_audio_async(data, Some(self.defaults.merged_decode(options)))
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
    data: &[u8],
    options: Option<WaveformOptions>,
  ) -> Result<AsyncTask<WaveformTask>> {
    waveform_peaks_async(data, Some(self.defaults.merged(options)))
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
    samples: &[f32],
    sample_rate: f64,
    channels: f64,
    options: Option<WaveformOptions>,
  ) -> Result<AsyncTask<PcmWaveformTask>> {
    waveform_from_pcm_async(
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
    data: &[u8],
    options: Option<WaveformOptions>,
    source: Option<String>,
  ) -> Result<AsyncTask<AnalyzeTask>> {
    analyze_audio_async(data, Some(self.defaults.merged(options)), source)
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
  pub fn metadata_async(&self) -> AsyncTask<ParseTask> {
    AsyncTask::new(ParseTask {
      data: Arc::clone(&self.data),
      kind: Some(self.kind),
      source: self.source.clone(),
    })
  }

  #[napi]
  pub fn decode(&self, options: Option<DecodeOptions>) -> Result<DecodedAudio> {
    let config = decode_config(&self.defaults.merged_decode(options))?;
    decode_pcm(Arc::clone(&self.data), config).map(Into::into)
  }

  #[napi]
  pub fn decode_async(&self, options: Option<DecodeOptions>) -> Result<AsyncTask<DecodeTask>> {
    Ok(AsyncTask::new(DecodeTask {
      data: Arc::clone(&self.data),
      config: decode_config(&self.defaults.merged_decode(options))?,
    }))
  }

  #[napi]
  pub fn waveform(&self, options: Option<WaveformOptions>) -> Result<WaveformPeaks> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    decode_and_reduce(Arc::clone(&self.data), dc, wc).map(Into::into)
  }

  #[napi]
  pub fn waveform_async(
    &self,
    options: Option<WaveformOptions>,
  ) -> Result<AsyncTask<WaveformTask>> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    Ok(AsyncTask::new(WaveformTask {
      data: Arc::clone(&self.data),
      dc,
      wc,
    }))
  }

  #[napi]
  pub fn analyze(&self, options: Option<WaveformOptions>) -> Result<AudioAnalysis> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    let (meta, len, wave) = analyze(Arc::clone(&self.data), dc, wc)?;
    Ok(AudioAnalysis {
      specs: types::audio_specs(meta, len, self.source.clone()),
      waveform: wave.into(),
    })
  }

  #[napi]
  pub fn analyze_async(&self, options: Option<WaveformOptions>) -> Result<AsyncTask<AnalyzeTask>> {
    let (dc, wc) = waveform_configs(&self.defaults.merged(options))?;
    Ok(AsyncTask::new(AnalyzeTask {
      data: Arc::clone(&self.data),
      dc,
      wc,
      source: self.source.clone(),
    }))
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
  pub fn pcm_async(&self, options: Option<DecodeOptions>) -> Result<AsyncTask<PcmTask>> {
    Ok(AsyncTask::new(PcmTask {
      data: Arc::clone(&self.data),
      config: decode_config(&self.defaults.merged_decode(options))?,
      defaults: self.defaults.clone(),
    }))
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
    options: Option<WaveformOptions>,
  ) -> Result<AsyncTask<PcmWaveformTask>> {
    let (_, wc) = waveform_configs(&self.defaults.merged(options))?;
    Ok(AsyncTask::new(PcmWaveformTask {
      pcm: Arc::clone(&self.pcm),
      config: wc,
    }))
  }
}
