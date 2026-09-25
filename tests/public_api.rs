use approx::assert_relative_eq;
use ssfilt::{InputModel, LowPass, StreamingFilter, UpdateError};

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
    let mut filter = LowPass::<6>::builder(100.0).build().unwrap();
    filter.reset_to_steady(42.0).unwrap();
    assert_relative_eq!(filter.update(42.0, 50.0).unwrap(), 42.0, epsilon = 0.0);
}
