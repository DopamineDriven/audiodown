"""Reproducible fixtures. FFmpeg is a development-only dependency."""
import json, math, pathlib, struct, subprocess, wave
root = pathlib.Path(__file__).resolve().parent.parent
out = root / '__test__' / 'fixtures'
out.mkdir(parents=True, exist_ok=True)
rate, frames = 44100, 44100
raw = bytearray()
for i in range(frames):
    left = round(0.55 * math.sin(2 * math.pi * 440 * i / rate) * 32767)
    right = -left  # Deliberately opposite phase: naive mono averaging cancels it.
    if i == 22053: left, right = 32000, -31000  # One-sample transient.
    raw.extend(struct.pack('<hh', left, right))
with wave.open(str(out / 'stereo.wav'), 'wb') as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(rate); w.writeframes(raw)

def run(*args):
    subprocess.run(['ffmpeg','-hide_banner','-loglevel','error','-y',*map(str,args)], check=True)
for name,args in [('cbr.mp3',['-b:a','128k']),('vbr.mp3',['-q:a','2']),('no-xing.mp3',['-b:a','128k','-write_xing','0'])]:
    run('-i',out/'stereo.wav','-c:a','libmp3lame',*args,'-metadata','title=Precision fixture','-metadata','artist=audiodown',out/name)
    run('-i',out/name,'-f','f32le','-c:a','pcm_f32le',out/(name+'.reference.f32'))
run('-i',out/'stereo.wav','-ac','1','-ar','22050','-c:a','libmp3lame','-q:a','3',out/'mpeg2-mono.mp3')
run('-i',out/'stereo.wav','-c:a','pcm_f32le',out/'float.wav')
run('-i',out/'stereo.wav','-c:a','pcm_s24le',out/'pcm24.wav')
run('-i',out/'stereo.wav','-c:a','pcm_s16le','-rf64','always',out/'rf64.wav')
for codec in ['pcm_alaw','pcm_mulaw']:
    run('-i',out/'stereo.wav','-ac','1','-ar','8000','-c:a',codec,out/(codec+'.wav'))
# Small exact float fixture including legitimate excursions beyond [-1,1].
samples = [0.0, -0.25, 0.5, -1.25, 1.5, 0.0, 0.125]
payload = struct.pack('<7f',*samples)
fmt = struct.pack('<HHIIHH',3,1,8000,32000,4,32)
body = b'WAVEfmt ' + struct.pack('<I',len(fmt)) + fmt + b'data' + struct.pack('<I',len(payload)) + payload
(out/'exact-float.wav').write_bytes(b'RIFF'+struct.pack('<I',len(body))+body)
(out/'fixture-info.json').write_text(json.dumps({'sampleRate':rate,'frames':frames,'channels':2,'floatSamples':samples},indent=2)+'\n')
print('Generated audio fixtures and independent FFmpeg PCM references.')
