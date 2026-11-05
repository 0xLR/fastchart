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
    let event_ticks: Vec<_> = source
        .iter()
        .map(|event| {
            let event_tick = analysis
                .tempo_map
                .seconds_to_ticks(event.start_seconds, RESOLUTION);
            (phase_tick + quantize_pitched_tick(event_tick - phase_tick)).max(0)
        })
        .collect();
    let event_pitches: Vec<_> = source.iter().filter_map(|event| event.pitch).collect();
    let mut optimized_lanes = optimize_pitch_lanes(
        &event_pitches,
        &event_ticks,
        low_pitch,
        high_pitch,
        instrument,
    );
    stabilize_repeated_riff_lanes(
        &event_pitches,
        &event_ticks,
        &mut optimized_lanes,
        instrument,
    );
    let mut expert: Vec<FiveLaneNote> = Vec::new();
    let chord_events = if instrument == Instrument::Guitar {
        let mut ranked: Vec<_> = (0..source.len()).collect();
        ranked.sort_by(|left, right| {
            source[*right]
                .velocity
                .total_cmp(&source[*left].velocity)
                .then_with(|| {
                    source[*right]
                        .confidence
                        .total_cmp(&source[*left].confidence)
                })
        });
        ranked.truncate(source.len().div_ceil(4));
        ranked
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    } else {
        std::collections::BTreeSet::new()
    };
    for (event_index, event) in source.into_iter().enumerate() {
        let lane = optimized_lanes[event_index];
        let event_tick = analysis
            .tempo_map
            .seconds_to_ticks(event.start_seconds, RESOLUTION);
        let tick = event_ticks[event_index];
        let end_tick = analysis
            .tempo_map
            .seconds_to_ticks(event.start_seconds + event.duration_seconds, RESOLUTION);
        let raw_sustain = (end_tick - event_tick).max(0);
        let sustain = if raw_sustain >= i64::from(RESOLUTION / 3) {
            quantize_tick(raw_sustain, RESOLUTION, 8).max(i64::from(RESOLUTION / 4))
        } else {
            0
        };
        if expert.last().is_some_and(|note| note.tick == tick) {
            for existing in expert.iter_mut().rev().take_while(|note| note.tick == tick) {
                existing.sustain = existing.sustain.max(sustain);
            }
        } else {
            expert.push(FiveLaneNote {
                tick,
                lane,
                sustain,
            });
            if chord_events.contains(&event_index) {
                expert.push(FiveLaneNote {
                    tick,
                    lane: if lane < 4 { lane + 1 } else { lane - 1 },
                    sustain,
                });
            }
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

fn optimize_pitch_lanes(
    pitches: &[u8],
    ticks: &[i64],
    low_pitch: u8,
    high_pitch: u8,
    instrument: Instrument,
) -> Vec<u8> {
    if pitches.is_empty() {
        return Vec::new();
    }
    debug_assert_eq!(pitches.len(), ticks.len());
    // Guitar's greedy path is already a useful playability constraint. Optimize
    // around it instead of letting a single noisy pitch redraw an entire phrase.
    let targets = if instrument == Instrument::Guitar {
        greedy_pitch_lane_targets(pitches, ticks, low_pitch, high_pitch)
    } else {
        pitches
            .iter()
            .map(|pitch| pitch_target_lane(*pitch, low_pitch, high_pitch))
            .collect()
    };
    let mut costs = [0.0_f32; 5];
    let mut previous_lanes = vec![[0_u8; 5]; pitches.len()];
    for lane in 0..5_u8 {
        costs[usize::from(lane)] = lane_emission_cost(lane, targets[0], instrument);
    }
    for index in 1..pitches.len() {
        let mut next_costs = [f32::INFINITY; 5];
        for lane in 0..5_u8 {
            for previous_lane in 0..5_u8 {
                let cost = costs[usize::from(previous_lane)]
                    + lane_emission_cost(lane, targets[index], instrument)
                    + lane_transition_cost(
                        previous_lane,
                        lane,
                        pitches[index - 1],
                        pitches[index],
                        ticks[index - 1],
                        ticks[index],
                        low_pitch,
                        high_pitch,
                        instrument,
                    );
                if cost < next_costs[usize::from(lane)] {
                    next_costs[usize::from(lane)] = cost;
                    previous_lanes[index][usize::from(lane)] = previous_lane;
                }
            }
        }
        costs = next_costs;
    }
    let mut lane = (0..5_u8)
        .min_by(|left, right| costs[usize::from(*left)].total_cmp(&costs[usize::from(*right)]))
        .unwrap_or(2);
    let mut result = vec![0_u8; pitches.len()];
    for index in (0..pitches.len()).rev() {
        result[index] = lane;
        lane = previous_lanes[index][usize::from(lane)];
    }
    result
}

const RIFF_ONSETS: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct RiffSignature {
    starting_pitch: Option<u8>,
    pitch_intervals: Vec<i16>,
    tick_intervals: Vec<i64>,
}

fn stabilize_repeated_riff_lanes(
    pitches: &[u8],
    ticks: &[i64],
    lanes: &mut [u8],
    instrument: Instrument,
) -> usize {
    debug_assert_eq!(pitches.len(), ticks.len());
    debug_assert_eq!(pitches.len(), lanes.len());
    let onset_indices: Vec<_> = (0..ticks.len())
        .filter(|index| *index == 0 || ticks[*index] != ticks[*index - 1])
        .collect();
    if onset_indices.len() < RIFF_ONSETS * 2 {
        return 0;
    }
    let mut first_occurrences: BTreeMap<RiffSignature, usize> = BTreeMap::new();
    let mut changed = 0;
    for start in 0..=onset_indices.len() - RIFF_ONSETS {
        let window = &onset_indices[start..start + RIFF_ONSETS];
        let raw_pitch_intervals: Vec<_> = window
            .windows(2)
            .map(|pair| i16::from(pitches[pair[1]]) - i16::from(pitches[pair[0]]))
            .collect();
        let pitch_intervals = if instrument == Instrument::Guitar {
            raw_pitch_intervals
        } else {
            raw_pitch_intervals
                .into_iter()
                .map(pitch_interval_class)
                .collect()
        };
        let tick_intervals: Vec<_> = window
            .windows(2)
            .map(|pair| ticks[pair[1]] - ticks[pair[0]])
            .collect();
        if tick_intervals
            .iter()
            .any(|gap| *gap <= 0 || *gap > i64::from(RESOLUTION) * 2)
            || !informative_riff(&pitch_intervals)
        {
            continue;
        }
        let starting_pitch = (instrument == Instrument::Guitar).then_some(pitches[window[0]]);
        let signature = RiffSignature {
            starting_pitch,
            pitch_intervals,
            tick_intervals,
        };
        let Some(&canonical_start) = first_occurrences.get(&signature) else {
            first_occurrences.insert(signature, start);
            continue;
        };
        if start < canonical_start + RIFF_ONSETS {
            continue;
        }
        let canonical = &onset_indices[canonical_start..canonical_start + RIFF_ONSETS];
        let candidate: Vec<_> = canonical.iter().map(|index| lanes[*index]).collect();
        let deviation: usize = window
            .iter()
            .zip(&candidate)
            .map(|(index, lane)| usize::from(lanes[*index].abs_diff(*lane)))
            .sum();
        if deviation > RIFF_ONSETS
            || !riff_boundaries_are_playable(&onset_indices, start, &candidate, ticks, lanes)
        {
            continue;
        }
        for (index, lane) in window.iter().zip(candidate) {
            if lanes[*index] != lane {
                lanes[*index] = lane;
                changed += 1;
            }
        }
    }
    changed
}

fn pitch_interval_class(interval: i16) -> i16 {
    match interval {
        ..=-6 => -3,
        -5..=-3 => -2,
        -2..=-1 => -1,
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        6.. => 3,
    }
}

fn informative_riff(pitch_intervals: &[i16]) -> bool {
    let moving = pitch_intervals
        .iter()
        .filter(|interval| **interval != 0)
        .count();
    let mut distinct = pitch_intervals.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    moving >= 3 && distinct.len() >= 2
}

fn riff_boundaries_are_playable(
    onset_indices: &[usize],
    start: usize,
    candidate: &[u8],
    ticks: &[i64],
    lanes: &[u8],
) -> bool {
    let first = onset_indices[start];
    if start > 0 {
        let previous = onset_indices[start - 1];
        if ticks[first] - ticks[previous] <= i64::from(RESOLUTION) * 2
            && lanes[previous].abs_diff(candidate[0]) > 2
        {
            return false;
        }
    }
    let after = start + RIFF_ONSETS;
    if after < onset_indices.len() {
        let last = onset_indices[after - 1];
        let next = onset_indices[after];
        if ticks[next] - ticks[last] <= i64::from(RESOLUTION) * 2
            && candidate[RIFF_ONSETS - 1].abs_diff(lanes[next]) > 2
        {
            return false;
        }
    }
    true
}

fn greedy_pitch_lane_targets(
    pitches: &[u8],
    ticks: &[i64],
    low_pitch: u8,
    high_pitch: u8,
) -> Vec<u8> {
    let mut lanes = Vec::with_capacity(pitches.len());
    let mut previous_lane = 2_u8;
    let mut previous_pitch = None;
    let mut previous_tick = None;
    for (index, pitch) in pitches.iter().copied().enumerate() {
        let target = pitch_target_lane(pitch, low_pitch, high_pitch);
        let lane = if previous_pitch == Some(pitch) {
            previous_lane
        } else {
            target.clamp(previous_lane.saturating_sub(2), (previous_lane + 2).min(4))
        };
        lanes.push(lane);
        if previous_tick != Some(ticks[index]) {
            previous_lane = lane;
            previous_pitch = Some(pitch);
            previous_tick = Some(ticks[index]);
        }
    }
    lanes
}

fn pitch_target_lane(pitch: u8, low_pitch: u8, high_pitch: u8) -> u8 {
    if high_pitch <= low_pitch {
        return 2;
    }
    ((f32::from(pitch.saturating_sub(low_pitch)) / f32::from(high_pitch - low_pitch) * 4.0).round()
        as i16)
        .clamp(0, 4) as u8
}

fn lane_emission_cost(lane: u8, target: u8, instrument: Instrument) -> f32 {
    let weight = if instrument == Instrument::Guitar {
        1.8
    } else {
        0.9
    };
    f32::from(lane.abs_diff(target)) * weight
}

#[allow(clippy::too_many_arguments)]
fn lane_transition_cost(
    previous_lane: u8,
    lane: u8,
    previous_pitch: u8,
    pitch: u8,
    previous_tick: i64,
    tick: i64,
    low_pitch: u8,
    high_pitch: u8,
    instrument: Instrument,
) -> f32 {
    if tick - previous_tick > i64::from(RESOLUTION) * 2 {
        return 0.0;
    }
    let lane_change = i16::from(lane) - i16::from(previous_lane);
    let pitch_change = i16::from(pitch) - i16::from(previous_pitch);
    let lane_distance = lane.abs_diff(previous_lane);
    if pitch_change == 0 {
        return f32::from(lane_distance) * 3.5;
    }
    let mut cost = 0.0;
    if lane_change == 0 {
        cost += if instrument == Instrument::Bass {
            0.15
        } else {
            0.3
        };
    } else if lane_change.signum() != pitch_change.signum() {
        cost += if instrument == Instrument::Guitar {
            1.5 + f32::from(lane_distance) * 0.5
        } else {
            3.0 + f32::from(lane_distance)
        };
    }
    if lane_distance > 2 {
        cost += f32::from(lane_distance - 2)
            * if instrument == Instrument::Bass {
                3.5
            } else {
                2.5
            };
    }
    let pitch_span = i16::from(high_pitch.saturating_sub(low_pitch)).max(4);
    let expected_distance =
        ((pitch_change.unsigned_abs() as f32 * 4.0 / pitch_span as f32).round() as u8).clamp(1, 2);
    let contour_weight = if instrument == Instrument::Guitar {
        0.2
    } else {
        0.35
    };
    cost + f32::from(lane_distance.abs_diff(expected_distance)) * contour_weight
}

fn quantize_pitched_tick(tick: i64) -> i64 {
    [8, 12]
        .into_iter()
        .map(|subdivision| quantize_tick(tick, RESOLUTION, subdivision))
        .min_by_key(|candidate| (candidate - tick).abs())
        .unwrap_or(tick)
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
            Difficulty::Easy => note.lane.min(2),
            Difficulty::Medium => note.lane.min(3),
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
        assert_eq!(
            expert
                .iter()
                .map(|note| note.tick)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            20
        );
        assert!(chart.guitar_tracks[&Difficulty::Easy].len() < expert.len());
        assert!(expert.iter().all(|note| note.lane <= 4));
    }

    #[test]
    fn pitched_notes_snap_to_straight_and_triplet_grids() {
        assert_eq!(quantize_pitched_tick(23), 24);
        assert_eq!(quantize_pitched_tick(33), 32);
    }

    #[test]
    fn strong_guitar_accents_become_expert_chords() {
        let analysis = SongAnalysis {
            duration_seconds: 1.0,
            tempo_map: TempoMap::constant(120.0),
            beat_offset_seconds: 0.0,
            tracks: vec![InstrumentTrack {
                instrument: Instrument::Guitar,
                confidence: 1.0,
                events: vec![MusicalEvent {
                    start_seconds: 0.5,
                    duration_seconds: 0.0,
                    pitch: Some(60),
                    velocity: 0.9,
                    confidence: 0.9,
                    percussion: None,
                }],
            }],
        };
        let chart = generate_full_chart(&analysis);
        let expert = &chart.guitar_tracks[&Difficulty::Expert];
        assert_eq!(expert.len(), 2);
        assert_eq!(expert[0].tick, expert[1].tick);
        assert_ne!(expert[0].lane, expert[1].lane);
    }

    #[test]
    fn guitar_chords_are_limited_to_strongest_quarter_of_onsets() {
        let events = (0..8)
            .map(|index| MusicalEvent {
                start_seconds: index as f64 * 0.5,
                duration_seconds: 0.0,
                pitch: Some(52 + index),
                velocity: 0.2 + index as f32 * 0.1,
                confidence: 0.9,
                percussion: None,
            })
            .collect();
        let analysis = SongAnalysis {
            duration_seconds: 4.0,
            tempo_map: TempoMap::constant(120.0),
            beat_offset_seconds: 0.0,
            tracks: vec![InstrumentTrack {
                instrument: Instrument::Guitar,
                confidence: 1.0,
                events,
            }],
        };
        let expert = &generate_full_chart(&analysis).guitar_tracks[&Difficulty::Expert];
        assert_eq!(expert.len(), 10);
        let chord_onsets = expert
            .windows(2)
            .filter(|notes| notes[0].tick == notes[1].tick)
            .count();
        assert_eq!(chord_onsets, 2);
    }

    #[test]
    fn pitch_lane_optimizer_preserves_repetition_and_melodic_direction() {
        let pitches = [60, 60, 62, 64, 65, 67];
        let ticks = [0, 96, 192, 288, 384, 480];
        let lanes = optimize_pitch_lanes(&pitches, &ticks, 60, 67, Instrument::Guitar);

        assert_eq!(lanes[0], lanes[1]);
        assert!(lanes.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(lanes.windows(2).all(|pair| pair[0].abs_diff(pair[1]) <= 2));
        assert!(lanes.last() > lanes.first());
    }

    #[test]
    fn pitch_lane_optimizer_suppresses_an_isolated_octave_jump() {
        let pitches = [60, 61, 73, 62, 63];
        let ticks = [0, 96, 192, 288, 384];
        let lanes = optimize_pitch_lanes(&pitches, &ticks, 60, 73, Instrument::Guitar);

        assert!(lanes[1].abs_diff(lanes[2]) <= 2);
        assert!(lanes[2].abs_diff(lanes[3]) <= 2);
    }

    #[test]
    fn repeated_riffs_reuse_the_first_lane_pattern() {
        let pitches = [60, 62, 64, 62, 65, 67, 72, 74, 76, 74, 77, 79];
        let ticks = [0, 96, 192, 288, 384, 480, 960, 1056, 1152, 1248, 1344, 1440];
        let mut lanes = [0, 1, 2, 1, 2, 3, 1, 2, 3, 2, 3, 4];

        let changed = stabilize_repeated_riff_lanes(&pitches, &ticks, &mut lanes, Instrument::Bass);

        assert_eq!(changed, 6);
        assert_eq!(&lanes[..6], &lanes[6..]);
    }

    #[test]
    fn repeated_single_notes_are_not_mistaken_for_riffs() {
        let pitches = [60; 12];
        let ticks: Vec<_> = (0..12).map(|index| index * 96).collect();
        let mut lanes = [1, 1, 1, 1, 1, 1, 3, 3, 3, 3, 3, 3];

        let changed =
            stabilize_repeated_riff_lanes(&pitches, &ticks, &mut lanes, Instrument::Guitar);

        assert_eq!(changed, 0);
        assert_eq!(&lanes[6..], &[3; 6]);
    }

    #[test]
    fn guitar_riffs_require_the_same_pitch_register() {
        let pitches = [60, 62, 64, 62, 65, 67, 72, 74, 76, 74, 77, 79];
        let ticks = [0, 96, 192, 288, 384, 480, 960, 1056, 1152, 1248, 1344, 1440];
        let mut lanes = [0, 1, 2, 1, 2, 3, 1, 2, 3, 2, 3, 4];

        let changed =
            stabilize_repeated_riff_lanes(&pitches, &ticks, &mut lanes, Instrument::Guitar);

        assert_eq!(changed, 0);
    }

    #[test]
    fn lower_pitched_difficulties_use_progressive_lane_counts() {
        let expert: Vec<_> = (0..5)
            .map(|lane| FiveLaneNote {
                tick: i64::from(lane) * 300,
                lane,
                sustain: 0,
            })
            .collect();

        let easy: Vec<_> = reduce_pitched(&expert, Difficulty::Easy)
            .into_iter()
            .map(|note| note.lane)
            .collect();
        let medium: Vec<_> = reduce_pitched(&expert, Difficulty::Medium)
            .into_iter()
            .map(|note| note.lane)
            .collect();
        let hard: Vec<_> = reduce_pitched(&expert, Difficulty::Hard)
            .into_iter()
            .map(|note| note.lane)
            .collect();

        assert_eq!(easy, vec![0, 1, 2, 2, 2]);
        assert_eq!(medium, vec![0, 1, 2, 3, 3]);
        assert_eq!(hard, vec![0, 1, 2, 3, 4]);
    }
}
