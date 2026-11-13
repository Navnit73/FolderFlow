import { Outlet } from "react-router";
import { Sidebar } from "@/components/Sidebar";

export function AppLayout() {
  return (
    <div className="flex h-full bg-surface text-fg">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        <Outlet />
      </main>
    </div>
  );
}
