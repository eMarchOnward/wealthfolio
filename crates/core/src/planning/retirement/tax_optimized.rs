//! Tax-optimized planning style: turns a retirement plan and the goal's accounts
//! into a scenario for the MILP engine (a port of Owl) and its lifetime schedule
//! into a year-by-year playbook.
//!
//! The engine models one US federal filer. Money crosses this boundary in
//! today's currency; the engine inflates thresholds and spending each year.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use wealthfolio_optimization_engine::markets::{
    account_returns, glide_fractions, historical_average, historical_sequence, AccountMixes,
    ClassRates,
};
use wealthfolio_optimization_engine::mortality::Sex;
use wealthfolio_optimization_engine::social_security::{
    claiming_factor, full_retirement_age, MAX_CLAIM_AGE, MIN_CLAIM_AGE,
};
use wealthfolio_optimization_engine::tax::state::StateTax;
use wealthfolio_optimization_engine::{
    optimize, Accounts, ClaimStrategy, ContributionChoice, Convergence, Debt, EngineError,
    Objective, Person, PlanOptions, Rates, RothHistory, Scenario, Schedule, Sepp, SocialSecurity,
    SolveStatus, WithdrawalOrder, WorkingYear,
};

use crate::planning::home_purchase::HomePurchase;

use super::engine::{
    annual_expenses_at_year, plan_accumulation_return, plan_income_at_age, plan_retirement_return,
    resolve_plan_dc_payouts,
};
use super::model::{
    AssetMix, BucketMixes, InflationMethod, MarketSettings, RetirementIncomeStream, RetirementPlan,
    ReturnMethod, TaxOptimizerObjective, TaxOptimizerSettings, TaxPlanGoal,
};
use crate::errors::{Result, ValidationError};

/// Starting balances of the goal's accounts by tax treatment, in today's money.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxOptimizerAccounts {
    pub taxable: f64,
    /// Cost basis of `taxable` (cash counts at full value).
    pub taxable_cost_basis: f64,
    pub tax_deferred: f64,
    pub roth: f64,
    /// Health savings accounts.
    #[serde(default)]
    pub hsa: f64,
    /// Each bucket's asset mix from its holdings; `None` when there are none.
    #[serde(default)]
    pub mixes: Option<BucketMixes>,
    /// Value of holdings without an asset class, counted as stocks.
    #[serde(default)]
    pub unclassified: f64,
}

/// One year of the optimized playbook, in today's money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxOptimizedYear {
    pub year: i32,
    pub age: u32,
    /// Still earning wages this year.
    pub working: bool,
    /// Start-of-year balances.
    pub taxable_balance: f64,
    pub tax_deferred_balance: f64,
    pub roth_balance: f64,
    pub hsa_balance: f64,
    pub wages: f64,
    pub taxable_contribution: f64,
    pub tax_deferred_contribution: f64,
    pub roth_contribution: f64,
    pub hsa_contribution: f64,
    pub taxable_withdrawal: f64,
    pub tax_deferred_withdrawal: f64,
    pub roth_withdrawal: f64,
    pub hsa_withdrawal: f64,
    pub roth_conversion: f64,
    /// Surplus cash reinvested in the taxable account.
    pub taxable_deposit: f64,
    /// Leftover pay spent instead of invested.
    #[serde(default)]
    pub leftover_spent: f64,
    /// Gains realized and bought back to raise the cost basis.
    pub gain_harvest: f64,
    /// Employer contributions, given or the chosen match (part of
    /// `tax_deferred_contribution`).
    pub employer_match: f64,
    /// Employee pre-tax 401(k) deferrals (part of `tax_deferred_contribution`).
    #[serde(default)]
    pub pre_tax_deferral: f64,
    /// Chosen Roth 401(k) and Roth IRA contributions (part of
    /// `roth_contribution`); a given Roth contribution is in neither.
    #[serde(default)]
    pub roth_401k: f64,
    #[serde(default)]
    pub roth_ira: f64,
    pub debt_payment: f64,
    pub debt_balance: f64,
    pub social_security: f64,
    pub other_income: f64,
    /// Untaxed money in (+) or out (-): a home's down payment and closing
    /// costs, a home sale's proceeds.
    pub cash_flow: f64,
    pub required_minimum_distribution: f64,
    pub net_spending: f64,
    /// Minimum expenses (with housing) the plan has to cover this year.
    pub required_spending: f64,
    pub ordinary_taxable_income: f64,
    pub capital_gains: f64,
    pub federal_income_tax: f64,
    pub capital_gains_tax: f64,
    pub state_income_tax: f64,
    pub early_withdrawal_penalty: f64,
    pub medicare: f64,
    /// Net ACA marketplace premium after the premium tax credit.
    pub aca_premium: f64,
    /// IRMAA tier (0 = standard premium) once enrolled in Medicare.
    pub irmaa_tier: Option<usize>,
    pub magi: f64,
    /// Cumulative inflation from today; multiply to get that year's dollars.
    pub inflation_factor: f64,
}

/// The same plan run the conventional way: taxable accounts first, then
/// tax-deferred, then Roth, with no Roth conversions and Social Security at the
/// stream's start age (Owl's `compare_to_baseline` policy). Today's money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxPlanBaseline {
    /// `optimal`, `optimalWithinGap`, `timeLimit`, or `infeasible`.
    pub status: String,
    pub first_year_spending: f64,
    pub bequest_after_tax: f64,
    pub lifetime_federal_tax: f64,
    pub lifetime_state_tax: f64,
    pub lifetime_medicare: f64,
    pub lifetime_aca: f64,
    pub lifetime_early_withdrawal_penalty: f64,
}

/// Outcome of a tax-optimized solve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxOptimizedPlan {
    pub objective: TaxOptimizerObjective,
    /// `optimal`, `optimalWithinGap`, `timeLimit`, or `infeasible`.
    pub status: String,
    /// Whether the self-consistent tax loop reached a fixed point.
    pub converged: bool,
    pub iterations: usize,
    /// Why no plan exists, when `status` is `infeasible`.
    pub infeasible_reason: Option<String>,
    pub starting_accounts: TaxOptimizerAccounts,
    pub social_security_claim_age: Option<f64>,
    pub first_year_spending: f64,
    pub bequest_after_tax: f64,
    pub lifetime_spending: f64,
    pub lifetime_federal_tax: f64,
    pub lifetime_state_tax: f64,
    pub lifetime_medicare: f64,
    pub lifetime_aca: f64,
    pub lifetime_early_withdrawal_penalty: f64,
    pub lifetime_roth_conversions: f64,
    pub years: Vec<TaxOptimizedYear>,
    /// The conventional strategy for comparison; `None` when not computed.
    pub baseline: Option<TaxPlanBaseline>,
    /// Retirement age the plan was solved for.
    pub retirement_age: u32,
    /// Every candidate age compared by the earliest-age search (empty otherwise).
    pub retirement_age_options: Vec<RetirementAgeOption>,
    /// The linked home purchase, when there is one.
    pub home: Option<HomePlanSummary>,
    /// Given contributions held to their IRS limits, and from which age.
    #[serde(default)]
    pub contribution_caps: Vec<ContributionCap>,
}

/// A given (not optimized) contribution that growth would take past its IRS
/// limit, held to the limit from `age` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributionCap {
    pub account: ContributionAccount,
    pub age: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContributionAccount {
    PreTax401k,
    Roth,
    Hsa,
}

/// HSA contribution limits in 2026 dollars: self-only and family coverage.
const HSA_SELF_ONLY_LIMIT: f64 = 4_400.0;
const HSA_FAMILY_LIMIT: f64 = 8_750.0;

/// IRS contribution limits (2026 dollars) for the plan's HSA coverage.
fn contribution_limits(settings: &TaxOptimizerSettings) -> ContributionChoice {
    ContributionChoice {
        hsa_limit: if settings.hsa_family_coverage {
            HSA_FAMILY_LIMIT
        } else {
            HSA_SELF_ONLY_LIMIT
        },
        ..ContributionChoice::limits_2026()
    }
}

/// The given contributions of each of the first `working_years` plan years in
/// today's money, `[pre-tax 401(k), Roth, HSA]`: grown at the contribution
/// growth rate and held to that year's IRS limits, with which of them were held.
/// The Roth figure covers Roth 401(k) and Roth IRA, so its limit is the 401(k)
/// room the pre-tax deferral leaves plus an IRA's.
fn given_contributions(
    plan: &RetirementPlan,
    settings: &TaxOptimizerSettings,
    start_age: u32,
    working_years: usize,
) -> Vec<([f64; 3], [bool; 3])> {
    let limits = contribution_limits(settings);
    let investment = &plan.investment;
    (0..working_years)
        .map(|n| {
            let age = start_age + n as u32;
            let scale = ((1.0 + investment.contribution_growth_rate)
                / (1.0 + investment.inflation_rate))
                .powi(n as i32);
            let wanted = [
                settings.pre_tax_contribution,
                settings.roth_contribution,
                settings.hsa_contribution,
            ]
            .map(|amount| amount * scale);
            let deferral = limits.deferral_limit(age);
            let pre_tax = wanted[0].min(deferral);
            let given = [
                pre_tax,
                wanted[1].min(deferral - pre_tax + limits.ira_limit_at(age)),
                wanted[2].min(limits.hsa_limit_at(age)),
            ];
            (given, [0, 1, 2].map(|k| given[k] < wanted[k] - 0.5))
        })
        .collect()
}

/// The age each given contribution first hits its limit; none when the
/// optimizer chooses the contributions.
fn contribution_caps(plan: &RetirementPlan, scenario: &Scenario) -> Vec<ContributionCap> {
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    if settings.optimize_contributions {
        return Vec::new();
    }
    let given = given_contributions(
        plan,
        &settings,
        scenario.start_age(),
        scenario.working_years.len(),
    );
    [
        ContributionAccount::PreTax401k,
        ContributionAccount::Roth,
        ContributionAccount::Hsa,
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(k, account)| {
        given
            .iter()
            .position(|(_, held)| held[k])
            .map(|n| ContributionCap {
                account,
                age: scenario.start_age() + n as u32,
            })
    })
    .collect()
}

/// A linked home purchase as the plan models it, in today's money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomePlanSummary {
    pub goal_title: String,
    pub purchase_year: i32,
    pub purchase_age: u32,
    /// Price when bought.
    pub price: f64,
    /// Down payment plus closing costs, paid from the accounts.
    pub upfront_cost: f64,
    /// Amount borrowed and the monthly payment, in today's money at purchase.
    pub mortgage: f64,
    pub monthly_payment: f64,
    /// Yearly property tax, insurance, maintenance and HOA in the first year.
    pub yearly_owning_costs: f64,
    /// What the sale of the current home brings in, after selling costs.
    pub sale_proceeds: f64,
    /// The home's value at the end of the plan, beside the after-tax estate.
    pub value_at_end: f64,
}

/// What a linked home purchase does to the plan. Today's money unless noted.
struct HomeTerms {
    title: String,
    /// Plan year of the purchase.
    year: usize,
    upfront: f64,
    sale_proceeds: f64,
    /// The new mortgage (nominal, from the purchase year).
    mortgage: Option<Debt>,
    /// Property tax, insurance, maintenance and HOA by plan year (0 before).
    owning_costs: Vec<f64>,
    price: f64,
    value_at_end: f64,
    /// Nominal-to-today factor of the purchase year.
    deflator: f64,
}

/// The linked purchase over a plan of `years` starting in `start_year`, with
/// prices and inflation compounding yearly.
fn home_terms(
    settings: &TaxOptimizerSettings,
    start_year: i32,
    birth_year: i32,
    years: usize,
    inflation: f64,
) -> Result<Option<HomeTerms>> {
    let Some(linked) = settings.home_purchase.as_ref() else {
        return Ok(None);
    };
    let home: &HomePurchase = &linked.purchase;
    home.validate()?;
    let purchase_year = match home.purchase_age {
        Some(age) => birth_year + age as i32,
        None => linked
            .target_date
            .as_deref()
            .and_then(|d| d.get(..4)?.parse::<i32>().ok())
            .ok_or_else(|| invalid("Set a purchase date or age on the home goal"))?,
    };
    let year = usize::try_from(purchase_year - start_year).unwrap_or(0);
    if year >= years {
        return Err(invalid(
            "The home purchase is after the planning horizon; move it earlier",
        ));
    }
    let real_growth = (1.0 + home.appreciation) / (1.0 + inflation);
    let value = |n: usize| home.price * real_growth.powi(n as i32);
    let price = value(year);
    let principal =
        home.price * (1.0 + home.appreciation).powi(year as i32) * (1.0 - home.down_payment_rate);
    let mortgage = (principal > 0.0).then(|| Debt {
        label: format!("{} mortgage", linked.goal_title),
        balance: principal,
        annual_rate: home.mortgage_rate,
        annual_payment: home.monthly_payment(principal) * 12.0,
        start: year,
        payoff: None,
    });
    let owning_costs = (0..years)
        .map(|n| {
            if n < year {
                0.0
            } else {
                value(n) * (home.property_tax_rate + home.maintenance_rate)
                    + home.insurance
                    + home.hoa_monthly * 12.0
            }
        })
        .collect();
    let sale_proceeds = if settings.sell_current_home {
        settings.current_home_value.max(0.0)
            * real_growth.powi(year as i32)
            * (1.0 - settings.selling_cost_rate.clamp(0.0, 1.0))
    } else {
        0.0
    };
    Ok(Some(HomeTerms {
        title: linked.goal_title.clone(),
        year,
        upfront: price * (home.down_payment_rate + home.closing_cost_rate),
        sale_proceeds,
        mortgage,
        owning_costs,
        price,
        value_at_end: value(years),
        deflator: (1.0 + inflation).powi(year as i32),
    }))
}

