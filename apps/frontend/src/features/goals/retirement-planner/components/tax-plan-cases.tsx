import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  Button,
  Card,
  CardContent,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@wealthfolio/ui";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@wealthfolio/ui/components/ui/dialog";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { caseMatchingPlan } from "../lib/tax-plan-store";
import type { RetirementPlan, SavedTaxPlanRun, TaxPlanCase, TaxPlanStore } from "../types";

const CURRENT = "current";

function sameName(a: string, b: string) {
  return a.trim().toLocaleLowerCase() === b.trim().toLocaleLowerCase();
}

/**
 * Picks between the goal's own plan and named cases ("Retire at 64"): saved
 * inputs with their result, viewable at once and loadable back into the plan.
 */
export function TaxPlanCaseBar({
  plan,
  store,
  freshRun,
  viewingId,
  onView,
  onSaveStore,
  onCompare,
}: {
  plan: RetirementPlan;
  store: TaxPlanStore;
  /** The current plan's result when it matches the current inputs. */
  freshRun: SavedTaxPlanRun | null;
  viewingId: string | null;
  onView: (caseId: string | null) => void;
  onSaveStore: (store: TaxPlanStore, updated?: RetirementPlan) => void;
  /** Opens the side-by-side comparison; absent when there is nothing to compare. */
  onCompare?: () => void;
}) {
  const { t } = useTranslation();
  const viewing = store.cases.find((c) => c.id === viewingId) ?? null;
  const [naming, setNaming] = useState<"save" | "rename" | null>(null);
  const [name, setName] = useState("");
  const [confirm, setConfirm] = useState<"load" | "delete" | null>(null);

  const clash =
    naming !== null &&
    store.cases.some((c) => sameName(c.name, name) && (naming === "save" || c.id !== viewing?.id));

  const openSave = () => {
    const age = plan.personal.targetRetirementAge;
    setName(
      caseMatchingPlan(store.cases, plan)?.name ??
        (plan.taxOptimizer?.goal === "earliestAge"
          ? t("goals:tax_plan.cases.default_name_earliest")
          : t("goals:tax_plan.cases.default_name", { age })),
    );
    setNaming("save");
  };

  const submitName = () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    if (naming === "rename" && viewing) {
      onSaveStore({
        ...store,
        cases: store.cases.map((c) => (c.id === viewing.id ? { ...c, name: trimmed } : c)),
      });
    } else {
      const replaced = store.cases.find((c) => sameName(c.name, trimmed));
      const saved: TaxPlanCase = {
        id: replaced?.id ?? crypto.randomUUID(),
        name: trimmed,
        savedAt: new Date().toISOString(),
        plan,
        run: freshRun ?? undefined,
      };
      onSaveStore({
        ...store,
        cases: replaced
          ? store.cases.map((c) => (c.id === replaced.id ? saved : c))
          : [...store.cases, saved],
      });
      toast.success(t("goals:tax_plan.cases.saved", { name: trimmed }));
    }
    setNaming(null);
  };

  const load = () => {
    if (!viewing) return;
    onSaveStore({ ...store, lastRun: viewing.run }, viewing.plan);
    onView(null);
    setConfirm(null);
  };

  const remove = () => {
    if (!viewing) return;
    onSaveStore({ ...store, cases: store.cases.filter((c) => c.id !== viewing.id) });
    onView(null);
    setConfirm(null);
  };

  // Loading replaces the current inputs; warn when they are not kept in any case.
  const currentUnsaved = !caseMatchingPlan(store.cases, plan);

  return (
    <Card>
      <CardContent className="flex flex-col gap-3 py-3 sm:flex-row sm:items-center">
        <div className="flex items-center gap-2">
          <Label className="text-muted-foreground shrink-0 text-sm">
            {t("goals:tax_plan.cases.label")}
          </Label>
          <Select
            value={viewing ? viewing.id : CURRENT}
            onValueChange={(value) => onView(value === CURRENT ? null : value)}
          >
            <SelectTrigger className="w-60">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={CURRENT}>{t("goals:tax_plan.cases.current")}</SelectItem>
              {store.cases.map((c) => (
                <SelectItem key={c.id} value={c.id}>
                  {c.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex flex-wrap gap-2 sm:ml-auto">
          {onCompare && (
            <Button size="sm" variant="outline" onClick={onCompare}>
              <Icons.BarChart className="mr-2 h-4 w-4" />
              {t("goals:tax_plan.compare.open")}
            </Button>
          )}
          {viewing ? (
            <>
              <Button size="sm" onClick={() => (currentUnsaved ? setConfirm("load") : load())}>
                <Icons.Download className="mr-2 h-4 w-4" />
                {t("goals:tax_plan.cases.load")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setName(viewing.name);
                  setNaming("rename");
                }}
              >
                {t("goals:tax_plan.cases.rename")}
              </Button>
              <Button size="sm" variant="outline" onClick={() => setConfirm("delete")}>
                {t("goals:tax_plan.cases.delete")}
              </Button>
            </>
          ) : (
            <Button size="sm" variant="outline" onClick={openSave}>
              <Icons.Save className="mr-2 h-4 w-4" />
              {t("goals:tax_plan.cases.save_as")}
            </Button>
          )}
        </div>
      </CardContent>

      <Dialog open={naming !== null} onOpenChange={(open) => !open && setNaming(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {naming === "rename"
                ? t("goals:tax_plan.cases.rename_title")
                : t("goals:tax_plan.cases.save_title")}
            </DialogTitle>
            <DialogDescription>
              {naming === "rename"
                ? t("goals:tax_plan.cases.rename_description")
                : freshRun
                  ? t("goals:tax_plan.cases.save_description")
                  : t("goals:tax_plan.cases.save_description_no_result")}
            </DialogDescription>
          </DialogHeader>
          <form
            className="space-y-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (!(naming === "rename" && clash)) submitName();
            }}
          >
            <Label htmlFor="tax-plan-case-name">{t("goals:tax_plan.cases.name")}</Label>
            <Input
              id="tax-plan-case-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoFocus
            />
            {clash && (
              <p className="text-muted-foreground text-xs">
                {naming === "rename"
                  ? t("goals:tax_plan.cases.name_taken")
                  : t("goals:tax_plan.cases.will_replace")}
              </p>
            )}
            <DialogFooter className="pt-2">
              <Button type="button" variant="outline" onClick={() => setNaming(null)}>
                {t("goals:tax_plan.cases.cancel")}
              </Button>
              <Button type="submit" disabled={!name.trim() || (naming === "rename" && clash)}>
                {naming === "rename"
                  ? t("goals:tax_plan.cases.rename")
                  : t("goals:tax_plan.cases.save")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog open={confirm !== null} onOpenChange={(open) => !open && setConfirm(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {confirm === "delete"
                ? t("goals:tax_plan.cases.delete_title", { name: viewing?.name })
                : t("goals:tax_plan.cases.load_title", { name: viewing?.name })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {confirm === "delete"
                ? t("goals:tax_plan.cases.delete_description")
                : t("goals:tax_plan.cases.load_description")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("goals:tax_plan.cases.cancel")}</AlertDialogCancel>
            <AlertDialogAction onClick={confirm === "delete" ? remove : load}>
              {confirm === "delete"
                ? t("goals:tax_plan.cases.delete")
                : t("goals:tax_plan.cases.load")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}
