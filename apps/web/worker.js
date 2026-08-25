let wasm;

async function loadFastChart() {
  if (wasm) return wasm;
  const response = await fetch("./pkg/fastchart.wasm");
  if (!response.ok) throw new Error("FastChart's WebAssembly engine could not be loaded.");
  let module;
  try {
    module = await WebAssembly.instantiateStreaming(response.clone(), {});
  } catch {
    module = await WebAssembly.instantiate(await response.arrayBuffer(), {});
  }
  wasm = module.instance.exports;
  return wasm;
}

function writeString(engine, value) {
  const bytes = new TextEncoder().encode(value);
  const pointer = engine.fastchart_alloc_bytes(bytes.length);
  new Uint8Array(engine.memory.buffer, pointer, bytes.length).set(bytes);
  return { pointer, length: bytes.length };
}

function releaseString(engine, allocation) {
  engine.fastchart_free_bytes(allocation.pointer, allocation.length);
}

self.addEventListener("message", async ({ data }) => {
  if (data.type !== "generate") return;
  try {
    self.postMessage({ type: "progress", stage: "analyze", percent: 34 });
    const engine = await loadFastChart();
    let samples = new Float32Array(data.samples);
    const samplesPointer = engine.fastchart_alloc_f32(samples.length);
    new Float32Array(engine.memory.buffer, samplesPointer, samples.length).set(samples);
    const samplesLength = samples.length;
    samples = null;
    const title = writeString(engine, data.title);
    const artist = writeString(engine, data.artist);
    const audio = writeString(engine, data.audioFilename);
    let resultPointer;
    try {
      resultPointer = engine.fastchart_generate(
        samplesPointer,
        samplesLength,
        data.sampleRate,
        title.pointer,
        title.length,
        artist.pointer,
        artist.length,
        audio.pointer,
        audio.length,
        data.instrumentMask,
        data.difficultyMask,
      );
      const resultLength = engine.fastchart_result_length();
      const resultBytes = new Uint8Array(engine.memory.buffer, resultPointer, resultLength).slice();
      const result = JSON.parse(new TextDecoder().decode(resultBytes));
      if (result.error) throw new Error(result.error);
      self.postMessage({ type: "progress", stage: "chart", percent: 82 });
      self.postMessage({ type: "progress", stage: "pack", percent: 90 });
      self.postMessage({ type: "complete", result });
      engine.fastchart_free_bytes(resultPointer, resultLength);
    } finally {
      engine.fastchart_free_f32(samplesPointer, samplesLength);
      releaseString(engine, title);
      releaseString(engine, artist);
      releaseString(engine, audio);
    }
  } catch (error) {
    self.postMessage({ type: "error", message: error?.message || "Chart generation failed." });
  }
});
