# FastChart

FastChart is an experimental, local-first Clone Hero chart generator. The current Rust MVP uses FFT spectral flux to estimate tempo changes and classify percussion transients, creates four drum difficulties, and writes a Clone Hero song folder.

Pitched stem analysis can also generate playable five-lane guitar and bass abstractions with sustains and four derived difficulties.

## Run

FFmpeg must be available on `PATH` for native audio decoding.

```powershell
cargo run -p fastchart -- "path/to/song.opus" fastchart-output "Song Name" "Artist"
```

The ordinary single-file command analyzes drums, guitar, and bass from the same local mix. Separated stems generally produce better instrument classification when available.

Generate a combined drums, guitar, and bass chart from separated stems:

```powershell
cargo run -p fastchart -- stems "backing.opus" "guitar.opus" "bass.opus" "drums.wav" fastchart-output "Song Name" "Artist"
```

Inspect an existing chart:

```powershell
cargo run -p fastchart -- inspect "path/to/notes.chart"
```

Both `.chart` and `notes.mid` files can be inspected. To evaluate generated onset timing and drum-lane agreement against an authored chart:

```powershell
cargo run -p fastchart -- evaluate "path/to/audio.wav" "path/to/notes.mid-or-chart"
```

Compare generated guitar or bass timing with an authored MIDI:

```powershell
cargo run -p fastchart -- compare-pitched "generated/notes.chart" "reference/notes.mid-or-chart" guitar
```

Run the full test suite:

```powershell
cargo test --workspace
```

The reusable audio/DSP/chart crates do not invoke FFmpeg. The browser app decodes with the Web Audio API and passes mono PCM into `fastchart-dsp`, keeping audio entirely local.

## Browser app

The static browser app lives in `apps/web`. It runs analysis in a Web Worker,
calls the Rust engine through WebAssembly, and packages `notes.chart`, `song.ini`,
and Clone Hero-compatible audio into a ready-to-play ZIP without uploading the
song. MP3, OGG, OPUS, and WAV inputs are preserved; other decoded formats such
as FLAC and M4A are converted locally to 16-bit PCM WAV.

```powershell
cd apps/web
npm run build
npm run test:wasm
npm run serve
```

Then open `http://127.0.0.1:4173`. Building requires Rust's
`wasm32-unknown-unknown` target (`rustup target add wasm32-unknown-unknown`).
