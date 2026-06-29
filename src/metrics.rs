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

pub fn null_test(target: &[f32], candidate_matched: &[f32]) -> f32 {
    let diff: Vec<f32> = candidate_matched
        .iter()
        .zip(target.iter())
        .map(|(candidate, target)| candidate - target)
        .collect();

    rms(&diff)
}
