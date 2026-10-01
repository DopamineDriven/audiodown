//! Lightweight MP3 / WAV metadata parsing. Every read is bounds-checked, so
//! truncated header probes and arbitrary bytes never panic.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioKind {
  Mp3,
  Wav,
}

impl AudioKind {
  pub fn as_str(self) -> &'static str {
    match self {
      AudioKind::Mp3 => "mp3",
      AudioKind::Wav => "wav",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MpegVersion {
  V1,
  V2,
  V2_5,
}

impl MpegVersion {
  pub fn as_str(self) -> &'static str {
    match self {
      MpegVersion::V1 => "1",
      MpegVersion::V2 => "2",
      MpegVersion::V2_5 => "2.5",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum MpegLayer {
  I,
  II,
  III,
}

impl MpegLayer {
  pub fn as_str(self) -> &'static str {
    match self {
      MpegLayer::I => "I",
      MpegLayer::II => "II",
      MpegLayer::III => "III",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MpegChannelMode {
  Stereo,
  JointStereo,
  DualChannel,
  Mono,
}

impl MpegChannelMode {
  pub fn as_str(self) -> &'static str {
    match self {
      MpegChannelMode::Stereo => "stereo",
      MpegChannelMode::JointStereo => "joint-stereo",
      MpegChannelMode::DualChannel => "dual-channel",
      MpegChannelMode::Mono => "mono",
    }
  }

  pub fn channels(self) -> u32 {
    if self == MpegChannelMode::Mono { 1 } else { 2 }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavContainer {
  Riff,
  Rf64,
}

impl WavContainer {
  pub fn as_str(self) -> &'static str {
    match self {
      WavContainer::Riff => "RIFF",
      WavContainer::Rf64 => "RF64",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MpegFrame {
  pub offset: usize,
  pub version: MpegVersion,
  pub layer: MpegLayer,
  pub bitrate_kbps: u32,
  pub sample_rate: u32,
  pub channels: u32,
  pub channel_mode: MpegChannelMode,
  pub padding: bool,
  pub has_crc: bool,
  pub samples_per_frame: u32,
  pub frame_size: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mp3Tags {
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub track: Option<String>,
  pub genre: Option<String>,
  pub comment: Option<String>,
  pub year: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mp3TagKey {
  Title,
  Artist,
  Album,
  Track,
  Genre,
  Comment,
  Year,
}

impl Mp3Tags {
  fn slot(&mut self, key: Mp3TagKey) -> &mut Option<String> {
    match key {
      Mp3TagKey::Title => &mut self.title,
      Mp3TagKey::Artist => &mut self.artist,
      Mp3TagKey::Album => &mut self.album,
      Mp3TagKey::Track => &mut self.track,
      Mp3TagKey::Genre => &mut self.genre,
      Mp3TagKey::Comment => &mut self.comment,
      Mp3TagKey::Year => &mut self.year,
    }
  }

  fn set(&mut self, key: Mp3TagKey, value: String) {
    *self.slot(key) = Some(value);
  }

  /// Fields present in `over` replace the ones in `self`.
  pub fn overlay(self, over: Mp3Tags) -> Mp3Tags {
    Mp3Tags {
      title: over.title.or(self.title),
      artist: over.artist.or(self.artist),
      album: over.album.or(self.album),
      track: over.track.or(self.track),
      genre: over.genre.or(self.genre),
      comment: over.comment.or(self.comment),
      year: over.year.or(self.year),
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WavTags {
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub track: Option<String>,
  pub genre: Option<String>,
  pub comment: Option<String>,
  pub date: Option<String>,
  pub software: Option<String>,
  pub copyright: Option<String>,
  pub engineer: Option<String>,
  pub subject: Option<String>,
}

impl WavTags {
  fn slot(&mut self, id: &[u8]) -> Option<&mut Option<String>> {
    Some(match id {
      b"INAM" => &mut self.title,
      b"IART" => &mut self.artist,
      b"IPRD" => &mut self.album,
      b"ITRK" => &mut self.track,
      b"IGNR" => &mut self.genre,
      b"ICMT" => &mut self.comment,
      b"ICRD" => &mut self.date,
      b"ISFT" => &mut self.software,
      b"ICOP" => &mut self.copyright,
      b"IENG" => &mut self.engineer,
      b"ISBJ" => &mut self.subject,
      _ => return None,
    })
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Id3v2Entity {
  pub version: String,
  pub size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XingKind {
  /// Conventionally written by VBR/ABR encoders.
  Xing,
  /// Same layout, conventionally written for CBR streams.
  Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XingHeader {
  pub kind: XingKind,
  pub frames: Option<u32>,
  pub bytes: Option<u32>,
}

/// Where `duration_sec` came from, most to least trustworthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DurationSource {
  /// Every MPEG frame in the buffer was walked; exact for the bytes supplied.
  Frames,
  /// Frame count from a Xing/Info header; whole-file truth even for a prefix.
  Xing,
  /// WAVE `fact` chunk sample count.
  Fact,
  /// WAVE `data` chunk size divided by the byte rate.
  Data,
}

/// Header-arithmetic walk over every MPEG frame. No decoding happens, so this
/// is exact for CBR and VBR alike and costs microseconds per megabyte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameWalk {
  pub frames: u64,
  /// PCM frames per channel.
  pub samples: u64,
  pub bytes: u64,
  /// Gaps between frames that had to be skipped byte-by-byte.
  pub resyncs: u32,
  /// The last header's frame runs past the end of the buffer (a prefix probe).
  pub truncated: bool,
  pub min_kbps: u32,
  pub max_kbps: u32,
  pub sample_rate: u32,
  pub channels: u32,
}

impl FrameWalk {
  pub fn cbr(&self) -> bool {
    self.min_kbps == self.max_kbps
  }

  pub fn duration_sec(&self) -> f64 {
    self.samples as f64 / f64::from(self.sample_rate)
  }

  /// Average bitrate in bits per second across the walked frames.
  pub fn bitrate_bps(&self) -> u32 {
    if self.samples == 0 {
      return 0;
    }
    ((self.bytes as f64 * 8.0 * f64::from(self.sample_rate)) / self.samples as f64).round() as u32
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mp3Meta {
  pub id3v2: Option<Id3v2Entity>,
  pub tags: Mp3Tags,
  /// First validated MPEG frame.
  pub frame: Option<MpegFrame>,
  pub xing: Option<XingHeader>,
  /// `None` when no frame was found.
  pub walk: Option<FrameWalk>,
  pub duration_sec: Option<f64>,
  pub duration_source: Option<DurationSource>,
  pub audio_offset: usize,
}

impl Mp3Meta {
  /// Constant bitrate: every walked frame shares one bitrate. With a single
  /// frame the Xing/Info kind decides; unknown otherwise.
  pub fn cbr(&self) -> Option<bool> {
    match (&self.walk, &self.xing) {
      (Some(walk), _) if walk.frames >= 2 => Some(walk.cbr()),
      (_, Some(xing)) => Some(xing.kind == XingKind::Info),
      (Some(_), None) => Some(true),
      (None, None) => None,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WavMeta {
  pub container: WavContainer,
  pub format_tag: u16,
  /// WAVE_FORMAT_EXTENSIBLE's SubFormat GUID leading tag, when present.
  pub sub_format: Option<u16>,
  pub format: String,
  pub channels: u16,
  pub sample_rate: u32,
  pub byte_rate: u32,
  pub block_align: u16,
  pub bits_per_sample: u16,
  pub data_offset: Option<usize>,
  /// Bytes of the data chunk actually present in the supplied buffer.
  pub data_size: Option<u64>,
  /// PCM frames per channel from `fact`, else `data_size / block_align`.
  pub sample_frames: Option<u64>,
  pub duration_sec: Option<f64>,
  pub duration_source: Option<DurationSource>,
  pub tags: WavTags,
}

impl WavMeta {
  /// Format name with WAVE_FORMAT_EXTENSIBLE resolved through its SubFormat.
  pub fn codec(&self) -> String {
    match self.sub_format {
      Some(tag) if self.format_tag == 0xfffe => wav_format(tag),
      _ => self.format.clone(),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioMeta {
  Mp3(Mp3Meta),
  Wav(WavMeta),
}

// ── Byte helpers ────────────────────────────────────────────────

fn eq(b: &[u8], offset: usize, value: &[u8]) -> bool {
  b.get(offset..offset.saturating_add(value.len())) == Some(value)
}

fn le16(b: &[u8], offset: usize) -> Option<u16> {
  let bytes = b.get(offset..offset.checked_add(2)?)?;
  Some(u16::from_le_bytes(bytes.try_into().ok()?))
}

fn le32(b: &[u8], offset: usize) -> Option<u32> {
  let bytes = b.get(offset..offset.checked_add(4)?)?;
  Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn le64(b: &[u8], offset: usize) -> Option<u64> {
  let bytes = b.get(offset..offset.checked_add(8)?)?;
  Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

fn be32(b: &[u8], offset: usize) -> Option<u32> {
  let bytes = b.get(offset..offset.checked_add(4)?)?;
  Some(u32::from_be_bytes(bytes.try_into().ok()?))
}

fn synchsafe(b: &[u8], offset: usize) -> Option<usize> {
  let bytes = b.get(offset..offset.checked_add(4)?)?;
  if bytes.iter().any(|v| v & 0x80 != 0) {
    return None;
  }
  Some(bytes.iter().fold(0, |acc, v| (acc << 7) | usize::from(*v)))
}

fn latin1(b: &[u8]) -> String {
  b.iter()
    .take_while(|v| **v != 0)
    .map(|v| char::from(*v))
    .collect::<String>()
    .trim()
    .to_owned()
}

/// True MPEG audio sync. AAC ADTS uses a 12-bit sync word, so the reserved
/// version (01) and layer (00) checks keep obvious ADTS headers out.
fn is_sync(b0: u8, b1: u8) -> bool {
  b0 == 0xff && b1 & 0xe0 == 0xe0 && b1 & 0x18 != 0x08 && b1 & 0x06 != 0
}

fn id3_size(b: &[u8]) -> Option<usize> {
  if b.len() < 10 || !eq(b, 0, b"ID3") {
    return None;
  }
  let footer = if b[3] == 4 && b[5] & 0x10 != 0 { 10 } else { 0 };
  Some(10 + synchsafe(b, 6)? + footer)
}

pub fn sniff(b: &[u8]) -> Option<AudioKind> {
  if b.len() >= 12 && (eq(b, 0, b"RIFF") || eq(b, 0, b"RF64")) && eq(b, 8, b"WAVE") {
    return Some(AudioKind::Wav);
  }
  // A short probe may hold only the ID3 tag, so ID3 alone classifies as MP3.
  // ID3v1-only files are rare but valid.
  if id3_size(b).is_some()
    || (b.len() >= 2 && is_sync(b[0], b[1]))
    || (b.len() >= 128 && eq(b, b.len() - 128, b"TAG"))
  {
    return Some(AudioKind::Mp3);
  }
  None
}

pub fn parse_audio(b: &[u8]) -> Result<AudioMeta, String> {
  match sniff(b) {
    Some(AudioKind::Wav) => parse_wav(b).map(AudioMeta::Wav),
    Some(AudioKind::Mp3) => parse_mp3(b).map(AudioMeta::Mp3),
    None => Err("unrecognized audio magic".into()),
  }
}

// ── WAV ─────────────────────────────────────────────────────────

fn wav_format(tag: u16) -> String {
  match tag {
    1 => "pcm".into(),
    3 => "ieee-float".into(),
    6 => "alaw".into(),
    7 => "mulaw".into(),
    0xfffe => "extensible".into(),
    other => format!("tag:{other}"),
  }
}

pub fn parse_wav(b: &[u8]) -> Result<WavMeta, String> {
  if sniff(b) != Some(AudioKind::Wav) {
    return Err("not a WAVE buffer".into());
  }
  let rf64 = eq(b, 0, b"RF64");
  // `sniff` guarantees at least 12 bytes, so the RIFF size read cannot fail.
  let riff_size = le32(b, 4).unwrap_or(0) as usize;
  let mut end = if rf64 {
    b.len()
  } else {
    b.len().min(8usize.saturating_add(riff_size))
  };

  let mut meta = WavMeta {
    container: if rf64 {
      WavContainer::Rf64
    } else {
      WavContainer::Riff
    },
    format_tag: 0,
    sub_format: None,
    format: "unknown".into(),
    channels: 0,
    sample_rate: 0,
    byte_rate: 0,
    block_align: 0,
    bits_per_sample: 0,
    data_offset: None,
    data_size: None,
    sample_frames: None,
    duration_sec: None,
    duration_source: None,
    tags: WavTags::default(),
  };

  let mut sample_count: Option<u64> = None;
  let mut rf64_data_size: Option<u64> = None;
  let mut size_table: Vec<([u8; 4], u64)> = Vec::new();
  let mut offset = 12usize;

  while offset.saturating_add(8) <= end {
    let Some(id) = b.get(offset..offset + 4) else {
      break;
    };
    let id: [u8; 4] = id.try_into().unwrap_or([0; 4]);
    let Some(raw) = le32(b, offset + 4) else {
      break;
    };
    let body = offset + 8;
    let declared = if rf64 && raw == u32::MAX {
      if &id == b"data" {
        rf64_data_size.unwrap_or(u64::from(raw))
      } else if let Some(i) = size_table.iter().position(|(key, _)| *key == id) {
        size_table.remove(i).1
      } else {
        u64::from(raw)
      }
    } else {
      u64::from(raw)
    };
    let size = declared.min((end - body) as u64) as usize;

    match &id {
      b"ds64" if rf64 && size >= 28 => {
        let riff = le64(b, body).unwrap_or(0);
        end = riff.saturating_add(8).min(b.len() as u64) as usize;
        if end < body + size {
          return Err("invalid RF64 ds64 RIFF size".into());
        }
        rf64_data_size = le64(b, body + 8);
        if let Some(n) = le64(b, body + 16).filter(|n| *n > 0) {
          sample_count = Some(n);
        }
        let entries = le32(b, body + 24).unwrap_or(0) as usize;
        for i in 0..entries.min((size - 28) / 12) {
          let p = body + 28 + i * 12;
          if let (Some(key), Some(n)) = (b.get(p..p + 4), le64(b, p + 4)) {
            size_table.push((key.try_into().unwrap_or([0; 4]), n));
          }
        }
      }
      b"fmt " if size >= 16 => {
        meta.format_tag = le16(b, body).unwrap_or(0);
        meta.channels = le16(b, body + 2).unwrap_or(0);
        meta.sample_rate = le32(b, body + 4).unwrap_or(0);
        meta.byte_rate = le32(b, body + 8).unwrap_or(0);
        meta.block_align = le16(b, body + 12).unwrap_or(0);
        meta.bits_per_sample = le16(b, body + 14).unwrap_or(0);
        meta.format = wav_format(meta.format_tag);
        // Extensible: cbSize(2) validBits(2) channelMask(4) SubFormat GUID(16),
        // whose first two bytes are the real format tag.
        if meta.format_tag == 0xfffe && size >= 40 {
          meta.sub_format = le16(b, body + 24);
        }
      }
      b"data" => {
        meta.data_offset = Some(body);
        meta.data_size = Some(size as u64);
      }
      b"fact" if size >= 4 => {
        if sample_count.is_none() {
          sample_count = le32(b, body).map(u64::from);
        }
      }
      b"LIST" if size >= 4 && eq(b, body, b"INFO") => {
        let mut p = body + 4;
        let stop = body + size;
        while p.saturating_add(8) <= stop {
          let Some(n) = le32(b, p + 4) else { break };
          let n = n as usize;
          let q = p + 8;
          if let Some(slot) = b.get(p..p + 4).and_then(|id| meta.tags.slot(id)) {
            *slot = Some(latin1(&b[q..q + n.min(stop - q)]));
          }
          let next = q.saturating_add(n).saturating_add(n & 1);
          if next <= p || next > stop {
            break;
          }
          p = next;
        }
      }
      _ => {}
    }

    let next = (body as u64)
      .checked_add(declared)
      .and_then(|n| n.checked_add(declared & 1));
    match next {
      Some(n) if n > offset as u64 && n <= end as u64 => offset = n as usize,
      _ => break,
    }
  }

  if let Some(n) = sample_count.filter(|_| meta.sample_rate > 0) {
    meta.sample_frames = Some(n);
    meta.duration_sec = Some(n as f64 / f64::from(meta.sample_rate));
    meta.duration_source = Some(DurationSource::Fact);
  } else if let Some(bytes) = meta.data_size.filter(|_| meta.byte_rate > 0) {
    if meta.block_align > 0 {
      meta.sample_frames = Some(bytes / u64::from(meta.block_align));
    }
    meta.duration_sec = Some(bytes as f64 / f64::from(meta.byte_rate));
    meta.duration_source = Some(DurationSource::Data);
  }
  Ok(meta)
}

// ── ID3 ─────────────────────────────────────────────────────────

fn unsync(b: &[u8]) -> Vec<u8> {
  let mut out = Vec::with_capacity(b.len());
  let mut i = 0;
  while i < b.len() {
    out.push(b[i]);
    if b[i] == 0xff && b.get(i + 1) == Some(&0) {
      i += 1;
    }
    i += 1;
  }
  out
}

fn text(encoding: u8, b: &[u8]) -> String {
  let s = match encoding {
    0 => encoding_rs::WINDOWS_1252.decode(b).0.into_owned(),
    1 => {
      let (decoder, body) = if b.starts_with(&[0xfe, 0xff]) {
        (encoding_rs::UTF_16BE, &b[2..])
      } else if b.starts_with(&[0xff, 0xfe]) {
        (encoding_rs::UTF_16LE, &b[2..])
      } else {
        (encoding_rs::UTF_16LE, b)
      };
      decoder.decode_without_bom_handling(body).0.into_owned()
    }
    2 => encoding_rs::UTF_16BE
      .decode_without_bom_handling(b)
      .0
      .into_owned(),
    3 => String::from_utf8_lossy(b).into_owned(),
    _ => latin1(b),
  };
  s.trim_end_matches('\0').trim().to_owned()
}

/// COMM / COM: `<encoding><lang:3><description>\0<text>`.
fn comment(b: &[u8]) -> Option<String> {
  if b.len() < 4 {
    return None;
  }
  let encoding = b[0];
  let body = &b[4..];
  let split = if encoding == 1 || encoding == 2 {
    body
      .as_chunks::<2>()
      .0
      .iter()
      .position(|pair| *pair == [0, 0])
      .map(|i| i * 2 + 2)
  } else {
    body.iter().position(|v| *v == 0).map(|i| i + 1)
  }?;
  // The BOM on a UTF-16 description also specifies the comment byte order.
  let data = &body[split..];
  if encoding == 1
    && body.starts_with(&[0xfe, 0xff])
    && !data.starts_with(&[0xfe, 0xff])
    && !data.starts_with(&[0xff, 0xfe])
  {
    Some(text(2, data))
  } else {
    Some(text(encoding, data))
  }
}

fn id3_text_key(id: &[u8]) -> Option<Mp3TagKey> {
  Some(match id {
    b"TIT2" | b"TT2" => Mp3TagKey::Title,
    b"TPE1" | b"TP1" => Mp3TagKey::Artist,
    b"TALB" | b"TAL" => Mp3TagKey::Album,
    b"TRCK" | b"TRK" => Mp3TagKey::Track,
    b"TYER" | b"TYE" | b"TDRC" => Mp3TagKey::Year,
    b"TCON" | b"TCO" => Mp3TagKey::Genre,
    _ => return None,
  })
}

fn id3v2(b: &[u8]) -> Option<(Id3v2Entity, Mp3Tags)> {
  let size = id3_size(b)?;
  let major = b[3];
  if !(2..=4).contains(&major) {
    return None;
  }
  let entity = Id3v2Entity {
    version: format!("2.{major}.{}", b[4]),
    size,
  };
  let payload = synchsafe(b, 6)?;
  let end = b.len().min(10 + payload);
  let flags = b[5];
  let storage = if major < 4 && flags & 0x80 != 0 {
    unsync(&b[10..end])
  } else {
    b[10..end].to_vec()
  };
  let buf = storage.as_slice();
  let mut tags = Mp3Tags::default();
  let mut offset = 0usize;

  // v2.2's 0x40 flag means compression, not an extended header.
  if major == 2 && flags & 0x40 != 0 {
    return Some((entity, tags));
  }
  if major != 2 && flags & 0x40 != 0 {
    let n = if major == 3 {
      4usize.checked_add(be32(buf, 0)? as usize)?
    } else {
      synchsafe(buf, 0)?
    };
    if n < if major == 3 { 10 } else { 6 } || n > buf.len() {
      return None;
    }
    offset = n;
  }

  let header_size = if major == 2 { 6 } else { 10 };
  let id_len = if major == 2 { 3 } else { 4 };
  while offset.saturating_add(header_size) <= buf.len() && buf[offset] != 0 {
    let id = &buf[offset..offset + id_len];
    if !id
      .iter()
      .all(|v| v.is_ascii_uppercase() || v.is_ascii_digit())
    {
      break;
    }
    let n = if major == 2 {
      (usize::from(buf[offset + 3]) << 16)
        | (usize::from(buf[offset + 4]) << 8)
        | usize::from(buf[offset + 5])
    } else if major == 4 {
      synchsafe(buf, offset + 4)?
    } else {
      be32(buf, offset + 4)? as usize
    };
    let start = offset + header_size;
    let next = start.checked_add(n)?;
    if next > buf.len() {
      break;
    }
    let frame_flags = if major == 2 { 0 } else { buf[offset + 9] };
    let unsupported = if major == 3 {
      frame_flags & 0xc0 != 0
    } else {
      frame_flags & 0x0c != 0
    };
    if !unsupported {
      let raw = &buf[start..next];
      let unsynced = if major == 4 && (flags & 0x80 != 0 || frame_flags & 0x02 != 0) {
        Some(unsync(raw))
      } else {
        None
      };
      let mut data = unsynced.as_deref().unwrap_or(raw);
      let prefix = if major == 3 {
        usize::from(frame_flags & 0x20 != 0)
      } else {
        usize::from(frame_flags & 0x40 != 0) + if frame_flags & 0x01 != 0 { 4 } else { 0 }
      };
      if prefix <= data.len() {
        data = &data[prefix..];
        if let Some(key) = id3_text_key(id) {
          if let Some((&encoding, value)) = data.split_first() {
            tags.set(key, text(encoding, value));
          }
        } else if (id == b"COMM" || id == b"COM")
          && tags.comment.is_none()
          && let Some(value) = comment(data)
        {
          tags.comment = Some(value);
        }
      }
    }
    offset = next;
  }
  Some((entity, tags))
}

fn id3v1(b: &[u8]) -> Option<Mp3Tags> {
  if b.len() < 128 {
    return None;
  }
  let t = &b[b.len() - 128..];
  if &t[..3] != b"TAG" {
    return None;
  }
  let has_track = t[125] == 0 && t[126] != 0;
  let mut tags = Mp3Tags::default();
  let fields = [
    (Mp3TagKey::Title, 3, 30),
    (Mp3TagKey::Artist, 33, 30),
    (Mp3TagKey::Album, 63, 30),
    (Mp3TagKey::Year, 93, 4),
    (Mp3TagKey::Comment, 97, if has_track { 28 } else { 30 }),
  ];
  for (key, start, len) in fields {
    let value = latin1(&t[start..start + len]);
    if !value.is_empty() {
      tags.set(key, value);
    }
  }
  if has_track {
    tags.track = Some(t[126].to_string());
  }
  Some(tags)
}

// ── MPEG frames ─────────────────────────────────────────────────

const BITRATES_V1_L1: [u32; 15] = [
  0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
];
const BITRATES_V1_L2: [u32; 15] = [
  0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
const BITRATES_V1_L3: [u32; 15] = [
  0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const BITRATES_V2_L1: [u32; 15] = [
  0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
];
const BITRATES_V2_L23: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

pub fn mpeg_frame(b: &[u8], offset: usize) -> Option<MpegFrame> {
  let s = b.get(offset..offset.checked_add(4)?)?;
  if !is_sync(s[0], s[1]) {
    return None;
  }
  let version = match (s[1] >> 3) & 3 {
    3 => MpegVersion::V1,
    2 => MpegVersion::V2,
    0 => MpegVersion::V2_5,
    _ => return None,
  };
  let layer = match (s[1] >> 1) & 3 {
    3 => MpegLayer::I,
    2 => MpegLayer::II,
    1 => MpegLayer::III,
    _ => return None,
  };
  let bitrate_index = usize::from((s[2] >> 4) & 15);
  let sample_rate_index = usize::from((s[2] >> 2) & 3);
  if !(1..=14).contains(&bitrate_index) || sample_rate_index > 2 {
    return None;
  }
  let table = match (version, layer) {
    (MpegVersion::V1, MpegLayer::I) => BITRATES_V1_L1,
    (MpegVersion::V1, MpegLayer::II) => BITRATES_V1_L2,
    (MpegVersion::V1, MpegLayer::III) => BITRATES_V1_L3,
    (_, MpegLayer::I) => BITRATES_V2_L1,
    _ => BITRATES_V2_L23,
  };
  let bitrate_kbps = table[bitrate_index];
  let sample_rate = match version {
    MpegVersion::V1 => [44100, 48000, 32000],
    MpegVersion::V2 => [22050, 24000, 16000],
    MpegVersion::V2_5 => [11025, 12000, 8000],
  }[sample_rate_index];
  let channel_mode = match (s[3] >> 6) & 3 {
    0 => MpegChannelMode::Stereo,
    1 => MpegChannelMode::JointStereo,
    2 => MpegChannelMode::DualChannel,
    _ => MpegChannelMode::Mono,
  };
  let padding = s[2] & 2 != 0;
  let low_rate_l3 = layer == MpegLayer::III && version != MpegVersion::V1;
  let samples_per_frame = match layer {
    MpegLayer::I => 384,
    MpegLayer::III if low_rate_l3 => 576,
    _ => 1152,
  };
  let bitrate = bitrate_kbps * 1000;
  let frame_size = if layer == MpegLayer::I {
    // ((12 * bitrate / sampleRate) + padding) * 4
    ((12 * bitrate / sample_rate) + u32::from(padding)) * 4
  } else {
    // MPEG-2/2.5 Layer III: 72 * bitrate / sampleRate; everything else: 144 * ...
    let coefficient = if low_rate_l3 { 72 } else { 144 };
    (coefficient * bitrate / sample_rate) + u32::from(padding)
  } as usize;

  Some(MpegFrame {
    offset,
    version,
    layer,
    bitrate_kbps,
    sample_rate,
    channels: channel_mode.channels(),
    channel_mode,
    padding,
    // Protection bit: 0 = CRC follows header, 1 = no CRC.
    has_crc: s[1] & 1 == 0,
    samples_per_frame,
    frame_size,
  })
}

/// Scan up to 64 KiB for a frame whose successor is also a valid header with
/// the same version/layer/rate, which cheaply rejects false sync words.
fn find_frame(b: &[u8], start: usize) -> Option<MpegFrame> {
  if b.len() < 4 {
    return None;
  }
  let last = (b.len() - 4).min(start.saturating_add(65536));
  for offset in start..=last {
    if b[offset] != 0xff {
      continue;
    }
    let Some(frame) = mpeg_frame(b, offset) else {
      continue;
    };
    let next = offset.saturating_add(frame.frame_size);
    if next.saturating_add(4) > b.len()
      || mpeg_frame(b, next).is_some_and(|n| {
        n.version == frame.version && n.layer == frame.layer && n.sample_rate == frame.sample_rate
      })
    {
      return Some(frame);
    }
  }
  None
}

/// Xing / Info lives after the MPEG header, optional CRC and Layer III side
/// info.
fn xing(b: &[u8], frame: &MpegFrame) -> Option<XingHeader> {
  if frame.layer != MpegLayer::III {
    return None;
  }
  let side_info = match (frame.version, frame.channels) {
    (MpegVersion::V1, 1) => 17,
    (MpegVersion::V1, _) => 32,
    (_, 1) => 9,
    _ => 17,
  };
  let offset = frame.offset + 4 + if frame.has_crc { 2 } else { 0 } + side_info;
  let stop = b.len().min(frame.offset + frame.frame_size);
  let b = &b[..stop];
  let kind = if eq(b, offset, b"Xing") {
    XingKind::Xing
  } else if eq(b, offset, b"Info") {
    XingKind::Info
  } else {
    return None;
  };
  let flags = be32(b, offset + 4)?;
  let mut cursor = offset + 8;
  let mut frames = None;
  let mut bytes = None;
  if flags & 1 != 0 {
    frames = Some(be32(b, cursor)?);
    cursor += 4;
  }
  if flags & 2 != 0 {
    bytes = Some(be32(b, cursor)?);
  }
  Some(XingHeader {
    kind,
    frames,
    bytes,
  })
}

/// Hop frame-to-frame on header arithmetic from `start` to `end`. Bytes that
/// do not parse as a header are skipped one at a time; a gap followed by a
/// valid frame counts as one resync.
fn walk_frames(b: &[u8], start: usize, end: usize) -> Option<FrameWalk> {
  let end = end.min(b.len());
  let mut walk = FrameWalk {
    frames: 0,
    samples: 0,
    bytes: 0,
    resyncs: 0,
    truncated: false,
    min_kbps: u32::MAX,
    max_kbps: 0,
    sample_rate: 0,
    channels: 0,
  };
  let mut offset = start;
  let mut in_gap = false;
  while offset.saturating_add(4) <= end {
    let Some(frame) = mpeg_frame(b, offset) else {
      in_gap = true;
      offset += 1;
      continue;
    };
    if offset + frame.frame_size > end {
      walk.truncated = true;
      break;
    }
    if in_gap && walk.frames > 0 {
      walk.resyncs += 1;
    }
    in_gap = false;
    walk.frames += 1;
    walk.samples += u64::from(frame.samples_per_frame);
    walk.bytes += frame.frame_size as u64;
    walk.min_kbps = walk.min_kbps.min(frame.bitrate_kbps);
    walk.max_kbps = walk.max_kbps.max(frame.bitrate_kbps);
    walk.sample_rate = frame.sample_rate;
    walk.channels = frame.channels;
    offset += frame.frame_size;
  }
  (walk.frames > 0).then_some(walk)
}

pub fn parse_mp3(b: &[u8]) -> Result<Mp3Meta, String> {
  if sniff(b) != Some(AudioKind::Mp3) {
    return Err("not an MP3 buffer".into());
  }
  let v1 = id3v1(b);
  let v2 = id3v2(b);
  // ID3v2 wins over ID3v1 when both provide the same field.
  let mut tags = v1.clone().unwrap_or_default();
  let mut id3v2 = None;
  if let Some((entity, v2_tags)) = v2 {
    tags = tags.overlay(v2_tags);
    id3v2 = Some(entity);
  }
  // Skip even unsupported ID3 revisions so tag bytes are never scanned as audio.
  let audio_offset = id3_size(b).unwrap_or(0);
  let audio_end = b.len().saturating_sub(if v1.is_some() { 128 } else { 0 });
  let frame = find_frame(b, audio_offset);
  let xing = frame.as_ref().and_then(|f| xing(b, f));
  // The Xing/Info frame carries no audio and decoders skip it, so the walk
  // starts after it; its frame count then matches the header's.
  let walk = frame.as_ref().and_then(|f| {
    let start = if xing.is_some() {
      f.offset + f.frame_size
    } else {
      f.offset
    };
    walk_frames(b, start, audio_end)
  });

  let mut duration_sec = None;
  let mut duration_source = None;
  if let (Some(frame), Some(header)) = (&frame, &xing)
    && let Some(frames) = header.frames
    && walk.as_ref().is_none_or(|w| w.truncated)
  {
    // A prefix probe: the header knows the whole file, the walk does not.
    duration_sec =
      Some(f64::from(frames) * f64::from(frame.samples_per_frame) / f64::from(frame.sample_rate));
    duration_source = Some(DurationSource::Xing);
  } else if let Some(walk) = &walk {
    duration_sec = Some(walk.duration_sec());
    duration_source = Some(DurationSource::Frames);
  }

  Ok(Mp3Meta {
    id3v2,
    tags,
    frame,
    xing,
    walk,
    duration_sec,
    duration_source,
    audio_offset,
  })
}

// ── RF64 ────────────────────────────────────────────────────────

/// Rewrite an owned RF64 buffer into a RIFF buffer in place so Symphonia's
/// WAVE reader accepts it. Limited to RIFF's 32-bit size range.
pub(crate) fn normalize_rf64(b: &mut [u8]) -> Result<(), String> {
  if !eq(b, 0, b"RF64") {
    return Ok(());
  }
  if b.len() < 48 || !eq(b, 12, b"ds64") {
    return Err("RF64 decoding requires a leading ds64 chunk".into());
  }
  let ds_size = le32(b, 16).ok_or("truncated RF64 ds64")? as usize;
  if ds_size < 28 || 20usize.saturating_add(ds_size) > b.len() {
    return Err("invalid RF64 ds64 size".into());
  }
  let riff_size = le64(b, 20).ok_or("truncated RF64 ds64")?;
  let data_size = le64(b, 28).ok_or("truncated RF64 ds64")?;
  let riff =
    u32::try_from(riff_size).map_err(|_| "RF64 decoding supports buffers smaller than 4 GiB")?;
  let end = riff_size.checked_add(8).ok_or("RF64 size overflow")?;
  if end > b.len() as u64 || end < 20 + ds_size as u64 {
    return Err("truncated RF64 buffer".into());
  }
  let entries = le32(b, 44).ok_or("truncated RF64 ds64")? as usize;
  if entries > (ds_size - 28) / 12 {
    return Err("truncated RF64 size table".into());
  }
  let mut table = Vec::with_capacity(entries);
  for i in 0..entries {
    let p = 48 + i * 12;
    let key: [u8; 4] = b
      .get(p..p + 4)
      .and_then(|k| k.try_into().ok())
      .ok_or("truncated RF64 size table")?;
    table.push((key, le64(b, p + 4).ok_or("truncated RF64 size table")?));
  }

  let mut offset = 12usize;
  while offset.saturating_add(8) <= end as usize {
    let id: [u8; 4] = b
      .get(offset..offset + 4)
      .and_then(|k| k.try_into().ok())
      .ok_or("truncated RF64 chunk header")?;
    let raw = le32(b, offset + 4).ok_or("truncated RF64 chunk header")?;
    let body = offset + 8;
    let size = if raw == u32::MAX {
      let n = if &id == b"data" {
        data_size
      } else {
        let i = table
          .iter()
          .position(|(key, _)| *key == id)
          .ok_or("RF64 chunk has no ds64 size table entry")?;
        table.remove(i).1
      };
      let n32 = u32::try_from(n).map_err(|_| "RF64 decoding supports chunks smaller than 4 GiB")?;
      b[offset + 4..offset + 8].copy_from_slice(&n32.to_le_bytes());
      n
    } else {
      u64::from(raw)
    };
    let body_end = (body as u64)
      .checked_add(size)
      .ok_or("RF64 chunk size overflow")?;
    if body_end > end {
      return Err("truncated RF64 audio chunk".into());
    }
    offset = body_end.saturating_add(size & 1) as usize;
  }
  b[..4].copy_from_slice(b"RIFF");
  b[4..8].copy_from_slice(&riff.to_le_bytes());
  Ok(())
}
