use std::{env, path::PathBuf};

use maison_backend::TempoService;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut persist = true;
    let mut seasons = Vec::new();

    for arg in args {
        if arg == "--no-persist" {
            persist = false;
        } else {
            seasons.push(arg);
        }
    }

    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("backend has parent")
        .to_path_buf();
    let service = TempoService::new(source_root)?;
    let report = service.recalibrate(&seasons, persist).await?;

    println!("Calibration rebuilt for seasons: {}", report.seasons.join(", "));
    println!("accuracy={:.3}", report.params.calibration_accuracy);
    println!("red_recall={:.3}", report.params.calibration_red_recall);
    println!("white_recall={:.3}", report.params.calibration_white_recall);
    println!("macro_f1={:.3}", report.params.calibration_macro_f1);
    println!("samples={}", report.params.calibration_sample_count);
    println!("base_consumption={:.1}", report.params.base_consumption);
    println!("thermosensitivity={:.1}", report.params.thermosensitivity);
    println!("temp_reference={:.2}", report.params.temp_reference);
    println!("weekend_factor={:.3}", report.params.weekend_factor);
    println!("red_threshold_offset={:.3}", report.params.red_threshold_offset);
    println!("white_threshold_offset={:.3}", report.params.white_threshold_offset);
    println!("red_probability_scale={:.3}", report.params.red_probability_scale);
    println!("white_probability_scale={:.3}", report.params.white_probability_scale);

    if persist {
        println!("Saved to cache/tempo/calibration_params.json");
    } else {
        println!("Dry run only; calibration file not updated");
    }

    Ok(())
}
