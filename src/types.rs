//! JS boundary types. Everything here is emitted into `index.d.ts` by napi-rs;
//! the `From` impls translate the pure-Rust core types into them.

use std::collections::HashMap;

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::core::decode::Pcm;
use crate::core::metadata as meta;
use crate::core::waveform::Waveform;

fn nullable<T>(value: Option<T>) -> Either<T, Null> {
  match value {
    Some(v) => Either::A(v),
    None => Either::B(Null),
  }
}

// ── Enums (string unions in TypeScript) ─────────────────────────

#[napi(string_enum = "lowercase")]
pub enum AudioKind {
  Mp3,
  Wav,
}

impl From<meta::AudioKind> for AudioKind {
  fn from(kind: meta::AudioKind) -> Self {
    match kind {
      meta::AudioKind::Mp3 => AudioKind::Mp3,
      meta::AudioKind::Wav => AudioKind::Wav,
    }
  }
}

#[napi(string_enum)]
pub enum MpegVersion {
  #[napi(value = "1")]
  V1,
  #[napi(value = "2")]
  V2,
  #[napi(value = "2.5")]
  V25,
}

impl From<meta::MpegVersion> for MpegVersion {
  fn from(version: meta::MpegVersion) -> Self {
    match version {
      meta::MpegVersion::V1 => MpegVersion::V1,
      meta::MpegVersion::V2 => MpegVersion::V2,
      meta::MpegVersion::V2_5 => MpegVersion::V25,
    }
  }
}

#[napi(string_enum)]
#[allow(clippy::upper_case_acronyms)]
pub enum MpegLayer {
  I,
  II,
  III,
}

impl From<meta::MpegLayer> for MpegLayer {
  fn from(layer: meta::MpegLayer) -> Self {
    match layer {
      meta::MpegLayer::I => MpegLayer::I,
      meta::MpegLayer::II => MpegLayer::II,
      meta::MpegLayer::III => MpegLayer::III,
    }
  }
}

#[napi(string_enum = "kebab-case")]
pub enum MpegChannelMode {
  Stereo,
  JointStereo,
  DualChannel,
  Mono,
}

impl From<meta::MpegChannelMode> for MpegChannelMode {
  fn from(mode: meta::MpegChannelMode) -> Self {
    match mode {
      meta::MpegChannelMode::Stereo => MpegChannelMode::Stereo,
      meta::MpegChannelMode::JointStereo => MpegChannelMode::JointStereo,
      meta::MpegChannelMode::DualChannel => MpegChannelMode::DualChannel,
      meta::MpegChannelMode::Mono => MpegChannelMode::Mono,
    }
  }
}

// ── Metadata ────────────────────────────────────────────────────

#[napi(string_enum)]
pub enum AudioContainer {
  #[napi(value = "mpeg")]
  Mpeg,
  #[napi(value = "RIFF")]
  Riff,
  #[napi(value = "RF64")]
  Rf64,
}

#[napi(string_enum)]
pub enum XingKind {
  Xing,
  Info,
}

#[napi(string_enum = "lowercase")]
pub enum DurationSource {
  /// Every MPEG frame in the buffer was walked; exact for the bytes supplied.
  Frames,
  /// Xing/Info frame count; whole-file truth even when only a prefix was parsed.
  Xing,
  /// WAVE `fact` chunk sample count.
  Fact,
  /// WAVE `data` chunk size divided by the byte rate.
  Data,
}

impl From<meta::DurationSource> for DurationSource {
  fn from(source: meta::DurationSource) -> Self {
    match source {
      meta::DurationSource::Frames => DurationSource::Frames,
      meta::DurationSource::Xing => DurationSource::Xing,
      meta::DurationSource::Fact => DurationSource::Fact,
      meta::DurationSource::Data => DurationSource::Data,
    }
  }
}

#[napi(object, object_from_js = false)]
pub struct MpegFrame {
  /// Byte offset of the frame header within the buffer.
  pub offset: f64,
  pub version: MpegVersion,
  pub layer: MpegLayer,
  pub bitrate_kbps: u32,
  pub sample_rate: u32,
  #[napi(ts_type = "1 | 2")]
  pub channels: u32,
  pub channel_mode: MpegChannelMode,
  pub padding: bool,
  pub has_crc: bool,
  #[napi(ts_type = "384 | 576 | 1152")]
  pub samples_per_frame: u32,
  pub frame_size: u32,
}

