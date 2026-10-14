//! Actuarial longevity from the SSA 2025 period life table (Owl
//! `data/mortality_tables.py`, default table). Used to pick a planning horizon,
//! and, from any of Owl's tables, to draw random lifespans for Monte Carlo runs.

use serde::{Deserialize, Serialize};

use crate::data::tables::{
    IAM2012_FEMALE_QX, IAM2012_MALE_QX, PUBG2010_FEMALE_QX, PUBG2010_MALE_QX, PUBS2010_FEMALE_QX,
    PUBS2010_MALE_QX, PUBT2010_FEMALE_QX, PUBT2010_MALE_QX, RP2014_FEMALE_QX, RP2014_MALE_QX,
    SSA2025_FEMALE_QX, SSA2025_MALE_QX, VBT2015NS_FEMALE_QX, VBT2015NS_MALE_QX,
    VBT2015SM_FEMALE_QX, VBT2015SM_MALE_QX,
};
use crate::simulation::SplitMix64;

/// Oldest age in the table; death is certain before `MAX_AGE + 1`.
pub const MAX_AGE: u32 = 119;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sex {
    Male,
    Female,
}

/// The population a random lifespan is drawn from (Owl's table keys), shortest-
/// lived first at 65 after the general population.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MortalityTable {
    /// General US population: SSA 2025 period life table.
    #[default]
    #[serde(rename = "SSA2025")]
    Ssa2025,
    /// Smokers: SOA Valuation Basic Table 2015, smoker lives.
    #[serde(rename = "VBT2015-SM")]
    Vbt2015Smoker,
    /// Non-smokers: SOA Valuation Basic Table 2015, non-smoker lives.
    #[serde(rename = "VBT2015-NS")]
    Vbt2015NonSmoker,
    /// Private-sector pension recipients: SOA RP-2014 Healthy Annuitant.
    #[serde(rename = "RP2014")]
    Rp2014,
    /// Annuity buyers: SOA Individual Annuity Mortality 2012.
    #[serde(rename = "IAM2012")]
    Iam2012,
    /// Government employees: SOA Pub-2010 General.
    #[serde(rename = "Pub2010-General")]
    Pub2010General,
    /// Police, fire and corrections: SOA Pub-2010 Public Safety.
    #[serde(rename = "Pub2010-Safety")]
    Pub2010Safety,
    /// Teachers: SOA Pub-2010 Teachers.
    #[serde(rename = "Pub2010-Teacher")]
    Pub2010Teacher,
}

impl MortalityTable {
    pub const ALL: [MortalityTable; 8] = [
        Self::Ssa2025,
        Self::Vbt2015Smoker,
        Self::Vbt2015NonSmoker,
        Self::Rp2014,
        Self::Iam2012,
        Self::Pub2010General,
        Self::Pub2010Safety,
        Self::Pub2010Teacher,
    ];

    /// Chance of dying within a year at each exact age, 0..=[`MAX_AGE`].
    pub fn death_rates(self, sex: Sex) -> &'static [f64; 120] {
        match (self, sex) {
            (Self::Ssa2025, Sex::Male) => &SSA2025_MALE_QX,
            (Self::Ssa2025, Sex::Female) => &SSA2025_FEMALE_QX,
            (Self::Vbt2015Smoker, Sex::Male) => &VBT2015SM_MALE_QX,
            (Self::Vbt2015Smoker, Sex::Female) => &VBT2015SM_FEMALE_QX,
            (Self::Vbt2015NonSmoker, Sex::Male) => &VBT2015NS_MALE_QX,
            (Self::Vbt2015NonSmoker, Sex::Female) => &VBT2015NS_FEMALE_QX,
            (Self::Rp2014, Sex::Male) => &RP2014_MALE_QX,
            (Self::Rp2014, Sex::Female) => &RP2014_FEMALE_QX,
            (Self::Iam2012, Sex::Male) => &IAM2012_MALE_QX,
            (Self::Iam2012, Sex::Female) => &IAM2012_FEMALE_QX,
            (Self::Pub2010General, Sex::Male) => &PUBG2010_MALE_QX,
            (Self::Pub2010General, Sex::Female) => &PUBG2010_FEMALE_QX,
            (Self::Pub2010Safety, Sex::Male) => &PUBS2010_MALE_QX,
            (Self::Pub2010Safety, Sex::Female) => &PUBS2010_FEMALE_QX,
            (Self::Pub2010Teacher, Sex::Male) => &PUBT2010_MALE_QX,
            (Self::Pub2010Teacher, Sex::Female) => &PUBT2010_FEMALE_QX,
        }
    }
}

fn qx_table(sex: Sex) -> &'static [f64; 120] {
    match sex {
        Sex::Male => &SSA2025_MALE_QX,
        Sex::Female => &SSA2025_FEMALE_QX,
    }
}

/// Probability of dying within a year at exact age `age`.
pub fn death_probability(sex: Sex, age: u32) -> f64 {
    qx_table(sex)[age.min(MAX_AGE) as usize]
}

/// Probability that someone alive at `from_age` is still alive at `to_age`.
pub fn survival_probability(sex: Sex, from_age: u32, to_age: u32) -> f64 {
    (from_age..to_age.min(MAX_AGE + 1))
        .map(|age| 1.0 - death_probability(sex, age))
        .product::<f64>()
        * if to_age > MAX_AGE { 0.0 } else { 1.0 }
}

/// Remaining life expectancy at `age` in years (curtate expectancy plus half a year).
pub fn life_expectancy(sex: Sex, age: u32) -> f64 {
    let mut alive = 1.0;
    let mut expectancy = 0.0;
    for a in age..=MAX_AGE {
        alive *= 1.0 - death_probability(sex, a);
        expectancy += alive;
    }
    expectancy + 0.5
}

