use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use markoff_core::{Format, convert_file};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temporary_path(name: &str, extension: &str) -> PathBuf {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "markoff_benchmark_{name}_{}_{}.{}",
        std::process::id(),
        sequence,
        extension
    ))
}

fn json_records(record_count: usize) -> String {
    let records = (0..record_count)
        .map(|index| {
            format!(
                r#"{{"id":{index},"name":"record-{index}","active":{}}}"#,
                index % 2 == 0
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", records.join(","))
}

fn benchmark_json_to_xlsx(criterion: &mut Criterion) {
    let source = temporary_path("input", "json");
    fs::write(&source, json_records(10_000)).expect("write benchmark input");

    criterion.bench_function("json_to_xlsx_10k_rows", |bencher| {
        bencher.iter_batched(
            || temporary_path("output", "xlsx"),
            |output| {
                convert_file(&source, &output, Format::Json, Format::Xlsx)
                    .expect("convert benchmark input");
                fs::remove_file(output).expect("remove benchmark output");
            },
            BatchSize::SmallInput,
        );
    });

    fs::remove_file(source).expect("remove benchmark input");
}

criterion_group!(conversion_benches, benchmark_json_to_xlsx);
criterion_main!(conversion_benches);
