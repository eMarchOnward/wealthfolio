import { runRetirementTaxSafeSpending } from "@/adapters";
import { useMutation } from "@tanstack/react-query";
import {
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Input,
  Label,
  useAmountFormatting,
} from "@wealthfolio/ui";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Line,
  LineChart,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { PathSpending, RetirementPlan, TaxPlanSafeSpending, TaxSuccessMode } from "../types";
import { histogramBins, simulationCount, SUCCESS_MODES } from "../lib/tax-plan-risk";

export const DEFAULT_SUCCESS_TARGET = 0.9;

/**
 * The most the plan can spend while succeeding on at least the target share
 * of historical or resampled market paths, with the whole spending/success
 * trade-off. One solve per path. `saved` is the last search kept with the
 * result; `readOnly` shows it without offering a new one (a saved case).
 */
export function TaxPlanSafeSpendingCard({
  goalId,
  plan,
  retirementAge,
  saved,
  onResult,
  onTargetChange,
  readOnly = false,
}: {
  goalId: string;
  plan: RetirementPlan;
  retirementAge?: number;
  saved?: TaxPlanSafeSpending;
  onResult?: (result: TaxPlanSafeSpending) => void;
  /** Saves the target success rate with the plan. */
  onTargetChange?: (target: number) => void;
  readOnly?: boolean;
}) {
  const { t } = useTranslation();
  const { formatAmount, formatCompactAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(Math.round(value), plan.currency);
  const target = plan.taxOptimizer?.successTarget ?? DEFAULT_SUCCESS_TARGET;
  const [text, setText] = useState(String(Math.round(target * 100)));
  useEffect(() => setText(String(Math.round(target * 100))), [target]);
  const commitTarget = () => {
    const value = Number(text);
    if (Number.isFinite(value) && value >= 1 && value <= 100) onTargetChange?.(value / 100);
    else setText(String(Math.round(target * 100)));
  };

  const mutation = useMutation({
    mutationFn: (mode: TaxSuccessMode) =>
      runRetirementTaxSafeSpending(goalId, mode, target, retirementAge),
    onSuccess: (data) => onResult?.(data),
  });
  const result = mutation.data ?? saved;
  const running = mutation.isPending;
  const chartData = result?.frontier.map((point) => ({
    spending: point.spending,
    rate: Math.round(point.successRate * 1000) / 10,
  }));

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.safe_spending.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <p className="text-muted-foreground">{t("goals:tax_plan.safe_spending.description")}</p>
        {readOnly && !result && (
          <p className="text-muted-foreground">{t("goals:tax_plan.safe_spending.not_run")}</p>
        )}
        {!readOnly && (
          <div className="flex flex-wrap items-end gap-3">
            <div className="space-y-1">
              <Label htmlFor="tax-success-target" className="text-xs">
                {t("goals:tax_plan.safe_spending.target")}
              </Label>
              <div className="relative w-28">
                <Input
                  id="tax-success-target"
                  inputMode="decimal"
                  className="pr-8"
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                  onBlur={commitTarget}
                  onKeyDown={(e) => e.key === "Enter" && commitTarget()}
                />
                <span className="text-muted-foreground pointer-events-none absolute inset-y-0 right-3 flex items-center text-xs">
                  %
                </span>
              </div>
            </div>
            {SUCCESS_MODES.map((mode) => (
              <Button
                key={mode}
                variant="outline"
                size="sm"
                disabled={running}
                onClick={() => mutation.mutate(mode)}
              >
                {running && mutation.variables === mode && (
                  <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t(`goals:tax_plan.success.run_${mode}`, { count: simulationCount(plan) })}
              </Button>
            ))}
          </div>
        )}
        {running && (
          <p className="text-muted-foreground">{t("goals:tax_plan.safe_spending.running")}</p>
        )}
        {mutation.error && (
          <p className="text-destructive">
            {t("goals:tax_plan.failed", { message: String(mutation.error) })}
          </p>
        )}
        {result && !running && (
          <div className="space-y-3">
            <div>
              <p className="text-2xl font-semibold tabular-nums">
                {t("goals:tax_plan.safe_spending.result", { amount: money(result.spending) })}
              </p>
              <p className="text-muted-foreground">
                {t("goals:tax_plan.safe_spending.result_detail", {
                  rate: Math.round(result.targetRate * 100),
                  paths: result.paths,
                  method: t(`goals:tax_plan.success.run_${result.mode}`, { count: result.paths }),
                })}
              </p>
              <p className="text-muted-foreground">
                {t("goals:tax_plan.safe_spending.planned", {
                  amount: money(result.plannedSpending),
                  rate: Math.round(result.plannedSuccessRate * 100),
                })}
              </p>
            </div>
            {chartData && chartData.length > 1 && (
              <ResponsiveContainer width="100%" height={240}>
                <LineChart data={chartData} margin={{ top: 8, right: 16, bottom: 0, left: 8 }}>
                  <CartesianGrid strokeDasharray="3 3" vertical={false} />
                  <XAxis
                    dataKey="spending"
                    type="number"
                    domain={["dataMin", "dataMax"]}
                    tickFormatter={(value: number) => formatCompactAmount(value, plan.currency)}
                    tickLine={false}
                    axisLine={false}
                    fontSize={11}
                  />
                  <YAxis
                    domain={[0, 100]}
                    tickFormatter={(value: number) => `${value}%`}
                    tickLine={false}
                    axisLine={false}
                    fontSize={11}
                    width={40}
                  />
                  <RTooltip
                    formatter={(value) => `${Number(value)}%`}
                    labelFormatter={(value) =>
                      t("goals:tax_plan.safe_spending.chart_label", {
                        amount: money(Number(value)),
                      })
                    }
                  />
                  <ReferenceLine
                    y={Math.round(result.targetRate * 100)}
                    stroke="var(--muted-foreground)"
                    strokeDasharray="4 4"
                  />
                  <ReferenceLine
                    x={result.plannedSpending}
                    stroke="var(--chart-4)"
                    strokeDasharray="4 4"
                  />
                  <Line
                    type="stepAfter"
                    dataKey="rate"
                    name={t("goals:tax_plan.safe_spending.chart_rate")}
                    stroke="var(--chart-1)"
                    strokeWidth={2}
                    dot={false}
                  />
                </LineChart>
              </ResponsiveContainer>
            )}
            <p className="text-muted-foreground text-xs">
              {t("goals:tax_plan.safe_spending.chart_note")}
            </p>
            {result.lifespans && (
              <p className="text-muted-foreground text-xs">
                {t("goals:tax_plan.success.lifespans", {
                  p10: result.lifespans[0],
                  p50: result.lifespans[1],
                  p90: result.lifespans[2],
                })}
              </p>
            )}
            {result.pathSpending && result.pathSpending.length > 1 && (
              <PathSpendingCharts
                paths={result.pathSpending}
                planned={result.plannedSpending}
                byStartYear={result.mode === "historical"}
                currency={plan.currency}
              />
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

/**
 * The most each market path can spend: how they spread (Owl's histogram) and,
 * on history, which start years would have been hard. Paths below the planned
 * spending are the ones it fails on.
 */
function PathSpendingCharts({
  paths,
  planned,
  byStartYear,
  currency,
}: {
  paths: PathSpending[];
  planned: number;
  byStartYear: boolean;
  currency: string;
}) {
  const { t } = useTranslation();
  const { formatAmount, formatCompactAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(Math.round(value), currency);
  const compact = (value: number) => formatCompactAmount(value, currency);
  const bins = histogramBins(paths.map((p) => p.spending)).map((bin) => ({
    ...bin,
    mid: (bin.from + bin.to) / 2,
  }));
  const short = (bin: { to: number }) => bin.to < planned - 1;

  return (
    <div className="space-y-3 pt-2">
      <p className="font-medium">{t("goals:tax_plan.safe_spending.histogram_title")}</p>
      <ResponsiveContainer width="100%" height={200}>
        <BarChart data={bins} margin={{ top: 8, right: 16, bottom: 0, left: 8 }}>
          <CartesianGrid strokeDasharray="3 3" vertical={false} />
          <XAxis
            dataKey="mid"
            tickFormatter={(value: number) => compact(value)}
            tickLine={false}
            axisLine={false}
            fontSize={11}
          />
          <YAxis allowDecimals={false} tickLine={false} axisLine={false} fontSize={11} width={32} />
          <RTooltip
            formatter={(value) => [value, t("goals:tax_plan.safe_spending.histogram_paths")]}
            labelFormatter={(_, payload) => {
              const bin = payload?.[0]?.payload as { from: number; to: number } | undefined;
              return bin
                ? t("goals:tax_plan.safe_spending.histogram_range", {
                    from: money(bin.from),
                    to: money(bin.to),
                  })
                : "";
            }}
          />
          <Bar dataKey="count" radius={[2, 2, 0, 0]}>
            {bins.map((bin) => (
              <Cell
                key={bin.from}
                fill={short(bin) ? "var(--destructive)" : "var(--chart-1)"}
                fillOpacity={0.8}
              />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
      {byStartYear && (
        <>
          <p className="font-medium">{t("goals:tax_plan.safe_spending.by_year_title")}</p>
          <ResponsiveContainer width="100%" height={220}>
            <BarChart data={paths} margin={{ top: 8, right: 16, bottom: 0, left: 8 }}>
              <CartesianGrid strokeDasharray="3 3" vertical={false} />
              <XAxis
                dataKey="label"
                tickLine={false}
                axisLine={false}
                fontSize={11}
                interval="preserveStartEnd"
                minTickGap={16}
              />
              <YAxis
                tickFormatter={(value: number) => compact(value)}
                tickLine={false}
                axisLine={false}
                fontSize={11}
                width={48}
              />
              <RTooltip
                formatter={(value) => [
                  money(Number(value)),
                  t("goals:tax_plan.safe_spending.by_year_value"),
                ]}
                labelFormatter={(label) =>
                  t("goals:tax_plan.safe_spending.by_year_label", { year: String(label) })
                }
              />
              <ReferenceLine y={planned} stroke="var(--chart-4)" strokeDasharray="4 4" />
              <Bar dataKey="spending">
                {paths.map((path) => (
                  <Cell
                    key={path.label}
                    fill={path.spending < planned - 1 ? "var(--destructive)" : "var(--chart-1)"}
                    fillOpacity={0.8}
                  />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </>
      )}
      <p className="text-muted-foreground text-xs">
        {t("goals:tax_plan.safe_spending.distribution_note", { planned: money(planned) })}
      </p>
    </div>
  );
}
