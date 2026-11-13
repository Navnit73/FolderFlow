import { useEffect } from "react";
import { Outlet } from "react-router";
import { Sidebar } from "@/components/Sidebar";
import { useFolders } from "@/stores/folders";

export function AppLayout() {
  const load = useFolders((s) => s.load);
  const subscribe = useFolders((s) => s.subscribe);

  useEffect(() => {
    let off: (() => void) | undefined;
    let cancelled = false;
    void load();
    void subscribe().then((unsub) => {
      if (cancelled) unsub();
      else off = unsub;
    });
    return () => {
      cancelled = true;
      off?.();
    };
  }, [load, subscribe]);

  return (
    <div className="flex h-full bg-surface text-fg">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        <Outlet />
      </main>
    </div>
  );
}
