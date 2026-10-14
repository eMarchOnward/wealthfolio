import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { ACTIVITY_SUBTYPES, ActivityType } from "@/lib/constants";
import type { DraftActivity } from "../context";
import { createDraftActivities, draftToActivityImport } from "./draft-utils";
import {
  applyVanguardAdjustments,
  VANGUARD_SKIP_REASONS,
  VANGUARD_TEMPLATE_ID,
} from "./vanguard-adjustments";

const MIGRATION_PATH = resolve(
  __dirname,
  "../../../../../../../crates/storage-sqlite/migrations/2026-10-08-000001_vanguard_import_template/up.sql",
);

function loadSeededTemplate() {
  const sql = readFileSync(MIGRATION_PATH, "utf8");
  expect(sql).toContain(`'${VANGUARD_TEMPLATE_ID}'`);
  const json = /'(\{"fieldMappings".*\})'/.exec(sql)?.[1];
  if (!json) throw new Error("Vanguard template config not found in migration");
  return JSON.parse(json) as {
    fieldMappings: Record<string, string | string[]>;
    activityMappings: Record<string, string[]>;
    parseConfig: { dateFormat: string; decimalSeparator: string; thousandsSeparator: string };
  };
}

const HEADERS = [
  "Settlement date",
  "Trade date",
  "Symbol",
  "Name",
  "Type",
  "Account type",
  "Quantity",
  "Price",
  "Commission & fees**",
  "Amount",
];

const TOTAL_STOCK = "Vanguard Total Stock Market Index Fund Admiral Shares";
const SETTLEMENT_FUND = "Vanguard Federal Money Market Fund (Settlement Fund)";
const T_BILL = "U S TREASURY BILL CPN  0.00000 % MTD 2024-01-11 DT";

