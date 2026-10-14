use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::planning::home_purchase::LinkedHomePurchase;
use crate::portfolio::fire::GlidepathSettings;
pub use wealthfolio_optimization_engine::markets::{AssetMix, GlideShape};
pub use wealthfolio_optimization_engine::mortality::{MortalityTable, Sex};

/// Default annual draw estimate for defined-contribution income streams that
/// have fund details but no explicit monthly payout.
pub const DEFAULT_DC_PAYOUT_ESTIMATE_RATE: f64 = 0.035;

pub(crate) const FUNDING_TOLERANCE: f64 = 0.999;

fn default_pre_retirement_annual_return() -> f64 {
    0.0577
}

fn default_retirement_annual_return() -> f64 {
    0.0337
}

fn default_annual_investment_fee_rate() -> f64 {
    0.006
}

fn default_annual_volatility() -> f64 {
    0.12
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetirementPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub personal: PersonalProfile,
    pub expenses: ExpenseBudget,
    pub income_streams: Vec<RetirementIncomeStream>,
    pub investment: InvestmentAssumptions,
    pub tax: Option<TaxProfile>,
    pub currency: String,
    /// Inputs used only by the tax-optimized planning style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tax_optimizer: Option<TaxOptimizerSettings>,
}

fn default_heirs_tax_rate() -> f64 {
    0.30
}

fn default_dividend_yield() -> f64 {
    0.015
}

/// What the tax-optimized plan maximizes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaxOptimizerObjective {
    /// Spend the planned expenses every year and leave the largest after-tax estate.
    #[default]
    MaxBequest,
    /// Spend as much as possible (in the shape of the planned expenses) while
    /// leaving at least `bequest_floor`.
    MaxSpending,
}

/// The question the tax-optimized plan answers.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaxPlanGoal {
    /// Retire at the plan's target age and optimize for `objective`.
    #[default]
    FixedAge,
    /// Find the earliest retirement age that still funds the minimum
    /// expenses (and the bequest floor), comparing every candidate age.
    EarliestAge,
}

/// Whether the household owns or rents its home.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Housing {
    #[default]
    Own,
    Rent,
}

/// Where the tax-optimized plan's yearly market returns come from.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReturnMethod {
    /// The plan's projection assumptions (before and after retirement),
    /// the same for every account.
    #[default]
    Plan,
    /// Fixed yearly returns of each asset class, weighted by each account's mix.
    Fixed,
    /// The market history replayed from a start year.
    Historical,
    /// The geometric average of a range of historical years, every year.
    HistoricalAverage,
}

/// Where the tax-optimized plan's inflation comes from.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InflationMethod {
    #[default]
    Fixed,
    /// The history's own inflation (with a historical return method).
    Historical,
}

/// Yearly returns of the asset classes, as decimals (cash earns inflation).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetClassReturns {
    pub stocks: f64,
    pub bonds: f64,
    pub notes: f64,
}

impl Default for AssetClassReturns {
    /// Owl's conservative preset.
    fn default() -> Self {
        Self {
            stocks: 0.06,
            bonds: 0.04,
            notes: 0.033,
        }
    }
}

/// Asset mixes of the four account buckets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BucketMixes {
    pub taxable: AssetMix,
    pub tax_deferred: AssetMix,
    pub roth: AssetMix,
    pub hsa: AssetMix,
}

impl BucketMixes {
    pub fn all(mix: AssetMix) -> Self {
        Self {
            taxable: mix,
            tax_deferred: mix,
            roth: mix,
            hsa: mix,
        }
    }

    /// `[taxable, tax-deferred, Roth, HSA]`, the engine's account order.
    pub fn to_array(self) -> [AssetMix; 4] {
        [self.taxable, self.tax_deferred, self.roth, self.hsa]
    }
}

