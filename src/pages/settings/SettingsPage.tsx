import { Navigate, NavLink, useParams } from "react-router";
import { PageHeader } from "@/components/PageHeader";
import { cn } from "@/lib/cn";
import { AboutSection } from "./AboutSection";
import { AppearanceSection } from "./AppearanceSection";
import { BackupSection } from "./BackupSection";
import { GeneralSection } from "./GeneralSection";
import { MonitoringSection } from "./MonitoringSection";
import { NotificationsSection } from "./NotificationsSection";
import { OrganizationSection } from "./OrganizationSection";
import { PerformanceSection } from "./PerformanceSection";
import { PrivacySection } from "./PrivacySection";

const SECTIONS = [
  { id: "general", label: "General", Component: GeneralSection },
  { id: "monitoring", label: "Monitoring", Component: MonitoringSection },
  { id: "organization", label: "Organization", Component: OrganizationSection },
  { id: "performance", label: "Performance", Component: PerformanceSection },
  { id: "notifications", label: "Notifications", Component: NotificationsSection },
  { id: "appearance", label: "Appearance", Component: AppearanceSection },
  { id: "privacy", label: "Privacy", Component: PrivacySection },
  { id: "backup", label: "Backup", Component: BackupSection },
  { id: "about", label: "About", Component: AboutSection },
] as const;

export function SettingsPage() {
  const { section } = useParams();
  const active = SECTIONS.find((s) => s.id === section);
  if (!active) return <Navigate to="/settings/appearance" replace />;
  const { Component } = active;

  return (
    <>
      <PageHeader title="Settings" />
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Settings sections" className="w-48 shrink-0 space-y-0.5 border-r border-border p-2">
          {SECTIONS.map((s) => (
            <NavLink
              key={s.id}
              to={`/settings/${s.id}`}
              className={({ isActive }) =>
                cn(
                  "flex h-row items-center rounded-md px-2 text-sm",
                  isActive ? "bg-selected font-medium text-fg" : "text-fg-muted hover:bg-hover hover:text-fg",
                )
              }
            >
              {s.label}
            </NavLink>
          ))}
        </nav>
        <section aria-label={active.label} className="min-w-0 flex-1 overflow-y-auto p-page">
          <div className="max-w-content">
            <Component />
          </div>
        </section>
      </div>
    </>
  );
}
