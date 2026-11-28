//! A board that sets pull request tiers as an app.

mod board;
mod bot;
mod cache;
#[cfg(target_arch = "wasm32")]
mod edge;
mod github;
mod html;
mod layout;
mod limits;
mod overlap;
mod page;
mod rates;
mod route;
mod session;
mod unzip;

#[cfg(test)]
mod route_test;

pub use bot::tick;
pub use route::{Request, Response, serve};

/// Draws the board from live data on a developer machine.
#[cfg(not(target_arch = "wasm32"))]
pub mod preview {
    use crate::page::{self, View};
    use crate::session::Session;

    /// The GraphQL query for one page of open pull requests.
    pub const QUERY: &str = crate::github::QUERY;

    /// The board for reply pages of `QUERY`, with buttons that go nowhere.
    pub fn board(repo: &str, pages: Vec<serde_json::Value>) -> String {
        let config = crate::Config {
            repo: repo.into(),
            client_id: String::new(),
            client_secret: String::new(),
            session_key: Vec::new(),
            editors: Vec::new(),
            origin: String::new(),
            api: String::new(),
            web: "https://github.com".into(),
            act: false,
        };
        let session = Session {
            login: "preview".into(),
            id: 0,
            csrf: String::new(),
            expires: 0,
        };
        let pulls = crate::github::preview(pages, "tessera-tiers");
        page::board(&View {
            config: &config,
            session: &session,
            pulls: &pulls,
            faults: "",
        })
    }
}

use board::Pull;
use std::sync::Mutex;

/// Seconds a read of the pull requests serves later requests.
const RECENT_LIFE: u64 = 30;

/// The last read of the pull requests, shared by the requests that one
/// Worker instance serves.
pub struct Recent(Mutex<Read>);

struct Read {
    /// Grows on each change, so a read that started before it is not kept.
    version: u64,
    pulls: Option<(u64, Vec<Pull>)>,
}

impl Default for Recent {
    fn default() -> Recent {
        Recent::new()
    }
}

impl Recent {
    pub const fn new() -> Recent {
        Recent(Mutex::new(Read {
            version: 0,
            pulls: None,
        }))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Read> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn get(&self, now: u64) -> Option<Vec<Pull>> {
        let read = self.lock();
        let (at, pulls) = read.pulls.as_ref()?;
        (now < at + RECENT_LIFE).then(|| pulls.clone())
    }

    fn version(&self) -> u64 {
        self.lock().version
    }

    fn put(&self, version: u64, now: u64, pulls: Vec<Pull>) {
        let mut read = self.lock();
        if read.version == version {
            read.pulls = Some((now, pulls));
        }
    }

    fn clear(&self) {
        let mut read = self.lock();
        read.version += 1;
        read.pulls = None;
    }
}

pub struct Config {
    /// The repository as `owner/name`.
    pub repo: String,
    pub client_id: String,
    pub client_secret: String,
    pub session_key: Vec<u8>,
    /// GitHub user ids that may use the board.
    pub editors: Vec<u64>,
    /// The scheme and host that serve the board.
    pub origin: String,
    pub api: String,
    pub web: String,
    /// The bot changes the merge queue. Otherwise the board shows its plan.
    pub act: bool,
}

/// An outbound HTTP request.
pub struct Call {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<String>,
}

pub struct Reply {
    pub status: u16,
    /// The `Location` header of a redirect.
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// A failure, with the HTTP status to answer.
#[derive(Debug)]
pub struct Fault {
    pub status: u16,
    pub text: String,
}

impl Fault {
    pub fn new(status: u16, text: impl Into<String>) -> Fault {
        let text = text.into();
        Fault { status, text }
    }
}

/// The runtime under the board.
pub trait Host {
    fn fetch(&self, call: Call) -> impl Future<Output = Result<Reply, Fault>>;
    /// An RS256 signature made with the app key.
    fn sign(&self, text: &[u8]) -> Result<Vec<u8>, Fault>;
    fn nonce(&self) -> [u8; 16];
    /// Seconds since the Unix epoch.
    fn now(&self) -> u64;
    fn recent(&self) -> &Recent;
    /// A value from the store that all Worker instances share.
    fn load(&self, key: &str) -> impl Future<Output = Option<String>>;
    /// Keeps a value in the shared store for `life` seconds. A failure only
    /// costs a rebuild.
    fn store(&self, key: &str, value: String, life: u64) -> impl Future<Output = ()>;
}
