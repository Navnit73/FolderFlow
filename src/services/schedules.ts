import { commands } from "@/bindings";
import type {
  CatchUpPolicy,
  QuietHours,
  ScheduleConfig_Deserialize,
  ScheduleInput_Deserialize,
  ScheduleRunOutcome_Serialize,
  ScheduleRunStatus,
  ScheduleType,
  Schedule_Serialize,
  Weekday,
} from "@/bindings";
import { unwrap } from "./result";

export type Schedule = Schedule_Serialize;
export type ScheduleInput = ScheduleInput_Deserialize;
export type ScheduleConfig = ScheduleConfig_Deserialize;
export type ScheduleRunOutcome = ScheduleRunOutcome_Serialize;
export type { CatchUpPolicy, QuietHours, ScheduleRunStatus, ScheduleType, Weekday };

export const scheduleService = {
  list: (): Promise<Schedule[]> => unwrap(commands.schedulesList()),

  upsert: (input: ScheduleInput): Promise<Schedule> =>
    unwrap(commands.schedulesUpsert(input)),

  remove: (id: string): Promise<void> =>
    unwrap(commands.schedulesDelete(id)).then(() => undefined),

  setEnabled: (id: string, enabled: boolean): Promise<Schedule> =>
    unwrap(commands.schedulesSetEnabled(id, enabled)),

  runNow: (id: string): Promise<ScheduleRunOutcome> =>
    unwrap(commands.schedulesRunNow(id)),
};

