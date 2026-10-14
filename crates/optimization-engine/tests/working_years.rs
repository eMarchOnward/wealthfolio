//! Working years, contributions and the early-retirement rules: a worker earns
//! wages and contributes until retiring at 55, then lives on savings.

use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::tax::federal::{ordinary_income_tax, standard_deduction};
use wealthfolio_optimization_engine::tax::FilingStatus;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory,
    Scenario, Schedule, SocialSecurity, WorkingYear,
};

const WAGES: f64 = 120_000.0;
const WORKING_YEARS: usize = 8;

fn worker(contribution: WorkingYear) -> Scenario {
    Scenario {
        start_year: 2026,
        person: Person {
            birth_year: 1979,
            birth_month: 3,
            sex: Sex::Female,
            planning_age: 92,
        },
        accounts: Accounts {
            taxable: 150_000.0,
            taxable_cost_basis: 120_000.0,
            tax_deferred: 300_000.0,
            roth: 50_000.0,
            hsa: 0.0,
        },
        rates: Rates {
            inflation: 0.025,
            taxable_return: 0.05,
            tax_deferred_return: 0.05,
            roth_return: 0.05,
            dividend_yield: 0.015,
            annual_returns: Vec::new(),
            annual_inflation: Vec::new(),
            account_returns: Vec::new(),
        },
        social_security: Some(SocialSecurity {
            monthly_pia: 2_200.0,
            claim: ClaimStrategy::AtAge(67.0),
        }),
        working_years: vec![contribution; WORKING_YEARS],
        roth_history: RothHistory::default(),
        objective: Objective::MaxSpending { bequest_floor: 0.0 },
        options: PlanOptions {
            prior_magi: [WAGES, WAGES],
            // These tests check accounting identities, not the polish search.
            polish: false,
            ..PlanOptions::default()
        },
    }
}

fn solve(s: &Scenario) -> Schedule {
    let schedule = optimize(s).expect("feasible");
    assert!(schedule.converged(), "{:?}", schedule.convergence);
    schedule
}

#[test]
fn pre_tax_deferral_is_excluded_from_income_and_lands_in_the_ira() {
    let deferral = 23_000.0;
    let employer = 6_000.0;
    let s = worker(WorkingYear {
        wages: WAGES - deferral,
        tax_deferred_contribution: deferral,
        employer_contribution: employer,
        ..WorkingYear::default()
    });
    let schedule = solve(&s);
    let y0 = &schedule.years[0];
    assert_eq!(y0.wages, WAGES - deferral);
    assert_eq!(y0.tax_deferred_contribution, deferral + employer);

    // Taxable income is the W-2 wage less the standard deduction (plus any IRA
    // withdrawal or conversion the plan makes, which the identity accounts for).
    let deduction = standard_deduction(FilingStatus::Single, 1.0, 0, 2026, f64::INFINITY);
    let expected =
        WAGES - deferral + y0.traditional_ira_withdrawal + y0.roth_conversion - deduction;
    assert!(
        (y0.ordinary_taxable_income - expected).abs() < 1.0,
        "ordinary income {} vs {expected}",
        y0.ordinary_taxable_income
    );
    let tax = ordinary_income_tax(expected, FilingStatus::Single, 1.0);
    assert!((y0.ordinary_income_tax - tax).abs() < 1.0);

    // Contributions arrive through the year: half a year of growth.
    let y1 = &schedule.years[1];
    let expected_ira = 1.05
        * (y0.tax_deferred_balance - y0.traditional_ira_withdrawal - y0.roth_conversion)
        + (deferral + employer) * 1.025;
    assert!(
        (y1.tax_deferred_balance - expected_ira).abs() < 1.0,
        "IRA {} vs {expected_ira}",
        y1.tax_deferred_balance
    );
}

#[test]
fn roth_contributions_stay_in_taxable_income() {
    let amount = 20_000.0;
    let pre_tax = solve(&worker(WorkingYear {
        wages: WAGES - amount,
        tax_deferred_contribution: amount,
        ..WorkingYear::default()
    }));
    let roth = solve(&worker(WorkingYear {
        wages: WAGES,
        roth_contribution: amount,
        ..WorkingYear::default()
    }));
    // Same pay and the same savings, but the Roth dollars are taxed now.
    let (a, b) = (&pre_tax.years[0], &roth.years[0]);
    assert!(
        b.ordinary_taxable_income - b.roth_conversion - b.traditional_ira_withdrawal
            > a.ordinary_taxable_income - a.roth_conversion - a.traditional_ira_withdrawal + amount
                - 1.0
    );
    assert_eq!(b.roth_contribution, amount);
}

#[test]
fn no_penalized_ira_withdrawals_while_cheaper_money_exists() {
    let s = worker(WorkingYear {
        wages: WAGES - 20_000.0,
        tax_deferred_contribution: 20_000.0,
        ..WorkingYear::default()
    });
    let schedule = solve(&s);
    // Born March 1979: 59½ in the year turning 59 (2038).
    for y in &schedule.years {
        if y.age < 59 {
            assert_eq!(y.early_withdrawal_penalty, 0.0, "{} penalty", y.year);
        }
    }
    // The bridge years 55..59 are funded by taxable and Roth money, conversions
    // being allowed to mature.
    let bridge: f64 = schedule
        .years
        .iter()
        .filter(|y| (55..59).contains(&y.age))
        .map(|y| y.taxable_brokerage_liquidation + y.roth_withdrawal)
        .sum();
    assert!(bridge > 0.0);
}

#[test]
fn recent_conversions_are_not_withdrawn_before_they_mature() {
    // The whole Roth balance was converted last year; nothing else can fund the
    // first years except the taxable account and penalized IRA withdrawals.
    let mut s = worker(WorkingYear::default());
    s.working_years.clear();
    s.accounts.roth = 100_000.0;
    s.roth_history = RothHistory {
        conversions: vec![100_000.0],
        contributions: Vec::new(),
    };
    let schedule = solve(&s);
    let y0 = &schedule.years[0];
    // The retainer (last year's conversion grown 10%) exceeds the balance.
    assert_eq!(y0.roth_withdrawal, 0.0);
}
