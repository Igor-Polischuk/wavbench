use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    args::CompareSuiteArgs,
    audio_buffer::AudioBuffer,
    band_energy::{BANDS_1_3_OCTAVE, BandEnergy},
    metrics,
    noise_analysis::{NoiseAnalysisReport, analyze_noise_pair},
    sine_analysis::{SineAnalysisReport, analyze_sine_pair},
    sweep_analysis::{SweepAnalysisReport, analyze_sweep_pair},
    twotone_analysis::{TwotoneAnalysisReport, analyze_twotone_pair},
    wav::load_wav,
};

#[derive(Serialize)]
struct SuiteJsonReport {
    suite: SuiteJsonInfo,
    tests: Vec<TestJsonReport>,
}

#[derive(Serialize)]
struct SuiteJsonInfo {
    input_dir: String,
    target_dir: String,
    candidate_dir: String,
    test_count: usize,
}

#[derive(Serialize)]
struct TestJsonReport {
    id: String,
    #[serde(rename = "type")]
    test_type: String,
    files: TestJsonFiles,
    input: Value,
    target: Value,
    candidate: Value,
    diff: Value,
}

#[derive(Serialize)]
struct TestJsonFiles {
    input: String,
    target: String,
    candidate: String,
}

struct SuiteResult {
    report: SuiteJsonReport,
    tests: Vec<SuiteTestResult>,
    counts: BTreeMap<SuiteTestType, usize>,
    report_txt_path: PathBuf,
    report_json_path: PathBuf,
}

struct SuiteTestResult {
    id: String,
    test_type: SuiteTestType,
    files: TestJsonFiles,
    input: Value,
    target: Value,
    candidate: Value,
    diff: Value,
    text: TextTestReport,
}

enum TextTestReport {
    Sine {
        target: SineSuiteMetrics,
        candidate: SineSuiteMetrics,
        diff: SineSuiteMetrics,
    },
    Sweep {
        rows: Vec<BandCompareRow>,
    },
    Noise {
        spectrum_rows: Vec<BandCompareRow>,
        tilt_rows: Vec<NamedCompareRow>,
    },
    Twotone {
        target: TwotoneSuiteMetrics,
        candidate: TwotoneSuiteMetrics,
        diff: TwotoneSuiteMetrics,
        fundamentals: Vec<BandCompareRow>,
        im_products: Vec<BandCompareRow>,
    },
    General {
        target: GeneralSuiteMetrics,
        candidate: GeneralSuiteMetrics,
        diff: GeneralSuiteMetrics,
        bands: Vec<BandCompareRow>,
    },
}

#[derive(Clone, Debug)]
struct SineSuiteMetrics {
    output_rms_dbfs: f32,
    rms_gain_db: f32,
    fundamental_dbfs: f32,
    dc_offset: f32,
    noise_floor_dbfs: f32,
    thd_percent: f32,
    thdn_percent: f32,
    sinad_db: f32,
    harmonics: BTreeMap<String, f32>,
}

#[derive(Clone, Debug)]
struct TwotoneSuiteMetrics {
    output_rms_dbfs: f32,
    imd_percent: f32,
}

#[derive(Clone, Debug)]
struct GeneralSuiteMetrics {
    rms_dbfs: f32,
    peak_dbfs: f32,
}

#[derive(Clone, Debug)]
struct BandCompareRow {
    label: String,
    target: f32,
    candidate: f32,
    diff: f32,
}

#[derive(Clone, Debug)]
struct NamedCompareRow {
    label: String,
    target: f32,
    candidate: f32,
    diff: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum SuiteTestType {
    Sine,
    Sweep,
    Noise,
    Twotone,
    General,
}

impl SuiteTestType {
    fn json_name(self) -> &'static str {
        match self {
            SuiteTestType::Sine => "sine",
            SuiteTestType::Sweep => "sweep",
            SuiteTestType::Noise => "noise",
            SuiteTestType::Twotone => "twotone",
            SuiteTestType::General => "general",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            SuiteTestType::Sine => "SINE",
            SuiteTestType::Sweep => "SWEEP",
            SuiteTestType::Noise => "NOISE",
            SuiteTestType::Twotone => "TWO-TONE",
            SuiteTestType::General => "GENERAL",
        }
    }

    fn all() -> [Self; 5] {
        [
            SuiteTestType::Sine,
            SuiteTestType::Sweep,
            SuiteTestType::Noise,
            SuiteTestType::Twotone,
            SuiteTestType::General,
        ]
    }
}

pub fn run(args: CompareSuiteArgs) -> Result<(), Box<dyn Error>> {
    let result = build_suite(args)?;
    fs::create_dir_all(
        result
            .report_txt_path
            .parent()
            .expect("report path should have a parent"),
    )?;

    fs::write(&result.report_txt_path, render_text_report(&result))?;
    fs::write(
        &result.report_json_path,
        serde_json::to_string_pretty(&result.report)?,
    )?;

    print_console_summary(&result);

    Ok(())
}

