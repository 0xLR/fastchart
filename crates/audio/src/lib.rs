//! Small, dependency-free PCM preprocessing building blocks usable from native Rust and WASM.

#[derive(Clone, Debug, PartialEq)]
pub struct PcmAudio {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved, normalized floating-point samples.
    pub samples: Vec<f32>,
}

impl PcmAudio {
    pub fn duration_seconds(&self) -> f64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.channels) / f64::from(self.sample_rate)
    }

    pub fn to_mono(&self) -> Vec<f32> {
        let channels = usize::from(self.channels);
        if channels <= 1 {
            return self.samples.clone();
        }
        self.samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect()
    }
}

pub fn normalize(samples: &mut [f32]) {
    let peak = samples
        .iter()
        .fold(0.0_f32, |peak, value| peak.max(value.abs()));
    if peak > 1e-6 {
        let gain = 0.98 / peak;
        for sample in samples {
            *sample *= gain;
        }
    }
}

/// Peak-normalizes a primary signal and applies the same gain to a companion
/// signal. This preserves their relative level, which is important when the
/// companion represents stereo-side energy used to reject centered vocals.
pub fn normalize_with_companion(samples: &mut [f32], companion: &mut [f32]) {
    let peak = samples
        .iter()
        .fold(0.0_f32, |peak, value| peak.max(value.abs()));
    if peak > 1e-6 {
        let gain = 0.98 / peak;
        for sample in samples {
            *sample *= gain;
        }
        for sample in companion {
            *sample *= gain;
        }
    }
}

pub fn resample_linear(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if input.is_empty() || source_rate == 0 || target_rate == 0 || source_rate == target_rate {
        return input.to_vec();
    }
    let output_len =
        ((input.len() as u64 * u64::from(target_rate)) / u64::from(source_rate)) as usize;
    let ratio = f64::from(source_rate) / f64::from(target_rate);
    (0..output_len)
        .map(|index| {
            let position = index as f64 * ratio;
            let left = position.floor() as usize;
            let right = (left + 1).min(input.len() - 1);
            let fraction = (position - left as f64) as f32;
            input[left] + (input[right] - input[left]) * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_stereo_to_mono() {
        let audio = PcmAudio {
            sample_rate: 2,
            channels: 2,
            samples: vec![1.0, -1.0, 0.5, 0.5],
        };
        assert_eq!(audio.to_mono(), vec![0.0, 0.5]);
        assert_eq!(audio.duration_seconds(), 1.0);
    }

    #[test]
    fn resamples_linearly() {
        assert_eq!(resample_linear(&[0.0, 1.0], 2, 4), vec![0.0, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn companion_normalization_preserves_relative_level() {
        let mut primary = [0.5, -0.25];
        let mut companion = [0.05, -0.025];
        normalize_with_companion(&mut primary, &mut companion);
        assert!((primary[0] - 0.98).abs() < 1e-6);
        assert!((companion[0] - 0.098).abs() < 1e-6);
    }
}
