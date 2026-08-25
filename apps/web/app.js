import { makeZip } from "./zip.js";

const $ = (selector) => document.querySelector(selector);
const dropZone = $("#drop-zone");
const fileInput = $("#audio-file");
const fileCard = $("#file-card");
const fileName = $("#file-name");
const fileSize = $("#file-size");
const titleInput = $("#song-title");
const artistInput = $("#song-artist");
const songFields = $("#song-fields");
const instruments = $("#instruments");
const difficulties = $("#difficulties");
const generateButton = $("#generate");
const formError = $("#form-error");
const progressPanel = $("#progress-panel");
const resultPanel = $("#result-panel");
const progressBar = $("#progress-bar");
const progressTitle = $("#progress-title");
const progressDetail = $("#progress-detail");

let selectedFile;
let sourceAudio;
let generatedResult;
let worker;
let canceled = false;

window.fastChartReady = true;

const stages = {
  decode: ["Decoding your audio…", "Preparing a clean mono signal for the analysis engine."],
  analyze: ["Finding the pulse…", "FastChart is detecting tempo, transients, and pitched attacks."],
  chart: ["Building playable parts…", "Mapping events onto Clone Hero lanes and difficulty levels."],
  pack: ["Packing your chart…", "Preparing notes.chart, song.ini, and your audio for download."],
};

dropZone.addEventListener("keydown", (event) => {
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    fileInput.click();
  }
});
fileInput.addEventListener("change", () => chooseFile(fileInput.files[0]));
for (const eventName of ["dragenter", "dragover"]) {
  dropZone.addEventListener(eventName, (event) => {
    event.preventDefault();
    dropZone.classList.add("dragging");
  });
}
for (const eventName of ["dragleave", "drop"]) {
  dropZone.addEventListener(eventName, (event) => {
    event.preventDefault();
    dropZone.classList.remove("dragging");
  });
}
dropZone.addEventListener("drop", (event) => chooseFile(event.dataTransfer.files[0]));
$("#remove-file").addEventListener("click", resetFile);
$("#start-over").addEventListener("click", () => {
  resetFile();
  resultPanel.hidden = true;
  window.scrollTo({ top: 0, behavior: "smooth" });
});
generateButton.addEventListener("click", generateChart);
$("#cancel").addEventListener("click", cancelGeneration);
$("#download").addEventListener("click", downloadChart);

function chooseFile(file) {
  if (!file) return;
  if (!file.type.startsWith("audio/") && !/\.(wav|mp3|ogg|opus|flac|m4a)$/i.test(file.name)) {
    formError.textContent = "Choose a supported audio file.";
    return;
  }
  if (file.size > 500 * 1024 * 1024) {
    formError.textContent = "For this MVP, choose a file smaller than 500 MB.";
    return;
  }
  selectedFile = file;
  fileName.textContent = file.name;
  fileSize.textContent = formatBytes(file.size);
  titleInput.value = titleFromFilename(file.name);
  artistInput.value = "";
  dropZone.hidden = true;
  fileCard.hidden = false;
  songFields.hidden = false;
  instruments.disabled = false;
  difficulties.disabled = false;
  generateButton.disabled = false;
  formError.textContent = "";
  resultPanel.hidden = true;
}

function resetFile() {
  selectedFile = undefined;
  sourceAudio = undefined;
  generatedResult = undefined;
  fileInput.value = "";
  dropZone.hidden = false;
  fileCard.hidden = true;
  songFields.hidden = true;
  instruments.disabled = true;
  difficulties.disabled = true;
  generateButton.disabled = true;
  formError.textContent = "";
}

async function generateChart() {
  if (!selectedFile) return;
  const instrumentMask = checkboxMask(instruments);
  const difficultyMask = checkboxMask(difficulties);
  if (!instrumentMask || !difficultyMask) {
    formError.textContent = "Select at least one instrument and one difficulty.";
    return;
  }
  canceled = false;
  generatedResult = undefined;
  resultPanel.hidden = true;
  progressPanel.hidden = false;
  setStage("decode", 8);
  progressPanel.scrollIntoView({ behavior: "smooth", block: "center" });
  generateButton.disabled = true;
  formError.textContent = "";

  try {
    sourceAudio = await selectedFile.arrayBuffer();
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) throw new Error("This browser cannot decode audio files.");
    const context = new AudioContextClass();
    let audioBuffer;
    try {
      audioBuffer = await context.decodeAudioData(sourceAudio.slice(0));
    } finally {
      await context.close();
    }
    if (canceled) return;
    const mono = mixToMono(audioBuffer);
    setStage("analyze", 32);
    worker = new Worker("./worker.js", { type: "module" });
    worker.addEventListener("message", handleWorkerMessage);
    worker.addEventListener("error", () => failGeneration("The analysis worker stopped unexpectedly."));
    const audioFilename = `song.${fileExtension(selectedFile.name)}`;
    worker.postMessage(
      {
        type: "generate",
        samples: mono.buffer,
        sampleRate: audioBuffer.sampleRate,
        title: titleInput.value.trim() || titleFromFilename(selectedFile.name),
        artist: artistInput.value.trim() || "Unknown Artist",
        audioFilename,
        instrumentMask,
        difficultyMask,
      },
      [mono.buffer],
    );
  } catch (error) {
    failGeneration(error?.message || "The selected audio could not be decoded.");
  }
}