fn build_suite(args: CompareSuiteArgs) -> Result<SuiteResult, Box<dyn Error>> {
    validate_dir("input-dir", &args.input_dir)?;
    validate_dir("target-dir", &args.target_dir)?;
    validate_dir("candidate-dir", &args.candidate_dir)?;

    let input_files = wav_filenames(&args.input_dir)?;
    if input_files.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "No .wav files found in input-dir: {}",
                args.input_dir.display()
            ),
        )
        .into());
    }

    let target_files = wav_filenames(&args.target_dir)?;
    let candidate_files = wav_filenames(&args.candidate_dir)?;
    validate_exact_filename_set("target-dir", &input_files, &target_files)?;
    validate_exact_filename_set("candidate-dir", &input_files, &candidate_files)?;

    let mut specs = input_files
        .iter()
        .map(|file_name| SuiteFileSpec {
            file_name: file_name.clone(),
            test_type: detect_test_type(file_name),
        })
        .collect::<Vec<_>>();
    specs.sort_by(|left, right| {
        left.test_type
            .cmp(&right.test_type)
            .then_with(|| left.file_name.cmp(&right.file_name))
    });

    let mut json_tests = Vec::with_capacity(specs.len());
    let mut text_tests = Vec::with_capacity(specs.len());
    let mut counts = BTreeMap::new();

    for spec in specs {
        let test = analyze_suite_file(&args, &spec)?;
        *counts.entry(spec.test_type).or_insert(0) += 1;
        json_tests.push(test_json(&test));
        text_tests.push(test);
    }

    let report = SuiteJsonReport {
        suite: SuiteJsonInfo {
            input_dir: path_string(&args.input_dir),
            target_dir: path_string(&args.target_dir),
            candidate_dir: path_string(&args.candidate_dir),
            test_count: json_tests.len(),
        },
        tests: json_tests,
    };

    Ok(SuiteResult {
        report,
        tests: text_tests,
        counts,
        report_txt_path: args.report_dir.join("report.txt"),
        report_json_path: args.report_dir.join("report.json"),
    })
}

#[derive(Debug)]
struct SuiteFileSpec {
    file_name: String,
    test_type: SuiteTestType,
}

fn analyze_suite_file(
    args: &CompareSuiteArgs,
    spec: &SuiteFileSpec,
) -> Result<SuiteTestResult, Box<dyn Error>> {
    let input_path = args.input_dir.join(&spec.file_name);
    let target_path = args.target_dir.join(&spec.file_name);
    let candidate_path = args.candidate_dir.join(&spec.file_name);
    let input_wav = load_wav(&input_path)?;
    let target_wav = load_wav(&target_path)?;
    let candidate_wav = load_wav(&candidate_path)?;
    let id = test_id(&spec.file_name);
    let files = TestJsonFiles {
        input: path_string(&input_path),
        target: path_string(&target_path),
        candidate: path_string(&candidate_path),
    };

    let (input, target, candidate, diff, text) = match spec.test_type {
        SuiteTestType::Sine => {
            let target_report = analyze_sine_pair(&input_wav, &target_wav)?;
            let candidate_report = analyze_sine_pair(&input_wav, &candidate_wav)?;
            let target_metrics = sine_metrics(&target_report);
            let candidate_metrics = sine_metrics(&candidate_report);
            let diff_metrics = diff_sine_metrics(&candidate_metrics, &target_metrics);
            (
                sine_input_json(&target_report),
                sine_metrics_json(&target_metrics),
                sine_metrics_json(&candidate_metrics),
                sine_metrics_json(&diff_metrics),
                TextTestReport::Sine {
                    target: target_metrics,
                    candidate: candidate_metrics,
                    diff: diff_metrics,
                },
            )
        }
        SuiteTestType::Sweep => {
            let target_report = analyze_sweep_pair(&input_wav, &target_wav)?;
            let candidate_report = analyze_sweep_pair(&input_wav, &candidate_wav)?;
            let rows = response_rows(
                &target_report.frequency_response,
                &candidate_report.frequency_response,
                |point| point.frequency_hz,
                |point| point.magnitude_db,
            );
            (
                sweep_input_json(&target_report),
                sweep_metrics_json(&target_report),
                sweep_metrics_json(&candidate_report),
                sweep_diff_json(&target_report, &candidate_report),
                TextTestReport::Sweep { rows },
            )
        }
        SuiteTestType::Noise => {
            let target_report = analyze_noise_pair(&input_wav, &target_wav)?;
            let candidate_report = analyze_noise_pair(&input_wav, &candidate_wav)?;
            let spectrum_rows = response_rows(
                &target_report.average_spectrum,
                &candidate_report.average_spectrum,
                |point| point.frequency_hz,
                |point| point.response_db,
            );
            let tilt_rows = noise_tilt_rows(&target_report, &candidate_report);
            (
                noise_input_json(&target_report),
                noise_metrics_json(&target_report),
                noise_metrics_json(&candidate_report),
                noise_diff_json(&target_report, &candidate_report),
                TextTestReport::Noise {
                    spectrum_rows,
                    tilt_rows,
                },
            )
        }
        SuiteTestType::Twotone => {
            let target_report = analyze_twotone_pair(&input_wav, &target_wav)?;
            let candidate_report = analyze_twotone_pair(&input_wav, &candidate_wav)?;
            let target_metrics = twotone_metrics(&target_report);
            let candidate_metrics = twotone_metrics(&candidate_report);
            let diff_metrics = TwotoneSuiteMetrics {
                output_rms_dbfs: candidate_metrics.output_rms_dbfs - target_metrics.output_rms_dbfs,
                imd_percent: candidate_metrics.imd_percent - target_metrics.imd_percent,
            };
            let fundamentals = response_rows(
                &target_report.fundamentals,
                &candidate_report.fundamentals,
                |point| point.frequency_hz,
                |point| point.level_dbfs,
            );
            let im_products = response_rows(
                &target_report.intermodulation,
                &candidate_report.intermodulation,
                |point| point.frequency_hz,
                |point| point.relative_db,
            );
            (
                twotone_input_json(&target_report),
                twotone_metrics_json(&target_report),
                twotone_metrics_json(&candidate_report),
                twotone_diff_json(&target_report, &candidate_report),
                TextTestReport::Twotone {
                    target: target_metrics,
                    candidate: candidate_metrics,
                    diff: diff_metrics,
                    fundamentals,
                    im_products,
                },
            )
        }
        SuiteTestType::General => {
            validate_general_sample_rates(&input_wav, &target_wav, &candidate_wav)?;
            let target = general_metrics(&target_wav)?;
            let candidate = general_metrics(&candidate_wav)?;
            let diff = GeneralSuiteMetrics {
                rms_dbfs: candidate.rms_dbfs - target.rms_dbfs,
                peak_dbfs: candidate.peak_dbfs - target.peak_dbfs,
            };
            let target_energy =
                BandEnergy::from_samples(&target_wav.to_mono_left(), target_wav.sample_rate)?;
            let candidate_energy =
                BandEnergy::from_samples(&candidate_wav.to_mono_left(), candidate_wav.sample_rate)?;
            let bands = general_band_rows(&target_energy, &candidate_energy);
            (
                general_input_json(&input_wav),
                general_metrics_json(&target, &target_energy),
                general_metrics_json(&candidate, &candidate_energy),
                general_diff_json(&diff, &bands),
                TextTestReport::General {
                    target,
                    candidate,
                    diff,
                    bands,
                },
            )
        }
    };

    Ok(SuiteTestResult {
        id,
        test_type: spec.test_type,
        files,
        input,
        target,
        candidate,
        diff,
        text,
    })
}

