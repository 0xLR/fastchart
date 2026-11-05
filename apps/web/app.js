import { makeZip } from "./zip.js";
import {
  audioExtension,
  encodePcm16Wave,
  isCloneHeroAudioExtension,
  mixToMono,
} from "./audio.js";

const $ = (selector) => document.querySelector(selector);
const dropZone = $("#drop-zone");
const fileInput = $("#audio-file");
const fileCard = $("#file-card");
const fileName = $("#file-name");
const fileSize = $("#file-size");
const titleInput = $("#song-title");
const artistInput = $("#song-artist");
const songFields = $("#song-fields");
const metadataPanel = $("#optional-metadata");
const albumInput = $("#song-album");
const genreInput = $("#song-genre");
const yearInput = $("#song-year");
const albumTrackInput = $("#album-track");
const charterInput = $("#song-charter");
const albumArtInput = $("#album-art");
const backgroundArtInput = $("#background-art");
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
let packagedAudioFilename;
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
for (const input of [albumArtInput, backgroundArtInput]) {
  input.addEventListener("change", () => validateArtwork(input));
}

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
  metadataPanel.hidden = false;
  instruments.disabled = false;
  difficulties.disabled = false;
  generateButton.disabled = false;
  formError.textContent = "";
  resultPanel.hidden = true;
}

function resetFile() {
  selectedFile = undefined;
  sourceAudio = undefined;
  packagedAudioFilename = undefined;
  generatedResult = undefined;
  fileInput.value = "";
  dropZone.hidden = false;
  fileCard.hidden = true;
  songFields.hidden = true;
  metadataPanel.hidden = true;
  metadataPanel.open = false;
  albumInput.value = "";
  genreInput.value = "";
  yearInput.value = "";
  albumTrackInput.value = "";
  charterInput.value = "FastChart";
  albumArtInput.value = "";
  backgroundArtInput.value = "";
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
  const invalidMetadata = [yearInput, albumTrackInput].find((input) => !input.checkValidity());
  if (invalidMetadata) {
    invalidMetadata.reportValidity();
    formError.textContent = "Check the optional year and album track values.";
    return;
  }
  if (!validateArtwork(albumArtInput) || !validateArtwork(backgroundArtInput)) return;
  canceled = false;
  generatedResult = undefined;
  resultPanel.hidden = true;
  progressPanel.hidden = false;
  setStage("decode", 8);
  progressPanel.scrollIntoView({ behavior: "smooth", block: "center" });
  generateButton.disabled = true;
  formError.textContent = "";

  try {
    const originalAudio = await selectedFile.arrayBuffer();
    const AudioContextClass = window.AudioContext || window.webkitAudioContext;
    if (!AudioContextClass) throw new Error("This browser cannot decode audio files.");
    const context = new AudioContextClass();
    let audioBuffer;
    try {
      audioBuffer = await context.decodeAudioData(originalAudio.slice(0));
    } finally {
      await context.close();
    }
    if (canceled) return;
    const extension = audioExtension(selectedFile.name);
    if (isCloneHeroAudioExtension(extension)) {
      sourceAudio = originalAudio;
      packagedAudioFilename = `song.${extension}`;
    } else {
      sourceAudio = encodePcm16Wave(audioBuffer);
      packagedAudioFilename = "song.wav";
    }
    const mono = mixToMono(audioBuffer);
    setStage("analyze", 32);
    worker = new Worker("./worker.js", { type: "module" });
    worker.addEventListener("message", handleWorkerMessage);
    worker.addEventListener("error", () => failGeneration("The analysis worker stopped unexpectedly."));
    worker.postMessage(
      {
        type: "generate",
        samples: mono.buffer,
        sampleRate: audioBuffer.sampleRate,
        title: titleInput.value.trim() || titleFromFilename(selectedFile.name),
        artist: artistInput.value.trim() || "Unknown Artist",
        album: albumInput.value.trim(),
        genre: genreInput.value.trim(),
        year: yearInput.value.trim(),
        charter: charterInput.value.trim() || "FastChart",
        albumTrack: albumTrackInput.value.trim(),
        audioFilename: packagedAudioFilename,
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

async function downloadChart() {
  if (!generatedResult || !sourceAudio || !selectedFile || !packagedAudioFilename) return;
  const downloadButton = $("#download");
  downloadButton.disabled = true;
  const encoder = new TextEncoder();
  try {
    const files = [
      { name: "notes.chart", bytes: encoder.encode(generatedResult.chart) },
      { name: "song.ini", bytes: encoder.encode(generatedResult.ini) },
      { name: packagedAudioFilename, bytes: new Uint8Array(sourceAudio) },
    ];
    for (const [input, basename] of [
      [albumArtInput, "album"],
      [backgroundArtInput, "background"],
    ]) {
      const file = input.files[0];
      if (file) {
        files.push({
          name: `${basename}.${imageExtension(file)}`,
          bytes: new Uint8Array(await file.arrayBuffer()),
        });
      }
    }
    const zip = makeZip(files);
    const url = URL.createObjectURL(zip);
    const link = document.createElement("a");
    link.href = url;
    link.download = `${safeName(titleInput.value || "fastchart-song")}.zip`;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
  } catch {
    formError.textContent = "The artwork could not be added to the ZIP. Try choosing it again.";
  } finally {
    downloadButton.disabled = false;
  }
}

function validateArtwork(input) {
  const file = input.files[0];
  if (!file) return true;
  if (!/\.(png|jpe?g)$/i.test(file.name) || !["image/png", "image/jpeg", ""].includes(file.type)) {
    input.value = "";
    formError.textContent = "Artwork must be a PNG or JPG file.";
    return false;
  }
  if (file.size > 15 * 1024 * 1024) {
    input.value = "";
    formError.textContent = "Choose artwork smaller than 15 MB.";
    return false;
  }
  formError.textContent = "";
  return true;
}

function imageExtension(file) {
  return /\.png$/i.test(file.name) ? "png" : "jpg";
}

function checkboxMask(fieldset) { return [...fieldset.querySelectorAll("input:checked")].reduce((mask, input) => mask | Number(input.value), 0); }
function titleFromFilename(name) { return name.replace(/\.[^.]+$/, "").replace(/[_-]+/g, " ").replace(/\s+/g, " ").trim(); }
function safeName(name) { return name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "fastchart-song"; }
function formatBytes(bytes) { return bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(0)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`; }
