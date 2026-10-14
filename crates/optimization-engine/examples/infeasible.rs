//! Shows the elastic-filter report for an unfundable plan: `cargo run --example infeasible`.
use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory,
    Scenario, SocialSecurity,
};

fn main() {
    let scenario = Scenario {
        start_year: 2026,
        person: Person {
            birth_year: 1964,
            birth_month: 6,
            sex: Sex::Male,
            planning_age: 95,
        },
        accounts: Accounts {
            taxable: 500_000.0,
            taxable_cost_basis: 350_000.0,
            tax_deferred: 1_000_000.0,
            roth: 0.0,
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
            monthly_pia: 2_800.0,
            claim: ClaimStrategy::AtAge(67.0),
        }),
        working_years: Vec::new(),
        roth_history: RothHistory::default(),
        // A bequest floor the accounts cannot reach on top of $80k/year of spending.
        objective: Objective::MaxSpending {
            bequest_floor: 5_000_000.0,
        },
        options: PlanOptions {
            prior_magi: [90_000.0, 90_000.0],
            ..PlanOptions::default()
        },
    };
    match optimize(&scenario) {
        Ok(schedule) => println!(
            "unexpectedly feasible: {:.0}",
            schedule.first_year_spending()
        ),
        Err(error) => println!("{error}"),
    }
}
