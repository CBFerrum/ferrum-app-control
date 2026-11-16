// The control plane's own interface at /control/: cells, rollouts, incidents and workspaces, with
// every record in the records frame's lists and forms. The same release draws two pages of the
// workspace's portal (Ferrum docs/plan/apps-ui.md §9.1): a partner's customers at /portal/partner/,
// and a customer's service at /portal/service/; and the public status page anyone opens at
// /control/status/{workspace} (`shell.public`), without signing in.

import { useEffect } from "react";
import { Outlet, useRouteError } from "react-router";
import { bootstrap, isolate, isPublicPage, shell, t } from "@ferrum/client";
import { ButtonLink, StatePanel } from "@ferrum/kit";
import { mountApp, RequireAuth, setFrameMode } from "@ferrum/records/frame";
import "./styles.css";

const page = (name: "FormView" | "ListView" | "ReportView") => () => import("./records").then((m) => ({ Component: m[name] }));

function RouteError() {
  return <StatePanel kind="failed" error={useRouteError()} onRetry={() => window.location.reload()} />;
}

function NotFound({ back = t("Go to the control plane") }: { back?: string }) {
  return (
    <StatePanel kind="empty" title={t("Page not found")} action={<ButtonLink to="/">{isolate(back)}</ButtonLink>} />
  );
}

function Started() {
  useEffect(() => shell().ready(), []);
  return null;
}

/** The portal's pages, when the platform serves this release there. */
const entry = bootstrap()?.app?.entry;
const portal = entry === "portal/partner" || entry === "portal/service" ? entry : null;

/** The Control Plane's packages, its connector with ERP's among them: the sidebar shows their modules. */
const CONTROL_PACKAGES = ["control", "control_erp"];

// The public status page has no sign-in and no frame: a 401 there never asks anyone to sign in.
if (!portal && !isPublicPage()) setFrameMode("app", CONTROL_PACKAGES);

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
      { path: "*", get element() { return <NotFound back={t("Go back")} />; } },
    ],
  },
];

const staffRoutes = [
  {
    path: "/",
    element: (
      <>
        <Started />
        <Outlet />
      </>
    ),
    errorElement: <RouteError />,
    children: [
      // For anyone with the address.
      { path: "status/:tenant", lazy: () => import("./Status").then((m) => ({ Component: m.StatusPage })) },
      { path: "status/*", element: <main id="main" className="st-page" tabIndex={-1}><StatePanel kind="empty" title={t("Page not found")} /></main> },
      {
        // For the operators: signed in, in the frame.
        element: <RequireAuth />,
        children: [
          { index: true, lazy: () => import("./Home").then((m) => ({ Component: m.Home })) },
          { path: "e/:entity", lazy: page("ListView") },
          { path: "e/:entity/:id", lazy: page("FormView") },
          { path: "r/:name", lazy: page("ReportView") },
          { path: "*", element: <NotFound /> },
        ],
      },
    ],
  },
];

mountApp({
  catalogs: { ar: () => import("./locales/ar.json"), hi: () => import("./locales/hi.json") },
  ...(portal ? { route: portal, routes: portalRoutes } : { route: "control", routes: staffRoutes }),
});
