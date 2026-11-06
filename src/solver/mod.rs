mod dopri5;
mod input;

pub(crate) use dopri5::DormandPrinceRuntime;
pub(crate) use input::InputSegment;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolverDiagnostics<T> {
    pub(crate) accepted_steps: usize,
    pub(crate) rejected_steps: usize,
    pub(crate) derivative_evaluations: usize,
    pub(crate) smallest_accepted_step: Option<T>,
    pub(crate) largest_accepted_step: Option<T>,
}

impl<T> SolverDiagnostics<T> {
    pub(crate) const fn new() -> Self {
        Self {
            accepted_steps: 0,
            rejected_steps: 0,
            derivative_evaluations: 0,
            smallest_accepted_step: None,
            largest_accepted_step: None,
        }
    }
}
