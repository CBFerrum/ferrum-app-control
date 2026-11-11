// A customer's page of the workspace's portal (Ferrum docs/plan/apps-ui.md §9.1) at
// /portal/service/: how each of their workspaces is doing, its cell, coming maintenance and
// incidents, where their recovery requests stand, and their cases, with a case raised. It comes from
// /api/app/control/mine, which answers by the address the person signed in with.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, errorMessage, todayIso, t, isolate } from "@ferrum/client";
import { Badge, Button, Field, HomeSection, HomeSkeleton, LoadFailed, PageHead, PageStack, Queue, QueueGrid, QueueItem, StatePanel, toast, useShellTitle } from "@ferrum/kit";
import { IncidentCard } from "./Incident";
import { access, moment, tone } from "./words";

const BASE = "/api/app/control";

interface Workspace {
  slug: string;
  customer: string;
  health: string;
  problems?: string | null;
  version?: string | null;
  checked_at?: string | null;
  support_access_ends?: string | null;
  renews_on?: string | null;
  support_plan?: string | null;
  cell?: { cell: string; region: string; state: string; last_backup?: string | null; window: { from?: string | null; to?: string | null } } | null;
  maintenance: { release: string; from?: string | null; to?: string | null }[];
  incidents: { title: string; status: string; severity: string; updates: { at?: string | null; status: string; message: string }[] }[];
  recovery: { kind: string; status: string; asked_at?: string | null; note?: string | null }[];
  cases: { subject: string; severity: string; status: string; due_at?: string | null; resolution?: string | null }[];
}

function WorkspaceSection({ w, today }: { w: Workspace; today: string }) {
  const qc = useQueryClient();
  const [subject, setSubject] = useState("");
  const [severity, setSeverity] = useState("Low");
  const [details, setDetails] = useState("");
  const [busy, setBusy] = useState(false);
  const raise = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post(`${BASE}/case`, { workspace: w.slug, subject: subject.trim(), severity, details: details.trim() });
      await qc.invalidateQueries({ queryKey: ["control", "mine"] });
      setSubject("");
      setDetails("");
      toast(t("Your case is with support"), "success");
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(false);
    }
  };
  const facts = [
    w.version ? t("Ferrum {version}", { version: w.version }) : t("Its version is not known yet"),
    w.checked_at ? t("Checked {value}", { value: moment(w.checked_at) }) : "",
    access(w.support_access_ends, today),
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <HomeSection title={isolate(w.customer)} actions={<Badge tone={tone(w.health)}>{isolate(w.health)}</Badge>}>
      <div className="grid gap-4">
        <p className="text-sm text-muted-foreground">
          {isolate(w.slug)} · {isolate(facts)}
          {w.problems ? <strong className="block font-medium text-foreground">{isolate(w.problems)}</strong> : null}
        </p>
        <QueueGrid>
          {w.cell && (
            <Queue title={t("Cells")} count={1}>
              <QueueItem
                title={isolate(w.cell.cell)}
                meta={isolate(
                  [
                    t(" in {region}", { region: w.cell.region }).trim(),
                    w.cell.last_backup ? t(" Last backup {value}.", { value: moment(w.cell.last_backup) }).trim() : t(" No backup reported.").trim(),
                    w.cell.window.from ? t(" Its maintenance window: {value} to {value2}.", { value: moment(w.cell.window.from), value2: moment(w.cell.window.to) }).trim() : "",
                  ]
                    .filter(Boolean)
                    .join(" "),
                )}
                trailing={<Badge tone={tone(w.cell.state)}>{isolate(w.cell.state)}</Badge>}
              />
            </Queue>
          )}
          <Queue title={t("Coming maintenance")} count={w.maintenance.length} empty={t("None planned.")}>
            {w.maintenance.map((m) => (
              <QueueItem key={m.release} title={isolate(t("Ferrum {release} comes in the window", { release: m.release }))} meta={isolate(t("{value} to {value2}.", { value: moment(m.from), value2: moment(m.to) }))} />
            ))}
          </Queue>
          {w.recovery.length > 0 && (
            <Queue title={t("Recovery requests")} count={w.recovery.length}>
              {w.recovery.map((r, n) => (
                <QueueItem key={n} title={isolate(t("{kind} asked {value}", { kind: r.kind, value: moment(r.asked_at) }))} meta={r.note ? isolate(r.note) : undefined} trailing={<Badge tone={tone(r.status)}>{isolate(r.status)}</Badge>} />
              ))}
            </Queue>
          )}
          <Queue title={t("Cases")} count={w.cases.length}>
            {w.cases.map((c, n) => (
              <QueueItem
                key={n}
                title={isolate(c.subject)}
                meta={isolate([c.severity, t("due {value}", { value: moment(c.due_at) }), c.resolution].filter(Boolean).join(" · "))}
                trailing={<Badge tone={tone(c.status)}>{isolate(c.status)}</Badge>}
              />
            ))}
          </Queue>
        </QueueGrid>
        {w.incidents.map((i) => (
          <IncidentCard key={i.title} i={i} />
        ))}
        <form className="form-card grid max-w-2xl gap-4" onSubmit={(e) => void raise(e)}>
          <h3 className="text-base font-semibold">{t("Raise a case")}</h3>
          <Field label={t("Subject")}>
            <input className="input" required maxLength={300} value={subject} onChange={(e) => setSubject(e.target.value)} />
          </Field>
          <Field label={t("How urgent")}>
            <select className="input" value={severity} onChange={(e) => setSeverity(e.target.value)}>
              <option>{t("Low")}</option>
              <option>{t("High")}</option>
              <option>{t("Critical")}</option>
            </select>
          </Field>
          <Field label={t("What happened")}>
            <textarea className="input" rows={3} value={details} onChange={(e) => setDetails(e.target.value)} />
          </Field>
          <div className="flex justify-end">
            <Button type="submit" variant="primary" loading={busy} disabled={!subject.trim()}>
              {t("Raise a case")}
            </Button>
          </div>
        </form>
      </div>
    </HomeSection>
  );
}

export function ServicePage() {
  useShellTitle(t("Your service"));
  const q = useQuery({ queryKey: ["control", "mine"], queryFn: ({ signal }) => api.get<{ workspaces: Workspace[] }>(`${BASE}/mine`, {}, signal), refetchInterval: 60_000 });
  const head = <PageHead title={t("Your service")} description={t("How your workspaces are doing, coming maintenance, incidents and your cases.")} />;
  if (q.error)
    return (
      <PageStack>
        {head}
        <LoadFailed error={q.error} onRetry={() => void q.refetch()} />
      </PageStack>
    );
  if (!q.data)
    return (
      <PageStack>
        <PageHead title={t("Your service")} />
        <HomeSkeleton queues={2} />
      </PageStack>
    );
  const today = todayIso();
  return (
    <PageStack gap="lg">
      {head}
      {q.data.workspaces.length === 0 && <StatePanel kind="empty" title={t("No workspace names you as its contact")} />}
      {q.data.workspaces.map((w) => (
        <WorkspaceSection key={w.slug} w={w} today={today} />
      ))}
    </PageStack>
  );
}
