//! Conventional DSP analysis for tempo and percussion-like onsets.

use fastchart_music::{
    Instrument, InstrumentTrack, MusicalEvent, PercussionKind, SongAnalysis, TempoMap, TempoPoint,
};

#[derive(Clone, Copy, Debug)]
pub struct AnalysisConfig {
    pub frame_size: usize,
    pub hop_size: usize,
    pub min_bpm: f64,
    pub max_bpm: f64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            frame_size: 1024,
            hop_size: 512,
            min_bpm: 70.0,
            max_bpm: 190.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FrameFeatures {
    flux: f32,
    low_flux: f32,
    mid_flux: f32,
    high_flux: f32,
    spectral_flatness: f32,
    centroid_hz: f32,
}

pub fn analyze_mono(samples: &[f32], sample_rate: u32, config: AnalysisConfig) -> SongAnalysis {
    if samples.is_empty() || sample_rate == 0 {
        return SongAnalysis {
            duration_seconds: 0.0,
            tempo_map: TempoMap::constant(120.0),
            beat_offset_seconds: 0.0,
            tracks: vec![],
        };
    }
    let features = frame_features(samples, sample_rate, config);
    let novelty = novelty_curve(&features);
    let bpm = estimate_bpm(&novelty, sample_rate, config);
    let tempo_map = estimate_tempo_map(&novelty, bpm, sample_rate, config);
    let threshold = adaptive_threshold(&novelty);
    let mut events = Vec::new();
    for index in 1..novelty.len().saturating_sub(1) {
        if novelty[index] > novelty[index - 1]
            && novelty[index] >= novelty[index + 1]
            && novelty[index] > threshold[index]
        {
            let strength = (novelty[index] / (threshold[index] + 1e-6)).min(3.0) / 3.0;
            let confidence = (0.45 + strength * 0.5).min(0.98);
            for (percussion, band_confidence) in classify_transient(features[index]) {
                events.push(MusicalEvent {
                    start_seconds: index as f64 * config.hop_size as f64 / f64::from(sample_rate),
                    duration_seconds: 0.0,
                    pitch: None,
                    velocity: strength,
                    confidence: confidence * (0.82 + band_confidence * 0.18),
                    percussion: Some(percussion),
                });
            }
        }
    }
    suppress_near_duplicates(&mut events, 0.045);
    gate_drum_entry(&mut events);
    let beat_offset_seconds = strongest_beat_phase(&novelty, bpm, sample_rate, config);
    SongAnalysis {
        duration_seconds: samples.len() as f64 / f64::from(sample_rate),
        tempo_map,
        beat_offset_seconds,
        tracks: vec![InstrumentTrack {
            instrument: Instrument::Drums,
            confidence: if events.is_empty() { 0.0 } else { 0.65 },
            events,
        }],
    }
}

/// Detects pitched attacks for five-lane guitar or bass mapping. This intentionally
/// produces a playable monophonic abstraction rather than literal tablature.
pub fn analyze_pitched_mono(
    samples: &[f32],
    sample_rate: u32,
    instrument: Instrument,
    config: AnalysisConfig,
) -> InstrumentTrack {
    if samples.is_empty() || sample_rate == 0 {
        return InstrumentTrack {
            instrument,
            confidence: 0.0,
            events: Vec::new(),
        };
    }
    let (minimum_hz, maximum_hz) = match instrument {
        Instrument::Bass => (35.0, 420.0),
        Instrument::Guitar => (65.0, 1_300.0),
        _ => (55.0, 1_000.0),
    };
    let features = frame_features(samples, sample_rate, config);
    let novelty = novelty_curve(&features);
    let mut threshold = adaptive_threshold(&novelty);
    if instrument == Instrument::Guitar {
        for value in &mut threshold {
            *value *= 0.65;
        }
    }
    let mut events = Vec::new();
    let mut last_onset = -1.0_f64;
    for index in 1..novelty.len().saturating_sub(1) {
        if novelty[index] <= novelty[index - 1]
            || novelty[index] < novelty[index + 1]
            || novelty[index] <= threshold[index]
        {
            continue;
        }
        let start_seconds = index as f64 * config.hop_size as f64 / f64::from(sample_rate);
        if start_seconds - last_onset < 0.065 {
            continue;
        }
        let sample_index = index * config.hop_size;
        let Some((pitch, pitch_confidence)) =
            estimate_pitch(samples, sample_index, sample_rate, minimum_hz, maximum_hz)
        else {
            continue;
        };
        let strength = (novelty[index] / (threshold[index] + 1e-7)).min(3.0) / 3.0;
        events.push(MusicalEvent {
            start_seconds,
            duration_seconds: 0.0,
            pitch: Some(pitch),
            velocity: strength,
            confidence: (0.5 + 0.3 * strength + 0.2 * pitch_confidence).min(0.98),
            percussion: None,
        });
        last_onset = start_seconds;
    }
    let duration_seconds = samples.len() as f64 / f64::from(sample_rate);
    let maximum_events_per_second = match instrument {
        Instrument::Guitar => 4.5,
        Instrument::Bass => 4.25,
        _ => 4.5,
    };
    cap_event_density(
        &mut events,
        (duration_seconds * maximum_events_per_second).round() as usize,
    );
    for index in 0..events.len().saturating_sub(1) {
        let gap = events[index + 1].start_seconds - events[index].start_seconds;
        if gap >= 0.28 {
            events[index].duration_seconds = (gap * 0.72).min(2.0);
        }
    }
    InstrumentTrack {
        instrument,
        confidence: if events.is_empty() { 0.0 } else { 0.6 },
        events,
    }
}

fn cap_event_density(events: &mut Vec<MusicalEvent>, maximum_events: usize) {
    if events.len() <= maximum_events || maximum_events == 0 {
        return;
    }
    let mut ranked_indices: Vec<_> = (0..events.len()).collect();
    ranked_indices.sort_by(|left, right| {
        events[*right]
            .confidence
            .total_cmp(&events[*left].confidence)
            .then_with(|| events[*right].velocity.total_cmp(&events[*left].velocity))
    });
    let mut keep = vec![false; events.len()];
    for index in ranked_indices.into_iter().take(maximum_events) {
        keep[index] = true;
    }
    let mut index = 0;
    events.retain(|_| {
        let retain = keep[index];
        index += 1;
        retain
    });
}

fn estimate_pitch(
    samples: &[f32],
    onset_sample: usize,
    sample_rate: u32,
    minimum_hz: f32,
    maximum_hz: f32,
) -> Option<(u8, f32)> {
    let frame_size = 4096_usize;
    let analysis_start = onset_sample.saturating_add((sample_rate as f32 * 0.02) as usize);
    if analysis_start + frame_size > samples.len() {
        return None;
    }
    let mut spectrum = vec![Complex::default(); frame_size];
    for index in 0..frame_size {
        let window =
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (frame_size - 1) as f32).cos();
        spectrum[index].re = samples[analysis_start + index] * window;
    }
    fft_in_place(&mut spectrum);
    let bin_hz = sample_rate as f32 / frame_size as f32;
    let minimum_bin = (minimum_hz / bin_hz).ceil().max(1.0) as usize;
    let maximum_bin = (maximum_hz / bin_hz).floor() as usize;
    let magnitudes: Vec<_> = spectrum[..=frame_size / 2]
        .iter()
        .map(|value| value.magnitude())
        .collect();
    let mut scores = Vec::with_capacity(maximum_bin.saturating_sub(minimum_bin) + 1);
    for bin in minimum_bin..=maximum_bin.min(frame_size / 2) {
        let mut score = magnitudes[bin];
        for (harmonic, weight) in [(2_usize, 0.7_f32), (3, 0.45), (4, 0.3)] {
            if bin * harmonic < magnitudes.len() {
                score += magnitudes[bin * harmonic] * weight;
            }
        }
        scores.push((score, bin));
    }
    scores.sort_by(|left, right| right.0.total_cmp(&left.0));
    let (best_score, best_bin) = *scores.first()?;
    if best_score <= 1e-6 {
        return None;
    }
    let second_score = scores
        .iter()
        .find(|(_, bin)| bin.abs_diff(best_bin) > 1)
        .map_or(0.0, |(score, _)| *score);
    let frequency = best_bin as f32 * bin_hz;
    let midi = (69.0 + 12.0 * (frequency / 440.0).log2())
        .round()
        .clamp(0.0, 127.0) as u8;
    let confidence = (best_score / (best_score + second_score + 1e-9)).clamp(0.0, 1.0);
    Some((midi, confidence))
}

