//! Headless retirement-income optimizer: a Rust port of the mixed-integer linear
//! programming model in Owl (<https://github.com/mdlacasse/Owl>).
//!
//! The engine chooses yearly withdrawals from taxable, tax-deferred and Roth
//! accounts, Roth conversions and (optionally) the Social Security claiming
//! month to either maximize inflation-adjusted lifetime spending or maximize the
//! after-tax estate left to heirs. It has no UI or storage dependencies.

mod data;
pub mod diagnostics;
pub mod error;
pub mod markets;
mod model;
pub mod mortality;
mod planner;
pub mod rmd;
pub mod scenario;
pub mod schedule;
pub mod simulation;
pub mod social_security;
pub mod tax;

pub use error::{EngineError, Result};
pub use model::Family;
pub use planner::{diagnose_infeasibility, optimize};
pub use scenario::{
    Accounts, ClaimStrategy, ContributionChoice, Debt, Objective, Person, PlanOptions, Rates,
    RothHistory, Scenario, Sepp, SocialSecurity, WithdrawalOrder, WorkingYear,
};
pub use schedule::{Convergence, LoopResidual, Schedule, SolveStatus, YearPlan};
