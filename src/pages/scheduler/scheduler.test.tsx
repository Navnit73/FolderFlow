import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import {
  commands,
  folderFixture,
  resetBindingMocks,
  ruleFixture,
  scheduleFixture,
} from "@/test/mockBindings";
import { useSchedules } from "@/stores/schedules";
import { SchedulerPage } from "./SchedulerPage";

vi.mock("@/bindings", () => import("@/test/mockBindings"));

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function renderScheduler() {
  return render(
    <MemoryRouter>
      <SchedulerPage />
    </MemoryRouter>,
  );
}

describe("SchedulerPage (UI-SCHED-01..02)", () => {
  beforeEach(() => {
    resetBindingMocks();
    useSchedules.getState().reset();
    commands.foldersList.mockResolvedValue(ok([folderFixture({ id: "f1", name: "Downloads" })]));
    commands.rulesList.mockResolvedValue(ok([ruleFixture({ id: "r1", name: "Organize Invoices" })]));
  });

  it("UI-SCHED-01: lists schedules, toggles enabled, runs now, and deletes with confirmation", async () => {
    const sched = scheduleFixture({
      id: "s1",
      folderId: "f1",
      ruleId: "r1",
      scheduleType: "daily",
      enabled: true,
      config: {
        intervalSecs: null,
        time: "18:30",
        weekdays: [],
        quietHours: { start: "22:00", end: "07:00" },
        catchUp: "run_once",
      },
    });
    commands.schedulesList.mockResolvedValue(ok([sched]));
    commands.schedulesSetEnabled.mockResolvedValue(ok({ ...sched, enabled: false }));
    commands.schedulesRunNow.mockResolvedValue(
      ok({ scheduleId: "s1", status: "no_matching_files", report: null }),
    );

    renderScheduler();

    expect(await screen.findByText("Downloads · Organize Invoices")).toBeInTheDocument();
    expect(screen.getByText(/Daily at 18:30/i)).toBeInTheDocument();
    expect(screen.getByText(/Quiet 22:00–07:00/i)).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(screen.getByRole("switch", { name: /Toggle schedule/i }));
    await waitFor(() => {
      expect(commands.schedulesSetEnabled).toHaveBeenCalledWith("s1", false);
    });

    await user.click(screen.getByRole("button", { name: /Run now/i }));
    await waitFor(() => {
      expect(commands.schedulesRunNow).toHaveBeenCalledWith("s1");
    });
    expect(await screen.findByText(/No matching files/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Delete schedule/i }));
    await user.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => {
      expect(commands.schedulesDelete).toHaveBeenCalledWith("s1");
    });
  });

  it("UI-SCHED-02: creates interval and weekly schedules with quiet hours and catch-up policy", async () => {
    commands.schedulesList.mockResolvedValue(ok([]));

    renderScheduler();
    expect(await screen.findByText("No schedules configured")).toBeInTheDocument();

    const user = userEvent.setup();
    const newScheduleBtn = screen.getAllByRole("button", { name: /New schedule/i })[0];
    if (!newScheduleBtn) throw new Error("Expected New schedule button");
    await user.click(newScheduleBtn);

    await user.selectOptions(screen.getByLabelText("Target folder"), "f1");
    await user.selectOptions(screen.getByLabelText("Schedule type"), "interval");
    await user.click(screen.getByRole("button", { name: "Every 30 min" }));

    await user.selectOptions(screen.getByLabelText("Schedule type"), "weekly");
    await user.clear(screen.getByLabelText("Run time"));
    await user.type(screen.getByLabelText("Run time"), "09:15");

    // Select Mon and Fri weekdays
    await user.click(screen.getByRole("button", { name: "Mon" }));
    await user.click(screen.getByRole("button", { name: "Fri" }));

    // Enable quiet hours
    await user.click(screen.getByLabelText("Enable quiet hours"));
    await user.clear(screen.getByLabelText("Quiet start"));
    await user.type(screen.getByLabelText("Quiet start"), "23:00");
    await user.clear(screen.getByLabelText("Quiet end"));
    await user.type(screen.getByLabelText("Quiet end"), "06:30");

    await user.selectOptions(screen.getByLabelText("Missed run policy"), "skip");

    await user.click(screen.getByRole("button", { name: "Save schedule" }));

    await waitFor(() => {
      expect(commands.schedulesUpsert).toHaveBeenCalledWith({
        id: null,
        folderId: "f1",
        ruleId: null,
        scheduleType: "weekly",
        enabled: true,
        config: {
          intervalSecs: null,
          time: "09:15",
          weekdays: ["mon", "fri"],
          quietHours: { start: "23:00", end: "06:30" },
          catchUp: "skip",
        },
      });
    });

    // Also verify editing an existing schedule submits its id cleanly.
    const created = scheduleFixture({
      id: "sched-edit-1",
      folderId: "f1",
      ruleId: null,
      scheduleType: "daily",
      enabled: true,
      config: {
        intervalSecs: null,
        time: "09:15",
        weekdays: [],
        quietHours: null,
        catchUp: "skip",
      },
    });
    commands.schedulesList.mockResolvedValue(ok([created]));
    useSchedules.getState().reset();
    await useSchedules.getState().load();

    await user.click(await screen.findByRole("button", { name: /Edit schedule for Downloads/i }));
    await user.clear(screen.getByLabelText("Run time"));
    await user.type(screen.getByLabelText("Run time"), "10:45");
    await user.click(screen.getByRole("button", { name: "Save schedule" }));

    await waitFor(() => {
      expect(commands.schedulesUpsert).toHaveBeenLastCalledWith(
        expect.objectContaining({
          id: "sched-edit-1",
          config: expect.objectContaining({ time: "10:45" }),
        }),
      );
    });
  });
});
