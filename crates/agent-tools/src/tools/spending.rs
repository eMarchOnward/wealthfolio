//! Spending tools: reports, budgets, events, categories, splits and
//! categorization rules, read and changed as the Spending pages do.

use std::collections::HashSet;

use serde::Deserialize;
use wealthfolio_spending::activity_assignments::BulkCategoryAssignment;
use wealthfolio_spending::activity_splits::NewActivitySplit;
use wealthfolio_spending::analytics::{EventSummariesRequest, ReportRequest};
use wealthfolio_spending::budget::NewBudgetTarget;
use wealthfolio_spending::categorization_rules::UpdateCategorizationRule as RuleChanges;
use wealthfolio_spending::events::{NewEvent, NewEventType, UpdateEvent};
use wealthfolio_spending::insight::SpendingInsightRequest;
use wealthfolio_spending::settings::SpendingSettingsUpdate;

use super::support::{
    agent_tool, failed, invalid, object_schema, ok, parse, platform, require_confirm,
};
use crate::env::AgentPlatformServices;
use crate::tool::AgentToolError;

/// Same cap as the app's bulk categorize.
const MAX_CATEGORY_ASSIGNMENTS: usize = 1_000;

fn date_schema(description: &str) -> serde_json::Value {
    serde_json::json!({ "type": "string", "description": description })
}

fn account_ids_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "array", "items": { "type": "string" },
        "description": "Limit to these accounts (default: every spending account)."
    })
}

fn period_key_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "string",
        "description": "Budget month as YYYY-MM (default: the current month)."
    })
}

fn confirm_schema() -> serde_json::Value {
    serde_json::json!({ "type": "boolean", "description": "Must be true: deleting cannot be undone." })
}

/// Categorizes the still-uncategorized activities of the spending accounts
/// with the current rules, as the app does after a rule or settings change.
async fn categorize_uncategorized(
    platform: &AgentPlatformServices,
    account_ids: Option<Vec<String>>,
) -> Result<usize, AgentToolError> {
    let settings = platform
        .spending_settings_service
        .get()
        .await
        .map_err(failed)?;
    if !settings.enabled {
        return Ok(0);
    }
    let account_ids = account_ids.unwrap_or(settings.account_ids);
    if account_ids.is_empty() {
        return Ok(0);
    }
    platform
        .categorization_rules_service
        .rerun_all(&account_ids, true)
        .await
        .map_err(failed)
}

// ─── Reads ───────────────────────────────────────────────────────────────────

agent_tool! {
    GetSpendingSettings {
        name: "get_spending_settings",
        description: "Get the spending tracker settings: whether it is on, which accounts count as spending accounts, and which categories are excluded from spending.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            let platform = platform(&env)?;
            ok(platform.spending_settings_service.get().await.map_err(failed)?)
        }
    }
}

agent_tool! {
    GetSpendingReport {
        name: "get_spending_report",
        description: "Get the monthly spending report for a date range: income, spending and net by month, by category and by category group, in base currency.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "startDate": date_schema("First day, YYYY-MM-DD."),
                "endDate": date_schema("Last day, YYYY-MM-DD."),
                "accountIds": account_ids_schema()
            }),
            &["startDate", "endDate"],
        ),
        call: |env, args| {
            let request: ReportRequest = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .spending_analytics_service
                .monthly_report(request, &platform.timezone(), &env.base_currency())
                .await
                .map_err(failed)?)
        }
    }
}

