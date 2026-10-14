//! Solves a scenario: builds the MILP, runs Owl's self-consistent loop for the
//! non-linear tax terms, and turns the solution into a [`Schedule`].

use good_lp::solvers::highs::HighsSolution;
use good_lp::{ResolutionError, Solution, SolutionStatus, SolverModel};
use log::debug;

use crate::diagnostics::{self, InfeasibilityReport};
use crate::error::{EngineError, Result};
use crate::model::{build, Integrality, LoopParams, Model, Timeline, Vars};
use crate::scenario::{Scenario, WorkingYear};
use crate::schedule::{Convergence, LoopResidual, Schedule, SolveStatus, YearPlan};
use crate::social_security::claim_age_for_month;
use crate::tax::aca::{net_premium, premium_below_cliff, CLIFF as ACA_CLIFF};
use crate::tax::federal::{
    ltcg_tax, ordinary_income_tax, taxable_social_security, LTCG_RATES, ORDINARY_RATES,
    SS_MAX_TAXABLE_FRACTION,
};
use crate::tax::irmaa::annual_premium;

/// Owl's convergence bars: objective change (absolute, in today's dollars, or
/// relative) and the per-year plan/assumption disagreement.
const ABS_TOL: f64 = 100.0;
const REL_TOL: f64 = 5e-5;
const RESIDUAL_TOL: f64 = 50.0;
/// Weight of the new Social Security taxable-fraction estimate (Owl `_PSI_DAMP`).
const PSI_DAMP: f64 = 0.3;
/// Weight of the new ordinary-income estimate that sets LTCG band room.
const ORDINARY_DAMP: f64 = 0.5;
/// Exact (polish) iterations after the self-consistent loop.
const POLISH_ITERATIONS: usize = 4;
/// Longest objective cycle the loop looks for.
const MAX_CYCLE: usize = 4;

/// Optimizes the scenario and returns the lifetime schedule.
///
/// Returns [`EngineError::Infeasible`] with an elastic-filter report when no
/// plan satisfies the constraints (for example, spending that the assets
/// cannot fund, or a bequest floor that is too high).
pub fn optimize(scenario: &Scenario) -> Result<Schedule> {
    scenario.validate()?;
    let timeline = Timeline::new(scenario);
    let params = initial_params(scenario, &timeline);

    // An infeasible LP relaxation proves the MILP infeasible in milliseconds;
    // branch-and-bound can take minutes to reach the same conclusion.
    if relaxation_is_infeasible(scenario, &timeline, &params)? {
        let model = build(scenario, &timeline, &params, Integrality::Relaxed);
        let report = diagnostics::diagnose(model, scenario.start_year, &timeline.gamma)?;
        return Err(EngineError::Infeasible(report.to_string()));
    }

    let mut run = self_consistent_loop(
        scenario,
        &timeline,
        params,
        scenario.options.max_iterations,
        scenario.options.time_limit_secs,
    )?;
    if let Some(polished) = polish(scenario, &timeline, &run)? {
        let tol = ABS_TOL.max(REL_TOL * run.objective.abs());
        let iterations = run.iterations + polished.iterations;
        let mut residuals = std::mem::take(&mut run.residuals);
        residuals.extend(polished.residuals.iter().copied());
        if polished.objective > run.objective + tol && polished.residual <= RESIDUAL_TOL {
            debug!(
                "polish improved the objective {:.0} -> {:.0}",
                run.objective, polished.objective
            );
            run = polished;
        }
        run.iterations = iterations;
        run.residuals = residuals;
    }

    let mut schedule = run.schedule;
    schedule.iterations = run.iterations;
    schedule.convergence = run.outcome;
    schedule.loop_residuals = run.residuals;
    Ok(schedule)
}

