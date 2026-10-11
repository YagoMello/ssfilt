use approx::assert_relative_eq;
use ssfilt::{
    BandPass, BuildError, HighPass, InputModel, IntegrationConfig, IntegrationDiagnostics, LowPass,
    MAX_BESSEL_BAND_PASS_ORDER, MAX_BESSEL_ORDER, ResetError, Response, StreamingFilter,
    UpdateError,
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
    assert_relative_eq!(after_error, direct, epsilon = 0.0, max_relative = 0.0);
}

#[test]
fn public_reset_establishes_a_new_equilibrium() {
    let mut filter = LowPass::<6>::builder(100.0)
        .response(Response::Butterworth)
        .build()
        .unwrap();
    filter.reset_to_steady(42.0).unwrap();
    assert_relative_eq!(
        filter.update(42.0, 50.0).unwrap(),
        42.0,
        epsilon = 0.0,
        max_relative = 0.0
    );
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
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0, max_relative = 0.0);
    let output = filter.update(0.0, 0.01).unwrap();
    assert!(output.is_finite());
    assert!(output < 0.0);
    filter.reset_to_steady(4.0).unwrap();
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0, max_relative = 0.0);
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
    assert_relative_eq!(after_error, direct, epsilon = 0.0, max_relative = 0.0);
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
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0, max_relative = 0.0);
    assert!(filter.update(1.0, 0.01).unwrap().is_finite());
    filter.reset_to_steady(4.0).unwrap();
    assert_relative_eq!(filter.output(), 0.0, epsilon = 0.0, max_relative = 0.0);

    let concrete = BandPass::<8>::builder(10.0, 40.0).build().unwrap();
    assert_relative_eq!(
        concrete.lower_cutoff_hz(),
        10.0,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_relative_eq!(
        concrete.upper_cutoff_hz(),
        40.0,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_relative_eq!(
        concrete.center_hz(),
        20.0,
        epsilon = 1.0e-14,
        max_relative = 1.0e-14
    );
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
            assert_relative_eq!(
                tested.output(),
                output_before_failure,
                epsilon = 0.0,
                max_relative = 0.0
            );
            assert_eq!(tested.last_diagnostics(), diagnostics_before_failure);

            assert_relative_eq!(
                tested.update(0.75, 0.04).unwrap(),
                untouched.update(0.75, 0.04).unwrap(),
                epsilon = 0.0,
                max_relative = 0.0
            );
            assert_eq!(tested.last_diagnostics(), untouched.last_diagnostics());
            assert_relative_eq!(
                tested.update(0.5, 0.04).unwrap(),
                untouched.update(0.5, 0.04).unwrap(),
                epsilon = 0.0,
                max_relative = 0.0
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

#[test]
fn phase_equalizer_supports_only_validated_butterworth_orders_and_reports_metadata() {
    macro_rules! check_design {
        ($order:literal, $scalar:ty, $tau:expr, $target:expr, $added:expr, $tolerance:expr) => {{
            let tolerance: $scalar = $tolerance;
            let cutoff_a: $scalar = 2.0;
            let cutoff_b: $scalar = 8.0;
            let first = LowPass::<$order, $scalar>::builder(cutoff_a)
                .response(Response::Butterworth)
                .build_phase_equalized()
                .unwrap();
            let second = LowPass::<$order, $scalar>::builder(cutoff_b)
                .response(Response::Butterworth)
                .build_phase_equalized()
                .unwrap();

            assert_relative_eq!(
                first.cutoff_hz(),
                cutoff_a,
                epsilon = 0.0,
                max_relative = 0.0
            );
            assert_relative_eq!(
                second.cutoff_hz(),
                cutoff_b,
                epsilon = 0.0,
                max_relative = 0.0
            );
            assert_relative_eq!(
                first.validated_band_edge_hz() / cutoff_a,
                0.7,
                epsilon = tolerance,
                max_relative = tolerance
            );
            assert_relative_eq!(
                second.validated_band_edge_hz() / cutoff_b,
                0.7,
                epsilon = tolerance,
                max_relative = tolerance
            );

            let tau: $scalar = $tau;
            assert_relative_eq!(
                first.target_constant_delay_seconds() * tau * cutoff_a,
                $target,
                epsilon = tolerance,
                max_relative = tolerance
            );
            assert_relative_eq!(
                first.mean_added_delay_seconds() * tau * cutoff_a,
                $added,
                epsilon = tolerance,
                max_relative = tolerance
            );
            assert_relative_eq!(
                first.target_constant_delay_seconds() / second.target_constant_delay_seconds(),
                4.0,
                epsilon = tolerance,
                max_relative = tolerance
            );
            assert_relative_eq!(
                first.mean_added_delay_seconds() / second.mean_added_delay_seconds(),
                4.0,
                epsilon = tolerance,
                max_relative = tolerance
            );
        }};
    }

    check_design!(
        4,
        f32,
        core::f32::consts::TAU,
        6.615_894_3_f32,
        3.750_845_7_f32,
        2.0e-6
    );
    check_design!(
        8,
        f32,
        core::f32::consts::TAU,
        9.820_666_f32,
        4.311_202_5_f32,
        2.0e-6
    );
    check_design!(
        4,
        f64,
        core::f64::consts::TAU,
        6.615_894_124_512_561,
        3.750_845_706_889_359,
        2.0e-14
    );
    check_design!(
        8,
        f64,
        core::f64::consts::TAU,
        9.820_666_383_292_307,
        4.311_202_622_209_717,
        2.0e-14
    );
}

#[test]
fn phase_equalizer_rejects_default_and_non_butterworth_responses() {
    assert!(matches!(
        LowPass::<4>::builder(10.0).build_phase_equalized(),
        Err(BuildError::UnsupportedPhaseEqualization)
    ));

    for response in [
        Response::RepeatedPole,
        Response::Bessel,
        Response::Chebyshev1 { ripple_db: 0.5 },
        Response::Chebyshev1 {
            ripple_db: f64::NAN,
        },
    ] {
        assert!(matches!(
            LowPass::<4>::builder(10.0)
                .response(response)
                .build_phase_equalized(),
            Err(BuildError::UnsupportedPhaseEqualization)
        ));
    }
}

#[test]
fn phase_equalizer_rejects_unsupported_orders_and_preserves_validation_precedence() {
    assert!(matches!(
        LowPass::<1>::builder(10.0)
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::UnsupportedPhaseEqualization)
    ));
    assert!(matches!(
        LowPass::<16>::builder(10.0)
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::UnsupportedPhaseEqualization)
    ));

    let invalid_integration = IntegrationConfig {
        max_step_attempts: 0,
        ..IntegrationConfig::default()
    };
    assert!(matches!(
        LowPass::<0>::builder(f64::NAN)
            .response(Response::Bessel)
            .initial_input(f64::NAN)
            .integration(invalid_integration)
            .build_phase_equalized(),
        Err(BuildError::ZeroOrder)
    ));
    assert!(matches!(
        LowPass::<16>::builder(f64::NAN)
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::InvalidCutoff)
    ));
    assert!(matches!(
        LowPass::<16>::builder(10.0)
            .response(Response::RepeatedPole)
            .initial_input(f64::NAN)
            .build_phase_equalized(),
        Err(BuildError::InvalidInitialInput)
    ));
    assert!(matches!(
        LowPass::<16>::builder(10.0)
            .response(Response::RepeatedPole)
            .integration(invalid_integration)
            .build_phase_equalized(),
        Err(BuildError::InvalidIntegrationConfig)
    ));
}

