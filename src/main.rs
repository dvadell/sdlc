//! sdlc binary.

fn main() {
    if let Err(err) = sdlc::run() {
        eprintln!("sdlc: {err}");
        std::process::exit(1);
    }
}
