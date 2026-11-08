use fastchart_audio::{normalize, normalize_with_companion};
use fastchart_chart_core::{Difficulty, generate_drum_chart, generate_full_chart};
use fastchart_clonehero::{
    SongMetadata, serialize_chart, serialize_song_ini, summarize_chart, summarize_midi,
};
use fastchart_dsp::{
    AnalysisConfig, analyze_mono, analyze_pitched_mono, analyze_pitched_mono_with_guide,
};
use fastchart_music::Instrument;
use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("FastChart error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "stems") {
        generate_from_stems(&args[1..])?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "guide") {
        generate_with_guitar_guide(&args[1..])?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "compare-pitched") {
        let generated = args.get(1).ok_or(
            "usage: fastchart compare-pitched <generated.chart> <notes.mid|notes.chart> <guitar|bass>",
        )?;
        let reference = args.get(2).ok_or(
            "usage: fastchart compare-pitched <generated.chart> <notes.mid|notes.chart> <guitar|bass>",
        )?;
        let instrument = args
            .get(3)
            .ok_or("usage: fastchart compare-pitched <generated.chart> <notes.mid|notes.chart> <guitar|bass>")?
            .to_string_lossy();
        compare_pitched_charts(Path::new(generated), Path::new(reference), &instrument)?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "evaluate") {
        let audio_path = args
            .get(1)
            .ok_or("usage: fastchart evaluate <audio-file> <notes.mid|notes.chart>")?;
        let chart_path = args
            .get(2)
            .ok_or("usage: fastchart evaluate <audio-file> <notes.mid|notes.chart>")?;
        evaluate(Path::new(audio_path), Path::new(chart_path))?;
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "inspect") {
        let path = args
            .get(1)
            .ok_or("usage: fastchart inspect <notes.chart|notes.mid>")?;
        if Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mid"))
        {
            let summary = summarize_midi(&fs::read(path)?)?;
            println!("ticks per quarter: {}", summary.ticks_per_quarter);
            println!("tempo events: {}", summary.tempos.len());
            if let Some(bpm) = summary.initial_bpm() {
                println!("initial tempo: {bpm:.1} BPM");
            }
            for track in summary.tracks {
                let mut drum_lanes = [0_usize; 6];
                let expert_drums = if track.name.eq_ignore_ascii_case("PART DRUMS") {
                    for note in track
                        .notes
                        .iter()
                        .filter(|note| (96..=101).contains(&note.pitch))
                    {
                        drum_lanes[usize::from(note.pitch - 96)] += 1;
                    }
                    drum_lanes.iter().sum()
                } else {
                    0
                };
                println!(
                    "{}: {} note-on events{}",
                    track.name,
                    track.notes.len(),
                    if expert_drums > 0 {
                        format!("; {expert_drums} Expert drum gems")
                    } else {
                        String::new()
                    }
                );
                if expert_drums > 0 {
                    println!("  Expert pitches 96-101: {drum_lanes:?}");
                }
            }
            return Ok(());
        }
        let summary = summarize_chart(&fs::read_to_string(path)?);
        println!(
            "resolution: {}",
            summary
                .resolution
                .map_or_else(|| "unknown".into(), |v| v.to_string())
        );
        println!("tempo events: {}", summary.tempo_events);
        for (section, notes) in summary.sections {
            println!("{section}: {notes} notes");
        }
        return Ok(());
    }
    let input = args
        .first()
        .ok_or("usage: fastchart <audio-file> [output-directory] [song-name] [artist]")?;
    let input = Path::new(input);
    let output = args
        .get(1)
        .map_or_else(|| PathBuf::from("fastchart-output"), PathBuf::from);
    let mut samples = decode_with_ffmpeg(input, 44_100)?;
    normalize(&mut samples);
    println!(
        "Analyzing {:.1} seconds of audio...",
        samples.len() as f64 / 44_100.0
    );
    let config = AnalysisConfig::default();
    let mut analysis = analyze_mono(&samples, 44_100, config);
    analysis.tracks.push(analyze_pitched_mono(
        &samples,
        44_100,
        Instrument::Guitar,
        config,
    ));
    analysis.tracks.push(analyze_pitched_mono(
        &samples,
        44_100,
        Instrument::Bass,
        config,
    ));
    let chart = generate_full_chart(&analysis);
    let extension = compatible_audio_extension(input);
    let audio_filename = format!("song.{extension}");
    let name = args
        .get(2)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            input
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let artist = args
        .get(3)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown Artist".into());
    let metadata = SongMetadata {
        name,
        artist,
        audio_filename: audio_filename.clone(),
        song_length_ms: (analysis.duration_seconds * 1000.0).round() as u64,
        ..SongMetadata::default()
    };
    fs::create_dir_all(&output)?;
    fs::write(
        output.join("notes.chart"),
        serialize_chart(&metadata, &chart),
    )?;
    fs::write(
        output.join("song.ini"),
        serialize_song_ini(&metadata, &chart),
    )?;
    copy_compatible_audio(input, &output, "song")?;
    let drum_events = analysis
        .tracks
        .iter()
        .find(|track| track.instrument == Instrument::Drums)
        .map_or(0, |track| track.events.len());
    let guitar_events = analysis
        .tracks
        .iter()
        .find(|track| track.instrument == Instrument::Guitar)
        .map_or(0, |track| track.events.len());
    let bass_events = analysis
        .tracks
        .iter()
        .find(|track| track.instrument == Instrument::Bass)
        .map_or(0, |track| track.events.len());
    println!(
        "Estimated tempo: {:.1} BPM; detected {drum_events} drum, {guitar_events} guitar, and {bass_events} bass events.",
        analysis.tempo_map.bpm(),
    );
    for difficulty in [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ] {
        println!(
            "{}: drums {}, guitar {}, bass {}",
            difficulty.section(),
            chart.tracks[&difficulty].len(),
            chart.guitar_tracks[&difficulty].len(),
            chart.bass_tracks[&difficulty].len(),
        );
    }
    println!("Wrote Clone Hero song folder to {}", output.display());
    Ok(())
}

