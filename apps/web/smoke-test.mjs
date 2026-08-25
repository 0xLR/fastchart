import { readFile } from "node:fs/promises";
import { makeZip } from "./zip.js";

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
const audio = allocateString("song.wav");
const resultPointer = engine.fastchart_generate(
  samples, length, 44_100,
  title.pointer, title.length,
  artist.pointer, artist.length,
  audio.pointer, audio.length,
  7, 15,
);
const resultLength = engine.fastchart_result_length();
const result = JSON.parse(decoder.decode(new Uint8Array(engine.memory.buffer, resultPointer, resultLength)));
if (!result.chart.includes("[Song]") || !result.ini.includes("[song]") || result.bpm <= 0) {
  throw new Error("WASM response did not contain a valid FastChart payload.");
}
const zip = makeZip([
  { name: "notes.chart", bytes: encoder.encode(result.chart) },
  { name: "song.ini", bytes: encoder.encode(result.ini) },
]);
const zipBytes = new Uint8Array(await zip.arrayBuffer());
if (new DataView(zipBytes.buffer).getUint32(0, true) !== 0x04034b50) {
  throw new Error("ZIP packaging did not produce a valid local-file header.");
}
engine.fastchart_free_bytes(resultPointer, resultLength);
engine.fastchart_free_f32(samples, length);
for (const allocation of [title, artist, audio]) engine.fastchart_free_bytes(allocation.pointer, allocation.length);
console.log(`FastChart WASM and ZIP smoke test passed (${result.bpm.toFixed(1)} BPM, ${result.tempoRegions} tempo region).`);
