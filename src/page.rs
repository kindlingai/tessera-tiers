//! Draws the board. The element builder escapes all text.

use crate::Config;
use crate::board::{Pull, Tier, column, drafts, queue};
use crate::html::{El, el, text};
use crate::layout::{self, Layout, Spot};
use crate::overlap::Kind;
use crate::session::Session;
use std::collections::BTreeMap;

/// The percent of a limit the page marks as near.
pub(crate) const NEAR: u64 = 90;
/// Each tab's path and name.
const TABS: [(&str, &str); 4] = [
    ("/", "Tiers"),
    ("/rates", "Rates"),
    ("/cache", "Cache"),
    ("/limits", "Limits"),
];

pub struct View<'a> {
    pub config: &'a Config,
    pub session: &'a Session,
    pub pulls: &'a [Pull],
}

/// A boolean data attribute, present when `on`.
fn flag(el: El, name: &'static str, on: bool) -> El {
    if on { el.attr(name, "") } else { el }
}

fn hidden(name: &'static str, value: impl Into<String>) -> El {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value)
}

/// Notes about the pull request, named by kind for the style sheet.
/// GitHub's page for the merge queue of a branch.
fn queue_page(config: &Config, base: &str) -> String {
    format!("{}/{}/queue/{base}", config.web, config.repo)
}

fn tags(config: &Config, pull: &Pull) -> El {
    let ci = pull.ci.as_deref().unwrap_or("no checks");
    let mut tags = el("ul").attr("class", "tags");
    tags = tags.kid(text("li", format!("CI {ci}")).attr("class", "ci"));
    let setter = format!("label set by {}, CI ignores it", pull.setter);
    let notes = [
        (pull.stuck, "stuck", "conflicts with main".to_string()),
        (pull.waits, "waits", "waits on a human".to_string()),
        (pull.merging, "merging", "auto-merge on".to_string()),
        (!pull.trusted, "untrusted", setter),
    ];
    for (on, class, body) in notes {
        if on {
            tags = tags.kid(text("li", body).attr("class", class));
        }
    }
    if let Some(position) = pull.queued {
        let link = text("a", format!("merge queue #{position}"));
        let link = link.attr("href", queue_page(config, &pull.base));
        tags = tags.kid(el("li").attr("class", "queued").kid(link));
    }
    tags
}

/// A conflict reads "conflicts with" from the PR behind and "blocks" from
/// the PR ahead, because the PR ahead merges first.
fn overlaps(pull: &Pull, rank: &BTreeMap<u64, usize>) -> El {
    let mine = rank.get(&pull.number);
    let mut list = el("ul").attr("class", "overlaps");
    for overlap in &pull.overlaps {
        let behind = mine < rank.get(&overlap.other);
        let (verb, kind) = match overlap.kind {
            Kind::Conflict if behind => ("blocks", "blocks"),
            Kind::Conflict => ("conflicts with", "conflicts"),
            Kind::Shared => ("shares files with", "shares"),
        };
        let files = text("span", overlap.files.join(", ")).attr("class", "files");
        let line = el("li").attr("data-kind", kind).text(format!("{verb} "));
        let line = line.kid(text("b", format!("#{}", overlap.other))).text(" ");
        list = list.kid(line.kid(files));
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
    list.kid(text("li", none).attr("data-kind", "none"))
}

/// Buttons that move a pull request to another tier.
fn controls(pull: &Pull, session: &Session) -> El {
    let mut form = el("form")
        .attr("class", "controls")
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
    if !pull.draft {
        let button = text("button", "emergency landing");
        form = form.kid(button.attr("formaction", "/land"));
    }
    form
}

/// One flight strip. Its state goes into data attributes, and the style
/// sheet draws it.
fn card(view: &View, pull: &Pull, layout: &Layout, lane: &str) -> El {
    let Config { web, repo, .. } = view.config;
    let link = el("a")
        .attr("href", format!("{web}/{repo}/pull/{}", pull.number))
        .kid(text("b", format!("#{}", pull.number)))
        .text(format!(" {}", pull.title));
    let author = text("span", pull.author.as_str()).attr("class", "author");
    let mut links = [Vec::new(), Vec::new()];
    for overlap in &pull.overlaps {
        let kind = usize::from(overlap.kind == Kind::Conflict);
        links[kind].push(overlap.other.to_string());
    }
    let [shares, conflicts] = links.map(|numbers| numbers.join(" "));
    let mut card = el("article")
        .attr("id", format!("pr-{}", pull.number))
        .attr("data-lane", lane)
        .attr("data-ci", pull.ci.as_deref().unwrap_or("none"))
        .attr("data-shares", shares)
        .attr("data-conflicts", conflicts);
    let flags = [
        ("data-draft", pull.draft),
        ("data-waits", pull.waits),
        ("data-merging", pull.merging),
        ("data-stuck", pull.stuck),
        ("data-untrusted", !pull.trusted),
        ("data-blocks", layout.blocking.contains(&pull.number)),
        ("data-blocked", layout.blocked.contains(&pull.number)),
    ];
    for (name, on) in flags {
        card = flag(card, name, on);
    }
    if let Some(position) = pull.queued {
        card = card.attr("data-queued", position.to_string());
    }
    card = card
        .kid(el("header").kid(link).kid(author))
        .kid(tags(view.config, pull))
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
    let bar = el("span")
        .attr("class", "meter")
        .attr("style", format!("--share:{}%", share.min(100)));
    flag(bar, "data-near", share >= NEAR)
        .kid(el("i"))
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
        .kid(
            el("meta")
                .attr("name", "viewport")
                .attr("content", "width=device-width"),
        )
        .kid(
            el("link")
                .attr("rel", "stylesheet")
                .attr("href", "/app.css"),
        )
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
    let front = lanes.last().and_then(|q| q.first());
    let front = front.map(|pull| queue_page(view.config, &pull.base));
    for (lane, &title) in titles.iter().enumerate() {
        let style = format!("grid-column:{};grid-row:1", lane + 1);
        let head = el("h2").attr("data-lane", title).attr("style", style);
        // The queue title opens GitHub's queue page while the queue has PRs.
        let head = match front.as_ref().filter(|_| lane + 1 == titles.len()) {
            Some(href) => head.kid(text("a", title).attr("href", href)),
            None => head.text(title),
        };
        main = main.kid(head);
    }
    for &(pull, spot) in &layout.cards {
        let card = card(view, pull, &layout, titles[spot.lane]);
        main = main.kid(card.attr("style", area(spot)));
    }
    frame(view.config, view.session, main)
}
