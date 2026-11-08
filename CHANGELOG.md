# Changelog

## 0.5.9

Rebuilt with Ferrum-Frontend c59c9a2: the names of its models, fields and manifest in Arabic and Hindi (265 each in ui/src/locales/names, which the platform serves to every page of the workspace, so a record's labels, the launcher and the bar read in the person's language), and the sentences its pages still showed in English (split around a link or a figure, built in code from a template or word by word) whole and translated (104 words each in ui/src/locales). Translated by the model, to be reviewed by native speakers.

## 0.5.8

Rebuilt with Ferrum-Frontend 155518c: the sentences its pages built in code (counts, dates, what a toast or a screen reader says) go through t() too, as whole sentences where code chose a word, and carry built-in Arabic and Hindi (95 words each in ui/src/locales, translated by the model and to be reviewed by native speakers).

## 0.5.7

The assistant knows what its records mean in its pages (contributes.shell.assistant): their states, what counts and where figures come from, with questions it suggests asking there.

## 0.5.6

Rebuilt with Ferrum-Frontend 5b22e7b: its own words carry built-in Arabic and Hindi (89 words each, ui/src/locales, translated by the model and to be reviewed by native speakers), fetched for the page's language only. The labels it keeps in objects, its dialogs, its page titles and the words it keeps in variables go through t() as well, and the names and sizes it shows as they are keep their order right to left. Its pages are drawn once their words are in, and its entry carries only the parts of the kit it uses.

## 0.5.5

Rebuilt with Ferrum-Frontend 6537d65: an action's link, code or token shown once is kept in a dialog to copy, and on a page that reads right to left, English left untranslated keeps its own order. Its own words go through the kit's t() (84 places), so a workspace's own words for its language reach them, and Admin, Interface words lists them under it (dist/words.json).

## 0.5.4

Rebuilt with Ferrum-Frontend d5d98f2: a workspace's own words for its interface (Admin, Interface words) reach its pages as they load; an attachment field that holds a picture shows it; and a value another record already has is said so, where the form said the record had been changed by someone else.

## 0.5.3

