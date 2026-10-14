use crate::accounts::{account_supports_purpose, Account, AccountPurpose, AccountServiceTrait};
use crate::errors::Result;
use crate::goals::goals_model::{
    AccountAssetMap, AccountAssetValues, AccountValuationMap, Goal, GoalFundingRule,
    GoalFundingRuleInput, GoalPlan, GoalSummaryUpdate, NewGoal, PreparedRetirementSimulationInput,
    SaveGoalPlan, TaxPlanInput,
};
use crate::goals::goals_traits::{GoalRepositoryTrait, GoalServiceTrait};

/// The stored planner mode of the tax-optimized retirement style.
const PLANNER_MODE_OPTIMIZED: &str = "optimized";
use crate::planning::retirement::{
    end_items_at_home_purchase, move_housing_to_expenses, normalize_retirement_plan_ages,
    optimize_tax_plan, tax_plan_choice_costs, tax_plan_conversion_regret, tax_plan_safe_spending,
    tax_plan_stress_tests, tax_plan_success, try_compute_required_capital, AssetMix, BucketMixes,
    RetirementPlan, RetirementTimingMode, SuccessPathMode, TaxBucketBalances, TaxOptimizedPlan,
    TaxOptimizerAccounts, TaxPlanChoiceCosts, TaxPlanConversionRegret, TaxPlanSafeSpending,
    TaxPlanStressTests, TaxPlanSuccess, TaxProfile,
};
use crate::planning::{
    home_purchase_from_settings, validate_save_up_input, LinkedHomePurchase, SaveUpInput,
    SaveUpOverview,
};
use crate::portfolio::allocation::AllocationServiceTrait;
use crate::portfolio::fire::{compute_retirement_overview_with_mode, RetirementOverview};
use crate::utils::time_utils::{parse_user_timezone_or_default, user_today};
use async_trait::async_trait;
use chrono::{Months, NaiveDate};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

const GOAL_LIFECYCLE_ACTIVE: &str = "active";
const GOAL_LIFECYCLE_ACHIEVED: &str = "achieved";
const GOAL_LIFECYCLE_ARCHIVED: &str = "archived";
const RETIREMENT_MIN_ANNUAL_RETURN: f64 = -0.20;
const RETIREMENT_MAX_ANNUAL_RETURN: f64 = 0.50;
const RETIREMENT_MAX_ANNUAL_INVESTMENT_FEE: f64 = 0.10;
const RETIREMENT_MAX_DC_PAYOUT_RATE: f64 = 0.25;
const RETIREMENT_MAX_ANNUAL_VOLATILITY: f64 = 1.0;

// ─── Shared helpers ──────────────────────────────────────────────────────────

fn spending_scenario_target(plan: &RetirementPlan, spending_factor: f64) -> Option<f64> {
    let mut scenario = plan.clone();
    for item in &mut scenario.expenses.items {
        item.monthly_amount *= spending_factor;
    }
    try_compute_required_capital(&scenario, scenario.personal.target_retirement_age)
}

fn extract_plan_dc_linked_account_ids(plan: &RetirementPlan) -> HashSet<String> {
    plan.income_streams
        .iter()
        .filter(|s| s.stream_type == crate::planning::retirement::StreamKind::DefinedContribution)
        .filter_map(|s| s.linked_account_id.clone())
        .collect()
}

