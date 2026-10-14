//! End-to-end optimization tests for a standard single retiree:
//! $1M traditional IRA, $500k taxable, retiring at 62 in 2026, $80k/year target.
//!
//! Besides checking that HiGHS converges, every schedule is re-verified with
//! the engine's independent tax functions and the account identities, so the
//! MILP cannot silently drift from the tax rules it encodes.

use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::tax::federal::{
    ltcg_tax, ordinary_income_tax, standard_deduction, taxable_social_security,
};
use wealthfolio_optimization_engine::tax::irmaa::tier_for_magi;
use wealthfolio_optimization_engine::tax::FilingStatus;
use wealthfolio_optimization_engine::{
    diagnose_infeasibility, optimize, Accounts, ClaimStrategy, EngineError, Family, Objective,
    Person, PlanOptions, Rates, RothHistory, Scenario, Schedule, SocialSecurity, SolveStatus,
    WithdrawalOrder,
};

const TARGET_SPENDING: f64 = 80_000.0;
/// The self-consistent loop's convergence bar: average yearly gap between the
/// modelled and the statutory federal tax, in today's dollars.
const RESIDUAL_TOL: f64 = 50.0;
/// Largest gap tolerated in any single year.
const YEAR_TAX_TOL: f64 = 250.0;

fn standard_retiree(objective: Objective) -> Scenario {
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
            // These tests check the schedule against the tax rules; the polish
            // search has its own test below.
            polish: false,
            ..PlanOptions::default()
        },
    }
}

fn gamma(n: usize) -> f64 {
    1.025f64.powi(n as i32)
}

fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected:.2}, got {actual:.2}"
    );
}

fn assert_solved(schedule: &Schedule) {
    assert!(
        matches!(
            schedule.status,
            SolveStatus::Optimal | SolveStatus::OptimalWithinGap
        ),
        "status {:?}",
        schedule.status
    );
    assert!(
        schedule.converged(),
        "SC loop ended {:?} after {} iterations: {:?}",
        schedule.convergence,
        schedule.iterations,
        schedule.loop_residuals.last()
    );
}