fn frame_features(samples: &[f32], sample_rate: u32, config: AnalysisConfig) -> Vec<FrameFeatures> {
    let frame_size = config.frame_size.next_power_of_two().max(64);
    if samples.len() < frame_size {
        return Vec::new();
    }
    let window: Vec<_> = (0..frame_size)
        .map(|index| {
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / (frame_size - 1) as f32).cos()
        })
        .collect();
    let bin_hz = sample_rate as f32 / frame_size as f32;
    let max_bin = ((16_000.0 / bin_hz) as usize).min(frame_size / 2);
    let mut previous = vec![0.0_f32; frame_size / 2 + 1];
    let mut previous_log_magnitude = vec![0.0_f32; frame_size / 2 + 1];
    let mut magnitudes = vec![0.0_f32; frame_size / 2 + 1];
    let mut spectrum = vec![Complex::default(); frame_size];
    let mut result = Vec::with_capacity((samples.len() - frame_size) / config.hop_size.max(1) + 1);
    for start in (0..=samples.len() - frame_size).step_by(config.hop_size.max(1)) {
        for index in 0..frame_size {
            spectrum[index] = Complex {
                re: samples[start + index] * window[index],
                im: 0.0,
            };
        }
        fft_in_place(&mut spectrum);
        let mut magnitude_sum = 0.0_f32;
        let mut weighted_frequency = 0.0_f32;
        let mut log_sum = 0.0_f32;
        let mut flatness_bins = 0_usize;
        for bin in 1..=frame_size / 2 {
            let magnitude = spectrum[bin].magnitude();
            magnitudes[bin] = magnitude;
            let frequency = bin as f32 * bin_hz;
            if bin <= max_bin && frequency >= 35.0 {
                magnitude_sum += magnitude;
                weighted_frequency += magnitude * frequency;
                log_sum += (magnitude + 1e-12).ln();
                flatness_bins += 1;
            }
        }
        let mut low_flux = 0.0_f32;
        let mut mid_flux = 0.0_f32;
        let mut high_flux = 0.0_f32;
        let mut onset_flux = 0.0_f32;
        for bin in 1..=max_bin {
            let normalized = magnitudes[bin] / (magnitude_sum + 1e-12);
            let flux = (normalized - previous[bin]).max(0.0);
            previous[bin] = normalized;
            let log_magnitude = magnitudes[bin].ln_1p();
            onset_flux += (log_magnitude - previous_log_magnitude[bin]).max(0.0);
            previous_log_magnitude[bin] = log_magnitude;
            let frequency = bin as f32 * bin_hz;
            if (35.0..180.0).contains(&frequency) {
                low_flux += flux;
            } else if (180.0..2_500.0).contains(&frequency) {
                mid_flux += flux;
            } else if (2_500.0..16_000.0).contains(&frequency) {
                high_flux += flux;
            }
        }
        let arithmetic_mean = magnitude_sum / flatness_bins.max(1) as f32;
        let geometric_mean = (log_sum / flatness_bins.max(1) as f32).exp();
        result.push(FrameFeatures {
            flux: onset_flux / max_bin.max(1) as f32,
            low_flux,
            mid_flux,
            high_flux,
            spectral_flatness: (geometric_mean / (arithmetic_mean + 1e-12)).clamp(0.0, 1.0),
            centroid_hz: weighted_frequency / (magnitude_sum + 1e-12),
        });
    }
    result
}

