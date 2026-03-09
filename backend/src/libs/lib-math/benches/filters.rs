use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use lib_math::LowPassFilter;

fn low_pass_benchmark(c: &mut Criterion) {
    let mut filter = LowPassFilter::<1>::new(0.25);
    c.bench_function("low_pass_update", |b| {
        b.iter(|| {
            filter.update([black_box(1.0)]);
        });
    });
}

criterion_group!(benches, low_pass_benchmark);
criterion_main!(benches);
