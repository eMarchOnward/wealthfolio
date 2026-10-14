//! Allocation target tools: targets with their weights, drift against
//! current holdings, rebalance plans, and saving or deleting targets.

use std::sync::Arc;

use rust_decimal::Decimal;
use serde::Deserialize;
use wealthfolio_core::accounts::AccountPurpose;
use wealthfolio_core::portfolio::allocation_targets::{
    AllocationTarget, CalculateRebalancePlanInput, NewAllocationTarget, NewAllocationTargetWeight,
    ScenarioMode, ScopeType,
};
use wealthfolio_core::portfolios::{AccountScope, ResolvedAccountScope};

use super::support::{
    agent_tool, failed, invalid, object_schema, ok, parse, platform, require_confirm,
};
use crate::env::{AgentEnvironment, AgentPlatformServices};
use crate::tool::AgentToolError;

fn target_id_schema() -> serde_json::Value {
    serde_json::json!({ "type": "string", "description": "Allocation target id (from get_allocation_targets)." })
}

fn target(platform: &AgentPlatformServices, id: &str) -> Result<AllocationTarget, AgentToolError> {
    platform
        .allocation_target_service
        .get_target(id)
        .map_err(failed)?
        .ok_or_else(|| invalid(format!("Allocation target {id} not found")))
}

/// The accounts a target covers, as the drift view resolves them.
fn target_accounts(
    env: &Arc<dyn AgentEnvironment>,
    target: &AllocationTarget,
) -> Result<ResolvedAccountScope, AgentToolError> {
    let scope_id = || {
        target
            .scope_id
            .clone()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid(format!("Allocation target {} has no scope id", target.id)))
    };
    let scope = match target.scope_type {
        ScopeType::All => AccountScope::All,
        ScopeType::Account => AccountScope::Account {
            account_id: scope_id()?,
        },
        ScopeType::Portfolio => AccountScope::Portfolio {
            portfolio_id: scope_id()?,
        },
    };
    env.portfolio_service()
        .resolve_account_scope_for_purpose(&scope, &env.base_currency(), AccountPurpose::Holdings)
        .map_err(failed)
}

