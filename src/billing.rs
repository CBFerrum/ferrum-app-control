//! Hosting invoiced through ERP (the `control_erp` connector package, FERRUM-CLOUD-003). A hosted
//! workspace naming whom to bill is invoiced once for each month after the month ends, for its days
//! in it: from the day billing starts (set, or the day it was made on its cell) to the day it ends
//! (set, or the day it was closed), at what it pays a month; a month cut short pays its days' share.
//! Each month is a Sales Invoice in ERP, left for accounts to issue, and a Hosting Invoice that
//! keeps it once. Months and days are the control plane's own (its workspace's time zone).

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
    let billed: Vec<(Uuid, String, Uuid, Decimal, Option<NaiveDate>, Option<NaiveDate>, Option<NaiveDate>)> = sqlx::query_as(
        "select w.id, w.slug, w.bill_customer, w.monthly_amount, coalesce(w.bill_from, (w.provisioned_at at time zone $2)::date), w.bill_until, \
           (select max(h.month) from e_hosting_invoice h where h.tenant_id = w.tenant_id and h.workspace = w.id) \
         from e_hosted_workspace w where w.tenant_id = $1 and w.bill_customer is not null and w.monthly_amount > 0 and w.status in ('Active', 'Closed') \
         order by w.slug",
    )
    .bind(ctx.tenant)
    .bind(zone)
    .fetch_all(&mut **tx)
    .await?;
    let this_month = month_of(today);
    let mut n = 0;
    for (workspace, slug, customer, monthly, start, until, last) in billed {
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
                let inv = k
                    .insert_tx(
                        ctx,
                        tx,
                        "SalesInvoice",
                        obj(json!({"company": company.to_string(), "customer": customer.to_string(), "currency": currency.map(|c| c.to_string()),
                                   "remarks": what, "items": [line]})),
                    )
                    .await?;
                k.insert_tx(
                    ctx,
                    tx,
                    "HostingInvoice",
                    obj(json!({"workspace": workspace.to_string(), "month": month.to_string(), "period_from": from.to_string(), "period_to": to.to_string(),
                               "days": days, "amount": money(amount), "sales_invoice": inv.get("id")})),
                )
                .await?;
                n += 1;
            }
            month = next;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

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
