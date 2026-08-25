//! Instrument-independent timing and musical event types.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instrument {
    Guitar,
    Bass,
    Drums,
    Keys,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PercussionKind {
    Kick,
    Snare,
    Tom,
    Cymbal,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MusicalEvent {
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub pitch: Option<u8>,
    pub velocity: f32,
    pub confidence: f32,
    pub percussion: Option<PercussionKind>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentTrack {
    pub instrument: Instrument,
    pub confidence: f32,
    pub events: Vec<MusicalEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoPoint {
    pub start_seconds: f64,
    pub bpm: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    pub points: Vec<TempoPoint>,
}

impl TempoMap {
    pub fn constant(bpm: f64) -> Self {
        Self {
            points: vec![TempoPoint {
                start_seconds: 0.0,
                bpm,
            }],
        }
    }

    pub fn bpm(&self) -> f64 {
        self.points.first().map_or(120.0, |point| point.bpm)
    }

    pub fn bpm_at(&self, seconds: f64) -> f64 {
        self.points
            .iter()
            .rfind(|point| point.start_seconds <= seconds)
            .map_or_else(|| self.bpm(), |point| point.bpm)
    }

    pub fn seconds_to_ticks(&self, seconds: f64, resolution: u32) -> i64 {
        let target = seconds.max(0.0);
        let mut points = self.points.clone();
        points.sort_by(|left, right| left.start_seconds.total_cmp(&right.start_seconds));
        let mut current_seconds = 0.0;
        let mut current_bpm = points.first().map_or(120.0, |point| point.bpm);
        let mut beats = 0.0;
        for point in points.into_iter().skip(1) {
            if point.start_seconds > target {
                break;
            }
            beats += (point.start_seconds - current_seconds).max(0.0) * current_bpm / 60.0;
            current_seconds = point.start_seconds;
            current_bpm = point.bpm;
        }
        beats += (target - current_seconds).max(0.0) * current_bpm / 60.0;
        (beats * f64::from(resolution)).round() as i64
    }

    pub fn ticks_to_seconds(&self, ticks: i64, resolution: u32) -> f64 {
        let target_beats = ticks.max(0) as f64 / f64::from(resolution);
        let mut points = self.points.clone();
        points.sort_by(|left, right| left.start_seconds.total_cmp(&right.start_seconds));
        let mut current_seconds = 0.0;
        let mut current_bpm = points.first().map_or(120.0, |point| point.bpm);
        let mut elapsed_beats = 0.0;
        for point in points.into_iter().skip(1) {
            let segment_beats =
                (point.start_seconds - current_seconds).max(0.0) * current_bpm / 60.0;
            if elapsed_beats + segment_beats >= target_beats {
                return current_seconds + (target_beats - elapsed_beats) * 60.0 / current_bpm;
            }
            elapsed_beats += segment_beats;
            current_seconds = point.start_seconds;
            current_bpm = point.bpm;
        }
        current_seconds + (target_beats - elapsed_beats).max(0.0) * 60.0 / current_bpm
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SongAnalysis {
    pub duration_seconds: f64,
    pub tempo_map: TempoMap,
    pub beat_offset_seconds: f64,
    pub tracks: Vec<InstrumentTrack>,
}

pub fn seconds_to_ticks(seconds: f64, bpm: f64, resolution: u32) -> i64 {
    (seconds.max(0.0) * bpm / 60.0 * f64::from(resolution)).round() as i64
}

pub fn ticks_to_seconds(ticks: i64, bpm: f64, resolution: u32) -> f64 {
    ticks as f64 * 60.0 / (bpm * f64::from(resolution))
}

pub fn quantize_tick(tick: i64, resolution: u32, subdivision: u32) -> i64 {
    let step = (resolution / subdivision.max(1)).max(1) as i64;
    ((tick as f64 / step as f64).round() as i64) * step
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_round_trips() {
        let tick = seconds_to_ticks(1.5, 120.0, 192);
        assert_eq!(tick, 576);
        assert!((ticks_to_seconds(tick, 120.0, 192) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn quantizes_to_sixteenth_notes() {
        assert_eq!(quantize_tick(50, 192, 4), 48);
        assert_eq!(quantize_tick(75, 192, 4), 96);
    }

    #[test]
    fn variable_tempo_map_converts_both_directions() {
        let map = TempoMap {
            points: vec![
                TempoPoint {
                    start_seconds: 0.0,
                    bpm: 120.0,
                },
                TempoPoint {
                    start_seconds: 2.0,
                    bpm: 60.0,
                },
            ],
        };
        let tick = map.seconds_to_ticks(4.0, 192);
        assert_eq!(tick, 1_152);
        assert!((map.ticks_to_seconds(tick, 192) - 4.0).abs() < 1e-9);
    }
}
