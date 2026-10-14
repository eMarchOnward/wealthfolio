//! Social Security full retirement age and claiming-age adjustment, ported from
//! Owl `socialsecurity.py` (`getFRAs`, `getSelfFactor`, `_reduction_factor`).
//!
//! Claiming is modelled on Owl's monthly grid from age 62 to 70 inclusive
//! ([`CLAIM_MONTHS`] points); the optimizer picks one grid point with a binary.

use crate::error::{EngineError, Result};

/// Earliest and latest claiming ages.
pub const MIN_CLAIM_AGE: f64 = 62.0;
pub const MAX_CLAIM_AGE: f64 = 70.0;
/// Monthly grid from 62.0 to 70.0 inclusive.
pub const CLAIM_MONTHS: usize = 97;

/// Own-benefit reduction: 5/9 of 1% per month for the first 36 months early.
const SELF_REDUCTION_RATE: f64 = 5.0 / 9.0 / 100.0 * 12.0;
/// Further reduction: 5/12 of 1% per month beyond 36 months early.
const EXTENDED_REDUCTION_RATE: f64 = 0.05;
/// Delayed retirement credit: 8% per year after FRA.
const DELAYED_CREDIT_RATE: f64 = 0.08;

/// Full retirement age in fractional years (POMS RS 00615.003). Someone born on
/// January 1 is treated as born in the prior year.
pub fn full_retirement_age(birth_year: i32, birth_month: u32, birth_day: u32) -> f64 {
    let year = if birth_month == 1 && birth_day == 1 {
        birth_year - 1
    } else {
        birth_year
    };
    match year {
        y if y >= 1960 => 67.0,
        y if y >= 1955 => 66.0 + 2.0 * f64::from(y - 1954) / 12.0,
        y if y >= 1943 => 66.0,
        y if y >= 1938 => 65.0 + 2.0 * f64::from(y - 1937) / 12.0,
        _ => 65.0,
    }
}

/// Factor applied to the primary insurance amount (PIA) when claiming at
/// `claim_age` (conventional age, 62–70). People born on the 1st of a month
/// reach each SSA age one month earlier.
pub fn claiming_factor(fra: f64, claim_age: f64, born_on_first: bool) -> Result<f64> {
    if !(MIN_CLAIM_AGE..=MAX_CLAIM_AGE + 1e-9).contains(&claim_age) {
        return Err(EngineError::InvalidInput(format!(
            "claiming age {claim_age} is outside 62–70"
        )));
    }
    let ssa_age = claim_age + if born_on_first { 1.0 / 12.0 } else { 0.0 };
    let months_early = fra - ssa_age; // negative when claiming after FRA
    Ok(if months_early <= 0.0 {
        1.0 - DELAYED_CREDIT_RATE * months_early
    } else if months_early <= 3.0 {
        1.0 - SELF_REDUCTION_RATE * months_early
    } else {
        0.8 - EXTENDED_REDUCTION_RATE * (months_early - 3.0)
    })
}

/// Claiming age of grid point `k` (0 = 62.0, 96 = 70.0).
pub fn claim_age_for_month(k: usize) -> f64 {
    MIN_CLAIM_AGE + k as f64 / 12.0
}

/// Nearest grid point for a claiming age.
pub fn month_for_claim_age(claim_age: f64) -> usize {
    let k = ((claim_age - MIN_CLAIM_AGE) * 12.0).round();
    k.clamp(0.0, (CLAIM_MONTHS - 1) as f64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn fra_follows_birth_year_schedule() {
        assert!(close(full_retirement_age(1950, 6, 15), 66.0));
        assert!(close(full_retirement_age(1957, 6, 15), 66.5));
        assert!(close(full_retirement_age(1964, 6, 15), 67.0));
        // Born Jan 1, 1960 counts as 1959: 66 + 10/12.
        assert!(close(full_retirement_age(1960, 1, 1), 66.0 + 10.0 / 12.0));
        assert!(close(full_retirement_age(1930, 6, 15), 65.0));
    }

    #[test]
    fn claiming_curve_matches_ssa_endpoints() {
        // FRA 67: 70% at 62, 100% at 67, 124% at 70.
        assert!(close(
            claiming_factor(67.0, 62.0, false).expect("in range"),
            0.70
        ));
        assert!(close(
            claiming_factor(67.0, 67.0, false).expect("in range"),
            1.0
        ));
        assert!(close(
            claiming_factor(67.0, 70.0, false).expect("in range"),
            1.24
        ));
        // FRA 66: 75% at 62 and 132% at 70.
        assert!(close(
            claiming_factor(66.0, 62.0, false).expect("in range"),
            0.75
        ));
        assert!(close(
            claiming_factor(66.0, 70.0, false).expect("in range"),
            1.32
        ));
        // 36 months early is exactly the 80% kink.
        assert!(close(
            claiming_factor(67.0, 64.0, false).expect("in range"),
            0.80
        ));
    }

    #[test]
    fn claiming_curve_is_monotone_on_the_monthly_grid() {
        let factors: Vec<f64> = (0..CLAIM_MONTHS)
            .map(|k| claiming_factor(67.0, claim_age_for_month(k), false).expect("grid in range"))
            .collect();
        assert!(factors.windows(2).all(|w| w[1] > w[0]));
        assert!(close(claim_age_for_month(CLAIM_MONTHS - 1), 70.0));
        assert_eq!(month_for_claim_age(67.5), 66);
    }

    #[test]
    fn out_of_range_claiming_ages_are_rejected() {
        assert!(claiming_factor(67.0, 61.9, false).is_err());
        assert!(claiming_factor(67.0, 70.5, false).is_err());
    }
}
