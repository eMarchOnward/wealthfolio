//! Write tools for the app's records: accounts, account groups, contribution
//! limits, goals with their funding and plans, activities, taxonomy
//! categories and health issues, plus settings and import template reads.
//!
//! Updates take a JSON merge patch of the record (`changes`) and accept
//! `dryRun` to return the result without saving; deletes need
//! `confirm: true`.

use std::sync::Arc;

use serde::Deserialize;
use wealthfolio_core::accounts::{AccountUpdate, NewAccount};
use wealthfolio_core::activities::ActivityUpdate;
use wealthfolio_core::goals::{GoalFundingRuleInput, NewGoal, SaveGoalPlan};
use wealthfolio_core::limits::NewContributionLimit;
use wealthfolio_core::portfolios::{NewPortfolio, PortfolioUpdate};
use wealthfolio_core::taxonomies::NewCategory;

use super::support::{
    account_values, agent_tool, failed, invalid, merge_patch, object_schema, ok, parse, patched,
    platform, require_confirm,
};
use crate::env::AgentEnvironment;

fn confirm_schema() -> serde_json::Value {
    serde_json::json!({ "type": "boolean", "description": "Must be true: deleting cannot be undone." })
}

fn changes_schema(fields: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": format!("JSON merge patch of the fields to change: {fields}.")
    })
}

fn dry_run_schema() -> serde_json::Value {
    serde_json::json!({ "type": "boolean", "description": "Return what would be saved without saving it." })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateArgs {
    id: String,
    changes: serde_json::Value,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteArgs {
    id: String,
    #[serde(default)]
    confirm: bool,
}

fn update_schema(id: &str, fields: &str) -> serde_json::Value {
    object_schema(
        serde_json::json!({
            "id": { "type": "string", "description": id },
            "changes": changes_schema(fields),
            "dryRun": dry_run_schema()
        }),
        &["id", "changes"],
    )
}

fn delete_schema(id: &str) -> serde_json::Value {
    object_schema(
        serde_json::json!({ "id": { "type": "string", "description": id }, "confirm": confirm_schema() }),
        &["id", "confirm"],
    )
}

// ─── Accounts ────────────────────────────────────────────────────────────────

agent_tool! {
    CreateAccount {
        name: "create_account",
        description: "Create an account. accountType: SECURITIES, CASH, CRYPTOCURRENCY, PROPERTY... (as get_accounts shows); trackingMode TRANSACTIONS (built from activities) or HOLDINGS (from holdings snapshots).",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "name": { "type": "string" },
                "accountType": { "type": "string" },
                "group": { "type": ["string", "null"], "description": "e.g. Retirement, Taxable." },
                "currency": { "type": "string" },
                "isDefault": { "type": "boolean" },
                "isActive": { "type": "boolean" },
                "trackingMode": { "type": "string", "enum": ["TRANSACTIONS", "HOLDINGS", "NOT_SET"] },
                "accountNumber": { "type": ["string", "null"] },
                "meta": { "type": ["string", "null"], "description": "JSON text of extra account details." }
            }),
            &["name", "accountType", "currency"],
        ),
        call: |env, args| {
            let mut fields = serde_json::json!({ "isDefault": false, "isActive": true });
            merge_patch(&mut fields, &args);
            let mut account: NewAccount = parse(fields)?;
            // Broker links are made by broker sync, never here.
            account.provider = None;
            account.provider_account_id = None;
            ok(env.account_service().create_account(account).await.map_err(failed)?)
        }
    }
}

