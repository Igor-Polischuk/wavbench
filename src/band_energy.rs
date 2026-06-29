use realfft::RealFftPlanner;

use crate::audio_buffer::AudioBuffer;

#[derive(Debug)]
pub struct BandEnergy {
    pub sub: f32,        // 20-80
    pub bass: f32,       // 80-150
    pub low_mid: f32,    // 150-300
    pub mid: f32,        // 300-700
    pub upper_mid: f32,  // 700-1500
    pub presence: f32,   // 1500-3000
    pub brilliance: f32, // 3000-6000
    pub air: f32,        // 6000-10000
}

impl BandEnergy {
    fn new() -> Self {
        Self {
            sub: 0.0,
            bass: 0.0,
            low_mid: 0.0,
            mid: 0.0,
            upper_mid: 0.0,
            presence: 0.0,
            brilliance: 0.0,
            air: 0.0,
        }
    }

    pub fn add_power(&mut self, freq: f32, power: f32) {
        match freq {
            f if (20.0..80.0).contains(&f) => self.sub += power,
            f if (80.0..150.0).contains(&f) => self.bass += power,
            f if (150.0..300.0).contains(&f) => self.low_mid += power,
            f if (300.0..700.0).contains(&f) => self.mid += power,
            f if (700.0..1500.0).contains(&f) => self.upper_mid += power,
            f if (1500.0..3000.0).contains(&f) => self.presence += power,
            f if (3000.0..6000.0).contains(&f) => self.brilliance += power,
            f if (6000.0..10000.0).contains(&f) => self.air += power,
            _ => (),
        }
    }

    pub fn scale(&mut self, gain: f32) {
        self.sub *= gain;
        self.bass *= gain;
        self.low_mid *= gain;
        self.mid *= gain;
        self.upper_mid *= gain;
        self.presence *= gain;
        self.brilliance *= gain;
        self.air *= gain;
    }
}

const FFT_SIZE: usize = 4096;
const HOP_SIZE: usize = 2048;

pub fn get_band_energy(wav: &AudioBuffer) -> BandEnergy {
    let samples = wav.to_mono_left();

    let mut planner = RealFftPlanner::<f32>::new();
    let r2c = planner.plan_fft_forward(FFT_SIZE);

    let mut band_energy = BandEnergy::new();
    let mut block_count = 0.0f32;

    let mut start = 0;

    while start + FFT_SIZE <= samples.len() {
        let block = &samples[start..start + FFT_SIZE];

        let mut time_buffer = r2c.make_input_vec();

        for i in 0..FFT_SIZE {
            let window =
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos();

            time_buffer[i] = block[i] * window;
        }

        let mut spectrum = r2c.make_output_vec();
        r2c.process(&mut time_buffer, &mut spectrum).unwrap();

        for (i, bin) in spectrum.iter().enumerate() {
            let freq = i as f32 * wav.sample_rate as f32 / FFT_SIZE as f32;
            let power = bin.re * bin.re + bin.im * bin.im;

            band_energy.add_power(freq, power);
        }

        block_count += 1.0;
        start += HOP_SIZE;
    }

    band_energy.scale(1.0 / block_count);

    band_energy
}
