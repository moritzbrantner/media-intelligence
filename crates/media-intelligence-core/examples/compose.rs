//! Composes a `CompositionInput` JSON file and prints the resulting `EvidenceAnalysis`.
//!
//! ```text
//! cargo run -p media-intelligence-core --example compose -- \
//!     crates/media-intelligence-core/tests/fixtures/lecture_composition.json
//! ```

use std::process::ExitCode;

use media_intelligence_core::{compose, CompositionInput};

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: compose <composition-input.json>");
        return ExitCode::from(2);
    };
    let input: CompositionInput = match std::fs::read_to_string(&path)
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
    {
        Ok(input) => input,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::from(2);
        }
    };
    match compose(&input) {
        Ok(analysis) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&analysis).expect("analysis serializes")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("composition rejected: {error}");
            ExitCode::FAILURE
        }
    }
}