/// Market assumptions of the tax-optimized plan (Owl's rate options).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketSettings {
    pub method: ReturnMethod,
    /// Returns for [`ReturnMethod::Fixed`].
    pub fixed: AssetClassReturns,
    pub inflation_method: InflationMethod,
    /// Fixed inflation; `None` uses the plan's projection assumption.
    pub inflation: Option<f64>,
    /// First year replayed by [`ReturnMethod::Historical`].
    pub start_year: i32,
    /// Range averaged by [`ReturnMethod::HistoricalAverage`].
    pub average_from: i32,
    pub average_to: i32,
    /// Each bucket's mix today; `None` derives it from the holdings.
    pub starting_mix: Option<BucketMixes>,
    /// The mix every bucket glides to; `None` keeps the starting mix.
    pub target_mix: Option<AssetMix>,
    pub glide: GlideShape,
    /// Age the target mix is reached; `None` is the planning horizon.
    pub target_age: Option<u32>,
}

impl Default for MarketSettings {
    fn default() -> Self {
        Self {
            method: ReturnMethod::Plan,
            fixed: AssetClassReturns::default(),
            inflation_method: InflationMethod::Fixed,
            inflation: None,
            start_year: 1969,
            average_from: 1928,
            average_to: 2025,
            starting_mix: None,
            target_mix: None,
            glide: GlideShape::Linear,
            target_age: None,
        }
    }
}