agent_tool! {
    UpdateAccount {
        name: "update_account",
        description: "Change an account: name, accountType, group, isDefault, isActive, isArchived, trackingMode, accountNumber or meta. The currency cannot change.",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: update_schema(
            "Account id.",
            "name, accountType, group, isDefault, isActive, isArchived, trackingMode, accountNumber, meta",
        ),
        call: |env, args| {
            let args: UpdateArgs = parse(args)?;
            let account = env.account_service().get_account(&args.id).map_err(failed)?;
            let current = AccountUpdate {
                id: Some(account.id),
                name: account.name,
                account_type: account.account_type,
                group: account.group,
                is_default: account.is_default,
                is_active: account.is_active,
                platform_id: account.platform_id,
                account_number: account.account_number,
                meta: account.meta,
                provider: account.provider,
                provider_account_id: account.provider_account_id,
                is_archived: Some(account.is_archived),
                tracking_mode: Some(account.tracking_mode),
            };
            let mut update = patched(&current, Some(&args.changes))?;
            update.id = current.id.clone();
            update.provider = current.provider.clone();
            update.provider_account_id = current.provider_account_id.clone();
            if args.dry_run {
                return ok(serde_json::json!({ "dryRun": true, "account": update }));
            }
            ok(env.account_service().update_account(update).await.map_err(failed)?)
        }
    }
}

agent_tool! {
    DeleteAccount {
        name: "delete_account",
        description: "Delete an account with all its activities, holdings snapshots and history. Consider update_account with isArchived true instead.",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: delete_schema("Account id."),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the account and all its activities")?;
            env.account_service().delete_account(&args.id).await.map_err(failed)?;
            env.health_service().clear_cache().await;
            ok(serde_json::json!({ "deleted": args.id }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavePortfolioArgs {
    #[serde(default)]
    id: Option<String>,
    #[serde(flatten)]
    portfolio: NewPortfolio,
}

agent_tool! {
    SavePortfolio {
        name: "save_portfolio",
        description: "Create a portfolio (a named group of accounts, e.g. Retirement) or replace one's name, description and accounts (give its id).",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "id": { "type": "string", "description": "Portfolio to change; omit to create." },
                "name": { "type": "string" },
                "description": { "type": ["string", "null"] },
                "sortOrder": { "type": "integer" },
                "accountIds": { "type": "array", "items": { "type": "string" } }
            }),
            &["name", "accountIds"],
        ),
        call: |env, args| {
            let args: SavePortfolioArgs = parse(args)?;
            let service = env.portfolio_service();
            let portfolio = args.portfolio;
            let saved = match args.id {
                Some(id) => {
                    service
                        .update_portfolio(PortfolioUpdate {
                            id,
                            name: portfolio.name,
                            description: portfolio.description,
                            sort_order: portfolio.sort_order,
                            account_ids: portfolio.account_ids,
                        })
                        .await
                }
                None => service.create_portfolio(portfolio).await,
            };
            ok(saved.map_err(failed)?)
        }
    }
}

agent_tool! {
    DeletePortfolio {
        name: "delete_portfolio",
        description: "Delete a portfolio (account group). Its accounts are kept.",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: delete_schema("Portfolio id."),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the portfolio")?;
            env.portfolio_service()
                .delete_portfolio(&args.id)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.id }))
        }
    }
}

agent_tool! {
    SaveContributionLimit {
        name: "save_contribution_limit",
        description: "Create a contribution limit (e.g. 2026 IRA, $7000, for some accounts) or change one (give its id). accountIds is a comma-separated list of account ids.",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "id": { "type": ["string", "null"] },
                "groupName": { "type": "string" },
                "contributionYear": { "type": "integer" },
                "limitAmount": { "type": "number" },
                "accountIds": { "type": ["string", "null"] },
                "startDate": { "type": ["string", "null"] },
                "endDate": { "type": ["string", "null"] }
            }),
            &["groupName", "contributionYear", "limitAmount"],
        ),
        call: |env, args| {
            let limit: NewContributionLimit = parse(args)?;
            let service = env.contribution_limit_service();
            let saved = match limit.id.clone() {
                Some(id) => service.update_contribution_limit(&id, limit).await,
                None => service.create_contribution_limit(limit).await,
            };
            ok(saved.map_err(failed)?)
        }
    }
}