/// Re-derives taxes, cash flow, balances, RMDs and IRMAA tiers from the schedule.
fn verify_schedule(s: &Scenario, schedule: &Schedule) {
    let r = &s.rates;
    let mut tax_gap_today = 0.0;
    for (n, y) in schedule.years.iter().enumerate() {
        let g = gamma(n);
        let label = |what: &str| format!("{} {what}", y.year);

        // The bracket fill is the statutory ordinary tax on the model's taxable income.
        let ordinary_income = y.traditional_ira_withdrawal
            + y.roth_conversion
            + y.taxable_social_security
            + y.other_income;
        let taxable = (ordinary_income - y.standard_deduction).max(0.0);
        assert_close(
            y.ordinary_taxable_income,
            taxable,
            1.0,
            &label("taxable income"),
        );
        assert_close(
            y.ordinary_income_tax,
            ordinary_income_tax(taxable, FilingStatus::Single, g),
            1.0,
            &label("ordinary tax"),
        );

        // Independent statutory tax: taxable Social Security and the deduction are
        // re-derived here from the realized income rather than taken from the model.
        let other =
            y.traditional_ira_withdrawal + y.roth_conversion + y.other_income + y.capital_gains;
        let taxable_ss =
            taxable_social_security(other, y.social_security_benefit, FilingStatus::Single);
        let seniors = u32::from(y.age >= 65);
        let deduction =
            standard_deduction(FilingStatus::Single, g, seniors, y.year, other + taxable_ss);
        let true_taxable =
            (y.traditional_ira_withdrawal + y.roth_conversion + y.other_income + taxable_ss
                - deduction)
                .max(0.0);
        let statutory = ordinary_income_tax(true_taxable, FilingStatus::Single, g)
            + ltcg_tax(true_taxable, y.capital_gains, FilingStatus::Single, g);
        let modelled = y.ordinary_income_tax + y.capital_gains_tax;
        // LTCG room and SS taxability come from the self-consistent loop, so the
        // modelled tax matches within the loop's tolerance.
        assert_close(
            modelled,
            statutory,
            YEAR_TAX_TOL * g,
            &label("modelled vs statutory tax"),
        );
        assert_close(
            y.statutory_federal_tax,
            statutory,
            YEAR_TAX_TOL * g,
            &label("reported statutory tax"),
        );
        tax_gap_today += (modelled - statutory).abs() / g;

        // Cash flow balances.
        let inflow = y.traditional_ira_withdrawal
            + y.taxable_brokerage_liquidation
            + y.roth_withdrawal
            + y.social_security_benefit
            + y.other_income;
        let outflow = y.net_spending
            + y.taxable_deposit
            + y.ordinary_income_tax
            + y.capital_gains_tax
            + y.medicare_premium
            + y.early_withdrawal_penalty;
        assert_close(inflow, outflow, 1e-3, &label("cash flow"));

        // Balances roll forward and stay non-negative.
        let next = schedule.years.get(n + 1);
        let (nt, nd, nr) = next.map_or(
            (
                schedule.final_taxable_balance,
                schedule.final_tax_deferred_balance,
                schedule.final_roth_balance,
            ),
            |z| (z.taxable_balance, z.tax_deferred_balance, z.roth_balance),
        );
        let expected_taxable = (1.0 + r.taxable_return)
            * (y.taxable_balance - y.taxable_brokerage_liquidation + y.taxable_deposit);
        let expected_deferred = (1.0 + r.tax_deferred_return)
            * (y.tax_deferred_balance - y.traditional_ira_withdrawal - y.roth_conversion);
        let expected_roth =
            (1.0 + r.roth_return) * (y.roth_balance - y.roth_withdrawal + y.roth_conversion);
        assert_close(nt, expected_taxable, 1e-2, &label("taxable carryover"));
        assert_close(nd, expected_deferred, 1e-2, &label("deferred carryover"));
        assert_close(nr, expected_roth, 1e-2, &label("Roth carryover"));
        for balance in [y.taxable_balance, y.tax_deferred_balance, y.roth_balance] {
            assert!(balance >= -1e-6, "{} negative balance {balance}", y.year);
        }

        // RMDs are taken.
        assert!(
            y.traditional_ira_withdrawal + 1e-3 >= y.required_minimum_distribution,
            "{} RMD {} not met by {}",
            y.year,
            y.required_minimum_distribution,
            y.traditional_ira_withdrawal
        );

        // The IRMAA tier matches MAGI from two years earlier.
        if let Some(tier) = y.irmaa_tier {
            let lookback = if n >= 2 {
                schedule.years[n - 2].magi
            } else {
                s.options.prior_magi[n]
            };
            let expected = tier_for_magi(lookback, FilingStatus::Single, g);
            // A MAGI exactly on a floor may legitimately pick the lower tier.
            assert!(
                tier == expected
                    || (tier + 1 == expected && (lookback - lookback.round()).abs() < 1.0),
                "{} tier {tier} vs expected {expected} for lookback MAGI {lookback:.2}",
                y.year
            );
        }
    }
    let average_gap = tax_gap_today / schedule.years.len() as f64;
    assert!(
        average_gap <= RESIDUAL_TOL,
        "average modelled/statutory tax gap {average_gap:.2} exceeds {RESIDUAL_TOL}"
    );
}

