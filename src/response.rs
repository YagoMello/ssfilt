/// Largest order supported by [`Response::Bessel`].
///
/// Other response families are not subject to this limit.
pub const MAX_BESSEL_ORDER: usize = 25;

/// Analog response family used by the supported filter topologies.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum Response<T = f64> {
    /// Identical real poles, normalized so the complete filter is −3 dB at the
    /// requested cutoff.
    #[default]
    RepeatedPole,
    /// Maximally flat passband with −3 dB gain at the requested cutoff.
    Butterworth,
    /// Maximally flat group delay at DC with little transient ringing.
    ///
    /// The response has unity DC gain and is magnitude-normalized to −3 dB at
    /// the requested cutoff. Orders through [`MAX_BESSEL_ORDER`] are supported.
    Bessel,
    /// Equiripple passband with a steeper transition than Butterworth.
    ///
    /// `ripple_db` is the peak-to-peak passband ripple in decibels. It must be
    /// finite, greater than zero, and less than 3.0103 dB. The response is
    /// normalized to unity DC gain and −3 dB at the requested cutoff.
    Chebyshev1 {
        /// Peak-to-peak passband ripple in decibels.
        ripple_db: T,
    },
}
