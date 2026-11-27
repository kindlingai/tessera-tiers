//! Draws the board. The element builder escapes all text.

use crate::Config;
use crate::board::{Pull, Tier, column, drafts, queue};
use crate::html::{El, Part, el, text};
use crate::layout::{self, Layout, Spot};
use crate::overlap::Kind;
use crate::session::Session;
use std::collections::BTreeMap;

/// The percent of a limit the page marks as near.
pub(crate) const NEAR: u64 = 90;
/// Each tab's path and name.
const TABS: [(&str, &str); 3] = [("/", "Tiers"), ("/cache", "Cache"), ("/limits", "Limits")];

const STYLE: &str = "body{font:14px system-ui;margin:1rem}\
main.board{position:relative;display:grid;grid-template-columns:repeat(5,1fr);gap:0 1rem;align-items:start}\
.blocks{box-shadow:0 0 0 4px #a605}.blocked{box-shadow:0 0 0 4px #c005}\
.blocks.blocked{box-shadow:0 0 0 4px #c005,0 0 0 8px #a605}article{position:relative;z-index:1;background:#fff}\
svg.links{position:absolute;left:0;top:0;pointer-events:none;overflow:visible}\
.links line{stroke:#888;stroke-width:2;stroke-dasharray:4 3}.links line.conflicts{stroke:#c00;stroke-dasharray:none}\
article{border:1px solid #888;border-radius:4px;padding:.5rem;margin:.5rem 0}\
.tag{border:1px solid #888;border-radius:4px;padding:0 .3rem;margin-right:.3rem}\
.bad{border-color:#c00;color:#c00}li.bad{border:0}.holds{color:#740}\
form{display:inline}nav a{margin-right:1rem}\
table{border-collapse:collapse;margin-top:.5rem}\
td,th{text-align:left;padding:.25rem 1.5rem .25rem 0;white-space:nowrap;border-bottom:1px solid #eee}\
td:nth-child(2){font-family:ui-monospace,monospace}\
.meter{display:inline-block;width:8rem;height:.6rem;background:#eee;border-radius:3px;margin-right:.5rem}\
.meter i{display:block;height:100%;background:#888;border-radius:3px}.bad .meter i{background:#c00}ul{margin:.3rem 0;padding-left:1.2rem}\
.locked{opacity:.4;pointer-events:none}.error{color:#c00}";

pub struct View<'a> {
    pub config: &'a Config,
    pub session: &'a Session,
    pub pulls: &'a [Pull],
}

fn tag(body: impl Into<String>, bad: bool) -> El {
    let class = if bad { "tag bad" } else { "tag" };
    text("span", body).attr("class", class)
}

fn hidden(name: &'static str, value: impl Into<String>) -> El {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value)
}

fn tags(pull: &Pull) -> El {
    let mut tags = el("p");
    let ci = pull.ci.as_deref().unwrap_or("no checks");
    let red = matches!(ci, "failure" | "error");
    tags = tags.kid(tag(format!("CI {ci}"), red));
    if pull.stuck {
        tags = tags.kid(tag("conflicts with main", true));
    }
    if pull.waits {
        tags = tags.kid(tag("waits on a human", false));
    }
    if pull.merging {
        tags = tags.kid(tag("auto-merge on", false));
    }
    if let Some(position) = pull.queued {
        tags = tags.kid(tag(format!("merge queue #{position}"), false));
    }
    if !pull.trusted {
        let note = format!("label set by {}, CI ignores it", pull.setter);
        tags = tags.kid(tag(note, true));
    }
    tags
}

/// A conflict reads "conflicts with" from the PR behind and "blocks" from
/// the PR ahead, because the PR ahead merges first.
fn overlaps(pull: &Pull, rank: &BTreeMap<u64, usize>) -> El {
    let mine = rank.get(&pull.number);
    let mut list = el("ul");
    for overlap in &pull.overlaps {
        let behind = mine < rank.get(&overlap.other);
        let (verb, class) = match overlap.kind {
            Kind::Conflict if behind => ("blocks", "holds"),
            Kind::Conflict => ("conflicts with", "bad"),
            Kind::Shared => ("shares files with", ""),
        };
        let files = overlap.files.join(", ");
        let line = format!("{verb} #{}: {files}", overlap.other);
        list = list.kid(text("li", line).attr("class", class));
    }
    if !pull.overlaps.is_empty() {
        return list;
    }
    // The workflow compares only pull requests that merge with main.
    let none = if pull.stuck {
        "overlaps not checked"
    } else {
        "no overlaps"
    };
    list.kid(text("li", none))
}

