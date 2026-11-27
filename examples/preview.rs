//! Writes the board for a repository's open pull requests to a file, with
//! data read through the `gh` CLI. Usage: `preview owner/name out.html`, or
//! `preview owner/name out.html pages.json` to draw saved reply pages.

use serde_json::Value;
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (repo, out) = (&args[1], &args[2]);
    let (owner, name) = repo.split_once('/').expect("owner/name");
    if let Some(saved) = args.get(3) {
        let pages = serde_json::from_str(&std::fs::read_to_string(saved).expect("read"));
        let board = tessera_tiers::preview::board(repo, pages.expect("JSON pages"));
        std::fs::write(out, board).expect("write");
        return;
    }
    let mut pages = Vec::new();
    let mut after = None;
    loop {
        let mut gh = Command::new("gh");
        gh.args(["api", "graphql", "-f"])
            .arg(format!("query={}", tessera_tiers::preview::QUERY))
            .args([
                "-f",
                &format!("owner={owner}"),
                "-f",
                &format!("name={name}"),
            ]);
        if let Some(cursor) = &after {
            gh.args(["-f", &format!("after={cursor}")]);
        }
        let reply = gh.output().expect("gh runs");
        assert!(
            reply.status.success(),
            "{}",
            String::from_utf8_lossy(&reply.stderr)
        );
        let page: Value = serde_json::from_slice(&reply.stdout).expect("JSON");
        let info = &page["data"]["repository"]["pullRequests"]["pageInfo"];
        after = info["endCursor"].as_str().map(String::from);
        let more = info["hasNextPage"].as_bool().unwrap_or(false);
        pages.push(page);
        if !more {
            break;
        }
    }
    std::fs::write(out, tessera_tiers::preview::board(repo, pages)).expect("write");
}