fn compare_pitched_charts(
    generated_path: &Path,
    reference_path: &Path,
    instrument: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (section, midi_track) = match instrument.to_ascii_lowercase().as_str() {
        "guitar" => ("ExpertSingle", "PART GUITAR"),
        "bass" => ("ExpertDoubleBass", "PART BASS"),
        _ => return Err("instrument must be guitar or bass".into()),
    };
    let generated_chart = summarize_chart(&fs::read_to_string(generated_path)?);
    let generated_notes = generated_chart
        .notes
        .get(section)
        .ok_or_else(|| format!("generated chart has no {section} section"))?;

    let mut generated_ticks = BTreeMap::<i64, u8>::new();
    for note in generated_notes.iter().filter(|note| note.lane <= 4) {
        *generated_ticks.entry(note.tick).or_default() |= 1 << note.lane;
    }
    let generated: Vec<_> = generated_ticks
        .into_iter()
        .map(|(tick, lanes)| DrumEvent {
            seconds: generated_chart.tick_to_seconds(tick),
            lanes,
        })
        .collect();
    let reference = reference_pitched_events(reference_path, section, midi_track)?;
    let tolerance = 0.075;
    let matches = match_drum_events(&generated, &reference, tolerance);
    let precision = matches.len() as f64 / generated.len().max(1) as f64;
    let recall = matches.len() as f64 / reference.len().max(1) as f64;
    println!(
        "{instrument} Expert onsets: {} generated, {} authored",
        generated.len(),
        reference.len()
    );
    println!(
        "timing within ±75 ms: precision {:.1}%, recall {:.1}%, F1 {:.1}%",
        precision * 100.0,
        recall * 100.0,
        f1(precision, recall) * 100.0
    );
    let lane_matches: u32 = matches
        .iter()
        .map(|(generated_index, reference_index)| {
            (generated[*generated_index].lanes & reference[*reference_index].lanes).count_ones()
        })
        .sum();
    let mut lane_confusion = [[0_u32; 5]; 5];
    for (generated_index, reference_index) in &matches {
        let generated_mask = generated[*generated_index].lanes;
        let reference_mask = reference[*reference_index].lanes;
        if generated_mask.count_ones() == 1 {
            let generated_lane = generated_mask.trailing_zeros() as usize;
            for (reference_lane, count) in lane_confusion[generated_lane].iter_mut().enumerate() {
                if reference_mask & (1 << reference_lane) != 0 {
                    *count += 1;
                }
            }
        }
    }
    let generated_lanes: u32 = generated.iter().map(|event| event.lanes.count_ones()).sum();
    let reference_lanes: u32 = reference.iter().map(|event| event.lanes.count_ones()).sum();
    let lane_precision = f64::from(lane_matches) / f64::from(generated_lanes.max(1));
    let lane_recall = f64::from(lane_matches) / f64::from(reference_lanes.max(1));
    println!(
        "lane-aware gems: precision {:.1}%, recall {:.1}%, F1 {:.1}%",
        lane_precision * 100.0,
        lane_recall * 100.0,
        f1(lane_precision, lane_recall) * 100.0
    );
    println!("matched lane confusion (generated rows, authored columns):");
    for row in lane_confusion {
        println!("  {row:?}");
    }
    Ok(())
}

