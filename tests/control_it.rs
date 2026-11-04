//! Ferrum Cloud's control plane (FERRUM-CLOUD-001, -004, FERRUM-PARTNER-001, FERRUM-IND-021)
//! against a real PostgreSQL (`FERRUM_CONTROL_TEST_DATABASE_URL`, default
//! `postgres://ferrum:ferrum@localhost:5432/ferrum_control_test`; reset first). The operator's
//! workspace runs the control plane; a customer's workspace stands for a cell's, reached over HTTP
//! with the health-only support key its administrator gave, as the server answers one. Releases by
//! digest qualified by someone else; cells' health from their reports; workspaces placed by room;
//! a staged rollout through two maintenance windows that halts when a cell falls ill and when one
//! is not upgraded in time; incidents told in updates; the public status; recovery decided by
//! someone other than who asked; a partner's tier, templates, onboarding, cases and statement; a
//! customer's own page; and who may do what.

use chrono::{DateTime, Datelike, Duration, Utc, Weekday};
use ferrum_kernel::secret::SecretKey;
use ferrum_kernel::support::GrantInput;
use ferrum_kernel::tenant::NewTenant;
use ferrum_kernel::{Ctx, Db, DbConfig, Doc, Error, HookRegistry, Kernel, KernelSettings, ListQuery, SiteRequest};
use ferrum_meta::Catalog;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

const TOKEN_1: &str = "eu-1-token-0123456789abcdef0123456789";
const TOKEN_2: &str = "eu-2-token-0123456789abcdef0123456789";
const TOKEN_US: &str = "us-1-token-0123456789abcdef0123456789";

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
    async fn update(&self, ctx: &Ctx, entity: &str, id: Uuid, v: Value) -> Result<Doc, Error> {
        self.k.update(ctx, entity, id, obj(v), None).await
    }
    async fn user(&self, email: &str, roles: &[&str]) -> Ctx {
        let roles: Vec<String> = roles.iter().map(|r| r.to_string()).collect();
        let id = self.k.create_user(&self.admin, email, email, Some("Correct-Horse-9"), &roles, false).await.expect("user");
        let s = self.k.login("cloud", email, "Correct-Horse-9").await.expect("login");
        self.k.load_ctx(self.tenant, id, Some(s.token_version), "it".into()).await.expect("ctx")
    }
    async fn portal(&self, email: &str) -> Ctx {
        let id = self.k.create_user(&self.admin, email, email, None, &[], false).await.unwrap();
        self.k.set_user_kind(&self.admin, id, "portal").await.unwrap();
        self.k.load_ctx(self.tenant, id, None, "it".into()).await.unwrap()
    }
    async fn act(&self, ctx: &Ctx, entity: &str, id: Uuid, action: &str, params: Value) -> Result<Value, Error> {
        self.k.run_action(ctx, entity, id, action, params).await
    }
    async fn cell(&self, name: &str) -> Doc {
        let rows = self
            .k
            .list(
                &self.admin,
                "Cell",
                &ListQuery { filters: ferrum_kernel::query::parse_filters(&json!([["cell_name", "=", name]])).unwrap(), ..Default::default() },
            )
            .await
            .unwrap()
            .rows;
        self.get("Cell", uid(&rows[0]["id"])).await
    }
    async fn upkeep(&self, at: DateTime<Utc>) {
        ferrum_app_control::maintenance_at(&self.k, at).await.expect("upkeep");
    }
    /// A cell's readings, sent now and dated `at` as if they had come then.
    async fn reading(&self, cell: &str, token: &str, version: &str, digest: &str, at: DateTime<Utc>, disk: i64) {
        let body = json!({"version": version, "digest": digest, "config": "cfg-1", "backup_at": (at - Duration::hours(1)).to_rfc3339(),
                          "certificate_expires_at": (at + Duration::days(60)).to_rfc3339(), "cpu": 20, "memory": 30, "disk": disk});
        site(&self.k, self.tenant, None, "POST", "/report", &[("x-control-cell", cell), ("x-control-token", token)], body).await.expect("a reading");
        let mut tx = self.k.db.tenant_tx(self.tenant).await.unwrap();
        let moved = sqlx::query("update e_cell set reported_at = $1 where tenant_id = $2 and cell_name = $3")
            .bind(at)
            .bind(self.tenant)
            .bind(cell)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(moved.rows_affected(), 1);
        tx.commit().await.unwrap();
    }
}

/// The control plane's endpoint as `who` (or the public).
async fn site(k: &Kernel, tenant: Uuid, who: Option<&Ctx>, method: &str, path: &str, headers: &[(&str, &str)], body: Value) -> Result<Value, Error> {
    let site = k.hooks.site("control").expect("the control plane's endpoints").clone();
    let req = SiteRequest {
        tenant,
        user: who.cloned(),
        method: method.into(),
        path: path.into(),
        query: Default::default(),
        headers: headers.iter().map(|(n, v)| (n.to_string(), json!(v))).collect(),
        body: if body.is_null() { vec![] } else { serde_json::to_vec(&body).unwrap() },
        client_ip: "198.51.100.7".into(),
    };
    let r = site.handle(k, req).await?;
    Ok(serde_json::from_slice(&r.body).unwrap())
}

fn refused<V: std::fmt::Debug>(r: Result<V, Error>, kind: &str, says: &str) {
    let msg = format!("{r:?}");
    let ok = matches!(
        (&r, kind),
        (Err(Error::Invalid(_) | Error::Validation(_)), "invalid")
            | (Err(Error::Forbidden(_)), "forbidden")
            | (Err(Error::Unauthorized(_)), "unauthorized")
            | (Err(Error::NotFound(_)), "not found")
    );
    assert!(ok && msg.contains(says), "expected {kind} saying {says:?}, got {msg}");
}

