// The benchmark compiles the production config module in isolation; application-only
// helpers and the module's unit-test imports are intentionally unused by this target.
#[allow(dead_code, unused_imports)]
#[path = "../src/mode.rs"]
mod mode;

#[allow(dead_code, unused_imports)]
#[path = "../src/config.rs"]
mod config;

use config::Config;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::{fmt::Write as _, fs, hint::black_box, path::PathBuf};

fn benchmark_config_load(criterion: &mut Criterion) {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/bench-fixtures");
    fs::create_dir_all(&fixture_dir).unwrap_or_else(|error| {
        panic!(
            "failed to create benchmark fixture directory {}: {error:#}",
            fixture_dir.display()
        )
    });

    let mut group = criterion.benchmark_group("config_load");
    for connector_count in [1_usize, 48] {
        let path = fixture_dir.join(format!("{connector_count}-connectors.toml"));
        fs::write(&path, sample_config(connector_count)).unwrap_or_else(|error| {
            panic!(
                "failed to write benchmark fixture {}: {error:#}",
                path.display()
            )
        });
        Config::load(&path).unwrap_or_else(|error| {
            panic!(
                "failed to validate benchmark fixture {}: {error:#}",
                path.display()
            )
        });

        group.throughput(Throughput::Elements(connector_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(connector_count),
            &path,
            |bencher, path| {
                bencher.iter(|| {
                    let config = Config::load(black_box(path.as_path())).unwrap_or_else(|error| {
                        panic!(
                            "failed to load benchmark fixture {}: {error:#}",
                            path.display()
                        )
                    });
                    black_box(config)
                });
            },
        );

        fs::remove_file(&path).unwrap_or_else(|error| {
            panic!(
                "failed to remove benchmark fixture {}: {error:#}",
                path.display()
            )
        });
    }
    group.finish();
}

fn sample_config(connector_count: usize) -> String {
    let mut config = String::from(
        r#"network = "benchmark"
metrics_port = 8000
poll_interval_sec = 15

"#,
    );

    for index in 0..connector_count {
        writeln!(
            config,
            r#"[[connectors]]
name = "connector-{index}"
provider = "benchmark-provider"
features = []
rpc_url = "https://rpc-{index}.example"
rpc_timeout_sec = 30
ws_url = "wss://rpc-{index}.example"
ws_timeout_sec = 10
commitment = "confirmed"
"#
        )
        .expect("writing to a String cannot fail");
    }

    let route = (0..connector_count)
        .map(|index| format!("\"connector-{index}\""))
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(
        config,
        r#"[modes.health.commands]
rpc_health = [{route}]
ws_health = [{route}]

[modes.consensus.commands]
epoch_info = [{route}]
vote_accounts = [{route}]
recent_performance_samples = [{route}]

[modes.leadership.commands]
current_block_production = [{route}]
current_leader_schedule = [{route}]
next_leader_schedule = [{route}]

[modes.economics]
enabled = false"#
    )
    .expect("writing to a String cannot fail");
    config
}

criterion_group!(benches, benchmark_config_load);
criterion_main!(benches);