agent_tool! {
    GetSpendingInsight {
        name: "get_spending_insight",
        description: "Get the reconciled spending insight for a period compared with the prior period or the same period last year: totals, category changes and budget status.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "startDate": date_schema("First day, YYYY-MM-DD."),
                "endDate": date_schema("Last day, YYYY-MM-DD."),
                "compare": { "type": "string", "enum": ["prior", "year_over_year"] },
                "compareStartDate": date_schema("Custom comparison start, YYYY-MM-DD."),
                "compareEndDate": date_schema("Custom comparison end, YYYY-MM-DD."),
                "accountIds": account_ids_schema()
            }),
            &["startDate", "endDate"],
        ),
        call: |env, args| {
            let request: SpendingInsightRequest = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .spending_insight_service
                .compute(request, &env.base_currency(), &platform.timezone())
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PeriodArgs {
    #[serde(default)]
    period_key: Option<String>,
}

agent_tool! {
    GetBudget {
        name: "get_budget",
        description: "Get the budget for a month: groups, category targets, rollovers, and spent vs budgeted for each.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(serde_json::json!({ "periodKey": period_key_schema() }), &[]),
        call: |env, args| {
            let args: PeriodArgs = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .budget_service
                .get(args.period_key, &env.base_currency(), &platform.timezone())
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventListArgs {
    #[serde(default)]
    include_spending: bool,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
}

agent_tool! {
    ListSpendingEvents {
        name: "list_spending_events",
        description: "List spending events (trips, weddings, moves...) and event types; with includeSpending, also each event's total spending by category.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "includeSpending": { "type": "boolean" },
                "startDate": date_schema("Only spending on or after, YYYY-MM-DD."),
                "endDate": date_schema("Only spending on or before, YYYY-MM-DD.")
            }),
            &[],
        ),
        call: |env, args| {
            let args: EventListArgs = parse(args)?;
            let platform = platform(&env)?;
            let types = platform.events_service.list_types().await.map_err(failed)?;
            let events = platform
                .events_service
                .list_events_with_names()
                .await
                .map_err(failed)?;
            let spending = if args.include_spending {
                Some(
                    platform
                        .spending_analytics_service
                        .event_spending_summaries(
                            EventSummariesRequest {
                                start_date: args.start_date,
                                end_date: args.end_date,
                                currency: Some(env.base_currency()),
                            },
                            &platform.timezone(),
                        )
                        .await
                        .map_err(failed)?,
                )
            } else {
                None
            };
            ok(serde_json::json!({ "eventTypes": types, "events": events, "spending": spending }))
        }
    }
}

agent_tool! {
    ListCategorizationRules {
        name: "list_categorization_rules",
        description: "List the transaction categorization rules (pattern, match type, category, amount condition, priority, account) in the order they apply.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            let platform = platform(&env)?;
            ok(platform.categorization_rules_service.list().await.map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityArgs {
    activity_id: String,
}

agent_tool! {
    GetActivityCategories {
        name: "get_activity_categories",
        description: "Get a transaction's category assignments (one per taxonomy, with how each was set) and its category splits.",
        scopes: [SpendingRead],
        access: Read,
        schema: object_schema(serde_json::json!({ "activityId": { "type": "string" } }), &["activityId"]),
        call: |env, args| {
            let args: ActivityArgs = parse(args)?;
            let platform = platform(&env)?;
            let cash = &platform.cash_activity_service;
            ok(serde_json::json!({
                "assignments": cash.list_assignments(&args.activity_id).await.map_err(failed)?,
                "splits": cash.list_splits(&args.activity_id).await.map_err(failed)?,
            }))
        }
    }
}

// ─── Writes ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssignArgs {
    assignments: Vec<BulkCategoryAssignment>,
}

agent_tool! {
    AssignActivityCategories {
        name: "assign_activity_categories",
        description: "Set the category of one or more transactions (up to 1000), replacing each one's current category in that taxonomy. Use list_categorization_context or list_asset_taxonomies for category ids.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assignments": {
                    "type": "array",
                    "items": object_schema(
                        serde_json::json!({
                            "activityId": { "type": "string" },
                            "taxonomyId": { "type": "string" },
                            "categoryId": { "type": "string" }
                        }),
                        &["activityId", "taxonomyId", "categoryId"],
                    )
                }
            }),
            &["assignments"],
        ),
        call: |env, args| {
            let args: AssignArgs = parse(args)?;
            if args.assignments.is_empty() || args.assignments.len() > MAX_CATEGORY_ASSIGNMENTS {
                return Err(invalid(format!(
                    "Send between 1 and {MAX_CATEGORY_ASSIGNMENTS} assignments"
                )));
            }
            let platform = platform(&env)?;
            ok(platform
                .cash_activity_service
                .bulk_assign_categories(&args.assignments)
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearCategoryArgs {
    activity_id: String,
    taxonomy_id: String,
}

agent_tool! {
    ClearActivityCategory {
        name: "clear_activity_category",
        description: "Remove a transaction's category in one taxonomy, leaving it uncategorized there.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({ "activityId": { "type": "string" }, "taxonomyId": { "type": "string" } }),
            &["activityId", "taxonomyId"],
        ),
        call: |env, args| {
            let args: ClearCategoryArgs = parse(args)?;
            let platform = platform(&env)?;
            platform
                .cash_activity_service
                .unassign_category(&args.activity_id, &args.taxonomy_id)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "cleared": true }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitArgs {
    activity_id: String,
    splits: Vec<NewActivitySplit>,
}

agent_tool! {
    SetActivitySplits {
        name: "set_activity_splits",
        description: "Split a transaction across categories, replacing any existing split (amounts must add up to the transaction's amount). An empty list removes the split.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "activityId": { "type": "string" },
                "splits": {
                    "type": "array",
                    "items": object_schema(
                        serde_json::json!({
                            "taxonomyId": { "type": "string" },
                            "categoryId": { "type": "string" },
                            "amount": { "type": ["string", "number"] },
                            "note": { "type": ["string", "null"] },
                            "sortOrder": { "type": ["integer", "null"] }
                        }),
                        &["taxonomyId", "categoryId", "amount"],
                    )
                }
            }),
            &["activityId", "splits"],
        ),
        call: |env, args| {
            let args: SplitArgs = parse(args)?;
            let platform = platform(&env)?;
            let cash = &platform.cash_activity_service;
            if args.splits.is_empty() {
                cash.clear_splits(&args.activity_id).await.map_err(failed)?;
                return ok(serde_json::json!({ "splits": [] }));
            }
            ok(serde_json::json!({
                "splits": cash.replace_splits(&args.activity_id, args.splits).await.map_err(failed)?
            }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityEventArgs {
    activity_id: String,
    #[serde(default)]
    event_id: Option<String>,
}

agent_tool! {
    SetActivityEvent {
        name: "set_activity_event",
        description: "Tag a transaction with a spending event, or remove its event with eventId null.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "activityId": { "type": "string" },
                "eventId": { "type": ["string", "null"] }
            }),
            &["activityId"],
        ),
        call: |env, args| {
            let args: ActivityEventArgs = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .cash_activity_service
                .set_event(&args.activity_id, args.event_id)
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveEventArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(flatten)]
    changes: UpdateEvent,
}

agent_tool! {
    SaveSpendingEvent {
        name: "save_spending_event",
        description: "Create a spending event (omit id; name, eventTypeId, startDate and endDate required) or change one (give its id and only the fields to change).",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "id": { "type": "string", "description": "Event to change; omit to create." },
                "name": { "type": "string" },
                "description": { "type": ["string", "null"] },
                "eventTypeId": { "type": "string" },
                "startDate": date_schema("YYYY-MM-DD"),
                "endDate": date_schema("YYYY-MM-DD")
            }),
            &[],
        ),
        call: |env, args| {
            let args: SaveEventArgs = parse(args)?;
            let platform = platform(&env)?;
            let events = &platform.events_service;
            if let Some(id) = args.id {
                return ok(events.update_event(&id, args.changes).await.map_err(failed)?);
            }
            let changes = args.changes;
            let (Some(name), Some(event_type_id), Some(start_date), Some(end_date)) = (
                changes.name,
                changes.event_type_id,
                changes.start_date,
                changes.end_date,
            ) else {
                return Err(invalid(
                    "A new event needs name, eventTypeId, startDate and endDate",
                ));
            };
            ok(events
                .create_event(NewEvent {
                    id: None,
                    name,
                    description: changes.description.flatten(),
                    event_type_id,
                    start_date,
                    end_date,
                })
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteArgs {
    id: String,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    period_key: Option<String>,
}

agent_tool! {
    DeleteSpendingEvent {
        name: "delete_spending_event",
        description: "Delete a spending event; its transactions keep their categories but lose the event tag.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({ "id": { "type": "string" }, "confirm": confirm_schema() }),
            &["id", "confirm"],
        ),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the event")?;
            let platform = platform(&env)?;
            platform.events_service.delete_event(&args.id).await.map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.id }))
        }
    }
}

agent_tool! {
    CreateSpendingEventType {
        name: "create_spending_event_type",
        description: "Create a spending event type (e.g. Travel), optionally with a hex color.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({ "name": { "type": "string" }, "color": { "type": ["string", "null"] } }),
            &["name"],
        ),
        call: |env, args| {
            let mut new_type: NewEventType = parse(args)?;
            new_type.id = None;
            let platform = platform(&env)?;
            ok(platform.events_service.create_type(new_type).await.map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BudgetTargetArgs {
    target: NewBudgetTarget,
}

agent_tool! {
    SaveBudgetTarget {
        name: "save_budget_target",
        description: "Set a monthly budget amount for a category (targetType category, with taxonomyId and categoryId) or a group's buffer (targetType group_buffer, with groupId). Pass the target's id to change an existing one. Returns the month's budget.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "target": object_schema(
                    serde_json::json!({
                        "id": { "type": ["string", "null"] },
                        "periodKey": period_key_schema(),
                        "targetType": { "type": "string", "enum": ["category", "group_buffer"] },
                        "taxonomyId": { "type": ["string", "null"] },
                        "categoryId": { "type": ["string", "null"] },
                        "groupId": { "type": ["string", "null"] },
                        "amount": { "type": "string", "description": "Monthly amount, e.g. \"450.00\"." }
                    }),
                    &["periodKey", "targetType", "amount"],
                )
            }),
            &["target"],
        ),
        call: |env, args| {
            let args: BudgetTargetArgs = parse(args)?;
            let platform = platform(&env)?;
            let period_key = Some(args.target.period_key.clone());
            ok(platform
                .budget_service
                .upsert_target(args.target, period_key, &env.base_currency(), &platform.timezone())
                .await
                .map_err(failed)?)
        }
    }
}

