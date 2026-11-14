import { useEffect, useState } from "react";
import { createHashRouter, RouterProvider } from "react-router";
import { routes } from "@/app/routes";
import { ErrorState, LoadingState } from "@/components/States";
import { useOnboarding } from "@/stores/onboarding";
import { useSettings } from "@/stores/settings";

export function App() {
  const status = useSettings((s) => s.status);
  const loadError = useSettings((s) => s.loadError);
  const load = useSettings((s) => s.load);
  const onboardingStatus = useOnboarding((s) => s.status);
  const onboardingState = useOnboarding((s) => s.state);
  const loadOnboarding = useOnboarding((s) => s.load);
  // Hash routing: the bundled app serves a single index.html.
  const [router] = useState(() => createHashRouter(routes));

  useEffect(() => {
    void load();
    void loadOnboarding();
  }, [load, loadOnboarding]);

  useEffect(() => {
    if (
      status === "ready" &&
      onboardingStatus === "ready" &&
      onboardingState &&
      !onboardingState.completed &&
      !window.location.hash.startsWith("#/onboarding")
    ) {
      void router.navigate(`/onboarding/${onboardingState.currentStep}`, { replace: true });
    }
  }, [status, onboardingStatus, onboardingState, router]);

  if (status === "error" && loadError) {
    return (
      <div className="flex h-full items-center justify-center bg-surface p-page">
        <div className="w-full max-w-md">
          <ErrorState error={loadError} onRetry={() => void load()} />
        </div>
      </div>
    );
  }
  if (status !== "ready" || onboardingStatus === "idle" || onboardingStatus === "loading") {
    return (
      <div className="flex h-full items-center justify-center bg-surface">
        <LoadingState label="Starting FolderFlow…" />
      </div>
    );
  }
  return <RouterProvider router={router} />;
}
