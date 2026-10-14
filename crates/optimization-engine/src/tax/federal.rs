//! Federal income tax: ordinary brackets, standard deduction, long-term capital
//! gains stacking, Social Security taxability and NIIT.
//!
//! Values are the 2026 (OBBBA) figures from Owl `tax_federal.py`. Owl also keeps
//! pre-TCJA brackets for a hypothetical sunset year; OBBBA made the current
//! brackets permanent, so only those are ported.

use super::FilingStatus;

/// OBBBA ordinary income tax rates, lowest bracket first.
pub const ORDINARY_RATES: [f64; 7] = [0.10, 0.12, 0.22, 0.24, 0.32, 0.35, 0.37];

/// Upper edge of each ordinary bracket in 2026 taxable-income dollars, `[single, mfj]`.
/// The top bracket is open-ended.
const BRACKET_TOPS: [[f64; 7]; 2] = [
    [
        12_400.0,
        50_400.0,
        105_700.0,
        201_775.0,
        256_225.0,
        640_600.0,
        f64::INFINITY,
    ],
    [
        24_800.0,
        100_800.0,
        211_400.0,
        403_550.0,
        512_450.0,
        768_700.0,
        f64::INFINITY,
    ],
];

/// 2026 standard deduction, `[single, mfj]`.
const STANDARD_DEDUCTION: [f64; 2] = [16_100.0, 32_200.0];

/// Additional standard deduction per person aged 65+, `[single, mfj (per spouse)]`.
const EXTRA_65_DEDUCTION: [f64; 2] = [2_050.0, 1_650.0];

/// OBBBA §1002 senior "bonus" deduction per person aged 65+ (not indexed).
pub const SENIOR_BONUS: f64 = 6_000.0;
/// The bonus phases out at 6% of MAGI above these thresholds, `[single, mfj]`.
const SENIOR_BONUS_THRESHOLD: [f64; 2] = [75_000.0, 150_000.0];
const SENIOR_BONUS_PHASEOUT_RATE: f64 = 0.06;
/// Last tax year in which the senior bonus applies.
pub const SENIOR_BONUS_LAST_YEAR: i32 = 2028;

/// Taxable income above which LTCG is taxed at 15% and 20%, `[single, mfj]`.
const LTCG_THRESHOLDS: [[f64; 2]; 2] = [[49_450.0, 545_500.0], [98_900.0, 613_700.0]];
pub const LTCG_RATES: [f64; 3] = [0.0, 0.15, 0.20];

/// Social Security provisional-income thresholds (frozen since 1983/1993, not indexed).
const SS_TAXABILITY_LO: [f64; 2] = [25_000.0, 32_000.0];
const SS_TAXABILITY_HI: [f64; 2] = [34_000.0, 44_000.0];
/// At most 85% of benefits are ever taxable.
pub const SS_MAX_TAXABLE_FRACTION: f64 = 0.85;

/// Net investment income tax (IRC §1411); thresholds are not indexed.
const NIIT_THRESHOLD: [f64; 2] = [200_000.0, 250_000.0];
pub const NIIT_RATE: f64 = 0.038;

/// Width of each ordinary bracket in a year with inflation factor `gamma`
/// (the top bracket is unbounded).
pub fn bracket_widths(status: FilingStatus, gamma: f64) -> [f64; 7] {
    let tops = BRACKET_TOPS[status.index()];
    let mut widths = [0.0; 7];
    let mut previous = 0.0;
    for (t, top) in tops.iter().enumerate() {
        widths[t] = if top.is_finite() {
            (top - previous) * gamma
        } else {
            f64::INFINITY
        };
        previous = *top;
    }
    widths
}

/// Standard deduction for the year, including the additional 65+ amounts and,
/// through 2028, the OBBBA senior bonus phased out by `magi`.
pub fn standard_deduction(
    status: FilingStatus,
    gamma: f64,
    people_65_or_older: u32,
    tax_year: i32,
    magi: f64,
) -> f64 {
    let s = status.index();
    let seniors = f64::from(people_65_or_older);
    let mut deduction = (STANDARD_DEDUCTION[s] + seniors * EXTRA_65_DEDUCTION[s]) * gamma;
    if tax_year <= SENIOR_BONUS_LAST_YEAR {
        deduction += seniors * senior_bonus_per_person(status, magi);
    }
    deduction
}