/// Inputs for the tax-optimized (MILP) planning style. Money is in today's
/// currency and per year unless noted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaxOptimizerSettings {
    /// Income stream that represents Social Security. It is taxed and timed as
    /// Social Security rather than as a pension; its start age is the claim age
    /// and its monthly amount is the benefit at that age.
    #[serde(default)]
    pub social_security_stream_id: Option<String>,
    /// Let the optimizer pick the Social Security claiming month (62–70)
    /// instead of using the planned claim age.
    #[serde(default)]
    pub optimize_claim_age: bool,
    #[serde(default)]
    pub goal: TaxPlanGoal,
    /// Housing as saved before rent and home costs became spending items;
    /// read once by `move_housing_to_expenses`.
    #[serde(default)]
    pub housing: Housing,
    #[serde(default)]
    pub monthly_rent: f64,
    #[serde(default)]
    pub home_costs: f64,
    /// Active home goal whose purchase the plan includes: its savings join the
    /// plan's accounts, the down payment and closing costs leave them, and the
    /// mortgage and owning costs replace rent from the purchase on.
    #[serde(default)]
    pub home_goal_id: Option<String>,
    /// Sell the current home when the linked home is bought.
    #[serde(default)]
    pub sell_current_home: bool,
    /// The current home's value today; it grows like the new home's price.
    #[serde(default)]
    pub current_home_value: f64,
    /// Agent fees and other selling costs, as a share of the sale price.
    #[serde(default = "default_selling_cost_rate")]
    pub selling_cost_rate: f64,
    /// Label of the debt the sale repays, typically the current mortgage.
    #[serde(default)]
    pub current_home_debt: Option<String>,
    /// The linked home goal's purchase, filled in when the plan is solved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_purchase: Option<LinkedHomePurchase>,
    #[serde(default)]
    pub objective: TaxOptimizerObjective,
    /// After-tax estate to leave under `MaxSpending`.
    #[serde(default)]
    pub bequest_floor: f64,
    /// Tax rate heirs pay on inherited tax-deferred balances.
    #[serde(default = "default_heirs_tax_rate")]
    pub heirs_tax_rate: f64,
    /// Yearly cap on Roth conversions; none means no cap.
    #[serde(default)]
    pub max_roth_conversion: Option<f64>,
    /// Qualified-dividend yield of taxable investments.
    #[serde(default = "default_dividend_yield")]
    pub dividend_yield: f64,
    /// Pre-tax payroll deferral (traditional 401(k)) while working; it is taken
    /// out of `current_annual_salary` before tax.
    #[serde(default)]
    pub pre_tax_contribution: f64,
    /// Employer contributions to the 401(k) while working.
    #[serde(default)]
    pub employer_contribution: f64,
    /// Roth 401(k)/IRA contributions while working, paid from taxed pay.
    #[serde(default)]
    pub roth_contribution: f64,
    /// New money invested in taxable accounts while working.
    #[serde(default)]
    pub taxable_contribution: f64,
    /// Roth conversions in each of the last five years, most recent first
    /// (feeds the five-year rule).
    #[serde(default)]
    pub recent_roth_conversions: Vec<f64>,
    /// Roth contributions in each of the last five years, most recent first.
    #[serde(default)]
    pub recent_roth_contributions: Vec<f64>,
    /// Modified AGI of the last two years (Medicare IRMAA looks back two years);
    /// none uses the current salary.
    #[serde(default)]
    pub prior_magi: Option<f64>,
    /// How far yearly spending may deviate from the planned shape (0–1).
    #[serde(default)]
    pub spending_flexibility: f64,
    /// Two-letter state for state income tax (only "VA" is modelled); none
    /// means no state tax.
    #[serde(default)]
    pub state: Option<String>,
    /// Yearly benchmark (second-lowest silver) ACA premium for retired years
    /// before Medicare; 0 when not buying marketplace cover. Leave health
    /// insurance out of the planned expenses when this is set.
    #[serde(default)]
    pub aca_benchmark_premium: f64,
    /// HSA contributions per year while working (payroll, pre-tax).
    #[serde(default)]
    pub hsa_contribution: f64,
    /// Medical costs other than Medicare premiums that HSA money may pay.
    #[serde(default)]
    pub other_medical_expenses: f64,
    /// Rule of 55: the 401(k) balance usable penalty-free after leaving the job
    /// in or after the year turning 55.
    #[serde(default)]
    pub rule_of_55_balance: Option<f64>,
    /// 72(t) payments: start age and fixed yearly payment.
    #[serde(default)]
    pub sepp_start_age: Option<u32>,
    #[serde(default)]
    pub sepp_annual_payment: f64,
    /// Allow realizing gains (and buying back) to raise the taxable cost basis,
    /// typically inside the 0% capital-gains band.
    #[serde(default)]
    pub harvest_gains: bool,
    /// Let the optimizer choose each working year's 401(k) (pre-tax or Roth),
    /// Roth IRA and HSA contributions within IRS limits. The salary is then
    /// gross pay and the fixed contribution amounts above are ignored.
    #[serde(default)]
    pub optimize_contributions: bool,
    /// Employer match: this share of deferrals, up to `match_cap` of salary.
    #[serde(default)]
    pub match_rate: f64,
    #[serde(default)]
    pub match_cap: f64,
    /// Covered by a high-deductible plan (HSA contributions allowed).
    #[serde(default)]
    pub hsa_eligible: bool,
    /// Family (rather than self-only) HSA coverage, which sets the HSA limit.
    #[serde(default)]
    pub hsa_family_coverage: bool,
    /// Whether the 401(k) offers Roth deferrals.
    #[serde(default = "default_true")]
    pub roth_401k_available: bool,
    /// With optimized contributions: the share (0–1) of leftover pay invested
    /// in the taxable account, the rest spent. `None` invests all of it.
    #[serde(default)]
    pub leftover_invest_rate: Option<f64>,
    /// Loans paid on a schedule; extra principal is left to the optimizer.
    #[serde(default)]
    pub debts: Vec<TaxOptimizerDebt>,
    /// Stock share (the rest in bonds) of a bucket whose mix is unknown:
    /// no holdings to derive it from and no mix entered.
    #[serde(default = "default_equity_share")]
    pub equity_share: f64,
    /// Market returns, inflation and asset mixes.
    #[serde(default)]
    pub markets: MarketSettings,
    /// Share of market paths the safe spending must succeed on (0.9 = 90%);
    /// none uses 90%.
    #[serde(default)]
    pub success_target: Option<f64>,
    /// Market paths a simulated (Monte Carlo) risk check draws; none uses 100.
    #[serde(default)]
    pub simulation_count: Option<u32>,
    /// Market paths the simulated conversion regret check draws (each is
    /// solved once per conversion amount); none uses 30.
    #[serde(default)]
    pub regret_simulation_count: Option<u32>,
    /// Gives each simulated run a random lifespan instead of ending every run
    /// at the planning horizon (Owl's longevity risk); none keeps the horizon.
    #[serde(default)]
    pub random_lifespans: Option<RandomLifespans>,
}

