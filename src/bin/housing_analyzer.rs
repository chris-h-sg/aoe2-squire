use aoe2_squire::analysis::{housing, load_merged_observations};
use aoe2_squire::types::MergedRow;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();

    let verbose = args
        .iter()
        .any(|arg| arg == "--verbose" || arg == "-v" || arg == "--v");

    let path_arg = args.iter().skip(1).find(|arg| !arg.starts_with("-"));

    let rows = if let Some(path) = path_arg {
        let mut rdr = csv::ReaderBuilder::new().from_path(path)?;
        rdr.deserialize()
            .filter_map(|r: Result<MergedRow, _>| r.ok())
            .collect::<Vec<_>>()
    } else {
        let mut rdr = load_merged_observations()?;
        rdr.deserialize()
            .filter_map(|r: Result<MergedRow, _>| r.ok())
            .collect::<Vec<_>>()
    };

    housing::analyze_housing(rows.into_iter(), verbose)?;

    Ok(())
}
