use tomlsmith_benchmark::generate_corpus;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

const ISOLATED_CLI_ENVIRONMENT: &[&str] = &[
    "TOMLSMITH_BIN",
    "TOMLSMITH_TOMBI_BIN",
    "TOMLSMITH_TAPLO_BIN",
    "TOMLSMITH_PRETTIER_BIN",
    "TOMLSMITH_PRETTIER_PLUGIN",
    "TOMLSMITH_DPRINT_BIN",
    "TOMLSMITH_BURNTSUSHI_TOMLV_BIN",
    "TOMLSMITH_GO_TOMLL_BIN",
    "TOMLSMITH_GO_BIN",
    "DPRINT_CACHE_DIR",
    "TOMLSMITH_BENCH_FILTER",
    "TOMLSMITH_BENCH_WARMUP_SECS",
    "TOMLSMITH_BENCH_MEASUREMENT_SECS",
    "TOMLSMITH_BENCH_SAMPLE_SIZE",
    "TOMLSMITH_BENCH_RESULT_ROOT",
    "TOMLSMITH_BENCH_CARGO_COMMAND",
    "TOMLSMITH_BENCH_RUSTC_COMMAND",
    "TOMLSMITH_BENCH_NODE_COMMAND",
    "TOMLSMITH_BENCH_TIME_COMMAND",
    "CARGO_INCREMENTAL",
    "CARGO_BUILD_INCREMENTAL",
];

fn isolated_cli_command() -> assert_cmd::Command {
    let mut command = assert_cmd::cargo::cargo_bin_cmd!("tomlsmith-benchmark");
    for variable in ISOLATED_CLI_ENVIRONMENT {
        command.env_remove(variable);
    }
    for (variable, _) in std::env::vars_os() {
        if variable.to_string_lossy().starts_with("CARGO_PROFILE_BENCH_") {
            command.env_remove(variable);
        }
    }
    command
}

