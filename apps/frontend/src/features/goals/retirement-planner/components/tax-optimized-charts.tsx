import { useAmountFormatting } from "@wealthfolio/ui";
import { useTranslation } from "react-i18next";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  Cell,
  Legend,
  Line,
  ComposedChart,
  ResponsiveContainer,
  Tooltip as RTooltip,
  XAxis,
  YAxis,
} from "recharts";
import type { RetirementAgeOption, TaxOptimizedYear } from "../types";

const TAX_PLAN_COLORS = {
  wages: "var(--chart-8)",
  socialSecurity: "var(--chart-2)",
  otherIncome: "var(--chart-9)",
  taxable: "var(--chart-3)",
  taxDeferred: "var(--chart-1)",
  roth: "var(--chart-5)",
  hsa: "var(--chart-7)",
  conversion: "var(--chart-4)",
  tax: "hsl(8, 67%, 48%)",
  medicare: "var(--chart-6)",
  spending: "var(--foreground)",
};

const CHART_HEIGHT = 280;

function useAxis(currency: string) {
  const formatting = useAmountFormatting();
  return {
    tick: (value: number) => formatting.formatCompactAmount(value, currency),
    tooltip: (value: number) => formatting.formatAmount(value, currency),
  };
}

/** Where each year's spending money comes from, with the spending line on top. */
export function TaxPlanSourcesChart({
  years,
  currency,
}: {
  years: TaxOptimizedYear[];
  currency: string;
}) {
  const { t } = useTranslation();
  const axis = useAxis(currency);
  return (
    <ResponsiveContainer width="100%" height={CHART_HEIGHT}>
      <ComposedChart data={years} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
        <XAxis dataKey="age" tickLine={false} axisLine={false} fontSize={11} />
        <YAxis
          tickFormatter={axis.tick}
          tickLine={false}
          axisLine={false}
          fontSize={11}
          width={56}
        />
        <RTooltip
          formatter={(value) => axis.tooltip(Number(value))}
          labelFormatter={(age) => t("goals:tax_plan.chart.age_label", { age })}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Bar
          dataKey="wages"
          stackId="sources"
          name={t("goals:tax_plan.series.wages")}
          fill={TAX_PLAN_COLORS.wages}
        />
        <Bar
          dataKey="socialSecurity"
          stackId="sources"
          name={t("goals:tax_plan.series.social_security")}
          fill={TAX_PLAN_COLORS.socialSecurity}
        />
        <Bar
          dataKey="otherIncome"
          stackId="sources"
          name={t("goals:tax_plan.series.other_income")}
          fill={TAX_PLAN_COLORS.otherIncome}
        />
        <Bar
          dataKey="taxableWithdrawal"
          stackId="sources"
          name={t("goals:tax_plan.series.taxable")}
          fill={TAX_PLAN_COLORS.taxable}
        />
        <Bar
          dataKey="taxDeferredWithdrawal"
          stackId="sources"
          name={t("goals:tax_plan.series.tax_deferred")}
          fill={TAX_PLAN_COLORS.taxDeferred}
        />
        <Bar
          dataKey="rothWithdrawal"
          stackId="sources"
          name={t("goals:tax_plan.series.roth")}
          fill={TAX_PLAN_COLORS.roth}
        />
        <Bar
          dataKey="hsaWithdrawal"
          stackId="sources"
          name={t("goals:tax_plan.series.hsa")}
          fill={TAX_PLAN_COLORS.hsa}
        />
        <Line
          dataKey="netSpending"
          name={t("goals:tax_plan.series.spending")}
          stroke={TAX_PLAN_COLORS.spending}
          dot={false}
          strokeWidth={2}
          type="stepAfter"
        />
      </ComposedChart>
    </ResponsiveContainer>
  );
}

