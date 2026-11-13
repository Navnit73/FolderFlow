import { Navigate, type RouteObject } from "react-router";
import { AppLayout } from "@/layouts/AppLayout";
import { SettingsPage } from "@/pages/settings/SettingsPage";
import { defaultPath } from "./navigation";

export const routes: RouteObject[] = [
  {
    path: "/",
    element: <AppLayout />,
    children: [
      { index: true, element: <Navigate to={defaultPath()} replace /> },
      { path: "settings", element: <Navigate to="/settings/appearance" replace /> },
      { path: "settings/:section", element: <SettingsPage /> },
      { path: "*", element: <Navigate to={defaultPath()} replace /> },
    ],
  },
];
