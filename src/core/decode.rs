//! Symphonia-backed decoding to interleaved `f32` PCM.
//!
//! Decoding is deliberately sequential: MPEG Layer III frames borrow bits from
//! earlier frames (the bit reservoir), so packets cannot be decoded out of
//! order. Parallelism happens after this stage, in `waveform`.

use std::io::{Cursor, ErrorKind};
use std::sync::Arc;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::metadata::{AudioKind, normalize_rf64, sniff};

pub const DEFAULT_MAX_DECODED_SAMPLES: usize = 64_000_000;

#[derive(Clone, Copy, Debug)]
pub struct DecodeConfig {
  /// Apply encoder delay / padding trimming when the stream carries gapless metadata.
  pub gapless: bool,
  /// Fail on the first decoder error instead of skipping the packet.
  pub strict: bool,
  /// Upper bound on interleaved scalar samples kept in memory.
  pub max_decoded_samples: usize,
}

impl Default for DecodeConfig {
  fn default() -> Self {
    Self {
      gapless: true,
      strict: true,
      max_decoded_samples: DEFAULT_MAX_DECODED_SAMPLES,
    }
  }
}

/// Decoded audio. Samples are interleaved frames (`L0 R0 L1 R1 ...`) exactly
/// as the decoder produced them: no resampling, downmixing, clipping or
/// normalization.
#[derive(Clone, Debug)]
pub struct Pcm {
  pub samples: Vec<f32>,
  pub sample_rate: u32,
  pub channels: usize,
  pub gapless_enabled: bool,
  pub skipped_packets: u32,
}

impl Pcm {
  pub fn frames(&self) -> usize {
    self.samples.len() / self.channels
  }

  pub fn duration_sec(&self) -> f64 {
    self.frames() as f64 / f64::from(self.sample_rate)
  }
}

/// Lets Symphonia read shared, immutable bytes without copying them.
struct SharedBytes(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBytes {
  fn as_ref(&self) -> &[u8] {
    &self.0
  }
}

pub fn decode(data: Arc<Vec<u8>>, config: DecodeConfig) -> Result<Pcm, String> {
  if config.max_decoded_samples == 0 {
    return Err("maxDecodedSamples must be positive".into());
  }
  let kind = sniff(&data).ok_or("unrecognized audio magic")?;

  // Symphonia 0.5's WAVE reader expects RIFF. RF64 gets normalized on a
  // private copy so the caller's bytes (and the metadata parser) are untouched.
  let data = if kind == AudioKind::Wav && data.starts_with(b"RF64") {
    let mut owned = data.as_ref().clone();
    normalize_rf64(&mut owned)?;
    Arc::new(owned)
  } else {
    data
  };

  let mut hint = Hint::new();
  hint.with_extension(kind.as_str());
  let source = MediaSourceStream::new(
    Box::new(Cursor::new(SharedBytes(data))),
    MediaSourceStreamOptions::default(),
  );
  let probed = symphonia::default::get_probe()
    .format(
      &hint,
      source,
      &FormatOptions {
        enable_gapless: config.gapless,
        ..Default::default()
      },
      &MetadataOptions::default(),
    )
    .map_err(|e| format!("audio probe failed: {e}"))?;

  let mut format = probed.format;
  let track = format
    .default_track()
    .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
    .or_else(|| {
      format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
    })
    .ok_or("no supported audio track")?;
  let track_id = track.id;
  let mut decoder = symphonia::default::get_codecs()
    .make(&track.codec_params, &DecoderOptions::default())
    .map_err(|e| format!("unsupported audio codec: {e}"))?;

  let mut samples = Vec::new();
  let mut spec = None;
  let mut sample_buffer: Option<SampleBuffer<f32>> = None;
  let mut skipped_packets = 0u32;

  loop {
    let packet = match format.next_packet() {
      Ok(packet) => packet,
      Err(Error::IoError(e)) if e.kind() == ErrorKind::UnexpectedEof => break,
      Err(e) => return Err(format!("audio demux failed: {e}")),
    };
    if packet.track_id() != track_id {
      continue;
    }
    let decoded = match decoder.decode(&packet) {
      Ok(buffer) => buffer,
      Err(Error::DecodeError(_)) if !config.strict => {
        skipped_packets = skipped_packets.saturating_add(1);
        continue;
      }
      Err(e) => return Err(format!("audio decode failed: {e}")),
    };

    let current = *decoded.spec();
    let channels = current.channels.count();
    if current.rate == 0 || channels == 0 {
      return Err("invalid decoded signal specification".into());
    }
    match spec {
      Some(previous) if previous != current => {
        return Err("audio signal specification changed midstream".into());
      }
      Some(_) => {}
      None => spec = Some(current),
    }

    let additional = decoded
      .frames()
      .checked_mul(channels)
      .ok_or("decoded sample count overflow")?;
    let next = samples
      .len()
      .checked_add(additional)
      .ok_or("decoded sample count overflow")?;
    if next > config.max_decoded_samples {
      return Err("decoded audio exceeds maxDecodedSamples".into());
    }

    if sample_buffer
      .as_ref()
      .is_none_or(|b| b.capacity() < decoded.capacity() * channels)
    {
      sample_buffer = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, current));
    }
    let buffer = sample_buffer
      .as_mut()
      .expect("sample buffer was just allocated");
    // Symphonia's MPEG decoder already applies packet.trim_start/trim_end.
    // Applying those offsets here again would delete real samples.
    buffer.copy_interleaved_ref(decoded);
    if buffer.samples().iter().any(|v| !v.is_finite()) {
      return Err("decoded PCM contains non-finite samples".into());
    }
    samples
      .try_reserve(buffer.len())
      .map_err(|e| format!("PCM allocation failed: {e}"))?;
    samples.extend_from_slice(buffer.samples());
  }

  let spec = spec.ok_or("audio contained no decodable packets")?;
  if samples.is_empty() {
    return Err("audio contained no decoded samples".into());
  }
  Ok(Pcm {
    samples,
    sample_rate: spec.rate,
    channels: spec.channels.count(),
    gapless_enabled: config.gapless,
    skipped_packets,
  })
}
