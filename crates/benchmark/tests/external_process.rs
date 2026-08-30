#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

use tomlsmith_benchmark::{
    PRETTIER_PLUGIN_ENV, ProductId, ProductOperation, ProductRunner, ProductVersionSupport,
    TomlVersion, product_catalog,
};

#[test]
fn product_catalog_exposes_cross_language_end_to_end_entry_points() {
    let catalog = product_catalog();
    assert_eq!(
        catalog.iter().map(|product| product.id).collect::<Vec<_>>(),
        vec![
            ProductId::TomlSmith,
            ProductId::Tombi,
            ProductId::Taplo,
            ProductId::Prettier,
            ProductId::Dprint,
            ProductId::BurntSushiToml,
            ProductId::GoTomlTomll,
        ]
    );
    assert_eq!(
        catalog.iter().map(|product| product.required_version).collect::<Vec<_>>(),
        vec!["0.1.0", "1.4.1", "0.10.0", "3.9.6", "0.56.1", "1.6.0", "2.4.3"]
    );
    assert_eq!(
        catalog
            .iter()
            .find(|product| product.id == ProductId::TomlSmith)
            .map(|product| (product.display_name, product.implementation_language)),
        Some(("TomlSmith native CLI", "Rust"))
    );
    assert_eq!(
        catalog
            .iter()
            .find(|product| product.id == ProductId::Prettier)
            .unwrap()
            .implementation_language,
        "TypeScript"
    );
    assert_eq!(
        catalog
            .iter()
            .find(|product| product.id == ProductId::BurntSushiToml)
            .unwrap()
            .implementation_language,
        "Go"
    );
    assert_eq!(
        catalog.iter().find(|product| product.id == ProductId::Taplo).unwrap().toml_versions,
        &[TomlVersion::V1_0]
    );
    assert_eq!(
        catalog.iter().find(|product| product.id == ProductId::TomlSmith).unwrap().operations,
        &[ProductOperation::Check, ProductOperation::Format]
    );
    assert_eq!(
        catalog.iter().find(|product| product.id == ProductId::BurntSushiToml).unwrap().operations,
        &[ProductOperation::Check]
    );
    assert_eq!(
        catalog.iter().find(|product| product.id == ProductId::TomlSmith).unwrap().version_support,
        ProductVersionSupport::StrictSelectable
    );
    assert_eq!(
        catalog.iter().find(|product| product.id == ProductId::Tombi).unwrap().version_support,
        ProductVersionSupport::StrictSelectable
    );
    assert_eq!(
        catalog
            .iter()
            .find(|product| product.id == ProductId::BurntSushiToml)
            .unwrap()
            .version_support,
        ProductVersionSupport::CompatibleSubset
    );
    assert_eq!(
        catalog.iter().find(|product| product.id == ProductId::GoTomlTomll).unwrap().operations,
        &[ProductOperation::Format]
    );
    assert_eq!(serde_json::to_value(ProductId::TomlSmith).unwrap(), serde_json::json!("tomlsmith"));
    assert_eq!(
        serde_json::to_value(ProductId::BurntSushiToml).unwrap(),
        serde_json::json!("burntsushi-toml")
    );
}

#[test]
fn non_go_product_probes_reject_unpinned_versions() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("wrong-version-product-fake");
    write_executable(&binary, "#!/bin/sh\nprintf 'product 9.9.9\\n'\n");

    for (product_id, required_version) in [
        (ProductId::TomlSmith, "0.1.0"),
        (ProductId::Tombi, "1.4.1"),
        (ProductId::Taplo, "0.10.0"),
        (ProductId::Prettier, "3.9.6"),
        (ProductId::Dprint, "0.56.1"),
    ] {
        let error = ProductRunner::probe(product_id, &binary).unwrap_err();
        assert!(error.to_string().contains("9.9.9"), "{product_id:?}: {error}");
        assert!(error.to_string().contains(required_version), "{product_id:?}: {error}");
    }
}

