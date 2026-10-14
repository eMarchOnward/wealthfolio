import { ACTIVITY_SUBTYPES, ActivityType } from "@/lib/constants";
import type { DraftActivity } from "../context";
import { validateDraft } from "./draft-utils";
import { toNumber } from "./review-draft-utils";

/** Id of the built-in Vanguard template seeded by the vanguard_import_template migration. */
export const VANGUARD_TEMPLATE_ID = "system_vanguard";

export const VANGUARD_SKIP_REASONS = {
  sweep: "Vanguard settlement fund sweep (settlement fund is treated as cash)",
  settlementReinvestment:
    "Vanguard settlement fund reinvestment (settlement fund is treated as cash)",
  treasuryBill: "Treasury bill purchase/redemption (treated as cash; interest is imported)",
  transferPair: "Transfer between Vanguard accounts (matched pair, cancels out)",
} as const;

const COLUMNS = {
  tradeDate: "Trade date",
  symbol: "Symbol",
  name: "Name",
  type: "Type",
  accountType: "Account type",
  quantity: "Quantity",
} as const;

const SETTLEMENT_FUND_PATTERN = /\(settlement fund\)/i;
const TREASURY_BILL_PATTERN = /\bTREASURY BILL\b/i;
const TRANSFER_PATTERN = /^TRANSFER (FROM|TO) /i;

function normalizedType(value: string | undefined): string {
  return (value ?? "").trim().toUpperCase();
}

/** Unit price that reproduces `amount` for `quantity`, or undefined when either is missing. */
function derivedUnitPrice(amount: string | null | undefined, quantity: string | null | undefined) {
  const amountValue = toNumber(amount);
  const quantityValue = toNumber(quantity);
  if (!amountValue || !quantityValue) return undefined;
  return Number((amountValue / quantityValue).toFixed(8)).toString();
}

/**
 * Applies Vanguard "Transaction History" report conventions that a column/type
 * template cannot express:
 *
 * - drops the report's trailing disclosure lines
 * - treats the settlement fund (e.g. VMFXX) as cash: sweeps and its reinvestments
 *   are skipped and its dividends become cash interest
 * - treats Treasury bills as cash-equivalents: purchases and redemptions are
 *   skipped (redemptions return cost; the gain is reported as Interest)
 * - skips matched "TRANSFER FROM x" / "TRANSFER TO y" pairs for the same shares,
 *   which Vanguard reports when moving holdings between its own accounts
 * - fills the price of buys and sells reported without one (some reinvestments)
 * - converts legacy mutual-fund-account rows (blank "Account type"): buys without
 *   a price become external share transfers-in at their cost, and dividends that
 *   list reinvested shares become DRIP dividends
 *
 * Skipped rows keep a reason and can be restored in the review step.
 */
export function applyVanguardAdjustments(
  drafts: DraftActivity[],
  headers: string[],
  accountTypeById?: Map<string, string>,
): DraftActivity[] {
  const column = (row: string[], header: string): string => {
    const index = headers.indexOf(header);
    return index === -1 ? "" : (row[index] ?? "").trim();
  };

  const transferKey = (draft: DraftActivity): string => {
    const quantity = Math.abs(toNumber(column(draft.rawRow, COLUMNS.quantity)) ?? 0);
    return `${draft.accountId}|${draft.activityDate}|${column(draft.rawRow, COLUMNS.symbol)}|${quantity}`;
  };

  const transferDirections = new Map<string, Set<string>>();
  for (const draft of drafts) {
    const match = TRANSFER_PATTERN.exec(column(draft.rawRow, COLUMNS.type));
    if (!match) continue;
    const key = transferKey(draft);
    const directions = transferDirections.get(key) ?? new Set<string>();
    directions.add(match[1].toUpperCase());
    transferDirections.set(key, directions);
  }

  const skip = (draft: DraftActivity, reason: string): DraftActivity => ({
    ...draft,
    status: "skipped",
    skipReason: reason,
  });

  const revalidate = (draft: DraftActivity): DraftActivity => {
    const validation = validateDraft(draft, accountTypeById);
    return {
      ...draft,
      status: validation.status,
      errors: validation.errors,
      warnings: validation.warnings,
    };
  };

  return drafts.flatMap((draft): DraftActivity[] => {
    const row = draft.rawRow;
    const type = normalizedType(column(row, COLUMNS.type));
    const name = column(row, COLUMNS.name);

    if (!type && !column(row, COLUMNS.tradeDate)) return [];

    if (type === "SWEEP IN" || type === "SWEEP OUT") {
      return [skip(draft, VANGUARD_SKIP_REASONS.sweep)];
    }

    if (SETTLEMENT_FUND_PATTERN.test(name)) {
      if (type.startsWith("REINVESTMENT")) {
        return [skip(draft, VANGUARD_SKIP_REASONS.settlementReinvestment)];
      }
      if (type === "DIVIDEND") {
        return [
          revalidate({
            ...draft,
            activityType: ActivityType.INTEREST,
            symbol: undefined,
            assetCandidateKey: undefined,
            quantity: undefined,
            unitPrice: undefined,
            subtype: undefined,
          }),
        ];
      }
    }

    if (
      TREASURY_BILL_PATTERN.test(name) &&
      (type === "BUY" || type.startsWith("CORP ACTION (REDEMPTION)"))
    ) {
      return [skip(draft, VANGUARD_SKIP_REASONS.treasuryBill)];
    }

    if (TRANSFER_PATTERN.test(type)) {
      const directions = transferDirections.get(transferKey(draft));
      if (directions?.has("FROM") && directions.has("TO")) {
        return [skip(draft, VANGUARD_SKIP_REASONS.transferPair)];
      }
      return [draft];
    }

    const isLegacyAccountRow = !column(row, COLUMNS.accountType);
    if (isLegacyAccountRow && type === "BUY" && !toNumber(draft.unitPrice)) {
      const unitPrice = derivedUnitPrice(draft.amount, draft.quantity);
      if (unitPrice) {
        return [
          revalidate({
            ...draft,
            activityType: ActivityType.TRANSFER_IN,
            unitPrice,
            isExternal: true,
          }),
        ];
      }
    }

    if (isLegacyAccountRow && type === "DIVIDEND" && toNumber(draft.quantity)) {
      const unitPrice = derivedUnitPrice(draft.amount, draft.quantity);
      if (unitPrice) {
        return [revalidate({ ...draft, subtype: ACTIVITY_SUBTYPES.DRIP, unitPrice })];
      }
    }

    // Some reinvestments are reported without a price; the amount and shares give it.
    const isTrade =
      draft.activityType === ActivityType.BUY || draft.activityType === ActivityType.SELL;
    if (isTrade && !toNumber(draft.unitPrice)) {
      const unitPrice = derivedUnitPrice(draft.amount, draft.quantity);
      if (unitPrice) {
        return [revalidate({ ...draft, unitPrice: String(Math.abs(Number(unitPrice))) })];
      }
    }

    return [draft];
  });
}
