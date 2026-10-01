import test from 'ava'
import { readFileSync } from 'node:fs'

import {
  AudioDown,
  AudioPcm,
  AudioService,
  analyzeAudio,
  analyzeAudioAsync,
  decodeAudio,
  decodeAudioAsync,
  parseAudio,
  parseAudioAsync,
  parseMp3,
  parseMp3Async,
  parseWav,
  parseWavAsync,
  sniffAudio,
  waveformFromPcm,
  waveformFromPcmAsync,
  waveformPeaks,
  waveformPeaksAsync,
} from '../index'
import type { AudioSpecs, WaveformPeaks } from '../index'

const fixture = (name: string) => readFileSync(new URL(`./fixtures/${name}`, import.meta.url))
const close = (t: { true: (v: boolean, m?: string) => void }, a: number, b: number, tol = 1e-12) =>
  t.true(Math.abs(a - b) <= tol, `${a} vs ${b}`)
const FULL_SCALE = 32768

// ── sniffing ────────────────────────────────────────────────────

test('sniffAudio classifies by magic', (t) => {
  t.is(sniffAudio(fixture('cbr.mp3')), 'mp3')
  t.is(sniffAudio(fixture('stereo.wav')), 'wav')
  t.is(sniffAudio(fixture('rf64.wav')), 'wav')
  t.is(sniffAudio(Uint8Array.of(0xff, 0xf1, 0x50, 0x80)), null)
  t.is(sniffAudio(new Uint8Array(0)), null)
})

// ── AudioSpecs shape (ImageSpecs / Prisma / mp3Specs parity) ────

function assertSpecsShape(t: { is: (a: unknown, b: unknown, m?: string) => void }, specs: AudioSpecs, length: number) {
  t.is(specs.type, 'AUDIO')
  t.is(specs.format, specs.kind)
  t.is(specs.ext, specs.kind)
  t.is(specs.mimeType, specs.mime)
  t.is(specs.contentType, specs.mime)
  t.is(specs.byteSize, length)
  t.is(specs.fetchedBytes, length)
  t.is(specs.size, length)
}

test('mp3 specs carry pipeline fields and an exact frame walk', (t) => {
  const buf = fixture('cbr.mp3')
  const specs = parseAudio(buf, 'https://cdn.example/cbr.mp3')
  assertSpecsShape(t, specs, buf.length)
  t.is(specs.kind, 'mp3')
  t.is(specs.mime, 'audio/mpeg')
  t.is(specs.codec, 'mp3')
  t.is(specs.container, 'mpeg')
  t.is(specs.source, 'https://cdn.example/cbr.mp3')
  t.is(specs.sampleRate, 44100)
  t.is(specs.channels, 2)
  t.is(specs.bitrateKbps, 128)
  t.true(Math.abs((specs.bitrate ?? 0) - 128_000) < 100, `bitrate ${specs.bitrate}`)
  t.is(specs.cbr, true)
  t.is(specs.vbr, false)
  t.is(specs.xing?.kind, 'Info')
  t.is(specs.durationSource, 'frames')
  t.is(specs.frames, specs.xing?.frames ?? null)
  t.is(specs.resyncs, 0)
  t.is(specs.truncated, false)
  t.is(specs.bitsPerSample, null)
  t.is(specs.durationMs, Math.round((specs.durationSec ?? 0) * 1000))
  t.true((specs.durationSec ?? 0) > 1 && (specs.durationSec ?? 0) < 1.1, `duration ${specs.durationSec}`)
  t.is(specs.sampleFrames, (specs.frames ?? 0) * 1152)
  t.regex(specs.id3v2?.version ?? '', /^2\.[234]\.\d+$/)
  t.is(specs.audioOffset, specs.id3v2?.size ?? null)
  t.is(specs.frame?.version, '1')
  t.is(specs.frame?.layer, 'III')
  t.is(specs.title, 'Precision fixture')
  t.is(specs.artist, 'audiodown')
  t.is(specs.tags.title, 'Precision fixture')
  t.deepEqual(specs.metadata, { title: 'Precision fixture', artist: 'audiodown', ...specs.metadata })
  t.is(specs.year, null)
  t.is(specs.formatTag, null)
  t.is(specs.dataSize, null)
})

