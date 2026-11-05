// A customer's page of the workspace's portal (Ferrum docs/plan/apps-ui.md §9.1) at
// /portal/service/: how each of their workspaces is doing, its cell, coming maintenance and
// incidents, where their recovery requests stand, and their cases, with a case raised. It comes from
// /api/app/control/mine, which answers by the address the person signed in with.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, errorMessage } from "@ferrum/client";
import { Badge, Button, EmptyState, ErrorState, Field, PageLoading, toast, useShellTitle } from "@ferrum/kit";
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
      toast("Your case is with support", "success");
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
          {w.customer} <span className="faint">{w.slug}</span>
        </h3>
        <Badge tone={tone(w.health)}>{w.health}</Badge>
      </header>
      {w.problems && <p>{w.problems}</p>}
      <p className="faint">
        {w.version ? `Ferrum ${w.version}` : "Its version is not known yet"}
        {w.checked_at ? `, checked ${moment(w.checked_at)}` : ""}. {access(w.support_access_ends, today)}.
      </p>
      {w.cell && (
        <p>
          It runs on {w.cell.cell} in {w.cell.region}: <Badge tone={tone(w.cell.state)}>{w.cell.state}</Badge>
          {w.cell.last_backup ? ` Last backup ${moment(w.cell.last_backup)}.` : " No backup reported."}
          {w.cell.window.from ? ` Its maintenance window: ${moment(w.cell.window.from)} to ${moment(w.cell.window.to)}.` : ""}
        </p>
      )}
      {w.maintenance.map((m) => (
        <p key={m.release}>
          Ferrum {m.release} comes in the window {moment(m.from)} to {moment(m.to)}.
        </p>
      ))}
      {w.incidents.map((i) => (
        <div key={i.title} className="stack">
          <strong>
            {i.title} <Badge tone={tone(i.status)}>{i.status}</Badge>
          </strong>
          <ul className="stack">
            {i.updates.map((u, n) => (
              <li key={n}>
                {moment(u.at)} · {u.status}: {u.message}
              </li>
            ))}
          </ul>
        </div>
      ))}
      {w.recovery.length > 0 && (
        <div className="stack">
          <h4>Recovery requests</h4>
          <ul className="stack">
            {w.recovery.map((r, n) => (
              <li key={n}>
                {r.kind} asked {moment(r.asked_at)} <Badge tone={tone(r.status)}>{r.status}</Badge>
                {r.note ? ` · ${r.note}` : ""}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="stack">
        <h4>Cases</h4>
        <ul className="stack">
          {w.cases.map((c, n) => (
            <li key={n}>
              {c.subject} · {c.severity} · due {moment(c.due_at)} <Badge tone={tone(c.status)}>{c.status}</Badge>
              {c.resolution ? ` · ${c.resolution}` : ""}
            </li>
          ))}
        </ul>
        <form className="stack" onSubmit={(e) => void raise(e)}>
          <Field label="Subject">
            <input className="input" required maxLength={300} value={subject} onChange={(e) => setSubject(e.target.value)} />
          </Field>
          <Field label="How urgent">
            <select className="input" value={severity} onChange={(e) => setSeverity(e.target.value)}>
              <option>Low</option>
              <option>High</option>
              <option>Critical</option>
            </select>
          </Field>
          <Field label="What happened">
            <textarea className="input" rows={3} value={details} onChange={(e) => setDetails(e.target.value)} />
          </Field>
          <Button type="submit" variant="primary" loading={busy} disabled={!subject.trim()}>
            Raise a case
          </Button>
        </form>
      </div>
    </section>
  );
}

export function ServicePage() {
  useShellTitle("Your service");
  const q = useQuery({ queryKey: ["control", "mine"], queryFn: ({ signal }) => api.get<{ workspaces: Workspace[] }>(`${BASE}/mine`, {}, signal), refetchInterval: 60_000 });
  if (q.error) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <PageLoading />;
  const today = new Date().toISOString().slice(0, 10);
  return (
    <div className="stack np-page">
      <div className="page-head">
        <div>
          <h1 className="page-title">Your service</h1>
          <p className="page-sub">How your workspaces are doing, coming maintenance, incidents and your cases.</p>
        </div>
      </div>
      {q.data.workspaces.length === 0 && <EmptyState title="No workspace names you as its contact" />}
      {q.data.workspaces.map((w) => (
        <Card key={w.slug} w={w} today={today} />
      ))}
    </div>
  );
}
