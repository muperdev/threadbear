// The connection to the runner: JSON-RPC 2.0 over its WebSocket
// (docs/protocol.md). It signs in with the challenge (auth.rs), says hello,
// passes requests through and hands notifications on, and reconnects by
// itself when the runner restarts. The interface only ever sees results
// and notifications, never the token.
use crate::auth::{self, Role};
use crate::config::RunnerConfig;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

const CLIENT_NAME: &str = "threadbear-desktop";
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    Connecting { url: String },
    #[serde(rename_all = "camelCase")]
    Connected { url: String, hello: Value },
    #[serde(rename_all = "camelCase")]
    Disconnected { url: String, reason: String, retry_in_ms: u64 },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    fn local(message: impl Into<String>) -> Self {
        RpcError { code: -1, message: message.into(), data: None }
    }
}

/// Where status changes and notifications go: the app's event channel, or a
/// channel in tests.
pub trait Sink: Send + Sync + 'static {
    fn status(&self, status: &Status);
    fn notification(&self, method: &str, params: &Value);
}

type Pending = HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>;

pub struct Client {
    config: RunnerConfig,
    sink: Box<dyn Sink>,
    out: Mutex<Option<mpsc::UnboundedSender<Message>>>,
    pending: Mutex<Pending>,
    next_id: AtomicU64,
    status: Mutex<Status>,
    /// Wakes the reconnect wait when someone presses Try again.
    retry: tokio::sync::Notify,
}

impl Client {
    pub fn new(config: RunnerConfig, sink: impl Sink) -> Arc<Self> {
        let url = config.url();
        Arc::new(Client {
            config,
            sink: Box::new(sink),
            out: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            // 1 is auth.prove's.
            next_id: AtomicU64::new(2),
            status: Mutex::new(Status::Connecting { url }),
            retry: tokio::sync::Notify::new(),
        })
    }

