import { CheckCircle2, KeyRound, ShieldCheck, Upload, WifiOff } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { PageHeader } from "@/components/PageHeader";
import { ErrorState, LoadingState } from "@/components/States";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { TextField } from "@/components/ui/text-field";
import { cn } from "@/lib/cn";
import type { LicenseStatus } from "@/services/license";
import { useLicense } from "@/stores/license";

function editionLabel(edition: LicenseStatus["edition"]): string {
  switch (edition) {
    case "lifetime":
      return "Lifetime";
    case "pro":
      return "Pro";
    case "trial":
      return "Trial";
    default:
      return "—";
  }
}

function sourceLabel(license: LicenseStatus): string {
  if (license.source === "offline_key") return "Offline Ed25519 signature";
  if (license.source === "online_provider") {
    return license.providerName ? `Online (${license.providerName})` : "Online license server";
  }
  return "Not activated";
}

function badgeMeta(license: LicenseStatus): { label: string; className: string } {
  switch (license.state) {
    case "active_offline":
    case "active_online":
      return { label: "Licensed ✓", className: "bg-success-subtle text-success" };
    case "expired":
      return { label: "Expired", className: "bg-warning-subtle text-warning" };
    case "invalid":
      return { label: "Invalid", className: "bg-danger-subtle text-danger" };
    case "unlicensed":
    default:
      return { label: "Unlicensed", className: "bg-muted text-fg-muted" };
  }
}

