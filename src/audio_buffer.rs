#[derive(Debug)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub frames: u32,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub id: String,
}

impl AudioBuffer {
    pub fn to_mono_left(&self) -> Vec<f32> {
        if self.channels == 1 {
            return self.samples.clone();
        }

        self.samples
            .chunks_exact(self.channels)
            .map(|frame| frame[0])
            .collect()
    }
}
