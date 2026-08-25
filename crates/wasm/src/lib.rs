//! Dependency-free browser boundary for the FastChart analysis engine.
//!
//! The exported allocation functions form a deliberately small ABI so the static
//! web app can load the module without a JavaScript package manager or bindgen.

use fastchart_audio::normalize;
use fastchart_chart_core::{Difficulty, GeneratedChart, generate_full_chart};
use fastchart_clonehero::{SongMetadata, serialize_chart, serialize_song_ini};
use fastchart_dsp::{AnalysisConfig, analyze_mono, analyze_pitched_mono};
use fastchart_music::Instrument;
use std::sync::atomic::{AtomicUsize, Ordering};

const DRUMS: u32 = 1;
const GUITAR: u32 = 2;
const BASS: u32 = 4;
const ALL_DIFFICULTIES: u32 = 0b1111;

static RESULT_LENGTH: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub extern "C" fn fastchart_alloc_f32(length: usize) -> *mut f32 {
    let mut samples = vec![0.0_f32; length].into_boxed_slice();
    let pointer = samples.as_mut_ptr();
    std::mem::forget(samples);
    pointer
}

#[unsafe(no_mangle)]
pub extern "C" fn fastchart_free_f32(pointer: *mut f32, length: usize) {
    if pointer.is_null() {
        return;
    }
    // SAFETY: callers must pass a pointer and length returned by fastchart_alloc_f32.
    unsafe {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer, length,
        )))
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn fastchart_alloc_bytes(length: usize) -> *mut u8 {
    let mut bytes = vec![0_u8; length].into_boxed_slice();
    let pointer = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    pointer
}

#[unsafe(no_mangle)]
pub extern "C" fn fastchart_free_bytes(pointer: *mut u8, length: usize) {
    if pointer.is_null() {
        return;
    }
    // SAFETY: callers must pass a pointer and length returned by FastChart.
    unsafe {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer, length,
        )))
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn fastchart_result_length() -> usize {
    RESULT_LENGTH.load(Ordering::Relaxed)
}

/// Generates a UTF-8 JSON response and returns its pointer. The caller owns the
/// returned byte buffer and releases it with `fastchart_free_bytes`.
///
/// # Safety
///
/// Every non-null pointer must reference a live allocation containing at least
/// the corresponding length. The allocations must remain live until this call
/// returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fastchart_generate(
    samples_pointer: *const f32,
    samples_length: usize,
    sample_rate: u32,
    title_pointer: *const u8,
    title_length: usize,
    artist_pointer: *const u8,
    artist_length: usize,
    audio_pointer: *const u8,
    audio_length: usize,
    instrument_mask: u32,
    difficulty_mask: u32,
) -> *mut u8 {
    let response = if samples_pointer.is_null() || samples_length == 0 || sample_rate == 0 {
        error_json("The decoded audio was empty.")
    } else {
        // SAFETY: JavaScript writes these allocations before invoking this function and
        // keeps them alive until it returns.
        let samples = unsafe { std::slice::from_raw_parts(samples_pointer, samples_length) };
        let title = read_string(title_pointer, title_length, "Unknown Song");
        let artist = read_string(artist_pointer, artist_length, "Unknown Artist");
        let audio_filename = read_string(audio_pointer, audio_length, "song.wav");
        generate_json(
            samples,
            sample_rate,
            &title,
            &artist,
            &audio_filename,
            instrument_mask,
            difficulty_mask,
        )
    };
    let mut output = response.into_bytes().into_boxed_slice();
    RESULT_LENGTH.store(output.len(), Ordering::Relaxed);
    let pointer = output.as_mut_ptr();
    std::mem::forget(output);
    pointer
}

fn read_string(pointer: *const u8, length: usize, fallback: &str) -> String {
    if pointer.is_null() || length == 0 {
        return fallback.to_owned();
    }
    // SAFETY: the browser boundary supplies a live allocation of exactly `length` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    String::from_utf8_lossy(bytes).trim().to_owned()
}

