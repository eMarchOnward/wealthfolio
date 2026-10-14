import { optimizeRetirementTaxPlan } from "@/adapters";
import { useMutation } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { planInputsKey } from "../lib/tax-plan-store";
import type {
  RetirementPlan,
  SavedTaxPlanRun,
  TaxPlanChoiceCosts,
  TaxPlanConversionRegret,
  TaxPlanSafeSpending,
  TaxPlanStore,
  TaxPlanStressTests,
  TaxPlanSuccess,
} from "../types";

/**
 * Runs the tax optimizer on demand: a solve takes seconds to minutes, so it is
 * started by the user rather than on every edit. Each result is saved with the
 * goal, so revisiting the page shows it at once. The result belongs to the
 * inputs it was solved for; editing the plan marks it stale until the next run.
 */
export function useTaxOptimizedPlan(
  goalId: string,
  plan: RetirementPlan,
  store: TaxPlanStore,
  onSaveStore: (store: TaxPlanStore, updated?: RetirementPlan) => void,
) {
  const inputsKey = useMemo(() => planInputsKey(plan), [plan]);
  // The save callbacks read the latest store: a run outlives the render that started it.
  const latest = useRef({ store, onSaveStore });
  useEffect(() => {
    latest.current = { store, onSaveStore };
  }, [store, onSaveStore]);
  const runningKey = useRef<string | null>(null);
  // Shown until the saved copy comes back from the refetch.
  const [unsaved, setUnsaved] = useState<SavedTaxPlanRun | null>(null);

  const mutation = useMutation({
    mutationFn: () => optimizeRetirementTaxPlan(goalId),
    onMutate: () => {
      runningKey.current = inputsKey;
    },
    onSuccess: (result) => {
      const run: SavedTaxPlanRun = {
        inputsKey: runningKey.current ?? "",
        computedAt: new Date().toISOString(),
        result,
      };
      setUnsaved(run);
      const { store: latestStore, onSaveStore: save } = latest.current;
      save({ ...latestStore, lastRun: run });
    },
  });

  // A different goal starts over.
  useEffect(() => {
    setUnsaved(null);
  }, [goalId]);

  // Keeps an analysis of the last result with it.
  const saveWithRun = (patch: Partial<SavedTaxPlanRun>) => {
    const { store: latestStore, onSaveStore: save } = latest.current;
    const lastRun = latestStore.lastRun;
    if (!lastRun) return;
    save({ ...latestStore, lastRun: { ...lastRun, ...patch } });
  };

  const saved = store.lastRun;
  const current =
    unsaved && (!saved || unsaved.computedAt > saved.computedAt) ? unsaved : (saved ?? null);

  return {
    savedRun: current,
    result: current?.result ?? null,
    isStale: current !== null && current.inputsKey !== inputsKey,
    isRunning: mutation.isPending,
    error: mutation.error,
    /** Solves the plan; `onDone` follows a successful run. */
    run: (onDone?: () => void) => mutation.mutate(undefined, { onSuccess: () => onDone?.() }),
    /** Keeps a success-rate check with the result it tested. */
    saveSuccess: (success: TaxPlanSuccess) => {
      const { store: latestStore, onSaveStore: save } = latest.current;
      const lastRun = latestStore.lastRun;
      if (!lastRun) return;
      save({ ...latestStore, lastRun: { ...lastRun, success } });
    },
    /** Keeps a safe-spending search with the result it searched. */
    saveSafeSpending: (safeSpending: TaxPlanSafeSpending) => saveWithRun({ safeSpending }),
    /** Keeps a choice-cost analysis with the result it analysed. */
    saveChoiceCosts: (choiceCosts: TaxPlanChoiceCosts) => saveWithRun({ choiceCosts }),
    /** Keeps stress tests with the result they tested. */
    saveStressTests: (stressTests: TaxPlanStressTests) => saveWithRun({ stressTests }),
    /** Keeps a conversion regret check with the result it checked. */
    saveConversionRegret: (conversionRegret: TaxPlanConversionRegret) =>
      saveWithRun({ conversionRegret }),
  };
}