fn test_json(test: &SuiteTestResult) -> TestJsonReport {
    TestJsonReport {
        id: test.id.clone(),
        test_type: test.test_type.json_name().to_string(),
        files: TestJsonFiles {
            input: test.files.input.clone(),
            target: test.files.target.clone(),
            candidate: test.files.candidate.clone(),
        },
        input: test.input.clone(),
        target: test.target.clone(),
        candidate: test.candidate.clone(),
        diff: test.diff.clone(),
    }
}

fn validate_dir(label: &str, path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("{label} does not exist: {}", path.display()),
        ));
    }

    if !path.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{label} is not a directory: {}", path.display()),
        ));
    }

    Ok(())
}

fn wav_filenames(dir: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let mut files = BTreeSet::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || !is_wav_path(&path) {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!("WAV filename is not valid UTF-8: {}", path.display()),
                )
            })?;
        files.insert(file_name.to_string());
    }

    Ok(files)
}

fn is_wav_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
}

fn validate_exact_filename_set(
    label: &str,
    input_files: &BTreeSet<String>,
    actual_files: &BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    let missing = input_files
        .difference(actual_files)
        .cloned()
        .collect::<Vec<_>>();
    let extra = actual_files
        .difference(input_files)
        .cloned()
        .collect::<Vec<_>>();

    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }

    let mut parts = Vec::new();
    if !missing.is_empty() {
        parts.push(format!("missing WAV files: {}", missing.join(", ")));
    }
    if !extra.is_empty() {
        parts.push(format!("extra WAV files: {}", extra.join(", ")));
    }

    Err(io::Error::new(
        ErrorKind::InvalidInput,
        format!(
            "{label} does not exactly match input-dir: {}",
            parts.join("; ")
        ),
    )
    .into())
}

fn detect_test_type(file_name: &str) -> SuiteTestType {
    let stem = file_name.strip_suffix(".wav").unwrap_or(file_name);

    if stem.starts_with("sine_") {
        SuiteTestType::Sine
    } else if stem.starts_with("sweep_") {
        SuiteTestType::Sweep
    } else if stem.starts_with("pink_noise") || stem.starts_with("white_noise") {
        SuiteTestType::Noise
    } else if stem.starts_with("twotone_") || stem.starts_with("two_tone_") {
        SuiteTestType::Twotone
    } else {
        SuiteTestType::General
    }
}

