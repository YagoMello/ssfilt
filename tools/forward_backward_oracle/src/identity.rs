//! Frozen private identities for represented response-matrix records.

use core::fmt;

use forward_backward_protocol::{CaseIdentity, FilterOrder};
use sha2::{Digest, Sha256};

use crate::{
    CaseCoordinate, PhysicalCutoff, RecordScalar, RepresentedRecord, ToneFrequency, ToneOrder,
};

const RECORD_DOMAIN: &[u8] = b"ssfilt.forward-backward.record\0";
const RECORD_SCHEMA: u8 = 1;

/// The four independently scored forms of one response cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CellVariant {
    Base,
    ScalarRefined,
    ReversedBase,
    ReversedScalarRefined,
}

/// The semantic coordinates that distinguish one response-matrix cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ResponseCellKey {
    coordinate: CaseCoordinate,
    scalar: RecordScalar,
    cutoff: PhysicalCutoff,
    filter_order: FilterOrder,
    variant: CellVariant,
}

impl ResponseCellKey {
    #[must_use]
    pub(crate) const fn new(
        coordinate: CaseCoordinate,
        scalar: RecordScalar,
        cutoff: PhysicalCutoff,
        filter_order: FilterOrder,
        variant: CellVariant,
    ) -> Self {
        Self {
            coordinate,
            scalar,
            cutoff,
            filter_order,
            variant,
        }
    }

    #[must_use]
    pub(crate) const fn coordinate(self) -> CaseCoordinate {
        self.coordinate
    }

    #[must_use]
    pub(crate) const fn scalar(self) -> RecordScalar {
        self.scalar
    }

    #[must_use]
    pub(crate) const fn cutoff(self) -> PhysicalCutoff {
        self.cutoff
    }

    #[must_use]
    pub(crate) const fn filter_order(self) -> FilterOrder {
        self.filter_order
    }

    #[must_use]
    pub(crate) const fn variant(self) -> CellVariant {
        self.variant
    }
}

/// Which record count could not be represented by the protocol's `u32` field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordCountField {
    Intervals,
    Samples,
}

/// The represented value that failed exact narrowing to `f32`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecordValueField {
    Cutoff,
    Interval,
    Sample,
}

/// Failure while assigning a protocol identity to a represented response cell.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum IdentityError {
    CountTooLarge {
        field: RecordCountField,
        count: usize,
    },
    NonExactF32 {
        field: RecordValueField,
        index: Option<usize>,
        value: f64,
    },
    ScalarMismatch {
        key_scalar: RecordScalar,
        record_scalar: RecordScalar,
    },
    CutoffMismatch {
        key_cutoff: PhysicalCutoff,
        record_cutoff_hz: f64,
    },
    ToneOrderMismatch {
        coordinate_order: ToneOrder,
        filter_order: FilterOrder,
    },
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountTooLarge { field, count } => {
                write!(formatter, "{field:?} count {count} exceeds u32")
            }
            Self::NonExactF32 {
                field,
                index,
                value,
            } => match index {
                Some(index) => write!(
                    formatter,
                    "represented {field:?} {index} ({value}) does not round-trip exactly through f32"
                ),
                None => write!(
                    formatter,
                    "represented {field:?} ({value}) does not round-trip exactly through f32"
                ),
            },
            Self::ScalarMismatch {
                key_scalar,
                record_scalar,
            } => write!(
                formatter,
                "response key scalar {key_scalar:?} differs from record scalar {record_scalar:?}"
            ),
            Self::CutoffMismatch {
                key_cutoff,
                record_cutoff_hz,
            } => write!(
                formatter,
                "response key cutoff {key_cutoff:?} differs from record cutoff {record_cutoff_hz} Hz"
            ),
            Self::ToneOrderMismatch {
                coordinate_order,
                filter_order,
            } => write!(
                formatter,
                "tone coordinate order {coordinate_order:?} differs from filter order {filter_order:?}"
            ),
        }
    }
}

impl std::error::Error for IdentityError {}

