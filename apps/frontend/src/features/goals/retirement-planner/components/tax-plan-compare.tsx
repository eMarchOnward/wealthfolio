import {
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Checkbox,
  Label,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  useAmountFormatting,
  useNumberFormatting,
} from "@wealthfolio/ui";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  MAX_COMPARED,
  settingLabel,
  settingsDifferences,
  settingValue,
  type CompareItem,
} from "../lib/tax-plan-compare";
import type { TaxOptimizedPlan } from "../types";

const LINE_COLORS = ["var(--chart-1)", "var(--chart-3)", "var(--chart-5)", "var(--chart-2)"];
const SHOWN_DIFFERENCES = 15;

interface Row {
  label: string;
  values: string[];
  /** Highlighted when the cases differ here, since it changes what the numbers mean. */
  highlight?: boolean;
}

/**
 * Saved cases (and the current plan) side by side: how each was set, what it
 * spends, pays in tax and leaves, its success rate, the settings that differ,
 * and total balances over time.
 */
export function TaxPlanCompare({
  items,
  currency,
  onClose,
}: {
  items: CompareItem[];
  currency: string;
  /** Absent where the comparison has its own tab. */
  onClose?: () => void;
}) {
  const { t } = useTranslation();
  const { formatAmount, formatCompactAmount } = useAmountFormatting();
  const { formatPercent } = useNumberFormatting();
  const money = (value: number) => formatAmount(Math.round(value), currency);
  const [selected, setSelected] = useState<string[]>(() =>
    items.slice(0, MAX_COMPARED).map((item) => item.id),
  );
  const shown = items.filter((item) => selected.includes(item.id));
  const toggle = (id: string, on: boolean) =>
    setSelected((current) =>
      on
        ? current.length < MAX_COMPARED
          ? [...current, id]
          : current
        : current.filter((value) => value !== id),
    );

  const results = shown.map((item) => item.run.result);
  const feasible = (r: TaxOptimizedPlan) => r.status !== "infeasible";
  const cell = (r: TaxOptimizedPlan, value: string) =>
    feasible(r) ? value : t("goals:tax_plan.compare.no_solution");
  const objectives = results.map((r) =>
    r.objective === "maxSpending"
      ? t("goals:tax_plan.settings.objective_max_spending")
      : t("goals:tax_plan.settings.objective_max_bequest"),
  );
  const rows: Row[] = [
    {
      label: t("goals:tax_plan.compare.how_set"),
      values: results.map((r) =>
        r.retirementAgeOptions.length > 0
          ? t("goals:tax_plan.compare.earliest_age", { age: r.retirementAge })
          : t("goals:tax_plan.compare.fixed_age"),
      ),
    },
    {
      label: t("goals:tax_plan.compare.objective"),
      values: objectives,
      highlight: objectives.some((o) => o !== objectives[0]),
    },
    {
      label: t("goals:tax_plan.compare.retirement_age"),
      values: results.map((r) => String(r.retirementAge)),
    },
    {
      label: t("goals:tax_plan.compare.first_year_spending"),
      values: results.map((r) => cell(r, money(r.firstYearSpending))),
    },
    {
      label: t("goals:tax_plan.compare.lifetime_spending"),
      values: results.map((r) => cell(r, money(r.lifetimeSpending))),
    },
    {
      label: t("goals:tax_plan.compare.estate"),
      values: results.map((r) => cell(r, money(r.bequestAfterTax))),
    },
    ...(results.some((r) => r.home)
      ? [
          {
            label: t("goals:tax_plan.compare.home_value"),
            values: results.map((r) => (r.home ? money(r.home.valueAtEnd) : "—")),
          },
        ]
      : []),
    {
      label: t("goals:tax_plan.compare.lifetime_tax"),
      values: results.map((r) =>
        cell(
          r,
          money(r.lifetimeFederalTax + r.lifetimeStateTax + r.lifetimeEarlyWithdrawalPenalty),
        ),
      ),
    },
    {
      label: t("goals:tax_plan.compare.health"),
      values: results.map((r) => cell(r, money(r.lifetimeMedicare + r.lifetimeAca))),
    },
    {
      label: t("goals:tax_plan.compare.roth_conversions"),
      values: results.map((r) => cell(r, money(r.lifetimeRothConversions))),
    },
    {
      label: t("goals:tax_plan.compare.claim_age"),
      values: results.map((r) =>
        r.socialSecurityClaimAge == null
          ? "—"
          : String(Math.round(r.socialSecurityClaimAge * 10) / 10),
      ),
    },
    {
      label: t("goals:tax_plan.compare.success"),
      values: shown.map((item) => {
        const success = item.run.success;
        return success
          ? t("goals:tax_plan.compare.success_value", {
              rate: formatPercent(success.successRate, { digits: 0 }),
              method: t(`goals:tax_plan.success.run_${success.mode}`, { count: success.paths }),
              paths: success.paths,
            })
          : t("goals:tax_plan.compare.not_checked");
      }),
    },
    {
      label: t("goals:tax_plan.compare.safe_spending"),
      values: shown.map((item) => {
        const safe = item.run.safeSpending;
        return safe
          ? t("goals:tax_plan.compare.safe_spending_value", {
              amount: money(safe.spending),
              rate: Math.round(safe.targetRate * 100),
            })
          : t("goals:tax_plan.compare.not_checked");
      }),
    },
    {
      label: t("goals:tax_plan.compare.solved_on"),
      values: shown.map((item) => item.run.computedAt.slice(0, 10)),
    },
  ];

  const differences = useMemo(() => settingsDifferences(shown.map((item) => item.plan)), [shown]);

  // Total balances by age, one line per case.
  const chartData = useMemo(() => {
    const byAge = new Map<number, Record<string, number>>();
    shown.forEach((item, index) => {
      for (const year of item.run.result.years) {
        const point = byAge.get(year.age) ?? { age: year.age };
        point[`c${index}`] =
          year.taxableBalance + year.taxDeferredBalance + year.rothBalance + year.hsaBalance;
        byAge.set(year.age, point);
      }
    });
    return [...byAge.values()].sort((a, b) => a.age - b.age);
  }, [shown]);

  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between gap-3 pb-2">
        <div className="space-y-1">
          <CardTitle className="text-sm">{t("goals:tax_plan.compare.title")}</CardTitle>
          <p className="text-muted-foreground text-xs">{t("goals:tax_plan.compare.description")}</p>
        </div>
        {onClose && (
          <Button size="sm" variant="outline" onClick={onClose}>
            {t("goals:tax_plan.compare.close")}
          </Button>
        )}
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="flex flex-wrap gap-x-5 gap-y-2">
          {items.map((item) => {
            const checked = selected.includes(item.id);
            return (
              <div key={item.id} className="flex items-center gap-2">
                <Checkbox
                  id={`compare-${item.id}`}
                  checked={checked}
                  disabled={!checked && selected.length >= MAX_COMPARED}
                  onCheckedChange={(value) => toggle(item.id, value === true)}
                />
                <Label htmlFor={`compare-${item.id}`} className="text-sm">
                  {item.name}
                </Label>
              </div>
            );
          })}
        </div>
        <p className="text-muted-foreground text-xs">
          {t("goals:tax_plan.compare.pick", { max: MAX_COMPARED })}
        </p>

        {shown.length < 2 ? (
          <p className="text-muted-foreground text-sm">{t("goals:tax_plan.compare.pick_two")}</p>
        ) : (
          <>
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead />
                    {shown.map((item) => (
                      <TableHead key={item.id} className="text-right">
                        {item.name}
                      </TableHead>
                    ))}
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {rows.map((row) => (
                    <TableRow
                      key={row.label}
                      className={row.highlight ? "bg-amber-500/10" : undefined}
                    >
                      <TableCell className="text-muted-foreground text-xs">{row.label}</TableCell>
                      {row.values.map((value, index) => (
                        <TableCell key={index} className="text-right tabular-nums">
                          {value}
                        </TableCell>
                      ))}
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            {rows.some((row) => row.highlight) && (
              <p className="text-xs text-amber-700 dark:text-amber-400">
                {t("goals:tax_plan.compare.objective_differs")}
              </p>
            )}

            <div className="space-y-2">
              <h4 className="text-muted-foreground text-xs font-semibold uppercase tracking-wide">
                {t("goals:tax_plan.compare.differences")}
              </h4>
              {differences.length === 0 ? (
                <p className="text-muted-foreground text-sm">
                  {t("goals:tax_plan.compare.no_differences")}
                </p>
              ) : (
                <div className="overflow-x-auto">
                  <Table>
                    <TableBody>
                      {differences.slice(0, SHOWN_DIFFERENCES).map((difference) => (
                        <TableRow key={difference.path}>
                          <TableCell className="text-muted-foreground text-xs">
                            {settingLabel(difference.path)}
                          </TableCell>
                          {difference.values.map((value, index) => (
                            <TableCell key={index} className="text-right text-xs tabular-nums">
                              {settingValue(value)}
                            </TableCell>
                          ))}
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              )}
              {differences.length > SHOWN_DIFFERENCES && (
                <p className="text-muted-foreground text-xs">
                  {t("goals:tax_plan.compare.more_differences", {
                    count: differences.length - SHOWN_DIFFERENCES,
                  })}
                </p>
              )}
            </div>

            <div className="space-y-2">
              <h4 className="text-muted-foreground text-xs font-semibold uppercase tracking-wide">
                {t("goals:tax_plan.compare.balances")}
              </h4>
              <ResponsiveContainer width="100%" height={260}>
                <LineChart data={chartData} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
                  <CartesianGrid strokeDasharray="3 3" vertical={false} />
                  <XAxis dataKey="age" tickLine={false} axisLine={false} fontSize={11} />
                  <YAxis
                    tickFormatter={(value: number) => formatCompactAmount(value, currency)}
                    tickLine={false}
                    axisLine={false}
                    fontSize={11}
                    width={56}
                  />
                  <RTooltip
                    formatter={(value) => money(Number(value))}
                    labelFormatter={(age) => t("goals:tax_plan.chart.age_label", { age })}
                  />
                  <Legend wrapperStyle={{ fontSize: 11 }} />
                  {shown.map((item, index) => (
                    <Line
                      key={item.id}
                      type="monotone"
                      dataKey={`c${index}`}
                      name={item.name}
                      stroke={LINE_COLORS[index % LINE_COLORS.length]}
                      strokeWidth={2}
                      dot={false}
                    />
                  ))}
                </LineChart>
              </ResponsiveContainer>
              <p className="text-muted-foreground text-xs">
                {t("goals:tax_plan.compare.balances_note")}
              </p>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