agent_tool! {
    DeleteBudgetTarget {
        name: "delete_budget_target",
        description: "Delete a budget target. Returns the month's budget.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "id": { "type": "string" },
                "periodKey": period_key_schema(),
                "confirm": confirm_schema()
            }),
            &["id", "confirm"],
        ),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the budget target")?;
            let platform = platform(&env)?;
            ok(platform
                .budget_service
                .delete_target(&args.id, args.period_key, &env.base_currency(), &platform.timezone())
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateRuleArgs {
    id: String,
    changes: RuleChanges,
}

agent_tool! {
    UpdateCategorizationRule {
        name: "update_categorization_rule",
        description: "Change a categorization rule (only the fields given; null clears an optional field). Uncategorized transactions are then re-categorized with the rules. Create rules with create_categorization_rule.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "id": { "type": "string" },
                "changes": {
                    "type": "object",
                    "description": "Any of: name, pattern, matchType (contains|starts_with|exact|regex), taxonomyId, categoryId, activityType, amountOp (eq|gt|gte|lt|lte|between), amountValue, amountValue2, priority, isGlobal, accountId."
                }
            }),
            &["id", "changes"],
        ),
        call: |env, args| {
            let args: UpdateRuleArgs = parse(args)?;
            let platform = platform(&env)?;
            let rule = platform
                .categorization_rules_service
                .update(&args.id, args.changes)
                .await
                .map_err(failed)?;
            let categorized = categorize_uncategorized(&platform, None).await?;
            ok(serde_json::json!({ "rule": rule, "categorized": categorized }))
        }
    }
}

