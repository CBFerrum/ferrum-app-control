//! Hosting invoiced through ERP (the `control_erp` connector package, FERRUM-CLOUD-003). A hosted
//! workspace naming whom to bill is invoiced once for each month after the month ends, for its days
//! in it: from the day billing starts (set, or the day it was made on its cell) to the day it ends
//! (set, or the day it was closed), at what it pays a month; a month cut short pays its days' share.
//! Each month is a Sales Invoice in ERP, left for accounts to issue, and a Hosting Invoice that
//! keeps it once. Usage beyond what it pays a month covers is charged too: each active person over
//! those included and each whole GB of storage over what is included, at the month's peak as its
//! health reports counted (Workspace Usage). Months and days are the control plane's own (its
//! workspace's time zone).

use crate::obj;
use async_trait::async_trait;
use chrono::{Datelike, Duration, Months, NaiveDate};
use ferrum_domain::util::{money, round, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result, Tx};
use rust_decimal::Decimal;
use serde_json::json;
use uuid::Uuid;

/// A date a record holds.
fn day(doc: &Doc, k: &str) -> Option<NaiveDate> {
    s(doc, k).and_then(|v| NaiveDate::parse_from_str(v.get(..10).unwrap_or(v), "%Y-%m-%d").ok())
}

/// The first day of `day`'s month.
fn month_of(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

/// What `monthly` comes to for `days` of a month of `whole` days: all of it for the whole month,
/// else the days' share to the cent.
pub(crate) fn share(monthly: Decimal, days: i64, whole: i64) -> Decimal {
    if days >= whole || whole <= 0 { monthly } else { round(monthly * Decimal::from(days) / Decimal::from(whole), 2) }
}

pub struct BillingHook;

#[async_trait]
impl DocHook for BillingHook {
    fn name(&self) -> &'static str {
        "control_erp.workspace"
    }

    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        // Closed, it is billed up to the day it was closed, unless a day was set.
        if s(doc, "status") == Some("Closed") && s(doc, "bill_until").is_none() {
            doc.insert("bill_until".into(), json!(hc.ctx.today().to_string()));
        }
        Ok(())
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if let (Some(from), Some(until)) = (day(doc, "bill_from"), day(doc, "bill_until"))
            && until < from
        {
            return Err(Error::field("bill_until", "is on or after the day billing starts"));
        }
        Ok(())
    }
}

