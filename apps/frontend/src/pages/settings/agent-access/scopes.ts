// Canonical agent-access token scopes shared by the web (server PAT) and
// desktop (Tauri) surfaces. The backend rejects unknown scopes, empty scope
// sets, and write scopes without their prerequisites; this module keeps the
// UI in sync with that contract.

import type { TFunction } from "i18next";

export type ScopeKey =
  | "accounts:read"
  | "holdings:read"
  | "performance:read"
  | "activities:read"
  | "financial-planning:read"
  | "health:read"
  | "classification:read"
  | "spending:read"
  | "activities:draft"
  | "activities:write"
  | "classification:suggest"
  | "classification:write"
  | "market-data:write"
  | "planning:run"
  | "accounts:write"
  | "goals:write"
  | "spending:write"
  | "portfolio:write"
  | "health:write";

export type ScopeGroup = "read" | "write";

export interface ScopeMeta {
  key: ScopeKey;
  /** i18n key suffix under `settings:agentAccess.scope.<i18n>`. */
  i18n: string;
  group: ScopeGroup;
}

/** Ordered list of all scopes, grouped reads first then write/suggest. */
export const SCOPES: ScopeMeta[] = [
  { key: "accounts:read", i18n: "accounts_read", group: "read" },
  { key: "holdings:read", i18n: "holdings_read", group: "read" },
  { key: "performance:read", i18n: "performance_read", group: "read" },
  { key: "activities:read", i18n: "activities_read", group: "read" },
  { key: "financial-planning:read", i18n: "financial_planning_read", group: "read" },
  { key: "health:read", i18n: "health_read", group: "read" },
  { key: "classification:read", i18n: "classification_read", group: "read" },
  { key: "spending:read", i18n: "spending_read", group: "read" },
  { key: "activities:draft", i18n: "activities_draft", group: "write" },
  { key: "activities:write", i18n: "activities_write", group: "write" },
  { key: "classification:suggest", i18n: "classification_suggest", group: "write" },
  { key: "classification:write", i18n: "classification_write", group: "write" },
  { key: "market-data:write", i18n: "market_data_write", group: "write" },
  { key: "planning:run", i18n: "planning_run", group: "write" },
  { key: "accounts:write", i18n: "accounts_write", group: "write" },
  { key: "goals:write", i18n: "goals_write", group: "write" },
  { key: "spending:write", i18n: "spending_write", group: "write" },
  { key: "portfolio:write", i18n: "portfolio_write", group: "write" },
  { key: "health:write", i18n: "health_write", group: "write" },
];

/**
 * Scopes each scope needs, mirroring `AgentScopeSet::dependency_error` in
 * `crates/agent-tools/src/scope.rs`: committing needs the draft or suggest
 * scope, and every other write or run scope needs the read scope of what it
 * changes.
 */
export const SCOPE_PREREQUISITES: Partial<Record<ScopeKey, ScopeKey[]>> = {
  "activities:write": ["activities:draft"],
  "classification:write": ["classification:suggest"],
  "market-data:write": ["holdings:read"],
  "planning:run": ["financial-planning:read"],
  "goals:write": ["financial-planning:read"],
  "accounts:write": ["accounts:read"],
  "spending:write": ["spending:read"],
  "portfolio:write": ["holdings:read"],
  "health:write": ["health:read"],
};

/** The read scopes, in canonical order. */
export const READ_SCOPES: ScopeKey[] = SCOPES.filter((scope) => scope.group === "read").map(
  (scope) => scope.key,
);

export interface ScopePreset {
  key: string;
  /** i18n key suffix under `settings:agentAccess.preset.<i18n>`. */
  i18n: string;
  scopes: ScopeKey[];
}

/** Named presets. Order matters for "summarize to preset name" matching. */
export const SCOPE_PRESETS: ScopePreset[] = [
  {
    key: "read-only",
    i18n: "read_only",
    scopes: [...READ_SCOPES],
  },
  {
    key: "read-activity-draft",
    i18n: "read_activity_draft",
    scopes: [...READ_SCOPES, "activities:draft"],
  },
  {
    key: "read-activity-write",
    i18n: "read_activity_write",
    scopes: [...READ_SCOPES, "activities:draft", "activities:write"],
  },
  {
    key: "read-activity-write-classification-suggest",
    i18n: "read_activity_write_classification_suggest",
    scopes: [
      ...READ_SCOPES,
      "activities:draft",
      "activities:write",
      "classification:suggest",
      "classification:write",
    ],
  },
  {
    key: "full-access",
    i18n: "full_access",
    scopes: SCOPES.map((scope) => scope.key),
  },
];

const SCOPE_I18N = new Map(SCOPES.map((scope) => [scope.key, scope.i18n]));

/** Human label for a scope key; falls back to the raw key for unknown scopes. */
export function scopeLabel(t: TFunction, key: string): string {
  const suffix = SCOPE_I18N.get(key as ScopeKey);
  return suffix ? t(`settings:agentAccess.scope.${suffix}.label`) : key;
}

/** Human description for a scope key; falls back to the raw key for unknown scopes. */
export function scopeDescription(t: TFunction, key: string): string {
  const suffix = SCOPE_I18N.get(key as ScopeKey);
  return suffix ? t(`settings:agentAccess.scope.${suffix}.description`) : key;
}

/** Localized label for a preset. */
export function presetLabel(t: TFunction, preset: ScopePreset): string {
  return t(`settings:agentAccess.preset.${preset.i18n}`);
}

/**
 * Apply dependency rules. Returns scopes in canonical order with duplicates removed.
 */
export function applyScopeDependencies(scopes: Iterable<string>): ScopeKey[] {
  const set = new Set<string>(scopes);
  for (const [scope, prerequisites] of Object.entries(SCOPE_PREREQUISITES)) {
    if (set.has(scope)) {
      prerequisites.forEach((prerequisite) => set.add(prerequisite));
    }
  }
  return SCOPES.filter((scope) => set.has(scope.key)).map((scope) => scope.key);
}

/** The first selected scope that needs `key`, which keeps `key` switched on. */
export function requiredBy(key: ScopeKey, selected: string[]): ScopeKey | undefined {
  return SCOPES.find(
    (scope) => selected.includes(scope.key) && (SCOPE_PREREQUISITES[scope.key] ?? []).includes(key),
  )?.key;
}

/** Returns the preset whose scope set exactly matches `scopes`, if any. */
export function matchPreset(scopes: string[]): ScopePreset | undefined {
  const target = new Set(scopes);
  return SCOPE_PRESETS.find(
    (preset) =>
      preset.scopes.length === target.size && preset.scopes.every((scope) => target.has(scope)),
  );
}