/// Hash the exact scalar-tagged record representation without buffering it.
///
/// The byte stream is the frozen domain separator, schema and scalar tags,
/// native-width cutoff bits, little-endian interval and sample counts, then
/// native-width interval bits followed by sample bits. For an `f32` record,
/// each widened `f64` value is checked before its original-width bits are fed
/// to the incremental hash state.
pub(crate) fn record_digest(record: &RepresentedRecord) -> Result<[u8; 32], IdentityError> {
    let interval_count = checked_count(
        RecordCountField::Intervals,
        record.intervals_seconds().len(),
    )?;
    let sample_count = checked_count(RecordCountField::Samples, record.samples().len())?;

    let mut hasher = Sha256::new();
    hasher.update(RECORD_DOMAIN);
    hasher.update([RECORD_SCHEMA]);

    match record.scalar() {
        RecordScalar::F32 => {
            hasher.update([1]);
            let cutoff = exact_f32_bits(record.cutoff_hz(), RecordValueField::Cutoff, None)?;
            hasher.update(cutoff.to_le_bytes());
            hasher.update(interval_count.to_le_bytes());
            hasher.update(sample_count.to_le_bytes());
            for (index, &interval) in record.intervals_seconds().iter().enumerate() {
                let bits = exact_f32_bits(interval, RecordValueField::Interval, Some(index))?;
                hasher.update(bits.to_le_bytes());
            }
            for (index, &sample) in record.samples().iter().enumerate() {
                let bits = exact_f32_bits(sample, RecordValueField::Sample, Some(index))?;
                hasher.update(bits.to_le_bytes());
            }
        }
        RecordScalar::F64 => {
            hasher.update([2]);
            hasher.update(record.cutoff_hz().to_bits().to_le_bytes());
            hasher.update(interval_count.to_le_bytes());
            hasher.update(sample_count.to_le_bytes());
            for &interval in record.intervals_seconds() {
                hasher.update(interval.to_bits().to_le_bytes());
            }
            for &sample in record.samples() {
                hasher.update(sample.to_bits().to_le_bytes());
            }
        }
    }

    Ok(hasher.finalize().into())
}

/// Encode one frozen semantic coordinate using the protocol's fixed bit map.
///
/// The `u64` layout is fixed as follows:
///
/// | Bits | Meaning and tags |
/// | --- | --- |
/// | 63..60 | Schema version, currently `1`. |
/// | 59..56 | Family: `1` time-domain, `2` tone, `3` reserved for boundary probes. |
/// | 55..54 | Variant: `0` base, `1` scalar-refined, `2` reversed base, `3` reversed scalar-refined. |
/// | 53 | Scalar: `0` `f32`, `1` `f64`. |
/// | 52 | Cutoff: `0` 1 Hz, `1` 37 Hz. |
/// | 51..48 | Literal filter order: `1`, `2`, `4`, or `8`. |
/// | 47..44 | Primary coordinate: time fixture tags `1`..`7` or tone-frequency tags `1`..`5`. |
/// | 43..42 | Phase: `0` for time-domain, tone-phase tags `1` or `2`. |
/// | 41..40 | Maximum-interval tags `1`..`3`. |
/// | 39..38 | Schedule tags `1`..`3`. |
/// | 37..0 | Reserved and required to remain zero. |
pub(crate) fn case_id(key: &ResponseCellKey) -> Result<u64, IdentityError> {
    let variant_tag = match key.variant {
        CellVariant::Base => 0,
        CellVariant::ScalarRefined => 1,
        CellVariant::ReversedBase => 2,
        CellVariant::ReversedScalarRefined => 3,
    };
    let scalar_tag = match key.scalar {
        RecordScalar::F32 => 0,
        RecordScalar::F64 => 1,
    };
    let cutoff_tag = match key.cutoff {
        PhysicalCutoff::OneHz => 0,
        PhysicalCutoff::ThirtySevenHz => 1,
    };
    let order_tag = order_tag(key.filter_order);

    let (family_tag, primary_tag, phase_tag, q_max, schedule) = match key.coordinate {
        CaseCoordinate::TimeDomain(coordinate) => (
            1_u64,
            match coordinate.fixture {
                crate::TimeDomainFixture::UnequalMultitone => 1,
                crate::TimeDomainFixture::GaussianPulse => 2,
                crate::TimeDomainFixture::TaperedChirp => 3,
                crate::TimeDomainFixture::InteriorStep => 4,
                crate::TimeDomainFixture::LeftAdjacentStep => 5,
                crate::TimeDomainFixture::RightAdjacentStep => 6,
                crate::TimeDomainFixture::TriangularPulse => 7,
            },
            0_u64,
            coordinate.q_max,
            coordinate.schedule,
        ),
        CaseCoordinate::Tone(coordinate) => {
            let coordinate_order = match coordinate.order {
                ToneOrder::One => FilterOrder::One,
                ToneOrder::Two => FilterOrder::Two,
                ToneOrder::Four => FilterOrder::Four,
                ToneOrder::Eight => FilterOrder::Eight,
            };
            if coordinate_order != key.filter_order {
                return Err(IdentityError::ToneOrderMismatch {
                    coordinate_order: coordinate.order,
                    filter_order: key.filter_order,
                });
            }
            let primary = match coordinate.frequency {
                ToneFrequency::OneSixteenth => 1,
                ToneFrequency::OneQuarter => 2,
                ToneFrequency::SevenTenths => 3,
                ToneFrequency::One => 4,
                ToneFrequency::Two => 5,
            };
            let phase = match coordinate.phase {
                crate::TonePhase::SeventeenHundredths => 1,
                crate::TonePhase::OneNineteen => 2,
            };
            (2_u64, primary, phase, coordinate.q_max, coordinate.schedule)
        }
    };

    let q_max_tag = match q_max {
        crate::QMax::PiOver128 => 1,
        crate::QMax::PiOver64 => 2,
        crate::QMax::PiOver32 => 3,
    };
    let schedule_tag = match schedule {
        crate::ScheduleKind::Uniform => 1,
        crate::ScheduleKind::Jitter => 2,
        crate::ScheduleKind::Uneven => 3,
    };

    Ok((1_u64 << 60)
        | (family_tag << 56)
        | (variant_tag << 54)
        | (scalar_tag << 53)
        | (cutoff_tag << 52)
        | (order_tag << 48)
        | (primary_tag << 44)
        | (phase_tag << 42)
        | (q_max_tag << 40)
        | (schedule_tag << 38))
}