/// The plan a self-consistent loop settled on.
struct LoopRun {
    schedule: Schedule,
    /// Loop parameters implied by that plan (its own taxes, premiums, gains).
    implied: LoopParams,
    objective: f64,
    /// Largest per-family residual of that plan.
    residual: f64,
    iterations: usize,
    outcome: Convergence,
    residuals: Vec<LoopResidual>,
}

/// Owl's self-consistent loop: solve, recompute the non-linear tax terms from
/// the solution, damp, and repeat until the objective and the terms settle.
/// When `params` asks for exact Social Security taxability or LTCG bands, every
/// iteration keeps asking: only the remaining loop terms are re-estimated.
fn self_consistent_loop(
    s: &Scenario,
    t: &Timeline,
    mut params: LoopParams,
    max_iterations: usize,
    time_limit_secs: f64,
) -> Result<LoopRun> {
    let (exact_ss, exact_ltcg) = (params.exact_ss_taxability, params.exact_ltcg);
    let mut history: Vec<(Schedule, LoopParams)> = Vec::new();
    let mut residuals: Vec<LoopResidual> = Vec::new();
    let mut outcome = Convergence::MaxIterations;

    for iteration in 1..=max_iterations {
        let (status, values, objective) = solve_once(s, t, &params, time_limit_secs)?;
        let mut implied = updated_params(s, t, &params, &values);
        implied.exact_ss_taxability = exact_ss;
        implied.exact_ltcg = exact_ltcg;
        let plan = extract(s, t, &params, &implied, &values, status);
        let residual = loop_residual(s, t, &params, &implied, &values, objective);
        debug!("iteration {iteration}: {residual:?}");

        let tol = ABS_TOL.max(REL_TOL * objective.abs());
        let settled = residuals
            .last()
            .is_some_and(|prev| (objective - prev.objective).abs() <= tol);
        residuals.push(residual);
        history.push((plan, implied.clone()));
        if settled && residual.max() <= RESIDUAL_TOL {
            outcome = Convergence::Converged;
            break;
        }
        if let Some(length) = detect_cycle(&residuals, tol) {
            outcome = Convergence::Cycle { length };
            break;
        }
        params = damped(&params, implied);
    }

    // Converged: the last plan. Otherwise, within the final cycle or the whole run,
    // the best plan whose taxes already agree with the statutory ones; failing that,
    // the most self-consistent one. Ranking inconsistent iterates by objective would
    // favour the one whose tax assumptions are most optimistic, not most accurate.
    let window = match outcome {
        Convergence::Converged => 1,
        Convergence::Cycle { length } => length,
        Convergence::MaxIterations => history.len(),
    };
    let iterations = history.len();
    let first = iterations.saturating_sub(window);
    let consistent = (first..iterations)
        .filter(|i| residuals[*i].max() <= RESIDUAL_TOL)
        .max_by(|a, b| residuals[*a].objective.total_cmp(&residuals[*b].objective));
    let best = consistent
        .or_else(|| {
            (first..iterations).min_by(|a, b| residuals[*a].max().total_cmp(&residuals[*b].max()))
        })
        .ok_or_else(|| EngineError::Solver("no iteration ran".into()))?;
    let (objective, residual) = (residuals[best].objective, residuals[best].max());
    let (schedule, implied) = history.swap_remove(best);
    Ok(LoopRun {
        schedule,
        implied,
        objective,
        residual,
        iterations,
        outcome,
        residuals,
    })
}

