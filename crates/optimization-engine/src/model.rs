//! MILP formulation of Owl's yearly planning model on top of `good_lp`.
//!
//! Index `n` runs over plan years `0..N`; balances are start-of-year values for
//! `0..=N`. Withdrawals, conversions and deposits happen at the start of the year
//! and the remainder grows at the account's return (Owl `_add_account_balance_carryover`).
//!
//! Decision variables (Owl names in brackets):
//! - `traditional_ira_withdrawal_t`, `taxable_brokerage_liquidation_t`,
//!   `roth_withdrawal_t` [`w_ijn`], `roth_conversion_t` [`x_in`]
//! - surplus deposits into the taxable account [`d_in`/`s_n`], net spending [`g_n`]
//! - ordinary bracket fill [`f_tn`], standard deduction used [`e_n`],
//!   LTCG band allocation [`q_pn`] with regime binaries [`zl`]
//! - Medicare cost [`m_n`] with IRMAA tier portions/binaries [`h`, `zm`]
//! - Social Security claiming-month binaries [`zssa`] and the derived
//!   `social_security_active_t` binary
//!
//! Non-linear tax pieces Owl resolves with its self-consistent loop are passed
//! in as [`LoopParams`]: the taxable fraction of Social Security, the MAGI that
//! phases out the senior deduction, the gain fraction of taxable liquidations
//! (from tracked cost basis), and Medicare costs when IRMAA is not optimized.

use std::fmt;

use good_lp::{constraint, variable, Constraint, Expression, ProblemVariables, Variable};

use crate::rmd::rmd_fraction;
use crate::scenario::{ClaimStrategy, Objective, Scenario, WithdrawalOrder, ROTH_MATURATION_YEARS};
use crate::social_security::{
    claim_age_for_month, claiming_factor, full_retirement_age, month_for_claim_age, CLAIM_MONTHS,
};
use crate::tax::aca::{CLIFF as ACA_CLIFF, FPL_2026_SINGLE};
use crate::tax::federal::{
    bracket_widths, ltcg_thresholds, ss_taxability_thresholds, standard_deduction, LTCG_RATES,
    ORDINARY_RATES, SS_MAX_TAXABLE_FRACTION,
};
use crate::tax::irmaa::{tier_annual_costs, tier_floors, IRMAA_TIERS, MEDICARE_AGE};

/// Ordinary income tax brackets.
pub(crate) const BRACKETS: usize = ORDINARY_RATES.len();
/// Tax-deferred withdrawals before 59½ pay the 10% early-distribution penalty.
/// Owl's yearly convention: the year someone turns 59 is penalty-free when they
/// were born January–June, and the year they turn 60 otherwise.
const PENALTY_FREE_AGE: u32 = 59;
const EARLY_WITHDRAWAL_PENALTY: f64 = 0.10;
/// Growth credited to Roth activity from before the plan when sizing the
/// maturation retainer (Owl's `oldTau1`).
const PAST_ROTH_GROWTH: f64 = 1.10;

/// Constraint families, used to label rows for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    InitialBalance,
    TaxableCarryover,
    TaxDeferredCarryover,
    RothCarryover,
    RothMaturation,
    HsaCarryover,
    HsaQualifiedExpenses,
    StateTaxableIncome,
    StateRetirementExemption,
    AcaCliff,
    PenaltyFreeWithdrawals,
    GainHarvest,
    ContributionLimit,
    LeftoverInvested,
    DebtCarryover,
    DebtSchedule,
    CashFlow,
    TaxableIncome,
    RequiredMinimumDistribution,
    SpendingProfileFloor,
    SpendingProfileCeiling,
    RequiredSpending,
    FixedSpending,
    BequestFloor,
    LtcgPartitionFloor,
    LtcgPartitionCeiling,
    LtcgRegime15,
    LtcgRegime20,
    LtcgRoom15,
    LtcgZeroBandShutoff,
    LtcgRoom20,
    LtcgLowBandsShutoff,
    LtcgRegimeOrder,
    WithdrawalOrder,
    IrmaaOneTier,
    IrmaaMagiDefinition,
    IrmaaTierFloor,
    IrmaaTierCeiling,
    IrmaaCost,
    SocialSecurityOneClaimMonth,
    SocialSecurityActive,
    SocialSecurityTaxability,
    OrdinaryIncomeMoveUp,
    OrdinaryIncomeMoveDown,
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Family::InitialBalance => "starting account balance",
            Family::TaxableCarryover => "taxable account balance carryover",
            Family::TaxDeferredCarryover => "tax-deferred account balance carryover",
            Family::RothCarryover => "Roth account balance carryover",
            Family::RothMaturation => "Roth five-year maturation",
            Family::HsaCarryover => "HSA balance carryover",
            Family::HsaQualifiedExpenses => "HSA withdrawals within qualified medical costs",
            Family::StateTaxableIncome => "state taxable income definition",
            Family::StateRetirementExemption => "state retirement income exemption",
            Family::AcaCliff => "ACA premium tax credit cliff (400% of poverty level)",
            Family::PenaltyFreeWithdrawals => "Rule of 55 / 72(t) penalty-free withdrawals",
            Family::GainHarvest => "capital gains harvesting",
            Family::ContributionLimit => "contribution limit",
            Family::LeftoverInvested => "share of leftover pay invested",
            Family::DebtCarryover => "debt balance carryover",
            Family::DebtSchedule => "debt minimum payments",
            Family::CashFlow => "cash flow (income - taxes - deposits = spending)",
            Family::TaxableIncome => "ordinary taxable income definition",
            Family::RequiredMinimumDistribution => "required minimum distribution",
            Family::SpendingProfileFloor => "spending profile floor",
            Family::SpendingProfileCeiling => "spending profile ceiling",
            Family::RequiredSpending => "required (must-have) spending",
            Family::FixedSpending => "fixed net spending (maxBequest)",
            Family::BequestFloor => "bequest floor (maxSpending)",
            Family::LtcgPartitionFloor => "capital gains band partition (floor)",
            Family::LtcgPartitionCeiling => "capital gains band partition (ceiling)",
            Family::LtcgRegime15 => "0% LTCG band regime selector",
            Family::LtcgRegime20 => "15% LTCG band regime selector",
            Family::LtcgRoom15 => "0% LTCG band room",
            Family::LtcgZeroBandShutoff => "0% LTCG band shutoff",
            Family::LtcgRoom20 => "15% LTCG band room",
            Family::LtcgLowBandsShutoff => "0%/15% LTCG band shutoff",
            Family::LtcgRegimeOrder => "LTCG regime ordering",
            Family::WithdrawalOrder => "taxable-first withdrawal order",
            Family::IrmaaOneTier => "exactly one IRMAA tier",
            Family::IrmaaMagiDefinition => "IRMAA MAGI (two-year lookback)",
            Family::IrmaaTierFloor => "IRMAA tier floor",
            Family::IrmaaTierCeiling => "IRMAA tier ceiling",
            Family::IrmaaCost => "Medicare premium",
            Family::SocialSecurityOneClaimMonth => "exactly one Social Security claiming month",
            Family::SocialSecurityActive => "Social Security active flag",
            Family::SocialSecurityTaxability => "taxable Social Security (provisional income)",
            Family::OrdinaryIncomeMoveUp | Family::OrdinaryIncomeMoveDown => {
                "ordinary income change from previous iteration (loop stabilizer)"
            }
        };
        f.write_str(text)
    }
}

impl Family {
    /// Whether infeasibility diagnosis may relax this family. Only goals and
    /// money flows are relaxed; starting balances, account dynamics and the tax
    /// definitions are facts, and relaxing them would "explain" a conflict with a
    /// meaningless fix (such as a larger starting balance).
    pub fn is_elastic(self) -> bool {
        self.elastic_weight().is_some()
    }