#[test]
fn list_json_exposes_machine_readable_adapters_and_fixtures() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    let output = isolated_cli_command()
        .args(["--root", directory.path().to_str().unwrap(), "list", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["adapters"].as_array().unwrap().len(), 4);
    assert_eq!(json["fixtures"].as_array().unwrap().len(), 19);
    let tombi = json["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|product| product["descriptor"]["id"] == "tombi")
        .unwrap();
    assert_eq!(tombi["availability"]["status"], "skipped");
    assert!(
        json["adapters"]
            .as_array()
            .unwrap()
            .iter()
            .all(|adapter| adapter["version_mode"].is_object() && adapter["seams"].is_array())
    );
    assert_eq!(json["products"].as_array().unwrap().len(), 7);
    assert!(json["products"].as_array().unwrap().iter().all(|product| {
        product["descriptor"]["operations"].is_array()
            && product["descriptor"]["binary_env"].is_string()
    }));
}

#[test]
fn verify_json_reports_the_selected_comparable_case_and_succeeds() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    let mut command = isolated_cli_command();
    command.env("TOMLSMITH_BENCH_FILTER", "e2e/check/cold-stdin/1.0/v1_0_small");
    let output = command
        .args(["--root", directory.path().to_str().unwrap(), "verify", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["passed"], true);
    assert_eq!(json["fixture_count"], 1);
    assert!(!json["cases"].as_array().unwrap().is_empty());
    assert_eq!(json["failures"].as_array().unwrap().len(), 0);
    assert!(!json["semantic_equivalence"].as_array().unwrap().is_empty());
    assert!(!json["formatter_invariants"].as_array().unwrap().is_empty());
    assert_eq!(json["products"]["statuses"].as_array().unwrap().len(), 7);
    assert!(
        json["products"]["statuses"]
            .as_array()
            .unwrap()
            .iter()
            .all(|product| { product["availability"]["status"] == "skipped" })
    );
    assert!(json["products"]["cases"].as_array().unwrap().is_empty());
}

#[test]
fn env_json_captures_reproduction_metadata_and_benchmark_settings() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    let output = isolated_cli_command()
        .args(["--root", directory.path().to_str().unwrap(), "env", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert!(json["os"].as_str().unwrap().len() > 1);
    assert!(json["arch"].as_str().unwrap().len() > 1);
    assert!(json["logical_cpus"].as_u64().unwrap() >= 1);
    assert_eq!(json["benchmark_settings"]["sample_size"], 30);
    assert_eq!(json["benchmark_settings"]["warm_up_seconds"], 3.0);
    assert_eq!(json["benchmark_settings"]["measurement_seconds"], 5.0);
    assert_eq!(json["benchmark_settings"]["process_timeout_seconds"], 120);
    assert_eq!(json["corpus_manifest_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(json["cargo_lock_sha256"].as_str().unwrap().len(), 64);
    assert!(!json["build"]["target_triple"].as_str().unwrap().is_empty());
    assert_eq!(json["build"]["cargo_profile"]["name"], "bench");
    assert_eq!(json["build"]["metadata_collector_profile"], "debug");
    assert_eq!(json["build"]["allocator"], "rust_std_default_system");
    assert!(json["power"]["unavailable_reason"].is_string());
    assert!(json["benchmark_settings"]["filter"].is_null());
    assert_eq!(json["products"].as_array().unwrap().len(), 7);
    assert!(
        json["products"]
            .as_array()
            .unwrap()
            .iter()
            .all(|product| { product["availability"]["status"] == "skipped" })
    );
    assert_git_metadata_is_consistent(&json["benchmark_git"]);
    assert!(json["rustc"]["version_verbose"].as_str().unwrap().contains("rustc"));
    assert!(json["runtimes"]["go"].is_null());
    assert!(json["runtimes"]["node"].is_null());
}

#[cfg(all(unix, any(target_os = "linux", target_os = "macos")))]
#[test]
fn peak_rss_json_reports_separate_fresh_process_samples() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let tomlsmith = directory.path().join("tomlsmith-product");
    write_fake_tool(&tomlsmith, "tomlsmith 0.3.0");

    let output = isolated_cli_command()
        .args([
            "--root",
            directory.path().to_str().unwrap(),
            "peak-rss",
            "--fixture",
            "v1_0_small",
            "--operation",
            "check",
            "--samples",
            "3",
            "--json",
        ])
        .env("TOMLSMITH_BIN", &tomlsmith)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["fixture_id"], "v1_0_small");
    assert_eq!(json["operation"], "check");
    assert_eq!(json["input_bytes"], 4_096);
    assert_eq!(json["samples"], 3);
    assert_eq!(json["cases"].as_array().unwrap().len(), 1);
    assert_eq!(json["cases"][0]["product_id"], "tomlsmith");
    assert_eq!(json["cases"][0]["peak_rss_bytes"].as_array().unwrap().len(), 3);
    assert!(json["cases"][0]["median_peak_rss_bytes"].as_u64().unwrap() > 0);
    assert!(json["cases"][0]["max_peak_rss_bytes"].as_u64().unwrap() > 0);
}

#[cfg(unix)]
#[test]
fn env_json_records_runtimes_required_by_enabled_product_clis() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    let go_product = directory.path().join("burntsushi-toml-product");
    let prettier = directory.path().join("prettier-product");
    let go = directory.path().join("selected-go");
    let node = directory.path().join("selected-node");
    let prettier_plugin = directory.path().join("prettier-plugin-toml");
    write_fake_tool(&go_product, "unused product version output");
    write_fake_tool(&prettier, "3.9.6");
    write_fake_go(&go);
    write_fake_tool(&node, "v24.7.0");
    write_fake_prettier_plugin(&prettier_plugin);

    let output = isolated_cli_command()
        .args(["--root", directory.path().to_str().unwrap(), "env", "--json"])
        .env("TOMLSMITH_BURNTSUSHI_TOMLV_BIN", &go_product)
        .env("TOMLSMITH_GO_BIN", &go)
        .env("TOMLSMITH_PRETTIER_BIN", &prettier)
        .env("TOMLSMITH_PRETTIER_PLUGIN", &prettier_plugin)
        .env("TOMLSMITH_BENCH_NODE_COMMAND", &node)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["runtimes"]["go"]["command"], go.to_string_lossy().as_ref());
    assert_eq!(json["runtimes"]["go"]["version_verbose"], "go version go1.27.0 test");
    assert_eq!(json["runtimes"]["node"]["command"], node.to_string_lossy().as_ref());
    assert_eq!(json["runtimes"]["node"]["version_verbose"], "v24.7.0");
}

fn assert_git_metadata_is_consistent(metadata: &serde_json::Value) {
    let dirty = metadata["dirty"].as_bool().expect("Git dirty status should be available");
    let dirty_diff_sha256 = &metadata["dirty_diff_sha256"];
    if dirty {
        assert_eq!(dirty_diff_sha256.as_str().map(str::len), Some(64));
    } else {
        assert!(dirty_diff_sha256.is_null());
    }
}

#[test]
fn env_rejects_non_finite_benchmark_durations() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    for invalid in ["NaN", "inf", "-inf"] {
        let output = isolated_cli_command()
            .args(["--root", directory.path().to_str().unwrap(), "env", "--json"])
            .env("TOMLSMITH_BENCH_WARMUP_SECS", invalid)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{invalid} unexpectedly succeeded");
        assert!(String::from_utf8_lossy(&output.stderr).contains("finite"));
    }
}

