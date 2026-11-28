//! The Cloudflare Worker entry point.

use crate::{Call, Config, Fault, Host, Recent, Reply, Request, bot, serve};
use rsa::PrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::{SignatureEncoding, Signer};
use sha2::Sha256;
use std::sync::OnceLock;
use worker::kv::KvStore;
use worker::{
    Context, Env, Fetch, Headers, Method, RequestInit, RequestRedirect, ScheduleContext,
    ScheduledEvent, event,
};

fn fault(error: worker::Error) -> Fault {
    Fault::new(502, error.to_string())
}

static RECENT: Recent = Recent::new();

/// The app key, read once per Worker instance.
static KEY: OnceLock<SigningKey<Sha256>> = OnceLock::new();

struct Edge {
    key: &'static SigningKey<Sha256>,
    store: KvStore,
}

fn bad(name: &str, error: impl std::fmt::Display) -> worker::Error {
    worker::Error::RustError(format!("{name}: {error}"))
}

impl Host for Edge {
    async fn fetch(&self, call: Call) -> Result<Reply, Fault> {
        let headers = Headers::new();
        for (key, value) in &call.headers {
            headers.set(key, value).map_err(fault)?;
        }
        let mut init = RequestInit::new();
        init.with_method(Method::from(call.method.to_string()));
        init.with_headers(headers);
        init.with_body(call.body.map(Into::into));
        // The client follows redirects, so storage never sees a token.
        init.with_redirect(RequestRedirect::Manual);
        let request = worker::Request::new_with_init(&call.url, &init).map_err(fault)?;
        let mut reply = Fetch::Request(request).send().await.map_err(fault)?;
        Ok(Reply {
            status: reply.status_code(),
            location: reply.headers().get("location").map_err(fault)?,
            body: reply.bytes().await.map_err(fault)?,
        })
    }

    fn sign(&self, text: &[u8]) -> Result<Vec<u8>, Fault> {
        Ok(self.key.sign(text).to_vec())
    }

    fn nonce(&self) -> [u8; 16] {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).expect("the runtime has a random source");
        nonce
    }

    fn recent(&self) -> &Recent {
        &RECENT
    }

    async fn load(&self, key: &str) -> Option<String> {
        self.store.get(key).text().await.ok().flatten()
    }

    async fn store(&self, key: &str, value: String, life: u64) {
        if let Ok(put) = self.store.put(key, value) {
            let _ = put.expiration_ttl(life).execute().await;
        }
    }

    fn now(&self) -> u64 {
        worker::Date::now().as_millis() / 1000
    }
}

/// The config and the host, from the Worker's variables and secrets.
fn setup(env: &Env) -> worker::Result<(Config, Edge)> {
    // A pasted secret often brings a stray space or newline.
    let secret = |name: &str| env.secret(name).map(|s| s.to_string().trim().to_string());
    let mut editors = Vec::new();
    for id in secret("EDITORS")?.split(',') {
        editors.push(id.trim().parse().map_err(|e| bad("EDITORS", e))?);
    }
    let config = Config {
        repo: env.var("REPO")?.to_string(),
        client_id: secret("CLIENT_ID")?,
        client_secret: secret("CLIENT_SECRET")?,
        session_key: secret("SESSION_KEY")?.into_bytes(),
        editors,
        origin: env.var("ORIGIN")?.to_string(),
        act: env.var("QUEUE_BOT")?.to_string() == "act",
        api: "https://api.github.com".into(),
        web: "https://github.com".into(),
    };
    if KEY.get().is_none() {
        let key = PrivateKey::from_pkcs1_pem(&secret("APP_KEY")?);
        let _ = KEY.set(SigningKey::new(key.map_err(|e| bad("APP_KEY", e))?));
    }
    let key = KEY.get().expect("the key was set above");
    let store = env.kv("STORE")?;
    Ok((config, Edge { key, store }))
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let done = match setup(&env) {
        Ok((config, edge)) => bot::tick(&edge, &config).await,
        Err(error) => Err(fault(error)),
    };
    if let Err(fault) = done {
        worker::console_error!("queue bot: {}", fault.text);
    }
}

#[event(fetch)]
async fn fetch(
    mut req: worker::Request,
    env: Env,
    _ctx: Context,
) -> worker::Result<worker::Response> {
    let (config, edge) = setup(&env)?;
    let url = req.url()?;
    let header = |name: &str| req.headers().get(name).map(Option::unwrap_or_default);
    let request = Request {
        method: req.method().to_string(),
        path: url.path().into(),
        query: url.query().unwrap_or_default().into(),
        cookie: header("cookie")?,
        source: header("origin")?,
        body: req.text().await?,
    };
    let done = serve(&edge, &config, &request).await;
    let headers = Headers::new();
    for (key, value) in &done.headers {
        headers.append(key, value)?;
    }
    let reply = worker::Response::ok(done.body)?;
    Ok(reply.with_status(done.status).with_headers(headers))
}
