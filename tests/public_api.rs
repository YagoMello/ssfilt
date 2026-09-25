use approx::assert_relative_eq;
use ssfilt::{
    BandPass, BuildError, HighPass, InputModel, IntegrationDiagnostics, LowPass,
    MAX_BESSEL_BAND_PASS_ORDER, MAX_BESSEL_ORDER, Response, StreamingFilter, UpdateError,
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