#[test]
fn env_rejects_every_cargo_bench_profile_override() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    for (name, value) in [
        ("CARGO_PROFILE_BENCH_OPT_LEVEL", "1"),
        ("CARGO_PROFILE_BENCH_FUTURE_FIELD", "surprise"),
        ("CARGO_INCREMENTAL", "1"),
        ("CARGO_BUILD_INCREMENTAL", "true"),
    ] {
        let output = isolated_cli_command()
            .args(["--root", directory.path().to_str().unwrap(), "env", "--json"])
            .env(name, value)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(name), "{stderr}");
        assert!(stderr.contains("not allowed"), "{stderr}");
    }
}

#[cfg(unix)]
#[test]
fn env_reports_the_exact_cargo_and_rustc_commands_selected_by_the_runner() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let cargo = directory.path().join("selected-cargo");
    let rustc = directory.path().join("selected-rustc");
    write_fake_tool(&cargo, "selected cargo 9.9.9");
    write_fake_tool(&rustc, "selected rustc 8.8.8");

    let output = isolated_cli_command()
        .args(["--root", directory.path().to_str().unwrap(), "env", "--json"])
        .env("TOMLSMITH_BENCH_CARGO_COMMAND", &cargo)
        .env("TOMLSMITH_BENCH_RUSTC_COMMAND", &rustc)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["cargo"]["command"], cargo.to_string_lossy().as_ref());
    assert_eq!(json["cargo"]["version_verbose"], "selected cargo 9.9.9");
    assert_eq!(json["rustc"]["command"], rustc.to_string_lossy().as_ref());
    assert_eq!(json["rustc"]["version_verbose"], "selected rustc 8.8.8");
}

#[test]
fn run_bench_script_rejects_extra_criterion_arguments() {
    let workspace_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let output = std::process::Command::new("bash")
        .arg(workspace_root.join("scripts/run-bench.sh"))
        .args(["test-run", "--sample-size", "10"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not accept Criterion"));
}

#[test]
fn run_bench_requires_one_exact_latency_and_peak_rss_lane() {
    let workspace_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
    let output = std::process::Command::new("bash")
        .arg(workspace_root.join("scripts/run-bench.sh"))
        .arg("non-exact-filter")
        .env("TOMLSMITH_BENCH_FILTER", "e2e/check/cold-stdin/1.0")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must select one exact"));
}

#[cfg(unix)]
#[test]
fn run_bench_rejects_empty_csv_results_and_removes_staging_directory() {
    let directory = tempfile::tempdir().unwrap();
    let fake_cargo = directory.path().join("fake-cargo");
    write_fake_cargo(&fake_cargo);
    let result_root = directory.path().join("results");
    let workspace_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();

    let output = std::process::Command::new("bash")
        .arg(workspace_root.join("scripts/run-bench.sh"))
        .arg("zero-match")
        .env("CARGO", &fake_cargo)
        .env("TOMLSMITH_BENCH_RESULT_ROOT", &result_root)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Criterion raw.csv"));
    assert!(!result_root.join("zero-match").exists());
    assert!(!result_root.join(".zero-match.lock").exists());
    assert!(
        std::fs::read_dir(&result_root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("staging"))
    );
}

#[cfg(unix)]
#[test]
fn run_bench_publishes_staging_directory_only_after_csv_exists() {
    let directory = tempfile::tempdir().unwrap();
    let fake_cargo = directory.path().join("fake-cargo");
    write_fake_cargo(&fake_cargo);
    let result_root = directory.path().join("results");
    let workspace_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();

    let output = std::process::Command::new("bash")
        .arg(workspace_root.join("scripts/run-bench.sh"))
        .arg("complete-run")
        .current_dir(directory.path())
        .env("CARGO", &fake_cargo)
        .env("FAKE_CARGO_CREATE_CSV", "1")
        .env("FAKE_CARGO_EXPECT_CWD", workspace_root)
        .env("TOMLSMITH_BENCH_RESULT_ROOT", &result_root)
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let final_directory = result_root.join("complete-run");
    assert!(final_directory.join("criterion/fake/new/raw.csv").is_file());
    assert_eq!(
        std::fs::read_to_string(final_directory.join("csv-files.txt")).unwrap(),
        "criterion/fake/new/raw.csv\n"
    );
    assert!(final_directory.join("peak-rss.json").is_file());
    assert!(!result_root.join(".complete-run.lock").exists());
    assert!(
        std::fs::read_dir(&result_root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("staging"))
    );
}

#[cfg(unix)]
#[test]
fn run_bench_keeps_latency_and_peak_rss_on_the_same_lane() {
    let directory = tempfile::tempdir().unwrap();
    let fake_cargo = directory.path().join("fake-cargo");
    write_fake_cargo(&fake_cargo);
    let command_log = directory.path().join("cargo-commands.log");
    let result_root = directory.path().join("results");
    let workspace_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();

    let output = std::process::Command::new("bash")
        .arg(workspace_root.join("scripts/run-bench.sh"))
        .arg("aligned-run")
        .env("CARGO", &fake_cargo)
        .env("FAKE_CARGO_CREATE_CSV", "1")
        .env("FAKE_CARGO_LOG", &command_log)
        .env("TOMLSMITH_BENCH_FILTER", "e2e/format/cold-stdin/1.1/v1_1_small")
        .env("TOMLSMITH_BENCH_RESULT_ROOT", &result_root)
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let commands = std::fs::read_to_string(command_log).unwrap();
    assert!(
        commands.contains("peak-rss --fixture v1_1_small --operation format --samples 3 --json"),
        "{commands}"
    );
}

#[cfg(unix)]
#[test]
fn run_bench_rejects_a_preexisting_run_id_lock_without_touching_it() {
    let directory = tempfile::tempdir().unwrap();
    let fake_cargo = directory.path().join("fake-cargo");
    write_fake_cargo(&fake_cargo);
    let result_root = directory.path().join("results");
    let lock_directory = result_root.join(".concurrent-run.lock");
    std::fs::create_dir_all(&lock_directory).unwrap();
    let workspace_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();

    let output = std::process::Command::new("bash")
        .arg(workspace_root.join("scripts/run-bench.sh"))
        .arg("concurrent-run")
        .env("CARGO", &fake_cargo)
        .env("FAKE_CARGO_CREATE_CSV", "1")
        .env("TOMLSMITH_BENCH_RESULT_ROOT", &result_root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("run id is locked"));
    assert!(lock_directory.is_dir(), "a contender must not remove the owner's lock");
    assert!(!result_root.join("concurrent-run").exists());
    assert!(
        std::fs::read_dir(&result_root)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains("staging"))
    );
}

#[cfg(unix)]
fn write_fake_cargo(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ -n "${FAKE_CARGO_LOG:-}" ]; then
  printf '%s\n' "$*" >> "$FAKE_CARGO_LOG"
fi
case " $* " in
  *" bench "*)
    if [ "${FAKE_CARGO_CREATE_CSV:-}" = "1" ]; then
      mkdir -p "$CRITERION_HOME/fake/new"
      printf 'group,function,value\n' > "$CRITERION_HOME/fake/new/raw.csv"
    fi
    ;;
  *) printf '{}\n' ;;
