// A partner's page of the workspace's portal (Ferrum docs/plan/apps-ui.md §9.1) at
// /portal/partner/: its customers' workspaces with their health, renewals, support access and
// cases; a new customer onboarded from a setup template; a case raised; and its statements. All of
// it comes from /api/app/control/partner, which answers by the address the person signed in with.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, ApiError, errorMessage, todayIso, t as tr, isolate } from "@ferrum/client";
import { Badge, Button, Field, HomeSection, HomeSkeleton, LoadFailed, PageHead, PageSplit, PageStack, Queue, QueueGrid, QueueItem, StatePanel, toast, useShellTitle } from "@ferrum/kit";
import { access, ends, tone } from "./words";

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
  useShellTitle(tr("Your customers"));
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
  if (q.error instanceof ApiError && q.error.status === 403) return <StatePanel kind="denied" title={tr("This page is for a partner's members")} />;
  if (q.error)
    return (
      <PageStack>
        <PageHead title={tr("Your customers")} />
        <LoadFailed error={q.error} onRetry={() => void q.refetch()} />
      </PageStack>
    );
  if (!q.data)
    return (
      <PageStack>
        <PageHead title={tr("Your customers")} />
        <HomeSkeleton queues={2} />
      </PageStack>
    );
  const { partner, workspaces, statements, templates } = q.data;
  const today = todayIso();
  const onboard = (e: FormEvent) => {
    e.preventDefault();
    void run("onboard", () => api.post(`${BASE}/partner/onboard`, { ...form, template: form.template || undefined }), tr("{customer_name} is onboarding", { customer_name: form.customer_name })).then((ok) => {
      if (ok) setForm({ ...form, slug: "", customer_name: "", address: "", contact_email: "" });
    });
  };
  const raise = (e: FormEvent) => {
    e.preventDefault();
    void run("case", () => api.post(`${BASE}/case`, { ...kase, workspace: kase.workspace || workspaces[0]?.slug }), tr("The case is with support")).then((ok) => {
      if (ok) setKase({ ...kase, subject: "", details: "" });
    });
  };
  const set = (k: keyof typeof form) => (e: { target: { value: string } }) => setForm({ ...form, [k]: e.target.value });
  const attention = needsAttention(workspaces);
  const why = (c: Customer) =>
    c.health === "Unreachable" || c.health === "Degraded"
      ? `${c.health}: ${c.problems ?? ""}`
      : c.support_access_ending
        ? tr("{value}: ask them for a new health key", { value: access(c.support_access_ends, today).toLowerCase() })
        : tr("renews {value}", { value: ends(c.renews_on, today) });
  return (
    <PageStack>
      <PageHead
        title={isolate(partner.name)}
        description={isolate(partner.certified_until ? tr("{tier} partner, certified until {certified_until}", { tier: partner.tier, certified_until: partner.certified_until }) : tr("{tier} partner", { tier: partner.tier }))}
      />
      {attention.length > 0 && (
        <QueueGrid columns={1}>
          <Queue title={tr("Needs your attention")} count={attention.length} tone="attention">
            {attention.map((c) => (
              <QueueItem key={c.slug} title={isolate(c.customer)} meta={isolate(why(c))} trailing={c.health === "Unreachable" || c.health === "Degraded" ? <Badge tone={tone(c.health)}>{isolate(c.health)}</Badge> : undefined} />
            ))}
          </Queue>
        </QueueGrid>
      )}
      <HomeSection title={tr("Your customers")} count={workspaces.length} empty={tr("No customers yet.")}>
        {workspaces.length > 0 && (
          <div className="card">
            <div className="table-wrap" role="region" aria-label={tr("Your customers")} tabIndex={0}>
              <table className="table">
                <thead>
                  <tr>
                    <th scope="col">{tr("Customer")}</th>
                    <th scope="col">{tr("Health")}</th>
                    <th scope="col">{tr("Plan")}</th>
                    <th scope="col">{tr("Renews")}</th>
                    <th scope="col">{tr("Support access ends")}</th>
                    <th scope="col" className="num">
                      {tr("Open cases")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {workspaces.map((c) => (
                    <tr key={c.slug}>
                      <td>
                        {isolate(c.customer)} <span className="faint">{isolate(c.slug)}</span>
                      </td>
                      <td>
                        <Badge tone={tone(c.health)}>{isolate(c.health)}</Badge>
                      </td>
                      <td>
                        {c.plan ?? ""} {c.support_plan ? tr("· {support_plan} support", { support_plan: c.support_plan }) : ""}
                      </td>
                      <td>{isolate(ends(c.renews_on, today))}</td>
                      <td>{isolate(ends(c.support_access_ends, today))}</td>
                      <td className="num">{c.open_cases}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </HomeSection>
      <QueueGrid columns={1}>
        <Queue title={tr("Statements")} count={statements.length} empty={tr("None issued yet.")}>
          {statements.map((st) => (
            <QueueItem
              key={st.month}
              title={isolate(st.month.slice(0, 7))}
              meta={tr("Your customers paid {total}", { total: st.total })}
              trailing={<span className="num">{tr("Your share {partner_share}", { partner_share: st.partner_share })}</span>}
            />
          ))}
        </Queue>
      </QueueGrid>
      <PageSplit
        main={
          <HomeSection title={tr("Onboard a customer")}>
            <form className="form-card" onSubmit={onboard}>
              <div className="form-grid" style={{ ["--cols" as string]: 2 }}>
                <Field label={tr("Customer")}>
                  <input className="input" required value={form.customer_name} onChange={set("customer_name")} />
                </Field>
                <Field label={tr("Workspace name (lower-case letters, digits and hyphens)")}>
                  <input className="input" required pattern="[a-z0-9][a-z0-9-]{1,62}" value={form.slug} onChange={set("slug")} />
                </Field>
                <Field label={tr("Its address")}>
                  <input className="input" type="url" required placeholder="https://" value={form.address} onChange={set("address")} />
                </Field>
                <Field label={tr("Region")}>
                  <input className="input" required value={form.region} onChange={set("region")} />
                </Field>
                <Field label={tr("Their contact")}>
                  <input className="input" type="email" value={form.contact_email} onChange={set("contact_email")} />
                </Field>
                <Field label={tr("Set up from")}>
                  <select className="input" value={form.template} onChange={set("template")}>
                    <option value="">{tr("Nothing yet")}</option>
                    {templates.map((t) => (
                      <option key={t.name} value={t.name}>
                        {tr("{name} (revision {revision}:", { name: t.name, revision: t.revision })} {isolate(t.packages.join(", "))})
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label={tr("Support")}>
                  <select className="input" value={form.support_plan} onChange={set("support_plan")}>
                    <option>{tr("Standard")}</option>
                    <option>{tr("Premium")}</option>
                  </select>
                </Field>
              </div>
              <div className="mt-4 flex justify-end">
                <Button type="submit" variant="primary" loading={busy === "onboard"}>
                  {tr("Onboard")}
                </Button>
              </div>
            </form>
          </HomeSection>
        }
        aside={
          <HomeSection title={tr("Raise a case")}>
            <form className="form-card grid gap-4" onSubmit={raise}>
              <Field label={tr("For")}>
                <select className="input" value={kase.workspace || workspaces[0]?.slug || ""} onChange={(e) => setKase({ ...kase, workspace: e.target.value })}>
                  {workspaces.map((c) => (
                    <option key={c.slug} value={c.slug}>
                      {isolate(c.customer)}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={tr("Subject")}>
                <input className="input" required maxLength={300} value={kase.subject} onChange={(e) => setKase({ ...kase, subject: e.target.value })} />
              </Field>
              <Field label={tr("How urgent")}>
                <select className="input" value={kase.severity} onChange={(e) => setKase({ ...kase, severity: e.target.value })}>
                  <option>{tr("Low")}</option>
                  <option>{tr("High")}</option>
                  <option>{tr("Critical")}</option>
                </select>
              </Field>
              <Field label={tr("What happened")}>
                <textarea className="input" rows={3} value={kase.details} onChange={(e) => setKase({ ...kase, details: e.target.value })} />
              </Field>
              <div className="flex justify-end">
                <Button type="submit" variant="primary" loading={busy === "case"} disabled={!workspaces.length || !kase.subject.trim()}>
                  {tr("Raise it")}
                </Button>
              </div>
            </form>
          </HomeSection>
        }
      />
    </PageStack>
  );
}
