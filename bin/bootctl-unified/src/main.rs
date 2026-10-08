//! The standalone generic-bootctl module binary: its payload method (`--install-plan`,
//! `--vendor-probe`) plus serving the property-gated native backends.
mod install;
mod vintf;

use std::path::Path;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--install-plan") => {
            install::install_plan(Path::new(args.get(2).map(String::as_str).unwrap_or("/")))?;
            Ok(())
        }
        Some("--vendor-probe") => {
            generic_bootctl_backends::vendor::probe(Path::new(
                args.get(2).ok_or("missing library")?,
            ))?;
            Ok(())
        }
        Some(_) => Err("unexpected arguments".into()),
        None => bootctl_unified::serve(generic_bootctl_backends::service()?),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("generic-bootctl: {e}");
        std::process::exit(1);
    }
}
