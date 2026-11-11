// The control plane's console: cells by region with their health and what is wrong, rollouts in
// progress wave by wave, open incidents, and the workspaces that are not healthy. Each opens its
// record.

import { useQuery } from "@tanstack/react-query";
import { api, docPath, t, isolate } from "@ferrum/client";
import { Badge, ButtonLink, HomeSection, HomeSkeleton, LoadFailed, PageHead, PageStack, Queue, QueueGrid, QueueItem, StatePanel, Tile, TileGrid, useShellTitle } from "@ferrum/kit";
import { tone } from "./words";

export interface Row {
  id: string;
  [k: string]: unknown;
}

const list = (entity: string, filters: unknown[] = [], extra: Record<string, string | number> = {}) =>
  api.get<{ rows: Row[] }>(docPath(entity), { filters: JSON.stringify(filters), limit: 500, ...extra }).then((r) => r.rows);

/** The queues show this many rows each; the rest is "and N more", linking to the list. */
const LIMIT = 5;

/** Whether a rollout needs a person: only one that has halted or failed does; a running one is normal work. */
export function needsPerson(rollouts: Row[]): boolean {
  return rollouts.some((r) => r.status === "Halted" || r.status === "Failed");
}

/** Rows by a field's value, the values in name order. */
export function groupBy(rows: Row[], field: string): [string, Row[]][] {
  const out = new Map<string, Row[]>();
  for (const r of rows) {
    const k = String(r[field] ?? "");
    out.set(k, [...(out.get(k) ?? []), r]);
  }
  return [...out.entries()].sort(([a], [b]) => a.localeCompare(b));
}

/** A rollout's waves, one line: "eu-1: Done · eu-2: Soaking". */
function WaveLine({ id, names, reason }: { id: string; names: Map<string, string>; reason?: string }) {
  const q = useQuery({ queryKey: ["control", "rollout", id], queryFn: () => api.get<Row>(docPath("Rollout", id)) });
  const waves = (q.data?.waves as Row[] | undefined) ?? [];
  const line = [reason, waves.map((w) => `${names.get(String(w.cell)) ?? t("a cell")}: ${String(w.state)}`).join(" · ")].filter(Boolean).join(" · ");
  return <span title={line}>{isolate(line)}</span>;
}

const count = (n: number, one: string, many: string) => (n === 1 ? one : many);

