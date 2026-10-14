//! Optimizer output: a year-by-year lifetime schedule (nominal dollars unless noted).

use serde::{Deserialize, Serialize};

/// How the final solve terminated.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SolveStatus {
    /// Proven optimal (zero MIP gap).
    Optimal,
    /// Stopped at the configured relative MIP gap.
    OptimalWithinGap,
    /// Best plan found before the time limit.
    TimeLimit,
}

/// How Owl's self-consistent loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Convergence {
    /// The objective settled and the plan agrees with the tax terms it was built on.
    Converged,
    /// The loop settled on a repeating cycle; the best plan in the cycle is returned.
    Cycle { length: usize },
    /// The iteration limit was reached; the best plan found is returned.
    MaxIterations,
}

/// Per-iteration loop diagnostics: per-year averages, in today's dollars, of how
/// far the MILP's modelled costs are from the statutory amounts the resulting
/// plan implies (Owl's residual).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LoopResidual {
    pub objective: f64,
    /// |statutory federal tax - modelled federal tax|.
    pub tax: f64,
    /// |statutory Medicare premium - modelled premium| (loop mode only).
    pub medicare: f64,
}

impl LoopResidual {
    /// The larger of the two disagreements.
    pub fn max(&self) -> f64 {
        self.tax.max(self.medicare)
    }
}

/// One plan year.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YearPlan {
    pub year: i32,
    pub age: u32,
    /// Start-of-year balances.
    pub taxable_balance: f64,
    pub tax_deferred_balance: f64,
    pub roth_balance: f64,
    pub hsa_balance: f64,
    pub traditional_ira_withdrawal: f64,
    pub taxable_brokerage_liquidation: f64,
    pub roth_withdrawal: f64,
    pub roth_conversion: f64,
    /// Surplus cash reinvested in the taxable account.
    pub taxable_deposit: f64,
    /// Leftover pay spent instead of invested (`leftover_invested`).
    #[serde(default)]
    pub leftover_spent: f64,
    pub social_security_active: bool,
    pub social_security_benefit: f64,
    pub other_income: f64,
    /// Untaxed money in (+) or out (-): a down payment, a home sale.
    #[serde(default)]
    pub cash_flow: f64,
    /// Taxable wages (after pre-tax deferrals).
    pub wages: f64,
    /// Contributions arriving during the year: taxable, tax-deferred (employee
    /// and employer) and Roth.
    pub taxable_contribution: f64,
    pub tax_deferred_contribution: f64,
    pub roth_contribution: f64,
    pub hsa_contribution: f64,
    /// Tax-free HSA withdrawals for qualified medical costs.
    pub hsa_withdrawal: f64,
    /// Gains realized and bought back to raise the taxable account's basis.
    pub gain_harvest: f64,
    /// Employer contributions, given or the chosen match (included in
    /// `tax_deferred_contribution`).
    pub employer_match: f64,
    /// Employee pre-tax 401(k) deferrals, given or chosen (included in
    /// `tax_deferred_contribution`).
    #[serde(default)]
    pub pre_tax_deferral: f64,
    /// Chosen Roth 401(k) deferrals and Roth IRA contributions (included in
    /// `roth_contribution`); a given Roth contribution is in neither.
    #[serde(default)]
    pub roth_401k: f64,
    #[serde(default)]
    pub roth_ira: f64,
    /// Debt payments (scheduled plus extra) and the balance still owed.
    pub debt_payment: f64,
    pub debt_balance: f64,
    pub state_income_tax: f64,
    /// Net ACA marketplace premium after the premium tax credit.
    pub aca_premium: f64,
    /// Required minimum distribution for the year (0 before RMD age).
    pub required_minimum_distribution: f64,
    pub net_spending: f64,
    /// Net spending in first-year dollars.
    pub net_spending_today: f64,
    pub taxable_social_security: f64,
    pub standard_deduction: f64,
    pub ordinary_taxable_income: f64,
    pub capital_gains: f64,
    pub magi: f64,
    pub ordinary_income_tax: f64,
    pub capital_gains_tax: f64,
    /// Federal income tax recomputed from the realized income with the statutory
    /// rules; differs from the modelled tax only by the loop residual.
    pub statutory_federal_tax: f64,
    pub early_withdrawal_penalty: f64,
    pub medicare_premium: f64,
    /// IRMAA tier (0 = standard premium), when enrolled in Medicare.
    pub irmaa_tier: Option<usize>,
}

/// The optimized lifetime plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub status: SolveStatus,
    /// Self-consistent iterations performed and whether they converged.
    pub iterations: usize,
    pub convergence: Convergence,
    /// Per-iteration change in the loop parameters, for convergence diagnostics.
    pub loop_residuals: Vec<LoopResidual>,
    /// Social Security claiming age used (None without benefits).
    pub claim_age: Option<f64>,
    pub years: Vec<YearPlan>,
    pub final_taxable_balance: f64,
    pub final_tax_deferred_balance: f64,
    pub final_roth_balance: f64,
    /// Estate after heirs' tax on the tax-deferred balance, nominal and in today's dollars.
    pub bequest_after_tax: f64,
    pub bequest_after_tax_today: f64,
    /// Sum of yearly net spending in today's dollars.
    pub lifetime_spending_today: f64,
    pub lifetime_federal_tax: f64,
    pub lifetime_state_tax: f64,
    pub lifetime_medicare: f64,
    pub lifetime_aca: f64,
    pub final_hsa_balance: f64,
}

impl Schedule {
    /// True when the loop reached a fixed point.
    pub fn converged(&self) -> bool {
        self.convergence == Convergence::Converged
    }

    /// First-year net spending in today's dollars (the "sustainable spending" figure).
    pub fn first_year_spending(&self) -> f64 {
        self.years.first().map_or(0.0, |y| y.net_spending_today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn residual(tax: f64, medicare: f64) -> LoopResidual {
        LoopResidual {
            objective: 0.0,
            tax,
            medicare,
        }
    }

    #[test]
    fn residual_max_takes_the_larger_disagreement() {
        assert_eq!(residual(10.0, 3.0).max(), 10.0);
        assert_eq!(residual(1.0, 30.0).max(), 30.0);
    }

    #[test]
    fn empty_schedule_reports_zero_spending_and_convergence_state() {
        let schedule = Schedule {
            status: SolveStatus::Optimal,
            iterations: 3,
            convergence: Convergence::Cycle { length: 2 },
            loop_residuals: vec![],
            claim_age: None,
            years: vec![],
            final_taxable_balance: 0.0,
            final_tax_deferred_balance: 0.0,
            final_roth_balance: 0.0,
            bequest_after_tax: 0.0,
            bequest_after_tax_today: 0.0,
            lifetime_spending_today: 0.0,
            lifetime_federal_tax: 0.0,
            lifetime_state_tax: 0.0,
            lifetime_medicare: 0.0,
            lifetime_aca: 0.0,
            final_hsa_balance: 0.0,
        };
        assert_eq!(schedule.first_year_spending(), 0.0);
        assert!(!schedule.converged());
    }
}