fn reference_pitched_events(
    path: &Path,
    section: &str,
    midi_track: &str,
) -> Result<Vec<DrumEvent>, Box<dyn std::error::Error>> {
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mid"))
    {
        let midi = summarize_midi(&fs::read(path)?)?;
        let track = midi
            .track(midi_track)
            .ok_or_else(|| format!("reference MIDI has no {midi_track} track"))?;
        let mut by_tick = BTreeMap::<u64, u8>::new();
        for note in track
            .notes
            .iter()
            .filter(|note| (96..=100).contains(&note.pitch))
        {
            *by_tick.entry(note.tick).or_default() |= 1 << (note.pitch - 96);
        }
        return Ok(by_tick
            .into_iter()
            .map(|(tick, lanes)| DrumEvent {
                seconds: midi.tick_to_seconds(tick),
                lanes,
            })
            .collect());
    }

    let chart = summarize_chart(&fs::read_to_string(path)?);
    let mut by_tick = BTreeMap::<i64, u8>::new();
    for note in chart
        .notes
        .get(section)
        .ok_or_else(|| format!("reference chart has no {section} section"))?
        .iter()
        .filter(|note| note.lane <= 4)
    {
        *by_tick.entry(note.tick).or_default() |= 1 << note.lane;
    }
    Ok(by_tick
        .into_iter()
        .map(|(tick, lanes)| DrumEvent {
            seconds: chart.tick_to_seconds(tick),
            lanes,
        })
        .collect())
}

