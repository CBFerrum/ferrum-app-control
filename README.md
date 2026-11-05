# Control Plane

Ferrum Cloud's control plane for [Ferrum](https://github.com/CBFerrum/Ferrum). It runs in a
workspace of its own, apart from the cells that run customers' workspaces:
- the cells and how each is doing, from what its tooling reports;
- releases by digest, rolled out in stages within maintenance windows;
- incidents, a public status, and recovery requests;
- partners: their customers, setups, cases, statements and certification.

It holds where each workspace runs and on what terms, and never a workspace's records. If it is
down, the cells and their workspaces carry on.

| | |
|---|---|
| 🛰️ App | `control`, native, in the server |
| 🧩 Package | `control` |
| 🖥️ Interface | its own, at `/control/`: cells by region, rollouts wave by wave, open incidents, workspaces that are not healthy |
| 🌐 Portal | a partner's customers at `/portal/partner/`, and a customer's service at `/portal/service/` |
| 🔐 Roles | Fleet Operator, Release Manager |
| 📜 Licence | AGPL-3.0-only |

## Highlights

- **Apart from what it runs.** A cell's tooling posts its readings: version, digest,
  configuration fingerprint, last backup, certificate expiry, and processor, memory and disk use.
  A workspace's health is read from its own address with the health-only support key its
  customer's administrator gave (`/api/support/report`): queues and failures, no records. The
  customer sees each of those requests, and ends the access when they choose.
- **Cells' health.** A cell is down when it has not reported for 15 minutes or its certificate has
  expired. It is degraded when it runs another release than it should, its configuration has
  drifted, its last backup is more than 26 hours old, its certificate ends within 14 days, or a
  resource is at 90%.
- **Placement.** A workspace left without a cell goes to its region's shared cell with the most
  room. A full cell, a down one and a dedicated one take none. A closed workspace leaves its room.
- **Releases by digest.** A release is known by its `sha256:` digest, which never changes. A
  Release Manager other than who recorded it qualifies it, and only a qualified release is rolled
  out.
- **Staged rollouts.** A rollout goes to its cells one wave at a time. A wave starts when its
  cell's weekly maintenance window opens, and soaks once the cell runs the release and is
  healthy. It is done when the soak has passed. A cell is in one running rollout at a time. The
  rollout halts when a cell falls ill while soaking, when one is not upgraded within its window, or
  when its release is withdrawn. When it starts, the contacts of the workspaces on its cells are
  told when to expect it.
- **Incidents and status.** Each change of an incident's status is said in an update, and updates
  stay as posted. The public status shows each cell's state by region, open incidents and those
  resolved in the last three days, and coming maintenance. It names no customer.
- **Recovery.** A restore, export or move is decided by an operator other than who asked. Its
  customer sees where it stands.
- **Partners.** A certified or premier tier needs a certification that has not lapsed, and lapses
  to registered when it does. A partner's members see their customers' health, renewals, support
  access ending and open cases. They onboard customers from setup templates, which are versioned:
  a workspace keeps the revision it was set up from. They raise cases, and see their statements.
- **Cases and statements.** A case is due by its workspace's support plan and severity, counted
  from when it was raised, and resolving it says how. A partner's statement is issued once a month
  has ended, once for that month, from what its customers' active workspaces pay.

## A cell's report

```bash
curl -X POST "https://control.example/api/site/<workspace>/control/report" \
  -H "x-control-cell: eu-1" -H "x-control-token: $CELL_TOKEN" -H "content-type: application/json" \
  -d '{"version": "0.4.1", "digest": "sha256:…", "config": "…", "backup_at": "2026-11-04T01:00:00Z",
       "certificate_expires_at": "2027-01-10T00:00:00Z", "cpu": 20, "memory": 35, "disk": 41}'
```

The cell's token is set on its record and kept sealed. A cell reports at most twice a minute.
Polling workspaces on private addresses needs `FERRUM_WEBHOOKS_ALLOW_PRIVATE`.

## Develop

```bash
cargo test                       # needs FERRUM_CONTROL_TEST_DATABASE_URL's database
cd ui && npm run build && npm run check
```

`tests/control_it.rs` runs the control plane in one workspace and a customer's in another. The
customer's is reached over HTTP with its health-only key, answered as the server answers one. It
walks:
- releases recorded and qualified;
- cells reporting, healthy, degraded, down and quiet;
- workspaces placed by room;
- health read, degraded and refused once the key is revoked;
- a rollout through two windows that halts twice, and one that finishes;
- incidents and the public status;
- recovery requests;
- a partner's page, onboarding, cases and statement;
- a customer's own page;
- who may do what.

## Not yet

- Provisioning a cell or a workspace from here: a workspace is recorded, then made by the cell's
  own sign-up or its operator.
- Backups and restores run by the cells' own tooling. A recovery request records the decision,
  not the restore.
- Prices, invoices and payment, and amounts in more than one currency: monthly amounts are
  metadata for statements.
- A status page for the public beyond its JSON.
- Edge gateways' own health in a partner's portfolio, and partner certification by exams or
  trained staff.

## Changelog

| Version | Highlights |
|---|---|
| **0.1.5** | Personal-data requests find customer contacts, their support cases and restore requests. |
| **0.1.4** | Rebuilt with Ferrum-Frontend daeadda: actions say what they did. |
| **0.1.3** | Support access is said plainly when it is not given or has ended, and a second statement for a month is refused in plain words. |
| **0.1.2** | Rebuilt with Ferrum-Frontend 80ff179: labels name their fields, and linked titles show their titles. |
| **0.1.1** | The partner page says plainly when it is not yours, the console's rollouts name their release, and the portal's fields are named. |
| **0.1.0** | Cells and their health, releases by digest, staged rollouts in maintenance windows, incidents and status, recovery requests, and partners' portfolios (FERRUM-CLOUD-001, -004, FERRUM-PARTNER-001, FERRUM-IND-021). |

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
