//! Times one optimization of the standard retiree: `cargo run --example time_solve`.
use std::time::Instant;

use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory,
    Scenario, SocialSecurity,
};

fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    let exact_ltcg = std::env::args().any(|a| a == "--ltcg");
    let iterations = std::env::args()
        .find_map(|a| a.strip_prefix("--iterations=").and_then(|v| v.parse().ok()))
        .unwrap_or(29);
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
        objective: Objective::MaxSpending {
            bequest_floor: 250_000.0,
        },
        options: PlanOptions {
            prior_magi: [90_000.0, 90_000.0],
            max_iterations: iterations,
            solver_verbose: verbose,
            optimize_ltcg: exact_ltcg,
            ..PlanOptions::default()
        },
    };
    let start = Instant::now();
    match optimize(&scenario) {
        Ok(s) => {
            for (i, r) in s.loop_residuals.iter().enumerate() {
                println!("  iter {}: {r:?}", i + 1);
            }
            for y in &s.years {
                let modelled = y.ordinary_income_tax + y.capital_gains_tax;
                if (modelled - y.statutory_federal_tax).abs() > 25.0
                    || std::env::args().any(|a| a == "--years")
                {
                    println!(
                        "  {} age {} ira {:>8.0} conv {:>8.0} liq {:>8.0} ss {:>7.0} txss {:>7.0} G {:>8.0} gains {:>7.0} ord_tax {:>7.0} cg_tax {:>6.0} statutory {:>7.0} irmaa {:?}",
                        y.year, y.age, y.traditional_ira_withdrawal, y.roth_conversion, y.taxable_brokerage_liquidation,
                        y.social_security_benefit, y.taxable_social_security, y.ordinary_taxable_income, y.capital_gains,
                        y.ordinary_income_tax, y.capital_gains_tax, y.statutory_federal_tax, y.irmaa_tier
                    );
                }
            }
            println!(
            "status {:?}, iterations {}, convergence {:?}, first-year spending {:.0}, bequest {:.0}, {:.1}s",
            s.status,
            s.iterations,
            s.convergence,
            s.first_year_spending(),
            s.bequest_after_tax_today,
            start.elapsed().as_secs_f64()
        )
        }
        Err(e) => println!("error after {:.1}s: {e}", start.elapsed().as_secs_f64()),
    }
}
