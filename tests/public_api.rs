use approx::assert_relative_eq;
use ssfilt::{
    BandPass, BandStop, BuildError, DormandPrince54, HighPass, InputModel, IntegrationDiagnostics,
    Integrator, LowPass, MAX_BESSEL_BAND_PASS_ORDER, MAX_BESSEL_BAND_STOP_ORDER, MAX_BESSEL_ORDER,
    ResetError, Response, RungeKutta4, StreamingFilter, UpdateError,
};

fn update_generic<F>(filter: &mut F, input: F::Scalar, dt: F::Scalar) -> F::Scalar
where
    F: StreamingFilter,
{
    filter.update(input, dt).unwrap()
}

#[test]
fn readme_style_usage_works() {
    let mut filter = LowPass::<4>::builder(20.0)
        .input_model(InputModel::Linear)
        .initial_input(0.0)
        .build()
        .unwrap();

    let value = filter.update(1.0, 0.012).unwrap();
    assert!(value.is_finite());
    assert!(value > 0.0 && value < 1.0);
}

#[test]
fn streaming_trait_supports_generic_callers() {
    let mut filter = LowPass::<2, f32>::builder(10.0)
        .input_model(InputModel::CurrentHold)
        .build()
        .unwrap();
    let value = update_generic(&mut filter, 1.0, 0.01);
    assert!(value > 0.0 && value < 1.0);
}

fn update_with_integrator<const N: usize, I>(
    filter: &mut LowPass<N, f32, I>,
    input: f32,
    dt_seconds: f32,
) -> Result<f32, UpdateError>
where
    I: Integrator<f32, N>,
{
    filter.update(input, dt_seconds)
}

#[test]
fn integrator_trait_supports_external_generic_callers() {
    let policy = DormandPrince54 {
        absolute_tolerance: 1.0e-5,
        relative_tolerance: 1.0e-3,
        max_step_seconds: Some(0.002),
        max_step_attempts: 128,
    };
    let mut filter = LowPass::<4, f32>::builder(20.0)
        .integrator(policy)
        .build()
        .unwrap();

    let output = update_with_integrator(&mut filter, 1.0, 0.01).unwrap();
    assert!(output.is_finite());
}

#[test]
fn fixed_runge_kutta_is_selectable_for_every_topology() {
    let policy = RungeKutta4 {
        max_step_seconds: 0.001_f32,
        max_steps: 64,
    };
    let mut low_pass = LowPass::<2, f32>::builder(20.0)
        .integrator(policy)
        .build()
        .unwrap();
    let mut high_pass = HighPass::<2, f32>::builder(20.0)
        .integrator(policy)
        .build()
        .unwrap();
    let mut band_pass = BandPass::<4, f32>::builder(10.0, 40.0)
        .integrator(policy)
        .build()
        .unwrap();
    let mut band_stop = BandStop::<4, f32>::builder(10.0, 40.0)
        .integrator(policy)
        .build()
        .unwrap();

    for output in [
        low_pass.update(1.0, 0.002).unwrap(),
        high_pass.update(1.0, 0.002).unwrap(),
        band_pass.update(1.0, 0.002).unwrap(),
        band_stop.update(1.0, 0.002).unwrap(),
    ] {
        assert!(output.is_finite());
    }
}

#[test]
fn fixed_runge_kutta_matches_a_first_order_step_and_reports_work() {
    let policy = RungeKutta4 {
        max_step_seconds: 0.005_f64,
        max_steps: 64,
    };
    let mut filter = LowPass::<1>::builder(3.0)
        .input_model(InputModel::CurrentHold)
        .integrator(policy)
        .build()
        .unwrap();

    let actual = filter.update(1.0, 0.012).unwrap();
    let expected = 1.0 - (-core::f64::consts::TAU * 3.0 * 0.012).exp();
    assert_relative_eq!(actual, expected, epsilon = 2.0e-7);

    let diagnostics = filter.last_diagnostics();
    assert_eq!(diagnostics.accepted_steps(), 3);
    assert_eq!(diagnostics.rejected_steps(), 0);
    assert_eq!(diagnostics.derivative_evaluations(), 12);
    assert_relative_eq!(
        diagnostics.smallest_accepted_step_seconds().unwrap(),
        0.004,
        epsilon = 1.0e-15
    );
    assert_relative_eq!(
        diagnostics.largest_accepted_step_seconds().unwrap(),
        0.004,
        epsilon = 1.0e-15
    );
}

