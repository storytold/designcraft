//! File › Export PDF on the `designcraft-cli perf` stress document, timed.
//!
//! ```text
//! cargo run --release -p designcraft-cli --example pdf_export -- [--pages N] [--runs N] [--out FILE.pdf]
//! ```
//!
//! Builds the synthetic document (200 pages, 1M characters in threaded stories, 500 placed
//! images, gradients, a drop shadow per spread), opens it in a headless session and runs
//! `file.exportPdf` on all pages: one warm-up export (composition), then `--runs` timed ones.
//! Prints each run, the median and the minimum. `--out` keeps the last PDF, written with a fixed
//! creation date so two builds can be compared byte for byte.

use std::time::Instant;

use designcraft_cli::stress;

fn main() -> Result<(), String> {
    let mut spec = stress::Spec::default();
    let mut runs = 7usize;
    let mut out: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut num = || args.next().and_then(|v| v.parse::<usize>().ok()).ok_or(format!("{a} needs a number"));
        match a.as_str() {
            "--pages" => spec.pages = num()?,
            "--runs" => runs = num()?.max(1),
            "--out" => out = Some(args.next().ok_or("--out needs a file")?),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    let doc = stress::synthetic(&spec)?;
    let bytes = designcraft_format::save(&doc).map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join(format!("designcraft-pdf-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("stress.designcraft");
    let keep = out.is_some();
    let pdf = out.map_or_else(|| dir.join("stress.pdf"), std::path::PathBuf::from);
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let mut s = designcraft_engine::Session::new();
    s.execute("file.open", &serde_json::json!({"path": file})).map_err(|e| e.to_string())?;
    let params = serde_json::json!({"path": pdf, "created": 1_700_000_000});
    let mut export = || -> Result<f64, String> {
        let t = Instant::now();
        s.execute("file.exportPdf", &params).map_err(|e| e.to_string())?;
        Ok(t.elapsed().as_secs_f64() * 1000.0)
    };
    let warm = export()?;
    println!("{} pages, warm-up export (with composition) {warm:.1} ms", doc.page_count());
    let mut ms = Vec::with_capacity(runs);
    for i in 0..runs {
        let t = export()?;
        println!("run {}: {t:.1} ms", i + 1);
        ms.push(t);
    }
    ms.sort_by(f64::total_cmp);
    let size = std::fs::metadata(&pdf).map_or(0, |m| m.len());
    println!("median {:.1} ms, min {:.1} ms, {} kB", ms[ms.len() / 2], ms[0], size / 1024);
    let _ = std::fs::remove_file(&file);
    if !keep {
        let _ = std::fs::remove_file(&pdf);
    }
    let _ = std::fs::remove_dir(&dir);
    Ok(())
}
