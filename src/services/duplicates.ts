import {
  commands,
  type DupActionReport_Serialize,
  type DupAction_Deserialize,
  type DupExportFormat,
  type DuplicateGroupSummary_Serialize,
  type DuplicateGroupsResponse_Serialize,
  type DuplicateMemberView_Serialize,
  type DuplicateScanOptions_Deserialize,
  type DuplicateScanReport_Serialize,
  type DuplicateSummaryTotals,
} from "@/bindings";
import { unwrap } from "./result";

export type DuplicateGroupSummary = DuplicateGroupSummary_Serialize;
export type DuplicateGroupsResponse = DuplicateGroupsResponse_Serialize;
export type DuplicateMemberView = DuplicateMemberView_Serialize;
export type DuplicateScanOptions = DuplicateScanOptions_Deserialize;
export type DuplicateScanReport = DuplicateScanReport_Serialize;
export type DupAction = DupAction_Deserialize;
export type DupActionReport = DupActionReport_Serialize;
export type { DupExportFormat, DuplicateSummaryTotals };

export const duplicatesService = {
  scan: (opts: DuplicateScanOptions = {}): Promise<DuplicateScanReport> => unwrap(commands.dupScan(opts)),
  listGroups: (includeIgnored = false, search: string | null = null): Promise<DuplicateGroupsResponse> =>
    unwrap(commands.dupGroups(includeIgnored, search)),
  groupDetails: (groupId: string): Promise<DuplicateGroupSummary> => unwrap(commands.dupGroupDetails(groupId)),
  applyAction: (action: DupAction): Promise<DupActionReport> => unwrap(commands.dupAction(action)),
  exportReport: (format: DupExportFormat, includeIgnored = false): Promise<string> =>
    unwrap(commands.dupExport(format, includeIgnored)),
  revealFile: (fileRecordId: string): Promise<boolean> => unwrap(commands.dupRevealFile(fileRecordId)),
  pickReviewFolder: (): Promise<string | null> => unwrap(commands.dupPickReviewFolder()),
};