impl HomeTerms {
    fn summary(&self, start_year: i32, start_age: u32) -> HomePlanSummary {
        HomePlanSummary {
            goal_title: self.title.clone(),
            purchase_year: start_year + self.year as i32,
            purchase_age: start_age + self.year as u32,
            price: self.price,
            upfront_cost: self.upfront,
            mortgage: self.mortgage.as_ref().map_or(0.0, |m| m.balance) / self.deflator,
            monthly_payment: self
                .mortgage
                .as_ref()
                .map_or(0.0, |m| m.annual_payment / 12.0)
                / self.deflator,
            yearly_owning_costs: self.owning_costs[self.year],
            sale_proceeds: self.sale_proceeds,
            value_at_end: self.value_at_end,
        }
    }
}

/// One candidate age of the earliest-retirement search, in today's money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetirementAgeOption {
    pub age: u32,
    /// The minimum expenses (and bequest floor) can be funded.
    pub feasible: bool,
    /// Average yearly spending the plan sustains once retired (0 when not feasible).
    pub retirement_spending: f64,
    /// Average yearly minimum expenses once retired.
    pub required_spending: f64,
    pub bequest_after_tax: f64,
}

fn invalid(message: impl Into<String>) -> crate::errors::Error {
    ValidationError::InvalidInput(message.into()).into()
}

/// Birth year and month from the plan, falling back to the current age.
fn birth_year_month(plan: &RetirementPlan, as_of: NaiveDate) -> (i32, u32) {
    plan.personal
        .birth_year_month
        .as_deref()
        .and_then(|value| {
            let (year, month) = value.split_once('-')?;
            let month: u32 = month.parse().ok()?;
            (1..=12)
                .contains(&month)
                .then_some((year.parse().ok()?, month))
        })
        .unwrap_or_else(|| (as_of.year() - plan.personal.current_age as i32, 1))
}

fn social_security_stream<'a>(
    plan: &'a RetirementPlan,
    settings: &TaxOptimizerSettings,
) -> Option<&'a RetirementIncomeStream> {
    let id = settings.social_security_stream_id.as_deref()?;
    plan.income_streams.iter().find(|stream| stream.id == id)
}

/// The planned claim age and the monthly benefit at that age: from the linked
/// income stream, else from the amounts entered in the tax settings.
fn planned_social_security(
    stream: Option<&RetirementIncomeStream>,
    settings: &TaxOptimizerSettings,
) -> Option<(f64, f64)> {
    let (claim_age, monthly) = match stream {
        Some(stream) => (
            f64::from(stream.start_age),
            stream.monthly_amount.unwrap_or(0.0),
        ),
        None => (
            settings.social_security_claim_age.unwrap_or(67.0),
            settings.social_security_monthly,
        ),
    };
    (monthly > 0.0).then_some((claim_age.clamp(MIN_CLAIM_AGE, MAX_CLAIM_AGE), monthly))
}

/// Social Security at the planned claim age and benefit, from which the
/// benefit at full retirement age (PIA) follows.
fn social_security(
    stream: Option<&RetirementIncomeStream>,
    settings: &TaxOptimizerSettings,
    birth_year: i32,
    birth_month: u32,
) -> Result<Option<SocialSecurity>> {
    let Some((claim_age, monthly)) = planned_social_security(stream, settings) else {
        return Ok(None);
    };
    let fra = full_retirement_age(birth_year, birth_month, 15);
    let factor = claiming_factor(fra, claim_age, false)
        .map_err(|e| invalid(format!("Social Security claiming age: {e}")))?;
    Ok(Some(SocialSecurity {
        monthly_pia: monthly / factor,
        claim: if settings.optimize_claim_age {
            ClaimStrategy::Optimize
        } else {
            ClaimStrategy::AtAge(claim_age)
        },
    }))
}

/// Each bucket's asset mix in each of `years` plan years: the mix entered,
/// else the holdings' mix (else `equity_share` in stocks, the rest in bonds),
/// gliding to the target mix by the target age.
fn yearly_mixes(
    settings: &TaxOptimizerSettings,
    accounts: &TaxOptimizerAccounts,
    start_age: u32,
    horizon_age: u32,
    years: usize,
) -> Vec<AccountMixes> {
    let markets = &settings.markets;
    let fallback = AssetMix::stocks_bonds(settings.equity_share);
    let start = markets
        .starting_mix
        .or(accounts.mixes)
        .unwrap_or(BucketMixes::all(fallback))
        .to_array()
        .map(|mix| mix.normalized().unwrap_or(fallback));
    let Some(target) = markets.target_mix.and_then(|mix| mix.normalized()) else {
        return vec![start; years];
    };
    let span = markets
        .target_age
        .unwrap_or(horizon_age)
        .saturating_sub(start_age) as usize;
    glide_fractions(years, span, markets.glide)
        .into_iter()
        .map(|t| start.map(|mix| mix.toward(&target, t)))
        .collect()
}

/// Asset-class returns and inflation in each of `years` plan years, or `None`
/// to use the plan's projection returns. `inflation` is the fixed rate.
fn market_rates(
    markets: &MarketSettings,
    inflation: f64,
    years: usize,
) -> Result<Option<Vec<ClassRates>>> {
    let fixed_inflation = |mut rates: Vec<ClassRates>| {
        if markets.inflation_method == InflationMethod::Fixed {
            for r in &mut rates {
                r.inflation = inflation;
            }
        }
        rates
    };
    Ok(match markets.method {
        ReturnMethod::Plan => None,
        ReturnMethod::Fixed => Some(vec![
            ClassRates {
                stocks: markets.fixed.stocks,
                bonds: markets.fixed.bonds,
                notes: markets.fixed.notes,
                inflation,
            };
            years
        ]),
        ReturnMethod::Historical => {
            let rates = historical_sequence(markets.start_year, years).ok_or_else(|| {
                invalid(format!(
                    "Market history starts in 1928 and ends in 2025; {} is outside it",
                    markets.start_year
                ))
            })?;
            Some(fixed_inflation(rates))
        }
        ReturnMethod::HistoricalAverage => {
            let average =
                historical_average(markets.average_from, markets.average_to).ok_or_else(|| {
                    invalid("Choose at least two years between 1928 and 2025 to average")
                })?;
            Some(fixed_inflation(vec![average; years]))
        }
    })
}

/// Builds the engine scenario for `plan` starting in `as_of`'s year.
pub fn build_tax_optimized_scenario(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
) -> Result<Scenario> {
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    let personal = &plan.personal;
    let investment = &plan.investment;
    let inflation = investment.inflation_rate;
    let (birth_year, birth_month) = birth_year_month(plan, as_of);
    let start_year = as_of.year();
    let start_age = u32::try_from(start_year - birth_year)
        .map_err(|_| invalid("Birth date is in the future"))?;
    if personal.planning_horizon_age <= start_age {
        return Err(invalid("Planning horizon must be after the current age"));
    }
    let years = (personal.planning_horizon_age - start_age + 1) as usize;
    let real = |nominal: f64, n: usize| nominal / (1.0 + inflation).powi(n as i32);
    let working = |n: usize| start_age + (n as u32) < personal.target_retirement_age;

    // Expenses in today's money, by plan year, and the must-have ones: the
    // floor every year's spending has to cover (health insurance, taxes and
    // debt payments are modelled on their own). From a linked purchase on,
    // the new home's owning costs are must-have spending; rent and other items
    // that end at the purchase stop there (see `end_items_at_home_purchase`).
    let market_inflation = settings.markets.inflation.unwrap_or(inflation);
    let home = home_terms(&settings, start_year, birth_year, years, market_inflation)?;
    let (mut expenses, required): (Vec<f64>, Vec<f64>) = (0..years)
        .map(|n| {
            let age = personal.current_age + n as u32;
            let (total, essential) =
                annual_expenses_at_year(&plan.expenses, age, n as u32, inflation);
            let owning = home
                .as_ref()
                .filter(|h| n >= h.year)
                .map_or(0.0, |h| h.owning_costs[n]);
            (real(total, n) + owning, real(essential, n) + owning)
        })
        .unzip();
    let mut cash_flows = Vec::new();
    if let Some(h) = &home {
        cash_flows = vec![0.0; years];
        cash_flows[h.year] = h.sale_proceeds - h.upfront;
    }
    let first_planned = expenses
        .iter()
        .copied()
        .find(|e| *e > 0.0)
        .ok_or_else(|| invalid("Add at least one planned expense"))?;
    let mut last = first_planned;
    for e in expenses.iter_mut() {
        if *e > 0.0 {
            last = *e;
        } else {
            *e = last;
        }
    }

    // Income streams other than Social Security are fully taxable income.
    let ss_stream = social_security_stream(plan, &settings);
    let other_streams: Vec<RetirementIncomeStream> = plan
        .income_streams
        .iter()
        .filter(|s| ss_stream.is_none_or(|ss| ss.id != s.id))
        .cloned()
        .collect();
    let payouts = resolve_plan_dc_payouts(
        &other_streams,
        personal.current_age,
        personal.target_retirement_age,
        plan_accumulation_return(plan),
    );
    let other_income: Vec<f64> = (0..years)
        .map(|n| {
            let age = personal.current_age + n as u32;
            real(
                plan_income_at_age(&other_streams, &payouts, age, n as u32, inflation),
                n,
            )
        })
        .collect();

    // Wages and contributions until the target retirement age.
    let salary = personal.current_annual_salary.unwrap_or(0.0);
    let salary_growth = personal.salary_growth_rate.unwrap_or(inflation);
    let contribution_growth = investment.contribution_growth_rate;
    let given = given_contributions(
        plan,
        &settings,
        start_age,
        (0..years).take_while(|n| working(*n)).count(),
    );
    let working_years: Vec<WorkingYear> = given
        .iter()
        .enumerate()
        .map(|(n, ([pre_tax, roth, hsa], _))| {
            let pay = real(salary * (1.0 + salary_growth).powi(n as i32), n);
            let scale = real((1.0 + contribution_growth).powi(n as i32), n);
            WorkingYear {
                // Payroll 401(k) and HSA deferrals are taken out before tax;
                // optimized contributions are chosen out of gross pay instead.
                wages: if settings.optimize_contributions {
                    pay
                } else {
                    (pay - pre_tax - hsa).max(0.0)
                },
                tax_deferred_contribution: *pre_tax,
                employer_contribution: settings.employer_contribution * scale,
                roth_contribution: *roth,
                taxable_contribution: settings.taxable_contribution * scale,
                hsa_contribution: *hsa,
            }
        })
        .collect();

    let annual_returns: Vec<f64> = (0..years)
        .map(|n| {
            if working(n) {
                plan_accumulation_return(plan)
            } else {
                plan_retirement_return(plan)
            }
        })
        .collect();
    let markets = &settings.markets;
    let fee = investment.annual_investment_fee_rate.max(0.0);
    let mixes = yearly_mixes(
        &settings,
        accounts,
        start_age,
        personal.planning_horizon_age,
        years,
    );
    let (account_returns, annual_inflation) = match market_rates(markets, market_inflation, years)?
    {
        None => (Vec::new(), Vec::new()),
        Some(rates) => (
            rates
                .iter()
                .zip(&mixes)
                .map(|(r, m)| account_returns(m, r, fee))
                .collect(),
            rates.iter().map(|r| r.inflation).collect(),
        ),
    };

    let objective = match settings.objective {
        TaxOptimizerObjective::MaxBequest => Objective::MaxBequest {
            net_spending: expenses[0],
        },
        TaxOptimizerObjective::MaxSpending => Objective::MaxSpending {
            bequest_floor: settings.bequest_floor.max(0.0),
        },
    };
    // Without a figure from the last tax returns, pay less pre-tax deferrals.
    let prior_magi = settings
        .prior_magi
        .unwrap_or(salary - settings.pre_tax_contribution - settings.hsa_contribution)
        .max(0.0);

    Ok(Scenario {
        start_year,
        person: Person {
            birth_year,
            birth_month,
            sex: Sex::Male,
            planning_age: personal.planning_horizon_age,
        },
        accounts: Accounts {
            taxable: accounts.taxable.max(0.0),
            taxable_cost_basis: accounts
                .taxable_cost_basis
                .clamp(0.0, accounts.taxable.max(0.0)),
            tax_deferred: accounts.tax_deferred.max(0.0),
            roth: accounts.roth.max(0.0),
            hsa: accounts.hsa.max(0.0),
        },
        rates: Rates {
            inflation: annual_inflation
                .first()
                .copied()
                .unwrap_or(market_inflation),
            taxable_return: annual_returns[0],
            tax_deferred_return: annual_returns[0],
            roth_return: annual_returns[0],
            dividend_yield: settings.dividend_yield.clamp(0.0, 0.2),
            annual_returns,
            annual_inflation,
            account_returns,
        },
        social_security: social_security(ss_stream, &settings, birth_year, birth_month)?,
        working_years,
        roth_history: RothHistory {
            conversions: settings.recent_roth_conversions.clone(),
            contributions: settings.recent_roth_contributions.clone(),
        },
        objective,
        options: PlanOptions {
            prior_magi: [prior_magi, prior_magi],
            max_roth_conversion: settings.max_roth_conversion,
            heirs_tax_rate: settings.heirs_tax_rate,
            spending_flexibility: settings.spending_flexibility,
            spending_profile: expenses.iter().map(|e| e / expenses[0]).collect(),
            required_spending: required,
            other_taxable_income: other_income,
            cash_flows,
            state_tax: settings.state.as_deref().and_then(StateTax::for_code),
            aca_benchmark_premium: settings.aca_benchmark_premium.max(0.0),
            other_medical_expenses: settings.other_medical_expenses.max(0.0),
            rule_of_55: settings.rule_of_55_balance.filter(|cap| *cap > 0.0),
            sepp: settings
                .sepp_start_age
                .filter(|_| settings.sepp_annual_payment > 0.0)
                .map(|age| Sepp {
                    start_year: birth_year + age as i32,
                    annual_payment: settings.sepp_annual_payment,
                }),
            allow_gain_harvesting: settings.harvest_gains,
            optimize_contributions: settings.optimize_contributions.then(|| ContributionChoice {
                hsa_limit: if settings.hsa_eligible {
                    contribution_limits(&settings).hsa_limit
                } else {
                    0.0
                },
                match_rate: settings.match_rate.max(0.0),
                match_cap: settings.match_cap.max(0.0),
                allow_roth_401k: settings.roth_401k_available,
                ..ContributionChoice::limits_2026()
            }),
            leftover_invested: settings
                .leftover_invest_rate
                .filter(|_| settings.optimize_contributions)
                .map(|rate| rate.clamp(0.0, 1.0)),
            debts: settings
                .debts
                .iter()
                .filter(|debt| debt.balance > 0.0)
                .map(|debt| Debt {
                    label: debt.label.clone(),
                    balance: debt.balance,
                    annual_rate: debt.annual_rate,
                    annual_payment: debt.annual_payment,
                    start: 0,
                    // A home sale repays the current home's loan.
                    payoff: home
                        .as_ref()
                        .filter(|_| settings.sell_current_home)
                        .filter(|_| settings.current_home_debt.as_deref() == Some(&debt.label))
                        .map(|h| h.year),
                })
                .chain(home.as_ref().and_then(|h| h.mortgage.clone()))
                .collect(),
            ..PlanOptions::default()
        },
    })
}