fn generate_from_stems(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 5 {
        return Err("usage: fastchart stems <backing> <guitar> <bass> <drums> <output-directory> [song-name] [artist]".into());
    }
    let backing_path = Path::new(&args[0]);
    let guitar_path = Path::new(&args[1]);
    let bass_path = Path::new(&args[2]);
    let drums_path = Path::new(&args[3]);
    let output = Path::new(&args[4]);
    let config = AnalysisConfig::default();

    println!("Analyzing drum stem...");
    let mut samples = decode_with_ffmpeg(drums_path, 44_100)?;
    normalize(&mut samples);
    let mut analysis = analyze_mono(&samples, 44_100, config);
    drop(samples);

    println!("Analyzing guitar stem...");
    let mut samples = decode_with_ffmpeg(guitar_path, 44_100)?;
    normalize(&mut samples);
    analysis.tracks.push(analyze_pitched_mono(
        &samples,
        44_100,
        Instrument::Guitar,
        config,
    ));
    drop(samples);

    println!("Analyzing bass stem...");
    let mut samples = decode_with_ffmpeg(bass_path, 44_100)?;
    normalize(&mut samples);
    analysis.tracks.push(analyze_pitched_mono(
        &samples,
        44_100,
        Instrument::Bass,
        config,
    ));
    let chart = generate_full_chart(&analysis);

    let name = args
        .get(5)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            backing_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let artist = args
        .get(6)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown Artist".into());
    let backing_extension = compatible_audio_extension(backing_path);
    let metadata = SongMetadata {
        name,
        artist,
        audio_filename: format!("song.{backing_extension}"),
        song_length_ms: (analysis.duration_seconds * 1000.0).round() as u64,
        ..SongMetadata::default()
    };
    fs::create_dir_all(output)?;
    fs::write(
        output.join("notes.chart"),
        serialize_chart(&metadata, &chart),
    )?;
    fs::write(
        output.join("song.ini"),
        serialize_song_ini(&metadata, &chart),
    )?;
    copy_named(backing_path, output, "song")?;
    copy_named(guitar_path, output, "guitar")?;
    copy_named(bass_path, output, "rhythm")?;
    copy_named(drums_path, output, "drums")?;
    if let Some(parent) = backing_path.parent() {
        let album = parent.join("album.jpg");
        if album.is_file() {
            fs::copy(album, output.join("album.jpg"))?;
        }
    }
    println!("Estimated tempo: {:.1} BPM", analysis.tempo_map.bpm());
    for difficulty in [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ] {
        println!(
            "{}: drums {}, guitar {}, bass {}",
            difficulty.section(),
            chart.tracks.get(&difficulty).map_or(0, Vec::len),
            chart.guitar_tracks.get(&difficulty).map_or(0, Vec::len),
            chart.bass_tracks.get(&difficulty).map_or(0, Vec::len),
        );
    }
    println!("Wrote multi-instrument song folder to {}", output.display());
    Ok(())
}

fn generate_with_guitar_guide(
    args: &[std::ffi::OsString],
) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 3 {
        return Err("usage: fastchart guide <audio-file> <guitar-onset-guide> <output-directory> [song-name] [artist]".into());
    }
    let input = Path::new(&args[0]);
    let guide_path = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let mut samples = decode_with_ffmpeg(input, 44_100)?;
    let mut guitar_guide = decode_with_ffmpeg(guide_path, 44_100)?;
    let shared_length = samples.len().min(guitar_guide.len());
    samples.truncate(shared_length);
    guitar_guide.truncate(shared_length);
    normalize_with_companion(&mut samples, &mut guitar_guide);
    let config = AnalysisConfig::default();
    let mut analysis = analyze_mono(&samples, 44_100, config);
    analysis.tracks.push(analyze_pitched_mono_with_guide(
        &samples,
        &guitar_guide,
        44_100,
        Instrument::Guitar,
        config,
    ));
    analysis.tracks.push(analyze_pitched_mono(
        &samples,
        44_100,
        Instrument::Bass,
        config,
    ));
    let chart = generate_full_chart(&analysis);
    let name = args
        .get(3)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            input
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let artist = args
        .get(4)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown Artist".into());
    let extension = compatible_audio_extension(input);
    let metadata = SongMetadata {
        name,
        artist,
        audio_filename: format!("song.{extension}"),
        song_length_ms: (analysis.duration_seconds * 1000.0).round() as u64,
        ..SongMetadata::default()
    };
    fs::create_dir_all(output)?;
    fs::write(
        output.join("notes.chart"),
        serialize_chart(&metadata, &chart),
    )?;
    fs::write(
        output.join("song.ini"),
        serialize_song_ini(&metadata, &chart),
    )?;
    copy_compatible_audio(input, output, "song")?;
    println!(
        "Wrote guide-assisted chart with {} Expert guitar notes to {}",
        chart
            .guitar_tracks
            .get(&Difficulty::Expert)
            .map_or(0, Vec::len),
        output.display()
    );
    Ok(())
}

