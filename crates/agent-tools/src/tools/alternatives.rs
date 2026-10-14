//! Alternative asset tools: properties, vehicles, collectibles, precious
//! metals, other assets and liabilities (loans, mortgages), with their
//! valuations, loan schedules and loan actions.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use wealthfolio_core::assets::loan::{LoanAction, LoanSetup};
use wealthfolio_core::assets::{
    CreateAlternativeAssetRequest, LinkLiabilityRequest, UpdateAssetDetailsRequest,
    UpdateValuationRequest,
};
use wealthfolio_core::events::DomainEvent;

use super::support::{agent_tool, failed, object_schema, ok, parse, platform, require_confirm};

fn asset_id_schema() -> serde_json::Value {
    serde_json::json!({ "type": "string", "description": "Alternative asset id (from get_alternative_assets)." })
}

const LOAN_SETUP: &str = "Loan terms: { originalAmount, originationDate (YYYY-MM-DD), interestRate (decimal, e.g. 0.065), schedule: { paymentAmount, frequency, termEndDate, interestMethod, ... } }, as returned in a liability's `loan` field.";

agent_tool! {
    GetAlternativeAssets {
        name: "get_alternative_assets",
        description: "List alternative assets and liabilities (property, vehicle, collectible, precious metal, other, loans and mortgages) with current value, purchase price and gain, valuation date, metadata, linked asset and loan terms.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            let platform = platform(&env)?;
            ok(platform
                .alternative_asset_service
                .get_alternative_holdings()
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetArgs {
    asset_id: String,
}

agent_tool! {
    GetLoanPayments {
        name: "get_loan_payments",
        description: "Get a loan's payment history and schedule: each payment's date, amount, interest and principal, and the balance after it.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(serde_json::json!({ "assetId": asset_id_schema() }), &["assetId"]),
        call: |env, args| {
            let args: AssetArgs = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .alternative_asset_service
                .get_loan_payments(&args.asset_id)
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewTermsArgs {
    #[serde(default)]
    asset_id: Option<String>,
    setup: LoanSetup,
}

agent_tool! {
    PreviewLoanTerms {
        name: "preview_loan_terms",
        description: "Preview the payment schedule loan terms produce (for a new loan, or an existing one with assetId) without saving anything.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "assetId": { "type": "string" },
                "setup": { "type": "object", "description": LOAN_SETUP }
            }),
            &["setup"],
        ),
        call: |env, args| {
            let args: PreviewTermsArgs = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .alternative_asset_service
                .preview_loan_terms(args.asset_id.as_deref(), &args.setup)
                .map_err(failed)?)
        }
    }
}

agent_tool! {
    CreateAlternativeAsset {
        name: "create_alternative_asset",
        description: "Create an alternative asset or liability with its current value. kind: PROPERTY, VEHICLE, COLLECTIBLE, PRECIOUS_METAL, PRIVATE_EQUITY, OTHER or LIABILITY. A liability can link to the asset it finances (linkedAssetId) and carry loan terms (loan).",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "kind": { "type": "string", "enum": ["PROPERTY", "VEHICLE", "COLLECTIBLE", "PRECIOUS_METAL", "PRIVATE_EQUITY", "OTHER", "LIABILITY"] },
                "name": { "type": "string" },
                "currency": { "type": "string" },
                "currentValue": { "type": ["string", "number"], "description": "For a liability, the balance owed (positive)." },
                "valueDate": { "type": "string", "description": "YYYY-MM-DD" },
                "purchasePrice": { "type": ["string", "number", "null"] },
                "purchaseDate": { "type": ["string", "null"] },
                "metadata": { "type": ["object", "null"], "description": "Kind-specific details, e.g. property address or vehicle make." },
                "linkedAssetId": { "type": ["string", "null"] },
                "loan": { "type": ["object", "null"], "description": LOAN_SETUP }
            }),
            &["kind", "name", "currency", "currentValue", "valueDate"],
        ),
        call: |env, args| {
            let request: CreateAlternativeAssetRequest = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .alternative_asset_service
                .create_alternative_asset(request)
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ValuationArgs {
    asset_id: String,
    value: Decimal,
    date: NaiveDate,
    #[serde(default)]
    notes: Option<String>,
}

agent_tool! {
    UpdateAlternativeAssetValuation {
        name: "update_alternative_asset_valuation",
        description: "Record an alternative asset's value (or a liability's balance) on a date; history before it is kept.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "value": { "type": ["string", "number"] },
                "date": { "type": "string", "description": "YYYY-MM-DD" },
                "notes": { "type": ["string", "null"] }
            }),
            &["assetId", "value", "date"],
        ),
        call: |env, args| {
            let args: ValuationArgs = parse(args)?;
            let platform = platform(&env)?;
            ok(platform
                .alternative_asset_service
                .update_valuation(UpdateValuationRequest {
                    asset_id: args.asset_id,
                    value: args.value,
                    date: args.date,
                    notes: args.notes,
                })
                .await
                .map_err(failed)?)
        }
    }
}

