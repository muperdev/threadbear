// Threadbear as a native GPUI app: the runner connection (../client), drawn
// with GPUI and gpui-kit's components. GPUI has no tokio, so the
// connection runs on a runtime of its own and hands its events across on a
// channel; requests run there too and their results come back as tasks.
// Read-outs that exist only for the scripted control (`automation.rs`).
#![cfg_attr(not(feature = "automation"), allow(dead_code, unused_imports))]
mod app;
#[cfg(feature = "automation")]
mod automation;
mod composer;
mod composer_card;
mod composer_chips;
mod files;
mod folders;
#[cfg(feature = "frame-stats")]
mod frame_stats;
mod header;
mod highlights;
mod highlights_project;
mod highlights_store;
mod icons;
mod move_sheet;
mod new_thread;
mod open_gate;
mod patches;
mod prefs;
mod rails;
mod remote;
mod rows;
mod settings;
mod shell;
mod sidebar;
mod slash_palette;
mod style;
mod thread;
mod viewer;

use app::Threadbear;
use gpui_kit::*;
use serde_json::Value;
use settings::Settings;
use std::sync::{Arc, LazyLock};
use threadbear_client::runner::{Client, RpcError, Sink, Status};
use threadbear_core::zoom;
use tokio::sync::mpsc;

pub static RT: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("a tokio runtime"));

pub enum Seen {
    Status(Status),
    Note(String, Value),
}

pub(crate) struct ChannelSink(pub(crate) mpsc::UnboundedSender<Seen>);

impl Sink for ChannelSink {
    fn status(&self, status: &Status) {
        let _ = self.0.send(Seen::Status(status.clone()));
    }
    fn notification(&self, method: &str, params: &Value) {
        let _ = self.0.send(Seen::Note(method.to_string(), params.clone()));
    }
}

/// A request on the connection's runtime, awaited from GPUI's.
pub async fn call(client: Arc<Client>, method: impl Into<String>, params: Value) -> Result<Value, RpcError> {
    let method = method.into();
    RT.spawn(async move { client.call(&method, params).await })
        .await
        .unwrap_or_else(|e| Err(RpcError { code: -1, message: e.to_string(), data: None }))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Where the window opens: where it was left, if that's still on a screen
/// (`MainWindowFrame.track`), else centred at the Swift app's default size.
fn initial_bounds(settings: &Settings, cx: &App) -> Bounds<Pixels> {
    let screens: Vec<zoom::Rect> = cx
        .displays()
        .iter()
        .map(|d| {
            let b = d.bounds();
            zoom::Rect { x: f64::from(b.origin.x), y: f64::from(b.origin.y), width: f64::from(b.size.width), height: f64::from(b.size.height) }
        })
        .collect();
    let saved = settings.saved.window_frame.filter(|f| zoom::on_screen(zoom::Rect { x: f.x, y: f.y, width: f.width, height: f.height }, &screens));
    match saved {
        Some(f) => Bounds { origin: point(px(f.x as f32), px(f.y as f32)), size: size(px(f.width as f32), px(f.height as f32)) },
        None => Bounds::centered(None, size(px(1180.), px(800.)), cx),
    }
}

fn main() {
    // All of Lucide (about 1 MB), so `icons.rs` can name any icon; the default
    // `Assets` has only gpui-kit's own hundred.
    gpui_kit::application().with_assets(gpui_kit::assets::AllAssets).run(|cx| {
        gpui_kit::init(cx);
        let settings = Settings::load();
        let bounds = initial_bounds(&settings, cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // No title bar: the panels' bars are the top of the window,
                // with the traffic lights over the top-left panel's bar and
                // the bars moving the window (ThreadbearApp, BearDock's
                // README says what a hosting window must set).
                titlebar: Some(TitlebarOptions { title: Some("Threadbear".into()), appears_transparent: true, traffic_light_position: Some(point(px(9.), px(10.))) }),
                app_owns_titlebar_drag: true,
                window_min_size: Some(size(px(760.), px(480.))),
                app_id: Some("threadbear".into()),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| Threadbear::new(settings, window, cx)),
        )
        .expect("failed to open the window");
    });
}