test('Xing marks VBR, missing Xing still walks exactly', (t) => {
  const vbr = parseMp3(fixture('vbr.mp3'))
  t.is(vbr.kind, 'mp3')
  t.is(vbr.vbr, true)
  t.is(vbr.cbr, false)
  t.is(vbr.xing?.kind, 'Xing')
  t.is(vbr.frames, vbr.xing?.frames ?? null)
  const noXing = parseMp3(fixture('no-xing.mp3'))
  t.is(noXing.xing, null)
  t.is(noXing.cbr, true)
  t.is(noXing.durationSource, 'frames')
  const mono = parseMp3(fixture('mpeg2-mono.mp3'))
  t.is(mono.frame?.version, '2')
  t.is(mono.channels, 1)
  t.is(mono.sampleRate, 22050)
})

test('a header-only prefix reports truncation and falls back to Xing', (t) => {
  const full = fixture('vbr.mp3')
  const probe = parseMp3(full.subarray(0, 2048))
  t.is(probe.truncated, true)
  t.is(probe.durationSource, 'xing')
  t.is(probe.durationSec, parseMp3(full).durationSec)
  t.is(probe.byteSize, 2048)
})

test('wav specs expose format chunk fields', (t) => {
  const buf = fixture('stereo.wav')
  const specs = parseAudio(buf)
  assertSpecsShape(t, specs, buf.length)
  t.is(specs.kind, 'wav')
  t.is(specs.mime, 'audio/wav')
  t.is(specs.codec, 'pcm')
  t.is(specs.container, 'RIFF')
  t.is(specs.source, undefined)
  t.is(specs.sampleRate, 44100)
  t.is(specs.channels, 2)
  t.is(specs.bitsPerSample, 16)
  t.is(specs.byteRate, 176400)
  t.is(specs.blockAlign, 4)
  t.is(specs.bitrate, 176400 * 8)
  t.is(specs.bitrateKbps, Math.floor((176400 * 8) / 1000))
  t.is(specs.durationSec, 1)
  t.is(specs.durationMs, 1000)
  t.is(specs.durationSource, 'data')
  t.is(specs.sampleFrames, 44100)
  t.is(specs.dataSize, 176400)
  t.is(specs.audioOffset, specs.dataOffset)
  t.is(specs.cbr, true)
  t.is(specs.frames, null)
  t.is(specs.resyncs, null)
  t.is(specs.frame, null)
  t.is(specs.id3v2, null)
  t.is(specs.xing, null)
  t.is(parseWav(fixture('rf64.wav')).container, 'RF64')
  const float = parseWav(fixture('float.wav'))
  t.is(float.codec, 'ieee-float')
  t.is(float.formatTag, 0xfffe)
  t.is(parseWav(fixture('exact-float.wav')).codec, 'ieee-float')
  t.is(parseWav(fixture('pcm_alaw.wav')).codec, 'alaw')
  t.is(parseWav(fixture('pcm_mulaw.wav')).codec, 'mulaw')
})

test('bare sync word yields nulls, no tags and no metadata record', (t) => {
  const specs = parseMp3(Uint8Array.of(0xff, 0xfb))
  t.is(specs.kind, 'mp3')
  t.is(specs.frame, null)
  t.is(specs.durationSec, null)
  t.is(specs.durationSource, null)
  t.is(specs.id3v2, null)
  t.is(specs.cbr, null)
  t.deepEqual(specs.tags, {})
  t.is(specs.metadata, undefined)
})

test('Buffer and sliced Uint8Array inputs respect byteOffset', (t) => {
  const b = fixture('cbr.mp3')
  const large = Buffer.concat([Buffer.alloc(31), b, Buffer.alloc(21)])
  const view = new Uint8Array(large.buffer, large.byteOffset + 31, b.length)
  t.deepEqual(parseAudio(view), parseAudio(b))
  t.is(decodeAudio(view).frameCount, 44100)
})

test('metadata async output equals sync output', async (t) => {
  for (const f of ['stereo.wav', 'rf64.wav', 'vbr.mp3']) {
    t.deepEqual(await parseAudioAsync(fixture(f), 'src'), parseAudio(fixture(f), 'src'))
  }
  t.is((await parseMp3Async(fixture('cbr.mp3'))).kind, 'mp3')
  t.is((await parseWavAsync(fixture('stereo.wav'))).kind, 'wav')
})