fn status_label(status: SolveStatus) -> &'static str {
    match status {
        SolveStatus::Optimal => "optimal",
        SolveStatus::OptimalWithinGap => "optimalWithinGap",
        SolveStatus::TimeLimit => "timeLimit",
    }
}

fn playbook(
    scenario: &Scenario,
    accounts: &TaxOptimizerAccounts,
    objective: TaxOptimizerObjective,
    schedule: &Schedule,
) -> TaxOptimizedPlan {
    let working_years = scenario.working_years.len();
    let years: Vec<TaxOptimizedYear> = schedule
        .years
        .iter()
        .enumerate()
        .map(|(n, y)| {
            let gamma = scenario.gamma(n);
            let today = |nominal: f64| nominal / gamma;
            TaxOptimizedYear {
                year: y.year,
                age: y.age,
                working: n < working_years,
                taxable_balance: today(y.taxable_balance),
                tax_deferred_balance: today(y.tax_deferred_balance),
                roth_balance: today(y.roth_balance),
                hsa_balance: today(y.hsa_balance),
                wages: today(y.wages),
                taxable_contribution: today(y.taxable_contribution),
                tax_deferred_contribution: today(y.tax_deferred_contribution),
                roth_contribution: today(y.roth_contribution),
                hsa_contribution: today(y.hsa_contribution),
                taxable_withdrawal: today(y.taxable_brokerage_liquidation),
                tax_deferred_withdrawal: today(y.traditional_ira_withdrawal),
                roth_withdrawal: today(y.roth_withdrawal),
                hsa_withdrawal: today(y.hsa_withdrawal),
                roth_conversion: today(y.roth_conversion),
                taxable_deposit: today(y.taxable_deposit),
                leftover_spent: today(y.leftover_spent),
                gain_harvest: today(y.gain_harvest),
                employer_match: today(y.employer_match),
                pre_tax_deferral: today(y.pre_tax_deferral),
                roth_401k: today(y.roth_401k),
                roth_ira: today(y.roth_ira),
                debt_payment: today(y.debt_payment),
                debt_balance: today(y.debt_balance),
                social_security: today(y.social_security_benefit),
                other_income: today(y.other_income),
                cash_flow: today(y.cash_flow),
                required_minimum_distribution: today(y.required_minimum_distribution),
                net_spending: y.net_spending_today,
                required_spending: scenario
                    .options
                    .required_spending
                    .get(n)
                    .copied()
                    .unwrap_or(0.0),
                ordinary_taxable_income: today(y.ordinary_taxable_income),
                capital_gains: today(y.capital_gains),
                federal_income_tax: today(y.ordinary_income_tax),
                capital_gains_tax: today(y.capital_gains_tax),
                state_income_tax: today(y.state_income_tax),
                early_withdrawal_penalty: today(y.early_withdrawal_penalty),
                medicare: today(y.medicare_premium),
                aca_premium: today(y.aca_premium),
                irmaa_tier: y.irmaa_tier,
                magi: today(y.magi),
                inflation_factor: gamma,
            }
        })
        .collect();
    let sum = |f: fn(&TaxOptimizedYear) -> f64| years.iter().map(f).sum::<f64>();
    TaxOptimizedPlan {
        objective,
        status: status_label(schedule.status).into(),
        converged: schedule.convergence == Convergence::Converged,
        iterations: schedule.iterations,
        infeasible_reason: None,
        starting_accounts: *accounts,
        social_security_claim_age: schedule.claim_age,
        first_year_spending: schedule.first_year_spending(),
        bequest_after_tax: schedule.bequest_after_tax_today,
        lifetime_spending: schedule.lifetime_spending_today,
        lifetime_federal_tax: sum(|y| y.federal_income_tax + y.capital_gains_tax),
        lifetime_state_tax: sum(|y| y.state_income_tax),
        lifetime_medicare: sum(|y| y.medicare),
        lifetime_aca: sum(|y| y.aca_premium),
        lifetime_early_withdrawal_penalty: sum(|y| y.early_withdrawal_penalty),
        lifetime_roth_conversions: sum(|y| y.roth_conversion),
        years,
        baseline: None,
        retirement_age: scenario.start_age() + working_years as u32,
        retirement_age_options: Vec::new(),
        home: None,
        contribution_caps: Vec::new(),
    }
}

/// The plan's linked home purchase as solved, if any.
fn home_summary(plan: &RetirementPlan, scenario: &Scenario) -> Result<Option<HomePlanSummary>> {
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    let terms = home_terms(
        &settings,
        scenario.start_year,
        scenario.person.birth_year,
        scenario.years(),
        settings
            .markets
            .inflation
            .unwrap_or(plan.investment.inflation_rate),
    )?;
    Ok(terms.map(|t| t.summary(scenario.start_year, scenario.start_age())))
}

/// Solves the conventional-strategy baseline of `scenario`, claiming Social
/// Security at `stated_claim_age` when the plan leaves the age to the optimizer.
fn baseline(scenario: &Scenario, stated_claim_age: f64) -> Result<TaxPlanBaseline> {
    let mut naive = scenario.clone();
    naive.options.withdrawal_order = WithdrawalOrder::TaxableFirst;
    naive.options.allow_roth_conversions = false;
    // The comparison point, not the answer: the loop alone is enough.
    naive.options.polish = false;
    if let Some(ss) = naive.social_security.as_mut() {
        if matches!(ss.claim, ClaimStrategy::Optimize) {
            ss.claim = ClaimStrategy::AtAge(stated_claim_age);
        }
    }
    match optimize(&naive) {
        Ok(schedule) => {
            let today = |n: usize, nominal: f64| nominal / naive.gamma(n);
            let total = |f: &dyn Fn(&wealthfolio_optimization_engine::YearPlan) -> f64| {
                schedule
                    .years
                    .iter()
                    .enumerate()
                    .map(|(n, y)| today(n, f(y)))
                    .sum::<f64>()
            };
            Ok(TaxPlanBaseline {
                status: status_label(schedule.status).into(),
                first_year_spending: schedule.first_year_spending(),
                bequest_after_tax: schedule.bequest_after_tax_today,
                lifetime_federal_tax: total(&|y| y.ordinary_income_tax + y.capital_gains_tax),
                lifetime_state_tax: total(&|y| y.state_income_tax),
                lifetime_medicare: total(&|y| y.medicare_premium),
                lifetime_aca: total(&|y| y.aca_premium),
                lifetime_early_withdrawal_penalty: total(&|y| y.early_withdrawal_penalty),
            })
        }
        Err(EngineError::Infeasible(_)) => Ok(TaxPlanBaseline {
            status: "infeasible".into(),
            first_year_spending: 0.0,
            bequest_after_tax: 0.0,
            lifetime_federal_tax: 0.0,
            lifetime_state_tax: 0.0,
            lifetime_medicare: 0.0,
            lifetime_aca: 0.0,
            lifetime_early_withdrawal_penalty: 0.0,
        }),
        Err(EngineError::InvalidInput(message)) => Err(invalid(message)),
        Err(other) => Err(crate::errors::Error::Unexpected(format!(
            "Tax optimizer baseline failed: {other}"
        ))),
    }
}

/// How the success-probability paths are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SuccessPathMode {
    /// Every historical window since 1928.
    Historical,
    /// Resampled historical years (Monte Carlo).
    Bootstrap,
    /// Years drawn from a correlated lognormal fitted to the history (Monte
    /// Carlo).
    Lognormal,
}

/// Chance that the planned expenses are funded, re-solving the plan on each
/// return and inflation path (today's money).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxPlanSuccess {
    pub mode: SuccessPathMode,
    pub net_spending: f64,
    pub paths: usize,
    pub funded: usize,
    pub success_rate: f64,
    /// 10th, 50th and 90th percentile after-tax estates (unfunded paths as 0).
    pub bequest_percentiles: [f64; 3],
    /// Labels (start year or sample) of the paths that ran out.
    pub failed_paths: Vec<String>,
    /// 10th, 50th and 90th percentile ages at death, when each run drew a
    /// random lifespan.
    #[serde(default)]
    pub lifespans: Option<[u32; 3]>,
}

/// Simulated (Monte Carlo) paths drawn unless the plan sets how many, and the
/// range it may ask for; the regret check solves each path once per amount.
const DEFAULT_SIMULATIONS: u32 = 100;
const DEFAULT_REGRET_SIMULATIONS: u32 = 30;
const MAX_SIMULATIONS: u32 = 2_000;
const MIN_SIMULATIONS: u32 = 10;
const BOOTSTRAP_SEED: u64 = 20_260_101;
/// Seed for the random lifespans, apart from the markets'.
const LIFESPAN_SEED: u64 = 19_280_101;

