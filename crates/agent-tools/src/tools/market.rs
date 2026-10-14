//! Market data and holdings-history tools: assets and their profiles, quote
//! history, exchange rates, tax lots and holdings snapshots, plus the market
//! data writes (sync, quote deletes, exchange rates, asset profiles).

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use wealthfolio_core::assets::UpdateAssetProfile as AssetProfileUpdate;
use wealthfolio_core::events::DomainEvent;
use wealthfolio_core::fx::NewExchangeRate;
use wealthfolio_core::quotes::SyncMode;

use super::support::{
    agent_tool, failed, invalid, object_schema, ok, parse, patched, platform, require_confirm,
};
use crate::env::AgentPlatformServices;

const MAX_ASSETS: usize = 200;
const MAX_QUOTES: usize = 1_000;
const DEFAULT_QUOTES: usize = 400;
const MAX_SYNC_DAYS: i64 = 365 * 30;
const MANUAL_SOURCE: &str = "MANUAL";

fn date_schema(description: &str) -> serde_json::Value {
    serde_json::json!({ "type": "string", "description": description })
}

fn asset_id_schema() -> serde_json::Value {
    serde_json::json!({ "type": "string", "description": "Asset id (from get_holdings or get_assets)." })
}

/// Recalculates valuations from the saved quotes and rates, as the app does
/// after a manual market data change.
fn recalculate(platform: &AgentPlatformServices) {
    platform
        .domain_event_sink
        .emit(DomainEvent::PriceHistoryChanged);
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetAssetsArgs {
    #[serde(default)]
    asset_ids: Option<Vec<String>>,
    #[serde(default)]
    query: Option<String>,
}

agent_tool! {
    GetAssets {
        name: "get_assets",
        description: "List assets (securities, funds, crypto, cash and alternative assets) with kind, name, symbol, exchange, quote currency and quote mode (MARKET from providers, MANUAL from saved quotes). Filter by assetIds or a name/symbol query; at most 200 are returned.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "assetIds": { "type": "array", "items": { "type": "string" } },
                "query": { "type": "string", "description": "Case-insensitive match on id, name or symbol." }
            }),
            &[],
        ),
        call: |env, args| {
            let args: GetAssetsArgs = parse(args)?;
            let assets = match &args.asset_ids {
                Some(ids) => env.asset_service().get_assets_by_asset_ids(ids).await,
                None => env.asset_service().get_assets(),
            }
            .map_err(failed)?;
            let query = args.query.map(|query| query.to_lowercase());
            let matches: Vec<_> = assets
                .into_iter()
                .filter(|asset| {
                    let Some(query) = &query else { return true };
                    [Some(&asset.id), asset.name.as_ref(), asset.display_code.as_ref(), asset.instrument_symbol.as_ref()]
                        .into_iter()
                        .flatten()
                        .any(|text| text.to_lowercase().contains(query))
                })
                .collect();
            let total = matches.len();
            let assets: Vec<_> = matches.into_iter().take(MAX_ASSETS).collect();
            ok(serde_json::json!({ "assets": assets, "total": total, "truncated": total > MAX_ASSETS }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteHistoryArgs {
    asset_id: String,
    #[serde(default)]
    start_date: Option<NaiveDate>,
    #[serde(default)]
    end_date: Option<NaiveDate>,
    #[serde(default)]
    limit: Option<usize>,
}

agent_tool! {
    GetQuoteHistory {
        name: "get_quote_history",
        description: "Get an asset's saved daily quotes (close, open/high/low, currency, source) between optional dates, newest first; at most 1000 (default 400).",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "startDate": date_schema("YYYY-MM-DD"),
                "endDate": date_schema("YYYY-MM-DD"),
                "limit": { "type": "integer" }
            }),
            &["assetId"],
        ),
        call: |env, args| {
            let args: QuoteHistoryArgs = parse(args)?;
            let mut quotes = env
                .quote_service()
                .get_historical_quotes(&args.asset_id)
                .map_err(failed)?;
            quotes.retain(|quote| {
                let day = quote.timestamp.date_naive();
                args.start_date.is_none_or(|start| day >= start)
                    && args.end_date.is_none_or(|end| day <= end)
            });
            quotes.sort_by_key(|quote| std::cmp::Reverse(quote.timestamp));
            let total = quotes.len();
            quotes.truncate(args.limit.unwrap_or(DEFAULT_QUOTES).clamp(1, MAX_QUOTES));
            ok(serde_json::json!({ "quotes": quotes, "total": total }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetIdsArgs {
    asset_ids: Vec<String>,
}

agent_tool! {
    GetLatestQuotes {
        name: "get_latest_quotes",
        description: "Get the latest saved quote of each asset.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({ "assetIds": { "type": "array", "items": { "type": "string" } } }),
            &["assetIds"],
        ),
        call: |env, args| {
            let args: AssetIdsArgs = parse(args)?;
            if args.asset_ids.len() > MAX_ASSETS {
                return Err(invalid(format!("Ask for at most {MAX_ASSETS} assets")));
            }
            ok(env.quote_service().get_latest_quotes(&args.asset_ids).map_err(failed)?)
        }
    }
}

agent_tool! {
    GetExchangeRates {
        name: "get_exchange_rates",
        description: "Get the latest exchange rate of every currency pair the app tracks, with its source (provider or MANUAL) and date.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            let platform = platform(&env)?;
            ok(platform.fx_service.get_latest_exchange_rates().map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LotsArgs {
    asset_id: String,
    #[serde(default)]
    include_snapshot_positions: bool,
}

agent_tool! {
    GetTaxLots {
        name: "get_tax_lots",
        description: "Get an asset's open tax lots across accounts: acquisition date, quantity, cost basis per share and total, current value and unrealized gain, and holding period. includeSnapshotPositions adds positions from holdings-only accounts.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "includeSnapshotPositions": { "type": "boolean" }
            }),
            &["assetId"],
        ),
        call: |env, args| {
            let args: LotsArgs = parse(args)?;
            ok(env
                .holdings_service()
                .get_asset_lot_view(&args.asset_id, args.include_snapshot_positions)
                .await
                .map_err(failed)?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotsArgs {
    account_id: String,
    #[serde(default)]
    start_date: Option<NaiveDate>,
    #[serde(default)]
    end_date: Option<NaiveDate>,
    #[serde(default)]
    date: Option<NaiveDate>,
}

agent_tool! {
    GetHoldingsSnapshots {
        name: "get_holdings_snapshots",
        description: "List an account's holdings snapshots (dated positions and cash: calculated from activities, entered manually, or imported) between optional dates; with date, return that day's snapshot as holdings instead.",
        scopes: [HoldingsRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "accountId": { "type": "string" },
                "startDate": date_schema("YYYY-MM-DD"),
                "endDate": date_schema("YYYY-MM-DD"),
                "date": date_schema("Return this day's snapshot holdings, YYYY-MM-DD.")
            }),
            &["accountId"],
        ),
        call: |env, args| {
            let args: SnapshotsArgs = parse(args)?;
            let platform = platform(&env)?;
            let snapshots = &platform.snapshot_service;
            if let Some(date) = args.date {
                let snapshot = snapshots
                    .get_holdings_keyframes(&args.account_id, Some(date), Some(date))
                    .map_err(failed)?
                    .into_iter()
                    .find(|snapshot| snapshot.snapshot_date == date)
                    .ok_or_else(|| invalid(format!("No snapshot on {date}")))?;
                return ok(env
                    .holdings_service()
                    .holdings_from_snapshot(&snapshot, &env.base_currency())
                    .await
                    .map_err(failed)?);
            }
            ok(snapshots
                .get_snapshot_metadata(&args.account_id, args.start_date, args.end_date)
                .map_err(failed)?)
        }
    }
}

// ─── Writes ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncArgs {
    #[serde(default)]
    asset_ids: Option<Vec<String>>,
    #[serde(default)]
    refetch_recent_days: Option<i64>,
    #[serde(default)]
    backfill_days: Option<i64>,
}

agent_tool! {
    SyncMarketData {
        name: "sync_market_data",
        description: "Fetch new quotes and exchange rates from the market data providers (all assets, or assetIds), then recalculate valuations. refetchRecentDays re-downloads the last N days; backfillDays rebuilds up to N days of history.",
        scopes: [HoldingsRead, MarketDataWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assetIds": { "type": "array", "items": { "type": "string" } },
                "refetchRecentDays": { "type": "integer" },
                "backfillDays": { "type": "integer" }
            }),
            &[],
        ),
        call: |env, args| {
            let args: SyncArgs = parse(args)?;
            let mode = match (args.refetch_recent_days, args.backfill_days) {
                (Some(_), Some(_)) => {
                    return Err(invalid("Give refetchRecentDays or backfillDays, not both"))
                }
                (Some(days), None) => SyncMode::RefetchRecent { days: days.clamp(1, MAX_SYNC_DAYS) },
                (None, Some(days)) => SyncMode::BackfillHistory { days: days.clamp(1, MAX_SYNC_DAYS) },
                (None, None) => SyncMode::Incremental,
            };
            let platform = platform(&env)?;
            let result = env
                .quote_service()
                .sync(mode, args.asset_ids)
                .await
                .map_err(failed)?;
            if result.synced > 0 {
                recalculate(&platform);
            }
            ok(serde_json::json!({
                "synced": result.synced,
                "quotesSynced": result.quotes_synced,
                "skipped": result.skipped,
                "failed": result.failed,
                "failures": result
                    .failures
                    .iter()
                    .map(|(asset_id, error)| serde_json::json!({ "assetId": asset_id, "error": error }))
                    .collect::<Vec<_>>(),
            }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteQuoteArgs {
    quote_id: String,
    #[serde(default)]
    confirm: bool,
}

agent_tool! {
    DeleteQuote {
        name: "delete_quote",
        description: "Delete a saved quote (by its id from get_quote_history) and recalculate valuations. Save corrected quotes with prepare_quote_import / commit_quote_import.",
        scopes: [HoldingsRead, MarketDataWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "quoteId": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true: deleting cannot be undone." }
            }),
            &["quoteId", "confirm"],
        ),
        call: |env, args| {
            let args: DeleteQuoteArgs = parse(args)?;
            require_confirm(args.confirm, "the quote")?;
            let platform = platform(&env)?;
            env.quote_service().delete_quote(&args.quote_id).await.map_err(failed)?;
            recalculate(&platform);
            ok(serde_json::json!({ "deleted": args.quote_id }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeRateArgs {
    from_currency: String,
    to_currency: String,
    #[serde(default)]
    rate: Option<Decimal>,
    #[serde(default)]
    source: Option<String>,
}

agent_tool! {
    SaveExchangeRate {
        name: "save_exchange_rate",
        description: "Add a currency pair, or set a manual pair's rate. With source MANUAL (default) the rate is required and used as-is; with a provider source (e.g. YAHOO) the pair's rates are fetched. Valuations are recalculated.",
        scopes: [HoldingsRead, MarketDataWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "fromCurrency": { "type": "string" },
                "toCurrency": { "type": "string" },
                "rate": { "type": ["number", "string"] },
                "source": { "type": "string", "description": "MANUAL (default) or a provider id." }
            }),
            &["fromCurrency", "toCurrency"],
        ),
        call: |env, args| {
            let args: ExchangeRateArgs = parse(args)?;
            let platform = platform(&env)?;
            let fx = &platform.fx_service;
            let source = args.source.unwrap_or_else(|| MANUAL_SOURCE.to_string());
            let manual = source == MANUAL_SOURCE;
            let existing = fx
                .get_latest_exchange_rates()
                .map_err(failed)?
                .into_iter()
                .find(|rate| rate.from_currency == args.from_currency && rate.to_currency == args.to_currency);
            let saved = match (existing, manual) {
                (Some(_), true) => {
                    let rate = args.rate.ok_or_else(|| invalid("A manual rate needs `rate`"))?;
                    fx.update_exchange_rate(&args.from_currency, &args.to_currency, rate)
                        .await
                        .map_err(failed)?
                }
                (Some(existing), false) => existing,
                (None, _) => {
                    if manual && args.rate.is_none() {
                        return Err(invalid("A manual rate needs `rate`"));
                    }
                    fx.add_exchange_rate(NewExchangeRate {
                        from_currency: args.from_currency,
                        to_currency: args.to_currency,
                        rate: args.rate.unwrap_or_default(),
                        source,
                    })
                    .await
                    .map_err(failed)?
                }
            };
            if !manual {
                env.quote_service()
                    .sync(SyncMode::Incremental, Some(vec![saved.id.clone()]))
                    .await
                    .map_err(failed)?;
            }
            recalculate(&platform);
            ok(saved)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteRateArgs {
    rate_id: String,
    #[serde(default)]
    confirm: bool,
}

agent_tool! {
    DeleteExchangeRate {
        name: "delete_exchange_rate",
        description: "Delete a currency pair's exchange rate (by id from get_exchange_rates) and recalculate valuations.",
        scopes: [HoldingsRead, MarketDataWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "rateId": { "type": "string" },
                "confirm": { "type": "boolean", "description": "Must be true: deleting cannot be undone." }
            }),
            &["rateId", "confirm"],
        ),
        call: |env, args| {
            let args: DeleteRateArgs = parse(args)?;
            require_confirm(args.confirm, "the exchange rate")?;
            let platform = platform(&env)?;
            platform.fx_service.delete_exchange_rate(&args.rate_id).await.map_err(failed)?;
            recalculate(&platform);
            ok(serde_json::json!({ "deleted": args.rate_id }))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetProfileArgs {
    asset_id: String,
    changes: serde_json::Value,
    #[serde(default)]
    dry_run: bool,
}

agent_tool! {
    UpdateAssetProfile {
        name: "update_asset_profile",
        description: "Change an asset's profile: name, displayCode, notes, kind, quoteMode (MARKET or MANUAL), quoteCcy, instrumentType, instrumentSymbol, instrumentExchangeMic, providerConfig or metadata (a JSON merge patch of those fields). dryRun returns the profile that would be saved.",
        scopes: [HoldingsRead, MarketDataWrite],
        access: Write,
        schema: object_schema(
            serde_json::json!({
                "assetId": asset_id_schema(),
                "changes": { "type": "object" },
                "dryRun": { "type": "boolean" }
            }),
            &["assetId", "changes"],
        ),
        call: |env, args| {
            let args: AssetProfileArgs = parse(args)?;
            let asset = env.asset_service().get_asset_by_id(&args.asset_id).map_err(failed)?;
            let current = AssetProfileUpdate {
                name: asset.name,
                display_code: asset.display_code,
                notes: asset.notes.unwrap_or_default(),
                kind: Some(asset.kind),
                quote_mode: Some(asset.quote_mode),
                quote_ccy: Some(asset.quote_ccy),
                instrument_type: asset.instrument_type,
                instrument_symbol: asset.instrument_symbol,
                instrument_exchange_mic: asset.instrument_exchange_mic,
                provider_config: asset.provider_config,
                metadata: asset.metadata,
            };
            let profile = patched(&current, Some(&args.changes))?;
            if args.dry_run {
                return ok(serde_json::json!({ "dryRun": true, "profile": profile }));
            }
            ok(env
                .asset_service()
                .update_asset_profile(&args.asset_id, profile)
                .await
                .map_err(failed)?)
        }
    }
}
