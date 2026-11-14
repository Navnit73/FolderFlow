import { Navigate, type RouteObject } from "react-router";
import { AppLayout } from "@/layouts/AppLayout";
import { ActivityPage } from "@/pages/activity/ActivityPage";
import { CategoriesPage } from "@/pages/categories/CategoriesPage";
import { DuplicatesPage } from "@/pages/duplicates/DuplicatesPage";
import { FolderDetailPage } from "@/pages/folders/FolderDetailPage";
import { FoldersPage } from "@/pages/folders/FoldersPage";
import { LicensePage } from "@/pages/license/LicensePage";
import { OnboardingPage } from "@/pages/onboarding/OnboardingPage";
import { OverviewPage } from "@/pages/overview/OverviewPage";
import { PreviewPage } from "@/pages/preview/PreviewPage";
import { RuleBuilderPage } from "@/pages/rules/RuleBuilderPage";
import { SchedulerPage } from "@/pages/scheduler/SchedulerPage";
import { RulesPage } from "@/pages/rules/RulesPage";
import { SettingsPage } from "@/pages/settings/SettingsPage";
import { defaultPath } from "./navigation";

export const routes: RouteObject[] = [
  { path: "/onboarding", element: <OnboardingPage /> },
  { path: "/onboarding/:step", element: <OnboardingPage /> },
  {
    path: "/",
    element: <AppLayout />,
    children: [
      { index: true, element: <Navigate to={defaultPath()} replace /> },
      { path: "overview", element: <OverviewPage /> },
      { path: "folders", element: <FoldersPage /> },
      { path: "folders/:id", element: <FolderDetailPage /> },
      { path: "rules", element: <RulesPage /> },
      { path: "rules/new", element: <RuleBuilderPage /> },
      { path: "rules/:id", element: <RuleBuilderPage /> },
      { path: "preview", element: <PreviewPage /> },
      { path: "activity", element: <ActivityPage /> },
      { path: "duplicates", element: <DuplicatesPage /> },
      { path: "scheduler", element: <SchedulerPage /> },
      { path: "categories", element: <CategoriesPage /> },
      { path: "settings", element: <Navigate to="/settings/appearance" replace /> },
      { path: "settings/:section", element: <SettingsPage /> },
      { path: "license", element: <LicensePage /> },
      { path: "*", element: <Navigate to={defaultPath()} replace /> },
    ],
  },
];