impl From<meta::MpegFrame> for MpegFrame {
  fn from(f: meta::MpegFrame) -> Self {
    MpegFrame {
      offset: f.offset as f64,
      version: f.version.into(),
      layer: f.layer.into(),
      bitrate_kbps: f.bitrate_kbps,
      sample_rate: f.sample_rate,
      channels: f.channels,
      channel_mode: f.channel_mode.into(),
      padding: f.padding,
      has_crc: f.has_crc,
      samples_per_frame: f.samples_per_frame,
      frame_size: f.frame_size as u32,
    }
  }
}

#[napi(object, object_from_js = false)]
pub struct XingHeader {
  pub kind: XingKind,
  pub frames: Either<u32, Null>,
  pub bytes: Either<u32, Null>,
}

#[napi(object, object_from_js = false, js_name = "Id3v2Entity")]
pub struct Id3v2Entity {
  #[napi(ts_type = "`2.${2 | 3 | 4}.${number}`")]
  pub version: String,
  /// Total tag size in bytes, including header and footer.
  pub size: f64,
}

/// Every tag either container can carry. MP3 fills the ID3 subset, WAVE fills
/// the LIST/INFO subset; absent tags are omitted.
#[napi(object, object_from_js = false)]
pub struct AudioTags {
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub track: Option<String>,
  pub genre: Option<String>,
  pub comment: Option<String>,
  /// Raw ID3 TYER/TDRC text, e.g. `"2024"` or `"2024-05-01T12:00:00"`.
  pub year: Option<String>,
  /// Raw WAVE ICRD text.
  pub date: Option<String>,
  pub software: Option<String>,
  pub copyright: Option<String>,
  pub engineer: Option<String>,
  pub subject: Option<String>,
}

impl AudioTags {
  fn entries(&self) -> Vec<(&'static str, &str)> {
    [
      ("title", &self.title),
      ("artist", &self.artist),
      ("album", &self.album),
      ("track", &self.track),
      ("genre", &self.genre),
      ("comment", &self.comment),
      ("year", &self.year),
      ("date", &self.date),
      ("software", &self.software),
      ("copyright", &self.copyright),
      ("engineer", &self.engineer),
      ("subject", &self.subject),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.as_deref().map(|v| (key, v)))
    .collect()
  }

  /// `ImageSpecs.metadata`-style bag of every present tag, or `None` when empty.
  fn record(&self) -> Option<HashMap<String, String>> {
    let entries = self.entries();
    if entries.is_empty() {
      return None;
    }
    Some(
      entries
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    )
  }
}

impl From<meta::Mp3Tags> for AudioTags {
  fn from(t: meta::Mp3Tags) -> Self {
    AudioTags {
      title: t.title,
      artist: t.artist,
      album: t.album,
      track: t.track,
      genre: t.genre,
      comment: t.comment,
      year: t.year,
      date: None,
      software: None,
      copyright: None,
      engineer: None,
      subject: None,
    }
  }
}

impl From<meta::WavTags> for AudioTags {
  fn from(t: meta::WavTags) -> Self {
    AudioTags {
      title: t.title,
      artist: t.artist,
      album: t.album,
      track: t.track,
      genre: t.genre,
      comment: t.comment,
      year: None,
      date: t.date,
      software: t.software,
      copyright: t.copyright,
      engineer: t.engineer,
      subject: t.subject,
    }
  }
}

/// First run of four ASCII digits, so `"2024"`, `"2024-05-01"` and
/// `"May 2024"` all yield 2024.
fn parse_year(value: Option<&str>) -> Option<u32> {
  let bytes = value?.as_bytes();
  bytes
    .windows(4)
    .find(|w| w.iter().all(u8::is_ascii_digit))
    .and_then(|w| std::str::from_utf8(w).ok())
    .and_then(|w| w.parse().ok())
}

const MIME_MP3: &str = "audio/mpeg";
const MIME_WAV: &str = "audio/wav";