/// Last age reached with at least probability `p` (e.g. `p = 0.1` gives a
/// conservative horizon that only 10% of people outlive). Planning to this age
/// is a common way to guard against longevity risk.
pub fn age_at_survival_probability(sex: Sex, current_age: u32, p: f64) -> u32 {
    let mut alive = 1.0;
    for age in current_age..=MAX_AGE {
        let next = alive * (1.0 - death_probability(sex, age));
        if next < p {
            return age;
        }
        alive = next;
    }
    MAX_AGE
}

/// Each age at death from `alive_at` to [`MAX_AGE`] and its probability for
/// someone alive at `alive_at` (Owl's `survival_pmf`).
pub fn lifespan_distribution(table: MortalityTable, sex: Sex, alive_at: u32) -> Vec<(u32, f64)> {
    let q = table.death_rates(sex);
    let mut alive = 1.0;
    (alive_at.min(MAX_AGE)..=MAX_AGE)
        .map(|age| {
            let dies = alive * q[age as usize];
            alive -= dies;
            (age, dies)
        })
        .collect()
}

/// `count` ages at death for someone alive at `alive_at`, from a fixed `seed`
/// (Owl's `sample_lifespans`).
pub fn sample_lifespans(
    table: MortalityTable,
    sex: Sex,
    alive_at: u32,
    count: usize,
    seed: u64,
) -> Vec<u32> {
    let pmf = lifespan_distribution(table, sex, alive_at);
    let mut rng = SplitMix64(seed);
    (0..count)
        .map(|_| {
            let mut u = rng.unit();
            for (age, p) in &pmf {
                if u <= *p {
                    return *age;
                }
                u -= p;
            }
            MAX_AGE
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_endpoints_are_valid() {
        assert!((death_probability(Sex::Male, 65) - 0.017897).abs() < 1e-12);
        assert!((death_probability(Sex::Female, 65) - 0.011018).abs() < 1e-12);
        assert_eq!(death_probability(Sex::Male, MAX_AGE), 1.0);
        assert_eq!(death_probability(Sex::Male, 150), 1.0);
    }

    #[test]
    fn life_expectancy_is_plausible_and_women_live_longer() {
        let male = life_expectancy(Sex::Male, 65);
        let female = life_expectancy(Sex::Female, 65);
        // SSA 2025: roughly 18 years for men and 20.5 for women at 65.
        assert!((16.0..20.0).contains(&male), "male {male}");
        assert!((19.0..23.0).contains(&female), "female {female}");
        assert!(female > male);
    }

    #[test]
    fn survival_probability_decreases_and_reaches_zero() {
        let s80 = survival_probability(Sex::Male, 62, 80);
        let s90 = survival_probability(Sex::Male, 62, 90);
        assert!(s80 > s90 && s90 > 0.0);
        assert_eq!(survival_probability(Sex::Male, 62, 62), 1.0);
        assert_eq!(survival_probability(Sex::Male, 62, 121), 0.0);
    }

    #[test]
    fn conservative_horizon_is_beyond_median() {
        let median = age_at_survival_probability(Sex::Male, 62, 0.5);
        let conservative = age_at_survival_probability(Sex::Male, 62, 0.1);
        assert!(conservative > median);
        assert!(
            (90..=100).contains(&conservative),
            "p10 horizon {conservative}"
        );
    }

    #[test]
    fn every_table_is_a_full_distribution() {
        for table in MortalityTable::ALL {
            for sex in [Sex::Female, Sex::Male] {
                for alive_at in [0, 45, 65, 100, MAX_AGE] {
                    let pmf = lifespan_distribution(table, sex, alive_at);
                    let total: f64 = pmf.iter().map(|(_, p)| p).sum();
                    assert!((total - 1.0).abs() < 1e-9, "{table:?} {sex:?} {alive_at}");
                    assert_eq!(pmf[0].0, alive_at);
                }
            }
        }
    }

    #[test]
    fn tables_order_by_life_expectancy_as_in_owl() {
        // Owl's notes, at 65 with men and women averaged: smokers 82.3, the
        // general population 83.3, annuity buyers 86.7.
        let at_65 = |table| {
            let mean = |sex| {
                lifespan_distribution(table, sex, 65)
                    .iter()
                    .map(|(age, p)| f64::from(*age) * p)
                    .sum::<f64>()
            };
            (mean(Sex::Male) + mean(Sex::Female)) / 2.0
        };
        assert!((at_65(MortalityTable::Vbt2015Smoker) - 82.3).abs() < 0.6);
        assert!((at_65(MortalityTable::Ssa2025) - 83.3).abs() < 0.6);
        assert!((at_65(MortalityTable::Iam2012) - 86.7).abs() < 0.6);
    }

    #[test]
    fn samples_follow_the_table() {
        let draws = sample_lifespans(MortalityTable::Ssa2025, Sex::Male, 60, 20_000, 7);
        assert!(draws.iter().all(|age| (60..=MAX_AGE).contains(age)));
        let mean = draws.iter().map(|a| f64::from(*a)).sum::<f64>() / draws.len() as f64;
        let expected: f64 = lifespan_distribution(MortalityTable::Ssa2025, Sex::Male, 60)
            .iter()
            .map(|(age, p)| f64::from(*age) * p)
            .sum();
        assert!((mean - expected).abs() < 0.3, "{mean} vs {expected}");
        let again = sample_lifespans(MortalityTable::Ssa2025, Sex::Male, 60, 10, 7);
        assert_eq!(draws[..10], again[..]);
    }

    #[test]
    fn tables_keep_owls_keys() {
        let key = serde_json::to_string(&MortalityTable::Vbt2015Smoker).expect("json");
        assert_eq!(key, "\"VBT2015-SM\"");
    }
}
