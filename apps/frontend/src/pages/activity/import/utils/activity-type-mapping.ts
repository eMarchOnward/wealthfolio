import { ActivityType } from "@/lib/constants";

export const LEGACY_ACTIVITY_MAPPING_PREFIX_LENGTH = 12;

export function normalizeActivityLabel(value: string): string {
  return value
    .trim()
    .toUpperCase()
    .replace(/[\s-]+/g, "_");
}

function isLegacyActivityLabelMatch(
  normalizedCsvValue: string,
  normalizedMappedValue: string,
): boolean {
  return (
    normalizedMappedValue.length === LEGACY_ACTIVITY_MAPPING_PREFIX_LENGTH &&
    normalizedCsvValue.length > LEGACY_ACTIVITY_MAPPING_PREFIX_LENGTH &&
    normalizedCsvValue.startsWith(normalizedMappedValue) &&
    !normalizedMappedValue.endsWith("_") &&
    normalizedCsvValue[LEGACY_ACTIVITY_MAPPING_PREFIX_LENGTH] !== "_"
  );
}

/** Opt-in prefix match for labels with variable suffixes, e.g. "TRANSFER FROM *". */
function isWildcardActivityLabelMatch(
  normalizedCsvValue: string,
  normalizedMappedValue: string,
): boolean {
  if (!normalizedMappedValue.endsWith("*")) return false;
  const prefix = normalizedMappedValue.slice(0, -1);
  return prefix.length > 0 && normalizedCsvValue.startsWith(prefix);
}

/**
 * Find the best activity type match for a CSV value.
 * Only explicit user/template mappings count as resolved.
 * A narrow legacy fallback keeps old 12-character truncated mappings working
 * without reintroducing broad prefix collisions like TRANSFER_OUT vs TRANSFER_OUT_FEE.
 * Mapped values ending in "*" match any label with that prefix; exact matches win.
 */
export function findMappedActivityType(
  csvValue: string,
  activityMappings: Record<string, string[]>,
): ActivityType | null {
  const normalized = normalizeActivityLabel(csvValue);
  let legacyMatch: ActivityType | null = null;

  for (const [activityType, csvValues] of Object.entries(activityMappings)) {
    for (const value of csvValues ?? []) {
      const normalizedValue = normalizeActivityLabel(value);

      if (normalized === normalizedValue) {
        return activityType as ActivityType;
      }

      if (
        isWildcardActivityLabelMatch(normalized, normalizedValue) ||
        isLegacyActivityLabelMatch(normalized, normalizedValue)
      ) {
        if (legacyMatch && legacyMatch !== activityType) {
          return null;
        }
        legacyMatch = activityType as ActivityType;
      }
    }
  }

  return legacyMatch;
}
