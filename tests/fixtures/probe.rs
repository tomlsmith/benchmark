// Public-interface driver for Vitest; no test assertions or replacement probe logic.
use std::{env, process::{Command, ExitCode}};
use tomlsmith_benchmark::{ProductId, ProductRunner};

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let mode = arguments.next().unwrap();
    let binary = arguments.next().unwrap();
    if mode == "once" {
        // A control proving that Linux really rejects execution while the writer is open.
        return match Command::new(&binary).arg("--version").output() {
            Ok(output) if output.status.success() => ExitCode::SUCCESS,
            Ok(output) => {
                eprintln!("version command failed: {}", output.status);
                ExitCode::FAILURE
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
    match ProductRunner::probe(ProductId::TomlSmith, &binary) {
        Ok(runner) => {
            println!("{}", runner.version_output());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
