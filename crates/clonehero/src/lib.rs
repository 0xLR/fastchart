//! Clone Hero `.chart` and `song.ini` serialization.

use fastchart_chart_core::{Difficulty, GeneratedChart};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongMetadata {
    pub name: String,
    pub artist: String,
    pub album: String,
    pub album_track: String,
    pub genre: String,
    pub year: String,
    pub charter: String,
    pub audio_filename: String,
    pub song_length_ms: u64,
}

impl Default for SongMetadata {
    fn default() -> Self {
        Self {
            name: "Unknown Song".into(),
            artist: "Unknown Artist".into(),
            album: String::new(),
            album_track: String::new(),
            genre: String::new(),
            year: String::new(),
            charter: "FastChart".into(),
            audio_filename: "song.ogg".into(),
            song_length_ms: 0,
        }
    }
}

pub fn serialize_chart(metadata: &SongMetadata, chart: &GeneratedChart) -> String {
    let mut output = format!(
        "[Song]\n{{\n  Name = \"{}\"\n  Artist = \"{}\"\n  Charter = \"{}\"\n  Album = \"{}\"\n  Year = \"{}\"\n  Offset = 0\n  Resolution = {}\n  Player2 = bass\n  Difficulty = 0\n  PreviewStart = 0\n  PreviewEnd = 0\n  Genre = \"{}\"\n  MediaType = \"cd\"\n  MusicStream = \"{}\"\n}}\n[SyncTrack]\n{{\n  0 = TS 4\n",
        escape(&metadata.name),
        escape(&metadata.artist),
        escape(&metadata.charter),
        escape(&metadata.album),
        escape(&metadata.year),
        chart.resolution,
        escape(&metadata.genre),
        escape(&metadata.audio_filename)
    );
    if chart.tempo_points.is_empty() {
        output.push_str(&format!(
            "  0 = B {}\n",
            (chart.bpm * 1000.0).round() as u64
        ));
    } else {
        for point in &chart.tempo_points {
            output.push_str(&format!(
                "  {} = B {}\n",
                point.tick,
                (point.bpm * 1000.0).round() as u64
            ));
        }
    }
    output.push_str("}\n[Events]\n{\n  0 = E \"section FastChart Generated\"\n}\n");
    if !chart.tracks.is_empty() {
        for difficulty in [
            Difficulty::Easy,
            Difficulty::Medium,
            Difficulty::Hard,
            Difficulty::Expert,
        ] {
            output.push_str(&format!("[{}]\n{{\n", difficulty.section()));
            if let Some(notes) = chart.tracks.get(&difficulty) {
                for note in notes {
                    output.push_str(&format!(
                        "  {} = N {} {}\n",
                        note.tick, note.lane as u8, note.sustain
                    ));
                }
            }
            output.push_str("}\n");
        }
    }
    serialize_five_lane_tracks(
        &mut output,
        &chart.guitar_tracks,
        Difficulty::guitar_section,
    );
    serialize_five_lane_tracks(&mut output, &chart.bass_tracks, Difficulty::bass_section);
    output
}

fn serialize_five_lane_tracks(
    output: &mut String,
    tracks: &BTreeMap<Difficulty, Vec<fastchart_chart_core::FiveLaneNote>>,
    section: fn(Difficulty) -> &'static str,
) {
    if tracks.is_empty() {
        return;
    }
    for difficulty in [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Expert,
    ] {
        output.push_str(&format!("[{}]\n{{\n", section(difficulty)));
        if let Some(notes) = tracks.get(&difficulty) {
            for note in notes {
                output.push_str(&format!(
                    "  {} = N {} {}\n",
                    note.tick, note.lane, note.sustain
                ));
            }
        }
        output.push_str("}\n");
    }
}

