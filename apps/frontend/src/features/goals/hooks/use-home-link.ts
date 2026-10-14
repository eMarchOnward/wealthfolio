import { getGoalPlan, saveGoalPlan } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { GoalPlan } from "@/lib/types";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { linkedHomeGoalId, settingsWithLinkedHome } from "../save-up/lib/home-link";
import { useGoals } from "./use-goals";

/** The retirement goal, its plan, the home goal it includes, and a way to change that. */
export function useRetirementHomeLink() {
  const queryClient = useQueryClient();
  const { goals } = useGoals();
  const retirementGoal = goals.find(
    (goal) => goal.goalType === "retirement" && goal.statusLifecycle !== "archived",
  );
  const retirementId = retirementGoal?.id ?? "";
  const { data: retirementPlan, isLoading } = useQuery<GoalPlan | null, Error>({
    queryKey: QueryKeys.goalPlan(retirementId),
    queryFn: () => getGoalPlan(retirementId),
    enabled: !!retirementGoal,
  });

  const link = useMutation({
    mutationFn: async (homeGoalId: string | null) => {
      const plan = await getGoalPlan(retirementId);
      if (!retirementGoal || !plan) throw new Error("Set up the retirement goal's plan first.");
      return saveGoalPlan({
        goalId: retirementId,
        planKind: "retirement",
        plannerMode: plan.plannerMode,
        settingsJson: settingsWithLinkedHome(plan, homeGoalId),
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: QueryKeys.goalPlan(retirementId) });
      queryClient.invalidateQueries({ queryKey: QueryKeys.retirementOverview(retirementId) });
      queryClient.invalidateQueries({ queryKey: [QueryKeys.GOALS] });
    },
    onError: (error) =>
      toast.error(error instanceof Error ? error.message : "Failed to update the retirement plan."),
  });

  return {
    retirementGoal,
    retirementPlan: retirementPlan ?? null,
    linkedHomeId: linkedHomeGoalId(retirementPlan),
    isLoading,
    link,
  };
}
