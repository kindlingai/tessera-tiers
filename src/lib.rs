//! A board that sets pull request tiers as an app.

mod board;
mod cache;
#[cfg(target_arch = "wasm32")]
mod edge;
mod github;
mod html;
mod limits;
mod overlap;
mod page;
mod route;
mod session;
mod unzip;

#[cfg(test)]
mod route_test;

pub use route::{Request, Response, serve};

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
}
