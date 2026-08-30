use std::{collections::BTreeMap, fs};

use tomlsmith_benchmark::{
    Adapter, AdapterDescriptor, AdapterError, AdapterOutput, Availability, CanonicalValue,
    FixtureClass, FixtureCorpus, FixturePurpose, FormatState, Operation, SeamDescriptor,
    SourceKind, SyntaxProfile, TomlVersion, VersionMode, WorkloadShape, built_in_catalog,
    generate_corpus, semantic_digest, verify_corpus, verify_corpus_with_product_filter,
};

#[test]
fn catalog_declares_capabilities_without_comparing_unsupported_work() {
    let catalog = built_in_catalog();
    let ids = catalog.iter().map(|adapter| adapter.descriptor().id).collect::<Vec<_>>();
    assert_eq!(ids, ["tomlsmith", "toml", "toml_edit", "taplo"]);

    let toml = catalog.iter().find(|adapter| adapter.descriptor().id == "toml").unwrap();
    assert!(!toml.supports(Operation::DocumentPipeline));
    assert!(toml.supports(Operation::SemanticDecode));

    let taplo = catalog.iter().find(|adapter| adapter.descriptor().id == "taplo").unwrap();
    assert!(taplo.supports_version(TomlVersion::V1_0));
    assert!(!taplo.supports_version(TomlVersion::V1_1));
    let tomlsmith = catalog.iter().find(|adapter| adapter.descriptor().id == "tomlsmith").unwrap();
    assert!(tomlsmith.supports_version(TomlVersion::V1_0));
    assert!(tomlsmith.supports_version(TomlVersion::V1_1));

    assert!(matches!(tomlsmith.descriptor().version_mode, VersionMode::StrictSelectable));
    assert!(matches!(toml.descriptor().version_mode, VersionMode::CompatibleSubset { .. }));
    assert!(matches!(taplo.descriptor().version_mode, VersionMode::Declared { .. }));

    let tomlsmith_document = tomlsmith.descriptor().seam(Operation::DocumentPipeline).unwrap();
    let toml_edit = catalog.iter().find(|adapter| adapter.descriptor().id == "toml_edit").unwrap();
    let toml_edit_document = toml_edit.descriptor().seam(Operation::DocumentPipeline).unwrap();
    let taplo_document = taplo.descriptor().seam(Operation::DocumentPipeline).unwrap();
    assert_eq!(tomlsmith_document.seam_id, "tomlsmith.document.full_document");
    assert_eq!(toml_edit_document.seam_id, "toml_edit.document.document_mut");
    assert_eq!(taplo_document.seam_id, "taplo.document.syntax_tree");
    assert_ne!(tomlsmith_document.comparability_class, toml_edit_document.comparability_class);
    assert_ne!(toml_edit_document.comparability_class, taplo_document.comparability_class);

    let tomlsmith_format = tomlsmith.descriptor().seam(Operation::FormatPipeline).unwrap();
    let taplo_format = taplo.descriptor().seam(Operation::FormatPipeline).unwrap();
    assert_eq!(tomlsmith_format.comparability_class, "source_to_formatted_text");
    assert_eq!(tomlsmith_format.comparability_class, taplo_format.comparability_class);
}

#[test]
fn verification_report_keeps_operations_separate_and_checks_expectations() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let report = verify_corpus_with_product_filter(
        &corpus,
        &built_in_catalog(),
        Some("e2e/check/cold-stdin/1.0/v1_0_small"),
    );

    assert!(report.passed, "{:#?}", report.failures);
    assert_eq!(report.fixture_count, 1);
    assert!(report.cases.len() > 3);
    assert!(report.cases.iter().all(|case| case.passed));
    assert!(report.semantic_equivalence.iter().all(|case| case.passed));
    assert!(report.formatter_invariants.iter().all(|case| case.passed));
    assert!(report.semantic_equivalence.iter().all(|case| {
        case.members.windows(2).all(|members| members[0].digest_sha256 == members[1].digest_sha256)
    }));
    assert!(report.cases.iter().any(|case| case.operation == Operation::DocumentPipeline));
    assert!(report.cases.iter().any(|case| case.operation == Operation::SemanticDecode));
    assert!(report.cases.iter().any(|case| case.operation == Operation::FormatPipeline));
    let invalid_report =
        verify_corpus_with_product_filter(&corpus, &built_in_catalog(), Some("v1_0_invalid"));
    assert!(invalid_report.passed, "{:#?}", invalid_report.failures);
    assert!(!invalid_report.cases.iter().any(|case| case.operation == Operation::FormatPipeline));
    assert!(invalid_report.cases.iter().all(|case| case.detected_invalid));
}

