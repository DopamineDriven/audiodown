//! Pure Rust core. Nothing in here depends on napi, so it is unit-testable
//! with plain `cargo test` and reusable outside the Node binding.

pub(crate) mod decode;
pub(crate) mod metadata;
pub(crate) mod waveform;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use decode::{DecodeConfig, decode};
use metadata::{AudioMeta, parse_audio};
use waveform::{Waveform, WaveformConfig, peaks};

/// Metadata plus decoded waveform from one input snapshot.
///
/// Metadata parsing only scans headers, decoding walks the whole stream; the
/// two are independent, so they run under `rayon::join` and the waveform
/// reduction itself fans out across the pool.
pub(crate) fn analyze(
  data: Arc<Vec<u8>>,
  decode_config: DecodeConfig,
  waveform_config: WaveformConfig,
) -> Result<(AudioMeta, Waveform), String> {
  let (meta, waveform) = rayon::join(
    || parse_audio(&data),
    || decode(Arc::clone(&data), decode_config).and_then(|pcm| peaks(&pcm, waveform_config)),
  );
  Ok((meta?, waveform?))
}