/// Combine the frozen semantic case coordinate with its represented-record digest.
pub(crate) fn identity_for(
    key: &ResponseCellKey,
    record: &RepresentedRecord,
) -> Result<CaseIdentity, IdentityError> {
    // Preserve deterministic error precedence: scalar, coordinate/order,
    // cutoff, then record encoding.
    if key.scalar != record.scalar() {
        return Err(IdentityError::ScalarMismatch {
            key_scalar: key.scalar,
            record_scalar: record.scalar(),
        });
    }
    let case_id = case_id(key)?;
    // The frozen cutoffs (1 Hz and 37 Hz) are exactly representable as both
    // supported scalar widths, so comparing their widened f64 bits is exact.
    if record.cutoff_hz().to_bits() != key.cutoff.hz().to_bits() {
        return Err(IdentityError::CutoffMismatch {
            key_cutoff: key.cutoff,
            record_cutoff_hz: record.cutoff_hz(),
        });
    }
    Ok(CaseIdentity {
        case_id,
        record_digest: record_digest(record)?,
    })
}

fn checked_count(field: RecordCountField, count: usize) -> Result<u32, IdentityError> {
    u32::try_from(count).map_err(|_| IdentityError::CountTooLarge { field, count })
}

fn exact_f32_bits(
    value: f64,
    field: RecordValueField,
    index: Option<usize>,
) -> Result<u32, IdentityError> {
    let narrowed = value as f32;
    if f64::from(narrowed).to_bits() != value.to_bits() {
        return Err(IdentityError::NonExactF32 {
            field,
            index,
            value,
        });
    }
    Ok(narrowed.to_bits())
}

