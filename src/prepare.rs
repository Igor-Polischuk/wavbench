use std::{
    error::Error,
    io::{self, ErrorKind},
};

use crate::{
    audio_buffer::AudioBuffer,
    metrics::{db, rms},
};

pub struct PreparedComparison {
    pub target: Vec<f32>,
    pub candidate_matched: Vec<f32>,
    pub gain_db: f32,
    pub candidate_rms: f32,
    pub target_rms: f32,
}

impl PreparedComparison {
    pub fn prepare(
        target_wav: &AudioBuffer,
        candidate_wav: &AudioBuffer,
    ) -> Result<Self, Box<dyn Error>> {
        let target = target_wav.to_mono_left();
        let candidate = candidate_wav.to_mono_left();

        // let offset = find_best_offset(&target, &candidate, 4096);
        // println!("Best offset: {} samples", offset);
        // let (target, candidate) = slices_with_offset(&target, &candidate, offset);

        let target = target.to_vec();
        let candidate = candidate.to_vec();

        let len = target.len().min(candidate.len());
        if len == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "WAV files do not contain comparable samples",
            )
            .into());
        }

        let target = target[..len].to_vec();
        let candidate = candidate[..len].to_vec();

        let target_rms = rms(&target);
        let candidate_rms = rms(&candidate);
        if !candidate_rms.is_finite() || candidate_rms <= 0.0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "Candidate WAV RMS is zero; cannot apply gain matching",
            )
            .into());
        }

        let gain = target_rms / candidate_rms;
        let gain_db = db(gain);

        let candidate_matched = candidate.iter().map(|x| x * gain).collect();

        Ok(PreparedComparison {
            target,
            candidate_matched,
            gain_db,
            candidate_rms,
            target_rms,
        })
    }
}
