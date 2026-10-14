//! Tool implementations.
//!
//! Tools migrate here from `wealthfolio-ai` one at a time; each keeps its
//! existing name, schema, and output shape (guarded by the parity
//! snapshots in `crates/ai/tests/`).

pub mod accounts;
pub mod activities;
pub mod activity_import;
pub mod allocation;
pub mod alternatives;
pub mod asset_classification;
pub mod asset_taxonomies;
pub mod cash_balances;
pub mod categorization_context;
pub mod commit_activity;
pub mod commit_asset_classification;
pub mod commit_categorization_rule;
pub mod contribution_limits;
pub mod create_categorization_rule;
pub mod goals;
pub mod health;
pub mod holdings;
pub mod income;
pub mod market;
pub mod net_worth;
pub mod performance;
pub mod planning;
pub mod portfolios;
pub mod propose_categories;
pub mod quote_import;
pub mod record_activities;
pub mod record_activity;
pub mod records;
pub mod spending;
mod support;
pub mod targets;
pub mod transfer_links;
pub mod valuation;

pub use accounts::{AccountDto, GetAccounts, GetAccountsArgs, GetAccountsOutput};
pub use activities::{ActivityDto, SearchActivities, SearchActivitiesArgs, SearchActivitiesOutput};
// `allocation::HoldingDto` is intentionally not re-exported here: it would
// collide with `holdings::HoldingDto`. Reach it as `allocation::HoldingDto`.
pub use allocation::{
    AllocationDto, GetAssetAllocation, GetAssetAllocationArgs, GetAssetAllocationOutput,
};
pub use asset_taxonomies::{
    AssetTaxonomyAssignmentDto, AssetTaxonomyCategoryDto, AssetTaxonomyDto,
    GetAssetTaxonomyAssignments, GetAssetTaxonomyAssignmentsArgs,
    GetAssetTaxonomyAssignmentsOutput, ListAssetTaxonomies, ListAssetTaxonomiesArgs,
    ListAssetTaxonomiesOutput, ResolvedAssetDto,
};
pub use cash_balances::{
    AccountCashSummary, CashBalanceEntry, GetCashBalances, GetCashBalancesArgs,
    GetCashBalancesOutput,
};
pub use categorization_context::{
    CategoryExample, CategoryOption, ContextSummary, ListCategorizationContext,
    ListCategorizationContextArgs, ListCategorizationContextOutput, Proposal, TaxonomySummary,
    UnproposedActivity,
};
pub use contribution_limits::{
    ContributionLimitDto, GetContributionLimits, GetContributionLimitsArgs,
    GetContributionLimitsOutput,
};
pub use goals::{GetGoals, GetGoalsArgs, GetGoalsOutput, GoalDto};
pub use health::{GetHealthStatus, GetHealthStatusArgs, GetHealthStatusOutput, HealthIssueDto};
pub use holdings::{GetHoldings, GetHoldingsArgs, GetHoldingsOutput, HoldingDto};
pub use income::{GetIncome, GetIncomeArgs, GetIncomeOutput, TopAssetDto};
pub use net_worth::{
    GetNetWorth, GetNetWorthArgs, GetNetWorthOutput, NetWorthHistoryPointDto, NetWorthLineDto,
};
pub use performance::{
    GetPerformance, GetPerformanceArgs, GetPerformanceOutput, PerformanceAttributionOutput,
    PerformanceDataQualityOutput, PerformanceReturnsOutput, PerformanceRiskOutput,
};
pub use portfolios::{GetPortfolios, GetPortfoliosArgs, GetPortfoliosOutput, PortfolioDto};
pub use valuation::{
    GetValuationHistory, GetValuationHistoryArgs, GetValuationHistoryOutput, ValuationPointDto,
};

// Draft/suggest tools migrated from `wealthfolio-ai`.
pub use asset_classification::{
    AssignmentPreviewDto, CandidateAssignmentPreviewDto, ClassificationChangesDto,
    PrepareAssetClassification, PrepareAssetClassificationArgs, PrepareAssetClassificationOutput,
    PreparedAssignmentInput, PreparedTaxonomyDto,
};
pub use create_categorization_rule::{
    CreateCategorizationRule, CreateCategorizationRuleArgs, CreateCategorizationRuleOutput,
};
pub use propose_categories::{
    AiProposal, ProposalSummary, ProposeCategories, ProposeCategoriesArgs, ProposeCategoriesOutput,
};
pub use record_activities::{
    ActivityDraftRow, BatchValidationSummary, RecordActivities, RecordActivitiesArgs,
    RecordActivitiesOutput,
};
pub use record_activity::{
    AccountOption, ActivityDraft, RecordActivity, RecordActivityArgs, RecordActivityOutput,
    ResolvedAsset, SubtypeOption, ValidationError, ValidationResult,
};

