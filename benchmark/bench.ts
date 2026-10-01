import { readFileSync } from 'node:fs'
import { Bench } from 'tinybench'

import {
  AudioDown,
  AudioService,
  analyzeAudio,
  analyzeAudioAsync,
  decodeAudio,
  decodeAudioAsync,
  parseAudio,
  parseAudioAsync,
  waveformFromPcm,
  waveformPeaks,
  waveformPeaksAsync,
} from '../index.js'

const fixture = (name: string) => readFileSync(new URL(`../__test__/fixtures/${name}`, import.meta.url))

const mp3 = fixture('cbr.mp3')
const wav = fixture('stereo.wav')

// Ten minutes of 48 kHz stereo: the kind of buffer a generated track produces.
const LONG_FRAMES = 48_000 * 60 * 10
const long = new Float32Array(LONG_FRAMES * 2)
for (let i = 0; i < LONG_FRAMES; i++) {
  const v = Math.sin(i * 0.013) * Math.sin(i * 0.00007)
  long[i * 2] = v
  long[i * 2 + 1] = -v
}

const service = new AudioService()
const pcmHandle = new AudioDown(mp3).pcm()

const b = new Bench()

b.add('parseAudio mp3 (sync)', () => parseAudio(mp3))
b.add('parseAudioAsync mp3', async () => await parseAudioAsync(mp3))
b.add('parseAudio wav (sync)', () => parseAudio(wav))
b.add('AudioService.parseAudio mp3', () => service.parseAudio(mp3))

b.add('decodeAudio mp3 1s (sync)', () => decodeAudio(mp3))
b.add('decodeAudioAsync mp3 1s', async () => await decodeAudioAsync(mp3))
b.add('decodeAudio wav 1s (sync)', () => decodeAudio(wav))

b.add('waveformPeaks mp3 1s, 1024 peaks (sync)', () => waveformPeaks(mp3, { peakCount: 1024 }))
b.add('waveformPeaksAsync mp3 1s, 1024 peaks', async () => await waveformPeaksAsync(mp3, { peakCount: 1024 }))
b.add('analyzeAudio mp3 1s (sync)', () => analyzeAudio(mp3))
b.add('analyzeAudioAsync mp3 1s', async () => await analyzeAudioAsync(mp3))

b.add('waveformFromPcm 10min stereo, 2048 peaks, serial', () =>
  waveformFromPcm(long, 48_000, 2, { peakCount: 2048, parallel: false }),
)
b.add('waveformFromPcm 10min stereo, 2048 peaks, rayon', () =>
  waveformFromPcm(long, 48_000, 2, { peakCount: 2048, parallel: true }),
)
b.add('waveformFromPcm 10min stereo, 1 frame/peak, rayon', () =>
  waveformFromPcm(long, 48_000, 2, { samplesPerPeak: 480, parallel: true }),
)

b.add('3 resolutions via waveformPeaks (3 decodes)', () => {
  waveformPeaks(mp3, { peakCount: 256 })
  waveformPeaks(mp3, { peakCount: 1024 })
  waveformPeaks(mp3, { peakCount: 4096 })
})
b.add('3 resolutions via AudioPcm (1 decode, reused)', () => {
  pcmHandle.waveform({ peakCount: 256 })
  pcmHandle.waveform({ peakCount: 1024 })
  pcmHandle.waveform({ peakCount: 4096 })
})

b.run().then(() => {
  console.table(b.table())
})
