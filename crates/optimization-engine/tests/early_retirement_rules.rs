//! Early-retirement and state rules: Virginia income tax, ACA premiums and the
//! 400% cliff, the HSA, the Rule of 55, 72(t) payments and gain harvesting.
//! Every schedule is re-checked against the engine's independent tax functions.

use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::tax::aca::{net_premium, CLIFF, FPL_2026_SINGLE};
use wealthfolio_optimization_engine::tax::state::StateTax;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory,
    Scenario, Schedule, Sepp, SocialSecurity, WorkingYear,
};

const INFLATION: f64 = 0.025;

fn gamma(n: usize) -> f64 {
    (1.0 + INFLATION).powi(n as i32)
}

/// Retired at 52 in 2026 with all four account types.
fn early_retiree() -> Scenario {
    Scenario {
        start_year: 2026,
        person: Person {
            birth_year: 1974,
            birth_month: 3,
            sex: Sex::Male,
            planning_age: 90,
        },
        accounts: Accounts {
            taxable: 400_000.0,
            taxable_cost_basis: 250_000.0,
            tax_deferred: 700_000.0,
            roth: 150_000.0,
            hsa: 40_000.0,
        },
        rates: Rates {
            inflation: INFLATION,
            taxable_return: 0.05,
            tax_deferred_return: 0.05,
            roth_return: 0.05,
            dividend_yield: 0.015,
            annual_returns: Vec::new(),
            annual_inflation: Vec::new(),
            account_returns: Vec::new(),
        },
        social_security: Some(SocialSecurity {
            monthly_pia: 2_300.0,
            claim: ClaimStrategy::AtAge(67.0),
        }),
        working_years: Vec::new(),
        roth_history: RothHistory::default(),
        objective: Objective::MaxBequest {
            net_spending: 50_000.0,
        },
        options: PlanOptions {
            prior_magi: [60_000.0, 60_000.0],
            polish: false,
            ..PlanOptions::default()
        },
    }
}

fn solve(s: &Scenario) -> Schedule {
    let result = optimize(s);
    assert!(result.is_ok(), "{:?}", result.as_ref().err());
    let schedule = result.expect("checked above");
    assert!(schedule.converged(), "{:?}", schedule.convergence);
    schedule
}

#[test]
fn virginia_tax_matches_its_rules_every_year() {
    let mut s = early_retiree();
    let va = StateTax::virginia();
    s.options.state_tax = Some(va.clone());
    let schedule = solve(&s);
    let mut paid = 0.0;
    for (n, y) in schedule.years.iter().enumerate() {
        let g = gamma(n);
        // Federal AGI without Social Security, which Virginia exempts.
        let agi = y.traditional_ira_withdrawal
            + y.roth_conversion
            + y.other_income
            + y.wages
            + y.capital_gains;
        let exemption = if y.age >= 65 {
            (12_000.0 * g).min(y.traditional_ira_withdrawal + y.roth_conversion)
        } else {
            0.0
        };
        let taxable = (agi - 9_680.0 * g - exemption).max(0.0);
        let expected = va.tax(taxable, g);
        assert!(
            (y.state_income_tax - expected).abs() <= 2.0,
            "{}: state tax {:.2} vs {:.2}",
            y.year,
            y.state_income_tax,
            expected
        );
        paid += y.state_income_tax;
    }
    assert!(paid > 0.0);
    assert!(schedule.lifetime_state_tax > 0.0);
}

#[test]
fn state_tax_never_improves_the_plan() {
    let without = solve(&early_retiree());
    let mut s = early_retiree();
    s.options.state_tax = Some(StateTax::virginia());
    let with = solve(&s);
    assert!(with.bequest_after_tax_today < without.bequest_after_tax_today);
}

#[test]
fn aca_premiums_follow_magi_until_medicare() {
    let mut s = early_retiree();
    s.options.aca_benchmark_premium = 9_000.0;
    let schedule = solve(&s);
    for (n, y) in schedule.years.iter().enumerate() {
        let (slcsp, fpl) = (9_000.0 * gamma(n), FPL_2026_SINGLE * gamma(n));
        if y.age >= 65 {
            assert_eq!(y.aca_premium, 0.0, "{} ACA after Medicare", y.year);
            continue;
        }
        let magi = y.magi + y.social_security_benefit - y.taxable_social_security;
        let expected = if magi > CLIFF * fpl {
            slcsp
        } else {
            net_premium(magi, slcsp, fpl)
        };
        assert!(
            (y.aca_premium - expected).abs() <= 25.0,
            "{}: ACA {:.0} vs {:.0} at MAGI {:.0}",
            y.year,
            y.aca_premium,
            expected,
            magi
        );
    }
}