/// Success probability of the plan's planned expenses. CPU-heavy (minutes):
/// call it off the async workers.
///
/// `retirement_age` overrides the plan's target (the age an earliest-age
/// search found). Under that goal the spending tested is the least spending in
/// the planned shape that covers every year's must-have expenses.
pub fn tax_plan_success(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    mode: SuccessPathMode,
    retirement_age: Option<u32>,
) -> Result<TaxPlanSuccess> {
    use wealthfolio_optimization_engine::simulation::success_probability_of_runs;
    let plan = &match retirement_age {
        Some(age) => retiring_at(plan, age),
        None => plan.clone(),
    };
    let sim = success_paths(plan, accounts, as_of, mode, true)?;
    let net_spending = tested_spending(plan, &sim.scenario, as_of);
    let report = success_probability_of_runs(&sim.runs(), net_spending, solver_threads())
        .map_err(|e| crate::errors::Error::Unexpected(format!("Success analysis failed: {e}")))?;
    Ok(TaxPlanSuccess {
        mode,
        net_spending,
        paths: report.paths,
        funded: report.funded,
        success_rate: report.success_rate,
        bequest_percentiles: report.bequest_percentiles,
        failed_paths: report
            .outcomes
            .iter()
            .filter(|o| !o.funded)
            .map(|o| o.label.clone())
            .collect(),
        lifespans: sim.lifespan_percentiles(),
    })
}

/// The most spending that succeeds on at least the target share of paths,
/// and the whole spending/success trade-off. One solve per path. Today's money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxPlanSafeSpending {
    pub mode: SuccessPathMode,
    /// Share of paths that must fund the spending (0.9 = 90%).
    pub target_rate: f64,
    /// First-year spending (following the planned shape) at the target rate.
    pub spending: f64,
    pub paths: usize,
    /// The plan's spending and the share of these paths that fund it.
    pub planned_spending: f64,
    pub planned_success_rate: f64,
    /// Spending and the share of paths that fund it, from low to high spending.
    pub frontier: Vec<SuccessFrontierPoint>,
    /// The most each path can spend, in path order (historical paths by start
    /// year).
    #[serde(default)]
    pub path_spending: Vec<PathSpending>,
    /// As in [`TaxPlanSuccess::lifespans`].
    #[serde(default)]
    pub lifespans: Option<[u32; 3]>,
}

/// The most first-year spending one market path funds (today's money).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathSpending {
    /// The path's first historical year, or its sample number.
    pub label: String,
    pub spending: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuccessFrontierPoint {
    pub spending: f64,
    pub success_rate: f64,
}

/// The most the plan can spend while succeeding on at least `target_rate` of
/// the paths, with no estate required at the end (like the success check).
/// CPU-heavy (minutes): call it off the async workers.
pub fn tax_plan_safe_spending(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    mode: SuccessPathMode,
    retirement_age: Option<u32>,
    target_rate: f64,
) -> Result<TaxPlanSafeSpending> {
    use wealthfolio_optimization_engine::simulation::{
        spending_at_success_rate, spending_capacities_of_runs, success_frontier,
    };
    if !(0.01..=1.0).contains(&target_rate) {
        return Err(invalid(
            "The target success rate must be between 1% and 100%",
        ));
    }
    let plan = &match retirement_age {
        Some(age) => retiring_at(plan, age),
        None => plan.clone(),
    };
    let sim = success_paths(plan, accounts, as_of, mode, true)?;
    let capacities = spending_capacities_of_runs(&sim.runs(), solver_threads())
        .map_err(|e| crate::errors::Error::Unexpected(format!("Safe spending failed: {e}")))?;
    let planned_spending = tested_spending(plan, &sim.scenario, as_of);
    let funded = capacities
        .iter()
        .filter(|c| c.max_spending >= planned_spending - 1.0)
        .count();
    Ok(TaxPlanSafeSpending {
        mode,
        target_rate,
        spending: spending_at_success_rate(&capacities, target_rate),
        paths: capacities.len(),
        planned_spending,
        planned_success_rate: funded as f64 / capacities.len().max(1) as f64,
        frontier: success_frontier(&capacities)
            .into_iter()
            .map(|(spending, success_rate)| SuccessFrontierPoint {
                spending,
                success_rate,
            })
            .collect(),
        path_spending: capacities
            .into_iter()
            .map(|c| PathSpending {
                label: c.label,
                spending: c.max_spending,
            })
            .collect(),
        lifespans: sim.lifespan_percentiles(),
    })
}

/// The plan and the market paths of a success analysis; with random
/// lifespans, each path also has its own plan, ending at its age at death.
struct SuccessPaths {
    /// The plan to its planning horizon.
    scenario: Scenario,
    paths: Vec<wealthfolio_optimization_engine::simulation::ReturnPath>,
    /// The plan at each age at death drawn, and each path's plan and age.
    lifetimes: Vec<Scenario>,
    lifetime_of: Vec<usize>,
    lifespans: Vec<u32>,
}

impl SuccessPaths {
    fn runs(&self) -> Vec<wealthfolio_optimization_engine::simulation::Run<'_>> {
        use wealthfolio_optimization_engine::simulation::Run;
        self.paths
            .iter()
            .enumerate()
            .map(|(k, path)| Run {
                scenario: self
                    .lifetime_of
                    .get(k)
                    .map_or(&self.scenario, |&i| &self.lifetimes[i]),
                path,
            })
            .collect()
    }

    /// 10th, 50th and 90th percentile ages at death drawn, if any were.
    fn lifespan_percentiles(&self) -> Option<[u32; 3]> {
        if self.lifespans.is_empty() {
            return None;
        }
        let mut ages = self.lifespans.clone();
        ages.sort_unstable();
        let at = |p: f64| ages[((ages.len() - 1) as f64 * p).round() as usize];
        Some([at(0.1), at(0.5), at(0.9)])
    }
}

/// The plan's scenario and its return paths for a success analysis. With
/// `lifespans` and a simulated `mode`, each run draws an age at death from the
/// plan's random lifespans, given the person lives to retire, and plans to it.
fn success_paths(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    mode: SuccessPathMode,
    lifespans: bool,
) -> Result<SuccessPaths> {
    use wealthfolio_optimization_engine::mortality::sample_lifespans;
    use wealthfolio_optimization_engine::simulation::{
        bootstrap_paths, historical_paths, lognormal_paths,
    };
    let scenario = build_tax_optimized_scenario(plan, accounts, as_of)?;
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    let simulations = simulation_count(settings.simulation_count, DEFAULT_SIMULATIONS);
    let current_age = scenario.start_age();
    let alive_at = plan.personal.target_retirement_age.max(current_age);
    let drawn: Vec<u32> = match (settings.random_lifespans, mode) {
        (Some(_), SuccessPathMode::Historical) | (None, _) => Vec::new(),
        (Some(_), _) if !lifespans => Vec::new(),
        (Some(random), _) => sample_lifespans(
            random.table,
            random.sex,
            alive_at,
            simulations,
            LIFESPAN_SEED,
        )
        .into_iter()
        // At least a year of retirement.
        .map(|age| age.max(alive_at + 1))
        .collect(),
    };

    let planned_years = scenario.years();
    let last_age = drawn
        .iter()
        .copied()
        .fold(plan.personal.planning_horizon_age, u32::max);
    let years = planned_years + (last_age - plan.personal.planning_horizon_age) as usize;
    let mixes = yearly_mixes(
        &settings,
        accounts,
        current_age,
        plan.personal.planning_horizon_age,
        years,
    );
    let fee = plan.investment.annual_investment_fee_rate.max(0.0);
    let paths = match mode {
        SuccessPathMode::Historical => historical_paths(years, &mixes, fee),
        SuccessPathMode::Bootstrap => {
            bootstrap_paths(years, &mixes, fee, simulations, BOOTSTRAP_SEED)
        }
        SuccessPathMode::Lognormal => {
            lognormal_paths(years, &mixes, fee, simulations, BOOTSTRAP_SEED)
        }
    };
    if paths.is_empty() {
        return Err(invalid(format!(
            "The plan spans {years} years, longer than the market history"
        )));
    }

    // One plan per age at death drawn.
    let mut ages: Vec<u32> = drawn.clone();
    ages.sort_unstable();
    ages.dedup();
    let lifetimes = ages
        .iter()
        .map(|&age| {
            let mut lifetime = plan.clone();
            lifetime.personal.planning_horizon_age = age;
            build_tax_optimized_scenario(&lifetime, accounts, as_of)
        })
        .collect::<Result<Vec<_>>>()?;
    let lifetime_of = drawn
        .iter()
        .map(|age| ages.binary_search(age).unwrap_or_default())
        .collect();
    Ok(SuccessPaths {
        scenario,
        paths,
        lifetimes,
        lifetime_of,
        lifespans: drawn,
    })
}

/// Simulated paths to draw: the plan's count, kept in range, or `default`.
fn simulation_count(count: Option<u32>, default: u32) -> usize {
    count
        .unwrap_or(default)
        .clamp(MIN_SIMULATIONS, MAX_SIMULATIONS) as usize
}

/// First-year spending (today's money) a success analysis tests: the planned
/// expenses, or under the earliest-age goal the least spending in the plan's
/// shape that covers the minimum expenses every year.
fn tested_spending(plan: &RetirementPlan, scenario: &Scenario, as_of: NaiveDate) -> f64 {
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    match (settings.goal, scenario.objective) {
        (TaxPlanGoal::EarliestAge, _) => {
            let profile = &scenario.options.spending_profile;
            scenario
                .options
                .required_spending
                .iter()
                .enumerate()
                .map(|(n, r)| r / profile.get(n).copied().unwrap_or(1.0))
                .fold(0.0, f64::max)
        }
        (_, Objective::MaxBequest { net_spending }) => net_spending,
        (_, Objective::MaxSpending { .. }) => planned_first_year_spending(plan, as_of),
    }
}

/// The first year's planned expenses in today's money.
fn planned_first_year_spending(plan: &RetirementPlan, as_of: NaiveDate) -> f64 {
    let mut maxed = plan.clone();
    if let Some(settings) = maxed.tax_optimizer.as_mut() {
        settings.objective = TaxOptimizerObjective::MaxBequest;
    }
    let accounts = TaxOptimizerAccounts::default();
    match build_tax_optimized_scenario(&maxed, &accounts, as_of).map(|s| s.objective) {
        Ok(Objective::MaxBequest { net_spending }) => net_spending,
        _ => 0.0,
    }
}

/// Latest retirement age the earliest-age search tries (unless the plan's
/// target is later).
const LATEST_SEARCHED_RETIREMENT_AGE: u32 = 70;

/// Solver threads for multi-solve analyses, leaving a core for the app.
fn solver_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1).clamp(1, 4))
        .unwrap_or(1)
}