/// Where random lifespans are drawn from: an actuarial table for the person's
/// sex, given they live to retire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RandomLifespans {
    pub sex: Sex,
    #[serde(default)]
    pub table: MortalityTable,
}

fn default_true() -> bool {
    true
}

fn default_selling_cost_rate() -> f64 {
    0.06
}

fn default_equity_share() -> f64 {
    0.6
}

/// A loan for the tax-optimized plan, in today's money.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaxOptimizerDebt {
    pub label: String,
    pub balance: f64,
    /// Yearly interest rate (0.065 = 6.5%).
    pub annual_rate: f64,
    /// Scheduled yearly payment (principal and interest).
    pub annual_payment: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersonalProfile {
    #[serde(default)]
    pub birth_year_month: Option<String>,
    pub current_age: u32,
    pub target_retirement_age: u32,
    pub planning_horizon_age: u32,
    pub current_annual_salary: Option<f64>,
    pub salary_growth_rate: Option<f64>,
}

pub fn age_from_birth_year_month(birth_year_month: &str, as_of: NaiveDate) -> Option<u32> {
    let (year, month) = birth_year_month.split_once('-')?;
    let birth_year = year.parse::<i32>().ok()?;
    let birth_month = month.parse::<u32>().ok()?;
    if !(1..=12).contains(&birth_month) || birth_year > as_of.year() {
        return None;
    }

    let mut age = as_of.year() - birth_year;
    if as_of.month() < birth_month {
        age -= 1;
    }
    u32::try_from(age).ok()
}

/// Id of the spending item rent became; see `move_housing_to_expenses`.
pub const RENT_EXPENSE_ID: &str = "housing-rent";
/// Id of the spending item an owned home's costs became.
pub const HOME_COSTS_EXPENSE_ID: &str = "housing-home-costs";

/// Moves rent or the owned home's costs from the tax-optimized settings,
/// where plans saved before spending items could end at a home purchase kept
/// them, into must-have spending items, and clears the old fields. Rent ends
/// at a linked purchase, as it did; the home's costs end there when the home
/// is sold. A plan already moved is left as it is.
pub fn move_housing_to_expenses(plan: &mut RetirementPlan) {
    let Some(settings) = plan.tax_optimizer.as_mut() else {
        return;
    };
    let (id, label, monthly, yearly, ends) = match settings.housing {
        Housing::Rent if settings.monthly_rent > 0.0 => {
            (RENT_EXPENSE_ID, "Rent", settings.monthly_rent, false, true)
        }
        Housing::Own if settings.home_costs > 0.0 => (
            HOME_COSTS_EXPENSE_ID,
            "Home costs (tax, insurance, upkeep)",
            settings.home_costs / 12.0,
            true,
            settings.sell_current_home,
        ),
        _ => return,
    };
    settings.monthly_rent = 0.0;
    settings.home_costs = 0.0;
    if plan
        .expenses
        .items
        .iter()
        .any(|item| item.id.as_deref() == Some(id))
    {
        return;
    }
    plan.expenses.items.push(ExpenseBucket {
        id: Some(id.to_string()),
        label: Some(label.to_string()),
        monthly_amount: monthly,
        inflation_rate: None,
        start_age: None,
        end_age: None,
        essential: Some(true),
        paid_while_working: false,
        yearly,
        ends_at_home_purchase: ends,
    });
}

