//! Static US federal tax and Medicare rules (2026 values, indexed by inflation).
//!
//! Ported from Owl `owlplanner/tax_federal.py`. All thresholds are 2026 nominal
//! dollars; callers scale them by a cumulative inflation factor `gamma` (1.0 in
//! the first plan year) the same way Owl does.

pub mod aca;
pub mod federal;
pub mod irmaa;
pub mod state;

use serde::{Deserialize, Serialize};

/// Federal filing status. Owl models single filers and married couples filing jointly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilingStatus {
    Single,
    MarriedFilingJointly,
}

impl FilingStatus {
    /// Column index into Owl's `[single, mfj]` tables.
    pub(crate) fn index(self) -> usize {
        match self {
            FilingStatus::Single => 0,
            FilingStatus::MarriedFilingJointly => 1,
        }
    }
}
