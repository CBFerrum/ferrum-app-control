// An incident as the control plane tells it, on the public status page and in a customer's service
// page: its title with its severity and state, since when, and its updates, newest first.

import { t, isolate } from "@ferrum/client";
import { Badge } from "@ferrum/kit";
import { moment, tone } from "./words";

export interface IncidentShape {
  title: string;
  severity?: string | null;
  status: string;
  started_at?: string | null;
  resolved_at?: string | null;
  cells?: string[];
  updates: { at?: string | null; status?: string | null; message?: string | null }[];
}

export function IncidentCard({ i }: { i: IncidentShape }) {
  const since = [i.started_at ? `${t("Since")} ${moment(i.started_at)}${i.resolved_at ? t(", resolved {value}", { value: moment(i.resolved_at) }) : ""}` : "", ...(i.cells?.length ? [i.cells.join(", ")] : [])].filter(Boolean).join(" · ");
  return (
    <article className="card card-pad grid gap-3" aria-label={i.title}>
      <header className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-base font-semibold">{isolate(i.title)}</h3>
        <span className="flex gap-2">
          {i.severity && <Badge tone="neutral">{isolate(i.severity)}</Badge>}
          <Badge tone={tone(i.status)}>{isolate(i.status)}</Badge>
        </span>
      </header>
      {since && <p className="text-xs text-muted-foreground">{isolate(since)}</p>}
      {i.updates.length > 0 && (
        <ol className="grid gap-1 text-sm" aria-label={t("Updates on {title}, newest first", { title: i.title })}>
          {i.updates.map((u, n) => (
            <li key={n}>
              <span className="text-muted-foreground">{isolate(moment(u.at))}</span> · <strong>{isolate(u.status ?? "")}</strong>
              {u.message ? `: ${u.message}` : ""}
            </li>
          ))}
        </ol>
      )}
    </article>
  );
}
