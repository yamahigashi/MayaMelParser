#![forbid(unsafe_code)]

mod app;
mod args;
mod diagnostics;
mod report;
#[cfg(test)]
mod tests;

fn main() {
    match app::run() {
        Ok(()) => {}
        Err(app::RunError::Cli(error)) => error.exit(),
        Err(app::RunError::Message(error)) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
