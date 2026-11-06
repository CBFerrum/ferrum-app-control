//! Hosting invoiced through ERP (the Control Plane's connector with ERP, FERRUM-CLOUD-003) against
//! a real PostgreSQL (`FERRUM_CONTROL_ERP_TEST_DATABASE_URL`, default
//! `postgres://ferrum:ferrum@localhost:5432/ferrum_control_erp_test`; reset first), in a workspace
//! whose day is never UTC's: each billed workspace invoiced once a month after the month, its first
//! month for its days from when billing starts (set, or the day it was made on its cell, by the
//! control plane's day) and its last up to the day it ends; a closed one billed up to the day it
//! closed; none without whom to bill or before it is made; and a partner's statement counting what
//! was invoiced.

use chrono::{DateTime, Timelike, Utc};
use ferrum_kernel::secret::SecretKey;
use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, Error, HookRegistry, Kernel, KernelSettings, ListQuery};
use ferrum_meta::Catalog;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

fn obj(v: Value) -> Doc {
    v.as_object().cloned().unwrap()
}

fn uid(v: &Value) -> Uuid {
    v.as_str().unwrap_or_else(|| panic!("not an id: {v}")).parse().unwrap()
}

fn dec(v: &Value) -> f64 {
    v.as_str().map(|s| s.parse().unwrap()).or_else(|| v.as_f64()).unwrap_or_else(|| panic!("not a number: {v}"))
}