    /// Cost per unit of slack in the elastic filter. Goals are relaxed first, so
    /// a conflict is explained as "this goal is out of reach by $X"; cash flow is
    /// relaxed only when no goal can absorb the conflict.
    pub(crate) fn elastic_weight(self) -> Option<f64> {
        match self {
            Family::FixedSpending
            | Family::BequestFloor
            | Family::SpendingProfileFloor
            | Family::SpendingProfileCeiling
            | Family::RequiredSpending => Some(1.0),
            Family::RequiredMinimumDistribution => Some(10.0),
            Family::CashFlow => Some(100.0),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sense {
    Eq,
    Le,
    Ge,
}

/// One linear constraint `expr (=|<=|>=) rhs`, kept symbolic so the model can be
/// materialized either strictly or with elastic slack for diagnostics.
pub(crate) struct Row {
    pub family: Family,
    pub year: Option<usize>,
    pub expr: Expression,
    pub sense: Sense,
    pub rhs: f64,
}

impl Row {
    pub fn to_constraint(&self) -> Constraint {
        let expr = self.expr.clone();
        match self.sense {
            Sense::Eq => constraint::eq(expr, self.rhs),
            Sense::Le => constraint::leq(expr, self.rhs),
            Sense::Ge => constraint::geq(expr, self.rhs),
        }
    }
}

/// Parameters fixed during one solve and updated by the self-consistent loop.
#[derive(Debug, Clone)]
pub(crate) struct LoopParams {
    /// Taxable fraction of Social Security benefits per year (≤ 0.85).
    pub ss_taxable_fraction: Vec<f64>,
    /// MAGI estimate per year for the senior-deduction phase-out.
    pub magi_estimate: Vec<f64>,
    /// Fraction of each taxable-account liquidation that is a capital gain.
    pub gain_fraction: Vec<f64>,
    /// Medicare premiums per year when IRMAA is not optimized.
    pub medicare_cost: Vec<f64>,
    /// Ordinary taxable income per year, which sets LTCG band room in loop mode.
    pub ordinary_income_estimate: Vec<f64>,
    /// Proximal weight per year (today's dollars of objective per dollar) on moving
    /// ordinary income away from `ordinary_income_estimate`. It stands in for the
    /// LTCG cost loop mode cannot see and vanishes once the loop is at a fixed point.
    pub proximal_weight: Vec<f64>,
    /// Net ACA premium per year at the previous iteration's MAGI; with the exact
    /// cliff, the premium at that MAGI or just under the cliff if it was over.
    pub aca_cost: Vec<f64>,
    /// Compute taxable Social Security exactly with binaries for this solve
    /// (needs a fixed claim) instead of from `ss_taxable_fraction`.
    pub exact_ss_taxability: bool,
    /// Select LTCG bands exactly with binaries for this solve, whatever
    /// `optimize_ltcg` says (used by the polish solve).
    pub exact_ltcg: bool,
}

/// Year-by-year constants derived from the scenario.
#[derive(Debug, Clone)]
pub(crate) struct Timeline {
    pub years: usize,
    pub gamma: Vec<f64>,
    pub age: Vec<u32>,
    pub calendar_year: Vec<i32>,
    pub bracket_widths: Vec<[f64; BRACKETS]>,
    pub ltcg_thresholds: Vec<[f64; 2]>,
    pub rmd_fraction: Vec<f64>,
    pub early_penalty: Vec<f64>,
    /// Wages plus other taxable income (nominal); both are taxed and spendable.
    pub other_income: Vec<f64>,
    /// Wages alone (nominal), for reporting.
    pub wages: Vec<f64>,
    /// Untaxed money in (+) or out (-) of the accounts (nominal).
    pub cash_flows: Vec<f64>,
    /// Contributions arriving during the year (nominal): taxable, tax-deferred
    /// (employee and employer) and Roth.
    pub contributions: Vec<[f64; 3]>,
    /// Contributions paid out of wages (Roth and taxable), which leave the cash flow.
    pub contributions_from_wages: Vec<f64>,
    /// HSA contributions (nominal), zero from Medicare age.
    pub hsa_contributions: Vec<f64>,
    /// Non-Medicare medical costs an HSA may pay for (nominal).
    pub other_medical: Vec<f64>,
    /// Benchmark ACA premium (nominal) in years on marketplace cover, else 0.
    pub aca_benchmark: Vec<f64>,
    /// Federal poverty level for one (nominal).
    pub fpl: Vec<f64>,
    /// Required 72(t) payment (nominal), else 0.
    pub sepp: Vec<f64>,
    /// Years in which the Rule of 55 makes plan withdrawals penalty-free.
    pub rule_of_55: Vec<bool>,
    /// State bracket widths, deduction and retirement-exemption cap (nominal).
    pub state_widths: Vec<Vec<f64>>,
    pub state_deduction: Vec<f64>,
    pub state_exemption_cap: Vec<f64>,
    /// Employer match rate (0 without optimized contributions).
    pub match_rate: f64,
    /// Optimized-contribution caps per working year (nominal): elective
    /// deferral, IRA, HSA, and the pay the match can apply to times its cap.
    pub deferral_cap: Vec<f64>,
    pub ira_cap: Vec<f64>,
    pub hsa_cap: Vec<f64>,
    pub match_base: Vec<f64>,
    /// Scheduled balance of each debt at the start of each year (nominal).
    pub debt_schedule: Vec<Vec<f64>>,
    pub profile: Vec<f64>,
    /// Must-have spending each year (nominal); 0 = no floor.
    pub required_spending: Vec<f64>,
    pub medicare_enrolled: Vec<bool>,
    pub irmaa_floors: Vec<[f64; IRMAA_TIERS - 1]>,
    pub irmaa_costs: Vec<[f64; IRMAA_TIERS]>,
    /// Upper bound on any year's income, used for big-M constants (Owl `_incomeCeiling`).
    pub income_ceiling: Vec<f64>,
    /// `ss_benefit[k][n]`: nominal benefit in year `n` when claiming at grid month `k`.
    pub ss_benefit: Vec<Vec<f64>>,
    /// Claiming months the optimizer may choose (or the single fixed month).
    pub ss_allowed: Vec<bool>,
}

impl Timeline {
    pub fn new(s: &Scenario) -> Self {
        let years = s.years();
        let start_age = s.start_age();
        let status = s.filing_status();
        let gamma: Vec<f64> = (0..=years).map(|n| s.gamma(n)).collect();
        let age: Vec<u32> = (0..years).map(|n| start_age + n as u32).collect();
        let at =
            |values: &Vec<f64>, n: usize, default: f64| values.get(n).copied().unwrap_or(default);

        let (ss_benefit, ss_allowed) = social_security_table(s, &gamma);
        let max_return = (0..years)
            .flat_map(|n| s.rates.for_year(n))
            .fold(0.0, f64::max);
        let portfolio =
            s.accounts.taxable + s.accounts.tax_deferred + s.accounts.roth + s.accounts.hsa;
        let working: Vec<_> = (0..years).map(|n| s.working_year(n)).collect();
        let wages: Vec<f64> = (0..years).map(|n| working[n].wages * gamma[n]).collect();
        let penalty_free_age = PENALTY_FREE_AGE + u32::from(s.person.birth_month > 6);
        let retired = |n: usize| working[n].wages <= 0.0;
        // Separation from the employer: the first year without wages.
        let separation = (0..years).find(|n| retired(*n));
        let rule_of_55 = (0..years)
            .map(|n| {
                s.options.rule_of_55.is_some_and(|cap| cap > 0.0)
                    && separation.is_some_and(|sep| n >= sep && age[sep] >= 55)
            })
            .collect();
        let state = s.options.state_tax.as_ref();
        let choice = s.options.optimize_contributions.as_ref();
        let chosen = |n: usize| choice.filter(|_| working[n].wages > 0.0);
        let max_ss = (0..years)
            .map(|n| ss_benefit.iter().map(|row| row[n]).fold(0.0, f64::max))
            .collect::<Vec<_>>();

        Self {
            years,
            calendar_year: (0..years).map(|n| s.start_year + n as i32).collect(),
            bracket_widths: (0..years)
                .map(|n| bracket_widths(status, gamma[n]))
                .collect(),
            ltcg_thresholds: (0..years)
                .map(|n| ltcg_thresholds(status, gamma[n]))
                .collect(),
            rmd_fraction: age
                .iter()
                .map(|a| rmd_fraction(s.person.birth_year, *a))
                .collect(),
            early_penalty: age
                .iter()
                .map(|a| {
                    if *a < penalty_free_age {
                        EARLY_WITHDRAWAL_PENALTY
                    } else {
                        0.0
                    }
                })
                .collect(),
            other_income: (0..years)
                .map(|n| at(&s.options.other_taxable_income, n, 0.0) * gamma[n] + wages[n])
                .collect(),
            cash_flows: (0..years)
                .map(|n| at(&s.options.cash_flows, n, 0.0) * gamma[n])
                .collect(),
            contributions: (0..years)
                .map(|n| {
                    let w = &working[n];
                    // Optimized years choose these amounts instead.
                    let fixed = if chosen(n).is_some() { 0.0 } else { 1.0 };
                    [
                        w.taxable_contribution * gamma[n],
                        fixed * (w.tax_deferred_contribution + w.employer_contribution) * gamma[n],
                        fixed * w.roth_contribution * gamma[n],
                    ]
                })
                .collect(),
            contributions_from_wages: (0..years)
                .map(|n| {
                    let roth = if chosen(n).is_some() {
                        0.0
                    } else {
                        working[n].roth_contribution
                    };
                    (roth + working[n].taxable_contribution) * gamma[n]
                })
                .collect(),
            match_rate: choice.map_or(0.0, |c| c.match_rate),
            deferral_cap: (0..years)
                .map(|n| chosen(n).map_or(0.0, |c| c.deferral_limit(age[n]) * gamma[n]))
                .collect(),
            ira_cap: (0..years)
                .map(|n| chosen(n).map_or(0.0, |c| c.ira_limit_at(age[n]) * gamma[n]))
                .collect(),
            hsa_cap: (0..years)
                .map(|n| {
                    chosen(n)
                        .filter(|_| age[n] < MEDICARE_AGE)
                        .map_or(0.0, |c| c.hsa_limit_at(age[n]) * gamma[n])
                })
                .collect(),
            match_base: (0..years)
                .map(|n| chosen(n).map_or(0.0, |c| c.match_rate * c.match_cap * wages[n]))
                .collect(),
            debt_schedule: s
                .options
                .debts
                .iter()
                .map(|debt| {
                    let mut balance = debt.balance;
                    (0..=years)
                        .map(|n| {
                            if n < debt.start || debt.payoff.is_some_and(|p| n > p) {
                                return 0.0;
                            }
                            let now = balance;
                            balance = ((balance - debt.annual_payment).max(0.0))
                                * (1.0 + debt.annual_rate);
                            now
                        })
                        .collect()
                })
                .collect(),
            hsa_contributions: (0..years)
                .map(|n| {
                    if age[n] >= MEDICARE_AGE || chosen(n).is_some() {
                        0.0
                    } else {
                        working[n].hsa_contribution * gamma[n]
                    }
                })
                .collect(),
            other_medical: (0..years)
                .map(|n| s.options.other_medical_expenses * gamma[n])
                .collect(),
            aca_benchmark: (0..years)
                .map(|n| {
                    if age[n] < MEDICARE_AGE && retired(n) {
                        s.options.aca_benchmark_premium * gamma[n]
                    } else {
                        0.0
                    }
                })
                .collect(),
            fpl: (0..years).map(|n| FPL_2026_SINGLE * gamma[n]).collect(),
            sepp: (0..years)
                .map(|n| match s.options.sepp {
                    // Payments run five years or to 59½, whichever is later.
                    Some(sepp) => {
                        let year = s.start_year + n as i32;
                        let first = sepp.start_year;
                        let ends = (first + 5).max(s.person.birth_year + penalty_free_age as i32);
                        if (first..ends).contains(&year) {
                            sepp.annual_payment
                        } else {
                            0.0
                        }
                    }
                    None => 0.0,
                })
                .collect(),
            rule_of_55,
            state_widths: (0..years)
                .map(|n| state.map_or_else(Vec::new, |st| st.bracket_widths(gamma[n])))
                .collect(),
            state_deduction: (0..years)
                .map(|n| state.map_or(0.0, |st| st.standard_deduction * gamma[n]))
                .collect(),
            state_exemption_cap: (0..years)
                .map(|n| {
                    state.map_or(0.0, |st| {
                        if age[n] >= st.exemption_age {
                            st.retirement_exemption * gamma[n]
                        } else {
                            0.0
                        }
                    })
                })
                .collect(),
            profile: (0..years)
                .map(|n| at(&s.options.spending_profile, n, 1.0) * gamma[n])
                .collect(),
            required_spending: (0..years)
                .map(|n| at(&s.options.required_spending, n, 0.0) * gamma[n])
                .collect(),
            medicare_enrolled: age.iter().map(|a| *a >= MEDICARE_AGE).collect(),
            irmaa_floors: (0..years).map(|n| tier_floors(status, gamma[n])).collect(),
            irmaa_costs: (0..years)
                .map(|n| tier_annual_costs(gamma[n], s.options.include_part_d))
                .collect(),
            income_ceiling: (0..years)
                .map(|n| {
                    2.0 * portfolio * (1.0 + max_return).powi(n as i32)
                        + max_ss[n]
                        + at(&s.options.other_taxable_income, n, 0.0) * gamma[n]
                        + wages[n]
                        + 1.0
                })
                .collect(),
            gamma,
            age,
            wages,
            ss_benefit,
            ss_allowed,
        }
    }

    /// Standard deduction in year `n` given a MAGI estimate (senior bonus phase-out).
    pub fn standard_deduction(&self, s: &Scenario, n: usize, magi: f64) -> f64 {
        let seniors = u32::from(self.age[n] >= 65);
        standard_deduction(
            s.filing_status(),
            self.gamma[n],
            seniors,
            self.calendar_year[n],
            magi,
        )
    }
}

/// Benefit table for every claiming month and which months are selectable.
fn social_security_table(s: &Scenario, gamma: &[f64]) -> (Vec<Vec<f64>>, Vec<bool>) {
    let years = s.years();
    let Some(ss) = &s.social_security else {
        return (vec![vec![0.0; years]], vec![true]);
    };
    let p = &s.person;
    let fra = full_retirement_age(p.birth_year, p.birth_month, 15);
    let birth = f64::from(p.birth_year) + f64::from(p.birth_month - 1) / 12.0;
    let fixed_month = match ss.claim {
        ClaimStrategy::AtAge(age) => Some(month_for_claim_age(age)),
        ClaimStrategy::Optimize => None,
    };

    let mut table = Vec::with_capacity(CLAIM_MONTHS);
    let mut allowed = Vec::with_capacity(CLAIM_MONTHS);
    for k in 0..CLAIM_MONTHS {
        let claim_age = claim_age_for_month(k);
        let claim_time = birth + claim_age;
        // A claiming month already in the past cannot be chosen by the optimizer.
        allowed.push(match fixed_month {
            Some(fixed) => k == fixed,
            None => claim_time >= f64::from(s.start_year),
        });
        let factor = claiming_factor(fra, claim_age, false).unwrap_or(0.0);
        let annual = 12.0 * ss.monthly_pia * factor;
        table.push(
            (0..years)
                .map(|n| {
                    let year_start = f64::from(s.start_year) + n as f64;
                    let active = (year_start + 1.0 - claim_time).clamp(0.0, 1.0);
                    annual * gamma[n] * active
                })
                .collect(),
        );
    }
    (table, allowed)
}

/// Handles to the decision variables, for reading the solution.
pub(crate) struct Vars {
    pub b_taxable: Vec<Variable>,
    pub b_deferred: Vec<Variable>,
    pub b_roth: Vec<Variable>,
    pub taxable_brokerage_liquidation: Vec<Variable>,
    pub traditional_ira_withdrawal: Vec<Variable>,
    pub roth_withdrawal: Vec<Variable>,
    pub roth_conversion: Vec<Variable>,
    pub deposit: Vec<Variable>,
    /// Working years' leftover pay spent instead of deposited (`leftover_invested`).
    pub leftover_spent: Vec<Option<Variable>>,
    pub spending: Vec<Variable>,
    pub bracket_fill: Vec<[Variable; BRACKETS]>,
    pub deduction_used: Vec<Variable>,
    pub ltcg_band: Vec<[Variable; 3]>,
    pub medicare: Vec<Variable>,
    pub irmaa_tier: Vec<Option<[Variable; IRMAA_TIERS]>>,
    pub claim_month: Vec<Variable>,
    pub social_security_active: Vec<Variable>,
    /// Taxable Social Security per year when computed exactly (`None` in loop
    /// mode or in years without benefits).
    pub ss_taxable: Vec<Option<Variable>>,
    pub b_hsa: Vec<Variable>,
    pub hsa_withdrawal: Vec<Variable>,
    /// State bracket fill, deduction used and retirement exemption used.
    pub state_fill: Vec<Vec<Variable>>,
    pub state_deduction: Vec<Variable>,
    pub state_exemption: Vec<Variable>,
    /// 1 when MAGI is over the ACA cliff (exact-cliff mode, ACA years only).
    pub aca_over_cliff: Vec<Option<Variable>>,
    /// Tax-deferred withdrawals made penalty-free by the Rule of 55.
    pub rule_of_55_withdrawal: Vec<Option<Variable>>,
    /// Gains realized and bought back (when harvesting is allowed).
    pub gain_harvest: Vec<Option<Variable>>,
    /// Chosen contributions in working years (optimized-contribution mode).
    pub contributions: Vec<Option<ContributionVars>>,
    /// Per debt: balance at the start of each year and the payment made.
    pub debt_balance: Vec<Vec<Variable>>,
    pub debt_payment: Vec<Vec<Variable>>,
}

/// One working year's chosen contributions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContributionVars {
    pub pre_tax: Variable,
    pub roth_401k: Variable,
    pub roth_ira: Variable,
    pub hsa: Variable,
    pub employer: Variable,
}

impl ContributionVars {
    /// Money taken out of pay this year.
    fn taken_from_pay(&self) -> Expression {
        terms(&[
            (1.0, self.pre_tax),
            (1.0, self.roth_401k),
            (1.0, self.roth_ira),
            (1.0, self.hsa),
        ])
    }
}

impl Vars {
    /// Social Security benefit in year `n` as an expression of the claim binaries.
    pub fn social_security(&self, t: &Timeline, n: usize) -> Expression {
        let mut e = Expression::with_capacity(self.claim_month.len());
        for (k, z) in self.claim_month.iter().enumerate() {
            let benefit = t.ss_benefit[k][n];
            if benefit != 0.0 {
                e.add_mul(benefit, *z);
            }
        }
        e
    }

    /// Realized long-term gains and qualified dividends in year `n`, including
    /// dividends on the half year this year's taxable contributions are invested
    /// (Owl's `0.5 * kappa` term).
    pub fn capital_gains(
        &self,
        s: &Scenario,
        t: &Timeline,
        p: &LoopParams,
        n: usize,
    ) -> Expression {
        let dy = s.rates.dividend_yield;
        let mut e = Expression::with_capacity(3);
        e += 0.5 * dy * t.contributions[n][0];
        e.add_mul(dy, self.b_taxable[n]);
        e.add_mul(
            p.gain_fraction[n] - dy,
            self.taxable_brokerage_liquidation[n],
        );
        e.add_mul(dy, self.deposit[n]);
        if let Some(h) = self.gain_harvest[n] {
            e.add_mul(p.gain_fraction[n], h);
        }
        e
    }

    /// Ordinary taxable income before the deduction plus realized gains (AGI-basis MAGI).
    pub fn magi(&self, s: &Scenario, t: &Timeline, p: &LoopParams, n: usize) -> Expression {
        let mut e = self.capital_gains(s, t, p, n);
        e.add_mul(1.0, self.deduction_used[n]);
        for f in &self.bracket_fill[n] {
            e.add_mul(1.0, *f);
        }
        e
    }

    /// After-heir-tax estate at the end of the plan (nominal).
    pub fn bequest(&self, s: &Scenario) -> Expression {
        let last = self.b_taxable.len() - 1;
        let mut e = Expression::with_capacity(3);
        e.add_mul(1.0, self.b_taxable[last]);
        e.add_mul(1.0 - s.options.heirs_tax_rate, self.b_deferred[last]);
        e.add_mul(1.0, self.b_roth[last]);
        // Heirs pay income tax on inherited HSA money, as on an IRA.
        e.add_mul(1.0 - s.options.heirs_tax_rate, self.b_hsa[last]);
        for balances in &self.debt_balance {
            e.add_mul(-1.0, balances[last]);
        }
        e
    }

    /// ACA premium in year `n`: the loop estimate, plus the jump to the full
    /// benchmark when the exact-cliff binary says MAGI is over the cliff.
    pub fn aca_premium(&self, t: &Timeline, p: &LoopParams, n: usize) -> Expression {
        let mut e = Expression::with_capacity(1);
        if t.aca_benchmark[n] <= 0.0 {
            return e;
        }
        e += p.aca_cost[n];
        if let Some(z) = self.aca_over_cliff[n] {
            e.add_mul(t.aca_benchmark[n] - p.aca_cost[n], z);
        }
        e
    }

    /// The penalized part of this year's tax-deferred withdrawal.
    pub fn penalized_withdrawal(&self, t: &Timeline, n: usize) -> Expression {
        let mut e = terms(&[(1.0, self.traditional_ira_withdrawal[n])]);
        e += -t.sepp[n];
        if let Some(free) = self.rule_of_55_withdrawal[n] {
            e.add_mul(-1.0, free);
        }
        e
    }
}

/// A fully specified model: variables, symbolic rows and the objective to maximize.
pub(crate) struct Model {
    pub problem: ProblemVariables,
    pub rows: Vec<Row>,
    pub objective: Expression,
    pub vars: Vars,
}

struct Builder {
    rows: Vec<Row>,
}

impl Builder {
    fn add(
        &mut self,
        family: Family,
        year: Option<usize>,
        expr: Expression,
        sense: Sense,
        rhs: f64,
    ) {
        self.rows.push(Row {
            family,
            year,
            expr,
            sense,
            rhs,
        });
    }
}

fn terms(pairs: &[(f64, Variable)]) -> Expression {
    let mut e = Expression::with_capacity(pairs.len());
    for (coef, var) in pairs {
        e.add_mul(*coef, *var);
    }
    e
}

/// How integer variables are treated when building the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Integrality {
    /// Binaries are binary (the real MILP).
    Exact,
    /// Binaries are continuous in [0, 1] (LP relaxation, for fast checks).
    Relaxed,
}

/// Builds the MILP for one self-consistent iteration.
pub(crate) fn build(s: &Scenario, t: &Timeline, p: &LoopParams, integrality: Integrality) -> Model {
    let n_years = t.years;
    let mut pv = ProblemVariables::new();
    let binary = |name: String| match integrality {
        Integrality::Exact => variable().binary().name(name),
        Integrality::Relaxed => variable().clamp(0.0, 1.0).name(name),
    };
    let nonneg = |pv: &mut ProblemVariables, count: usize, name: &str| -> Vec<Variable> {
        (0..count)
            .map(|n| pv.add(variable().min(0.0).name(format!("{name}_{n}"))))
            .collect()
    };

    let b_taxable = nonneg(&mut pv, n_years + 1, "taxable_balance");
    let b_deferred = nonneg(&mut pv, n_years + 1, "tax_deferred_balance");
    let b_roth = nonneg(&mut pv, n_years + 1, "roth_balance");
    let taxable_brokerage_liquidation = nonneg(&mut pv, n_years, "taxable_brokerage_liquidation");
    let traditional_ira_withdrawal = nonneg(&mut pv, n_years, "traditional_ira_withdrawal");
    let roth_withdrawal = nonneg(&mut pv, n_years, "roth_withdrawal");
    let roth_conversion: Vec<Variable> = (0..n_years)
        .map(|n| {
            let v = variable().min(0.0).name(format!("roth_conversion_{n}"));
            let o = &s.options;
            pv.add(match (o.first_year_conversion, o.max_roth_conversion) {
                _ if !o.allow_roth_conversions => v.max(0.0),
                // This year's conversion pinned (to weigh one decision across markets).
                (Some(x), _) if n == 0 => v.min(x * t.gamma[0]).max(x * t.gamma[0]),
                (_, Some(cap)) => v.max(cap * t.gamma[n]),
                (_, None) => v,
            })
        })
        .collect();
    let deposit = nonneg(&mut pv, n_years, "taxable_deposit");
    let leftover_spent: Vec<Option<Variable>> = (0..n_years)
        .map(|n| {
            s.options
                .leftover_invested
                .filter(|share| *share < 1.0 && t.wages[n] > 0.0)
                .map(|_| pv.add(variable().min(0.0).name(format!("leftover_spent_{n}"))))
        })
        .collect();
    let spending = nonneg(&mut pv, n_years, "net_spending");
    let bracket_fill: Vec<[Variable; BRACKETS]> = (0..n_years)
        .map(|n| {
            std::array::from_fn(|b| {
                let width = t.bracket_widths[n][b];
                let v = variable().min(0.0).name(format!("bracket_{b}_{n}"));
                pv.add(if width.is_finite() { v.max(width) } else { v })
            })
        })
        .collect();
    let deduction_used: Vec<Variable> = (0..n_years)
        .map(|n| {
            let sigma = t.standard_deduction(s, n, p.magi_estimate[n]);
            pv.add(
                variable()
                    .clamp(0.0, sigma)
                    .name(format!("deduction_used_{n}")),
            )
        })
        .collect();
    let ltcg_band: Vec<[Variable; 3]> = (0..n_years)
        .map(|n| {
            std::array::from_fn(|b| pv.add(variable().min(0.0).name(format!("ltcg_band_{b}_{n}"))))
        })
        .collect();
    let ltcg_regime: Vec<Option<(Variable, Variable)>> = (0..n_years)
        .map(|n| {
            (s.options.optimize_ltcg || p.exact_ltcg).then(|| {
                (
                    pv.add(binary(format!("ltcg_room15_{n}"))),
                    pv.add(binary(format!("ltcg_room20_{n}"))),
                )
            })
        })
        .collect();
    let medicare: Vec<Variable> = (0..n_years)
        .map(|n| {
            let v = variable().min(0.0).name(format!("medicare_{n}"));
            pv.add(if !t.medicare_enrolled[n] {
                v.max(0.0)
            } else if s.options.optimize_medicare {
                v
            } else {
                v.clamp(p.medicare_cost[n], p.medicare_cost[n])
            })
        })
        .collect();
    let irmaa_portion: Vec<Option<[Variable; IRMAA_TIERS]>> = (0..n_years)
        .map(|n| {
            (s.options.optimize_medicare && t.medicare_enrolled[n]).then(|| {
                std::array::from_fn(|q| {
                    pv.add(variable().min(0.0).name(format!("irmaa_magi_{q}_{n}")))
                })
            })
        })
        .collect();
    let irmaa_tier: Vec<Option<[Variable; IRMAA_TIERS]>> = (0..n_years)
        .map(|n| {
            irmaa_portion[n]
                .map(|_| std::array::from_fn(|q| pv.add(binary(format!("irmaa_tier_{q}_{n}")))))
        })
        .collect();
    let claim_month: Vec<Variable> = t
        .ss_allowed
        .iter()
        .enumerate()
        .map(|(k, allowed)| {
            let v = binary(format!("claim_month_{k}"));
            pv.add(if *allowed { v } else { v.max(0.0) })
        })
        .collect();
    let social_security_active: Vec<Variable> = (0..n_years)
        .map(|n| pv.add(binary(format!("social_security_active_{n}"))))
        .collect();
    // Taxable-first gates (Owl `zo`): taxable exhausted, then tax-deferred exhausted.
    let order_gates: Vec<Option<(Variable, Variable)>> = (0..n_years)
        .map(|n| {
            (s.options.withdrawal_order == WithdrawalOrder::TaxableFirst).then(|| {
                (
                    pv.add(binary(format!("taxable_exhausted_{n}"))),
                    pv.add(binary(format!("tax_deferred_exhausted_{n}"))),
                )
            })
        })
        .collect();
    // Exact taxable Social Security needs a known benefit, so a fixed claim.
    let fixed_claim = match t.ss_allowed.iter().filter(|a| **a).count() {
        1 if s.social_security.is_some() => t.ss_allowed.iter().position(|a| *a),
        _ => None,
    };
    let ss_tax: Vec<Option<SsTaxVars>> = (0..n_years)
        .map(|n| {
            let k = fixed_claim.filter(|_| p.exact_ss_taxability)?;
            let benefit = t.ss_benefit[k][n];
            (benefit > 0.0).then(|| SsTaxVars {
                benefit,
                taxable: pv.add(
                    variable()
                        .clamp(0.0, SS_MAX_TAXABLE_FRACTION * benefit)
                        .name(format!("ss_taxable_{n}")),
                ),
                over_lo: pv.add(variable().min(0.0).name(format!("ss_over_lo_{n}"))),
                over_hi: pv.add(variable().min(0.0).name(format!("ss_over_hi_{n}"))),
                half_tier: pv.add(variable().min(0.0).name(format!("ss_half_tier_{n}"))),
                tier_capped: pv.add(binary(format!("ss_tier_capped_{n}"))),
                fully_taxed: pv.add(binary(format!("ss_fully_taxed_{n}"))),
            })
        })
        .collect();

    let b_hsa = nonneg(&mut pv, n_years + 1, "hsa_balance");
    let hsa_withdrawal = nonneg(&mut pv, n_years, "hsa_withdrawal");
    let state_fill: Vec<Vec<Variable>> = (0..n_years)
        .map(|n| {
            t.state_widths[n]
                .iter()
                .enumerate()
                .map(|(k, width)| {
                    let v = variable().min(0.0).name(format!("state_bracket_{k}_{n}"));
                    pv.add(if width.is_finite() { v.max(*width) } else { v })
                })
                .collect()
        })
        .collect();
    let state_deduction: Vec<Variable> = (0..n_years)
        .map(|n| {
            pv.add(
                variable()
                    .clamp(0.0, t.state_deduction[n])
                    .name(format!("state_deduction_{n}")),
            )
        })
        .collect();
    let state_exemption: Vec<Variable> = (0..n_years)
        .map(|n| {
            pv.add(
                variable()
                    .clamp(0.0, t.state_exemption_cap[n])
                    .name(format!("state_exemption_{n}")),
            )
        })
        .collect();
    let aca_over_cliff: Vec<Option<Variable>> = (0..n_years)
        .map(|n| {
            (s.options.aca_exact_cliff && t.aca_benchmark[n] > 0.0)
                .then(|| pv.add(binary(format!("aca_over_cliff_{n}"))))
        })
        .collect();
    let rule_of_55_withdrawal: Vec<Option<Variable>> = (0..n_years)
        .map(|n| {
            (t.rule_of_55[n] && t.early_penalty[n] > 0.0)
                .then(|| pv.add(variable().min(0.0).name(format!("rule_of_55_{n}"))))
        })
        .collect();
    let gain_harvest: Vec<Option<Variable>> = (0..n_years)
        .map(|n| {
            s.options
                .allow_gain_harvesting
                .then(|| pv.add(variable().min(0.0).name(format!("gain_harvest_{n}"))))
        })
        .collect();

    let contributions: Vec<Option<ContributionVars>> = (0..n_years)
        .map(|n| {
            let choice = s.options.optimize_contributions.as_ref()?;
            if t.deferral_cap[n] <= 0.0 && t.ira_cap[n] <= 0.0 {
                return None;
            }
            let cap = |pv: &mut ProblemVariables, name: &str, max: f64| {
                pv.add(
                    variable()
                        .clamp(0.0, max.max(0.0))
                        .name(format!("{name}_{n}")),
                )
            };
            let roth_401k_cap = if choice.allow_roth_401k {
                t.deferral_cap[n]
            } else {
                0.0
            };
            let roth_ira_cap = if choice.allow_roth_ira {
                t.ira_cap[n]
            } else {
                0.0
            };
            Some(ContributionVars {
                pre_tax: cap(&mut pv, "pre_tax_deferral", t.deferral_cap[n]),
                roth_401k: cap(&mut pv, "roth_401k", roth_401k_cap),
                roth_ira: cap(&mut pv, "roth_ira", roth_ira_cap),
                hsa: cap(&mut pv, "hsa_contribution", t.hsa_cap[n]),
                employer: cap(&mut pv, "employer_match", t.match_base[n]),
            })
        })
        .collect();
    let debt_balance: Vec<Vec<Variable>> = (0..s.options.debts.len())
        .map(|k| nonneg(&mut pv, n_years + 1, &format!("debt_{k}_balance")))
        .collect();
    let debt_payment: Vec<Vec<Variable>> = (0..s.options.debts.len())
        .map(|k| nonneg(&mut pv, n_years, &format!("debt_{k}_payment")))
        .collect();

    let vars = Vars {
        b_taxable,
        b_deferred,
        b_roth,
        taxable_brokerage_liquidation,
        traditional_ira_withdrawal,
        roth_withdrawal,
        roth_conversion,
        deposit,
        leftover_spent,
        spending,
        bracket_fill,
        deduction_used,
        ltcg_band,
        medicare,
        irmaa_tier,
        claim_month,
        social_security_active,
        ss_taxable: ss_tax
            .iter()
            .map(|x| x.as_ref().map(|x| x.taxable))
            .collect(),
        b_hsa,
        hsa_withdrawal,
        state_fill,
        state_deduction,
        state_exemption,
        aca_over_cliff,
        rule_of_55_withdrawal,
        gain_harvest,
        contributions,
        debt_balance,
        debt_payment,
    };
    let v = &vars;
    let mut b = Builder { rows: Vec::new() };
    let r = &s.rates;

    // Starting balances.
    for (var, amount) in [
        (v.b_taxable[0], s.accounts.taxable),
        (v.b_deferred[0], s.accounts.tax_deferred),
        (v.b_roth[0], s.accounts.roth),
        (v.b_hsa[0], s.accounts.hsa),
    ] {
        b.add(
            Family::InitialBalance,
            Some(0),
            terms(&[(1.0, var)]),
            Sense::Eq,
            amount,
        );
    }

    for n in 0..n_years {
        let y = Some(n);
        let (wt, wd, wr) = (
            v.taxable_brokerage_liquidation[n],
            v.traditional_ira_withdrawal[n],
            v.roth_withdrawal[n],
        );
        let (x, d, g) = (v.roth_conversion[n], v.deposit[n], v.spending[n]);

        // Balance carryover: next = (1 + r) * (balance - outflows + inflows), plus
        // contributions arriving through the year at half a year's growth.
        let [kt, kd, kr] = t.contributions[n];
        let [rt, rd, rr, rh] = r.for_year(n);
        let gt = 1.0 + rt;
        b.add(
            Family::TaxableCarryover,
            y,
            terms(&[
                (1.0, v.b_taxable[n + 1]),
                (-gt, v.b_taxable[n]),
                (gt, wt),
                (-gt, d),
            ]),
            Sense::Eq,
            kt * (1.0 + rt / 2.0),
        );
        let gd = 1.0 + rd;
        b.add(
            Family::TaxDeferredCarryover,
            y,
            terms(&[
                (1.0, v.b_deferred[n + 1]),
                (-gd, v.b_deferred[n]),
                (gd, wd),
                (gd, x),
            ]) - chosen_into(&v.contributions[n], 1.0 + rd / 2.0, |c| {
                terms(&[(1.0, c.pre_tax), (1.0, c.employer)])
            }),
            Sense::Eq,
            kd * (1.0 + rd / 2.0),
        );
        let gr = 1.0 + rr;
        b.add(
            Family::RothCarryover,
            y,
            terms(&[
                (1.0, v.b_roth[n + 1]),
                (-gr, v.b_roth[n]),
                (gr, wr),
                (-gr, x),
            ]) - chosen_into(&v.contributions[n], 1.0 + rr / 2.0, |c| {
                terms(&[(1.0, c.roth_401k), (1.0, c.roth_ira)])
            }),
            Sense::Eq,
            kr * (1.0 + rr / 2.0),
        );
        add_contribution_rows(&mut b, t, v, n);
        add_debt_rows(&mut b, s, t, v, n);
        add_roth_maturation_row(&mut b, s, t, v, n);
        add_hsa_rows(&mut b, t, v, n, 1.0 + rh);
        add_penalty_free_rows(&mut b, t, v, n);
        add_gain_harvest_row(&mut b, v, n);

        // Cash flow: spending + deposits + taxes + Medicare = withdrawals + income.
        let ss = v.social_security(t, n);
        let mut cash = terms(&[
            (1.0, g),
            (1.0, d),
            (1.0, v.medicare[n]),
            (-1.0, wt),
            (-1.0, wd),
            (-1.0, wr),
            (-1.0, v.hsa_withdrawal[n]),
            (LTCG_RATES[1], v.ltcg_band[n][1]),
            (LTCG_RATES[2], v.ltcg_band[n][2]),
        ]);
        for (bracket, rate) in v.bracket_fill[n].iter().zip(ORDINARY_RATES) {
            cash.add_mul(rate, *bracket);
        }
        if let Some(state) = &s.options.state_tax {
            for (fill, (_, rate)) in v.state_fill[n].iter().zip(&state.brackets) {
                cash.add_mul(*rate, *fill);
            }
        }
        if t.early_penalty[n] > 0.0 {
            cash += t.early_penalty[n] * v.penalized_withdrawal(t, n);
        }
        cash += v.aca_premium(t, p, n);
        if let Some(c) = &v.contributions[n] {
            cash += c.taken_from_pay();
        }
        for payments in &v.debt_payment {
            cash.add_mul(1.0, payments[n]);
        }
        if let (Some(spent), Some(share)) = (v.leftover_spent[n], s.options.leftover_invested) {
            cash.add_mul(1.0, spent);
            // Leftover pay splits into the invested share and the spent rest,
            // share * spent = (1 - share) * deposit; money coming in this year
            // (a home sale) may be deposited whole.
            let share = share.max(0.0);
            let mut split = terms(&[(share, spent), (share - 1.0, d)]);
            if t.cash_flows[n] > 0.0 {
                let exempt = pv.add(
                    variable()
                        .clamp(0.0, t.cash_flows[n])
                        .name(format!("deposit_not_from_pay_{n}")),
                );
                split.add_mul(1.0 - share, exempt);
            }
            b.add(Family::LeftoverInvested, y, split, Sense::Eq, 0.0);
        }
        cash -= ss.clone();
        b.add(
            Family::CashFlow,
            y,
            cash,
            Sense::Eq,
            t.other_income[n] - t.contributions_from_wages[n] + t.cash_flows[n],
        );

        // Ordinary income = deduction used + bracket fill.
        let mut income = terms(&[(1.0, v.deduction_used[n]), (-1.0, wd), (-1.0, x)]);
        // Pre-tax deferrals and payroll HSA contributions come out of taxable pay.
        if let Some(c) = &v.contributions[n] {
            income.add_mul(1.0, c.pre_tax);
            income.add_mul(1.0, c.hsa);
        }
        for bracket in &v.bracket_fill[n] {
            income.add_mul(1.0, *bracket);
        }
        match &ss_tax[n] {
            Some(exact) => {
                income.add_mul(-1.0, exact.taxable);
                add_ss_taxability_rows(&mut b, s, t, p, v, n, exact);
            }
            None => income.add_mul(-p.ss_taxable_fraction[n], ss.clone()),
        }
        b.add(
            Family::TaxableIncome,
            y,
            income,
            Sense::Eq,
            t.other_income[n],
        );
        if let Some(state) = &s.options.state_tax {
            let taxable_ss = match &ss_tax[n] {
                Some(exact) => terms(&[(1.0, exact.taxable)]),
                None => ss.clone() * p.ss_taxable_fraction[n],
            };
            add_state_tax_rows(&mut b, state, v, n, taxable_ss);
        }
        add_aca_cliff_row(&mut b, s, t, p, v, n);

        // Required minimum distribution on the start-of-year balance.
        if t.rmd_fraction[n] > 0.0 {
            b.add(
                Family::RequiredMinimumDistribution,
                y,
                terms(&[(1.0, wd), (-t.rmd_fraction[n], v.b_deferred[n])]),
                Sense::Ge,
                0.0,
            );
        }

        add_ltcg_rows(&mut b, s, t, p, v, n, ltcg_regime[n]);
        if let Some(gates) = order_gates[n] {
            add_withdrawal_order_rows(&mut b, t, v, n, gates);
        }
        add_irmaa_rows(&mut b, s, t, p, v, n, irmaa_portion[n].as_ref());

        // social_security_active_t = 1 exactly when the chosen claim pays in year n.
        let mut active = terms(&[(1.0, v.social_security_active[n])]);
        for (k, z) in v.claim_month.iter().enumerate() {
            if t.ss_benefit[k][n] > 0.0 {
                active.add_mul(-1.0, *z);
            }
        }
        b.add(Family::SocialSecurityActive, y, active, Sense::Eq, 0.0);
    }

    add_rule_of_55_cap(&mut b, s, t, v);

    // Exactly one claiming month.
    b.add(
        Family::SocialSecurityOneClaimMonth,
        None,
        terms(&v.claim_month.iter().map(|z| (1.0, *z)).collect::<Vec<_>>()),
        Sense::Eq,
        1.0,
    );

    // Spending follows the profile within the flexibility band (Owl `_add_income_profile`).
    let lam = s.options.spending_flexibility;
    for n in 1..n_years {
        let (g0, gn) = (v.spending[0], v.spending[n]);
        let (xi0, xin) = (t.profile[0], t.profile[n]);
        b.add(
            Family::SpendingProfileFloor,
            Some(n),
            terms(&[(xi0, gn), (-(1.0 - lam) * xin, g0)]),
            Sense::Ge,
            0.0,
        );
        b.add(
            Family::SpendingProfileCeiling,
            Some(n),
            terms(&[(xi0, gn), (-(1.0 + lam) * xin, g0)]),
            Sense::Le,
            0.0,
        );
    }

    // Must-have expenses are met every year, whatever the objective.
    for n in 0..n_years {
        let required = t.required_spending[n];
        if required > 0.0 {
            b.add(
                Family::RequiredSpending,
                Some(n),
                terms(&[(1.0, v.spending[n])]),
                Sense::Ge,
                required,
            );
        }
    }

    let last_gamma = t.gamma[n_years];
    let mut objective = match s.objective {
        Objective::MaxSpending { bequest_floor } => {
            b.add(
                Family::BequestFloor,
                None,
                v.bequest(s),
                Sense::Ge,
                bequest_floor * last_gamma,
            );
            let mut e = Expression::with_capacity(n_years);
            for n in 0..n_years {
                e.add_mul(1.0 / t.gamma[n], v.spending[n]);
            }
            e
        }
        Objective::MaxBequest { net_spending } => {
            b.add(
                Family::FixedSpending,
                Some(0),
                terms(&[(1.0, v.spending[0])]),
                Sense::Eq,
                net_spending * t.profile[0],
            );
            v.bequest(s) * (1.0 / last_gamma)
        }
    };

    // Tie-breakers (Owl `epsilon`): convert early rather than late, keep money in
    // Roth, and avoid withdraw-then-redeposit round trips.
    // Proximal stabilizer for the LTCG loop: |G_n - estimate_n| priced at the weight.
    for n in 0..n_years {
        let weight = p.proximal_weight[n];
        if weight <= 0.0 || s.options.optimize_ltcg || p.exact_ltcg {
            continue;
        }
        let deviation = pv.add(
            variable()
                .min(0.0)
                .name(format!("ordinary_income_move_{n}")),
        );
        let mut up = terms(&[(-1.0, deviation)]);
        let mut down = terms(&[(-1.0, deviation)]);
        for f in &v.bracket_fill[n] {
            up.add_mul(1.0, *f);
            down.add_mul(-1.0, *f);
        }
        let anchor = p.ordinary_income_estimate[n];
        b.add(Family::OrdinaryIncomeMoveUp, Some(n), up, Sense::Le, anchor);
        b.add(
            Family::OrdinaryIncomeMoveDown,
            Some(n),
            down,
            Sense::Le,
            -anchor,
        );
        objective.add_mul(-weight / t.gamma[n], deviation);
    }

    let eps = s.options.epsilon;
    if eps > 0.0 {
        for n in 0..n_years {
            objective.add_mul(-eps * (1.0 + n as f64), v.roth_conversion[n]);
            objective.add_mul(-eps, v.roth_withdrawal[n]);
            objective.add_mul(-eps, v.deposit[n]);
        }
    }

    Model {
        problem: pv,
        rows: b.rows,
        objective,
        vars,
    }
}

/// Taxable first, then tax-deferred, then Roth (Owl `_add_withdrawal_ordering`).
/// Tax-deferred withdrawals beyond the RMD wait until the taxable account is
/// empty at year end, and Roth withdrawals until the tax-deferred one is too.
/// A surplus deposit keeps the taxable balance positive, so a year cannot both
/// bank a surplus and open the next gate.
fn add_withdrawal_order_rows(
    b: &mut Builder,
    t: &Timeline,
    v: &Vars,
    n: usize,
    (taxable_empty, deferred_empty): (Variable, Variable),
) {
    let y = Some(n);
    // Every gated quantity is a balance or a withdrawal, bounded by the ceiling.
    let m = t.income_ceiling[n];
    b.add(
        Family::WithdrawalOrder,
        y,
        terms(&[
            (1.0, v.traditional_ira_withdrawal[n]),
            (-t.rmd_fraction[n], v.b_deferred[n]),
            (-m, taxable_empty),
        ]),
        Sense::Le,
        0.0,
    );
    b.add(
        Family::WithdrawalOrder,
        y,
        terms(&[(1.0, v.roth_withdrawal[n]), (-m, deferred_empty)]),
        Sense::Le,
        0.0,
    );
    b.add(
        Family::WithdrawalOrder,
        y,
        terms(&[(1.0, v.b_taxable[n + 1]), (m, taxable_empty)]),
        Sense::Le,
        m,
    );
    b.add(
        Family::WithdrawalOrder,
        y,
        terms(&[(1.0, v.b_deferred[n + 1]), (m, deferred_empty)]),
        Sense::Le,
        m,
    );
    b.add(
        Family::WithdrawalOrder,
        y,
        terms(&[(1.0, deferred_empty), (-1.0, taxable_empty)]),
        Sense::Le,
        0.0,
    );
}

/// HSA (Owl's fourth account): contributions arrive through the year, and
/// withdrawals are tax-free but limited to qualified medical costs, here
/// Medicare premiums plus `other_medical_expenses`. It grows at its own
/// return (the tax-deferred return unless per-account returns are given).
fn add_hsa_rows(b: &mut Builder, t: &Timeline, v: &Vars, n: usize, growth: f64) {
    let y = Some(n);
    b.add(
        Family::HsaCarryover,
        y,
        terms(&[
            (1.0, v.b_hsa[n + 1]),
            (-growth, v.b_hsa[n]),
            (growth, v.hsa_withdrawal[n]),
        ]) - chosen_into(&v.contributions[n], 1.0 + (growth - 1.0) / 2.0, |c| {
            terms(&[(1.0, c.hsa)])
        }),
        Sense::Eq,
        t.hsa_contributions[n] * (1.0 + (growth - 1.0) / 2.0),
    );
    b.add(
        Family::HsaQualifiedExpenses,
        y,
        terms(&[(1.0, v.hsa_withdrawal[n]), (-1.0, v.medicare[n])]),
        Sense::Le,
        t.other_medical[n],
    );
}

/// `scale` times the chosen contributions `pick` selects, or nothing in years
/// whose contributions are given.
fn chosen_into(
    contributions: &Option<ContributionVars>,
    scale: f64,
    pick: impl Fn(&ContributionVars) -> Expression,
) -> Expression {
    match contributions {
        Some(c) => pick(c) * scale,
        None => Expression::with_capacity(0),
    }
}

/// Shared 401(k) deferral limit, payroll deferrals within pay, and the match:
/// `match_rate` of deferrals, already capped at `match_rate * match_cap * pay`.
fn add_contribution_rows(b: &mut Builder, t: &Timeline, v: &Vars, n: usize) {
    let Some(c) = &v.contributions[n] else {
        return;
    };
    let y = Some(n);
    b.add(
        Family::ContributionLimit,
        y,
        terms(&[(1.0, c.pre_tax), (1.0, c.roth_401k)]),
        Sense::Le,
        t.deferral_cap[n],
    );
    b.add(
        Family::ContributionLimit,
        y,
        terms(&[(1.0, c.pre_tax), (1.0, c.roth_401k), (1.0, c.hsa)]),
        Sense::Le,
        t.wages[n],
    );
    let rate = t.match_rate;
    b.add(
        Family::ContributionLimit,
        y,
        terms(&[(1.0, c.employer), (-rate, c.pre_tax), (-rate, c.roth_401k)]),
        Sense::Le,
        0.0,
    );
}

/// A loan: interest accrues on what is left after this year's payment, and the
/// balance may not fall behind the scheduled one. A future loan is taken out
/// at the start of its first year (the lender pays the seller, so no cash
/// moves); a loan with a payoff year is cleared during it.
fn add_debt_rows(b: &mut Builder, s: &Scenario, t: &Timeline, v: &Vars, n: usize) {
    for (k, debt) in s.options.debts.iter().enumerate() {
        let (balance, payment) = (&v.debt_balance[k], &v.debt_payment[k]);
        let y = Some(n);
        if n == 0 {
            b.add(
                Family::DebtCarryover,
                y,
                terms(&[(1.0, balance[0])]),
                Sense::Eq,
                if debt.start == 0 { debt.balance } else { 0.0 },
            );
        }
        // Nothing is owed before a later loan starts, so nothing can be paid:
        // a payment the year before would shrink the loan it brings.
        if n < debt.start {
            b.add(
                Family::DebtCarryover,
                y,
                terms(&[(1.0, payment[n])]),
                Sense::Eq,
                0.0,
            );
        }
        let g = 1.0 + debt.annual_rate;
        b.add(
            Family::DebtCarryover,
            y,
            terms(&[(1.0, balance[n + 1]), (-g, balance[n]), (g, payment[n])]),
            Sense::Eq,
            if debt.start == n + 1 {
                debt.balance
            } else {
                0.0
            },
        );
        b.add(
            Family::DebtSchedule,
            y,
            terms(&[(1.0, balance[n + 1])]),
            Sense::Le,
            t.debt_schedule[k][n + 1],
        );
    }
}

/// 72(t) payments must be taken and are penalty-free; under the Rule of 55,
/// plan withdrawals beyond them are penalty-free up to the plan balance
/// available (today's dollars, summed over the eligible years).
fn add_penalty_free_rows(b: &mut Builder, t: &Timeline, v: &Vars, n: usize) {
    let y = Some(n);
    if t.sepp[n] > 0.0 {
        b.add(
            Family::PenaltyFreeWithdrawals,
            y,
            terms(&[(1.0, v.traditional_ira_withdrawal[n])]),
            Sense::Ge,
            t.sepp[n],
        );
    }
    if let Some(free) = v.rule_of_55_withdrawal[n] {
        b.add(
            Family::PenaltyFreeWithdrawals,
            y,
            terms(&[(1.0, free), (-1.0, v.traditional_ira_withdrawal[n])]),
            Sense::Le,
            -t.sepp[n],
        );
    }
}

/// The cumulative Rule of 55 allowance, in today's dollars.
fn add_rule_of_55_cap(b: &mut Builder, s: &Scenario, t: &Timeline, v: &Vars) {
    let Some(cap) = s.options.rule_of_55 else {
        return;
    };
    let mut used = Expression::with_capacity(t.years);
    for (n, free) in v.rule_of_55_withdrawal.iter().enumerate() {
        if let Some(free) = free {
            used.add_mul(1.0 / t.gamma[n], *free);
        }
    }
    b.add(Family::PenaltyFreeWithdrawals, None, used, Sense::Le, cap);
}

/// A harvest sells and immediately rebuys shares still in the account after
/// this year's withdrawal.
fn add_gain_harvest_row(b: &mut Builder, v: &Vars, n: usize) {
    if let Some(h) = v.gain_harvest[n] {
        b.add(
            Family::GainHarvest,
            Some(n),
            terms(&[
                (1.0, h),
                (-1.0, v.b_taxable[n]),
                (1.0, v.taxable_brokerage_liquidation[n]),
            ]),
            Sense::Le,
            0.0,
        );
    }
}

/// State taxable income (Owl `_add_state_taxable_income`): federal gross
/// ordinary income (bracket fill plus the federal deduction used) plus realized
/// gains, less Social Security when the state does not tax it, less the state
/// deduction and the retirement exemption, which only tax-deferred withdrawals
/// and (where allowed) Roth conversions can use.
fn add_state_tax_rows(
    b: &mut Builder,
    state: &crate::tax::state::StateTax,
    v: &Vars,
    n: usize,
    taxable_social_security: Expression,
) {
    let y = Some(n);
    let mut row = terms(&[
        (1.0, v.state_deduction[n]),
        (1.0, v.state_exemption[n]),
        (-1.0, v.deduction_used[n]),
    ]);
    for fill in &v.state_fill[n] {
        row.add_mul(1.0, *fill);
    }
    for fill in &v.bracket_fill[n] {
        row.add_mul(-1.0, *fill);
    }
    for band in &v.ltcg_band[n] {
        row.add_mul(-1.0, *band);
    }
    if !state.taxes_social_security {
        row += taxable_social_security;
    }
    b.add(Family::StateTaxableIncome, y, row, Sense::Eq, 0.0);

    let mut eligible = terms(&[
        (1.0, v.state_exemption[n]),
        (-1.0, v.traditional_ira_withdrawal[n]),
    ]);
    if state.conversions_exempt {
        eligible.add_mul(-1.0, v.roth_conversion[n]);
    }
    b.add(
        Family::StateRetirementExemption,
        y,
        eligible,
        Sense::Le,
        0.0,
    );
}

/// With the exact cliff, MAGI above 400% of the poverty level forces the binary
/// that charges the full benchmark premium.
fn add_aca_cliff_row(
    b: &mut Builder,
    s: &Scenario,
    t: &Timeline,
    p: &LoopParams,
    v: &Vars,
    n: usize,
) {
    let Some(z) = v.aca_over_cliff[n] else {
        return;
    };
    let mut row = v.magi(s, t, p, n);
    row.add_mul(-t.income_ceiling[n], z);
    b.add(
        Family::AcaCliff,
        Some(n),
        row,
        Sense::Le,
        ACA_CLIFF * t.fpl[n],
    );
}

/// Variables of the exact taxable-Social-Security formulation for one year.
struct SsTaxVars {
    /// The (fixed-claim) benefit paid this year.
    benefit: f64,
    /// Taxable benefits (Owl `tss`).
    taxable: Variable,
    /// Provisional income above the lower and upper thresholds (`plo`, `phi`).
    over_lo: Variable,
    over_hi: Variable,
    /// min(threshold gap, benefit, over_lo): the amount taxed at 50% (`pmin`).
    half_tier: Variable,
    /// 1 when the 50% tier is capped rather than equal to `over_lo` (`zs0`).
    tier_capped: Variable,
    /// 1 when 85% of the benefit is taxable (`zs1`).
    fully_taxed: Variable,
}

/// Exact taxable Social Security (Owl `_configure_ss_taxability_lp`):
/// provisional income is non-SS income plus realized gains plus half the
/// benefit; 50% of the excess over the lower threshold (capped) and 85% of the
/// excess over the upper one are taxable, up to 85% of the benefit.
fn add_ss_taxability_rows(
    b: &mut Builder,
    s: &Scenario,
    t: &Timeline,
    p: &LoopParams,
    v: &Vars,
    n: usize,
    x: &SsTaxVars,
) {
    let y = Some(n);
    let (lo, hi) = ss_taxability_thresholds(s.filing_status());
    let cap = (hi - lo).min(x.benefit);
    let ceiling = t.income_ceiling[n];

    // Provisional income minus its constant part.
    let mut provisional = terms(&[
        (1.0, v.traditional_ira_withdrawal[n]),
        (1.0, v.roth_conversion[n]),
    ]);
    provisional += v.capital_gains(s, t, p, n);
    let fixed = t.other_income[n] + 0.5 * x.benefit;

    // over_lo >= provisional - lo and over_hi >= provisional - hi (both >= 0).
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.over_lo)]) - provisional.clone(),
        Sense::Ge,
        fixed - lo,
    );
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.over_hi)]) - provisional,
        Sense::Ge,
        fixed - hi,
    );

    // half_tier = min(cap, over_lo), selected by tier_capped.
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.half_tier)]),
        Sense::Le,
        cap,
    );
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.half_tier), (-1.0, x.over_lo)]),
        Sense::Le,
        0.0,
    );
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.half_tier), (-cap, x.tier_capped)]),
        Sense::Ge,
        0.0,
    );
    let m_lo = (ceiling - lo).max(0.0);
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.half_tier), (-1.0, x.over_lo), (m_lo, x.tier_capped)]),
        Sense::Ge,
        0.0,
    );

    // taxable = min(0.85 * benefit, 0.5 * half_tier + 0.85 * over_hi), selected by fully_taxed.
    let full = SS_MAX_TAXABLE_FRACTION * x.benefit;
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.taxable), (-0.5, x.half_tier), (-0.85, x.over_hi)]),
        Sense::Le,
        0.0,
    );
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[(1.0, x.taxable), (-full, x.fully_taxed)]),
        Sense::Ge,
        0.0,
    );
    let m_formula = 0.5 * cap + 0.85 * (ceiling - hi).max(0.0);
    b.add(
        Family::SocialSecurityTaxability,
        y,
        terms(&[
            (1.0, x.taxable),
            (-0.5, x.half_tier),
            (-0.85, x.over_hi),
            (m_formula, x.fully_taxed),
        ]),
        Sense::Ge,
        0.0,
    );
}