export function LicensePage() {
  const {
    license,
    status,
    busy,
    loadError,
    actionError,
    load,
    activate,
    pickAndActivateOfflineFile,
    deactivate,
    clearActionError,
  } = useLicense();
  const [keyInput, setKeyInput] = useState("");
  const [confirmDeactivateOpen, setConfirmDeactivateOpen] = useState(false);

  useEffect(() => {
    if (status === "idle") {
      void load();
    }
  }, [status, load]);

  const handleActivate = async (e: FormEvent) => {
    e.preventDefault();
    const trimmed = keyInput.trim();
    if (!trimmed) return;
    const updated = await activate(trimmed);
    if (updated?.licensed) {
      setKeyInput("");
    }
  };

  const handleConfirmDeactivate = async () => {
    setConfirmDeactivateOpen(false);
    await deactivate();
  };

  let body;
  if (status === "error" && loadError && !license) {
    body = <ErrorState error={loadError} onRetry={() => void load()} />;
  } else if (!license) {
    body = <LoadingState label="Checking license status in macOS Keychain…" />;
  } else {
    const badge = badgeMeta(license);
    body = (
      <div className="mx-auto max-w-content space-y-5">
        {actionError && <ErrorState error={actionError} onDismiss={clearActionError} />}
        {license.lastError && !actionError && (
          <ErrorState
            error={{
              code: "validation",
              message: license.lastError,
              hint: null,
              field: null,
            }}
          />
        )}

        {/* Current License Status */}
        <section
          aria-label="License status"
          className="rounded-xl border border-border bg-subtle p-5"
        >
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2.5">
                {license.licensed ? (
                  <CheckCircle2 className="size-5 text-success" aria-hidden />
                ) : (
                  <KeyRound className="size-5 text-fg-muted" aria-hidden />
                )}
                <h2 className="text-base font-semibold text-fg">
                  {license.licensed ? "FolderFlow is licensed" : "No active license"}
                </h2>
                <span className={cn("rounded-sm px-2 py-0.5 text-xs font-medium", badge.className)}>
                  {badge.label}
                </span>
              </div>
              <p className="text-sm text-fg-muted">
                {license.licensed
                  ? "Verified locally from macOS Keychain with zero background network calls."
                  : "Activate with an offline signed license key (FF1…), a .fflicense file, or a purchased license key."}
              </p>
            </div>
            {license.licensed && (
              <Button
                variant="secondary"
                disabled={busy}
                onClick={() => setConfirmDeactivateOpen(true)}
              >
                Deactivate Mac
              </Button>
            )}
          </div>

          <dl className="mt-4 grid grid-cols-1 gap-3 border-t border-border pt-4 text-sm sm:grid-cols-2 lg:grid-cols-4">
            <div>
              <dt className="text-xs text-fg-muted">Edition</dt>
              <dd className="mt-0.5 font-medium text-fg">{editionLabel(license.edition)}</dd>
            </div>
            <div>
              <dt className="text-xs text-fg-muted">Verification</dt>
              <dd className="mt-0.5 font-medium text-fg">{sourceLabel(license)}</dd>
            </div>
            <div>
              <dt className="text-xs text-fg-muted">License ID / Key</dt>
              <dd className="mt-0.5 truncate font-mono text-xs text-fg">
                {license.licenseId ?? license.maskedKey ?? "—"}
              </dd>
            </div>
            <div>
              <dt className="text-xs text-fg-muted">Validity</dt>
              <dd className="mt-0.5 font-medium text-fg">
                {!license.licensed
                  ? "—"
                  : license.edition === "lifetime" || license.expiresAtMs === null
                    ? "Never expires"
                    : new Date(license.expiresAtMs).toLocaleDateString()}
              </dd>
            </div>
          </dl>
        </section>

        {/* Activation Controls */}
        <section
          aria-label="Activate license"
          className="rounded-xl border border-border bg-surface p-5"
        >
          <h3 className="text-sm font-semibold text-fg">
            {license.licensed ? "Replace or update license" : "Activate FolderFlow"}
          </h3>
          <form onSubmit={(e) => void handleActivate(e)} className="mt-3 space-y-4">
            <TextField
              id="license-key-input"
              label="License key or offline signed key"
              value={keyInput}
              onChange={(e) => setKeyInput(e.target.value)}
              placeholder="FF1.<payload>.<signature> or XXXX-XXXX-XXXX-XXXX"
              hint={
                license.onlineProviderConfigured
                  ? "Verifies offline signed keys immediately, or validates once with your license server."
                  : "Online activation server is not configured in this build yet. Paste an offline signed key (FF1…) or import a .fflicense file."
              }
            />
            <div className="flex flex-wrap items-center gap-2">
              <Button type="submit" variant="primary" disabled={busy || keyInput.trim().length === 0}>
                <KeyRound aria-hidden />
                {busy ? "Activating…" : "Activate License"}
              </Button>
              <Button
                type="button"
                variant="secondary"
                disabled={busy}
                onClick={() => void pickAndActivateOfflineFile()}
              >
                <Upload aria-hidden />
                Import .fflicense File…
              </Button>
            </div>
          </form>
        </section>

        {/* Offline & Privacy Guarantee */}
        <section
          aria-label="Offline verification guarantee"
          className="grid grid-cols-1 gap-3 sm:grid-cols-2"
        >
          <div className="rounded-xl border border-border bg-subtle p-4 text-sm">
            <div className="flex items-center gap-2 font-medium text-fg">
              <WifiOff className="size-4 text-accent-fg" aria-hidden />
              Works Forever Offline
            </div>
            <p className="mt-1 text-xs leading-relaxed text-fg-muted">
              Once activated, FolderFlow verifies its cryptographic Ed25519 license locally on launch
              and never contacts remote license servers in the background.
            </p>
          </div>
          <div className="rounded-xl border border-border bg-subtle p-4 text-sm">
            <div className="flex items-center gap-2 font-medium text-fg">
              <ShieldCheck className="size-4 text-success" aria-hidden />
              Stored in macOS Keychain
            </div>
            <p className="mt-1 text-xs leading-relaxed text-fg-muted">
              Your license credential is saved exclusively in your macOS login Keychain — never in
              plain-text app files or logs.
            </p>
          </div>
        </section>
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title="License"
        description="Activate FolderFlow offline with an Ed25519 license key or .fflicense file stored safely in macOS Keychain."
      />
      <div className="min-h-0 flex-1 overflow-y-auto p-page">{body}</div>
      <ConfirmDialog
        open={confirmDeactivateOpen}
        title="Deactivate this Mac?"
        confirmLabel="Deactivate License"
        danger
        onConfirm={() => void handleConfirmDeactivate()}
        onCancel={() => setConfirmDeactivateOpen(false)}
      >
        Removing the license from this Mac clears the credential stored in macOS Keychain so you can
        activate FolderFlow on another Mac. None of your folders, rules, or files are affected.
      </ConfirmDialog>
    </>
  );
}

