//! Medicare Part B and Part D premiums with IRMAA surcharges (CMS 2026), ported
//! from Owl `tax_federal.py` (`mediVals`, `mediCosts`). IRMAA uses MAGI from two
//! years earlier; the cliffs are modelled in the optimizer with one binary per bracket.

use super::FilingStatus;

/// Number of IRMAA tiers (standard premium plus five surcharge tiers).
pub const IRMAA_TIERS: usize = 6;

/// MAGI tier floors in 2026 dollars, `[single, mfj]`. Tier `q` applies above floor `q`.
const IRMAA_FLOORS: [[f64; IRMAA_TIERS]; 2] = [
    [0.0, 109_000.0, 137_000.0, 171_000.0, 205_000.0, 500_000.0],
    [0.0, 218_000.0, 274_000.0, 342_000.0, 410_000.0, 750_000.0],
];

/// Monthly Part B amounts: standard premium, then incremental surcharges per tier.
const PART_B_MONTHLY: [f64; IRMAA_TIERS] = [202.90, 81.20, 121.70, 121.70, 121.70, 40.70];
/// Monthly Part D IRMAA increments per tier (the base plan premium is separate).
const PART_D_MONTHLY: [f64; IRMAA_TIERS] = [0.0, 14.50, 21.80, 21.80, 21.80, 11.10];

/// Age at which Medicare starts.
pub const MEDICARE_AGE: u32 = 65;

/// Inflation-adjusted MAGI floors that separate the tiers (`IRMAA_TIERS - 1` values).
pub fn tier_floors(status: FilingStatus, gamma: f64) -> [f64; IRMAA_TIERS - 1] {
    let floors = IRMAA_FLOORS[status.index()];
    let mut out = [0.0; IRMAA_TIERS - 1];
    for (q, value) in out.iter_mut().enumerate() {
        *value = floors[q + 1] * gamma;
    }
    out
}

/// Annual premium for one enrollee in each tier, inflation-adjusted and cumulative
/// (tier `q` cost includes the standard premium and all lower surcharges).
pub fn tier_annual_costs(gamma: f64, include_part_d: bool) -> [f64; IRMAA_TIERS] {
    let mut costs = [0.0; IRMAA_TIERS];
    let mut cumulative = 0.0;
    for q in 0..IRMAA_TIERS {
        cumulative += PART_B_MONTHLY[q]
            + if include_part_d {
                PART_D_MONTHLY[q]
            } else {
                0.0
            };
        costs[q] = 12.0 * cumulative * gamma;
    }
    costs
}

/// Tier selected by a MAGI value.
pub fn tier_for_magi(magi: f64, status: FilingStatus, gamma: f64) -> usize {
    tier_floors(status, gamma)
        .iter()
        .filter(|floor| magi > **floor)
        .count()
}

/// Annual premium for `enrollees` people given the MAGI from two years earlier.
pub fn annual_premium(
    lookback_magi: f64,
    status: FilingStatus,
    gamma: f64,
    enrollees: u32,
    include_part_d: bool,
) -> f64 {
    let tier = tier_for_magi(lookback_magi, status, gamma);
    f64::from(enrollees) * tier_annual_costs(gamma, include_part_d)[tier]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn standard_premium_applies_below_first_cliff() {
        let premium = annual_premium(100_000.0, FilingStatus::Single, 1.0, 1, false);
        assert!(close(premium, 12.0 * 202.90));
    }

    #[test]
    fn crossing_a_cliff_by_one_dollar_jumps_the_premium() {
        let below = annual_premium(109_000.0, FilingStatus::Single, 1.0, 1, true);
        let above = annual_premium(109_001.0, FilingStatus::Single, 1.0, 1, true);
        assert!(close(above - below, 12.0 * (81.20 + 14.50)));
    }

    #[test]
    fn tiers_are_cumulative_and_indexed() {
        let costs = tier_annual_costs(1.0, false);
        let top: f64 = PART_B_MONTHLY.iter().sum::<f64>() * 12.0;
        assert!(close(costs[IRMAA_TIERS - 1], top));
        assert!(costs.windows(2).all(|w| w[1] > w[0]));
        assert_eq!(tier_for_magi(1_000_000.0, FilingStatus::Single, 1.0), 5);
        assert_eq!(
            tier_for_magi(300_000.0, FilingStatus::MarriedFilingJointly, 1.0),
            2
        );
        // Floors scale with inflation, so the same nominal MAGI drops a tier.
        assert_eq!(tier_for_magi(120_000.0, FilingStatus::Single, 1.2), 0);
    }

    #[test]
    fn premium_scales_with_enrollees() {
        let one = annual_premium(50_000.0, FilingStatus::MarriedFilingJointly, 1.0, 1, true);
        let two = annual_premium(50_000.0, FilingStatus::MarriedFilingJointly, 1.0, 2, true);
        assert!(close(two, 2.0 * one));
    }
}
