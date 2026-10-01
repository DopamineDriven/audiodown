# audiodown design

Date: 2026-09-30

## Goal

`@d0paminedriven/audiodown` is a napi-rs 3 native addon for Node that parses
MP3/WAV metadata, decodes MP3/WAV to interleaved `f32` PCM, and reduces that
PCM to exhaustive waveform envelopes (min/max/peak/RMS per bucket). The first
consumer is a websocket server that inspects Lyria-generated audio and needs a
decoded waveform array, not a bitrate estimate.

The package mirrors `@d0paminedriven/pdfdown` (`~/rust/napi-rs`) in layout,
tooling and CI so the two projects feel identical to work in.

## Non-goals

- Browser or WASM-only decoding paths beyond the `wasm32-wasip1-threads`
  target napi-rs already builds.
- Codecs other than MPEG Layer I/II/III and RIFF/RF64 WAVE (PCM, float,
  A-law, mu-law, ADPCM).
- Resampling, downmixing, normalization, loudness or true-peak metering.

## Layout

```
.cargo/config.toml            windows crt-static (as pdfdown)
.github/renovate.json         as pdfdown
.github/workflows/CI.yml      pdfdown CI minus OCR/render jobs, APP_NAME=audiodown
.husky/pre-commit             lint-staged
.yarn/releases/yarn-4.12.0.cjs
__test__/index.spec.ts        ava tests against local fixtures
__test__/fixtures/            wav/mp3 fixtures + FFmpeg f32 references
benchmark/bench.ts            tinybench over fixtures
scripts/fixtures.py           regenerates fixtures with FFmpeg
src/lib.rs                    napi functions, AsyncTask impls, AudioDown and AudioPcm classes
src/types.rs                  #[napi(object)] boundary types + From<core> impls
src/core/mod.rs
src/core/metadata.rs          sniff, parse_mp3, parse_wav, normalize_rf64 (pure Rust)
src/core/decode.rs            Symphonia decode -> Pcm (sequential)
src/core/waveform.rs          rayon bucket reduction
src/core/tests.rs             #[cfg(test)] unit tests, so plain `cargo test` works
```

`src/core` never imports napi. napi 3's default `dyn-symbols` feature resolves
Node-API symbols at load time, so `cargo test` links the bindings as-is with
no feature gating. GPT's original drop is left untouched in `audio-metadata-rs/` and is
git/prettier/oxlint-ignored until it is deleted.

## Public API

All types are emitted by napi-rs into `index.d.ts`; the only hand-written
piece is `scripts/dts-header.txt`, which adds the `AudioMeta`,
`ExpandedAudioSpecs`, `Mp3Meta` and `WavMeta` aliases for parity with the
TypeScript package. The primary entry point is the `AudioService` class, which
has the same shape as the TypeScript `AudioService` (`parseAudio`, `parseMp3`,
`parseWav`) and adds decoding, waveform reduction and async variants. The
standalone functions are the same code; `AudioDown` is the per-buffer
"parse once, call many" handle in the style of `PdfDown`; `AudioPcm` keeps
decoded PCM in Rust so several resolutions cost one decode.

### `AudioSpecs` (what every parse call returns)

Pipeline compatibility drove the shape: one flat interface with nullable
fields, like `ImageSpecs`, rather than a discriminated union. It carries
the `Expanded*Specs` fields (`type`, `format`, `contentType`, `byteSize`,
`fetchedBytes`, `source`, `metadata`), the Prisma `AudioMetadata` columns
(`durationMs`, `bitrate`, `sampleRate`, `channels`, `codec`, `title`,
`artist`, `album`, `year`, `genre`) and the ws-server `mp3Specs` fields
(`mime`, `ext`, `size`, `cbr`, `frames`, bps `bitrate`). `kind` narrows it;
MP3-only detail (`id3v2`, `frame`, `xing`, `audioOffset`) and WAVE-only detail
(`formatTag`, `byteRate`, `blockAlign`, `dataOffset`, `dataSize`) are `null`
on the other container. `tags` keeps every raw tag, `metadata` is the
`Record<string, string>` bag of present tags.

MP3 duration is exact without decoding: a header-arithmetic walk over every
frame (`durationSource: 'frames'`). A prefix probe sets `truncated` and falls
back to the Xing/Info frame count (`'xing'`). WAVE reports `'fact'` or
`'data'`. The Xing/Info frame itself is skipped by the walk so its count
matches the header. `codec` resolves WAVE_FORMAT_EXTENSIBLE through its
SubFormat GUID.