fn novelty_curve(features: &[FrameFeatures]) -> Vec<f32> {
    features.iter().map(|feature| feature.flux).collect()
}

fn adaptive_threshold(novelty: &[f32]) -> Vec<f32> {
    (0..novelty.len())
        .map(|index| {
            let start = index.saturating_sub(16);
            let end = (index + 17).min(novelty.len());
            let values = &novelty[start..end];
            let mean = values.iter().sum::<f32>() / values.len().max(1) as f32;
            let mut ordered = values.to_vec();
            ordered.sort_by(f32::total_cmp);
            let median = ordered[ordered.len() / 2];
            median * 1.5 + mean * 0.8 + 1e-7
        })
        .collect()
}

fn estimate_bpm(novelty: &[f32], sample_rate: u32, config: AnalysisConfig) -> f64 {
    if novelty.len() < 8 {
        return 120.0;
    }
    let frames_per_second = f64::from(sample_rate) / config.hop_size as f64;
    let min_lag = (frames_per_second * 60.0 / config.max_bpm) as usize;
    let max_lag = ((frames_per_second * 60.0 / config.min_bpm) as usize).min(novelty.len() / 2);
    let mean = novelty.iter().sum::<f32>() / novelty.len() as f32;
    let mut best = (0.0_f64, 120.0_f64);
    for lag in min_lag.max(1)..=max_lag.max(min_lag.max(1)) {
        let score: f64 = novelty[lag..]
            .iter()
            .zip(&novelty[..novelty.len() - lag])
            .map(|(a, b)| f64::from((*a - mean).max(0.0) * (*b - mean).max(0.0)))
            .sum();
        let bpm = frames_per_second * 60.0 / lag as f64;
        let weighted = score * (1.0 - ((bpm - 120.0).abs() / 500.0));
        if weighted > best.0 {
            best = (weighted, bpm);
        }
    }
    (best.1 * 10.0).round() / 10.0
}

