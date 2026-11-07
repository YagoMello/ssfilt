use crate::scalar::{from_f64, from_usize};
use crate::{BuildError, Response, Scalar};

use super::bessel_table;

#[derive(Clone, Copy, Debug)]
pub(crate) enum PrototypeFactor<T> {
    Real { rate: T },
    Quadratic { damping: T, frequency_squared: T },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PrototypeFactors<T, const N: usize> {
    factors: [Option<PrototypeFactor<T>>; N],
    count: usize,
}

impl<T: Scalar, const N: usize> PrototypeFactors<T, N> {
    pub(crate) fn new(
        response: Response<T>,
        order: usize,
        invalid_geometry: BuildError,
    ) -> Result<Self, BuildError> {
        if order == 0 || order > N {
            return Err(invalid_geometry);
        }

        let mut factors = Self {
            factors: [None; N],
            count: 0,
        };
        match response {
            Response::RepeatedPole => {
                let rate = T::one() / (T::LN_2() / from_usize::<T>(order)).exp_m1().sqrt();
                for _ in 0..order {
                    factors.push(PrototypeFactor::Real { rate }, invalid_geometry)?;
                }
            }
            Response::Butterworth => {
                if !order.is_multiple_of(2) {
                    factors.push(PrototypeFactor::Real { rate: T::one() }, invalid_geometry)?;
                }
                let denominator = from_f64::<T>(2.0) * from_usize::<T>(order);
                for index in 0..order / 2 {
                    let angle = T::PI() * from_usize::<T>(2 * index + 1) / denominator;
                    factors.push(
                        PrototypeFactor::Quadratic {
                            damping: from_f64::<T>(2.0) * angle.sin(),
                            frequency_squared: T::one(),
                        },
                        invalid_geometry,
                    )?;
                }
            }
            Response::Bessel => {
                let prototype =
                    bessel_table::prototype(order).ok_or(BuildError::UnsupportedBesselOrder)?;
                if !order.is_multiple_of(2) {
                    factors.push(
                        PrototypeFactor::Real {
                            rate: from_f64(prototype.real_rate),
                        },
                        invalid_geometry,
                    )?;
                }
                for (&damping, &frequency_squared) in
                    prototype.damping.iter().zip(prototype.frequency_squared)
                {
                    factors.push(
                        PrototypeFactor::Quadratic {
                            damping: from_f64(damping),
                            frequency_squared: from_f64(frequency_squared),
                        },
                        invalid_geometry,
                    )?;
                }
            }
            Response::Chebyshev1 { ripple_db } => {
                factors.add_chebyshev(order, ripple_db, invalid_geometry)?;
            }
        }

        if !factors.is_valid() {
            return Err(invalid_geometry);
        }
        factors.sort_low_q_first();
        Ok(factors)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = PrototypeFactor<T>> + '_ {
        self.factors[..self.count]
            .iter()
            .map(|factor| factor.unwrap())
    }

    fn add_chebyshev(
        &mut self,
        order: usize,
        ripple_db: T,
        invalid_geometry: BuildError,
    ) -> Result<(), BuildError> {
        let max_ripple_db = from_f64(3.010_299_956_639_812);
        if !ripple_db.is_finite() || ripple_db <= T::zero() || ripple_db >= max_ripple_db {
            return Err(BuildError::InvalidPassbandRipple);
        }

        let order_scalar = from_usize::<T>(order);
        let epsilon_squared = (T::LN_10() * ripple_db / from_f64(10.0)).exp_m1();
        let epsilon = epsilon_squared.sqrt();
        let mu = (T::one() / epsilon).asinh() / order_scalar;
        let sinh_mu = mu.sinh();
        let cosh_mu = mu.cosh();
        let chebyshev_at_cutoff = if order.is_multiple_of(2) {
            (T::one() / epsilon_squared + from_f64(2.0)).sqrt()
        } else {
            T::one() / epsilon
        };
        let cutoff_scale = (chebyshev_at_cutoff.acosh() / order_scalar).cosh();
        if !cutoff_scale.is_finite() || cutoff_scale <= T::zero() {
            return Err(BuildError::InvalidPassbandRipple);
        }

        if !order.is_multiple_of(2) {
            self.push(
                PrototypeFactor::Real {
                    rate: sinh_mu / cutoff_scale,
                },
                invalid_geometry,
            )?;
        }
        let denominator = from_f64::<T>(2.0) * order_scalar;
        for index in 0..order / 2 {
            let angle = T::PI() * from_usize::<T>(2 * index + 1) / denominator;
            let real = sinh_mu * angle.sin() / cutoff_scale;
            let imaginary = cosh_mu * angle.cos() / cutoff_scale;
            self.push(
                PrototypeFactor::Quadratic {
                    damping: from_f64::<T>(2.0) * real,
                    frequency_squared: real.mul_add(real, imaginary * imaginary),
                },
                invalid_geometry,
            )?;
        }
        Ok(())
    }

