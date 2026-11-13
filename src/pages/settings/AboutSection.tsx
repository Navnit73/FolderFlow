import { useEffect, useState } from "react";
import { ErrorState, LoadingState } from "@/components/States";
import { toUiError } from "@/services/result";
import { appService } from "@/services/settings";
import type { AppInfo, UiError } from "@/types/settings";
import { SettingRow } from "./SettingRow";

type State = { kind: "loading" } | { kind: "ready"; info: AppInfo } | { kind: "error"; error: UiError };

function fetchInfo(setState: (s: State) => void): void {
  appService.info().then(
    (info) => setState({ kind: "ready", info }),
    (e: unknown) => setState({ kind: "error", error: toUiError(e) }),
  );
}

function Value({ children }: { children: string }) {
  return <span className="ff-selectable font-mono text-xs text-fg-muted">{children}</span>;
}

export function AboutSection() {
  const [state, setState] = useState<State>({ kind: "loading" });

  useEffect(() => fetchInfo(setState), []);

  const retry = () => {
    setState({ kind: "loading" });
    fetchInfo(setState);
  };

  if (state.kind === "loading") return <LoadingState />;
  if (state.kind === "error") return <ErrorState error={state.error} onRetry={retry} />;
  const { info } = state;

  return (
    <div>
      <SettingRow label="Version">
        <Value>{info.version}</Value>
      </SettingRow>
      <SettingRow label="Database schema" description="Applied migration version.">
        <Value>{String(info.schemaVersion)}</Value>
      </SettingRow>
      <SettingRow label="Data folder" description="Settings, rules and the operation history are stored here.">
        <Value>{info.dataDir}</Value>
      </SettingRow>
      <SettingRow label="Logs folder" description="Diagnostic logs stay on this Mac.">
        <Value>{info.logDir}</Value>
      </SettingRow>
    </div>
  );
}
