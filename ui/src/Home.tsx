// The control plane's console: cells by region with their health and what is wrong, rollouts in
// progress wave by wave, open incidents, and the workspaces that are not healthy. Each opens its
// record.

import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { api, docPath } from "@ferrum/client";
import { Badge, ErrorState, PageLoading, useShellTitle } from "@ferrum/kit";
import { tone } from "./words";

export interface Row {
  id: string;
  [k: string]: unknown;
}

const list = (entity: string, filters: unknown[] = [], extra: Record<string, string | number> = {}) =>
  api.get<{ rows: Row[] }>(docPath(entity), { filters: JSON.stringify(filters), limit: 500, ...extra }).then((r) => r.rows);

/** Rows by a field's value, the values in name order. */
export function groupBy(rows: Row[], field: string): [string, Row[]][] {
  const out = new Map<string, Row[]>();
  for (const r of rows) {
    const k = String(r[field] ?? "");
    out.set(k, [...(out.get(k) ?? []), r]);
  }
  return [...out.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function Waves({ id, names }: { id: string; names: Map<string, string> }) {
  const q = useQuery({ queryKey: ["control", "rollout", id], queryFn: () => api.get<Row>(docPath("Rollout", id)) });
  const waves = (q.data?.waves as Row[] | undefined) ?? [];
  return (
    <ol className="row" style={{ gap: 8, flexWrap: "wrap" }}>
      {waves.map((w) => (
        <li key={w.id}>
          <Badge tone={tone(String(w.state))}>
            {names.get(String(w.cell)) ?? "a cell"} · {String(w.state)}
          </Badge>
        </li>
      ))}
    </ol>
  );
}

export function Home() {
  useShellTitle("Control Plane");
  const cells = useQuery({ queryKey: ["control", "cells"], queryFn: () => list("Cell", [], { sort: "cell_name asc" }), refetchInterval: 60_000 });
  const rollouts = useQuery({ queryKey: ["control", "rollouts"], queryFn: () => list("Rollout", [["status", "in", ["Running", "Halted"]]]) });
  const incidents = useQuery({ queryKey: ["control", "incidents"], queryFn: () => list("ServiceIncident", [["status", "!=", "Resolved"]]) });
  const ill = useQuery({ queryKey: ["control", "ill"], queryFn: () => list("HostedWorkspace", [["health", "in", ["Degraded", "Unreachable"]]]) });
  const failed = [cells, rollouts, incidents, ill].find((q) => q.isError);
  if (failed) return <ErrorState error={failed.error} onRetry={() => void failed.refetch()} />;
  if (!cells.data || !rollouts.data || !incidents.data || !ill.data) return <PageLoading />;
  const names = new Map(cells.data.map((c) => [c.id, String(c.cell_name)]));
  return (
    <div className="np-page stack">
      <header className="row" style={{ justifyContent: "space-between" }}>
        <h2>Cells</h2>
        <Link to="/e/Cell/new">New cell</Link>
      </header>
      {cells.data.length === 0 && <p className="faint">No cell yet: add the deployments that run customers' workspaces.</p>}
      {groupBy(cells.data, "region").map(([region, rows]) => (
        <section key={region} className="card card-pad stack" aria-label={region}>
          <h3>{region}</h3>
          <div className="np-columns">
            {rows.map((c) => (
              <Link key={c.id} className="card card-pad stack" to={`/e/Cell/${c.id}`}>
                <span className="row" style={{ justifyContent: "space-between" }}>
                  <strong>{String(c.cell_name)}</strong>
                  <Badge tone={tone(String(c.health))}>{String(c.health)}</Badge>
                </span>
                <span className="faint">
                  {String(c.placed ?? 0)} of {String(c.capacity)} workspaces · {String(c.reported_version ?? "not reported")}
                </span>
                {c.problems ? <span>{String(c.problems)}</span> : null}
              </Link>
            ))}
          </div>
        </section>
      ))}
      <section className="card card-pad stack" aria-label="Rollouts">
        <header className="row" style={{ justifyContent: "space-between" }}>
          <h3>Rollouts</h3>
          <Link to="/e/Rollout/new">New rollout</Link>
        </header>
        {rollouts.data.length === 0 && <p className="faint">None running.</p>}
        {rollouts.data.map((r) => (
          <div key={r.id} className="stack">
            <Link to={`/e/Rollout/${r.id}`}>
              {String((r._titles as Record<string, string> | undefined)?.release ?? r.release)} <Badge tone={tone(String(r.status))}>{String(r.status)}</Badge>
            </Link>
            {r.halted_reason ? <span>{String(r.halted_reason)}</span> : null}
            <Waves id={r.id} names={names} />
          </div>
        ))}
      </section>
      <section className="card card-pad stack" aria-label="Incidents">
        <header className="row" style={{ justifyContent: "space-between" }}>
          <h3>Open incidents</h3>
          <Link to="/e/ServiceIncident/new">New incident</Link>
        </header>
        {incidents.data.length === 0 && <p className="faint">None open.</p>}
        <ul className="stack">
          {incidents.data.map((i) => (
            <li key={i.id}>
              <Link to={`/e/ServiceIncident/${i.id}`}>{String(i.title)}</Link> · {String(i.severity)} <Badge tone={tone(String(i.status))}>{String(i.status)}</Badge>
            </li>
          ))}
        </ul>
      </section>
      <section className="card card-pad stack" aria-label="Workspaces not healthy">
        <h3>Workspaces not healthy</h3>
        {ill.data.length === 0 && <p className="faint">Every workspace whose health can be read is healthy.</p>}
        <ul className="stack">
          {ill.data.map((w) => (
            <li key={w.id}>
              <Link to={`/e/HostedWorkspace/${w.id}`}>{String(w.slug)}</Link> · {String(w.customer_name)} <Badge tone={tone(String(w.health))}>{String(w.health)}</Badge>
              {w.health_problems ? ` · ${String(w.health_problems)}` : ""}
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
