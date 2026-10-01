use std::sync::Arc;

use super::decode::{self, DecodeConfig, Pcm};
use super::metadata::{
  self, AudioKind, AudioMeta, DurationSource, MpegLayer, MpegVersion, WavContainer, XingKind,
};
use super::waveform::{self, Resolution, Waveform, WaveformConfig};

fn fixture(name: &str) -> Vec<u8> {
  std::fs::read(format!(
    "{}/__test__/fixtures/{name}",
    env!("CARGO_MANIFEST_DIR")
  ))
  .unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn decode_fixture(name: &str, config: DecodeConfig) -> Result<Pcm, String> {
  decode::decode(Arc::new(fixture(name)), config)
}

fn pcm(samples: Vec<f32>, channels: usize) -> Pcm {
  Pcm {
    samples,
    channels,
    sample_rate: 8000,
    gapless_enabled: false,
    skipped_packets: 0,
  }
}

fn peaks(p: &Pcm, resolution: Resolution, parallel: bool) -> Waveform {
  waveform::peaks(
    p,
    WaveformConfig {
      resolution,
      parallel,
    },
  )
  .unwrap()
}

fn synchsafe(n: usize) -> [u8; 4] {
  [
    ((n >> 21) & 127) as u8,
    ((n >> 14) & 127) as u8,
    ((n >> 7) & 127) as u8,
    (n & 127) as u8,
  ]
}

/// Build a minimal ID3v2.{major} tag holding one frame.
fn tag(major: u8, id: &[u8], payload: &[u8]) -> Vec<u8> {
  let mut frame = id.to_vec();
  if major == 2 {
    frame.extend_from_slice(&[0, (payload.len() >> 8) as u8, payload.len() as u8]);
  } else {
    frame.extend_from_slice(&if major == 4 {
      synchsafe(payload.len())
    } else {
      (payload.len() as u32).to_be_bytes()
    });
    frame.extend_from_slice(&[0, 0]);
  }
  frame.extend_from_slice(payload);
  let mut out = b"ID3".to_vec();
  out.extend_from_slice(&[major, 0, 0]);
  out.extend_from_slice(&synchsafe(frame.len()));
  out.extend(frame);
  out
}

// ── sniffing ────────────────────────────────────────────────────

#[test]
fn recognizes_wave_and_mp3() {
  assert_eq!(
    metadata::sniff(&fixture("stereo.wav")),
    Some(AudioKind::Wav)
  );
  assert_eq!(metadata::sniff(&fixture("rf64.wav")), Some(AudioKind::Wav));
  assert_eq!(metadata::sniff(&fixture("cbr.mp3")), Some(AudioKind::Mp3));
  assert_eq!(
    metadata::sniff(&fixture("no-xing.mp3")),
    Some(AudioKind::Mp3)
  );
}

#[test]
fn rejects_adts() {
  assert_eq!(metadata::sniff(&[0xff, 0xf1, 0x50, 0x80]), None);
}

#[test]
fn empty_and_short_inputs() {
  for n in 0..12 {
    assert!(metadata::parse_audio(&vec![0; n]).is_err());
  }
}

// ── WAV ─────────────────────────────────────────────────────────

#[test]
fn wave_metadata() {
  let m = metadata::parse_wav(&fixture("stereo.wav")).unwrap();
  assert_eq!(m.container, WavContainer::Riff);
  assert_eq!(m.format, "pcm");
  assert_eq!(m.channels, 2);
  assert_eq!(m.sample_rate, 44100);
  assert_eq!(m.bits_per_sample, 16);
  assert_eq!(m.data_size, Some(176400));
  assert_eq!(m.duration_sec, Some(1.0));
}

#[test]
fn rf64_metadata() {
  let m = metadata::parse_wav(&fixture("rf64.wav")).unwrap();
  assert_eq!(m.container, WavContainer::Rf64);
  assert_eq!(m.data_size, Some(176400));
  assert_eq!(m.duration_sec, Some(1.0));
}

#[test]
fn missing_wave_chunks_are_nullable() {
  let m = metadata::parse_wav(b"RIFF\x04\0\0\0WAVE").unwrap();
  assert_eq!(m.data_offset, None);
  assert_eq!(m.duration_sec, None);
  assert_eq!(m.format, "unknown");
}

#[test]
fn malformed_chunk_sizes_do_not_panic() {
  let mut b = fixture("stereo.wav");
  b[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
  assert!(metadata::parse_wav(&b).is_ok());
}

#[test]
fn wav_rejects_mp3_and_vice_versa() {
  assert!(metadata::parse_wav(&fixture("cbr.mp3")).is_err());
  assert!(metadata::parse_mp3(&fixture("stereo.wav")).is_err());
}

// ── MPEG frames ─────────────────────────────────────────────────

#[test]
fn parses_mpeg1() {
  let f = metadata::mpeg_frame(&[0xff, 0xfb, 0x90, 0], 0).unwrap();
  assert_eq!(f.version, MpegVersion::V1);
  assert_eq!(f.layer, MpegLayer::III);
  assert_eq!(f.bitrate_kbps, 128);
  assert_eq!(f.sample_rate, 44100);
  assert_eq!(f.samples_per_frame, 1152);
  assert_eq!(f.frame_size, 417);
  assert!(!f.has_crc);
}

#[test]
fn parses_mpeg2() {
  let f = metadata::mpeg_frame(&[0xff, 0xf3, 0x80, 0xc0], 0).unwrap();
  assert_eq!(f.version, MpegVersion::V2);
  assert_eq!(f.samples_per_frame, 576);
  assert_eq!(f.channels, 1);
}

#[test]
fn rejects_reserved_headers() {
  assert!(metadata::mpeg_frame(&[0xff, 0xfb, 0x9c, 0], 0).is_none());
  assert!(metadata::mpeg_frame(&[0xff, 0xfb, 0, 0], 0).is_none());
}

#[test]
fn distinguishes_info_from_xing() {
  let cbr = metadata::parse_mp3(&fixture("cbr.mp3")).unwrap();
  let vbr = metadata::parse_mp3(&fixture("vbr.mp3")).unwrap();
  assert_eq!(cbr.xing.as_ref().unwrap().kind, XingKind::Info);
  assert_eq!(vbr.xing.as_ref().unwrap().kind, XingKind::Xing);
  assert_eq!(cbr.cbr(), Some(true));
  assert_eq!(vbr.cbr(), Some(false));
  assert_eq!(vbr.frame.as_ref().unwrap().version, MpegVersion::V1);
  assert_eq!(cbr.tags.title.as_deref(), Some("Precision fixture"));
}

#[test]
fn frame_walk_is_exact_for_complete_files() {
  for name in ["cbr.mp3", "vbr.mp3", "no-xing.mp3"] {
    let m = metadata::parse_mp3(&fixture(name)).unwrap();
    let walk = m.walk.as_ref().unwrap_or_else(|| panic!("{name} walk"));
    assert!(!walk.truncated, "{name}");
    assert_eq!(walk.resyncs, 0, "{name}");
    assert_eq!(walk.sample_rate, 44100, "{name}");
    assert_eq!(walk.channels, 2, "{name}");
    assert_eq!(m.duration_source, Some(DurationSource::Frames), "{name}");
    // Encoder delay/padding add a frame or two over the 1 s source.
    let d = m.duration_sec.unwrap();
    assert!((1.0..1.1).contains(&d), "{name}: {d}");
    // Xing/Info frame counts agree with the walk on a complete file.
    if let Some(frames) = m.xing.as_ref().and_then(|x| x.frames) {
      assert_eq!(u64::from(frames), walk.frames, "{name}");
    }
  }
  let no_xing = metadata::parse_mp3(&fixture("no-xing.mp3")).unwrap();
  assert!(no_xing.xing.is_none());
  assert_eq!(no_xing.cbr(), Some(true));
  // Padding alternates 417/418-byte frames, so the average sits within a few bps of nominal.
  let bps = i64::from(no_xing.walk.as_ref().unwrap().bitrate_bps());
  assert!((bps - 128_000).abs() < 100, "{bps}");
}

#[test]
fn prefix_probe_prefers_xing_and_reports_truncation() {
  let full = fixture("vbr.mp3");
  let probe = metadata::parse_mp3(&full[..2048]).unwrap();
  let walk = probe.walk.as_ref().unwrap();
  assert!(walk.truncated);
  assert_eq!(probe.duration_source, Some(DurationSource::Xing));
  let whole = metadata::parse_mp3(&full).unwrap();
  assert_eq!(probe.duration_sec, whole.duration_sec);
}

#[test]
fn wave_duration_sources() {
  let m = metadata::parse_wav(&fixture("stereo.wav")).unwrap();
  assert_eq!(m.duration_source, Some(DurationSource::Data));
  assert_eq!(m.sample_frames, Some(44100));
  let m = metadata::parse_wav(&fixture("float.wav")).unwrap();
  assert_eq!(m.sample_frames, Some(44100));
  assert_eq!(m.duration_sec, Some(1.0));
}

// ── ID3 ─────────────────────────────────────────────────────────

#[test]
fn parses_id3_versions() {
  for major in 2..=4 {
    let m = metadata::parse_mp3(&tag(
      major,
      if major == 2 { b"TT2" } else { b"TIT2" },
      b"\x03Hello\0",
    ))
    .unwrap();
    assert_eq!(m.tags.title.as_deref(), Some("Hello"));
    assert_eq!(m.id3v2.unwrap().version, format!("2.{major}.0"));
  }
}

#[test]
fn parses_comment_description() {
  let m = metadata::parse_mp3(&tag(4, b"COMM", b"\x03engdescription\0Actual comment\0")).unwrap();
  assert_eq!(m.tags.comment.as_deref(), Some("Actual comment"));
}

#[test]
fn parses_utf16be_comment() {
  let mut b = vec![1, b'e', b'n', b'g', 0xfe, 0xff, 0, 0];
  b.extend_from_slice(&[0, 72, 0, 105, 0, 0]);
  let m = metadata::parse_mp3(&tag(3, b"COMM", &b)).unwrap();
  assert_eq!(m.tags.comment.as_deref(), Some("Hi"));
}

#[test]
fn id3v2_wins_over_v1() {
  let mut b = tag(4, b"TIT2", b"\x03new");
  let mut v1 = vec![0; 128];
  v1[..3].copy_from_slice(b"TAG");
  v1[3..6].copy_from_slice(b"old");
  v1[33..39].copy_from_slice(b"artist");
  b.extend(v1);
  let m = metadata::parse_mp3(&b).unwrap();
  assert_eq!(m.tags.title.as_deref(), Some("new"));
  assert_eq!(m.tags.artist.as_deref(), Some("artist"));
}

#[test]
fn bare_sync_word_yields_nulls() {
  let m = metadata::parse_mp3(&[0xff, 0xfb]).unwrap();
  assert!(m.frame.is_none());
  assert!(m.walk.is_none());
  assert!(m.duration_sec.is_none());
  assert_eq!(m.cbr(), None);
  assert!(m.id3v2.is_none());
  assert_eq!(m.tags, Default::default());
  assert!(matches!(
    metadata::parse_audio(&[0xff, 0xfb]).unwrap(),
    AudioMeta::Mp3(_)
  ));
}

// ── robustness ──────────────────────────────────────────────────

#[test]
fn header_probes_do_not_panic() {
  for name in ["vbr.mp3", "rf64.wav", "cbr.mp3", "stereo.wav"] {
    let b = fixture(name);
    for n in 0..b.len().min(1024) {
      let _ = metadata::parse_audio(&b[..n]);
    }
  }
}

#[test]
fn deterministic_garbage_does_not_panic() {
  let mut state = 0x12345678u32;
  for n in 0..1024 {
    let mut b = vec![0; n];
    for v in &mut b {
      state ^= state << 13;
      state ^= state >> 17;
      state ^= state << 5;
      *v = state as u8;
    }
    let _ = metadata::parse_audio(&b);
    let _ = metadata::mpeg_frame(&b, n / 2);
    let _ = decode::decode(Arc::new(b), DecodeConfig::default());
  }
}

// ── waveform ────────────────────────────────────────────────────

#[test]
fn exact_extrema_and_rms() {
  let p = pcm(vec![0., -0.25, 0.5, -1.25, 1.5, 0., 0.125], 1);
  let w = peaks(&p, Resolution::SamplesPerPeak(3), false);
  assert_eq!(w.boundaries, vec![0, 3, 6, 7]);
  assert_eq!(w.channels[0].min, vec![-0.25, -1.25, 0.125]);
  assert_eq!(w.channels[0].max, vec![0.5, 1.5, 0.125]);
  assert_eq!(w.channels[0].peak, vec![0.5, 1.5, 0.125]);
  assert!((w.channels[0].rms[0] - (0.3125f64 / 3.).sqrt()).abs() < 1e-15);
}

#[test]
fn peak_count_uses_all_frames() {
  let p = pcm((0..10).map(|n| n as f32).collect(), 1);
  let w = peaks(&p, Resolution::PeakCount(3), false);
  assert_eq!(w.boundaries, vec![0, 3, 6, 10]);
  assert_eq!(w.channels[0].max, vec![2., 5., 9.]);
}

#[test]
fn stereo_is_not_downmixed() {
  let p = pcm(vec![0.75, -0.75, -0.5, 0.5], 2);
  let w = peaks(&p, Resolution::PeakCount(1), false);
  assert_eq!(w.channels[0].peak, vec![0.75]);
  assert_eq!(w.channels[1].peak, vec![0.75]);
  assert_eq!(w.channels[0].min, vec![-0.5]);
  assert_eq!(w.channels[1].max, vec![0.5]);
}

#[test]
fn no_empty_buckets() {
  let p = pcm(vec![0.1, 0.2], 1);
  assert_eq!(
    peaks(&p, Resolution::PeakCount(1024), false).boundaries,
    vec![0, 1, 2]
  );
}

#[test]
fn final_single_sample_transient_is_preserved() {
  let mut p = pcm(vec![0.; 1001], 1);
  p.samples[1000] = 1.;
  let w = peaks(&p, Resolution::SamplesPerPeak(100), false);
  assert_eq!(w.channels[0].peak[10], 1.);
}

#[test]
fn parallel_matches_serial_mono_and_stereo() {
  for channels in [1usize, 2, 3] {
    let p = pcm(
      (0..300_001 * channels)
        .map(|n| ((n as f32) * 0.17).sin())
        .collect(),
      channels,
    );
    let a = peaks(&p, Resolution::PeakCount(2048), false);
    let b = peaks(&p, Resolution::PeakCount(2048), true);
    assert_eq!(a, b, "channels={channels}");
  }
}

#[test]
fn rejects_invalid_pcm() {
  for p in [
    pcm(vec![], 1),
    pcm(vec![f32::NAN], 1),
    pcm(vec![f32::INFINITY, 0.0], 1),
    pcm(vec![0.], 2),
  ] {
    assert!(waveform::peaks(&p, WaveformConfig::default()).is_err());
  }
  let big: Vec<f32> = (0..waveform::PARALLEL_THRESHOLD + 7)
    .map(|i| if i == 1234 { f32::NAN } else { 0.0 })
    .collect();
  assert!(waveform::peaks(&pcm(big, 1), WaveformConfig::default()).is_err());
}

#[test]
fn rejects_bad_resolutions() {
  let p = pcm(vec![0.; 20], 1);
  for r in [
    Resolution::PeakCount(0),
    Resolution::SamplesPerPeak(0),
    Resolution::PeakCount(1_000_001),
  ] {
    assert!(
      waveform::peaks(
        &p,
        WaveformConfig {
          resolution: r,
          parallel: false
        }
      )
      .is_err()
    );
  }
}

// ── decode ──────────────────────────────────────────────────────

#[test]
fn decodes_pcm_wave_exactly() {
  let p = decode_fixture("stereo.wav", DecodeConfig::default()).unwrap();
  assert_eq!(p.frames(), 44100);
  assert_eq!(p.channels, 2);
  assert_eq!(p.samples[22053 * 2], 32000. / 32768.);
  assert_eq!(p.samples[22053 * 2 + 1], -31000. / 32768.);
  assert_eq!(p.duration_sec(), 1.);
}

#[test]
fn decoded_sample_limit_is_enforced() {
  let err = decode_fixture(
    "stereo.wav",
    DecodeConfig {
      max_decoded_samples: 100,
      ..Default::default()
    },
  )
  .unwrap_err();
  assert!(err.contains("maxDecodedSamples"), "{err}");
}

#[test]
fn gapless_mp3_frame_counts() {
  for name in ["cbr.mp3", "vbr.mp3"] {
    let p = decode_fixture(name, DecodeConfig::default()).unwrap();
    assert_eq!(p.frames(), 44100, "{name}");
    assert_eq!(p.skipped_packets, 0);
    let padded = decode_fixture(
      name,
      DecodeConfig {
        gapless: false,
        ..Default::default()
      },
    )
    .unwrap();
    assert!(padded.frames() > 44100, "{name}");
  }
}

#[test]
fn mp3_matches_independent_decoder() {
  for name in ["cbr.mp3", "vbr.mp3", "no-xing.mp3"] {
    let p = decode_fixture(name, DecodeConfig::default()).unwrap();
    let reference: Vec<f32> = fixture(&format!("{name}.reference.f32"))
      .as_chunks::<4>()
      .0
      .iter()
      .map(|s| f32::from_le_bytes(*s))
      .collect();
    assert_eq!(p.samples.len(), reference.len(), "{name}");
    let max = p
      .samples
      .iter()
      .zip(reference)
      .map(|(a, b)| (a - b).abs())
      .fold(0., f32::max);
    assert!(max < 0.00003, "{name}: {max}");
  }
}

#[test]
fn decodes_other_wave_codecs() {
  for name in ["pcm24.wav", "float.wav", "rf64.wav"] {
    let p = decode_fixture(name, DecodeConfig::default()).unwrap();
    assert_eq!(p.frames(), 44100, "{name}");
    assert_eq!(p.channels, 2, "{name}");
  }
  for name in ["pcm_alaw.wav", "pcm_mulaw.wav"] {
    let p = decode_fixture(name, DecodeConfig::default()).unwrap();
    assert_eq!(p.sample_rate, 8000, "{name}");
    assert_eq!(p.channels, 1, "{name}");
    assert_eq!(p.frames(), 8000, "{name}");
  }
  let mono = decode_fixture("mpeg2-mono.mp3", DecodeConfig::default()).unwrap();
  assert_eq!(mono.sample_rate, 22050);
  assert_eq!(mono.channels, 1);
  assert_eq!(mono.frames(), 22050);
}

#[test]
fn float_wave_keeps_excursions_above_full_scale() {
  let p = decode_fixture("exact-float.wav", DecodeConfig::default()).unwrap();
  assert_eq!(p.samples, vec![0., -0.25, 0.5, -1.25, 1.5, 0., 0.125]);
}

#[test]
fn decode_rejects_garbage() {
  let err = decode::decode(Arc::new(vec![0, 1, 2, 3]), DecodeConfig::default()).unwrap_err();
  assert!(err.contains("unrecognized"), "{err}");
}

// ── analyze ─────────────────────────────────────────────────────

#[test]
fn analyze_returns_metadata_and_waveform_from_one_snapshot() {
  let (meta, wave) = super::analyze(
    Arc::new(fixture("vbr.mp3")),
    DecodeConfig::default(),
    WaveformConfig {
      resolution: Resolution::PeakCount(321),
      parallel: true,
    },
  )
  .unwrap();
  assert!(matches!(meta, AudioMeta::Mp3(ref m) if m.cbr() == Some(false)));
  assert_eq!(wave.boundaries.len(), 322);
  assert_eq!(wave.duration_sec, 1.0);
  assert_eq!(wave.channel_count, 2);
}

#[test]
fn extensible_wave_resolves_sub_format() {
  let m = metadata::parse_wav(&fixture("float.wav")).unwrap();
  assert_eq!(m.format_tag, 0xfffe);
  assert_eq!(m.format, "extensible");
  assert_eq!(m.sub_format, Some(3));
  assert_eq!(m.codec(), "ieee-float");
  let m = metadata::parse_wav(&fixture("exact-float.wav")).unwrap();
  assert_eq!(m.format_tag, 3);
  assert_eq!(m.sub_format, None);
  assert_eq!(m.codec(), "ieee-float");
}