/// `plan` retiring at `age`.
fn retiring_at(plan: &RetirementPlan, age: u32) -> RetirementPlan {
    let mut plan = plan.clone();
    plan.personal.target_retirement_age = age;
    plan
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

/// Solves the tax-optimized plan for the plan's goal. CPU-heavy (seconds to
/// minutes): call it off the async workers.
pub fn optimize_tax_plan(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
) -> Result<TaxOptimizedPlan> {
    match plan.tax_optimizer.clone().unwrap_or_default().goal {
        TaxPlanGoal::FixedAge => solve_plan(plan, accounts, as_of),
        TaxPlanGoal::EarliestAge => earliest_retirement(plan, accounts, as_of),
    }
}

/// Solves every retirement age from now to the latest searched age for the
/// most spending each sustains (must-have expenses as the floor), then the
/// earliest feasible age in full. Without a feasible age the latest one is
/// solved, so the plan explains what is out of reach.
fn earliest_retirement(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
) -> Result<TaxOptimizedPlan> {
    let mut plan = plan.clone();
    if let Some(settings) = plan.tax_optimizer.as_mut() {
        settings.objective = TaxOptimizerObjective::MaxSpending;
    }
    let (birth_year, _) = birth_year_month(&plan, as_of);
    let current_age = u32::try_from(as_of.year() - birth_year)
        .map_err(|_| invalid("Birth date is in the future"))?;
    let latest = plan.personal.planning_horizon_age.saturating_sub(1).min(
        plan.personal
            .target_retirement_age
            .max(LATEST_SEARCHED_RETIREMENT_AGE),
    );
    if latest < current_age {
        return Err(invalid("Planning horizon must be after the current age"));
    }
    let required = build_tax_optimized_scenario(&plan, accounts, as_of)?
        .options
        .required_spending;
    if required.iter().all(|r| *r <= 0.0) {
        return Err(invalid(
            "Mark your must-have spending: the search finds the earliest age that covers it",
        ));
    }

    let ages: Vec<u32> = (current_age..=latest).collect();
    let options = parallel_map(&ages, solver_threads(), |age| {
        retirement_age_option(&plan, accounts, as_of, *age)
    })?;
    let chosen = options
        .iter()
        .find(|o| o.feasible)
        .map_or(latest, |o| o.age);
    let mut result = solve_plan(&retiring_at(&plan, chosen), accounts, as_of)?;
    result.retirement_age_options = options;
    Ok(result)
}

/// The most spending `plan` sustains retiring at `age` (loop only, no polish).
fn retirement_age_option(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    age: u32,
) -> Result<RetirementAgeOption> {
    let mut scenario = build_tax_optimized_scenario(&retiring_at(plan, age), accounts, as_of)?;
    scenario.options.polish = false;
    let retired_from = scenario.working_years.len();
    let required_spending = average(
        scenario
            .options
            .required_spending
            .get(retired_from..)
            .unwrap_or_default(),
    );
    match optimize(&scenario) {
        Ok(schedule) => {
            let spending: Vec<f64> = schedule
                .years
                .iter()
                .skip(retired_from)
                .map(|y| y.net_spending_today)
                .collect();
            Ok(RetirementAgeOption {
                age,
                feasible: true,
                retirement_spending: average(&spending),
                required_spending,
                bequest_after_tax: schedule.bequest_after_tax_today,
            })
        }
        Err(EngineError::Infeasible(_)) => Ok(RetirementAgeOption {
            age,
            feasible: false,
            retirement_spending: 0.0,
            required_spending,
            bequest_after_tax: 0.0,
        }),
        Err(EngineError::InvalidInput(message)) => Err(invalid(message)),
        Err(other) => Err(crate::errors::Error::Unexpected(format!(
            "Tax optimizer failed: {other}"
        ))),
    }
}

/// Maps `items` on up to `threads` threads, keeping their order.
fn parallel_map<T: Sync, R: Send>(
    items: &[T],
    threads: usize,
    f: impl Fn(&T) -> Result<R> + Sync,
) -> Result<Vec<R>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let (next, f) = (&next, &f);
    let mut indexed: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads.clamp(1, items.len().max(1)))
            .map(|_| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(i) else {
                            return Ok::<_, crate::errors::Error>(done);
                        };
                        done.push((i, f(item)?));
                    }
                })
            })
            .collect();
        let mut all = Vec::with_capacity(items.len());
        for handle in handles {
            let batch = handle.join().map_err(|_| {
                crate::errors::Error::Unexpected("A tax optimizer thread panicked".into())
            })??;
            all.extend(batch);
        }
        Ok::<_, crate::errors::Error>(all)
    })?;
    indexed.sort_by_key(|(i, _)| *i);
    Ok(indexed.into_iter().map(|(_, r)| r).collect())
}

/// Solves `plan` at its target retirement age, with the conventional baseline.
fn solve_plan(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
) -> Result<TaxOptimizedPlan> {
    let scenario = build_tax_optimized_scenario(plan, accounts, as_of)?;
    let objective = plan.tax_optimizer.clone().unwrap_or_default().objective;
    match optimize(&scenario) {
        Ok(schedule) => {
            let mut result = playbook(&scenario, accounts, objective, &schedule);
            result.home = home_summary(plan, &scenario)?;
            result.contribution_caps = contribution_caps(plan, &scenario);
            let settings = plan.tax_optimizer.clone().unwrap_or_default();
            let stated_claim_age =
                planned_social_security(social_security_stream(plan, &settings), &settings)
                    .map_or(MAX_CLAIM_AGE, |(age, _)| age);
            result.baseline = Some(baseline(&scenario, stated_claim_age)?);
            Ok(result)
        }
        Err(EngineError::Infeasible(reason)) => Ok(TaxOptimizedPlan {
            objective,
            status: "infeasible".into(),
            converged: false,
            iterations: 0,
            infeasible_reason: Some(reason),
            starting_accounts: *accounts,
            social_security_claim_age: None,
            first_year_spending: 0.0,
            bequest_after_tax: 0.0,
            lifetime_spending: 0.0,
            lifetime_federal_tax: 0.0,
            lifetime_state_tax: 0.0,
            lifetime_medicare: 0.0,
            lifetime_aca: 0.0,
            lifetime_early_withdrawal_penalty: 0.0,
            lifetime_roth_conversions: 0.0,
            years: Vec::new(),
            baseline: None,
            retirement_age: scenario.start_age() + scenario.working_years.len() as u32,
            retirement_age_options: Vec::new(),
            home: home_summary(plan, &scenario)?,
            contribution_caps: contribution_caps(plan, &scenario),
        }),
        Err(EngineError::InvalidInput(message)) => Err(invalid(message)),
        Err(other) => Err(crate::errors::Error::Unexpected(format!(
            "Tax optimizer failed: {other}"
        ))),
    }
}

/// Yearly spending (today's money) added to the floor or the planned spending
/// when costing those choices.
const CHOICE_SPENDING_STEP: f64 = 1_000.0;
/// Estate (today's money) added to the bequest floor when costing it.
const CHOICE_ESTATE_STEP: f64 = 10_000.0;

/// One choice nudged, and what that does to the plan's goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceCost {
    /// `minimumExpenses` and `spending` (+ `change` a year), `bequestFloor`
    /// (+ `change`), `retireLater` and `retireEarlier` (one year),
    /// `homePurchase` (without the linked purchase), `conversionCap` (the
    /// `change` cap lifted), `claimAge` (the optimizer picks instead of `change`).
    pub choice: String,
    pub change: f64,
    /// Change in the goal, today's money: first-year spending per year when
    /// maximizing spending, the after-tax estate when maximizing it. `None`
    /// when the nudged plan has no solution.
    pub effect: Option<f64>,
}

/// What each choice costs the plan's goal, by re-solving with one nudged at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxPlanChoiceCosts {
    pub objective: TaxOptimizerObjective,
    /// The goal's value as planned, from the same quick solve as the nudges.
    pub base: f64,
    pub costs: Vec<ChoiceCost>,
}

/// The value the plan maximizes, in today's money.
fn goal_value(objective: &Objective, schedule: &Schedule) -> f64 {
    match objective {
        Objective::MaxSpending { .. } => schedule.first_year_spending(),
        Objective::MaxBequest { .. } => schedule.bequest_after_tax_today,
    }
}

/// Re-solves the plan with each choice nudged (quick solves, no polish) and
/// reports how its goal moves: the dollar cost or value of the minimum
/// expenses, the spending or bequest target, the retirement age, a linked home
/// purchase, a Roth conversion cap and a fixed Social Security age.
/// CPU-heavy (a minute or more): call it off the async workers.
pub fn tax_plan_choice_costs(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    retirement_age: Option<u32>,
) -> Result<TaxPlanChoiceCosts> {
    let plan = fixed_age_plan(plan, retirement_age);
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    let quick = |plan: &RetirementPlan| quick_scenario(plan, accounts, as_of);
    let base = quick(&plan)?;
    let mut variants: Vec<(&str, f64, Scenario)> = Vec::new();

    let mut s = base.clone();
    for floor in s.options.required_spending.iter_mut() {
        *floor += CHOICE_SPENDING_STEP;
    }
    variants.push(("minimumExpenses", CHOICE_SPENDING_STEP, s));
    let mut s = base.clone();
    match base.objective {
        Objective::MaxBequest { net_spending } => {
            s.objective = Objective::MaxBequest {
                net_spending: net_spending + CHOICE_SPENDING_STEP,
            };
            variants.push(("spending", CHOICE_SPENDING_STEP, s));
        }
        Objective::MaxSpending { bequest_floor } => {
            s.objective = Objective::MaxSpending {
                bequest_floor: bequest_floor + CHOICE_ESTATE_STEP,
            };
            variants.push(("bequestFloor", CHOICE_ESTATE_STEP, s));
        }
    }

    let age = plan.personal.target_retirement_age;
    if plan.personal.current_annual_salary.unwrap_or(0.0) > 0.0 && age > base.start_age() {
        if age + 1 < plan.personal.planning_horizon_age {
            variants.push(("retireLater", 1.0, quick(&retiring_at(&plan, age + 1))?));
        }
        variants.push(("retireEarlier", 1.0, quick(&retiring_at(&plan, age - 1))?));
    }
    if settings.home_purchase.is_some() {
        let mut without = plan.clone();
        if let Some(s) = without.tax_optimizer.as_mut() {
            s.home_purchase = None;
            s.sell_current_home = false;
        }
        variants.push(("homePurchase", 0.0, quick(&without)?));
    }
    if let Some(cap) = base.options.max_roth_conversion {
        let mut s = base.clone();
        s.options.max_roth_conversion = None;
        variants.push(("conversionCap", cap, s));
    }
    if let Some(ClaimStrategy::AtAge(claim)) = base.social_security.as_ref().map(|ss| ss.claim) {
        let mut s = base.clone();
        if let Some(ss) = s.social_security.as_mut() {
            ss.claim = ClaimStrategy::Optimize;
        }
        variants.push(("claimAge", claim, s));
    }

    let mut scenarios = vec![base];
    scenarios.extend(variants.iter().map(|(_, _, s)| s.clone()));
    let values = goal_values(&scenarios)?;
    let base_value = values[0].ok_or_else(|| {
        invalid("The plan has no solution as it stands; fix that before costing its choices")
    })?;
    Ok(TaxPlanChoiceCosts {
        objective: settings.objective,
        base: base_value,
        costs: variants
            .iter()
            .zip(&values[1..])
            .map(|((choice, change, _), value)| ChoiceCost {
                choice: (*choice).to_string(),
                change: *change,
                effect: value.map(|v| v - base_value),
            })
            .collect(),
    })
}

/// `plan` at `retirement_age` (when given). An earliest-age plan is weighed
/// at its age, maximizing spending.
fn fixed_age_plan(plan: &RetirementPlan, retirement_age: Option<u32>) -> RetirementPlan {
    let mut plan = match retirement_age {
        Some(age) => retiring_at(plan, age),
        None => plan.clone(),
    };
    if let Some(settings) = plan.tax_optimizer.as_mut() {
        if settings.goal == TaxPlanGoal::EarliestAge {
            settings.goal = TaxPlanGoal::FixedAge;
            settings.objective = TaxOptimizerObjective::MaxSpending;
        }
    }
    plan
}

/// The plan's scenario for a quick solve (no polish).
fn quick_scenario(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
) -> Result<Scenario> {
    let mut s = build_tax_optimized_scenario(plan, accounts, as_of)?;
    s.options.polish = false;
    Ok(s)
}

/// Each scenario's goal value (see [`goal_value`]), solved in parallel;
/// `None` where it has no solution or the solver gives up on it (as Owl
/// counts a failed solve), so one hard case doesn't sink the rest.
fn goal_values(scenarios: &[Scenario]) -> Result<Vec<Option<f64>>> {
    parallel_map(scenarios, solver_threads(), |s| match optimize(s) {
        Ok(schedule) => Ok(Some(goal_value(&s.objective, &schedule))),
        Err(EngineError::Infeasible(_)) | Err(EngineError::Solver(_)) => Ok(None),
        Err(EngineError::InvalidInput(message)) => Err(invalid(message)),
        Err(other) => Err(crate::errors::Error::Unexpected(format!(
            "Tax optimizer failed: {other}"
        ))),
    })
}

/// Return lost every year in the lower-returns stress, inflation added in the
/// inflation stress, and the share added to the expenses in the spending one.
const STRESS_RETURN_DRAG: f64 = 0.02;
const STRESS_INFLATION: f64 = 0.015;
const STRESS_EXPENSES: f64 = 0.10;
/// Share of Social Security still paid in the benefit-cut stress: what the
/// trustees project the program can pay once its trust fund runs out.
const STRESS_SOCIAL_SECURITY_PAID: f64 = 0.77;
/// Years added to the planning horizon in the longevity stress.
const STRESS_EXTRA_YEARS: u32 = 5;
/// Market history replayed from the first retirement year on.
const STRESS_RETIREMENT_MARKETS: [i32; 3] = [1929, 1966, 2000];

/// How the plan's goal moves under one shock it does not control.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StressCase {
    /// `lowerReturns`, `higherInflation`, `higherExpenses`,
    /// `socialSecurityCut`, `liveLonger` or `retireInto` (with `year`).
    pub stress: String,
    /// The historical year a `retireInto` stress replays from retirement on.
    pub year: Option<i32>,
    /// Change in the goal, today's money (as in [`ChoiceCost::effect`]);
    /// `None` when the plan has no solution under the shock.
    pub effect: Option<f64>,
}

