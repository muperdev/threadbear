// How smoothly the window draws, from GPUI's own frame profiler. Only in a
// build with the `frame-stats` feature (the one installed for Mike, and the
// perf runs): View > Show Frame Rate puts a readout in the corner, and the
// automation's `frame-stats` hands the numbers to features/tools/scroll-perf.ts.
//
// GPUI asks macOS for a frame at every refresh of the screen the window is on
// (a CAMetalDisplayLink with the default rate range), so 120 a second on a
// ProMotion screen, 60 on most external ones, and only while something is
// changing. A frame is late when its work, from the screen's signal to handing
// the frame over, took longer than one refresh: that's a hitch you can see.
use crate::style::px;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// Below this many frames a second nothing is moving: the readout's own
/// redraw is twice a second.
const IDLE_FPS: f64 = 4.;
const SAMPLE_EVERY: Duration = Duration::from_millis(500);

thread_local! {
    /// What `frame-stats` with `reset` saw, so the next report is just what
    /// happened since (the perf tool resets before it starts scrolling).
    static BASELINE: RefCell<Option<FrameDurationSnapshot>> = const { RefCell::new(None) };
}

#[derive(Default, Clone, Copy)]
struct Counts {
    presented: u64,
    late: u64,
    missed: u64,
}

fn counts(frames: &RefreshIntervalFrames) -> Counts {
    Counts {
        presented: frames.frames_on_time + frames.frames_late + frames.frames_unmeasured,
        late: frames.frames_late,
        missed: frames.missed_refreshes,
    }
}

fn hz(frames: &RefreshIntervalFrames) -> Option<f64> {
    frames.refresh_interval.map(|i| (1. / i.as_secs_f64()).round())
}

fn ms(nanos: u64) -> f64 {
    (nanos as f64 / 1e4).round() / 100.
}

/// The same displays' frames in `now`, less what `before` had of them.
fn since(now: &FrameDurationSnapshot, before: Option<&FrameDurationSnapshot>) -> Vec<(RefreshIntervalFrames, Counts)> {
    now.by_refresh_interval
        .iter()
        .map(|frames| {
            let earlier = before.and_then(|b| b.by_refresh_interval.iter().find(|f| f.refresh_interval == frames.refresh_interval));
            let mut frames = frames.clone();
            let mut c = counts(&frames);
            if let Some(earlier) = earlier {
                let e = counts(earlier);
                c = Counts {
                    presented: c.presented.saturating_sub(e.presented),
                    late: c.late.saturating_sub(e.late),
                    missed: c.missed.saturating_sub(e.missed),
                };
                // Histograms only gain counts, so taking the earlier one away
                // leaves exactly the frames since.
                let _ = frames.work.subtract(&earlier.work);
                let _ = frames.draw_duration.subtract(&earlier.draw_duration);
                frames.frames_drawn = frames.frames_drawn.saturating_sub(earlier.frames_drawn);
            }
            (frames, c)
        })
        .filter(|(_, c)| c.presented > 0)
        .collect()
}

/// `frame-stats`: per screen rate the window drew at, the frames and how long
/// their work took, since the last `reset` (or since it opened).
#[cfg(feature = "automation")]
pub(crate) fn report(window: &Window, reset: bool) -> Value {
    let now = window.frame_duration_snapshot();
    let out = BASELINE.with(|b| {
        let b = b.borrow();
        let screens: Vec<Value> = since(&now, b.as_ref())
            .iter()
            .map(|(f, c)| {
                let w = &f.work;
                let d = &f.draw_duration;
                json!({
                    "hz": hz(f),
                    "drawn": f.frames_drawn,
                    "presented": c.presented,
                    "late": c.late,
                    "missedRefreshes": c.missed,
                    "workMs": { "p50": ms(w.value_at_quantile(0.5)), "p90": ms(w.value_at_quantile(0.9)), "p99": ms(w.value_at_quantile(0.99)), "max": ms(w.max()) },
                    "drawMs": { "p50": ms(d.value_at_quantile(0.5)), "p90": ms(d.value_at_quantile(0.9)), "p99": ms(d.value_at_quantile(0.99)), "max": ms(d.max()) },
                })
            })
            .collect();
        json!({ "screens": screens })
    });
    if reset {
        BASELINE.with(|b| *b.borrow_mut() = Some(now));
    }
    out
}

/// View > Show Frame Rate: frames a second over the last half second, the
/// frames in it that were late, and the slowest one's work.
pub(crate) struct FrameRate {
    last: Option<(Instant, FrameDurationSnapshot)>,
    text: SharedString,
    _sampler: Task<()>,
}

impl FrameRate {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sampler = cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(SAMPLE_EVERY).await;
            if this.update_in(cx, |this, window, cx| this.sample(window, cx)).is_err() {
                break;
            }
        });
        Self { last: None, text: "Measuring…".into(), _sampler: sampler }
    }

    fn sample(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = window.frame_duration_snapshot();
        let at = Instant::now();
        if let Some((then, before)) = &self.last {
            let seconds = at.duration_since(*then).as_secs_f64();
            let screens = since(&now, Some(before));
            let c = screens.iter().fold(Counts::default(), |a, (_, c)| Counts {
                presented: a.presented + c.presented,
                late: a.late + c.late,
                missed: a.missed + c.missed,
            });
            // The rate of the screen it drew most of those frames on.
            let screen = screens.iter().max_by_key(|(_, c)| c.presented).and_then(|(f, _)| hz(f));
            let slowest = screens.iter().map(|(f, _)| f.work.max()).max().unwrap_or(0);
            let fps = c.presented as f64 / seconds;
            let of = screen.map(|hz| format!(" of {hz:.0}")).unwrap_or_default();
            self.text = if fps < IDLE_FPS {
                match screen.or_else(|| now.by_refresh_interval.last().and_then(hz)) {
                    Some(hz) => format!("Nothing moving · {hz:.0} Hz screen").into(),
                    None => "Nothing moving".into(),
                }
            } else {
                format!("{fps:.0} fps{of} · {} late · slowest {:.1} ms", c.late, ms(slowest)).into()
            };
            cx.notify();
        }
        self.last = Some((at, now));
    }
}

impl Render for FrameRate {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .bottom(px(10.))
            .right(px(10.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .bg(gpui_kit::black().opacity(0.72))
            .text_color(gpui_kit::white())
            .text_size(px(11.))
            .font_family(cx.theme().mono_font_family.clone())
            .child(self.text.clone())
    }
}
