//! Run-level selection of one private batch candidate.

use clap::ValueEnum;

/// Private implementation exercised by one diagnostic run.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CandidateKind {
    /// Endpoint-only reconstruction retained as a negative control.
    Endpoint,
    /// Bounded full-state knot storage with coupled trajectory replay.
    Trajectory,
}

impl CandidateKind {
    pub(crate) const fn client_test_filter(self) -> &'static str {
        match self {
            Self::Endpoint => "batch_matrix_client::batch_matrix_endpoint_client",
            Self::Trajectory => "batch_matrix_client::batch_matrix_trajectory_client",
        }
    }

    pub(crate) const fn report_label(self) -> &'static str {
        match self {
            Self::Endpoint => "endpoint-replay negative control",
            Self::Trajectory => "bounded trajectory replay",
        }
    }

    pub(crate) const fn report_note(self) -> &'static str {
        match self {
            Self::Endpoint => {
                "This endpoint-replay negative control omits the physical forward tail, so every cell is expected to fail the tail gate even when whole-record error happens to pass."
            }
            Self::Trajectory => {
                "This bounded candidate stores forward knot states, replays each continuous trajectory interval during the reverse pass, and uses the analytic constant-continuation tail. Passing this filtered slice is diagnostic only; candidate selection still requires the complete frozen matrix."
            }
        }
    }

    pub(crate) const fn attempts_per_interval(self) -> u64 {
        match self {
            Self::Endpoint => 128,
            Self::Trajectory => 192,
        }
    }

    pub(crate) const fn derivatives_per_interval(self) -> u64 {
        match self {
            Self::Endpoint => 896,
            Self::Trajectory => 1_344,
        }
    }
}
