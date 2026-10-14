//! Planner inputs. Money is in today's (first plan year) dollars unless a field
//! says otherwise; the engine inflates thresholds and spending each year by
//! `rates.inflation`, mirroring Owl's `gamma_n` series.

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, Result};
use crate::mortality::Sex;
use crate::social_security::{MAX_CLAIM_AGE, MIN_CLAIM_AGE};
use crate::tax::state::StateTax;
use crate::tax::FilingStatus;

/// The account owner. The engine currently models a single filer; Owl's
/// two-spouse model (survivor benefits, filing-status switch at first death,
/// per-spouse accounts) is deferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub birth_year: i32,
    /// 1–12. Used for Social Security timing and FRA.
    pub birth_month: u32,
    pub sex: Sex,
    /// Age in the final plan year (inclusive). See
    /// [`crate::mortality::age_at_survival_probability`] for a longevity-based choice.
    pub planning_age: u32,
}

/// Starting balances in today's dollars.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Accounts {
    /// Taxable brokerage balance.
    pub taxable: f64,
    /// Cost basis of the taxable balance (≤ `taxable`).
    pub taxable_cost_basis: f64,
    /// Traditional IRA / 401(k) balance.
    pub tax_deferred: f64,
    /// Roth IRA balance.
    pub roth: f64,
    /// Health savings account balance.
    #[serde(default)]
    pub hsa: f64,
}

/// Annual nominal rates as decimals (0.05 = 5%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rates {
    pub inflation: f64,
    pub taxable_return: f64,
    pub tax_deferred_return: f64,
    pub roth_return: f64,
    /// Qualified-dividend yield of the taxable account (part of `taxable_return`),
    /// taxed each year at LTCG rates.
    pub dividend_yield: f64,
    /// Per-year return applied to every account, overriding the three constant
    /// returns (a glide path: e.g. higher before retirement than after). Years
    /// past the end reuse the last value. Empty uses the constant returns.
    #[serde(default)]
    pub annual_returns: Vec<f64>,
    /// Per-year inflation (a historical or simulated path), overriding
    /// `inflation`; years past the end reuse the last value.
    #[serde(default)]
    pub annual_inflation: Vec<f64>,
    /// Per-year returns of each account, `[taxable, tax-deferred, Roth, HSA]`
    /// (each with its own asset mix), overriding `annual_returns`. Years past
    /// the end reuse the last value.
    #[serde(default)]
    pub account_returns: Vec<[f64; 4]>,
}