fn at(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

struct T {
    k: Arc<Kernel>,
    tenant: Uuid,
    admin: Ctx,
}

impl T {
    async fn insert(&self, ctx: &Ctx, entity: &str, v: Value) -> Doc {
        self.k.insert(ctx, entity, obj(v)).await.unwrap_or_else(|e| panic!("insert {entity}: {e:?}"))
    }
    async fn get(&self, entity: &str, id: Uuid) -> Doc {
        self.k.get(&self.admin, entity, id, false).await.unwrap_or_else(|e| panic!("get {entity}: {e:?}"))
    }
    async fn find(&self, entity: &str, filters: Value) -> Vec<Doc> {
        let q = ListQuery { filters: ferrum_kernel::query::parse_filters(&filters).unwrap(), limit: Some(100), ..Default::default() };
        self.k.list(&self.admin, entity, &q).await.unwrap().rows
    }
    async fn user(&self, email: &str, roles: &[&str]) -> Ctx {
        let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
        let id = self.k.create_user(&self.admin, email, email, Some("Correct-Horse-9"), &roles, false).await.expect("user");
        let s = self.k.login("cloud", email, "Correct-Horse-9").await.expect("login");
        self.k.load_ctx(self.tenant, id, Some(s.token_version), "it".into()).await.expect("ctx")
    }
    /// A workspace's months invoiced: (month, days, amount), in order.
    async fn months(&self, workspace: Uuid) -> Vec<(String, i64, f64)> {
        let mut out: Vec<(String, i64, f64)> = self
            .find("HostingInvoice", json!([["workspace", "=", workspace.to_string()]]))
            .await
            .iter()
            .map(|h| (h["month"].as_str().unwrap().to_string(), h["days"].as_i64().unwrap(), dec(&h["amount"])))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
    async fn upkeep(&self, now: &str) {
        ferrum_app_control::maintenance_at(&self.k, at(now)).await.expect("upkeep");
    }
}

fn refused<V: std::fmt::Debug>(r: Result<V, Error>, kind: &str, says: &str) {
    let msg = format!("{r:?}");
    let ok = matches!((&r, kind), (Err(Error::Invalid(_) | Error::Validation(_)), "invalid") | (Err(Error::Forbidden(_)), "forbidden"));
    assert!(ok && msg.contains(says), "expected {kind} saying {says:?}, got {msg}");
}

#[test]
fn hosting_invoiced() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let url =
        std::env::var("FERRUM_CONTROL_ERP_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_control_erp_test".into());
    let db = Db::connect(DbConfig { url, max_connections: 16, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_erp::packages().expect("ERP's packages"));
    packages.extend(ferrum_app_control::packages().expect("app packages"));
    let catalog = Catalog::from_packages(packages).expect("catalog");
    assert_eq!(catalog.links_out_of_reach(ferrum_app_control::CONNECTOR_PACKAGE), Vec::<String>::new(), "whoever links a record can look it up");
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    ferrum_app_erp::register(&mut hooks);
    ferrum_app_control::register(&mut hooks);
    let settings = KernelSettings { secret_key: Some(SecretKey::new([7u8; 32])), ..Default::default() };
    let k = Kernel::boot(db, catalog, hooks, settings, None).await.expect("boot");
    ferrum_domain::maintenance(&k).await.expect("partitions");
    ferrum_app_erp::maintenance(&k).await.expect("partitions");
    // A day of its own, never UTC's: behind it early in UTC's day, ahead of it later.
    let zone = if Utc::now().hour() < 11 { "Etc/GMT+12" } else { "Etc/GMT-14" };
    let tenant = k
        .create_tenant(NewTenant {
            slug: "cloud".into(),
            name: "Ferrum Cloud".into(),
            admin_email: "admin@cloud.test".into(),
            admin_password: "Correct-Horse-9".into(),
            admin_name: None,
            packages: vec![ferrum_app_control::CONNECTOR_PACKAGE.into()],
            settings: Some(json!({"fiscal_year_start_month": 1, "timezone": zone})),
        })
        .await
        .expect("the operator's workspace");
    let session = k.login("cloud", "admin@cloud.test", "Correct-Horse-9").await.expect("login");
    let admin = k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx");
    let t = T { k: k.clone(), tenant, admin };
    let a = t.admin.clone();
    let op = t.user("op@cloud.test", &["Fleet Operator"]).await;
    let rm = t.user("rm@cloud.test", &["Release Manager"]).await;

    // ───────────── the books, and where hosting is invoiced from ─────────────
    let usd = t.find("Currency", json!([["code", "=", "USD"]])).await.remove(0);
    let company = uid(&t.insert(&a, "Company", json!({"company_name": "Ferrum Cloud", "abbr": "FC", "default_currency": usd["id"]})).await["id"]);
    let services = t.find("ItemGroup", json!([["group_name", "=", "Services"]])).await.remove(0);
    let nos = t.find("Uom", json!([["uom_name", "=", "Nos"]])).await.remove(0);
    let hosting = uid(&t
        .insert(
            &a,
            "Item",
            json!({"item_code": "HOSTING", "item_name": "Hosting", "item_group": services["id"], "stock_uom": nos["id"], "is_stock_item": false}),
        )
        .await["id"]);
    let acme = uid(&t.insert(&a, "Customer", json!({"customer_name": "Acme", "customer_type": "Company"})).await["id"]);
    t.insert(&op, "HostingBilling", json!({"company": company.to_string(), "item": hosting.to_string()})).await;
    t.insert(
        &op,
        "Cell",
        json!({"cell_name": "eu-1", "region": "eu", "capacity": 5, "maintenance_day": "Sunday", "maintenance_hour": 2, "maintenance_hours": 2,
               "desired_config": "cfg-1", "report_token": "eu-1-token-0123456789abcdef0123456789"}),
    )
    .await;
    let workspace = |slug: &str, terms: Value| {
        let mut w = json!({"slug": slug, "customer_name": slug, "address": format!("https://{slug}.example"), "region": "eu", "status": "Active",
                           "monthly_amount": "3100"});
        w.as_object_mut().unwrap().extend(obj(terms));
        w
    };
    let acme_ws = uid(&t.insert(&op, "HostedWorkspace", workspace("acme", json!({"bill_customer": acme.to_string(), "bill_from": "2026-01-15"}))).await["id"]);
    let unbilled = uid(&t.insert(&op, "HostedWorkspace", workspace("unbilled", json!({}))).await["id"]);
    let soon = uid(&t
        .insert(&op, "HostedWorkspace", workspace("soon", json!({"status": "Onboarding", "bill_customer": acme.to_string(), "bill_from": "2026-01-01"})))
        .await["id"]);
    // Made on its cell at 11:30 UTC on 10 February: the 9th or the 11th where the control plane is.
    let made = uid(&t.insert(&op, "HostedWorkspace", workspace("made", json!({"bill_customer": acme.to_string()}))).await["id"]);
    let mut tx = t.k.db.tenant_tx(t.tenant).await.unwrap();
    sqlx::query("update e_hosted_workspace set provisioned_at = $1 where tenant_id = $2 and id = $3")
        .bind(at("2026-02-10T11:30:00Z"))
        .bind(t.tenant)
        .bind(made)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let made_on = if zone == "Etc/GMT+12" { 9 } else { 11 };
    let made_days = 28 - made_on + 1;
    let made_feb = (3100.0 * f64::from(made_days) / 28.0 * 100.0).round() / 100.0;

    // One that pays for 10 people and 5 GB a month, and grew past both in January, as its health
    // reports counted (read in the control plane's upkeep; set here as it would have kept them).
    let growing = uid(&t
        .insert(
            &op,
            "HostedWorkspace",
            workspace(
                "growing",
                json!({"bill_customer": acme.to_string(), "bill_from": "2026-01-01", "monthly_amount": "1000", "bill_people_included": 10,
                       "bill_person_price": "50", "bill_storage_included_gb": "5", "bill_gb_price": "2"}),
            ),
        )
        .await["id"]);
    let system = ferrum_domain::util::elevated(&a);
    for (day, people, gb) in [("2026-01-05", 8, "3.2"), ("2026-01-20", 13, "7.4")] {
        t.insert(&system, "WorkspaceUsage", json!({"workspace": growing.to_string(), "day": day, "people": people, "storage_gb": gb})).await;
    }

    // ───────────── each month after it ends, once ─────────────
    t.upkeep("2026-03-15T12:00:00Z").await;
    assert_eq!(
        t.months(acme_ws).await,
        vec![("2026-01-01".to_string(), 17, 1700.0), ("2026-02-01".to_string(), 28, 3100.0)],
        "17 of January's 31 days, then February"
    );
    assert_eq!(t.months(made).await, vec![("2026-02-01".to_string(), i64::from(made_days), made_feb)], "from the day it was made, by the control plane's day");
    assert!(t.months(unbilled).await.is_empty() && t.months(soon).await.is_empty(), "nobody to bill, or not made yet");
    let january = t.find("HostingInvoice", json!([["workspace", "=", acme_ws.to_string()], ["month", "=", "2026-01-01"]])).await.remove(0);
    let inv = t.get("SalesInvoice", uid(&january["sales_invoice"])).await;
    assert_eq!((inv["customer"].as_str(), inv["docstatus"].as_i64()), (Some(acme.to_string().as_str()), Some(0)), "Acme's, left for accounts to issue");
    let line = &inv["items"][0];
    assert_eq!((line["item"].as_str(), dec(&line["price_list_rate"])), (Some(hosting.to_string().as_str()), 1700.0));
    assert!(line["description"].as_str().unwrap().ends_with("2026-01-15 to 2026-01-31 (17 of 31 days)"), "{line}");
    // Its usage beyond its terms, at January's peak: 3 people and 3 whole GB (of 2.4 over).
    assert_eq!(
        t.months(growing).await,
        vec![("2026-01-01".to_string(), 31, 1156.0), ("2026-02-01".to_string(), 28, 1000.0)],
        "1000, 3 × 50 and 3 × 2; nothing read in February"
    );
    let jan = t.find("HostingInvoice", json!([["workspace", "=", growing.to_string()], ["month", "=", "2026-01-01"]])).await.remove(0);
    assert_eq!((jan["peak_people"].as_i64(), dec(&jan["peak_storage_gb"])), (Some(13), 7.4));
    let inv = t.get("SalesInvoice", uid(&jan["sales_invoice"])).await;
    let lines: Vec<(f64, f64)> = inv["items"].as_array().unwrap().iter().map(|l| (dec(&l["qty"]), dec(&l["price_list_rate"]))).collect();
    assert_eq!(lines, vec![(1.0, 1000.0), (3.0, 50.0), (3.0, 2.0)]);
    t.upkeep("2026-03-20T12:00:00Z").await;
    assert_eq!((t.months(acme_ws).await.len(), t.months(made).await.len()), (2, 1), "a month is invoiced once");

    // ───────────── to the day it ends ─────────────
    refused(t.k.update(&op, "HostedWorkspace", acme_ws, obj(json!({"bill_until": "2026-01-10"})), None).await, "invalid", "on or after the day billing starts");
    t.k.update(&op, "HostedWorkspace", acme_ws, obj(json!({"status": "Closed", "bill_until": "2026-03-10"})), None).await.expect("closed");
    t.upkeep("2026-05-15T12:00:00Z").await;
    assert_eq!(t.months(acme_ws).await.last().unwrap(), &("2026-03-01".to_string(), 10, 1000.0), "up to the day it ends, and nothing after");
    assert_eq!(t.months(acme_ws).await.len(), 3);
    assert_eq!(t.months(made).await[1..], [("2026-03-01".to_string(), 31, 3100.0), ("2026-04-01".to_string(), 30, 3100.0)]);
    // Closed without a day, it is billed up to the day it closed (the control plane's).
    t.k.update(&op, "HostedWorkspace", made, obj(json!({"status": "Closed"})), None).await.expect("closed");
    assert_eq!(t.get("HostedWorkspace", made).await["bill_until"], json!(a.today().to_string()));

    // ───────────── a partner's statement counts what was invoiced ─────────────
    let partner = uid(&t.insert(&op, "Partner", json!({"partner_name": "Northwind Partners", "revenue_share": "20"})).await["id"]);
    for w in [acme_ws, unbilled] {
        t.k.update(&op, "HostedWorkspace", w, obj(json!({"partner": partner.to_string()})), None).await.unwrap();
    }
    let issued = t.k.run_action(&op, "Partner", partner, "issue_statement", json!({"month": "2026-03-15"})).await.expect("March's statement");
    let statement = t.get("RevenueStatement", uid(&issued["statement"])).await;
    let mut lines: Vec<(Uuid, f64, f64)> =
        statement["lines"].as_array().unwrap().iter().map(|l| (uid(&l["workspace"]), dec(&l["amount"]), dec(&l["share"]))).collect();
    lines.sort_by(|x, y| x.1.total_cmp(&y.1));
    assert_eq!(lines, vec![(acme_ws, 1000.0, 200.0), (unbilled, 3100.0, 620.0)], "March as invoiced, closed or not; else its month's terms");
    assert_eq!((dec(&statement["total"]), dec(&statement["partner_share"])), (4100.0, 820.0));

    // ───────────── who may do what ─────────────
    let q = ListQuery::default();
    assert!(t.k.list(&op, "HostingInvoice", &q).await.is_ok(), "an operator sees what was invoiced");
    assert!(t.k.list(&rm, "HostingInvoice", &q).await.is_err(), "a release manager does not");
    refused(t.k.insert(&op, "HostingInvoice", obj(json!({"workspace": acme_ws.to_string(), "month": "2026-06-01"}))).await, "forbidden", "");
}
