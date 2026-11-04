import { describe, expect, it } from "vitest";
import { groupBy } from "./Home";
import { type Customer, needsAttention } from "./Partner";
import { daysUntil, ends, tone } from "./words";

describe("the control plane's words", () => {
  it("tones and ends", () => {
    expect([tone("Healthy"), tone("Degraded"), tone("Unreachable"), tone(undefined)]).toEqual(["green", "amber", "red", "neutral"]);
    expect(daysUntil("2026-11-10", "2026-11-04")).toBe(6);
    expect([ends("2026-11-01", "2026-11-04"), ends("2026-11-04", "2026-11-04"), ends("2026-11-05", "2026-11-04"), ends("2027-01-01", "2026-11-04"), ends(null, "2026-11-04")]).toEqual([
      "ended",
      "today",
      "in 1 day",
      "2027-01-01",
      "not given",
    ]);
  });

  it("cells by region", () => {
    expect(groupBy([{ id: "1", region: "us" }, { id: "2", region: "eu" }, { id: "3", region: "eu" }], "region").map(([r, l]) => [r, l.length])).toEqual([
      ["eu", 2],
      ["us", 1],
    ]);
  });

  it("what needs a partner first", () => {
    const c = (slug: string, more: Partial<Customer>): Customer => ({ slug, customer: slug, status: "Active", health: "Healthy", renewing_soon: false, support_access_ending: false, open_cases: 0, ...more });
    const list = [c("fine", {}), c("renews", { renewing_soon: true }), c("down", { health: "Unreachable" }), c("key", { support_access_ending: true }), c("ill", { health: "Degraded" })];
    expect(needsAttention(list).map((x) => x.slug)).toEqual(["down", "ill", "key", "renews"]);
  });
});
