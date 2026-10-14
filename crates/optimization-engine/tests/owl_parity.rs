//! Parity with Owl: each case here mirrors one in `tests/owl_reference/run_owl.py`,
//! whose Owl results are checked in as `owl_reference.json`. Regenerate that
//! file with Owl's interpreter when Owl or a case changes.
//!
//! Strict cases hold no taxable account, so no capital gains arise. With gains,
//! Owl's self-consistent loop can stop on a plan that under-taxes them (its own
//! `fixedPointResidual` reports the gap), which makes its spending an upper bound
//! rather than a reference; those cases only check the engine stays below it.

use std::collections::HashMap;

use serde::Deserialize;
use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory,
    Scenario, Schedule, SocialSecurity, WorkingYear,
};

#[derive(Debug, Deserialize)]
struct OwlResult {
    first_year_spending_today: f64,
    bequest_after_tax_today: f64,
    spending_today: Vec<f64>,
    tax_deferred_withdrawal: Vec<f64>,
    roth_conversion: Vec<f64>,
    taxable_withdrawal: Vec<f64>,
    roth_withdrawal: Vec<f64>,
    ordinary_tax: Vec<f64>,
    ltcg_tax: Vec<f64>,
    medicare: Vec<f64>,
    magi: Vec<f64>,
    ltcg_residual: f64,
}

fn reference(case: &str) -> OwlResult {
    let mut all: HashMap<String, OwlResult> =
        serde_json::from_str(include_str!("owl_reference/owl_reference.json"))
            .expect("owl_reference.json parses");
    assert!(
        all.contains_key(case),
        "{case} missing from owl_reference.json"
    );
    all.remove(case).expect("checked above")
}

/// Inputs shared by the Python and Rust sides of a case.
struct Case {
    birth_year: i32,
    birth_month: u32,
    planning_age: u32,
    taxable: f64,
    taxable_cost_basis: f64,
    tax_deferred: f64,
    roth: f64,
    monthly_pia: f64,
    dividend_yield: f64,
    prior_magi: f64,
    working_years: Vec<WorkingYear>,
}

fn standard_retiree() -> Case {
    Case {
        birth_year: 1964,
        birth_month: 6,
        planning_age: 95,
        taxable: 500_000.0,
        taxable_cost_basis: 350_000.0,
        tax_deferred: 1_000_000.0,
        roth: 0.0,
        monthly_pia: 2_800.0,
        dividend_yield: 0.015,
        prior_magi: 90_000.0,
        working_years: Vec::new(),
    }
}

/// Works ten more years at $140k taxable wages with a $24.5k pre-tax deferral.
fn early_retiree() -> Case {
    Case {
        birth_year: 1981,
        birth_month: 4,
        planning_age: 92,
        taxable: 250_000.0,
        taxable_cost_basis: 180_000.0,
        tax_deferred: 400_000.0,
        roth: 60_000.0,
        monthly_pia: 2_400.0,
        dividend_yield: 0.015,
        prior_magi: 150_000.0,
        working_years: vec![
            WorkingYear {
                wages: 140_000.0,
                tax_deferred_contribution: 24_500.0,
                ..WorkingYear::default()
            };
            10
        ],
    }
}

/// Moves the taxable balance to the Roth account (`no_gains` in run_owl.py).
fn without_gains(mut case: Case) -> Case {
    case.roth += case.taxable;
    case.taxable = 0.0;
    case.taxable_cost_basis = 0.0;
    case.dividend_yield = 0.0;
    case
}

fn scenario(case: Case, objective: Objective) -> Scenario {
    Scenario {
        start_year: 2026,
        person: Person {
            birth_year: case.birth_year,
            birth_month: case.birth_month,
            sex: Sex::Male,
            planning_age: case.planning_age,
        },
        accounts: Accounts {
            taxable: case.taxable,
            taxable_cost_basis: case.taxable_cost_basis,
            tax_deferred: case.tax_deferred,
            roth: case.roth,
            hsa: 0.0,
        },
        rates: Rates {
            inflation: 0.025,
            taxable_return: 0.05,
            tax_deferred_return: 0.05,
            roth_return: 0.05,
            dividend_yield: case.dividend_yield,
            annual_returns: Vec::new(),
            annual_inflation: Vec::new(),
            account_returns: Vec::new(),
        },
        social_security: Some(SocialSecurity {
            monthly_pia: case.monthly_pia,
            claim: ClaimStrategy::AtAge(67.0),
        }),
        working_years: case.working_years,
        roth_history: RothHistory::default(),
        objective,
        options: PlanOptions {
            prior_magi: [case.prior_magi, case.prior_magi],
            ..PlanOptions::default()
        },
    }
}

