use bpx::diff::DiffEngine;
use bpx::diff::similar::SimilarDiffEngine;
use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::Arc;
use std::time::Duration;
fn generate_json_data(size: usize, variation: f32) -> (Vec<u8>, Vec<u8>) {
    let base = match size {
        100 => r#"{"id":"123","name":"Alice Johnson","email":"alice@example.com","status":"active"}"#.to_string(),
        1000 => {
            r#"{"id":"123","name":"Alice Johnson","email":"alice@example.com","phone":"+1234567890","address":{"street":"123 Main St","city":"Springfield","state":"IL","zip":"62701"},"preferences":{"newsletter":true,"notifications":"email","theme":"dark"},"metadata":{"created_at":"2024-01-01T00:00:00Z","updated_at":"2024-01-15T12:30:00Z","last_login":"2024-03-01T09:15:00Z","login_count":42,"tags":["premium","verified","beta_tester"],"subscription":{"plan":"pro","expires":"2024-12-31T23:59:59Z","auto_renew":true}},"status":"active"}"#.to_string()
        }
        10000 => {
            let mut json = String::from(r#"{"id":"123","name":"Alice Johnson","email":"alice@example.com","logs":["#);
            for i in 0..100 {
                json.push_str(&format!(
                    r#"{{"timestamp":"2024-03-01T09:{:02}:00Z","level":"INFO","message":"User action {}"}},"#,
                    i % 60, i
                ));
            }
            json.push_str(r#"],"metrics":{"#);
            for i in 0..50 {
                json.push_str(&format!(r#""metric_{}":{},"#, i, i * 100));
            }
            json.push_str(r#""total":5000},"status":"active"}"#);
            json
        }
        _ => r#"{"id":"123","data":"test"}"#.to_string(),
    };

    let modified = match variation {
        v if v < 0.1 => base.replace("Alice Johnson", "Alicia Johnson"),
        v if v < 0.3 => base
            .replace("Alice Johnson", "Alicia J. Smith")
            .replace("active", "inactive")
            .replace("alice@example.com", "alicia.smith@example.org"),
        _ => {
            format!(
                "{}{}",
                base.trim_end_matches('}'),
                r#","new_field":"This is additional data that wasn't present before","another_field":12345}"#
            )
        }
    };

    (base.as_bytes().to_vec(), modified.as_bytes().to_vec())
}

fn generate_log_data(lines: usize, new_lines: usize) -> (Vec<u8>, Vec<u8>) {
    let mut base = String::new();
    for i in 0..lines {
        base.push_str(&format!(
            "[2024-03-01T09:00:{:02}Z] INFO: Application event {} occurred\n",
            i % 60,
            i
        ));
    }

    let mut modified = base.clone();
    for i in lines..(lines + new_lines) {
        modified.push_str(&format!(
            "[2024-03-01T09:01:{:02}Z] INFO: Application event {} occurred\n",
            i % 60,
            i
        ));
    }

    (base.as_bytes().to_vec(), modified.as_bytes().to_vec())
}

fn benchmark_json_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_updates");
    group.measurement_time(Duration::from_secs(1));
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));

    let engine = Arc::new(SimilarDiffEngine::new());

    for size in [1000].iter() {
        for variation in [0.05].iter() {
            let (original, modified) = generate_json_data(*size, *variation);
            let change_percent = (*variation * 100.0) as u32;

            group.throughput(Throughput::Bytes(modified.len() as u64));

            group.bench_with_input(
                BenchmarkId::new("REST", format!("{}B_{}%change", size, change_percent)),
                &modified,
                |b, data| {
                    b.iter(|| {
                        let _sent = data.clone();
                        data.len()
                    });
                },
            );

            group.bench_with_input(
                BenchmarkId::new("BPX", format!("{}B_{}%change", size, change_percent)),
                &(&original, &modified),
                |b, (orig, modif)| {
                    b.iter(|| {
                        let diff = engine
                            .compute_diff(&Bytes::from(orig.to_vec()), &Bytes::from(modif.to_vec()))
                            .unwrap();
                        diff.len()
                    });
                },
            );
        }
    }

    group.finish();
}

fn benchmark_log_streaming(c: &mut Criterion) {
    let mut group = c.benchmark_group("log_streaming");
    group.measurement_time(Duration::from_secs(1));
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));

    let engine = Arc::new(SimilarDiffEngine::new());

    for initial_lines in [100].iter() {
        for new_lines in [10].iter() {
            let (original, modified) = generate_log_data(*initial_lines, *new_lines);

            group.throughput(Throughput::Bytes(modified.len() as u64));

            group.bench_with_input(
                BenchmarkId::new("REST", format!("{}lines_+{}new", initial_lines, new_lines)),
                &modified,
                |b, data| {
                    b.iter(|| {
                        let _sent = data.clone();
                        data.len()
                    });
                },
            );

            group.bench_with_input(
                BenchmarkId::new("BPX", format!("{}lines_+{}new", initial_lines, new_lines)),
                &(&original, &modified),
                |b, (orig, modif)| {
                    b.iter(|| {
                        let diff = engine
                            .compute_diff(&Bytes::from(orig.to_vec()), &Bytes::from(modif.to_vec()))
                            .unwrap();
                        diff.len()
                    });
                },
            );
        }
    }

    group.finish();
}

fn benchmark_bandwidth_savings(c: &mut Criterion) {
    let mut group = c.benchmark_group("bandwidth_savings");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));

    let engine = Arc::new(SimilarDiffEngine::new());

    let scenarios = vec![
        ("minimal_change", generate_json_data(1000, 0.02)),
        ("small_update", generate_json_data(1000, 0.1)),
        ("moderate_change", generate_json_data(1000, 0.3)),
        ("append_only", generate_log_data(500, 20)),
        (
            "large_payload_small_change",
            generate_json_data(10000, 0.05),
        ),
    ];

    for (name, (original, modified)) in scenarios {
        let rest_size = modified.len();
        let diff = engine
            .compute_diff(
                &Bytes::from(original.to_vec()),
                &Bytes::from(modified.to_vec()),
            )
            .unwrap();
        let bpx_size = diff.len();
        let savings = ((rest_size - bpx_size) as f64 / rest_size as f64 * 100.0) as u32;

        group.bench_function(format!("{}/REST_{}B", name, rest_size), |b| {
            b.iter(|| rest_size)
        });

        group.bench_function(
            format!("{}/BPX_{}B_{}%saved", name, bpx_size, savings),
            |b| b.iter(|| bpx_size),
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_json_updates,
    benchmark_log_streaming,
    benchmark_bandwidth_savings
);
criterion_main!(benches);
