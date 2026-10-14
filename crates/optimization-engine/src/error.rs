use thiserror::Error;

/// Errors returned by the optimization engine.
#[derive(Debug, Error)]
pub enum EngineError {
    /// A scenario input is outside the range the model supports.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The solver proved that no plan satisfies every constraint.
    /// The report lists the constraint families and years that had to be relaxed.
    #[error("the plan is infeasible:\n{0}")]
    Infeasible(String),

    /// The objective can grow without limit (a modelling error, never expected).
    #[error("the plan is unbounded")]
    Unbounded,

    /// Any other solver failure.
    #[error("solver error: {0}")]
    Solver(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;