#[test]
fn fixed_runge_kutta_rejects_invalid_configuration() {
    for policy in [
        RungeKutta4 {
            max_step_seconds: 0.0,
            max_steps: 64,
        },
        RungeKutta4 {
            max_step_seconds: f64::NAN,
            max_steps: 64,
        },
        RungeKutta4 {
            max_step_seconds: 0.001,
            max_steps: 0,
        },
    ] {
        assert!(matches!(
            LowPass::<2>::builder(20.0).integrator(policy).build(),
            Err(BuildError::InvalidIntegrationConfig)
        ));
    }
}

#[test]
fn fixed_runge_kutta_budget_failure_is_transactional() {
    let policy = RungeKutta4 {
        max_step_seconds: 0.001_f64,
        max_steps: 2,
    };
    let mut tested = LowPass::<2>::builder(20.0)
        .input_model(InputModel::Linear)
        .integrator(policy)
        .build()
        .unwrap();
    tested.update(0.25, 0.001).unwrap();
    let mut untouched = tested;

    assert_eq!(
        tested.update(1.0, 0.01),
        Err(UpdateError::StepBudgetExceeded)
    );
    assert_eq!(
        tested.update(-0.5, 0.001).unwrap(),
        untouched.update(-0.5, 0.001).unwrap()
    );
    assert_eq!(tested.last_diagnostics(), untouched.last_diagnostics());
}

fn assert_rk4_tracks_reference<A, B>(mut reference: A, mut fixed: B, tolerance: f64)
where
    A: StreamingFilter<Scalar = f64>,
    B: StreamingFilter<Scalar = f64>,
{
    let samples = [0.3, 1.0, -0.4, 0.7, -0.9, 0.2, 0.8, -0.1];
    let intervals = [
        0.000_7, 0.001_3, 0.002_1, 0.000_4, 0.001_8, 0.000_9, 0.002_4, 0.000_6,
    ];
    for (input, dt) in samples.into_iter().zip(intervals) {
        let expected = reference.update(input, dt).unwrap();
        let actual = fixed.update(input, dt).unwrap();
        assert_relative_eq!(actual, expected, epsilon = tolerance);
    }
}

#[test]
fn fixed_runge_kutta_tracks_adaptive_reference_for_representative_filters() {
    let policy = RungeKutta4 {
        max_step_seconds: 0.001,
        max_steps: 8,
    };

    assert_rk4_tracks_reference(
        LowPass::<4>::builder(20.0)
            .response(Response::Butterworth)
            .build()
            .unwrap(),
        LowPass::<4>::builder(20.0)
            .response(Response::Butterworth)
            .integrator(policy)
            .build()
            .unwrap(),
        1.0e-4,
    );
    assert_rk4_tracks_reference(
        HighPass::<4>::builder(20.0)
            .response(Response::Chebyshev1 { ripple_db: 0.5 })
            .build()
            .unwrap(),
        HighPass::<4>::builder(20.0)
            .response(Response::Chebyshev1 { ripple_db: 0.5 })
            .integrator(policy)
            .build()
            .unwrap(),
        1.0e-4,
    );
    assert_rk4_tracks_reference(
        BandPass::<4>::builder(10.0, 40.0)
            .response(Response::Bessel)
            .build()
            .unwrap(),
        BandPass::<4>::builder(10.0, 40.0)
            .response(Response::Bessel)
            .integrator(policy)
            .build()
            .unwrap(),
        1.0e-4,
    );
    assert_rk4_tracks_reference(
        BandStop::<4>::builder(10.0, 40.0)
            .response(Response::Butterworth)
            .build()
            .unwrap(),
        BandStop::<4>::builder(10.0, 40.0)
            .response(Response::Butterworth)
            .integrator(policy)
            .build()
            .unwrap(),
        1.0e-4,
    );
}