/// Each month that ended before `today` and was not invoiced yet, invoiced once for every billed
/// workspace: how many.
pub(crate) async fn invoice(k: &Kernel, ctx: &Ctx, tx: &mut Tx, today: NaiveDate) -> Result<usize> {
    let settings: Option<(Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as("select company, item from e_hosting_billing where tenant_id = $1 limit 1").bind(ctx.tenant).fetch_optional(&mut **tx).await?;
    let Some((Some(company), Some(item))) = settings else { return Ok(0) };
    let currency: Option<Uuid> = sqlx::query_scalar("select default_currency from e_company where tenant_id = $1 and id = $2")
        .bind(ctx.tenant)
        .bind(company)
        .fetch_optional(&mut **tx)
        .await?
        .flatten();
    let zone = if ctx.schema.timezone.is_empty() { "UTC" } else { ctx.schema.timezone.as_str() };
    // Each billed workspace: whom, at what a month, from and until when, and its last month invoiced.
    type Billed = (Uuid, String, Uuid, Decimal, Option<NaiveDate>, Option<NaiveDate>, Option<NaiveDate>, Terms);
    type Terms = (Option<i64>, Option<Decimal>, Option<Decimal>, Option<Decimal>);
    let billed: Vec<(
        Uuid,
        String,
        Uuid,
        Decimal,
        Option<NaiveDate>,
        Option<NaiveDate>,
        Option<NaiveDate>,
        Option<i64>,
        Option<Decimal>,
        Option<Decimal>,
        Option<Decimal>,
    )> = sqlx::query_as(
        "select w.id, w.slug, w.bill_customer, w.monthly_amount, coalesce(w.bill_from, (w.provisioned_at at time zone $2)::date), w.bill_until, \
           (select max(h.month) from e_hosting_invoice h where h.tenant_id = w.tenant_id and h.workspace = w.id), \
           w.bill_people_included::bigint, w.bill_person_price, w.bill_storage_included_gb, w.bill_gb_price \
         from e_hosted_workspace w where w.tenant_id = $1 and w.bill_customer is not null and w.monthly_amount > 0 and w.status in ('Active', 'Closed') \
         order by w.slug",
    )
    .bind(ctx.tenant)
    .bind(zone)
    .fetch_all(&mut **tx)
    .await?;
    let this_month = month_of(today);
    let mut n = 0;
    let billed: Vec<Billed> = billed.into_iter().map(|(w, s, c, m, f, u, l, pi, pp, si, sp)| (w, s, c, m, f, u, l, (pi, pp, si, sp))).collect();
    for (workspace, slug, customer, monthly, start, until, last, terms) in billed {
        let Some(start) = start else { continue };
        let mut month = match last {
            Some(m) => m.checked_add_months(Months::new(1)).ok_or_else(|| Error::internal("a month past the calendar"))?,
            None => month_of(start),
        };
        while month < this_month && until.is_none_or(|u| u >= month) {
            let next = month.checked_add_months(Months::new(1)).ok_or_else(|| Error::internal("a month past the calendar"))?;
            let end = next - Duration::days(1);
            let (from, to) = (start.max(month), until.map_or(end, |u| u.min(end)));
            if from <= to {
                let (days, whole) = ((to - from).num_days() + 1, (end - month).num_days() + 1);
                let amount = share(monthly, days, whole);
                let cut = if days < whole { format!(" ({days} of {whole} days)") } else { String::new() };
                let what = format!("Hosting of {slug}, {from} to {to}{cut}");
                let line = json!({"item": item.to_string(), "qty": "1", "price_list_rate": money(amount), "discount_percent": "0", "description": what});
                // The month's peaks, as its health reports counted them.
                let (peak_people, peak_gb): (Option<i64>, Option<Decimal>) = sqlx::query_as(
                    "select max(people)::bigint, max(storage_gb) from e_workspace_usage where tenant_id = $1 and workspace = $2 and day between $3 and $4",
                )
                .bind(ctx.tenant)
                .bind(workspace)
                .bind(from)
                .bind(to)
                .fetch_one(&mut **tx)
                .await?;
                let mut lines = vec![line];
                let mut total = amount;
                for (qty, price, what) in beyond(terms, peak_people, peak_gb, month) {
                    lines.push(json!({"item": item.to_string(), "qty": qty.to_string(), "price_list_rate": money(price), "discount_percent": "0", "description": what}));
                    total += price * Decimal::from(qty);
                }
                let inv = k
                    .insert_tx(
                        ctx,
                        tx,
                        "SalesInvoice",
                        obj(json!({"company": company.to_string(), "customer": customer.to_string(), "currency": currency.map(|c| c.to_string()),
                                   "remarks": what, "items": lines})),
                    )
                    .await?;
                k.insert_tx(
                    ctx,
                    tx,
                    "HostingInvoice",
                    obj(json!({"workspace": workspace.to_string(), "month": month.to_string(), "period_from": from.to_string(), "period_to": to.to_string(),
                               "days": days, "amount": money(total), "sales_invoice": inv.get("id"), "peak_people": peak_people,
                               "peak_storage_gb": peak_gb.map(|g| g.to_string())})),
                )
                .await?;
                n += 1;
            }
            month = next;
        }
    }
    Ok(n)
}

/// What a month's peaks charge beyond its terms (people included, price an extra person, GB
/// included, price an extra GB): (how many, at what price, what it is), each whole.
pub(crate) fn beyond(
    (people_included, person_price, gb_included, gb_price): (Option<i64>, Option<Decimal>, Option<Decimal>, Option<Decimal>),
    peak_people: Option<i64>,
    peak_gb: Option<Decimal>,
    month: NaiveDate,
) -> Vec<(i64, Decimal, String)> {
    let mut out = Vec::new();
    let when = month.format("%B %Y");
    if let (Some(price), Some(peak)) = (person_price.filter(|p| *p > Decimal::ZERO), peak_people) {
        let included = people_included.unwrap_or(0);
        if peak > included {
            out.push((peak - included, price, format!("People beyond the {included} included: {peak} at the peak of {when}")));
        }
    }
    if let (Some(price), Some(peak)) = (gb_price.filter(|p| *p > Decimal::ZERO), peak_gb) {
        let included = gb_included.unwrap_or_default();
        let extra = (peak - included).ceil();
        if extra > Decimal::ZERO {
            let gb = i64::try_from(extra).unwrap_or(i64::MAX);
            out.push((gb, price, format!("Storage beyond the {} GB included: {} GB at the peak of {when}", included.normalize(), peak.normalize())));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_beyond_the_terms() {
        let d = |s: &str| s.parse::<Decimal>().unwrap();
        let march = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let terms = (Some(10), Some(d("50")), Some(d("5")), Some(d("2")));
        let lines = beyond(terms, Some(13), Some(d("7.4")), march);
        assert_eq!(lines.iter().map(|l| (l.0, l.1)).collect::<Vec<_>>(), vec![(3, d("50")), (3, d("2"))], "3 people, and 2.4 GB as 3 whole");
        assert!(lines[0].2.contains("13 at the peak of March 2026"), "{lines:?}");
        assert!(beyond(terms, Some(10), Some(d("5")), march).is_empty(), "within what is included");
        assert!(beyond((Some(10), None, None, None), Some(40), Some(d("90")), march).is_empty(), "no price, no charge");
        assert!(beyond(terms, None, None, march).is_empty(), "no readings, no charge");
    }

    #[test]
    fn a_month_cut_short_pays_its_days() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(month_of(d("2026-02-28")), d("2026-02-01"));
        assert_eq!(month_of(d("2026-03-01")), d("2026-03-01"));
        assert_eq!(share(Decimal::from(3100), 17, 31), Decimal::from(1700));
        assert_eq!(share(Decimal::from(1000), 1, 3), Decimal::new(33333, 2));
        assert_eq!(share(Decimal::from(2800), 28, 28), Decimal::from(2800), "a whole month, all of it");
        assert_eq!(share(Decimal::from(2800), 30, 28), Decimal::from(2800), "never more than a month");
    }
}