/// OBBBA senior bonus for one qualifying person after the MAGI phase-out.
pub fn senior_bonus_per_person(status: FilingStatus, magi: f64) -> f64 {
    let excess = (magi - SENIOR_BONUS_THRESHOLD[status.index()]).max(0.0);
    (SENIOR_BONUS - SENIOR_BONUS_PHASEOUT_RATE * excess).max(0.0)
}

/// Tax on ordinary taxable income (after deductions).
pub fn ordinary_income_tax(taxable_income: f64, status: FilingStatus, gamma: f64) -> f64 {
    let mut remaining = taxable_income.max(0.0);
    let mut tax = 0.0;
    for (width, rate) in bracket_widths(status, gamma).iter().zip(ORDINARY_RATES) {
        let portion = remaining.min(*width);
        tax += portion * rate;
        remaining -= portion;
        if remaining <= 0.0 {
            break;
        }
    }
    tax
}

/// The 0%→15% and 15%→20% LTCG thresholds on total taxable income.
pub fn ltcg_thresholds(status: FilingStatus, gamma: f64) -> [f64; 2] {
    let t = LTCG_THRESHOLDS[status.index()];
    [t[0] * gamma, t[1] * gamma]
}

/// Tax on long-term capital gains (and qualified dividends), which stack on top
/// of ordinary taxable income (Owl `capitalGainTax`).
pub fn ltcg_tax(ordinary_taxable: f64, ltcg: f64, status: FilingStatus, gamma: f64) -> f64 {
    let ltcg = ltcg.max(0.0);
    let ordinary = ordinary_taxable.max(0.0);
    let [t15, t20] = ltcg_thresholds(status, gamma);
    let top = ordinary + ltcg;
    // Portion of the gains that lies in each rate band once stacked on ordinary income.
    let in_band = |lo: f64, hi: f64| (top.min(hi) - ordinary.max(lo)).max(0.0);
    LTCG_RATES[1] * in_band(t15, t20) + LTCG_RATES[2] * in_band(t20, f64::INFINITY)
}

/// Taxable portion of Social Security benefits (IRS Pub. 915; Owl
/// `compute_social_security_taxability`). `other_income` is provisional income
/// excluding Social Security (AGI items plus tax-exempt interest).
pub fn taxable_social_security(other_income: f64, benefits: f64, status: FilingStatus) -> f64 {
    if benefits <= 0.0 {
        return 0.0;
    }
    let s = status.index();
    let (lo, hi) = (SS_TAXABILITY_LO[s], SS_TAXABILITY_HI[s]);
    let provisional = other_income + 0.5 * benefits;
    if provisional < lo {
        return 0.0;
    }
    let tier_50 = 0.5 * benefits.min((hi - lo).min(provisional - lo));
    if provisional < hi {
        return tier_50;
    }
    (tier_50 + SS_MAX_TAXABLE_FRACTION * (provisional - hi)).min(SS_MAX_TAXABLE_FRACTION * benefits)
}

/// The provisional-income thresholds `(lo, hi)` for Social Security taxability.
pub fn ss_taxability_thresholds(status: FilingStatus) -> (f64, f64) {
    let s = status.index();
    (SS_TAXABILITY_LO[s], SS_TAXABILITY_HI[s])
}