function handleWorkerMessage({ data }) {
  if (data.type === "progress") {
    setStage(data.stage, data.percent);
  } else if (data.type === "complete") {
    generatedResult = data.result;
    setStage("pack", 100);
    window.setTimeout(showResult, 250);
    stopWorker();
  } else if (data.type === "error") {
    failGeneration(data.message);
  }
}

function showResult() {
  progressPanel.hidden = true;
  resultPanel.hidden = false;
  $("#stat-bpm").textContent = generatedResult.bpm.toFixed(1);
  $("#stat-drums").textContent = generatedResult.notes.drums.toLocaleString();
  $("#stat-guitar").textContent = generatedResult.notes.guitar.toLocaleString();
  $("#stat-bass").textContent = generatedResult.notes.bass.toLocaleString();
  generateButton.disabled = false;
  resultPanel.scrollIntoView({ behavior: "smooth", block: "center" });
}

function cancelGeneration() {
  canceled = true;
  stopWorker();
  progressPanel.hidden = true;
  generateButton.disabled = false;
  formError.textContent = "Generation canceled. Your audio stayed on this device.";
}

function failGeneration(message) {
  stopWorker();
  progressPanel.hidden = true;
  generateButton.disabled = false;
  formError.textContent = message;
  window.scrollTo({ top: 0, behavior: "smooth" });
}

function stopWorker() {
  if (worker) worker.terminate();
  worker = undefined;
}

function setStage(stage, percent) {
  const [title, detail] = stages[stage];
  progressTitle.textContent = title;
  progressDetail.textContent = detail;
  progressBar.style.width = `${percent}%`;
  const order = ["decode", "analyze", "chart", "pack"];
  const current = order.indexOf(stage);
  order.forEach((name, index) => {
    const element = $(`#stage-${name}`);
    element.classList.toggle("active", index === current);
    element.classList.toggle("done", index < current || percent === 100);
  });
}

function mixToMono(audioBuffer) {
  const mono = new Float32Array(audioBuffer.length);
  for (let channel = 0; channel < audioBuffer.numberOfChannels; channel += 1) {
    const input = audioBuffer.getChannelData(channel);
    for (let index = 0; index < mono.length; index += 1) mono[index] += input[index];
  }
  const scale = 1 / audioBuffer.numberOfChannels;
  for (let index = 0; index < mono.length; index += 1) mono[index] *= scale;
  return mono;
}

async function downloadChart() {
  if (!generatedResult || !sourceAudio || !selectedFile) return;
  const encoder = new TextEncoder();
  const audioName = `song.${fileExtension(selectedFile.name)}`;
  const zip = makeZip([
    { name: "notes.chart", bytes: encoder.encode(generatedResult.chart) },
    { name: "song.ini", bytes: encoder.encode(generatedResult.ini) },
    { name: audioName, bytes: new Uint8Array(sourceAudio) },
  ]);
  const url = URL.createObjectURL(zip);
  const link = document.createElement("a");
  link.href = url;
  link.download = `${safeName(titleInput.value || "fastchart-song")}.zip`;
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

function checkboxMask(fieldset) { return [...fieldset.querySelectorAll("input:checked")].reduce((mask, input) => mask | Number(input.value), 0); }
function fileExtension(name) { return name.split(".").pop().toLowerCase().replace(/[^a-z0-9]/g, "") || "wav"; }
function titleFromFilename(name) { return name.replace(/\.[^.]+$/, "").replace(/[_-]+/g, " ").replace(/\s+/g, " ").trim(); }
function safeName(name) { return name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "fastchart-song"; }
function formatBytes(bytes) { return bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(0)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
