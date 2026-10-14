//! Fake LSP server binary for tests. `--full` selects full-document sync (default: incremental).
#[allow(dead_code)]
mod fake_server;

fn main() {
    let full = std::env::args().any(|a| a == "--full");
    let code = fake_server::run(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        fake_server::FakeOptions { full_sync: full, no_save: std::env::args().any(|a| a == "--no-save") },
    );
    std::process::exit(code);
}