#[test]
fn standard_retiree_max_bequest_converges_and_funds_target_spending() {
    let s = standard_retiree(Objective::MaxBequest {
        net_spending: TARGET_SPENDING,
    });
    let schedule = optimize(&s).expect("plan should be feasible");
    assert_solved(&schedule);
    assert_eq!(
        schedule.status,
        SolveStatus::Optimal,
        "HiGHS should prove optimality"
    );
    verify_schedule(&s, &schedule);
    assert_eq!(schedule.years.len(), 34);

    for (n, y) in schedule.years.iter().enumerate() {
        assert_close(
            y.net_spending,
            TARGET_SPENDING * gamma(n),
            1e-3,
            "inflation-adjusted spending",
        );
        assert_close(y.net_spending_today, TARGET_SPENDING, 1e-3, "real spending");
        // Benefits start in the claiming year (June 2031) and never stop.
        assert_eq!(
            y.social_security_active,
            y.year >= 2031,
            "{} SS flag",
            y.year
        );
    }
    assert_eq!(schedule.claim_age, Some(67.0));
    assert!(schedule.bequest_after_tax_today > 0.0);
    assert!(schedule.lifetime_federal_tax > 0.0);
}

#[test]
fn standard_retiree_max_spending_beats_the_target_and_honours_bequest_floors() {
    // $80k/year is affordable with an estate to spare, so with no bequest floor the
    // sustainable spending must exceed it.
    let s = standard_retiree(Objective::MaxSpending { bequest_floor: 0.0 });
    let schedule = optimize(&s).expect("plan should be feasible");
    assert_solved(&schedule);
    verify_schedule(&s, &schedule);
    let unconstrained = schedule.first_year_spending();
    assert!(
        unconstrained > TARGET_SPENDING,
        "sustainable spending {unconstrained:.0} should exceed the target"
    );
    // Flat real spending profile.
    for y in &schedule.years {
        assert_close(
            y.net_spending_today,
            unconstrained,
            1e-2,
            "flat real spending",
        );
    }
    // Maximizing spending leaves nothing beyond the floor.
    assert_close(
        schedule.bequest_after_tax_today,
        0.0,
        1.0,
        "bequest at floor",
    );

    // A bequest floor costs spending and is met exactly.
    let floor = 250_000.0;
    let s = standard_retiree(Objective::MaxSpending {
        bequest_floor: floor,
    });
    let with_floor = optimize(&s).expect("plan should be feasible");
    assert_solved(&with_floor);
    verify_schedule(&s, &with_floor);
    assert!(with_floor.first_year_spending() < unconstrained);
    assert_close(
        with_floor.bequest_after_tax_today,
        floor,
        1.0,
        "bequest at floor",
    );
}

#[test]
fn the_two_objectives_are_dual_to_each_other() {
    // Spending the maxSpending optimum under maxBequest should leave about the same
    // estate. The tax model is non-convex and solved by a self-consistent loop, so the
    // two objectives may settle on slightly different fixed points.
    let floor = 250_000.0;
    let spending_plan = optimize(&standard_retiree(Objective::MaxSpending {
        bequest_floor: floor,
    }))
    .expect("maxSpending plan");
    let bequest_plan = optimize(&standard_retiree(Objective::MaxBequest {
        net_spending: spending_plan.first_year_spending(),
    }))
    .expect("maxBequest plan");
    assert_close(
        bequest_plan.bequest_after_tax_today,
        floor,
        3_000.0,
        "dual bequest",
    );
}

#[test]
fn optimizer_chooses_a_claiming_month_and_flags_active_years() {
    let mut s = standard_retiree(Objective::MaxSpending { bequest_floor: 0.0 });
    s.social_security = Some(SocialSecurity {
        monthly_pia: 2_800.0,
        claim: ClaimStrategy::Optimize,
    });
    let schedule = optimize(&s).expect("plan should be feasible");
    assert_solved(&schedule);
    verify_schedule(&s, &schedule);

    let claim = schedule.claim_age.expect("a claiming age is chosen");
    assert!((62.0..=70.0).contains(&claim));
    let flags: Vec<bool> = schedule
        .years
        .iter()
        .map(|y| y.social_security_active)
        .collect();
    // Once active, benefits continue for life.
    let first = flags.iter().position(|f| *f).expect("benefits start");
    assert!(flags[first..].iter().all(|f| *f));
    // Optimizing the claim can only improve on a fixed age-67 claim.
    let fixed = optimize(&standard_retiree(Objective::MaxSpending {
        bequest_floor: 0.0,
    }))
    .expect("fixed-claim plan");
    assert!(schedule.first_year_spending() >= fixed.first_year_spending() - 1.0);
}

