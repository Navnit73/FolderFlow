import { useEffect, useState } from "react";
import { createHashRouter, RouterProvider } from "react-router";
import { routes } from "@/app/routes";
import { ErrorState, LoadingState } from "@/components/States";
import { useSettings } from "@/stores/settings";

export function App() {
  const status = useSettings((s) => s.status);
  const loadError = useSettings((s) => s.loadError);
  const load = useSettings((s) => s.load);
  // Hash routing: the bundled app serves a single index.html.
  const [router] = useState(() => createHashRouter(routes));

  useEffect(() => {
    void load();
  }, [load]);

  if (status === "error" && loadError) {
    return (
      <div className="flex h-full items-center justify-center bg-surface p-page">
        <div className="w-full max-w-md">
          <ErrorState error={loadError} onRetry={() => void load()} />
        </div>
      </div>
    );
  }
  if (status !== "ready") {
    return (
      <div className="flex h-full items-center justify-center bg-surface">
        <LoadingState label="Starting FolderFlow…" />
      </div>
    );
  }
  return <RouterProvider router={router} />;
}
