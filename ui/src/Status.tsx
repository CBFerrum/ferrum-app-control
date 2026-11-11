// The public status page at /control/status/{workspace} (`shell.public`): anyone with the address,
// signed in or not, reads how each region's cells are doing, the incidents told publicly with their
// updates, and coming maintenance. It names no customer: it draws what /api/site/{workspace}/control/status
// answers, which is all a status page may say, and reads it again every minute.

import { useQuery } from "@tanstack/react-query";
import { useEffect } from "react";
import { useParams } from "react-router";
import { ApiError, api, bootstrap, t, isolate } from "@ferrum/client";
import { Badge, HomeSection, HomeSkeleton, LoadFailed, PageHead, PageStack, placed, Queue, QueueGrid, QueueItem, StatePanel } from "@ferrum/kit";
import { IncidentCard } from "./Incident";
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
  if (states.length === 0) return { state: "Unknown", says: t("No regions are reported yet") };
  if (worst === "Outage") return { state: worst, says: t("Some systems are down") };
  if (worst === "Degraded") return { state: worst, says: t("Some systems are degraded") };
  if (worst !== "Operational") return { state: worst, says: t("Some systems have not reported") };
  return open ? { state: "Degraded", says: (open === 1 ? t("All systems operational, with an open incident") : t("All systems operational, with {open} open incidents", { open })) } : { state: worst, says: t("All systems operational") };
}

/** The page as drawn: `json` is where programs read the same. */
export function StatusView({ status, name, json, checked }: { status: Status; name: string; json: string; checked?: Date }) {
  const head = overall(status);
  const open = status.incidents.filter((i) => i.status !== "Resolved");
  const past = status.incidents.filter((i) => i.status === "Resolved");
  const incident = (i: Status["incidents"][number]) => <IncidentCard key={`${i.title}-${i.started_at ?? ""}`} i={i} />;
  const bad = (state: string) => state !== "Operational";
  return (
    <main id="main" className="st-page" tabIndex={-1}>
      <PageStack>
        <PageHead eyebrow={isolate(name)} title={t("Service status")} />
        <section className="card card-pad flex flex-wrap items-center justify-between gap-2" aria-label={t("Now")}>
          <strong className="text-xl font-semibold">{isolate(head.says)}</strong>
          <Badge tone={tone(head.state)}>{isolate(head.state)}</Badge>
        </section>

        <HomeSection title={t("Incidents")} count={open.length} empty={t("No open incidents.")}>
          {open.length > 0 && <div className="grid gap-4">{open.map(incident)}</div>}
        </HomeSection>

        <HomeSection title={t("Regions")} empty={t("No regions are reported yet")}>
          {status.regions.length > 0 && (
            <QueueGrid>
              {status.regions.map((r) => (
                <Queue key={r.region} title={r.region || t("Unnamed region")} count={r.cells.length} tone={r.cells.some((c) => bad(c.state) && c.state !== "Unknown") ? "attention" : "default"}>
                  {r.cells.map((c) => (
                    <QueueItem key={c.cell} title={isolate(c.cell)} trailing={<Badge tone={tone(c.state)}>{isolate(c.state)}</Badge>} />
                  ))}
                </Queue>
              ))}
            </QueueGrid>
          )}
        </HomeSection>

        <QueueGrid columns={1}>
          <Queue title={t("Coming maintenance")} count={status.maintenance.length} empty={t("None planned.")}>
            {status.maintenance.map((m, n) => (
              <QueueItem
                key={n}
                title={isolate(m.release ? t("Ferrum {release} on {cell}", { release: m.release, cell: m.cell ?? t("a cell") }) : t("An upgrade on {cell}", { cell: m.cell ?? t("a cell") }))}
                meta={isolate(`${m.region ? t(" in {region}", { region: m.region }) : ""}${m.from ? t(", in its window {value} to {value2}", { value: moment(m.from), value2: moment(m.to) }) : ""}`.trim())}
              />
            ))}
          </Queue>
        </QueueGrid>

        {past.length > 0 && (
          <HomeSection title={t("Resolved in the last three days")} count={past.length}>
            <div className="grid gap-4">{past.map(incident)}</div>
          </HomeSection>
        )}

        <footer className="text-xs text-muted-foreground">
          {placed(checked ? t("Read {value}; read again every minute. Programs read the same at {link}.", { value: moment(checked.toISOString()) }) : t("Programs read the same at {link}."), { link: <a href={json}>{isolate(json)}</a> })}
        </footer>
      </PageStack>
    </main>
  );
}

export function StatusPage() {
  const { tenant = "" } = useParams();
  const json = `/api/site/${encodeURIComponent(tenant)}/control/status`;
  const name = bootstrap()?.workspace.name ?? tenant;
  useEffect(() => {
    document.title = t("Service status · {name}", { name });
  }, [name]);
  const q = useQuery({ queryKey: ["control", "status", tenant], queryFn: ({ signal }) => api.get<Status>(json, undefined, signal), refetchInterval: 60_000 });
  if (q.error) {
    if (q.error instanceof ApiError && q.error.status === 404) return <main id="main" className="st-page" tabIndex={-1}><StatePanel kind="empty" title={t("No status page here")} /></main>;
    return <main id="main" className="st-page" tabIndex={-1}><PageStack><PageHead title={t("Service status")} /><LoadFailed error={q.error} onRetry={() => void q.refetch()} /></PageStack></main>;
  }
  if (!q.data) return <main id="main" className="st-page" tabIndex={-1}><PageStack><PageHead title={t("Service status")} /><HomeSkeleton queues={2} /></PageStack></main>;
  return <StatusView status={q.data} name={name} json={json} checked={new Date(q.dataUpdatedAt)} />;
}
