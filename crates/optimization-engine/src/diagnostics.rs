//! Infeasibility isolation. When the strict model is infeasible, the same rows
//! are re-solved with non-negative slack on every constraint while minimizing
//! total slack (an elastic filter). The planner runs it on the LP relaxation
//! (binaries in [0, 1]) so it finishes in milliseconds; a conflict that exists
//! only because of integrality would surface as a solver-level infeasibility
//! without relaxed rows. The relaxation (and the first iteration's tax
//! estimates) are optimistic, so reported amounts are lower bounds on the gap. Rows that still need slack are the
//! conflicting ones; the report lists them by year together with that year's
//! binding constraints, so a contradictory spending floor or bequest target can
//! be traced quickly.

use std::collections::BTreeMap;
use std::fmt;

use good_lp::{constraint, variable, Expression, Solution, SolverModel};

use crate::error::{EngineError, Result};
use crate::model::{Family, Model, Sense};

/// Slack below this many dollars is treated as zero.
const SLACK_TOLERANCE: f64 = 1e-4;
/// A row is reported as binding when its two sides are this close.
const BINDING_TOLERANCE: f64 = 1e-3;

/// A constraint that had to be relaxed to make the plan feasible.
#[derive(Debug, Clone)]
pub struct RelaxedConstraint {
    pub family: Family,
    pub year: Option<usize>,
    /// How far the constraint had to move, in nominal dollars.
    pub slack: f64,
    /// The same amount in first-year (today's) dollars.
    pub slack_today: f64,
    /// `true` when the left-hand side had to exceed the bound (shortfall otherwise).
    pub over: bool,
}

/// Elastic-filter result for an infeasible plan.
#[derive(Debug, Clone)]
pub struct InfeasibilityReport {
    pub relaxed: Vec<RelaxedConstraint>,
    /// For each year (`None` = plan-wide rows) with a relaxed row, the families of
    /// all binding constraints in that year.
    pub binding_by_year: BTreeMap<Option<usize>, Vec<Family>>,
    /// First calendar year of the plan, for readable year labels.
    pub start_year: i32,
}

impl fmt::Display for InfeasibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = |year: Option<usize>| match year {
            Some(n) => format!("{} (year {n})", self.start_year + n as i32),
            None => "whole plan".to_string(),
        };
        writeln!(f, "Constraints relaxed to reach feasibility:")?;
        for r in &self.relaxed {
            let what = match (r.family, r.over) {
                // Cash-flow rows are outflows minus inflows.
                (Family::CashFlow, true) => "cash shortfall of at least".to_string(),
                (Family::CashFlow, false) => "unusable cash surplus of at least".to_string(),
                (family, true) => format!("{family} exceeded by at least"),
                (family, false) => format!("{family} short by at least"),
            };
            let what = format!(
                "{what} ${:.0} (${:.0} in today's dollars)",
                r.slack, r.slack_today
            );
            writeln!(f, "  - {}: {what}", label(r.year))?;
        }
        for (year, families) in &self.binding_by_year {
            let names: Vec<String> = families.iter().map(ToString::to_string).collect();
            writeln!(f, "  Binding in {}: {}", label(*year), names.join("; "))?;
        }
        Ok(())
    }
}

