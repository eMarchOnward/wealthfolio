//! Compares the two objectives on the standard retiree: `cargo run --example duality`.
use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, Objective, Person, PlanOptions, Rates, RothHistory,
    Scenario, SocialSecurity,
};

fn scenario(objective: Objective) -> Scenario {
    Scenario {
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
        objective,
        options: PlanOptions {
            prior_magi: [90_000.0, 90_000.0],
            loop_stabilizer: std::env::var("STABILIZER")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(PlanOptions::default().loop_stabilizer),
            ..PlanOptions::default()
        },
    }
}

fn main() {
    let at_80k = optimize(&scenario(Objective::MaxBequest {
        net_spending: 80_000.0,
    }))
    .expect("feasible");
    println!(
        "maxBequest @80k: bequest {:.0} ({:?}, {} it), lifetime tax {:.0}",
        at_80k.bequest_after_tax_today,
        at_80k.convergence,
        at_80k.iterations,
        at_80k.lifetime_federal_tax
    );
    for floor in [0.0, 250_000.0] {
        let sp = optimize(&scenario(Objective::MaxSpending {
            bequest_floor: floor,
        }))
        .expect("feasible");
        let back = optimize(&scenario(Objective::MaxBequest {
            net_spending: sp.first_year_spending(),
        }))
        .expect("feasible");
        println!(
            "floor {floor:.0}: maxSpending {:.2} ({:?}, {} it, tax {:.0}) -> maxBequest at that spending leaves {:.0} ({:?}, {} it, tax {:.0})",
            sp.first_year_spending(), sp.convergence, sp.iterations, sp.lifetime_federal_tax,
            back.bequest_after_tax_today, back.convergence, back.iterations, back.lifetime_federal_tax
        );
    }
}