#[test]
fn literal_benchmark_filter_limits_the_fast_preflight_to_its_selected_fixture() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let report = verify_corpus_with_product_filter(
        &corpus,
        &built_in_catalog(),
        Some("e2e/check/cold-stdin/1.0/v1_0_small"),
    );

    assert!(report.passed, "{:#?}", report.failures);
    assert_eq!(report.fixture_count, 1);
    assert!(report.cases.iter().all(|case| case.fixture_id == "v1_0_small"));
    assert!(report.semantic_equivalence.iter().all(|case| case.fixture_id == "v1_0_small"));
    assert!(report.formatter_invariants.iter().all(|case| case.fixture_id == "v1_0_small"));
}

#[test]
fn canonical_semantic_digest_agrees_for_every_typed_toml_value() {
    let source = r#"string = "hello 😀"
integer = -42
float = 3.5
positive_infinity = inf
not_a_number = nan
boolean = true
offset_datetime = 1979-05-27T07:32:00Z
local_datetime = 1979-05-27T07:32:00
local_date = 1979-05-27
local_time = 07:32:00
array = [1, "two", false]
inline = { z = 1, a = "two" }

[table]
nested = "value"
"#;
    let digests = built_in_catalog()
        .into_iter()
        .filter(|adapter| {
            adapter.supports(Operation::SemanticDecode)
                && adapter.supports_version(TomlVersion::V1_0)
        })
        .map(|adapter| {
            let canonical =
                adapter.canonical_semantics(TomlVersion::V1_0, source).unwrap().unwrap();
            (adapter.descriptor().id, semantic_digest(&canonical))
        })
        .collect::<Vec<_>>();

    assert_eq!(digests.len(), 3);
    assert!(digests.windows(2).all(|pair| pair[0].1 == pair[1].1), "{digests:#?}");
}

#[test]
fn correctness_gate_detects_tampered_semantics_and_non_idempotent_formatter() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let adapters: Vec<Box<dyn Adapter>> = vec![
        built_in_catalog().into_iter().find(|adapter| adapter.descriptor().id == "toml").unwrap(),
        Box::new(TamperedSemanticAdapter),
        Box::new(NonIdempotentFormatter),
    ];

    let report = verify_corpus(&corpus, &adapters);
    assert!(!report.passed);
    assert!(report.semantic_equivalence.iter().any(|case| !case.passed));
    assert!(report.formatter_invariants.iter().any(|case| !case.idempotent));
}

const TEST_SEMANTIC_SEAMS: &[SeamDescriptor] = &[SeamDescriptor {
    operation: Operation::SemanticDecode,
    seam_id: "test.semantic.tampered",
    comparability_class: "semantic_value_correctness",
}];
const TEST_FORMAT_SEAMS: &[SeamDescriptor] = &[SeamDescriptor {
    operation: Operation::FormatPipeline,
    seam_id: "test.format.non_idempotent",
    comparability_class: "source_to_formatted_text",
}];
const TEST_VERSIONS: &[TomlVersion] = &[TomlVersion::V1_0, TomlVersion::V1_1];
const TEST_CAVEATS: &[&str] = &[];

