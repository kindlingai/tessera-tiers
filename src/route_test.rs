//! The routes against a fake GitHub.

use crate::session::{self, Session};
use crate::{Call, Config, Fault, Host, Recent, Reply, Request, Response, serve, tick};
use std::cell::RefCell;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

const KEY: &[u8] = b"a test key of thirty-two bytes..";
const HERE: &str = "https://tiers.test";

/// #3 was labelled first. An agent labelled #4.
const PULLS: &str = r#"{"data":{"repository":{"pullRequests":{
"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
{"number":2,"title":"<script>alert(1)</script>","isDraft":true,"mergeable":"MERGEABLE",
 "author":{"login":"ann","__typename":"User"},
 "labels":{"nodes":[{"name":"priority"},{"name":"needs-human-review"}]},
 "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"FAILURE"}}}]},
 "timelineItems":{"nodes":[{"createdAt":"2026-11-02T00:00:00Z",
  "label":{"name":"priority"},"actor":{"login":"tiers","__typename":"Bot"}}]},
 "comments":{"nodes":[{"author":{"login":"github-actions","__typename":"Bot"},
  "body":"<!-- conflict-notice -->\n### Overlap with other open pull requests\nA conflict line names files that conflict with that PR.\nThe PR that merges second must rebase. A shared line names\nfiles both PRs change that merge cleanly. This notice fails no check.\n\n- conflict #3: `a/<b>.rs`\n- shared #4: `c.rs` `d.rs`\n"}]}},
{"number":3,"title":"first","isDraft":false,"mergeable":"MERGEABLE","author":null,
 "autoMergeRequest":{"enabledAt":"2026-11-27T00:00:00Z"},
 "labels":{"nodes":[{"name":"priority"}]},"commits":{"nodes":[]},
 "timelineItems":{"nodes":[{"createdAt":"2026-11-01T00:00:00Z",
  "label":{"name":"priority"},"actor":{"login":"tiers","__typename":"Bot"}}]},
 "comments":{"nodes":[{"author":{"login":"ann","__typename":"User"},
  "body":"<!-- conflict-notice -->\n- conflict #9: `forged.rs`"}]}},
{"number":5,"title":"chore(main): release 1.0.0","headRefName":"release-please--branches--main",
 "isDraft":false,"mergeable":"MERGEABLE","author":null,"labels":{"nodes":[]},
 "commits":{"nodes":[]},"timelineItems":{"nodes":[]},"comments":{"nodes":[]}},
{"number":4,"title":"agent","baseRefName":"main","isDraft":false,"mergeable":"CONFLICTING","author":null,
 "mergeQueueEntry":{"position":2},
 "labels":{"nodes":[{"name":"landing"}]},"commits":{"nodes":[]},
 "timelineItems":{"nodes":[{"createdAt":"2026-11-03T00:00:00Z",
  "label":{"name":"landing"},"actor":{"login":"mmastrac","__typename":"User"}}]},
 "comments":{"nodes":[]}}]}}}}"#;

/// Priority #10 passes its checks and conflicts with queued #11 and with
/// landing #13. Landing #12 passes and conflicts with nothing.
const QUEUE: &str = r#"{"data":{"repository":{"pullRequests":{
"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
{"id":"P10","number":10,"title":"hot","headRefOid":"h10","baseRefName":"main","isDraft":false,
 "isMergeQueueEnabled":true,"mergeable":"MERGEABLE","author":null,
 "labels":{"nodes":[{"name":"priority"}]},
 "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]},
 "timelineItems":{"nodes":[{"createdAt":"2026-11-02T00:00:00Z",
  "label":{"name":"priority"},"actor":{"login":"tiers","__typename":"Bot"}}]},
 "comments":{"nodes":[{"author":{"login":"github-actions","__typename":"Bot"},
  "body":"<!-- conflict-notice -->\n- conflict #11: `a.rs`"}]}},
{"id":"P11","number":11,"title":"queued","headRefOid":"h11","baseRefName":"main","isDraft":false,
 "isMergeQueueEnabled":true,"mergeable":"MERGEABLE","author":null,
 "autoMergeRequest":{"enabledAt":"2026-11-27T00:00:00Z"},"mergeQueueEntry":{"position":1},
 "labels":{"nodes":[]},"commits":{"nodes":[]},"timelineItems":{"nodes":[]},"comments":{"nodes":[]}},
{"id":"P12","number":12,"title":"free","headRefOid":"h12","baseRefName":"main","isDraft":false,
 "isMergeQueueEnabled":true,"mergeable":"MERGEABLE","author":null,
 "labels":{"nodes":[{"name":"landing"}]},
 "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]},
 "timelineItems":{"nodes":[{"createdAt":"2026-11-03T00:00:00Z",
  "label":{"name":"landing"},"actor":{"login":"tiers","__typename":"Bot"}}]},
 "comments":{"nodes":[]}},
{"id":"P13","number":13,"title":"held","headRefOid":"h13","baseRefName":"main","isDraft":false,
 "isMergeQueueEnabled":true,"mergeable":"MERGEABLE","author":null,
 "labels":{"nodes":[{"name":"landing"}]},
 "commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]},
 "timelineItems":{"nodes":[{"createdAt":"2026-11-01T00:00:00Z",
  "label":{"name":"landing"},"actor":{"login":"tiers","__typename":"Bot"}}]},
 "comments":{"nodes":[{"author":{"login":"github-actions","__typename":"Bot"},
  "body":"<!-- conflict-notice -->\n- conflict #10: `b.rs`"}]}}]}}}}"#;

