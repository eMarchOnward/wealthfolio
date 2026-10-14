//! Required minimum distributions (Owl `tax_federal.rho_in`), using the IRS
//! Pub. 590-B Uniform Lifetime table. The Joint Life table (spouse more than ten
//! years younger) is not ported because the engine models a single owner.

use crate::data::tables::{UNIFORM_LIFETIME_BASE_AGE, UNIFORM_LIFETIME_DIVISORS};

/// RMD starting age by birth year (SECURE 1.0 / 2.0).
pub fn rmd_start_age(birth_year: i32) -> u32 {
    match birth_year {
        y if y < 1949 => 70,
        1949 | 1950 => 72,
        1951..=1959 => 73,
        _ => 75,
    }
}

/// Uniform Lifetime divisor at `age`, or `None` below the table's first age.
/// Ages past the end of the table use the last divisor.
pub fn uniform_lifetime_divisor(age: u32) -> Option<f64> {
    let offset = age.checked_sub(UNIFORM_LIFETIME_BASE_AGE)? as usize;
    UNIFORM_LIFETIME_DIVISORS
        .get(offset)
        .or_else(|| UNIFORM_LIFETIME_DIVISORS.last())
        .copied()
}

/// Fraction of the prior year-end tax-deferred balance that must be withdrawn
/// in the year the owner turns `age` (0 before the RMD age).
pub fn rmd_fraction(birth_year: i32, age: u32) -> f64 {
    if age < rmd_start_age(birth_year) {
        return 0.0;
    }
    uniform_lifetime_divisor(age).map_or(0.0, |divisor| 1.0 / divisor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_age_follows_secure_act_cohorts() {
        assert_eq!(rmd_start_age(1945), 70);
        assert_eq!(rmd_start_age(1950), 72);
        assert_eq!(rmd_start_age(1955), 73);
        assert_eq!(rmd_start_age(1964), 75);
    }

    #[test]
    fn fractions_use_uniform_lifetime_divisors() {
        assert_eq!(rmd_fraction(1964, 74), 0.0);
        assert!((rmd_fraction(1964, 75) - 1.0 / 24.6).abs() < 1e-12);
        assert!((rmd_fraction(1955, 73) - 1.0 / 26.5).abs() < 1e-12);
        // Past the table's end the final divisor (2.0 at 120) applies.
        assert!((rmd_fraction(1964, 125) - 0.5).abs() < 1e-12);
        assert_eq!(uniform_lifetime_divisor(60), None);
    }

    #[test]
    fn fractions_increase_with_age() {
        let fractions: Vec<f64> = (75..110).map(|age| rmd_fraction(1964, age)).collect();
        assert!(fractions.windows(2).all(|w| w[1] >= w[0]));
    }
}
