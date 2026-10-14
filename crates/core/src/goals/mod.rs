//! Goals module - domain models, services, and traits.

mod goals_model;
mod goals_service;
mod goals_traits;

pub use goals_model::{
    AccountAssetMap, AccountAssetValues, AccountValuationMap, Goal, GoalFundingRule,
    GoalFundingRuleInput, GoalPlan, GoalSummaryUpdate, NewGoal, PreparedRetirementSimulationInput,
    SaveGoalPlan, TaxPlanInput,
};
pub use goals_service::{account_asset_map, cost_basis_map, validate_retirement_plan, GoalService};
pub use goals_traits::{GoalRepositoryTrait, GoalServiceTrait};