fn validate_general_sample_rates(
    input_wav: &AudioBuffer,
    target_wav: &AudioBuffer,
    candidate_wav: &AudioBuffer,
) -> io::Result<()> {
    if input_wav.sample_rate == target_wav.sample_rate
        && target_wav.sample_rate == candidate_wav.sample_rate
    {
        return Ok(());
    }

    Err(io::Error::new(
        ErrorKind::InvalidInput,
        format!(
            "Sample rates differ for general comparison: input={}Hz target={}Hz candidate={}Hz",
            input_wav.sample_rate, target_wav.sample_rate, candidate_wav.sample_rate
        ),
    ))
}

fn sine_metrics(report: &SineAnalysisReport) -> SineSuiteMetrics {
    let mut harmonics = BTreeMap::new();
    for number in 2..=5 {
        if let Some(harmonic) = report
            .harmonics
            .iter()
            .find(|harmonic| harmonic.number == number)
        {
            harmonics.insert(format!("h{number}_dbc"), harmonic.relative_db);
        }
    }

    SineSuiteMetrics {
        output_rms_dbfs: report.output.rms_dbfs,
        rms_gain_db: report.rms_gain_db,
        fundamental_dbfs: report.fundamental.level_dbfs,
        dc_offset: report.dc_offset,
        noise_floor_dbfs: report.noise_floor_dbfs,
        thd_percent: report.distortion.thd_percent,
        thdn_percent: report.distortion.thdn_percent,
        sinad_db: report.distortion.sinad_db,
        harmonics,
    }
}

fn diff_sine_metrics(candidate: &SineSuiteMetrics, target: &SineSuiteMetrics) -> SineSuiteMetrics {
    let mut harmonics = BTreeMap::new();
    for (key, target_value) in &target.harmonics {
        if let Some(candidate_value) = candidate.harmonics.get(key) {
            harmonics.insert(key.clone(), candidate_value - target_value);
        }
    }

    SineSuiteMetrics {
        output_rms_dbfs: candidate.output_rms_dbfs - target.output_rms_dbfs,
        rms_gain_db: candidate.rms_gain_db - target.rms_gain_db,
        fundamental_dbfs: candidate.fundamental_dbfs - target.fundamental_dbfs,
        dc_offset: candidate.dc_offset - target.dc_offset,
        noise_floor_dbfs: candidate.noise_floor_dbfs - target.noise_floor_dbfs,
        thd_percent: candidate.thd_percent - target.thd_percent,
        thdn_percent: candidate.thdn_percent - target.thdn_percent,
        sinad_db: candidate.sinad_db - target.sinad_db,
        harmonics,
    }
}

fn twotone_metrics(report: &TwotoneAnalysisReport) -> TwotoneSuiteMetrics {
    TwotoneSuiteMetrics {
        output_rms_dbfs: report.output.rms_dbfs,
        imd_percent: report.imd.percent,
    }
}

fn general_metrics(wav: &AudioBuffer) -> Result<GeneralSuiteMetrics, Box<dyn Error>> {
    let samples = wav.to_mono_left();
    if samples.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("WAV does not contain samples: {}", wav.id),
        )
        .into());
    }

    Ok(GeneralSuiteMetrics {
        rms_dbfs: metrics::db(metrics::rms(&samples)),
        peak_dbfs: metrics::db(peak(&samples)),
    })
}

fn general_band_rows(target: &BandEnergy, candidate: &BandEnergy) -> Vec<BandCompareRow> {
    BANDS_1_3_OCTAVE
        .iter()
        .enumerate()
        .map(|(index, &frequency_hz)| {
            let target_db = metrics::db(target.bands[index]);
            let candidate_db = metrics::db(candidate.bands[index]);
            BandCompareRow {
                label: format_frequency_label(frequency_hz),
                target: target_db,
                candidate: candidate_db,
                diff: candidate_db - target_db,
            }
        })
        .collect()
}

fn response_rows<T>(
    target: &[T],
    candidate: &[T],
    frequency: impl Fn(&T) -> f32 + Copy,
    value: impl Fn(&T) -> f32 + Copy,
) -> Vec<BandCompareRow> {
    target
        .iter()
        .filter_map(|target_point| {
            let frequency_hz = frequency(target_point);
            let candidate_point = candidate
                .iter()
                .find(|point| (frequency(*point) - frequency_hz).abs() <= 0.5)?;
            let target_value = value(target_point);
            let candidate_value = value(candidate_point);

            Some(BandCompareRow {
                label: format_frequency_label(frequency_hz),
                target: target_value,
                candidate: candidate_value,
                diff: candidate_value - target_value,
            })
        })
        .collect()
}

fn noise_tilt_rows(
    target: &NoiseAnalysisReport,
    candidate: &NoiseAnalysisReport,
) -> Vec<NamedCompareRow> {
    target
        .spectral_tilt
        .iter()
        .filter_map(|target_band| {
            let candidate_band = candidate
                .spectral_tilt
                .iter()
                .find(|band| band.name == target_band.name)?;

            Some(NamedCompareRow {
                label: target_band.name.to_string(),
                target: target_band.response_db,
                candidate: candidate_band.response_db,
                diff: candidate_band.response_db - target_band.response_db,
            })
        })
        .collect()
}