/// The plan re-optimized under shocks it does not control: the optimizer
/// adapts conversions, withdrawals and spending to each one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxPlanStressTests {
    pub objective: TaxOptimizerObjective,
    /// The goal's value as planned, from the same quick solve as the stresses.
    pub base: f64,
    pub stresses: Vec<StressCase>,
}

/// Re-solves the plan (quick solves, no polish) under lower returns, higher
/// inflation, higher expenses, a Social Security cut, a longer life, and the
/// markets of 1929, 1966 and 2000 from the first retirement year on, and
/// reports how its goal moves. CPU-heavy (a minute or more): call it off the
/// async workers.
pub fn tax_plan_stress_tests(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    retirement_age: Option<u32>,
) -> Result<TaxPlanStressTests> {
    let plan = fixed_age_plan(plan, retirement_age);
    let settings = plan.tax_optimizer.clone().unwrap_or_default();
    let base = quick_scenario(&plan, accounts, as_of)?;
    let years = base.years();
    let inflation_in = |n: usize| {
        base.rates
            .annual_inflation
            .get(n)
            .or(base.rates.annual_inflation.last())
            .copied()
            .unwrap_or(base.rates.inflation)
    };
    let mut variants: Vec<(&str, Option<i32>, Scenario)> = Vec::new();

    let mut s = base.clone();
    s.rates.account_returns = (0..years)
        .map(|n| base.rates.for_year(n).map(|r| r - STRESS_RETURN_DRAG))
        .collect();
    variants.push(("lowerReturns", None, s));

    let mut s = base.clone();
    s.rates.annual_inflation = (0..years)
        .map(|n| inflation_in(n) + STRESS_INFLATION)
        .collect();
    variants.push(("higherInflation", None, s));

    let mut s = base.clone();
    for floor in s.options.required_spending.iter_mut() {
        *floor *= 1.0 + STRESS_EXPENSES;
    }
    if let Objective::MaxBequest { net_spending } = s.objective {
        s.objective = Objective::MaxBequest {
            net_spending: net_spending * (1.0 + STRESS_EXPENSES),
        };
    }
    variants.push(("higherExpenses", None, s));

    if base.social_security.is_some() {
        let mut s = base.clone();
        if let Some(ss) = s.social_security.as_mut() {
            ss.monthly_pia *= STRESS_SOCIAL_SECURITY_PAID;
        }
        variants.push(("socialSecurityCut", None, s));
    }

    let mut longer = plan.clone();
    longer.personal.planning_horizon_age += STRESS_EXTRA_YEARS;
    variants.push((
        "liveLonger",
        None,
        quick_scenario(&longer, accounts, as_of)?,
    ));

    // Before retirement the planned markets; from the first retirement year,
    // the markets of a bad historical start, each account earning its mix.
    let retired = base.working_years.len();
    if retired < years {
        let mixes = yearly_mixes(
            &settings,
            accounts,
            base.start_age(),
            plan.personal.planning_horizon_age,
            years,
        );
        let fee = plan.investment.annual_investment_fee_rate.max(0.0);
        for start in STRESS_RETIREMENT_MARKETS {
            let Some(history) = historical_sequence(start, years - retired) else {
                continue;
            };
            let mut s = base.clone();
            s.rates.account_returns = (0..years)
                .map(|n| match n.checked_sub(retired) {
                    Some(k) => account_returns(&mixes[n], &history[k], fee),
                    None => base.rates.for_year(n),
                })
                .collect();
            s.rates.annual_inflation = (0..years)
                .map(|n| match n.checked_sub(retired) {
                    Some(k) => history[k].inflation,
                    None => inflation_in(n),
                })
                .collect();
            variants.push(("retireInto", Some(start), s));
        }
    }

    let mut scenarios = vec![base.clone()];
    scenarios.extend(variants.iter().map(|(_, _, s)| s.clone()));
    let values = goal_values(&scenarios)?;
    let base_value = values[0].ok_or_else(|| {
        invalid("The plan has no solution as it stands; fix that before stress testing it")
    })?;
    Ok(TaxPlanStressTests {
        objective: settings.objective,
        base: base_value,
        stresses: variants
            .iter()
            .zip(&values[1..])
            .map(|((stress, year, _), value)| StressCase {
                stress: (*stress).to_string(),
                year: *year,
                effect: value.map(|v| v - base_value),
            })
            .collect(),
    })
}

/// Market paths the conversion regret check uses: every third historical
/// window (simulated ones: the plan's regret count).
const REGRET_HISTORICAL_STEP: usize = 3;

/// One first-year Roth conversion amount weighed across market paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionOption {
    /// Conversion this year, today's money.
    pub amount: f64,
    /// How much less after-tax estate it leaves than the best amount on the
    /// same path, on average and at worst (today's money).
    pub mean_regret: f64,
    pub worst_regret: f64,
    /// Share of paths on which it is the best amount.
    pub best_share: f64,
    /// Share of paths on which the planned spending is still funded.
    pub funded_share: f64,
}

/// Whether this year's recommended Roth conversion holds up if markets turn
/// out differently (Owl's conversion regret): each amount is pinned, the rest
/// of the plan re-optimized on every path, and the estates compared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxPlanConversionRegret {
    pub mode: SuccessPathMode,
    pub paths: usize,
    /// The plan's first-year conversion, today's money.
    pub recommended: f64,
    /// Spending held fixed on every path, today's money.
    pub net_spending: f64,
    pub options: Vec<ConversionOption>,
}

/// Amounts to weigh around the recommended conversion (within any cap).
fn conversion_grid(recommended: f64, cap: Option<f64>) -> Vec<f64> {
    let mut grid: Vec<f64> = if recommended >= 1_000.0 {
        [0.0, 0.5, 1.0, 1.5, 2.0]
            .iter()
            .map(|f| (recommended * f).round())
            .collect()
    } else {
        vec![0.0, 10_000.0, 25_000.0, 50_000.0]
    };
    if let Some(cap) = cap {
        for amount in grid.iter_mut() {
            *amount = amount.min(cap);
        }
    }
    grid.dedup();
    grid
}

