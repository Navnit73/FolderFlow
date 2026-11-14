import { commands } from "@/bindings";
import type {
  ActivityFilter_Deserialize,
  ActivityPage_Serialize,
  OpKind,
  OperationView_Serialize,
  OpStatus,
  PageRequest,
  RevealTarget,
  ReviewDecision,
  ReviewItemView_Serialize,
  ReviewResolveReport_Serialize,
  UndoConflict,
  UndoReport_Serialize,
  UndoTarget_Deserialize,
} from "@/bindings";
import { unwrap } from "./result";

export type ActivityFilter = ActivityFilter_Deserialize;
export type ActivityPage = ActivityPage_Serialize;
export type OperationView = OperationView_Serialize;
export type ReviewItemView = ReviewItemView_Serialize;
export type ReviewResolveReport = ReviewResolveReport_Serialize;
export type UndoReport = UndoReport_Serialize;
export type UndoTarget = UndoTarget_Deserialize;
export type { OpKind, OpStatus, PageRequest, RevealTarget, ReviewDecision, UndoConflict };

export const activityService = {
  list: (filter: ActivityFilter, page: PageRequest): Promise<ActivityPage> =>
    unwrap(commands.activityList(filter, page)),
  undo: (target: UndoTarget): Promise<UndoReport> => unwrap(commands.activityUndo(target)),
  retry: (opId: string): Promise<OperationView> => unwrap(commands.activityRetry(opId)),
  exportCsv: (filter: ActivityFilter): Promise<string> => unwrap(commands.activityExportCsv(filter)),
  clear: (beforeMs: number | null = null): Promise<number> => unwrap(commands.activityClear(beforeMs)),
  revealInFinder: (opId: string, which: RevealTarget): Promise<boolean> =>
    unwrap(commands.revealInFinder(opId, which)),
  reviewList: (): Promise<ReviewItemView[]> => unwrap(commands.reviewList()),
  reviewResolve: (id: string, decision: ReviewDecision): Promise<ReviewResolveReport> =>
    unwrap(commands.reviewResolve(id, decision)),
};

