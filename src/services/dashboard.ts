import { commands, type DashboardStats_Serialize, type DbMaintenanceReport } from "@/bindings";
import { unwrap } from "./result";

export type DashboardStats = DashboardStats_Serialize;
export type { DbMaintenanceReport };

export const dashboardService = {
  getStats: (): Promise<DashboardStats> => unwrap(commands.dashboardStats()),
  runMaintenance: (): Promise<DbMaintenanceReport> => unwrap(commands.dbMaintenance()),
};