#[test]
fn band_stop_uses_final_order_two_edges_and_streaming_trait() {
    let mut filter = BandStop::<4, f32>::builder(10.0, 40.0)
        .response(Response::Butterworth)
        .input_model(InputModel::PreviousHold)
        .initial_input(2.0)
        .build()
        .unwrap();

    assert_eq!(filter.lower_cutoff_hz(), 10.0);
    assert_eq!(filter.upper_cutoff_hz(), 40.0);
    assert_eq!(filter.center_hz(), 20.0);
    assert_eq!(filter.response(), Response::Butterworth);
    assert_eq!(filter.output(), 2.0);
    assert!(update_generic(&mut filter, 1.0, 0.002).is_finite());
}

#[test]
fn band_stop_rejects_invalid_order_edges_and_bessel_limit() {
    assert!(matches!(
        BandStop::<3>::builder(10.0, 40.0).build(),
        Err(BuildError::InvalidBandStopOrder)
    ));
    assert!(matches!(
        BandStop::<4>::builder(40.0, 10.0).build(),
        Err(BuildError::InvalidBandStopEdges)
    ));
    assert!(
        BandStop::<MAX_BESSEL_BAND_STOP_ORDER>::builder(10.0, 40.0)
            .response(Response::Bessel)
            .build()
            .is_ok()
    );
    assert!(matches!(
        BandStop::<52>::builder(10.0, 40.0)
            .response(Response::Bessel)
            .build(),
        Err(BuildError::UnsupportedBesselOrder)
    ));
}

#[test]
fn band_stop_errors_are_transactional() {
    let mut tested = BandStop::<4>::builder(10.0, 40.0)
        .response(Response::Chebyshev1 { ripple_db: 0.5 })
        .build()
        .unwrap();
    tested.update(0.25, 0.001).unwrap();
    let mut untouched = tested;

    assert_eq!(
        tested.update(f64::INFINITY, 0.001),
        Err(UpdateError::NonFiniteInput)
    );
    assert_eq!(
        tested.update(-0.5, 0.001).unwrap(),
        untouched.update(-0.5, 0.001).unwrap()
    );
    assert_eq!(tested.last_diagnostics(), untouched.last_diagnostics());
}

fn first_order_rk4_error(max_step_seconds: f64) -> f64 {
    let cutoff_hz = 3.0;
    let dt = 0.05;
    let mut filter = LowPass::<1>::builder(cutoff_hz)
        .input_model(InputModel::CurrentHold)
        .integrator(RungeKutta4 {
            max_step_seconds,
            max_steps: 128,
        })
        .build()
        .unwrap();
    let actual = filter.update(1.0, dt).unwrap();
    let expected = 1.0 - (-core::f64::consts::TAU * cutoff_hz * dt).exp();
    (actual - expected).abs()
}

#[test]
fn fixed_runge_kutta_exhibits_fourth_order_convergence_and_f32_accuracy() {
    let coarse = first_order_rk4_error(0.01);
    let medium = first_order_rk4_error(0.005);
    let fine = first_order_rk4_error(0.0025);
    assert!(coarse / medium > 12.0, "coarse={coarse}, medium={medium}");
    assert!(medium / fine > 12.0, "medium={medium}, fine={fine}");

    let cutoff_hz = 3.0_f32;
    let dt = 0.012_f32;
    let mut filter = LowPass::<1, f32>::builder(cutoff_hz)
        .input_model(InputModel::CurrentHold)
        .integrator(RungeKutta4 {
            max_step_seconds: 0.001,
            max_steps: 16,
        })
        .build()
        .unwrap();
    let actual = filter.update(1.0, dt).unwrap();
    let expected = 1.0 - (-core::f32::consts::TAU * cutoff_hz * dt).exp();
    assert_relative_eq!(actual, expected, epsilon = 2.0e-6);
}

