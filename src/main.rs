//! sdlc binary.

use clap::Parser;

#[tokio::main]
async fn main() {
    let config =
        match sdlc::Config::from_parts(sdlc::CliArgs::parse(), |key| std::env::var(key).ok()) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("sdlc: {err}");
                std::process::exit(1);
            }
        };

    if let Err(err) = sdlc::init_tracing() {
        eprintln!("sdlc: {err}");
        std::process::exit(1);
    }

    let listener = match sdlc::bind(&config).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("sdlc: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = sdlc::serve(config, listener).await {
        eprintln!("sdlc: {err}");
        std::process::exit(1);
    }
}