/// Two PRs opened and one merged in the fake clock's hour, and a release PR.
const PRS: &str = r#"[
{"updated_at":"1970-01-01T00:10:00Z","created_at":"1970-01-01T00:01:00Z",
 "merged_at":"1970-01-01T00:09:00Z","head":{"ref":"fix"}},
{"updated_at":"1970-01-01T00:05:00Z","created_at":"1970-01-01T00:02:00Z",
 "merged_at":null,"head":{"ref":"feat"}},
{"updated_at":"1970-01-01T00:04:00Z","created_at":"1970-01-01T00:03:00Z",
 "merged_at":null,"head":{"ref":"release-please--branches--main"}}]"#;

/// Usage at the default limit of 10 GiB.
const USAGE: &str = r#"{"active_caches_size_in_bytes":10737418240,"active_caches_count":3}"#;
const CACHES: &str = r#"{"actions_caches":[
{"ref":"refs/pull/7/merge","size_in_bytes":1073741824,"last_accessed_at":"2026-11-02"},
{"ref":"refs/heads/main","size_in_bytes":1073741824,"last_accessed_at":"2026-11-01"},
{"ref":"refs/heads/main","size_in_bytes":1073741824,"last_accessed_at":"2026-11-03"}]}"#;

/// Entries from `stickler lint --json` run on tessera.
const LINT: &str = r#"{"tool":"lint","title":"Code shape","rules":{},"findings":[],
"skipped":null,"measures":[
{"rule":"file-lines","path":"engine/models/src/dflash.rs","value":1000,"max":1000},
{"rule":"crate-lines","path":"engine/engine/Cargo.toml","value":12723,"max":15000},
{"rule":"crate-files","path":"engine/gpu/dl/Cargo.toml","value":1,"max":25}]}"#;
const RUNS: &str = r#"{"workflow_runs":[{"id":1,"head_sha":"aaaaaaaaaaaa"},
{"id":2,"head_sha":"bbbbbbbbbbbb"}]}"#;

/// An artifact zip: one deflated entry, with a descriptor.
fn zip(name: &str, data: &[u8]) -> Vec<u8> {
    let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
    let n = name.len() as u32;
    let mut out = Vec::new();
    let mut put = |fields: &[(u32, usize)], tail: &[u8]| {
        for &(v, width) in fields {
            let bytes = v.to_le_bytes();
            out.extend((0..width).map(|i| bytes.get(i).copied().unwrap_or(0)));
        }
        out.extend_from_slice(tail);
    };
    let local = [
        (0x0403_4b50, 4),
        (20, 2),
        (8, 2),
        (8, 2),
        (0, 16),
        (n, 2),
        (0, 2),
    ];
    put(&local, name.as_bytes());
    put(&[], &packed);
    let central = (30 + n as usize + packed.len()) as u32;
    let size = packed.len() as u32;
    let head = [
        (0x0201_4b50, 4),
        (20, 2),
        (20, 2),
        (8, 2),
        (8, 2),
        (0, 8),
        (size, 4),
    ];
    put(&head, &[]);
    put(
        &[(data.len() as u32, 4), (n, 2), (0, 12), (0, 4)],
        name.as_bytes(),
    );
    let count = [
        (0x0605_4b50, 4),
        (0, 4),
        (1, 2),
        (1, 2),
        (46 + n, 4),
        (central, 4),
        (0, 2),
    ];
    put(&count, &[]);
    out
}

/// Records each call as `METHOD path authorization body`.
#[derive(Default)]
struct Fake {
    calls: RefCell<Vec<String>>,
    /// The open pull requests GraphQL answers with, `PULLS` when empty.
    pulls: &'static str,
    recent: Recent,
    store: RefCell<std::collections::BTreeMap<String, String>>,
}

