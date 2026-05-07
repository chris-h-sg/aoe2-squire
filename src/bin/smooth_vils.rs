use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use csv::{ReaderBuilder, WriterBuilder, StringRecord};
use rts_analyzer::analysis::smoothing;
use rts_analyzer::constants::{CAPTURE_INTERVAL_MS, SMOOTH_VILS_MIN_SPIKE, SMOOTH_VILS_MAX_DURATION_MS};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: smooth_vils <input_csv> [output_csv] [min_spike] [max_duration_ms]");
        eprintln!("Default: output_csv=output/<input_stem>_smoothed.csv, min_spike={}, max_duration_ms={}", 
            SMOOTH_VILS_MIN_SPIKE, SMOOTH_VILS_MAX_DURATION_MS);
        std::process::exit(1);
    }

    let input_path = Path::new(&args[1]);
    
    let mut arg_idx = 2;
    let output_path = if args.len() > arg_idx && args[arg_idx].parse::<i32>().is_err() {
        let p = PathBuf::from(&args[arg_idx]);
        arg_idx += 1;
        p
    } else {
        let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("telemetry");
        let out_dir = Path::new("output");
        if !out_dir.exists() {
            std::fs::create_dir_all(out_dir)?;
        }
        out_dir.join(format!("{}_smoothed.csv", stem))
    };

    let min_spike = args.get(arg_idx).and_then(|s| s.parse::<i32>().ok()).unwrap_or(SMOOTH_VILS_MIN_SPIKE);
    let max_duration_ms = args.get(arg_idx + 1).and_then(|s| s.parse::<u64>().ok()).unwrap_or(SMOOTH_VILS_MAX_DURATION_MS);

    // Convert duration from ms to frames
    let max_duration_frames = (max_duration_ms / CAPTURE_INTERVAL_MS) as usize;

    println!("Input: {}", input_path.display());
    println!("Output: {}", output_path.display());
    println!("Settings: min_spike={}, max_duration_ms={} ({} frames)", 
        min_spike, max_duration_ms, max_duration_frames);

    let mut rdr = ReaderBuilder::new().from_path(input_path)?;
    let headers = rdr.headers()?.clone();
    
    let pop_vils_idx = headers.iter().position(|h| h == "pop_vils")
        .ok_or("CSV must have a 'pop_vils' column")?;

    let mut records: Vec<StringRecord> = Vec::new();
    for result in rdr.records() {
        records.push(result?);
    }

    if records.is_empty() {
        println!("Empty CSV, nothing to do.");
        return Ok(());
    }

    // Extract values for smoothing (as u32)
    let mut values: Vec<Option<u32>> = records.iter()
        .map(|r| r.get(pop_vils_idx).and_then(|s| s.parse::<u32>().ok()))
        .collect();

    // Use the shared smoothing logic
    let smoothed_count = smoothing::smooth_sequence(&mut values, min_spike, max_duration_frames);

    let mut wtr = WriterBuilder::new().from_path(&output_path)?;
    wtr.write_record(&headers)?;
    for (idx, record) in records.iter().enumerate() {
        let mut final_record = Vec::new();
        for (col_idx, field) in record.iter().enumerate() {
            if col_idx == pop_vils_idx {
                final_record.push(values[idx].map(|v| v.to_string()).unwrap_or_else(|| field.to_string()));
            } else {
                final_record.push(field.to_string());
            }
        }
        wtr.write_record(&final_record)?;
    }
    wtr.flush()?;

    println!("Smoothed {} outliers in 'pop_vils'.", smoothed_count);
    Ok(())
}
