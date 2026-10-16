//! Canonical coordinate manifest for the first filtered diagnostic slice.
//!
//! This is deliberately a diagnostic manifest, not evidence for the complete
//! response matrix. Its bytes are part of the reproducibility identity, so
//! stable tags and field order are written explicitly rather than through
//! `Debug` formatting.

use core::fmt;

use forward_backward_protocol::FilterOrder;
use sha2::{Digest, Sha256};

use crate::{
    CaseCoordinate, FixtureCoordinate, PhysicalCutoff, QMax, RecordScalar, ScheduleKind,
    TimeDomainFixture,
    identity::{CellVariant, IdentityError, ResponseCellKey, case_id},
};

pub(crate) const MANIFEST_SCHEMA_VERSION: u8 = 1;
pub(crate) const FILTERED_RUN_KIND: &str = "filtered_diagnostic";

const FILTERED_CUTOFFS: [PhysicalCutoff; 2] =
    [PhysicalCutoff::OneHz, PhysicalCutoff::ThirtySevenHz];
const FILTERED_VARIANTS: [CellVariant; 4] = [
    CellVariant::Base,
    CellVariant::ScalarRefined,
    CellVariant::ReversedBase,
    CellVariant::ReversedScalarRefined,
];

/// The fixed coordinates selected by the first report-producing diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FilteredSentinelSelection {
    coordinate: FixtureCoordinate,
    scalar: RecordScalar,
    filter_order: FilterOrder,
}

impl FilteredSentinelSelection {
    #[must_use]
    pub(crate) const fn coordinate(self) -> CaseCoordinate {
        CaseCoordinate::TimeDomain(self.coordinate)
    }

    #[must_use]
    pub(crate) const fn fixture_coordinate(self) -> FixtureCoordinate {
        self.coordinate
    }

    #[must_use]
    pub(crate) const fn scalar(self) -> RecordScalar {
        self.scalar
    }

    #[must_use]
    pub(crate) const fn filter_order(self) -> FilterOrder {
        self.filter_order
    }

    #[must_use]
    pub(crate) const fn cutoffs(self) -> &'static [PhysicalCutoff; 2] {
        &FILTERED_CUTOFFS
    }

    #[must_use]
    pub(crate) const fn variants(self) -> &'static [CellVariant; 4] {
        &FILTERED_VARIANTS
    }
}

/// One canonical response-cell entry in a coordinate manifest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ManifestEntry {
    key: ResponseCellKey,
    case_id: u64,
    cutoff_tag: &'static str,
    variant_tag: &'static str,
}

impl ManifestEntry {
    fn from_key(key: ResponseCellKey) -> Result<Self, ManifestError> {
        Ok(Self {
            case_id: case_id(&key).map_err(ManifestError::Identity)?,
            cutoff_tag: physical_cutoff_tag(key.cutoff()),
            variant_tag: cell_variant_tag(key.variant()),
            key,
        })
    }

    #[must_use]
    pub(crate) const fn key(self) -> ResponseCellKey {
        self.key
    }

    #[must_use]
    pub(crate) const fn case_id(self) -> u64 {
        self.case_id
    }

    #[must_use]
    pub(crate) const fn cutoff_tag(self) -> &'static str {
        self.cutoff_tag
    }

    #[must_use]
    pub(crate) const fn variant_tag(self) -> &'static str {
        self.variant_tag
    }
}

/// The complete canonical data for the first filtered diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FilteredSentinelManifest {
    selection: FilteredSentinelSelection,
    entries: Vec<ManifestEntry>,
}

impl FilteredSentinelManifest {
    #[must_use]
    pub(crate) const fn selection(&self) -> FilteredSentinelSelection {
        self.selection
    }

    #[must_use]
    pub(crate) fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    /// Render the exact UTF-8, LF-terminated coordinate manifest.
    #[must_use]
    pub(crate) fn canonical_bytes(&self) -> Vec<u8> {
        canonical_manifest_bytes(self)
    }

    /// Hash the exact bytes returned by [`Self::canonical_bytes`].
    #[must_use]
    pub(crate) fn coordinate_digest(&self) -> [u8; 32] {
        coordinate_digest(self)
    }
}

/// Failure while constructing a deterministic filtered coordinate manifest.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ManifestError {
    Identity(IdentityError),
    NonAscendingCaseId { previous: u64, current: u64 },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => {
                write!(formatter, "could not derive manifest identity: {error}")
            }
            Self::NonAscendingCaseId { previous, current } => write!(
                formatter,
                "manifest case ID 0x{current:016x} does not follow 0x{previous:016x} in ascending order"
            ),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::NonAscendingCaseId { .. } => None,
        }
    }
}