/// Buttons that move a pull request to another tier.
fn controls(pull: &Pull, session: &Session) -> El {
    let mut form = el("form")
        .attr("method", "post")
        .attr("action", "/tier")
        .kid(hidden("csrf", &session.csrf))
        .kid(hidden("pr", pull.number.to_string()));
    for tier in Tier::ALL {
        let verb = match tier == pull.tier {
            true if pull.trusted => continue,
            true => "confirm",
            false => "to",
        };
        let button = text("button", format!("{verb} {}", tier.name()));
        form = form.kid(button.attr("name", "tier").attr("value", tier.name()));
    }
    if !pull.merging && !pull.draft && pull.queued.is_none() {
        form = form.kid(text("button", "enable auto-merge").attr("formaction", "/automerge"));
    }
    if pull.waits {
        let button = text("button", "remove needs-human-review");
        form = form.kid(button.attr("formaction", "/review"));
    }
    form
}

/// Rings for a PR that blocks another, or that another blocks.
fn marks(number: u64, layout: &Layout) -> String {
    let mut marks = Vec::new();
    if layout.blocking.contains(&number) {
        marks.push("blocks");
    }
    if layout.blocked.contains(&number) {
        marks.push("blocked");
    }
    marks.join(" ")
}

fn card(view: &View, pull: &Pull, layout: &Layout) -> El {
    let Config { web, repo, .. } = view.config;
    let link = text("a", format!("#{} {}", pull.number, pull.title))
        .attr("href", format!("{web}/{repo}/pull/{}", pull.number));
    let by = format!(" by {}", pull.author);
    let mut links = [Vec::new(), Vec::new()];
    for overlap in &pull.overlaps {
        let kind = usize::from(overlap.kind == Kind::Conflict);
        links[kind].push(overlap.other.to_string());
    }
    let [shares, conflicts] = links.map(|numbers| numbers.join(" "));
    let card = el("article")
        .attr("id", format!("pr-{}", pull.number))
        .attr("data-shares", shares)
        .attr("data-conflicts", conflicts)
        .attr("class", marks(pull.number, layout))
        .kid(el("p").kid(link).text(by))
        .kid(tags(pull))
        .kid(overlaps(pull, &layout.rank));
    // GitHub merges a queued PR on its own.
    match pull.queued {
        Some(_) => card,
        None => card.kid(controls(pull, view.session)),
    }
}

/// The tabs, the login and a sign-out button.
fn account(session: &Session) -> El {
    let out = el("form")
        .attr("method", "post")
        .attr("action", "/logout")
        .kid(hidden("csrf", &session.csrf))
        .kid(text("button", "Sign out"));
    let mut nav = el("nav");
    for (path, name) in TABS {
        nav = nav.kid(text("a", name).attr("href", path));
    }
    nav.text(format!("{} ", session.login)).kid(out)
}

/// A bar filled to `share` percent, then the number. Marked at `NEAR`.
pub(crate) fn meter(share: u64) -> El {
    let fill = el("i").attr("style", format!("width:{}%", share.min(100)));
    let bar = el("span").attr("class", "meter").kid(fill);
    let class = if share >= NEAR { "bad" } else { "" };
    el("span")
        .attr("class", class)
        .kid(bar)
        .text(format!("{share}%"))
}

/// A page for a signed-in editor.
pub(crate) fn frame(config: &Config, session: &Session, main: El) -> String {
    shell(config, [account(session), main])
}

/// A whole page with the repository's title.
fn shell(config: &Config, parts: [El; 2]) -> String {
    let title = format!("{} tiers", config.repo);
    let head = el("head")
        .kid(el("meta").attr("charset", "utf-8"))
        .kid(text("title", title.clone()))
        .kid(el("style").kid(Part::Raw(STYLE.into())))
        .kid(el("script").attr("src", "/app.js").attr("defer", ""));
    let body = el("body").kid(text("h1", title)).kids(parts);
    el("html").attr("lang", "en").kid(head).kid(body).page()
}

/// The page for a visitor with no session.
pub(crate) fn welcome(config: &Config) -> String {
    let link = text("a", "Sign in with GitHub").attr("href", "/login");
    shell(config, [el("p").kid(link), el("main")])
}

/// Grid lines for a cell. Row 1 holds the lane titles.
fn area(spot: Spot) -> String {
    let (lane, row) = (spot.lane + 1, spot.row + 2);
    format!("grid-column:{lane}/{};grid-row:{row}/{}", lane + 1, row + 1)
}

/// The lanes from left to right, toward the merge.
pub(crate) fn board(view: &View) -> String {
    let mut titles = vec!["draft"];
    let mut lanes = vec![drafts(view.pulls)];
    for tier in [Tier::Untagged, Tier::Landing, Tier::Priority] {
        titles.push(tier.name());
        lanes.push(column(view.pulls, tier));
    }
    titles.push("merge queue");
    lanes.push(queue(view.pulls));
    let layout = layout::lay(&lanes);
    let mut main = el("main").attr("class", "board");
    for (lane, title) in titles.into_iter().enumerate() {
        let style = format!("grid-column:{};grid-row:1", lane + 1);
        main = main.kid(text("h2", title).attr("style", style));
    }
    for &(pull, spot) in &layout.cards {
        let card = card(view, pull, &layout);
        main = main.kid(card.attr("style", area(spot)));
    }
    frame(view.config, view.session, main)
}