impl Host for Fake {
    async fn fetch(&self, call: Call) -> Result<Reply, Fault> {
        let auth = call.headers.iter().find(|(key, _)| *key == "authorization");
        let auth = auth.map(|(_, value)| value.as_str()).unwrap_or_default();
        let path = call.url.split_once(".test").unwrap().1;
        let zip =
            (call.url == "https://store.test/blob").then(|| zip("lint.json", LINT.as_bytes()));
        let sent = call.body.unwrap_or_default();
        let body = match (call.method, path) {
            ("GET", "/repos/o/r/installation") => r#"{"id":7,"app_slug":"tiers"}"#,
            ("POST", "/app/installations/7/access_tokens") => r#"{"token":"app"}"#,
            ("POST", "/graphql") if sent.contains("pullRequest(number") => {
                r#"{"data":{"repository":{"pullRequest":{"commits":{"nodes":[
                {"commit":{"statusCheckRollup":{"state":"PENDING"}}}]}}}}}"#
            }
            ("POST", "/graphql") if self.pulls.is_empty() => PULLS,
            ("POST", "/graphql") => self.pulls,
            ("GET", "/repos/o/r/pulls/5") => {
                r#"{"state":"open","head":{"sha":"tip"},"labels":[{"name":"landing"}]}"#
            }
            ("POST", "/login/oauth/access_token") => r#"{"access_token":"user"}"#,
            ("GET", "/user") => r#"{"login":"Matt","id":7}"#,
            ("GET", "/repos/o/r/actions/cache/usage") => USAGE,
            ("GET", path) if path.contains("/pulls?state=all") && path.ends_with("page=1") => PRS,
            ("GET", "/repos/o/r/actions/caches?per_page=100&page=1") => CACHES,
            ("GET", path) if path.contains("/runs?branch=main&event=push") => RUNS,
            ("GET", "/repos/o/r/actions/runs/2/artifacts?name=checks-lint") => {
                r#"{"artifacts":[{"id":5}]}"#
            }
            _ => "[]",
        };
        let line = format!("{} {path} {auth} {sent}", call.method);
        self.calls.borrow_mut().push(line);
        // The landing label is gone when the delete arrives.
        let moved = path.ends_with("/artifacts/5/zip");
        let status = match (call.method, path.ends_with("/storage-limit")) {
            _ if moved => 302,
            ("DELETE", _) => 404,
            (_, true) => 402,
            _ => 200,
        };
        Ok(Reply {
            status,
            location: moved.then(|| "https://store.test/blob".into()),
            body: zip.unwrap_or_else(|| body.into()),
        })
    }

    fn sign(&self, _: &[u8]) -> Result<Vec<u8>, Fault> {
        Ok(vec![1])
    }

    fn nonce(&self) -> [u8; 16] {
        [7; 16]
    }

    fn now(&self) -> u64 {
        1000
    }

    fn recent(&self) -> &Recent {
        &self.recent
    }

    async fn load(&self, key: &str) -> Option<String> {
        self.store.borrow().get(key).cloned()
    }

    async fn store(&self, key: &str, value: String, _: Option<u64>) {
        self.store.borrow_mut().insert(key.into(), value);
    }
}

fn wait<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

fn config() -> Config {
    Config {
        repo: "o/r".into(),
        client_id: "id".into(),
        client_secret: "secret".into(),
        session_key: KEY.into(),
        editors: vec![7],
        origin: HERE.into(),
        api: "https://api.test".into(),
        web: "https://web.test".into(),
    }
}

fn send(fake: &Fake, request: &Request) -> Response {
    wait(serve(fake, &config(), request))
}

/// A request with a session for `login`.
fn request(method: &str, path: &str, login: &str, body: &str) -> Request {
    let session = Session {
        login: login.into(),
        id: if login == "Matt" { 7 } else { 9 },
        csrf: "token".into(),
        expires: 2000,
    };
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let mut cookie = format!("__Host-state=st; {}=", session::NAME);
    if !login.is_empty() {
        cookie += &session::seal(KEY, &session);
    }
    Request {
        method: method.into(),
        path: path.into(),
        query: query.into(),
        cookie,
        source: HERE.into(),
        body: body.into(),
    }
}

fn ask(fake: &Fake, method: &str, path: &str, login: &str, body: &str) -> Response {
    send(fake, &request(method, path, login, body))
}

