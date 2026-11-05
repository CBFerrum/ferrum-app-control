import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { violations } from "../../../ferrum/web/src/test/axe";
import { overall, type Status, StatusView } from "./Status";

const cells = (...states: string[]) => [{ region: "eu", cells: states.map((state, n) => ({ cell: `eu-${n + 1}`, state })) }];

const status: Status = {
  regions: [
    { region: "eu", cells: [{ cell: "eu-1", state: "Operational" }, { cell: "eu-2", state: "Degraded" }] },
    { region: "us", cells: [{ cell: "us-1", state: "Operational" }] },
  ],
  incidents: [
    {
      title: "Slow sign-in in eu-2",
      severity: "Minor",
      status: "Monitoring",
      started_at: "2026-11-06T01:00:00Z",
      resolved_at: null,
      cells: ["eu-2"],
      updates: [
        { at: "2026-11-06T01:30:00Z", status: "Monitoring", message: "A fix is out; watching." },
        { at: "2026-11-06T01:00:00Z", status: "Investigating", message: "Sign-in takes 20 s." },
      ],
    },
    { title: "Backups late", severity: "Minor", status: "Resolved", started_at: "2026-11-04T09:00:00Z", resolved_at: "2026-11-04T10:00:00Z", cells: ["us-1"], updates: [] },
  ],
  maintenance: [{ cell: "eu-1", region: "eu", release: "0.5.0", from: "2026-11-08T02:00:00Z", to: "2026-11-08T04:00:00Z" }],
};

describe("the public status page", () => {
  it("says the worst of it first", () => {
    expect(overall({ regions: cells("Operational", "Operational"), incidents: [] })).toEqual({ state: "Operational", says: "All systems operational" });
    expect(overall({ regions: cells("Operational"), incidents: [{ ...status.incidents[0] }] }).says).toBe("All systems operational, with an open incident");
    expect(overall({ regions: cells("Operational"), incidents: [status.incidents[1]] }).says).toBe("All systems operational");
    expect(overall({ regions: cells("Operational", "Degraded"), incidents: [] }).says).toBe("Some systems are degraded");
    expect(overall({ regions: cells("Degraded", "Outage", "Unknown"), incidents: [] })).toEqual({ state: "Outage", says: "Some systems are down" });
    expect(overall({ regions: cells("Operational", "Unknown"), incidents: [] }).says).toBe("Some systems have not reported");
    expect(overall({ regions: [], incidents: [] }).state).toBe("Unknown");
  });

  it("draws regions, incidents with their updates, maintenance and what was resolved", () => {
    const html = renderToString(<StatusView status={status} name="Ferrum Cloud" json="/api/site/cloud/control/status" />);
    // React separates adjacent text with <!-- -->: gone, and tags become spaces.
    const text = html.replace(/<!--.*?-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");
    expect(text).toContain("Ferrum Cloud Service status");
    expect(text).toContain("Some systems are degraded");
    expect(text).toContain("Slow sign-in in eu-2");
    // Newest update first, as the endpoint gives them.
    expect(text.indexOf("A fix is out; watching.")).toBeLessThan(text.indexOf("Sign-in takes 20 s."));
    expect(text).toContain("Ferrum 0.5.0 on eu-1 in eu, in its window 2026-11-08 02:00 UTC to 2026-11-08 04:00 UTC");
    expect(text).toContain("Resolved in the last three days");
    expect(text.indexOf("Backups late")).toBeGreaterThan(text.indexOf("Resolved in the last three days"));
    expect(html).toContain('href="/api/site/cloud/control/status"');
  });

  it("says when nothing is wrong or planned", () => {
    const html = renderToString(<StatusView status={{ regions: cells("Operational"), incidents: [], maintenance: [] }} name="Ferrum Cloud" json="/x" />);
    expect(html).toContain("No open incidents.");
    expect(html).toContain("None planned.");
    expect(html).not.toContain("Resolved in the last three days");
  });

  it("passes axe", async () => {
    // The checker draws each screen inside a <main> of its own.
    const html = renderToString(<StatusView status={status} name="Ferrum Cloud" json="/api/site/cloud/control/status" checked={new Date("2026-11-06T02:00:00Z")} />)
      .replace(/<main /g, "<div ")
      .replace(/<\/main>/g, "</div>");
    expect(await violations(html)).toEqual([]);
  });
});