export function Home() {
  useShellTitle(t("Control Plane"));
  const cells = useQuery({ queryKey: ["control", "cells"], queryFn: () => list("Cell", [], { sort: "cell_name asc" }), refetchInterval: 60_000 });
  const rollouts = useQuery({ queryKey: ["control", "rollouts"], queryFn: () => list("Rollout", [["status", "in", ["Running", "Halted"]]], { titles: "true" }) });
  const incidents = useQuery({ queryKey: ["control", "incidents"], queryFn: () => list("ServiceIncident", [["status", "!=", "Resolved"]]) });
  const ill = useQuery({ queryKey: ["control", "ill"], queryFn: () => list("HostedWorkspace", [["health", "in", ["Degraded", "Unreachable"]]]) });
  const failed = [cells, rollouts, incidents, ill].find((q) => q.isError);
  const create = (
    <>
      <ButtonLink to="/e/Cell/new" icon="plus">
        {t("New cell")}
      </ButtonLink>
      <ButtonLink to="/e/Rollout/new" icon="plus">
        {t("New rollout")}
      </ButtonLink>
      <ButtonLink to="/e/ServiceIncident/new" variant="primary" icon="plus">
        {t("New incident")}
      </ButtonLink>
    </>
  );
  if (failed)
    return (
      <PageStack>
        <PageHead title={t("The fleet")} actions={create} />
        <LoadFailed error={failed.error} onRetry={() => void failed.refetch()} />
      </PageStack>
    );
  if (!cells.data || !rollouts.data || !incidents.data || !ill.data)
    return (
      <PageStack>
        <PageHead title={t("The fleet")} />
        <HomeSkeleton queues={3} />
      </PageStack>
    );
  const names = new Map(cells.data.map((c) => [c.id, String(c.cell_name)]));
  const summary = [
    cells.data.length && isolate(count(cells.data.length, t("1 cell"), t("{n} cells", { n: cells.data.length }))),
    incidents.data.length && isolate(count(incidents.data.length, t("1 open incident"), t("{n} open incidents", { n: incidents.data.length }))),
    ill.data.length && isolate(count(ill.data.length, t("1 workspace not healthy"), t("{n} workspaces not healthy", { n: ill.data.length }))),
  ]
    .filter(Boolean)
    .join(" · ");
  const parts = (...p: unknown[]) => isolate(p.filter(Boolean).join(" · "));
  const first = cells.data.length === 0 && rollouts.data.length === 0 && incidents.data.length === 0 && ill.data.length === 0;
  return (
    <PageStack>
      <PageHead title={t("The fleet")} description={summary || undefined} actions={create} />
      {first ? (
        <StatePanel kind="empty" title={t("No cell yet: add the deployments that run customers' workspaces.")} action={<ButtonLink to="/e/Cell/new" variant="primary" icon="plus">{t("New cell")}</ButtonLink>} />
      ) : (
        <>
          <QueueGrid>
            <Queue title={t("Open incidents")} count={incidents.data.length} tone={incidents.data.length ? "attention" : "default"} empty={t("None open.")} more={{ to: "/e/ServiceIncident", count: incidents.data.length - LIMIT }}>
              {incidents.data.slice(0, LIMIT).map((i) => (
                <QueueItem key={i.id} to={`/e/ServiceIncident/${i.id}`} title={isolate(String(i.title))} meta={isolate(String(i.severity ?? ""))} trailing={<Badge tone={tone(String(i.status))}>{isolate(String(i.status))}</Badge>} />
              ))}
            </Queue>
            <Queue title={t("Workspaces not healthy")} count={ill.data.length} tone={ill.data.length ? "attention" : "default"} empty={t("Every workspace whose health can be read is healthy.")} more={{ to: "/e/HostedWorkspace", count: ill.data.length - LIMIT }}>
              {ill.data.slice(0, LIMIT).map((w) => (
                <QueueItem key={w.id} to={`/e/HostedWorkspace/${w.id}`} title={isolate(String(w.slug))} meta={parts(w.customer_name, w.health_problems)} trailing={<Badge tone={tone(String(w.health))}>{isolate(String(w.health))}</Badge>} />
              ))}
            </Queue>
            <Queue title={t("Rollouts")} count={rollouts.data.length} tone={needsPerson(rollouts.data) ? "attention" : "default"} empty={t("None running.")} more={{ to: "/e/Rollout", count: rollouts.data.length - LIMIT }}>
              {rollouts.data.slice(0, LIMIT).map((r) => (
                <QueueItem
                  key={r.id}
                  to={`/e/Rollout/${r.id}`}
                  title={isolate(String((r._titles as Record<string, string> | undefined)?.release ?? r.release))}
                  meta={<WaveLine id={r.id} names={names} reason={r.halted_reason ? String(r.halted_reason) : undefined} />}
                  trailing={<Badge tone={tone(String(r.status))}>{isolate(String(r.status))}</Badge>}
                />
              ))}
            </Queue>
          </QueueGrid>
          {cells.data.length === 0 && <HomeSection title={t("Cells")} empty={t("No cell yet: add the deployments that run customers' workspaces.")} />}
          {groupBy(cells.data, "region").map(([region, rows]) => (
            <HomeSection key={region} title={region ? isolate(region) : t("Unnamed region")} count={rows.length}>
              <TileGrid min="md" label={region}>
                {rows.map((c) => (
                  <Tile
                    key={c.id}
                    to={`/e/Cell/${c.id}`}
                    title={isolate(String(c.cell_name))}
                    meta={
                      <>
                        <span>
                          {isolate(`${t("{value} of {value2}", { value: String(c.placed ?? 0), value2: String(c.capacity) })} ${t("workspaces ·")} ${String(c.reported_version ?? "not reported")}`)}
                        </span>
                        {c.problems ? <span>{isolate(String(c.problems))}</span> : null}
                      </>
                    }
                    progress={Number(c.capacity) > 0 ? { value: Number(c.placed ?? 0), max: Number(c.capacity), label: t("Workspaces placed") } : undefined}
                    status={<Badge tone={tone(String(c.health))}>{isolate(String(c.health))}</Badge>}
                  />
                ))}
              </TileGrid>
            </HomeSection>
          ))}
        </>
      )}
    </PageStack>
  );
}
