// A partner's page of the workspace's portal (Ferrum docs/plan/apps-ui.md §9.1) at
// /portal/partner/: its customers' workspaces with their health, renewals, support access and
// cases; a new customer onboarded from a setup template; a case raised; and its statements. All of
// it comes from /api/app/control/partner, which answers by the address the person signed in with.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, ApiError, errorMessage } from "@ferrum/client";
import { Badge, Button, EmptyState, ErrorState, Field, PageLoading, toast, useShellTitle } from "@ferrum/kit";
import { ends, tone } from "./words";

const BASE = "/api/app/control";

export interface Customer {
  slug: string;
  customer: string;
  status: string;
  health: string;
  problems?: string | null;
  plan?: string | null;
  support_plan?: string | null;
  renews_on?: string | null;
  renewing_soon: boolean;
  support_access_ends?: string | null;
  support_access_ending: boolean;
  open_cases: number;
}

interface Portfolio {
  partner: { name: string; tier: string; certified_until?: string | null };
  workspaces: Customer[];
  statements: { month: string; total: string; partner_share: string }[];
  templates: { name: string; revision: number; packages: string[]; apps: string[] }[];
}

/** What needs the partner first: unreachable or degraded workspaces, then renewals and access ending. */
export function needsAttention(list: Customer[]): Customer[] {
  const rank = (c: Customer) => (c.health === "Unreachable" ? 0 : c.health === "Degraded" ? 1 : c.support_access_ending ? 2 : c.renewing_soon ? 3 : 9);
  return list.filter((c) => rank(c) < 9).sort((a, b) => rank(a) - rank(b) || a.slug.localeCompare(b.slug));
}