fn sine_input_json(report: &SineAnalysisReport) -> Value {
    json!({
        "frequency_hz": report.input.frequency_hz.map(r2),
        "duration_secs": r2(report.input.duration_secs),
        "peak_dbfs": r2(report.input.peak_dbfs),
        "rms_dbfs": r2(report.input.rms_dbfs)
    })
}

fn sine_metrics_json(metrics: &SineSuiteMetrics) -> Value {
    json!({
        "output_rms_dbfs": r2(metrics.output_rms_dbfs),
        "rms_gain_db": r2(metrics.rms_gain_db),
        "fundamental_dbfs": r2(metrics.fundamental_dbfs),
        "dc_offset": r5(metrics.dc_offset),
        "noise_floor_dbfs": r1(metrics.noise_floor_dbfs),
        "thd_percent": r2(metrics.thd_percent),
        "thdn_percent": r2(metrics.thdn_percent),
        "sinad_db": r1(metrics.sinad_db),
        "harmonics": numeric_map_json(metrics.harmonics.iter().map(|(key, value)| (key.as_str(), *value)), 1)
    })
}

fn sweep_input_json(report: &SweepAnalysisReport) -> Value {
    json!({
        "start_frequency_hz": r2(report.input.start_frequency_hz),
        "end_frequency_hz": r2(report.input.end_frequency_hz),
        "duration_secs": r2(report.input.duration_secs),
        "rms_dbfs": r2(report.input.rms_dbfs)
    })
}

fn sweep_metrics_json(report: &SweepAnalysisReport) -> Value {
    json!({
        "output_peak_dbfs": r2(report.output.peak_dbfs),
        "output_rms_dbfs": r2(report.output.rms_dbfs),
        "rms_gain_db": r2(report.output.rms_gain_db),
        "delay_samples": report.delay_samples,
        "frequency_response": numeric_map_json(
            report.frequency_response.iter().map(|point| {
                (frequency_key(point.frequency_hz), point.magnitude_db)
            }),
            1,
        )
    })
}

fn sweep_diff_json(target: &SweepAnalysisReport, candidate: &SweepAnalysisReport) -> Value {
    let rows = response_rows(
        &target.frequency_response,
        &candidate.frequency_response,
        |point| point.frequency_hz,
        |point| point.magnitude_db,
    );

    json!({
        "output_peak_dbfs": r2(candidate.output.peak_dbfs - target.output.peak_dbfs),
        "output_rms_dbfs": r2(candidate.output.rms_dbfs - target.output.rms_dbfs),
        "rms_gain_db": r2(candidate.output.rms_gain_db - target.output.rms_gain_db),
        "delay_samples": candidate.delay_samples - target.delay_samples,
        "frequency_response": numeric_map_json(rows.iter().map(|row| (row.label.as_str(), row.diff)), 1)
    })
}

fn noise_input_json(report: &NoiseAnalysisReport) -> Value {
    json!({
        "rms_dbfs": r2(report.input.rms_dbfs),
        "peak_dbfs": r2(report.input.peak_dbfs),
        "crest_factor_db": r2(report.input.crest_factor_db)
    })
}

fn noise_metrics_json(report: &NoiseAnalysisReport) -> Value {
    json!({
        "output_rms_dbfs": r2(report.output.rms_dbfs),
        "output_peak_dbfs": r2(report.output.peak_dbfs),
        "output_crest_factor_db": r2(report.output.crest_factor_db),
        "dc_offset": report.output.dc_offset.map(r5),
        "rms_gain_db": r2(report.rms_gain_db),
        "average_spectrum": numeric_map_json(
            report.average_spectrum.iter().map(|point| {
                (frequency_key(point.frequency_hz), point.response_db)
            }),
            1,
        ),
        "spectral_tilt": numeric_map_json(
            report.spectral_tilt.iter().map(|band| (band.name, band.response_db)),
            1,
        )
    })
}

fn noise_diff_json(target: &NoiseAnalysisReport, candidate: &NoiseAnalysisReport) -> Value {
    let spectrum_rows = response_rows(
        &target.average_spectrum,
        &candidate.average_spectrum,
        |point| point.frequency_hz,
        |point| point.response_db,
    );
    let tilt_rows = noise_tilt_rows(target, candidate);

    json!({
        "output_rms_dbfs": r2(candidate.output.rms_dbfs - target.output.rms_dbfs),
        "output_peak_dbfs": r2(candidate.output.peak_dbfs - target.output.peak_dbfs),
        "output_crest_factor_db": r2(candidate.output.crest_factor_db - target.output.crest_factor_db),
        "dc_offset": r5(candidate.output.dc_offset.unwrap_or(0.0) - target.output.dc_offset.unwrap_or(0.0)),
        "rms_gain_db": r2(candidate.rms_gain_db - target.rms_gain_db),
        "average_spectrum": numeric_map_json(spectrum_rows.iter().map(|row| (row.label.as_str(), row.diff)), 1),
        "spectral_tilt": numeric_map_json(tilt_rows.iter().map(|row| (row.label.as_str(), row.diff)), 1)
    })
}

