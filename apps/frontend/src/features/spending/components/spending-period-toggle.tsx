/**
 * Canonical period toggle for spending surfaces that need named comparison
 * periods (This month / Last month / 3M / 6M / YTD / 1Y). Used by the Insights page
 * and any future page that compares period-over-period totals.
 *
 * Why this exists: prior to this primitive each surface inlined its own
 * `AnimatedToggleGroup` config with subtly different labels/items. This file
 * is the single source of truth for the canonical insights period set.
 *
 * Not used by:
 * - The Budget page (uses `MonthSwitcher` — budgets are inherently
 *   per-month with rollover, no useful "1Y" view).
 *
 * Dashboard links use shared period-preference keys so the most recent explicit
 * dashboard or insights period selection wins when moving between surfaces.
 */
import { useMemo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import type { DateRange } from "@/lib/types";
import { cn } from "@/lib/utils";

import { AnimatedToggleGroup, useIsMobile } from "@wealthfolio/ui";

import { SpendingDatePicker } from "./spending-date-picker";
import { REPORTS_PERIODS, type ReportsPeriod } from "../lib/reports-period";

/** Reports periods plus the dashboard-only ten-year view. */
export type SpendingPeriodCode = ReportsPeriod | "10Y";

function useSpendingPeriodLabels(): Record<SpendingPeriodCode, ReactNode> {
  const { t } = useTranslation();
  return useMemo(
    () => ({
      MTD: (
        <>
          <span className="hidden sm:inline">{t("spending:period.thisMonth")}</span>
          <span className="sm:hidden">{t("spending:period.mtdShort")}</span>
        </>
      ),
      LAST_MONTH: (
        <>
          <span className="hidden sm:inline">{t("spending:period.lastMonth")}</span>
          <span className="sm:hidden">{t("spending:period.prevShort")}</span>
        </>
      ),
      "3M": "3M",
      "6M": "6M",
      YTD: "YTD",
      "1Y": "1Y",
      "10Y": "10Y",
    }),
    [t],
  );
}

interface SpendingPeriodToggleProps<P extends SpendingPeriodCode> {
  value: P | null;
  onValueChange: (next: P) => void;
  /** Periods offered; defaults to the reports periods. */
  periods?: readonly P[];
  /** Visual variant on `AnimatedToggleGroup`. Default mirrors prior call sites. */
  variant?: "default" | "secondary";
  /** Size pass-through. Default "xs" matches the prior insights placement. */
  size?: "compact" | "xs" | "sm" | "md";
  className?: string;
}

export function SpendingPeriodToggle<P extends SpendingPeriodCode = ReportsPeriod>({
  value,
  onValueChange,
  periods = REPORTS_PERIODS as readonly SpendingPeriodCode[] as readonly P[],
  variant = "secondary",
  size = "xs",
  className,
}: SpendingPeriodToggleProps<P>) {
  const labels = useSpendingPeriodLabels();
  return (
    <AnimatedToggleGroup<P>
      variant={variant}
      size={size}
      items={periods.map((p) => ({ value: p, label: labels[p] }))}
      value={value}
      onValueChange={onValueChange}
      className={className}
    />
  );
}

interface SpendingPeriodSelectorProps<P extends SpendingPeriodCode> {
  value: P | null;
  onValueChange: (next: P) => void;
  periods?: readonly P[];
  customMonth: string | null;
  customRange?: DateRange;
  maxMonth: string;
  onCustomMonthChange: (monthKey: string | null) => void;
  onCustomRangeChange?: (range: DateRange | undefined) => void;
  isLoading?: boolean;
  className?: string;
}

export function SpendingPeriodSelector<P extends SpendingPeriodCode = ReportsPeriod>({
  value,
  onValueChange,
  periods,
  customMonth,
  customRange,
  maxMonth,
  onCustomMonthChange,
  onCustomRangeChange,
  isLoading,
  className,
}: SpendingPeriodSelectorProps<P>) {
  const isMobile = useIsMobile();

  return (
    <div
      className={cn(
        "flex w-full min-w-0 max-w-full items-center justify-center gap-1.5",
        className,
      )}
      aria-busy={isLoading ? "true" : undefined}
    >
      <SpendingPeriodToggle
        value={value}
        onValueChange={onValueChange}
        periods={periods}
        size={isMobile ? "compact" : "sm"}
        variant="default"
        className="min-w-0 bg-transparent"
      />
      <SpendingDatePicker
        customMonth={customMonth}
        customRange={customRange}
        maxMonth={maxMonth}
        onCustomMonthChange={onCustomMonthChange}
        onCustomRangeChange={onCustomRangeChange}
      />
    </div>
  );
}