    /// "Try again" in the connection banner: stop waiting and reconnect now.
    pub fn retry_now(&self) {
        self.retry.notify_one();
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, status: Status) {
        *self.status.lock().unwrap() = status.clone();
        self.sink.status(&status);
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        let text = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        {
            let out = self.out.lock().unwrap();
            let Some(out) = out.as_ref() else {
                return Err(RpcError::local("Not connected to the runner"));
            };
            self.pending.lock().unwrap().insert(id, tx);
            if out.send(Message::text(text)).is_err() {
                self.pending.lock().unwrap().remove(&id);
                return Err(RpcError::local("Not connected to the runner"));
            }
        }
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RpcError::local("The connection to the runner closed")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(RpcError::local(format!("The runner didn't answer {method} in {} seconds", CALL_TIMEOUT.as_secs())))
            }
        }
    }

    /// Connects, and connects again whenever the connection drops, for as
    /// long as the app runs.
    pub async fn run(self: Arc<Self>) {
        let mut backoff_ms = 500;
        loop {
            let url = self.config.url();
            self.set_status(Status::Connecting { url: url.clone() });
            let reason = match self.clone().session(&url).await {
                Ok(()) => {
                    backoff_ms = 500;
                    "The runner closed the connection".to_string()
                }
                Err(e) => e,
            };
            self.out.lock().unwrap().take();
            for (_, tx) in self.pending.lock().unwrap().drain() {
                let _ = tx.send(Err(RpcError::local("The connection to the runner closed")));
            }
            self.set_status(Status::Disconnected { url, reason, retry_in_ms: backoff_ms });
            // "Try again" in the connection banner cuts the wait short.
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                _ = self.retry.notified() => {}
            }
            backoff_ms = (backoff_ms * 2).min(10_000);
        }
    }

    /// One connection, from dialing to closing. Ok once it was signed in
    /// and then closed; Err with what went wrong before that.
    async fn session(self: Arc<Self>, url: &str) -> Result<(), String> {
        let token = self.config.read_token()?;
        let (ws, _) = tokio_tungstenite::connect_async(url).await.map_err(|e| format!("Couldn't reach the runner at {url}: {e}"))?;
        let (mut write, mut read) = ws.split();

        // The challenge, our proof, and the runner's proof back.
        let challenge = tokio::time::timeout(AUTH_TIMEOUT, next_json(&mut read))
            .await
            .map_err(|_| "The runner didn't send an auth challenge".to_string())??;
        let runner_nonce = challenge
            .pointer("/params/nonce")
            .and_then(Value::as_str)
            .filter(|n| challenge.get("method").and_then(Value::as_str) == Some("auth.challenge") && auth::is_nonce(n))
            .ok_or("The runner didn't send an auth challenge, so it isn't the runner or it's older than this app")?
            .to_string();
        // A loopback client binds its proof to the port it dialed.
        let port = self.config.port;
        let client_nonce = auth::new_nonce();
        let prove = json!({
            "jsonrpc": "2.0", "id": 1, "method": "auth.prove",
            "params": { "nonce": client_nonce, "proof": auth::proof(&token, Role::Client, &runner_nonce, &client_nonce, "primary", port) }
        });
        write.send(Message::text(prove.to_string())).await.map_err(|e| e.to_string())?;
        let answer = tokio::time::timeout(AUTH_TIMEOUT, next_json(&mut read))
            .await
            .map_err(|_| "The runner didn't answer the sign-in".to_string())??;
        if let Some(err) = answer.get("error") {
            return Err(format!("The runner turned down the sign-in: {}", err.get("message").and_then(Value::as_str).unwrap_or("no reason given")));
        }
        let expected = auth::proof(&token, Role::Runner, &runner_nonce, &client_nonce, "primary", port);
        let theirs = answer.pointer("/result/proof").and_then(Value::as_str).unwrap_or("");
        if !auth::proof_matches(&expected, theirs) {
            return Err("Whatever is on the runner's port couldn't prove it's the runner, so this app won't talk to it".into());
        }

        // Signed in: a writer fed by `call`, and this task reading.
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        *self.out.lock().unwrap() = Some(tx);
        let writer = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(msg).await.is_err() {
                    break;
                }
            }
            let _ = write.close().await;
        });

        let me = self.clone();
        let url_owned = url.to_string();
        let hello = tokio::spawn(async move {
            let params = json!({ "client": CLIENT_NAME, "version": env!("CARGO_PKG_VERSION") });
            match me.call("hello", params).await {
                Ok(hello) => me.set_status(Status::Connected { url: url_owned, hello }),
                Err(e) => eprintln!("hello failed: {}", e.message),
            }
        });

        while let Some(msg) = read.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => continue,
            };
            let Ok(v) = serde_json::from_str::<Value>(text.as_str()) else { continue };
            self.dispatch(v);
        }
        hello.abort();
        self.out.lock().unwrap().take();
        let _ = writer.await;
        Ok(())
    }

    fn dispatch(&self, v: Value) {
        let id = v.get("id").and_then(Value::as_u64);
        let is_reply = v.get("result").is_some() || v.get("error").is_some();
        if let (Some(id), true) = (id, is_reply) {
            if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                let result = match v.get("error") {
                    Some(e) => Err(RpcError {
                        code: e.get("code").and_then(Value::as_i64).unwrap_or(-32000),
                        message: e.get("message").and_then(Value::as_str).unwrap_or("The runner gave no reason").to_string(),
                        data: e.get("data").cloned(),
                    }),
                    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(result);
            }
        } else if let Some(method) = v.get("method").and_then(Value::as_str) {
            self.sink.notification(method, v.get("params").unwrap_or(&Value::Null));
        }
    }
}

async fn next_json<S>(read: &mut S) -> Result<Value, String>
where
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).map_err(|e| format!("The runner sent something that isn't JSON: {e}")),
            Ok(Message::Close(_)) => return Err("The runner closed the connection while signing in".into()),
            Ok(_) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("The runner closed the connection while signing in".into())
}
