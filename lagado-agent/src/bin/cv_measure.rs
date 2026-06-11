/// cv_measure — Measurement binary for the CV box proposer.
///
/// Usage: cv_measure <image.png> [label]
///
/// Runs the proposer on every cell of the given PNG and prints per-cell box
/// counts (raw before filter, filtered after). Use this to measure junk-box
/// rate on real screenshots before committing to threshold values.
use lagado_agent::perception::cv_proposer::{extract_cell_rgb, propose_boxes};
use lagado_agent::perception::delta::{DeltaDetector, GRID_COLS, GRID_ROWS};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cv_measure <image.png> [label]");
        std::process::exit(1);
    }
    let path = &args[1];
    let label = args.get(2).map(String::as_str).unwrap_or(path.as_str());

    let png_bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to read {path}: {e}");
            std::process::exit(1);
        }
    };

    let img = match image::load_from_memory(&png_bytes) {
        Ok(i) => i.to_rgb8(),
        Err(e) => {
            eprintln!("Failed to decode PNG {path}: {e}");
            std::process::exit(1);
        }
    };

    let frame_w = img.width();
    let frame_h = img.height();
    let full_rgb = img.as_raw();

    println!("=== CV Proposer Measurement: {label} ===");
    println!("Frame: {frame_w}×{frame_h}  Grid: {GRID_COLS}×{GRID_ROWS}");
    println!("Thresholds: canny_low=15.0  canny_high=45.0  min_area=64px  max_area_frac=0.85  aspect=[0.05,20.0]");
    println!();
    println!("{:<10} {:>10} {:>12} {:>12} {:>10}",
        "cell", "cell_px", "raw_boxes", "filtered", "kept%");
    println!("{:-<60}", "");

    let mut total_raw = 0usize;
    let mut total_filtered = 0usize;
    let mut max_raw_cell = ("", 0usize);
    let mut max_filt_cell = ("", 0usize);

    let mut cell_ids = Vec::new();

    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            let cell_id = format!("c{row}_{col}");
            let (cx, cy, cw, ch) = DeltaDetector::cell_pixel_bounds(col, row, frame_w, frame_h);
            let cell_px = cw * ch;

            let cell_rgb = extract_cell_rgb(full_rgb, frame_w, cx, cy, cw, ch);
            let result = propose_boxes(&cell_rgb, cw, ch, (cx, cy));

            let raw = result.raw_count;
            let filt = result.boxes.len();
            let kept_pct = if raw > 0 { filt * 100 / raw } else { 0 };

            println!("{:<10} {:>10} {:>12} {:>12} {:>9}%",
                cell_id, cell_px, raw, filt, kept_pct);

            total_raw += raw;
            total_filtered += filt;

            if raw > max_raw_cell.1 { max_raw_cell = (Box::leak(cell_id.clone().into_boxed_str()), raw); }
            if filt > max_filt_cell.1 { max_filt_cell = (Box::leak(cell_id.clone().into_boxed_str()), filt); }
            cell_ids.push((cell_id, raw, filt));
        }
    }

    println!("{:-<60}", "");
    let total_kept_pct = if total_raw > 0 { total_filtered * 100 / total_raw } else { 0 };
    println!("{:<10} {:>10} {:>12} {:>12} {:>9}%",
        "TOTAL", frame_w * frame_h, total_raw, total_filtered, total_kept_pct);

    println!();
    println!("--- Distribution summary ---");
    let zero_raw = cell_ids.iter().filter(|(_, r, _)| *r == 0).count();
    let low_raw  = cell_ids.iter().filter(|(_, r, _)| *r > 0 && *r <= 10).count();
    let mid_raw  = cell_ids.iter().filter(|(_, r, _)| *r > 10 && *r <= 50).count();
    let high_raw = cell_ids.iter().filter(|(_, r, _)| *r > 50 && *r <= 200).count();
    let bliz_raw = cell_ids.iter().filter(|(_, r, _)| *r > 200).count();
    println!("Raw boxes per cell:   0={zero_raw}  1-10={low_raw}  11-50={mid_raw}  51-200={high_raw}  >200(blizzard)={bliz_raw}");

    let zero_f  = cell_ids.iter().filter(|(_, _, f)| *f == 0).count();
    let low_f   = cell_ids.iter().filter(|(_, _, f)| *f > 0 && *f <= 5).count();
    let mid_f   = cell_ids.iter().filter(|(_, _, f)| *f > 5 && *f <= 20).count();
    let high_f  = cell_ids.iter().filter(|(_, _, f)| *f > 20).count();
    println!("Filtered boxes/cell:  0={zero_f}  1-5={low_f}  6-20={mid_f}  >20={high_f}");
    println!("Highest raw:   {} ({} boxes)", max_raw_cell.0, max_raw_cell.1);
    println!("Highest filt:  {} ({} boxes)", max_filt_cell.0, max_filt_cell.1);
}
