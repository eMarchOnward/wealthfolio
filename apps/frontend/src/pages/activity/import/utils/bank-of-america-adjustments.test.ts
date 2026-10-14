import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { format } from "date-fns";
import { describe, expect, it } from "vitest";
import { AccountType, ActivityType } from "@/lib/constants";
import { setParsedData } from "../context/import-actions";
import { createDraftActivities } from "./draft-utils";
import {
  addBankOfAmericaTypeColumn,
  applyBankOfAmericaAdjustments,
  BANK_OF_AMERICA_SKIP_REASONS,
  BANK_OF_AMERICA_TEMPLATE_ID,
  BANK_OF_AMERICA_TYPE_COLUMN,
} from "./bank-of-america-adjustments";

const MIGRATION_PATH = resolve(
  __dirname,
  "../../../../../../../crates/storage-sqlite/migrations/2026-10-12-000002_bank_of_america_opening_balance/up.sql",
);

function loadSeededTemplate() {
  const sql = readFileSync(MIGRATION_PATH, "utf8");
  expect(sql).toContain(`'${BANK_OF_AMERICA_TEMPLATE_ID}'`);
  const json = /'(\{"fieldMappings".*\})'/.exec(sql)?.[1];
  if (!json) throw new Error("Bank of America template config not found in migration");
  return JSON.parse(json) as {
    fieldMappings: Record<string, string | string[]>;
    activityMappings: Record<string, string[]>;
    parseConfig: {
      dateFormat: string;
      decimalSeparator: string;
      thousandsSeparator: string;
      skipTopRows: number;
    };
  };
}

const HEADERS = ["Date", "Description", "Amount", "Running Bal."];

// Rows as the CSV parser returns them after skipping the summary block. The
// Zelle memos show how unescaped inner quotes come through; a comma inside them
// splits the row, which the parser then truncates to the header width.
const ROWS: string[][] = [
  ["05/01/2025", "Beginning balance as of 05/01/2025", "", "5,000.00"],
  ["05/02/2025", 'Zelle payment from A PAYER for RENT"; Conf# AAAA11111"', "500.00", "5,500.00"],
  ["05/06/2025", "CARD ISSUER DES:AUTOPAY ID:000000000000001 PPD", "-100.00", "5,400.00"],
  ["05/09/2025", "EMPLOYER DES:PAYROLL ID:000000000000003 PPD", "2,000.00", "7,400.00"],
  ["05/13/2025", "VANGUARD BUY DES:INVESTMENT ID:000000000000002 PPD", "-800.00", "6,600.00"],
  [
    "03/02/2026",
    "Zelle payment to A LANDLORD for Rent for March",
    ' 2026"; Conf# bbbb22222"',
    "-1,500.00",
  ],
];

const ACCOUNT_ID = "checking";

function importDrafts() {
  const template = loadSeededTemplate();
  const { headers, rows } = addBankOfAmericaTypeColumn(HEADERS, ROWS);
  const drafts = createDraftActivities(
    rows,
    headers,
    {
      fieldMappings: template.fieldMappings,
      activityMappings: template.activityMappings,
      symbolMappings: {},
      accountMappings: {},
    },
    { ...template.parseConfig, defaultCurrency: "USD" },
    ACCOUNT_ID,
    new Set([ACCOUNT_ID]),
    new Map([[ACCOUNT_ID, AccountType.CASH]]),
  );
  return applyBankOfAmericaAdjustments(drafts);
}

describe("addBankOfAmericaTypeColumn", () => {
  it("derives the type from the amount sign and cleans stray quotes", () => {
    const { headers, rows } = addBankOfAmericaTypeColumn(HEADERS, ROWS);

    expect(headers).toEqual([...HEADERS, BANK_OF_AMERICA_TYPE_COLUMN]);
    expect(rows.map((row) => row[4])).toEqual([
      "Beginning balance",
      "Credit",
      "Debit",
      "Credit",
      "Transfer to Vanguard",
      "Debit",
    ]);
    expect(rows[0][2]).toBe("5,000.00");
    expect(rows[1][1]).toBe("Zelle payment from A PAYER for RENT; Conf# AAAA11111");
    expect(rows[5].slice(1, 3)).toEqual([
      "Zelle payment to A LANDLORD for Rent for March, 2026; Conf# bbbb22222",
      "-1,500.00",
    ]);
  });

  it("leaves other files unchanged", () => {
    const headers = ["Date", "Type", "Amount"];
    const rows = [["2025-05-01", "DEPOSIT", "10"]];

    expect(addBankOfAmericaTypeColumn(headers, rows)).toEqual({ headers, rows });
  });

  it("is applied once when parsed data is stored", () => {
    const first = setParsedData(HEADERS, ROWS);
    if (first.type !== "SET_PARSED_DATA") throw new Error("unexpected action");
    const again = setParsedData(first.payload.headers, first.payload.rows);
    if (again.type !== "SET_PARSED_DATA") throw new Error("unexpected action");

    expect(again.payload.headers).toEqual([...HEADERS, BANK_OF_AMERICA_TYPE_COLUMN]);
  });
});

describe("Bank of America template", () => {
  it("skips the summary block above the transaction header", () => {
    expect(loadSeededTemplate().parseConfig.skipTopRows).toBe(6);
  });

  it("imports the opening balance as a deposit, skipped until restored", () => {
    const [opening] = importDrafts();

    expect(opening.skipReason).toBe(BANK_OF_AMERICA_SKIP_REASONS.openingBalance);
  });

  it("imports credits as deposits, debits as withdrawals and Vanguard contributions as transfers out", () => {
    const drafts = importDrafts();

    expect(
      drafts.map((d) => [
        d.activityDate && format(new Date(d.activityDate), "yyyy-MM-dd"),
        d.activityType,
        d.amount,
        d.status,
        d.comment,
      ]),
    ).toEqual([
      [
        "2025-05-01",
        ActivityType.DEPOSIT,
        "5000.00",
        "skipped",
        "Beginning balance as of 05/01/2025",
      ],
      [
        "2025-05-02",
        ActivityType.DEPOSIT,
        "500.00",
        "valid",
        "Zelle payment from A PAYER for RENT; Conf# AAAA11111",
      ],
      [
        "2025-05-06",
        ActivityType.WITHDRAWAL,
        "100.00",
        "valid",
        "CARD ISSUER DES:AUTOPAY ID:000000000000001 PPD",
      ],
      [
        "2025-05-09",
        ActivityType.DEPOSIT,
        "2000.00",
        "valid",
        "EMPLOYER DES:PAYROLL ID:000000000000003 PPD",
      ],
      [
        "2025-05-13",
        ActivityType.TRANSFER_OUT,
        "800.00",
        "valid",
        "VANGUARD BUY DES:INVESTMENT ID:000000000000002 PPD",
      ],
      [
        "2026-03-02",
        ActivityType.WITHDRAWAL,
        "1500.00",
        "valid",
        "Zelle payment to A LANDLORD for Rent for March, 2026; Conf# bbbb22222",
      ],
    ]);
  });
});