/// Ends the items marked `ends_at_home_purchase` at `purchase_age`.
pub fn end_items_at_home_purchase(budget: &mut ExpenseBudget, purchase_age: u32) {
    for item in budget.items.iter_mut().filter(|i| i.ends_at_home_purchase) {
        item.end_age = Some(
            item.end_age
                .map_or(purchase_age, |end| end.min(purchase_age)),
        );
    }
}

pub fn normalize_retirement_plan_ages(plan: &mut RetirementPlan, as_of: chrono::NaiveDate) {
    if let Some(age) = plan
        .personal
        .birth_year_month
        .as_deref()
        .and_then(|birth_year_month| age_from_birth_year_month(birth_year_month, as_of))
    {
        plan.personal.current_age = age;
    }
}

#[cfg(test)]
mod tests {
    const AS_OF: chrono::NaiveDate = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    use super::{
        age_from_birth_year_month, normalize_retirement_plan_ages, PayoutMode,
        RetirementIncomeStream, RetirementPlan, StreamKind,
    };
    use chrono::{Datelike, NaiveDate};

    #[test]
    fn derives_age_from_birth_year_month() {
        let as_of = NaiveDate::from_ymd_opt(2026, 4, 21).unwrap();

        assert_eq!(age_from_birth_year_month("1981-04", as_of), Some(45));
        assert_eq!(age_from_birth_year_month("1981-05", as_of), Some(44));
        assert_eq!(age_from_birth_year_month("1981-13", as_of), None);
    }

    #[test]
    fn old_withdrawal_rule_json_is_ignored() {
        let raw = r#"{
            "personal": {
                "currentAge": 45,
                "targetRetirementAge": 55,
                "planningHorizonAge": 90
            },
            "expenses": { "items": [{ "monthlyAmount": 6000.0 }] },
            "incomeStreams": [],
            "investment": {
                "preRetirementAnnualReturn": 0.057,
                "retirementAnnualReturn": 0.034,
                "annualInvestmentFeeRate": 0.006,
                "annualVolatility": 0.12,
                "inflationRate": 0.02,
                "monthlyContribution": 3000.0,
                "contributionGrowthRate": 0.02,
                "glidePath": null
            },
            "withdrawal": {
                "safeWithdrawalRate": 0.04,
                "strategy": "guardrails",
                "guardrails": { "ceilingRate": 0.06 }
            },
            "tax": null,
            "currency": "CAD"
        }"#;

        let plan: RetirementPlan = serde_json::from_str(raw).expect("old JSON should parse");
        let serialized = serde_json::to_string(&plan).expect("plan should serialize");