agent_tool! {
    UpdateAlternativeAssetDetails {
        name: "update_alternative_asset_details",
        description: "Change an alternative asset's name, notes, metadata (keys given are set; a null value removes the key) or loan terms.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "name": { "type": ["string", "null"] },
                "notes": { "type": ["string", "null"] },
                "metadata": { "type": ["object", "null"] },
                "loan": { "type": ["object", "null"], "description": LOAN_SETUP }
            }),
            &["assetId"],
        ),
        call: |env, args| {
            let request: UpdateAssetDetailsRequest = parse(args)?;
            let platform = platform(&env)?;
            platform
                .alternative_asset_service
                .update_asset_details(request)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "updated": true }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteAssetArgs {
    asset_id: String,
    #[serde(default)]
    confirm: bool,
}

agent_tool! {
    DeleteAlternativeAsset {
        name: "delete_alternative_asset",
        description: "Delete an alternative asset or liability with its valuation history.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "confirm": { "type": "boolean", "description": "Must be true: deleting cannot be undone." }
            }),
            &["assetId", "confirm"],
        ),
        call: |env, args| {
            let args: DeleteAssetArgs = parse(args)?;
            require_confirm(args.confirm, "the asset and its history")?;
            let platform = platform(&env)?;
            platform
                .alternative_asset_service
                .delete_alternative_asset(&args.asset_id)
                .await
                .map_err(failed)?;
            ok(serde_json::json!({ "deleted": args.asset_id }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkArgs {
    liability_id: String,
    #[serde(default)]
    target_asset_id: Option<String>,
}

agent_tool! {
    LinkLiability {
        name: "link_liability",
        description: "Link a liability to the asset it finances (e.g. a mortgage to its property), or unlink it with targetAssetId null.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "liabilityId": { "type": "string" },
                "targetAssetId": { "type": ["string", "null"] }
            }),
            &["liabilityId"],
        ),
        call: |env, args| {
            let args: LinkArgs = parse(args)?;
            let platform = platform(&env)?;
            let service = &platform.alternative_asset_service;
            let response = match args.target_asset_id {
                Some(target_asset_id) => service
                    .link_liability(LinkLiabilityRequest {
                        liability_id: args.liability_id,
                        target_asset_id,
                    })
                    .await,
                None => service.unlink_liability(&args.liability_id).await,
            };
            ok(response.map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoanActionArgs {
    asset_id: String,
    action: LoanAction,
}

agent_tool! {
    ApplyLoanAction {
        name: "apply_loan_action",
        description: "Apply an action to a loan, as the loan page does. action.type: confirm_balance {date, balance}; extra_repayment {date, amount}; change_payment {date, paymentAmount}; recalculate {date, annualRate}; renew {date, annualRate, paymentAmount?, frequency?, interestMethod?, termEndDate?, balance?}; close {date}; set_terms (loan terms). Valuations are recalculated when balances change.",
        scopes: [HoldingsRead, PortfolioWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "action": { "type": "object", "description": "Tagged by `type`, see the tool description." }
            }),
            &["assetId", "action"],
        ),
        call: |env, args| {
            let args: LoanActionArgs = parse(args)?;
            let platform = platform(&env)?;
            let result = platform
                .alternative_asset_service
                .apply_loan_action(&args.asset_id, args.action)
                .await
                .map_err(failed)?;
            if result.balances_changed {
                platform.domain_event_sink.emit(DomainEvent::PriceHistoryChanged);
            }
            ok(serde_json::json!({ "balancesChanged": result.balances_changed }))
        }
    }
}