fn compute_goal_value_from_shares(
    funding_rules: &[GoalFundingRule],
    valuations: &AccountValuationMap,
) -> f64 {
    let value: f64 = funding_rules
        .iter()
        .filter_map(|r| {
            valuations
                .get(&r.account_id)
                .map(|&v| v * r.share_percent / 100.0)
        })
        .sum();
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn build_account_share_totals(rules: &[GoalFundingRule]) -> HashMap<String, f64> {
    let mut totals = HashMap::new();
    for rule in rules {
        *totals.entry(rule.account_id.clone()).or_default() += rule.share_percent;
    }
    totals
}

fn build_retirement_seed_rules(
    eligible_accounts: &[Account],
    participating_rules: &[GoalFundingRule],
) -> Vec<GoalFundingRuleInput> {
    let existing_share_totals = build_account_share_totals(participating_rules);

    eligible_accounts
        .iter()
        .filter(|a| account_supports_purpose(&a.account_type, AccountPurpose::GoalFunding))
        .filter_map(|a| {
            let remaining_share = (100.0
                - existing_share_totals.get(&a.id).copied().unwrap_or(0.0))
            .clamp(0.0, 100.0);
            (remaining_share > 0.0).then(|| GoalFundingRuleInput {
                account_id: a.id.clone(),
                share_percent: remaining_share,
                tax_bucket: None,
            })
        })
        .collect()
}

fn date_for_plan_age(plan: &RetirementPlan, age: u32, as_of: NaiveDate) -> Option<String> {
    let years = age.checked_sub(plan.personal.current_age)?;
    let today = as_of;
    today
        .checked_add_months(Months::new(years.saturating_mul(12)))
        .map(|date| date.format("%Y-%m-%d").to_string())
}

fn compute_summary_current_value(
    goal: &Goal,
    funding_rules: &[GoalFundingRule],
    valuations: &AccountValuationMap,
) -> f64 {
    if matches!(
        goal.status_lifecycle.as_str(),
        GOAL_LIFECYCLE_ACHIEVED | GOAL_LIFECYCLE_ARCHIVED
    ) {
        goal.summary_current_value
            .unwrap_or_else(|| compute_goal_value_from_shares(funding_rules, valuations))
    } else {
        compute_goal_value_from_shares(funding_rules, valuations)
    }
}

fn retirement_goal_health_from_overview_status(status: &str) -> String {
    match status {
        "achieved" | "on_track" => "on_track".to_string(),
        "at_risk" => "at_risk".to_string(),
        "off_track" => "off_track".to_string(),
        _ => "not_applicable".to_string(),
    }
}

fn invalid_input(message: impl Into<String>) -> Result<()> {
    Err(crate::errors::ValidationError::InvalidInput(message.into()).into())
}

fn validate_finite_range(label: &str, value: f64, min: f64, max: f64) -> Result<()> {
    if !value.is_finite() || value < min || value > max {
        invalid_input(format!(
            "{label} must be between {:.0}% and {:.0}%",
            min * 100.0,
            max * 100.0
        ))
    } else {
        Ok(())
    }
}

fn validate_non_negative_amount(label: &str, value: f64) -> Result<()> {
    if !value.is_finite() || value < 0.0 {
        invalid_input(format!("{label} cannot be negative"))
    } else {
        Ok(())
    }
}

fn validate_goal_lifecycle(status_lifecycle: &str) -> Result<()> {
    if matches!(
        status_lifecycle,
        GOAL_LIFECYCLE_ACTIVE | GOAL_LIFECYCLE_ACHIEVED | GOAL_LIFECYCLE_ARCHIVED
    ) {
        Ok(())
    } else {
        Err(crate::errors::ValidationError::InvalidInput(format!(
            "Unsupported goal lifecycle '{}'",
            status_lifecycle
        ))
        .into())
    }
}

fn validate_goal_funding_capacity(
    goal_id: &str,
    rules: &[GoalFundingRuleInput],
    participating_rules: &[GoalFundingRule],
) -> Result<()> {
    let mut account_totals: HashMap<String, f64> = HashMap::new();
    for r in participating_rules {
        if r.goal_id != goal_id {
            *account_totals.entry(r.account_id.clone()).or_default() += r.share_percent;
        }
    }

    for rule in rules {
        let total = account_totals.entry(rule.account_id.clone()).or_default();
        let used_elsewhere = *total;
        let combined = used_elsewhere + rule.share_percent;
        if combined > 100.0 {
            let max_available = (100.0 - used_elsewhere).max(0.0);
            return Err(crate::errors::ValidationError::InvalidInput(format!(
                "Account '{}' is overallocated: requested {:.1}%, used elsewhere {:.1}%, max available {:.1}%",
                rule.account_id, rule.share_percent, used_elsewhere, max_available
            ))
            .into());
        }
        *total = combined;
    }

    Ok(())
}

fn funding_rules_as_input(rules: Vec<GoalFundingRule>) -> Vec<GoalFundingRuleInput> {
    rules
        .into_iter()
        .map(|rule| GoalFundingRuleInput {
            account_id: rule.account_id,
            share_percent: rule.share_percent,
            tax_bucket: rule.tax_bucket,
        })
        .collect()
}

fn is_supported_tax_bucket(tax_bucket: Option<&str>) -> bool {
    matches!(
        tax_bucket,
        None | Some("taxable")
            | Some("tax_deferred")
            | Some("tax-deferred")
            | Some("tax_free")
            | Some("tax-free")
            | Some("hsa")
    )
}

fn compute_tax_bucket_balances(
    funding_rules: &[GoalFundingRule],
    valuations: &AccountValuationMap,
) -> TaxBucketBalances {
    let mut balances = TaxBucketBalances::default();
    for rule in funding_rules {
        if let Some(&value) = valuations.get(&rule.account_id) {
            let share_value = (value * rule.share_percent / 100.0).max(0.0);
            match rule.tax_bucket.as_deref() {
                Some("tax_deferred") | Some("tax-deferred") => balances.tax_deferred += share_value,
                // Qualified HSA withdrawals are tax-free; the tax-optimized style
                // models the HSA as its own account.
                Some("tax_free") | Some("tax-free") | Some("hsa") => {
                    balances.tax_free += share_value
                }
                Some("taxable") | None => balances.taxable += share_value,
                Some(unknown) => {
                    log::warn!(
                        "Unknown goal funding tax bucket '{unknown}' for rule {}; treating as taxable",
                        rule.id
                    );
                    balances.taxable += share_value;
                }
            }
        }
    }
    balances
}

/// The goal's share of the accounts marked as HSAs.
fn hsa_balance(funding_rules: &[GoalFundingRule], valuations: &AccountValuationMap) -> f64 {
    funding_rules
        .iter()
        .filter(|rule| rule.tax_bucket.as_deref() == Some("hsa"))
        .filter_map(|rule| {
            let value = valuations.get(&rule.account_id)?.max(0.0);
            Some(value * rule.share_percent / 100.0)
        })
        .sum()
}

/// Top-level `asset_classes` categories the tax-optimized plan counts as stocks.
const STOCK_LIKE_ASSET_CLASSES: [&str; 5] = [
    "EQUITY",
    "REAL_ESTATE",
    "COMMODITIES",
    "ALTERNATIVES",
    "DIGITAL_ASSETS",
];

/// Each account's holdings by asset class (the `asset_classes` taxonomy, rolled
/// up to its top level): the `asset_map` for
/// [`GoalServiceTrait::optimize_retirement_tax_plan`].
pub async fn account_asset_map(
    allocation: &dyn AllocationServiceTrait,
    account_ids: &[String],
    base_currency: &str,
) -> Result<AccountAssetMap> {
    use rust_decimal::prelude::ToPrimitive;
    let mut map = AccountAssetMap::new();
    for id in account_ids {
        let contributions = allocation
            .get_holding_contributions_for_taxonomy_for_accounts(
                std::slice::from_ref(id),
                base_currency,
                "asset_classes",
                id,
            )
            .await?;
        let mut values = AccountAssetValues::default();
        for c in &contributions.contributions {
            let value = c.value.to_f64().unwrap_or(0.0).max(0.0);
            match c.category_id.as_str() {
                "CASH" => values.cash += value,
                "FIXED_INCOME" => values.fixed_income += value,
                class if STOCK_LIKE_ASSET_CLASSES.contains(&class) => values.stocks += value,
                _ => values.unclassified += value,
            }
        }
        map.insert(id.clone(), values);
    }
    Ok(map)
}

/// The asset mix of `values`: unclassified holdings count as stocks, and fixed
/// income is split evenly between corporate bonds and Treasury notes.
fn asset_mix(values: &AccountAssetValues) -> AssetMix {
    AssetMix {
        stocks: values.stocks + values.unclassified,
        bonds: values.fixed_income / 2.0,
        notes: values.fixed_income / 2.0,
        cash: values.cash,
    }
}

fn add_values(total: &mut AccountAssetValues, values: &AccountAssetValues, share: f64) {
    total.stocks += values.stocks * share;
    total.fixed_income += values.fixed_income * share;
    total.cash += values.cash * share;
    total.unclassified += values.unclassified * share;
}

/// Each bucket's asset mix from the goal's share of its accounts' holdings
/// (a bucket without holdings takes the goal's overall mix), and the value of
/// holdings without an asset class. `None` when no account has holdings.
fn bucket_mixes(
    funding_rules: &[GoalFundingRule],
    assets: &AccountAssetMap,
) -> (Option<BucketMixes>, f64) {
    let mut buckets = [AccountAssetValues::default(); 4];
    let mut overall = AccountAssetValues::default();
    for rule in funding_rules {
        let Some(values) = assets.get(&rule.account_id) else {
            continue;
        };
        let share = rule.share_percent.max(0.0) / 100.0;
        let bucket = match rule.tax_bucket.as_deref() {
            Some("tax_deferred") | Some("tax-deferred") => 1,
            Some("tax_free") | Some("tax-free") => 2,
            Some("hsa") => 3,
            _ => 0,
        };
        add_values(&mut buckets[bucket], values, share);
        add_values(&mut overall, values, share);
    }
    let Some(overall_mix) = asset_mix(&overall).normalized() else {
        return (None, 0.0);
    };
    let [taxable, tax_deferred, roth, hsa] =
        buckets.map(|values| asset_mix(&values).normalized().unwrap_or(overall_mix));
    (
        Some(BucketMixes {
            taxable,
            tax_deferred,
            roth,
            hsa,
        }),
        overall.unclassified,
    )
}

/// Each account's cost basis in base currency from its latest daily valuation,
/// with cash at full value: the `cost_basis_map` for
/// [`GoalServiceTrait::optimize_retirement_tax_plan`].
pub fn cost_basis_map(
    valuations: &[crate::portfolio::valuation::DailyAccountValuation],
) -> AccountValuationMap {
    use rust_decimal::prelude::ToPrimitive;
    valuations
        .iter()
        .filter_map(|v| {
            let basis = (v.cost_basis_base + v.cash_balance_base).to_f64()?;
            Some((v.account_id.clone(), basis))
        })
        .collect()
}

/// Cost basis of the goal's share of its taxable accounts. An account without a
/// known basis counts at full value (no unrealized gain), and a basis never
/// exceeds the value it belongs to.
fn taxable_cost_basis(
    funding_rules: &[GoalFundingRule],
    valuations: &AccountValuationMap,
    cost_basis: &AccountValuationMap,
) -> f64 {
    funding_rules
        .iter()
        .filter(|rule| matches!(rule.tax_bucket.as_deref(), Some("taxable") | None))
        .filter_map(|rule| {
            let value = valuations.get(&rule.account_id)?.max(0.0);
            let basis = cost_basis
                .get(&rule.account_id)
                .copied()
                .unwrap_or(value)
                .clamp(0.0, value);
            Some(basis * rule.share_percent / 100.0)
        })
        .sum()
}

pub fn validate_retirement_plan(plan: &RetirementPlan) -> Result<()> {
    let p = &plan.personal;
    if p.current_age >= p.planning_horizon_age {
        return invalid_input("Current age must be less than planning horizon");
    }
    if p.target_retirement_age <= p.current_age {
        return invalid_input("Target retirement age must be after current age");
    }
    if p.target_retirement_age > p.planning_horizon_age {
        return invalid_input("Target retirement age must be before planning horizon");
    }
    if let Some(value) = p.current_annual_salary {
        validate_non_negative_amount("Current salary", value)?;
    }
    if let Some(value) = p.salary_growth_rate {
        validate_finite_range("Salary growth", value, -0.50, 0.50)?;
    }
    for (bucket, _) in plan.expenses.all_buckets() {
        validate_non_negative_amount("Retirement spending", bucket.monthly_amount)?;
        if let Some(rate) = bucket.inflation_rate {
            validate_finite_range("Spending inflation override", rate, -0.10, 0.20)?;
        }
        if let (Some(start_age), Some(end_age)) = (bucket.start_age, bucket.end_age) {
            if start_age >= end_age {
                return invalid_input("Spending To age must be after From age");
            }
        }
    }
    let i = &plan.investment;
    validate_finite_range(
        "Return before retirement",
        i.pre_retirement_annual_return,
        RETIREMENT_MIN_ANNUAL_RETURN,
        RETIREMENT_MAX_ANNUAL_RETURN,
    )?;
    validate_finite_range(
        "Return during retirement",
        i.retirement_annual_return,
        RETIREMENT_MIN_ANNUAL_RETURN,
        RETIREMENT_MAX_ANNUAL_RETURN,
    )?;
    validate_finite_range(
        "Annual investment fee",
        i.annual_investment_fee_rate,
        0.0,
        RETIREMENT_MAX_ANNUAL_INVESTMENT_FEE,
    )?;
    validate_finite_range(
        "Annual volatility",
        i.annual_volatility,
        0.0,
        RETIREMENT_MAX_ANNUAL_VOLATILITY,
    )?;
    validate_finite_range("Inflation", i.inflation_rate, -0.10, 0.20)?;
    validate_non_negative_amount("Monthly contribution", i.monthly_contribution)?;
    validate_finite_range(
        "Contribution growth",
        i.contribution_growth_rate,
        -0.50,
        0.50,
    )?;
    if i.pre_retirement_annual_return - i.annual_investment_fee_rate <= -0.99
        || i.retirement_annual_return - i.annual_investment_fee_rate <= -0.99
    {
        return invalid_input("Return after fees must be greater than -99%");
    }
    for stream in &plan.income_streams {
        if stream.start_age > p.planning_horizon_age {
            return invalid_input("Income start age must be within the planning horizon");
        }
        if let Some(value) = stream.monthly_amount {
            validate_non_negative_amount("Income amount", value)?;
        }
        if let Some(value) = stream.annual_growth_rate {
            validate_finite_range("Income growth", value, -0.10, 0.20)?;
        }
        if let Some(value) = stream.current_value {
            validate_non_negative_amount("Defined-contribution current value", value)?;
        }
        if let Some(value) = stream.monthly_contribution {
            validate_non_negative_amount("Defined-contribution contribution", value)?;
        }
        if let Some(value) = stream.accumulation_return {
            validate_finite_range(
                "Defined-contribution return",
                value,
                RETIREMENT_MIN_ANNUAL_RETURN,
                RETIREMENT_MAX_ANNUAL_RETURN,
            )?;
        }
        if let Some(value) = stream.payout_rate {
            validate_finite_range(
                "Defined-contribution payout rate",
                value,
                0.0,
                RETIREMENT_MAX_DC_PAYOUT_RATE,
            )?;
        }
        if let Some(value) = stream.post_payout_return {
            validate_finite_range(
                "Defined-contribution return during payout",
                value,
                RETIREMENT_MIN_ANNUAL_RETURN,
                RETIREMENT_MAX_ANNUAL_RETURN,
            )?;
        }
        if stream.is_drawdown()
            && stream.current_value.unwrap_or(0.0) <= 0.0
            && stream.monthly_contribution.unwrap_or(0.0) <= 0.0
        {
            return invalid_input(
                "A drawdown fund needs a balance or contributions to draw against",
            );
        }
    }
    if let Some(ref tax) = plan.tax {
        validate_finite_range(
            "Taxable withdrawal tax rate",
            tax.taxable_withdrawal_rate,
            0.0,
            1.0,
        )?;
        validate_finite_range(
            "Tax-deferred withdrawal tax rate",
            tax.tax_deferred_withdrawal_rate,
            0.0,
            1.0,
        )?;
        validate_finite_range(
            "Tax-free withdrawal tax rate",
            tax.tax_free_withdrawal_rate,
            0.0,
            1.0,
        )?;
        if let Some(rate) = tax.early_withdrawal_penalty_rate {
            validate_finite_range("Early withdrawal penalty", rate, 0.0, 1.0)?;
        }
        if let Some(age) = tax.early_withdrawal_penalty_age {
            if age > p.planning_horizon_age {
                return invalid_input(
                    "Early withdrawal penalty age must be within the planning horizon",
                );
            }
        }
    }
    // Reject duplicate linkedAccountId values across DC streams
    let dc_linked_ids: Vec<&str> = plan
        .income_streams
        .iter()
        .filter(|s| s.stream_type == crate::planning::retirement::StreamKind::DefinedContribution)
        .filter_map(|s| s.linked_account_id.as_deref())
        .collect();
    let mut seen = HashSet::new();
    for id in &dc_linked_ids {
        if !seen.insert(*id) {
            return Err(crate::errors::ValidationError::InvalidInput(format!(
                "Duplicate linked account '{}' across DC income streams",
                id
            ))
            .into());
        }
    }
    Ok(())
}

pub struct GoalService<T: GoalRepositoryTrait> {
    goal_repo: Arc<T>,
    account_service: Arc<dyn AccountServiceTrait>,
    timezone: Arc<RwLock<String>>,
}

impl<T: GoalRepositoryTrait> GoalService<T> {
    pub fn new(goal_repo: Arc<T>, account_service: Arc<dyn AccountServiceTrait>) -> Self {
        GoalService {
            goal_repo,
            account_service,
            timezone: Arc::new(RwLock::new(String::new())),
        }
    }

    pub fn with_timezone(mut self, timezone: Arc<RwLock<String>>) -> Self {
        self.timezone = timezone;
        self
    }

    fn today(&self) -> chrono::NaiveDate {
        user_today(parse_user_timezone_or_default(
            &self
                .timezone
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        ))
    }

    fn validate_goal_funding_accounts(&self, account_ids: &HashSet<String>) -> Result<()> {
        if account_ids.is_empty() {
            return Ok(());
        }

        let requested: Vec<String> = account_ids.iter().cloned().collect();
        let accounts = self.account_service.get_accounts_by_ids(&requested)?;
        let accounts_by_id: HashMap<String, Account> = accounts
            .into_iter()
            .map(|account| (account.id.clone(), account))
            .collect();

        for account_id in requested {
            let account = accounts_by_id.get(&account_id).ok_or_else(|| {
                crate::errors::ValidationError::InvalidInput(format!(
                    "Account '{}' does not exist",
                    account_id
                ))
            })?;
            if !account_supports_purpose(&account.account_type, AccountPurpose::GoalFunding) {
                return Err(crate::errors::ValidationError::InvalidInput(format!(
                    "Account '{}' is not eligible for goal funding",
                    account_id
                ))
                .into());
            }
        }

        Ok(())
    }

    fn prepare_retirement_input(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        as_of: NaiveDate,
    ) -> Result<PreparedRetirementSimulationInput> {
        let goal = self.goal_repo.load_goal(goal_id)?;
        if goal.goal_type != "retirement" {
            return Err(crate::errors::ValidationError::InvalidInput(format!(
                "Goal {} is not a retirement goal",
                goal_id
            ))
            .into());
        }

        let stored_plan = self.goal_repo.load_goal_plan(goal_id)?.ok_or_else(|| {
            crate::errors::ValidationError::InvalidInput(format!(
                "No plan found for goal {}",
                goal_id
            ))
        })?;
        let funding_rules = self.goal_repo.load_funding_rules(goal_id)?;
        let planner_mode =
            RetirementTimingMode::from_str(stored_plan.planner_mode.as_deref().unwrap_or("fire"));

        let mut retirement_plan: RetirementPlan = serde_json::from_str(&stored_plan.settings_json)?;
        normalize_retirement_plan_ages(&mut retirement_plan, as_of);
        validate_retirement_plan(&retirement_plan)?;

        let mut funding_rules = funding_rules;
        move_housing_to_expenses(&mut retirement_plan);
        let home_goal_id = retirement_plan
            .tax_optimizer
            .as_ref()
            .and_then(|settings| settings.home_goal_id.clone());
        match self.linked_home(home_goal_id.as_deref(), &funding_rules) {
            Ok(Some((home, home_rules))) => {
                let current_age = retirement_plan.personal.current_age;
                if let Some(age) = home.purchase_age(current_age, as_of) {
                    end_items_at_home_purchase(&mut retirement_plan.expenses, age);
                }
                // The tax-optimized style models the purchase itself (see
                // `tax_optimizer_input`); the FIRE and Traditional styles see
                // it as expenses, with any savings the home goal holds in the
                // portfolio.
                if stored_plan.planner_mode.as_deref() != Some(PLANNER_MODE_OPTIMIZED) {
                    let items = home.expense_items(current_age, as_of);
                    retirement_plan.expenses.items.extend(items);
                    funding_rules.extend(home_rules);
                }
            }
            Ok(None) => {}
            // An unfinished home goal must not break the retirement plan.
            Err(err) => log::warn!("Linked home goal left out of the plan: {err}"),
        }

        let current_portfolio = compute_goal_value_from_shares(&funding_rules, valuation_map);
        let bucket_balances = compute_tax_bucket_balances(&funding_rules, valuation_map);

        let tax = retirement_plan.tax.get_or_insert(TaxProfile {
            taxable_withdrawal_rate: 0.0,
            tax_deferred_withdrawal_rate: 0.0,
            tax_free_withdrawal_rate: 0.0,
            early_withdrawal_penalty_rate: None,
            early_withdrawal_penalty_age: None,
            country_code: None,
            withdrawal_buckets: TaxBucketBalances::default(),
        });
        tax.withdrawal_buckets = bucket_balances;

        Ok(PreparedRetirementSimulationInput {
            plan: retirement_plan,
            current_portfolio,
            planner_mode,
        })
    }

    /// The purchase of the active home goal `home_goal_id` and its funding
    /// rules, each in the bucket the retirement goal gives that account (the
    /// down payment savings join the plan's accounts). `None` when no active
    /// home goal is linked.
    fn linked_home(
        &self,
        home_goal_id: Option<&str>,
        retirement_rules: &[GoalFundingRule],
    ) -> Result<Option<(LinkedHomePurchase, Vec<GoalFundingRule>)>> {
        let Some(id) = home_goal_id else {
            return Ok(None);
        };
        let Ok(goal) = self.goal_repo.load_goal(id) else {
            return Ok(None);
        };
        if goal.goal_type != "home" || goal.status_lifecycle != GOAL_LIFECYCLE_ACTIVE {
            return Ok(None);
        }
        let settings_json = self
            .goal_repo
            .load_goal_plan(id)?
            .map(|plan| plan.settings_json)
            .unwrap_or_default();
        let purchase = home_purchase_from_settings(&settings_json).ok_or_else(|| {
            crate::errors::ValidationError::InvalidInput(format!(
                "Enter the home's price and mortgage on the \"{}\" goal, or unlink it",
                goal.title
            ))
        })?;
        let target_date = goal
            .target_date
            .clone()
            .filter(|d| !d.is_empty())
            .or_else(|| {
                serde_json::from_str::<serde_json::Value>(&settings_json)
                    .ok()
                    .and_then(|v| v.get("targetDate")?.as_str().map(str::to_string))
            });
        let rules = self
            .goal_repo
            .load_funding_rules(id)?
            .into_iter()
            .map(|mut rule| {
                rule.tax_bucket = retirement_rules
                    .iter()
                    .find(|r| r.account_id == rule.account_id)
                    .and_then(|r| r.tax_bucket.clone());
                rule
            })
            .collect();
        Ok(Some((
            LinkedHomePurchase {
                goal_title: goal.title,
                purchase,
                target_date,
            },
            rules,
        )))
    }

    /// The saved plan and the goal's accounts by tax treatment, for the
    /// tax-optimized style.
    fn tax_optimizer_input(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
    ) -> Result<(RetirementPlan, TaxOptimizerAccounts, NaiveDate)> {
        let as_of = self.today();
        let mut prepared = self.prepare_retirement_input(goal_id, valuation_map, as_of)?;
        let mut funding_rules = self.goal_repo.load_funding_rules(goal_id)?;
        if let Some(settings) = prepared.plan.tax_optimizer.as_mut() {
            if let Some((home, home_rules)) =
                self.linked_home(settings.home_goal_id.as_deref(), &funding_rules)?
            {
                settings.home_purchase = Some(home);
                funding_rules.extend(home_rules);
            }
        }
        let buckets = compute_tax_bucket_balances(&funding_rules, valuation_map);
        let hsa = hsa_balance(&funding_rules, valuation_map);
        let (mixes, unclassified) = bucket_mixes(&funding_rules, asset_map);
        let accounts = TaxOptimizerAccounts {
            taxable: buckets.taxable,
            taxable_cost_basis: taxable_cost_basis(&funding_rules, valuation_map, cost_basis_map),
            tax_deferred: buckets.tax_deferred,
            roth: (buckets.tax_free - hsa).max(0.0),
            hsa,
            mixes,
            unclassified,
        };
        Ok((prepared.plan, accounts, as_of))
    }
}

#[async_trait]
impl<T: GoalRepositoryTrait + Send + Sync> GoalServiceTrait for GoalService<T> {
    fn get_goals(&self) -> Result<Vec<Goal>> {
        self.goal_repo.load_goals()
    }

    fn get_goal(&self, goal_id: &str) -> Result<Goal> {
        self.goal_repo.load_goal(goal_id)
    }

    async fn create_goal(&self, new_goal: NewGoal) -> Result<Goal> {
        let is_retirement = new_goal.goal_type == "retirement";
        if let Some(status_lifecycle) = new_goal.status_lifecycle.as_deref() {
            validate_goal_lifecycle(status_lifecycle)?;
        }

        if is_retirement {
            let goals = self.goal_repo.load_goals()?;
            let existing = goals.iter().any(|g| {
                g.goal_type == "retirement" && g.status_lifecycle != GOAL_LIFECYCLE_ARCHIVED
            });
            if existing {
                return Err(crate::errors::ValidationError::InvalidInput(
                    "Only one active retirement goal is allowed".to_string(),
                )
                .into());
            }
        }

        if is_retirement {
            let eligible = self.account_service.get_active_non_archived_accounts()?;
            let participating_rules = self.goal_repo.load_participating_funding_rules()?;
            let seed_rules = build_retirement_seed_rules(&eligible, &participating_rules);
            self.goal_repo
                .insert_goal_with_funding(new_goal, seed_rules)
                .await
        } else {
            self.goal_repo.insert_new_goal(new_goal).await
        }
    }

    async fn update_goal(&self, updated_goal_data: Goal) -> Result<Goal> {
        let existing = self.goal_repo.load_goal(&updated_goal_data.id)?;
        if existing.goal_type != updated_goal_data.goal_type {
            return Err(crate::errors::ValidationError::InvalidInput(
                "Goal type cannot be changed after creation".to_string(),
            )
            .into());
        }
        validate_goal_lifecycle(&updated_goal_data.status_lifecycle)?;
        if updated_goal_data.goal_type == "retirement"
            && updated_goal_data.status_lifecycle != GOAL_LIFECYCLE_ARCHIVED
        {
            let goals = self.goal_repo.load_goals()?;
            let existing_retirement = goals.iter().any(|g| {
                g.id != updated_goal_data.id
                    && g.goal_type == "retirement"
                    && g.status_lifecycle != GOAL_LIFECYCLE_ARCHIVED
            });
            if existing_retirement {
                return Err(crate::errors::ValidationError::InvalidInput(
                    "Only one active retirement goal is allowed".to_string(),
                )
                .into());
            }
        }
        if existing.status_lifecycle != GOAL_LIFECYCLE_ACTIVE
            && updated_goal_data.status_lifecycle == GOAL_LIFECYCLE_ACTIVE
        {
            let rules =
                funding_rules_as_input(self.goal_repo.load_funding_rules(&updated_goal_data.id)?);
            let participating_rules = self.goal_repo.load_participating_funding_rules()?;
            validate_goal_funding_capacity(&updated_goal_data.id, &rules, &participating_rules)?;
        }
        self.goal_repo.update_goal(updated_goal_data).await
    }

    async fn delete_goal(&self, goal_id_to_delete: String) -> Result<usize> {
        self.goal_repo.delete_goal(goal_id_to_delete).await
    }

    fn get_goal_funding(&self, goal_id: &str) -> Result<Vec<GoalFundingRule>> {
        self.goal_repo.load_funding_rules(goal_id)
    }

    async fn save_goal_funding(
        &self,
        goal_id: &str,
        rules: Vec<GoalFundingRuleInput>,
    ) -> Result<Vec<GoalFundingRule>> {
        let goal = self.goal_repo.load_goal(goal_id)?;
        let is_retirement = goal.goal_type == "retirement";

        // Reject duplicate accountId entries
        let mut seen_accounts = HashSet::new();
        for rule in &rules {
            if !seen_accounts.insert(rule.account_id.clone()) {
                return Err(crate::errors::ValidationError::InvalidInput(format!(
                    "Duplicate account '{}' in funding rules",
                    rule.account_id
                ))
                .into());
            }
        }

        // Validate share_percent range
        for rule in &rules {
            if !(0.0..=100.0).contains(&rule.share_percent) {
                return Err(crate::errors::ValidationError::InvalidInput(
                    "share_percent must be between 0 and 100".to_string(),
                )
                .into());
            }
        }
        if is_retirement {
            for rule in &rules {
                if !is_supported_tax_bucket(rule.tax_bucket.as_deref()) {
                    return Err(crate::errors::ValidationError::InvalidInput(format!(
                        "Unsupported tax bucket '{}'",
                        rule.tax_bucket.as_deref().unwrap_or_default()
                    ))
                    .into());
                }
            }
        }

        self.validate_goal_funding_accounts(&seen_accounts)?;

        // Clear tax_bucket for non-retirement goals
        let rules: Vec<GoalFundingRuleInput> = if is_retirement {
            rules
        } else {
            rules
                .into_iter()
                .map(|mut r| {
                    r.tax_bucket = None;
                    r
                })
                .collect()
        };

        // Validate DC-linked accounts not in rules
        if is_retirement {
            if let Ok(Some(plan)) = self.goal_repo.load_goal_plan(goal_id) {
                if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&plan.settings_json)
                {
                    if let Some(streams) = settings.get("incomeStreams").and_then(|s| s.as_array())
                    {
                        for stream in streams {
                            if let Some(linked) =
                                stream.get("linkedAccountId").and_then(|v| v.as_str())
                            {
                                if rules.iter().any(|r| r.account_id == linked) {
                                    return Err(crate::errors::ValidationError::InvalidInput(
                                        format!("Account '{}' is linked to a pension fund and cannot be added to funding rules", linked),
                                    ).into());
                                }
                            }
                        }
                    }
                }
            }
        }

        let participating_rules = self.goal_repo.load_participating_funding_rules()?;
        validate_goal_funding_capacity(goal_id, &rules, &participating_rules)?;

        self.goal_repo.save_goal_funding(goal_id, rules).await
    }

    fn get_goal_plan(&self, goal_id: &str) -> Result<Option<GoalPlan>> {
        self.goal_repo.load_goal_plan(goal_id)
    }

    async fn save_goal_plan(&self, mut plan: SaveGoalPlan) -> Result<GoalPlan> {
        let goal = self.goal_repo.load_goal(&plan.goal_id)?;
        let valid = match goal.goal_type.as_str() {
            "retirement" => plan.plan_kind == "retirement",
            _ => plan.plan_kind == "save_up",
        };
        if !valid {
            return Err(crate::errors::ValidationError::InvalidInput(format!(
                "Plan kind '{}' is not valid for goal type '{}'",
                plan.plan_kind, goal.goal_type
            ))
            .into());
        }
        if plan.planner_mode.is_some() && plan.plan_kind != "retirement" {
            return Err(crate::errors::ValidationError::InvalidInput(
                "planner_mode is only valid for retirement plans".to_string(),
            )
            .into());
        }
        if plan.plan_kind == "retirement" {
            let mut settings_json: serde_json::Value = serde_json::from_str(&plan.settings_json)
                .map_err(|e| {
                    crate::errors::ValidationError::InvalidInput(format!(
                        "Invalid retirement plan JSON: {}",
                        e
                    ))
                })?;
            let mut retirement_plan: RetirementPlan = serde_json::from_value(settings_json.clone())
                .map_err(|e| {
                    crate::errors::ValidationError::InvalidInput(format!(
                        "Invalid retirement plan JSON: {}",
                        e
                    ))
                })?;
            normalize_retirement_plan_ages(&mut retirement_plan, self.today());
            validate_retirement_plan(&retirement_plan)?;

            // Reject linking a DC stream to an account that has participating shares
            let dc_linked = extract_plan_dc_linked_account_ids(&retirement_plan);
            if !dc_linked.is_empty() {
                self.validate_goal_funding_accounts(&dc_linked)?;
                let participating = self.goal_repo.load_participating_funding_rules()?;
                for account_id in &dc_linked {
                    if participating.iter().any(|r| r.account_id == *account_id) {
                        return Err(crate::errors::ValidationError::InvalidInput(format!(
                            "Account '{}' has participating goal shares and cannot be linked as DC",
                            account_id
                        ))
                        .into());
                    }
                }
            }
            if let Some(personal) = settings_json
                .get_mut("personal")
                .and_then(|value| value.as_object_mut())
            {
                personal.insert(
                    "currentAge".to_string(),
                    serde_json::Value::from(retirement_plan.personal.current_age),
                );
            }
            plan.settings_json = serde_json::to_string(&settings_json)?;
        }

        self.goal_repo.save_goal_plan(plan).await
    }

    async fn delete_goal_plan(&self, goal_id: &str) -> Result<usize> {
        self.goal_repo.delete_goal_plan(goal_id).await
    }

    async fn refresh_goal_summary(
        &self,
        goal_id: &str,
        valuations: &AccountValuationMap,
    ) -> Result<Goal> {
        let goal = self.goal_repo.load_goal(goal_id)?;
        let rules = self.goal_repo.load_funding_rules(goal_id)?;
        let is_retirement = goal.goal_type == "retirement";

        let current_value = compute_summary_current_value(&goal, &rules, valuations);
        let has_retirement_plan = if is_retirement {
            self.goal_repo.load_goal_plan(goal_id)?.is_some()
        } else {
            false
        };

        let as_of = self.today();
        let retirement_summary = if has_retirement_plan {
            let prepared = self.prepare_retirement_input(goal_id, valuations, as_of)?;
            let overview = compute_retirement_overview_with_mode(
                &prepared.plan,
                prepared.current_portfolio,
                prepared.planner_mode,
                as_of,
            );
            let completion_age = match prepared.planner_mode {
                RetirementTimingMode::Fire => overview
                    .fi_age
                    .or(overview.suggested_goal_age_if_unchanged)
                    .unwrap_or(prepared.plan.personal.target_retirement_age),
                RetirementTimingMode::Traditional => prepared.plan.personal.target_retirement_age,
            };
            Some((
                if overview.required_capital_reachable {
                    Some(overview.required_capital_at_goal_age)
                } else {
                    None
                },
                date_for_plan_age(&prepared.plan, completion_age, as_of),
                overview.portfolio_at_goal_age,
                overview.status,
            ))
        } else {
            None
        };

        // Determine target
        let target = if is_retirement {
            if has_retirement_plan {
                retirement_summary
                    .as_ref()
                    .and_then(|(target, _, _, _)| *target)
                    .or(goal.summary_target_amount)
            } else {
                None
            }
        } else {
            goal.target_amount.or(goal.summary_target_amount)
        };

        let progress = match target {
            Some(t) if t > 0.0 => Some((current_value / t).min(1.0_f64)),
            _ => None,
        };

        let projected_value_at_target_date = if is_retirement && !has_retirement_plan {
            None
        } else {
            retirement_summary
                .as_ref()
                .map(|(_, _, projected, _)| *projected)
                .or(goal.projected_value_at_target_date)
        };

        let health = if goal.status_lifecycle == GOAL_LIFECYCLE_ACHIEVED {
            "on_track".to_string()
        } else if is_retirement {
            retirement_summary
                .as_ref()
                .map(|(_, _, _, status)| retirement_goal_health_from_overview_status(status))
                .unwrap_or_else(|| "not_applicable".to_string())
        } else {
            match (projected_value_at_target_date, target) {
                (Some(proj), Some(t)) if t > 0.0 => {
                    let ratio = proj / t;
                    if ratio >= 1.0 {
                        "on_track"
                    } else if ratio >= 0.9 {
                        "at_risk"
                    } else {
                        "off_track"
                    }
                    .to_string()
                }
                _ => "not_applicable".to_string(),
            }
        };

        let update = GoalSummaryUpdate {
            summary_target_amount: target,
            summary_current_value: Some(current_value),
            summary_progress: progress,
            projected_completion_date: if is_retirement && !has_retirement_plan {
                None
            } else {
                retirement_summary
                    .as_ref()
                    .and_then(|(_, date, _, _)| date.clone())
                    .or(goal.projected_completion_date.clone())
            },
            projected_value_at_target_date,
            status_health: health,
        };

        self.goal_repo
            .update_goal_summary_fields(goal_id, update)
            .await?;
        self.goal_repo.load_goal(goal_id)
    }

    async fn compute_retirement_overview(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
    ) -> Result<RetirementOverview> {
        let as_of = self.today();
        let prepared = self.prepare_retirement_input(goal_id, valuation_map, as_of)?;
        let mut overview = compute_retirement_overview_with_mode(
            &prepared.plan,
            prepared.current_portfolio,
            prepared.planner_mode,
            as_of,
        );

        if matches!(prepared.planner_mode, RetirementTimingMode::Fire) {
            overview.lean_required_capital_at_goal_age =
                spending_scenario_target(&prepared.plan, 0.7);
            overview.fat_required_capital_at_goal_age =
                spending_scenario_target(&prepared.plan, 1.5);
        }
        Ok(overview)
    }

    async fn prepare_retirement_simulation_input(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
    ) -> Result<PreparedRetirementSimulationInput> {
        self.prepare_retirement_input(goal_id, valuation_map, self.today())
    }

    async fn retirement_tax_plan_accounts(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
    ) -> Result<TaxOptimizerAccounts> {
        let (_, accounts, _) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        Ok(accounts)
    }

    async fn retirement_tax_plan_input(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
    ) -> Result<TaxPlanInput> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        Ok(TaxPlanInput {
            plan,
            accounts,
            as_of,
        })
    }

    async fn optimize_retirement_tax_plan(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
    ) -> Result<TaxOptimizedPlan> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        crate::portfolio::coordinator::blocking(move || optimize_tax_plan(&plan, &accounts, as_of))
            .await
    }

    async fn retirement_tax_plan_success(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        mode: SuccessPathMode,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanSuccess> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        crate::portfolio::coordinator::blocking(move || {
            tax_plan_success(&plan, &accounts, as_of, mode, retirement_age)
        })
        .await
    }

    async fn retirement_tax_plan_safe_spending(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        mode: SuccessPathMode,
        retirement_age: Option<u32>,
        target_rate: f64,
    ) -> Result<TaxPlanSafeSpending> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        crate::portfolio::coordinator::blocking(move || {
            tax_plan_safe_spending(&plan, &accounts, as_of, mode, retirement_age, target_rate)
        })
        .await
    }

    async fn retirement_tax_plan_choice_costs(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanChoiceCosts> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        crate::portfolio::coordinator::blocking(move || {
            tax_plan_choice_costs(&plan, &accounts, as_of, retirement_age)
        })
        .await
    }

    async fn retirement_tax_plan_stress_tests(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanStressTests> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        crate::portfolio::coordinator::blocking(move || {
            tax_plan_stress_tests(&plan, &accounts, as_of, retirement_age)
        })
        .await
    }

    async fn retirement_tax_plan_conversion_regret(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        mode: SuccessPathMode,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanConversionRegret> {
        let (plan, accounts, as_of) =
            self.tax_optimizer_input(goal_id, valuation_map, cost_basis_map, asset_map)?;
        crate::portfolio::coordinator::blocking(move || {
            tax_plan_conversion_regret(&plan, &accounts, as_of, mode, retirement_age)
        })
        .await
    }

    async fn compute_save_up_overview(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
    ) -> Result<SaveUpOverview> {
        let goal = self.goal_repo.load_goal(goal_id)?;
        let plan = self.goal_repo.load_goal_plan(goal_id)?;
        let funding_rules = self.goal_repo.load_funding_rules(goal_id)?;

        let current_value = compute_summary_current_value(&goal, &funding_rules, valuation_map);

        // Parse settings from plan if it exists
        let (monthly_contribution, expected_return) = if let Some(p) = &plan {
            let settings: serde_json::Value = serde_json::from_str(&p.settings_json)?;
            (
                settings
                    .get("monthlyContribution")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                settings
                    .get("expectedAnnualReturn")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.07),
            )
        } else {
            (0.0, 0.07)
        };

        let input = SaveUpInput {
            // Save-up plans do not carry a separate currency. The web and Tauri goal write paths
            // force goal amounts to the configured base currency, matching valuation_map values.
            current_value,
            target_amount: goal.target_amount.unwrap_or(0.0),
            target_date: goal.target_date.clone(),
            monthly_contribution,
            expected_annual_return: expected_return,
        };

        let as_of = self.today();
        validate_save_up_input(&input, as_of)?;
        Ok(crate::planning::compute_save_up_overview(&input, as_of))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{AccountUpdate, NewAccount};
    use crate::goals::goals_model::GoalFundingRule;
    use crate::goals::{GoalRepositoryTrait, GoalServiceTrait};
    use crate::planning::retirement::*;
    use async_trait::async_trait;
    use chrono::NaiveDateTime;
    use std::sync::Mutex;

    fn test_account(id: &str, account_type: &str) -> Account {
        Account {
            id: id.into(),
            name: format!("Account {id}"),
            account_type: account_type.into(),
            group: None,
            currency: "USD".into(),
            is_default: false,
            is_active: true,
            created_at: NaiveDateTime::default(),
            updated_at: NaiveDateTime::default(),
            platform_id: None,
            account_number: None,
            meta: None,
            provider: None,
            provider_account_id: None,
            is_archived: false,
            tracking_mode: crate::accounts::TrackingMode::NotSet,
        }
    }

    fn test_goal(status_lifecycle: &str, summary_current_value: Option<f64>) -> Goal {
        Goal {
            id: "goal-1".into(),
            goal_type: "custom_save_up".into(),
            title: "Goal".into(),
            description: None,
            target_amount: Some(100_000.0),
            status_lifecycle: status_lifecycle.into(),
            status_health: "not_applicable".into(),
            priority: 0,
            cover_image_key: None,
            currency: Some("USD".into()),
            start_date: None,
            target_date: None,
            summary_current_value,
            summary_progress: None,
            projected_completion_date: None,
            projected_value_at_target_date: None,
            created_at: String::new(),
            updated_at: String::new(),
            summary_target_amount: None,
        }
    }

    fn share_rule(goal_id: &str, account_id: &str, pct: f64) -> GoalFundingRule {
        GoalFundingRule {
            id: format!("r-{}-{}", goal_id, account_id),
            goal_id: goal_id.into(),
            account_id: account_id.into(),
            share_percent: pct,
            tax_bucket: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn valid_retirement_plan() -> RetirementPlan {
        RetirementPlan {
            version: None,
            personal: PersonalProfile {
                birth_year_month: None,
                current_age: 45,
                target_retirement_age: 55,
                planning_horizon_age: 90,
                current_annual_salary: None,
                salary_growth_rate: None,
            },
            expenses: ExpenseBudget {
                items: vec![ExpenseBucket {
                    id: None,
                    label: None,
                    monthly_amount: 3_000.0,
                    inflation_rate: None,
                    start_age: None,
                    end_age: None,
                    essential: Some(true),
                    paid_while_working: false,
                    yearly: false,
                    ends_at_home_purchase: false,
                }],
            },
            income_streams: vec![],
            investment: InvestmentAssumptions {
                pre_retirement_annual_return: 0.0577,
                retirement_annual_return: 0.0337,
                annual_investment_fee_rate: 0.006,
                annual_volatility: 0.12,
                inflation_rate: 0.02,
                monthly_contribution: 1_000.0,
                contribution_growth_rate: 0.0,
                glide_path: None,
            },
            tax: None,
            currency: "USD".into(),
            tax_optimizer: None,
        }
    }

    #[test]
    fn spending_scenario_targets_preserve_income_and_original_plan() {
        let mut plan = valid_retirement_plan();
        plan.investment.inflation_rate = 0.0;
        plan.investment.retirement_annual_return = 0.0;
        plan.investment.annual_investment_fee_rate = 0.0;
        plan.income_streams.push(RetirementIncomeStream {
            id: "pension".into(),
            label: "Pension".into(),
            stream_type: StreamKind::DefinedBenefit,
            start_age: 55,
            annual_growth_rate: None,
            adjust_for_inflation: false,
            monthly_amount: Some(1_500.0),
            linked_account_id: None,
            current_value: None,
            monthly_contribution: None,
            accumulation_return: None,
            payout_rate: None,
            payout_mode: None,
            post_payout_return: None,
        });
        let original = plan.clone();
        let base = try_compute_required_capital(&plan, 55).unwrap();
        let lean = spending_scenario_target(&plan, 0.7).unwrap();
        let fat = spending_scenario_target(&plan, 1.5).unwrap();
        // Expenses change; pension income does not. Net spending is 40% / 200%
        // of the base plan here, rather than 70% / 150% of its capital target.
        assert!((lean / base - 0.4).abs() < 0.000_001);
        assert!((fat / base - 2.0).abs() < 0.000_001);
        assert_eq!(plan, original);
    }

    fn retirement_goal(id: &str, status_lifecycle: &str) -> Goal {
        Goal {
            id: id.into(),
            goal_type: "retirement".into(),
            title: "Retirement".into(),
            description: None,
            target_amount: None,
            status_lifecycle: status_lifecycle.into(),
            status_health: "not_applicable".into(),
            priority: 0,
            cover_image_key: None,
            currency: Some("USD".into()),
            start_date: None,
            target_date: None,
            summary_current_value: None,
            summary_progress: None,
            projected_completion_date: None,
            projected_value_at_target_date: None,
            created_at: String::new(),
            updated_at: String::new(),
            summary_target_amount: None,
        }
    }

    fn not_found(id: &str) -> crate::errors::Error {
        crate::errors::DatabaseError::NotFound(id.to_string()).into()
    }

    #[derive(Default)]
    struct MockGoalRepository {
        goals: Mutex<HashMap<String, Goal>>,
        funding_rules: Mutex<HashMap<String, Vec<GoalFundingRule>>>,
        plans: Mutex<HashMap<String, GoalPlan>>,
    }

    impl MockGoalRepository {
        fn new(goals: Vec<Goal>) -> Self {
            Self {
                goals: Mutex::new(goals.into_iter().map(|g| (g.id.clone(), g)).collect()),
                funding_rules: Mutex::new(HashMap::new()),
                plans: Mutex::new(HashMap::new()),
            }
        }

        fn set_funding_rules(&self, goal_id: &str, rules: Vec<GoalFundingRule>) {
            self.funding_rules
                .lock()
                .unwrap()
                .insert(goal_id.to_string(), rules);
        }
    }

    #[async_trait]
    impl GoalRepositoryTrait for MockGoalRepository {
        fn load_goals(&self) -> Result<Vec<Goal>> {
            Ok(self.goals.lock().unwrap().values().cloned().collect())
        }

        fn load_goal(&self, goal_id: &str) -> Result<Goal> {
            self.goals
                .lock()
                .unwrap()
                .get(goal_id)
                .cloned()
                .ok_or_else(|| not_found(goal_id))
        }

        async fn insert_new_goal(&self, new_goal: NewGoal) -> Result<Goal> {
            let goal = Goal {
                id: new_goal.id.unwrap_or_else(|| "new-goal".to_string()),
                goal_type: new_goal.goal_type,
                title: new_goal.title,
                description: new_goal.description,
                target_amount: new_goal.target_amount,
                status_lifecycle: new_goal
                    .status_lifecycle
                    .unwrap_or_else(|| GOAL_LIFECYCLE_ACTIVE.to_string()),
                status_health: new_goal
                    .status_health
                    .unwrap_or_else(|| "not_applicable".to_string()),
                priority: new_goal.priority.unwrap_or(0),
                cover_image_key: new_goal.cover_image_key,
                currency: new_goal.currency,
                start_date: new_goal.start_date,
                target_date: new_goal.target_date,
                summary_current_value: None,
                summary_progress: None,
                projected_completion_date: None,
                projected_value_at_target_date: None,
                created_at: new_goal.created_at.unwrap_or_default(),
                updated_at: new_goal.updated_at.unwrap_or_default(),
                summary_target_amount: None,
            };
            self.goals
                .lock()
                .unwrap()
                .insert(goal.id.clone(), goal.clone());
            Ok(goal)
        }

        async fn insert_goal_with_funding(
            &self,
            new_goal: NewGoal,
            funding_rules: Vec<GoalFundingRuleInput>,
        ) -> Result<Goal> {
            let goal = self.insert_new_goal(new_goal).await?;
            self.save_goal_funding(&goal.id, funding_rules).await?;
            Ok(goal)
        }

        async fn update_goal(&self, goal_update: Goal) -> Result<Goal> {
            self.goals
                .lock()
                .unwrap()
                .insert(goal_update.id.clone(), goal_update.clone());
            Ok(goal_update)
        }

        async fn delete_goal(&self, goal_id_to_delete: String) -> Result<usize> {
            Ok(self
                .goals
                .lock()
                .unwrap()
                .remove(&goal_id_to_delete)
                .map(|_| 1)
                .unwrap_or(0))
        }

        fn load_funding_rules(&self, goal_id: &str) -> Result<Vec<GoalFundingRule>> {
            Ok(self
                .funding_rules
                .lock()
                .unwrap()
                .get(goal_id)
                .cloned()
                .unwrap_or_default())
        }

        fn load_participating_funding_rules(&self) -> Result<Vec<GoalFundingRule>> {
            let goals = self.goals.lock().unwrap();
            let funding_rules = self.funding_rules.lock().unwrap();
            Ok(funding_rules
                .iter()
                .filter(|(goal_id, _)| {
                    goals
                        .get(*goal_id)
                        .map(|g| g.status_lifecycle == GOAL_LIFECYCLE_ACTIVE)
                        .unwrap_or(false)
                })
                .flat_map(|(_, rules)| rules.clone())
                .collect())
        }

        async fn save_goal_funding(
            &self,
            goal_id: &str,
            rules: Vec<GoalFundingRuleInput>,
        ) -> Result<Vec<GoalFundingRule>> {
            let saved: Vec<GoalFundingRule> = rules
                .into_iter()
                .enumerate()
                .map(|(idx, rule)| GoalFundingRule {
                    id: format!("{goal_id}-{idx}"),
                    goal_id: goal_id.to_string(),
                    account_id: rule.account_id,
                    share_percent: rule.share_percent,
                    tax_bucket: rule.tax_bucket,
                    created_at: String::new(),
                    updated_at: String::new(),
                })
                .collect();
            self.set_funding_rules(goal_id, saved.clone());
            Ok(saved)
        }

        fn load_goal_plan(&self, goal_id: &str) -> Result<Option<GoalPlan>> {
            Ok(self.plans.lock().unwrap().get(goal_id).cloned())
        }

        async fn save_goal_plan(&self, plan: SaveGoalPlan) -> Result<GoalPlan> {
            let saved = GoalPlan {
                goal_id: plan.goal_id,
                plan_kind: plan.plan_kind,
                planner_mode: plan.planner_mode,
                settings_json: plan.settings_json,
                summary_json: plan.summary_json.unwrap_or_else(|| "{}".to_string()),
                version: 1,
                created_at: String::new(),
                updated_at: String::new(),
            };
            self.plans
                .lock()
                .unwrap()
                .insert(saved.goal_id.clone(), saved.clone());
            Ok(saved)
        }

        async fn delete_goal_plan(&self, goal_id: &str) -> Result<usize> {
            Ok(self
                .plans
                .lock()
                .unwrap()
                .remove(goal_id)
                .map(|_| 1)
                .unwrap_or(0))
        }

        async fn update_goal_summary_fields(
            &self,
            goal_id: &str,
            update: GoalSummaryUpdate,
        ) -> Result<()> {
            let mut goals = self.goals.lock().unwrap();
            let goal = goals.get_mut(goal_id).ok_or_else(|| not_found(goal_id))?;
            goal.summary_target_amount = update.summary_target_amount;
            goal.summary_current_value = update.summary_current_value;
            goal.summary_progress = update.summary_progress;
            goal.projected_completion_date = update.projected_completion_date;
            goal.projected_value_at_target_date = update.projected_value_at_target_date;
            goal.status_health = update.status_health;
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockAccountService {
        accounts: Mutex<Vec<Account>>,
    }

    #[async_trait]
    impl AccountServiceTrait for MockAccountService {
        async fn create_account(&self, _new_account: NewAccount) -> Result<Account> {
            unimplemented!("not needed in goals service tests")
        }

        async fn update_account(&self, _account_update: AccountUpdate) -> Result<Account> {
            unimplemented!("not needed in goals service tests")
        }

        async fn delete_account(&self, _account_id: &str) -> Result<()> {
            unimplemented!("not needed in goals service tests")
        }

        fn get_account(&self, account_id: &str) -> Result<Account> {
            self.accounts
                .lock()
                .unwrap()
                .iter()
                .find(|account| account.id == account_id)
                .cloned()
                .ok_or_else(|| not_found(account_id))
        }

        fn list_accounts(
            &self,
            _is_active_filter: Option<bool>,
            _is_archived_filter: Option<bool>,
            _account_ids: Option<&[String]>,
        ) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_all_accounts(&self) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_active_accounts(&self) -> Result<Vec<Account>> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| account.is_active)
                .cloned()
                .collect())
        }

        fn get_accounts_by_ids(&self, account_ids: &[String]) -> Result<Vec<Account>> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| account_ids.contains(&account.id))
                .cloned()
                .collect())
        }

        fn get_non_archived_accounts(&self) -> Result<Vec<Account>> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| !account.is_archived)
                .cloned()
                .collect())
        }

        fn get_active_non_archived_accounts(&self) -> Result<Vec<Account>> {
            Ok(self
                .accounts
                .lock()
                .unwrap()
                .iter()
                .filter(|account| account.is_active && !account.is_archived)
                .cloned()
                .collect())
        }

        fn get_base_currency(&self) -> Option<String> {
            Some("USD".to_string())
        }
    }

    #[test]
    fn share_value_basic() {
        let rules = vec![share_rule("g1", "acct-1", 70.0)];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 100_000.0);
        let v = compute_goal_value_from_shares(&rules, &vals);
        assert!((v - 70_000.0).abs() < 0.01, "70% of 100k = {}", v);
    }

    #[test]
    fn share_value_multiple_accounts() {
        let rules = vec![
            share_rule("g1", "acct-1", 70.0),
            share_rule("g1", "acct-2", 30.0),
        ];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 100_000.0);
        vals.insert("acct-2".into(), 200_000.0);
        let v = compute_goal_value_from_shares(&rules, &vals);
        // 100k*0.70 + 200k*0.30 = 70k + 60k = 130k
        assert!((v - 130_000.0).abs() < 0.01, "multi-account: {}", v);
    }

    #[test]
    fn share_value_empty_rules_is_zero() {
        let rules: Vec<GoalFundingRule> = vec![];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 100_000.0);
        let v = compute_goal_value_from_shares(&rules, &vals);
        assert!((v).abs() < 0.01, "empty rules: {}", v);
    }

    #[test]
    fn share_value_missing_account_ignored() {
        let rules = vec![share_rule("g1", "missing", 100.0)];
        let vals = HashMap::new();
        let v = compute_goal_value_from_shares(&rules, &vals);
        assert!((v).abs() < 0.01, "missing account: {}", v);
    }

    #[test]
    fn share_value_includes_negative_account_values() {
        let rules = vec![
            share_rule("g1", "acct-1", 100.0),
            share_rule("g1", "acct-2", 100.0),
        ];
        let vals = HashMap::from([
            ("acct-1".to_string(), -20_000.0),
            ("acct-2".to_string(), 70_000.0),
        ]);

        let v = compute_goal_value_from_shares(&rules, &vals);

        assert!((v - 50_000.0).abs() < 0.01, "value: {}", v);
    }

    #[test]
    fn tax_bucket_balances_from_shares() {
        let rules = vec![
            GoalFundingRule {
                tax_bucket: Some("tax_deferred".into()),
                ..share_rule("g1", "acct-1", 100.0)
            },
            GoalFundingRule {
                tax_bucket: Some("tax_free".into()),
                ..share_rule("g1", "acct-2", 50.0)
            },
            GoalFundingRule {
                tax_bucket: Some("taxable".into()),
                ..share_rule("g1", "acct-3", 100.0)
            },
        ];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 100_000.0);
        vals.insert("acct-2".into(), 80_000.0);
        vals.insert("acct-3".into(), 60_000.0);

        let b = compute_tax_bucket_balances(&rules, &vals);
        assert!((b.tax_deferred - 100_000.0).abs() < 0.01);
        assert!((b.tax_free - 40_000.0).abs() < 0.01);
        assert!((b.taxable - 60_000.0).abs() < 0.01);
    }

    #[test]
    fn tax_bucket_defaults_to_taxable() {
        let rules = vec![
            share_rule("g1", "acct-1", 50.0),
            GoalFundingRule {
                id: "unknown-bucket".into(),
                tax_bucket: Some("tax_deffered".into()),
                ..share_rule("g1", "acct-1", 50.0)
            },
        ];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 50_000.0);

        let b = compute_tax_bucket_balances(&rules, &vals);
        assert!((b.taxable - 50_000.0).abs() < 0.01);
        assert!((b.tax_deferred).abs() < 0.01);
        assert!((b.tax_free).abs() < 0.01);
    }

    #[test]
    fn tax_bucket_balances_use_non_negative_bucket_weights() {
        let rules = vec![
            GoalFundingRule {
                tax_bucket: Some("taxable".into()),
                ..share_rule("g1", "acct-1", 100.0)
            },
            GoalFundingRule {
                tax_bucket: Some("tax_deferred".into()),
                ..share_rule("g1", "acct-2", 100.0)
            },
        ];
        let vals = HashMap::from([
            ("acct-1".to_string(), -20_000.0),
            ("acct-2".to_string(), 70_000.0),
        ]);

        let b = compute_tax_bucket_balances(&rules, &vals);

        assert!((b.taxable).abs() < 0.01);
        assert!((b.tax_deferred - 70_000.0).abs() < 0.01);
        assert!((b.total() - 70_000.0).abs() < 0.01);
    }

    #[test]
    fn bucket_mixes_weigh_the_goals_share_of_each_accounts_holdings() {
        let rules = vec![
            GoalFundingRule {
                tax_bucket: Some("taxable".into()),
                ..share_rule("g1", "acct-1", 50.0)
            },
            GoalFundingRule {
                tax_bucket: Some("tax_deferred".into()),
                ..share_rule("g1", "acct-2", 100.0)
            },
            GoalFundingRule {
                tax_bucket: Some("hsa".into()),
                ..share_rule("g1", "acct-3", 100.0)
            },
        ];
        let values = |stocks, fixed_income, cash, unclassified| AccountAssetValues {
            stocks,
            fixed_income,
            cash,
            unclassified,
        };
        let assets = HashMap::from([
            ("acct-1".to_string(), values(60.0, 0.0, 20.0, 20.0)),
            ("acct-2".to_string(), values(0.0, 100.0, 0.0, 0.0)),
        ]);

        let (mixes, unclassified) = bucket_mixes(&rules, &assets);
        let mixes = mixes.expect("holdings");
        // Unclassified holdings count as stocks.
        assert!((mixes.taxable.stocks - 0.8).abs() < 1e-12);
        assert!((mixes.taxable.cash - 0.2).abs() < 1e-12);
        // Fixed income splits evenly between Baa bonds and T-notes.
        assert!((mixes.tax_deferred.bonds - 0.5).abs() < 1e-12);
        assert!((mixes.tax_deferred.notes - 0.5).abs() < 1e-12);
        // Buckets without holdings take the goal's overall mix: 40 stocks,
        // 100 fixed income, 10 cash.
        assert!((mixes.hsa.stocks - 40.0 / 150.0).abs() < 1e-12);
        assert_eq!(mixes.roth, mixes.hsa);
        assert!((unclassified - 10.0).abs() < 1e-12);

        assert_eq!(bucket_mixes(&rules, &HashMap::new()), (None, 0.0));
    }

    #[test]
    fn retirement_plan_validation_rejects_inverted_spending_window() {
        let mut plan = valid_retirement_plan();
        plan.expenses.items[0].start_age = Some(75);
        plan.expenses.items[0].end_age = Some(65);

        let err = validate_retirement_plan(&plan).expect_err("window should be rejected");
        assert!(err.to_string().contains("To age must be after From age"));
    }

    #[test]
    fn retirement_plan_validation_rejects_bad_numeric_assumptions() {
        let mut plan = valid_retirement_plan();
        plan.investment.monthly_contribution = -1.0;
        assert!(validate_retirement_plan(&plan)
            .expect_err("negative contribution should be rejected")
            .to_string()
            .contains("Monthly contribution"));

        let mut plan = valid_retirement_plan();
        plan.investment.inflation_rate = f64::NAN;
        assert!(validate_retirement_plan(&plan)
            .expect_err("non-finite inflation should be rejected")
            .to_string()
            .contains("Inflation"));

        let mut plan = valid_retirement_plan();
        plan.expenses.items[0].inflation_rate = Some(-0.50);
        assert!(validate_retirement_plan(&plan)
            .expect_err("invalid expense inflation should be rejected")
            .to_string()
            .contains("Spending inflation"));

        let mut plan = valid_retirement_plan();
        plan.income_streams.push(RetirementIncomeStream {
            id: "cpp".into(),
            label: "CPP".into(),
            stream_type: StreamKind::DefinedBenefit,
            start_age: 65,
            adjust_for_inflation: true,
            annual_growth_rate: None,
            monthly_amount: Some(-10.0),
            linked_account_id: None,
            current_value: None,
            monthly_contribution: None,
            accumulation_return: None,
            payout_rate: None,
            payout_mode: None,
            post_payout_return: None,
        });
        assert!(validate_retirement_plan(&plan)
            .expect_err("negative income should be rejected")
            .to_string()
            .contains("Income amount"));

        let mut plan = valid_retirement_plan();
        plan.tax = Some(TaxProfile {
            taxable_withdrawal_rate: 0.15,
            tax_deferred_withdrawal_rate: 0.30,
            tax_free_withdrawal_rate: 0.0,
            early_withdrawal_penalty_rate: Some(1.5),
            early_withdrawal_penalty_age: Some(65),
            country_code: Some("CA".into()),
            withdrawal_buckets: TaxBucketBalances::default(),
        });
        assert!(validate_retirement_plan(&plan)
            .expect_err("invalid tax penalty should be rejected")
            .to_string()
            .contains("Early withdrawal penalty"));
    }

    #[test]
    fn retirement_plan_validation_covers_drawdown_funds() {
        let drawdown_fund =
            |current_value: Option<f64>, post_payout_return: Option<f64>| RetirementIncomeStream {
                id: "dc".into(),
                label: "RRSP".into(),
                stream_type: StreamKind::DefinedContribution,
                start_age: 65,
                adjust_for_inflation: false,
                annual_growth_rate: None,
                monthly_amount: None,
                linked_account_id: None,
                current_value,
                monthly_contribution: None,
                accumulation_return: None,
                payout_rate: Some(0.08),
                payout_mode: Some(PayoutMode::Drawdown),
                post_payout_return,
            };

        let mut plan = valid_retirement_plan();
        plan.income_streams.push(drawdown_fund(None, None));
        assert!(validate_retirement_plan(&plan)
            .expect_err("a drawdown fund with nothing in it should be rejected")
            .to_string()
            .contains("drawdown fund"));

        let mut plan = valid_retirement_plan();
        plan.income_streams
            .push(drawdown_fund(Some(100_000.0), Some(0.99)));
        assert!(validate_retirement_plan(&plan)
            .expect_err("an out-of-range payout-phase return should be rejected")
            .to_string()
            .contains("return during payout"));

        let mut plan = valid_retirement_plan();
        plan.income_streams
            .push(drawdown_fund(Some(100_000.0), Some(0.04)));
        validate_retirement_plan(&plan).expect("a funded drawdown should validate");
    }

    #[test]
    fn retirement_plan_validation_accepts_frontend_assumption_caps() {
        let mut plan = valid_retirement_plan();
        plan.investment.pre_retirement_annual_return = RETIREMENT_MAX_ANNUAL_RETURN;
        plan.investment.retirement_annual_return = RETIREMENT_MAX_ANNUAL_RETURN;
        plan.investment.annual_investment_fee_rate = RETIREMENT_MAX_ANNUAL_INVESTMENT_FEE;
        plan.investment.annual_volatility = RETIREMENT_MAX_ANNUAL_VOLATILITY;
        plan.income_streams.push(RetirementIncomeStream {
            id: "dc".into(),
            label: "Defined contribution".into(),
            stream_type: StreamKind::DefinedContribution,
            start_age: 45,
            adjust_for_inflation: false,
            annual_growth_rate: None,
            monthly_amount: None,
            linked_account_id: None,
            current_value: Some(10_000.0),
            monthly_contribution: Some(100.0),
            accumulation_return: Some(RETIREMENT_MAX_ANNUAL_RETURN),
            payout_rate: None,
            payout_mode: None,
            post_payout_return: None,
        });

        validate_retirement_plan(&plan).expect("frontend caps should validate");
    }

    #[tokio::test]
    async fn save_goal_plan_preserves_unknown_frontend_fields() {
        let goal = retirement_goal("goal-1", GOAL_LIFECYCLE_ACTIVE);
        let repo = Arc::new(MockGoalRepository::new(vec![goal]));
        let service = GoalService::new(repo.clone(), Arc::new(MockAccountService::default()));
        let mut plan = valid_retirement_plan();
        plan.personal.birth_year_month = None;
        let mut settings = serde_json::to_value(&plan).unwrap();
        let object = settings.as_object_mut().unwrap();
        object.insert(
            "version".to_string(),
            serde_json::Value::String("v3".into()),
        );
        settings["expenses"]["items"][0]["id"] = serde_json::Value::String("expense-1".into());
        settings["expenses"]["items"][0]["label"] = serde_json::Value::String("Core".into());

        service
            .save_goal_plan(SaveGoalPlan {
                goal_id: "goal-1".into(),
                plan_kind: "retirement".into(),
                planner_mode: Some("fire".into()),
                settings_json: serde_json::to_string(&settings).unwrap(),
                summary_json: None,
            })
            .await
            .unwrap();

        let saved = repo.load_goal_plan("goal-1").unwrap().unwrap();
        let saved_settings: serde_json::Value = serde_json::from_str(&saved.settings_json).unwrap();
        assert_eq!(saved_settings["version"], "v3");
        assert_eq!(saved_settings["expenses"]["items"][0]["id"], "expense-1");
        assert_eq!(saved_settings["expenses"]["items"][0]["label"], "Core");
    }

    #[test]
    fn retirement_goal_health_maps_overview_status() {
        assert_eq!(
            retirement_goal_health_from_overview_status("achieved"),
            "on_track"
        );
        assert_eq!(
            retirement_goal_health_from_overview_status("on_track"),
            "on_track"
        );
        assert_eq!(
            retirement_goal_health_from_overview_status("at_risk"),
            "at_risk"
        );
        assert_eq!(
            retirement_goal_health_from_overview_status("off_track"),
            "off_track"
        );
        assert_eq!(
            retirement_goal_health_from_overview_status("unknown"),
            "not_applicable"
        );
    }

    #[test]
    fn retirement_seed_rules_use_remaining_capacity() {
        let eligible_accounts = vec![
            test_account("acct-1", "SECURITIES"),
            test_account("acct-2", "CASH"),
            test_account("acct-3", "CRYPTOCURRENCY"),
            test_account("acct-4", "OTHER"),
            test_account("acct-card", "CREDIT_CARD"),
        ];
        let participating_rules = vec![
            share_rule("goal-a", "acct-1", 30.0),
            share_rule("goal-b", "acct-2", 100.0),
            share_rule("goal-c", "acct-3", 80.0),
        ];

        let seed_rules = build_retirement_seed_rules(&eligible_accounts, &participating_rules);
        let shares_by_account: HashMap<String, f64> = seed_rules
            .into_iter()
            .map(|rule| (rule.account_id, rule.share_percent))
            .collect();

        assert_eq!(shares_by_account.get("acct-1"), Some(&70.0));
        assert_eq!(shares_by_account.get("acct-3"), Some(&20.0));
        assert!(!shares_by_account.contains_key("acct-2"));
        assert!(!shares_by_account.contains_key("acct-4"));
        assert!(!shares_by_account.contains_key("acct-card"));
    }

    #[test]
    fn a_linked_home_goal_brings_its_purchase_and_savings() {
        let mut home = test_goal(GOAL_LIFECYCLE_ACTIVE, None);
        home.id = "home".into();
        home.goal_type = "home".into();
        home.title = "Lake house".into();
        home.target_date = Some("2031-01-01".into());
        let repo = Arc::new(MockGoalRepository::new(vec![home.clone()]));
        repo.set_funding_rules(
            "home",
            vec![
                share_rule("home", "acct-1", 20.0),
                share_rule("home", "acct-2", 100.0),
            ],
        );
        repo.plans.lock().unwrap().insert(
            "home".into(),
            GoalPlan {
                goal_id: "home".into(),
                plan_kind: "save_up".into(),
                planner_mode: None,
                settings_json: r#"{"targetDate":"2030-05-01","homePurchase":{"price":500000}}"#
                    .into(),
                summary_json: String::new(),
                version: 1,
                created_at: String::new(),
                updated_at: String::new(),
            },
        );
        let service = GoalService::new(repo.clone(), Arc::new(MockAccountService::default()));
        let mut retirement_rule = share_rule("retirement", "acct-1", 80.0);
        retirement_rule.tax_bucket = Some("tax_deferred".into());

        let (linked, rules) = service
            .linked_home(Some("home"), &[retirement_rule.clone()])
            .expect("load")
            .expect("linked");
        assert_eq!(linked.goal_title, "Lake house");
        assert_eq!(linked.purchase.price, 500_000.0);
        assert_eq!(linked.target_date.as_deref(), Some("2031-01-01"));
        // Each account keeps the retirement goal's tax bucket.
        assert_eq!(rules[0].tax_bucket.as_deref(), Some("tax_deferred"));
        assert_eq!(rules[1].tax_bucket, None);

        assert!(service.linked_home(None, &[]).expect("none").is_none());
        assert!(service
            .linked_home(Some("gone"), &[])
            .expect("gone")
            .is_none());
        home.status_lifecycle = GOAL_LIFECYCLE_ACHIEVED.into();
        repo.goals.lock().unwrap().insert("home".into(), home);
        assert!(service
            .linked_home(Some("home"), &[])
            .expect("bought")
            .is_none());
    }

    #[test]
    fn fire_and_traditional_plans_take_a_linked_home_as_expenses() {
        let mut home = test_goal(GOAL_LIFECYCLE_ACTIVE, None);
        home.id = "home".into();
        home.goal_type = "home".into();
        home.title = "Lake house".into();
        let retirement = retirement_goal("retirement", GOAL_LIFECYCLE_ACTIVE);
        let repo = Arc::new(MockGoalRepository::new(vec![home, retirement]));
        repo.set_funding_rules(
            "retirement",
            vec![share_rule("retirement", "acct-1", 100.0)],
        );
        repo.set_funding_rules("home", vec![share_rule("home", "acct-2", 100.0)]);
        let plan = |goal_id: &str, kind: &str, mode: Option<&str>, settings: String| GoalPlan {
            goal_id: goal_id.into(),
            plan_kind: kind.into(),
            planner_mode: mode.map(str::to_string),
            settings_json: settings,
            summary_json: String::new(),
            version: 1,
            created_at: String::new(),
            updated_at: String::new(),
        };
        repo.plans.lock().unwrap().insert(
            "home".into(),
            plan(
                "home",
                "save_up",
                None,
                r#"{"homePurchase":{"price":400000,"purchaseAge":50}}"#.into(),
            ),
        );
        let mut settings = valid_retirement_plan();
        // Rent kept in the old tax-plan field: moved into a must-have item
        // that ends at the purchase, in every style.
        settings.tax_optimizer = Some(TaxOptimizerSettings {
            home_goal_id: Some("home".into()),
            housing: Housing::Rent,
            monthly_rent: 2_000.0,
            ..TaxOptimizerSettings::default()
        });
        let settings = serde_json::to_string(&settings).unwrap();
        let service = GoalService::new(repo.clone(), Arc::new(MockAccountService::default()));
        let values: AccountValuationMap =
            HashMap::from([("acct-1".into(), 500_000.0), ("acct-2".into(), 50_000.0)]);
        let as_of = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        for (mode, linked) in [("traditional", true), ("fire", true), ("optimized", false)] {
            repo.plans.lock().unwrap().insert(
                "retirement".into(),
                plan("retirement", "retirement", Some(mode), settings.clone()),
            );
            let prepared = service
                .prepare_retirement_input("retirement", &values, as_of)
                .expect("prepared");
            let home_items: Vec<_> = prepared
                .plan
                .expenses
                .items
                .iter()
                .filter(|item| {
                    item.id
                        .as_deref()
                        .is_some_and(|id| id.starts_with("home-purchase"))
                })
                .collect();
            let rent = prepared
                .plan
                .expenses
                .items
                .iter()
                .find(|item| item.id.as_deref() == Some(RENT_EXPENSE_ID))
                .expect("rent moved");
            assert_eq!(rent.end_age, Some(50), "{mode}");
            if linked {
                // The home's expenses, and its savings in the portfolio.
                assert_eq!(home_items.len(), 3, "{mode}");
                assert!(home_items[0].paid_while_working);
                assert_eq!(prepared.current_portfolio, 550_000.0, "{mode}");
            } else {
                // The tax-optimized style models the home itself.
                assert!(home_items.is_empty());
                assert_eq!(prepared.current_portfolio, 500_000.0);
            }
        }
    }

    #[tokio::test]
    async fn update_goal_rejects_unarchiving_retirement_when_another_exists() {
        let archived_goal = retirement_goal("old-retirement", GOAL_LIFECYCLE_ARCHIVED);
        let active_goal = retirement_goal("new-retirement", GOAL_LIFECYCLE_ACTIVE);
        let repo = Arc::new(MockGoalRepository::new(vec![
            archived_goal.clone(),
            active_goal,
        ]));
        let service = GoalService::new(repo, Arc::new(MockAccountService::default()));

        let mut updated = archived_goal;
        updated.status_lifecycle = GOAL_LIFECYCLE_ACTIVE.to_string();

        let err = service
            .update_goal(updated)
            .await
            .expect_err("unarchiving a second retirement goal should fail");

        assert!(err
            .to_string()
            .contains("Only one active retirement goal is allowed"));
    }

    #[tokio::test]
    async fn update_goal_rejects_reactivation_when_funding_would_overallocate_account() {
        let mut inactive_goal = test_goal(GOAL_LIFECYCLE_ARCHIVED, None);
        inactive_goal.id = "inactive-goal".into();
        let mut active_goal = test_goal(GOAL_LIFECYCLE_ACTIVE, None);
        active_goal.id = "active-goal".into();
        let repo = Arc::new(MockGoalRepository::new(vec![
            inactive_goal.clone(),
            active_goal,
        ]));
        repo.set_funding_rules(
            "inactive-goal",
            vec![share_rule("inactive-goal", "acct-1", 50.0)],
        );
        repo.set_funding_rules(
            "active-goal",
            vec![share_rule("active-goal", "acct-1", 60.0)],
        );
        let service = GoalService::new(repo, Arc::new(MockAccountService::default()));

        let mut updated = inactive_goal;
        updated.status_lifecycle = GOAL_LIFECYCLE_ACTIVE.to_string();

        let err = service
            .update_goal(updated)
            .await
            .expect_err("reactivating overallocated funding should fail");

        assert!(err.to_string().contains("overallocated"));
    }

    #[tokio::test]
    async fn save_goal_funding_rejects_unknown_tax_bucket() {
        let goal = retirement_goal("goal-1", GOAL_LIFECYCLE_ACTIVE);
        let repo = Arc::new(MockGoalRepository::new(vec![goal]));
        let service = GoalService::new(repo, Arc::new(MockAccountService::default()));

        let err = service
            .save_goal_funding(
                "goal-1",
                vec![GoalFundingRuleInput {
                    account_id: "acct-1".into(),
                    share_percent: 50.0,
                    tax_bucket: Some("tax_deffered".into()),
                }],
            )
            .await
            .expect_err("unknown tax bucket should fail validation");

        assert!(err.to_string().contains("Unsupported tax bucket"));
    }

    #[tokio::test]
    async fn save_goal_funding_rejects_credit_card_accounts() {
        let goal = retirement_goal("goal-1", GOAL_LIFECYCLE_ACTIVE);
        let repo = Arc::new(MockGoalRepository::new(vec![goal]));
        let account_service = MockAccountService {
            accounts: Mutex::new(vec![test_account("acct-card", "CREDIT_CARD")]),
        };
        let service = GoalService::new(repo, Arc::new(account_service));

        let err = service
            .save_goal_funding(
                "goal-1",
                vec![GoalFundingRuleInput {
                    account_id: "acct-card".into(),
                    share_percent: 50.0,
                    tax_bucket: Some("taxable".into()),
                }],
            )
            .await
            .expect_err("credit cards should not be goal funding accounts");

        assert!(err.to_string().contains("not eligible for goal funding"));
    }

    #[tokio::test]
    async fn refresh_retirement_summary_without_plan_updates_current_value_only() {
        let mut goal = retirement_goal("goal-1", GOAL_LIFECYCLE_ACTIVE);
        goal.summary_target_amount = Some(500_000.0);
        goal.summary_progress = Some(0.2);
        goal.projected_completion_date = Some("2045-01-01".to_string());
        goal.projected_value_at_target_date = Some(750_000.0);
        goal.status_health = "on_track".to_string();

        let repo = Arc::new(MockGoalRepository::new(vec![goal]));
        repo.set_funding_rules("goal-1", vec![share_rule("goal-1", "acct-1", 50.0)]);
        let service = GoalService::new(repo, Arc::new(MockAccountService::default()));
        let valuations = HashMap::from([("acct-1".to_string(), 80_000.0)]);

        let refreshed = service
            .refresh_goal_summary("goal-1", &valuations)
            .await
            .expect("refresh should not require a saved retirement plan");

        assert_eq!(refreshed.summary_current_value, Some(40_000.0));
        assert_eq!(refreshed.summary_target_amount, None);
        assert_eq!(refreshed.summary_progress, None);
        assert_eq!(refreshed.projected_completion_date, None);
        assert_eq!(refreshed.projected_value_at_target_date, None);
        assert_eq!(refreshed.status_health, "not_applicable");
    }

    #[test]
    fn achieved_summary_value_uses_persisted_summary_value() {
        let goal = test_goal("achieved", Some(42_000.0));
        let rules = vec![share_rule("goal-1", "acct-1", 100.0)];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 80_000.0);

        let current_value = compute_summary_current_value(&goal, &rules, &vals);
        assert!((current_value - 42_000.0).abs() < 0.01);
    }

    #[test]
    fn active_summary_value_uses_live_share_value() {
        let goal = test_goal("active", Some(42_000.0));
        let rules = vec![share_rule("goal-1", "acct-1", 100.0)];
        let mut vals = HashMap::new();
        vals.insert("acct-1".into(), 80_000.0);

        let current_value = compute_summary_current_value(&goal, &rules, &vals);
        assert!((current_value - 80_000.0).abs() < 0.01);
    }
}