#[test]
fn phase_equalizer_rejects_cutoffs_with_unrepresentable_metadata() {
    assert!(matches!(
        LowPass::<4, f32>::builder(f32::from_bits(1))
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::InvalidCutoff)
    ));
    assert!(matches!(
        LowPass::<4, f64>::builder(f64::from_bits(1))
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::InvalidCutoff)
    ));
    assert!(matches!(
        LowPass::<8, f32>::builder(f32::MAX)
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::InvalidCutoff)
    ));
    assert!(matches!(
        LowPass::<8, f64>::builder(f64::MAX)
            .response(Response::Butterworth)
            .build_phase_equalized(),
        Err(BuildError::InvalidCutoff)
    ));
}

#[test]
fn phase_equalizer_honors_initial_input_input_model_and_step_limit() {
    let mut initialized = LowPass::<4>::builder(5.0)
        .response(Response::Butterworth)
        .initial_input(3.25)
        .build_phase_equalized()
        .unwrap();
    assert_relative_eq!(
        initialized.output(),
        3.25,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_relative_eq!(
        initialized.update(3.25, 10.0).unwrap(),
        3.25,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert!(initialized.last_diagnostics().used_equilibrium_shortcut());

    let mut previous = LowPass::<4>::builder(5.0)
        .response(Response::Butterworth)
        .input_model(InputModel::PreviousHold)
        .build_phase_equalized()
        .unwrap();
    let mut current = LowPass::<4>::builder(5.0)
        .response(Response::Butterworth)
        .input_model(InputModel::CurrentHold)
        .build_phase_equalized()
        .unwrap();
    assert_relative_eq!(
        previous.update(1.0, 0.02).unwrap(),
        0.0,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert!(current.update(1.0, 0.02).unwrap() > 0.0);

    let max_step_seconds = 0.002;
    let integration = IntegrationConfig {
        max_step_seconds: Some(max_step_seconds),
        ..IntegrationConfig::default()
    };
    let mut limited = LowPass::<4>::builder(5.0)
        .response(Response::Butterworth)
        .integration(integration)
        .build_phase_equalized()
        .unwrap();
    limited.update(1.0, 0.05).unwrap();
    let diagnostics = limited.last_diagnostics();
    assert!(diagnostics.accepted_steps() > 1);
    assert!(diagnostics.largest_accepted_step_seconds().unwrap() <= max_step_seconds + 1.0e-14);
}

#[test]
fn phase_equalizer_supports_trait_objects_and_transactional_reset_and_updates() {
    let mut boxed: Box<dyn StreamingFilter<Scalar = f64>> = Box::new(
        LowPass::<4>::builder(4.0)
            .response(Response::Butterworth)
            .input_model(InputModel::PreviousHold)
            .build_phase_equalized()
            .unwrap(),
    );
    assert_relative_eq!(boxed.output(), 0.0, epsilon = 0.0, max_relative = 0.0);
    assert_relative_eq!(
        boxed.update(1.0, 0.01).unwrap(),
        0.0,
        epsilon = 0.0,
        max_relative = 0.0
    );

    boxed.reset_to_steady(2.0).unwrap();
    assert_relative_eq!(boxed.output(), 2.0, epsilon = 0.0, max_relative = 0.0);
    boxed.reset();
    assert_relative_eq!(boxed.output(), 0.0, epsilon = 0.0, max_relative = 0.0);

    let mut tested = LowPass::<4>::builder(4.0)
        .response(Response::Butterworth)
        .input_model(InputModel::PreviousHold)
        .build_phase_equalized()
        .unwrap();
    tested.update(0.75, 0.02).unwrap();
    tested.update(0.5, 0.02).unwrap();
    let mut reference = tested;
    let output_before_failure = tested.output();
    let diagnostics_before_failure = tested.last_diagnostics();

    assert_eq!(
        tested.update(f64::NAN, 0.02),
        Err(UpdateError::NonFiniteInput)
    );
    assert_relative_eq!(
        tested.output(),
        output_before_failure,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_eq!(tested.last_diagnostics(), diagnostics_before_failure);
    assert_relative_eq!(
        tested.update(0.25, 0.03).unwrap(),
        reference.update(0.25, 0.03).unwrap(),
        epsilon = 0.0,
        max_relative = 0.0
    );

    let diagnostics_before_reset = tested.last_diagnostics();
    let output_before_reset = tested.output();
    let mut reset_reference = tested;
    assert_eq!(
        tested.reset_to_steady(f64::NAN),
        Err(ResetError::NonFiniteInput)
    );
    assert_relative_eq!(
        tested.output(),
        output_before_reset,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_eq!(tested.last_diagnostics(), diagnostics_before_reset);
    assert_relative_eq!(
        tested.update(0.125, 0.03).unwrap(),
        reset_reference.update(0.125, 0.03).unwrap(),
        epsilon = 0.0,
        max_relative = 0.0
    );

    tested.reset_to_steady(1.5).unwrap();
    assert_relative_eq!(tested.output(), 1.5, epsilon = 0.0, max_relative = 0.0);
    assert_eq!(tested.last_diagnostics(), IntegrationDiagnostics::default());
    assert_relative_eq!(
        tested.update(1.5, 10.0).unwrap(),
        1.5,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert!(tested.last_diagnostics().used_equilibrium_shortcut());
    tested.reset();
    assert_relative_eq!(tested.output(), 0.0, epsilon = 0.0, max_relative = 0.0);
    assert_eq!(tested.last_diagnostics(), IntegrationDiagnostics::default());
}

#[test]
fn phase_equalizer_invalid_delta_time_is_transactional() {
    for invalid_dt in [0.0, -0.1, f64::NAN] {
        let mut tested = LowPass::<4>::builder(4.0)
            .response(Response::Butterworth)
            .input_model(InputModel::Linear)
            .build_phase_equalized()
            .unwrap();
        tested.update(0.75, 0.02).unwrap();
        let mut reference = tested;
        let output_before_failure = tested.output();
        let diagnostics_before_failure = tested.last_diagnostics();

        assert_eq!(
            tested.update(0.5, invalid_dt),
            Err(UpdateError::InvalidDeltaTime)
        );
        assert_relative_eq!(
            tested.output(),
            output_before_failure,
            epsilon = 0.0,
            max_relative = 0.0
        );
        assert_eq!(tested.last_diagnostics(), diagnostics_before_failure);
        assert_relative_eq!(
            tested.update(0.25, 0.01).unwrap(),
            reference.update(0.25, 0.01).unwrap(),
            epsilon = 0.0,
            max_relative = 0.0
        );
        assert_eq!(tested.last_diagnostics(), reference.last_diagnostics());
    }
}

#[test]
fn phase_equalizer_step_budget_failure_is_transactional() {
    let integration = IntegrationConfig {
        max_step_seconds: Some(1.0e-4),
        max_step_attempts: 1,
        ..IntegrationConfig::default()
    };
    let mut tested = LowPass::<4>::builder(4.0)
        .response(Response::Butterworth)
        .input_model(InputModel::Linear)
        .integration(integration)
        .build_phase_equalized()
        .unwrap();
    tested.update(0.75, 1.0e-5).unwrap();
    let mut reference = tested;
    let output_before_failure = tested.output();
    let diagnostics_before_failure = tested.last_diagnostics();

    assert_eq!(
        tested.update(-0.5, 0.1),
        Err(UpdateError::StepBudgetExceeded)
    );
    assert_relative_eq!(
        tested.output(),
        output_before_failure,
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_eq!(tested.last_diagnostics(), diagnostics_before_failure);
    assert_relative_eq!(
        tested.update(0.25, 1.0e-6).unwrap(),
        reference.update(0.25, 1.0e-6).unwrap(),
        epsilon = 0.0,
        max_relative = 0.0
    );
    assert_eq!(tested.last_diagnostics(), reference.last_diagnostics());
}
