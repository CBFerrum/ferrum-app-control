# Changelog

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