fn generate_json(
    input_samples: &[f32],
    sample_rate: u32,
    title: &str,
    artist: &str,
    audio_filename: &str,
    instrument_mask: u32,
    difficulty_mask: u32,
) -> String {
    let instrument_mask = if instrument_mask == 0 {
        DRUMS
    } else {
        instrument_mask
    };
    let difficulty_mask = if difficulty_mask == 0 {
        ALL_DIFFICULTIES
    } else {
        difficulty_mask
    };
    let mut samples = input_samples.to_vec();
    normalize(&mut samples);
    let config = AnalysisConfig::default();
    let mut analysis = analyze_mono(&samples, sample_rate, config);
    if instrument_mask & GUITAR != 0 {
        analysis.tracks.push(analyze_pitched_mono(
            &samples,
            sample_rate,
            Instrument::Guitar,
            config,
        ));
    }
    if instrument_mask & BASS != 0 {
        analysis.tracks.push(analyze_pitched_mono(
            &samples,
            sample_rate,
            Instrument::Bass,
            config,
        ));
    }
    let mut chart = generate_full_chart(&analysis);
    filter_chart(&mut chart, instrument_mask, difficulty_mask);
    let metadata = SongMetadata {
        name: title.to_owned(),
        artist: artist.to_owned(),
        audio_filename: audio_filename.to_owned(),
        song_length_ms: (analysis.duration_seconds * 1000.0).round() as u64,
        ..SongMetadata::default()
    };
    let chart_text = serialize_chart(&metadata, &chart);
    let ini_text = serialize_song_ini(&metadata, &chart);
    let drum_notes = note_count(&chart.tracks);
    let guitar_notes = note_count(&chart.guitar_tracks);
    let bass_notes = note_count(&chart.bass_tracks);
    format!(
        "{{\"chart\":\"{}\",\"ini\":\"{}\",\"bpm\":{:.1},\"durationSeconds\":{:.3},\"tempoRegions\":{},\"notes\":{{\"drums\":{},\"guitar\":{},\"bass\":{}}}}}",
        json_escape(&chart_text),
        json_escape(&ini_text),
        chart.bpm,
        analysis.duration_seconds,
        chart.tempo_points.len(),
        drum_notes,
        guitar_notes,
        bass_notes,
    )
}

fn filter_chart(chart: &mut GeneratedChart, instruments: u32, difficulties: u32) {
    if instruments & DRUMS == 0 {
        chart.tracks.clear();
    }
    if instruments & GUITAR == 0 {
        chart.guitar_tracks.clear();
    }
    if instruments & BASS == 0 {
        chart.bass_tracks.clear();
    }
    chart
        .tracks
        .retain(|difficulty, _| selected(*difficulty, difficulties));
    chart
        .guitar_tracks
        .retain(|difficulty, _| selected(*difficulty, difficulties));
    chart
        .bass_tracks
        .retain(|difficulty, _| selected(*difficulty, difficulties));
}

fn selected(difficulty: Difficulty, mask: u32) -> bool {
    let bit = match difficulty {
        Difficulty::Easy => 1,
        Difficulty::Medium => 2,
        Difficulty::Hard => 4,
        Difficulty::Expert => 8,
    };
    mask & bit != 0
}

fn note_count<T>(tracks: &std::collections::BTreeMap<Difficulty, Vec<T>>) -> usize {
    tracks.values().map(Vec::len).sum()
}

fn error_json(message: &str) -> String {
    format!("{{\"error\":\"{}\"}}", json_escape(message))
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_is_valid_enough_for_browser_json_parser() {
        let samples = vec![0.0; 4_410];
        let response = generate_json(
            &samples,
            44_100,
            "A \"title\"",
            "An artist",
            "song.wav",
            DRUMS | GUITAR | BASS,
            ALL_DIFFICULTIES,
        );
        assert!(response.starts_with("{\"chart\":"));
        assert!(response.contains("A \\\"title\\\""));
        assert!(response.contains("\"tempoRegions\":"));
    }

    #[test]
    fn instrument_filter_removes_unselected_sections() {
        let response = generate_json(&[0.0; 2_205], 44_100, "Song", "Artist", "song.wav", BASS, 8);
        assert!(!response.contains("ExpertDrums"));
        assert!(!response.contains("ExpertSingle"));
        assert!(response.contains("ExpertDoubleBass"));
    }
}