#[test]
fn roth_conversion_cap_is_respected() {
    let cap = 20_000.0;
    let mut s = standard_retiree(Objective::MaxSpending { bequest_floor: 0.0 });
    s.options.max_roth_conversion = Some(cap);
    let schedule = optimize(&s).expect("plan should be feasible");
    verify_schedule(&s, &schedule);
    for (n, y) in schedule.years.iter().enumerate() {
        assert!(
            y.roth_conversion <= cap * gamma(n) + 1e-3,
            "{} conversion {}",
            y.year,
            y.roth_conversion
        );
    }
}

#[test]
fn medicare_loop_mode_matches_premium_schedule() {
    let mut s = standard_retiree(Objective::MaxBequest {
        net_spending: TARGET_SPENDING,
    });
    s.options.optimize_medicare = false;
    let schedule = optimize(&s).expect("plan should be feasible");
    assert_solved(&schedule);
    verify_schedule(&s, &schedule);
    for y in &schedule.years {
        assert_eq!(y.medicare_premium > 0.0, y.age >= 65, "{} premium", y.year);
    }
}

#[test]
fn unfundable_spending_is_reported_as_infeasible_with_conflicts() {
    let s = standard_retiree(Objective::MaxBequest {
        net_spending: 400_000.0,
    });
    let result = optimize(&s);
    assert!(
        matches!(&result, Err(EngineError::Infeasible(report)) if report.contains("fixed net spending")),
        "expected infeasible, got {result:?}"
    );

    let report = diagnose_infeasibility(&s)
        .expect("diagnosis runs")
        .expect("scenario is infeasible");
    // The spending goal is what cannot be met, and only goals are relaxed.
    assert!(report
        .relaxed
        .iter()
        .any(|r| r.family == Family::FixedSpending && r.slack_today > 100_000.0));
    assert!(report.relaxed.iter().all(|r| r.family.is_elastic()));
    assert!(!report.binding_by_year.is_empty());
}

#[test]
fn unreachable_bequest_floor_is_reported_with_its_shortfall() {
    let s = standard_retiree(Objective::MaxSpending {
        bequest_floor: 5_000_000.0,
    });
    let report = diagnose_infeasibility(&s)
        .expect("diagnosis runs")
        .expect("scenario is infeasible");
    let shortfall = report
        .relaxed
        .iter()
        .find(|r| r.family == Family::BequestFloor)
        .expect("bequest floor is the conflict");
    assert!(!shortfall.over);
    // The diagnosis runs on an optimistic relaxation, so its shortfall is a lower
    // bound on the real gap to the largest reachable estate, and a close one.
    let max_estate = optimize(&standard_retiree(Objective::MaxBequest {
        net_spending: 0.0,
    }))
    .expect("zero spending is feasible");
    let true_gap = 5_000_000.0 - max_estate.bequest_after_tax_today;
    assert!(
        shortfall.slack_today <= true_gap + 1.0 && shortfall.slack_today >= 0.75 * true_gap,
        "reported {:.0} vs true gap {true_gap:.0}",
        shortfall.slack_today
    );
}

#[test]
fn longevity_table_sets_a_conservative_horizon() {
    use wealthfolio_optimization_engine::mortality::age_at_survival_probability;
    let mut s = standard_retiree(Objective::MaxBequest {
        net_spending: TARGET_SPENDING,
    });
    s.person.planning_age = age_at_survival_probability(Sex::Male, 62, 0.1);
    let schedule = optimize(&s).expect("plan should be feasible");
    assert_solved(&schedule);
    assert_eq!(
        schedule.years.last().map(|y| y.age),
        Some(s.person.planning_age)
    );
}