    fn push(
        &mut self,
        factor: PrototypeFactor<T>,
        invalid_geometry: BuildError,
    ) -> Result<(), BuildError> {
        let slot = self.factors.get_mut(self.count).ok_or(invalid_geometry)?;
        *slot = Some(factor);
        self.count += 1;
        Ok(())
    }

    fn is_valid(&self) -> bool {
        self.iter().all(|factor| match factor {
            PrototypeFactor::Real { rate } => rate.is_finite() && rate > T::zero(),
            PrototypeFactor::Quadratic {
                damping,
                frequency_squared,
            } => {
                damping.is_finite()
                    && damping > T::zero()
                    && frequency_squared.is_finite()
                    && frequency_squared > T::zero()
            }
        })
    }

    fn sort_low_q_first(&mut self) {
        for index in 1..self.count {
            let mut position = index;
            while position > 0
                && should_precede(
                    self.factors[position].unwrap(),
                    self.factors[position - 1].unwrap(),
                )
            {
                self.factors.swap(position - 1, position);
                position -= 1;
            }
        }
    }
}

fn should_precede<T: Scalar>(candidate: PrototypeFactor<T>, current: PrototypeFactor<T>) -> bool {
    match (candidate, current) {
        (PrototypeFactor::Real { .. }, PrototypeFactor::Quadratic { .. }) => true,
        (PrototypeFactor::Quadratic { .. }, PrototypeFactor::Real { .. })
        | (PrototypeFactor::Real { .. }, PrototypeFactor::Real { .. }) => false,
        (
            PrototypeFactor::Quadratic {
                damping: candidate_damping,
                frequency_squared: candidate_frequency_squared,
            },
            PrototypeFactor::Quadratic {
                damping: current_damping,
                frequency_squared: current_frequency_squared,
            },
        ) => {
            candidate_damping / candidate_frequency_squared.sqrt()
                > current_damping / current_frequency_squared.sqrt()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SecondOrderDenominator<T> {
    pub(crate) damping: T,
    pub(crate) frequency_squared: T,
}

pub(crate) fn factor_transformed_quadratic<T: Scalar>(
    prototype_damping: T,
    prototype_frequency_squared: T,
    fractional_bandwidth: T,
) -> Option<[SecondOrderDenominator<T>; 2]> {
    let two = from_f64::<T>(2.0);
    let four = from_f64::<T>(4.0);
    let bandwidth_squared = fractional_bandwidth * fractional_bandwidth;
    let middle = four + prototype_frequency_squared * bandwidth_squared;
    let discriminant = middle.mul_add(
        middle,
        -(four * prototype_damping * prototype_damping * bandwidth_squared),
    );
    if !discriminant.is_finite() || discriminant < T::zero() {
        return None;
    }

    let discriminant_root = discriminant.sqrt();
    let separation_squared = bandwidth_squared
        * (prototype_frequency_squared
            + (from_f64::<T>(8.0) * prototype_frequency_squared
                - four * prototype_damping * prototype_damping
                + prototype_frequency_squared * prototype_frequency_squared * bandwidth_squared)
                / (discriminant_root + four))
        / two;
    if !separation_squared.is_finite() || separation_squared < T::zero() {
        return None;
    }

    let y = (four + separation_squared).sqrt();
    let separation = separation_squared.sqrt();
    let high_frequency = (y + separation) / two;
    let low_frequency = T::one() / high_frequency;
    let damping_scale = prototype_damping * fractional_bandwidth / y;
    let result = [
        SecondOrderDenominator {
            damping: damping_scale * low_frequency,
            frequency_squared: low_frequency * low_frequency,
        },
        SecondOrderDenominator {
            damping: damping_scale * high_frequency,
            frequency_squared: high_frequency * high_frequency,
        },
    ];
    result
        .iter()
        .all(|section| {
            section.damping.is_finite()
                && section.damping > T::zero()
                && section.frequency_squared.is_finite()
                && section.frequency_squared > T::zero()
        })
        .then_some(result)
}
