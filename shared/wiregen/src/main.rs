use std::process::ExitCode;

fn main() -> ExitCode {
    let verify = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--verify") => true,
        Some(_) => {
            eprintln!("usage: wiregen [--verify]");
            return ExitCode::from(2);
        }
    };
    if verify {
        let stale = wiregen::stale();
        for path in &stale {
            eprintln!("stale: {path} (run `cargo run -p wiregen`)");
        }
        return if stale.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let root = wiregen::repo_root();
    for output in wiregen::outputs() {
        if let Err(err) = std::fs::write(root.join(output.path), &output.contents) {
            eprintln!("{}: {err}", output.path);
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
