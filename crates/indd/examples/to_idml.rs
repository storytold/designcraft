//! `cargo run -p designcraft-indd --example to_idml -- IN.indd OUT.idml`
#![allow(clippy::unwrap_used, clippy::expect_used)]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(src), Some(dst)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: to_idml IN.indd OUT.idml");
        std::process::exit(2);
    };
    let bytes = std::fs::read(src).expect("read input");
    let name = std::path::Path::new(src).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match designcraft_indd::to_idml_named(&bytes, &name) {
        Ok(idml) => std::fs::write(dst, idml).expect("write output"),
        Err(e) => {
            eprintln!("{src}: {e}");
            std::process::exit(1);
        }
    }
}