agent_tool! {
    DeleteCategorizationRule {
        name: "delete_categorization_rule",
        description: "Delete a categorization rule. Transactions it already categorized keep their categories.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({ "id": { "type": "string" }, "confirm": confirm_schema() }),
            &["id", "confirm"],
        ),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the rule")?;
            let platform = platform(&env)?;
            platform
                .categorization_rules_service
                .delete(&args.id)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.id }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RerunArgs {
    #[serde(default = "default_true")]
    only_uncategorized: bool,
}

fn default_true() -> bool {
    true
}

agent_tool! {
    RerunCategorizationRules {
        name: "rerun_categorization_rules",
        description: "Apply the categorization rules to the spending accounts' transactions: only uncategorized ones (default), or all of them (onlyUncategorized false, which replaces rule-set categories but keeps manual ones). Returns how many were categorized.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(serde_json::json!({ "onlyUncategorized": { "type": "boolean" } }), &[]),
        call: |env, args| {
            let args: RerunArgs = parse(args)?;
            let platform = platform(&env)?;
            let settings = platform.spending_settings_service.get().await.map_err(failed)?;
            let categorized = if settings.enabled {
                platform
                    .categorization_rules_service
                    .rerun_all(&settings.account_ids, args.only_uncategorized)
                    .await
                    .map_err(failed)?
            } else {
                0
            };
            ok(serde_json::json!({ "categorized": categorized }))
        }
    }
}

agent_tool! {
    UpdateSpendingSettings {
        name: "update_spending_settings",
        description: "Change the spending tracker settings: enabled, the spending accounts (accountIds, the full list), and excluded categories (excludedCategoryIds). Newly added accounts are categorized with the rules.",
        scopes: [SpendingRead, SpendingWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "enabled": { "type": "boolean" },
                "accountIds": { "type": "array", "items": { "type": "string" } },
                "excludedCategoryIds": { "type": "array", "items": { "type": "string" } }
            }),
            &[],
        ),
        call: |env, args| {
            let update: SpendingSettingsUpdate = parse(args)?;
            let platform = platform(&env)?;
            let (before, after) = platform
                .spending_settings_service
                .update_with_previous(update)
                .await
                .map_err(failed)?;
            let new_accounts: Vec<String> = if !before.enabled && after.enabled {
                after.account_ids.clone()
            } else {
                let before: HashSet<&String> = before.account_ids.iter().collect();
                after
                    .account_ids
                    .iter()
                    .filter(|id| !before.contains(id))
                    .cloned()
                    .collect()
            };
            let categorized = categorize_uncategorized(&platform, Some(new_accounts)).await?;
            ok(serde_json::json!({ "settings": after, "categorized": categorized }))
        }
    }
}
