use std::{hint::black_box, path::Path, time::Duration};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tomlsmith_benchmark::{
    Adapter, BenchmarkSettings, FixtureCorpus, Operation, ProductOperation, ProductRunner,
    TomlVersion, built_in_catalog, product_catalog,
};

fn criterion_config() -> Criterion {
    let settings = BenchmarkSettings::from_env().unwrap_or_else(|error| {
        panic!("invalid benchmark configuration: {error}");
    });
    Criterion::default()
        .warm_up_time(Duration::from_secs_f64(settings.warm_up_seconds))
        .measurement_time(Duration::from_secs_f64(settings.measurement_seconds))
        .sample_size(settings.sample_size)
        .configure_from_args()
}

fn benchmark_competitors(criterion: &mut Criterion) {
    let settings = BenchmarkSettings::from_env().unwrap_or_else(|error| {
        panic!("invalid benchmark configuration: {error}");
    });
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("benchmark crate is nested under the workspace root");
    let corpus = FixtureCorpus::load(root).expect("fixture corpus must pass integrity checks");
    let adapters = built_in_catalog();

    benchmark_isolated_seams(criterion, &settings, &corpus, &adapters);
    benchmark_format_comparison(criterion, &settings, &corpus, &adapters);
    benchmark_products(criterion, &settings, &corpus);
}

fn benchmark_isolated_seams(
    criterion: &mut Criterion,
    settings: &BenchmarkSettings,
    corpus: &FixtureCorpus,
    adapters: &[Box<dyn Adapter>],
) {
    for operation in [Operation::DocumentPipeline, Operation::SemanticDecode] {
        for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
            for adapter in adapters
                .iter()
                .filter(|adapter| adapter.supports(operation) && adapter.supports_version(version))
            {
                let seam =
                    adapter.descriptor().seam(operation).expect("supported operation has a seam");
                for fixture in corpus
                    .fixtures()
                    .iter()
                    .filter(|fixture| fixture.expected_valid && fixture.toml_version == version)
                {
                    let group_id =
                        format!("microbench/seam/{}/{version}/{}", seam.seam_id, fixture.id);
                    if !settings.includes(&group_id) {
                        continue;
                    }
                    let mut group = criterion.benchmark_group(group_id);
                    group.throughput(Throughput::Bytes(fixture.bytes as u64));
                    group.bench_function(adapter.descriptor().id, |bencher| {
                        bencher.iter(|| {
                            black_box(
                                adapter
                                    .run(operation, version, black_box(fixture.source()))
                                    .unwrap_or_else(|error| {
                                        panic!(
                                            "{} rejected valid TOML {version} fixture {}: {error}",
                                            adapter.descriptor().id,
                                            fixture.id
                                        )
                                    }),
                            )
                        });
                    });
                    group.finish();
                }
            }
        }
    }
}

fn benchmark_format_comparison(
    criterion: &mut Criterion,
    settings: &BenchmarkSettings,
    corpus: &FixtureCorpus,
    adapters: &[Box<dyn Adapter>],
) {
    for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
        for fixture in corpus
            .fixtures()
            .iter()
            .filter(|fixture| fixture.expected_valid && fixture.toml_version == version)
        {
            let group_id =
                format!("microbench/format/source_to_formatted_text/{version}/{}", fixture.id);
            if !settings.includes(&group_id) {
                continue;
            }
            let mut group = criterion.benchmark_group(group_id);
            group.throughput(Throughput::Bytes(fixture.bytes as u64));
            for adapter in adapters.iter().filter(|adapter| {
                adapter.supports(Operation::FormatPipeline) && adapter.supports_version(version)
            }) {
                group.bench_function(adapter.descriptor().id, |bencher| {
                    bencher.iter(|| {
                        black_box(
                            adapter
                                .run(
                                    Operation::FormatPipeline,
                                    version,
                                    black_box(fixture.source()),
                                )
                                .unwrap_or_else(|error| {
                                    panic!(
                                        "{} rejected valid TOML {version} fixture {}: {error}",
                                        adapter.descriptor().id,
                                        fixture.id
                                    )
                                }),
                        )
                    });
                });
            }
            group.finish();
        }
    }
}

fn benchmark_products(
    criterion: &mut Criterion,
    settings: &BenchmarkSettings,
    corpus: &FixtureCorpus,
) {
    let runners = product_catalog()
        .iter()
        .filter_map(|product| {
            ProductRunner::from_env(product.id).unwrap_or_else(|error| {
                panic!("invalid {} product configuration: {error}", product.id.as_str())
            })
        })
        .collect::<Vec<_>>();
    if runners.is_empty() {
        return;
    }

    let isolation = tempfile::tempdir().expect("create product isolation root");
    for operation in [ProductOperation::Check, ProductOperation::Format] {
        for version in [TomlVersion::V1_0, TomlVersion::V1_1] {
            for fixture in corpus
                .fixtures()
                .iter()
                .filter(|fixture| fixture.expected_valid && fixture.toml_version == version)
            {
                let group_id =
                    format!("e2e/{}/cold-stdin/{version}/{}", operation.as_str(), fixture.id);
                if !settings.includes(&group_id) {
                    continue;
                }
                let participating = runners
                    .iter()
                    .filter(|runner| {
                        runner.descriptor().operations.contains(&operation)
                            && runner.descriptor().supports_version(version)
                    })
                    .collect::<Vec<_>>();
                if participating.is_empty() {
                    continue;
                }

                let mut group = criterion.benchmark_group(group_id);
                group.throughput(Throughput::Bytes(fixture.bytes as u64));
                for runner in participating {
                    let working_directory = runner
                        .prepare_isolation(isolation.path(), version)
                        .unwrap_or_else(|error| {
                            panic!(
                                "failed to prepare {} for TOML {version}: {error}",
                                runner.product_id().as_str()
                            )
                        });
                    runner
                        .run_prepared_bounded(
                            operation,
                            version,
                            fixture.source(),
                            &working_directory,
                        )
                        .unwrap_or_else(|error| {
                            panic!(
                                "{} {} preflight rejected valid TOML {version} fixture {}: {error}",
                                runner.product_id().as_str(),
                                operation.as_str(),
                                fixture.id
                            )
                        });
                    group.bench_function(runner.product_id().as_str(), |bencher| {
                        bencher.iter(|| {
                            let output = runner
                                .run_prepared(
                                    operation,
                                    version,
                                    black_box(fixture.source()),
                                    &working_directory,
                                )
                                .unwrap_or_else(|error| {
                                    panic!(
                                        "{} {} rejected valid TOML {version} fixture {}: {error}",
                                        runner.product_id().as_str(),
                                        operation.as_str(),
                                        fixture.id
                                    )
                                });
                            black_box(&output.stdout);
                            black_box(&output.stderr);
                            black_box(output.fingerprint);
                            output
                        });
                    });
                }
                group.finish();
            }
        }
    }
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = benchmark_competitors
}
criterion_main!(benches);
