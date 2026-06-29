use std::{
    env,
    ffi::OsString,
    io::{self, ErrorKind},
    path::PathBuf,
};

#[derive(Debug)]
pub struct AnalyzerArgs {
    pub first_wav_path: PathBuf,
    pub second_wav_path: PathBuf,
}

pub fn parse_args() -> io::Result<AnalyzerArgs> {
    let mut args = env::args_os();
    let program_name = args
        .next()
        .unwrap_or_else(|| OsString::from("wav-analyzer"));

    parse_paths(args, &program_name)
}

fn parse_paths<I>(args: I, program_name: &OsString) -> io::Result<AnalyzerArgs>
where
    I: IntoIterator<Item = OsString>,
{
    let mut paths = args.into_iter().map(PathBuf::from).collect::<Vec<_>>();

    if paths.len() != 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Expected exactly 2 WAV paths, got {}.\nUsage: {} <first.wav> <second.wav>",
                paths.len(),
                program_name.to_string_lossy()
            ),
        ));
    }

    let second_wav_path = paths.pop().expect("second path exists after length check");
    let first_wav_path = paths.pop().expect("first path exists after length check");

    Ok(AnalyzerArgs {
        first_wav_path,
        second_wav_path,
    })
}
