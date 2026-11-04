# Provenance

This app's design and code are its own. No code was copied from another project.

- Its shape follows common practice for operating a hosted service: a fleet inventory with health
  from reports, placement by capacity, releases by immutable digest, staged rollouts in
  maintenance windows with a soak, incidents told in updates on a status page, and approvals by a
  second person.
- What it reads of a workspace is Ferrum's own support report, through a health-only support key
  the customer gives (Ferrum docs/support-access.md).
