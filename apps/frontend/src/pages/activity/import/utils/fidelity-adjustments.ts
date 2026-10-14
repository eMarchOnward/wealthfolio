/** Id of the built-in template seeded by the fidelity_netbenefits_import_template migration. */
export const FIDELITY_NETBENEFITS_TEMPLATE_ID = "system_fidelity_netbenefits";

/** Columns added to Fidelity NetBenefits exports, which carry no symbol or price. */
export const FIDELITY_SYMBOL_COLUMN = "Symbol";
export const FIDELITY_PRICE_COLUMN = "Price";

export const FIDELITY_TYPES = {
  contribution: "Contributions",
  transferIn: "Transfer In",
  /** Added before each contribution: the cash the contribution brings in. */
  contributionDeposit: "Contribution deposit",
} as const;

const HEADERS = ["Date", "Investment", "Transaction Type", "Amount", "Shares/Unit"] as const;
const DATE_PATTERN = /^\d{1,2}\/\d{1,2}\/\d{4}$/;

function isFidelityExport(headers: string[]): boolean {
  return headers.length === HEADERS.length && HEADERS.every((header, i) => headers[i] === header);
}

function toNumber(value: string | undefined): number {
  return Number((value ?? "").replace(/,/g, "").trim());
}

/**
 * Symbol for a plan fund, which has no ticker: its name without spaces or
 * punctuation ("BR LP INDEX 2035 F" becomes "BRLPINDEX2035F"), so later
 * downloads land on the same fund.
 */
export function fidelityFundSymbol(investment: string): string {
  return investment.toUpperCase().replace(/[^A-Z0-9]/g, "");
}

/** Price that reproduces `amount` for `shares`, or "" when either is missing. */
function unitPrice(amount: string, shares: string): string {
  const amountValue = toNumber(amount);
  const sharesValue = toNumber(shares);
  if (!amountValue || !sharesValue) return "";
  return Number((amountValue / sharesValue).toFixed(8)).toString();
}

/**
 * Adapts a Fidelity NetBenefits workplace-plan transaction history
 * ("Date, Investment, Transaction Type, Amount, Shares/Unit") so a column
 * template can map it:
 *
 * - drops the plan name and date range lines above the header (the template
 *   reads its header from the third non-empty line), and anything else that is
 *   not a dated transaction
 * - adds a "Symbol" column from the fund name (plan funds have no ticker) and a
 *   "Price" column of amount / shares
 * - puts a "Contribution deposit" row before each payroll contribution, so the
 *   contribution counts as money added and its purchase records the fund's
 *   price that day
 *
 * Other files are returned unchanged.
 */
export function adaptFidelityNetBenefitsExport(
  headers: string[],
  rows: string[][],
): { headers: string[]; rows: string[][] } {
  if (!isFidelityExport(headers)) return { headers, rows };

  const adapted: string[][] = [];
  for (const row of rows) {
    const [date = "", investment = "", type = "", amount = "", shares = ""] = row.map((cell) =>
      cell.trim(),
    );
    if (!DATE_PATTERN.test(date)) continue;
    const symbol = fidelityFundSymbol(investment);
    if (type === FIDELITY_TYPES.contribution) {
      adapted.push([date, investment, FIDELITY_TYPES.contributionDeposit, amount, "", "", ""]);
    }
    adapted.push([date, investment, type, amount, shares, symbol, unitPrice(amount, shares)]);
  }
  return {
    headers: [...HEADERS, FIDELITY_SYMBOL_COLUMN, FIDELITY_PRICE_COLUMN],
    rows: adapted,
  };
}
