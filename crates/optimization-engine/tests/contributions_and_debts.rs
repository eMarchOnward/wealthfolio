//! Phase 4: the optimizer chooses working-year contributions within IRS limits
//! (and earns the employer match), and decides whether to pay debts down early.

use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, ContributionChoice, Debt, Objective, Person, PlanOptions,
    Rates, RothHistory, Scenario, Schedule, SocialSecurity, WorkingYear,
};

const INFLATION: f64 = 0.025;
const GROSS_PAY: f64 = 130_000.0;

fn gamma(n: usize) -> f64 {
    (1.0 + INFLATION).powi(n as i32)
}

/// A 46-year-old earning $130k gross who retires at 55.
fn worker() -> Scenario {
    Scenario {
        start_year: 2026,
        person: Person {
            birth_year: 1980,
            birth_month: 5,
            sex: Sex::Female,
            planning_age: 92,
        },
        accounts: Accounts {
            taxable: 150_000.0,
            taxable_cost_basis: 120_000.0,
            tax_deferred: 300_000.0,
            roth: 60_000.0,
            hsa: 10_000.0,
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
        working_years: vec![
            WorkingYear {
                wages: GROSS_PAY,
                ..WorkingYear::default()
            };
            9
        ],
        roth_history: RothHistory::default(),
        objective: Objective::MaxSpending { bequest_floor: 0.0 },
        options: PlanOptions {
            prior_magi: [GROSS_PAY, GROSS_PAY],
            polish: false,
            ..PlanOptions::default()
        },
    }
}

fn choice() -> ContributionChoice {
    ContributionChoice {
        hsa_limit: 4_400.0,
        match_rate: 0.5,
        match_cap: 0.06,
        ..ContributionChoice::limits_2026()
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
fn chosen_contributions_respect_limits_and_capture_the_match() {
    let mut s = worker();
    s.options.optimize_contributions = Some(choice());
    let schedule = solve(&s);
    let c = choice();
    for (n, y) in schedule.years.iter().take(9).enumerate() {
        let g = gamma(n);
        let deferrals = y.tax_deferred_contribution - y.employer_match;
        assert!(
            y.employer_match <= 0.5 * 0.06 * GROSS_PAY * g + 1.0,
            "{} match {:.0}",
            y.year,
            y.employer_match
        );
        // Deferrals (pre-tax plus Roth 401(k)) stay under the elective limit;
        // the Roth total also holds the Roth IRA.
        assert!(
            deferrals <= c.deferral_limit(y.age) * g + 1.0,
            "{} deferrals {:.0}",
            y.year,
            deferrals
        );
        assert!(y.roth_contribution <= (c.deferral_limit(y.age) + c.ira_limit_at(y.age)) * g + 1.0);
        assert!(y.hsa_contribution <= c.hsa_limit_at(y.age) * g + 1.0);
        // Each chosen contribution is reported on its own.
        assert!((y.pre_tax_deferral + y.employer_match - y.tax_deferred_contribution).abs() < 1.0);
        assert!((y.roth_401k + y.roth_ira - y.roth_contribution).abs() < 1.0);
        // The match is free money: the plan always earns all of it.
        assert!(
            y.employer_match >= 0.5 * 0.06 * GROSS_PAY * g - 1.0,
            "{} leaves match on the table: {:.0}",
            y.year,
            y.employer_match
        );
    }
}

#[test]
fn choosing_contributions_never_does_worse_than_a_fixed_plan() {
    // A typical fixed plan: 10% pre-tax, the match, nothing else.
    let mut fixed = worker();
    for w in &mut fixed.working_years {
        w.wages = GROSS_PAY - 13_000.0;
        w.tax_deferred_contribution = 13_000.0;
        w.employer_contribution = 0.5 * 0.06 * GROSS_PAY;
    }
    let given = solve(&fixed);
    // Given amounts are reported as such, with no chosen Roth split.
    let first = &given.years[0];
    assert!((first.pre_tax_deferral - 13_000.0).abs() < 1.0);
    assert!((first.employer_match - 0.5 * 0.06 * GROSS_PAY).abs() < 1.0);
    assert_eq!(first.roth_401k + first.roth_ira, 0.0);

    let mut free = worker();
    free.options.optimize_contributions = Some(choice());
    let chosen = solve(&free);
    assert!(
        chosen.first_year_spending() >= given.first_year_spending() - 1.0,
        "chosen {:.0} vs fixed {:.0}",
        chosen.first_year_spending(),
        given.first_year_spending()
    );
}

#[test]
fn expensive_debt_is_paid_down_early_and_cheap_debt_is_not() {
    let debt = |rate: f64| Debt {
        label: format!("loan at {rate}"),
        balance: 40_000.0,
        annual_rate: rate,
        annual_payment: 8_000.0,
        ..Debt::default()
    };
    let scheduled = |rate: f64, n: usize| {
        let d = debt(rate);
        let mut balance = d.balance;
        for _ in 0..n {
            balance = (balance - d.annual_payment).max(0.0) * (1.0 + d.annual_rate);
        }
        balance
    };

    let mut s = worker();
    s.options.debts = vec![debt(0.18)];
    let costly = solve(&s);
    // At 18% (far above the 5% return), the balance is gone well ahead of plan.
    assert!(
        costly.years[2].debt_balance < scheduled(0.18, 2) - 1_000.0,
        "18% debt {:.0} vs schedule {:.0}",
        costly.years[2].debt_balance,
        scheduled(0.18, 2)
    );

    s.options.debts = vec![debt(0.01)];
    let cheap = solve(&s);
    // At 1%, investing wins: the balance follows the minimum schedule.
    for (n, y) in cheap.years.iter().take(5).enumerate() {
        assert!(
            (y.debt_balance - scheduled(0.01, n)).abs() < 1.0,
            "{} 1% debt {:.0} vs schedule {:.0}",
            y.year,
            y.debt_balance,
            scheduled(0.01, n)
        );
    }
}

#[test]
fn a_payment_that_cannot_cover_interest_is_rejected() {
    let mut s = worker();
    s.options.debts = vec![Debt {
        label: "underwater".into(),
        balance: 100_000.0,
        annual_rate: 0.1,
        annual_payment: 5_000.0,
        ..Debt::default()
    }];
    assert!(optimize(&s).is_err());
}

#[test]
fn a_home_bought_later_takes_its_down_payment_and_mortgage_then() {
    let mut s = worker();
    // Today's loan is cleared by a home sale in year 3; the new mortgage starts then.
    s.options.debts = vec![
        Debt {
            label: "current mortgage".into(),
            balance: 150_000.0,
            annual_rate: 0.03,
            annual_payment: 12_000.0,
            payoff: Some(3),
            ..Debt::default()
        },
        Debt {
            label: "new mortgage".into(),
            balance: 320_000.0,
            annual_rate: 0.02,
            annual_payment: 24_300.0,
            start: 3,
            ..Debt::default()
        },
    ];
    let mut flows = vec![0.0; 3];
    flows.push(250_000.0 - 92_000.0);
    s.options.cash_flows = flows;
    let plan = solve(&s);

    let y = &plan.years;
    assert!(y[1].debt_balance > 100_000.0 && y[1].debt_balance < 150_000.0);
    assert!((y[3].cash_flow - 158_000.0 * s.gamma(3)).abs() < 1.0);
    // Cheaper than the 5% return, the new mortgage follows its schedule; the
    // old one is gone.
    let new_owed = y[4].debt_balance;
    assert!(
        (new_owed - (320_000.0 - 24_300.0) * 1.02).abs() < 1.0,
        "new mortgage after its first year {new_owed:.0}"
    );
    assert!(y[0].debt_payment >= 12_000.0 - 1.0);

    s.options.debts[1].start = s.years();
    assert!(
        optimize(&s).is_err(),
        "a loan starting after the plan is rejected"
    );
}

#[test]
fn the_first_years_roth_conversion_can_be_pinned() {
    let mut s = worker();
    s.options.first_year_conversion = Some(12_345.0);
    let plan = solve(&s);
    assert!((plan.years[0].roth_conversion - 12_345.0 * s.gamma(0)).abs() < 1.0);
    s.options.first_year_conversion = Some(-1.0);
    assert!(optimize(&s).is_err());
}

#[test]
fn only_the_chosen_share_of_leftover_pay_is_invested() {
    let mut all = worker();
    all.options.optimize_contributions = Some(choice());
    let invested_all = solve(&all);

    let mut part = all.clone();
    part.options.leftover_invested = Some(0.8);
    let invested_part = solve(&part);

    let mut saw_leftover = false;
    for y in invested_part.years.iter().take(9) {
        // 80% deposited, 20% spent: deposit = 4 x spent.
        assert!(
            (y.taxable_deposit - 4.0 * y.leftover_spent).abs() < 1.0,
            "{} deposit {:.0} spent {:.0}",
            y.year,
            y.taxable_deposit,
            y.leftover_spent
        );
        saw_leftover |= y.leftover_spent > 1.0;
    }
    assert!(saw_leftover, "no working year had leftover pay");
    // Retired years deposit freely.
    assert!(invested_part
        .years
        .iter()
        .skip(9)
        .all(|y| y.leftover_spent == 0.0));
    // Spending the rest leaves less for retirement.
    assert!(
        invested_part.first_year_spending() <= invested_all.first_year_spending() + 1.0,
        "part {:.0} vs all {:.0}",
        invested_part.first_year_spending(),
        invested_all.first_year_spending()
    );
}

#[test]
fn a_later_loan_cannot_be_paid_before_it_starts() {
    // Dearer than the 5% return, so the plan pays it off as soon as it can.
    let mut s = worker();
    s.options.debts = vec![Debt {
        label: "new mortgage".into(),
        balance: 300_000.0,
        annual_rate: 0.08,
        annual_payment: 26_000.0,
        start: 3,
        ..Debt::default()
    }];
    let plan = solve(&s);
    for y in plan.years.iter().take(3) {
        assert!(
            y.debt_payment.abs() < 1.0,
            "{} pays {:.0} before the loan exists",
            y.year,
            y.debt_payment
        );
    }
    assert!(
        plan.years[3].debt_payment > 26_000.0,
        "paid down once it starts"
    );
}