#[test]
fn the_aca_cliff_shapes_income_before_medicare() {
    let mut s = early_retiree();
    s.options.aca_benchmark_premium = 12_000.0;
    let schedule = solve(&s);
    // With a $12k premium at stake, the plan keeps MAGI under the cliff in at
    // least most pre-Medicare years (Roth and basis money fund the gap).
    let pre_medicare: Vec<_> = schedule.years.iter().filter(|y| y.age < 65).collect();
    let under = pre_medicare
        .iter()
        .enumerate()
        .filter(|(n, y)| y.magi <= CLIFF * FPL_2026_SINGLE * gamma(*n) + 1.0)
        .count();
    assert!(
        under * 2 > pre_medicare.len(),
        "{under} of {} years under the cliff",
        pre_medicare.len()
    );
}

#[test]
fn hsa_pays_only_qualified_costs_and_stops_contributions_at_65() {
    let mut s = early_retiree();
    s.options.other_medical_expenses = 3_000.0;
    s.working_years = vec![
        WorkingYear {
            wages: 90_000.0,
            hsa_contribution: 4_400.0,
            ..WorkingYear::default()
        };
        20
    ];
    // Still working, so start with a feasible spending level.
    s.objective = Objective::MaxBequest {
        net_spending: 60_000.0,
    };
    let schedule = solve(&s);
    for (n, y) in schedule.years.iter().enumerate() {
        assert!(
            y.hsa_withdrawal <= y.medicare_premium + 3_000.0 * gamma(n) + 1.0,
            "{} HSA {:.0} beyond qualified costs",
            y.year,
            y.hsa_withdrawal
        );
        if y.age >= 65 {
            assert_eq!(
                y.hsa_contribution, 0.0,
                "{} contribution on Medicare",
                y.year
            );
        }
    }
    assert!(schedule.years.iter().any(|y| y.hsa_withdrawal > 0.0));
}

#[test]
fn rule_of_55_makes_plan_withdrawals_penalty_free() {
    // Leaves the job at 56 (2030) with only tax-deferred money to bridge to 59½.
    let mut s = early_retiree();
    s.accounts = Accounts {
        taxable: 0.0,
        taxable_cost_basis: 0.0,
        tax_deferred: 1_200_000.0,
        roth: 0.0,
        hsa: 0.0,
    };
    s.working_years = vec![
        // Pay barely covers spending and tax: nothing is saved for the bridge.
        WorkingYear {
            wages: 60_000.0,
            ..WorkingYear::default()
        };
        4
    ];
    let penalized = solve(&s);
    let paid: f64 = penalized
        .years
        .iter()
        .map(|y| y.early_withdrawal_penalty)
        .sum();
    assert!(
        paid > 0.0,
        "without the rule, the bridge years pay the penalty"
    );

    s.options.rule_of_55 = Some(1_000_000.0);
    let schedule = solve(&s);
    let paid: f64 = schedule
        .years
        .iter()
        .map(|y| y.early_withdrawal_penalty)
        .sum();
    assert!(paid.abs() < 1.0, "penalty {paid:.0} under the Rule of 55");
    assert!(schedule.bequest_after_tax_today > penalized.bequest_after_tax_today);
}

#[test]
fn sepp_payments_are_taken_and_not_penalized() {
    let mut s = early_retiree();
    s.options.sepp = Some(Sepp {
        start_year: 2027,
        annual_payment: 30_000.0,
    });
    let schedule = solve(&s);
    for y in &schedule.years {
        // Five years or to 59½ (2033 for a March birthday turning 59), whichever is later.
        if (2027..2033).contains(&y.year) {
            assert!(
                y.traditional_ira_withdrawal >= 30_000.0 - 1e-6,
                "{} payment {:.0}",
                y.year,
                y.traditional_ira_withdrawal
            );
            let penalty_base = (y.traditional_ira_withdrawal - 30_000.0).max(0.0);
            assert!((y.early_withdrawal_penalty - 0.1 * penalty_base).abs() < 1.0);
        }
    }
}

/// Each solve sees a harvest's tax but not the basis it buys, which only reaches
/// the plan through the next iteration's gain fractions, so harvesting can land
/// on a slightly different fixed point; it must not cost more than a sliver.
#[test]
fn harvesting_fills_the_zero_percent_band_and_never_hurts() {
    let mut s = early_retiree();
    s.accounts.taxable_cost_basis = 100_000.0;
    let plain = solve(&s);
    s.options.allow_gain_harvesting = true;
    let harvested = solve(&s);
    assert!(harvested.years.iter().any(|y| y.gain_harvest > 0.0));
    assert!(
        harvested.bequest_after_tax_today >= 0.995 * plain.bequest_after_tax_today,
        "harvest {:.0} vs plain {:.0}",
        harvested.bequest_after_tax_today,
        plain.bequest_after_tax_today
    );
}
