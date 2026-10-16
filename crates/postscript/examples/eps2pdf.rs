//! `cargo run -p designcraft-postscript --example eps2pdf -- in.eps out.pdf`: convert one EPS.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(input), Some(output)) = (args.first(), args.get(1)) else {
        eprintln!("usage: eps2pdf in.eps out.pdf");
        return std::process::ExitCode::FAILURE;
    };
    let bytes = match std::fs::read(input) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{input}: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let t = std::time::Instant::now();
    match designcraft_postscript::eps_to_pdf(&bytes, designcraft_postscript::Limits::default()) {
        Ok(c) => {
            eprintln!(
                "{input}: {:.0} ms, bbox {:?}, {} warnings, {} steps, {} KiB",
                t.elapsed().as_secs_f64() * 1000.0,
                c.bbox,
                c.warnings.len(),
                c.steps,
                c.memory / 1024
            );
            for w in c.warnings.iter().take(20) {
                eprintln!("  {w}");
            }
            if let Err(e) = std::fs::write(output, &c.pdf) {
                eprintln!("{output}: {e}");
                return std::process::ExitCode::FAILURE;
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{input}: {e} ({:.0} ms)", t.elapsed().as_secs_f64() * 1000.0);
            std::process::ExitCode::FAILURE
        }
    }
}