#[test]
fn feasible_scenarios_have_no_infeasibility_report() {
    let s = standard_retiree(Objective::MaxBequest {
        net_spending: TARGET_SPENDING,
    });
    assert!(diagnose_infeasibility(&s)
        .expect("diagnosis runs")
        .is_none());
}

#[test]
fn polishing_never_leaves_a_worse_or_inconsistent_plan() {
    let mut s = standard_retiree(Objective::MaxBequest {
        net_spending: TARGET_SPENDING,
    });
    let loop_only = optimize(&s).expect("feasible");
    s.options.polish = true;
    let polished = optimize(&s).expect("feasible");
    assert_solved(&polished);
    verify_schedule(&s, &polished);
    assert!(
        polished.bequest_after_tax_today >= loop_only.bequest_after_tax_today - 1.0,
        "polished {:.0} vs loop {:.0}",
        polished.bequest_after_tax_today,
        loop_only.bequest_after_tax_today
    );
}

#[test]
fn taxable_first_baseline_follows_the_rule_and_never_beats_the_optimum() {
    let s = standard_retiree(Objective::MaxBequest {
        net_spending: TARGET_SPENDING,
    });
    let optimal = optimize(&s).expect("feasible");
    let mut naive = s.clone();
    naive.options.withdrawal_order = WithdrawalOrder::TaxableFirst;
    naive.options.allow_roth_conversions = false;
    let baseline = optimize(&naive).expect("feasible");
    // Converged on average (the loop's bar); selling taxable shares first puts
    // large gains in single years, where loop-mode LTCG bands can be a few
    // hundred dollars off, so the strict per-year check is for optimized plans.
    assert_solved(&baseline);

    let mut taxable_empty = false;
    for y in &baseline.years {
        assert_eq!(y.roth_conversion, 0.0, "{} conversion", y.year);
        // The gate looks at the year-end balance, so the year that empties the
        // taxable account may already draw on the IRA.
        taxable_empty |=
            y.taxable_balance - y.taxable_brokerage_liquidation + y.taxable_deposit < 1.0;
        if !taxable_empty {
            // Until the taxable account runs out, the IRA only pays its RMD.
            assert!(
                y.traditional_ira_withdrawal <= y.required_minimum_distribution + 1.0,
                "{} IRA {} before taxable is empty",
                y.year,
                y.traditional_ira_withdrawal
            );
            assert_eq!(y.roth_withdrawal, 0.0, "{} Roth", y.year);
        }
    }
    assert!(
        optimal.bequest_after_tax_today >= baseline.bequest_after_tax_today - 1.0,
        "optimal {:.0} vs baseline {:.0}",
        optimal.bequest_after_tax_today,
        baseline.bequest_after_tax_today
    );
}

#[test]
fn required_spending_is_a_hard_floor() {
    let sustainable = optimize(&standard_retiree(Objective::MaxSpending {
        bequest_floor: 0.0,
    }))
    .expect("plan should be feasible")
    .first_year_spending();

    // Below what the plan sustains, the floor changes nothing.
    let mut s = standard_retiree(Objective::MaxSpending { bequest_floor: 0.0 });
    s.options.required_spending = vec![0.9 * sustainable; s.years()];
    let floored = optimize(&s).expect("floor below the optimum is feasible");
    assert_solved(&floored);
    assert_close(
        floored.first_year_spending(),
        sustainable,
        0.005 * sustainable,
        "spending with a slack floor",
    );
    for y in &floored.years {
        assert!(y.net_spending_today >= 0.9 * sustainable - 1.0);
    }

    // Above it, no plan exists and the floor is what is out of reach.
    s.options.required_spending = vec![1.2 * sustainable; s.years()];
    let report = diagnose_infeasibility(&s)
        .expect("diagnosis runs")
        .expect("scenario is infeasible");
    assert!(report
        .relaxed
        .iter()
        .any(|r| r.family == Family::RequiredSpending));
}
