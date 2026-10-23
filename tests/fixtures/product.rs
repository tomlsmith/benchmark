// A portable product double compiled into a temporary directory by Vitest.
// Assertions and lifecycle checks belong to the TypeScript test suite.
use std::{
    env, fs,
    io::{self, Read, Write},
    process::{self, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

fn main() {
    if env::args().any(|argument| argument == "--version") {
        println!("tomlsmith 0.4.0");
        return;
    }
    let state = env::var("TOMLSMITH_TEST_STATE").unwrap();
    let child = env::args().any(|argument| argument == "--fixture-child");
    let role = if child { "descendant" } else { "parent" };
    fs::write(format!("{state}/{role}.pid"), process::id().to_string()).unwrap();
    if child {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(30) {
            // SIGKILL may arrive between truncating and writing a file. Publish atomically so
            // the last completed heartbeat remains observable after successful termination.
            fs::write(format!("{state}/heartbeat-next"), started.elapsed().as_nanos().to_string())
                .unwrap();
            fs::rename(format!("{state}/heartbeat-next"), format!("{state}/heartbeat")).unwrap();
            thread::sleep(Duration::from_millis(10));
        }
        return;
    }

    let mode = env::var("TOMLSMITH_TEST_MODE").unwrap();
    if mode == "normal" {
        // Fill stderr before consuming stdin to exercise concurrent draining.
        io::stderr().write_all(&vec![b'x'; 128 * 1024]).unwrap();
        io::copy(&mut io::stdin(), &mut io::stdout()).unwrap();
        return;
    }
    if mode == "failure" {
        let mut input = Vec::new();
        io::stdin().read_to_end(&mut input).unwrap();
        eprintln!("fixture command failed");
        process::exit(23);
    }

    if mode == "late-exit" {
        thread::sleep(Duration::from_millis(700));
    }
    let mut command = Command::new(env::current_exe().unwrap());
    command.arg("--fixture-child");
    if mode == "hold-stdin" {
        command.stdin(Stdio::inherit()).stdout(Stdio::null()).stderr(Stdio::null());
    } else if matches!(mode.as_str(), "hold-stdout" | "late-exit" | "escape-group") {
        command.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::null());
    } else if mode == "hold-stderr" {
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit());
    } else {
        command.stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }
    #[cfg(unix)]
    if mode == "escape-group" {
        command.process_group(0);
    }
    let _descendant = command.spawn().unwrap();
    eprintln!("fixture stderr before timeout");
    if mode == "live-parent" {
        thread::sleep(Duration::from_secs(30));
    }
    // In the other modes the direct process exits successfully; its descendant keeps a pipe open.
}
