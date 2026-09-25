/// Analog low-pass response family.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum Response {
    /// Identical real poles, normalized so the complete filter is −3 dB at the
    /// requested cutoff.
    #[default]
    RepeatedPole,
    /// Maximally flat passband with −3 dB gain at the requested cutoff.
    Butterworth,
}
