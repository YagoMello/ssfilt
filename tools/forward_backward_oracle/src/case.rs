//! Shared identity for frozen time-domain and tone cases.

use crate::{FixtureCoordinate, ToneCoordinate};

/// Stable coordinate wrapper for either nominal forward-backward case family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaseCoordinate {
    /// One of the shared-duration analytic time-domain records.
    TimeDomain(FixtureCoordinate),
    /// One of the order- and frequency-specific analytic tone records.
    Tone(ToneCoordinate),
}

impl From<FixtureCoordinate> for CaseCoordinate {
    fn from(coordinate: FixtureCoordinate) -> Self {
        Self::TimeDomain(coordinate)
    }
}

impl From<ToneCoordinate> for CaseCoordinate {
    fn from(coordinate: ToneCoordinate) -> Self {
        Self::Tone(coordinate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QMax, ScheduleKind, TimeDomainFixture, ToneFrequency, ToneOrder, TonePhase};

    #[test]
    fn conversions_keep_the_case_family_and_full_coordinate() {
        let fixture = FixtureCoordinate {
            fixture: TimeDomainFixture::RightAdjacentStep,
            q_max: QMax::PiOver64,
            schedule: ScheduleKind::Jitter,
        };
        let tone = ToneCoordinate {
            order: ToneOrder::Eight,
            frequency: ToneFrequency::SevenTenths,
            phase: TonePhase::OneNineteen,
            q_max: QMax::PiOver32,
            schedule: ScheduleKind::Uneven,
        };

        assert_eq!(
            CaseCoordinate::from(fixture),
            CaseCoordinate::TimeDomain(fixture)
        );
        assert_eq!(CaseCoordinate::from(tone), CaseCoordinate::Tone(tone));
        assert_ne!(CaseCoordinate::from(fixture), CaseCoordinate::from(tone));
    }
}