// Rows as the CSV parser returns them after skipping the report's 3 title lines.
const ROWS: string[][] = [
  // Legacy mutual-fund account: buy without price, dividend with reinvested shares
  ["", "2/6/2015", "VTSAX", TOTAL_STOCK, "Buy", "", "100.0000", "", "", "$5,000.0000"],
  ["", "3/24/2015", "VTSAX", TOTAL_STOCK, "Dividend", "", "0.5000", "", "", "$25.0000"],
  // Matched move between Vanguard accounts
  [
    "",
    "10/9/2018",
    "VTSAX",
    TOTAL_STOCK,
    "TRANSFER TO 11111111",
    "",
    "-100.5000",
    "",
    "",
    "$7,000.0000",
  ],
  [
    "10/9/2018",
    "10/9/2018",
    "VTSAX",
    TOTAL_STOCK,
    "TRANSFER FROM 22222222",
    "CASH",
    "100.5000",
    "",
    "",
    "",
  ],
  // Brokerage rows
  [
    "11/21/2018",
    "11/20/2018",
    "VTSAX",
    TOTAL_STOCK,
    "Buy",
    "CASH",
    "75.9760",
    "$65.8100",
    "Free",
    "-$5,000.0000",
  ],
  [
    "11/21/2018",
    "11/21/2018",
    "",
    "To: BANK OF AMERICA, NA",
    "Funds Received",
    "CASH",
    "",
    "",
    "",
    "$5,000.0000",
  ],
  [
    "12/21/2018",
    "12/21/2018",
    "VTSAX",
    TOTAL_STOCK,
    "Reinvestment",
    "CASH",
    "2.6810",
    "$59.7700",
    "",
    "-$150.0000",
  ],
  ["12/21/2018", "12/21/2018", "VTSAX", TOTAL_STOCK, "Dividend", "CASH", "", "", "", "$150.0000"],
  ["1/31/2019", "1/31/2019", "VMFXX", SETTLEMENT_FUND, "Dividend", "CASH", "", "", "", "$1.2500"],
  [
    "1/31/2019",
    "1/31/2019",
    "VMFXX",
    SETTLEMENT_FUND,
    "Reinvestment",
    "CASH",
    "1.2500",
    "$1.0000",
    "",
    "-$1.2500",
  ],
  [
    "7/7/2023",
    "7/7/2023",
    "VMFXX",
    SETTLEMENT_FUND,
    "Sweep in",
    "CASH",
    "",
    "",
    "",
    "-$3,000.0000",
  ],
  [
    "7/13/2023",
    "7/10/2023",
    "",
    T_BILL,
    "Buy",
    "CASH",
    "3,000.0000",
    "$97.0000",
    "Free",
    "-$2,910.0000",
  ],
  [
    "7/13/2023",
    "7/13/2023",
    "VMFXX",
    SETTLEMENT_FUND,
    "Sweep out",
    "CASH",
    "",
    "",
    "",
    "$2,910.0000",
  ],
  ["1/11/2024", "1/11/2024", "", T_BILL, "Interest", "CASH", "", "", "", "$90.0000"],
  [
    "1/11/2024",
    "1/11/2024",
    "",
    T_BILL,
    "Corp Action (Redemption)",
    "CASH",
    "3,000.0000",
    "",
    "",
    "$2,910.0000",
  ],
  [
    "5/30/2024",
    "5/30/2024",
    "",
    "From: BANK OF AMERICA, NA",
    "Withdrawal",
    "CASH",
    "",
    "",
    "",
    "-$2,000.0000",
  ],
  ["1/6/2025", "1/6/2025", "", "CASH", "Transfer (Outgoing)", "CASH", "", "", "", "-$8,000.0000"],
  // Rollover IRA rows
  ["11/8/2017", "11/8/2017", "", "CASH", "Rollover (incoming)", "CASH", "", "", "", "$50,000.0000"],
  [
    "12/20/2017",
    "12/20/2017",
    "VTSAX",
    TOTAL_STOCK,
    "Capital gain (LT)",
    "CASH",
    "",
    "",
    "",
    "$12.3400",
  ],
  [
    "12/20/2017",
    "12/20/2017",
    "VTSAX",
    TOTAL_STOCK,
    "Reinvestment (LT gain)",
    "CASH",
    "0.1900",
    "$64.9500",
    "",
    "-$12.3400",
  ],
  [
    "1/30/2025",
    "1/30/2025",
    "VTSAX",
    TOTAL_STOCK,
    "Recharacterization (outgoing)",
    "CASH",
    "-25.3840",
    "",
    "",
    "-$3,709.3600",
  ],
  // Roth IRA rows
  [
    "11/12/2019",
    "11/12/2019",
    "",
    "To: BANK OF AMERICA, NA",
    "Contribution",
    "CASH",
    "",
    "",
    "",
    "$114.6300",
  ],
  [
    "8/18/2017",
    "8/18/2017",
    "VFIAX",
    "Vanguard 500 Index Fund Admiral Shares",
    "Conversion",
    "CASH",
    "68.7630",
    "",
    "",
    "$15,443.4400",
  ],
  [
    "1/15/2021",
    "1/15/2021",
    "VFIAX",
    "Vanguard 500 Index Fund Admiral Shares",
    "Sell (exchange)",
    "CASH",
    "-141.0490",
    "$347.9300",
    "",
    "$49,075.1800",
  ],
  [
    "1/15/2021",
    "1/15/2021",
    "VTSAX",
    TOTAL_STOCK,
    "Buy (exchange)",
    "CASH",
    "511.4660",
    "$95.9500",
    "",
    "-$49,075.1800",
  ],
  [
    "5/17/2019",
    "5/17/2019",
    "VBMFX",
    "Vanguard Total Bond Market Index Fund Investor Shares",
    "Share Conversion (outgoing)",
    "CASH",
    "-682.3120",
    "",
    "",
    "-$7,300.7400",
  ],
  [
    "5/17/2019",
    "5/17/2019",
    "VBTLX",
    "Vanguard Total Bond Market Index Fund Admiral Shares",
    "Share Conversion (incoming)",
    "CASH",
    "682.3120",
    "",
    "",
    "$7,300.7400",
  ],
  // A reinvestment reported without a price
  [
    "5/17/2019",
    "5/17/2019",
    "VBTLX",
    "Vanguard Total Bond Market Index Fund Admiral Shares",
    "Reinvestment",
    "CASH",
    "0.9770",
    "",
    "",
    "-$10.4500",
  ],
  // Report disclosures that follow the transactions
  ["DISCLOSURES"],
  ["(C) 2018 The Vanguard Group, Inc. All rights reserved."],
];

