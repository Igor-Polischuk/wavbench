// pub struct WavAnalizer {

// }

use crate::audio_buffer::AudioBuffer;

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |m, x| m.max(x.abs()))
}

pub fn rms(samples: &[f32]) -> f32 {
    let sum = samples.iter().map(|x| x * x).sum::<f32>();
    (sum / samples.len() as f32).sqrt()
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

pub fn correlation(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());

    let mut dot = 0.0f64;
    let mut energy_a = 0.0f64;
    let mut energy_b = 0.0f64;

    for (&a, &b) in a.iter().zip(b.iter()) {
        let a = a as f64;
        let b = b as f64;

        dot += a * b;
        energy_a += a * a;
        energy_b += b * b;
    }

    let denom = (energy_a * energy_b).sqrt();

    if denom == 0.0 {
        return 0.0;
    }

    (dot / denom) as f32
}

pub fn compare_rms(target_wav: &AudioBuffer, candidate_wav: &AudioBuffer) {
    let target = target_wav.to_mono_left();
    let candidate = candidate_wav.to_mono_left();

    let len = target.len().min(candidate.len());

    let target = &target[..len];
    let candidate = &candidate[..len];

    let target_rms = rms(target);
    let candidate_rms = rms(candidate);
    println!("{}", String::from("-").repeat(40));
    println!(
        "Target ({}) RMS:  {:.2} dBFS",
        target_wav.id,
        db(target_rms)
    );
    println!(
        "Candidate ({}) RMS: {:.2} dBFS",
        candidate_wav.id,
        db(candidate_rms)
    );
    println!("{}", String::from("-").repeat(40));

    let gain = target_rms / candidate_rms;
    println!("Candidate needs gain: {:.2} dB", db(gain));

    let candidate_matched: Vec<f32> = candidate.iter().map(|x| x * gain).collect();

    let diff: Vec<f32> = candidate_matched
        .iter()
        .zip(target.iter())
        .map(|(candidate, target)| candidate - target)
        .collect();

    println!("Null RMS: {:.2} dBFS", db(rms(&diff)));
    println!(
        "Candidate matched peak: {:.2} dBFS",
        db(peak(&candidate_matched))
    );

    let corr = correlation(&candidate_matched, target);
    println!("Correlation: {:.4}", corr); // 1 is equal, less than 0.7 is diff
}