esac
if [ -n "${FAKE_CARGO_EXPECT_CWD:-}" ] && [ "$PWD" != "$FAKE_CARGO_EXPECT_CWD" ]; then
  printf 'unexpected cwd: %s\n' "$PWD" >&2
  exit 91
fi
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn write_fake_tool(path: &std::path::Path, version: &str) {
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\ncase \"${{1:-}}\" in\n  --version|-Vv) printf '%s\\n' '{version}' ;;\n  *) cat >/dev/null ;;\nesac\n"
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn write_fake_go(path: &std::path::Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
if [ "$1" = "version" ] && [ "$2" = "-m" ]; then
  printf '%s: go1.27.0\n' "$3"
  printf '\tmod\tgithub.com/BurntSushi/toml\tv1.6.0\th1:test\n'
elif [ "$1" = "version" ]; then
  printf 'go version go1.27.0 test\n'
else
  exit 9
fi
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[cfg(unix)]
fn write_fake_prettier_plugin(path: &std::path::Path) {
    std::fs::create_dir_all(path.join("lib")).unwrap();
    std::fs::write(
        path.join("package.json"),
        "{\n  \"name\": \"prettier-plugin-toml\",\n  \"version\": \"2.0.6\",\n  \"module\": \"./lib/index.js\"\n}\n",
    )
    .unwrap();
    std::fs::write(path.join("lib/index.js"), "export default {};\n").unwrap();
}

#[test]
fn generate_json_writes_a_corpus_that_check_accepts() {
    let directory = tempfile::tempdir().unwrap();
    let generated = isolated_cli_command()
        .args(["--root", directory.path().to_str().unwrap(), "generate", "--json"])
        .output()
        .unwrap();
    assert!(generated.status.success(), "{}", String::from_utf8_lossy(&generated.stderr));
    let generated_json: serde_json::Value = serde_json::from_slice(&generated.stdout).unwrap();
    assert_eq!(generated_json["fixtures"].as_array().unwrap().len(), 19);

    let checked = isolated_cli_command()
        .args(["--root", directory.path().to_str().unwrap(), "generate", "--check", "--json"])
        .output()
        .unwrap();
    assert!(checked.status.success(), "{}", String::from_utf8_lossy(&checked.stderr));
    let checked_json: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked_json["matches"], true);
}