const ACCOUNT_ID = "vanguard-brokerage";

function importVanguardRows(rows: string[][] = ROWS): DraftActivity[] {
  const template = loadSeededTemplate();
  const accountTypeById = new Map([[ACCOUNT_ID, "SECURITIES"]]);
  const drafts = createDraftActivities(
    rows,
    HEADERS,
    {
      fieldMappings: template.fieldMappings,
      activityMappings: template.activityMappings,
      symbolMappings: {},
      accountMappings: {},
    },
    { ...template.parseConfig, defaultCurrency: "USD" },
    ACCOUNT_ID,
    new Set([ACCOUNT_ID]),
    accountTypeById,
  );
  return applyVanguardAdjustments(drafts, HEADERS, accountTypeById);
}

function draftFor(drafts: DraftActivity[], tradeDate: string, type: string): DraftActivity {
  const draft = drafts.find((d) => d.rawRow[1] === tradeDate && d.rawRow[4] === type);
  if (!draft) throw new Error(`No draft for ${tradeDate} ${type}`);
  return draft;
}

describe("applyVanguardAdjustments", () => {
  const drafts = importVanguardRows();

  it("prices a reinvestment reported without a price from its amount", () => {
    const reinvestment = draftFor(drafts, "5/17/2019", "Reinvestment");
    expect(reinvestment.status).toBe("valid");
    expect(Number(reinvestment.unitPrice)).toBeCloseTo(10.45 / 0.977, 6);
  });

  it("drops the report's disclosure lines", () => {
    expect(drafts.some((d) => d.rawRow[0] === "DISCLOSURES")).toBe(false);
    expect(drafts).toHaveLength(ROWS.length - 2);
  });

  it("maps regular brokerage and IRA rows without manual fixes", () => {
    const expectations: [string, string, string][] = [
      ["11/20/2018", "Buy", ActivityType.BUY],
      ["11/21/2018", "Funds Received", ActivityType.TRANSFER_IN],
      ["12/21/2018", "Reinvestment", ActivityType.BUY],
      ["12/21/2018", "Dividend", ActivityType.DIVIDEND],
      ["1/11/2024", "Interest", ActivityType.INTEREST],
      ["5/30/2024", "Withdrawal", ActivityType.WITHDRAWAL],
      ["1/6/2025", "Transfer (Outgoing)", ActivityType.WITHDRAWAL],
      ["11/8/2017", "Rollover (incoming)", ActivityType.DEPOSIT],
      ["12/20/2017", "Capital gain (LT)", ActivityType.DIVIDEND],
      ["12/20/2017", "Reinvestment (LT gain)", ActivityType.BUY],
      // Roth IRA: contributions are transfers from the bank so they can be linked.
      ["11/12/2019", "Contribution", ActivityType.TRANSFER_IN],
      ["8/18/2017", "Conversion", ActivityType.TRANSFER_IN],
      ["1/15/2021", "Sell (exchange)", ActivityType.SELL],
      ["1/15/2021", "Buy (exchange)", ActivityType.BUY],
      ["5/17/2019", "Share Conversion (outgoing)", ActivityType.TRANSFER_OUT],
      ["5/17/2019", "Share Conversion (incoming)", ActivityType.TRANSFER_IN],
    ];
    for (const [date, type, expected] of expectations) {
      const draft = draftFor(drafts, date, type);
      expect({ type, activityType: draft.activityType, status: draft.status }).toEqual({
        type,
        activityType: expected,
        status: "valid",
      });
    }

    const buy = draftFor(drafts, "11/20/2018", "Buy");
    expect(buy.activityDate).toContain("2018-11-20");
    expect(buy).toMatchObject({
      symbol: "VTSAX",
      quantity: "75.9760",
      unitPrice: "65.8100",
      amount: "5000.0000",
    });
  });

  it("moves recharacterized shares out instead of withdrawing cash", () => {
    expect(draftFor(drafts, "1/30/2025", "Recharacterization (outgoing)")).toMatchObject({
      activityType: ActivityType.TRANSFER_OUT,
      symbol: "VTSAX",
      quantity: "25.3840",
      status: "valid",
    });
  });

  it("treats the settlement fund as cash", () => {
    expect(draftFor(drafts, "7/7/2023", "Sweep in")).toMatchObject({
      status: "skipped",
      skipReason: VANGUARD_SKIP_REASONS.sweep,
    });
    expect(draftFor(drafts, "7/13/2023", "Sweep out").status).toBe("skipped");
    expect(draftFor(drafts, "1/31/2019", "Reinvestment")).toMatchObject({
      status: "skipped",
      skipReason: VANGUARD_SKIP_REASONS.settlementReinvestment,
    });
    expect(draftFor(drafts, "1/31/2019", "Dividend")).toMatchObject({
      activityType: ActivityType.INTEREST,
      symbol: undefined,
      amount: "1.2500",
      status: "valid",
    });
  });

  it("treats Treasury bills as cash-equivalents but keeps their interest", () => {
    expect(draftFor(drafts, "7/10/2023", "Buy")).toMatchObject({
      status: "skipped",
      skipReason: VANGUARD_SKIP_REASONS.treasuryBill,
    });
    expect(draftFor(drafts, "1/11/2024", "Corp Action (Redemption)").status).toBe("skipped");
    expect(draftFor(drafts, "1/11/2024", "Interest").status).toBe("valid");
  });

  it("skips matched transfers between Vanguard accounts", () => {
    for (const type of ["TRANSFER TO 11111111", "TRANSFER FROM 22222222"]) {
      expect(draftFor(drafts, "10/9/2018", type)).toMatchObject({
        status: "skipped",
        skipReason: VANGUARD_SKIP_REASONS.transferPair,
      });
    }
  });

  it("keeps an unmatched transfer leg as a share transfer", () => {
    const [transferIn] = importVanguardRows([ROWS[3]]);
    expect(transferIn).toMatchObject({
      activityType: ActivityType.TRANSFER_IN,
      quantity: "100.5000",
    });
    expect(transferIn.status).not.toBe("skipped");
  });

  it("converts legacy mutual-fund-account rows, deriving prices from amount", () => {
    expect(draftFor(drafts, "2/6/2015", "Buy")).toMatchObject({
      activityType: ActivityType.TRANSFER_IN,
      isExternal: true,
      quantity: "100.0000",
      unitPrice: "50",
      status: "valid",
    });
    expect(draftFor(drafts, "3/24/2015", "Dividend")).toMatchObject({
      activityType: ActivityType.DIVIDEND,
      subtype: ACTIVITY_SUBTYPES.DRIP,
      quantity: "0.5000",
      unitPrice: "50",
      amount: "25.0000",
      status: "valid",
    });
  });

  it("produces identical activities on re-import so duplicate detection matches", () => {
    const toImports = (list: DraftActivity[]) =>
      list.filter((d) => d.status !== "skipped").map(draftToActivityImport);
    expect(toImports(importVanguardRows())).toEqual(toImports(drafts));
  });
});
