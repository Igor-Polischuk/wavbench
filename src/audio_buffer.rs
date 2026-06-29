#[derive(Debug)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub frames: u32,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
}
