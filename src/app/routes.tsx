import { Navigate, type RouteObject } from "react-router";
import { AppLayout } from "@/layouts/AppLayout";
import { FolderDetailPage } from "@/pages/folders/FolderDetailPage";
import { FoldersPage } from "@/pages/folders/FoldersPage";
import { SettingsPage } from "@/pages/settings/SettingsPage";
import { defaultPath } from "./navigation";

export const routes: RouteObject[] = [
  {
    path: "/",
    element: <AppLayout />,
    children: [
      { index: true, element: <Navigate to={defaultPath()} replace /> },
      { path: "folders", element: <FoldersPage /> },
      { path: "folders/:id", element: <FolderDetailPage /> },
      { path: "settings", element: <Navigate to="/settings/appearance" replace /> },
      { path: "settings/:section", element: <SettingsPage /> },
      { path: "*", element: <Navigate to={defaultPath()} replace /> },
    ],
  },
];