/// Build the eight-cell reverse-start-sentinel diagnostic in canonical order.
///
/// Variants are the outer axis and cutoffs are the inner axis. That order is
/// also the ascending order of the frozen case-ID encoding. The IDs are
/// derived from semantic keys; no opaque ID table participates in production
/// construction.
pub(crate) fn filtered_sentinel_manifest() -> Result<FilteredSentinelManifest, ManifestError> {
    let selection = FilteredSentinelSelection {
        coordinate: FixtureCoordinate {
            fixture: TimeDomainFixture::RightAdjacentStep,
            q_max: QMax::PiOver32,
            schedule: ScheduleKind::Uneven,
        },
        scalar: RecordScalar::F64,
        filter_order: FilterOrder::Eight,
    };

    let mut entries = Vec::with_capacity(selection.cutoffs().len() * selection.variants().len());
    let mut previous_case_id = None;

    for &variant in selection.variants() {
        for &cutoff in selection.cutoffs() {
            let entry = ManifestEntry::from_key(ResponseCellKey::new(
                selection.coordinate(),
                selection.scalar(),
                cutoff,
                selection.filter_order(),
                variant,
            ))?;
            if let Some(previous) = previous_case_id
                && entry.case_id() <= previous
            {
                return Err(ManifestError::NonAscendingCaseId {
                    previous,
                    current: entry.case_id(),
                });
            }
            previous_case_id = Some(entry.case_id());
            entries.push(entry);
        }
    }

    Ok(FilteredSentinelManifest { selection, entries })
}

/// Render a manifest as canonical CSV without relying on host CSV settings.
///
/// Every emitted value belongs to a restricted ASCII vocabulary without CSV
/// quoting characters. Repeating run metadata on every row keeps the file a
/// single typed table while making its incomplete diagnostic status explicit.
#[must_use]
pub(crate) fn canonical_manifest_bytes(manifest: &FilteredSentinelManifest) -> Vec<u8> {
    let selection = manifest.selection();
    let coordinate = selection.fixture_coordinate();
    let (fixture, q_max, schedule) = (coordinate.fixture, coordinate.q_max, coordinate.schedule);

    let mut output = String::from(
        "schema_version,run_kind,complete,acceptance_eligible,entry_index,case_id,fixture,q_max,schedule,scalar,order,cutoff,variant\n",
    );
    for (index, entry) in manifest.entries().iter().enumerate() {
        output.push_str(&MANIFEST_SCHEMA_VERSION.to_string());
        output.push(',');
        output.push_str(FILTERED_RUN_KIND);
        output.push_str(",false,false,");
        output.push_str(&index.to_string());
        output.push_str(",0x");
        output.push_str(&format!("{:016x}", entry.case_id()));
        output.push(',');
        output.push_str(time_domain_fixture_tag(fixture));
        output.push(',');
        output.push_str(q_max_tag(q_max));
        output.push(',');
        output.push_str(schedule_tag(schedule));
        output.push(',');
        output.push_str(record_scalar_tag(selection.scalar()));
        output.push(',');
        output.push_str(filter_order_tag(selection.filter_order()));
        output.push(',');
        output.push_str(entry.cutoff_tag());
        output.push(',');
        output.push_str(entry.variant_tag());
        output.push('\n');
    }
    output.into_bytes()
}

/// SHA-256 of the exact canonical manifest byte sequence.
#[must_use]
pub(crate) fn coordinate_digest(manifest: &FilteredSentinelManifest) -> [u8; 32] {
    Sha256::digest(canonical_manifest_bytes(manifest)).into()
}

#[must_use]
pub(crate) const fn time_domain_fixture_tag(fixture: TimeDomainFixture) -> &'static str {
    match fixture {
        TimeDomainFixture::UnequalMultitone => "unequal_multitone",
        TimeDomainFixture::GaussianPulse => "gaussian_pulse",
        TimeDomainFixture::TaperedChirp => "tapered_chirp",
        TimeDomainFixture::InteriorStep => "interior_step",
        TimeDomainFixture::LeftAdjacentStep => "left_adjacent_step",
        TimeDomainFixture::RightAdjacentStep => "right_adjacent_step",
        TimeDomainFixture::TriangularPulse => "triangular_pulse",
    }
}

#[must_use]
pub(crate) const fn q_max_tag(q_max: QMax) -> &'static str {
    match q_max {
        QMax::PiOver128 => "pi_over_128",
        QMax::PiOver64 => "pi_over_64",
        QMax::PiOver32 => "pi_over_32",
    }
}

#[must_use]
pub(crate) const fn schedule_tag(schedule: ScheduleKind) -> &'static str {
    match schedule {
        ScheduleKind::Uniform => "uniform",
        ScheduleKind::Jitter => "jitter",
        ScheduleKind::Uneven => "uneven",
    }
}

#[must_use]
pub(crate) const fn record_scalar_tag(scalar: RecordScalar) -> &'static str {
    match scalar {
        RecordScalar::F32 => "f32",
        RecordScalar::F64 => "f64",
    }
}

#[must_use]
pub(crate) const fn filter_order_tag(order: FilterOrder) -> &'static str {
    match order {
        FilterOrder::One => "1",
        FilterOrder::Two => "2",
        FilterOrder::Four => "4",
        FilterOrder::Eight => "8",
    }
}