fn extension_or<'a>(path: &'a Path, fallback: &'a str) -> &'a str {
    path.extension().and_then(OsStr::to_str).unwrap_or(fallback)
}

fn copy_named(source: &Path, output: &Path, stem: &str) -> Result<(), io::Error> {
    copy_compatible_audio(source, output, stem)
}

fn compatible_audio_extension(path: &Path) -> &str {
    let extension = extension_or(path, "wav");
    if matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp3" | "ogg" | "opus" | "wav"
    ) {
        extension
    } else {
        "wav"
    }
}

fn copy_compatible_audio(source: &Path, output: &Path, stem: &str) -> Result<(), io::Error> {
    let source_extension = extension_or(source, "wav");
    let output_extension = compatible_audio_extension(source);
    let destination = output.join(format!("{stem}.{output_extension}"));
    if source_extension.eq_ignore_ascii_case(output_extension) {
        fs::copy(source, destination)?;
        return Ok(());
    }
    let result = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(source)
        .args(["-c:a", "pcm_s16le"])
        .arg(&destination)
        .output()?;
    if result.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "FFmpeg could not create {}: {}",
            destination.display(),
            String::from_utf8_lossy(&result.stderr)
        )))
    }
}

fn evaluate(audio_path: &Path, chart_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut samples = decode_with_ffmpeg(audio_path, 44_100)?;
    normalize(&mut samples);
    let analysis = analyze_mono(&samples, 44_100, AnalysisConfig::default());
    let chart = generate_drum_chart(&analysis);
    let (reference, authored_bpm) = reference_drum_events(chart_path)?;

    let mut generated_by_tick = BTreeMap::<i64, u8>::new();
    for note in &chart.tracks[&Difficulty::Expert] {
        *generated_by_tick.entry(note.tick).or_default() |= 1 << note.lane as u8;
    }
    let generated: Vec<_> = generated_by_tick
        .into_iter()
        .map(|(tick, lanes)| DrumEvent {
            seconds: chart.tick_to_seconds(tick),
            lanes,
        })
        .collect();

    let tolerance = 0.075;
    let matches = match_drum_events(&generated, &reference, tolerance);
    let onset_precision = matches.len() as f64 / generated.len().max(1) as f64;
    let onset_recall = matches.len() as f64 / reference.len().max(1) as f64;
    let lane_matches: u32 = matches
        .iter()
        .map(|(generated_index, reference_index)| {
            (generated[*generated_index].lanes & reference[*reference_index].lanes).count_ones()
        })
        .sum();
    let generated_lanes: u32 = generated.iter().map(|event| event.lanes.count_ones()).sum();
    let reference_lanes: u32 = reference.iter().map(|event| event.lanes.count_ones()).sum();
    let lane_precision = f64::from(lane_matches) / f64::from(generated_lanes.max(1));
    let lane_recall = f64::from(lane_matches) / f64::from(reference_lanes.max(1));
    println!("FastChart tempo: {:.1} BPM", analysis.tempo_map.bpm());
    if let Some(bpm) = authored_bpm {
        println!("authored initial tempo: {bpm:.1} BPM");
    }
    println!(
        "unique Expert onsets: {} generated, {} authored",
        generated.len(),
        reference.len()
    );
    println!(
        "timing within ±{:.0} ms: precision {:.1}%, recall {:.1}%, F1 {:.1}%",
        tolerance * 1000.0,
        onset_precision * 100.0,
        onset_recall * 100.0,
        f1(onset_precision, onset_recall) * 100.0
    );
    println!(
        "lane-aware gems: precision {:.1}%, recall {:.1}%, F1 {:.1}%",
        lane_precision * 100.0,
        lane_recall * 100.0,
        f1(lane_precision, lane_recall) * 100.0
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DrumEvent {
    seconds: f64,
    lanes: u8,
}

fn reference_drum_events(
    chart_path: &Path,
) -> Result<(Vec<DrumEvent>, Option<f64>), Box<dyn std::error::Error>> {
    if chart_path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mid"))
    {
        let midi = summarize_midi(&fs::read(chart_path)?)?;
        let drums = midi
            .track("PART DRUMS")
            .ok_or("MIDI has no PART DRUMS track")?;
        let mut by_tick = BTreeMap::<u64, u8>::new();
        for note in drums
            .notes
            .iter()
            .filter(|note| (96..=100).contains(&note.pitch))
        {
            *by_tick.entry(note.tick).or_default() |= 1 << (note.pitch - 96);
        }
        let events = by_tick
            .into_iter()
            .map(|(tick, lanes)| DrumEvent {
                seconds: midi.tick_to_seconds(tick),
                lanes,
            })
            .collect();
        return Ok((events, midi.initial_bpm()));
    }

    let chart = summarize_chart(&fs::read_to_string(chart_path)?);
    let mut by_tick = BTreeMap::<i64, u8>::new();
    for note in chart
        .notes
        .get("ExpertDrums")
        .ok_or("chart has no ExpertDrums section")?
        .iter()
        .filter(|note| note.lane <= 4)
    {
        *by_tick.entry(note.tick).or_default() |= 1 << note.lane;
    }
    let events = by_tick
        .into_iter()
        .map(|(tick, lanes)| DrumEvent {
            seconds: chart.tick_to_seconds(tick),
            lanes,
        })
        .collect();
    Ok((events, chart.initial_bpm()))
}

fn match_drum_events(
    generated: &[DrumEvent],
    reference: &[DrumEvent],
    tolerance: f64,
) -> Vec<(usize, usize)> {
    let (mut generated_index, mut reference_index) = (0, 0);
    let mut matched = Vec::new();
    while generated_index < generated.len() && reference_index < reference.len() {
        let difference = generated[generated_index].seconds - reference[reference_index].seconds;
        if difference.abs() <= tolerance {
            matched.push((generated_index, reference_index));
            generated_index += 1;
            reference_index += 1;
        } else if difference < 0.0 {
            generated_index += 1;
        } else {
            reference_index += 1;
        }
    }
    matched
}

fn f1(precision: f64, recall: f64) -> f64 {
    if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    }
}

fn decode_with_ffmpeg(
    path: &Path,
    sample_rate: u32,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    decode_with_ffmpeg_channels(path, sample_rate, 1)
}

fn decode_with_ffmpeg_channels(
    path: &Path,
    sample_rate: u32,
    channels: u8,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            &channels.to_string(),
            "-ar",
            &sample_rate.to_string(),
            "pipe:1",
        ])
        .output()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                "could not run ffmpeg; install FFmpeg and ensure it is on PATH",
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "FFmpeg could not decode audio: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output
        .stdout
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().expect("four-byte chunk")))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{DrumEvent, compatible_audio_extension, match_drum_events};
    use std::path::Path;

    #[test]
    fn onset_matching_is_one_to_one() {
        let generated = [
            DrumEvent {
                seconds: 1.0,
                lanes: 1,
            },
            DrumEvent {
                seconds: 1.01,
                lanes: 2,
            },
            DrumEvent {
                seconds: 2.0,
                lanes: 4,
            },
        ];
        let reference = [
            DrumEvent {
                seconds: 1.02,
                lanes: 1,
            },
            DrumEvent {
                seconds: 2.04,
                lanes: 4,
            },
        ];
        assert_eq!(match_drum_events(&generated, &reference, 0.05).len(), 2);
    }

    #[test]
    fn unsupported_clone_hero_audio_uses_wav() {
        assert_eq!(compatible_audio_extension(Path::new("song.flac")), "wav");
        assert_eq!(compatible_audio_extension(Path::new("song.m4a")), "wav");
        assert_eq!(compatible_audio_extension(Path::new("song.opus")), "opus");
    }
}