/** Start-of-year balances by tax treatment. */
export function TaxPlanBalancesChart({
  years,
  currency,
}: {
  years: TaxOptimizedYear[];
  currency: string;
}) {
  const { t } = useTranslation();
  const axis = useAxis(currency);
  return (
    <ResponsiveContainer width="100%" height={CHART_HEIGHT}>
      <AreaChart data={years} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
        <XAxis dataKey="age" tickLine={false} axisLine={false} fontSize={11} />
        <YAxis
          tickFormatter={axis.tick}
          tickLine={false}
          axisLine={false}
          fontSize={11}
          width={56}
        />
        <RTooltip
          formatter={(value) => axis.tooltip(Number(value))}
          labelFormatter={(age) => t("goals:tax_plan.chart.age_label", { age })}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Area
          dataKey="taxableBalance"
          stackId="balances"
          name={t("goals:tax_plan.series.taxable")}
          stroke={TAX_PLAN_COLORS.taxable}
          fill={TAX_PLAN_COLORS.taxable}
          fillOpacity={0.6}
        />
        <Area
          dataKey="taxDeferredBalance"
          stackId="balances"
          name={t("goals:tax_plan.series.tax_deferred")}
          stroke={TAX_PLAN_COLORS.taxDeferred}
          fill={TAX_PLAN_COLORS.taxDeferred}
          fillOpacity={0.6}
        />
        <Area
          dataKey="rothBalance"
          stackId="balances"
          name={t("goals:tax_plan.series.roth")}
          stroke={TAX_PLAN_COLORS.roth}
          fill={TAX_PLAN_COLORS.roth}
          fillOpacity={0.6}
        />
        <Area
          dataKey="hsaBalance"
          stackId="balances"
          name={t("goals:tax_plan.series.hsa")}
          stroke={TAX_PLAN_COLORS.hsa}
          fill={TAX_PLAN_COLORS.hsa}
          fillOpacity={0.6}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}

/** Roth conversions next to the taxes and premiums they trade against. */
export function TaxPlanConversionsChart({
  years,
  currency,
}: {
  years: TaxOptimizedYear[];
  currency: string;
}) {
  const { t } = useTranslation();
  const axis = useAxis(currency);
  const data = years.map((y) => ({
    age: y.age,
    rothConversion: y.rothConversion,
    taxes: y.federalIncomeTax + y.capitalGainsTax + y.stateIncomeTax + y.earlyWithdrawalPenalty,
    medicare: y.medicare + y.acaPremium,
  }));
  return (
    <ResponsiveContainer width="100%" height={CHART_HEIGHT}>
      <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
        <XAxis dataKey="age" tickLine={false} axisLine={false} fontSize={11} />
        <YAxis
          tickFormatter={axis.tick}
          tickLine={false}
          axisLine={false}
          fontSize={11}
          width={56}
        />
        <RTooltip
          formatter={(value) => axis.tooltip(Number(value))}
          labelFormatter={(age) => t("goals:tax_plan.chart.age_label", { age })}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Bar
          dataKey="rothConversion"
          name={t("goals:tax_plan.series.roth_conversion")}
          fill={TAX_PLAN_COLORS.conversion}
        />
        <Bar
          dataKey="taxes"
          stackId="costs"
          name={t("goals:tax_plan.series.taxes")}
          fill={TAX_PLAN_COLORS.tax}
        />
        <Bar
          dataKey="medicare"
          stackId="costs"
          name={t("goals:tax_plan.series.medicare")}
          fill={TAX_PLAN_COLORS.medicare}
        />
      </BarChart>
    </ResponsiveContainer>
  );
}

/**
 * The most each retirement age lets you spend per year once retired, against
 * the must-have expenses; ages that cannot cover them have no bar.
 */
export function RetirementAgeChart({
  options,
  earliestAge,
  currency,
}: {
  options: RetirementAgeOption[];
  earliestAge: number | null;
  currency: string;
}) {
  const { t } = useTranslation();
  const axis = useAxis(currency);
  return (
    <ResponsiveContainer width="100%" height={CHART_HEIGHT}>
      <ComposedChart data={options} margin={{ top: 8, right: 8, bottom: 0, left: 8 }}>
        <XAxis dataKey="age" tickLine={false} axisLine={false} fontSize={11} />
        <YAxis
          tickFormatter={axis.tick}
          tickLine={false}
          axisLine={false}
          fontSize={11}
          width={56}
        />
        <RTooltip
          formatter={(value) => axis.tooltip(Number(value))}
          labelFormatter={(age) => t("goals:tax_plan.age_search.retire_at", { age })}
        />
        <Legend wrapperStyle={{ fontSize: 11 }} />
        <Bar
          dataKey="retirementSpending"
          name={t("goals:tax_plan.age_search.spending")}
          fill={TAX_PLAN_COLORS.socialSecurity}
        >
          {options.map((option) => (
            <Cell
              key={option.age}
              fill={
                option.age === earliestAge ? TAX_PLAN_COLORS.roth : TAX_PLAN_COLORS.socialSecurity
              }
            />
          ))}
        </Bar>
        <Line
          dataKey="requiredSpending"
          name={t("goals:tax_plan.age_search.required")}
          stroke={TAX_PLAN_COLORS.tax}
          strokeDasharray="4 3"
          strokeWidth={2}
          dot={false}
          type="stepAfter"
        />
      </ComposedChart>
    </ResponsiveContainer>
  );
}