/// Flat, nullable audio specs shaped like `ImageSpecs` / `ExpandedImgSpecs`
/// from `@d0paminedriven/fs`, with the Prisma `AudioMetadata` columns and the
/// ws-server `mp3Specs` fields folded in so the same object feeds all three.
#[napi(object, object_from_js = false)]
pub struct AudioSpecs {
  // ── asset identity (ImageSpecs / Expanded* / AssetReadyPayload parity) ──
  #[napi(js_name = "type", ts_type = "'AUDIO'")]
  pub asset_type: String,
  /// Discriminant for narrowing; equals `format` and `ext`.
  pub kind: AudioKind,
  /// `ImageSpecs.format` analogue.
  pub format: AudioKind,
  pub ext: AudioKind,
  #[napi(ts_type = "'audio/mpeg' | 'audio/wav'")]
  pub mime: String,
  /// `DocSpecs.mimeType` analogue; same value as `mime`.
  #[napi(ts_type = "'audio/mpeg' | 'audio/wav'")]
  pub mime_type: String,
  /// `Expanded*Specs.contentType` analogue; same value as `mime`.
  #[napi(ts_type = "'audio/mpeg' | 'audio/wav'")]
  pub content_type: String,
  /// `mp3` / `mp2` / `mp1` by MPEG layer; WAVE format name otherwise, with
  /// WAVE_FORMAT_EXTENSIBLE resolved through its SubFormat GUID.
  #[napi(
    ts_type = "'mp3' | 'mp2' | 'mp1' | 'pcm' | 'ieee-float' | 'alaw' | 'mulaw' | 'extensible' | 'unknown' | `tag:${number}`"
  )]
  pub codec: String,
  pub container: AudioContainer,
  /// Length of the parsed buffer in bytes (`Expanded*Specs.byteSize`).
  pub byte_size: f64,
  /// Same as `byteSize`: the bytes this parse actually saw.
  pub fetched_bytes: f64,
  /// Same as `byteSize` (`mp3Specs.size`).
  pub size: f64,
  /// Passed through from the caller, e.g. a CDN URL.
  pub source: Option<String>,

  // ── timing ──
  pub duration_sec: Either<f64, Null>,
  /// Rounded milliseconds (Prisma `AudioMetadata.duration`).
  pub duration_ms: Either<f64, Null>,
  pub duration_source: Either<DurationSource, Null>,

  // ── signal ──
  pub sample_rate: Either<u32, Null>,
  pub channels: Either<u32, Null>,
  /// Average bits per second across the whole stream (`mp3Specs.bitrate`).
  pub bitrate: Either<u32, Null>,
  /// Nominal kilobits per second of the first frame (MP3) or the byte rate (WAVE).
  pub bitrate_kbps: Either<u32, Null>,
  /// WAVE only.
  pub bits_per_sample: Either<u32, Null>,
  pub cbr: Either<bool, Null>,
  pub vbr: Either<bool, Null>,
  /// MP3: MPEG frames walked. WAVE: `null`.
  pub frames: Either<f64, Null>,
  /// PCM frames per channel known from headers without decoding.
  pub sample_frames: Either<f64, Null>,
  /// MP3: gaps between frames that were skipped. WAVE: `null`.
  pub resyncs: Either<u32, Null>,
  /// MP3: the last frame runs past the buffer (prefix probe). WAVE: `null`.
  pub truncated: Either<bool, Null>,

  // ── tags (Prisma AudioMetadata columns flattened) ──
  pub title: Either<String, Null>,
  pub artist: Either<String, Null>,
  pub album: Either<String, Null>,
  pub genre: Either<String, Null>,
  /// Four-digit year parsed from the ID3 year/date or WAVE ICRD tag.
  pub year: Either<u32, Null>,
  pub tags: AudioTags,
  /// `ImageSpecs.metadata`-style record of every present tag.
  pub metadata: Option<HashMap<String, String>>,

  // ── MP3 detail (null for WAVE) ──
  #[napi(js_name = "id3v2")]
  pub id3v2: Either<Id3v2Entity, Null>,
  /// First validated MPEG frame.
  pub frame: Either<MpegFrame, Null>,
  pub xing: Either<XingHeader, Null>,
  /// Byte offset where audio starts: after ID3v2 for MP3, the `data` body for WAVE.
  pub audio_offset: Either<f64, Null>,

  // ── WAVE detail (null for MP3) ──
  pub format_tag: Either<u32, Null>,
  pub byte_rate: Either<u32, Null>,
  pub block_align: Either<u32, Null>,
  pub data_offset: Either<f64, Null>,
  /// Bytes of the `data` chunk present in the buffer (a prefix underestimates).
  pub data_size: Either<f64, Null>,
}

