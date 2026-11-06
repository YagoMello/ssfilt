/// Assumption made about the input between consecutive samples.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum InputModel {
    /// Linearly interpolate from the preceding input to the current input.
    #[default]
    Linear,
    /// Hold the preceding input through the elapsed interval.
    PreviousHold,
    /// Hold the current input through the elapsed interval.
    CurrentHold,
}