/// The loop fixes each year's taxable share of Social Security and the LTCG
/// band room from the previous iterate, so it can settle on a fixed point whose
/// income pattern is not the best one. Polishing re-solves with both computed
/// exactly (binaries), starting from the loop's own estimates; only the gain
/// fractions, the senior-deduction phase-out and loop-mode Medicare are still
/// iterated, which settles in a few capped solves.
fn polish(s: &Scenario, t: &Timeline, run: &LoopRun) -> Result<Option<LoopRun>> {
    let fixed_claim =
        s.social_security.is_some() && t.ss_allowed.iter().filter(|a| **a).count() == 1;
    if !s.options.polish || !fixed_claim || s.options.polish_time_secs <= 0.0 {
        return Ok(None);
    }
    let mut exact = run.implied.clone();
    exact.exact_ss_taxability = true;
    exact.exact_ltcg = true;
    match self_consistent_loop(s, t, exact, POLISH_ITERATIONS, s.options.polish_time_secs) {
        Ok(polished) => Ok(Some(polished)),
        // The loop already produced a feasible plan; a failed polish just keeps it.
        Err(EngineError::Infeasible(_)) | Err(EngineError::Solver(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// Length of a repeating objective cycle at the end of `history`, if any.
fn detect_cycle(history: &[LoopResidual], tol: f64) -> Option<usize> {
    (2..=MAX_CYCLE).find(|&len| {
        history.len() >= 2 * len
            && (1..=len).all(|i| {
                let a = history[history.len() - i].objective;
                let b = history[history.len() - i - len].objective;
                (a - b).abs() <= tol
            })
    })
}

/// Next iteration's parameters: the Social Security taxable fraction (as in Owl)
/// and the ordinary-income estimate behind LTCG band room are damped; the other
/// implied values are taken directly.
fn damped(previous: &LoopParams, implied: LoopParams) -> LoopParams {
    let blend = |old: &[f64], new: &[f64], weight: f64| -> Vec<f64> {
        old.iter()
            .zip(new)
            .map(|(o, n)| weight * n + (1.0 - weight) * o)
            .collect()
    };
    LoopParams {
        ss_taxable_fraction: blend(
            &previous.ss_taxable_fraction,
            &implied.ss_taxable_fraction,
            PSI_DAMP,
        ),
        ordinary_income_estimate: blend(
            &previous.ordinary_income_estimate,
            &implied.ordinary_income_estimate,
            ORDINARY_DAMP,
        ),
        ..implied
    }
}

/// Builds the strict model and reports why it is infeasible, if it is.
pub fn diagnose_infeasibility(scenario: &Scenario) -> Result<Option<InfeasibilityReport>> {
    scenario.validate()?;
    let timeline = Timeline::new(scenario);
    let params = initial_params(scenario, &timeline);
    if relaxation_is_infeasible(scenario, &timeline, &params)? {
        let model = build(scenario, &timeline, &params, Integrality::Relaxed);
        return diagnostics::diagnose(model, scenario.start_year, &timeline.gamma).map(Some);
    }
    match solve_once(
        scenario,
        &timeline,
        &params,
        scenario.options.time_limit_secs,
    ) {
        Ok(_) => Ok(None),
        Err(EngineError::Infeasible(_)) => diagnostics::diagnose(
            build(scenario, &timeline, &params, Integrality::Relaxed),
            scenario.start_year,
            &timeline.gamma,
        )
        .map(Some),
        Err(other) => Err(other),
    }
}

/// True when the LP relaxation (binaries in [0, 1]) has no feasible point.
fn relaxation_is_infeasible(s: &Scenario, t: &Timeline, p: &LoopParams) -> Result<bool> {
    let Model {
        problem,
        rows,
        objective,
        ..
    } = build(s, t, p, Integrality::Relaxed);
    let mut solver = problem.maximise(objective).using(good_lp::highs);
    solver.set_verbose(false);
    for row in &rows {
        solver.add_constraint(row.to_constraint());
    }
    match solver.solve() {
        Ok(_) => Ok(false),
        Err(ResolutionError::Infeasible) => Ok(true),
        Err(ResolutionError::Unbounded) => Err(EngineError::Unbounded),
        Err(other) => Err(EngineError::Solver(other.to_string())),
    }
}

/// A solved model: variable handles plus the solver's values.
pub(crate) struct Values {
    vars: Vars,
    solution: HighsSolution,
}

impl Values {
    /// Variable value with solver noise below a micro-dollar rounded to zero.
    fn get(&self, v: good_lp::Variable) -> f64 {
        clean(self.solution.value(v))
    }

    fn eval(&self, e: &good_lp::Expression) -> f64 {
        clean(e.eval_with(&self.solution))
    }
}

fn clean(x: f64) -> f64 {
    if x.abs() < 1e-7 {
        0.0
    } else {
        x
    }
}

fn solve_once(
    s: &Scenario,
    t: &Timeline,
    p: &LoopParams,
    time_limit_secs: f64,
) -> Result<(SolveStatus, Values, f64)> {
    let Model {
        problem,
        rows,
        objective,
        vars,
    } = build(s, t, p, Integrality::Exact);
    let objective_expr = objective.clone();

    let mut solver = problem.maximise(objective).using(good_lp::highs);
    solver.set_verbose(s.options.solver_verbose);
    let mut solver = solver
        .set_mip_rel_gap(s.options.mip_rel_gap)
        .map_err(|e| EngineError::InvalidInput(format!("MIP gap: {e:?}")))?
        .set_time_limit(time_limit_secs)
        .set_threads(1);
    for row in &rows {
        solver.add_constraint(row.to_constraint());
    }

    match solver.solve() {
        Ok(solution) => {
            let status = match solution.status() {
                SolutionStatus::Optimal => SolveStatus::Optimal,
                SolutionStatus::GapLimit => SolveStatus::OptimalWithinGap,
                SolutionStatus::TimeLimit => SolveStatus::TimeLimit,
            };
            let objective = objective_expr.eval_with(&solution);
            Ok((status, Values { vars, solution }, objective))
        }
        Err(ResolutionError::Infeasible) => {
            let report = diagnostics::diagnose(
                build(s, t, p, Integrality::Relaxed),
                s.start_year,
                &t.gamma,
            )?;
            Err(EngineError::Infeasible(report.to_string()))
        }
        Err(ResolutionError::Unbounded) => Err(EngineError::Unbounded),
        Err(other) => Err(EngineError::Solver(other.to_string())),
    }
}

fn initial_params(s: &Scenario, t: &Timeline) -> LoopParams {
    let a = &s.accounts;
    let gain_fraction = if a.taxable > 0.0 {
        (1.0 - a.taxable_cost_basis / a.taxable).max(0.0)
    } else {
        0.0
    };
    let years = t.years;
    let mut params = LoopParams {
        // Start conservatively: 85% of benefits taxable and no senior bonus.
        ss_taxable_fraction: vec![SS_MAX_TAXABLE_FRACTION; years],
        magi_estimate: vec![f64::INFINITY; years],
        gain_fraction: vec![gain_fraction; years],
        medicare_cost: vec![0.0; years],
        ordinary_income_estimate: vec![0.0; years],
        proximal_weight: vec![0.0; years],
        aca_cost: (0..years)
            .map(|n| {
                let slcsp = t.aca_benchmark[n];
                if s.options.aca_exact_cliff {
                    premium_below_cliff(slcsp, t.fpl[n])
                } else {
                    slcsp
                }
            })
            .collect(),
        exact_ss_taxability: false,
        exact_ltcg: false,
    };
    params.medicare_cost = medicare_from_magi(s, t, &vec![0.0; years]);
    params
}

/// Medicare premiums for the loop (non-optimized) mode from yearly MAGI.
fn medicare_from_magi(s: &Scenario, t: &Timeline, magi: &[f64]) -> Vec<f64> {
    (0..t.years)
        .map(|n| {
            if !t.medicare_enrolled[n] {
                return 0.0;
            }
            let lookback = if n >= 2 {
                magi[n - 2]
            } else {
                s.options.prior_magi[n]
            };
            annual_premium(
                lookback,
                s.filing_status(),
                t.gamma[n],
                1,
                s.options.include_part_d,
            )
        })
        .collect()
}

/// Recomputes the loop parameters from a solution (Owl's SC-loop update).
fn updated_params(s: &Scenario, t: &Timeline, p: &LoopParams, values: &Values) -> LoopParams {
    let v = &values.vars;
    let status = s.filing_status();
    let mut ss_taxable_fraction = Vec::with_capacity(t.years);
    let mut magi = Vec::with_capacity(t.years);
    let mut aca_cost = Vec::with_capacity(t.years);
    let ordinary_income_estimate = (0..t.years)
        .map(|n| v.bracket_fill[n].iter().map(|f| values.get(*f)).sum())
        .collect();
    for n in 0..t.years {
        let ss = values.eval(&v.social_security(t, n));
        let other = values.get(v.traditional_ira_withdrawal[n])
            + values.get(v.roth_conversion[n])
            + t.other_income[n]
            - pre_tax_deferrals(values, n)
            + values.eval(&v.capital_gains(s, t, p, n));
        let taxable = taxable_social_security(other, ss, status);
        ss_taxable_fraction.push(if ss > 0.0 {
            taxable / ss
        } else {
            SS_MAX_TAXABLE_FRACTION
        });
        magi.push(other + taxable);
        let (slcsp, fpl) = (t.aca_benchmark[n], t.fpl[n]);
        let aca_magi = other + ss;
        aca_cost.push(
            if s.options.aca_exact_cliff && aca_magi >= ACA_CLIFF * fpl {
                // The binary prices the cliff; the loop carries the subsidized cost.
                premium_below_cliff(slcsp, fpl)
            } else {
                net_premium(aca_magi, slcsp, fpl)
            },
        );
    }

    // Average-cost basis tracking for the taxable account. New money (surplus
    // deposits and contributions) and reinvested dividends add basis at cost;
    // Owl leaves dividends out, which overstates later gains.
    let mut basis = s.accounts.taxable_cost_basis;
    let mut gain_fraction = Vec::with_capacity(t.years);
    for n in 0..t.years {
        let balance = values.get(v.b_taxable[n]);
        let withdrawn = values.get(v.taxable_brokerage_liquidation[n]);
        let deposited = values.get(v.deposit[n]) + t.contributions[n][0];
        let fraction = if balance > 1e-6 {
            (1.0 - basis / balance).clamp(0.0, 1.0)
        } else {
            gain_fraction.last().copied().unwrap_or(p.gain_fraction[n])
        };
        gain_fraction.push(fraction);
        let sold_basis = if balance > 1e-6 {
            basis * (withdrawn / balance).min(1.0)
        } else {
            0.0
        };
        // A harvest sells and rebuys: the realized gain becomes basis.
        if let Some(h) = v.gain_harvest[n] {
            basis += fraction * values.get(h);
        }
        let dividends = s.rates.dividend_yield
            * (balance - withdrawn + values.get(v.deposit[n]) + 0.5 * t.contributions[n][0])
                .max(0.0);
        basis = (basis - sold_basis + deposited + dividends).max(0.0);
    }

    let proximal_weight = (0..t.years)
        .map(|n| {
            if values.eval(&v.capital_gains(s, t, p, n)) > 1.0 {
                s.options.loop_stabilizer
            } else {
                0.0
            }
        })
        .collect();

    LoopParams {
        ss_taxable_fraction,
        proximal_weight,
        medicare_cost: medicare_from_magi(s, t, &magi),
        magi_estimate: magi,
        aca_cost,
        gain_fraction,
        ordinary_income_estimate,
        exact_ss_taxability: false,
        exact_ltcg: false,
    }
}

/// Per-year disagreement, in today's dollars, between the costs the MILP
/// modelled and the statutory costs of the plan it produced.
fn loop_residual(
    s: &Scenario,
    t: &Timeline,
    used: &LoopParams,
    implied: &LoopParams,
    values: &Values,
    objective: f64,
) -> LoopResidual {
    let v = &values.vars;
    let (mut tax, mut medicare) = (0.0, 0.0);
    for n in 0..t.years {
        let today = 1.0 / t.gamma[n];
        let [_, q1, q2] = v.ltcg_band[n];
        let modelled: f64 = v.bracket_fill[n]
            .iter()
            .zip(ORDINARY_RATES)
            .map(|(f, rate)| rate * values.get(*f))
            .sum::<f64>()
            + LTCG_RATES[1] * values.get(q1)
            + LTCG_RATES[2] * values.get(q2);
        tax += today * (statutory_tax(s, t, implied, values, n) - modelled).abs();
        medicare += today * (used.medicare_cost[n] - implied.medicare_cost[n]).abs();
        // ACA premiums the solve assumed vs those its own MAGI implies; over the
        // cliff the binary charges the full premium exactly.
        let over_cliff = v.aca_over_cliff[n].is_some_and(|z| values.get(z) > 0.5);
        if t.aca_benchmark[n] > 0.0 && !over_cliff {
            medicare += today * (used.aca_cost[n] - implied.aca_cost[n]).abs();
        }
    }
    let years = t.years as f64;
    LoopResidual {
        objective,
        tax: tax / years,
        medicare: medicare / years,
    }
}

/// Federal income tax of year `n` computed from the solution's realized income
/// with the statutory rules and the parameters the solution implies.
fn statutory_tax(
    s: &Scenario,
    t: &Timeline,
    implied: &LoopParams,
    values: &Values,
    n: usize,
) -> f64 {
    let v = &values.vars;
    let status = s.filing_status();
    let ss = values.eval(&v.social_security(t, n));
    let gains = values.eval(&v.capital_gains(s, t, implied, n)).max(0.0);
    let income = values.get(v.traditional_ira_withdrawal[n])
        + values.get(v.roth_conversion[n])
        + t.other_income[n]
        - pre_tax_deferrals(values, n)
        + implied.ss_taxable_fraction[n] * ss;
    let deduction = t.standard_deduction(s, n, implied.magi_estimate[n]);
    let taxable = (income - deduction).max(0.0);
    ordinary_income_tax(taxable, status, t.gamma[n]) + ltcg_tax(taxable, gains, status, t.gamma[n])
}

/// A value of year `n`'s chosen contributions, or 0 when they were given.
fn chosen(values: &Values, n: usize, pick: impl Fn(&crate::model::ContributionVars) -> f64) -> f64 {
    values.vars.contributions[n].as_ref().map_or(0.0, pick)
}

/// Pre-tax 401(k) deferrals and payroll HSA contributions chosen in year `n`,
/// which come out of taxable pay.
fn pre_tax_deferrals(values: &Values, n: usize) -> f64 {
    chosen(values, n, |c| values.get(c.pre_tax) + values.get(c.hsa))
}

fn extract(
    s: &Scenario,
    t: &Timeline,
    p: &LoopParams,
    implied: &LoopParams,
    values: &Values,
    status: SolveStatus,
) -> Schedule {
    let v = &values.vars;
    let n_years = t.years;
    let mut years = Vec::with_capacity(n_years);
    for n in 0..n_years {
        let ss = values.eval(&v.social_security(t, n));
        let ordinary: f64 = v.bracket_fill[n].iter().map(|f| values.get(*f)).sum();
        let ordinary_tax: f64 = v.bracket_fill[n]
            .iter()
            .zip(ORDINARY_RATES)
            .map(|(f, rate)| rate * values.get(*f))
            .sum();
        let [_, q1, q2] = v.ltcg_band[n];
        let withdrawal = values.get(v.traditional_ira_withdrawal[n]);
        let state_tax: f64 = match &s.options.state_tax {
            Some(state) => v.state_fill[n]
                .iter()
                .zip(&state.brackets)
                .map(|(f, (_, rate))| rate * values.get(*f))
                .sum(),
            None => 0.0,
        };
        let spending = values.get(v.spending[n]);
        // A given working-year amount (nominal); optimized years choose
        // theirs instead (see `Timeline::contributions`).
        let given = |amount: fn(&WorkingYear) -> f64| {
            s.working_years
                .get(n)
                .filter(|w| s.options.optimize_contributions.is_none() || w.wages <= 0.0)
                .map_or(0.0, |w| amount(w) * t.gamma[n])
        };
        years.push(YearPlan {
            year: t.calendar_year[n],
            age: t.age[n],
            taxable_balance: values.get(v.b_taxable[n]),
            tax_deferred_balance: values.get(v.b_deferred[n]),
            roth_balance: values.get(v.b_roth[n]),
            hsa_balance: values.get(v.b_hsa[n]),
            traditional_ira_withdrawal: withdrawal,
            taxable_brokerage_liquidation: values.get(v.taxable_brokerage_liquidation[n]),
            roth_withdrawal: values.get(v.roth_withdrawal[n]),
            roth_conversion: values.get(v.roth_conversion[n]),
            taxable_deposit: values.get(v.deposit[n]),
            leftover_spent: v.leftover_spent[n].map_or(0.0, |x| values.get(x)),
            social_security_active: values.get(v.social_security_active[n]) > 0.5,
            social_security_benefit: ss,
            other_income: t.other_income[n] - t.wages[n],
            cash_flow: t.cash_flows[n],
            wages: t.wages[n],
            taxable_contribution: t.contributions[n][0],
            tax_deferred_contribution: t.contributions[n][1]
                + chosen(values, n, |c| {
                    values.get(c.pre_tax) + values.get(c.employer)
                }),
            roth_contribution: t.contributions[n][2]
                + chosen(values, n, |c| {
                    values.get(c.roth_401k) + values.get(c.roth_ira)
                }),
            hsa_contribution: t.hsa_contributions[n] + chosen(values, n, |c| values.get(c.hsa)),
            employer_match: given(|w| w.employer_contribution)
                + chosen(values, n, |c| values.get(c.employer)),
            pre_tax_deferral: given(|w| w.tax_deferred_contribution)
                + chosen(values, n, |c| values.get(c.pre_tax)),
            roth_401k: chosen(values, n, |c| values.get(c.roth_401k)),
            roth_ira: chosen(values, n, |c| values.get(c.roth_ira)),
            debt_payment: v.debt_payment.iter().map(|p| values.get(p[n])).sum(),
            debt_balance: v.debt_balance.iter().map(|b| values.get(b[n])).sum(),
            hsa_withdrawal: values.get(v.hsa_withdrawal[n]),
            gain_harvest: v.gain_harvest[n].map_or(0.0, |h| values.get(h)),
            state_income_tax: state_tax,
            aca_premium: values.eval(&v.aca_premium(t, p, n)),
            required_minimum_distribution: t.rmd_fraction[n] * values.get(v.b_deferred[n]),
            net_spending: spending,
            net_spending_today: spending / t.gamma[n],
            taxable_social_security: match v.ss_taxable[n] {
                Some(taxable) => values.get(taxable),
                None => p.ss_taxable_fraction[n] * ss,
            },
            standard_deduction: t.standard_deduction(s, n, p.magi_estimate[n]),
            ordinary_taxable_income: ordinary,
            capital_gains: values.eval(&v.capital_gains(s, t, p, n)),
            magi: values.eval(&v.magi(s, t, p, n)),
            ordinary_income_tax: ordinary_tax,
            capital_gains_tax: LTCG_RATES[1] * values.get(q1) + LTCG_RATES[2] * values.get(q2),
            statutory_federal_tax: statutory_tax(s, t, implied, values, n),
            early_withdrawal_penalty: t.early_penalty[n]
                * values.eval(&v.penalized_withdrawal(t, n)).max(0.0),
            medicare_premium: values.get(v.medicare[n]),
            irmaa_tier: v.irmaa_tier[n]
                .map(|tiers| tiers.iter().position(|z| values.get(*z) > 0.5).unwrap_or(0)),
        });
    }

    let last = n_years;
    let final_taxable = values.get(v.b_taxable[last]);
    let final_deferred = values.get(v.b_deferred[last]);
    let final_roth = values.get(v.b_roth[last]);
    let bequest = values.eval(&v.bequest(s));
    let claim_age = s.social_security.as_ref().and_then(|_| {
        v.claim_month
            .iter()
            .position(|z| values.get(*z) > 0.5)
            .map(claim_age_for_month)
    });

    Schedule {
        status,
        iterations: 0,
        convergence: Convergence::MaxIterations,
        loop_residuals: Vec::new(),
        claim_age,
        lifetime_spending_today: years.iter().map(|y| y.net_spending_today).sum(),
        lifetime_federal_tax: years
            .iter()
            .map(|y| y.ordinary_income_tax + y.capital_gains_tax)
            .sum(),
        lifetime_state_tax: years.iter().map(|y| y.state_income_tax).sum(),
        lifetime_medicare: years.iter().map(|y| y.medicare_premium).sum(),
        lifetime_aca: years.iter().map(|y| y.aca_premium).sum(),
        final_hsa_balance: values.get(v.b_hsa[last]),
        years,
        final_taxable_balance: final_taxable,
        final_tax_deferred_balance: final_deferred,
        final_roth_balance: final_roth,
        bequest_after_tax: bequest,
        bequest_after_tax_today: bequest / t.gamma[last],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::tests::standard_retiree;

    fn residual(objective: f64) -> LoopResidual {
        LoopResidual {
            objective,
            tax: 0.0,
            medicare: 0.0,
        }
    }

    #[test]
    fn detects_two_cycles_but_not_steady_progress() {
        let cycling: Vec<_> = [1.0, 5.0, 9.0, 5.0, 9.0]
            .iter()
            .map(|o| residual(*o))
            .collect();
        assert_eq!(detect_cycle(&cycling, 0.5), Some(2));
        let improving: Vec<_> = [1.0, 2.0, 3.0, 4.0, 5.0]
            .iter()
            .map(|o| residual(*o))
            .collect();
        assert_eq!(detect_cycle(&improving, 0.5), None);
        assert_eq!(detect_cycle(&cycling[..3], 0.5), None);
    }

    #[test]
    fn damping_blends_only_the_oscillating_estimates() {
        let s = standard_retiree();
        let t = Timeline::new(&s);
        let old = initial_params(&s, &t);
        let mut new = old.clone();
        new.ss_taxable_fraction = vec![0.0; t.years];
        new.ordinary_income_estimate = vec![1_000.0; t.years];
        new.gain_fraction = vec![0.5; t.years];
        let next = damped(&old, new);
        assert!((next.ss_taxable_fraction[0] - 0.7 * SS_MAX_TAXABLE_FRACTION).abs() < 1e-12);
        assert!((next.ordinary_income_estimate[0] - 500.0).abs() < 1e-12);
        assert_eq!(next.gain_fraction[0], 0.5);
    }

    #[test]
    fn initial_parameters_start_conservative() {
        let s = standard_retiree();
        let t = Timeline::new(&s);
        let p = initial_params(&s, &t);
        assert!(p
            .ss_taxable_fraction
            .iter()
            .all(|f| *f == SS_MAX_TAXABLE_FRACTION));
        // $150k unrealized gain on $500k.
        assert!((p.gain_fraction[0] - 0.3).abs() < 1e-12);
        assert!(p.magi_estimate.iter().all(|m| m.is_infinite()));
        assert!(p.proximal_weight.iter().all(|w| *w == 0.0));
        // Loop-mode Medicare starts at 65 with the standard premium.
        assert_eq!(p.medicare_cost[2], 0.0);
        assert!(p.medicare_cost[3] > 0.0);
    }
}