fn mp3_specs(m: meta::Mp3Meta, byte_size: usize, source: Option<String>) -> AudioSpecs {
  let cbr = m.cbr();
  let tags: AudioTags = m.tags.into();
  let frame = m.frame.as_ref();
  let walk = m.walk.as_ref();
  let codec = match frame.map(|f| f.layer) {
    Some(meta::MpegLayer::I) => "mp1",
    Some(meta::MpegLayer::II) => "mp2",
    _ => "mp3",
  };
  AudioSpecs {
    asset_type: "AUDIO".into(),
    kind: AudioKind::Mp3,
    format: AudioKind::Mp3,
    ext: AudioKind::Mp3,
    mime: MIME_MP3.into(),
    mime_type: MIME_MP3.into(),
    content_type: MIME_MP3.into(),
    codec: codec.into(),
    container: AudioContainer::Mpeg,
    byte_size: byte_size as f64,
    fetched_bytes: byte_size as f64,
    size: byte_size as f64,
    source,
    duration_sec: nullable(m.duration_sec),
    duration_ms: nullable(m.duration_sec.map(|s| (s * 1000.0).round())),
    duration_source: nullable(m.duration_source.map(Into::into)),
    sample_rate: nullable(frame.map(|f| f.sample_rate)),
    channels: nullable(frame.map(|f| f.channels)),
    bitrate: nullable(walk.map(meta::FrameWalk::bitrate_bps)),
    bitrate_kbps: nullable(frame.map(|f| f.bitrate_kbps)),
    bits_per_sample: Either::B(Null),
    cbr: nullable(cbr),
    vbr: nullable(cbr.map(|c| !c)),
    frames: nullable(walk.map(|w| w.frames as f64)),
    sample_frames: nullable(walk.map(|w| w.samples as f64)),
    resyncs: nullable(walk.map(|w| w.resyncs)),
    truncated: nullable(walk.map(|w| w.truncated)),
    title: nullable(tags.title.clone()),
    artist: nullable(tags.artist.clone()),
    album: nullable(tags.album.clone()),
    genre: nullable(tags.genre.clone()),
    year: nullable(parse_year(tags.year.as_deref())),
    metadata: tags.record(),
    tags,
    id3v2: nullable(m.id3v2.map(|e| Id3v2Entity {
      version: e.version,
      size: e.size as f64,
    })),
    frame: nullable(m.frame.map(MpegFrame::from)),
    xing: nullable(m.xing.map(|x| XingHeader {
      kind: match x.kind {
        meta::XingKind::Xing => XingKind::Xing,
        meta::XingKind::Info => XingKind::Info,
      },
      frames: nullable(x.frames),
      bytes: nullable(x.bytes),
    })),
    audio_offset: Either::A(m.audio_offset as f64),
    format_tag: Either::B(Null),
    byte_rate: Either::B(Null),
    block_align: Either::B(Null),
    data_offset: Either::B(Null),
    data_size: Either::B(Null),
  }
}

