# Control Plane

Ferrum Cloud's control plane for [Ferrum](https://github.com/CBFerrum/Ferrum). It runs in a
workspace of its own, apart from the cells that run customers' workspaces:
- the cells and how each is doing, from what its tooling reports;
- releases by digest, rolled out in stages within maintenance windows;
- incidents, a public status, and recovery requests;
- partners: their customers, setups, cases, statements and certification;
- with ERP, each hosted workspace invoiced once a month for its days.

It holds where each workspace runs and on what terms, and never a workspace's records. If it is
down, the cells and their workspaces carry on.

| | |
|---|---|
| 🛰️ App | `control`, native, in the server |
| 🧩 Package | `control`; with ERP, the connector `control-erp` (package `control_erp`) |
| 🖥️ Interface | its own, at `/control/`: cells by region, rollouts wave by wave, open incidents, workspaces that are not healthy |
| 🌐 Portal | a partner's customers at `/portal/partner/`, and a customer's service at `/portal/service/` |
| 🔐 Roles | Fleet Operator, Release Manager |
| 📜 Licence | AGPL-3.0-only |

## Highlights

- **Apart from what it runs.** A cell's tooling posts its readings: version, digest,
  configuration fingerprint, last backup, certificate expiry, and processor, memory and disk use.
  A workspace's health is read from its own address with the health-only support key its
  customer's administrator gave (`/api/support/report`): queues and failures, no records, and what
  each installed app says of its own part, IoT Edge of its gateways among them. Those count in the
  workspace's health and show in partners' and customers' views. The customer sees each of those
  requests, and ends the access when they choose.
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
  resolved in the last three days, and coming maintenance. It names no customer. Anyone reads it at
  `/control/status/{workspace}` without signing in, read again every minute, or as JSON at
  `/api/site/{workspace}/control/status`.
- **Recovery.** A restore, export or move is decided by an operator other than who asked. Its
  customer sees where it stands.
- **Partners.** A certified or premier tier needs a certification that has not lapsed, and lapses
  to registered when it does. A partner's members see their customers' health, renewals, support
  access ending and open cases. They onboard customers from setup templates, which are versioned:
  a workspace keeps the revision it was set up from. They raise cases, and see their statements.
- **Made on its cell.** A workspace is made on its cell from its setup: the cell's
  `POST /api/tenants` with the cell's bootstrap token, the setup's packages and settings, and the
  workspace's first administrator, who chooses their own password. A fleet operator makes it with
  Make It on Its Cell. A partner onboarding a customer with its first administrator gets it made
  at once, or told why the cell could not. It is made once.
- **Cases and statements.** A case is due by its workspace's support plan and severity, counted
  from when it was raised, and resolving it says how. A partner's statement is issued once a month
  has ended, once for that month. It counts what each of its customers' workspaces was invoiced
  for that month through ERP, or else what an active one pays a month.
- **Invoiced through ERP.** With ERP installed, the connector Control Plane and ERP (`control-erp`)
  bills hosted workspaces. Hosting Billing names the company invoicing and the item hosting is
  invoiced as. A workspace names whom to bill (Billed To, an ERP customer), and optionally the
  first and last days billed. Otherwise billing runs from the day it was made on its cell to the
  day it was closed, both by the control plane's own day. Once a month has ended, upkeep invoices
  it for each billed workspace, once, as an ERP Sales Invoice left for accounts to issue:
  - its days in the month at what it pays a month;
  - a month cut short pays its days' share, written "(17 of 31 days)" on the line.

  A Hosting Invoice keeps each month, one per workspace and month.
- **Usage.** Each health reading keeps what a workspace uses that day (Workspace Usage): its active
  people and the storage its files take. Its report says so from Ferrum with the support report's
  usage; an older server says nothing of it. With ERP, a workspace's terms may name the people and
  GB what it pays a month covers, with a price for each extra person and each extra whole GB. Its
  month is then invoiced for its peak beyond them, a line each.

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
- workspaces made on their cell through a stand-in that answers as the server does: refused
  without an administrator, an address, the right token or packages the cell has, made at once
  for a partner's customer, and never twice;
- a customer's own page;
- who may do what.