fn report(name: &str, schedule: &Schedule, owl: &OwlResult) {
    println!(
        "{name}: {:?} {:?} after {} iterations; engine first-year {:.0} vs Owl {:.0}; bequest {:.0} vs {:.0}",
        schedule.status,
        schedule.convergence,
        schedule.iterations,
        schedule.first_year_spending(),
        owl.first_year_spending_today,
        schedule.bequest_after_tax_today,
        owl.bequest_after_tax_today,
    );
    for (n, y) in schedule.years.iter().enumerate() {
        let at = |v: &Vec<f64>| v.get(n).copied().unwrap_or(f64::NAN);
        println!(
            "  {} txw {:>7.0}/{:>7.0} rw {:>7.0}/{:>7.0} spend {:>7.0}/{:>7.0} ira {:>7.0}/{:>7.0} conv {:>7.0}/{:>7.0} tax {:>6.0}/{:>6.0} cg {:>5.0}/{:>5.0} med {:>6.0}/{:>6.0} magi {:>7.0}/{:>7.0}",
            y.year,
            y.taxable_brokerage_liquidation,
            at(&owl.taxable_withdrawal),
            y.roth_withdrawal,
            at(&owl.roth_withdrawal),
            y.net_spending_today,
            at(&owl.spending_today),
            y.traditional_ira_withdrawal,
            at(&owl.tax_deferred_withdrawal),
            y.roth_conversion,
            at(&owl.roth_conversion),
            y.ordinary_income_tax,
            at(&owl.ordinary_tax),
            y.capital_gains_tax,
            at(&owl.ltcg_tax),
            y.medicare_premium,
            at(&owl.medicare),
            y.magi,
            at(&owl.magi),
        );
    }
}

/// Owl's answer is one fixed point of its self-consistent loop, not a proven
/// optimum: the engine must reach it (within `shortfall`, relative) and may beat
/// it, but not by more than `excess` (relative), which would point at a modelling
/// difference rather than a better plan.
fn assert_at_least_owl(case: &str, engine: f64, owl: f64, shortfall: f64, excess: f64) {
    assert!(
        engine >= owl * (1.0 - shortfall) && engine <= owl * (1.0 + excess),
        "{case}: engine {engine:.0} vs Owl {owl:.0}"
    );
}

fn assert_spending_matches(case: &str, scenario: Scenario) {
    let owl = reference(case);
    let schedule = optimize(&scenario).expect("feasible");
    report(case, &schedule, &owl);
    assert_at_least_owl(
        case,
        schedule.first_year_spending(),
        owl.first_year_spending_today,
        0.003,
        0.02,
    );
}

fn assert_bequest_matches(case: &str, scenario: Scenario) {
    let owl = reference(case);
    let schedule = optimize(&scenario).expect("feasible");
    report(case, &schedule, &owl);
    assert_at_least_owl(
        case,
        schedule.bequest_after_tax_today,
        owl.bequest_after_tax_today,
        0.01,
        0.06,
    );
}

#[test]
fn retiree_without_gains_spends_what_owl_spends() {
    assert_spending_matches(
        "standard_retiree_no_gains_max_spending",
        scenario(
            without_gains(standard_retiree()),
            Objective::MaxSpending { bequest_floor: 0.0 },
        ),
    );
}

#[test]
fn retiree_without_gains_leaves_what_owl_leaves() {
    assert_bequest_matches(
        "standard_retiree_no_gains_max_bequest",
        scenario(
            without_gains(standard_retiree()),
            Objective::MaxBequest {
                net_spending: 80_000.0,
            },
        ),
    );
}

#[test]
fn worker_retiring_early_spends_what_owl_spends() {
    assert_spending_matches(
        "early_retiree_no_gains_max_spending",
        scenario(
            without_gains(early_retiree()),
            Objective::MaxSpending { bequest_floor: 0.0 },
        ),
    );
}

#[test]
fn worker_retiring_early_leaves_what_owl_leaves() {
    assert_bequest_matches(
        "early_retiree_no_gains_max_bequest",
        scenario(
            without_gains(early_retiree()),
            Objective::MaxBequest {
                net_spending: 70_000.0,
            },
        ),
    );
}

/// With gains, Owl's loop under-taxes them, so its spending bounds the engine's
/// from above; the engine's own statutory checks live in `standard_retiree.rs`.
#[test]
fn with_gains_owl_overstates_spending_by_under_taxing_them() {
    let owl = reference("standard_retiree_max_spending");
    assert!(
        owl.ltcg_residual > 10_000.0,
        "the reference is expected to carry Owl's LTCG loop residual"
    );
    let schedule = optimize(&scenario(
        standard_retiree(),
        Objective::MaxSpending { bequest_floor: 0.0 },
    ))
    .expect("feasible");
    report("standard_retiree_max_spending", &schedule, &owl);
    assert!(schedule.first_year_spending() <= owl.first_year_spending_today);
    assert!(schedule.first_year_spending() >= 0.97 * owl.first_year_spending_today);
}

#[test]
#[ignore = "diagnostic: how the LTCG treatment moves the early retiree's estate"]
fn early_retiree_ltcg_treatment_probe() {
    let owl = reference("early_retiree_no_gains_max_bequest");
    for (label, exact, stabilizer) in [
        ("loop, stabilizer off", false, 0.0),
        ("exact LTCG", true, 0.05),
    ] {
        let mut s = scenario(
            without_gains(early_retiree()),
            Objective::MaxBequest {
                net_spending: 70_000.0,
            },
        );
        s.options.optimize_ltcg = exact;
        s.options.loop_stabilizer = stabilizer;
        let started = std::time::Instant::now();
        let schedule = optimize(&s).expect("feasible");
        println!("{label}: {:.1}s", started.elapsed().as_secs_f64());
        report(label, &schedule, &owl);
    }
}

#[test]
fn worker_without_social_security_leaves_what_owl_leaves() {
    let mut case = without_gains(early_retiree());
    case.monthly_pia = 0.0;
    assert_bequest_matches(
        "early_retiree_no_gains_no_ss_max_bequest",
        scenario(
            case,
            Objective::MaxBequest {
                net_spending: 55_000.0,
            },
        ),
    );
}