static TAMPERED_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "tampered",
    display_name: "tampered semantic adapter",
    version: Some("test"),
    upstream: "test-only",
    engine_family: "test-only",
    availability: Availability::Available,
    seams: TEST_SEMANTIC_SEAMS,
    toml_versions: TEST_VERSIONS,
    version_mode: VersionMode::StrictSelectable,
    included_work: "test-only tampering",
    caveats: TEST_CAVEATS,
};

static NON_IDEMPOTENT_DESCRIPTOR: AdapterDescriptor = AdapterDescriptor {
    id: "non_idempotent",
    display_name: "non-idempotent formatter",
    version: Some("test"),
    upstream: "test-only",
    engine_family: "test-only",
    availability: Availability::Available,
    seams: TEST_FORMAT_SEAMS,
    toml_versions: TEST_VERSIONS,
    version_mode: VersionMode::StrictSelectable,
    included_work: "test-only formatter",
    caveats: TEST_CAVEATS,
};

#[derive(Debug)]
struct TamperedSemanticAdapter;

impl Adapter for TamperedSemanticAdapter {
    fn descriptor(&self) -> &'static AdapterDescriptor {
        &TAMPERED_DESCRIPTOR
    }

    fn run(
        &self,
        operation: Operation,
        _version: TomlVersion,
        _source: &str,
    ) -> Result<AdapterOutput, AdapterError> {
        if operation == Operation::SemanticDecode {
            Ok(AdapterOutput { diagnostics: 0, fingerprint: 1 })
        } else {
            Err(AdapterError::Unsupported { adapter: self.descriptor().id, operation })
        }
    }

    fn canonical_semantics(
        &self,
        _version: TomlVersion,
        _source: &str,
    ) -> Result<Option<CanonicalValue>, AdapterError> {
        let mut table = BTreeMap::new();
        table.insert("tampered".to_owned(), CanonicalValue::Integer(1));
        Ok(Some(CanonicalValue::Table(table)))
    }
}

#[derive(Debug)]
struct NonIdempotentFormatter;

impl Adapter for NonIdempotentFormatter {
    fn descriptor(&self) -> &'static AdapterDescriptor {
        &NON_IDEMPOTENT_DESCRIPTOR
    }

    fn run(
        &self,
        operation: Operation,
        _version: TomlVersion,
        source: &str,
    ) -> Result<AdapterOutput, AdapterError> {
        if operation == Operation::FormatPipeline {
            Ok(AdapterOutput { diagnostics: 0, fingerprint: source.len() as u64 + 10 })
        } else {
            Err(AdapterError::Unsupported { adapter: self.descriptor().id, operation })
        }
    }

    fn canonical_semantics(
        &self,
        version: TomlVersion,
        source: &str,
    ) -> Result<Option<CanonicalValue>, AdapterError> {
        built_in_catalog()
            .into_iter()
            .find(|adapter| adapter.descriptor().id == "toml")
            .unwrap()
            .canonical_semantics(version, source)
    }

    fn format_for_verification(
        &self,
        _version: TomlVersion,
        source: &str,
    ) -> Result<Option<String>, AdapterError> {
        Ok(Some(format!("{source}# churn\n")))
    }
}

#[test]
fn generated_corpus_manifest_contains_only_execution_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let manifest = generate_corpus(directory.path()).unwrap();
    let json = serde_json::to_value(&manifest).unwrap();

    assert_eq!(json.as_object().unwrap().keys().collect::<Vec<_>>(), ["fixtures"]);
}

