// A customer's page of the workspace's portal (Ferrum docs/plan/apps-ui.md §9.1) at
// /portal/service/: how each of their workspaces is doing, its cell, coming maintenance and
// incidents, where their recovery requests stand, and their cases, with a case raised. It comes from
// /api/app/control/mine, which answers by the address the person signed in with.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, errorMessage, todayIso, t, isolate } from "@ferrum/client";
import { Badge, Button, ErrorState, Field, ListSkeleton, StatePanel, toast, useShellTitle } from "@ferrum/kit";
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

function Card({ w, today }: { w: Workspace; today: string }) {
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
  return (
    <section className="card card-pad stack" aria-label={w.customer}>
      <header className="row" style={{ justifyContent: "space-between" }}>
        <h3>
          {isolate(w.customer)} <span className="faint">{isolate(w.slug)}</span>
        </h3>
        <Badge tone={tone(w.health)}>{isolate(w.health)}</Badge>
      </header>
      {w.problems && <p>{isolate(w.problems)}</p>}
      <p className="faint">
        {w.version ? t("Ferrum {version}", { version: w.version }) : t("Its version is not known yet")}
        {w.checked_at ? t(", checked {value}", { value: moment(w.checked_at) }) : ""}. {isolate(access(w.support_access_ends, today))}.
      </p>
      {w.cell && (
        <p>
          {t("It runs on {cell} in {region}:", { cell: w.cell.cell, region: w.cell.region })} <Badge tone={tone(w.cell.state)}>{isolate(w.cell.state)}</Badge>
          {w.cell.last_backup ? t(" Last backup {value}.", { value: moment(w.cell.last_backup) }) : t(" No backup reported.")}
          {w.cell.window.from ? t(" Its maintenance window: {value} to {value2}.", { value: moment(w.cell.window.from), value2: moment(w.cell.window.to) }) : ""}
        </p>
      )}
      {w.maintenance.map((m) => (
        <p key={m.release}>
          {t("Ferrum {release} comes in the window", { release: m.release })} {t("{value} to {value2}.", { value: moment(m.from), value2: moment(m.to) })}
        </p>
      ))}
      {w.incidents.map((i) => (
        <div key={i.title} className="stack">
          <strong>
            {isolate(i.title)} <Badge tone={tone(i.status)}>{isolate(i.status)}</Badge>
          </strong>
          <ul className="stack">
            {i.updates.map((u, n) => (
              <li key={n}>
                {isolate(moment(u.at))} · {isolate(u.status)}: {isolate(u.message)}
              </li>
            ))}
          </ul>
        </div>
      ))}
      {w.recovery.length > 0 && (
        <div className="stack">
          <h4>{t("Recovery requests")}</h4>
          <ul className="stack">
            {w.recovery.map((r, n) => (
              <li key={n}>
                {t("{kind} asked {value}", { kind: r.kind, value: moment(r.asked_at) })} <Badge tone={tone(r.status)}>{isolate(r.status)}</Badge>
                {r.note ? ` · ${r.note}` : ""}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="stack">
        <h4>{t("Cases")}</h4>
        <ul className="stack">
          {w.cases.map((c, n) => (
            <li key={n}>
              {t("{subject} · {severity} · due", { subject: c.subject, severity: c.severity })} {isolate(moment(c.due_at))} <Badge tone={tone(c.status)}>{isolate(c.status)}</Badge>
              {c.resolution ? ` · ${c.resolution}` : ""}
            </li>
          ))}
        </ul>
        <form className="stack" onSubmit={(e) => void raise(e)}>
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
          <Button type="submit" variant="primary" loading={busy} disabled={!subject.trim()}>
            {t("Raise a case")}
          </Button>
        </form>
      </div>
    </section>
  );
}

export function ServicePage() {
  useShellTitle(t("Your service"));
  const q = useQuery({ queryKey: ["control", "mine"], queryFn: ({ signal }) => api.get<{ workspaces: Workspace[] }>(`${BASE}/mine`, {}, signal), refetchInterval: 60_000 });
  if (q.error) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <ListSkeleton className="np-page" />;
  const today = todayIso();
  return (
    <div className="stack np-page">
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("Your service")}</h1>
          <p className="page-sub">{t("How your workspaces are doing, coming maintenance, incidents and your cases.")}</p>
        </div>
      </div>
      {q.data.workspaces.length === 0 && <StatePanel kind="empty" title={t("No workspace names you as its contact")} />}
      {q.data.workspaces.map((w) => (
        <Card key={w.slug} w={w} today={today} />
      ))}
    </div>
  );
}
