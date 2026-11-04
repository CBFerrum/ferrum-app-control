// The control plane's own interface at /control/: cells, rollouts, incidents and workspaces, with
// every record in the records frame's lists and forms. The same release draws two pages of the
// workspace's portal (Ferrum docs/plan/apps-ui.md §9.1): a partner's customers at /portal/partner/,
// and a customer's service at /portal/service/.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, Link, Outlet, RouterProvider, useRouteError } from "react-router";
import { bootstrap, routeBase, shell } from "@ferrum/client";
import { ConfirmHost, EmptyState, ErrorState, Toaster } from "@ferrum/kit";
import { RequireAuth, setFrameMode } from "@ferrum/records/frame";
import "./styles.css";

const page = (name: "FormView" | "ListView" | "ReportView" | "AppSettingsPage" | "ScreenPage") => () => import("./records").then((m) => ({ Component: m[name] }));

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: 5_000 } } });

function RouteError() {
  return <ErrorState error={useRouteError()} onRetry={() => window.location.reload()} />;
}

function NotFound({ back = "Go to the control plane" }: { back?: string }) {
  return (
    <EmptyState title="Page not found">
      <Link to="/">{back}</Link>
    </EmptyState>
  );
}

function Started() {
  useEffect(() => shell().ready(), []);
  return null;
}

/** The portal's pages, when the platform serves this release there. */
const entry = bootstrap()?.app?.entry;
const portal = entry === "portal/partner" || entry === "portal/service" ? entry : null;

if (!portal) setFrameMode("app", ["control"]);

const portalRoutes = [
  {
    path: "/",
    element: (
      <main>
        <Started />
        <Outlet />
      </main>
    ),
    errorElement: <RouteError />,
    children: [
      portal === "portal/partner"
        ? { index: true, lazy: () => import("./Partner").then((m) => ({ Component: m.PartnerPage })) }
        : { index: true, lazy: () => import("./Service").then((m) => ({ Component: m.ServicePage })) },
      { path: "*", element: <NotFound back="Go back" /> },
    ],
  },
];

const staffRoutes = [
  {
    path: "/",
    element: (
      <>
        <Started />
        <RequireAuth />
      </>
    ),
    errorElement: <RouteError />,
    children: [
      { index: true, lazy: () => import("./Home").then((m) => ({ Component: m.Home })) },
      { path: "e/:entity", lazy: page("ListView") },
      { path: "e/:entity/:id", lazy: page("FormView") },
      { path: "r/:name", lazy: page("ReportView") },
      { path: "apps/:app/settings", lazy: page("AppSettingsPage") },
      { path: "x/:app/:page", lazy: page("ScreenPage") },
      { path: "*", element: <NotFound /> },
    ],
  },
];

const router = portal ? createBrowserRouter(portalRoutes, { basename: routeBase(portal) }) : createBrowserRouter(staffRoutes, { basename: routeBase("control") });

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
      <Toaster />
      <ConfirmHost />
    </QueryClientProvider>
  </StrictMode>,
);