#[test]
fn fixed_runge_kutta_model_cap_handles_narrow_and_wide_band_passes() {
    let policy = RungeKutta4 {
        max_step_seconds: 0.1,
        max_steps: 1_024,
    };
    for (lower, upper, dt) in [(55.0, 65.0, 0.01), (1.0, 1_000.0, 0.005)] {
        let mut reference = BandPass::<4>::builder(lower, upper)
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap();
        let mut fixed = BandPass::<4>::builder(lower, upper)
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .integrator(policy)
            .build()
            .unwrap();

        let expected = reference.update(1.0, dt).unwrap();
        let actual = fixed.update(1.0, dt).unwrap();
        assert_relative_eq!(actual, expected, epsilon = 2.0e-3);
        assert!(
            fixed
                .last_diagnostics()
                .largest_accepted_step_seconds()
                .unwrap()
                < policy.max_step_seconds
        );
    }
}

#[test]
fn fixed_runge_kutta_previous_hold_high_pass_preserves_endpoint_semantics() {
    let policy = RungeKutta4 {
        max_step_seconds: 0.001,
        max_steps: 64,
    };
    let mut reference = HighPass::<4>::builder(20.0)
        .response(Response::Butterworth)
        .input_model(InputModel::PreviousHold)
        .build()
        .unwrap();
    let mut fixed = HighPass::<4>::builder(20.0)
        .response(Response::Butterworth)
        .input_model(InputModel::PreviousHold)
        .integrator(policy)
        .build()
        .unwrap();

    assert_relative_eq!(
        fixed.update(1.0, 0.002).unwrap(),
        reference.update(1.0, 0.002).unwrap(),
        epsilon = 1.0e-12
    );
    assert_relative_eq!(
        fixed.update(0.5, 0.003).unwrap(),
        reference.update(0.5, 0.003).unwrap(),
        epsilon = 2.0e-5
    );
    assert!(fixed.last_diagnostics().derivative_evaluations() > 0);
}

#[test]
fn fixed_runge_kutta_one_step_audio_workload_has_bounded_reference_error() {
    const CUTOFF_HZ: f64 = 1_000.0;
    const DT: f64 = 1.0 / 48_000.0;
    const INPUTS: [f64; 8] = [0.8, -0.3, 1.0, -0.9, 0.2, 0.6, -0.7, 0.1];

    let mut reference = LowPass::<4>::builder(CUTOFF_HZ)
        .response(Response::Butterworth)
        .build()
        .unwrap();
    let mut fixed = LowPass::<4>::builder(CUTOFF_HZ)
        .response(Response::Butterworth)
        .integrator(RungeKutta4 {
            max_step_seconds: DT,
            max_steps: 1,
        })
        .build()
        .unwrap();
    let mut max_error = 0.0_f64;

    for index in 0..4_096 {
        let input = INPUTS[index % INPUTS.len()];
        let expected = reference.update(input, DT).unwrap();
        let actual = fixed.update(input, DT).unwrap();
        max_error = max_error.max((actual - expected).abs());
    }

    assert!(max_error < 2.0e-5, "maximum error was {max_error}");
}

#[test]
fn error_does_not_change_subsequent_behavior() {
    let mut tested = LowPass::<3>::builder(3.0).build().unwrap();
    let mut untouched = tested;

    assert_eq!(
        tested.update(f64::NAN, 0.1),
        Err(UpdateError::NonFiniteInput)
    );
    let after_error = tested.update(0.75, 0.04).unwrap();
    let direct = untouched.update(0.75, 0.04).unwrap();
    assert_relative_eq!(after_error, direct, epsilon = 0.0);
}

