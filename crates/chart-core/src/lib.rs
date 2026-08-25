//! Deterministic conversion from generic musical events into playable drum charts.

use fastchart_music::{Instrument, PercussionKind, SongAnalysis, quantize_tick};
use std::collections::BTreeMap;

pub const RESOLUTION: u32 = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
    Expert,
}

impl Difficulty {
    pub fn section(self) -> &'static str {
        match self {
            Self::Easy => "EasyDrums",
            Self::Medium => "MediumDrums",
            Self::Hard => "HardDrums",
            Self::Expert => "ExpertDrums",
        }
    }

    pub fn guitar_section(self) -> &'static str {
        match self {
            Self::Easy => "EasySingle",
            Self::Medium => "MediumSingle",
            Self::Hard => "HardSingle",
            Self::Expert => "ExpertSingle",
        }
    }

    pub fn bass_section(self) -> &'static str {
        match self {
            Self::Easy => "EasyDoubleBass",
            Self::Medium => "MediumDoubleBass",
            Self::Hard => "HardDoubleBass",
            Self::Expert => "ExpertDoubleBass",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DrumLane {
    Kick = 0,
    Red = 1,
    Yellow = 2,
    Blue = 3,
    Green = 4,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChartNote {
    pub tick: i64,
    pub lane: DrumLane,
    pub sustain: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiveLaneNote {
    pub tick: i64,
    pub lane: u8,
    pub sustain: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedChart {
    pub resolution: u32,
    pub bpm: f64,
    pub tempo_points: Vec<ChartTempoPoint>,
    pub tracks: BTreeMap<Difficulty, Vec<ChartNote>>,
    pub guitar_tracks: BTreeMap<Difficulty, Vec<FiveLaneNote>>,
    pub bass_tracks: BTreeMap<Difficulty, Vec<FiveLaneNote>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartTempoPoint {
    pub tick: i64,
    pub bpm: f64,
}

impl GeneratedChart {
    pub fn tick_to_seconds(&self, target_tick: i64) -> f64 {
        let mut points = self.tempo_points.clone();
        points.sort_by_key(|point| point.tick);
        let mut current_tick = 0_i64;
        let mut current_bpm = points.first().map_or(self.bpm, |point| point.bpm);
        let mut seconds = 0.0;
        for point in points.into_iter().skip(1) {
            if point.tick > target_tick {
                break;
            }
            seconds += (point.tick - current_tick).max(0) as f64 * 60.0
                / (current_bpm * f64::from(self.resolution));
            current_tick = point.tick;
            current_bpm = point.bpm;
        }
        seconds
            + (target_tick - current_tick).max(0) as f64 * 60.0
                / (current_bpm * f64::from(self.resolution))
    }
}

pub fn generate_drum_chart(analysis: &SongAnalysis) -> GeneratedChart {
    let bpm = analysis.tempo_map.bpm();
    let phase_tick = analysis
        .tempo_map
        .seconds_to_ticks(analysis.beat_offset_seconds, RESOLUTION);
    let source = analysis
        .tracks
        .iter()
        .find(|track| track.instrument == Instrument::Drums);
    let expert = source.map_or_else(Vec::new, |track| {
        let mut notes: Vec<_> = track
            .events
            .iter()
            .filter(|event| event.confidence >= 0.42)
            .map(|event| {
                let event_tick = analysis
                    .tempo_map
                    .seconds_to_ticks(event.start_seconds, RESOLUTION);
                let tick =
                    (phase_tick + quantize_tick(event_tick - phase_tick, RESOLUTION, 8)).max(0);
                ChartNote {
                    tick,
                    lane: lane_for(
                        event.percussion.unwrap_or(PercussionKind::Unknown),
                        event.velocity,
                    ),
                    sustain: 0,
                }
            })
            .collect();
        notes.sort_by_key(|note| (note.tick, note.lane));
        notes.dedup_by_key(|note| (note.tick, note.lane));
        enforce_playability(&mut notes, bpm);
        notes
    });
    let mut tracks = BTreeMap::new();
    tracks.insert(Difficulty::Expert, expert.clone());
    tracks.insert(
        Difficulty::Hard,
        reduce(&expert, Difficulty::Hard, phase_tick),
    );
    tracks.insert(
        Difficulty::Medium,
        reduce(&expert, Difficulty::Medium, phase_tick),
    );
    tracks.insert(
        Difficulty::Easy,
        reduce(&expert, Difficulty::Easy, phase_tick),
    );
    GeneratedChart {
        resolution: RESOLUTION,
        bpm,
        tempo_points: analysis
            .tempo_map
            .points
            .iter()
            .map(|point| ChartTempoPoint {
                tick: analysis
                    .tempo_map
                    .seconds_to_ticks(point.start_seconds, RESOLUTION),
                bpm: point.bpm,
            })
            .collect(),
        tracks,
        guitar_tracks: BTreeMap::new(),
        bass_tracks: BTreeMap::new(),
    }
}

pub fn generate_full_chart(analysis: &SongAnalysis) -> GeneratedChart {
    let mut chart = generate_drum_chart(analysis);
    chart.guitar_tracks = generate_pitched_difficulties(analysis, Instrument::Guitar);
    chart.bass_tracks = generate_pitched_difficulties(analysis, Instrument::Bass);
    chart
}

fn generate_pitched_difficulties(
    analysis: &SongAnalysis,
    instrument: Instrument,
) -> BTreeMap<Difficulty, Vec<FiveLaneNote>> {
    let Some(track) = analysis
        .tracks
        .iter()
        .find(|track| track.instrument == instrument)
    else {
        return BTreeMap::new();
    };
    let phase_tick = analysis
        .tempo_map
        .seconds_to_ticks(analysis.beat_offset_seconds, RESOLUTION);
    let mut source: Vec<_> = track
        .events
        .iter()
        .filter(|event| event.confidence >= 0.48 && event.pitch.is_some())
        .collect();
    source.sort_by(|left, right| left.start_seconds.total_cmp(&right.start_seconds));
    let mut pitches: Vec<_> = source.iter().filter_map(|event| event.pitch).collect();
    pitches.sort_unstable();
    let low_pitch = pitches.get(pitches.len() / 10).copied().unwrap_or(48);
    let high_pitch = pitches
        .get(pitches.len().saturating_mul(9) / 10)
        .copied()
        .unwrap_or(low_pitch.saturating_add(12));
    let mut expert = Vec::new();
    let mut remembered_lanes = BTreeMap::<u8, u8>::new();
    let mut previous_lane = 2_u8;
    let mut previous_pitch = None;
    for event in source {
        let pitch = event.pitch.expect("filtered pitched event");
        let target = *remembered_lanes.entry(pitch).or_insert_with(|| {
            if high_pitch <= low_pitch {
                2
            } else {
                (((pitch.saturating_sub(low_pitch)) as f32 / f32::from(high_pitch - low_pitch)
                    * 4.0)
                    .round() as i16)
                    .clamp(0, 4) as u8
            }
        });
        let lane = if previous_pitch == Some(pitch) {
            previous_lane
        } else {
            target.clamp(previous_lane.saturating_sub(2), (previous_lane + 2).min(4))
        };
        let event_tick = analysis
            .tempo_map
            .seconds_to_ticks(event.start_seconds, RESOLUTION);
        let tick = (phase_tick + quantize_tick(event_tick - phase_tick, RESOLUTION, 8)).max(0);
        let end_tick = analysis
            .tempo_map
            .seconds_to_ticks(event.start_seconds + event.duration_seconds, RESOLUTION);
        let raw_sustain = (end_tick - event_tick).max(0);
        let sustain = if raw_sustain >= i64::from(RESOLUTION / 3) {
            quantize_tick(raw_sustain, RESOLUTION, 8).max(i64::from(RESOLUTION / 4))
        } else {
            0
        };
        if let Some(existing) = expert
            .last_mut()
            .filter(|note: &&mut FiveLaneNote| note.tick == tick)
        {
            if sustain > existing.sustain {
                existing.lane = lane;
                existing.sustain = sustain;
            }
        } else {
            expert.push(FiveLaneNote {
                tick,
                lane,
                sustain,
            });
            previous_lane = lane;
            previous_pitch = Some(pitch);
        }
    }
    let mut tracks = BTreeMap::new();
    tracks.insert(Difficulty::Expert, expert.clone());
    tracks.insert(Difficulty::Hard, reduce_pitched(&expert, Difficulty::Hard));
    tracks.insert(
        Difficulty::Medium,
        reduce_pitched(&expert, Difficulty::Medium),
    );
    tracks.insert(Difficulty::Easy, reduce_pitched(&expert, Difficulty::Easy));
    tracks
}

fn reduce_pitched(expert: &[FiveLaneNote], difficulty: Difficulty) -> Vec<FiveLaneNote> {
    let min_gap = match difficulty {
        Difficulty::Hard => 72,
        Difficulty::Medium => 144,
        Difficulty::Easy => 288,
        Difficulty::Expert => return expert.to_vec(),
    };
    let mut result = Vec::new();
    let mut last_tick = i64::MIN / 2;
    for note in expert {
        if note.tick - last_tick < min_gap {
            continue;
        }
        let lane = match difficulty {
            Difficulty::Easy => match note.lane {
                0 | 1 => 1,
                2 => 2,
                _ => 3,
            },
            Difficulty::Medium => match note.lane {
                0 => 1,
                4 => 3,
                lane => lane,
            },
            _ => note.lane,
        };
        result.push(FiveLaneNote {
            tick: note.tick,
            lane,
            sustain: note.sustain,
        });
        last_tick = note.tick;
    }
    result
}

fn lane_for(kind: PercussionKind, velocity: f32) -> DrumLane {
    match kind {
        PercussionKind::Kick => DrumLane::Kick,
        PercussionKind::Snare => DrumLane::Red,
        PercussionKind::Tom => {
            if velocity > 0.7 {
                DrumLane::Green
            } else {
                DrumLane::Blue
            }
        }
        PercussionKind::Cymbal => {
            if velocity > 1.0 {
                DrumLane::Green
            } else {
                DrumLane::Yellow
            }
        }
        PercussionKind::Unknown => DrumLane::Red,
    }
}

fn enforce_playability(notes: &mut Vec<ChartNote>, bpm: f64) {
    let min_tick_gap = (f64::from(RESOLUTION) * bpm / 60.0 / 18.0).ceil() as i64;
    let mut last_tick = i64::MIN / 2;
    notes.retain(|note| {
        if note.tick != last_tick && note.tick - last_tick < min_tick_gap {
            return false;
        }
        if note.tick != last_tick {
            last_tick = note.tick;
        }
        true
    });
    let mut counts = BTreeMap::<i64, usize>::new();
    notes.retain(|note| {
        let count = counts.entry(note.tick).or_default();
        *count += 1;
        *count <= 2
    });
}

fn reduce(expert: &[ChartNote], difficulty: Difficulty, phase_tick: i64) -> Vec<ChartNote> {
    let (grid, min_gap, allow_kick) = match difficulty {
        Difficulty::Hard => (24, 72, true),
        Difficulty::Medium => (48, 96, true),
        Difficulty::Easy => (96, 192, false),
        Difficulty::Expert => return expert.to_vec(),
    };
    let mut result = Vec::new();
    let mut last_tick = i64::MIN / 2;
    for note in expert {
        if (note.tick - phase_tick).rem_euclid(grid) != 0
            || (note.tick != last_tick && note.tick - last_tick < min_gap)
            || (!allow_kick && note.lane == DrumLane::Kick)
        {
            continue;
        }
        let lane = match difficulty {
            Difficulty::Easy => DrumLane::Red,
            Difficulty::Medium if matches!(note.lane, DrumLane::Blue | DrumLane::Green) => {
                DrumLane::Yellow
            }
            _ => note.lane,
        };
        if !result
            .iter()
            .any(|existing: &ChartNote| existing.tick == note.tick && existing.lane == lane)
        {
            result.push(ChartNote {
                tick: note.tick,
                lane,
                sustain: 0,
            });
            if note.tick != last_tick {
                last_tick = note.tick;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastchart_music::{InstrumentTrack, MusicalEvent, TempoMap};

    #[test]
    fn difficulties_get_progressively_simpler() {
        let events = (0..32)
            .map(|i| MusicalEvent {
                start_seconds: i as f64 / 16.0,
                duration_seconds: 0.0,
                pitch: None,
                velocity: 0.8,
                confidence: 0.9,
                percussion: Some(PercussionKind::Snare),
            })
            .collect();
        let analysis = SongAnalysis {
            duration_seconds: 4.0,
            tempo_map: TempoMap::constant(120.0),
            beat_offset_seconds: 0.0,
            tracks: vec![InstrumentTrack {
                instrument: Instrument::Drums,
                confidence: 1.0,
                events,
            }],
        };
        let chart = generate_drum_chart(&analysis);
        assert!(chart.tracks[&Difficulty::Easy].len() < chart.tracks[&Difficulty::Medium].len());
        assert!(chart.tracks[&Difficulty::Medium].len() < chart.tracks[&Difficulty::Hard].len());
        assert!(chart.tracks[&Difficulty::Hard].len() < chart.tracks[&Difficulty::Expert].len());
    }

    #[test]
    fn beat_phase_does_not_shift_notes_out_of_sync() {
        let event = MusicalEvent {
            start_seconds: 0.5,
            duration_seconds: 0.0,
            pitch: None,
            velocity: 0.8,
            confidence: 0.9,
            percussion: Some(PercussionKind::Snare),
        };
        let analysis = SongAnalysis {
            duration_seconds: 1.0,
            tempo_map: TempoMap::constant(120.0),
            beat_offset_seconds: 0.125,
            tracks: vec![InstrumentTrack {
                instrument: Instrument::Drums,
                confidence: 1.0,
                events: vec![event],
            }],
        };
        let tick = generate_drum_chart(&analysis).tracks[&Difficulty::Expert][0].tick;
        assert!(tick >= 168, "note was shifted too early: {tick}");
    }

    #[test]
    fn maps_ascending_pitch_to_playable_guitar_lanes() {
        let events = (0..20)
            .map(|index| MusicalEvent {
                start_seconds: index as f64 * 0.25,
                duration_seconds: 0.2,
                pitch: Some(48 + index),
                velocity: 0.8,
                confidence: 0.9,
                percussion: None,
            })
            .collect();
        let analysis = SongAnalysis {
            duration_seconds: 5.0,
            tempo_map: TempoMap::constant(120.0),
            beat_offset_seconds: 0.0,
            tracks: vec![InstrumentTrack {
                instrument: Instrument::Guitar,
                confidence: 1.0,
                events,
            }],
        };
        let chart = generate_full_chart(&analysis);
        let expert = &chart.guitar_tracks[&Difficulty::Expert];
        assert_eq!(expert.len(), 20);
        assert!(expert.windows(2).all(|pair| pair[0].lane <= pair[1].lane));
        assert!(chart.guitar_tracks[&Difficulty::Easy].len() < expert.len());
        assert!(expert.iter().all(|note| note.lane <= 4));
    }
}
