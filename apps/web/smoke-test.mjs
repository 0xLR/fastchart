import { readFile } from "node:fs/promises";
import { makeZip } from "./zip.js";
import { encodePcm16Wave, isCloneHeroAudioExtension } from "./audio.js";

const wasmBytes = await readFile(new URL("./dist/pkg/fastchart.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(wasmBytes, {});
const engine = instance.exports;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function allocateString(value) {
  const bytes = encoder.encode(value);
  const pointer = engine.fastchart_alloc_bytes(bytes.length);
  new Uint8Array(engine.memory.buffer, pointer, bytes.length).set(bytes);
  return { pointer, length: bytes.length };
}

const length = 4_410;
const samples = engine.fastchart_alloc_f32(length);
const title = allocateString("Browser Smoke Test");
const artist = allocateString("FastChart");
const album = allocateString("Test Album");
const genre = allocateString("Rock");
const year = allocateString("2026");
const charter = allocateString("FastChart Smoke Test");
const albumTrack = allocateString("2");
const audio = allocateString("song.wav");
const resultPointer = engine.fastchart_generate(
  samples, length, 44_100,
  title.pointer, title.length,
  artist.pointer, artist.length,
  album.pointer, album.length,
  genre.pointer, genre.length,
  year.pointer, year.length,
  charter.pointer, charter.length,
  albumTrack.pointer, albumTrack.length,
  audio.pointer, audio.length,
  7, 15,
);
const resultLength = engine.fastchart_result_length();
const result = JSON.parse(decoder.decode(new Uint8Array(engine.memory.buffer, resultPointer, resultLength)));
if (
  !result.chart.includes("[Song]") ||
  !result.ini.includes("album = Test Album") ||
  !result.ini.includes("album_track = 2") ||
  result.bpm <= 0
) {
  throw new Error("WASM response did not contain a valid FastChart payload.");
}
const zip = makeZip([
  { name: "notes.chart", bytes: encoder.encode(result.chart) },
  { name: "song.ini", bytes: encoder.encode(result.ini) },
  { name: "album.png", bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47]) },
]);
const zipBytes = new Uint8Array(await zip.arrayBuffer());
if (new DataView(zipBytes.buffer).getUint32(0, true) !== 0x04034b50) {
  throw new Error("ZIP packaging did not produce a valid local-file header.");
}
if (!decoder.decode(zipBytes).includes("album.png")) {
  throw new Error("ZIP packaging omitted the selected artwork filename.");
}
const wave = encodePcm16Wave({
  numberOfChannels: 2,
  length: 2,
  sampleRate: 44_100,
  getChannelData: (channel) =>
    channel === 0 ? new Float32Array([0.5, -0.5]) : new Float32Array([-0.25, 0.25]),
});
const waveView = new DataView(wave);
if (
  waveView.getUint32(0, false) !== 0x52494646 ||
  waveView.getUint32(8, false) !== 0x57415645 ||
  waveView.getUint16(22, true) !== 2 ||
  wave.byteLength !== 52 ||
  isCloneHeroAudioExtension("flac")
) {
  throw new Error("Clone Hero WAV fallback did not produce a valid stereo PCM file.");
}
engine.fastchart_free_bytes(resultPointer, resultLength);
engine.fastchart_free_f32(samples, length);
for (const allocation of [title, artist, album, genre, year, charter, albumTrack, audio]) {
  engine.fastchart_free_bytes(allocation.pointer, allocation.length);
}
console.log(`FastChart WASM and ZIP smoke test passed (${result.bpm.toFixed(1)} BPM, ${result.tempoRegions} tempo region).`);