/// Net investment income tax (Owl `computeNIIT`).
pub fn niit(magi: f64, net_investment_income: f64, status: FilingStatus) -> f64 {
    let excess = magi - NIIT_THRESHOLD[status.index()];
    if excess <= 0.0 {
        return 0.0;
    }
    NIIT_RATE * excess.min(net_investment_income.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn bracket_widths_follow_tops_and_scale_with_inflation() {
        let w = bracket_widths(FilingStatus::Single, 1.0);
        assert!(close(w[0], 12_400.0));
        assert!(close(w[1], 38_000.0));
        assert!(w[6].is_infinite());
        let w2 = bracket_widths(FilingStatus::MarriedFilingJointly, 1.5);
        assert!(close(w2[0], 24_800.0 * 1.5));
    }

    #[test]
    fn ordinary_tax_matches_hand_computation() {
        // 10% of 12,400 + 12% of 37,600
        assert!(close(
            ordinary_income_tax(50_000.0, FilingStatus::Single, 1.0),
            5_752.0
        ));
        assert!(close(
            ordinary_income_tax(-5.0, FilingStatus::Single, 1.0),
            0.0
        ));
        // Top bracket: 37% marginal far above the last threshold.
        let a = ordinary_income_tax(1_000_000.0, FilingStatus::Single, 1.0);
        let b = ordinary_income_tax(1_000_100.0, FilingStatus::Single, 1.0);
        assert!(close(b - a, 37.0));
    }

    #[test]
    fn standard_deduction_adds_65_plus_amounts_and_phases_out_bonus() {
        let base = standard_deduction(FilingStatus::Single, 1.0, 0, 2026, 0.0);
        assert!(close(base, 16_100.0));
        let senior = standard_deduction(FilingStatus::Single, 1.0, 1, 2026, 50_000.0);
        assert!(close(senior, 16_100.0 + 2_050.0 + 6_000.0));
        // Bonus phased out by 6% of MAGI above $75k: gone at $175k.
        assert!(close(
            senior_bonus_per_person(FilingStatus::Single, 125_000.0),
            3_000.0
        ));
        assert!(close(
            senior_bonus_per_person(FilingStatus::Single, 200_000.0),
            0.0
        ));
        // Bonus expires after 2028.
        let later = standard_deduction(FilingStatus::Single, 1.0, 1, 2029, 0.0);
        assert!(close(later, 18_150.0));
    }

    #[test]
    fn ltcg_stacks_on_ordinary_income() {
        // Ordinary 40k + gains 20k, single: 9,450 at 0%, 10,550 at 15%.
        assert!(close(
            ltcg_tax(40_000.0, 20_000.0, FilingStatus::Single, 1.0),
            1_582.5
        ));
        // Entirely within the 0% band.
        assert!(close(
            ltcg_tax(10_000.0, 20_000.0, FilingStatus::Single, 1.0),
            0.0
        ));
        // Ordinary income already above the 20% threshold.
        assert!(close(
            ltcg_tax(600_000.0, 1_000.0, FilingStatus::Single, 1.0),
            200.0
        ));
    }

    #[test]
    fn social_security_taxability_follows_pub_915() {
        // PI = 30k + 12k = 42k, single: 0.5*min(24k, 9k) + 0.85*(42k - 34k)
        let taxable = taxable_social_security(30_000.0, 24_000.0, FilingStatus::Single);
        assert!(close(taxable, 11_300.0));
        // Below the low threshold nothing is taxable.
        assert!(close(
            taxable_social_security(5_000.0, 20_000.0, FilingStatus::Single),
            0.0
        ));
        // Capped at 85% of benefits.
        let capped = taxable_social_security(500_000.0, 40_000.0, FilingStatus::Single);
        assert!(close(capped, 34_000.0));
        // Between thresholds: 50% tier only.
        let mid = taxable_social_security(20_000.0, 20_000.0, FilingStatus::Single);
        assert!(close(mid, 2_500.0));
    }

    #[test]
    fn niit_applies_only_above_threshold() {
        assert!(close(niit(150_000.0, 50_000.0, FilingStatus::Single), 0.0));
        assert!(close(
            niit(260_000.0, 100_000.0, FilingStatus::Single),
            0.038 * 60_000.0
        ));
        assert!(close(
            niit(260_000.0, 10_000.0, FilingStatus::Single),
            0.038 * 10_000.0
        ));
    }
}
