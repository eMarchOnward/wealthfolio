//! Success probability over historical and bootstrap paths.

use wealthfolio_optimization_engine::markets::AssetMix;
use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::simulation::{
    bootstrap_paths, historical_paths, spending_at_success_rate, spending_capacities,
    success_probability,
};
use wealthfolio_optimization_engine::{
    Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory, Scenario,
    SocialSecurity,
};

/// A 65-year-old retiree with $1.2M, planning to 90.
fn retiree() -> Scenario {
    Scenario {
        start_year: 2026,
        person: Person {
            birth_year: 1961,
            birth_month: 6,
            sex: Sex::Female,
            planning_age: 90,
        },
        accounts: Accounts {
            taxable: 300_000.0,
            taxable_cost_basis: 250_000.0,
            tax_deferred: 700_000.0,
            roth: 200_000.0,
            hsa: 0.0,
        },
        rates: Rates {
            inflation: 0.025,
            taxable_return: 0.05,
            tax_deferred_return: 0.05,
            roth_return: 0.05,
            dividend_yield: 0.0,
            annual_returns: Vec::new(),
            annual_inflation: Vec::new(),
            account_returns: Vec::new(),
        },
        social_security: Some(SocialSecurity {
            monthly_pia: 2_400.0,
            claim: ClaimStrategy::AtAge(67.0),
        }),
        working_years: Vec::new(),
        roth_history: RothHistory::default(),
        objective: Objective::MaxSpending { bequest_floor: 0.0 },
        options: PlanOptions {
            prior_magi: [80_000.0, 80_000.0],
            polish: false,
            ..PlanOptions::default()
        },
    }
}

fn sixty_forty() -> Vec<[AssetMix; 4]> {
    vec![[AssetMix::stocks_bonds(0.6); 4]]
}

#[test]
fn modest_spending_survives_every_sampled_history_and_lavish_spending_none() {
    let s = retiree();
    let paths: Vec<_> = historical_paths(s.years(), &sixty_forty(), 0.002)
        .into_iter()
        .step_by(12)
        .collect();
    assert!(paths.len() >= 5);

    let modest = success_probability(&s, 30_000.0, &paths, 2).expect("runs");
    assert_eq!(modest.paths, paths.len());
    assert_eq!(modest.funded, paths.len(), "{:?}", modest.outcomes);
    assert!((modest.success_rate - 1.0).abs() < 1e-12);
    assert!(modest.bequest_percentiles[0] > 0.0);

    let lavish = success_probability(&s, 250_000.0, &paths, 2).expect("runs");
    assert_eq!(lavish.funded, 0);
    assert_eq!(lavish.bequest_percentiles, [0.0; 3]);
}

#[test]
fn bootstrap_rates_fall_as_spending_rises() {
    let s = retiree();
    let paths = bootstrap_paths(s.years(), &sixty_forty(), 0.002, 8, 42);
    let low = success_probability(&s, 55_000.0, &paths, 2).expect("runs");
    let high = success_probability(&s, 85_000.0, &paths, 2).expect("runs");
    assert!(low.success_rate >= high.success_rate);
}

#[test]
fn short_paths_are_rejected() {
    let s = retiree();
    let paths = historical_paths(10, &sixty_forty(), 0.0);
    assert!(success_probability(&s, 40_000.0, &paths, 1).is_err());
}

#[test]
fn capacities_agree_with_the_success_check() {
    let s = retiree();
    let paths = bootstrap_paths(s.years(), &sixty_forty(), 0.002, 6, 7);
    let caps = spending_capacities(&s, &paths, 2).expect("runs");
    assert_eq!(caps.len(), paths.len());
    // Spending what two thirds of the paths can afford: those paths fund it.
    let spending = spending_at_success_rate(&caps, 2.0 / 3.0);
    assert!(spending > 0.0);
    let check = success_probability(&s, spending, &paths, 2).expect("runs");
    assert!(
        check.success_rate >= 2.0 / 3.0 - 1e-9,
        "{spending} {caps:?} {:?}",
        check.outcomes
    );
}