// ── decoding ────────────────────────────────────────────────────

test('PCM WAV decode preserves exact integer scaling', (t) => {
  const p = decodeAudio(fixture('stereo.wav'))
  t.true(p.samples instanceof Float32Array)
  t.is(p.frameCount, 44100)
  t.is(p.channels, 2)
  t.is(p.sampleRate, 44100)
  t.is(p.durationSec, 1)
  t.is(p.samples[22053 * 2], 32000 / FULL_SCALE)
  t.is(p.samples[22053 * 2 + 1], -31000 / FULL_SCALE)
})

test('float WAV retains excursions above full scale', (t) => {
  const p = decodeAudio(fixture('exact-float.wav'))
  t.deepEqual(Array.from(p.samples), [0, -0.25, 0.5, -1.25, 1.5, 0, 0.125])
})

test('PCM24, extensible float, RF64, A-law and mu-law decode', (t) => {
  for (const f of ['pcm24.wav', 'float.wav', 'rf64.wav']) t.is(decodeAudio(fixture(f)).frameCount, 44100, f)
  for (const f of ['pcm_alaw.wav', 'pcm_mulaw.wav']) {
    const p = decodeAudio(fixture(f))
    t.is(p.sampleRate, 8000, f)
    t.is(p.channels, 1, f)
    t.is(p.frameCount, 8000, f)
  }
})

test('gapless MP3 decoding has the original number of frames', async (t) => {
  for (const f of ['cbr.mp3', 'vbr.mp3']) {
    const p = await decodeAudioAsync(fixture(f))
    t.is(p.frameCount, 44100, f)
    t.is(p.durationSec, 1, f)
    t.is(p.gaplessEnabled, true)
    t.is(p.skippedPackets, 0)
    t.true(decodeAudio(fixture(f), { gapless: false }).frameCount > 44100, f)
  }
})

test('MPEG2 mono and MP3 without Xing are decodable', (t) => {
  const p = decodeAudio(fixture('mpeg2-mono.mp3'))
  t.is(p.sampleRate, 22050)
  t.is(p.channels, 1)
  t.is(p.frameCount, 22050)
  t.true(decodeAudio(fixture('no-xing.mp3')).frameCount > 44100)
})

test('MP3 PCM agrees with the independent FFmpeg reference', (t) => {
  for (const f of ['cbr.mp3', 'vbr.mp3', 'no-xing.mp3']) {
    const pcm = decodeAudio(fixture(f))
    const ref = fixture(`${f}.reference.f32`)
    t.is(pcm.samples.length, ref.length / 4, f)
    let max = 0
    let energy = 0
    for (let i = 0; i < pcm.samples.length; i++) {
      const err = pcm.samples[i] - ref.readFloatLE(i * 4)
      max = Math.max(max, Math.abs(err))
      energy += err * err
    }
    t.true(max < 3e-5, `${f}: max error ${max}`)
    t.true(Math.sqrt(energy / pcm.samples.length) < 3e-6, `${f}: rms error`)
  }
})

// ── waveform ────────────────────────────────────────────────────

test('min, max, peak, RMS and boundaries are exact', (t) => {
  const w = waveformPeaks(fixture('exact-float.wav'), { samplesPerPeak: 3 })
  t.deepEqual(Array.from(w.boundaries), [0, 3, 6, 7])
  t.is(w.peakCount, 3)
  const c = w.channels[0]
  t.true(c.min instanceof Float32Array)
  t.true(c.rms instanceof Float64Array)
  t.deepEqual(Array.from(c.min), [-0.25, -1.25, 0.125])
  t.deepEqual(Array.from(c.max), [0.5, 1.5, 0.125])
  t.deepEqual(Array.from(c.peak), [0.5, 1.5, 0.125])
  t.deepEqual(Array.from(w.envelope), [0.5, 1.5, 0.125])
  close(t, c.rms[0], Math.sqrt(0.3125 / 3))
})