Rebuilt with Ferrum-Frontend 63280d3: right to left, its pages mirror as they read (icons that point along the line, margins and alignment by the line's start and end, arrow keys along it), and the records pages' words are in Arabic and Hindi.

## 0.5.2

Rebuilt with Ferrum-Frontend a64416a: a new record's box whose default is 1 shows ticked, as it is saved. An action's dialog says every refusal, the errors on what the action made (another record's rows) among them, and an action with parameters and a confirmation asks once.

## 0.5.1

A new incident's Public box is ticked, as the incident is saved: its default is `true`, not `1`,
which a new form showed unticked while saving the incident as public. An operator leaving the box
alone to keep an incident internal would have put it on the status page.

## 0.5.0

A workspace's health counts what its installed apps say of their own parts (FERRUM-IND-021): the
support report's `apps` (Ferrum PR 90), each app's problems named by its package. IoT Edge says how
many of its gateways have gone quiet or report problems, so a partner sees their customers'
gateways in its portfolio, and the customer on their page. A server that does not report it yet
reads as before.

## 0.4.1

Rebuilt with Ferrum-Frontend a76295e: its pages show times on a person's own clock when they chose one on their Account page, while today and due dates stay on the workspace's day. Its pages count today on the workspace's day, not UTC's, so the day's figures are right in the evening west of Greenwich and the early morning east of it.

## 0.4.0

Usage charged beyond a workspace's terms (FERRUM-CLOUD-003).

- **Workspace Usage:** each health reading keeps what the workspace uses that day: its active
  people and its files' storage in GB, the day's last reading standing. This comes from the
  `usage` its support report gives, from the Ferrum release that adds it; an older server says
  nothing of it.
- **Control Plane and ERP 0.2.0:**
  - a workspace's terms gain People Included, Price an Extra Person, Storage Included (GB) and
    Price an Extra GB;
  - each month invoiced adds a line for the people beyond those included at the month's peak, and
    one for each whole GB of storage beyond what is included at its peak;
  - the Hosting Invoice keeps the peaks.

## 0.3.1

Rebuilt with Ferrum-Frontend 24c3951: its pages show times on the workspace's clock, naming the zone when it is not the browser's, a draft numbered on submit is headed as a draft, and a record of an app that is not installed says so.

## 0.3.0

Hosting invoiced through ERP (FERRUM-CLOUD-003).

- **Control Plane and ERP** (`control-erp`, package `control_erp`): a connector that ships with
  the Control Plane and is offered where ERP is installed.
  - Hosting Billing names the company invoicing and the item hosting is invoiced as.
  - A hosted workspace names whom to bill (Billed To), and optionally its first and last days
    billed. Otherwise billing runs from the day it was made on its cell to the day it was closed.
  - Once a month has ended, upkeep invoices each billed, active or closed workspace for its days
    in it, at what it pays a month, as an ERP Sales Invoice left for accounts to issue. A month cut
    short pays its days' share. A Hosting Invoice keeps the month, unique per workspace and month,
    so a month is never invoiced twice.
- **Statements:** a partner's statement counts what each workspace was invoiced for the month
  through ERP. Without an invoice, it counts what an active workspace pays a month, as before.
- **The control plane's day:** months, the day a workspace was made or closed, statements and
  certifications go by its workspace's own day, not UTC's.
- The Control Plane's sidebar shows the connector's Hosting Billing module where it is installed.

## 0.2.1

Rebuilt with Ferrum-Frontend bb60115: a record page reads an empty number or check as the server does, so an action whose condition needs one shows, and a workspace's settings open as their one record.

## 0.2.0

Workspaces made on their cell (FERRUM-CLOUD-001, FERRUM-PARTNER-001).

- A cell has its API address and its bootstrap token, sealed like its report token.
- A hosted workspace names its first administrator. Make It on Its Cell (a fleet operator, while
  it is onboarding) asks the cell's `POST /api/tenants`, with the bootstrap token, for the
  workspace: its slug, its customer's name, its setup's packages and settings, and its
  administrator. The first password is random and kept nowhere; the administrator chooses their
  own. The control plane records when the workspace was made, and makes it once.
- A refusal says why: no administrator, no address or token, the token refused, or what the cell
  said (a package it lacks, a slug taken).
- A partner onboarding a customer with its first administrator gets the workspace made on its
  cell at once. Where the cell cannot make it, the workspace is still recorded, and the answer says
  why, for an operator to make it later.
- Personal-data requests reach a workspace's first administrator.

## 0.1.7

The public status page: anyone opens `/control/status/{workspace}` without signing in (`shell.public`) and reads each region's cells and their state, the incidents told publicly with their updates, newest first, coming maintenance in each cell's window, and what was resolved in the last three days, read again every minute. Its headline is the worst state of any cell, or says how many incidents are open. It draws what `/api/site/{workspace}/control/status` answers, which names no customer, and points programs there. Built with Ferrum-Frontend 01b8ff0 (FERRUM-CLOUD-004).

## 0.1.6

Rebuilt with Ferrum-Frontend 00dea07: in a workflow state that names what may change, the other fields show read-only, and the fields the state needs show required.

## 0.1.5

Personal-data requests find a hosted customer's contact, the support cases they raised and the restores they asked for. Erasure takes their address off the workspace and the cases; a restore request is kept as evidence, and the answer says why (Ferrum's FERRUM-PRIVACY-002).

## 0.1.4

Rebuilt with Ferrum-Frontend daeadda: after an action runs, a record page shows what the action said it did rather than only that it is done.

## 0.1.3

From the browser walk of a recovery request and a partner's statement.

- A workspace without support access says "Support access is not given" (and "has ended" once it
  has), not "Support access ends not given", on the customer's and the partner's pages.
- A second statement for a month is refused as "Northwind Partners already has its statement for
  October 2026", not with a possessive that read "Partners's".
- Rebuilt with Ferrum-Frontend 16b5c57.

## 0.1.2

Rebuilt with Ferrum-Frontend 80ff179: a field's label names its control for assistive technology, and a record whose title is a link shows that record's title, not its id.

## 0.1.1

From a browser walk of the control plane and its portal pages.

- Someone who is no partner's member is told plainly that the partner page is theirs, not shown a failure to try again.
- The console's rollouts name their release by its version, not its id.
- Rebuilt with Ferrum-Frontend's fields that name their controls, so the portal's forms are named for assistive technology.

## 0.1.0

Ferrum Cloud's control plane, apart from the workspaces it runs (FERRUM-CLOUD-001, -004,
FERRUM-PARTNER-001, FERRUM-IND-021).

- Cells reporting their version, digest, configuration, backups, certificate and resources, and
  their health worked out from it.
- Workspaces placed in their region's shared cell with the most room; their health read with the
  customer's health-only support key.
- Releases by digest, qualified by someone other than who recorded them.
- Staged rollouts in maintenance windows, soaking before the next wave, halting on failure,
  customers told when they start.
- Incidents told in updates; the public status.
- Recovery requests decided by someone other than who asked.
- Partners and their tiers, setup templates by revision, onboarding, support cases due by plan,
  and revenue statements.
- Its own interface at `/control/`, and portal pages for partners and customers.
