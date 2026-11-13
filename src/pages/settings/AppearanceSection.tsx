import { Monitor, Moon, Rows3, Rows4, Sun } from "lucide-react";
import { ErrorState } from "@/components/States";
import { SegmentedControl } from "@/components/ui/segmented-control";
import { useSettings } from "@/stores/settings";
import type { Density, ThemeMode } from "@/types/settings";
import { SettingRow } from "./SettingRow";

const THEMES = [
  { value: "system", label: "System", icon: Monitor },
  { value: "light", label: "Light", icon: Sun },
  { value: "dark", label: "Dark", icon: Moon },
] satisfies { value: ThemeMode; label: string; icon: typeof Sun }[];

const DENSITIES = [
  { value: "comfortable", label: "Comfortable", icon: Rows3 },
  { value: "compact", label: "Compact", icon: Rows4 },
] satisfies { value: Density; label: string; icon: typeof Sun }[];

export function AppearanceSection() {
  const settings = useSettings((s) => s.settings);
  const saveError = useSettings((s) => s.saveError);
  const update = useSettings((s) => s.update);
  if (!settings) return null;
  const { theme, density } = settings.appearance;

  return (
    <div>
      {saveError && <ErrorState error={saveError} />}
      <SettingRow label="Appearance" description="Follow macOS, or always use light or dark.">
        <SegmentedControl
          label="Appearance"
          value={theme}
          options={THEMES}
          onChange={(value) => update((s) => ({ ...s, appearance: { ...s.appearance, theme: value } }))}
        />
      </SettingRow>
      <SettingRow label="Density" description="Compact fits more rows in tables and lists.">
        <SegmentedControl
          label="Density"
          value={density}
          options={DENSITIES}
          onChange={(value) => update((s) => ({ ...s, appearance: { ...s.appearance, density: value } }))}
        />
      </SettingRow>
    </div>
  );
}