test('opposite-phase stereo and one-sample impulses survive bucketing', (t) => {
  const w = waveformPeaks(fixture('stereo.wav'), { peakCount: 20 })
  t.is(w.channelCount, 2)
  t.is(Math.max(...w.channels[0].peak), 32000 / FULL_SCALE)
  t.is(Math.max(...w.channels[1].peak), 31000 / FULL_SCALE)
  t.true(w.channels[0].peak.every((n) => n > 0.5))
  t.true(w.channels[1].peak.every((n) => n > 0.5))
  for (let i = 0; i < w.peakCount; i++) {
    t.is(w.envelope[i], Math.max(w.channels[0].peak[i], w.channels[1].peak[i]))
  }
})

test('PCM reuse supports arbitrary exact bucket resolutions', async (t) => {
  const p = decodeAudio(fixture('vbr.mp3'))
  const reused = await waveformFromPcmAsync(p.samples, p.sampleRate, p.channels, { peakCount: 137 })
  const direct = waveformPeaks(fixture('vbr.mp3'), { peakCount: 137 })
  t.is(reused.peakCount, 137)
  t.deepEqual(reused.boundaries, direct.boundaries)
  t.deepEqual(reused.channels, direct.channels)
  t.deepEqual(reused.envelope, direct.envelope)
})

test('small inputs cap peakCount without producing empty buckets', (t) => {
  const w = waveformFromPcm(Float32Array.of(0.1, 0.2), 8000, 1, { peakCount: 1024 })
  t.is(w.peakCount, 2)
  t.deepEqual(Array.from(w.boundaries), [0, 1, 2])
})

test('serial and parallel reduction are deterministic', (t) => {
  for (const channels of [1, 2]) {
    const samples = Float32Array.from({ length: 300_001 * channels }, (_, i) => Math.sin(i * 0.17))
    const a = waveformFromPcm(samples, 48000, channels, { peakCount: 2048, parallel: false })
    const b = waveformFromPcm(samples, 48000, channels, { peakCount: 2048, parallel: true })
    t.deepEqual(a, b)
  }
})

test('analyzeAudio parses specs and reduces the waveform from one snapshot', async (t) => {
  const a = await analyzeAudioAsync(fixture('vbr.mp3'), { peakCount: 321 }, 'cdn://vbr')
  t.is(a.specs.kind, 'mp3')
  t.is(a.specs.source, 'cdn://vbr')
  t.is(a.waveform.peakCount, 321)
  t.is(a.waveform.durationSec, 1)
  t.deepEqual(a, analyzeAudio(fixture('vbr.mp3'), { peakCount: 321 }, 'cdn://vbr'))
  const envelope = Array.from(a.waveform.envelope, (v) => Math.round(Math.min(1, v) * 100))
  t.is(envelope.length, 321)
  t.true(envelope.every((v) => Number.isInteger(v) && v >= 0 && v <= 100))
})

test('async methods snapshot input before JS can mutate it', async (t) => {
  const b = fixture('cbr.mp3')
  const pending = analyzeAudioAsync(b, { peakCount: 10 })
  b.fill(0)
  t.is((await pending).waveform.frameCount, 44100)
  const p = Float32Array.of(0.5, -0.25)
  const work = waveformFromPcmAsync(p, 8000, 1, { peakCount: 1 })
  p.fill(0)
  t.is((await work).channels[0].peak[0], 0.5)
})

test('output typed arrays remain valid across garbage collection', async (t) => {
  const w = await waveformPeaksAsync(fixture('cbr.mp3'))
  const expected = Array.from(w.channels[0].peak)
  if (globalThis.gc) for (let i = 0; i < 3; i++) globalThis.gc()
  t.deepEqual(Array.from(w.channels[0].peak), expected)
})

// ── validation ──────────────────────────────────────────────────

test('rejects ambiguous resolutions, fractions and invalid inputs', (t) => {
  const b = fixture('stereo.wav')
  for (const o of [
    { peakCount: 0 },
    { peakCount: -1 },
    { peakCount: 1.5 },
    { peakCount: NaN },
    { peakCount: Infinity },
    { samplesPerPeak: 0 },
    { peakCount: 10, samplesPerPeak: 5 },
    { peakCount: 1_000_001 },
  ]) {
    t.throws(() => waveformPeaks(b, o), undefined, JSON.stringify(o))
  }
  t.throws(() => decodeAudio(b, { maxDecodedSamples: 0 }))
  t.throws(() => decodeAudio(b, { maxDecodedSamples: 100 }), { message: /maxDecodedSamples/ })
  t.throws(() => waveformFromPcm(Float32Array.of(NaN), 8000, 1), { message: /non-finite/ })
  t.throws(() => waveformFromPcm(Float32Array.of(0), 0, 1))
  t.throws(() => waveformFromPcm(Float32Array.of(0), 8000, 1.5))
  t.throws(() => waveformFromPcm(Float32Array.of(0), 8000, 2), { message: /complete sample frames/ })
  t.throws(() => parseAudio(new ArrayBuffer(8) as unknown as Uint8Array))
  t.throws(() => parseWav(fixture('cbr.mp3')), { message: /not a WAVE/ })
  t.throws(() => parseMp3(b), { message: /not an MP3/ })
  t.throws(() => parseAudio(Uint8Array.of(0, 1, 2, 3)), { message: /unrecognized/ })
})

