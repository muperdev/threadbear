// The Rust client against the mock runner (app/Tools/mock-runner), the same
// stand-in the Mac app's tests use: it signs in with the challenge, says
// hello, lists the inbox and reads a thread. Needs Node, and the mock's
// node_modules (`npm ci` in app/Tools/mock-runner).
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use threadbear_client::config::RunnerConfig;
use threadbear_client::runner::{Client, Sink, Status};
use tokio::sync::mpsc;

struct Mock {
    child: Child,
    home: PathBuf,
}

impl Drop for Mock {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn start_mock() -> (Mock, RunnerConfig) {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let home = std::env::temp_dir().join(format!("tb-desktop-mock-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&home).unwrap();
    let ready = home.join("ready");
    let child = Command::new("node")
        .arg(repo.join("app/Tools/mock-runner/server.mjs"))
        .args(["--port", "0", "--quiet", "--data-dir"])
        .arg(&home)
        .arg("--ready-file")
        .arg(&ready)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("node on PATH");
    let mock = Mock { child, home: home.clone() };
    let start = Instant::now();
    while !ready.exists() {
        assert!(start.elapsed() < Duration::from_secs(20), "the mock runner didn't start");
        std::thread::sleep(Duration::from_millis(50));
    }
    let port: u16 = std::fs::read_to_string(&ready).unwrap().trim().parse().unwrap();
    (mock, RunnerConfig { home, port })
}

// A counter, not the clock: Windows' timer can give two tests started together
// the same reading, and then one's "wrong token" overwrote the other's.
fn rand_suffix() -> u32 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

enum Seen {
    Status(Status),
    Note(String, Value),
}

struct ChannelSink(mpsc::UnboundedSender<Seen>);

impl Sink for ChannelSink {
    fn status(&self, status: &Status) {
        let _ = self.0.send(Seen::Status(status.clone()));
    }
    fn notification(&self, method: &str, params: &Value) {
        let _ = self.0.send(Seen::Note(method.to_string(), params.clone()));
    }
}

async fn until_connected(rx: &mut mpsc::UnboundedReceiver<Seen>) -> Value {
    let wait = async {
        loop {
            match rx.recv().await {
                Some(Seen::Status(Status::Connected { hello, .. })) => return hello,
                Some(Seen::Status(Status::Disconnected { reason, .. })) => panic!("disconnected: {reason}"),
                Some(_) => continue,
                None => panic!("sink closed"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait).await.expect("connected in time")
}

#[tokio::test]
async fn signs_in_lists_the_inbox_and_reads_a_thread() {
    let (_mock, config) = start_mock();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let client = Client::new(config, ChannelSink(tx));
    tokio::spawn(client.clone().run());

    let hello = until_connected(&mut rx).await;
    assert_eq!(hello["protocol"], 1);

    let list = client.call("threads.list", json!({ "include": ["inbox"] })).await.unwrap();
    let threads = list["threads"].as_array().unwrap();
    assert!(!threads.is_empty(), "the mock has demo threads");
    let id = threads[0]["id"].as_str().unwrap();

    // As the app reads a thread: its history, then everything after it.
    let page = client.call("threads.events", json!({ "threadId": id, "limit": 500 })).await.unwrap();
    assert!(page["events"].as_array().is_some_and(|e| !e.is_empty()));
    let sub = client.call("threads.subscribe", json!({ "threadId": id, "afterSeq": page["lastSeq"] })).await.unwrap();
    assert!(sub["events"].as_array().is_some_and(|e| e.is_empty()));

    let err = client.call("threads.events", json!({ "threadId": "no-such-thread" })).await.unwrap_err();
    assert_eq!(err.code, -32004);
}

#[tokio::test]
async fn refuses_a_runner_that_cannot_prove_itself() {
    let (_mock, config) = start_mock();
    // Same runner, wrong token: the mock rejects our proof, and we never
    // get as far as hello.
    std::fs::write(config.token_path(), "not-the-token").unwrap();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let client = Client::new(config, ChannelSink(tx));
    tokio::spawn(client.clone().run());
    let reason = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Some(Seen::Status(Status::Disconnected { reason, .. })) => return reason,
                Some(Seen::Status(Status::Connected { .. })) => panic!("connected with the wrong token"),
                _ => continue,
            }
        }
    })
    .await
    .unwrap();
    assert!(!reason.is_empty());
    assert!(client.call("threads.list", json!({})).await.is_err());
}