fn wav_specs(m: meta::WavMeta, byte_size: usize, source: Option<String>) -> AudioSpecs {
  let codec = m.codec();
  let tags: AudioTags = m.tags.into();
  let known = m.sample_rate > 0;
  AudioSpecs {
    asset_type: "AUDIO".into(),
    kind: AudioKind::Wav,
    format: AudioKind::Wav,
    ext: AudioKind::Wav,
    mime: MIME_WAV.into(),
    mime_type: MIME_WAV.into(),
    content_type: MIME_WAV.into(),
    codec,
    container: match m.container {
      meta::WavContainer::Riff => AudioContainer::Riff,
      meta::WavContainer::Rf64 => AudioContainer::Rf64,
    },
    byte_size: byte_size as f64,
    fetched_bytes: byte_size as f64,
    size: byte_size as f64,
    source,
    duration_sec: nullable(m.duration_sec),
    duration_ms: nullable(m.duration_sec.map(|s| (s * 1000.0).round())),
    duration_source: nullable(m.duration_source.map(Into::into)),
    sample_rate: nullable(known.then_some(m.sample_rate)),
    channels: nullable((m.channels > 0).then_some(u32::from(m.channels))),
    bitrate: nullable((m.byte_rate > 0).then(|| m.byte_rate.saturating_mul(8))),
    bitrate_kbps: nullable((m.byte_rate > 0).then(|| m.byte_rate.saturating_mul(8) / 1000)),
    bits_per_sample: nullable((m.bits_per_sample > 0).then_some(u32::from(m.bits_per_sample))),
    cbr: nullable(known.then_some(true)),
    vbr: nullable(known.then_some(false)),
    frames: Either::B(Null),
    sample_frames: nullable(m.sample_frames.map(|n| n as f64)),
    resyncs: Either::B(Null),
    truncated: Either::B(Null),
    title: nullable(tags.title.clone()),
    artist: nullable(tags.artist.clone()),
    album: nullable(tags.album.clone()),
    genre: nullable(tags.genre.clone()),
    year: nullable(parse_year(tags.date.as_deref())),
    metadata: tags.record(),
    tags,
    id3v2: Either::B(Null),
    frame: Either::B(Null),
    xing: Either::B(Null),
    audio_offset: nullable(m.data_offset.map(|n| n as f64)),
    format_tag: Either::A(u32::from(m.format_tag)),
    byte_rate: Either::A(m.byte_rate),
    block_align: Either::A(u32::from(m.block_align)),
    data_offset: nullable(m.data_offset.map(|n| n as f64)),
    data_size: nullable(m.data_size.map(|n| n as f64)),
  }
}

pub fn audio_specs(m: meta::AudioMeta, byte_size: usize, source: Option<String>) -> AudioSpecs {
  match m {
    meta::AudioMeta::Mp3(m) => mp3_specs(m, byte_size, source),
    meta::AudioMeta::Wav(m) => wav_specs(m, byte_size, source),
  }
}

// ── Options ─────────────────────────────────────────────────────

#[napi(object)]
#[derive(Clone, Default)]
pub struct DecodeOptions {
  /// Trim encoder delay/padding when gapless metadata is present. Default `true`.
  pub gapless: Option<bool>,
  /// Fail on the first decoder error instead of skipping the packet. Default `true`.
  pub strict: Option<bool>,
  /// Cap on interleaved scalar samples kept in memory. Default `64_000_000`.
  pub max_decoded_samples: Option<f64>,
}

#[napi(object)]
#[derive(Clone, Default)]
pub struct WaveformOptions {
  /// Target bucket count (capped at the frame count). Default `1024`. Exclusive with `samplesPerPeak`.
  pub peak_count: Option<f64>,
  /// Fixed frames per bucket. Exclusive with `peakCount`.
  pub samples_per_peak: Option<f64>,
  /// Reduce buckets on the rayon pool for large inputs. Default `true`.
  pub parallel: Option<bool>,
  pub gapless: Option<bool>,
  pub strict: Option<bool>,
  pub max_decoded_samples: Option<f64>,
}

impl WaveformOptions {
  /// Per-call options over service defaults. A call that names either
  /// resolution selector replaces both defaults so they never conflict.
  pub fn merged(&self, call: Option<WaveformOptions>) -> WaveformOptions {
    let Some(call) = call else {
      return self.clone();
    };
    let resolution_overridden = call.peak_count.is_some() || call.samples_per_peak.is_some();
    WaveformOptions {
      peak_count: if resolution_overridden {
        call.peak_count
      } else {
        self.peak_count
      },
      samples_per_peak: if resolution_overridden {
        call.samples_per_peak
      } else {
        self.samples_per_peak
      },
      parallel: call.parallel.or(self.parallel),
      gapless: call.gapless.or(self.gapless),
      strict: call.strict.or(self.strict),
      max_decoded_samples: call.max_decoded_samples.or(self.max_decoded_samples),
    }
  }