`tests/billing_it.rs` (`FERRUM_CONTROL_ERP_TEST_DATABASE_URL`) runs the connector with ERP's
records, in a workspace whose day is never UTC's:
- months invoiced once after they end, the first from the day billing starts or the day the
  workspace was made, by the control plane's day;
- none for a workspace with nobody to bill, or not made yet;
- the last month up to the day billing ends, and a workspace closed without one billed up to the
  day it closed;
- a partner's statement counting what was invoiced;
- who may do what.

## Not yet

- Provisioning a cell from here: cells are made by their operators' tooling, and report in.
- Installing a setup's store apps when its workspace is made: its administrator installs them from
  the App Manager. The administrator's first password is chosen with sign-in's code (cells with
  mail).
- Backups and restores run by the cells' own tooling. A recovery request records the decision,
  not the restore.
- Price lists, and payment: a workspace is invoiced what it pays a month and its usage beyond its
  terms, in the invoicing company's currency. A change of terms applies to the months invoiced
  after it, and usage is charged at the month's peak, not prorated. Invoices are paid as ERP takes
  payments, and payment gateways wait for their keys. Without ERP, monthly amounts are only what
  statements count.
- Partner certification by exams or trained staff. Apps' own health shows only once the workspace's
  server reports it (Ferrum with the support report's `apps`).

## Changelog

| Version | Highlights |
|---|---|
| **0.5.7** | The assistant knows its records. |
| **0.5.6** | Rebuilt with Ferrum-Frontend 5b22e7b: its own words in Arabic and Hindi. |
| **0.5.5** | Rebuilt with Ferrum-Frontend 6537d65: once-shown answers kept; English isolated right to left; its own words translatable by a workspace. |
| **0.5.4** | Rebuilt with Ferrum-Frontend d5d98f2: a workspace's own interface words; pictures in attachment fields; a duplicate value said so. |
| **0.5.3** | Rebuilt with Ferrum-Frontend 63280d3: right to left, pages mirror; the records pages' words in Arabic and Hindi. |
| **0.5.2** | Rebuilt with Ferrum-Frontend a64416a: a box whose default is 1 shows ticked. |
| **0.5.1** | A new incident's Public box shows ticked, as it is saved. |
| **0.5.0** | A workspace's health counts what its installed apps say of their own parts, IoT Edge's gateways among them. |
| **0.4.1** | Rebuilt with Ferrum-Frontend a76295e: times on a person's own clock when they chose one. Today is the workspace's day, not UTC's. |
| **0.4.0** | Each workspace's daily usage from its health report; with ERP, people and storage beyond its terms charged at the month's peak. |
| **0.3.1** | Rebuilt with Ferrum-Frontend 24c3951: times on the workspace's clock; drafts headed as drafts. |
| **0.3.0** | With ERP, hosted workspaces invoiced once a month for their days (connector `control-erp`), and partners' statements count what was invoiced; days are the control plane's own. |
| **0.2.1** | Rebuilt with Ferrum-Frontend bb60115: empty numbers read as the server reads them; settings open as their record. |
| **0.2.0** | Workspaces made on their cell from their setup with the cell's bootstrap token, at once when a partner onboards a customer with its first administrator. |
| **0.1.7** | The public status page at `/control/status/{workspace}`: each region's cells, open incidents with their updates, coming maintenance and what was resolved in the last three days, for anyone, signed in or not. |
| **0.1.6** | Rebuilt with Ferrum-Frontend 00dea07: a workflow state's kept fields are read-only, its needed ones required. |
| **0.1.5** | Personal-data requests find customer contacts, their support cases and restore requests. |
| **0.1.4** | Rebuilt with Ferrum-Frontend daeadda: actions say what they did. |
| **0.1.3** | Support access is said plainly when it is not given or has ended, and a second statement for a month is refused in plain words. |
| **0.1.2** | Rebuilt with Ferrum-Frontend 80ff179: labels name their fields, and linked titles show their titles. |
| **0.1.1** | The partner page says plainly when it is not yours, the console's rollouts name their release, and the portal's fields are named. |
| **0.1.0** | Cells and their health, releases by digest, staged rollouts in maintenance windows, incidents and status, recovery requests, and partners' portfolios (FERRUM-CLOUD-001, -004, FERRUM-PARTNER-001, FERRUM-IND-021). |

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