// MCP-only commit tools.
pub use commit_activity::{
    CommitActivityDraft, CommitActivityDraftOutput, CommitActivityDrafts, CommitActivityDraftsArgs,
    CommitActivityDraftsOutput, CommitError, CommittedActivity,
};
pub use commit_asset_classification::{
    CommitAssetClassificationAssignmentInput, CommitAssetClassificationDraft,
    CommitAssetClassificationDraftArgs, CommitAssetClassificationDraftOutput,
    CommittedAssetClassificationAssignment,
};
pub use commit_categorization_rule::{
    CommitCategorizationRule, CommitCategorizationRuleArgs, CommitCategorizationRuleOutput,
};

// MCP-only transfer linking tools.
pub use transfer_links::{
    FindTransferMatches, FindTransferMatchesArgs, FindTransferMatchesOutput,
    LinkTransferActivities, LinkTransfersOutput, TransferCandidateDto, TransferDto,
    TransferPairDto, TransferPairError, UnlinkTransferActivities, UnlinkTransfersOutput,
    UnlinkedTransferDto,
};

// MCP-only quote import tools.
pub use quote_import::{
    CommitQuoteImport, CommitQuoteImportOutput, PrepareQuoteImport, PrepareQuoteImportOutput,
    QuoteImportArgs, QuoteImportInput, QuoteImportSummary, QuoteRowResult,
};

// MCP-only CSV import tools (validate + dedup-safe import pipeline).
pub use activity_import::{
    ActivityImportArgs, ActivityImportRow, CommitActivityImport, CommitActivityImportOutput,
    GetImportMapping, GetImportMappingArgs, ImportPreviewSummary, ImportRowResult,
    PrepareActivityImport, PrepareActivityImportOutput,
};

use std::sync::Arc;

use crate::tool::AgentTool;

/// The v1 read-only tool set, in catalog (and LLM-visible) order — the
/// read tools keep their historical relative order, but all of them are
/// now registered before the assistant's write tools (previously the two
/// kinds were interleaved).
pub fn v1_read_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetHoldings),
        Arc::new(GetAccounts),
        Arc::new(GetCashBalances),
        Arc::new(SearchActivities),
        Arc::new(GetGoals),
        Arc::new(GetValuationHistory),
        Arc::new(GetIncome),
        Arc::new(GetAssetAllocation),
        Arc::new(GetPerformance),
        Arc::new(GetHealthStatus),
        Arc::new(ListCategorizationContext),
        Arc::new(ListAssetTaxonomies),
        Arc::new(GetAssetTaxonomyAssignments),
        Arc::new(GetPortfolios),
        Arc::new(GetNetWorth),
        Arc::new(GetContributionLimits),
    ]
}

/// The draft/suggest tools, in catalog (and LLM-visible) order. These prepare
/// drafts or proposals for user review — they never mutate data themselves.
pub fn draft_suggest_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(RecordActivity),
        Arc::new(RecordActivities),
        Arc::new(ProposeCategories),
        Arc::new(CreateCategorizationRule),
        Arc::new(PrepareAssetClassification),
    ]
}

/// The MCP-only commit tools that persist reviewed drafts. Never exposed to the
/// in-app assistant (which writes through its confirmation widget instead).
pub fn commit_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(CommitActivityDraft),
        Arc::new(CommitActivityDrafts),
        Arc::new(CommitAssetClassificationDraft),
        Arc::new(CommitCategorizationRule),
    ]
}

/// The MCP-only CSV import tools: fetch an account's mapping, preview rows
/// (validate + duplicate detection), and import through the real pipeline.
/// Not exposed to the in-app assistant (which has its own `import_csv` flow).
pub fn import_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(GetImportMapping),
        Arc::new(PrepareActivityImport),
        Arc::new(CommitActivityImport),
    ]
}

/// The MCP-only transfer linking tools: find unlinked transfers with their
/// candidates, then link or unlink pairs. Not exposed to the in-app assistant,
/// which links through the Link Transfer dialog.
pub fn transfer_link_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(FindTransferMatches),
        Arc::new(LinkTransferActivities),
        Arc::new(UnlinkTransferActivities),
    ]
}