        assert_eq!(plan.currency, "CAD");
        assert!(!serialized.contains("withdrawal"));
        assert!(!serialized.contains("safeWithdrawalRate"));
    }

    #[test]
    fn a_fund_saved_without_a_payout_mode_is_an_annuity() {
        let raw = r#"{
            "id": "dc",
            "label": "RRSP",
            "streamType": "dc",
            "startAge": 65,
            "adjustForInflation": false,
            "annualGrowthRate": null,
            "monthlyAmount": null,
            "linkedAccountId": null,
            "currentValue": 100000.0,
            "monthlyContribution": 500.0,
            "accumulationReturn": 0.04
        }"#;

        let stream: RetirementIncomeStream =
            serde_json::from_str(raw).expect("a plan saved before drawdown existed should parse");

        assert_eq!(stream.payout_mode, None);
        assert_eq!(stream.post_payout_return, None);
        assert_eq!(stream.payout_mode.unwrap_or_default(), PayoutMode::Annuity);
        assert!(
            !stream.is_drawdown(),
            "an absent payout mode must not turn a shipped plan into a drawdown"
        );
    }

    #[test]
    fn payout_mode_round_trips_through_json() {
        let raw = r#"{
            "id": "dc",
            "label": "RRSP",
            "streamType": "dc",
            "startAge": 65,
            "adjustForInflation": false,
            "monthlyAmount": null,
            "currentValue": 100000.0,
            "monthlyContribution": null,
            "accumulationReturn": 0.04,
            "payoutMode": "drawdown",
            "postPayoutReturn": 0.03
        }"#;

        let stream: RetirementIncomeStream = serde_json::from_str(raw).expect("should parse");

        assert_eq!(stream.payout_mode, Some(PayoutMode::Drawdown));
        assert_eq!(stream.post_payout_return, Some(0.03));
        assert!(stream.is_drawdown());

        let serialized = serde_json::to_string(&stream).expect("should serialize");
        assert!(serialized.contains("\"payoutMode\":\"drawdown\""));
    }

    #[test]
    fn a_defined_benefit_stream_is_never_a_drawdown() {
        let stream = RetirementIncomeStream {
            id: "db".into(),
            label: "State pension".into(),
            stream_type: StreamKind::DefinedBenefit,
            start_age: 67,
            adjust_for_inflation: true,
            annual_growth_rate: None,
            monthly_amount: Some(1_200.0),
            linked_account_id: None,
            current_value: None,
            monthly_contribution: None,
            accumulation_return: None,
            payout_rate: None,
            payout_mode: Some(PayoutMode::Drawdown),
            post_payout_return: None,
        };

        assert!(
            !stream.is_drawdown(),
            "a defined-benefit stream has no balance to draw against"
        );
    }

    #[test]
    fn age_normalization_only_updates_current_age() {
        let today = AS_OF;
        let birth_year_month = format!("{}-{:02}", today.year() - 40, today.month());
        let raw = format!(
            r#"{{
                "personal": {{
                    "birthYearMonth": "{birth_year_month}",
                    "currentAge": 1,
                    "targetRetirementAge": 55,
                    "planningHorizonAge": 90
                }},
                "expenses": {{ "items": [{{ "id": "living", "label": "Living", "monthlyAmount": 6000.0 }}] }},
                "incomeStreams": [],
                "investment": {{
                    "preRetirementAnnualReturn": 0.057,
                    "retirementAnnualReturn": 0.034,
                    "annualInvestmentFeeRate": 0.006,
                    "annualVolatility": 0.12,
                    "inflationRate": 0.02,
                    "monthlyContribution": 3000.0,
                    "contributionGrowthRate": 0.02,
                    "glidePath": null
                }},
                "tax": null,
                "currency": "CAD"
            }}"#
        );
        let mut plan: RetirementPlan = serde_json::from_str(&raw).expect("plan should parse");
        let mut expected = serde_json::to_value(&plan).expect("plan should serialize");
        expected["personal"]["currentAge"] = serde_json::Value::from(40);

        normalize_retirement_plan_ages(&mut plan, AS_OF);
        let prior_day = AS_OF.pred_opt().unwrap();
        let mut prior_plan = plan.clone();
        normalize_retirement_plan_ages(&mut prior_plan, prior_day);
        assert_eq!(
            prior_plan.personal.current_age + 1,
            plan.personal.current_age
        );

        let actual = serde_json::to_value(&plan).expect("plan should serialize");
        assert_eq!(actual, expected);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseBudget {
    #[serde(default)]
    pub items: Vec<ExpenseBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseBucket {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub monthly_amount: f64,
    pub inflation_rate: Option<f64>,
    pub start_age: Option<u32>,
    pub end_age: Option<u32>,
    pub essential: Option<bool>,
    /// Paid from the portfolio even before retirement, such as a home's down
    /// payment. Other expenses before retirement come out of income.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paid_while_working: bool,
    /// Entered and shown per year. `monthly_amount` still holds the amount per
    /// month; this only changes how it is shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub yearly: bool,
    /// Ends when the linked home is bought, such as rent or the current
    /// home's costs when it is sold.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ends_at_home_purchase: bool,
}

impl ExpenseBudget {
    pub fn all_buckets(&self) -> Vec<(&ExpenseBucket, bool)> {
        self.items
            .iter()
            .map(|bucket| (bucket, bucket.essential.unwrap_or(true)))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetirementIncomeStream {
    pub id: String,
    pub label: String,
    pub stream_type: StreamKind,
    pub start_age: u32,
    pub adjust_for_inflation: bool,
    pub annual_growth_rate: Option<f64>,
    /// Net monthly payout in today's money. Tax on portfolio withdrawals is modeled separately.
    pub monthly_amount: Option<f64>,
    pub linked_account_id: Option<String>,
    pub current_value: Option<f64>,
    pub monthly_contribution: Option<f64>,
    pub accumulation_return: Option<f64>,
    /// Annual draw rate applied to the projected fund balance once payouts start.
    /// Falls back to `DEFAULT_DC_PAYOUT_ESTIMATE_RATE` when unset.
    pub payout_rate: Option<f64>,
    /// What happens to the fund at `start_age`. Absent means `Annuity`, so plans
    /// saved before this field existed keep projecting exactly as they did.
    pub payout_mode: Option<PayoutMode>,
    /// Annual return the balance still invested earns during a `Drawdown` payout
    /// phase. Falls back to the plan's retirement return.
    pub post_payout_return: Option<f64>,
}

impl RetirementIncomeStream {
    /// A fund that keeps its balance through the payout phase, and can therefore
    /// run out. Only a defined-contribution stream has a balance to draw against.
    pub fn is_drawdown(&self) -> bool {
        self.stream_type == StreamKind::DefinedContribution
            && self.payout_mode.unwrap_or_default() == PayoutMode::Drawdown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamKind {
    #[serde(rename = "db")]
    DefinedBenefit,
    #[serde(rename = "dc")]
    DefinedContribution,
}

/// What a defined-contribution fund does with its balance at `start_age`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum PayoutMode {
    /// The pot buys an income. It leaves the projection at `start_age` and the
    /// income runs to the horizon, funded by capital consumption and mortality
    /// credits rather than by the fund's own return.
    #[default]
    #[serde(rename = "annuity")]
    Annuity,
    /// The pot stays invested and is drawn against. The income stops when the
    /// balance is gone.
    #[serde(rename = "drawdown")]
    Drawdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InvestmentAssumptions {
    #[serde(
        alias = "expectedAnnualReturn",
        default = "default_pre_retirement_annual_return"
    )]
    pub pre_retirement_annual_return: f64,
    #[serde(default = "default_retirement_annual_return")]
    pub retirement_annual_return: f64,
    #[serde(default = "default_annual_investment_fee_rate")]
    pub annual_investment_fee_rate: f64,
    #[serde(alias = "expectedReturnStdDev", default = "default_annual_volatility")]
    pub annual_volatility: f64,
    pub inflation_rate: f64,
    pub monthly_contribution: f64,
    pub contribution_growth_rate: f64,
    pub glide_path: Option<GlidepathSettings>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetirementTimingMode {
    #[default]
    #[serde(rename = "fire")]
    Fire,
    /// Also the timing of the tax-optimized style ("optimized"), which retires
    /// at the target age; its tax plan is solved separately.
    #[serde(rename = "traditional", alias = "optimized")]
    Traditional,
}

impl RetirementTimingMode {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Self {
        match value {
            "traditional" | "optimized" => Self::Traditional,
            _ => Self::Fire,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fire => "fire",
            Self::Traditional => "traditional",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetirementStartReason {
    #[serde(rename = "funded")]
    Funded,
    #[serde(rename = "target_age_forced")]
    TargetAgeForced,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaxBucketBalances {
    pub taxable: f64,
    pub tax_deferred: f64,
    pub tax_free: f64,
}

impl TaxBucketBalances {
    pub fn total(&self) -> f64 {
        self.taxable + self.tax_deferred + self.tax_free
    }

    pub fn scale_to_total(&self, total: f64) -> Self {
        let source_total = self.total();
        if total <= 0.0 {
            return Self::default();
        }
        if source_total <= 0.0 {
            return Self {
                taxable: total,
                tax_deferred: 0.0,
                tax_free: 0.0,
            };
        }
        let scale = total / source_total;
        Self {
            taxable: self.taxable * scale,
            tax_deferred: self.tax_deferred * scale,
            tax_free: self.tax_free * scale,
        }
    }
}

/// Simple effective-tax model with optional bucket balances for retirement-only
/// withdrawal ordering. Missing balances fall back to "all taxable".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaxProfile {
    /// Effective rate applied to taxable-account withdrawals (0.0–1.0).
    pub taxable_withdrawal_rate: f64,
    /// Effective rate applied to tax-deferred-account withdrawals (0.0–1.0).
    pub tax_deferred_withdrawal_rate: f64,
    /// Effective rate applied to tax-free-account withdrawals (typically 0.0).
    pub tax_free_withdrawal_rate: f64,
    /// Penalty rate on withdrawals before `early_withdrawal_penalty_age`.
    pub early_withdrawal_penalty_rate: Option<f64>,
    /// Age at which early-withdrawal penalty no longer applies.
    pub early_withdrawal_penalty_age: Option<u32>,
    /// Country code for future locale-specific presets.
    pub country_code: Option<String>,
    /// Retirement-only spendable balances by tax bucket.
    /// Until contribution routing exists, these balances also define the tax-bucket mix for
    /// future contributions.
    #[serde(default)]
    pub withdrawal_buckets: TaxBucketBalances,
}

#[cfg(test)]
mod housing_tests {
    use super::*;

    fn housing_plan(housing: Housing, sell: bool) -> RetirementPlan {
        let mut plan: RetirementPlan = serde_json::from_value(serde_json::json!({
            "personal": { "currentAge": 45, "targetRetirementAge": 60, "planningHorizonAge": 90 },
            "expenses": { "items": [] },
            "incomeStreams": [],
            "investment": {
                "inflationRate": 0.02, "monthlyContribution": 0, "contributionGrowthRate": 0
            },
            "tax": null,
            "currency": "USD"
        }))
        .expect("plan");
        plan.tax_optimizer = Some(TaxOptimizerSettings {
            housing,
            monthly_rent: 2_000.0,
            home_costs: 9_000.0,
            sell_current_home: sell,
            ..TaxOptimizerSettings::default()
        });
        plan
    }

    #[test]
    fn rent_becomes_a_must_have_item_that_ends_at_the_purchase() {
        let mut plan = housing_plan(Housing::Rent, false);
        move_housing_to_expenses(&mut plan);
        let rent = &plan.expenses.items[0];
        assert_eq!(rent.id.as_deref(), Some(RENT_EXPENSE_ID));
        assert_eq!(rent.monthly_amount, 2_000.0);
        assert_eq!(rent.essential, Some(true));
        assert!(rent.ends_at_home_purchase && !rent.yearly);
        let settings = plan.tax_optimizer.as_ref().expect("settings");
        assert_eq!((settings.monthly_rent, settings.home_costs), (0.0, 0.0));

        // Moved once: running it again adds nothing.
        move_housing_to_expenses(&mut plan);
        assert_eq!(plan.expenses.items.len(), 1);

        end_items_at_home_purchase(&mut plan.expenses, 52);
        assert_eq!(plan.expenses.items[0].end_age, Some(52));
    }

    #[test]
    fn home_costs_become_a_yearly_item_that_ends_only_when_the_home_is_sold() {
        for sell in [false, true] {
            let mut plan = housing_plan(Housing::Own, sell);
            move_housing_to_expenses(&mut plan);
            let costs = &plan.expenses.items[0];
            assert_eq!(costs.id.as_deref(), Some(HOME_COSTS_EXPENSE_ID));
            assert!((costs.monthly_amount * 12.0 - 9_000.0).abs() < 1e-9);
            assert!(costs.yearly);
            assert_eq!(costs.ends_at_home_purchase, sell);
        }
    }
}