impl Rates {
    /// Returns of the taxable, tax-deferred, Roth and HSA accounts in plan
    /// year `n`. Without per-account returns the HSA grows at the tax-deferred
    /// return.
    pub fn for_year(&self, n: usize) -> [f64; 4] {
        if let Some(r) = self.account_returns.get(n).or(self.account_returns.last()) {
            return *r;
        }
        match self.annual_returns.get(n).or(self.annual_returns.last()) {
            Some(r) => [*r; 4],
            None => [
                self.taxable_return,
                self.tax_deferred_return,
                self.roth_return,
                self.tax_deferred_return,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ClaimStrategy {
    /// Claim at this age (62–70, monthly precision).
    AtAge(f64),
    /// Let the optimizer choose the claiming month (Owl `withSSAges="optimize"`).
    Optimize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSecurity {
    /// Monthly primary insurance amount at full retirement age, today's dollars.
    pub monthly_pia: f64,
    pub claim: ClaimStrategy,
}

/// The two objectives Owl supports.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Objective {
    /// Maximize total inflation-adjusted spending while leaving at least
    /// `bequest_floor` (today's dollars, after heirs' tax) at the end.
    MaxSpending { bequest_floor: f64 },
    /// Spend `net_spending` per year (today's dollars, following the spending
    /// profile) and maximize the after-tax estate left to heirs.
    MaxBequest { net_spending: f64 },
}

/// Order in which accounts may be drawn down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WithdrawalOrder {
    /// The optimizer chooses freely.
    #[default]
    Optimal,
    /// Taxable first, then tax-deferred (beyond RMDs), then Roth: the
    /// conventional rule of thumb (Owl `withdrawalOrder="taxable_first"`).
    TaxableFirst,
}

/// Model options (Owl `solve()` options).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOptions {
    /// Model IRMAA cliffs exactly with binaries (Owl `withMedicare="optimize"`).
    /// When false, premiums are fixed from the previous iteration's MAGI.
    pub optimize_medicare: bool,
    /// Select LTCG bands exactly with big-M binaries (Owl `withLTCG="optimize"`).
    /// When false (Owl's default), ordinary income from the previous
    /// self-consistent iteration sets the 0%/15% band room and no binaries are used.
    pub optimize_ltcg: bool,
    /// After the self-consistent loop, polish the plan with a few solves that
    /// compute taxable Social Security and the LTCG bands exactly (Owl's
    /// `withSSTaxability` / `withLTCG="optimize"`, used only at the end because
    /// they are slow). The loop fixes those terms from the previous iterate and
    /// can settle on a worse fixed point. Applies when the claiming age is fixed;
    /// the polished plan is kept only when it is better and self-consistent.
    pub polish: bool,
    /// Time allowed for each polish solve, in seconds; 0 skips polishing.
    pub polish_time_secs: f64,
    /// Include Part D IRMAA surcharges with Part B.
    pub include_part_d: bool,
    /// MAGI for the two years before the plan (IRMAA two-year lookback).
    pub prior_magi: [f64; 2],
    /// Per-year cap on Roth conversions in today's dollars; `None` = unlimited.
    pub max_roth_conversion: Option<f64>,
    /// State income tax; `None` for a state without one (or not modelled).
    pub state_tax: Option<StateTax>,
    /// Yearly benchmark (second-lowest silver) ACA premium in today's dollars,
    /// paid in retired years before Medicare; 0 when not buying marketplace cover.
    pub aca_benchmark_premium: f64,
    /// Model the 400%-of-poverty cliff exactly with one binary per ACA year;
    /// otherwise the premium follows the previous iteration (Owl's loop mode).
    pub aca_exact_cliff: bool,
    /// Medical costs other than Medicare (today's dollars per year) that HSA
    /// withdrawals may pay for, besides Medicare premiums.
    pub other_medical_expenses: f64,
    /// Rule of 55: penalty-free amount (today's dollars) available from the
    /// employer plan of a job left in or after the year turning 55.
    pub rule_of_55: Option<f64>,
    /// 72(t) payments, if any.
    pub sepp: Option<Sepp>,
    /// Loans paid on a schedule, with optional extra principal.
    pub debts: Vec<Debt>,
    /// Optimize working-year contributions instead of taking them as given.
    pub optimize_contributions: Option<ContributionChoice>,
    /// Share (0–1) of each working year's leftover pay deposited in the
    /// taxable account; the rest is spent. `None` deposits all of it.
    #[serde(default)]
    pub leftover_invested: Option<f64>,
    /// Let the optimizer realize gains in the taxable account and buy back
    /// (raising cost basis), typically to fill the 0% LTCG band. Owl does not.
    pub allow_gain_harvesting: bool,
    /// Whether Roth conversions are allowed at all (a baseline turns them off).
    pub allow_roth_conversions: bool,
    /// Fix the first year's Roth conversion (today's dollars) instead of
    /// choosing it, e.g. to see how one decision fares across markets.
    #[serde(default)]
    pub first_year_conversion: Option<f64>,
    pub withdrawal_order: WithdrawalOrder,
    /// Tax rate heirs pay on inherited tax-deferred balances.
    pub heirs_tax_rate: f64,
    /// Allowed deviation of yearly spending from the profile (Owl `lambdha`).
    pub spending_flexibility: f64,
    /// Relative spending shape per year (1.0 = flat in real terms). Missing years use 1.0.
    pub spending_profile: Vec<f64>,
    /// Spending that must be met each year whatever the objective (must-have
    /// expenses), in today's dollars. Missing years have no floor.
    pub required_spending: Vec<f64>,
    /// Other taxable income per year in today's dollars (pension, part-time work).
    pub other_taxable_income: Vec<f64>,
    /// Untaxed money coming in (+) or going out (-) per year, in today's
    /// dollars: a home's down payment, or what a home sale brings in.
    #[serde(default)]
    pub cash_flows: Vec<f64>,
    /// Tie-breaking penalty (Owl `epsilon`) discouraging churn.
    pub epsilon: f64,
    /// Relative MIP gap at which HiGHS stops.
    pub mip_rel_gap: f32,
    /// Wall-clock limit per solve, in seconds.
    pub time_limit_secs: f64,
    /// Weight (dollars of objective per dollar) that discourages ordinary income
    /// from moving between self-consistent iterations in years with capital gains;
    /// stabilizes LTCG loop mode and vanishes at a fixed point. 0 disables it.
    pub loop_stabilizer: f64,
    /// Maximum self-consistent iterations (Owl's SC loop).
    pub max_iterations: usize,
    /// Print HiGHS progress logs (debugging).
    pub solver_verbose: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            optimize_medicare: true,
            optimize_ltcg: false,
            polish: true,
            polish_time_secs: 15.0,
            include_part_d: true,
            prior_magi: [0.0, 0.0],
            max_roth_conversion: None,
            state_tax: None,
            aca_benchmark_premium: 0.0,
            aca_exact_cliff: true,
            other_medical_expenses: 0.0,
            rule_of_55: None,
            sepp: None,
            allow_gain_harvesting: false,
            debts: Vec::new(),
            optimize_contributions: None,
            leftover_invested: None,
            allow_roth_conversions: true,
            first_year_conversion: None,
            withdrawal_order: WithdrawalOrder::Optimal,
            heirs_tax_rate: 0.30,
            spending_flexibility: 0.0,
            spending_profile: Vec::new(),
            required_spending: Vec::new(),
            other_taxable_income: Vec::new(),
            cash_flows: Vec::new(),
            epsilon: 5e-7,
            mip_rel_gap: 1e-4,
            time_limit_secs: 120.0,
            loop_stabilizer: 0.05,
            max_iterations: 29,
            solver_verbose: false,
        }
    }
}