/// The MCP-only platform tools: goal planning and the planners, spending,
/// allocation targets, alternative assets, market data and the writes that
/// reach past activities and classification. Not exposed to the in-app
/// assistant, which has no confirmation card for these.
pub fn platform_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![
        Arc::new(planning::GetGoalDetails),
        Arc::new(planning::GetGoalOverview),
        Arc::new(planning::GetTaxPlanAccounts),
        Arc::new(planning::PreviewSaveUpPlan),
        Arc::new(planning::RunRetirementProjection),
        Arc::new(planning::RunRetirementMonteCarlo),
        Arc::new(planning::RunRetirementStressTests),
        Arc::new(planning::RunRetirementScenarios),
        Arc::new(planning::RunRetirementSensitivityMap),
        Arc::new(planning::OptimizeTaxPlan),
        Arc::new(planning::RunTaxPlanSuccess),
        Arc::new(planning::RunTaxPlanSafeSpending),
        Arc::new(planning::RunTaxPlanChoiceCosts),
        Arc::new(planning::RunTaxPlanStressTests),
        Arc::new(planning::RunTaxPlanConversionRegret),
        Arc::new(spending::GetSpendingSettings),
        Arc::new(spending::GetSpendingReport),
        Arc::new(spending::GetSpendingInsight),
        Arc::new(spending::GetBudget),
        Arc::new(spending::ListSpendingEvents),
        Arc::new(spending::ListCategorizationRules),
        Arc::new(spending::GetActivityCategories),
        Arc::new(targets::GetAllocationTargets),
        Arc::new(targets::GetAllocationDrift),
        Arc::new(targets::CalculateRebalancePlan),
        Arc::new(alternatives::GetAlternativeAssets),
        Arc::new(alternatives::GetLoanPayments),
        Arc::new(alternatives::PreviewLoanTerms),
        Arc::new(market::GetAssets),
        Arc::new(market::GetQuoteHistory),
        Arc::new(market::GetLatestQuotes),
        Arc::new(market::GetExchangeRates),
        Arc::new(market::GetTaxLots),
        Arc::new(market::GetHoldingsSnapshots),
        Arc::new(records::GetSettings),
        Arc::new(records::ListImportTemplates),
        // Writes.
        Arc::new(records::CreateAccount),
        Arc::new(records::UpdateAccount),
        Arc::new(records::DeleteAccount),
        Arc::new(records::SavePortfolio),
        Arc::new(records::DeletePortfolio),
        Arc::new(records::SaveContributionLimit),
        Arc::new(records::DeleteContributionLimit),
        Arc::new(records::CreateGoal),
        Arc::new(records::UpdateGoal),
        Arc::new(records::DeleteGoal),
        Arc::new(records::SaveGoalFunding),
        Arc::new(records::UpdateGoalPlan),
        Arc::new(records::UpdateActivity),
        Arc::new(records::DeleteActivity),
        Arc::new(records::CreateTaxonomyCategory),
        Arc::new(records::UpdateTaxonomyCategory),
        Arc::new(records::DeleteTaxonomyCategory),
        Arc::new(records::DismissHealthIssue),
        Arc::new(spending::AssignActivityCategories),
        Arc::new(spending::ClearActivityCategory),
        Arc::new(spending::SetActivitySplits),
        Arc::new(spending::SetActivityEvent),
        Arc::new(spending::SaveSpendingEvent),
        Arc::new(spending::DeleteSpendingEvent),
        Arc::new(spending::CreateSpendingEventType),
        Arc::new(spending::SaveBudgetTarget),
        Arc::new(spending::DeleteBudgetTarget),
        Arc::new(spending::UpdateCategorizationRule),
        Arc::new(spending::DeleteCategorizationRule),
        Arc::new(spending::RerunCategorizationRules),
        Arc::new(spending::UpdateSpendingSettings),
        Arc::new(targets::SaveAllocationTarget),
        Arc::new(targets::DeleteAllocationTarget),
        Arc::new(alternatives::CreateAlternativeAsset),
        Arc::new(alternatives::UpdateAlternativeAssetValuation),
        Arc::new(alternatives::UpdateAlternativeAssetDetails),
        Arc::new(alternatives::DeleteAlternativeAsset),
        Arc::new(alternatives::LinkLiability),
        Arc::new(alternatives::ApplyLoanAction),
        Arc::new(market::SyncMarketData),
        Arc::new(market::DeleteQuote),
        Arc::new(market::SaveExchangeRate),
        Arc::new(market::DeleteExchangeRate),
        Arc::new(market::UpdateAssetProfile),
    ]
}

/// The MCP-only quote import tools: preview reviewed closing prices for
/// existing assets, then save them as manual quotes. Not exposed to the in-app
/// assistant, which has no confirmation card for quotes.
pub fn quote_import_tools() -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(PrepareQuoteImport), Arc::new(CommitQuoteImport)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog order is LLM-visible (tool listing) and mirrors the
    /// read tools' historical relative order — append-only.
    #[test]
    fn v1_read_tools_names_and_order_are_stable() {
        let names: Vec<&str> = v1_read_tools().iter().map(|tool| tool.name()).collect();
        assert_eq!(
            names,
            vec![
                "get_holdings",
                "get_accounts",
                "get_cash_balances",
                "search_activities",
                "get_goals",
                "get_valuation_history",
                "get_income",
                "get_asset_allocation",
                "get_performance",
                "get_health_status",
                "list_categorization_context",
                "list_asset_taxonomies",
                "get_asset_taxonomy_assignments",
                "get_portfolios",
                "get_net_worth",
                "get_contribution_limits",
            ]
        );
    }
}
