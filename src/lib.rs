//! Ferrum Cloud's control plane (FERRUM-CLOUD-001, -004, FERRUM-PARTNER-001, FERRUM-IND-021), run
//! in a workspace of its own, apart from the cells that run customers' workspaces. It holds where
//! each workspace runs and on what terms, never the workspace's records: a cell's tooling reports
//! how the cell is, and a customer's health-only support key shows how the workspace is. If it is
//! down, the cells and their workspaces go on as they were.
//!
//! * `fleet`: releases by digest, qualified by someone other than who recorded them; cells and
//!   their health from their reports; staged rollouts in maintenance windows, halting on failure.
//! * `hosting`: workspaces placed by capacity, their health read with the customer's key;
//!   incidents told in updates; recovery requests decided by someone other than who asked.
//! * `partners`: partners and their tiers, setup templates, support cases due by plan, and
//!   revenue statements.
//! * `site`: cells' reports and the public status, and the customers' and partners' pages.
//! * `billing`: with ERP (the connector package `control_erp`), each billed workspace invoiced once
//!   a month through ERP, for its days in the month.
//!
//! This app's own design; no code was copied (docs/PROVENANCE.md). Hooks are registered per entity,
//! so a workspace without the `control` package never runs them.

mod billing;
mod fleet;
mod hosting;
mod partners;
mod site;

use chrono::{DateTime, Utc};
use ferrum_domain::reports::ReportSource;
use ferrum_kernel::{Ctx, Doc, Error, HookRegistry, Kernel, Result};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

include!(concat!(env!("OUT_DIR"), "/packages.rs"));
include!(concat!(env!("OUT_DIR"), "/ui.rs"));

/// This app's manifest (`app.yaml`).
pub const MANIFEST: &str = include_str!("../app.yaml");

/// This app's metadata packages, embedded at build time.
pub fn packages() -> std::result::Result<Vec<ferrum_meta::Package>, ferrum_meta::MetaError> {
    ferrum_meta::catalog::load_embedded(EMBEDDED)
}

/// Registers this app's document hooks, and its endpoints.
pub fn register(hooks: &mut HookRegistry) {
    hooks
        .register("Release", Arc::new(fleet::ReleaseHook))
        .register("Cell", Arc::new(fleet::CellHook))
        .register("Rollout", Arc::new(fleet::RolloutHook))
        .register("HostedWorkspace", Arc::new(hosting::WorkspaceHook))
        .register("ServiceIncident", Arc::new(hosting::IncidentHook))
        .register("RecoveryRequest", Arc::new(hosting::RecoveryHook))
        .register("Partner", Arc::new(partners::PartnerHook))
        .register("SetupTemplate", Arc::new(partners::TemplateHook))
        .register("SupportCase", Arc::new(partners::CaseHook));
    hooks.scoped(&[CONNECTOR_PACKAGE.to_string()], |h| {
        h.register("HostedWorkspace", Arc::new(billing::BillingHook));
    });
    hooks.add_site("control", Arc::new(site::ControlSite));
}

/// The connector with ERP's package.
pub const CONNECTOR_PACKAGE: &str = "control_erp";

/// The manifest of the connector app with ERP (`connectors/erp/app.yaml`): its package is
/// `control_erp`, its code this crate's. It ships with the Control Plane.
pub const CONNECTOR_MANIFEST: &str = include_str!("../connectors/erp/app.yaml");

/// This app's reports: none of its own yet.
pub fn reports() -> Vec<Arc<dyn ReportSource>> {
    vec![]
}

/// Upkeep: certifications that lapsed, cells' health, rollouts moved on, workspaces' health read
/// from their cells, and, with ERP, the months ended invoiced.
pub async fn maintenance(k: &Kernel) -> Result<()> {
    maintenance_at(k, Utc::now()).await
}

/// [`maintenance`] as of `now`.
pub async fn maintenance_at(k: &Kernel, now: DateTime<Utc>) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active' order by id").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut first_error: Option<Error> = None;
    for tenant in tenants {
        let run = async {
            let schema = k.registry.schema(tenant).await?;
            if !schema.packages.iter().any(|p| p == "control") {
                return Ok(());
            }
            let ctx = Ctx::system(tenant, schema);
            // `now`'s day where the control plane is (at its zone's offset today).
            let today = now.with_timezone(ctx.now_local().offset()).date_naive();
            let mut tx = k.db.tenant_tx(tenant).await?;
            partners::lapse(k, &ctx, &mut tx, today).await?;
            fleet::refresh_cells(k, &ctx, &mut tx, now).await?;
            fleet::advance(k, &ctx, &mut tx, now).await?;
            tx.commit().await?;
            // Each workspace's cell is asked with no transaction open.
            hosting::read_health(k, &ctx, now).await?;
            if ctx.schema.packages.iter().any(|p| p == CONNECTOR_PACKAGE) {
                let mut tx = k.db.tenant_tx(tenant).await?;
                billing::invoice(k, &ctx, &mut tx, today).await?;
                tx.commit().await?;
            }
            Ok::<(), Error>(())
        };
        if let Err(e) = run.await {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// A document from JSON.
pub(crate) fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap_or_default()
}

/// A moment a record holds.
pub(crate) fn at(doc: &Doc, k: &str) -> Option<DateTime<Utc>> {
    doc.get(k).and_then(Value::as_str).and_then(|v| DateTime::parse_from_rfc3339(v).ok()).map(|d| d.with_timezone(&Utc))
}

/// A whole number a record holds.
pub(crate) fn int(doc: &Doc, k: &str) -> Option<i64> {
    doc.get(k).and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

/// Whether `ctx` runs the fleet.
pub(crate) fn operator(ctx: &Ctx) -> bool {
    ctx.is_admin || ctx.roles.iter().any(|r| r == "Fleet Operator")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interface_is_a_release_the_platform_takes() {
        let m = ferrum_apps::AppManifest::parse(MANIFEST).expect("app.yaml");
        assert_eq!(m.routes(), vec![("control".to_string(), vec![])]);
        let files = UI.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
        let ui = ferrum_apps::ui::prepare("control", files).expect("ui/dist is an interface release");
        assert!(UI.iter().any(|(p, _)| *p == "index.html") && !UI.iter().any(|(p, _)| p.starts_with('.')), "{}", ui.digest);
        let connector = ferrum_apps::AppManifest::parse(CONNECTOR_MANIFEST).expect("connectors/erp/app.yaml");
        assert_eq!(connector.packages, vec![CONNECTOR_PACKAGE.to_string()]);
    }
}