/// Earned income and contributions for one plan year, in today's dollars
/// (Owl's Wages and Contributions table).
///
/// Unlike Owl, whose *anticipated wages* are net of every contribution column,
/// `wages` here is taxable pay (W-2 box 1): pre-tax deferrals are already out
/// of it, but Roth and taxable contributions are paid from it. The two agree
/// whenever only pre-tax and employer contributions are made; with Roth
/// contributions this keeps them in taxable income, as the tax code does.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkingYear {
    /// Taxable wages after pre-tax payroll deferrals.
    pub wages: f64,
    /// Employee pre-tax deferrals to the tax-deferred account (traditional
    /// 401(k), deductible IRA). Already excluded from `wages`.
    pub tax_deferred_contribution: f64,
    /// Employer contributions to the tax-deferred account (match, profit
    /// sharing). Never part of wages.
    pub employer_contribution: f64,
    /// After-tax contributions to the Roth account, paid out of wages.
    pub roth_contribution: f64,
    /// New money invested in the taxable account, paid out of wages.
    pub taxable_contribution: f64,
    /// HSA contributions (employee payroll and employer); like Owl, `wages` are
    /// already net of the employee part. Ignored from age 65 (Medicare).
    #[serde(default)]
    pub hsa_contribution: f64,
}

/// A loan paid on a fixed schedule. Extra principal payments are a decision:
/// the balance must stay at or below the scheduled one (minimum payments) and
/// whatever is left at the end reduces the estate. Nominal dollars.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Debt {
    pub label: String,
    /// Balance when the loan starts (at the start of plan year `start`).
    pub balance: f64,
    /// Yearly interest rate (0.06 = 6%).
    pub annual_rate: f64,
    /// Scheduled yearly payment (principal and interest).
    pub annual_payment: f64,
    /// Plan year the loan is taken out (a future mortgage); 0 is owed today.
    #[serde(default)]
    pub start: usize,
    /// Plan year in which the rest is repaid in full (the home is sold).
    #[serde(default)]
    pub payoff: Option<usize>,
}

/// Let the optimizer choose each working year's savings: pre-tax and Roth
/// 401(k) deferrals, Roth IRA and HSA contributions, and the employer match
/// they earn. `WorkingYear::wages` is then gross pay (before deferrals), and
/// the year's fixed tax-deferred, employer, Roth and HSA amounts are ignored.
/// Limits are 2026 dollars, indexed by inflation (Owl `tax_federal.py`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContributionChoice {
    /// 401(k) elective deferral limit, shared by pre-tax and Roth deferrals.
    pub elective_limit: f64,
    /// Extra deferral from 50, and instead of it at 60–63.
    pub catch_up_50: f64,
    pub catch_up_60_to_63: f64,
    pub ira_limit: f64,
    pub ira_catch_up_50: f64,
    /// HSA limit (0 without a high-deductible plan) and the 55+ catch-up.
    pub hsa_limit: f64,
    pub hsa_catch_up_55: f64,
    /// Employer match: `match_rate` of deferrals, up to `match_cap` of pay.
    pub match_rate: f64,
    pub match_cap: f64,
    pub allow_roth_401k: bool,
    pub allow_roth_ira: bool,
}