#[must_use]
pub(crate) const fn physical_cutoff_tag(cutoff: PhysicalCutoff) -> &'static str {
    match cutoff {
        PhysicalCutoff::OneHz => "one_hz",
        PhysicalCutoff::ThirtySevenHz => "thirty_seven_hz",
    }
}

#[must_use]
pub(crate) const fn cell_variant_tag(variant: CellVariant) -> &'static str {
    match variant {
        CellVariant::Base => "base",
        CellVariant::ScalarRefined => "scalar_refined",
        CellVariant::ReversedBase => "reversed_base",
        CellVariant::ReversedScalarRefined => "reversed_scalar_refined",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_CASE_IDS: [u64; 8] = [
        0x1128_63c0_0000_0000,
        0x1138_63c0_0000_0000,
        0x1168_63c0_0000_0000,
        0x1178_63c0_0000_0000,
        0x11a8_63c0_0000_0000,
        0x11b8_63c0_0000_0000,
        0x11e8_63c0_0000_0000,
        0x11f8_63c0_0000_0000,
    ];

    const EXPECTED_BYTES: &[u8] = concat!(
        "schema_version,run_kind,complete,acceptance_eligible,entry_index,case_id,fixture,q_max,schedule,scalar,order,cutoff,variant\n",
        "1,filtered_diagnostic,false,false,0,0x112863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,one_hz,base\n",
        "1,filtered_diagnostic,false,false,1,0x113863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,thirty_seven_hz,base\n",
        "1,filtered_diagnostic,false,false,2,0x116863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,one_hz,scalar_refined\n",
        "1,filtered_diagnostic,false,false,3,0x117863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,thirty_seven_hz,scalar_refined\n",
        "1,filtered_diagnostic,false,false,4,0x11a863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,one_hz,reversed_base\n",
        "1,filtered_diagnostic,false,false,5,0x11b863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,thirty_seven_hz,reversed_base\n",
        "1,filtered_diagnostic,false,false,6,0x11e863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,one_hz,reversed_scalar_refined\n",
        "1,filtered_diagnostic,false,false,7,0x11f863c000000000,right_adjacent_step,pi_over_32,uneven,f64,8,thirty_seven_hz,reversed_scalar_refined\n",
    )
    .as_bytes();

    #[test]
    fn filtered_manifest_has_the_exact_case_ids_in_canonical_order() {
        let manifest = filtered_sentinel_manifest().unwrap();
        let actual: Vec<_> = manifest
            .entries()
            .iter()
            .map(|entry| entry.case_id())
            .collect();

        assert_eq!(actual, EXPECTED_CASE_IDS);
        assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
        for entry in manifest.entries() {
            assert_eq!(case_id(&entry.key()).unwrap(), entry.case_id());
        }
    }

    #[test]
    fn canonical_manifest_bytes_are_exact_and_platform_independent() {
        let manifest = filtered_sentinel_manifest().unwrap();
        let bytes = manifest.canonical_bytes();

        assert_eq!(bytes, EXPECTED_BYTES);
        assert!(bytes.ends_with(b"\n"));
        assert!(!bytes.contains(&b'\r'));
        assert!(core::str::from_utf8(&bytes).is_ok());
    }

    #[test]
    fn coordinate_digest_hashes_the_exact_manifest_bytes() {
        let manifest = filtered_sentinel_manifest().unwrap();
        let expected: [u8; 32] = Sha256::digest(EXPECTED_BYTES).into();

        assert_eq!(manifest.coordinate_digest(), expected);
        assert_eq!(coordinate_digest(&manifest), expected);
    }

    #[test]
    fn selected_axis_tags_are_stable_snake_case() {
        assert_eq!(
            time_domain_fixture_tag(TimeDomainFixture::RightAdjacentStep),
            "right_adjacent_step"
        );
        assert_eq!(q_max_tag(QMax::PiOver32), "pi_over_32");
        assert_eq!(schedule_tag(ScheduleKind::Uneven), "uneven");
        assert_eq!(record_scalar_tag(RecordScalar::F64), "f64");
        assert_eq!(filter_order_tag(FilterOrder::Eight), "8");
        assert_eq!(physical_cutoff_tag(PhysicalCutoff::OneHz), "one_hz");
        assert_eq!(
            physical_cutoff_tag(PhysicalCutoff::ThirtySevenHz),
            "thirty_seven_hz"
        );
        assert_eq!(cell_variant_tag(CellVariant::Base), "base");
        assert_eq!(
            cell_variant_tag(CellVariant::ScalarRefined),
            "scalar_refined"
        );
        assert_eq!(cell_variant_tag(CellVariant::ReversedBase), "reversed_base");
        assert_eq!(
            cell_variant_tag(CellVariant::ReversedScalarRefined),
            "reversed_scalar_refined"
        );
    }
}
