use realfft::RealFftPlanner;

use crate::metrics::{db, rms};

const FFT_SIZE: usize = 4096;
const HOP_SIZE: usize = 2048;

pub const BANDS_1_3_OCTAVE: [f32; 27] = [
    20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 255.0, 315.0, 400.0,
    500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0,
    8000.0,
];

#[derive(Debug)]
pub struct BandEnergy {
    pub bands: [f32; 27],
}

impl BandEnergy {
    pub fn new() -> Self {
        Self { bands: [0.0; 27] }
    }

    pub fn power_db(&self, x: f32) -> f32 {
        10.0 * x.max(1e-20).log10()
    }

    pub fn relative_db(&self, band_power: f32) -> f32 {
        self.power_db(band_power / self.total().max(1e-20))
    }

    pub fn from_samples(samples: &[f32], sample_rate: u32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(FFT_SIZE);

        let mut band_energy = BandEnergy::new();
        let mut block_count = 0.0f32;

        let mut start = 0;

        while start + FFT_SIZE <= samples.len() {
            let block = &samples[start..start + FFT_SIZE];

            if db(rms(block)) < -60.0 {
                block_count += 1.0;
                start += HOP_SIZE;
                continue;
            }

            let mut time_buffer = r2c.make_input_vec();

            for i in 0..FFT_SIZE {
                let window = 0.5
                    - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos();

                time_buffer[i] = block[i] * window;
            }

            let mut spectrum = r2c.make_output_vec();
            r2c.process(&mut time_buffer, &mut spectrum).unwrap();

            for (i, bin) in spectrum.iter().enumerate() {
                let freq = i as f32 * sample_rate as f32 / FFT_SIZE as f32;
                let power = bin.re * bin.re + bin.im * bin.im;

                band_energy.add_power(freq, power);
            }

            block_count += 1.0;
            start += HOP_SIZE;
        }

        band_energy.scale(1.0 / block_count);

        band_energy
    }

    fn add_power(&mut self, freq: f32, power: f32) {
        // Межі для 1/3 октави обчислюються як f_center * /0.891 та f_center * 1.122
        // Але для простоти пошуку знайдемо найближчу центральну частоту у логарифмічному просторі
        if freq < 17.8 || freq > 8912.0 {
            return; // Ігноруємо те, що поза межами нашого аналізу
        }

        let mut closest_idx = 0;
        let mut min_diff = f32::MAX;

        // Шукаємо найближчу центральну частоту
        for (idx, &center_freq) in BANDS_1_3_OCTAVE.iter().enumerate() {
            let diff = (freq / center_freq).ln().abs(); // Логарифмічна відстань
            if diff < min_diff {
                min_diff = diff;
                closest_idx = idx;
            }
        }

        self.bands[closest_idx] += power;
    }

    pub fn scale(&mut self, gain: f32) {
        for val in self.bands.iter_mut() {
            *val *= gain;
        }
    }

    pub fn total(&self) -> f32 {
        self.bands.iter().sum()
    }
}