impl ContributionChoice {
    /// 2026 limits with no match, Roth allowed, no HSA.
    pub fn limits_2026() -> Self {
        Self {
            elective_limit: 24_500.0,
            catch_up_50: 8_000.0,
            catch_up_60_to_63: 11_250.0,
            ira_limit: 7_500.0,
            ira_catch_up_50: 1_100.0,
            hsa_limit: 0.0,
            hsa_catch_up_55: 1_000.0,
            match_rate: 0.0,
            match_cap: 0.0,
            allow_roth_401k: true,
            allow_roth_ira: true,
        }
    }

    /// Elective deferral limit at `age` (2026 dollars).
    pub fn deferral_limit(&self, age: u32) -> f64 {
        self.elective_limit
            + match age {
                60..=63 => self.catch_up_60_to_63,
                50.. => self.catch_up_50,
                _ => 0.0,
            }
    }

    pub fn ira_limit_at(&self, age: u32) -> f64 {
        self.ira_limit + if age >= 50 { self.ira_catch_up_50 } else { 0.0 }
    }

    pub fn hsa_limit_at(&self, age: u32) -> f64 {
        if self.hsa_limit <= 0.0 {
            return 0.0;
        }
        self.hsa_limit + if age >= 55 { self.hsa_catch_up_55 } else { 0.0 }
    }
}

/// A 72(t) series of substantially equal periodic payments from the
/// tax-deferred account: penalty-free before 59½ but fixed once started.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sepp {
    /// Calendar year of the first payment.
    pub start_year: i32,
    /// Fixed yearly payment (nominal: 72(t) payments do not inflate).
    pub annual_payment: f64,
}

/// Roth activity in the five years before the plan, most recent year first,
/// in the dollars of the year it happened. Feeds the five-year maturation rule.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RothHistory {
    pub conversions: Vec<f64>,
    pub contributions: Vec<f64>,
}

/// Roth activity this many years back counts toward the maturation rule.
pub const ROTH_MATURATION_YEARS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// First plan year (calendar).
    pub start_year: i32,
    pub person: Person,
    pub accounts: Accounts,
    pub rates: Rates,
    pub social_security: Option<SocialSecurity>,
    /// Wages and contributions by plan year (index 0 = `start_year`); later
    /// years without an entry have none.
    #[serde(default)]
    pub working_years: Vec<WorkingYear>,
    #[serde(default)]
    pub roth_history: RothHistory,
    pub objective: Objective,
    pub options: PlanOptions,
}

impl Scenario {
    /// Filing status for every plan year (single owner).
    pub fn filing_status(&self) -> FilingStatus {
        FilingStatus::Single
    }

    /// Age reached during the first plan year.
    pub fn start_age(&self) -> u32 {
        u32::try_from(self.start_year - self.person.birth_year).unwrap_or(0)
    }

    /// Number of plan years.
    pub fn years(&self) -> usize {
        (self.person.planning_age.saturating_sub(self.start_age()) + 1) as usize
    }

    /// Wages and contributions for plan year `n` (zero when not working).
    pub fn working_year(&self, n: usize) -> WorkingYear {
        self.working_years.get(n).cloned().unwrap_or_default()
    }

    /// Cumulative inflation factor for year `n` (1.0 in the first year); `n` may be `years()`.
    pub fn gamma(&self, n: usize) -> f64 {
        let r = &self.rates;
        if r.annual_inflation.is_empty() {
            return (1.0 + r.inflation).powi(n as i32);
        }
        (0..n)
            .map(|k| {
                1.0 + r
                    .annual_inflation
                    .get(k)
                    .or(r.annual_inflation.last())
                    .copied()
                    .unwrap_or(r.inflation)
            })
            .product()
    }

