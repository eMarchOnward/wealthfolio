//! Shared plumbing for the MCP platform tools: a declaration macro, argument
//! and result helpers, JSON merge patches, and the account values the
//! planning tools start from.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use wealthfolio_core::goals::{
    account_asset_map, cost_basis_map, AccountAssetMap, AccountValuationMap,
};
use wealthfolio_core::portfolio::valuation::CurrentAccountValuationService;
use wealthfolio_core::utils::time_utils::{parse_user_timezone_or_default, user_today};

use crate::env::{AgentEnvironment, AgentPlatformServices};
use crate::tool::{AgentToolError, AgentToolResult};

/// Declares a stateless tool: a unit struct plus its [`crate::AgentTool`]
/// impl. `call` receives the environment and the raw JSON arguments.
macro_rules! agent_tool {
    (
        $(#[$meta:meta])*
        $ty:ident {
            name: $name:literal,
            description: $description:expr,
            scopes: [$($scope:ident),* $(,)?],
            access: $access:ident,
            schema: $schema:expr,
            call: |$env:ident, $args:ident| $body:block $(,)?
        }
    ) => {
        $(#[$meta])*
        pub struct $ty;

        #[async_trait::async_trait]
        impl $crate::tool::AgentTool for $ty {
            fn name(&self) -> &'static str {
                $name
            }

            fn description(&self) -> &'static str {
                $description
            }

            fn input_schema(&self) -> serde_json::Value {
                $schema
            }

            fn required_scopes(&self) -> &'static [$crate::scope::AgentScope] {
                &[$($crate::scope::AgentScope::$scope),*]
            }

            fn access_level(&self) -> $crate::tool::AgentToolAccess {
                $crate::tool::AgentToolAccess::$access
            }

            #[allow(unused_variables)]
            async fn call(
                &self,
                $env: std::sync::Arc<dyn $crate::env::AgentEnvironment>,
                $args: serde_json::Value,
            ) -> Result<$crate::tool::AgentToolResult, $crate::tool::AgentToolError> {
                $body
            }
        }
    };
}
pub(crate) use agent_tool;

/// Parses tool arguments; `null` (no arguments) reads as an empty object.
pub(crate) fn parse<T: DeserializeOwned>(args: serde_json::Value) -> Result<T, AgentToolError> {
    let args = if args.is_null() {
        serde_json::json!({})
    } else {
        args
    };
    Ok(serde_json::from_value(args)?)
}

/// Wraps a serializable value as the tool result. MCP structured results are
/// objects, so a list comes back as `{ items, count }` and any other
/// non-object value as `{ result }`.
pub(crate) fn ok(value: impl Serialize) -> Result<AgentToolResult, AgentToolError> {
    let content = match serde_json::to_value(value)? {
        serde_json::Value::Array(items) => {
            serde_json::json!({ "count": items.len(), "items": items })
        }
        object @ serde_json::Value::Object(_) => object,
        other => serde_json::json!({ "result": other }),
    };
    Ok(AgentToolResult { content })
}

/// Maps a service error to a tool execution failure.
pub(crate) fn failed(err: impl std::fmt::Display) -> AgentToolError {
    AgentToolError::ExecutionFailed(err.to_string())
}

pub(crate) fn invalid(message: impl Into<String>) -> AgentToolError {
    AgentToolError::InvalidInput(message.into())
}

/// The host's platform services, or an error naming the tool's host gap.
pub(crate) fn platform(
    env: &Arc<dyn AgentEnvironment>,
) -> Result<AgentPlatformServices, AgentToolError> {
    env.platform_services().ok_or_else(|| {
        AgentToolError::ExecutionFailed("This tool is not available on this host".to_string())
    })
}

/// Today in the user's timezone.
pub(crate) fn today(platform: &AgentPlatformServices) -> chrono::NaiveDate {
    user_today(parse_user_timezone_or_default(&platform.timezone()))
}

/// Deletes are irreversible, so every delete tool asks for `confirm: true`.
pub(crate) fn require_confirm(confirm: bool, what: &str) -> Result<(), AgentToolError> {
    if confirm {
        Ok(())
    } else {
        Err(invalid(format!(
            "Deleting {what} cannot be undone. Call again with \"confirm\": true to delete it."
        )))
    }
}