pub fn serialize_song_ini(metadata: &SongMetadata, chart: &GeneratedChart) -> String {
    let mut output = format!(
        "[song]\nname = {}\nartist = {}\nalbum = {}\ngenre = {}\nyear = {}\ncharter = {}\nsong_length = {}\n",
        ini(&metadata.name),
        ini(&metadata.artist),
        ini(&metadata.album),
        ini(&metadata.genre),
        ini(&metadata.year),
        ini(&metadata.charter),
        metadata.song_length_ms
    );
    if !chart.tracks.is_empty() {
        output.push_str("diff_drums = 1\n");
    }
    if !chart.guitar_tracks.is_empty() {
        output.push_str("diff_guitar = 1\n");
    }
    if !chart.bass_tracks.is_empty() {
        output.push_str("diff_bass = 1\n");
    }
    if !metadata.album_track.trim().is_empty() {
        output.push_str(&format!(
            "album_track = {}\n",
            ini(metadata.album_track.trim())
        ));
    }
    output
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChartSummary {
    pub resolution: Option<u32>,
    pub tempo_events: usize,
    pub sections: BTreeMap<String, usize>,
    pub tempos: Vec<(i64, u64)>,
    pub notes: BTreeMap<String, Vec<ChartFileNote>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartFileNote {
    pub tick: i64,
    pub lane: u8,
    pub sustain: i64,
}

impl ChartSummary {
    pub fn initial_bpm(&self) -> Option<f64> {
        self.tempos
            .iter()
            .min_by_key(|(tick, _)| *tick)
            .map(|(_, value)| *value as f64 / 1000.0)
    }

    pub fn tick_to_seconds(&self, target_tick: i64) -> f64 {
        let Some(resolution) = self.resolution.filter(|resolution| *resolution > 0) else {
            return 0.0;
        };
        let mut tempos = self.tempos.clone();
        tempos.sort_by_key(|(tick, _)| *tick);
        let mut current_tick = 0_i64;
        let mut bpm_value = 120_000_u64;
        let mut seconds = 0.0;
        for (tick, next_bpm) in tempos {
            if tick > target_tick {
                break;
            }
            seconds +=
                chart_tick_span_seconds(tick.saturating_sub(current_tick), bpm_value, resolution);
            current_tick = tick;
            bpm_value = next_bpm;
        }
        seconds
            + chart_tick_span_seconds(
                target_tick.saturating_sub(current_tick),
                bpm_value,
                resolution,
            )
    }
}

fn chart_tick_span_seconds(ticks: i64, bpm_value: u64, resolution: u32) -> f64 {
    ticks.max(0) as f64 * 60_000.0 / bpm_value as f64 / f64::from(resolution)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidiTrackSummary {
    pub name: String,
    pub notes: Vec<MidiNote>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MidiNote {
    pub tick: u64,
    pub pitch: u8,
    pub velocity: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MidiSummary {
    pub ticks_per_quarter: u16,
    pub tempos: Vec<(u64, u32)>,
    pub tracks: Vec<MidiTrackSummary>,
}

impl MidiSummary {
    pub fn initial_bpm(&self) -> Option<f64> {
        self.tempos
            .iter()
            .min_by_key(|(tick, _)| tick)
            .map(|(_, micros_per_quarter)| 60_000_000.0 / f64::from(*micros_per_quarter))
    }

    pub fn track(&self, name: &str) -> Option<&MidiTrackSummary> {
        self.tracks
            .iter()
            .find(|track| track.name.eq_ignore_ascii_case(name))
    }

    pub fn tick_to_seconds(&self, target_tick: u64) -> f64 {
        if self.ticks_per_quarter == 0 {
            return 0.0;
        }
        let mut tempos = self.tempos.clone();
        tempos.sort_by_key(|(tick, _)| *tick);
        let mut current_tick = 0_u64;
        let mut micros_per_quarter = 500_000_u32;
        let mut seconds = 0.0;
        for (tick, next_tempo) in tempos {
            if tick > target_tick {
                break;
            }
            seconds += midi_tick_span_seconds(
                tick.saturating_sub(current_tick),
                micros_per_quarter,
                self.ticks_per_quarter,
            );
            current_tick = tick;
            micros_per_quarter = next_tempo;
        }
        seconds
            + midi_tick_span_seconds(
                target_tick.saturating_sub(current_tick),
                micros_per_quarter,
                self.ticks_per_quarter,
            )
    }
}

fn midi_tick_span_seconds(ticks: u64, micros_per_quarter: u32, division: u16) -> f64 {
    ticks as f64 * f64::from(micros_per_quarter) / f64::from(division) / 1_000_000.0
}

/// Reads the portions of a Standard MIDI File needed to inspect Clone Hero fixtures.
pub fn summarize_midi(input: &[u8]) -> Result<MidiSummary, String> {
    if input.len() < 14 || &input[..4] != b"MThd" {
        return Err("not a Standard MIDI File".into());
    }
    let header_len = be_u32(&input[4..8]) as usize;
    if header_len < 6 || input.len() < 8 + header_len {
        return Err("invalid MIDI header length".into());
    }
    let track_count = be_u16(&input[10..12]);
    let division = be_u16(&input[12..14]);
    if division & 0x8000 != 0 {
        return Err("SMPTE MIDI time division is not supported".into());
    }
    let mut summary = MidiSummary {
        ticks_per_quarter: division,
        ..MidiSummary::default()
    };
    let mut position = 8 + header_len;
    for track_index in 0..track_count {
        if position + 8 > input.len() || &input[position..position + 4] != b"MTrk" {
            return Err(format!("missing MIDI track chunk {track_index}"));
        }
        let length = be_u32(&input[position + 4..position + 8]) as usize;
        position += 8;
        let end = position
            .checked_add(length)
            .filter(|end| *end <= input.len())
            .ok_or("MIDI track extends past end of file")?;
        let mut tick = 0_u64;
        let mut running_status = None;
        let mut track = MidiTrackSummary::default();
        while position < end {
            tick += u64::from(read_variable(input, &mut position, end)?);
            if position >= end {
                return Err("missing MIDI event".into());
            }
            let status = if input[position] & 0x80 != 0 {
                let status = input[position];
                position += 1;
                if status < 0xf0 {
                    running_status = Some(status);
                }
                status
            } else {
                running_status.ok_or("MIDI running status without prior status")?
            };
            match status {
                0x80..=0x8f | 0xa0..=0xaf | 0xb0..=0xbf | 0xe0..=0xef => {
                    take(input, &mut position, end, 2)?;
                }
                0x90..=0x9f => {
                    let event = take(input, &mut position, end, 2)?;
                    if event[1] != 0 {
                        track.notes.push(MidiNote {
                            tick,
                            pitch: event[0],
                            velocity: event[1],
                        });
                    }
                }
                0xc0..=0xdf => {
                    take(input, &mut position, end, 1)?;
                }
                0xff => {
                    let kind = take(input, &mut position, end, 1)?[0];
                    let length = read_variable(input, &mut position, end)? as usize;
                    let data = take(input, &mut position, end, length)?;
                    if kind == 0x03 {
                        track.name = String::from_utf8_lossy(data).into_owned();
                    } else if kind == 0x51 && data.len() == 3 {
                        let micros =
                            u32::from(data[0]) << 16 | u32::from(data[1]) << 8 | u32::from(data[2]);
                        summary.tempos.push((tick, micros));
                    }
                }
                0xf0 | 0xf7 => {
                    let length = read_variable(input, &mut position, end)? as usize;
                    take(input, &mut position, end, length)?;
                }
                _ => return Err(format!("unsupported MIDI status 0x{status:02x}")),
            }
        }
        if track.name.is_empty() {
            track.name = format!("Track {track_index}");
        }
        summary.tracks.push(track);
    }
    Ok(summary)
}

fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_variable(input: &[u8], position: &mut usize, end: usize) -> Result<u32, String> {
    let mut value = 0_u32;
    for _ in 0..4 {
        let byte = take(input, position, end, 1)?[0];
        value = (value << 7) | u32::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("invalid MIDI variable-length value".into())
}

fn take<'a>(
    input: &'a [u8],
    position: &mut usize,
    end: usize,
    length: usize,
) -> Result<&'a [u8], String> {
    let next = position.checked_add(length).ok_or("MIDI offset overflow")?;
    if next > end || next > input.len() {
        return Err("truncated MIDI event".into());
    }
    let data = &input[*position..next];
    *position = next;
    Ok(data)
}

pub fn summarize_chart(input: &str) -> ChartSummary {
    let mut summary = ChartSummary::default();
    let mut section = String::new();
    for line in input
        .lines()
        .map(|line| line.trim().trim_start_matches('\u{feff}'))
    {
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_owned();
            summary.sections.entry(section.clone()).or_default();
        } else if section == "Song" && line.starts_with("Resolution") {
            summary.resolution = line
                .split('=')
                .nth(1)
                .and_then(|value| value.trim().parse().ok());
        } else if let Some((tick, event)) = line.split_once('=') {
            let Some(tick) = tick.trim().parse::<i64>().ok() else {
                continue;
            };
            let mut parts = event.split_whitespace();
            match parts.next() {
                Some("B") if section == "SyncTrack" => {
                    if let Some(bpm) = parts.next().and_then(|value| value.parse().ok()) {
                        summary.tempos.push((tick, bpm));
                        summary.tempo_events += 1;
                    }
                }
                Some("N") => {
                    if let (Some(lane), Some(sustain)) = (
                        parts.next().and_then(|value| value.parse().ok()),
                        parts.next().and_then(|value| value.parse().ok()),
                    ) {
                        summary
                            .notes
                            .entry(section.clone())
                            .or_default()
                            .push(ChartFileNote {
                                tick,
                                lane,
                                sustain,
                            });
                        *summary.sections.entry(section.clone()).or_default() += 1;
                    }
                }
                _ => {}
            }
        }
    }
    summary
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], " ")
}
fn ini(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastchart_chart_core::{ChartTempoPoint, GeneratedChart};

    #[test]
    fn generated_chart_can_be_summarized() {
        let chart = GeneratedChart {
            resolution: 192,
            bpm: 123.4,
            tempo_points: Vec::new(),
            tracks: BTreeMap::new(),
            guitar_tracks: BTreeMap::new(),
            bass_tracks: BTreeMap::new(),
        };
        let text = serialize_chart(&SongMetadata::default(), &chart);
        let summary = summarize_chart(&text);
        assert_eq!(summary.resolution, Some(192));
        assert_eq!(summary.tempo_events, 1);
        assert!(!summary.sections.contains_key("ExpertDrums"));
    }

    #[test]
    fn serializes_variable_tempo_points() {
        let chart = GeneratedChart {
            resolution: 192,
            bpm: 120.0,
            tempo_points: vec![
                ChartTempoPoint {
                    tick: 0,
                    bpm: 120.0,
                },
                ChartTempoPoint {
                    tick: 384,
                    bpm: 130.0,
                },
            ],
            tracks: BTreeMap::new(),
            guitar_tracks: BTreeMap::new(),
            bass_tracks: BTreeMap::new(),
        };
        let summary = summarize_chart(&serialize_chart(&SongMetadata::default(), &chart));
        assert_eq!(summary.tempo_events, 2);
        assert_eq!(summary.tempos, vec![(0, 120_000), (384, 130_000)]);
    }

    #[test]
    fn serializes_optional_song_metadata() {
        let metadata = SongMetadata {
            name: "September".into(),
            artist: "Earth, Wind & Fire".into(),
            album: "The Best of Earth, Wind & Fire, Vol. 1".into(),
            album_track: "1".into(),
            genre: "Funk".into(),
            year: "1978".into(),
            charter: "FastChart Tester".into(),
            ..SongMetadata::default()
        };
        let chart = GeneratedChart {
            resolution: 192,
            bpm: 126.0,
            tempo_points: Vec::new(),
            tracks: BTreeMap::new(),
            guitar_tracks: BTreeMap::new(),
            bass_tracks: BTreeMap::new(),
        };

        let chart_text = serialize_chart(&metadata, &chart);
        let ini_text = serialize_song_ini(&metadata, &chart);
        assert!(chart_text.contains("Album = \"The Best of Earth, Wind & Fire, Vol. 1\""));
        assert!(chart_text.contains("Year = \"1978\""));
        assert!(ini_text.contains("genre = Funk"));
        assert!(ini_text.contains("charter = FastChart Tester"));
        assert!(ini_text.contains("album_track = 1"));
    }

    #[test]
    fn parses_clone_hero_reference_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test_assets/Avenged Sevenfold - Acid Rain (Nea7x)/notes.chart"
        );
        let input = std::fs::read_to_string(path).expect("reference chart");
        let summary = summarize_chart(&input);
        assert_eq!(summary.resolution, Some(192));
        assert_eq!(summary.tempo_events, 9);
        assert!(summary.sections["ExpertSingle"] > 100);
    }

    #[test]
    fn parses_midi_drum_reference_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test_assets/Megadeth - Symphony of Destruction (Harmonix)/notes.mid"
        );
        let input = std::fs::read(path).expect("MIDI reference chart");
        let summary = summarize_midi(&input).expect("valid MIDI fixture");
        let drums = summary.track("PART DRUMS").expect("drum track");
        let expert_notes = drums
            .notes
            .iter()
            .filter(|note| (96..=101).contains(&note.pitch))
            .count();
        assert!(summary.initial_bpm().is_some());
        assert!(expert_notes > 100);
    }
}
