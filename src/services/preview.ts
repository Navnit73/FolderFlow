import { commands } from "@/bindings";
import type {
  ConflictPolicy,
  PlanCategoryCount,
  PlanConflict,
  PlanExecutionReport_Serialize,
  PlanItem,
  PlanItemFilter_Deserialize,
  PlanItemsPage,
  PlanItemStatus,
  PlanOpKind,
  PlanRuleCount,
  PlanScope_Deserialize,
  PlanSummary,
  SkippedPlanItem,
} from "@/bindings";
import { unwrap } from "./result";

export type PlanScope = PlanScope_Deserialize;
export type PlanItemFilter = PlanItemFilter_Deserialize;
export type PlanExecutionReport = PlanExecutionReport_Serialize;
export type {
  ConflictPolicy,
  PlanCategoryCount,
  PlanConflict,
  PlanItem,
  PlanItemsPage,
  PlanItemStatus,
  PlanOpKind,
  PlanRuleCount,
  PlanSummary,
  SkippedPlanItem,
};

export const previewService = {
  create: (scope: PlanScope = {}): Promise<PlanSummary> =>
    unwrap(
      commands.planCreate({
        folderId: scope.folderId ?? null,
        ruleId: scope.ruleId ?? null,
        filePaths: scope.filePaths ?? null,
      }),
    ),

  items: (
    planId: string,
    filter: PlanItemFilter = {},
    limit = 100,
    offset = 0,
  ): Promise<PlanItemsPage> =>
    unwrap(
      commands.planItems(
        planId,
        {
          search: filter.search ?? null,
          category: filter.category ?? null,
          ruleId: filter.ruleId ?? null,
          onlyConflicts: filter.onlyConflicts ?? null,
        },
        limit,
        offset,
      ),
    ),

  execute: (
    planId: string,
    selectedItemIds: string[] | null,
    conflictPolicy: ConflictPolicy,
  ): Promise<PlanExecutionReport> =>
    unwrap(commands.planExecute(planId, selectedItemIds, conflictPolicy)),

  discard: (planId: string): Promise<boolean> =>
    unwrap(commands.planDiscard(planId)),
};

