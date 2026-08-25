const CLONE_HERO_AUDIO_EXTENSIONS = new Set(["mp3", "ogg", "opus", "wav"]);

export function audioExtension(filename) {
  return filename.split(".").pop().toLowerCase().replace(/[^a-z0-9]/g, "") || "wav";
}

export function isCloneHeroAudioExtension(extension) {
  return CLONE_HERO_AUDIO_EXTENSIONS.has(extension.toLowerCase());
}

export function mixToMono(audioBuffer) {
  const mono = new Float32Array(audioBuffer.length);
  for (let channel = 0; channel < audioBuffer.numberOfChannels; channel += 1) {
    const input = audioBuffer.getChannelData(channel);
    for (let index = 0; index < mono.length; index += 1) mono[index] += input[index];
  }
  const scale = 1 / audioBuffer.numberOfChannels;
  for (let index = 0; index < mono.length; index += 1) mono[index] *= scale;
  return mono;
}

/** Encodes a browser AudioBuffer as stereo-or-mono 16-bit PCM WAV. */
export function encodePcm16Wave(audioBuffer) {
  const channelCount = Math.min(audioBuffer.numberOfChannels, 2);
  const bytesPerSample = 2;
  const dataLength = audioBuffer.length * channelCount * bytesPerSample;
  const output = new ArrayBuffer(44 + dataLength);
  const view = new DataView(output);
  writeAscii(view, 0, "RIFF");
  view.setUint32(4, 36 + dataLength, true);
  writeAscii(view, 8, "WAVE");
  writeAscii(view, 12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, channelCount, true);
  view.setUint32(24, audioBuffer.sampleRate, true);
  view.setUint32(28, audioBuffer.sampleRate * channelCount * bytesPerSample, true);
  view.setUint16(32, channelCount * bytesPerSample, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, "data");
  view.setUint32(40, dataLength, true);

  const channels = Array.from({ length: channelCount }, (_, channel) =>
    audioBuffer.getChannelData(channel),
  );
  let offset = 44;
  for (let frame = 0; frame < audioBuffer.length; frame += 1) {
    for (const channel of channels) {
      const sample = Math.max(-1, Math.min(1, channel[frame]));
      view.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
      offset += bytesPerSample;
    }
  }
  return output;
}

function writeAscii(view, offset, value) {
  for (let index = 0; index < value.length; index += 1) {
    view.setUint8(offset + index, value.charCodeAt(index));
  }
}
