import { ExternalLink, RotateCcw } from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { ErrorState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { loginItemService, type LoginItemStatus } from "@/services/license";
import { useOnboarding } from "@/stores/onboarding";
import { useSettings } from "@/stores/settings";
import { SettingRow } from "./SettingRow";

function loginItemHint(status: LoginItemStatus | null): string | null {
  switch (status) {
    case "enabled":
      return "Registered with macOS Login Items.";
    case "requires_approval":
      return "Pending approval in System Settings → General → Login Items.";
    case "not_found":
      return "Available when running from the packaged FolderFlow.app bundle.";
    case "unsupported":
      return "Login item registration is unsupported on this macOS version.";
    default:
      return null;
  }
}

export function GeneralSection() {
  const navigate = useNavigate();
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);
  const restartOnboarding = useOnboarding((s) => s.restartOnboarding);
  const [loginStatus, setLoginStatus] = useState<LoginItemStatus | null>(null);

  useEffect(() => {
    let active = true;
    void loginItemService
      .status()
      .then((st) => {
        if (active) setLoginStatus(st);
      })
      .catch(() => {
        // Ignore non-fatal login item status query failure.
      });
    return () => {
      active = false;
    };
  }, [settings?.general.launchAtLogin]);

  const handleRerun = async () => {
    const resetState = await restartOnboarding();
    if (resetState) {
      void navigate("/onboarding/1");
    }
  };

  const hint = loginItemHint(loginStatus);

  return (
    <div className="space-y-3">
      {saveError && <ErrorState error={saveError} />}
      {settings && (
        <div>
          <SettingRow
            label="Launch at login"
            description={
              hint
                ? `Automatically start FolderFlow when you sign in to macOS. ${hint}`
                : "Automatically start FolderFlow when you sign in to macOS."
            }
          >
            <div className="flex items-center gap-2">
              {loginStatus === "requires_approval" && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => void loginItemService.openSettings()}
                >
                  <ExternalLink aria-hidden />
                  Open Login Items…
                </Button>
              )}
              <Switch
                id="launch-at-login"
                checked={settings.general.launchAtLogin}
                onChange={(checked) =>
                  void update((cur) => ({
                    ...cur,
                    general: { ...cur.general, launchAtLogin: checked },
                  }))
                }
              />
            </div>
          </SettingRow>
          <SettingRow
            label="Start monitoring automatically"
            description="Begin watching enabled folders as soon as FolderFlow launches."
          >
            <Switch
              id="start-monitoring-auto"
              checked={settings.general.startMonitoringAutomatically}
              onChange={(checked) =>
                void update((cur) => ({
                  ...cur,
                  general: { ...cur.general, startMonitoringAutomatically: checked },
                }))
              }
            />
          </SettingRow>
          <SettingRow
            label="Start minimized to menu bar"
            description="Hide the main window on launch while keeping background monitoring active."
          >
            <Switch
              id="start-minimized"
              checked={settings.general.startMinimized}
              onChange={(checked) =>
                void update((cur) => ({
                  ...cur,
                  general: { ...cur.general, startMinimized: checked },
                }))
              }
            />
          </SettingRow>
          <SettingRow
            label="Show menu bar icon"
            description="Display FolderFlow status and quick actions in the macOS menu bar."
          >
            <Switch
              id="show-menu-bar-icon"
              checked={settings.general.showMenuBarIcon}
              onChange={(checked) =>
                void update((cur) => ({
                  ...cur,
                  general: { ...cur.general, showMenuBarIcon: checked },
                }))
              }
            />
          </SettingRow>
        </div>
      )}
      <SettingRow
        label="Setup wizard"
        description="Re-open the 6-step setup assistant to pick folders, preferences, and starter rules."
      >
        <Button variant="secondary" onClick={() => void handleRerun()}>
          <RotateCcw aria-hidden />
          Re-run setup wizard
        </Button>
      </SettingRow>
    </div>
  );
}