agent_tool! {
    DeleteContributionLimit {
        name: "delete_contribution_limit",
        description: "Delete a contribution limit.",
        scopes: [AccountsRead, AccountsWrite],
        access: Write,
        schema: delete_schema("Contribution limit id."),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the contribution limit")?;
            env.contribution_limit_service()
                .delete_contribution_limit(&args.id)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.id }))
        }
    }
}

// ─── Goals ───────────────────────────────────────────────────────────────────

/// Refreshes a goal's saved summary after its funding or plan changes, as
/// the app does; a failure leaves the old summary and is reported.
async fn refresh_goal_summary(env: &Arc<dyn AgentEnvironment>, goal_id: &str) -> Option<String> {
    let platform = match platform(env) {
        Ok(platform) => platform,
        Err(err) => return Some(err.to_string()),
    };
    let result = match account_values(env, &platform).await {
        Ok(values) => env
            .goal_service()
            .refresh_goal_summary(goal_id, &values)
            .await
            .map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    result.err()
}

agent_tool! {
    CreateGoal {
        name: "create_goal",
        description: "Create a goal. goalType: retirement (one per profile), save_up, or another type the app shows. Set its funding with save_goal_funding and its plan with update_goal_plan.",
        scopes: [FinancialPlanningRead, GoalsWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "goalType": { "type": "string" },
                "title": { "type": "string" },
                "description": { "type": ["string", "null"] },
                "targetAmount": { "type": ["number", "null"] },
                "priority": { "type": ["integer", "null"] },
                "startDate": { "type": ["string", "null"] },
                "targetDate": { "type": ["string", "null"], "description": "YYYY-MM-DD" }
            }),
            &["goalType", "title"],
        ),
        call: |env, args| {
            let mut goal: NewGoal = parse(args)?;
            goal.id = None;
            goal.currency = Some(env.base_currency());
            ok(env.goal_service().create_goal(goal).await.map_err(failed)?)
        }
    }
}

agent_tool! {
    UpdateGoal {
        name: "update_goal",
        description: "Change a goal: title, description, targetAmount, priority, statusLifecycle (active, achieved, archived...), startDate, targetDate or coverImageKey.",
        scopes: [FinancialPlanningRead, GoalsWrite],
        access: Write,
        schema: update_schema(
            "Goal id.",
            "title, description, targetAmount, priority, statusLifecycle, startDate, targetDate, coverImageKey",
        ),
        call: |env, args| {
            let args: UpdateArgs = parse(args)?;
            let current = env.goal_service().get_goal(&args.id).map_err(failed)?;
            let mut goal = patched(&current, Some(&args.changes))?;
            goal.id = current.id.clone();
            goal.goal_type = current.goal_type.clone();
            goal.currency = Some(env.base_currency());
            if args.dry_run {
                return ok(serde_json::json!({ "dryRun": true, "goal": goal }));
            }
            ok(env.goal_service().update_goal(goal).await.map_err(failed)?)
        }
    }
}