#[test]
fn editor_moves_tier() {
    let fake = Fake::default();
    let page = ask(&fake, "GET", "/", "Matt", "").body;
    assert!(page.contains(r#"<button name="tier" value="landing">to landing</button>"#));
    fake.calls.borrow_mut().clear();
    ask(&fake, "GET", "/", "Matt", "");
    assert!(fake.calls.borrow().is_empty());
    let form = "pr=5&tier=priority&csrf=token";
    let done = ask(&fake, "POST", "/tier", "Matt", form);
    assert_eq!(done.status, 303);
    let calls = fake.calls.borrow();
    let labels = "/repos/o/r/issues/5/labels";
    let added = format!(r#"POST {labels} Bearer app {{"labels":["priority"]}}"#);
    assert_eq!(
        calls[3..],
        [added, format!("DELETE {labels}/landing Bearer app ")]
    );
    drop(calls);
    fake.calls.borrow_mut().clear();
    assert_eq!(ask(&fake, "POST", "/automerge", "Matt", form).status, 303);
    assert_eq!(ask(&fake, "POST", "/review", "Matt", form).status, 303);
    let calls = fake.calls.borrow();
    assert!(calls[3].starts_with("POST /graphql Bearer app ") && calls[3].contains("SQUASH"));
    let released = format!("DELETE {labels}/needs-human-review Bearer app ");
    assert_eq!(calls.last(), Some(&released));
    drop(calls);
    ask(&fake, "GET", "/", "Matt", "");
    assert!(
        fake.calls
            .borrow()
            .last()
            .unwrap()
            .starts_with("POST /graphql")
    );
}

#[test]
fn refuses_other_callers() {
    let fake = Fake::default();
    let body = "pr=5&tier=priority&csrf=token";
    assert_eq!(ask(&fake, "POST", "/tier", "", body).status, 401);
    assert_eq!(ask(&fake, "POST", "/tier", "ann", body).status, 403);
    let forged = "pr=5&tier=priority&csrf=guess";
    assert_eq!(ask(&fake, "POST", "/tier", "Matt", forged).status, 403);
    let mut foreign = request("POST", "/tier", "Matt", body);
    foreign.source = "https://evil.test".into();
    assert_eq!(send(&fake, &foreign).status, 403);
    assert_eq!(ask(&fake, "GET", "/", "ann", "").status, 403);
    assert!(ask(&fake, "GET", "/", "", "").body.contains("/login"));
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn board_escapes_text() {
    let page = ask(&Fake::default(), "GET", "/", "Matt", "").body;
    assert!(page.contains("&lt;script&gt;alert(1)") && !page.contains("<script>"));
    let files = r#"<b>#3</b> <span class="files">a/&lt;b&gt;.rs</span>"#;
    assert!(page.contains(files) && page.contains(r#"<b>#4</b> <span class="files">c.rs, d.rs"#));
    assert!(page.contains("label set by mmastrac") && !page.contains("forged.rs"));
    assert!(page.contains("no overlaps") && page.contains("overlaps not checked"));
    let actions = [r#"formaction="/review""#, "auto-merge on"];
    assert!(actions.iter().all(|a| page.contains(a)));
    // Draft #2 is in the left lane and conflicts with #3, which is ahead.
    let spots = [
        r#"data-queued="2" style="grid-column:5/6;grid-row:3/4">"#,
        r#"data-merging="" data-blocks="" style="grid-column:4/5;grid-row:3/4">"#,
        r#"<article id="pr-2" data-lane="draft" data-ci="failure" data-shares="4" data-conflicts="3""#,
        r#"data-draft="" data-waits="" data-blocked="" style="grid-column:1/2;grid-row:4/5">"#,
    ];
    assert!(spots.iter().all(|s| page.contains(s)), "{page}");
    let queued = page.split(r#"id="pr-4""#).nth(1).unwrap();
    assert!(!queued.split("</article>").next().unwrap().contains("<form"));
    assert!(!page.contains("release 1.0.0"));
    assert!(page.contains(r#"src="/app.js""#));
    let queue = r#"<a href="https://web.test/o/r/queue/main">"#;
    assert!(page.contains(&format!("{queue}merge queue #2</a>")));
    assert!(page.contains(&format!("{queue}merge queue</a></h2>")));
    assert!(page.find(r#"id="pr-3""#).unwrap() < page.find(r#"id="pr-2""#).unwrap());
}

#[test]
fn sign_in_sets_session() {
    let fake = Fake::default();
    let refused = ask(&fake, "GET", "/auth?code=c&state=no", "", "");
    assert_eq!(refused.status, 403);
    let done = ask(&fake, "GET", "/auth?code=c&state=st", "", "");
    let cookie = &done.headers.last().unwrap().1;
    assert!(cookie.ends_with("; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=28800"));
    let value = cookie.split_once(';').unwrap().0;
    let value = session::cookie(value, session::NAME).unwrap();
    assert_eq!(session::open(KEY, value, 1000).unwrap().id, 7);
    assert!(session::open(b"other", value, 1000).is_none());
}

#[test]
fn tabs_show_usage() {
    let fake = Fake::default();
    let cache = ask(&fake, "GET", "/cache", "Matt", "").body;
    let main = "<td>refs/heads/main</td><td>2</td><td>2.00 GiB</td><td>2026-11-03</td>";
    assert!(
        cache.contains("100%</span> 10.00 GiB of 10 GiB, GitHub default") && cache.contains(main)
    );
    let limits = ask(&fake, "GET", "/limits", "Matt", "").body;
    let link = r#"<a href="https://web.test/o/r/blob/bbbbbbbbbbbb/engine/models/src/dflash.rs">"#;
    assert!(limits.contains(link), "{limits}");
    let file = r#"dflash.rs</a></td><td>1000 / 1000</td><td><span class="meter" style="--share:100%" data-near="">"#;
    assert!(limits.contains(">bbbbbbbb</a>. 2 of 3 measures") && limits.contains(file));
    let last = fake.calls.borrow().last().cloned().unwrap();
    assert!(last.starts_with("GET /blob  "));
}

#[test]
fn landing_asks_twice() {
    let fake = Fake::default();
    let land = |form: &str| ask(&fake, "POST", "/land", "Matt", form).status;
    assert_eq!(land("pr=5&csrf=token"), 400);
    assert_eq!(land("pr=5&csrf=token&sure=yes"), 409);
    assert_eq!(land("pr=5&csrf=token&sure=yes&red=yes"), 303);
    let merged = r#"PUT /repos/o/r/pulls/5/merge Bearer app {"merge_method":"squash","sha":"tip"}"#;
    assert!(fake.calls.borrow().iter().any(|c| c == merged));
}

#[test]
fn rates_rebuild_hourly() {
    let fake = Fake::default();
    let page = ask(&fake, "GET", "/rates", "Matt", "").body;
    assert!(
        page.contains(r#"title="1970-01-01 00 UTC: 2 opened, 1 merged""#),
        "{page}"
    );
    fake.calls.borrow_mut().clear();
    ask(&fake, "GET", "/rates", "Matt", "");
    let listed = fake
        .calls
        .borrow()
        .iter()
        .any(|c| c.contains("/pulls?state=all"));
    assert!(!listed);
}

#[test]
fn bot_moves_queue() {
    let fake = Fake {
        pulls: QUEUE,
        ..Fake::default()
    };
    let page = ask(&fake, "GET", "/", "Matt", "").body;
    assert!(page.contains("Queue bot plan, not applied") && page.contains(r#"data-bot="dequeue""#));
    wait(tick(&fake, &config())).unwrap();
    let mode = |name: &str| {
        ask(
            &fake,
            "POST",
            "/bot",
            "Matt",
            &format!("mode={name}&csrf=token"),
        )
    };
    assert_eq!(mode("act").status, 303);
    let mut sent = Vec::new();
    for call in fake.calls.borrow().iter() {
        let Some((_, body)) = call.split_once("Bearer app ") else {
            continue;
        };
        let body: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        if let Some(query) = body["query"].as_str().filter(|q| q.starts_with("mutation")) {
            let name = query.split(['{', '(']).nth(2).unwrap();
            sent.push(format!("{name} {}", body["variables"]));
        } else if call.contains("/comments") {
            sent.push(call.split(' ').take(2).collect::<Vec<_>>().join(" "));
        }
    }
    let expected = [
        r#"disablePullRequestAutoMerge {"id":"P11"}"#,
        r#"dequeuePullRequest {"id":"P11"}"#,
        "POST /repos/o/r/issues/11/comments",
        r#"enqueuePullRequest {"head":"h10","id":"P10","jump":true}"#,
        r#"enqueuePullRequest {"head":"h12","id":"P12","jump":false}"#,
    ];
    assert_eq!(sent, expected);
    assert_eq!(mode("hold").status, 303);
    let last = fake.calls.borrow().last().cloned().unwrap();
    assert!(last.contains("/issues/11/comments") && last.contains("holds the merge queue"));
    assert!(
        ask(&fake, "GET", "/", "Matt", "")
            .body
            .contains("Queue bot holds the merge queue")
    );
    assert_eq!(mode("pause").status, 400);
}