export function PartnerPage() {
  useShellTitle("Your customers");
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["control", "partner"], queryFn: ({ signal }) => api.get<Portfolio>(`${BASE}/partner`, {}, signal) });
  const [busy, setBusy] = useState<string | null>(null);
  const [form, setForm] = useState({ slug: "", customer_name: "", address: "", region: "", contact_email: "", template: "", plan: "", support_plan: "Standard" });
  const [kase, setKase] = useState({ workspace: "", subject: "", severity: "Low", details: "" });
  const run = async (key: string, f: () => Promise<unknown>, done: string) => {
    setBusy(key);
    try {
      await f();
      await qc.invalidateQueries({ queryKey: ["control", "partner"] });
      toast(done, "success");
      return true;
    } catch (err) {
      toast(errorMessage(err), "error");
      return false;
    } finally {
      setBusy(null);
    }
  };
  // Someone who is no partner's member is told so plainly, not shown a failure to try again.
  if (q.error instanceof ApiError && q.error.status === 403) return <EmptyState title="This page is for a partner's members" />;
  if (q.error) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <PageLoading />;
  const { partner, workspaces, statements, templates } = q.data;
  const today = new Date().toISOString().slice(0, 10);
  const onboard = (e: FormEvent) => {
    e.preventDefault();
    void run("onboard", () => api.post(`${BASE}/partner/onboard`, { ...form, template: form.template || undefined }), `${form.customer_name} is onboarding`).then((ok) => {
      if (ok) setForm({ ...form, slug: "", customer_name: "", address: "", contact_email: "" });
    });
  };
  const raise = (e: FormEvent) => {
    e.preventDefault();
    void run("case", () => api.post(`${BASE}/case`, { ...kase, workspace: kase.workspace || workspaces[0]?.slug }), "The case is with support").then((ok) => {
      if (ok) setKase({ ...kase, subject: "", details: "" });
    });
  };
  const set = (k: keyof typeof form) => (e: { target: { value: string } }) => setForm({ ...form, [k]: e.target.value });
  const attention = needsAttention(workspaces);
  return (
    <div className="stack np-page">
      <div className="page-head">
        <div>
          <h1 className="page-title">{partner.name}</h1>
          <p className="page-sub">
            {partner.tier} partner{partner.certified_until ? `, certified until ${partner.certified_until}` : ""}
          </p>
        </div>
      </div>
      {attention.length > 0 && (
        <section className="card card-pad stack" aria-label="Needs your attention">
          <h3>Needs your attention</h3>
          <ul className="stack">
            {attention.map((c) => (
              <li key={c.slug}>
                <strong>{c.customer}</strong> ·{" "}
                {c.health === "Unreachable" || c.health === "Degraded"
                  ? `${c.health}: ${c.problems ?? ""}`
                  : c.support_access_ending
                    ? `support access ends ${ends(c.support_access_ends, today)}: ask them for a new health key`
                    : `renews ${ends(c.renews_on, today)}`}
              </li>
            ))}
          </ul>
        </section>
      )}
      <section className="card card-pad stack" aria-label="Your customers">
        <h3>Your customers</h3>
        <table className="table">
          <thead>
            <tr>
              <th>Customer</th>
              <th>Health</th>
              <th>Plan</th>
              <th>Renews</th>
              <th>Support access ends</th>
              <th>Open cases</th>
            </tr>
          </thead>
          <tbody>
            {workspaces.map((c) => (
              <tr key={c.slug}>
                <td>
                  {c.customer} <span className="faint">{c.slug}</span>
                </td>
                <td>
                  <Badge tone={tone(c.health)}>{c.health}</Badge>
                </td>
                <td>
                  {c.plan ?? ""} {c.support_plan ? `· ${c.support_plan} support` : ""}
                </td>
                <td>{ends(c.renews_on, today)}</td>
                <td>{ends(c.support_access_ends, today)}</td>
                <td>{c.open_cases}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <section className="card card-pad stack" aria-label="Onboard a customer">
        <h3>Onboard a customer</h3>
        <form className="stack" onSubmit={onboard}>
          <Field label="Customer">
            <input className="input" required value={form.customer_name} onChange={set("customer_name")} />
          </Field>
          <Field label="Workspace name (lower-case letters, digits and hyphens)">
            <input className="input" required pattern="[a-z0-9][a-z0-9-]{1,62}" value={form.slug} onChange={set("slug")} />
          </Field>
          <Field label="Its address">
            <input className="input" type="url" required placeholder="https://" value={form.address} onChange={set("address")} />
          </Field>
          <Field label="Region">
            <input className="input" required value={form.region} onChange={set("region")} />
          </Field>
          <Field label="Their contact">
            <input className="input" type="email" value={form.contact_email} onChange={set("contact_email")} />
          </Field>
          <Field label="Set up from">
            <select className="input" value={form.template} onChange={set("template")}>
              <option value="">Nothing yet</option>
              {templates.map((t) => (
                <option key={t.name} value={t.name}>
                  {t.name} (revision {t.revision}: {t.packages.join(", ")})
                </option>
              ))}
            </select>
          </Field>
          <Field label="Support">
            <select className="input" value={form.support_plan} onChange={set("support_plan")}>
              <option>Standard</option>
              <option>Premium</option>
            </select>
          </Field>
          <Button type="submit" variant="primary" loading={busy === "onboard"}>
            Onboard
          </Button>
        </form>
      </section>
      <section className="card card-pad stack" aria-label="Raise a case">
        <h3>Raise a case</h3>
        <form className="stack" onSubmit={raise}>
          <Field label="For">
            <select className="input" value={kase.workspace || workspaces[0]?.slug || ""} onChange={(e) => setKase({ ...kase, workspace: e.target.value })}>
              {workspaces.map((c) => (
                <option key={c.slug} value={c.slug}>
                  {c.customer}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Subject">
            <input className="input" required maxLength={300} value={kase.subject} onChange={(e) => setKase({ ...kase, subject: e.target.value })} />
          </Field>
          <Field label="How urgent">
            <select className="input" value={kase.severity} onChange={(e) => setKase({ ...kase, severity: e.target.value })}>
              <option>Low</option>
              <option>High</option>
              <option>Critical</option>
            </select>
          </Field>
          <Field label="What happened">
            <textarea className="input" rows={3} value={kase.details} onChange={(e) => setKase({ ...kase, details: e.target.value })} />
          </Field>
          <Button type="submit" variant="primary" loading={busy === "case"} disabled={!workspaces.length || !kase.subject.trim()}>
            Raise it
          </Button>
        </form>
      </section>
      <section className="card card-pad stack" aria-label="Statements">
        <h3>Statements</h3>
        {statements.length === 0 && <p className="faint">None issued yet.</p>}
        <ul className="stack">
          {statements.map((st) => (
            <li key={st.month}>
              {st.month.slice(0, 7)} · your customers paid {st.total} · your share {st.partner_share}
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
