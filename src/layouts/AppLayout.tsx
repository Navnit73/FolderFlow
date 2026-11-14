import { useEffect } from "react";
import { Outlet, useNavigate } from "react-router";
import { events } from "@/bindings";
import { Sidebar } from "@/components/Sidebar";
import { useGlobalShortcuts } from "@/hooks/useGlobalShortcuts";
import { useFolders } from "@/stores/folders";
import { useLicense } from "@/stores/license";

export function AppLayout() {
  const navigate = useNavigate();
  const loadFolders = useFolders((s) => s.load);
  const subscribeFolders = useFolders((s) => s.subscribe);
  const loadLicense = useLicense((s) => s.load);

  useGlobalShortcuts();

  useEffect(() => {
    let offFolders: (() => void) | undefined;
    let offTrayNav: (() => void) | undefined;
    let cancelled = false;
    void loadFolders();
    void loadLicense();
    void subscribeFolders().then((unsub) => {
      if (cancelled) unsub();
      else offFolders = unsub;
    });
    void events.trayNavigateEvent
      .listen((e) => {
        if (!cancelled) void navigate(e.payload.path);
      })
      .then((unsub) => {
        if (cancelled) unsub();
        else offTrayNav = unsub;
      });
    return () => {
      cancelled = true;
      offFolders?.();
      offTrayNav?.();
    };
  }, [loadFolders, loadLicense, subscribeFolders, navigate]);

  return (
    <div className="flex h-full bg-surface text-fg">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        <Outlet />
      </main>
    </div>
  );
}
