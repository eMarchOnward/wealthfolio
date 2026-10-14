import type { RetirementPlan, SavedTaxPlanRun, TaxPlanStore } from "../types";

/** Something the comparison can show: the current plan or a saved case, with its result. */
export interface CompareItem {
  id: string;
  name: string;
  plan: RetirementPlan;
  run: SavedTaxPlanRun;
}

export const CURRENT_PLAN_ID = "current";
export const MAX_COMPARED = 4;

/** The current plan (when its result is up to date) and every case saved with a result. */
export function comparableItems(
  store: TaxPlanStore,
  plan: RetirementPlan,
  freshRun: SavedTaxPlanRun | null,
  currentName: string,
): CompareItem[] {
  const items: CompareItem[] = [];
  if (freshRun) items.push({ id: CURRENT_PLAN_ID, name: currentName, plan, run: freshRun });
  for (const c of store.cases) {
    if (c.run) items.push({ id: c.id, name: c.name, plan: c.plan, run: c.run });
  }
  return items;
}

/** Inputs left out of the differences: derived or not set by the user. */
const IGNORED_PATHS = new Set(["tax", "version", "currency", "personal.currentAge"]);

/** Leaf values of `value` by dotted path; arrays are one value each. */
function flatten(value: unknown, path: string, out: Map<string, unknown>) {
  if (IGNORED_PATHS.has(path)) return;
  if (value && typeof value === "object" && !Array.isArray(value)) {
    for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
      flatten(child, path ? `${path}.${key}` : key, out);
    }
    return;
  }
  if (value !== undefined) out.set(path, value);
}

export interface SettingDifference {
  path: string;
  /** One value per plan, in order; `undefined` where a plan does not set it. */
  values: unknown[];
}

/** The settings whose values are not the same in every plan, in path order. */
export function settingsDifferences(plans: RetirementPlan[]): SettingDifference[] {
  const flat = plans.map((plan) => {
    const out = new Map<string, unknown>();
    flatten(plan, "", out);
    return out;
  });
  const paths = new Set(flat.flatMap((f) => [...f.keys()]));
  const differences: SettingDifference[] = [];
  for (const path of [...paths].sort()) {
    const values = flat.map((f) => f.get(path));
    const keys = values.map((v) => JSON.stringify(v ?? null));
    if (keys.some((key) => key !== keys[0])) differences.push({ path, values });
  }
  return differences;
}

/** "taxOptimizer.markets.startYear" → "Tax Plan › Markets › Start year". */
export function settingLabel(path: string): string {
  return path
    .split(".")
    .map((part) => {
      if (part === "taxOptimizer") return "Tax Plan";
      const words = part.replace(/([a-z0-9])([A-Z])/g, "$1 $2").toLowerCase();
      return words.charAt(0).toUpperCase() + words.slice(1);
    })
    .join(" › ");
}

/** A short display form of a setting's value. */
export function settingValue(value: unknown): string {
  if (value === undefined || value === null || value === "") return "—";
  if (Array.isArray(value)) return value.length === 1 ? "1 item" : `${value.length} items`;
  if (typeof value === "number") return String(Math.round(value * 10_000) / 10_000);
  if (typeof value === "object") {
    const text = JSON.stringify(value);
    return text.length > 40 ? `${text.slice(0, 39)}…` : text;
  }
  return String(value as string | boolean | bigint);
}
