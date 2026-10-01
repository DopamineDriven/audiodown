# `@d0paminedriven/audiodown`

![CI](https://github.com/DopamineDriven/audiodown/workflows/CI/badge.svg)

Rust-powered MP3/WAV inspection for Node.js via [napi-rs](https://napi.rs). Parses metadata (ID3v1/v2, Xing/Info, RIFF/RF64 `fmt`/`fact`/`LIST`) without decoding, decodes MP3 and WAVE to interleaved `f32` PCM with [Symphonia](https://github.com/pdeljanov/Symphonia), and reduces that PCM to exhaustive waveform envelopes (min/max/peak/RMS per bucket) on a [rayon](https://github.com/rayon-rs/rayon) pool. Every sample is visited; a one-sample transient survives any bucket size.

It is the Rust successor to the TypeScript `AudioService`, and its output is shaped to drop straight into an `ImageSpecs`/`DocSpecs`-style pipeline (`type`, `format`, `contentType`, `byteSize`, `fetchedBytes`, `source`, …) and a Prisma `AudioMetadata` row (`duration` in ms, `bitrate`, `sampleRate`, `channels`, `codec`, `title`/`artist`/`album`/`year`/`genre`, `waveformPeaks`).

## Install

```bash
npm install @d0paminedriven/audiodown
# or
yarn add @d0paminedriven/audiodown
# or
pnpm add @d0paminedriven/audiodown
```

No FFmpeg, system codec or C toolchain is needed at runtime. Prebuilt binaries ship for Linux (gnu/musl, x64/arm64/armv7), macOS (x64/arm64), Windows (x64/x86/arm64), FreeBSD, Android and `wasm32-wasip1-threads`.

## Quick start

```typescript
import { readFile } from 'fs/promises'
import { AudioService } from '@d0paminedriven/audiodown'

const audio = new AudioService()
const buffer = await readFile('track.mp3')

// Headers only; microseconds. Exact MP3 duration from a frame walk, no decoding.
const specs = audio.parseAudio(buffer, 'https://cdn.example/track.mp3')
specs.kind // 'mp3' | 'wav'
specs.durationMs // 114651
specs.bitrate // 128000 (bps, averaged over every frame)
specs.title // 'Precision fixture' | null

// Decode + reduce off the event loop; metadata and decode run under rayon::join.
const { specs: meta, waveform } = await audio.analyzeAudioAsync(buffer, { peakCount: 2048 })
waveform.envelope // Float32Array(2048): max |peak| across channels per bucket
waveform.channels[0].rms // Float64Array(2048)
```

## API

### `AudioService` class

Stateless, construct once. Every method is also exported as a standalone function with the same signature (minus `this`). Constructor defaults are merged under per-call options; a call that names either resolution selector (`peakCount` or `samplesPerPeak`) replaces both defaults.

```typescript
export declare class AudioService {
  constructor(defaults?: WaveformOptions)
  get defaults(): WaveformOptions

  sniffAudio(data: Uint8Array): AudioKind | null

  parseAudio(data: Uint8Array, source?: string): AudioSpecs
  parseAudioAsync(data: Uint8Array, source?: string): Promise<AudioSpecs>
  parseMp3(data: Uint8Array, source?: string): Mp3Meta
  parseMp3Async(data: Uint8Array, source?: string): Promise<Mp3Meta>
  parseWav(data: Uint8Array, source?: string): WavMeta
  parseWavAsync(data: Uint8Array, source?: string): Promise<WavMeta>

  decodeAudio(data: Uint8Array, options?: DecodeOptions): DecodedAudio
  decodeAudioAsync(data: Uint8Array, options?: DecodeOptions): Promise<DecodedAudio>

  waveformPeaks(data: Uint8Array, options?: WaveformOptions): WaveformPeaks
  waveformPeaksAsync(data: Uint8Array, options?: WaveformOptions): Promise<WaveformPeaks>
  waveformFromPcm(samples: Float32Array, sampleRate: number, channels: number, options?: WaveformOptions): WaveformPeaks
  waveformFromPcmAsync(
    samples: Float32Array,
    sampleRate: number,
    channels: number,
    options?: WaveformOptions,
  ): Promise<WaveformPeaks>

  analyzeAudio(data: Uint8Array, options?: WaveformOptions, source?: string): AudioAnalysis
  analyzeAudioAsync(data: Uint8Array, options?: WaveformOptions, source?: string): Promise<AudioAnalysis>

  /** Snapshot a buffer once and reuse it; inherits this service's defaults. */
  open(data: Uint8Array, source?: string): AudioDown
}
```

Inputs are `Uint8Array`, so `Buffer` and sliced views work and `byteOffset` is honoured. Sync methods borrow the bytes with no copy. Async methods copy once before dispatch, so mutating the caller's buffer afterwards cannot race the worker. All CPU work after that copy runs on the libuv pool.

### `AudioDown` class

Parse once, call many, in the style of `PdfDown`. The buffer is copied exactly once into shared memory; sync methods borrow it and async methods share it with the worker through `Arc`, so there is no per-call re-copy.

```typescript
export declare class AudioDown {
  constructor(data: Uint8Array, source?: string)
  get kind(): AudioKind
  get byteSize(): number
  get source(): string | null
  metadata(): AudioSpecs
  metadataAsync(): Promise<AudioSpecs>
  decode(options?: DecodeOptions): DecodedAudio
  decodeAsync(options?: DecodeOptions): Promise<DecodedAudio>
  waveform(options?: WaveformOptions): WaveformPeaks
  waveformAsync(options?: WaveformOptions): Promise<WaveformPeaks>
  analyze(options?: WaveformOptions): AudioAnalysis
  analyzeAsync(options?: WaveformOptions): Promise<AudioAnalysis>
  /** Decode once into an AudioPcm handle; reduce it at any number of resolutions. */
  pcm(options?: DecodeOptions): AudioPcm
  pcmAsync(options?: DecodeOptions): Promise<AudioPcm>
}
```

### `AudioPcm` class

Interleaved `f32` PCM held in Rust. Reduce it at several resolutions without the samples ever crossing into JS. Build one from `AudioDown.pcm()` or from your own `Float32Array` (for example raw PCM from a realtime generation stream).

```typescript
export declare class AudioPcm {
  constructor(samples: Float32Array, sampleRate: number, channels: number, defaults?: WaveformOptions)
  get sampleRate(): number
  get channels(): number
  get frameCount(): number
  get durationSec(): number
  get gaplessEnabled(): boolean
  get skippedPackets(): number
  samples(): Float32Array // copy
  decoded(): DecodedAudio // copy
  waveform(options?: WaveformOptions): WaveformPeaks
  waveformAsync(options?: WaveformOptions): Promise<WaveformPeaks>
}
```

## Types

### `AudioSpecs`

One flat, nullable shape for both containers; `kind` narrows it. The aliases `AudioMeta`, `ExpandedAudioSpecs`, `Mp3Meta` (`kind: 'mp3'`) and `WavMeta` (`kind: 'wav'`) are exported for parity with the TypeScript package.

| Field                                                           | Value                                                                        | Parity                                                        |
| --------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `type`                                                          | `'AUDIO'`                                                                    | `ImageSpecs.type`, `DocSpecs.type`                            |
| `kind`, `format`, `ext`                                         | `'mp3' \| 'wav'`                                                             | `ImageSpecs.format`, asset `ext`                              |
| `mime`, `mimeType`, `contentType`                               | `'audio/mpeg' \| 'audio/wav'`                                                | `mp3Specs.mime`, `DocSpecs.mimeType`, `Expanded*.contentType` |
| `codec`                                                         | `'mp3' \| 'mp2' \| 'mp1' \| 'pcm' \| 'ieee-float' \| 'alaw' \| 'mulaw' \| …` | Prisma `codec`; EXTENSIBLE resolved via SubFormat             |
| `container`                                                     | `'mpeg' \| 'RIFF' \| 'RF64'`                                                 |                                                               |
| `byteSize`, `fetchedBytes`, `size`                              | bytes parsed                                                                 | `Expanded*.byteSize/fetchedBytes`, `mp3Specs.size`            |
| `source?`                                                       | passed through                                                               | `Expanded*.source`                                            |
| `durationSec`, `durationMs`                                     | `number \| null`                                                             | Prisma `duration` (ms)                                        |
| `durationSource`                                                | `'frames' \| 'xing' \| 'fact' \| 'data' \| null`                             | how trustworthy the duration is                               |
| `sampleRate`, `channels`, `bitsPerSample`                       | `number \| null`                                                             | Prisma `sampleRate`, `channels`                               |
| `bitrate`                                                       | average **bps** over every frame                                             | `mp3Specs.bitrate`                                            |
| `bitrateKbps`                                                   | nominal kbps of the first frame / byte rate                                  | the old `frame.bitrateKbps`                                   |
| `cbr`, `vbr`                                                    | `boolean \| null`                                                            | `mp3Specs.cbr`                                                |
| `frames`, `sampleFrames`, `resyncs`, `truncated`                | frame-walk results (MP3); `sampleFrames` also for WAVE                       | `mp3Specs.frames`                                             |
| `title`, `artist`, `album`, `genre`, `year`                     | flattened, `year` as a 4-digit number                                        | Prisma columns                                                |
| `tags`                                                          | `AudioTags`: every raw ID3 / LIST-INFO tag                                   | the old `tags`                                                |
| `metadata?`                                                     | `Record<string, string>` of present tags                                     | `ImageSpecs.metadata`                                         |
| `id3v2`, `frame`, `xing`, `audioOffset`                         | MP3 detail (`null` for WAVE)                                                 | the old `Mp3Meta`                                             |
| `formatTag`, `byteRate`, `blockAlign`, `dataOffset`, `dataSize` | WAVE detail (`null` for MP3)                                                 | the old `WavMeta`                                             |

MP3 duration comes from walking every frame header (`durationSource: 'frames'`), which is exact for CBR and VBR alike and needs no decoding. When only a prefix of the file was supplied (`truncated: true`), the Xing/Info frame count is used instead (`'xing'`) because it describes the whole file. WAVE uses `fact` samples, else `data` bytes over the byte rate. `bitrate` is bits per second averaged over the walked bytes; `bitrateKbps` is the nominal rate of the first frame. Both conventions exist in the wild, so both are provided.

### `DecodedAudio`, `WaveformPeaks`, options

```typescript
export interface DecodeOptions {
  gapless?: boolean // trim encoder delay/padding when metadata allows. Default true
  strict?: boolean // reject decoder errors instead of skipping packets. Default true
  maxDecodedSamples?: number // cap on interleaved scalar samples. Default 64_000_000
}

export interface WaveformOptions extends DecodeOptions {
  peakCount?: number // target buckets, capped at frame count. Default 1024
  samplesPerPeak?: number // fixed frames per bucket; exclusive with peakCount
  parallel?: boolean // rayon for large inputs. Default true
}

export interface DecodedAudio {
  samples: Float32Array // interleaved: L0 R0 L1 R1 ...
  sampleRate: number
  channels: number
  frameCount: number // per channel
  durationSec: number
  gaplessEnabled: boolean
  skippedPackets: number
}

export interface WaveformChannel {
  min: Float32Array
  max: Float32Array
  peak: Float32Array // max(|min|, |max|)
  rms: Float64Array // sqrt(sum(sample²) / frames), accumulated in f64
}

export interface WaveformPeaks {
  sampleRate: number
  channelCount: number
  frameCount: number
  durationSec: number // decoded: frameCount / sampleRate
  peakCount: number // actual bucket count
  boundaries: Uint32Array // peakCount + 1 frame offsets
  channels: Array<WaveformChannel>
  envelope: Float32Array // max(channel.peak[i]) across channels
  gaplessEnabled: boolean
  skippedPackets: number
}

export interface AudioAnalysis {
  specs: AudioSpecs
  waveform: WaveformPeaks
}
```

Bucket `i` covers frames `[boundaries[i], boundaries[i + 1])`; time is `frameOffset / sampleRate`. With `peakCount`, boundaries are `floor(i * frameCount / peakCount)`; with `samplesPerPeak`, `min(i * samplesPerPeak, frameCount)`. No bucket is ever empty. Channels are never averaged, so opposite-phase stereo does not cancel. Values are raw decoded `f32` (float WAVE excursions above ±1 are preserved); these are sample peaks, not inter-sample true peaks.

## Usage

### Persist a generated track (metadata + waveform in one pass)

```typescript
import { AudioService } from '@d0paminedriven/audiodown'

const audio = new AudioService({ peakCount: 1024 })

export async function inspectGeneratedAudio(buffer: Buffer, cdnUrl: string) {
  const { specs, waveform } = await audio.analyzeAudioAsync(buffer, undefined, cdnUrl)

  // Prisma AudioMetadata row, straight from the specs.
  const row = {
    format: specs.mime,
    duration: specs.durationMs ?? Math.round(waveform.durationSec * 1000),
    bitrate: specs.bitrateKbps,
    sampleRate: specs.sampleRate,
    channels: specs.channels,
    codec: specs.codec,
    title: specs.title,
    artist: specs.artist,
    album: specs.album,
    year: specs.year,
    genre: specs.genre,
    waveformPeaks: Array.from(waveform.envelope, (v) => Math.round(Math.min(1, v) * 100)),
  }
  return { specs, row, ext: specs.ext, contentType: specs.contentType, byteSize: specs.byteSize }
}
```

### Probe a remote file with a ranged fetch

Metadata parsing accepts a header prefix. The walk reports `truncated: true`, and `durationSource` tells you whether the Xing/Info header could still supply a whole-file duration.

```typescript
const head = await fetchAudio(url, 64 * 1024) // your ranged fetch
const specs = audio.parseAudio(head.bytes, url)
if (specs.truncated && specs.durationSource !== 'xing') {
  // No Xing/Info header: durationSec covers only the fetched prefix.
}
```

### Decode once, reduce many

```typescript
const down = audio.open(buffer, cdnUrl)
const pcm = await down.pcmAsync()

const [thumb, detail] = await Promise.all([
  pcm.waveformAsync({ peakCount: 256 }),
  pcm.waveformAsync({ samplesPerPeak: 480 }), // 10 ms buckets at 48 kHz
])
```

### Raw PCM from a realtime stream

```typescript
// Int16 interleaved stereo at 48 kHz, as a realtime music API emits it.
const int16 = new Int16Array(chunk.buffer, chunk.byteOffset, chunk.byteLength / 2)
const f32 = Float32Array.from(int16, (v) => v / 32768)
const pcm = new AudioPcm(f32, 48_000, 2)
const peaks = pcm.waveform({ peakCount: 512 })
```

### Discriminate on `kind`

```typescript
const specs = audio.parseAudio(buffer)
if (specs.kind === 'mp3') {
  specs.frame?.layer // 'I' | 'II' | 'III'
  specs.xing?.kind // 'Xing' | 'Info'
} else {
  specs.container // 'RIFF' | 'RF64'
  specs.bitsPerSample
}
```

## Errors

Invalid options (`peakCount: 1.5`, `NaN`, `Infinity`, both resolution selectors, `maxDecodedSamples: 0`, fractional `channels`) throw synchronously with `InvalidArg` before any work is queued, even for async methods. Parse, decode and reduce failures throw `GenericFailure` from sync methods and reject the promise from async ones. `strict: false` skips packets that fail to decode and counts them in `skippedPackets`; the timeline then shortens by the skipped audio.

## Performance

`yarn bench` on a Linux x64 laptop (tinybench, release build):

| Task                                           | Latency |
| ---------------------------------------------- | ------: |
| `parseAudio` mp3 (1 s file, full frame walk)   |   13 µs |
| `parseAudio` wav                               |  6.6 µs |
| `decodeAudio` mp3 1 s                          |  1.1 ms |
| `waveformPeaks` mp3 1 s, 1024 peaks            |  1.3 ms |
| `analyzeAudio` mp3 1 s                         |  1.5 ms |
| `waveformFromPcm` 10 min 48 kHz stereo, serial |  157 ms |
| `waveformFromPcm` 10 min 48 kHz stereo, rayon  |   50 ms |
| 3 resolutions via `waveformPeaks` (3 decodes)  |  4.0 ms |
| 3 resolutions via `AudioPcm` (1 decode)        |  0.8 ms |

Where the parallelism is:

- **Waveform reduction** walks interleaved frames once per bucket, updating every channel as it goes, and splits buckets across the rayon pool with `par_chunks_mut`. Each bucket is reduced sequentially, so parallel and serial output are bit-identical. The pool is used above 262,144 samples; `parallel: false` forces serial.
- **`analyzeAudio`** runs metadata parsing and decode+reduce under `rayon::join`.
- **Decoding stays sequential** by design: MPEG Layer III frames depend on the bit reservoir of earlier frames.
- **Async methods** run on the libuv pool. Size both pools before the process starts:

```sh
RAYON_NUM_THREADS=4 UV_THREADPOOL_SIZE=8 node dist/server.js
```

Limits: 1,000,000 buckets, 4,000,000 channel-buckets, `maxDecodedSamples` interleaved samples (default 64,000,000, about 256 MB of PCM). Enforce input-size and concurrency limits at your service boundary.

## Development

```bash
yarn install
yarn build          # release addon + index.js + index.d.ts
yarn test           # ava, against __test__/fixtures
yarn test:rs        # cargo test (pure-Rust core)
yarn lint:rs        # cargo fmt --check + clippy -D warnings
yarn lint           # oxlint
yarn bench          # tinybench
yarn fixtures       # regenerate fixtures with FFmpeg + Python
```

Fixtures are generated by `scripts/fixtures.py`: a 1 s 44.1 kHz stereo sine with opposite-phase channels and a one-sample transient, encoded to CBR/VBR/no-Xing MP3, MPEG-2 mono, PCM24, extensible float, RF64, A-law and mu-law, plus FFmpeg's own decode of each MP3 as an `f32le` reference. Decoded PCM matches that reference within `3e-5` max absolute error.

Layout mirrors [`@d0paminedriven/pdfdown`](https://github.com/DopamineDriven/pdfdown): `src/lib.rs` is the Node-API surface, `src/types.rs` the JS boundary types, `src/core/` pure Rust (`metadata`, `decode`, `waveform`) with unit tests in `src/core/tests.rs`.

## License

MIT