fn estimate_tempo_map(
    novelty: &[f32],
    global_bpm: f64,
    sample_rate: u32,
    config: AnalysisConfig,
) -> TempoMap {
    let frames_per_second = f64::from(sample_rate) / config.hop_size as f64;
    let window = (frames_per_second * 20.0).round() as usize;
    let hop = (frames_per_second * 10.0).round() as usize;
    if novelty.len() < window.max(16) {
        return TempoMap::constant(global_bpm);
    }
    let mut candidates = Vec::new();
    for start in (0..novelty.len()).step_by(hop.max(1)) {
        let end = (start + window).min(novelty.len());
        if end - start < window / 2 {
            break;
        }
        let measured = estimate_bpm(&novelty[start..end], sample_rate, config);
        let bpm = octave_nearest(measured, global_bpm, config);
        candidates.push(TempoPoint {
            start_seconds: start as f64 / frames_per_second,
            bpm,
        });
    }
    let mut smoothed = Vec::with_capacity(candidates.len());
    for index in 0..candidates.len() {
        let start = index.saturating_sub(1);
        let end = (index + 2).min(candidates.len());
        let mut bpms: Vec<_> = candidates[start..end]
            .iter()
            .map(|point| point.bpm)
            .collect();
        bpms.sort_by(f64::total_cmp);
        smoothed.push(TempoPoint {
            start_seconds: candidates[index].start_seconds,
            bpm: bpms[bpms.len() / 2],
        });
    }
    let mut points: Vec<TempoPoint> = Vec::new();
    for point in smoothed {
        if let Some(previous) = points.last()
            && (point.bpm - previous.bpm).abs() < 1.0
        {
            continue;
        }
        points.push(point);
    }
    if points.is_empty() {
        TempoMap::constant(global_bpm)
    } else {
        points[0].start_seconds = 0.0;
        TempoMap { points }
    }
}