#[test]
fn generated_corpus_is_specification_driven_and_covers_each_toml_syntax_family() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();

    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        let fixture = corpus
            .fixtures()
            .iter()
            .find(|fixture| fixture.toml_version == version && fixture.class == FixtureClass::Small)
            .unwrap();
        assert_eq!(fixture.purpose, FixturePurpose::Headline);
        assert_eq!(fixture.source_kind, SourceKind::SpecificationGenerated);
        assert_eq!(fixture.format_state, FormatState::Formatted);
        assert!((4 * 1024..16 * 1024).contains(&fixture.bytes));
        assert!(fixture.provenance.contains("TOML specification"));
        assert!(fixture.source_url.is_none());
        assert!(fixture.source_revision.is_none());

        let source = fixture.source();
        for syntax in [
            "bare_key =",
            "\"quoted.key\" =",
            "'literal.key' =",
            "dotted . key =",
            "basic_string =",
            "multiline_basic = \"\"\"",
            "literal_string = '",
            "multiline_literal = '''",
            "decimal_integer =",
            "hex_integer = 0x",
            "octal_integer = 0o",
            "binary_integer = 0b",
            "positive_infinity = inf",
            "not_a_number = nan",
            "offset_datetime =",
            "local_datetime =",
            "local_date =",
            "local_time =",
            "mixed_array = [",
            "inline_table = {",
            "[standard_table]",
            "[[products]]",
            "[[products.variants]]",
        ] {
            assert!(source.contains(syntax), "TOML {version} is missing {syntax:?}");
        }

        if version == TomlVersion::V1_1 {
            assert_eq!(fixture.syntax_profile, SyntaxProfile::Toml11Native);
            for syntax in [
                "escape_character = \"\\e\"",
                "hex_escape = \"\\x41\"",
                "short_offset_datetime = 1979-05-27T07:32Z",
                "short_local_datetime = 1979-05-27T07:32",
                "short_local_time = 07:32",
                "multiline_inline_table = {\n",
            ] {
                assert!(source.contains(syntax), "TOML 1.1 is missing {syntax:?}");
            }
            assert!(source.contains("trailing = true,\n}"));
        } else {
            assert_eq!(fixture.syntax_profile, SyntaxProfile::CommonSubset);
            assert!(!source.contains("escape_character = \"\\e\""));
            assert!(!source.contains("hex_escape = \"\\x41\""));
            assert!(!source.contains("short_local_time = 07:32"));
            assert!(!source.contains("multiline_inline_table = {\n"));
        }
    }

    assert!(corpus.fixtures().iter().all(|fixture| {
        fixture.source_kind == SourceKind::SpecificationGenerated
            && fixture.source_url.is_none()
            && fixture.source_revision.is_none()
    }));
}

#[test]
fn generated_corpus_has_a_practical_ordered_size_gradient() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();

    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        let fixtures = corpus
            .fixtures()
            .iter()
            .filter(|fixture| {
                fixture.toml_version == version
                    && matches!(
                        fixture.class,
                        FixtureClass::Small
                            | FixtureClass::Medium
                            | FixtureClass::Large
                            | FixtureClass::Xlarge
                    )
            })
            .collect::<Vec<_>>();
        assert_eq!(fixtures.len(), 4);

        let by_class = |class| fixtures.iter().find(|fixture| fixture.class == class).unwrap();
        let small = by_class(FixtureClass::Small);
        let medium = by_class(FixtureClass::Medium);
        let large = by_class(FixtureClass::Large);
        let stress = by_class(FixtureClass::Xlarge);

        assert!((4 * 1024..16 * 1024).contains(&small.bytes));
        assert!((128 * 1024..160 * 1024).contains(&medium.bytes));
        assert!((1024 * 1024..1050 * 1024).contains(&large.bytes));
        assert!((10 * 1024 * 1024..10 * 1024 * 1024 + 4 * 1024).contains(&stress.bytes));
        assert!(
            small.bytes < medium.bytes && medium.bytes < large.bytes && large.bytes < stress.bytes
        );
        assert_eq!(medium.purpose, FixturePurpose::Scaling);
        assert_eq!(large.purpose, FixturePurpose::Scaling);
        assert_eq!(stress.purpose, FixturePurpose::Stress);
        assert!(!medium.source().as_bytes().windows(2).any(|pair| pair == b"\r\n"));
        assert!(large.lines > medium.lines);
        assert!(stress.lines > large.lines);
    }
}