#[test]
fn generic_product_runner_executes_a_cold_stdin_process_in_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("tombi-product-fake");
    write_fake_tombi(&binary, "1.4.1");
    let runner = ProductRunner::probe(ProductId::Tombi, &binary).unwrap();
    let source = "title=\"fixture\"\n";

    let output = runner
        .run(
            ProductOperation::Format,
            TomlVersion::V1_1,
            source,
            directory.path().join("product-isolation"),
        )
        .unwrap();

    assert_eq!(output.stdout, source.as_bytes());
    assert_eq!(fs::read_to_string(binary.with_extension("stdin")).unwrap(), source);
    assert_eq!(
        fs::read_to_string(binary.with_extension("argv")).unwrap(),
        "format --offline --quiet --stdin-filename fixture.toml -\n"
    );
    let config =
        fs::read_to_string(output.isolation_directory.unwrap().join("tombi.toml")).unwrap();
    assert!(config.contains("toml-version = \"v1.1.0\""));
    assert!(config.contains("enabled = false"));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn product_runner_measures_peak_rss_in_a_separate_process_run() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("tombi-memory-product-fake");
    write_fake_tombi(&binary, "1.4.1");
    let runner = ProductRunner::probe(ProductId::Tombi, &binary).unwrap();
    let isolation = runner
        .prepare_isolation(directory.path().join("memory-isolation"), TomlVersion::V1_1)
        .unwrap();

    let output = runner
        .run_prepared_with_peak_rss(
            ProductOperation::Check,
            TomlVersion::V1_1,
            "title = \"fixture\"\n",
            isolation,
        )
        .unwrap();

    assert!(output.peak_rss_bytes > 0);
    assert!(output.process.status_success);
    assert_eq!(
        fs::read_to_string(binary.with_extension("stdin")).unwrap(),
        "title = \"fixture\"\n"
    );
}

#[test]
fn generic_product_runner_drains_streaming_output_while_writing_large_stdin() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("tombi-streaming-product-fake");
    write_fake_tombi(&binary, "1.4.1");
    let runner = ProductRunner::probe(ProductId::Tombi, &binary).unwrap();
    let source = format!("payload = '''\n{}\n'''\n", "x".repeat(10 * 1024 * 1024));

    let output = runner
        .run(
            ProductOperation::Format,
            TomlVersion::V1_0,
            &source,
            directory.path().join("streaming-isolation"),
        )
        .unwrap();

    assert_eq!(output.stdout, source.as_bytes());
}

#[test]
fn product_runner_terminates_a_process_that_exceeds_its_explicit_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("tombi-timeout-product-fake");
    write_executable(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'tombi 1.4.1\\n'; exit 0; fi\nsleep 5\n",
    );
    let runner = ProductRunner::probe(ProductId::Tombi, &binary)
        .unwrap()
        .with_process_timeout(Duration::from_millis(50))
        .unwrap();
    let started = Instant::now();

    let error = runner
        .run(
            ProductOperation::Check,
            TomlVersion::V1_0,
            "title = \"fixture\"\n",
            directory.path().join("timeout-isolation"),
        )
        .unwrap_err();

    assert!(started.elapsed() < Duration::from_secs(2), "{error}");
    assert!(error.to_string().contains("timed out"), "{error}");
    assert!(error.to_string().contains("50 ms"), "{error}");
}