/// Roth five-year rule (Owl `_add_roth_maturation_constraints`): conversions
/// from the last five years, grown, and the gains on contributions from those
/// years must stay in the account. Contribution principal is conservatively
/// held too, as in Owl, since the LP does not track it separately.
fn add_roth_maturation_row(b: &mut Builder, s: &Scenario, t: &Timeline, v: &Vars, n: usize) {
    let history = &s.roth_history;
    let mut retained = terms(&[(1.0, v.b_roth[n]), (-1.0, v.roth_withdrawal[n])]);
    let (mut planned, mut past) = (0.0, 0.0);
    let mut gains = 1.0;
    for back in 1..=ROTH_MATURATION_YEARS {
        if let Some(year) = n.checked_sub(back) {
            // Market declines do not shrink the retainer (Owl: max(1, growth)).
            gains *= (1.0 + s.rates.for_year(year)[2]).max(1.0);
            retained.add_mul(-gains, v.roth_conversion[year]);
            planned += (gains - 1.0) * t.contributions[year][2];
            if let Some(c) = &v.contributions[year] {
                retained.add_mul(-(gains - 1.0), c.roth_401k);
                retained.add_mul(-(gains - 1.0), c.roth_ira);
            }
        } else {
            gains *= PAST_ROTH_GROWTH;
            let earlier = back - n - 1;
            let converted = history.conversions.get(earlier).copied().unwrap_or(0.0);
            let contributed = history.contributions.get(earlier).copied().unwrap_or(0.0);
            past += (gains - 1.0) * contributed + gains * converted;
        }
    }
    // Owl grows pre-plan activity at a flat 10%; after a market decline that can
    // exceed what the account holds, which would make every plan infeasible
    // rather than just block withdrawals. Cap it at the starting balance grown.
    let grown_start = s.accounts.roth
        * (0..n)
            .map(|year| 1.0 + s.rates.for_year(year)[2])
            .product::<f64>();
    b.add(
        Family::RothMaturation,
        Some(n),
        retained,
        Sense::Ge,
        planned + past.min(grown_start.max(0.0)),
    );
}

