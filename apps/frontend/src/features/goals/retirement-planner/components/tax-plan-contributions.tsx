import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  useAmountFormatting,
} from "@wealthfolio/ui";
import { useTranslation } from "react-i18next";
import type { ContributionCap, TaxOptimizedYear } from "../types";

/** A given Roth contribution, which may be Roth 401(k) or Roth IRA. */
const givenRoth = (y: TaxOptimizedYear) =>
  y.rothContribution - (y.roth401k ?? 0) - (y.rothIra ?? 0);

const COLUMNS: [string, (y: TaxOptimizedYear) => number][] = [
  ["pre_tax_401k", (y) => y.preTaxDeferral ?? 0],
  ["roth_401k", (y) => y.roth401k ?? 0],
  ["roth_ira", (y) => y.rothIra ?? 0],
  ["roth", givenRoth],
  ["hsa", (y) => y.hsaContribution],
  ["employer", (y) => y.employerMatch],
  ["taxable", (y) => y.taxableContribution],
];

/**
 * What to put into each account in each working year, in today's money:
 * the optimizer's choices, or the given amounts held to the IRS limits.
 */
export function TaxPlanContributionsTable({
  years,
  caps,
  currency,
}: {
  years: TaxOptimizedYear[];
  caps: ContributionCap[];
  currency: string;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const working = years.filter((y) => y.working);
  if (working.length === 0) {
    return (
      <p className="text-muted-foreground text-sm">{t("goals:tax_plan.contributions.none")}</p>
    );
  }
  // Results solved before the breakdown existed lack it.
  if (working[0].preTaxDeferral === undefined) {
    return (
      <p className="text-muted-foreground text-sm">{t("goals:tax_plan.contributions.rerun")}</p>
    );
  }
  const money = (value: number) =>
    Math.abs(value) < 0.5 ? "—" : formatAmount(Math.round(value), currency);
  // Accounts nothing goes into are left out.
  const columns = COLUMNS.filter(([, value]) => working.some((y) => Math.abs(value(y)) >= 0.5));
  const total = (y: TaxOptimizedYear) => columns.reduce((sum, [, value]) => sum + value(y), 0);

  return (
    <div className="space-y-3">
      <div className="overflow-x-auto">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("goals:tax_plan.table.year")}</TableHead>
              <TableHead>{t("goals:tax_plan.table.age")}</TableHead>
              {columns.map(([key]) => (
                <TableHead key={key} className="text-right">
                  {t(`goals:tax_plan.contributions.columns.${key}`)}
                </TableHead>
              ))}
              <TableHead className="text-right">
                {t("goals:tax_plan.contributions.columns.total")}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {working.map((y) => (
              <TableRow key={y.year}>
                <TableCell className="tabular-nums">{y.year}</TableCell>
                <TableCell className="tabular-nums">{y.age}</TableCell>
                {columns.map(([key, value]) => (
                  <TableCell key={key} className="text-right tabular-nums">
                    {money(value(y))}
                  </TableCell>
                ))}
                <TableCell className="text-right font-medium tabular-nums">
                  {money(total(y))}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      <div className="text-muted-foreground space-y-1 text-xs">
        {caps.map((cap) => (
          <p key={cap.account}>
            {t(`goals:tax_plan.contributions.capped_${cap.account}`, { age: cap.age })}
          </p>
        ))}
        {columns.some(([key]) => key === "roth") && (
          <p>{t("goals:tax_plan.contributions.roth_note")}</p>
        )}
        <p>{t("goals:tax_plan.contributions.note")}</p>
      </div>
    </div>
  );
}