test('decode failures reject the async Promise', async (t) => {
  await t.throwsAsync(decodeAudioAsync(Uint8Array.of(0, 1, 2, 3)), { message: /unrecognized/ })
})

// ── AudioService ────────────────────────────────────────────────

test('AudioService mirrors the standalone functions', async (t) => {
  const service = new AudioService()
  const mp3 = fixture('cbr.mp3')
  const wav = fixture('stereo.wav')
  t.is(service.sniffAudio(mp3), 'mp3')
  t.deepEqual(service.parseAudio(mp3, 'x'), parseAudio(mp3, 'x'))
  t.deepEqual(service.parseMp3(mp3), parseMp3(mp3))
  t.deepEqual(service.parseWav(wav), parseWav(wav))
  t.deepEqual(await service.parseAudioAsync(wav), parseAudio(wav))
  t.deepEqual(await service.parseMp3Async(mp3), parseMp3(mp3))
  t.deepEqual(await service.parseWavAsync(wav), parseWav(wav))
  t.deepEqual(service.decodeAudio(wav), decodeAudio(wav))
  t.deepEqual(await service.decodeAudioAsync(mp3), decodeAudio(mp3))
  t.deepEqual(service.waveformPeaks(wav, { peakCount: 50 }), waveformPeaks(wav, { peakCount: 50 }))
  t.deepEqual(await service.waveformPeaksAsync(wav, { peakCount: 50 }), waveformPeaks(wav, { peakCount: 50 }))
  const pcm = decodeAudio(wav)
  t.deepEqual(
    service.waveformFromPcm(pcm.samples, pcm.sampleRate, pcm.channels, { peakCount: 7 }),
    waveformFromPcm(pcm.samples, pcm.sampleRate, pcm.channels, { peakCount: 7 }),
  )
  t.deepEqual(
    await service.waveformFromPcmAsync(pcm.samples, pcm.sampleRate, pcm.channels, { peakCount: 7 }),
    waveformFromPcm(pcm.samples, pcm.sampleRate, pcm.channels, { peakCount: 7 }),
  )
  t.deepEqual(service.analyzeAudio(mp3, { peakCount: 9 }), analyzeAudio(mp3, { peakCount: 9 }))
  t.deepEqual(await service.analyzeAudioAsync(mp3, { peakCount: 9 }), analyzeAudio(mp3, { peakCount: 9 }))
})

test('AudioService defaults merge under per-call options', (t) => {
  const service = new AudioService({ peakCount: 64, gapless: false })
  t.deepEqual(service.defaults, { peakCount: 64, gapless: false })
  const wav = fixture('stereo.wav')
  t.is(service.waveformPeaks(wav).peakCount, 64)
  // A call that names either selector replaces both defaults.
  t.is(service.waveformPeaks(wav, { samplesPerPeak: 4410 }).peakCount, 10)
  t.is(service.waveformPeaks(wav, { peakCount: 5 }).peakCount, 5)
  // Decode defaults flow into decodeAudio too.
  t.true(service.decodeAudio(fixture('cbr.mp3')).frameCount > 44100)
  t.is(service.decodeAudio(fixture('cbr.mp3'), { gapless: true }).frameCount, 44100)
  t.throws(() => new AudioService({ peakCount: 0 }))
  t.throws(() => new AudioService({ peakCount: 1, samplesPerPeak: 1 }))
})

// ── AudioDown ───────────────────────────────────────────────────