/// Applies an RFC 7386 JSON merge patch: objects merge key by key, `null`
/// removes a key, anything else replaces the target.
pub(crate) fn merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    let Some(patch_fields) = patch.as_object() else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = serde_json::json!({});
    }
    let fields = target
        .as_object_mut()
        .expect("target was just made an object");
    for (key, value) in patch_fields {
        if value.is_null() {
            fields.remove(key);
        } else {
            merge_patch(
                fields.entry(key.clone()).or_insert(serde_json::Value::Null),
                value,
            );
        }
    }
}

/// `value` with `patch` merged in, read back as `T`.
pub(crate) fn patched<T: Serialize + DeserializeOwned>(
    value: &T,
    patch: Option<&serde_json::Value>,
) -> Result<T, AgentToolError> {
    let Some(patch) = patch else {
        return Ok(serde_json::from_value(serde_json::to_value(value)?)?);
    };
    let mut json = serde_json::to_value(value)?;
    merge_patch(&mut json, patch);
    serde_json::from_value(json).map_err(|err| invalid(format!("Invalid changes: {err}")))
}

/// Current value of every active account in base currency.
pub(crate) async fn account_values(
    env: &Arc<dyn AgentEnvironment>,
    platform: &AgentPlatformServices,
) -> Result<AccountValuationMap, AgentToolError> {
    let account_service = env.account_service();
    let asset_service = env.asset_service();
    let quote_service = env.quote_service();
    CurrentAccountValuationService::new(
        account_service.as_ref(),
        platform.snapshot_service.as_ref(),
        asset_service.as_ref(),
        quote_service.as_ref(),
        platform.fx_service.as_ref(),
    )
    .base_value_by_account(&env.base_currency(), today(platform))
    .await
    .map_err(failed)
}

/// Account values, cost bases and holdings by asset class: what the
/// tax-optimized plan starts from.
pub(crate) async fn tax_plan_maps(
    env: &Arc<dyn AgentEnvironment>,
    platform: &AgentPlatformServices,
) -> Result<(AccountValuationMap, AccountValuationMap, AccountAssetMap), AgentToolError> {
    let values = account_values(env, platform).await?;
    let account_ids: Vec<String> = values.keys().cloned().collect();
    let latest = env
        .valuation_service()
        .get_latest_valuations(&account_ids)
        .map_err(failed)?;
    let assets = account_asset_map(
        env.allocation_service().as_ref(),
        &account_ids,
        &env.base_currency(),
    )
    .await
    .map_err(failed)?;
    Ok((values, cost_basis_map(&latest), assets))
}

/// Runs CPU-heavy planning work on the blocking pool.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AgentToolError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| failed("The calculation stopped unexpectedly"))
}

/// Schema of an object with the given properties and required keys.
pub(crate) fn object_schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_patch_merges_objects_and_removes_nulls() {
        let mut target = serde_json::json!({
            "a": 1, "b": { "c": 2, "d": 3 }, "e": [1, 2]
        });
        merge_patch(
            &mut target,
            &serde_json::json!({ "a": 5, "b": { "c": null, "x": 9 }, "e": [3] }),
        );
        assert_eq!(
            target,
            serde_json::json!({ "a": 5, "b": { "d": 3, "x": 9 }, "e": [3] })
        );
    }

    #[test]
    fn results_are_always_objects() {
        assert_eq!(
            ok(vec![1, 2]).unwrap().content,
            serde_json::json!({ "count": 2, "items": [1, 2] })
        );
        assert_eq!(
            ok(None::<u8>).unwrap().content,
            serde_json::json!({ "result": null })
        );
        assert_eq!(
            ok(serde_json::json!({ "a": 1 })).unwrap().content,
            serde_json::json!({ "a": 1 })
        );
    }

    #[test]
    fn delete_needs_confirmation() {
        assert!(require_confirm(false, "the goal").is_err());
        assert!(require_confirm(true, "the goal").is_ok());
    }
}
