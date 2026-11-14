import { RotateCcw } from "lucide-react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useOnboarding } from "@/stores/onboarding";
import { useSettings } from "@/stores/settings";
import { SettingRow } from "./SettingRow";

export function GeneralSection() {
  const navigate = useNavigate();
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  const restartOnboarding = useOnboarding((s) => s.restartOnboarding);

  const handleRerun = async () => {
    const resetState = await restartOnboarding();
    if (resetState) {
      void navigate("/onboarding/1");
    }
  };

  return (
    <div>
      {settings && (
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