    /// Validates input ranges; called by the planner before building the model.
    pub fn validate(&self) -> Result<()> {
        let bad = |msg: String| Err(EngineError::InvalidInput(msg));
        let p = &self.person;
        if !(1..=12).contains(&p.birth_month) {
            return bad(format!("birth month {} must be 1–12", p.birth_month));
        }
        let age = self.start_year - p.birth_year;
        if !(18..=110).contains(&age) {
            return bad(format!("age {age} in the start year is outside 18–110"));
        }
        if i64::from(p.planning_age) <= i64::from(age) || p.planning_age > 119 {
            return bad(format!(
                "planning age {} must be after the current age {age} and at most 119",
                p.planning_age
            ));
        }
        let a = &self.accounts;
        for (name, value) in [
            ("taxable", a.taxable),
            ("taxable_cost_basis", a.taxable_cost_basis),
            ("tax_deferred", a.tax_deferred),
            ("roth", a.roth),
            ("hsa", a.hsa),
        ] {
            if !value.is_finite() || value < 0.0 {
                return bad(format!("{name} balance must be a non-negative amount"));
            }
        }
        if a.taxable_cost_basis > a.taxable + 1e-6 {
            return bad("taxable cost basis cannot exceed the taxable balance".into());
        }
        let r = &self.rates;
        for (name, value) in [
            ("inflation", r.inflation),
            ("taxable_return", r.taxable_return),
            ("tax_deferred_return", r.tax_deferred_return),
            ("roth_return", r.roth_return),
        ] {
            if !value.is_finite() || value <= -0.5 || value > 0.5 {
                return bad(format!("{name} rate {value} is outside (-50%, 50%]"));
            }
        }
        if r.annual_inflation
            .iter()
            .any(|v| !v.is_finite() || *v <= -0.5 || *v > 0.5)
        {
            return bad("annual inflation must be within (-50%, 50%]".into());
        }
        if r.annual_returns
            .iter()
            .chain(r.account_returns.iter().flatten())
            .any(|v| !v.is_finite() || *v <= -0.6 || *v > 0.6)
        {
            return bad("annual returns must be within (-60%, 60%]".into());
        }
        // Dividends are paid even in a year the price falls, so the yield is
        // not bounded by the year's total return.
        if !(0.0..=0.2).contains(&r.dividend_yield) {
            return bad("dividend yield must be between 0 and 20%".into());
        }
        if let Some(ss) = &self.social_security {
            if !ss.monthly_pia.is_finite() || ss.monthly_pia < 0.0 {
                return bad("monthly PIA must be non-negative".into());
            }
            if let ClaimStrategy::AtAge(claim) = ss.claim {
                if !(MIN_CLAIM_AGE..=MAX_CLAIM_AGE).contains(&claim) {
                    return bad(format!("claiming age {claim} is outside 62–70"));
                }
            }
        }
        match self.objective {
            Objective::MaxSpending { bequest_floor } if bequest_floor < 0.0 => {
                return bad("bequest floor must be non-negative".into());
            }
            Objective::MaxBequest { net_spending } if net_spending < 0.0 => {
                return bad("net spending must be non-negative".into());
            }
            _ => {}
        }
        let o = &self.options;
        if !(0.0..1.0).contains(&o.heirs_tax_rate) {
            return bad("heirs' tax rate must be in [0, 1)".into());
        }
        if !(0.0..1.0).contains(&o.spending_flexibility) {
            return bad("spending flexibility must be in [0, 1)".into());
        }
        if o.spending_profile
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return bad("spending profile values must be positive".into());
        }
        if o.required_spending
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return bad("required spending must be non-negative".into());
        }
        if o.other_taxable_income
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return bad("other income must be non-negative".into());
        }
        if o.cash_flows.iter().any(|v| !v.is_finite()) {
            return bad("cash flows must be finite".into());
        }
        for (n, w) in self.working_years.iter().enumerate() {
            for (name, value) in [
                ("wages", w.wages),
                ("tax-deferred contribution", w.tax_deferred_contribution),
                ("employer contribution", w.employer_contribution),
                ("Roth contribution", w.roth_contribution),
                ("taxable contribution", w.taxable_contribution),
                ("HSA contribution", w.hsa_contribution),
            ] {
                if !value.is_finite() || value < 0.0 {
                    return bad(format!("{name} in plan year {n} must be non-negative"));
                }
            }
        }
        let history = &self.roth_history;
        if history.conversions.len() > ROTH_MATURATION_YEARS
            || history.contributions.len() > ROTH_MATURATION_YEARS
        {
            return bad(format!(
                "Roth history covers at most the last {ROTH_MATURATION_YEARS} years"
            ));
        }
        if history
            .conversions
            .iter()
            .chain(&history.contributions)
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return bad("past Roth conversions and contributions must be non-negative".into());
        }
        for (name, value) in [
            ("ACA benchmark premium", o.aca_benchmark_premium),
            ("other medical expenses", o.other_medical_expenses),
            ("Rule of 55 amount", o.rule_of_55.unwrap_or(0.0)),
            (
                "72(t) payment",
                o.sepp.map_or(0.0, |sepp| sepp.annual_payment),
            ),
        ] {
            if !value.is_finite() || value < 0.0 {
                return bad(format!("{name} must be non-negative"));
            }
        }
        for debt in &o.debts {
            if !(debt.balance >= 0.0 && debt.annual_payment >= 0.0)
                || !(0.0..1.0).contains(&debt.annual_rate)
            {
                return bad(format!(
                    "debt '{}' has an invalid amount or rate",
                    debt.label
                ));
            }
            if debt.balance * debt.annual_rate >= debt.annual_payment && debt.balance > 0.0 {
                return bad(format!(
                    "debt '{}': the payment does not cover the interest",
                    debt.label
                ));
            }
            if debt.start >= self.years() || debt.payoff.is_some_and(|p| p < debt.start) {
                return bad(format!(
                    "debt '{}' starts or is repaid outside the plan",
                    debt.label
                ));
            }
        }
        if o.first_year_conversion
            .is_some_and(|x| !x.is_finite() || x < 0.0)
        {
            return bad("the first year's Roth conversion must be non-negative".into());
        }
        if o.max_roth_conversion.is_some_and(|cap| cap < 0.0) {
            return bad("max Roth conversion must be non-negative".into());
        }
        if o.max_iterations == 0 {
            return bad("max_iterations must be at least 1".into());
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Single retiree used across the engine's tests: born 1964, retiring at 62
    /// in 2026, planning to age 95.
    pub(crate) fn standard_retiree() -> Scenario {
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
            objective: Objective::MaxBequest {
                net_spending: 80_000.0,
            },
            options: PlanOptions::default(),
        }
    }

    #[test]
    fn timeline_is_derived_from_ages() {
        let s = standard_retiree();
        assert_eq!(s.start_age(), 62);
        assert_eq!(s.years(), 34);
        assert!((s.gamma(0) - 1.0).abs() < 1e-12);
        assert!((s.gamma(2) - 1.025f64.powi(2)).abs() < 1e-12);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        let mut s = standard_retiree();
        s.accounts.taxable_cost_basis = 600_000.0;
        assert!(s.validate().is_err());

        let mut s = standard_retiree();
        s.person.planning_age = 60;
        assert!(s.validate().is_err());

        let mut s = standard_retiree();
        s.social_security = Some(SocialSecurity {
            monthly_pia: 2_000.0,
            claim: ClaimStrategy::AtAge(71.0),
        });
        assert!(s.validate().is_err());

        let mut s = standard_retiree();
        s.options.heirs_tax_rate = 1.5;
        assert!(s.validate().is_err());
    }

    #[test]
    fn per_account_returns_take_precedence_and_allow_down_years() {
        let mut r = standard_retiree().rates;
        assert_eq!(r.for_year(3), [0.05; 4]);
        r.annual_returns = vec![0.07, 0.04];
        assert_eq!(r.for_year(5), [0.04; 4]);
        r.account_returns = vec![[0.08, 0.06, 0.07, 0.05], [-0.3, -0.2, -0.25, 0.01]];
        assert_eq!(r.for_year(0), [0.08, 0.06, 0.07, 0.05]);
        assert_eq!(r.for_year(9), [-0.3, -0.2, -0.25, 0.01]);

        // A dividend yield above a losing year's return is still valid.
        let mut s = standard_retiree();
        s.rates = r;
        assert!(s.validate().is_ok());
        s.rates.account_returns[1][0] = -0.7;
        assert!(s.validate().is_err());
    }
}