agent_tool! {
    GetAllocationTargets {
        name: "get_allocation_targets",
        description: "List allocation targets (target mixes by taxonomy category, e.g. 60% equity / 40% bonds) with their category weights in basis points, drift bands, rebalance settings and sell constraints.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({ "includeArchived": { "type": "boolean" } }),
            &[],
        ),
        call: |env, args| {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Args {
                #[serde(default)]
                include_archived: bool,
            }
            let args: Args = parse(args)?;
            let platform = platform(&env)?;
            let service = &platform.allocation_target_service;
            let mut targets = Vec::new();
            for target in service.list_targets().map_err(failed)? {
                if target.archived_at.is_some() && !args.include_archived {
                    continue;
                }
                let weights = service.list_weights_for_target(&target.id).map_err(failed)?;
                let constraints = service.list_target_constraints(&target.id).map_err(failed)?;
                targets.push(serde_json::json!({
                    "target": target, "weights": weights, "constraints": constraints
                }));
            }
            ok(targets)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriftArgs {
    target_id: String,
    #[serde(default)]
    include_holdings: bool,
}

agent_tool! {
    GetAllocationDrift {
        name: "get_allocation_drift",
        description: "Compare a target's accounts with the target: current vs target percentage and value for each category, drift, and whether each is outside its band; with includeHoldings, the holdings in each category too.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({ "targetId": target_id_schema(), "includeHoldings": { "type": "boolean" } }),
            &["targetId"],
        ),
        call: |env, args| {
            let args: DriftArgs = parse(args)?;
            let platform = platform(&env)?;
            let target = target(&platform, &args.target_id)?;
            let accounts = target_accounts(&env, &target)?;
            let base = env.base_currency();
            let drift = &platform.drift_service;
            let report = if args.include_holdings {
                drift
                    .get_drift_report_with_holdings_for_target(
                        &target.id,
                        &accounts.account_ids,
                        &base,
                        &accounts.scope_id,
                    )
                    .await
            } else {
                drift
                    .get_drift_report_for_target(
                        &target.id,
                        &accounts.account_ids,
                        &base,
                        &accounts.scope_id,
                    )
                    .await
            };
            ok(report.map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RebalanceArgs {
    target_id: String,
    #[serde(default)]
    available_cash: Option<Decimal>,
    #[serde(default)]
    scenario_mode: Option<ScenarioMode>,
    #[serde(default)]
    eligible_asset_ids: Option<Vec<String>>,
}

agent_tool! {
    CalculateRebalancePlan {
        name: "calculate_rebalance_plan",
        description: "Calculate the trades that bring a target's accounts back toward the target: investing new cash only (cash_flow_only, default), selling to rebalance (sell_to_rebalance), or both (hybrid). Nothing is traded or saved.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "targetId": target_id_schema(),
                "availableCash": { "type": ["number", "string"], "description": "New cash to invest, base currency (default 0)." },
                "scenarioMode": { "type": "string", "enum": ["cash_flow_only", "sell_to_rebalance", "hybrid"] },
                "eligibleAssetIds": { "type": "array", "items": { "type": "string" }, "description": "Only trade these assets." }
            }),
            &["targetId"],
        ),
        call: |env, args| {
            let args: RebalanceArgs = parse(args)?;
            let platform = platform(&env)?;
            let target = target(&platform, &args.target_id)?;
            let accounts = target_accounts(&env, &target)?;
            ok(platform
                .rebalance_service
                .calculate_plan(CalculateRebalancePlanInput {
                    target_id: target.id,
                    available_cash: args.available_cash.unwrap_or_default(),
                    account_ids: accounts.account_ids,
                    base_currency: env.base_currency(),
                    aggregated_account_id: accounts.scope_id,
                    scenario_mode: args.scenario_mode.unwrap_or_default(),
                    eligible_asset_ids: args.eligible_asset_ids,
                })
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveTargetArgs {
    #[serde(default)]
    id: Option<String>,
    target: NewAllocationTarget,
    weights: Vec<NewAllocationTargetWeight>,
}

agent_tool! {
    SaveAllocationTarget {
        name: "save_allocation_target",
        description: "Create an allocation target (omit id) or replace one's settings and weights (give its id). Weights are taxonomy category ids with targetBps (10000 = 100%) and should add up to 10000.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "id": { "type": "string" },
                "target": object_schema(
                    serde_json::json!({
                        "name": { "type": "string" },
                        "scopeType": { "type": "string", "enum": ["all", "portfolio", "account"] },
                        "scopeId": { "type": ["string", "null"], "description": "Account or portfolio id for those scopes." },
                        "taxonomyId": { "type": "string", "description": "e.g. asset_classes" },
                        "triggerType": { "type": "string", "enum": ["manual", "threshold"] },
                        "driftBandBps": { "type": "integer", "description": "Allowed drift, e.g. 500 = 5 points." },
                        "bandType": { "type": "string", "enum": ["absolute", "hybrid"] },
                        "relativeFactorBps": { "type": "integer" },
                        "rebalanceGoal": { "type": "string", "enum": ["nearest_band", "exact_target"] },
                        "minTradeAmount": { "type": "string" },
                        "wholeSharesOnly": { "type": "boolean" },
                        "allowSells": { "type": "boolean" },
                        "maxTurnoverBps": { "type": ["integer", "null"] }
                    }),
                    &["name", "scopeType", "taxonomyId", "triggerType", "driftBandBps"],
                ),
                "weights": {
                    "type": "array",
                    "items": object_schema(
                        serde_json::json!({
                            "categoryId": { "type": "string" },
                            "targetBps": { "type": "integer" },
                            "isLocked": { "type": "boolean" },
                            "isRequired": { "type": "boolean" }
                        }),
                        &["categoryId", "targetBps", "isLocked", "isRequired"],
                    )
                }
            }),
            &["target", "weights"],
        ),
        call: |env, args| {
            let args: SaveTargetArgs = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .allocation_target_service
                .save_target_with_weights(args.id, args.target, args.weights)
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteTargetArgs {
    target_id: String,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    archive: bool,
}

agent_tool! {
    DeleteAllocationTarget {
        name: "delete_allocation_target",
        description: "Delete an allocation target, or archive it (archive true) to hide it but keep it.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "targetId": target_id_schema(),
                "archive": { "type": "boolean" },
                "confirm": { "type": "boolean", "description": "Must be true to delete (not needed to archive)." }
            }),
            &["targetId"],
        ),
        call: |env, args| {
            let args: DeleteTargetArgs = parse(args)?;
            if !args.archive {
                require_confirm(args.confirm, "the allocation target")?;
            }
            let platform = platform(&env)?;
            let service = &platform.allocation_target_service;
            if args.archive {
                return ok(service.archive_target(&args.target_id).await.map_err(failed)?);
            }
            service.delete_target(&args.target_id).await.map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.target_id }))
        }
    }
}
