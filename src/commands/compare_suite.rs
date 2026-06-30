use std::error::Error;

use crate::{args::CompareSuiteArgs, compare_suite};

pub fn run(args: CompareSuiteArgs) -> Result<(), Box<dyn Error>> {
    compare_suite::run(args)
}