agent_tool! {
    DeleteGoal {
        name: "delete_goal",
        description: "Delete a goal with its funding rules and plan.",
        scopes: [FinancialPlanningRead, GoalsWrite],
        access: Write,
        schema: delete_schema("Goal id."),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the goal and its plan")?;
            env.goal_service().delete_goal(args.id.clone()).await.map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.id }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FundingArgs {
    goal_id: String,
    rules: Vec<GoalFundingRuleInput>,
}

agent_tool! {
    SaveGoalFunding {
        name: "save_goal_funding",
        description: "Replace which accounts fund a goal: each rule has accountId, sharePercent (0-100; an account's shares across goals cannot exceed 100) and taxBucket (taxable, tax_deferred, tax_free; the retirement planner reads it).",
        scopes: [FinancialPlanningRead, GoalsWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "goalId": { "type": "string" },
                "rules": {
                    "type": "array",
                    "items": object_schema(
                        serde_json::json!({
                            "accountId": { "type": "string" },
                            "sharePercent": { "type": "number" },
                            "taxBucket": { "type": ["string", "null"] }
                        }),
                        &["accountId", "sharePercent"],
                    )
                }
            }),
            &["goalId", "rules"],
        ),
        call: |env, args| {
            let args: FundingArgs = parse(args)?;
            let rules = env
                .goal_service()
                .save_goal_funding(&args.goal_id, args.rules)
                .await
                .map_err(failed)?;
            let summary_error = refresh_goal_summary(&env, &args.goal_id).await;
            ok(serde_json::json!({ "rules": rules, "summaryRefreshError": summary_error }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoalPlanArgs {
    goal_id: String,
    #[serde(default)]
    settings: Option<serde_json::Value>,
    #[serde(default)]
    planner_mode: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

agent_tool! {
    UpdateGoalPlan {
        name: "update_goal_plan",
        description: "Change a goal's saved plan: `settings` is a JSON merge patch of the plan settings from get_goal_details (for a retirement goal: personal, expenses, incomeStreams, investment, tax, and taxOptimizer for the tax-optimized style, including its saved cases); plannerMode switches a retirement goal's style (fire, traditional, optimized). The app validates the result; dryRun returns it without saving. Test changes first with planOverrides on the run_* tools.",
        scopes: [FinancialPlanningRead, GoalsWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "goalId": { "type": "string" },
                "settings": { "type": "object", "description": "JSON merge patch of the plan settings." },
                "plannerMode": { "type": "string", "enum": ["fire", "traditional", "optimized"] },
                "dryRun": dry_run_schema()
            }),
            &["goalId"],
        ),
        call: |env, args| {
            let args: GoalPlanArgs = parse(args)?;
            let goals = env.goal_service();
            let goal = goals.get_goal(&args.goal_id).map_err(failed)?;
            let existing = goals.get_goal_plan(&args.goal_id).map_err(failed)?;
            let plan_kind = if goal.goal_type == "retirement" { "retirement" } else { "save_up" };
            let mut settings = match &existing {
                Some(plan) => serde_json::from_str(&plan.settings_json)
                    .map_err(|err| failed(format!("Saved plan settings are not valid JSON: {err}")))?,
                None => serde_json::json!({}),
            };
            if let Some(patch) = &args.settings {
                merge_patch(&mut settings, patch);
            }
            if plan_kind == "retirement" {
                if let Some(object) = settings.as_object_mut() {
                    object.insert("currency".to_string(), env.base_currency().into());
                }
            }
            let planner_mode = args
                .planner_mode
                .or_else(|| existing.as_ref().and_then(|plan| plan.planner_mode.clone()));
            if planner_mode.is_some() && plan_kind != "retirement" {
                return Err(invalid("plannerMode applies to retirement goals only"));
            }
            if args.dry_run {
                return ok(serde_json::json!({
                    "dryRun": true, "planKind": plan_kind, "plannerMode": planner_mode, "settings": settings
                }));
            }
            let saved = goals
                .save_goal_plan(SaveGoalPlan {
                    goal_id: args.goal_id.clone(),
                    plan_kind: plan_kind.to_string(),
                    planner_mode,
                    settings_json: serde_json::to_string(&settings)?,
                    summary_json: existing.map(|plan| plan.summary_json),
                })
                .await
                .map_err(failed)?;
            let summary_error = refresh_goal_summary(&env, &args.goal_id).await;
            ok(serde_json::json!({
                "planKind": saved.plan_kind,
                "plannerMode": saved.planner_mode,
                "settings": serde_json::from_str::<serde_json::Value>(&saved.settings_json).unwrap_or(settings),
                "version": saved.version,
                "summaryRefreshError": summary_error,
            }))
        }
    }
}

// ─── Activities ──────────────────────────────────────────────────────────────

agent_tool! {
    UpdateActivity {
        name: "update_activity",
        description: "Change an activity (by id from search_activities): any of accountId, activityType, subtype, activityDate, quantity, unitPrice, amount, fee, tax, currency, fxRate, notes, needsReview, metadata, or asset ({ id } or { symbol, exchangeMic }). Fields not given keep their values; null clears a number. A linked transfer's other side is updated to match. dryRun returns the change without saving.",
        scopes: [ActivitiesRead, ActivitiesDraft, ActivitiesWrite],
        access: Write,
        schema: update_schema(
            "Activity id.",
            "accountId, activityType, subtype, activityDate (ISO date or date-time), quantity, unitPrice, amount, fee, tax, currency, fxRate, notes, needsReview, metadata (JSON text), asset",
        ),
        call: |env, args| {
            let args: UpdateArgs = parse(args)?;
            let existing = env.activity_service().get_activity(&args.id).map_err(failed)?;
            let mut fields = serde_json::json!({
                "accountId": existing.account_id,
                "activityType": existing.activity_type,
                "activityDate": existing.activity_date.to_rfc3339(),
                "currency": existing.currency,
            });
            merge_patch(&mut fields, &args.changes);
            if let Some(object) = fields.as_object_mut() {
                object.insert("id".to_string(), args.id.clone().into());
            }
            let update: ActivityUpdate = serde_json::from_value(fields.clone())
                .map_err(|err| invalid(format!("Invalid changes: {err}")))?;
            if args.dry_run {
                return ok(serde_json::json!({ "dryRun": true, "before": existing, "update": fields }));
            }
            let updated = env.activity_service().update_activity(update).await.map_err(failed)?;
            env.health_service().clear_cache().await;
            ok(updated)
        }
    }
}

agent_tool! {
    DeleteActivity {
        name: "delete_activity",
        description: "Delete an activity (by id from search_activities). Holdings and valuations are recalculated.",
        scopes: [ActivitiesRead, ActivitiesDraft, ActivitiesWrite],
        access: Write,
        schema: delete_schema("Activity id."),
        call: |env, args| {
            let args: DeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the activity")?;
            let deleted = env
                .activity_service()
                .delete_activity(args.id)
                .await
                .map_err(failed)?;
            env.health_service().clear_cache().await;
            ok(deleted)
        }
    }
}

// ─── Taxonomy categories ─────────────────────────────────────────────────────

agent_tool! {
    CreateTaxonomyCategory {
        name: "create_taxonomy_category",
        description: "Add a category to a taxonomy (asset classes, sectors, regions, spending categories, a custom taxonomy...), optionally under a parent category.",
        scopes: [ClassificationRead, ClassificationSuggest, ClassificationWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "taxonomyId": { "type": "string" },
                "parentId": { "type": ["string", "null"] },
                "name": { "type": "string" },
                "key": { "type": "string", "description": "Stable machine key, e.g. dining_out." },
                "color": { "type": "string", "description": "Hex color, e.g. #4f86f7." },
                "description": { "type": ["string", "null"] },
                "sortOrder": { "type": "integer" },
                "icon": { "type": ["string", "null"] }
            }),
            &["taxonomyId", "name", "key", "color"],
        ),
        call: |env, args| {
            let mut fields = serde_json::json!({ "sortOrder": 0 });
            merge_patch(&mut fields, &args);
            let mut category: NewCategory = parse(fields)?;
            category.id = None;
            ok(env.taxonomy_service().create_category(category).await.map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CategoryUpdateArgs {
    taxonomy_id: String,
    category_id: String,
    changes: serde_json::Value,
    #[serde(default)]
    dry_run: bool,
}

agent_tool! {
    UpdateTaxonomyCategory {
        name: "update_taxonomy_category",
        description: "Change a taxonomy category's name, key, color, description, icon, sortOrder or parentId.",
        scopes: [ClassificationRead, ClassificationSuggest, ClassificationWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "taxonomyId": { "type": "string" },
                "categoryId": { "type": "string" },
                "changes": changes_schema("name, key, color, description, icon, sortOrder, parentId"),
                "dryRun": dry_run_schema()
            }),
            &["taxonomyId", "categoryId", "changes"],
        ),
        call: |env, args| {
            let args: CategoryUpdateArgs = parse(args)?;
            let taxonomy = env
                .taxonomy_service()
                .get_taxonomy(&args.taxonomy_id)
                .map_err(failed)?
                .ok_or_else(|| invalid(format!("Taxonomy {} not found", args.taxonomy_id)))?;
            let current = taxonomy
                .categories
                .into_iter()
                .find(|category| category.id == args.category_id)
                .ok_or_else(|| invalid(format!("Category {} not found", args.category_id)))?;
            let mut category = patched(&current, Some(&args.changes))?;
            category.id = current.id.clone();
            category.taxonomy_id = current.taxonomy_id.clone();
            if args.dry_run {
                return ok(serde_json::json!({ "dryRun": true, "category": category }));
            }
            ok(env.taxonomy_service().update_category(category).await.map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CategoryDeleteArgs {
    taxonomy_id: String,
    category_id: String,
    #[serde(default)]
    confirm: bool,
}

agent_tool! {
    DeleteTaxonomyCategory {
        name: "delete_taxonomy_category",
        description: "Delete a taxonomy category; assets and transactions assigned to it become unassigned.",
        scopes: [ClassificationRead, ClassificationSuggest, ClassificationWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "taxonomyId": { "type": "string" },
                "categoryId": { "type": "string" },
                "confirm": confirm_schema()
            }),
            &["taxonomyId", "categoryId", "confirm"],
        ),
        call: |env, args| {
            let args: CategoryDeleteArgs = parse(args)?;
            require_confirm(args.confirm, "the category")?;
            env.taxonomy_service()
                .delete_category(&args.taxonomy_id, &args.category_id)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.category_id }))
        }
    }
}

// ─── Health ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HealthIssueArgs {
    issue_id: String,
    #[serde(default)]
    data_hash: Option<String>,
    #[serde(default)]
    restore: bool,
}

agent_tool! {
    DismissHealthIssue {
        name: "dismiss_health_issue",
        description: "Dismiss a health issue (by id and dataHash from get_health_status) so it stops showing until its data changes, or bring a dismissed issue back with restore true.",
        scopes: [HealthRead, HealthWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "issueId": { "type": "string" },
                "dataHash": { "type": "string", "description": "Required to dismiss." },
                "restore": { "type": "boolean" }
            }),
            &["issueId"],
        ),
        call: |env, args| {
            let args: HealthIssueArgs = parse(args)?;
            let health = env.health_service();
            if args.restore {
                health.restore_issue(&args.issue_id).await.map_err(failed)?;
                return ok(serde_json::json!({ "restored": args.issue_id }));
            }
            let data_hash = args
                .data_hash
                .ok_or_else(|| invalid("dataHash is required to dismiss an issue"))?;
            health
                .dismiss_issue(&args.issue_id, &data_hash)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "dismissed": args.issue_id }))
        }
    }
}

// ─── Reads ───────────────────────────────────────────────────────────────────

agent_tool! {
    GetSettings {
        name: "get_settings",
        description: "Get the app settings: base currency, timezone, language, number formatting region, theme and default return metric.",
        scopes: [AccountsRead],
        access: Read,
        schema: object_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            ok(env.settings_service().get_settings().map_err(failed)?)
        }
    }
}

agent_tool! {
    ListImportTemplates {
        name: "list_import_templates",
        description: "List the CSV import templates (built-in broker templates and saved ones): column mappings, activity type mappings and parse settings. get_import_mapping shows the mapping an account uses.",
        scopes: [ActivitiesRead],
        access: Read,
        schema: object_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            ok(env.activity_service().list_import_templates().map_err(failed)?)
        }
    }
}
