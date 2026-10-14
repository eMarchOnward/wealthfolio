//! Goal service and repository trait definitions.

use crate::errors::Result;
use crate::goals::goals_model::{
    AccountAssetMap, AccountValuationMap, Goal, GoalFundingRule, GoalFundingRuleInput, GoalPlan,
    GoalSummaryUpdate, NewGoal, PreparedRetirementSimulationInput, SaveGoalPlan, TaxPlanInput,
};
use crate::planning::retirement::{
    SuccessPathMode, TaxOptimizedPlan, TaxOptimizerAccounts, TaxPlanChoiceCosts,
    TaxPlanConversionRegret, TaxPlanSafeSpending, TaxPlanStressTests, TaxPlanSuccess,
};
use crate::planning::SaveUpOverview;
use crate::portfolio::fire::RetirementOverview;
use async_trait::async_trait;

/// Trait for goal repository operations
#[async_trait]
pub trait GoalRepositoryTrait: Send + Sync {
    fn load_goals(&self) -> Result<Vec<Goal>>;
    fn load_goal(&self, goal_id: &str) -> Result<Goal>;
    async fn insert_new_goal(&self, new_goal: NewGoal) -> Result<Goal>;
    async fn insert_goal_with_funding(
        &self,
        new_goal: NewGoal,
        funding_rules: Vec<GoalFundingRuleInput>,
    ) -> Result<Goal>;
    async fn update_goal(&self, goal_update: Goal) -> Result<Goal>;
    async fn delete_goal(&self, goal_id_to_delete: String) -> Result<usize>;

    // Funding rules
    fn load_funding_rules(&self, goal_id: &str) -> Result<Vec<GoalFundingRule>>;
    fn load_participating_funding_rules(&self) -> Result<Vec<GoalFundingRule>>;
    async fn save_goal_funding(
        &self,
        goal_id: &str,
        rules: Vec<GoalFundingRuleInput>,
    ) -> Result<Vec<GoalFundingRule>>;

    // Plans
    fn load_goal_plan(&self, goal_id: &str) -> Result<Option<GoalPlan>>;
    async fn save_goal_plan(&self, plan: SaveGoalPlan) -> Result<GoalPlan>;
    async fn delete_goal_plan(&self, goal_id: &str) -> Result<usize>;

    // Goal summary
    async fn update_goal_summary_fields(
        &self,
        goal_id: &str,
        update: GoalSummaryUpdate,
    ) -> Result<()>;
}

/// Trait for goal service operations
#[async_trait]
pub trait GoalServiceTrait: Send + Sync {
    fn get_goals(&self) -> Result<Vec<Goal>>;
    fn get_goal(&self, goal_id: &str) -> Result<Goal>;
    async fn create_goal(&self, new_goal: NewGoal) -> Result<Goal>;
    async fn update_goal(&self, updated_goal_data: Goal) -> Result<Goal>;
    async fn delete_goal(&self, goal_id_to_delete: String) -> Result<usize>;

    // Funding
    fn get_goal_funding(&self, goal_id: &str) -> Result<Vec<GoalFundingRule>>;
    async fn save_goal_funding(
        &self,
        goal_id: &str,
        rules: Vec<GoalFundingRuleInput>,
    ) -> Result<Vec<GoalFundingRule>>;

    // Plans
    fn get_goal_plan(&self, goal_id: &str) -> Result<Option<GoalPlan>>;
    async fn save_goal_plan(&self, plan: SaveGoalPlan) -> Result<GoalPlan>;
    async fn delete_goal_plan(&self, goal_id: &str) -> Result<usize>;

    // Summary
    async fn refresh_goal_summary(
        &self,
        goal_id: &str,
        valuations: &AccountValuationMap,
    ) -> Result<Goal>;

    // Retirement overview
    async fn compute_retirement_overview(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
    ) -> Result<RetirementOverview>;

    async fn prepare_retirement_simulation_input(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
    ) -> Result<PreparedRetirementSimulationInput>;

    /// The goal's starting balances by tax treatment with each bucket's asset
    /// mix from its holdings: what the tax-optimized plan starts from.
    async fn retirement_tax_plan_accounts(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
    ) -> Result<TaxOptimizerAccounts>;

    /// The plan and starting accounts the tax-optimized plan of a retirement
    /// goal solves, so a caller can try changed settings without saving them.
    async fn retirement_tax_plan_input(
        &self,
        _goal_id: &str,
        _valuation_map: &AccountValuationMap,
        _cost_basis_map: &AccountValuationMap,
        _asset_map: &AccountAssetMap,
    ) -> Result<TaxPlanInput> {
        Err(crate::errors::Error::Unexpected(
            "Tax plan inputs are not available".to_string(),
        ))
    }

    /// Solves the tax-optimized plan of a retirement goal. `cost_basis_map`
    /// holds each account's cost basis in base currency, cash at full value;
    /// `asset_map` its holdings by asset class.
    async fn optimize_retirement_tax_plan(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
    ) -> Result<TaxOptimizedPlan>;

    /// Chance that the tax-optimized plan's expenses are funded across
    /// historical or resampled return paths, retiring at `retirement_age` when
    /// given (else the plan's target age).
    async fn retirement_tax_plan_success(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        mode: SuccessPathMode,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanSuccess>;

    /// The most the tax-optimized plan can spend while succeeding on at least
    /// `target_rate` of the return paths, and the spending/success trade-off.
    #[allow(clippy::too_many_arguments)]
    async fn retirement_tax_plan_safe_spending(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        mode: SuccessPathMode,
        retirement_age: Option<u32>,
        target_rate: f64,
    ) -> Result<TaxPlanSafeSpending>;

    /// What each choice of the tax-optimized plan costs its goal.
    async fn retirement_tax_plan_choice_costs(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanChoiceCosts>;

    /// The tax-optimized plan re-solved under shocks it does not control.
    async fn retirement_tax_plan_stress_tests(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanStressTests>;

    /// How this year's recommended Roth conversion fares across market paths.
    async fn retirement_tax_plan_conversion_regret(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
        cost_basis_map: &AccountValuationMap,
        asset_map: &AccountAssetMap,
        mode: SuccessPathMode,
        retirement_age: Option<u32>,
    ) -> Result<TaxPlanConversionRegret>;

    // Save-up overview
    async fn compute_save_up_overview(
        &self,
        goal_id: &str,
        valuation_map: &AccountValuationMap,
    ) -> Result<SaveUpOverview>;
}