fn octave_nearest(measured: f64, reference: f64, config: AnalysisConfig) -> f64 {
    [measured / 2.0, measured, measured * 2.0]
        .into_iter()
        .filter(|bpm| *bpm >= config.min_bpm && *bpm <= config.max_bpm)
        .min_by(|left, right| {
            (left - reference)
                .abs()
                .total_cmp(&(right - reference).abs())
        })
        .unwrap_or(measured)
}

fn strongest_beat_phase(
    novelty: &[f32],
    bpm: f64,
    sample_rate: u32,
    config: AnalysisConfig,
) -> f64 {
    let period = (f64::from(sample_rate) * 60.0 / bpm / config.hop_size as f64)
        .round()
        .max(1.0) as usize;
    let mut best = (0.0_f32, 0_usize);
    for phase in 0..period.min(novelty.len()) {
        let score = (phase..novelty.len())
            .step_by(period)
            .map(|i| novelty[i])
            .sum();
        if score > best.0 {
            best = (score, phase);
        }
    }
    best.1 as f64 * config.hop_size as f64 / f64::from(sample_rate)
}

fn classify_transient(feature: FrameFeatures) -> Vec<(PercussionKind, f32)> {
    let total = feature.low_flux + feature.mid_flux + feature.high_flux;
    if total <= 1e-6 {
        return Vec::new();
    }
    let (low_share, mid_share, high_share) = (
        feature.low_flux / total,
        feature.mid_flux / total,
        feature.high_flux / total,
    );
    let kick_score = low_share
        * if feature.centroid_hz < 3_000.0 {
            1.35
        } else {
            0.9
        };
    let cymbal_score = high_share * (0.85 + feature.spectral_flatness);
    let snare_score =
        mid_share * (0.72 + 0.5 * feature.spectral_flatness) + high_share.min(0.35) * 0.18;
    let kick = (kick_score >= 0.12).then_some((PercussionKind::Kick, kick_score.min(1.0)));
    let mut upper = Vec::with_capacity(2);
    if cymbal_score >= 0.34 {
        upper.push((PercussionKind::Cymbal, cymbal_score.min(1.0)));
    }
    if snare_score >= 0.38 {
        upper.push((PercussionKind::Snare, snare_score.min(1.0)));
    }
    upper.sort_by(|left, right| right.1.total_cmp(&left.1));
    let mut classes = Vec::with_capacity(2);
    if let Some(kick) = kick {
        classes.push(kick);
        classes.extend(upper.into_iter().take(1));
    } else {
        classes.extend(upper.into_iter().take(2));
    }
    if classes.is_empty() {
        if high_share >= mid_share && high_share >= low_share {
            classes.push((PercussionKind::Cymbal, high_share));
        } else if mid_share >= low_share {
            classes.push((PercussionKind::Snare, snare_score.min(1.0)));
        } else {
            classes.push((PercussionKind::Kick, kick_score.min(1.0)));
        }
    }
    classes
}

#[derive(Clone, Copy, Debug, Default)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    fn magnitude(self) -> f32 {
        (self.re * self.re + self.im * self.im).sqrt()
    }

    fn multiply(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }
}

fn fft_in_place(values: &mut [Complex]) {
    debug_assert!(values.len().is_power_of_two());
    let length = values.len();
    let mut reversed = 0_usize;
    for index in 1..length {
        let mut bit = length >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;
        if index < reversed {
            values.swap(index, reversed);
        }
    }
    let mut block = 2;
    while block <= length {
        let angle = -2.0 * std::f32::consts::PI / block as f32;
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..length).step_by(block) {
            let mut twiddle = Complex { re: 1.0, im: 0.0 };
            for offset in 0..block / 2 {
                let even = values[start + offset];
                let odd = values[start + offset + block / 2].multiply(twiddle);
                values[start + offset] = Complex {
                    re: even.re + odd.re,
                    im: even.im + odd.im,
                };
                values[start + offset + block / 2] = Complex {
                    re: even.re - odd.re,
                    im: even.im - odd.im,
                };
                twiddle = twiddle.multiply(root);
            }
        }
        block *= 2;
    }
}

