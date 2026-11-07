import { t } from "@ferrum/client";
// Words and tones shared by the control plane's pages.

export type Tone = "green" | "amber" | "red" | "neutral";

/** A health's or a state's tone. */
export function tone(state?: string | null): Tone {
  switch (state) {
    case "Healthy":
    case "Operational":
    case "Done":
    case "Resolved":
      return "green";
    case "Degraded":
    case "Upgrading":
    case "Soaking":
    case "Halted":
    case "Monitoring":
    case "Identified":
      return "amber";
    case "Down":
    case "Outage":
    case "Unreachable":
    case "Failed":
    case "Investigating":
      return "red";
    default:
      return "neutral";
  }
}

/** Days from `today` to `day` (both YYYY-MM-DD), or null without a day. */
export function daysUntil(day: string | null | undefined, today: string): number | null {
  if (!day) return null;
  const ms = Date.parse(`${day.slice(0, 10)}T00:00:00Z`) - Date.parse(`${today.slice(0, 10)}T00:00:00Z`);
  return Number.isNaN(ms) ? null : Math.round(ms / 86_400_000);
}

/** When something ends, said plainly: "ended", "today", "in 3 days" or the day itself. */
export function ends(day: string | null | undefined, today: string): string {
  const n = daysUntil(day, today);
  if (n === null) return "not given";
  if (n < 0) return "ended";
  if (n === 0) return "today";
  if (n <= 14) return (n === 1 ? t("in 1 day") : t("in {n} days", { n }));
  return day!.slice(0, 10);
}

/** Where a workspace's support access stands: "Support access is not given", "… has ended", "… ends in 3 days". */
export function access(day: string | null | undefined, today: string): string {
  const when = ends(day, today);
  if (when === "not given") return "Support access is not given";
  if (when === "ended") return "Support access has ended";
  return t("Support access ends {when}", { when });
}

/** A moment as people read it, in UTC. */
export function moment(at?: string | null): string {
  if (!at) return "";
  const d = new Date(at);
  return Number.isNaN(d.getTime()) ? at : t("{value} UTC", { value: d.toISOString().slice(0, 16).replace("T", " ") });
}