/// Long-term capital gains stacked on ordinary income (Owl `_configure_ltcg_constraints`).
/// With `regime` binaries the band is selected exactly (MILP mode); otherwise the
/// previous iteration's ordinary income bounds the 0% and 15% bands (loop mode).
fn add_ltcg_rows(
    b: &mut Builder,
    s: &Scenario,
    t: &Timeline,
    p: &LoopParams,
    v: &Vars,
    n: usize,
    regime: Option<(Variable, Variable)>,
) {
    let y = Some(n);
    let [q0, q1, q2] = v.ltcg_band[n];
    let [t15, t20] = t.ltcg_thresholds[n];

    // Bands must cover the realized gains (and not more than a dollar beyond).
    let mut partition = terms(&[(1.0, q0), (1.0, q1), (1.0, q2)]);
    partition -= v.capital_gains(s, t, p, n);
    b.add(
        Family::LtcgPartitionFloor,
        y,
        partition.clone(),
        Sense::Ge,
        0.0,
    );
    b.add(Family::LtcgPartitionCeiling, y, partition, Sense::Le, 1.0);

    let Some((zl15, zl20)) = regime else {
        let ordinary = p.ordinary_income_estimate[n];
        let room15 = (t15 - ordinary).max(0.0);
        let room20 = (t20 - ordinary).max(0.0);
        b.add(
            Family::LtcgRoom15,
            y,
            terms(&[(1.0, q0)]),
            Sense::Le,
            room15,
        );
        b.add(
            Family::LtcgRoom20,
            y,
            terms(&[(1.0, q0), (1.0, q1)]),
            Sense::Le,
            room20,
        );
        return;
    };

    let big_m = t20.max(t.income_ceiling[n] - t15);
    let ordinary = || {
        let mut e = Expression::with_capacity(BRACKETS);
        for f in &v.bracket_fill[n] {
            e.add_mul(1.0, *f);
        }
        e
    };

    // zl15 = 1 when ordinary taxable income is at or below the 15% threshold.
    b.add(
        Family::LtcgRegime15,
        y,
        ordinary() + big_m * zl15,
        Sense::Ge,
        t15,
    );
    b.add(
        Family::LtcgRegime15,
        y,
        ordinary() + big_m * zl15,
        Sense::Le,
        t15 + big_m,
    );
    b.add(
        Family::LtcgRegime20,
        y,
        ordinary() + big_m * zl20,
        Sense::Ge,
        t20,
    );
    b.add(
        Family::LtcgRegime20,
        y,
        ordinary() + big_m * zl20,
        Sense::Le,
        t20 + big_m,
    );

    b.add(
        Family::LtcgRoom15,
        y,
        ordinary() + q0 + big_m * zl15,
        Sense::Le,
        t15 + big_m,
    );
    b.add(
        Family::LtcgZeroBandShutoff,
        y,
        terms(&[(1.0, q0), (-big_m, zl15)]),
        Sense::Le,
        0.0,
    );
    b.add(
        Family::LtcgRoom20,
        y,
        ordinary() + q0 + q1 + big_m * zl20,
        Sense::Le,
        t20 + big_m,
    );
    b.add(
        Family::LtcgLowBandsShutoff,
        y,
        terms(&[(1.0, q0), (1.0, q1), (-big_m, zl20)]),
        Sense::Le,
        0.0,
    );
    b.add(
        Family::LtcgRegimeOrder,
        y,
        terms(&[(1.0, zl15), (-1.0, zl20)]),
        Sense::Le,
        0.0,
    );
}