#[test]
fn public_reset_establishes_a_new_equilibrium() {
    let mut filter = LowPass::<6>::builder(100.0)
        .response(Response::Butterworth)
        .build()
        .unwrap();
    filter.reset_to_steady(42.0).unwrap();
    assert_relative_eq!(filter.update(42.0, 50.0).unwrap(), 42.0, epsilon = 0.0);
}

#[test]
fn integration_diagnostics_are_available_without_extending_the_trait() {
    let mut filter = LowPass::<4>::builder(20.0)
        .response(Response::Butterworth)
        .input_model(InputModel::CurrentHold)
        .build()
        .unwrap();
    assert_eq!(
        filter.last_diagnostics(),
        IntegrationDiagnostics::<f64>::default()
    );

    filter.update(1.0, 0.01).unwrap();
    let diagnostics = filter.last_diagnostics();
    assert!(diagnostics.accepted_steps() > 0);
    assert!(diagnostics.attempted_steps() >= diagnostics.accepted_steps());
    assert!(diagnostics.derivative_evaluations() > 0);
    assert!(diagnostics.smallest_accepted_step_seconds().unwrap() > 0.0);
    assert!(diagnostics.largest_accepted_step_seconds().unwrap() <= 0.01);
}

#[test]
fn default_dormand_prince_behavior_is_stable_during_integrator_refactoring() {
    // This deliberately freezes representative output and controller work at
    // the bit level. Change it only alongside an explicit numerical decision,
    // not as fallout from moving integration behind a policy abstraction.
    let samples = [
        (0.25_f64, 0.000_7_f64),
        (1.0, 0.001_3),
        (-0.5, 0.004_1),
        (0.75, 0.000_2),
        (0.1, 0.012_7),
    ];
    let expected_f64 = [
        (0x3e80_1c4f_fa69_08e0, 4, 1, 30),
        (0x3ef7_f512_d5a7_3578, 7, 1, 48),
        (0x3f71_317b_2913_4cf1, 16, 2, 108),
        (0x3f73_7010_7589_23c8, 1, 0, 6),
        (0x3fc2_9aa5_c7e8_df35, 22, 2, 144),
    ];
    let mut f64_filter = LowPass::<4>::builder(20.0)
        .response(Response::Butterworth)
        .input_model(InputModel::Linear)
        .build()
        .unwrap();
    for ((input, dt), (output_bits, accepted, rejected, evaluations)) in
        samples.into_iter().zip(expected_f64)
    {
        assert_eq!(f64_filter.update(input, dt).unwrap().to_bits(), output_bits);
        let diagnostics = f64_filter.last_diagnostics();
        assert_eq!(diagnostics.accepted_steps(), accepted);
        assert_eq!(diagnostics.rejected_steps(), rejected);
        assert_eq!(diagnostics.derivative_evaluations(), evaluations);
    }

    let expected_f32 = [
        (0x33ff_8d9b, 1, 0, 6),
        (0x37bf_a2ff, 2, 1, 18),
        (0x3b89_8d06, 4, 1, 30),
        (0x3b9b_81a7, 1, 0, 6),
        (0x3e14_d564, 6, 2, 48),
    ];
    let samples_f32 = [
        (0.25_f32, 0.000_7_f32),
        (1.0, 0.001_3),
        (-0.5, 0.004_1),
        (0.75, 0.000_2),
        (0.1, 0.012_7),
    ];
    let mut f32_filter = LowPass::<4, f32>::builder(20.0)
        .response(Response::Butterworth)
        .input_model(InputModel::Linear)
        .build()
        .unwrap();
    for ((input, dt), (output_bits, accepted, rejected, evaluations)) in
        samples_f32.into_iter().zip(expected_f32)
    {
        assert_eq!(f32_filter.update(input, dt).unwrap().to_bits(), output_bits);
        let diagnostics = f32_filter.last_diagnostics();
        assert_eq!(diagnostics.accepted_steps(), accepted);
        assert_eq!(diagnostics.rejected_steps(), rejected);
        assert_eq!(diagnostics.derivative_evaluations(), evaluations);
    }
}