fn order_tag(order: FilterOrder) -> u64 {
    match order {
        FilterOrder::One => 1,
        FilterOrder::Two => 2,
        FilterOrder::Four => 4,
        FilterOrder::Eight => 8,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::{
        FixtureCoordinate, PHYSICAL_CUTOFFS, Q_MAXES, QMax, SCALAR_ORDER, SCHEDULES,
        TIME_DOMAIN_FIXTURES, TONE_FREQUENCIES, TONE_ORDERS, TONE_PHASES, ToneCoordinate,
        ToneFrequency, TonePhase,
    };

    const ORDERS: [FilterOrder; 4] = [
        FilterOrder::One,
        FilterOrder::Two,
        FilterOrder::Four,
        FilterOrder::Eight,
    ];
    const VARIANTS: [CellVariant; 4] = [
        CellVariant::Base,
        CellVariant::ScalarRefined,
        CellVariant::ReversedBase,
        CellVariant::ReversedScalarRefined,
    ];

    fn f32_vector_record() -> RepresentedRecord {
        RepresentedRecord::from_f32(
            37.0,
            vec![0.125, f32::from_bits(1)],
            vec![-0.0, f32::from_bits(1), 0.0],
        )
        .unwrap()
    }

    fn f64_vector_record() -> RepresentedRecord {
        RepresentedRecord::from_f64(
            37.0,
            vec![0.125, f64::from_bits(1)],
            vec![-0.0, f64::from_bits(1), 0.0],
        )
        .unwrap()
    }

    fn fixture_coordinate(
        fixture: crate::TimeDomainFixture,
        q_max: QMax,
        schedule: crate::ScheduleKind,
    ) -> CaseCoordinate {
        CaseCoordinate::TimeDomain(FixtureCoordinate {
            fixture,
            q_max,
            schedule,
        })
    }

    fn all_response_keys() -> Vec<ResponseCellKey> {
        let mut keys = Vec::with_capacity(9_792);
        for fixture in TIME_DOMAIN_FIXTURES {
            for q_max in Q_MAXES {
                for schedule in SCHEDULES {
                    let coordinate = fixture_coordinate(fixture, q_max, schedule);
                    for scalar in SCALAR_ORDER {
                        for cutoff in PHYSICAL_CUTOFFS {
                            for filter_order in ORDERS {
                                for variant in VARIANTS {
                                    keys.push(ResponseCellKey::new(
                                        coordinate,
                                        scalar,
                                        cutoff,
                                        filter_order,
                                        variant,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        for order in TONE_ORDERS {
            let filter_order = match order {
                ToneOrder::One => FilterOrder::One,
                ToneOrder::Two => FilterOrder::Two,
                ToneOrder::Four => FilterOrder::Four,
                ToneOrder::Eight => FilterOrder::Eight,
            };
            for frequency in TONE_FREQUENCIES {
                for phase in TONE_PHASES {
                    for q_max in Q_MAXES {
                        for schedule in SCHEDULES {
                            let coordinate = CaseCoordinate::Tone(ToneCoordinate {
                                order,
                                frequency,
                                phase,
                                q_max,
                                schedule,
                            });
                            for scalar in SCALAR_ORDER {
                                for cutoff in PHYSICAL_CUTOFFS {
                                    for variant in VARIANTS {
                                        keys.push(ResponseCellKey::new(
                                            coordinate,
                                            scalar,
                                            cutoff,
                                            filter_order,
                                            variant,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        keys
    }

    #[test]
    fn canonical_sha256_vectors_cover_native_bits_and_signed_zero() {
        let f32_record = f32_vector_record();
        assert_eq!(f32_record.samples()[0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(
            f32_record.samples()[1].to_bits(),
            f64::from(f32::from_bits(1)).to_bits()
        );
        assert_eq!(
            record_digest(&f32_record).unwrap(),
            [
                0xb3, 0x86, 0x22, 0x94, 0x48, 0x41, 0xc2, 0xb7, 0x38, 0xc7, 0xc8, 0xe0, 0x71, 0xb7,
                0x2e, 0xf1, 0x0b, 0x47, 0xce, 0x41, 0x2d, 0x24, 0xbc, 0x17, 0x72, 0x5e, 0x3f, 0xe7,
                0x68, 0x3f, 0x9f, 0x14,
            ]
        );

        let f64_record = f64_vector_record();
        assert_eq!(f64_record.samples()[0].to_bits(), (-0.0_f64).to_bits());
        assert_eq!(f64_record.samples()[1].to_bits(), 1);
        assert_eq!(
            record_digest(&f64_record).unwrap(),
            [
                0x49, 0x6b, 0xf0, 0x7d, 0xdb, 0xd3, 0xaf, 0x4a, 0x12, 0xc5, 0xd6, 0x41, 0x8d, 0x57,
                0x71, 0x63, 0x08, 0x0a, 0x0e, 0x58, 0x66, 0x89, 0x99, 0x93, 0xc7, 0x5b, 0x99, 0x40,
                0x58, 0xae, 0x1a, 0xda,
            ]
        );
    }

    #[test]
    fn digest_is_repeatable_and_changes_for_each_record_field_and_length() {
        let baseline = RepresentedRecord::from_f64(37.0, vec![0.125], vec![-0.0, 1.0]).unwrap();
        let digest = record_digest(&baseline).unwrap();
        assert_eq!(record_digest(&baseline).unwrap(), digest);

        let scalar_change =
            RepresentedRecord::from_f32(37.0, vec![0.125], vec![-0.0, 1.0]).unwrap();
        let cutoff_change =
            RepresentedRecord::from_f64(36.0, vec![0.125], vec![-0.0, 1.0]).unwrap();
        let interval_change =
            RepresentedRecord::from_f64(37.0, vec![0.25], vec![-0.0, 1.0]).unwrap();
        let sample_change =
            RepresentedRecord::from_f64(37.0, vec![0.125], vec![-0.0, 2.0]).unwrap();
        let length_change =
            RepresentedRecord::from_f64(37.0, vec![0.125, 0.25], vec![-0.0, 1.0, 2.0]).unwrap();

        for changed in [
            scalar_change,
            cutoff_change,
            interval_change,
            sample_change,
            length_change,
        ] {
            assert_ne!(record_digest(&changed).unwrap(), digest);
        }
    }

    #[test]
    fn f32_digest_rejects_every_non_exact_value_with_its_field_and_index() {
        let value = 37.000_000_1_f64;
        assert!(matches!(
            exact_f32_bits(value, RecordValueField::Cutoff, None),
            Err(IdentityError::NonExactF32 {
                field: RecordValueField::Cutoff,
                index: None,
                value: found,
            }) if found.to_bits() == value.to_bits()
        ));
        assert!(matches!(
            exact_f32_bits(value, RecordValueField::Interval, Some(3)),
            Err(IdentityError::NonExactF32 {
                field: RecordValueField::Interval,
                index: Some(3),
                value: found,
            }) if found.to_bits() == value.to_bits()
        ));
        assert!(matches!(
            exact_f32_bits(value, RecordValueField::Sample, Some(7)),
            Err(IdentityError::NonExactF32 {
                field: RecordValueField::Sample,
                index: Some(7),
                value: found,
            }) if found.to_bits() == value.to_bits()
        ));
        assert_eq!(
            exact_f32_bits(-0.0, RecordValueField::Sample, Some(0)).unwrap(),
            0x8000_0000
        );
        assert_eq!(
            exact_f32_bits(
                f64::from(f32::from_bits(1)),
                RecordValueField::Sample,
                Some(1)
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn digest_does_not_depend_on_coordinate_order_or_variant() {
        let record = RepresentedRecord::from_f64(1.0, vec![0.125], vec![0.0, 1.0]).unwrap();
        let first_key = ResponseCellKey::new(
            fixture_coordinate(
                crate::TimeDomainFixture::UnequalMultitone,
                QMax::PiOver128,
                crate::ScheduleKind::Uniform,
            ),
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            FilterOrder::One,
            CellVariant::Base,
        );
        let other_key = ResponseCellKey::new(
            fixture_coordinate(
                crate::TimeDomainFixture::GaussianPulse,
                QMax::PiOver32,
                crate::ScheduleKind::Uneven,
            ),
            RecordScalar::F64,
            PhysicalCutoff::OneHz,
            FilterOrder::Eight,
            CellVariant::ReversedScalarRefined,
        );
        let first = identity_for(&first_key, &record).unwrap();
        let other = identity_for(&other_key, &record).unwrap();
        assert_ne!(first.case_id, other.case_id);
        assert_eq!(first.record_digest, other.record_digest);
    }

    #[test]
    fn case_ids_match_independent_first_interior_and_last_vectors() {
        let keys = all_response_keys();
        assert_eq!(keys.len(), 9_792);
        assert_eq!(case_id(&keys[0]).unwrap(), 0x1101_1140_0000_0000);
        assert_eq!(case_id(&keys[4_896]).unwrap(), 0x1201_4540_0000_0000);
        assert_eq!(
            case_id(keys.last().unwrap()).unwrap(),
            0x12f8_5bc0_0000_0000
        );
    }

    #[test]
    fn every_base_cell_and_variant_has_a_unique_case_id() {
        let keys = all_response_keys();
        assert_eq!(keys.len(), 2_448 * 4);
        let mut ids = HashSet::with_capacity(keys.len());
        for key in &keys {
            let id = case_id(key).unwrap();
            assert!(ids.insert(id), "duplicate case ID {id:#018x} for {key:?}");
        }
        assert_eq!(ids.len(), 9_792);
    }

    #[test]
    fn counts_are_checked_against_the_protocol_width() {
        assert_eq!(
            checked_count(RecordCountField::Samples, u32::MAX as usize),
            Ok(u32::MAX)
        );
        if usize::BITS > u32::BITS {
            let too_large = usize::try_from(u64::from(u32::MAX) + 1).unwrap();
            assert_eq!(
                checked_count(RecordCountField::Intervals, too_large),
                Err(IdentityError::CountTooLarge {
                    field: RecordCountField::Intervals,
                    count: too_large,
                })
            );
        }
    }

    #[test]
    fn identity_rejects_key_scalar_mismatch_and_tone_order_mismatch() {
        let record = RepresentedRecord::from_f64(1.0, vec![], vec![0.0]).unwrap();
        let scalar_mismatch = ResponseCellKey::new(
            fixture_coordinate(
                crate::TimeDomainFixture::UnequalMultitone,
                QMax::PiOver128,
                crate::ScheduleKind::Uniform,
            ),
            RecordScalar::F32,
            PhysicalCutoff::ThirtySevenHz,
            FilterOrder::One,
            CellVariant::Base,
        );
        assert!(matches!(
            identity_for(&scalar_mismatch, &record),
            Err(IdentityError::ScalarMismatch {
                key_scalar: RecordScalar::F32,
                record_scalar: RecordScalar::F64,
            })
        ));

        let tone_mismatch = ResponseCellKey::new(
            CaseCoordinate::Tone(ToneCoordinate {
                order: ToneOrder::One,
                frequency: ToneFrequency::OneSixteenth,
                phase: TonePhase::SeventeenHundredths,
                q_max: QMax::PiOver128,
                schedule: crate::ScheduleKind::Uniform,
            }),
            RecordScalar::F64,
            PhysicalCutoff::ThirtySevenHz,
            FilterOrder::Eight,
            CellVariant::Base,
        );
        assert!(matches!(
            case_id(&tone_mismatch),
            Err(IdentityError::ToneOrderMismatch {
                coordinate_order: ToneOrder::One,
                filter_order: FilterOrder::Eight,
            })
        ));
        assert!(matches!(
            identity_for(&tone_mismatch, &record),
            Err(IdentityError::ToneOrderMismatch {
                coordinate_order: ToneOrder::One,
                filter_order: FilterOrder::Eight,
            })
        ));
    }

    #[test]
    fn identity_rejects_cutoff_mismatches_for_both_scalars_and_directions() {
        let make_key = |scalar, cutoff| {
            ResponseCellKey::new(
                fixture_coordinate(
                    crate::TimeDomainFixture::UnequalMultitone,
                    QMax::PiOver128,
                    crate::ScheduleKind::Uniform,
                ),
                scalar,
                cutoff,
                FilterOrder::One,
                CellVariant::Base,
            )
        };
        let assert_mismatch = |key_cutoff, record: &RepresentedRecord| {
            let key = make_key(record.scalar(), key_cutoff);
            assert!(matches!(
                identity_for(&key, record),
                Err(IdentityError::CutoffMismatch {
                    key_cutoff: reported_key_cutoff,
                    record_cutoff_hz,
                }) if reported_key_cutoff == key_cutoff
                    && record_cutoff_hz.to_bits() == record.cutoff_hz().to_bits()
            ));
        };

        let f32_at_37 = RepresentedRecord::from_f32(37.0, vec![], vec![0.0]).unwrap();
        let f32_at_1 = RepresentedRecord::from_f32(1.0, vec![], vec![0.0]).unwrap();
        let f64_at_37 = RepresentedRecord::from_f64(37.0, vec![], vec![0.0]).unwrap();
        let f64_at_1 = RepresentedRecord::from_f64(1.0, vec![], vec![0.0]).unwrap();

        assert_mismatch(PhysicalCutoff::OneHz, &f32_at_37);
        assert_mismatch(PhysicalCutoff::ThirtySevenHz, &f32_at_1);
        assert_mismatch(PhysicalCutoff::OneHz, &f64_at_37);
        assert_mismatch(PhysicalCutoff::ThirtySevenHz, &f64_at_1);
    }

    #[test]
    fn key_accessors_preserve_every_coordinate_component() {
        let coordinate = fixture_coordinate(
            crate::TimeDomainFixture::TriangularPulse,
            QMax::PiOver64,
            crate::ScheduleKind::Jitter,
        );
        let key = ResponseCellKey::new(
            coordinate,
            RecordScalar::F32,
            PhysicalCutoff::ThirtySevenHz,
            FilterOrder::Four,
            CellVariant::ScalarRefined,
        );
        assert_eq!(key.coordinate(), coordinate);
        assert_eq!(key.scalar(), RecordScalar::F32);
        assert_eq!(key.cutoff(), PhysicalCutoff::ThirtySevenHz);
        assert_eq!(key.filter_order(), FilterOrder::Four);
        assert_eq!(key.variant(), CellVariant::ScalarRefined);
    }
}