/// A workspace's front door as far as a support key goes, answered as the server answers it
/// (server/src/extract.rs, controllers/admin.rs): the key, the grant it belongs to, the request on
/// the customer's log, and the report.
async fn data_plane(k: Arc<Kernel>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let k = k.clone();
            tokio::spawn(async move {
                let (mut buf, mut n) = (vec![0u8; 16384], 0);
                while !buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf[n..]).await {
                        Ok(0) | Err(_) => break,
                        Ok(m) => n += m,
                    }
                }
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or_default().to_string();
                let key = head.lines().find_map(|l| {
                    let (name, value) = l.split_once(':')?;
                    name.eq_ignore_ascii_case("authorization").then(|| value.trim().strip_prefix("Bearer ").unwrap_or_default().to_string())
                });
                let (code, body) = answer(&k, &path, key).await;
                let resp = format!("HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

async fn answer(k: &Kernel, path: &str, key: Option<String>) -> (u16, String) {
    let Some(key) = key else { return (401, "{}".into()) };
    let Ok((s, _)) = k.verify_api_key(&key).await else { return (401, "{}".into()) };
    let Ok(ctx) = k.load_ctx(s.tenant, s.user, Some(s.token_version), "it".into()).await else { return (401, "{}".into()) };
    let Ok(Some((grant, level))) = k.support_level(&ctx).await else { return (401, r#"{"error":"this support access has ended"}"#.into()) };
    let refused = path != "/api/support/report";
    k.support_tried(&ctx, grant, "GET", path, refused).await.unwrap();
    if refused {
        return (403, "{}".into());
    }
    (200, k.support_report(&ctx, &level).await.unwrap().to_string())
}

#[test]
fn a_control_plane() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(16 << 20).build().unwrap();
    rt.block_on(async { tokio::spawn(scenario()).await.unwrap() });
}

async fn scenario() {
    let url = std::env::var("FERRUM_CONTROL_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://ferrum:ferrum@localhost:5432/ferrum_control_test".into());
    let db = Db::connect(DbConfig { url, max_connections: 16, ..Default::default() }).await.expect("connect");
    sqlx::raw_sql("drop schema if exists public cascade; create schema public;").execute(&db.pool).await.expect("reset");
    let mut packages = ferrum_domain::packages().expect("platform packages");
    packages.extend(ferrum_app_control::packages().expect("app packages"));
    let catalog = Catalog::from_packages(packages).expect("catalog");
    let mut hooks = HookRegistry::default();
    ferrum_domain::register(&mut hooks);
    ferrum_app_control::register(&mut hooks);
    // The cells answer on this machine in the test.
    let settings = KernelSettings { secret_key: Some(SecretKey::new([7u8; 32])), webhooks_allow_private: true, ..Default::default() };
    let k = Kernel::boot(db, catalog, hooks, settings, None).await.expect("boot");
    ferrum_domain::maintenance(&k).await.expect("partitions");
    let new = |slug: &str, packages: Vec<String>| NewTenant {
        slug: slug.into(),
        name: slug.into(),
        admin_email: format!("admin@{slug}.test"),
        admin_password: "Correct-Horse-9".into(),
        admin_name: None,
        packages,
        settings: Some(json!({"timezone": "UTC"})),
    };
    let tenant = k.create_tenant(new("cloud", vec!["control".into()])).await.expect("the operator's workspace");
    let acme = k.create_tenant(new("acme", vec![])).await.expect("a customer's workspace");
    let session = k.login("cloud", "admin@cloud.test", "Correct-Horse-9").await.expect("login");
    let admin = k.load_ctx(tenant, session.user, Some(session.token_version), "it".into()).await.expect("ctx");
    let s = k.login("acme", "admin@acme.test", "Correct-Horse-9").await.expect("login");
    let acme_admin = k.load_ctx(acme, s.user, Some(s.token_version), "it".into()).await.expect("ctx");
    let t = T { k: k.clone(), tenant, admin };
    let op1 = t.user("op1@cloud.test", &["Fleet Operator"]).await;
    let op2 = t.user("op2@cloud.test", &["Fleet Operator"]).await;
    let rm1 = t.user("rm1@cloud.test", &["Release Manager"]).await;
    let rm2 = t.user("rm2@cloud.test", &["Release Manager"]).await;
    let pat = t.portal("pat@partner.test").await;
    let contact = t.portal("it@acme.test").await;
    let eve = t.portal("eve@other.test").await;
    // A role of the workspace's own that may write these records, without being who decides.
    let rule = |p: Value| serde_json::from_value::<ferrum_meta::PermissionDef>(p).unwrap();
    for e in ["Release", "Rollout", "RecoveryRequest", "Partner"] {
        t.k.set_role_permission(&t.admin, "Fleet Clerk", &rule(json!({"entity": e, "read": true, "write": true}))).await.unwrap();
    }
    let clerk = t.user("clerk@cloud.test", &["Fleet Clerk"]).await;
    let address = data_plane(k.clone()).await;
    let now = Utc::now();
    let today = now.date_naive();

    // ───────────── releases, by digest ─────────────
    refused(t.k.insert(&rm1, "Release", obj(json!({"release_version": "0.4.0", "digest": "sha256:abc"}))).await, "invalid", "64 lower-case hexadecimal");
    let r1 = uid(&t.insert(&rm1, "Release", json!({"release_version": "0.4.0", "digest": digest('a')})).await["id"]);
    refused(t.act(&rm1, "Release", r1, "qualify", Value::Null).await, "forbidden", "someone other than who recorded it");
    refused(t.act(&op1, "Release", r1, "qualify", Value::Null).await, "forbidden", "");
    refused(t.act(&clerk, "Release", r1, "qualify", Value::Null).await, "forbidden", "a Release Manager qualifies a release");
    t.act(&rm2, "Release", r1, "qualify", Value::Null).await.expect("qualified");
    assert_eq!(t.get("Release", r1).await["qualified_by"], "rm2@cloud.test");
    refused(t.update(&rm1, "Release", r1, json!({"digest": digest('b')})).await, "invalid", "never changes");
    let r2 = uid(&t.insert(&rm1, "Release", json!({"release_version": "0.4.1", "digest": digest('b')})).await["id"]);
    t.act(&rm2, "Release", r2, "qualify", Value::Null).await.unwrap();
    let r3 = uid(&t.insert(&rm1, "Release", json!({"release_version": "0.5.0-rc.1", "digest": digest('c')})).await["id"]);
    refused(t.act(&clerk, "Release", r3, "withdraw", Value::Null).await, "forbidden", "a Release Manager withdraws a release");

    // ───────────── cells and what they report ─────────────
    let cell = |name: &str, region: &str, capacity: i64, day: &str, hour: i64, token: &str| {
        json!({"cell_name": name, "region": region, "capacity": capacity, "maintenance_day": day, "maintenance_hour": hour, "maintenance_hours": 2,
               "desired_config": "cfg-1", "report_token": token})
    };
    t.insert(&op1, "Cell", cell("eu-1", "eu", 2, "Sunday", 2, TOKEN_1)).await;
    t.insert(&op1, "Cell", cell("eu-2", "eu", 1, "Monday", 4, TOKEN_2)).await;
    let us1 = uid(&t.insert(&op1, "Cell", cell("us-1", "us", 5, "Sunday", 2, TOKEN_US)).await["id"]);
    let mut dedicated = cell("eu-d", "eu", 10, "Sunday", 2, "eu-d-token-0123456789abcdef0123456789");
    dedicated["cell_kind"] = json!("Dedicated");
    t.insert(&op1, "Cell", dedicated).await;
    let report = |cell: &'static str, token: &'static str, body: Value| {
        let k = k.clone();
        async move { site(&k, tenant, None, "POST", "/report", &[("x-control-cell", cell), ("x-control-token", token)], body).await }
    };
    let fine = |d: &str| {
        json!({"version": "0.4.0", "digest": d, "config": "cfg-1", "backup_at": (now - Duration::hours(1)).to_rfc3339(),
                                "certificate_expires_at": (now + Duration::days(60)).to_rfc3339(), "cpu": 20, "memory": 30, "disk": 40})
    };
    refused(site(&k, tenant, None, "POST", "/report", &[], fine(&digest('a'))).await, "unauthorized", "its name and its token");
    refused(report("eu-1", TOKEN_2, fine(&digest('a'))).await, "forbidden", "not this cell's token");
    refused(report("nowhere", TOKEN_1, fine(&digest('a'))).await, "forbidden", "not this cell's token");
    refused(report("eu-1", TOKEN_1, json!({"digest": "latest"})).await, "invalid", "64 lower-case hexadecimal");
    refused(report("eu-1", TOKEN_1, json!({"digest": digest('a'), "disk": 140})).await, "invalid", "0 to 100");
    assert_eq!(report("eu-1", TOKEN_1, fine(&digest('a'))).await.unwrap()["health"], "Healthy");
    let mut ill = fine(&digest('a'));
    ill["backup_at"] = json!((now - Duration::hours(30)).to_rfc3339());
    ill["disk"] = json!(95);
    ill["config"] = json!("cfg-0");
    let said = report("eu-2", TOKEN_2, ill).await.unwrap();
    assert_eq!(said["health"], "Degraded");
    assert_eq!(said["problems"], "its configuration has drifted; its last backup was 30 hours ago; disk at 95%");
    let mut expired = fine(&digest('a'));
    expired["certificate_expires_at"] = json!((now - Duration::minutes(5)).to_rfc3339());
    let said = report("eu-2", TOKEN_2, expired).await.unwrap();
    assert_eq!((said["health"].as_str(), said["problems"].as_str()), (Some("Down"), Some("its certificate has expired")));
    assert_eq!(t.cell("us-1").await["health"], "Unknown", "it has not reported");
    t.upkeep(Utc::now() + Duration::seconds(20 * 60 + 30)).await;
    let quiet = t.cell("eu-1").await;
    assert_eq!((quiet["health"].as_str(), quiet["problems"].as_str()), (Some("Down"), Some("no report for 20 minutes")));
    let mut expiring = fine(&digest('a'));
    expiring["certificate_expires_at"] = json!((now + Duration::days(5)).to_rfc3339());
    expiring["backup_at"] = Value::Null;
    let said = site(&k, tenant, None, "POST", "/report", &[("x-control-cell", "us-1"), ("x-control-token", TOKEN_US)], expiring).await.unwrap();
    let problems = said["problems"].as_str().unwrap();
    assert!(problems.starts_with("its certificate expires on") && problems.ends_with("; no backup reported"), "{said}");
    report("eu-1", TOKEN_1, fine(&digest('a'))).await.unwrap();
    report("eu-2", TOKEN_2, fine(&digest('a'))).await.unwrap();

    // ───────────── partners and setups ─────────────
    refused(t.k.insert(&op1, "Partner", obj(json!({"partner_name": "Northwind", "tier": "Certified"}))).await, "invalid", "certification that has not lapsed");
    let member = |e: &str| json!({"email": e});
    refused(
        t.k.insert(&op1, "Partner", obj(json!({"partner_name": "Northwind", "members": [member("pat@partner.test"), member("PAT@partner.test")]}))).await,
        "invalid",
        "each member once",
    );
    let partner = json!({"partner_name": "Northwind", "tier": "Certified", "certified_until": (today + Duration::days(30)).to_string(), "revenue_share": 20,
                         "members": [member("pat@partner.test")]});
    let northwind = uid(&t.insert(&op1, "Partner", partner).await["id"]);
    let other = uid(&t.insert(&op1, "Partner", json!({"partner_name": "Contoso"})).await["id"]);
    refused(t.k.insert(&op1, "SetupTemplate", obj(json!({"template_name": "Empty", "packages": "\n"}))).await, "invalid", "Packages is required");
    refused(t.k.insert(&op1, "SetupTemplate", obj(json!({"template_name": "Bad", "packages": "CRM!"}))).await, "invalid", "'CRM!' is not a name");
    let retail =
        uid(&t.insert(&op1, "SetupTemplate", json!({"template_name": "Retail starter", "packages": "crm\npos", "partner": northwind.to_string()})).await["id"]);
    t.insert(&op1, "SetupTemplate", json!({"template_name": "Contoso pack", "packages": "crm", "partner": other.to_string()})).await;
    t.update(&op1, "SetupTemplate", retail, json!({"template_name": "Retail starter"})).await.unwrap();
    assert_eq!(t.get("SetupTemplate", retail).await["revision"], 1, "its name is not what it installs");
    t.update(&op1, "SetupTemplate", retail, json!({"packages": "crm\npos\nhelpdesk"})).await.unwrap();
    assert_eq!(t.get("SetupTemplate", retail).await["revision"], 2);

    // ───────────── workspaces placed by room ─────────────
    let ws = |slug: &str, region: &str| json!({"slug": slug, "customer_name": slug.to_uppercase(), "address": format!("https://{slug}.eu.example"), "region": region});
    refused(
        t.k.insert(&op1, "HostedWorkspace", obj(json!({"slug": "x", "customer_name": "X", "address": "https://x.example", "region": "eu"}))).await,
        "invalid",
        "lower-case letters",
    );
    refused(
        t.k.insert(&op1, "HostedWorkspace", obj(json!({"slug": "xx", "customer_name": "X", "address": "http://x.example", "region": "eu"}))).await,
        "invalid",
        "https://",
    );
    let mut first = ws("acme", "eu");
    first["address"] = json!(address);
    first["contact_email"] = json!("IT@acme.test");
    first["partner"] = json!(northwind.to_string());
    first["support_plan"] = json!("Premium");
    first["monthly_amount"] = json!("400");
    first["term_end"] = json!((today + Duration::days(30)).to_string());
    first["status"] = json!("Active");
    first["template"] = json!(retail.to_string());
    let acme_ws = t.insert(&op1, "HostedWorkspace", first).await;
    let acme_id = uid(&acme_ws["id"]);
    let eu1 = uid(&t.cell("eu-1").await["id"]);
    let eu2 = uid(&t.cell("eu-2").await["id"]);
    assert_eq!((uid(&acme_ws["cell"]), acme_ws["contact_email"].as_str()), (eu1, Some("it@acme.test")), "the cell with the most room");
    assert_eq!(t.get("HostedWorkspace", acme_id).await["template_revision"], 2, "the setup as it stood");
    let mut beta = ws("beta", "eu");
    beta["partner"] = json!(northwind.to_string());
    beta["monthly_amount"] = json!("100");
    beta["status"] = json!("Active");
    beta["contact_email"] = json!("ops@beta.test");
    let beta = t.insert(&op1, "HostedWorkspace", beta).await;
    assert_eq!(uid(&beta["cell"]), eu1, "a tie goes by name, and a dedicated cell is no one else's");
    let mut gamma = ws("gamma", "eu");
    gamma["partner"] = json!(other.to_string());
    let gamma = t.insert(&op1, "HostedWorkspace", gamma).await;
    assert_eq!(uid(&gamma["cell"]), eu2);
    refused(t.k.insert(&op1, "HostedWorkspace", obj(ws("delta", "eu"))).await, "invalid", "no shared cell in eu has room");
    let mut pinned = ws("delta", "eu");
    pinned["cell"] = json!(eu1.to_string());
    refused(t.k.insert(&op1, "HostedWorkspace", obj(pinned.clone())).await, "invalid", "eu-1 is full");
    pinned["region"] = json!("us");
    pinned["cell"] = json!(eu2.to_string());
    refused(t.k.insert(&op1, "HostedWorkspace", obj(pinned)).await, "invalid", "eu-2 is in eu, not us");
    assert_eq!(t.cell("eu-1").await["placed"], 2);
    refused(t.update(&op1, "Cell", eu1, json!({"capacity": 1})).await, "invalid", "it runs 2 workspaces");
    t.update(&op1, "HostedWorkspace", uid(&beta["id"]), json!({"status": "Closed"})).await.unwrap();
    let mut delta = ws("delta", "eu");
    delta["partner"] = json!(northwind.to_string());
    delta["monthly_amount"] = json!("100");
    delta["status"] = json!("Active");
    delta["contact_email"] = json!("ops@delta.test");
    assert_eq!(uid(&t.insert(&op1, "HostedWorkspace", delta).await["cell"]), eu1, "a closed workspace leaves its room");

    // ───────────── health, read with the customer's key ─────────────
    let grant = GrantInput {
        level: "health".into(),
        grantee: "Ferrum Cloud".into(),
        entities: vec![],
        can_write: false,
        reason: "Health checks".into(),
        hours: 24 * 7,
    };
    let (grant_id, key) = k.grant_support(&acme_admin, grant).await.expect("a health-only key");
    t.update(&op1, "HostedWorkspace", acme_id, json!({"health_key": key, "health_key_expires": (today + Duration::days(7)).to_string()})).await.unwrap();
    t.upkeep(Utc::now()).await;
    let read = t.get("HostedWorkspace", acme_id).await;
    assert_eq!((read["health"].as_str(), read["health_problems"].as_str()), (Some("Healthy"), None), "{read:?}");
    assert!(read["running_version"].as_str().is_some_and(|v| !v.is_empty()));
    assert_eq!(t.get("HostedWorkspace", uid(&gamma["id"])).await["health_problems"], "no support key to read its health with");
    let asked = k.support_activity(&acme_admin, grant_id).await.unwrap();
    assert!(asked.iter().any(|r| r.to_string().contains("/api/support/report")), "the customer sees each request: {asked:?}");
    let mut tx = k.db.tenant_tx(acme).await.unwrap();
    sqlx::query("insert into outbox (tenant_id, topic, payload, dead) values ($1, 'test.lost', '{}', true)").bind(acme).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    t.upkeep(Utc::now()).await;
    let read = t.get("HostedWorkspace", acme_id).await;
    assert_eq!((read["health"].as_str(), read["health_problems"].as_str()), (Some("Degraded"), Some("1 events could not be delivered")));
    let mut tx = k.db.tenant_tx(acme).await.unwrap();
    sqlx::query("insert into job (tenant_id, id, kind, status) values ($1, gen_random_uuid(), 'test.job', 'failed')")
        .bind(acme)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("insert into outbox (tenant_id, topic, payload) select $1, 'test.wait', '{}' from generate_series(1, 1001)")
        .bind(acme)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    t.upkeep(Utc::now()).await;
    let problems = t.get("HostedWorkspace", acme_id).await["health_problems"].as_str().unwrap().to_string();
    assert!(problems.starts_with("1 events could not be delivered; 1 jobs failed; ") && problems.ends_with(" events are waiting"), "{problems}");
    k.revoke_support(&acme_admin, grant_id).await.unwrap();
    t.upkeep(Utc::now()).await;
    let read = t.get("HostedWorkspace", acme_id).await;
    assert_eq!(read["health"], "Unreachable");
    assert!(read["health_problems"].as_str().unwrap().contains("support key was refused"), "{read:?}");

    // ───────────── a staged rollout ─────────────
    let mut sunday = today + Duration::days(1);
    while sunday.weekday() != Weekday::Sun {
        sunday += Duration::days(1);
    }
    let t0 = sunday.and_hms_opt(2, 30, 0).unwrap().and_utc();
    let monday = |h: u32, m: u32| (sunday + Duration::days(1)).and_hms_opt(h, m, 0).unwrap().and_utc();
    t.reading("eu-1", TOKEN_1, "0.4.0", &digest('a'), t0 - Duration::minutes(1), 40).await;
    t.reading("eu-2", TOKEN_2, "0.4.0", &digest('a'), t0 - Duration::minutes(1), 40).await;
    let wave = |c: Uuid| json!({"cell": c.to_string()});
    refused(
        t.k.insert(&op1, "Rollout", obj(json!({"release": r2.to_string(), "waves": [wave(eu1), wave(eu1)]}))).await,
        "invalid",
        "a cell is in a rollout once",
    );
    let early = uid(&t.insert(&op1, "Rollout", json!({"release": r3.to_string(), "waves": [wave(eu2)]})).await["id"]);
    refused(t.act(&op1, "Rollout", early, "start", Value::Null).await, "invalid", "only a qualified release");
    let rollout = uid(&t.insert(&op1, "Rollout", json!({"release": r2.to_string(), "waves": [wave(eu1), wave(eu2)]})).await["id"]);
    refused(t.act(&rm1, "Rollout", rollout, "start", Value::Null).await, "forbidden", "");
    refused(t.act(&clerk, "Rollout", rollout, "start", Value::Null).await, "forbidden", "a Fleet Operator runs rollouts");
    t.act(&op1, "Rollout", rollout, "start", Value::Null).await.expect("started");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    let (told, all_told): (i64, i64) = sqlx::query_as(
        "select count(*) filter (where payload->>'to' = 'it@acme.test'), count(*) from outbox where tenant_id = $1 and topic = 'ferrum.mail' and payload->>'subject' like '%0.4.1%'",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!((told, all_told), (1, 2), "acme's and delta's contacts are told; beta is closed and gamma has none");
    let second = uid(&t.insert(&op1, "Rollout", json!({"release": r1.to_string(), "waves": [wave(eu2)]})).await["id"]);
    refused(t.act(&op1, "Rollout", second, "start", Value::Null).await, "invalid", "eu-2 is in another running rollout");
    let states = |r: &Doc| r["waves"].as_array().unwrap().iter().map(|w| w["state"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>();
    t.upkeep(t0 - Duration::hours(3)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Pending", "Pending"], "outside its window");
    t.upkeep(t0).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Upgrading", "Pending"]);
    let upgrading = t.cell("eu-1").await;
    assert_eq!((uid(&upgrading["desired_release"]), upgrading["health"].as_str()), (r2, Some("Degraded")), "{upgrading:?}");
    assert_eq!(upgrading["problems"], "it runs 0.4.0, not the release it should");
    let public = site(&k, tenant, None, "GET", "/status", &[], Value::Null).await.unwrap();
    let coming: Vec<&str> = public["maintenance"].as_array().unwrap().iter().map(|m| m["cell"].as_str().unwrap()).collect();
    assert_eq!(coming, ["eu-1", "eu-2"]);
    t.reading("eu-1", TOKEN_1, "0.4.1", &digest('b'), t0 + Duration::minutes(10), 40).await;
    t.upkeep(t0 + Duration::minutes(15)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Soaking", "Pending"]);
    t.reading("eu-1", TOKEN_1, "0.4.1", &digest('b'), t0 + Duration::hours(23), 40).await;
    t.upkeep(t0 + Duration::hours(23) + Duration::minutes(5)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Soaking", "Pending"], "the soak lasts 24 hours");
    t.reading("eu-1", TOKEN_1, "0.4.1", &digest('b'), monday(2, 45), 40).await;
    t.reading("eu-2", TOKEN_2, "0.4.0", &digest('a'), monday(2, 45), 40).await;
    t.upkeep(monday(2, 50)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Done", "Pending"], "eu-2's window opens at 04:00");
    let public = site(&k, tenant, None, "GET", "/status", &[], Value::Null).await.unwrap();
    assert_eq!(
        public["maintenance"].as_array().unwrap().iter().map(|m| m["cell"].as_str().unwrap()).collect::<Vec<_>>(),
        ["eu-2"],
        "done waves are not coming"
    );
    t.reading("eu-2", TOKEN_2, "0.4.0", &digest('a'), monday(4, 25), 40).await;
    t.upkeep(monday(4, 30)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Done", "Upgrading"]);
    t.reading("eu-2", TOKEN_2, "0.4.1", &digest('b'), monday(4, 35), 40).await;
    t.upkeep(monday(4, 40)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Done", "Soaking"]);
    t.reading("eu-2", TOKEN_2, "0.4.1", &digest('b'), monday(5, 0), 95).await;
    t.upkeep(monday(5, 5)).await;
    let halted = t.get("Rollout", rollout).await;
    assert_eq!(
        (states(&halted), halted["status"].as_str(), halted["halted_reason"].as_str()),
        (vec!["Done".into(), "Failed".into()], Some("Halted"), Some("eu-2 while soaking: disk at 95%"))
    );
    t.act(&op1, "Rollout", rollout, "resume", Value::Null).await.expect("resumed");
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Done", "Pending"]);
    t.upkeep(monday(5, 10)).await;
    assert_eq!(states(&t.get("Rollout", rollout).await), ["Done", "Upgrading"], "its window is still open");
    t.reading("eu-2", TOKEN_2, "0.4.1", &digest('b'), monday(6, 15), 95).await;
    t.upkeep(monday(6, 20)).await;
    let late = t.get("Rollout", rollout).await;
    assert_eq!((late["status"].as_str(), late["halted_reason"].as_str()), (Some("Halted"), Some("eu-2 was not upgraded within its maintenance window")));
    t.act(&op1, "Rollout", rollout, "resume", Value::Null).await.unwrap();
    refused(t.act(&op1, "Rollout", rollout, "halt", json!({"reason": " "})).await, "invalid", "halting a rollout says why");
    t.act(&rm2, "Release", r2, "withdraw", Value::Null).await.unwrap();
    t.upkeep(monday(6, 30)).await;
    assert_eq!(t.get("Rollout", rollout).await["halted_reason"], "its release was withdrawn");

    // ───────────── incidents, told in updates ─────────────
    let update = |status: &str, message: &str| json!({"status": status, "message": message});
    let incident = |updates: Vec<Value>| json!({"title": "Slow pages on eu-1", "severity": "Major", "status": "Investigating", "cells": [{"cell": eu1.to_string()}], "updates": updates});
    refused(
        t.k.insert(&op1, "ServiceIncident", obj(incident(vec![update("Identified", "A disk")]))).await,
        "invalid",
        "the latest update says the incident is Investigating",
    );
    let inc = t.insert(&op1, "ServiceIncident", incident(vec![update("Investigating", "Pages are slow; we are looking")])).await;
    let inc_id = uid(&inc["id"]);
    assert!(inc["started_at"].is_string() && inc["updates"][0]["posted_at"].is_string());
    let mut kept = inc["updates"].as_array().unwrap().clone();
    let first_update = kept[0].clone();
    kept.push(update("Identified", "A full disk on one node"));
    t.update(&op1, "ServiceIncident", inc_id, json!({"status": "Identified", "updates": kept.clone()})).await.unwrap();
    let mut edited = t.get("ServiceIncident", inc_id).await["updates"].as_array().unwrap().clone();
    edited[0]["message"] = json!("Nothing was wrong");
    refused(t.update(&op1, "ServiceIncident", inc_id, json!({"updates": edited})).await, "invalid", "stays as it was posted");
    let mut reordered = t.get("ServiceIncident", inc_id).await["updates"].as_array().unwrap().clone();
    reordered.reverse();
    refused(t.update(&op1, "ServiceIncident", inc_id, json!({"status": "Investigating", "updates": reordered})).await, "invalid", "said in a new update");
    let mut resolved = t.get("ServiceIncident", inc_id).await["updates"].as_array().unwrap().clone();
    resolved.push(update("Resolved", "The disk was replaced"));
    t.update(&op1, "ServiceIncident", inc_id, json!({"status": "Resolved", "updates": resolved})).await.unwrap();
    assert!(t.get("ServiceIncident", inc_id).await["resolved_at"].is_string());
    assert_eq!(first_update["status"], "Investigating");
    let mut hidden = incident(vec![update("Investigating", "Internal only")]);
    hidden["title"] = json!("A quiet one");
    hidden["public"] = json!(false);
    t.insert(&op1, "ServiceIncident", hidden).await;
    let mut elsewhere = incident(vec![update("Investigating", "Sign-in is slow")]);
    elsewhere["title"] = json!("Slow sign-in on eu-2");
    elsewhere["cells"] = json!([{"cell": eu2.to_string()}]);
    t.insert(&op1, "ServiceIncident", elsewhere).await;
    let public = site(&k, tenant, None, "GET", "/status", &[], Value::Null).await.unwrap();
    let shown: Vec<&str> = public["incidents"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert_eq!(shown, ["Slow sign-in on eu-2", "Slow pages on eu-1"], "the public ones only, newest first");
    assert_eq!(public["incidents"][1]["updates"][0]["message"], "The disk was replaced", "its updates newest first");
    let regions: Vec<&str> = public["regions"].as_array().unwrap().iter().map(|r| r["region"].as_str().unwrap()).collect();
    assert_eq!(regions, ["eu", "us"]);
    assert!(!public.to_string().contains("ACME") && !public.to_string().contains("acme"), "the status names no customer");

    // ───────────── recovery, decided by someone else ─────────────
    let restore = |to: DateTime<Utc>| json!({"workspace": acme_id.to_string(), "recovery_kind": "Restore", "restore_to": to.to_rfc3339(), "reason": "Invoices deleted by mistake"});
    refused(t.k.insert(&op1, "RecoveryRequest", obj(restore(Utc::now() + Duration::hours(1)))).await, "invalid", "a moment that has passed");
    let rec = uid(&t.insert(&op1, "RecoveryRequest", restore(Utc::now() - Duration::hours(2))).await["id"]);
    refused(t.act(&op1, "RecoveryRequest", rec, "approve", Value::Null).await, "forbidden", "someone other than who asked");
    refused(t.act(&clerk, "RecoveryRequest", rec, "approve", Value::Null).await, "forbidden", "a Fleet Operator decides recovery requests");
    refused(t.act(&op2, "RecoveryRequest", rec, "refuse", json!({})).await, "invalid", "says why");
    t.act(&op2, "RecoveryRequest", rec, "approve", Value::Null).await.unwrap();
    t.act(&op2, "RecoveryRequest", rec, "complete", Value::Null).await.unwrap();
    assert_eq!(t.get("RecoveryRequest", rec).await["decided_by"], "op2@cloud.test");

    // ───────────── a partner's page ─────────────
    let page = site(&k, tenant, Some(&pat), "GET", "/partner", &[], Value::Null).await.unwrap();
    assert_eq!((page["partner"]["tier"].as_str(), page["workspaces"].as_array().unwrap().len()), (Some("Certified"), 3), "acme, delta, and beta though closed");
    let acme_row = page["workspaces"].as_array().unwrap().iter().find(|w| w["slug"] == "acme").unwrap().clone();
    assert_eq!(
        (acme_row["health"].as_str(), acme_row["renewing_soon"].as_bool(), acme_row["support_access_ending"].as_bool()),
        (Some("Unreachable"), Some(true), Some(true))
    );
    let templates: Vec<&str> = page["templates"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(templates, ["Retail starter"], "its own setups and shared ones, not another partner's");
    refused(site(&k, tenant, Some(&eve), "GET", "/partner", &[], Value::Null).await, "forbidden", "a partner's members");
    refused(site(&k, tenant, None, "GET", "/partner", &[], Value::Null).await, "unauthorized", "sign in first");
    let quiet_us = t.cell("us-1").await;
    assert_eq!(quiet_us["health"], "Down", "quiet since the upkeep days ahead");
    refused(
        t.k.insert(&op1, "HostedWorkspace", obj(json!({"slug": "early", "customer_name": "Early", "address": "https://early.us.example", "region": "us"})))
            .await,
        "invalid",
        "no shared cell in us has room",
    );
    site(&k, tenant, None, "POST", "/report", &[("x-control-cell", "us-1"), ("x-control-token", TOKEN_US)], fine(&digest('a'))).await.unwrap();
    let zeta = json!({"slug": "zeta", "customer_name": "Zeta Ltd", "address": "https://zeta.us.example", "region": "us", "contact_email": "ops@zeta.test", "template": "Retail starter", "monthly_amount": "50"});
    let made = site(&k, tenant, Some(&pat), "POST", "/partner/onboard", &[], zeta).await.expect("onboarded");
    assert_eq!(made["cell"], "us-1");
    let mut theirs = json!({"slug": "omega", "customer_name": "Omega", "address": "https://omega.us.example", "region": "us", "template": "Contoso pack"});
    refused(site(&k, tenant, Some(&pat), "POST", "/partner/onboard", &[], theirs.clone()).await, "invalid", "not a setup you may use");
    theirs["template"] = Value::Null;
    refused(site(&k, tenant, Some(&eve), "POST", "/partner/onboard", &[], theirs).await, "forbidden", "onboard their customers");
    let zeta_ws =
        t.k.list(
            &t.admin,
            "HostedWorkspace",
            &ListQuery { filters: ferrum_kernel::query::parse_filters(&json!([["slug", "=", "zeta"]])).unwrap(), ..Default::default() },
        )
        .await
        .unwrap()
        .rows;
    let zeta_ws = t.get("HostedWorkspace", uid(&zeta_ws[0]["id"])).await;
    assert_eq!(
        (zeta_ws["status"].as_str(), uid(&zeta_ws["partner"]), zeta_ws["template_revision"].as_i64(), uid(&zeta_ws["cell"])),
        (Some("Onboarding"), northwind, Some(2), us1)
    );

    // ───────────── cases, due by plan ─────────────
    let raised =
        site(&k, tenant, Some(&pat), "POST", "/case", &[], json!({"workspace": "acme", "subject": "Reports are slow", "severity": "Critical"})).await.unwrap();
    let due = DateTime::parse_from_rfc3339(raised["due_at"].as_str().unwrap()).unwrap().with_timezone(&Utc);
    assert!((due - Utc::now() - Duration::hours(1)).num_minutes().abs() <= 1, "premium, critical: within the hour");
    let slow =
        t.k.list(
            &t.admin,
            "SupportCase",
            &ListQuery { filters: ferrum_kernel::query::parse_filters(&json!([["subject", "=", "Reports are slow"]])).unwrap(), ..Default::default() },
        )
        .await
        .unwrap()
        .rows;
    assert_eq!((uid(&slow[0]["partner"]), slow[0]["raised_by"].as_str()), (northwind, Some("pat@partner.test")));
    refused(
        site(&k, tenant, Some(&pat), "POST", "/case", &[], json!({"workspace": "gamma", "subject": "Contoso's customer"})).await,
        "forbidden",
        "a workspace of yours",
    );
    refused(site(&k, tenant, Some(&eve), "POST", "/case", &[], json!({"workspace": "acme", "subject": "Hello"})).await, "forbidden", "a workspace of yours");
    site(&k, tenant, Some(&contact), "POST", "/case", &[], json!({"workspace": "acme", "subject": "A question"})).await.expect("its contact raises one");
    let low = t.insert(&op1, "SupportCase", json!({"workspace": gamma["id"], "subject": "Slow export", "severity": "Low"})).await;
    let low_id = uid(&low["id"]);
    let hours = |c: &Doc| {
        let at = |k: &str| DateTime::parse_from_rfc3339(c[k].as_str().unwrap()).unwrap();
        (at("due_at") - at("created_at")).num_minutes()
    };
    assert_eq!(hours(&t.get("SupportCase", low_id).await), 72 * 60, "standard, low");
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_support_case set created_at = created_at - interval '2 hours' where id = $1").bind(low_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    t.update(&op1, "SupportCase", low_id, json!({"severity": "Critical"})).await.unwrap();
    assert_eq!(hours(&t.get("SupportCase", low_id).await), 4 * 60, "standard, critical, from when it was raised");
    refused(t.update(&op1, "SupportCase", low_id, json!({"status": "Resolved"})).await, "invalid", "resolving a case says how");
    t.update(&op1, "SupportCase", low_id, json!({"status": "Resolved", "resolution": "An index was added"})).await.unwrap();
    assert!(t.get("SupportCase", low_id).await["resolved_at"].is_string());

    // ───────────── statements ─────────────
    refused(t.act(&op1, "Partner", northwind, "issue_statement", json!({"month": today.to_string()})).await, "invalid", "a month that has ended");
    let issued = t.act(&op1, "Partner", northwind, "issue_statement", Value::Null).await.expect("last month's");
    assert_eq!(dec(&issued["partner_share"]), 100.0, "20% of 400 and 100; zeta is still onboarding");
    let statement = t.get("RevenueStatement", uid(&issued["statement"])).await;
    assert_eq!((dec(&statement["total"]), statement["lines"].as_array().unwrap().len()), (500.0, 2));
    refused(t.act(&op1, "Partner", northwind, "issue_statement", Value::Null).await, "invalid", "has been issued");
    refused(t.act(&rm1, "Partner", northwind, "issue_statement", Value::Null).await, "forbidden", "");
    refused(t.act(&clerk, "Partner", northwind, "issue_statement", Value::Null).await, "forbidden", "a Fleet Operator issues statements");

    // ───────────── a rollout that finishes, coming to another cell ─────────────
    let us_rollout = uid(&t.insert(&op1, "Rollout", json!({"release": r1.to_string(), "soak_hours": 1, "waves": [wave(us1)]})).await["id"]);
    t.act(&op1, "Rollout", us_rollout, "start", Value::Null).await.unwrap();

    // ───────────── the customer's own page ─────────────
    let mine = site(&k, tenant, Some(&contact), "GET", "/mine", &[], Value::Null).await.unwrap();
    let w = &mine["workspaces"][0];
    assert_eq!((mine["workspaces"].as_array().unwrap().len(), w["slug"].as_str(), w["cell"]["cell"].as_str()), (1, Some("acme"), Some("eu-1")));
    assert_eq!((w["recovery"][0]["kind"].as_str(), w["recovery"][0]["status"].as_str()), (Some("Restore"), Some("Done")));
    let titles: Vec<&str> = w["incidents"].as_array().unwrap().iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Slow pages on eu-1"], "the incidents on its own cell");
    assert!(w["maintenance"].as_array().unwrap().is_empty(), "us-1's coming upgrade is not its cell's");
    assert_eq!(w["cases"].as_array().unwrap().len(), 2);
    assert!(site(&k, tenant, Some(&eve), "GET", "/mine", &[], Value::Null).await.unwrap()["workspaces"].as_array().unwrap().is_empty());
    let next = t0 + Duration::days(7);
    t.reading("us-1", TOKEN_US, "0.4.0", &digest('a'), next - Duration::minutes(1), 40).await;
    t.upkeep(next).await;
    t.upkeep(next + Duration::minutes(5)).await;
    assert_eq!(states(&t.get("Rollout", us_rollout).await), ["Soaking"]);
    t.reading("us-1", TOKEN_US, "0.4.0", &digest('a'), next + Duration::minutes(64), 40).await;
    t.upkeep(next + Duration::minutes(66)).await;
    let finished = t.get("Rollout", us_rollout).await;
    assert_eq!((states(&finished), finished["status"].as_str()), (vec!["Done".to_string()], Some("Done")));
    let mut tx = k.db.tenant_tx(tenant).await.unwrap();
    sqlx::query("update e_service_incident set resolved_at = now() - interval '4 days' where id = $1").bind(inc_id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let public = site(&k, tenant, None, "GET", "/status", &[], Value::Null).await.unwrap();
    assert_eq!(public["incidents"].as_array().unwrap().len(), 1, "resolved days ago, it is no longer news");

    // ───────────── certifications lapse, and who may do what ─────────────
    t.upkeep(Utc::now() + Duration::days(31)).await;
    assert_eq!(t.get("Partner", northwind).await["tier"], "Registered");
    refused(t.k.insert(&rm1, "Cell", obj(cell("ap-1", "ap", 1, "Sunday", 2, "x"))).await, "forbidden", "");
    refused(t.k.list(&contact, "HostedWorkspace", &ListQuery::default()).await, "forbidden", "");
    refused(t.k.list(&pat, "RevenueStatement", &ListQuery::default()).await, "forbidden", "");
}