#[test]
fn runtime_order_can_use_the_object_safe_streaming_trait() {
    for order in [2, 4, 8] {
        let mut filter: Box<dyn StreamingFilter<Scalar = f64>> = match order {
            2 => Box::new(LowPass::<2>::builder(20.0).build().unwrap()),
            4 => Box::new(LowPass::<4>::builder(20.0).build().unwrap()),
            8 => Box::new(LowPass::<8>::builder(20.0).build().unwrap()),
            _ => unreachable!(),
        };
        let output = filter.update(1.0, 0.01).unwrap();
        assert!(output.is_finite());
    }
}

#[test]
fn runtime_order_can_use_an_allocation_free_f32_enum() {
    enum SensorFilter {
        Two(LowPass<2, f32>),
        Four(LowPass<4, f32>),
    }

    impl SensorFilter {
        fn update(&mut self, input: f32, dt: f32) -> Result<f32, UpdateError> {
            match self {
                Self::Two(filter) => filter.update(input, dt),
                Self::Four(filter) => filter.update(input, dt),
            }
        }
    }

    for order in [2, 4] {
        let mut filter = match order {
            2 => SensorFilter::Two(LowPass::<2, f32>::builder(20.0).build().unwrap()),
            4 => SensorFilter::Four(LowPass::<4, f32>::builder(20.0).build().unwrap()),
            _ => unreachable!(),
        };
        assert!(filter.update(1.0, 0.012).unwrap().is_finite());
    }
}

#[test]
fn chebyshev_response_is_configurable_through_the_public_builder() {
    let mut filter = LowPass::<4>::builder(20.0)
        .response(Response::Chebyshev1 { ripple_db: 0.5 })
        .build()
        .unwrap();
    assert_eq!(filter.response(), Response::Chebyshev1 { ripple_db: 0.5 });
    assert!(filter.update(1.0, 0.01).unwrap().is_finite());
}

#[test]
fn bessel_response_and_order_limit_are_public() {
    let mut filter = LowPass::<4>::builder(20.0)
        .response(Response::Bessel)
        .build()
        .unwrap();
    assert_eq!(filter.response(), Response::Bessel);
    assert!(filter.update(1.0, 0.01).unwrap().is_finite());
    assert_eq!(MAX_BESSEL_ORDER, 25);
    assert!(matches!(
        LowPass::<26>::builder(20.0)
            .response(Response::Bessel)
            .build(),
        Err(BuildError::UnsupportedBesselOrder)
    ));
}

#[test]
fn high_pass_uses_the_common_builder_and_streaming_trait() {
    let mut filter: Box<dyn StreamingFilter<Scalar = f64>> = Box::new(
        HighPass::<1>::builder(20.0)
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .initial_input(1.0)
            .build()
            .unwrap(),
    );
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0);
    let output = filter.update(0.0, 0.01).unwrap();
    assert!(output.is_finite());
    assert!(output < 0.0);
    filter.reset_to_steady(4.0).unwrap();
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0);
}

#[test]
fn high_pass_errors_are_transactional() {
    let mut tested = HighPass::<3>::builder(3.0)
        .response(Response::Bessel)
        .build()
        .unwrap();
    let mut untouched = tested;

    assert_eq!(
        tested.update(f64::NAN, 0.1),
        Err(UpdateError::NonFiniteInput)
    );
    let after_error = tested.update(0.75, 0.04).unwrap();
    let direct = untouched.update(0.75, 0.04).unwrap();
    assert_relative_eq!(after_error, direct, epsilon = 0.0);
}

