// The public status page at /control/status/{workspace} (`shell.public`): anyone with the address,
// signed in or not, reads how each region's cells are doing, the incidents told publicly with their
// updates, and coming maintenance. It names no customer: it draws what /api/site/{workspace}/control/status
// answers, which is all a status page may say, and reads it again every minute.

import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { useParams } from "react-router";
import { ApiError, api, bootstrap, t } from "@ferrum/client";
import { Badge, EmptyState, ErrorState, Spinner } from "@ferrum/kit";
import { moment, tone } from "./words";

export interface Status {
  regions: { region: string; cells: { cell: string; state: string }[] }[];
  incidents: {
    title: string;
    severity?: string | null;
    status: string;
    started_at?: string | null;
    resolved_at?: string | null;
    cells: string[];
    updates: { at?: string | null; status?: string | null; message?: string | null }[];
  }[];
  maintenance: { cell?: string | null; region?: string | null; release?: string | null; from?: string | null; to?: string | null }[];
}

const RANK: Record<string, number> = { Operational: 0, Unknown: 1, Degraded: 2, Outage: 3 };

/** The worst state of a set of cells, as the page's headline says it. */
export function overall(s: Pick<Status, "regions" | "incidents">): { state: string; says: string } {
  const states = s.regions.flatMap((r) => r.cells.map((c) => c.state));
  const open = s.incidents.filter((i) => i.status !== "Resolved").length;
  const worst = states.reduce((w, st) => ((RANK[st] ?? 1) > (RANK[w] ?? 0) ? st : w), "Operational");
  if (states.length === 0) return { state: "Unknown", says: "No regions are reported yet" };
  if (worst === "Outage") return { state: worst, says: "Some systems are down" };
  if (worst === "Degraded") return { state: worst, says: "Some systems are degraded" };
  if (worst !== "Operational") return { state: worst, says: "Some systems have not reported" };
  return open ? { state: "Degraded", says: `All systems operational, with ${open === 1 ? "an open incident" : `${open} open incidents`}` } : { state: worst, says: "All systems operational" };
}

/** The page as drawn: `json` is where programs read the same. */
export function StatusView({ status, name, json, checked }: { status: Status; name: string; json: string; checked?: Date }) {
  const head = overall(status);
  const open = status.incidents.filter((i) => i.status !== "Resolved");
  const past = status.incidents.filter((i) => i.status === "Resolved");
  const incident = (i: Status["incidents"][number]) => (
    <article key={`${i.title}-${i.started_at ?? ""}`} className="card card-pad stack" aria-label={i.title}>
      <header className="row" style={{ justifyContent: "space-between", flexWrap: "wrap", gap: 8 }}>
        <h3 className="font-semibold">{i.title}</h3>
        <span className="row" style={{ gap: 6 }}>
          {i.severity && <Badge tone="neutral">{i.severity}</Badge>}
          <Badge tone={tone(i.status)}>{i.status}</Badge>
        </span>
      </header>
      <p className="faint">
        {t("Since")} {moment(i.started_at)}
        {i.resolved_at ? `, resolved ${moment(i.resolved_at)}` : ""}
        {i.cells.length ? ` · ${i.cells.join(", ")}` : ""}
      </p>
      {i.updates.length > 0 && (
        <ol className="stack" aria-label={`Updates on ${i.title}, newest first`}>
          {i.updates.map((u, n) => (
            <li key={n}>
              <span className="faint">{moment(u.at)}</span> · <strong>{u.status}</strong>
              {u.message ? `: ${u.message}` : ""}
            </li>
          ))}
        </ol>
      )}
    </article>
  );
  return (
    <div className="np-page st-page">
      <header className="stack">
        <p className="faint">{name}</p>
        <h1 className="page-title">{t("Service status")}</h1>
      </header>
      <main id="main" className="stack" tabIndex={-1}>
        <section className="card card-pad row" aria-label={t("Now")} style={{ justifyContent: "space-between", flexWrap: "wrap", gap: 8 }}>
          <strong className="st-headline">{head.says}</strong>
          <Badge tone={tone(head.state)}>{head.state}</Badge>
        </section>

        <section className="stack" aria-labelledby="st-incidents">
          <h2 id="st-incidents" className="st-h">{t("Incidents")}</h2>
          {open.length ? open.map(incident) : <p className="faint">{t("No open incidents.")}</p>}
        </section>

        <section className="stack" aria-labelledby="st-regions">
          <h2 id="st-regions" className="st-h">{t("Regions")}</h2>
          <div className="np-columns">
            {status.regions.map((r) => (
              <section key={r.region} className="card card-pad stack" aria-labelledby={`st-region-${r.region}`}>
                <h3 id={`st-region-${r.region}`} className="font-semibold">{r.region || "Unnamed region"}</h3>
                <ul className="stack">
                  {r.cells.map((c) => (
                    <li key={c.cell} className="row" style={{ justifyContent: "space-between" }}>
                      <span>{c.cell}</span>
                      <Badge tone={tone(c.state)}>{c.state}</Badge>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
        </section>

        <section className="stack" aria-labelledby="st-maintenance">
          <h2 id="st-maintenance" className="st-h">{t("Coming maintenance")}</h2>
          {status.maintenance.length ? (
            <ul className="stack">
              {status.maintenance.map((m, n) => (
                <li key={n}>
                  {m.release ? `Ferrum ${m.release}` : "An upgrade"} on {m.cell ?? "a cell"}
                  {m.region ? ` in ${m.region}` : ""}
                  {m.from ? `, in its window ${moment(m.from)} to ${moment(m.to)}` : ""}
                </li>
              ))}
            </ul>
          ) : (
            <p className="faint">{t("None planned.")}</p>
          )}
        </section>

        {past.length > 0 && (
          <section className="stack" aria-labelledby="st-past">
            <h2 id="st-past" className="st-h">{t("Resolved in the last three days")}</h2>
            {past.map(incident)}
          </section>
        )}

        <footer className="faint">
          {checked ? `Read ${moment(checked.toISOString())}; read again every minute. ` : ""}
          {t("Programs read the same at")} <a href={json}>{json}</a>.
        </footer>
      </main>
    </div>
  );
}

export function StatusPage() {
  const { tenant = "" } = useParams();
  const json = `/api/site/${encodeURIComponent(tenant)}/control/status`;
  const name = bootstrap()?.workspace.name ?? tenant;
  useEffect(() => {
    document.title = `Service status · ${name}`;
  }, [name]);
  const q = useQuery({ queryKey: ["control", "status", tenant], queryFn: ({ signal }) => api.get<Status>(json, undefined, signal), refetchInterval: 60_000 });
  if (q.error) {
    if (q.error instanceof ApiError && q.error.status === 404) return <EmptyState title={t("No status page here")} icon="server" />;
    return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  }
  if (!q.data) {
    return (
      <div className="np-page" role="status">
        <Spinner /> <span className="faint">{t("Reading the status")}</span>
      </div>
    );
  }
  return <StatusView status={q.data} name={name} json={json} checked={new Date(q.dataUpdatedAt)} />;
}
