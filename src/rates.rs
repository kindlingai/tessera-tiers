//! Pull requests opened and merged per hour over the last day, and per day
//! over the last month. One snapshot per hour lives in the shared store, so
//! GitHub sees one rebuild per hour, whatever the traffic.

use crate::github::{Client, Install, RELEASE};
use crate::html::{El, el, text};
use crate::page;
use crate::session::Session;
use crate::{Config, Fault, Host};
use serde::{Deserialize, Serialize};

/// Bars in the hourly chart.
const HOURS: u64 = 24;
/// Bars in the daily chart.
const DAYS: u64 = 30;
const KEY: &str = "rates";
/// Seconds the store keeps a snapshot. It is stale after its hour anyway.
const KEEP: u64 = 2 * 3600;

#[derive(Serialize, Deserialize)]
pub struct Bucket {
    /// `YYYY-MM-DDTHH` for an hour, `YYYY-MM-DD` for a day.
    label: String,
    opened: u64,
    merged: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Rates {
    /// The hour the snapshot was built in.
    hour: String,
    hours: Vec<Bucket>,
    days: Vec<Bucket>,
}

/// `YYYY-MM-DDTHH` in UTC for seconds since the epoch.
fn stamp(secs: u64) -> String {
    // Days to a civil date, after Howard Hinnant's `civil_from_days`.
    let z = (secs / 86400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let hour = secs % 86400 / 3600;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}")
}

/// Empty buckets for `count` steps of `step` seconds up to `now`, oldest
/// first, labelled with the first `width` characters of the stamp.
fn buckets(now: u64, count: u64, step: u64, width: usize) -> Vec<Bucket> {
    let mut buckets = Vec::new();
    for back in (0..count).rev() {
        let mut label = stamp(now.saturating_sub(back * step));
        label.truncate(width);
        buckets.push(Bucket {
            label,
            opened: 0,
            merged: 0,
        });
    }
    buckets
}

/// Counts one GitHub timestamp into the bucket it falls in, if any.
fn tally(buckets: &mut [Bucket], at: &str, merged: bool) {
    for bucket in buckets {
        if at.starts_with(&bucket.label) {
            *if merged {
                &mut bucket.merged
            } else {
                &mut bucket.opened
            } += 1;
        }
    }
}

/// Reads every pull request GitHub updated since the oldest bar. Opening
/// and merging both update a PR, so none in the window is missed.
async fn build<H: Host>(
    client: &Client<'_, H>,
    install: &Install,
    now: u64,
) -> Result<Rates, Fault> {
    let mut hours = buckets(now, HOURS, 3600, 13);
    let mut days = buckets(now, DAYS, 86400, 10);
    let oldest = format!("{}T00:00:00Z", days[0].label);
    let repo = &client.config.repo;
    let mut page = 1;
    loop {
        let path = format!(
            "/repos/{repo}/pulls?state=all&sort=updated&direction=desc&per_page=100&page={page}"
        );
        let list = client.app("GET", &path, install, &[]).await?;
        let list = list.as_array().cloned().unwrap_or_default();
        let mut done = list.is_empty();
        for pull in &list {
            let updated = pull["updated_at"].as_str().unwrap_or_default();
            if updated < oldest.as_str() {
                done = true;
                break;
            }
            let head = pull["head"]["ref"].as_str().unwrap_or_default();
            if head.starts_with(RELEASE) {
                continue;
            }
            for (field, merged) in [("created_at", false), ("merged_at", true)] {
                if let Some(at) = pull[field].as_str() {
                    tally(&mut hours, at, merged);
                    tally(&mut days, at, merged);
                }
            }
        }
        if done {
            break;
        }
        page += 1;
    }
    Ok(Rates {
        hour: stamp(now),
        hours,
        days,
    })
}

/// This hour's snapshot from the store, or a new one built and stored.
pub async fn read<H: Host>(client: &Client<'_, H>, install: &Install) -> Result<Rates, Fault> {
    let host = client.host;
    let now = host.now();
    let stored = host.load(KEY).await;
    let stored = stored.and_then(|text| serde_json::from_str::<Rates>(&text).ok());
    if let Some(rates) = stored.filter(|r| r.hour == stamp(now)) {
        return Ok(rates);
    }
    let rates = build(client, install, now).await?;
    let text = serde_json::to_string(&rates).expect("plain data serializes");
    host.store(KEY, text, KEEP).await;
    Ok(rates)
}

/// One chart: a bar pair per bucket, scaled to the largest count.
fn chart(title: &str, buckets: &[Bucket], short: usize) -> El {
    let mut top = 1;
    for bucket in buckets {
        top = top.max(bucket.opened).max(bucket.merged);
    }
    let mut bars = el("ol").attr("class", "chart");
    for bucket in buckets {
        let style = format!(
            "--opened:{}%;--merged:{}%",
            bucket.opened * 100 / top,
            bucket.merged * 100 / top
        );
        let what = format!(
            "{} UTC: {} opened, {} merged",
            bucket.label.replace('T', " "),
            bucket.opened,
            bucket.merged
        );
        let bar = el("li")
            .attr("style", style)
            .attr("title", what)
            .attr("data-opened", bucket.opened.to_string())
            .attr("data-merged", bucket.merged.to_string())
            .kid(el("i").attr("class", "opened"))
            .kid(el("i").attr("class", "merged"))
            .kid(text("b", &bucket.label[bucket.label.len() - short..]));
        bars = bars.kid(bar);
    }
    let opened: u64 = buckets.iter().map(|b| b.opened).sum();
    let merged: u64 = buckets.iter().map(|b| b.merged).sum();
    let legend = el("p")
        .attr("class", "legend")
        .kid(text("span", format!("{opened} opened")).attr("class", "opened"))
        .kid(text("span", format!("{merged} merged")).attr("class", "merged"));
    el("section").kid(text("h2", title)).kid(legend).kid(bars)
}

pub fn render(config: &Config, session: &Session, rates: &Rates) -> String {
    let note = format!(
        "Times in UTC. Built in hour {}, rebuilt each hour.",
        rates.hour.replace('T', " ")
    );
    let main = el("main")
        .attr("class", "rates")
        .kid(text("p", note))
        .kid(chart("Last 24 hours", &rates.hours, 2))
        .kid(chart("Last 30 days", &rates.days, 5));
    page::frame(config, session, main)
}