#[test]
fn generic_command_matrix_uses_official_stdin_entry_points() {
    let directory = tempfile::tempdir().unwrap();
    let go = directory.path().join("go-fake");
    write_fake_go_build_info(&go);
    let plugin_package = directory.path().join("prettier-plugin-toml");
    let plugin = fs::canonicalize(write_fake_prettier_plugin(&plugin_package, "2.0.6")).unwrap();
    let plugin_argument = format!("--plugin={}", plugin.display());

    let cases: Vec<(ProductId, &str, ProductOperation, Vec<&str>)> = vec![
        (
            ProductId::TomlSmith,
            "0.1.0",
            ProductOperation::Check,
            vec!["--toml-version", "1.0", "check", "-"],
        ),
        (
            ProductId::TomlSmith,
            "0.1.0",
            ProductOperation::Format,
            vec!["--toml-version", "1.0", "fmt", "-"],
        ),
        (
            ProductId::Tombi,
            "1.4.1",
            ProductOperation::Check,
            vec!["lint", "--offline", "--quiet", "--stdin-filename", "fixture.toml", "-"],
        ),
        (
            ProductId::Tombi,
            "1.4.1",
            ProductOperation::Format,
            vec!["format", "--offline", "--quiet", "--stdin-filename", "fixture.toml", "-"],
        ),
        (
            ProductId::Taplo,
            "0.10.0",
            ProductOperation::Check,
            vec!["lint", "--colors", "never", "--no-auto-config", "--no-schema", "-"],
        ),
        (
            ProductId::Taplo,
            "0.10.0",
            ProductOperation::Format,
            vec!["format", "--colors", "never", "--no-auto-config", "-"],
        ),
        (
            ProductId::Prettier,
            "3.9.6",
            ProductOperation::Format,
            vec![
                plugin_argument.as_str(),
                "--stdin-filepath=fixture.toml",
                "--no-config",
                "--no-editorconfig",
            ],
        ),
        (ProductId::BurntSushiToml, "1.6.0", ProductOperation::Check, vec!["-"]),
        (ProductId::GoTomlTomll, "2.4.3", ProductOperation::Format, Vec::new()),
    ];

    for (product_id, required_version, operation, expected) in cases {
        let binary = directory.path().join(format!("{}-version-fake", product_id.as_str()));
        write_executable(&binary, &format!("#!/bin/sh\nprintf 'product {required_version}\\n'\n"));
        let mut runner = if matches!(product_id, ProductId::BurntSushiToml | ProductId::GoTomlTomll)
        {
            ProductRunner::probe_with_go_binary(product_id, &binary, &go).unwrap()
        } else {
            ProductRunner::probe(product_id, &binary).unwrap()
        };
        if product_id == ProductId::Prettier {
            runner = runner.with_prettier_plugin(&plugin).unwrap();
        }
        let isolation =
            runner.prepare_isolation(directory.path().join("matrix"), TomlVersion::V1_0).unwrap();
        let actual = runner
            .command_arguments(operation, TomlVersion::V1_0, isolation)
            .unwrap()
            .into_iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{product_id:?} {operation:?}");
    }

    let binary = directory.path().join("dprint-version-fake");
    write_executable(&binary, "#!/bin/sh\nprintf 'dprint 0.56.1\\n'\n");
    let dprint = ProductRunner::probe(ProductId::Dprint, &binary).unwrap();
    let isolation =
        dprint.prepare_isolation(directory.path().join("matrix"), TomlVersion::V1_1).unwrap();
    let dprint_arguments =
        dprint.command_arguments(ProductOperation::Format, TomlVersion::V1_1, &isolation).unwrap();
    assert_eq!(dprint_arguments[0], "fmt");
    assert_eq!(dprint_arguments[1], "--config");
    assert_eq!(dprint_arguments[2], isolation.join("dprint.json"));
    assert!(fs::read_to_string(isolation.join("dprint.json")).unwrap().contains("toml-0.8.0.wasm"));
}

#[test]
fn go_product_probe_always_checks_embedded_module_identity() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("tomlv-fake");
    write_executable(&binary, "#!/bin/sh\nprintf 'tomlv 1.6.0\\n'\n");
    let go = directory.path().join("go-fake");
    write_executable(
        &go,
        "#!/bin/sh\nprintf '%s\\n' \"$3: go1.25.0\" '\tmod\tgithub.com/example/not-toml\tv1.6.0\th1:fake'\n",
    );

    let error =
        ProductRunner::probe_with_go_binary(ProductId::BurntSushiToml, binary, go).unwrap_err();

    assert!(error.to_string().contains("github.com/BurntSushi/toml"), "{error}");
    assert!(error.to_string().contains("v1.6.0"), "{error}");
}

#[test]
fn prettier_companion_rejects_an_unpinned_package_version() {
    let directory = tempfile::tempdir().unwrap();
    let prettier = directory.path().join("prettier-fake");
    write_executable(&prettier, "#!/bin/sh\nprintf '3.9.6\\n'\n");
    let package = directory.path().join("prettier-plugin-toml");
    write_fake_prettier_plugin(&package, "2.0.5");

    let runner = ProductRunner::probe(ProductId::Prettier, prettier).unwrap();
    let error = runner.with_prettier_plugin(package).unwrap_err();

    assert!(error.to_string().contains("2.0.6"), "{error}");
    assert!(error.to_string().contains("2.0.5"), "{error}");
}

