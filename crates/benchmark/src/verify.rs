use std::collections::HashMap;

use serde::Serialize;

use crate::{
    Adapter, AdapterError, Fixture, FixtureCorpus, Operation, OptionalToolAvailability, ProductId,
    ProductOperation, ProductProcessError, ProductRunner, ProductStatus, ProductVersionSupport,
    TomlVersion, VersionMode, product_statuses, semantic_digest,
};

#[derive(Clone, Debug, Serialize)]
pub struct VerificationCase {
    pub adapter_id: &'static str,
    pub fixture_id: String,
    pub toml_version: TomlVersion,
    pub operation: Operation,
    pub expected_valid: bool,
    pub passed: bool,
    pub detected_invalid: bool,
    pub diagnostics: Option<usize>,
    pub outcome: &'static str,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticDigestMember {
    pub adapter_id: &'static str,
    pub seam_id: &'static str,
    pub digest_sha256: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticEquivalenceCase {
    pub fixture_id: String,
    pub toml_version: TomlVersion,
    pub passed: bool,
    pub members: Vec<SemanticDigestMember>,
}

#[derive(Clone, Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct FormatterInvariantCase {
    pub adapter_id: &'static str,
    pub seam_id: &'static str,
    pub fixture_id: String,
    pub toml_version: TomlVersion,
    pub passed: bool,
    pub reparsed: bool,
    pub semantic_preserved: bool,
    pub idempotent: bool,
    pub source_digest_sha256: Option<String>,
    pub formatted_digest_sha256: Option<String>,
    pub formatted_bytes: Option<usize>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerificationReport {
    pub fixture_count: usize,
    pub passed: bool,
    pub cases: Vec<VerificationCase>,
    pub semantic_equivalence: Vec<SemanticEquivalenceCase>,
    pub formatter_invariants: Vec<FormatterInvariantCase>,
    pub products: ProductVerificationReport,
    pub failures: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ProductVerificationCase {
    pub product_id: ProductId,
    pub operation: ProductOperation,
    pub fixture_id: String,
    pub toml_version: TomlVersion,
    pub expected_valid: bool,
    pub passed: bool,
    pub detected_invalid: bool,
    pub output_utf8: Option<bool>,
    pub reparsed: Option<bool>,
    pub semantic_preserved: Option<bool>,
    pub idempotent: Option<bool>,
    pub stdout_bytes: Option<usize>,
    pub stderr_bytes: Option<usize>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProductVerificationReport {
    pub statuses: Vec<ProductStatus>,
    pub passed: bool,
    pub cases: Vec<ProductVerificationCase>,
    pub failures: Vec<String>,
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn verify_corpus(corpus: &FixtureCorpus, adapters: &[Box<dyn Adapter>]) -> VerificationReport {
    verify_corpus_with_product_filter(corpus, adapters, None)
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn verify_corpus_with_product_filter(
    corpus: &FixtureCorpus,
    adapters: &[Box<dyn Adapter>],
    product_filter: Option<&str>,
) -> VerificationReport {
    let mut cases = Vec::new();
    let mut failures = Vec::new();
    let mut digest_cache = HashMap::<(&'static str, String), Result<String, String>>::new();
    let selected_fixture_id = product_filter.and_then(|filter| {
        corpus
            .fixtures()
            .iter()
            .find(|fixture| filter.contains(&fixture.id))
            .map(|fixture| fixture.id.as_str())
    });
    let fixture_selected =
        |fixture: &Fixture| selected_fixture_id.is_none_or(|id| fixture.id == id);

    for operation in
        [Operation::DocumentPipeline, Operation::SemanticDecode, Operation::FormatPipeline]
    {
        for adapter in adapters.iter().filter(|adapter| adapter.supports(operation)) {
            for fixture in corpus.fixtures().iter().filter(|fixture| {
                fixture_selected(fixture)
                    && adapter.supports_version(fixture.toml_version)
                    && adapter_fixture_applies(adapter.descriptor().version_mode, &fixture.tags)
                    && (operation != Operation::FormatPipeline || fixture.expected_valid)
            }) {
                let result = adapter.run(operation, fixture.toml_version, fixture.source());
                let (passed, detected_invalid, diagnostics, outcome, detail) = match result {
                    Ok(output) if fixture.expected_valid => {
                        (output.diagnostics == 0, false, Some(output.diagnostics), "accepted", None)
                    }
                    Ok(output) => (
                        output.diagnostics > 0,
                        output.diagnostics > 0,
                        Some(output.diagnostics),
                        if output.diagnostics > 0 { "diagnosed" } else { "accepted" },
                        None,
                    ),
                    Err(AdapterError::Rejected { message, .. }) if fixture.expected_valid => {
                        (false, false, None, "rejected", Some(message))
                    }
                    Err(AdapterError::Rejected { message, .. }) => {
                        (true, true, None, "rejected", Some(message))
                    }
                    Err(error) => (false, false, None, "adapter_error", Some(error.to_string())),
                };
                if !passed {
                    failures.push(format!(
                        "{} {} failed expectation for TOML {} fixture {} ({})",
                        adapter.descriptor().id,
                        operation,
                        fixture.toml_version,
                        fixture.id,
                        if fixture.expected_valid { "valid" } else { "invalid" }
                    ));
                }
                cases.push(VerificationCase {
                    adapter_id: adapter.descriptor().id,
                    fixture_id: fixture.id.clone(),
                    toml_version: fixture.toml_version,
                    operation,
                    expected_valid: fixture.expected_valid,
                    passed,
                    detected_invalid,
                    diagnostics,
                    outcome,
                    detail,
                });
            }
        }
    }

    let mut semantic_equivalence = Vec::new();
    for fixture in corpus
        .fixtures()
        .iter()
        .filter(|fixture| fixture_selected(fixture) && fixture.expected_valid)
    {
        let comparable = adapters
            .iter()
            .filter(|adapter| {
                adapter.supports(Operation::SemanticDecode)
                    && adapter.supports_version(fixture.toml_version)
            })
            .collect::<Vec<_>>();
        if comparable.len() < 2 {
            continue;
        }
        let mut members = Vec::with_capacity(comparable.len());
        for adapter in comparable {
            let seam = adapter.descriptor().seam(Operation::SemanticDecode);
            let result = seam.map_or_else(
                || {
                    Err(format!(
                        "adapter {} reports semantic support without a declared seam",
                        adapter.descriptor().id
                    ))
                },
                |_| canonical_digest(adapter.as_ref(), fixture),
            );
            digest_cache.insert((adapter.descriptor().id, fixture.id.clone()), result.clone());
            let seam_id = seam.map_or("invalid.missing.semantic_seam", |seam| seam.seam_id);
            match result {
                Ok(digest_sha256) => members.push(SemanticDigestMember {
                    adapter_id: adapter.descriptor().id,
                    seam_id,
                    digest_sha256: Some(digest_sha256),
                    error: None,
                }),
                Err(error) => members.push(SemanticDigestMember {
                    adapter_id: adapter.descriptor().id,
                    seam_id,
                    digest_sha256: None,
                    error: Some(error),
                }),
            }
        }
        let expected = members.first().and_then(|member| member.digest_sha256.as_deref());
        let passed = expected.is_some()
            && members.iter().all(|member| member.digest_sha256.as_deref() == expected);
        if !passed {
            failures.push(format!(
                "canonical semantic digests disagree for TOML {} fixture {}",
                fixture.toml_version, fixture.id
            ));
        }
        semantic_equivalence.push(SemanticEquivalenceCase {
            fixture_id: fixture.id.clone(),
            toml_version: fixture.toml_version,
            passed,
            members,
        });
    }

    let mut formatter_invariants = Vec::new();
    for adapter in adapters.iter().filter(|adapter| adapter.supports(Operation::FormatPipeline)) {
        let Some(seam_id) =
            adapter.descriptor().seam(Operation::FormatPipeline).map(|seam| seam.seam_id)
        else {
            failures.push(format!(
                "adapter {} reports formatter support without a declared seam",
                adapter.descriptor().id
            ));
            continue;
        };
        for fixture in corpus.fixtures().iter().filter(|fixture| {
            fixture_selected(fixture)
                && fixture.expected_valid
                && adapter.supports_version(fixture.toml_version)
        }) {
            let source_digest = digest_cache
                .entry((adapter.descriptor().id, fixture.id.clone()))
                .or_insert_with(|| canonical_digest(adapter.as_ref(), fixture))
                .clone();
            let formatted = adapter
                .format_for_verification(fixture.toml_version, fixture.source())
                .and_then(|formatted| {
                    formatted.ok_or(AdapterError::Unsupported {
                        adapter: adapter.descriptor().id,
                        operation: Operation::FormatPipeline,
                    })
                });

            let mut detail = Vec::new();
            let (formatted_text, formatted_bytes) = match formatted {
                Ok(formatted) => {
                    let bytes = formatted.len();
                    (Some(formatted), Some(bytes))
                }
                Err(error) => {
                    detail.push(format!("format failed: {error}"));
                    (None, None)
                }
            };
            let formatted_digest = formatted_text.as_deref().map_or_else(
                || Err("format produced no text".to_owned()),
                |text| canonical_digest_source(adapter.as_ref(), fixture.toml_version, text),
            );
            let reparsed = formatted_digest.is_ok();
            if let Err(error) = &formatted_digest {
                detail.push(format!("formatted output did not reparse: {error}"));
            }
            let semantic_preserved = matches!(
                (&source_digest, &formatted_digest),
                (Ok(source), Ok(formatted)) if source == formatted
            );
            if !semantic_preserved {
                detail.push("formatted output changed canonical semantics".to_owned());
            }
            let second_format = formatted_text
                .as_deref()
                .map(|text| adapter.format_for_verification(fixture.toml_version, text));
            let idempotent = matches!(
                (&formatted_text, second_format),
                (Some(first), Some(Ok(Some(second)))) if first == &second
            );
            if !idempotent {
                detail.push("second formatting pass changed output".to_owned());
            }
            let passed = reparsed && semantic_preserved && idempotent;
            if !passed {
                failures.push(format!(
                    "{} formatter invariants failed for TOML {} fixture {}",
                    adapter.descriptor().id,
                    fixture.toml_version,
                    fixture.id
                ));
            }
            formatter_invariants.push(FormatterInvariantCase {
                adapter_id: adapter.descriptor().id,
                seam_id,
                fixture_id: fixture.id.clone(),
                toml_version: fixture.toml_version,
                passed,
                reparsed,
                semantic_preserved,
                idempotent,
                source_digest_sha256: source_digest.ok(),
                formatted_digest_sha256: formatted_digest.ok(),
                formatted_bytes,
                detail: (!detail.is_empty()).then(|| detail.join("; ")),
            });
        }
    }

    let products = verify_products_matching(corpus, product_filter);
    failures.extend(products.failures.iter().cloned());

    VerificationReport {
        fixture_count: selected_fixture_id.map_or(corpus.fixtures().len(), |_| 1),
        passed: failures.is_empty(),
        cases,
        semantic_equivalence,
        formatter_invariants,
        products,
        failures,
    }
}

#[must_use]
pub fn verify_products(corpus: &FixtureCorpus) -> ProductVerificationReport {
    verify_products_matching(corpus, None)
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub fn verify_products_matching(
    corpus: &FixtureCorpus,
    product_filter: Option<&str>,
) -> ProductVerificationReport {
    let statuses = product_statuses();
    let mut cases = Vec::new();
    let mut failures = statuses
        .iter()
        .filter_map(|status| match &status.availability {
            OptionalToolAvailability::Invalid { reason } => Some(format!(
                "invalid {} product configuration: {reason}",
                status.descriptor.id.as_str()
            )),
            OptionalToolAvailability::Enabled | OptionalToolAvailability::Skipped { .. } => None,
        })
        .collect::<Vec<_>>();

    let isolation_root = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => {
            failures.push(format!("failed to create product verification isolation: {error}"));
            return ProductVerificationReport { statuses, passed: false, cases, failures };
        }
    };

    for status in statuses
        .iter()
        .filter(|status| matches!(status.availability, OptionalToolAvailability::Enabled))
    {
        let product_id = status.descriptor.id;
        let runner = match ProductRunner::from_env(product_id) {
            Ok(Some(runner)) => runner,
            Ok(None) => {
                failures.push(format!(
                    "{} was enabled during discovery but disappeared before verification",
                    product_id.as_str()
                ));
                continue;
            }
            Err(error) => {
                failures.push(format!(
                    "failed to resolve {} for verification: {error}",
                    product_id.as_str()
                ));
                continue;
            }
        };

        for &version in status.descriptor.toml_versions {
            let isolation = match runner.prepare_isolation(isolation_root.path(), version) {
                Ok(directory) => directory,
                Err(error) => {
                    failures.push(format!(
                        "failed to prepare {} for TOML {version}: {error}",
                        product_id.as_str()
                    ));
                    continue;
                }
            };
            for &operation in status.descriptor.operations {
                let operation_has_selected_fixture = corpus.fixtures().iter().any(|fixture| {
                    fixture.expected_valid
                        && fixture.toml_version == version
                        && product_group_matches(product_filter, operation, version, &fixture.id)
                });
                if !operation_has_selected_fixture {
                    continue;
                }
                for fixture in corpus.fixtures().iter().filter(|fixture| {
                    fixture.toml_version == version
                        && if fixture.expected_valid {
                            product_group_matches(product_filter, operation, version, &fixture.id)
                        } else {
                            operation != ProductOperation::Format
                                && product_fixture_applies(
                                    status.descriptor.version_support,
                                    &fixture.tags,
                                )
                        }
                }) {
                    let case = if fixture.expected_valid {
                        match operation {
                            ProductOperation::Format => {
                                verify_product_format(&runner, fixture, &isolation)
                            }
                            ProductOperation::Check => {
                                verify_product_acceptance(&runner, operation, fixture, &isolation)
                            }
                        }
                    } else {
                        verify_product_acceptance(&runner, operation, fixture, &isolation)
                    };
                    if !case.passed {
                        failures.push(format!(
                            "{} {} failed for TOML {} fixture {}: {}",
                            product_id.as_str(),
                            operation.as_str(),
                            version,
                            fixture.id,
                            case.detail.as_deref().unwrap_or("unknown failure")
                        ));
                    }
                    cases.push(case);
                }
            }
        }
    }

    ProductVerificationReport { statuses, passed: failures.is_empty(), cases, failures }
}

fn adapter_fixture_applies(version_mode: VersionMode, fixture_tags: &[String]) -> bool {
    !is_version_boundary(fixture_tags) || matches!(version_mode, VersionMode::StrictSelectable)
}

fn product_fixture_applies(
    version_support: ProductVersionSupport,
    fixture_tags: &[String],
) -> bool {
    !is_version_boundary(fixture_tags)
        || matches!(version_support, ProductVersionSupport::StrictSelectable)
}

fn is_version_boundary(fixture_tags: &[String]) -> bool {
    fixture_tags.iter().any(|tag| tag == "version-boundary")
}

fn product_group_matches(
    filter: Option<&str>,
    operation: ProductOperation,
    version: TomlVersion,
    fixture_id: &str,
) -> bool {
    let benchmark_id = format!("e2e/{}/cold-stdin/{version}/{fixture_id}", operation.as_str());
    filter.is_none_or(|filter| benchmark_id.contains(filter))
}

fn verify_product_acceptance(
    runner: &ProductRunner,
    operation: ProductOperation,
    fixture: &Fixture,
    isolation: &std::path::Path,
) -> ProductVerificationCase {
    match runner.run_prepared_bounded(operation, fixture.toml_version, fixture.source(), isolation)
    {
        Ok(output) => {
            let output_utf8 = String::from_utf8(output.stdout.clone()).is_ok()
                && String::from_utf8(output.stderr.clone()).is_ok();
            let passed = fixture.expected_valid && output_utf8;
            ProductVerificationCase {
                product_id: runner.product_id(),
                operation,
                fixture_id: fixture.id.clone(),
                toml_version: fixture.toml_version,
                expected_valid: fixture.expected_valid,
                passed,
                detected_invalid: false,
                output_utf8: Some(output_utf8),
                reparsed: None,
                semantic_preserved: None,
                idempotent: None,
                stdout_bytes: Some(output.stdout.len()),
                stderr_bytes: Some(output.stderr.len()),
                detail: (!passed).then(|| {
                    if !fixture.expected_valid {
                        "invalid TOML was accepted".to_owned()
                    } else if !output_utf8 {
                        "product output was not UTF-8".to_owned()
                    } else {
                        "product acceptance did not satisfy its correctness contract".to_owned()
                    }
                }),
            }
        }
        Err(error) if !fixture.expected_valid && product_process_failure_is_rejection(&error) => {
            ProductVerificationCase {
                product_id: runner.product_id(),
                operation,
                fixture_id: fixture.id.clone(),
                toml_version: fixture.toml_version,
                expected_valid: false,
                passed: true,
                detected_invalid: true,
                output_utf8: None,
                reparsed: None,
                semantic_preserved: None,
                idempotent: None,
                stdout_bytes: None,
                stderr_bytes: None,
                detail: None,
            }
        }
        Err(error) => failed_product_case(runner, operation, fixture, error.to_string()),
    }
}

fn product_process_failure_is_rejection(error: &ProductProcessError) -> bool {
    matches!(
        error,
        ProductProcessError::CommandFailed {
            product: ProductId::TomlSmith | ProductId::Tombi | ProductId::Taplo,
            operation: ProductOperation::Check,
            status: Some(1),
            ..
        } | ProductProcessError::CommandFailed {
            product: ProductId::BurntSushiToml,
            operation: ProductOperation::Check,
            status: Some(1),
            ..
        }
    )
}

fn verify_product_format(
    runner: &ProductRunner,
    fixture: &Fixture,
    isolation: &std::path::Path,
) -> ProductVerificationCase {
    let first = match runner.run_prepared_bounded(
        ProductOperation::Format,
        fixture.toml_version,
        fixture.source(),
        isolation,
    ) {
        Ok(output) => output,
        Err(error) => {
            return failed_product_case(
                runner,
                ProductOperation::Format,
                fixture,
                error.to_string(),
            );
        }
    };
    let stdout_bytes = first.stdout.len();
    let stderr_bytes = first.stderr.len();
    let formatted = match String::from_utf8(first.stdout) {
        Ok(formatted) => formatted,
        Err(error) => {
            return ProductVerificationCase {
                product_id: runner.product_id(),
                operation: ProductOperation::Format,
                fixture_id: fixture.id.clone(),
                toml_version: fixture.toml_version,
                expected_valid: true,
                passed: false,
                detected_invalid: false,
                output_utf8: Some(false),
                reparsed: Some(false),
                semantic_preserved: Some(false),
                idempotent: Some(false),
                stdout_bytes: Some(stdout_bytes),
                stderr_bytes: Some(stderr_bytes),
                detail: Some(format!("formatter stdout is not UTF-8: {error}")),
            };
        }
    };

    let source_digest =
        crate::correctness::canonical_tomlsmith(fixture.source(), fixture.toml_version)
            .map(|value| semantic_digest(&value));
    let formatted_digest =
        crate::correctness::canonical_tomlsmith(&formatted, fixture.toml_version)
            .map(|value| semantic_digest(&value));
    let reparsed = formatted_digest.is_ok();
    let semantic_preserved = matches!(
        (&source_digest, &formatted_digest),
        (Ok(source), Ok(formatted)) if source == formatted
    );
    let second = runner.run_prepared_bounded(
        ProductOperation::Format,
        fixture.toml_version,
        &formatted,
        isolation,
    );
    let idempotent = matches!(&second, Ok(output) if output.stdout == formatted.as_bytes());
    let passed = reparsed && semantic_preserved && idempotent;
    let mut detail = Vec::new();
    if let Err(error) = &formatted_digest {
        detail.push(format!("formatted output did not reparse: {error}"));
    }
    if !semantic_preserved {
        detail.push("formatted output changed canonical semantics".to_owned());
    }
    if !idempotent {
        detail.push(match second {
            Ok(_) => "second formatting pass changed output bytes".to_owned(),
            Err(error) => format!("second formatting pass failed: {error}"),
        });
    }
    ProductVerificationCase {
        product_id: runner.product_id(),
        operation: ProductOperation::Format,
        fixture_id: fixture.id.clone(),
        toml_version: fixture.toml_version,
        expected_valid: true,
        passed,
        detected_invalid: false,
        output_utf8: Some(true),
        reparsed: Some(reparsed),
        semantic_preserved: Some(semantic_preserved),
        idempotent: Some(idempotent),
        stdout_bytes: Some(stdout_bytes),
        stderr_bytes: Some(stderr_bytes),
        detail: (!detail.is_empty()).then(|| detail.join("; ")),
    }
}

fn failed_product_case(
    runner: &ProductRunner,
    operation: ProductOperation,
    fixture: &Fixture,
    detail: String,
) -> ProductVerificationCase {
    ProductVerificationCase {
        product_id: runner.product_id(),
        operation,
        fixture_id: fixture.id.clone(),
        toml_version: fixture.toml_version,
        expected_valid: fixture.expected_valid,
        passed: false,
        detected_invalid: false,
        output_utf8: None,
        reparsed: None,
        semantic_preserved: None,
        idempotent: None,
        stdout_bytes: None,
        stderr_bytes: None,
        detail: Some(detail),
    }
}

fn canonical_digest(adapter: &dyn Adapter, fixture: &Fixture) -> Result<String, String> {
    canonical_digest_source(adapter, fixture.toml_version, fixture.source())
}

fn canonical_digest_source(
    adapter: &dyn Adapter,
    version: TomlVersion,
    source: &str,
) -> Result<String, String> {
    adapter
        .canonical_semantics(version, source)
        .map_err(|error| error.to_string())?
        .map(|value| semantic_digest(&value))
        .ok_or_else(|| {
            format!(
                "adapter {} exposes no canonical semantic correctness seam",
                adapter.descriptor().id
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{
        product_fixture_applies, product_group_matches, product_process_failure_is_rejection,
    };
    use crate::{
        ProductId, ProductOperation, ProductProcessError, ProductVersionSupport, TomlVersion,
    };

    #[test]
    fn product_filter_matches_the_same_ids_used_by_criterion() {
        assert!(product_group_matches(
            None,
            ProductOperation::Format,
            TomlVersion::V1_1,
            "v1_1_stress"
        ));
        assert!(product_group_matches(
            Some("cold-stdin/1.0/v1_0_medium"),
            ProductOperation::Check,
            TomlVersion::V1_0,
            "v1_0_medium"
        ));
        assert!(!product_group_matches(
            Some("cold-stdin/1.0/v1_0_medium"),
            ProductOperation::Format,
            TomlVersion::V1_0,
            "v1_0_stress"
        ));
        assert!(!product_group_matches(
            Some("e2e/check/"),
            ProductOperation::Format,
            TomlVersion::V1_0,
            "v1_0_medium"
        ));
    }

    #[test]
    fn version_boundary_fixtures_only_apply_to_strict_selectors() {
        let boundary = vec!["invalid".to_owned(), "version-boundary".to_owned()];
        let ordinary = vec!["invalid".to_owned()];

        assert!(product_fixture_applies(ProductVersionSupport::StrictSelectable, &boundary));
        assert!(!product_fixture_applies(ProductVersionSupport::CompatibleSubset, &boundary));
        assert!(!product_fixture_applies(ProductVersionSupport::Fixed, &boundary));
        assert!(product_fixture_applies(ProductVersionSupport::CompatibleSubset, &ordinary));
    }

    #[test]
    fn only_the_pinned_products_expected_exit_codes_count_as_rejection() {
        let tombi_rejection = ProductProcessError::CommandFailed {
            product: ProductId::Tombi,
            operation: ProductOperation::Check,
            status: Some(1),
            stderr: "invalid TOML".to_owned(),
        };
        let rust_panic = ProductProcessError::CommandFailed {
            product: ProductId::Tombi,
            operation: ProductOperation::Check,
            status: Some(101),
            stderr: "thread 'main' panicked".to_owned(),
        };
        let arbitrary_failure = ProductProcessError::CommandFailed {
            product: ProductId::Tombi,
            operation: ProductOperation::Check,
            status: Some(99),
            stderr: "unexpected failure".to_owned(),
        };
        let signal = ProductProcessError::CommandFailed {
            product: ProductId::Tombi,
            operation: ProductOperation::Check,
            status: None,
            stderr: "terminated".to_owned(),
        };

        assert!(product_process_failure_is_rejection(&tombi_rejection));
        assert!(!product_process_failure_is_rejection(&rust_panic));
        assert!(!product_process_failure_is_rejection(&arbitrary_failure));
        assert!(!product_process_failure_is_rejection(&signal));
    }
}
