import type { DraftActivity } from "../context";

/** Id of the built-in template seeded by the bank_of_america_import_template migration. */
export const BANK_OF_AMERICA_TEMPLATE_ID = "system_bank_of_america";

/** Column added to Bank of America exports, which carry no transaction type. */
export const BANK_OF_AMERICA_TYPE_COLUMN = "Type";

export const BANK_OF_AMERICA_TYPES = {
  credit: "Credit",
  debit: "Debit",
  beginningBalance: "Beginning balance",
  vanguardContribution: "Transfer to Vanguard",
} as const;

export const BANK_OF_AMERICA_SKIP_REASONS = {
  openingBalance:
    "Opening balance. Restore it for the first statement you import; a later statement's balance is already counted.",
} as const;

const HEADERS = ["Date", "Description", "Amount", "Running Bal."] as const;
const BEGINNING_BALANCE_PATTERN = /^Beginning balance as of /i;
/** ACH pulls by Vanguard; they arrive as "Funds Received" in the Vanguard account. */
const VANGUARD_CONTRIBUTION_PATTERN = /^VANGUARD BUY\b/i;
const AMOUNT_PATTERN = /^-?[\d,]*\.?\d+$/;

/**
 * A comma inside an unescaped memo quote ("for "March, 2026"") splits the
 * description, pushing its tail into the amount column. Rejoin those pieces.
 */
function rejoinSplitDescription(row: string[]): string[] {
  let cells = row;
  while (cells.length > 3 && cells[2].trim() && !AMOUNT_PATTERN.test(cells[2].trim())) {
    cells = [cells[0], `${cells[1]},${cells[2]}`, ...cells.slice(3)];
  }
  return cells;
}

function isBankOfAmericaExport(headers: string[]): boolean {
  return headers.length === HEADERS.length && HEADERS.every((header, i) => headers[i] === header);
}

/**
 * Adapts a Bank of America checking/savings transaction export
 * ("Date, Description, Amount, Running Bal.") so a column template can map it:
 *
 * - adds a "Type" column from the amount sign: "Credit", "Debit", or
 *   "Beginning balance" for the statement's opening row, whose amount is taken
 *   from its running balance; Vanguard contributions get "Transfer to Vanguard"
 *   so they can be linked to the Vanguard side and count as saving, not spending
 * - strips the unescaped double quotes Bank of America leaves inside Zelle
 *   memos, which the CSV parser keeps as stray characters, and rejoins memos
 *   that a comma inside those quotes split across columns
 *
 * Other files are returned unchanged.
 */
export function addBankOfAmericaTypeColumn(
  headers: string[],
  rows: string[][],
): { headers: string[]; rows: string[][] } {
  if (!isBankOfAmericaExport(headers)) return { headers, rows };

  return {
    headers: [...headers, BANK_OF_AMERICA_TYPE_COLUMN],
    rows: rows.map((row) => {
      const [date = "", rawDescription = "", rawAmount = "", runningBalance = ""] =
        rejoinSplitDescription(row);
      const description = rawDescription.replace(/"/g, "").trim();
      const amount = rawAmount.trim();

      if (!amount && BEGINNING_BALANCE_PATTERN.test(description)) {
        return [
          date,
          description,
          runningBalance.trim(),
          runningBalance,
          BANK_OF_AMERICA_TYPES.beginningBalance,
        ];
      }

      const type = !amount
        ? ""
        : !amount.startsWith("-")
          ? BANK_OF_AMERICA_TYPES.credit
          : VANGUARD_CONTRIBUTION_PATTERN.test(description)
            ? BANK_OF_AMERICA_TYPES.vanguardContribution
            : BANK_OF_AMERICA_TYPES.debit;
      return [date, description, amount, runningBalance, type];
    }),
  };
}

/**
 * Skips the statement's opening balance, which the template imports as a
 * deposit: only the first statement imported needs it, and the review step can
 * restore it there.
 */
export function applyBankOfAmericaAdjustments(drafts: DraftActivity[]): DraftActivity[] {
  return drafts.map((draft) =>
    draft.rawRow[4]?.trim() === BANK_OF_AMERICA_TYPES.beginningBalance
      ? { ...draft, status: "skipped", skipReason: BANK_OF_AMERICA_SKIP_REASONS.openingBalance }
      : draft,
  );
}