#[test]
fn prettier_companion_accepts_package_or_entry_and_records_content_identity() {
    let directory = tempfile::tempdir().unwrap();
    let prettier = directory.path().join("prettier-fake");
    write_executable(&prettier, "#!/bin/sh\nprintf '3.9.6\\n'\n");
    let package = directory.path().join("prettier-plugin-toml");
    let entry = write_fake_prettier_plugin(&package, "2.0.6");
    let expected_package_json = fs::canonicalize(package.join("package.json")).unwrap();
    let expected_entry = fs::canonicalize(&entry).unwrap();

    for configured_path in [&package, &entry] {
        let runner = ProductRunner::probe(ProductId::Prettier, &prettier)
            .unwrap()
            .with_prettier_plugin(configured_path)
            .unwrap();
        assert_eq!(runner.prettier_plugin(), Some(expected_entry.as_path()));

        let status = runner.status();
        assert_eq!(status.companion_path.as_deref(), Some(expected_entry.as_path()));
        assert_eq!(status.companion_version.as_deref(), Some("2.0.6"));
        assert_eq!(
            status.companion_package_json_path.as_deref(),
            Some(expected_package_json.as_path())
        );
        assert_eq!(
            status.companion_package_json_sha256.as_deref(),
            Some("6e39a15498777a1307ca3f7b1a87b4ae21efff176f83c01974e881cd7ff89a84")
        );
        assert_eq!(
            status.companion_entry_sha256.as_deref(),
            Some("450f0af4f4c1ecc4c7180f2e364c8a59bfed69dd350fb6b47bce8641c2a37786")
        );
    }
}

#[test]
fn prettier_companion_rejects_a_standalone_javascript_path() {
    let directory = tempfile::tempdir().unwrap();
    let prettier = directory.path().join("prettier-fake");
    write_executable(&prettier, "#!/bin/sh\nprintf '3.9.6\\n'\n");
    let standalone = directory.path().join("not-an-installed-plugin.mjs");
    fs::write(&standalone, "export default {};\n").unwrap();

    let runner = ProductRunner::probe(ProductId::Prettier, prettier).unwrap();
    let error = runner.with_prettier_plugin(standalone).unwrap_err();

    assert!(error.to_string().contains("package.json"), "{error}");
}

#[test]
fn explicit_product_selection_distinguishes_skip_from_invalid_paths() {
    assert!(ProductRunner::from_explicit_paths(ProductId::Taplo, None, None).unwrap().is_none());
    let error = ProductRunner::from_explicit_paths(ProductId::Taplo, Some("taplo".into()), None)
        .unwrap_err();
    assert!(error.to_string().contains("must be absolute"), "{error}");

    let directory = tempfile::tempdir().unwrap();
    let prettier = directory.path().join("prettier-fake");
    write_executable(&prettier, "#!/bin/sh\nprintf '3.9.6\\n'\n");
    let error =
        ProductRunner::from_explicit_paths(ProductId::Prettier, Some(prettier), None).unwrap_err();
    assert!(error.to_string().contains(PRETTIER_PLUGIN_ENV), "{error}");
}

fn write_fake_tombi(path: &std::path::Path, version: &str) {
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'tombi {version} (fake-target)\n'
  exit 0
fi
printf '%s\n' "$*" >> "$0.argv"
case "$1" in
  lint)
    cat > "$0.stdin"
    printf 'lint consumed stdin\n' >&2
    ;;
  format)
    tee "$0.stdin"
    ;;
  *)
    exit 9
    ;;
esac
"#
    );
    fs::write(path, script).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_executable(path: &std::path::Path, source: &str) {
    fs::write(path, source).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_fake_go_build_info(path: &std::path::Path) {
    write_executable(
        path,
        r#"#!/bin/sh
case "$3" in
  *burntsushi-toml*)
    module='github.com/BurntSushi/toml'
    version='v1.6.0'
    ;;
  *go-toml-tomll*)
    module='github.com/pelletier/go-toml/v2'
    version='v2.4.3'
    ;;
  *)
    exit 9
    ;;
esac
printf '%s\n' "$3: go1.25.0"
printf '\tmod\t%s\t%s\th1:fake\n' "$module" "$version"
"#,
    );
}

fn write_fake_prettier_plugin(path: &std::path::Path, version: &str) -> std::path::PathBuf {
    let library = path.join("lib");
    fs::create_dir_all(&library).unwrap();
    fs::write(
        path.join("package.json"),
        format!(
            "{{\n  \"name\": \"prettier-plugin-toml\",\n  \"version\": \"{version}\",\n  \"module\": \"./lib/index.js\"\n}}\n"
        ),
    )
    .unwrap();
    let entry = library.join("index.js");
    fs::write(&entry, "export default {};\n").unwrap();
    entry
}
