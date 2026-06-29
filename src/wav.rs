use std::{
    error::Error,
    io::{self, ErrorKind, Read},
    path::Path,
};

use crate::audio_buffer::AudioBuffer;

pub fn load_wav(path: &Path) -> Result<AudioBuffer, Box<dyn Error>> {
    validate_wav_path(path)?;

    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let samples = read_samples_as_f32(&mut reader, spec)?;

    Ok(AudioBuffer {
        channels: spec.channels as usize,
        sample_rate: spec.sample_rate,
        frames: (samples.len() as u32) / (spec.channels as u32),
        bits_per_sample: spec.bits_per_sample,
        id: path
            .file_name()
            .unwrap()
            .to_os_string()
            .into_string()
            .unwrap(),
        samples,
    })
}

fn validate_wav_path(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("WAV file does not exist: {}", path.display()),
        ));
    }

    if !path.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("WAV path is not a file: {}", path.display()),
        ));
    }

    let is_wav = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"));

    if !is_wav {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("Expected a .wav file: {}", path.display()),
        ));
    }

    Ok(())
}

fn read_samples_as_f32<R: Read>(
    reader: &mut hound::WavReader<R>,
    spec: hound::WavSpec,
) -> Result<Vec<f32>, Box<dyn Error>> {
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => match spec.bits_per_sample {
            1..=16 => {
                read_int_samples::<i16, _, _>(reader, spec.bits_per_sample, |value| value as f32)?
            }
            17..=32 => {
                read_int_samples::<i32, _, _>(reader, spec.bits_per_sample, |value| value as f32)?
            }
            bits_per_sample => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!("Unsupported int bit depth: {bits_per_sample}"),
                )
                .into());
            }
        },
    };

    Ok(samples)
}

fn read_int_samples<T, R, F>(
    reader: &mut hound::WavReader<R>,
    bits_per_sample: u16,
    to_f32: F,
) -> Result<Vec<f32>, hound::Error>
where
    T: hound::Sample,
    R: Read,
    F: Fn(T) -> f32,
{
    let max = ((1_i64 << (bits_per_sample - 1)) - 1) as f32;

    reader
        .samples::<T>()
        .map(|sample| sample.map(|value| to_f32(value) / max))
        .collect()
}