```ts
class AudioService {
  constructor(defaults?: WaveformOptions)   // validated eagerly; per-call options override
  sniffAudio(data): AudioKind | null
  parseAudio / parseAudioAsync (data, source?): AudioSpecs
  parseMp3 / parseMp3Async (data, source?): Mp3Meta
  parseWav / parseWavAsync (data, source?): WavMeta
  decodeAudio / decodeAudioAsync (data, opts?): DecodedAudio
  waveformPeaks / waveformPeaksAsync (data, opts?): WaveformPeaks
  waveformFromPcm / waveformFromPcmAsync (samples, sampleRate, channels, opts?)
  analyzeAudio / analyzeAudioAsync (data, opts?, source?): { specs, waveform }
  open(data, source?): AudioDown
}
class AudioDown { kind, byteSize, source; metadata(); decode(); waveform(); analyze(); pcm(); + Async }
class AudioPcm { sampleRate, channels, frameCount, durationSec; samples(); decoded(); waveform(); waveformAsync() }
```

`WaveformPeaks.envelope` is `max(channel.peak[i])` across channels, the
single-line display envelope a `waveformPeaks` column wants.

When `AudioService` is constructed with defaults, a call that supplies either
resolution selector (`peakCount` or `samplesPerPeak`) replaces both default
selectors; every other option merges field by field.

Inputs are `&[u8]` / `&[f32]` so both `Buffer` and plain typed arrays work
and `byteOffset` is honoured. Byte offsets and sizes are `f64` on the JS side;
counts bounded by `u32` (frames, peaks, channels) are `u32`. Numeric options
are received as `f64` and validated (finite, integral, in range) so `1.5`,
`NaN` and `Infinity` are rejected with `InvalidArg` instead of being silently
truncated. `peakCount` and `samplesPerPeak` are mutually exclusive at runtime.

## Core

### metadata

Ported from GPT's `metadata.rs`, which is a superset of the TypeScript
`AudioService`: ID3v2.2/2.3/2.4 with unsynchronisation, extended headers,
frame flags, UTF-16 BOM handling for COMM; ID3v1; Xing/Info; RF64 `ds64`
tables. Output uses typed structs (`Mp3Tags`, `WavTags` with `Option<String>`
fields; enums for version/layer/mode/container) instead of `BTreeMap`.
Added over GPT's version: `walk_frames` (exact MPEG duration, average bps,
CBR detection, resync count, truncation flag), a full `XingHeader`,
`DurationSource`, WAVE `sample_frames` and EXTENSIBLE sub-format resolution.

### decode

Symphonia 0.5.5 (`mpa`, `wav`, `pcm`, `adpcm`). `decode(Arc<Vec<u8>>, DecodeConfig)`
takes shared bytes so the `AudioDown` class never re-copies input; RF64 is
normalised to RIFF on a private copy only when needed. Decoding is sequential
by design: MPEG Layer III frames depend on the bit reservoir. Guards:
`maxDecodedSamples` cap, mid-stream spec changes, non-finite samples,
`strict` vs skip-on-decode-error.

### waveform (rayon)

Every sample lands in exactly one bucket. `boundaries` is `peakCount + 1`
frame offsets. Reduction is one pass over interleaved frames per bucket that
updates all channels at once (one read of memory, no per-channel passes), run
with `par_chunks_mut(channels)` over a flat stats vector so there is no
per-bucket allocation. The non-finite check is folded into the same pass.
Serial and parallel produce identical output because each bucket is reduced
sequentially. `parallel` defaults to true and kicks in above 262,144 samples.
`analyzeAudio` runs metadata parsing and decode+reduce under `rayon::join`.

## Errors

Option validation -> `Status::InvalidArg`. Parse/decode/reduce failures ->
`Status::GenericFailure` with the core's message. Async failures reject the
promise; invalid options throw synchronously before a task is queued.

## Testing

- `cargo test`: unit tests in `src/core/tests.rs` ported from GPT (format
  sniffing, truncated probes, deterministic garbage, ID3 versions and
  comments, exact envelopes, serial/parallel parity, WAV/RF64/MP3 decode,
  FFmpeg reference comparison).
- `yarn test` (ava): the Node tests ported from GPT plus the class API,
  typed-array lifetime, async snapshotting and option validation.
- `yarn bench`: tinybench over the fixtures.
- CI: pdfdown's lint/build/test/publish matrix with `cargo test` added to
  the lint job.