/// Re-solves `model` elastically and reports the conflicting constraints.
///
/// `gamma` is the cumulative inflation factor per year (`years + 1` entries); rows
/// without a year (the bequest floor) are deflated with the final factor.
pub(crate) fn diagnose(
    model: Model,
    start_year: i32,
    gamma: &[f64],
) -> Result<InfeasibilityReport> {
    let Model {
        mut problem, rows, ..
    } = model;

    // Slack only on goal and money-flow rows (see `Family::is_elastic`).
    let mut slacks = Vec::with_capacity(rows.len());
    let mut total = Expression::with_capacity(rows.len());
    for row in &rows {
        slacks.push(row.family.elastic_weight().map(|weight| {
            let over = problem.add(variable().min(0.0));
            let under = problem.add(variable().min(0.0));
            total.add_mul(weight, over);
            total.add_mul(weight, under);
            (over, under)
        }));
    }

    let mut solver = problem.minimise(total).using(good_lp::highs);
    solver.set_verbose(false);
    for (row, slack) in rows.iter().zip(&slacks) {
        // lhs - over + under (sense) rhs
        let mut expr = row.expr.clone();
        if let Some((over, under)) = slack {
            expr.add_mul(-1.0, *over);
            expr.add_mul(1.0, *under);
        }
        solver.add_constraint(match row.sense {
            Sense::Eq => constraint::eq(expr, row.rhs),
            Sense::Le => constraint::leq(expr, row.rhs),
            Sense::Ge => constraint::geq(expr, row.rhs),
        });
    }
    let solution = solver
        .solve()
        .map_err(|e| EngineError::Solver(format!("elastic diagnosis failed: {e}")))?;

    let mut relaxed = Vec::new();
    for (row, slack) in rows.iter().zip(&slacks) {
        let Some((over, under)) = slack else {
            continue;
        };
        let (o, u) = (solution.value(*over), solution.value(*under));
        if o > SLACK_TOLERANCE || u > SLACK_TOLERANCE {
            let slack = o.max(u);
            let deflator = row
                .year
                .and_then(|n| gamma.get(n))
                .or(gamma.last())
                .copied()
                .unwrap_or(1.0);
            relaxed.push(RelaxedConstraint {
                family: row.family,
                year: row.year,
                slack,
                slack_today: slack / deflator,
                over: o > u,
            });
        }
    }
    relaxed.sort_by(|a, b| a.year.cmp(&b.year).then(a.family.cmp(&b.family)));

    let mut binding_by_year: BTreeMap<Option<usize>, Vec<Family>> = BTreeMap::new();
    for r in &relaxed {
        binding_by_year.entry(r.year).or_default();
    }
    for row in &rows {
        if let Some(list) = binding_by_year.get_mut(&row.year) {
            let lhs = row.expr.eval_with(&solution);
            if (lhs - row.rhs).abs() <= BINDING_TOLERANCE && !list.contains(&row.family) {
                list.push(row.family);
            }
        }
    }

    Ok(InfeasibilityReport {
        relaxed,
        binding_by_year,
        start_year,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goals_are_relaxed_before_cash_flow() {
        let goal = Family::BequestFloor.elastic_weight().expect("elastic");
        let cash = Family::CashFlow.elastic_weight().expect("elastic");
        assert!(goal < cash);
        // Facts and tax definitions are never relaxed.
        for family in [
            Family::InitialBalance,
            Family::TaxableCarryover,
            Family::TaxableIncome,
            Family::IrmaaOneTier,
            Family::SocialSecurityOneClaimMonth,
        ] {
            assert!(!family.is_elastic(), "{family:?}");
        }
    }

    #[test]
    fn report_lists_relaxed_rows_with_years_and_both_dollar_bases() {
        let mut binding_by_year = BTreeMap::new();
        binding_by_year.insert(Some(2), vec![Family::CashFlow, Family::TaxableIncome]);
        let report = InfeasibilityReport {
            relaxed: vec![
                RelaxedConstraint {
                    family: Family::CashFlow,
                    year: Some(2),
                    slack: 1_050.0,
                    slack_today: 1_000.0,
                    over: true,
                },
                RelaxedConstraint {
                    family: Family::BequestFloor,
                    year: None,
                    slack: 20.0,
                    slack_today: 10.0,
                    over: false,
                },
            ],
            binding_by_year,
            start_year: 2026,
        };
        let text = report.to_string();
        assert!(
            text.contains(
                "2028 (year 2): cash shortfall of at least $1050 ($1000 in today's dollars)"
            ),
            "{text}"
        );
        assert!(
            text.contains("whole plan: bequest floor (maxSpending) short by at least $20"),
            "{text}"
        );
        assert!(
            text.contains("Binding in 2028 (year 2): cash flow"),
            "{text}"
        );
    }
}