fn twotone_input_json(report: &TwotoneAnalysisReport) -> Value {
    json!({
        "first_frequency_hz": r2(report.input.first_frequency_hz),
        "second_frequency_hz": r2(report.input.second_frequency_hz)
    })
}

fn twotone_metrics_json(report: &TwotoneAnalysisReport) -> Value {
    json!({
        "output_peak_dbfs": r2(report.output.peak_dbfs),
        "output_rms_dbfs": r2(report.output.rms_dbfs),
        "rms_gain_db": r2(report.output.rms_gain_db),
        "imd_percent": r2(report.imd.percent),
        "imd_db": r1(report.imd.relative_db),
        "fundamentals": numeric_map_json(
            report.fundamentals.iter().map(|point| {
                (frequency_key(point.frequency_hz), point.level_dbfs)
            }),
            1,
        ),
        "intermodulation": numeric_map_json(
            report.intermodulation.iter().map(|point| {
                (frequency_key(point.frequency_hz), point.relative_db)
            }),
            1,
        )
    })
}

fn twotone_diff_json(target: &TwotoneAnalysisReport, candidate: &TwotoneAnalysisReport) -> Value {
    let fundamentals = response_rows(
        &target.fundamentals,
        &candidate.fundamentals,
        |point| point.frequency_hz,
        |point| point.level_dbfs,
    );
    let im_products = response_rows(
        &target.intermodulation,
        &candidate.intermodulation,
        |point| point.frequency_hz,
        |point| point.relative_db,
    );

    json!({
        "output_peak_dbfs": r2(candidate.output.peak_dbfs - target.output.peak_dbfs),
        "output_rms_dbfs": r2(candidate.output.rms_dbfs - target.output.rms_dbfs),
        "rms_gain_db": r2(candidate.output.rms_gain_db - target.output.rms_gain_db),
        "imd_percent": r2(candidate.imd.percent - target.imd.percent),
        "imd_db": r1(candidate.imd.relative_db - target.imd.relative_db),
        "fundamentals": numeric_map_json(fundamentals.iter().map(|row| (row.label.as_str(), row.diff)), 1),
        "intermodulation": numeric_map_json(im_products.iter().map(|row| (row.label.as_str(), row.diff)), 1)
    })
}

fn general_input_json(wav: &AudioBuffer) -> Value {
    let samples = wav.to_mono_left();
    json!({
        "duration_secs": r2(wav.frames as f32 / wav.sample_rate as f32),
        "sample_rate": wav.sample_rate,
        "channels": wav.channels,
        "rms_dbfs": r2(metrics::db(metrics::rms(&samples))),
        "peak_dbfs": r2(metrics::db(peak(&samples)))
    })
}

fn general_metrics_json(metrics: &GeneralSuiteMetrics, energy: &BandEnergy) -> Value {
    json!({
        "rms_dbfs": r2(metrics.rms_dbfs),
        "peak_dbfs": r2(metrics.peak_dbfs),
        "band_energy": numeric_map_json(
            BANDS_1_3_OCTAVE.iter().enumerate().map(|(index, &frequency_hz)| {
                (frequency_key(frequency_hz), crate::metrics::db(energy.bands[index]))
            }),
            1,
        )
    })
}

fn general_diff_json(metrics: &GeneralSuiteMetrics, bands: &[BandCompareRow]) -> Value {
    json!({
        "rms_dbfs": r2(metrics.rms_dbfs),
        "peak_dbfs": r2(metrics.peak_dbfs),
        "band_energy": numeric_map_json(bands.iter().map(|row| (row.label.as_str(), row.diff)), 1)
    })
}

fn numeric_map_json<K>(values: impl IntoIterator<Item = (K, f32)>, decimals: u32) -> Value
where
    K: AsRef<str>,
{
    let map = values
        .into_iter()
        .map(|(key, value)| (json_key(key.as_ref()), round(value, decimals)))
        .collect::<BTreeMap<_, _>>();
    json!(map)
}

fn render_text_report(result: &SuiteResult) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    writeln!(out, "{}", "=".repeat(56)).unwrap();
    writeln!(out, "COMPARE SUITE").unwrap();
    writeln!(out, "{}", "=".repeat(56)).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "Input dir:     {}", result.report.suite.input_dir).unwrap();
    writeln!(out, "Target dir:    {}", result.report.suite.target_dir).unwrap();
    writeln!(out, "Candidate dir: {}", result.report.suite.candidate_dir).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "Matched tests: {}", result.report.suite.test_count).unwrap();

    for test_type in SuiteTestType::all() {
        let tests = result
            .tests
            .iter()
            .filter(|test| test.test_type == test_type)
            .collect::<Vec<_>>();
        if tests.is_empty() {
            continue;
        }

        writeln!(out).unwrap();
        writeln!(out, "{}", "=".repeat(56)).unwrap();
        writeln!(out, "{}", test_type.heading()).unwrap();
        writeln!(out, "{}", "=".repeat(56)).unwrap();

        for test in tests {
            writeln!(out).unwrap();
            writeln!(out, "{}", test.id).unwrap();
            writeln!(out, "{}", "-".repeat(40)).unwrap();
            render_test_text(&mut out, &test.text);
        }
    }

    out
}

