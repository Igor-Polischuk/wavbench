use std::{
    error::Error,
    io::{self, ErrorKind},
    path::Path,
};

use crate::generator::presets::{BITS_PER_SAMPLE, CHANNELS, SAMPLE_RATE};

pub fn write_float_wav(path: &Path, samples: &[f32]) -> Result<(), Box<dyn Error>> {
    let spec = hound::WavSpec {
        channels: CHANNELS,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: BITS_PER_SAMPLE,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in samples {
        validate_sample(sample)?;
        writer.write_sample(sample)?;
    }
    writer.finalize()?;

    Ok(())
}

fn validate_sample(sample: f32) -> io::Result<()> {
    if !sample.is_finite() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Generated sample is not finite",
        ));
    }

    if sample.abs() > 1.0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Generated sample would clip",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use crate::generator::presets::{BITS_PER_SAMPLE, CHANNELS, SAMPLE_RATE};

    use super::write_float_wav;

    #[test]
    fn writes_mono_32_bit_float_wav() {
        let path = env::temp_dir().join(format!("wavbench_writer_test_{}.wav", process::id()));
        write_float_wav(&path, &[0.0, 0.5, -0.5]).expect("wav should be written");

        let reader = hound::WavReader::open(&path).expect("wav should be readable");
        let spec = reader.spec();

        assert_eq!(spec.channels, CHANNELS);
        assert_eq!(spec.sample_rate, SAMPLE_RATE);
        assert_eq!(spec.bits_per_sample, BITS_PER_SAMPLE);
        assert_eq!(spec.sample_format, hound::SampleFormat::Float);

        fs::remove_file(path).expect("test wav should be removed");
    }
}
