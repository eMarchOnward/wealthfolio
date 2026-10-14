import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { format } from "date-fns";
import { describe, expect, it } from "vitest";
import { AccountType, ActivityType } from "@/lib/constants";
import { setParsedData } from "../context/import-actions";
import { createDraftActivities } from "./draft-utils";
import {
  adaptFidelityNetBenefitsExport,
  FIDELITY_NETBENEFITS_TEMPLATE_ID,
  FIDELITY_PRICE_COLUMN,
  FIDELITY_SYMBOL_COLUMN,
  fidelityFundSymbol,
} from "./fidelity-adjustments";

const MIGRATION_PATH = resolve(
  __dirname,
  "../../../../../../../crates/storage-sqlite/migrations/2026-10-12-000001_fidelity_netbenefits_import_template/up.sql",
);

function loadSeededTemplate() {
  const sql = readFileSync(MIGRATION_PATH, "utf8");
  expect(sql).toContain(`'${FIDELITY_NETBENEFITS_TEMPLATE_ID}'`);
  const json = /'(\{"fieldMappings".*\})'/.exec(sql)?.[1];
  if (!json) throw new Error("Fidelity template config not found in migration");
  return JSON.parse(json) as {
    fieldMappings: Record<string, string | string[]>;
    activityMappings: Record<string, string[]>;
    parseConfig: {
      dateFormat: string;
      decimalSeparator: string;
      thousandsSeparator: string;
      headerRowIndex: number;
    };
  };
}

const HEADERS = ["Date", "Investment", "Transaction Type", "Amount", "Shares/Unit"];

// Rows as the CSV parser returns them with the template's header row: the plan
// name and date range above the header come through as rows, padded or cut to
// the header's five columns.
const ROWS: string[][] = [
  ["Plan name:", "ACME", " INC. RETIREMENT AND SAVINGS PLAN", "", ""],
  ["Date Range", "01/01/2015 - 10/08/2026", "", "", ""],
  ["09/25/2026", "BR LP INDEX 2045 F", "Contributions", "1,000.00", "80.000"],
  ["07/31/2025", "BR LP INDEX 2035 F", "Transfer In", "300,000.00", "25,000.000"],
];

const ACCOUNT_ID = "fidelity-401k";

function importDrafts() {
  const template = loadSeededTemplate();
  const { headers, rows } = adaptFidelityNetBenefitsExport(HEADERS, ROWS);
  return createDraftActivities(
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
    new Map([[ACCOUNT_ID, AccountType.SECURITIES]]),
  );
}

describe("adaptFidelityNetBenefitsExport", () => {
  it("keeps the transactions and adds symbols, prices and contribution deposits", () => {
    const { headers, rows } = adaptFidelityNetBenefitsExport(HEADERS, ROWS);

    expect(headers).toEqual([...HEADERS, FIDELITY_SYMBOL_COLUMN, FIDELITY_PRICE_COLUMN]);
    expect(rows).toEqual([
      ["09/25/2026", "BR LP INDEX 2045 F", "Contribution deposit", "1,000.00", "", "", ""],
      [
        "09/25/2026",
        "BR LP INDEX 2045 F",
        "Contributions",
        "1,000.00",
        "80.000",
        "BRLPINDEX2045F",
        "12.5",
      ],
      [
        "07/31/2025",
        "BR LP INDEX 2035 F",
        "Transfer In",
        "300,000.00",
        "25,000.000",
        "BRLPINDEX2035F",
        "12",
      ],
    ]);
  });

  it("names a fund by its letters and digits", () => {
    expect(fidelityFundSymbol("BR LP Index 2035 F")).toBe("BRLPINDEX2035F");
  });

  it("leaves other files unchanged", () => {
    const headers = ["Date", "Type", "Amount"];
    const rows = [["2025-05-01", "DEPOSIT", "10"]];

    expect(adaptFidelityNetBenefitsExport(headers, rows)).toEqual({ headers, rows });
  });

  it("is applied once when parsed data is stored", () => {
    const first = setParsedData(HEADERS, ROWS);
    if (first.type !== "SET_PARSED_DATA") throw new Error("unexpected action");
    const again = setParsedData(first.payload.headers, first.payload.rows);
    if (again.type !== "SET_PARSED_DATA") throw new Error("unexpected action");

    expect(again.payload.rows).toEqual(first.payload.rows);
  });
});

describe("Fidelity NetBenefits template", () => {
  it("reads the header below the plan name and date range", () => {
    expect(loadSeededTemplate().parseConfig.headerRowIndex).toBe(2);
  });

  it("imports contributions as a deposit and a buy, and the rollover as shares transferred in", () => {
    const drafts = importDrafts();

    expect(
      drafts.map((d) => [
        d.activityDate && format(new Date(d.activityDate), "yyyy-MM-dd"),
        d.activityType,
        d.symbol ?? "",
        d.quantity ?? "",
        d.unitPrice ?? "",
        d.amount ?? "",
        d.status,
      ]),
    ).toEqual([
      ["2026-09-25", ActivityType.DEPOSIT, "", "", "", "1000.00", "valid"],
      [
        "2026-09-25",
        ActivityType.BUY,
        "BRLPINDEX2045F",
        "80.000",
        "12.5",
        "1000.00",
        "valid",
      ],
      [
        "2025-07-31",
        ActivityType.TRANSFER_IN,
        "BRLPINDEX2035F",
        "25000.000",
        "12",
        "300000.00",
        "valid",
      ],
    ]);
  });
});
