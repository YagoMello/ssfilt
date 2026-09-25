use std::hint::black_box;

use criterion::measurement::WallTime;
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use ssfilt::{BandPass, HighPass, InputModel, LowPass, Response};

const CUTOFF_HZ: f64 = 1_000.0;
const AUDIO_SAMPLE_INTERVAL: f64 = 1.0 / 48_000.0;
const INPUTS: [f64; 8] = [0.8, -0.3, 1.0, -0.9, 0.2, 0.6, -0.7, 0.1];

fn response_name(response: Response) -> &'static str {
    match response {
        Response::RepeatedPole => "repeated-pole",
        Response::Butterworth => "butterworth",
        Response::Bessel => "bessel",
        Response::Chebyshev1 { .. } => "chebyshev-1",
        _ => "other",
    }
}

fn benchmark_order<const N: usize>(group: &mut BenchmarkGroup<'_, WallTime>, response: Response) {
    group.bench_with_input(
        BenchmarkId::new(response_name(response), N),
        &response,
        |bencher, &response| {
            let mut filter = LowPass::<N>::builder(CUTOFF_HZ)
                .response(response)
                .build()
                .unwrap();
            let mut index = 0;
            bencher.iter(|| {
                index = (index + 1) % INPUTS.len();
                black_box(
                    filter
                        .update(black_box(INPUTS[index]), black_box(AUDIO_SAMPLE_INTERVAL))
                        .unwrap(),
                )
            });
        },
    );
}

fn order_scaling(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("streaming/order");
    group.throughput(Throughput::Elements(1));
    for response in [
        Response::RepeatedPole,
        Response::Butterworth,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
    ] {
        benchmark_order::<1>(&mut group, response);
        benchmark_order::<2>(&mut group, response);
        benchmark_order::<4>(&mut group, response);
        benchmark_order::<8>(&mut group, response);
    }
    group.finish();
}

fn input_models(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("streaming/input-model");
    group.throughput(Throughput::Elements(1));
    for (name, input_model) in [
        ("linear", InputModel::Linear),
        ("previous-hold", InputModel::PreviousHold),
        ("current-hold", InputModel::CurrentHold),
    ] {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &input_model,
            |bencher, &input_model| {
                let mut filter = LowPass::<4>::builder(CUTOFF_HZ)
                    .response(Response::Butterworth)
                    .input_model(input_model)
                    .build()
                    .unwrap();
                let mut index = 0;
                bencher.iter(|| {
                    index = (index + 1) % INPUTS.len();
                    black_box(
                        filter
                            .update(black_box(INPUTS[index]), black_box(AUDIO_SAMPLE_INTERVAL))
                            .unwrap(),
                    )
                });
            },
        );
    }
    group.finish();
}

fn adaptive_work(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("streaming/normalized-duration");
    group.throughput(Throughput::Elements(1));
    for normalized_duration in [0.01, 0.1, 1.0, 10.0] {
        group.bench_with_input(
            BenchmarkId::from_parameter(normalized_duration),
            &normalized_duration,
            |bencher, &normalized_duration| {
                let dt_seconds = normalized_duration / (core::f64::consts::TAU * CUTOFF_HZ);
                let mut filter = LowPass::<4>::builder(CUTOFF_HZ)
                    .response(Response::Butterworth)
                    .input_model(InputModel::CurrentHold)
                    .build()
                    .unwrap();
                let mut input = -1.0;
                bencher.iter(|| {
                    input = -input;
                    black_box(
                        filter
                            .update(black_box(input), black_box(dt_seconds))
                            .unwrap(),
                    )
                });
            },
        );
    }
    group.finish();
}

fn topologies(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("streaming/topology");
    group.throughput(Throughput::Elements(1));
    group.bench_function("low-pass/4", |bencher| {
        let mut filter = LowPass::<4>::builder(CUTOFF_HZ)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut index = 0;
        bencher.iter(|| {
            index = (index + 1) % INPUTS.len();
            black_box(
                filter
                    .update(black_box(INPUTS[index]), black_box(AUDIO_SAMPLE_INTERVAL))
                    .unwrap(),
            )
        });
    });
    group.bench_function("high-pass/4", |bencher| {
        let mut filter = HighPass::<4>::builder(CUTOFF_HZ)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut index = 0;
        bencher.iter(|| {
            index = (index + 1) % INPUTS.len();
            black_box(
                filter
                    .update(black_box(INPUTS[index]), black_box(AUDIO_SAMPLE_INTERVAL))
                    .unwrap(),
            )
        });
    });
    group.bench_function("band-pass/4", |bencher| {
        let mut filter = BandPass::<4>::builder(CUTOFF_HZ / 2.0, CUTOFF_HZ * 2.0)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut index = 0;
        bencher.iter(|| {
            index = (index + 1) % INPUTS.len();
            black_box(
                filter
                    .update(black_box(INPUTS[index]), black_box(AUDIO_SAMPLE_INTERVAL))
                    .unwrap(),
            )
        });
    });
    group.bench_function("phase-equalized-low-pass/4+2", |bencher| {
        let base = LowPass::<4>::builder(CUTOFF_HZ)
            .response(Response::Butterworth)
            .build()
            .unwrap();
        let mut filter = base.equalize_phase::<2>(0.0, CUTOFF_HZ).unwrap();
        let mut index = 0;
        bencher.iter(|| {
            index = (index + 1) % INPUTS.len();
            black_box(
                filter
                    .update(black_box(INPUTS[index]), black_box(AUDIO_SAMPLE_INTERVAL))
                    .unwrap(),
            )
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    order_scaling,
    input_models,
    adaptive_work,
    topologies
);
criterion_main!(benches);
