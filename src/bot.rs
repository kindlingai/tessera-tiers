//! Keeps the merge queue in step with the tiers. A trusted priority PR goes
//! to the front of the queue once its checks pass, and each PR that
//! conflicts with it stays out of the queue until it merges. Of two priority
//! PRs that conflict, the first labelled holds the other out. Trusted landing
//! PRs join the queue once their checks pass.

use crate::board::{Pull, Tier, column, queue};
use crate::github::{Client, Install};
use crate::overlap::Kind;
use crate::{Config, Fault, Host};
use std::collections::BTreeSet;

const LEASE_KEY: &str = "bot-lease";
/// Seconds a run holds the lease. Cron starts a run each minute.
const LEASE: u64 = 50;
/// The shortest life the shared store keeps a value for.
const STORE_MIN: u64 = 60;
const FAULTS_KEY: &str = "bot-faults";
/// Seconds the board shows the failures of the last run.
const FAULTS_LIFE: u64 = 3600;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Act {
    /// To the front of the queue. A queued PR leaves the queue first.
    Jump,
    Enqueue,
    /// Out of the queue, because the priority PR `by` conflicts with it.
    Dequeue {
        by: u64,
    },
}

impl Act {
    pub fn name(self) -> &'static str {
        match self {
            Act::Jump => "jump",
            Act::Enqueue => "enqueue",
            Act::Dequeue { .. } => "dequeue",
        }
    }
}

pub struct Step<'a> {
    pub pull: &'a Pull,
    pub act: Act,
}

impl Step<'_> {
    pub fn describe(&self) -> String {
        let n = self.pull.number;
        match self.act {
            Act::Jump => format!("move #{n} to the front of the merge queue"),
            Act::Enqueue => format!("add #{n} to the merge queue"),
            Act::Dequeue { by } => {
                format!("remove #{n} from the merge queue: priority #{by} conflicts with it")
            }
        }
    }
}

fn names_conflict(a: &Pull, b: &Pull) -> bool {
    let mut found = false;
    for overlap in &a.overlaps {
        found |= overlap.kind == Kind::Conflict && overlap.other == b.number;
    }
    found
}

/// Trusted priority PRs that hold their conflicts out of the queue, first
/// labelled first.
fn leads(pulls: &[Pull]) -> Vec<&Pull> {
    let mut leads = Vec::new();
    for pull in pulls {
        if pull.tier == Tier::Priority && pull.trusted && !pull.draft && pull.queueable {
            leads.push(pull);
        }
    }
    leads.sort_by(|a, b| (&a.since, a.number).cmp(&(&b.since, b.number)));
    leads
}

/// The first lead labelled before `pull` that conflicts with it.
fn blocker(pull: &Pull, leads: &[&Pull]) -> Option<u64> {
    for lead in leads {
        if lead.number == pull.number {
            return None;
        }
        if names_conflict(lead, pull) || names_conflict(pull, lead) {
            return Some(lead.number);
        }
    }
    None
}

/// A trusted tier PR that the queue can take now.
fn ready(pull: &Pull) -> bool {
    let green = pull.ci.as_deref() == Some("success");
    let tagged = pull.tier != Tier::Untagged && pull.trusted;
    tagged && green && pull.queueable && !pull.draft && !pull.waits && !pull.stuck
}

/// The changes that bring the queue in step with the tiers, in the order
/// to make them.
pub fn plan(pulls: &[Pull]) -> Vec<Step<'_>> {
    let leads = leads(pulls);
    let mut steps = Vec::new();
    let mut jumps = BTreeSet::new();
    // A PR that stays in the queue and is not a lead sits ahead.
    let mut other_ahead = false;
    for pull in queue(pulls) {
        if let Some(by) = blocker(pull, &leads) {
            steps.push(Step {
                pull,
                act: Act::Dequeue { by },
            });
        } else if !leads.iter().any(|l| l.number == pull.number) {
            other_ahead = true;
        } else if other_ahead && ready(pull) {
            jumps.insert(pull.number);
        }
    }
    for lead in &leads {
        if lead.queued.is_none() && ready(lead) && blocker(lead, &leads).is_none() {
            jumps.insert(lead.number);
        }
    }
    // Each jump goes ahead of the last, so the first labelled jumps last.
    for &pull in leads.iter().rev() {
        if jumps.contains(&pull.number) {
            steps.push(Step {
                pull,
                act: Act::Jump,
            });
        }
    }
    for pull in column(pulls, Tier::Landing) {
        if ready(pull) && blocker(pull, &leads).is_none() {
            steps.push(Step {
                pull,
                act: Act::Enqueue,
            });
        }
    }
    steps
}

async fn apply<H: Host>(
    client: &Client<'_, H>,
    install: &Install,
    step: &Step<'_>,
) -> Result<(), Fault> {
    let pull = step.pull;
    match step.act {
        Act::Jump => {
            if pull.queued.is_some() {
                client.dequeue(install, pull).await?;
            }
            client.enqueue(install, pull, true).await
        }
        Act::Enqueue => client.enqueue(install, pull, false).await,
        Act::Dequeue { by } => {
            client.dequeue(install, pull).await?;
            let mut body = format!(
                "The tiers board removed this pull request from the merge queue. \
Priority #{by} conflicts with it and merges first."
            );
            if pull.merging {
                body += " Auto-merge is off.";
            }
            client.comment(install, pull.number, &body).await
        }
    }
}

/// One run. It changes the queue only when the config allows it, and only
/// when no other run holds the lease.
pub async fn tick<H: Host>(host: &H, config: &Config) -> Result<(), Fault> {
    if !config.act {
        return Ok(());
    }
    let now = host.now();
    let held = host
        .load(LEASE_KEY)
        .await
        .and_then(|v| v.parse::<u64>().ok());
    if held.is_some_and(|until| now < until) {
        return Ok(());
    }
    let until = (now + LEASE).to_string();
    host.store(LEASE_KEY, until, STORE_MIN).await;
    let client = Client { host, config };
    let install = client.install().await?;
    let pulls = client.pulls(&install).await?;
    let steps = plan(&pulls);
    let mut faults = Vec::new();
    for step in &steps {
        if let Err(fault) = apply(&client, &install, step).await {
            faults.push(format!("{}: {}", step.describe(), fault.text));
        }
    }
    if !steps.is_empty() {
        host.recent().clear();
    }
    host.store(FAULTS_KEY, faults.join("\n"), FAULTS_LIFE).await;
    host.store(LEASE_KEY, "0".into(), STORE_MIN).await;
    Ok(())
}

/// The failures of the last run, one per line.
pub async fn faults<H: Host>(host: &H) -> String {
    host.load(FAULTS_KEY).await.unwrap_or_default()
}
