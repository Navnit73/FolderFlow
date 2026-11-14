import {
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  CheckCircle2,
  Eye,
  FolderOpen,
  FolderPlus,
  RefreshCw,
  ShieldAlert,
  Sparkles,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router";
import { ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/cn";
import { formatBytes, plural } from "@/lib/format";
import type { KnownFolder, OrganizationPreference } from "@/services/onboarding";
import { useOnboarding } from "@/stores/onboarding";

const STEP_TITLES = [
  "Welcome",
  "Folders & Access",
  "Organization Style",
  "Starter Rules",
  "Initial Scan",
  "Preview & Confirm",
] as const;

const KNOWN_FOLDERS: { kind: KnownFolder; label: string }[] = [
  { kind: "downloads", label: "Add Downloads" },
  { kind: "desktop", label: "Add Desktop" },
  { kind: "documents", label: "Add Documents" },
  { kind: "pictures", label: "Add Pictures" },
  { kind: "movies", label: "Add Movies" },
  { kind: "music", label: "Add Music" },
];

const PREFERENCE_OPTIONS: {
  value: OrganizationPreference;
  label: string;
  description: string;
}[] = [
  {
    value: "by_category",
    label: "By category",
    description: "Group PDFs, screenshots, invoices, archives, and media by file category.",
  },
  {
    value: "by_type",
    label: "By type",
    description: "Group files strictly into folders based on file extensions and formats.",
  },
  {
    value: "by_date",
    label: "By date",
    description: "Sort files cleanly into Year / Month folders using file timestamps.",
  },
  {
    value: "custom_rules",
    label: "Custom rules",
    description: "Start minimal and build your own condition and template workflows.",
  },
  {
    value: "keep_original_structure",
    label: "Keep original structure",
    description: "Monitor folders for insights first without proposing automatic moves.",
  },
];

export function OnboardingPage() {
  const { step: stepParam } = useParams();
  const navigate = useNavigate();
  const {
    state,
    folders,
    accessByFolder,
    presets,
    scanReport,
    scanning,
    completing,
    status,
    loadError,
    actionError,
    load,
    loadResources,
    saveStep,
    addKnownFolder,
    addPickerFolder,
    recheckAccess,
    openPrivacySettings,
    setPreferenceLocal,
    togglePresetLocal,
    runScanPreview,
    complete,
    clearActionError,
  } = useOnboarding();

  const [recursive, setRecursive] = useState(false);
  const [confirmEnable, setConfirmEnable] = useState(false);

  useEffect(() => {
    void load().then((loaded) => {
      void loadResources();
      if (loaded && !stepParam) {
        void navigate(`/onboarding/${loaded.currentStep}`, { replace: true });
      }
    });
  }, [load, loadResources, navigate, stepParam]);

  const parsedStep = Number(stepParam);
  const currentStep =
    Number.isInteger(parsedStep) && parsedStep >= 1 && parsedStep <= 6
      ? parsedStep
      : state?.currentStep ?? 1;

  useEffect(() => {
    if ((currentStep === 5 || currentStep === 6) && status === "ready" && !scanReport && !scanning) {
      void runScanPreview();
    }
  }, [currentStep, status, scanReport, scanning, runScanPreview]);

  const goToStep = async (nextStep: number) => {
    const clamped = Math.max(1, Math.min(6, nextStep));
    await saveStep({
      currentStep: clamped,
      organizationPreference: state?.organizationPreference ?? null,
      selectedPresetIds: state?.selectedPresetIds ?? null,
    });
    void navigate(`/onboarding/${clamped}`);
  };

  if (status === "error" && loadError) {
    return (
      <div className="flex h-full items-center justify-center bg-surface p-page">
        <div className="w-full max-w-lg">
          <ErrorState error={loadError} onRetry={() => void load()} />
        </div>
      </div>
    );
  }

  if (status !== "ready" || !state) {
    return (
      <div className="flex h-full items-center justify-center bg-surface">
        <LoadingState label="Preparing setup…" />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col bg-surface">
      <header
        data-tauri-drag-region
        className="flex h-header shrink-0 items-center justify-between border-b border-border px-page"
      >
        <div className="flex items-center gap-2">
          <Sparkles className="size-4 text-accent-fg" aria-hidden />
          <span className="font-display text-sm font-semibold text-fg">
            FolderFlow Setup
          </span>
        </div>
        <ol aria-label="Setup steps" className="flex items-center gap-2 text-xs">
          {STEP_TITLES.map((label, idx) => {
            const num = idx + 1;
            const active = num === currentStep;
            const done = num < currentStep;
            return (
              <li
                key={label}
                className={cn(
                  "rounded-sm px-2 py-0.5",
                  active
                    ? "bg-selected font-medium text-fg"
                    : done
                      ? "text-accent-fg"
                      : "text-fg-subtle",
                )}
              >
                {num}. {label}
              </li>
            );
          })}
        </ol>
      </header>

      <main className="min-h-0 flex-1 overflow-y-auto p-page">
        <div className="mx-auto max-w-2xl space-y-6 py-4">
          {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}

          {currentStep === 1 && (
            <section className="space-y-4">
              <h1 className="font-display text-3xl font-semibold tracking-tight text-fg">
                Welcome to FolderFlow
              </h1>
              <p className="text-base text-fg-muted">
                Keep your Mac folders tidy with transparent, reversible organization rules.
              </p>
              <div className="rounded-lg border border-border bg-subtle p-4 text-sm text-fg-muted">
                <p className="font-medium text-fg">Safety first — read-only until you confirm</p>
                <p className="mt-1">
                  During setup, FolderFlow only scans and builds a dry-run preview. No files are
                  moved, renamed, or copied until you explicitly review and enable rules.
                </p>
              </div>
              <div className="flex justify-end pt-2">
                <Button variant="primary" onClick={() => void goToStep(2)}>
                  Get started
                  <ArrowRight aria-hidden />
                </Button>
              </div>
            </section>
          )}

          {currentStep === 2 && (
            <section className="space-y-4">
              <div>
                <h1 className="font-display text-xl font-semibold text-fg">
                  Choose folders to watch
                </h1>
                <p className="mt-1 text-sm text-fg-muted">
                  Pick the folders FolderFlow should inspect. You can grant macOS folder permission
                  and adjust subfolders anytime.
                </p>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                {KNOWN_FOLDERS.map((item) => (
                  <Button
                    key={item.kind}
                    variant="secondary"
                    onClick={() => void addKnownFolder(item.kind, recursive)}
                  >
                    <FolderPlus aria-hidden />
                    {item.label}
                  </Button>
                ))}
                <Button variant="secondary" onClick={() => void addPickerFolder(recursive)}>
                  <FolderOpen aria-hidden />
                  Choose folder…
                </Button>
              </div>

              <label className="inline-flex cursor-pointer items-center gap-2 text-xs text-fg-muted">
                <input
                  type="checkbox"
                  checked={recursive}
                  onChange={(e) => setRecursive(e.target.checked)}
                />
                Scan subfolders recursively for newly added folders
              </label>

              {folders.length > 0 && (
                <div className="divide-y divide-border rounded-lg border border-border bg-surface">
                  {folders.map((folder) => {
                    const access = accessByFolder[folder.id] ?? "granted";
                    return (
                      <div key={folder.id} className="space-y-2 p-3">
                        <div className="flex items-center justify-between gap-2">
                          <div className="min-w-0">
                            <p className="truncate font-medium text-fg">{folder.name}</p>
                            <p className="truncate font-mono text-xs text-fg-muted">{folder.path}</p>
                          </div>
                          <span
                            className={cn(
                              "rounded-sm px-2 py-0.5 text-xs font-medium",
                              access === "granted"
                                ? "bg-success-subtle text-success"
                                : "bg-danger-subtle text-danger",
                            )}
                          >
                            {access === "granted" ? "Access granted" : `Access ${access}`}
                          </span>
                        </div>
                        {access === "denied" && (
                          <div
                            role="alert"
                            className="space-y-2 rounded-md border border-border bg-danger-subtle p-3 text-xs text-fg"
                          >
                            <div className="flex items-start gap-2">
                              <ShieldAlert className="mt-0.5 size-4 shrink-0 text-danger" aria-hidden />
                              <div>
                                <p className="font-medium">
                                  macOS denied access to {folder.name}.
                                </p>
                                <p className="mt-0.5 text-fg-muted">
                                  Open System Settings → Privacy & Security → Files and Folders,
                                  enable FolderFlow, then re-check access.
                                </p>
                              </div>
                            </div>
                            <div className="flex flex-wrap gap-2">
                              <Button
                                size="sm"
                                variant="secondary"
                                onClick={() => void openPrivacySettings()}
                              >
                                Open Privacy & Security Settings
                              </Button>
                              <Button
                                size="sm"
                                variant="ghost"
                                onClick={() => void recheckAccess(folder.id)}
                              >
                                <RefreshCw aria-hidden />
                                Re-check access
                              </Button>
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              <div className="flex justify-between pt-2">
                <Button variant="ghost" onClick={() => void goToStep(1)}>
                  <ArrowLeft aria-hidden />
                  Back
                </Button>
                <Button variant="primary" onClick={() => void goToStep(3)}>
                  Continue
                  <ArrowRight aria-hidden />
                </Button>
              </div>
            </section>
          )}

          {currentStep === 3 && (
            <section className="space-y-4">
              <div>
                <h1 className="font-display text-xl font-semibold text-fg">
                  How should FolderFlow organize your files?
                </h1>
                <p className="mt-1 text-sm text-fg-muted">
                  Choose a guiding style — this tailors your recommended starter rules.
                </p>
              </div>

              <div role="radiogroup" aria-label="Organization preference" className="space-y-2">
                {PREFERENCE_OPTIONS.map((opt) => {
                  const selected = state.organizationPreference === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      onClick={() => setPreferenceLocal(opt.value)}
                      className={cn(
                        "w-full rounded-lg border p-3 text-left transition-colors duration-(--ff-duration-base)",
                        selected
                          ? "border-border-focus bg-selected"
                          : "border-border bg-surface hover:bg-hover",
                      )}
                    >
                      <p className="font-medium text-fg">{opt.label}</p>
                      <p className="mt-0.5 text-xs text-fg-muted">{opt.description}</p>
                    </button>
                  );
                })}
              </div>

              <div className="flex justify-between pt-2">
                <Button variant="ghost" onClick={() => void goToStep(2)}>
                  <ArrowLeft aria-hidden />
                  Back
                </Button>
                <Button variant="primary" onClick={() => void goToStep(4)}>
                  Continue
                  <ArrowRight aria-hidden />
                </Button>
              </div>
            </section>
          )}

          {currentStep === 4 && (
            <section className="space-y-4">
              <div>
                <h1 className="font-display text-xl font-semibold text-fg">
                  Choose starter rules
                </h1>
                <p className="mt-1 text-sm text-fg-muted">
                  Selected presets will be simulated right now and saved safely for you to review.
                </p>
              </div>

              <div className="divide-y divide-border rounded-lg border border-border bg-surface">
                {presets.map((preset) => {
                  const checked = state.selectedPresetIds.includes(preset.id);
                  return (
                    <label
                      key={preset.id}
                      className="flex cursor-pointer items-start gap-3 p-3 hover:bg-hover"
                    >
                      <input
                        type="checkbox"
                        className="mt-0.5"
                        checked={checked}
                        onChange={() => togglePresetLocal(preset.id)}
                      />
                      <div className="min-w-0">
                        <p className="font-medium text-fg">{preset.name}</p>
                        <p className="text-xs text-fg-muted">{preset.description}</p>
                      </div>
                    </label>
                  );
                })}
              </div>

              <div className="flex justify-between pt-2">
                <Button variant="ghost" onClick={() => void goToStep(3)}>
                  <ArrowLeft aria-hidden />
                  Back
                </Button>
                <Button
                  variant="primary"
                  disabled={scanning}
                  onClick={() => {
                    void saveStep({
                      currentStep: 5,
                      organizationPreference: state.organizationPreference,
                      selectedPresetIds: state.selectedPresetIds,
                    }).then(() => {
                      void runScanPreview();
                      void navigate("/onboarding/5");
                    });
                  }}
                >
                  {scanning ? "Scanning…" : "Scan & preview"}
                  <ArrowRight aria-hidden />
                </Button>
              </div>
            </section>
          )}

          {currentStep === 5 && (
            <section className="space-y-4">
              <div>
                <h1 className="font-display text-xl font-semibold text-fg">
                  What FolderFlow found
                </h1>
                <p className="mt-1 text-sm text-fg-muted">
                  Zero files were modified during this read-only scan.
                </p>
              </div>

              {scanning || !scanReport ? (
                <LoadingState label="Scanning folders (read-only)…" />
              ) : (
                <div className="space-y-4">
                  <div className="grid grid-cols-3 gap-3">
                    <div className="rounded-lg border border-border bg-subtle p-3">
                      <p className="text-xs text-fg-muted">Folders scanned</p>
                      <p className="mt-1 text-2xl font-semibold text-fg">
                        {scanReport.foldersScanned}
                      </p>
                    </div>
                    <div className="rounded-lg border border-border bg-subtle p-3">
                      <p className="text-xs text-fg-muted">Files inspected</p>
                      <p className="mt-1 text-2xl font-semibold text-fg">
                        {scanReport.totalFiles.toLocaleString()}
                      </p>
                    </div>
                    <div className="rounded-lg border border-border bg-subtle p-3">
                      <p className="text-xs text-fg-muted">Total size</p>
                      <p className="mt-1 text-2xl font-semibold text-fg">
                        {formatBytes(scanReport.totalBytes)}
                      </p>
                    </div>
                  </div>

                  {scanReport.byCategory.length > 0 && (
                    <div className="rounded-lg border border-border bg-surface p-3">
                      <p className="text-xs font-medium text-fg-muted">Files by category</p>
                      <div className="mt-2 flex flex-wrap gap-2">
                        {scanReport.byCategory.map((c) => (
                          <span
                            key={c.category}
                            className="rounded-sm bg-muted px-2 py-0.5 text-xs text-fg"
                          >
                            <span className="capitalize">{c.category}</span>: {c.count} (
                            {formatBytes(c.bytes)})
                          </span>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              )}

              <div className="flex justify-between pt-2">
                <Button variant="ghost" onClick={() => void goToStep(4)}>
                  <ArrowLeft aria-hidden />
                  Back
                </Button>
                <Button variant="primary" onClick={() => void goToStep(6)}>
                  Inspect preview
                  <ArrowRight aria-hidden />
                </Button>
              </div>
            </section>
          )}

          {currentStep === 6 && (
            <section className="space-y-4">
              <div>
                <h1 className="font-display text-xl font-semibold text-fg">
                  Review your first preview
                </h1>
                <p className="mt-1 text-sm text-fg-muted">
                  These changes are only simulated. New starter rules remain paused unless you
                  explicitly choose to enable them below.
                </p>
              </div>

              {scanning || !scanReport ? (
                <LoadingState label="Building dry-run preview…" />
              ) : (
                <div className="space-y-3">
                  <div className="flex items-center justify-between rounded-lg border border-border bg-subtle p-3 text-sm">
                    <span className="inline-flex items-center gap-2 font-medium text-fg">
                      <Eye className="size-4 text-accent-fg" aria-hidden />
                      {plural(scanReport.planSummary.filesMatched, "proposed file change")}
                    </span>
                    <span className="text-xs text-fg-muted">
                      {formatBytes(scanReport.planSummary.totalBytesAffected)} affected
                    </span>
                  </div>

                  {scanReport.sampleItems.length > 0 && (
                    <div className="divide-y divide-border rounded-lg border border-border bg-surface">
                      {scanReport.sampleItems.slice(0, 8).map((item) => (
                        <div key={item.id} className="p-3 text-sm">
                          <div className="flex items-center justify-between gap-2">
                            <span className="font-medium text-fg">{item.originalName}</span>
                            <span className="text-xs text-fg-muted">{item.ruleName}</span>
                          </div>
                          <p className="mt-0.5 truncate font-mono text-xs text-fg-muted">
                            → {item.proposedDestination}
                          </p>
                        </div>
                      ))}
                    </div>
                  )}

                  <label className="flex cursor-pointer items-start gap-2.5 rounded-lg border border-border bg-warning-subtle/40 p-3 text-sm text-fg">
                    <input
                      type="checkbox"
                      className="mt-0.5"
                      checked={confirmEnable}
                      onChange={(e) => setConfirmEnable(e.target.checked)}
                    />
                    <div>
                      <span className="inline-flex items-center gap-1.5 font-medium">
                        {confirmEnable ? (
                          <CheckCircle2 className="size-4 text-success" aria-hidden />
                        ) : (
                          <AlertTriangle className="size-4 text-warning" aria-hidden />
                        )}
                        Enable selected starter rules for automatic organization now
                      </span>
                      <p className="mt-0.5 text-xs text-fg-muted">
                        Leave unchecked to save rules disabled so you can test and execute from
                        Preview first.
                      </p>
                    </div>
                  </label>
                </div>
              )}

              <div className="flex justify-between pt-2">
                <Button variant="ghost" onClick={() => void goToStep(5)}>
                  <ArrowLeft aria-hidden />
                  Back
                </Button>
                <Button
                  variant="primary"
                  disabled={completing}
                  onClick={() => {
                    void complete(confirmEnable).then((rep) => {
                      if (rep) void navigate("/folders", { replace: true });
                    });
                  }}
                >
                  {completing ? "Finishing…" : "Finish setup"}
                </Button>
              </div>
            </section>
          )}
        </div>
      </main>
    </div>
  );
}