fn render_test_text(out: &mut String, text: &TextTestReport) {
    match text {
        TextTestReport::Sine {
            target,
            candidate,
            diff,
        } => {
            write_metric_header(out);
            write_metric_row(
                out,
                "Output RMS dBFS",
                target.output_rms_dbfs,
                candidate.output_rms_dbfs,
                diff.output_rms_dbfs,
                2,
                false,
            );
            write_metric_row(
                out,
                "RMS Gain dB",
                target.rms_gain_db,
                candidate.rms_gain_db,
                diff.rms_gain_db,
                2,
                true,
            );
            write_metric_row(
                out,
                "Fundamental dBFS",
                target.fundamental_dbfs,
                candidate.fundamental_dbfs,
                diff.fundamental_dbfs,
                2,
                false,
            );
            write_metric_row(
                out,
                "DC Offset",
                target.dc_offset,
                candidate.dc_offset,
                diff.dc_offset,
                5,
                true,
            );
            write_metric_row(
                out,
                "Noise Floor dBFS",
                target.noise_floor_dbfs,
                candidate.noise_floor_dbfs,
                diff.noise_floor_dbfs,
                1,
                false,
            );
            write_metric_row(
                out,
                "THD %",
                target.thd_percent,
                candidate.thd_percent,
                diff.thd_percent,
                2,
                false,
            );
            write_metric_row(
                out,
                "THD+N %",
                target.thdn_percent,
                candidate.thdn_percent,
                diff.thdn_percent,
                2,
                false,
            );
            write_metric_row(
                out,
                "SINAD dB",
                target.sinad_db,
                candidate.sinad_db,
                diff.sinad_db,
                1,
                false,
            );

            out.push('\n');
            out.push_str("Harmonics\n");
            for number in 2..=5 {
                let key = format!("h{number}_dbc");
                if let (Some(target_value), Some(candidate_value), Some(diff_value)) = (
                    target.harmonics.get(&key),
                    candidate.harmonics.get(&key),
                    diff.harmonics.get(&key),
                ) {
                    write_metric_row(
                        out,
                        &format!("H{number} dBc"),
                        *target_value,
                        *candidate_value,
                        *diff_value,
                        1,
                        false,
                    );
                }
            }
        }
        TextTestReport::Sweep { rows } => {
            write_band_header(out);
            for row in rows {
                write_band_row(
                    out,
                    &row.label,
                    row.target,
                    row.candidate,
                    row.diff,
                    1,
                    true,
                );
            }
        }
        TextTestReport::Noise {
            spectrum_rows,
            tilt_rows,
        } => {
            out.push_str("Average Spectrum\n");
            write_band_header(out);
            for row in spectrum_rows {
                write_band_row(
                    out,
                    &row.label,
                    row.target,
                    row.candidate,
                    row.diff,
                    1,
                    true,
                );
            }

            out.push('\n');
            out.push_str("Spectral Tilt\n");
            write_band_header(out);
            for row in tilt_rows {
                write_band_row(
                    out,
                    &row.label,
                    row.target,
                    row.candidate,
                    row.diff,
                    1,
                    true,
                );
            }
        }
        TextTestReport::Twotone {
            target,
            candidate,
            diff,
            fundamentals,
            im_products,
        } => {
            write_metric_header(out);
            write_metric_row(
                out,
                "Output RMS dBFS",
                target.output_rms_dbfs,
                candidate.output_rms_dbfs,
                diff.output_rms_dbfs,
                2,
                false,
            );
            write_metric_row(
                out,
                "IMD %",
                target.imd_percent,
                candidate.imd_percent,
                diff.imd_percent,
                2,
                false,
            );

            out.push('\n');
            out.push_str("Fundamentals\n");
            write_band_header(out);
            for row in fundamentals {
                write_band_row(
                    out,
                    &format!("{} dBFS", row.label),
                    row.target,
                    row.candidate,
                    row.diff,
                    1,
                    false,
                );
            }

            out.push('\n');
            out.push_str("IM Products\n");
            write_band_header(out);
            for row in im_products {
                write_band_row(
                    out,
                    &format!("{} dBc", row.label),
                    row.target,
                    row.candidate,
                    row.diff,
                    1,
                    false,
                );
            }
        }
        TextTestReport::General {
            target,
            candidate,
            diff,
            bands,
        } => {
            write_metric_header(out);
            write_metric_row(
                out,
                "RMS dBFS",
                target.rms_dbfs,
                candidate.rms_dbfs,
                diff.rms_dbfs,
                2,
                false,
            );
            write_metric_row(
                out,
                "Peak dBFS",
                target.peak_dbfs,
                candidate.peak_dbfs,
                diff.peak_dbfs,
                2,
                false,
            );

            out.push('\n');
            out.push_str("Band Energy\n");
            write_band_header(out);
            for row in bands {
                write_band_row(
                    out,
                    &row.label,
                    row.target,
                    row.candidate,
                    row.diff,
                    1,
                    false,
                );
            }
        }
    }
}