  pub fn merged_decode(&self, call: Option<DecodeOptions>) -> DecodeOptions {
    let call = call.unwrap_or_default();
    DecodeOptions {
      gapless: call.gapless.or(self.gapless),
      strict: call.strict.or(self.strict),
      max_decoded_samples: call.max_decoded_samples.or(self.max_decoded_samples),
    }
  }

  pub fn decode_options(&self) -> DecodeOptions {
    DecodeOptions {
      gapless: self.gapless,
      strict: self.strict,
      max_decoded_samples: self.max_decoded_samples,
    }
  }
}

// ── Decoded audio / waveform ────────────────────────────────────

#[napi(object, object_from_js = false)]
pub struct DecodedAudio {
  /// Interleaved frames: `L0 R0 L1 R1 ...` for stereo.
  pub samples: Float32Array,
  pub sample_rate: u32,
  pub channels: u32,
  /// Frames per channel, not total scalar samples.
  pub frame_count: u32,
  pub duration_sec: f64,
  pub gapless_enabled: bool,
  pub skipped_packets: u32,
}

impl From<Pcm> for DecodedAudio {
  fn from(p: Pcm) -> Self {
    let frame_count = u32::try_from(p.frames()).unwrap_or(u32::MAX);
    let duration_sec = p.duration_sec();
    DecodedAudio {
      samples: Float32Array::new(p.samples),
      sample_rate: p.sample_rate,
      channels: p.channels as u32,
      frame_count,
      duration_sec,
      gapless_enabled: p.gapless_enabled,
      skipped_packets: p.skipped_packets,
    }
  }
}

#[napi(object, object_from_js = false)]
pub struct WaveformChannel {
  /// Signed minimum per bucket.
  pub min: Float32Array,
  /// Signed maximum per bucket.
  pub max: Float32Array,
  /// `max(|min|, |max|)` per bucket.
  pub peak: Float32Array,
  /// `sqrt(sum(sample²) / frames)` per bucket, accumulated in f64.
  pub rms: Float64Array,
}

#[napi(object, object_from_js = false)]
pub struct WaveformPeaks {
  pub sample_rate: u32,
  pub channel_count: u32,
  /// Frames per channel.
  pub frame_count: u32,
  /// Decoded duration: `frameCount / sampleRate`.
  pub duration_sec: f64,
  /// Actual bucket count (`peakCount` is capped at `frameCount`).
  pub peak_count: u32,
  /// `peakCount + 1` frame offsets; bucket `i` covers `[boundaries[i], boundaries[i + 1])`.
  pub boundaries: Uint32Array,
  pub channels: Vec<WaveformChannel>,
  /// `max(channel.peak[i])` across channels: the single-line display envelope.
  /// `Array.from(envelope)` is what a `waveformPeaks` column wants.
  pub envelope: Float32Array,
  pub gapless_enabled: bool,
  pub skipped_packets: u32,
}

impl From<Waveform> for WaveformPeaks {
  fn from(w: Waveform) -> Self {
    let count = w.boundaries.len() - 1;
    let mut envelope = vec![0f32; count];
    for channel in &w.channels {
      for (e, p) in envelope.iter_mut().zip(&channel.peak) {
        *e = e.max(*p);
      }
    }
    WaveformPeaks {
      sample_rate: w.sample_rate,
      channel_count: w.channel_count as u32,
      frame_count: w.frame_count as u32,
      duration_sec: w.duration_sec,
      peak_count: (w.boundaries.len() - 1) as u32,
      boundaries: Uint32Array::new(w.boundaries),
      envelope: Float32Array::new(envelope),
      channels: w
        .channels
        .into_iter()
        .map(|c| WaveformChannel {
          min: Float32Array::new(c.min),
          max: Float32Array::new(c.max),
          peak: Float32Array::new(c.peak),
          rms: Float64Array::new(c.rms),
        })
        .collect(),
      gapless_enabled: w.gapless_enabled,
      skipped_packets: w.skipped_packets,
    }
  }
}

#[napi(object, object_from_js = false)]
pub struct AudioAnalysis {
  pub specs: AudioSpecs,
  pub waveform: WaveformPeaks,
}