test('AudioDown snapshots once and serves every operation', async (t) => {
  const buf = fixture('vbr.mp3')
  const down = new AudioDown(buf, 'cdn://vbr.mp3')
  t.is(down.kind, 'mp3')
  t.is(down.byteSize, buf.length)
  t.is(down.source, 'cdn://vbr.mp3')
  const specs = down.metadata()
  t.deepEqual(specs, parseAudio(buf, 'cdn://vbr.mp3'))
  t.deepEqual(await down.metadataAsync(), specs)
  t.is(down.decode().frameCount, 44100)
  t.is((await down.decodeAsync()).frameCount, 44100)
  t.deepEqual(down.waveform({ peakCount: 33 }), waveformPeaks(buf, { peakCount: 33 }))
  t.deepEqual(await down.waveformAsync({ peakCount: 33 }), waveformPeaks(buf, { peakCount: 33 }))
  t.deepEqual(down.analyze({ peakCount: 33 }), analyzeAudio(buf, { peakCount: 33 }, 'cdn://vbr.mp3'))
  t.deepEqual(await down.analyzeAsync({ peakCount: 33 }), analyzeAudio(buf, { peakCount: 33 }, 'cdn://vbr.mp3'))
  // The snapshot is independent of the caller's buffer.
  buf.fill(0)
  t.is(down.decode().frameCount, 44100)
  t.throws(() => new AudioDown(Uint8Array.of(0, 1, 2, 3)), { message: /unrecognized/ })
})

test('AudioService.open hands defaults to the AudioDown handle', (t) => {
  const service = new AudioService({ peakCount: 12 })
  const down = service.open(fixture('stereo.wav'), 'cdn://stereo.wav')
  t.true(down instanceof AudioDown)
  t.is(down.kind, 'wav')
  t.is(down.metadata().source, 'cdn://stereo.wav')
  t.is(down.waveform().peakCount, 12)
  t.is(down.analyze().waveform.peakCount, 12)
  t.is(down.waveform({ peakCount: 3 }).peakCount, 3)
})

// ── AudioPcm ────────────────────────────────────────────────────

test('AudioPcm reduces one decode at many resolutions', async (t) => {
  const buf = fixture('cbr.mp3')
  const down = new AudioDown(buf)
  const pcm = down.pcm()
  t.true(pcm instanceof AudioPcm)
  const decoded = decodeAudio(buf)
  t.is(pcm.sampleRate, decoded.sampleRate)
  t.is(pcm.channels, decoded.channels)
  t.is(pcm.frameCount, decoded.frameCount)
  t.is(pcm.durationSec, decoded.durationSec)
  t.is(pcm.gaplessEnabled, true)
  t.is(pcm.skippedPackets, 0)
  t.deepEqual(pcm.samples(), decoded.samples)
  t.deepEqual(pcm.decoded(), decoded)
  for (const peakCount of [8, 137, 2048]) {
    t.deepEqual(pcm.waveform({ peakCount }), waveformPeaks(buf, { peakCount }))
    t.deepEqual(await pcm.waveformAsync({ peakCount }), waveformPeaks(buf, { peakCount }))
  }
  const viaAsync = await down.pcmAsync()
  t.true(viaAsync instanceof AudioPcm)
  t.deepEqual(viaAsync.waveform({ peakCount: 16 }), pcm.waveform({ peakCount: 16 }))
})

test('AudioPcm accepts caller-supplied interleaved PCM', (t) => {
  const samples = Float32Array.of(0, -0.25, 0.5, -1.25, 1.5, 0, 0.125)
  const pcm = new AudioPcm(samples, 8000, 1, { samplesPerPeak: 3 })
  t.is(pcm.frameCount, 7)
  t.is(pcm.channels, 1)
  const w: WaveformPeaks = pcm.waveform()
  t.deepEqual(Array.from(w.boundaries), [0, 3, 6, 7])
  t.deepEqual(Array.from(w.channels[0].max), [0.5, 1.5, 0.125])
  samples.fill(0)
  t.deepEqual(Array.from(pcm.waveform().channels[0].max), [0.5, 1.5, 0.125])
  t.throws(() => new AudioPcm(Float32Array.of(0), 8000, 2), { message: /complete sample frames/ })
  t.throws(() => new AudioPcm(Float32Array.of(NaN), 8000, 1), { message: /non-finite/ })
})