fn suppress_near_duplicates(events: &mut Vec<MusicalEvent>, interval: f64) {
    let mut kept: Vec<MusicalEvent> = Vec::with_capacity(events.len());
    for event in events.drain(..) {
        if let Some(index) = kept.iter().rposition(|last| {
            last.percussion == event.percussion
                && event.start_seconds - last.start_seconds < interval
        }) {
            if event.velocity > kept[index].velocity {
                kept[index] = event;
            }
        } else {
            kept.push(event);
        }
    }
    *events = kept;
}

fn gate_drum_entry(events: &mut Vec<MusicalEvent>) {
    let kicks: Vec<_> = events
        .iter()
        .filter(|event| event.percussion == Some(PercussionKind::Kick))
        .map(|event| event.start_seconds)
        .collect();
    let entry = kicks
        .windows(3)
        .find(|window| window[2] - window[0] <= 4.0)
        .map(|window| (window[0] - 0.5).max(0.0));
    if let Some(entry) = entry.filter(|entry| *entry >= 1.0) {
        events.retain(|event| event.start_seconds >= entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_track(bpm: f64, seconds: usize, rate: u32) -> Vec<f32> {
        let mut samples = vec![0.0; seconds * rate as usize];
        let period = (60.0 / bpm * f64::from(rate)) as usize;
        for start in (0..samples.len()).step_by(period) {
            for offset in 0..64.min(samples.len() - start) {
                samples[start + offset] = 1.0 - offset as f32 / 64.0;
            }
        }
        samples
    }

    #[test]
    fn estimates_click_track_tempo_and_events() {
        let analysis = analyze_mono(
            &click_track(120.0, 12, 44_100),
            44_100,
            AnalysisConfig::default(),
        );
        assert!(
            (analysis.tempo_map.bpm() - 120.0).abs() < 3.0,
            "{}",
            analysis.tempo_map.bpm()
        );
        assert!(analysis.tracks[0].events.len() >= 18);
    }

    #[test]
    fn recurring_kicks_gate_false_intro_events() {
        let mut events = [
            (1.0, PercussionKind::Cymbal),
            (10.0, PercussionKind::Kick),
            (11.0, PercussionKind::Kick),
            (12.0, PercussionKind::Kick),
        ]
        .into_iter()
        .map(|(start_seconds, percussion)| MusicalEvent {
            start_seconds,
            duration_seconds: 0.0,
            pitch: None,
            velocity: 1.0,
            confidence: 1.0,
            percussion: Some(percussion),
        })
        .collect();
        gate_drum_entry(&mut events);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].start_seconds, 10.0);
    }

    #[test]
    fn detects_pitched_bass_bursts() {
        let sample_rate = 44_100_u32;
        let mut samples = vec![0.0_f32; sample_rate as usize * 4];
        for second in 1..=3 {
            let start = second * sample_rate as usize;
            for offset in 0..(sample_rate as usize / 3) {
                samples[start + offset] =
                    (2.0 * std::f32::consts::PI * 110.0 * offset as f32 / sample_rate as f32).sin()
                        * 0.8;
            }
        }
        let track = analyze_pitched_mono(
            &samples,
            sample_rate,
            Instrument::Bass,
            AnalysisConfig::default(),
        );
        assert!(track.events.len() >= 2);
        assert!(track.events.iter().any(|event| {
            event
                .pitch
                .is_some_and(|pitch| i16::from(pitch).abs_diff(45) <= 2)
        }));
    }

    #[test]
    fn pitched_density_cap_keeps_strongest_events_in_time_order() {
        let mut events: Vec<_> = (0..10)
            .map(|index| MusicalEvent {
                start_seconds: index as f64,
                duration_seconds: 0.0,
                pitch: Some(60),
                velocity: index as f32 / 10.0,
                confidence: index as f32 / 10.0,
                percussion: None,
            })
            .collect();
        cap_event_density(&mut events, 3);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].start_seconds, 7.0);
        assert_eq!(events[2].start_seconds, 9.0);
    }
}