#[test]
fn band_pass_uses_final_order_and_two_public_cutoff_edges() {
    let mut filter: Box<dyn StreamingFilter<Scalar = f64>> = Box::new(
        BandPass::<4>::builder(10.0, 40.0)
            .response(Response::Butterworth)
            .input_model(InputModel::CurrentHold)
            .build()
            .unwrap(),
    );
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0);
    assert!(filter.update(1.0, 0.01).unwrap().is_finite());
    filter.reset_to_steady(4.0).unwrap();
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0);

    let concrete = BandPass::<8>::builder(10.0, 40.0).build().unwrap();
    assert_relative_eq!(concrete.lower_cutoff_hz(), 10.0, epsilon = 0.0);
    assert_relative_eq!(concrete.upper_cutoff_hz(), 40.0, epsilon = 0.0);
    assert_relative_eq!(concrete.center_hz(), 20.0, epsilon = 0.0);
    assert_eq!(MAX_BESSEL_BAND_PASS_ORDER, 50);
}

#[test]
fn band_pass_rejects_odd_order_and_invalid_edges() {
    assert!(matches!(
        BandPass::<3>::builder(10.0, 40.0).build(),
        Err(BuildError::InvalidBandPassOrder)
    ));
    assert!(matches!(
        BandPass::<4>::builder(40.0, 10.0).build(),
        Err(BuildError::InvalidBandPassEdges)
    ));
}

#[test]
fn band_pass_rejects_unrepresentable_initial_equilibrium_for_each_scalar() {
    for response in [
        Response::Butterworth,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
    ] {
        assert!(matches!(
            BandPass::<4, f32>::builder(1.0, 2.0)
                .response(response)
                .initial_input(f32::MAX)
                .build(),
            Err(BuildError::UnrepresentableInitialEquilibrium)
        ));
    }
    for response in [
        Response::Butterworth,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
    ] {
        assert!(matches!(
            BandPass::<4, f64>::builder(1.0, 2.0)
                .response(response)
                .initial_input(f64::MAX)
                .build(),
            Err(BuildError::UnrepresentableInitialEquilibrium)
        ));
    }
}

#[test]
fn band_pass_rejects_unrepresentable_steady_state_transactionally_for_each_scalar() {
    macro_rules! assert_transactional_reset {
        ($scalar:ty, $maximum:expr, $response:expr) => {{
            let mut tested = BandPass::<4, $scalar>::builder(1.0, 2.0)
                .response($response)
                .build()
                .unwrap();
            tested.update(0.75, 0.04).unwrap();
            let mut untouched = tested;
            let output_before_failure = tested.output();
            let diagnostics_before_failure = tested.last_diagnostics();

            assert_eq!(
                tested.reset_to_steady($maximum),
                Err(ResetError::UnrepresentableSteadyState)
            );
            assert_relative_eq!(tested.output(), output_before_failure, epsilon = 0.0);
            assert_eq!(tested.last_diagnostics(), diagnostics_before_failure);

            assert_relative_eq!(
                tested.update(0.75, 0.04).unwrap(),
                untouched.update(0.75, 0.04).unwrap(),
                epsilon = 0.0
            );
            assert_eq!(tested.last_diagnostics(), untouched.last_diagnostics());
            assert_relative_eq!(
                tested.update(0.5, 0.04).unwrap(),
                untouched.update(0.5, 0.04).unwrap(),
                epsilon = 0.0
            );
            assert_eq!(tested.last_diagnostics(), untouched.last_diagnostics());
        }};
    }

    for response in [
        Response::Butterworth,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
    ] {
        assert_transactional_reset!(f32, f32::MAX, response);
    }
    for response in [
        Response::Butterworth,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
    ] {
        assert_transactional_reset!(f64, f64::MAX, response);
    }
}