fn write_metric_header(out: &mut String) {
    use std::fmt::Write as _;

    writeln!(
        out,
        "{:<22} {:>11} {:>13} {:>10}",
        "Metric", "Target", "Candidate", "Diff"
    )
    .unwrap();
}

fn write_metric_row(
    out: &mut String,
    label: &str,
    target: f32,
    candidate: f32,
    diff: f32,
    decimals: usize,
    signed_values: bool,
) {
    use std::fmt::Write as _;

    writeln!(
        out,
        "{:<22} {:>11} {:>13} {:>10}",
        label,
        format_number(target, decimals, signed_values),
        format_number(candidate, decimals, signed_values),
        format_number(diff, decimals, true)
    )
    .unwrap();
}

fn write_band_header(out: &mut String) {
    use std::fmt::Write as _;

    writeln!(
        out,
        "{:<14} {:>11} {:>13} {:>10}",
        "Freq", "Target dB", "Candidate dB", "Diff"
    )
    .unwrap();
}

fn write_band_row(
    out: &mut String,
    label: &str,
    target: f32,
    candidate: f32,
    diff: f32,
    decimals: usize,
    signed_values: bool,
) {
    use std::fmt::Write as _;

    writeln!(
        out,
        "{:<14} {:>11} {:>13} {:>10}",
        label,
        format_number(target, decimals, signed_values),
        format_number(candidate, decimals, signed_values),
        format_number(diff, decimals, true)
    )
    .unwrap();
}

fn print_console_summary(result: &SuiteResult) {
    println!("Found {} tests", result.report.suite.test_count);
    for test_type in SuiteTestType::all() {
        if let Some(count) = result.counts.get(&test_type) {
            println!("{}: {}", test_type.json_name(), count);
        }
    }

    println!();
    println!("Reports written:");
    println!("{}", result.report_txt_path.display());
    println!("{}", result.report_json_path.display());
}

fn format_number(value: f32, decimals: usize, signed: bool) -> String {
    if signed {
        format!("{:+.*}", decimals, value)
    } else {
        format!("{:.*}", decimals, value)
    }
}

fn format_frequency_label(frequency_hz: f32) -> String {
    if frequency_hz >= 1000.0 {
        return format!("{} kHz", trim_float(frequency_hz / 1000.0));
    }

    format!("{} Hz", trim_float(frequency_hz))
}

fn frequency_key(frequency_hz: f32) -> String {
    json_key(&format_frequency_label(frequency_hz))
}

fn json_key(label: &str) -> String {
    label
        .trim()
        .to_ascii_lowercase()
        .replace(' ', "_")
        .replace('-', "_")
}

fn trim_float(value: f32) -> String {
    if (value - value.round()).abs() < 0.001 {
        return format!("{:.0}", value);
    }

    let formatted = format!("{:.2}", value);
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn test_id(file_name: &str) -> String {
    file_name
        .strip_suffix(".wav")
        .or_else(|| file_name.strip_suffix(".WAV"))
        .unwrap_or(file_name)
        .to_string()
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}

fn r1(value: f32) -> f64 {
    round(value, 1)
}

fn r2(value: f32) -> f64 {
    round(value, 2)
}

fn r5(value: f32) -> f64 {
    round(value, 5)
}

fn round(value: f32, decimals: u32) -> f64 {
    let scale = 10_f64.powi(decimals as i32);
    ((value as f64) * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{SuiteTestType, detect_test_type, validate_exact_filename_set};

    #[test]
    fn detects_test_types_from_filename() {
        assert_eq!(detect_test_type("sine_100_-12.wav"), SuiteTestType::Sine);
        assert_eq!(
            detect_test_type("sweep_20_20k_-36.wav"),
            SuiteTestType::Sweep
        );
        assert_eq!(detect_test_type("pink_noise_-24.wav"), SuiteTestType::Noise);
        assert_eq!(detect_test_type("white_noise.wav"), SuiteTestType::Noise);
        assert_eq!(
            detect_test_type("twotone_700_1900_-24.wav"),
            SuiteTestType::Twotone
        );
        assert_eq!(detect_test_type("riff.wav"), SuiteTestType::General);
    }

    #[test]
    fn accepts_exact_filename_sets() {
        let input = set(&["a.wav", "b.wav"]);
        let actual = set(&["a.wav", "b.wav"]);

        validate_exact_filename_set("target-dir", &input, &actual)
            .expect("matching sets should be valid");
    }

    #[test]
    fn rejects_missing_or_extra_filename_sets() {
        let input = set(&["a.wav", "b.wav"]);
        let actual = set(&["a.wav", "c.wav"]);

        let error = validate_exact_filename_set("target-dir", &input, &actual)
            .expect_err("mismatched sets should fail")
            .to_string();

        assert!(error.contains("missing WAV files: b.wav"));
        assert!(error.contains("extra WAV files: c.wav"));
    }

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }
}