#[test]
fn generated_corpus_isolates_crlf_in_short_diagnostic_fixtures() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let fixtures = corpus
        .fixtures()
        .iter()
        .filter(|fixture| fixture.workload_shape == WorkloadShape::Crlf)
        .collect::<Vec<_>>();

    assert_eq!(fixtures.len(), 2);
    for fixture in fixtures {
        assert_eq!(fixture.class, FixtureClass::Edge);
        assert_eq!(fixture.purpose, FixturePurpose::Diagnostic);
        assert!(fixture.bytes < 1024);
        assert!(fixture.source().as_bytes().windows(2).any(|pair| pair == b"\r\n"));
        assert!(!fixture.source().contains("\"\"\""));
        assert!(!fixture.source().contains("'''"));
    }
}

#[test]
fn generated_corpus_models_engineering_ecosystem_files_for_toml_1_0() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let engineering = corpus
        .fixtures()
        .iter()
        .filter(|fixture| fixture.class == FixtureClass::Engineering)
        .collect::<Vec<_>>();

    assert_eq!(
        engineering.iter().map(|fixture| fixture.id.as_str()).collect::<Vec<_>>(),
        ["v1_0_cargo_lock", "v1_0_workspace_manifest", "v1_0_app_config"]
    );
    for fixture in &engineering {
        assert_eq!(fixture.toml_version, TomlVersion::V1_0);
        assert_eq!(fixture.purpose, FixturePurpose::Headline);
        assert_eq!(fixture.syntax_profile, SyntaxProfile::CommonSubset);
        assert_eq!(fixture.format_state, FormatState::Edited);
        assert!(fixture.expected_valid);
        assert!(fixture.relative_path.starts_with("fixtures/v1.0/engineering"));
        assert!(!fixture.source().as_bytes().windows(2).any(|pair| pair == b"\r\n"));
        assert!(fixture.tags.iter().any(|tag| tag == "engineering"));

        // The baked deterministic format drift gives the format lane real work:
        // squeezed assignments, doubled spaces and trailing line whitespace.
        assert!(fixture.source().contains("=\""), "{} lacks squeezed assignments", fixture.id);
        assert!(fixture.source().contains("  =  "), "{} lacks doubled spacing", fixture.id);
        assert!(
            fixture.source().lines().any(|line| line.ends_with("  ")),
            "{} lacks trailing whitespace drift",
            fixture.id
        );
    }

    let lock = engineering[0];
    assert!((440 * 1024..520 * 1024).contains(&lock.bytes));
    assert!(lock.source().contains("version = 4"));
    assert!(lock.source().matches("[[package]]").count() >= 1700);
    assert!(
        lock.source()
            .contains("source = \"registry+https://github.com/rust-lang/crates.io-index\"")
    );
    assert!(lock.source().contains("dependencies = [\n"));
    assert!(lock.source().contains("checksum"));

    let manifest = engineering[1];
    assert!((56 * 1024..76 * 1024).contains(&manifest.bytes));
    assert!(manifest.source().contains("[workspace]"));
    assert!(manifest.source().contains("members = [\n"));
    assert!(manifest.source().contains("[workspace.package]"));
    assert!(manifest.source().contains("[workspace.dependencies]"));
    assert!(manifest.source().contains("default-features = false"));
    assert!(manifest.source().contains("[profile.release]"));
    assert!(manifest.source().contains("[profile.dev]"));
    assert!(manifest.source().contains("[workspace.lints.rust]"));

    let config = engineering[2];
    assert!((40 * 1024..60 * 1024).contains(&config.bytes));
    assert!(config.source().contains("baseURL"));
    assert!(config.source().contains("buildDate"));
    assert!(config.source().contains("2026-08-29T12:00:00+08:00"));
    assert!(config.source().contains("[params.services.tier-0.service-00]"));
    assert!(config.source().matches("[[menu.main]]").count() >= 40);
    assert!(config.source().contains("[languages.zh]"));
    assert!(config.source().contains("[markup.goldmark.renderer]"));
    assert!(config.source().contains("logo.light"));
    assert!(config.source().contains("\"\"\""));
    assert!(config.source().contains("'''"));
}