/// Re-solves the plan at its planned spending on each market path with this
/// year's Roth conversion pinned to amounts around the recommended one, and
/// reports each amount's regret. CPU-heavy (minutes): call it off the async
/// workers.
pub fn tax_plan_conversion_regret(
    plan: &RetirementPlan,
    accounts: &TaxOptimizerAccounts,
    as_of: NaiveDate,
    mode: SuccessPathMode,
    retirement_age: Option<u32>,
) -> Result<TaxPlanConversionRegret> {
    let plan = &match retirement_age {
        Some(age) => retiring_at(plan, age),
        None => plan.clone(),
    };
    // Each run to the planning horizon: a random lifespan would blur how the
    // conversion amounts compare on the same markets.
    let mut fixed = plan.clone();
    if let Some(settings) = fixed.tax_optimizer.as_mut() {
        settings.simulation_count = Some(simulation_count(
            settings.regret_simulation_count,
            DEFAULT_REGRET_SIMULATIONS,
        ) as u32);
    }
    let SuccessPaths {
        scenario, paths, ..
    } = success_paths(&fixed, accounts, as_of, mode, false)?;
    let paths: Vec<_> = match mode {
        SuccessPathMode::Historical => paths.into_iter().step_by(REGRET_HISTORICAL_STEP).collect(),
        SuccessPathMode::Bootstrap | SuccessPathMode::Lognormal => paths,
    };
    let net_spending = tested_spending(plan, &scenario, as_of);
    let base = {
        let mut s = scenario.clone();
        s.objective = Objective::MaxBequest { net_spending };
        s.options.polish = false;
        s.options.max_iterations = s.options.max_iterations.min(12);
        s
    };
    if !base.options.allow_roth_conversions {
        return Err(invalid("Roth conversions are turned off in this plan"));
    }
    let recommended = match optimize(&base) {
        Ok(schedule) => schedule
            .years
            .first()
            .map_or(0.0, |y| y.roth_conversion / base.gamma(0)),
        Err(EngineError::Infeasible(_)) => {
            return Err(invalid(
                "The planned spending can't be funded as it stands; fix that first",
            ))
        }
        Err(EngineError::InvalidInput(message)) => return Err(invalid(message)),
        Err(other) => {
            return Err(crate::errors::Error::Unexpected(format!(
                "Tax optimizer failed: {other}"
            )))
        }
    };
    let grid = conversion_grid(recommended, base.options.max_roth_conversion);
    let years = base.years();
    let jobs: Vec<(usize, usize)> = (0..paths.len())
        .flat_map(|p| (0..grid.len()).map(move |g| (p, g)))
        .collect();
    let estates = parallel_map(&jobs, solver_threads(), |(p, g)| {
        let mut s = base.clone();
        s.options.first_year_conversion = Some(grid[*g]);
        s.rates.account_returns = paths[*p].returns[..years].to_vec();
        s.rates.annual_inflation = paths[*p].inflation[..years].to_vec();
        match optimize(&s) {
            Ok(schedule) => Ok(Some(schedule.bequest_after_tax_today)),
            Err(EngineError::Infeasible(_)) => Ok(None),
            Err(other) => Err(crate::errors::Error::Unexpected(format!(
                "Conversion regret failed: {other}"
            ))),
        }
    })?;

    let mut regret = vec![Vec::with_capacity(paths.len()); grid.len()];
    let mut best = vec![0usize; grid.len()];
    let mut funded = vec![0usize; grid.len()];
    for estates in estates.chunks(grid.len()) {
        let values: Vec<f64> = estates.iter().map(|e| e.unwrap_or(0.0)).collect();
        let top = values.iter().copied().fold(f64::MIN, f64::max);
        if let Some(winner) = values.iter().position(|v| *v >= top) {
            best[winner] += 1;
        }
        for (g, value) in values.iter().enumerate() {
            regret[g].push(top - value);
            funded[g] += usize::from(estates[g].is_some());
        }
    }
    let n = paths.len().max(1) as f64;
    Ok(TaxPlanConversionRegret {
        mode,
        paths: paths.len(),
        recommended,
        net_spending,
        options: grid
            .iter()
            .enumerate()
            .map(|(g, amount)| ConversionOption {
                amount: *amount,
                mean_regret: average(&regret[g]),
                worst_regret: regret[g].iter().copied().fold(0.0, f64::max),
                best_share: best[g] as f64 / n,
                funded_share: funded[g] as f64 / n,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::home_purchase::LinkedHomePurchase;
    use crate::planning::retirement::end_items_at_home_purchase;
    use crate::planning::retirement::{
        ExpenseBucket, ExpenseBudget, InvestmentAssumptions, MortalityTable, PersonalProfile,
        RandomLifespans, StreamKind, TaxOptimizerDebt,
    };

    const AS_OF: NaiveDate = match NaiveDate::from_ymd_opt(2026, 10, 9) {
        Some(date) => date,
        None => panic!("valid date"),
    };

    fn plan() -> RetirementPlan {
        RetirementPlan {
            version: None,
            personal: PersonalProfile {
                birth_year_month: Some("1981-04".into()),
                current_age: 45,
                target_retirement_age: 55,
                planning_horizon_age: 92,
                current_annual_salary: Some(140_000.0),
                salary_growth_rate: Some(0.025),
            },
            expenses: ExpenseBudget {
                items: vec![
                    ExpenseBucket {
                        id: None,
                        label: Some("Living".into()),
                        monthly_amount: 5_000.0,
                        inflation_rate: None,
                        start_age: None,
                        end_age: None,
                        essential: Some(true),
                        paid_while_working: false,
                        yearly: false,
                        ends_at_home_purchase: false,
                    },
                    ExpenseBucket {
                        id: None,
                        label: Some("Travel".into()),
                        monthly_amount: 1_000.0,
                        inflation_rate: None,
                        start_age: Some(55),
                        end_age: Some(75),
                        essential: Some(false),
                        paid_while_working: false,
                        yearly: false,
                        ends_at_home_purchase: false,
                    },
                ],
            },
            income_streams: vec![RetirementIncomeStream {
                id: "ss".into(),
                label: "Social Security".into(),
                stream_type: StreamKind::DefinedBenefit,
                start_age: 67,
                adjust_for_inflation: true,
                annual_growth_rate: None,
                monthly_amount: Some(2_400.0),
                linked_account_id: None,
                current_value: None,
                monthly_contribution: None,
                accumulation_return: None,
                payout_rate: None,
                payout_mode: None,
                post_payout_return: None,
            }],
            investment: InvestmentAssumptions {
                pre_retirement_annual_return: 0.06,
                retirement_annual_return: 0.045,
                annual_investment_fee_rate: 0.005,
                annual_volatility: 0.12,
                inflation_rate: 0.025,
                monthly_contribution: 0.0,
                contribution_growth_rate: 0.025,
                glide_path: None,
            },
            tax: None,
            currency: "USD".into(),
            tax_optimizer: Some(TaxOptimizerSettings {
                social_security_stream_id: Some("ss".into()),
                pre_tax_contribution: 24_500.0,
                employer_contribution: 6_000.0,
                heirs_tax_rate: 0.3,
                dividend_yield: 0.015,
                ..TaxOptimizerSettings::default()
            }),
        }
    }

    fn accounts() -> TaxOptimizerAccounts {
        TaxOptimizerAccounts {
            taxable: 250_000.0,
            taxable_cost_basis: 180_000.0,
            tax_deferred: 400_000.0,
            roth: 60_000.0,
            hsa: 0.0,
            ..TaxOptimizerAccounts::default()
        }
    }

    #[test]
    fn scenario_follows_the_plan() {
        let s = build_tax_optimized_scenario(&plan(), &accounts(), AS_OF).expect("valid plan");
        assert_eq!(s.start_year, 2026);
        assert_eq!((s.person.birth_year, s.person.birth_month), (1981, 4));
        assert_eq!(s.years(), (92 - 45 + 1) as usize);

        // Ten working years (ages 45..54): taxable pay is salary minus the deferral,
        // and salary growing with inflation is flat in today's money.
        assert_eq!(s.working_years.len(), 10);
        let w = &s.working_years[0];
        assert!((w.wages - (140_000.0 - 24_500.0)).abs() < 1e-6);
        assert_eq!(w.tax_deferred_contribution, 24_500.0);
        assert_eq!(w.employer_contribution, 6_000.0);
        assert!((s.working_years[9].wages - w.wages).abs() < 1e-6);

        // Returns net of fees switch at retirement.
        assert!((s.rates.for_year(0)[0] - 0.055).abs() < 1e-12);
        assert!((s.rates.for_year(10)[0] - 0.040).abs() < 1e-12);

        // Social Security comes from the linked stream, not as other income.
        let ss = s.social_security.as_ref().expect("benefits");
        assert!(matches!(ss.claim, ClaimStrategy::AtAge(age) if age == 67.0));
        // Born 1981: full retirement age 67, so the PIA is the benefit itself.
        assert!((ss.monthly_pia - 2_400.0).abs() < 1e-6);
        assert!(s.options.other_taxable_income.iter().all(|v| *v == 0.0));

        // Travel adds $12k a year from 55 to 74: the spending shape follows it.
        let profile = &s.options.spending_profile;
        assert!((profile[0] - 1.0).abs() < 1e-9);
        assert!((profile[10] - 72_000.0 / 60_000.0).abs() < 1e-9);
        assert!((profile[30] - 1.0).abs() < 1e-9);
        assert!(matches!(
            s.objective,
            Objective::MaxBequest { net_spending } if (net_spending - 60_000.0).abs() < 1e-6
        ));
        // The floor is the minimum expenses plus the costs of owning.
        assert!(s
            .options
            .required_spending
            .iter()
            .all(|r| (r - 60_000.0).abs() < 1e-6));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn social_security_can_be_entered_without_a_stream() {
        let mut p = plan();
        p.income_streams.clear();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.social_security_stream_id = None;
            settings.social_security_monthly = 2_400.0;
            settings.social_security_claim_age = Some(67.0);
        }
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("valid plan");
        let ss = s.social_security.expect("benefits");
        assert!(matches!(ss.claim, ClaimStrategy::AtAge(age) if age == 67.0));
        assert!((ss.monthly_pia - 2_400.0).abs() < 1e-6);
        // Recent MAGI defaults to pay less the pre-tax deferral.
        assert!((s.options.prior_magi[0] - (140_000.0 - 24_500.0)).abs() < 1e-6);
    }

    #[test]
    fn claim_age_can_be_left_to_the_optimizer() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.optimize_claim_age = true;
        }
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("valid plan");
        let ss = s.social_security.expect("benefits");
        assert!(matches!(ss.claim, ClaimStrategy::Optimize));
    }

    #[test]
    fn without_a_linked_stream_benefits_are_ordinary_income() {
        let mut p = plan();
        p.tax_optimizer = None;
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("valid plan");
        assert!(s.social_security.is_none());
        // Age 67 is plan year 22.
        assert!((s.options.other_taxable_income[22] - 28_800.0).abs() < 1e-6);
        assert_eq!(s.options.other_taxable_income[21], 0.0);
    }

    #[test]
    fn optimized_contributions_use_gross_pay_and_carry_debts() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.optimize_contributions = true;
            settings.match_rate = 0.5;
            settings.match_cap = 0.06;
            settings.hsa_eligible = true;
            settings.debts = vec![TaxOptimizerDebt {
                label: "Car".into(),
                balance: 12_000.0,
                annual_rate: 0.07,
                annual_payment: 6_000.0,
            }];
        }
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("valid plan");
        // Gross pay: the optimizer takes deferrals out of it.
        assert!((s.working_years[0].wages - 140_000.0).abs() < 1e-6);
        let choice = s
            .options
            .optimize_contributions
            .as_ref()
            .expect("optimized");
        assert_eq!(choice.match_rate, 0.5);
        assert_eq!(choice.hsa_limit, 4_400.0);
        assert_eq!(s.options.debts.len(), 1);
        assert_eq!(s.options.debts[0].annual_payment, 6_000.0);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn given_contributions_stop_at_irs_limits() {
        let mut p = plan();
        // 5% a year above inflation.
        p.investment.contribution_growth_rate = 1.025 * 1.05 - 1.0;
        p.tax_optimizer.as_mut().expect("settings").hsa_contribution = 4_400.0;
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("valid plan");
        // Held to the 401(k) limit, with the catch-up from 50.
        assert!((s.working_years[1].tax_deferred_contribution - 24_500.0).abs() < 1e-6);
        assert!((s.working_years[9].tax_deferred_contribution - 32_500.0).abs() < 1e-6);
        assert!((s.working_years[1].hsa_contribution - 4_400.0).abs() < 1e-6);
        // What is held back stays in taxable pay.
        assert!((s.working_years[1].wages - (140_000.0 - 24_500.0 - 4_400.0)).abs() < 1e-6);
        assert_eq!(
            contribution_caps(&p, &s),
            vec![
                ContributionCap {
                    account: ContributionAccount::PreTax401k,
                    age: 46
                },
                ContributionCap {
                    account: ContributionAccount::Hsa,
                    age: 46
                },
            ]
        );

        // Family coverage has a higher HSA limit.
        p.tax_optimizer
            .as_mut()
            .expect("settings")
            .hsa_family_coverage = true;
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("valid plan");
        assert!(s.working_years[1].hsa_contribution > 4_400.0);
        assert!(!contribution_caps(&p, &s)
            .iter()
            .any(|cap| cap.account == ContributionAccount::Hsa));
    }

    #[test]
    fn a_plan_without_expenses_is_rejected() {
        let mut p = plan();
        p.expenses.items.clear();
        assert!(build_tax_optimized_scenario(&p, &accounts(), AS_OF).is_err());
    }

    #[test]
    fn unaffordable_spending_is_reported_not_raised() {
        let mut p = plan();
        p.expenses.items[0].monthly_amount = 40_000.0;
        let result = optimize_tax_plan(&p, &accounts(), AS_OF).expect("solver ran");
        assert_eq!(result.status, "infeasible");
        assert!(result.infeasible_reason.is_some());
        assert!(result.years.is_empty());
    }

    #[test]
    fn optimized_plan_funds_the_expenses_and_reports_todays_money() {
        let result = optimize_tax_plan(&plan(), &accounts(), AS_OF).expect("solver ran");
        assert_ne!(
            result.status, "infeasible",
            "{:?}",
            result.infeasible_reason
        );
        assert_eq!(result.years.len(), 48);
        assert!((result.years[0].net_spending - 60_000.0).abs() < 1.0);
        assert!((result.years[10].net_spending - 72_000.0).abs() < 1.0);
        assert!(result.years[..10].iter().all(|y| y.working));
        assert!(!result.years[10].working);
        assert_eq!(result.social_security_claim_age, Some(67.0));
        assert!(result.bequest_after_tax > 0.0);
        assert!(result.years[0].inflation_factor == 1.0);
    }

    #[test]
    fn earliest_age_search_compares_every_age_and_solves_the_first_that_works() {
        let mut p = plan();
        p.personal.birth_year_month = Some("1964-04".into());
        p.personal.current_age = 62;
        p.personal.target_retirement_age = 65;
        p.personal.planning_horizon_age = 85;
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.goal = TaxPlanGoal::EarliestAge;
        }
        let accounts = TaxOptimizerAccounts {
            taxable: 100_000.0,
            taxable_cost_basis: 80_000.0,
            tax_deferred: 400_000.0,
            roth: 0.0,
            hsa: 0.0,
            ..TaxOptimizerAccounts::default()
        };
        let result = optimize_tax_plan(&p, &accounts, AS_OF).expect("solver ran");

        let ages: Vec<u32> = result
            .retirement_age_options
            .iter()
            .map(|o| o.age)
            .collect();
        assert_eq!(ages, (62..=70).collect::<Vec<_>>());
        let earliest = result
            .retirement_age_options
            .iter()
            .find(|o| o.feasible)
            .expect("working longer makes the must-haves affordable");
        assert!(earliest.age > 62, "retiring now should be out of reach");
        // Working longer never makes the plan worse.
        for pair in result.retirement_age_options.windows(2) {
            assert!(!pair[0].feasible || pair[1].feasible);
            if pair[0].feasible {
                assert!(pair[1].retirement_spending >= pair[0].retirement_spending - 1.0);
            }
        }
        for option in result.retirement_age_options.iter().filter(|o| o.feasible) {
            assert!(option.retirement_spending >= option.required_spending - 1.0);
        }

        // The full plan is the earliest feasible age, maximizing spending.
        assert_eq!(result.retirement_age, earliest.age);
        assert_eq!(result.objective, TaxOptimizerObjective::MaxSpending);
        assert_ne!(
            result.status, "infeasible",
            "{:?}",
            result.infeasible_reason
        );
        assert!(result
            .years
            .iter()
            .all(|y| y.net_spending >= y.required_spending - 1.0));
    }

    #[test]
    fn earliest_age_search_needs_must_have_spending() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.goal = TaxPlanGoal::EarliestAge;
        }
        for item in &mut p.expenses.items {
            item.essential = Some(false);
        }
        assert!(optimize_tax_plan(&p, &accounts(), AS_OF).is_err());
    }

    #[test]
    fn the_floor_is_the_must_have_spending_of_each_year() {
        // Living (60k a year) is must-have; travel (12k, ages 55-74) is not.
        let s = build_tax_optimized_scenario(&plan(), &accounts(), AS_OF).expect("scenario");
        assert!(s
            .options
            .required_spending
            .iter()
            .all(|r| (r - 60_000.0).abs() < 1e-6));

        // Made must-have, travel raises the floor only while it lasts.
        let mut p = plan();
        p.expenses.items[1].essential = Some(true);
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        let floor = &s.options.required_spending;
        assert!((floor[(54 - 45) as usize] - 60_000.0).abs() < 1e-6);
        assert!((floor[(55 - 45) as usize] - 72_000.0).abs() < 1e-6);
        assert!((floor[(75 - 45) as usize] - 60_000.0).abs() < 1e-6);
    }

    fn with_markets(edit: impl FnOnce(&mut MarketSettings)) -> RetirementPlan {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            edit(&mut settings.markets);
        }
        p
    }

    const STOCKS: AssetMix = AssetMix {
        stocks: 1.0,
        bonds: 0.0,
        notes: 0.0,
        cash: 0.0,
    };
    const CASH: AssetMix = AssetMix {
        stocks: 0.0,
        bonds: 0.0,
        notes: 0.0,
        cash: 1.0,
    };

    #[test]
    fn projection_returns_stay_the_default() {
        let s = build_tax_optimized_scenario(&plan(), &accounts(), AS_OF).expect("scenario");
        assert!(s.rates.account_returns.is_empty());
        assert!(s.rates.annual_inflation.is_empty());
        assert_eq!(s.rates.inflation, 0.025);
    }

    #[test]
    fn fixed_returns_follow_each_buckets_mix_and_glide() {
        let p = with_markets(|m| {
            m.method = ReturnMethod::Fixed;
            m.inflation = Some(0.03);
            m.starting_mix = Some(BucketMixes {
                taxable: STOCKS,
                tax_deferred: CASH,
                ..BucketMixes::all(AssetMix::stocks_bonds(0.5))
            });
            m.target_mix = Some(CASH);
            m.target_age = Some(65);
        });
        let fee = p.investment.annual_investment_fee_rate;
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        let r0 = s.rates.for_year(0);
        assert!((r0[0] - (0.06 - fee)).abs() < 1e-12, "taxable all stocks");
        assert!((r0[1] - (0.03 - fee)).abs() < 1e-12, "cash earns inflation");
        assert!(
            (r0[2] - (0.05 - fee)).abs() < 1e-12,
            "half stocks, half Baa"
        );
        // Linear glide from 45 to 65: halfway at 55, all cash from 65 on.
        let r10 = s.rates.for_year(10);
        assert!((r10[0] - (0.045 - fee)).abs() < 1e-12);
        assert!(s
            .rates
            .for_year(20)
            .iter()
            .all(|r| (r - (0.03 - fee)).abs() < 1e-12));
        assert_eq!(s.rates.inflation, 0.03);
    }

    #[test]
    fn history_replays_from_the_start_year_with_its_inflation() {
        let p = with_markets(|m| {
            m.method = ReturnMethod::Historical;
            m.start_year = 1966;
            m.inflation_method = InflationMethod::Historical;
            m.starting_mix = Some(BucketMixes::all(STOCKS));
        });
        let fee = p.investment.annual_investment_fee_rate;
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        let history = wealthfolio_optimization_engine::markets::history();
        let y1966 = history.iter().find(|y| y.year == 1966).expect("1966");
        assert!((s.rates.for_year(0)[0] - (y1966.stocks - fee)).abs() < 1e-12);
        assert_eq!(s.rates.annual_inflation[0], y1966.inflation);
        assert_eq!(s.rates.annual_inflation.len(), s.years());
        assert!(s.validate().is_ok(), "down years keep the dividend yield");

        let p = with_markets(|m| {
            m.method = ReturnMethod::Historical;
            m.start_year = 1900;
        });
        assert!(build_tax_optimized_scenario(&p, &accounts(), AS_OF).is_err());
    }

    #[test]
    fn holdings_set_the_mix_unless_one_is_entered() {
        let mut p = with_markets(|m| m.method = ReturnMethod::Fixed);
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.equity_share = 0.6;
        }
        let fee = p.investment.annual_investment_fee_rate;
        let held = TaxOptimizerAccounts {
            mixes: Some(BucketMixes::all(CASH)),
            ..accounts()
        };
        let s = build_tax_optimized_scenario(&p, &held, AS_OF).expect("scenario");
        assert!((s.rates.for_year(0)[0] - (0.025 - fee)).abs() < 1e-12);

        // Without holdings or an entered mix: equity_share in stocks.
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        let expected = 0.6 * 0.06 + 0.4 * 0.04 - fee;
        assert!((s.rates.for_year(0)[3] - expected).abs() < 1e-12);
    }

    fn linked_home(purchase_age: Option<u32>) -> LinkedHomePurchase {
        LinkedHomePurchase {
            goal_title: "Lake house".into(),
            purchase: HomePurchase {
                price: 400_000.0,
                down_payment_rate: 0.2,
                closing_cost_rate: 0.03,
                mortgage_rate: 0.06,
                term_years: 30,
                property_tax_rate: 0.01,
                insurance: 1_200.0,
                maintenance_rate: 0.01,
                hoa_monthly: 100.0,
                appreciation: 0.03,
                purchase_age,
            },
            target_date: Some("2030-06-01".into()),
        }
    }

    #[test]
    fn a_linked_home_replaces_rent_and_pays_down_and_mortgage_at_purchase() {
        let mut p = plan();
        p.investment.inflation_rate = 0.03;
        p.expenses.items[0].monthly_amount = 2_500.0;
        p.expenses.items.push(housing_item("Rent", 2_000.0));
        // Bought at 49 (2030), where the goal service ends the rent.
        end_items_at_home_purchase(&mut p.expenses, 49);
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.home_purchase = Some(linked_home(None));
        }
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        // Bought in 2030, plan year 4; prices grow with inflation here, so
        // today's-money amounts stay at the entered ones.
        let k = 4;
        let floor = &s.options.required_spending;
        assert_eq!(floor[k - 1], 54_000.0);
        assert!((floor[k] - (30_000.0 + 400_000.0 * 0.02 + 1_200.0 + 1_200.0)).abs() < 1e-6);
        let flows = &s.options.cash_flows;
        assert!((flows[k] + 400_000.0 * 0.23).abs() < 1e-6);
        assert!(flows.iter().enumerate().all(|(n, f)| n == k || *f == 0.0));
        let mortgage = s.options.debts.last().expect("mortgage");
        assert_eq!(mortgage.start, k);
        assert!((mortgage.balance - 320_000.0 * 1.03f64.powi(4)).abs() < 1e-6);

        // At an age instead: 1981 + 52 = 2033, plan year 7.
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.home_purchase = Some(linked_home(Some(52)));
        }
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        assert_eq!(s.options.debts.last().map(|d| d.start), Some(7));

        let result = optimize_tax_plan(&p, &accounts(), AS_OF).expect("solve");
        let home = result.home.expect("home summary");
        assert_eq!((home.purchase_year, home.purchase_age), (2033, 52));
        assert!((home.price - 400_000.0).abs() < 1e-6);
        assert!((home.value_at_end - 400_000.0).abs() < 1e-6);
        assert!((home.monthly_payment - 1_918.56).abs() < 0.01);
        assert!((home.mortgage - 320_000.0).abs() < 1e-6);
    }

    #[test]
    fn selling_the_current_home_repays_its_loan_and_stops_its_costs() {
        let mut p = plan();
        p.expenses.items[0].monthly_amount = 2_500.0;
        p.expenses
            .items
            .push(housing_item("Home costs", 8_000.0 / 12.0));
        end_items_at_home_purchase(&mut p.expenses, 49);
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.sell_current_home = true;
            settings.current_home_value = 300_000.0;
            settings.selling_cost_rate = 0.06;
            settings.current_home_debt = Some("Mortgage".into());
            settings.debts = vec![TaxOptimizerDebt {
                label: "Mortgage".into(),
                balance: 120_000.0,
                annual_rate: 0.04,
                annual_payment: 12_000.0,
            }];
            settings.home_purchase = Some(linked_home(None));
        }
        let s = build_tax_optimized_scenario(&p, &accounts(), AS_OF).expect("scenario");
        let k = 4;
        assert_eq!(s.options.debts[0].payoff, Some(k));
        assert_eq!(s.options.required_spending[k - 1], 38_000.0);
        assert!(s.options.required_spending[k] < 38_000.0 + 15_000.0);
        assert!(
            s.options.cash_flows[k] > 0.0,
            "the sale outweighs the down payment"
        );
    }

    /// A must-have housing item that ends at a linked purchase.
    fn housing_item(label: &str, monthly: f64) -> ExpenseBucket {
        ExpenseBucket {
            id: None,
            label: Some(label.into()),
            monthly_amount: monthly,
            inflation_rate: None,
            start_age: None,
            end_age: None,
            essential: Some(true),
            paid_while_working: false,
            yearly: false,
            ends_at_home_purchase: true,
        }
    }

    #[test]
    fn safe_spending_needs_a_sensible_target() {
        let p = plan();
        for target in [0.0, 1.5] {
            assert!(tax_plan_safe_spending(
                &p,
                &accounts(),
                AS_OF,
                SuccessPathMode::Historical,
                None,
                target
            )
            .is_err());
        }
    }

    #[test]
    fn choice_costs_nudge_one_choice_at_a_time() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.objective = TaxOptimizerObjective::MaxSpending;
            settings.max_roth_conversion = Some(20_000.0);
        }
        let costs = tax_plan_choice_costs(&p, &accounts(), AS_OF, None).expect("costs");
        let choices: Vec<&str> = costs.costs.iter().map(|c| c.choice.as_str()).collect();
        assert!(choices.contains(&"minimumExpenses"));
        assert!(choices.contains(&"bequestFloor"));
        assert!(choices.contains(&"conversionCap"));
        assert!(costs.base > 0.0);
        let effect = |name: &str| {
            costs
                .costs
                .iter()
                .find(|c| c.choice == name)
                .and_then(|c| c.effect)
        };
        // Leaving more costs spending; lifting a cap never hurts (beyond solver noise).
        assert!(effect("bequestFloor").is_some_and(|e| e < 0.0));
        assert!(effect("conversionCap").is_some_and(|e| e > -0.005 * costs.base));
    }

    #[test]
    fn stress_tests_reoptimize_under_each_shock() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.objective = TaxOptimizerObjective::MaxSpending;
        }
        let tests = tax_plan_stress_tests(&p, &accounts(), AS_OF, None).expect("stress tests");
        assert!(tests.base > 0.0);
        let effect = |name: &str, year: Option<i32>| {
            tests
                .stresses
                .iter()
                .find(|s| s.stress == name && s.year == year)
                .map(|s| s.effect)
        };
        for name in [
            "lowerReturns",
            "higherInflation",
            "higherExpenses",
            "socialSecurityCut",
            "liveLonger",
        ] {
            assert!(effect(name, None).is_some(), "{name} is tested");
        }
        // Each shock lowers the spending the plan can count on.
        for name in ["lowerReturns", "socialSecurityCut", "liveLonger"] {
            assert!(
                effect(name, None).flatten().is_none_or(|e| e < 0.0),
                "{name} costs spending"
            );
        }
        // Retiring into 1966's stagflation is worse than average markets.
        assert!(effect("retireInto", Some(1966))
            .expect("1966 is tested")
            .is_none_or(|e| e < 0.0));
        assert_eq!(
            tests
                .stresses
                .iter()
                .filter(|s| s.stress == "retireInto")
                .count(),
            STRESS_RETIREMENT_MARKETS.len()
        );
    }

    #[test]
    fn simulated_success_draws_the_planned_number_of_paths() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.simulation_count = Some(12);
        }
        for mode in [SuccessPathMode::Bootstrap, SuccessPathMode::Lognormal] {
            let sim = success_paths(&p, &accounts(), AS_OF, mode, true).expect("paths");
            assert_eq!(sim.paths.len(), 12);
        }
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.simulation_count = Some(1);
        }
        let sim =
            success_paths(&p, &accounts(), AS_OF, SuccessPathMode::Lognormal, true).expect("paths");
        assert_eq!(sim.paths.len(), MIN_SIMULATIONS as usize);
    }

    #[test]
    fn random_lifespans_give_each_run_its_own_horizon() {
        let mut p = plan();
        if let Some(settings) = p.tax_optimizer.as_mut() {
            settings.simulation_count = Some(40);
            settings.random_lifespans = Some(RandomLifespans {
                sex: Sex::Female,
                table: MortalityTable::Ssa2025,
            });
        }
        let sim =
            success_paths(&p, &accounts(), AS_OF, SuccessPathMode::Lognormal, true).expect("paths");
        let retire = p.personal.target_retirement_age;
        assert_eq!(sim.lifespans.len(), 40);
        assert!(sim.lifespans.iter().all(|age| *age > retire));
        // Lifespans vary, and every path covers its own plan.
        assert!(sim.lifetimes.len() > 5);
        for run in sim.runs() {
            assert!(run.path.returns.len() >= run.scenario.years());
        }
        let [p10, p50, p90] = sim.lifespan_percentiles().expect("drawn");
        assert!(
            p10 < p50 && p50 < p90 && (80..=95).contains(&p50),
            "{p10} {p50} {p90}"
        );

        // Historical replays and the regret check keep the planning horizon.
        let historical = success_paths(&p, &accounts(), AS_OF, SuccessPathMode::Historical, true)
            .expect("paths");
        assert!(historical.lifespans.is_empty() && historical.lifespan_percentiles().is_none());
        let fixed = success_paths(&p, &accounts(), AS_OF, SuccessPathMode::Lognormal, false)
            .expect("paths");
        assert!(fixed.lifespans.is_empty());
        assert!(fixed
            .runs()
            .iter()
            .all(|r| r.scenario.years() == fixed.scenario.years()));
    }

    #[test]
    fn conversion_grid_spans_the_recommendation_within_the_cap() {
        assert_eq!(
            conversion_grid(20_000.0, None),
            vec![0.0, 10_000.0, 20_000.0, 30_000.0, 40_000.0]
        );
        assert_eq!(
            conversion_grid(20_000.0, Some(25_000.0)),
            vec![0.0, 10_000.0, 20_000.0, 25_000.0]
        );
        assert_eq!(conversion_grid(0.0, None)[0], 0.0);
    }
}