/// IRMAA tier selection on MAGI from two years earlier (Owl
/// `_configure_Medicare_binary_variables` and `_add_Medicare_costs`).
fn add_irmaa_rows(
    b: &mut Builder,
    s: &Scenario,
    t: &Timeline,
    p: &LoopParams,
    v: &Vars,
    n: usize,
    portions: Option<&[Variable; IRMAA_TIERS]>,
) {
    let (Some(h), Some(z)) = (portions, v.irmaa_tier[n].as_ref()) else {
        return;
    };
    let y = Some(n);

    b.add(
        Family::IrmaaOneTier,
        y,
        terms(&z.iter().map(|zq| (1.0, *zq)).collect::<Vec<_>>()),
        Sense::Eq,
        1.0,
    );

    let mut magi = terms(&h.iter().map(|hq| (1.0, *hq)).collect::<Vec<_>>());
    let known = if n >= 2 {
        magi -= v.magi(s, t, p, n - 2);
        0.0
    } else {
        s.options.prior_magi[n]
    };
    b.add(Family::IrmaaMagiDefinition, y, magi, Sense::Eq, known);

    let floors = t.irmaa_floors[n];
    for q in 0..IRMAA_TIERS {
        if q > 0 {
            b.add(
                Family::IrmaaTierFloor,
                y,
                terms(&[(1.0, h[q]), (-floors[q - 1], z[q])]),
                Sense::Ge,
                0.0,
            );
        }
        let ceiling = if q < IRMAA_TIERS - 1 {
            floors[q]
        } else if n >= 2 {
            t.income_ceiling[n - 2]
        } else {
            t.income_ceiling[0].max(known + 1.0)
        };
        b.add(
            Family::IrmaaTierCeiling,
            y,
            terms(&[(1.0, h[q]), (-ceiling, z[q])]),
            Sense::Le,
            0.0,
        );
    }

    let mut cost = terms(&[(1.0, v.medicare[n])]);
    for (tier_cost, zq) in t.irmaa_costs[n].iter().zip(z) {
        cost.add_mul(-tier_cost, *zq);
    }
    b.add(Family::IrmaaCost, y, cost, Sense::Eq, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::tests::standard_retiree;
    use crate::scenario::{RothHistory, SocialSecurity, WorkingYear};

    fn params(t: &Timeline) -> LoopParams {
        LoopParams {
            ss_taxable_fraction: vec![0.85; t.years],
            magi_estimate: vec![0.0; t.years],
            gain_fraction: vec![0.3; t.years],
            medicare_cost: vec![0.0; t.years],
            ordinary_income_estimate: vec![0.0; t.years],
            proximal_weight: vec![0.0; t.years],
            aca_cost: vec![0.0; t.years],
            exact_ss_taxability: false,
            exact_ltcg: false,
        }
    }

    #[test]
    fn timeline_has_one_entry_per_year() {
        let s = standard_retiree();
        let t = Timeline::new(&s);
        assert_eq!(t.years, 34);
        assert_eq!(t.gamma.len(), 35);
        assert_eq!(t.age[0], 62);
        assert_eq!(t.age[33], 95);
        assert_eq!(t.calendar_year[0], 2026);
        // Medicare from 65, RMDs from 75 (born 1964), penalty only before 59½.
        assert!(!t.medicare_enrolled[2] && t.medicare_enrolled[3]);
        assert_eq!(t.rmd_fraction[12], 0.0);
        assert!(t.rmd_fraction[13] > 0.0);
        assert!(t.early_penalty.iter().all(|p| *p == 0.0));
    }

    #[test]
    fn early_withdrawal_penalty_follows_owls_half_year_convention() {
        // Turning 59 in 2026: penalty-free that year when born January–June.
        let mut s = standard_retiree();
        s.person.birth_year = 1967;
        s.person.birth_month = 6;
        let t = Timeline::new(&s);
        assert_eq!(t.age[0], 59);
        assert_eq!(t.early_penalty[0], 0.0);

        s.person.birth_month = 7;
        let t = Timeline::new(&s);
        assert_eq!(t.early_penalty[0], EARLY_WITHDRAWAL_PENALTY);
        assert_eq!(t.early_penalty[1], 0.0);
    }

    #[test]
    fn wages_are_income_and_contributions_are_inflated() {
        let mut s = standard_retiree();
        s.options.other_taxable_income = vec![1_000.0];
        s.working_years = vec![
            WorkingYear {
                wages: 100_000.0,
                tax_deferred_contribution: 20_000.0,
                employer_contribution: 5_000.0,
                roth_contribution: 7_000.0,
                taxable_contribution: 3_000.0,
                hsa_contribution: 0.0,
            };
            2
        ];
        let t = Timeline::new(&s);
        assert_eq!(t.other_income[0], 101_000.0);
        assert!((t.wages[1] - 100_000.0 * t.gamma[1]).abs() < 1e-9);
        assert_eq!(t.contributions[0], [3_000.0, 25_000.0, 7_000.0]);
        // Roth and taxable contributions come out of pay; pre-tax ones never reach it.
        assert_eq!(t.contributions_from_wages[0], 10_000.0);
        assert_eq!(t.wages[2], 0.0);
        assert_eq!(t.contributions[2], [0.0; 3]);
    }

    #[test]
    fn roth_maturation_retains_recent_conversions_and_contribution_gains() {
        let mut s = standard_retiree();
        s.accounts.roth = 100_000.0;
        s.roth_history = RothHistory {
            conversions: vec![10_000.0],
            contributions: vec![0.0, 6_000.0],
        };
        let t = Timeline::new(&s);
        let m = build(&s, &t, &params(&t), Integrality::Exact);
        let rows: Vec<_> = m
            .rows
            .iter()
            .filter(|r| r.family == Family::RothMaturation)
            .collect();
        assert_eq!(rows.len(), t.years);
        // Year 0: last year's conversion grown one year at 10%, plus the gains on
        // the contribution of two years ago (1.1^2 - 1).
        let expected = 10_000.0 * PAST_ROTH_GROWTH + 6_000.0 * (PAST_ROTH_GROWTH.powi(2) - 1.0);
        assert!((rows[0].rhs - expected).abs() < 1e-6, "rhs {}", rows[0].rhs);
        // Year 5: the history is out of the window.
        assert_eq!(rows[5].rhs, 0.0);

        // The retainer never exceeds what the account holds (after a decline).
        s.accounts.roth = 5_000.0;
        let m = build(&s, &t, &params(&t), Integrality::Exact);
        let first = m
            .rows
            .iter()
            .find(|r| r.family == Family::RothMaturation)
            .expect("row");
        assert_eq!(first.rhs, 5_000.0);
    }

    #[test]
    fn fixed_claim_pays_from_the_claiming_month_with_cola() {
        let s = standard_retiree();
        let t = Timeline::new(&s);
        let k = month_for_claim_age(67.0);
        assert_eq!(t.ss_allowed.iter().filter(|a| **a).count(), 1);
        assert!(t.ss_allowed[k]);
        let row = &t.ss_benefit[k];
        // Born June 1964: turns 67 in June 2031 (year index 5), so June–December pays.
        assert_eq!(row[4], 0.0);
        let full = 12.0 * 2_800.0 * t.gamma[6];
        assert!((row[5] - 7.0 / 12.0 * 12.0 * 2_800.0 * t.gamma[5]).abs() < 1e-6);
        assert!((row[6] - full).abs() < 1e-6);
    }

    #[test]
    fn optimized_claim_allows_only_future_months() {
        let mut s = standard_retiree();
        s.start_year = 2028; // already 64 in 2028
        s.social_security = Some(SocialSecurity {
            monthly_pia: 2_800.0,
            claim: ClaimStrategy::Optimize,
        });
        let t = Timeline::new(&s);
        // Claim times before January 2028 (age 63 years 7 months) are not selectable.
        assert!(!t.ss_allowed[0]);
        assert!(t.ss_allowed[CLAIM_MONTHS - 1]);
        let first_allowed = t
            .ss_allowed
            .iter()
            .position(|a| *a)
            .expect("some month allowed");
        assert!((claim_age_for_month(first_allowed) - (63.0 + 7.0 / 12.0)).abs() < 1e-9);
    }

    #[test]
    fn income_ceiling_bounds_portfolio_and_benefits() {
        let s = standard_retiree();
        let t = Timeline::new(&s);
        assert!(t.income_ceiling[0] >= 2.0 * 1_500_000.0);
        assert!(t.income_ceiling.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn model_contains_every_constraint_family_for_every_year() {
        let s = standard_retiree();
        let t = Timeline::new(&s);
        let m = build(&s, &t, &params(&t), Integrality::Exact);
        let count = |family: Family| m.rows.iter().filter(|r| r.family == family).count();
        assert_eq!(count(Family::CashFlow), t.years);
        assert_eq!(count(Family::TaxableIncome), t.years);
        assert_eq!(count(Family::TaxableCarryover), t.years);
        assert_eq!(count(Family::SocialSecurityActive), t.years);
        assert_eq!(count(Family::SocialSecurityOneClaimMonth), 1);
        assert_eq!(count(Family::FixedSpending), 1);
        assert_eq!(count(Family::BequestFloor), 0);
        assert_eq!(count(Family::RequiredMinimumDistribution), 21); // ages 75..=95
        let medicare_years = t.medicare_enrolled.iter().filter(|e| **e).count();
        assert_eq!(count(Family::IrmaaOneTier), medicare_years);
        assert_eq!(count(Family::SpendingProfileFloor), t.years - 1);
        assert_eq!(m.vars.claim_month.len(), CLAIM_MONTHS);
    }

    #[test]
    fn loop_mode_medicare_has_no_irmaa_binaries() {
        let mut s = standard_retiree();
        s.options.optimize_medicare = false;
        let t = Timeline::new(&s);
        let m = build(&s, &t, &params(&t), Integrality::Exact);
        assert!(m.vars.irmaa_tier.iter().all(Option::is_none));
        assert!(m.rows.iter().all(|r| r.family != Family::IrmaaCost));
    }
}