#[test]
fn generated_corpus_has_a_generic_truncated_array_for_each_toml_version() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let fixtures = corpus
        .fixtures()
        .iter()
        .filter(|fixture| fixture.workload_shape == WorkloadShape::InvalidTruncatedArray)
        .map(|fixture| {
            (
                fixture.id.as_str(),
                fixture.toml_version,
                fixture.relative_path.to_string_lossy().into_owned(),
                fixture.expected_valid,
                fixture.source(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        fixtures,
        [
            (
                "v1_0_invalid",
                TomlVersion::V1_0,
                "fixtures/v1.0/invalid/truncated-array.toml".to_owned(),
                false,
                "title = \"invalid fixture\"\nvalues = [1, 2\n",
            ),
            (
                "v1_1_invalid",
                TomlVersion::V1_1,
                "fixtures/v1.1/invalid/truncated-array.toml".to_owned(),
                false,
                "title = \"invalid fixture\"\nvalues = [1, 2\n",
            ),
        ]
    );
}

#[test]
fn generated_corpus_marks_each_toml_1_1_syntax_family_as_a_v1_0_version_boundary() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    let fixtures = corpus
        .fixtures()
        .iter()
        .filter(|fixture| fixture.tags.iter().any(|tag| tag == "version-boundary"))
        .map(|fixture| {
            (
                fixture.id.as_str(),
                fixture.toml_version,
                fixture.class,
                fixture.workload_shape,
                fixture.expected_valid,
                fixture.tags.iter().map(String::as_str).collect::<Vec<_>>(),
                fixture.source(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        fixtures,
        [
            (
                "v1_0_boundary_escape_e",
                TomlVersion::V1_0,
                FixtureClass::Invalid,
                WorkloadShape::VersionBoundaryEscapeE,
                false,
                vec!["invalid", "version-boundary", "escape-e", "correctness-only"],
                "value = \"\\e\"\n",
            ),
            (
                "v1_0_boundary_hex_escape",
                TomlVersion::V1_0,
                FixtureClass::Invalid,
                WorkloadShape::VersionBoundaryHexEscape,
                false,
                vec!["invalid", "version-boundary", "hex-escape", "correctness-only"],
                "value = \"\\x41\"\n",
            ),
            (
                "v1_0_boundary_omitted_seconds",
                TomlVersion::V1_0,
                FixtureClass::Invalid,
                WorkloadShape::VersionBoundaryOmittedSeconds,
                false,
                vec!["invalid", "version-boundary", "omitted-seconds", "correctness-only"],
                "value = 07:32\n",
            ),
            (
                "v1_0_boundary_inline_table",
                TomlVersion::V1_0,
                FixtureClass::Invalid,
                WorkloadShape::VersionBoundaryInlineTable,
                false,
                vec![
                    "invalid",
                    "version-boundary",
                    "multiline-inline-table",
                    "trailing-comma",
                    "correctness-only",
                ],
                "value = {\n  key = \"value\",\n}\n",
            ),
        ]
    );
}

#[test]
fn generated_corpus_is_reproducible_and_detects_tampering() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();

    let corpus = FixtureCorpus::load(directory.path()).unwrap();
    assert_eq!(corpus.fixtures().len(), 19);
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        for class in
            [FixtureClass::Small, FixtureClass::Medium, FixtureClass::Large, FixtureClass::Xlarge]
        {
            assert!(
                corpus
                    .fixtures()
                    .iter()
                    .any(|fixture| fixture.toml_version == version && fixture.class == class)
            );
        }
    }
    assert_eq!(corpus.fixtures().iter().filter(|fixture| fixture.expected_valid).count(), 13);
    assert!(
        corpus
            .fixtures()
            .iter()
            .filter(|fixture| fixture.class == FixtureClass::Xlarge)
            .all(|fixture| fixture.bytes >= 10 * 1024 * 1024)
    );
    assert!(
        corpus
            .fixtures()
            .iter()
            .filter(|fixture| fixture.workload_shape == WorkloadShape::Crlf)
            .all(|fixture| fixture.source().as_bytes().windows(2).any(|pair| pair == b"\r\n"))
    );

    let small_path = directory.path().join("fixtures/v1.0/small/specification.toml");
    let mut tampered = fs::read(&small_path).unwrap();
    tampered[0] = b'x';
    fs::write(&small_path, tampered).unwrap();
    let error = FixtureCorpus::load(directory.path()).unwrap_err();
    assert!(error.to_string().contains("checksum"), "{error}");
}

#[test]
fn corpus_loader_rejects_duplicate_workload_tags() {
    let directory = tempfile::tempdir().unwrap();
    generate_corpus(directory.path()).unwrap();
    let manifest_path = directory.path().join("fixtures/manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["fixtures"][0]["tags"] = serde_json::json!(["mixed-types", "mixed-types"]);
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();

    let error = FixtureCorpus::load(directory.path()).unwrap_err();
    assert!(error.to_string().contains("duplicate workload tag"), "{error}");
}

#[test]
fn adapters_execute_supported_operations_and_surface_invalid_input() {
    let valid = "title = \"fixture\"\n[owner]\nname = \"Tom\"\n";
    let invalid = "values = [1, 2\n";

    for adapter in built_in_catalog() {
        for operation in [Operation::DocumentPipeline, Operation::SemanticDecode] {
            if !adapter.supports(operation) {
                continue;
            }

            let valid_output =
                adapter.run(operation, TomlVersion::V1_0, valid).unwrap_or_else(|error| {
                    panic!("{} {operation} rejected valid input: {error}", adapter.descriptor().id)
                });
            assert_eq!(valid_output.diagnostics, 0, "{} {operation}", adapter.descriptor().id);

            match adapter.run(operation, TomlVersion::V1_0, invalid) {
                Ok(output) => assert!(
                    output.diagnostics > 0,
                    "{} {operation} silently accepted invalid input",
                    adapter.descriptor().id
                ),
                Err(AdapterError::Rejected { .. }) => {}
                Err(error) => panic!("{} {operation} returned {error}", adapter.descriptor().id),
            }
        }
    }
}

#[test]
fn format_pipeline_is_capability_aware_and_valid_only() {
    let valid = "title=\"fixture\"\nvalues=[1,2,3]\n";
    let catalog = built_in_catalog();
    let supported = catalog
        .iter()
        .filter(|adapter| adapter.supports(Operation::FormatPipeline))
        .map(|adapter| adapter.descriptor().id)
        .collect::<Vec<_>>();
    assert_eq!(supported, ["tomlsmith", "taplo"]);
    for adapter in catalog.iter().filter(|adapter| adapter.supports(Operation::FormatPipeline)) {
        let output = adapter.run(Operation::FormatPipeline, TomlVersion::V1_0, valid).unwrap();
        assert_eq!(output.diagnostics, 0, "{}", adapter.descriptor().id);
        assert!(output.fingerprint > 0, "{}", adapter.descriptor().id);
    }
}

#[test]
fn tomlsmith_honors_fixture_version_and_taplo_excludes_toml_1_1() {
    let source = "escape = \"\\e\"\nshort_time = 12:30\n";
    let catalog = built_in_catalog();
    let tomlsmith = catalog.iter().find(|adapter| adapter.descriptor().id == "tomlsmith").unwrap();
    let strict_1_0 = tomlsmith.run(Operation::DocumentPipeline, TomlVersion::V1_0, source).unwrap();
    let strict_1_1 = tomlsmith.run(Operation::DocumentPipeline, TomlVersion::V1_1, source).unwrap();
    assert!(strict_1_0.diagnostics > 0);
    assert_eq!(strict_1_1.diagnostics, 0);

    let taplo = catalog.iter().find(|adapter| adapter.descriptor().id == "taplo").unwrap();
    assert!(matches!(
        taplo.run(Operation::DocumentPipeline, TomlVersion::V1_1, source),
        Err(AdapterError::UnsupportedVersion { .. })
    ));
}
