// Scripted control of the demo, for screenshots and for testing the real
// gestures without touching the mouse, as Swift's ScriptedEvents.swift is:
// `BEARDOCK_EVENTS="down 1062 490;drag 640 420;esc;up;shot /tmp/x.png"`.
// Only built with the `automation` feature. Points are window-local, top-left
// origin. Steps: down x y | drag x y (in ten moves) | move x y | up | right x y
// | middle x y | dragby dx dy (the pointer moved in screen points, for a window that
// follows the pointer) | esc | wait ms | busy ms (block the main thread) | shot path | others prefix (each other window
// as prefix-N.png: a panel's window, its outline) | window n (send what follows
// to that window, 0 being the main one) | pos path (the window's top-left on screen) | quit. Add `BEARDOCK_FRESH=1` to
// start from the default layout without saving and `BEARDOCK_QUIT=1` to quit
// at the end.
use gpui_kit::test::{TestEventExt as _, TestWindowExt as _};
use gpui_kit::*;
use std::time::Duration;

pub fn run(main: AnyWindowHandle, cx: &mut App) {
    let Ok(script) = std::env::var("BEARDOCK_EVENTS") else { return };
    // Only when the script moves the pointer itself; `pointer-test.sh` drives
    // the real pointer and only asks for screenshots.
    let moves = ["down", "drag", "dragby", "move", "right", "middle"];
    threadbear_dock::platform::set_scripted_pointer(script.split(';').any(|s| s.split_whitespace().next().is_some_and(|c| moves.contains(&c))));
    let quit = std::env::var_os("BEARDOCK_QUIT").is_some();
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(Duration::from_millis(700)).await;
        let mut window = main;
        let mut last = point(px(0.), px(0.));
        for step in script.split(';') {
            let parts: Vec<&str> = step.split_whitespace().collect();
            let Some(&cmd) = parts.first() else { continue };
            let arg = |i: usize| parts.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.);
            match cmd {
                "down" => {
                    let p = point(px(arg(1)), px(arg(2)));
                    last = p;
                    let _ = window.update(cx, |_, window, cx| {
                        window.pointer_move(p, None, cx);
                        window.pointer_down(p, MouseButton::Left, cx);
                    });
                }
                "drag" => {
                    let to = point(px(arg(1)), px(arg(2)));
                    for i in 1..=10 {
                        let t = i as f32 / 10.;
                        let p = point(last.x + (to.x - last.x) * t, last.y + (to.y - last.y) * t);
                        let _ = window.update(cx, |_, window, cx| window.pointer_move(p, Some(MouseButton::Left), cx));
                        cx.background_executor().timer(Duration::from_millis(16)).await;
                    }
                    last = to;
                }
                "dragby" => {
                    // A real drag holds the pointer still over the window while the
                    // window moves under it, so a window that follows the pointer
                    // sees the same window-local point each time. So the pointer is
                    // given in screen terms, and each step becomes window-local
                    // against wherever the window is by then.
                    let (dx, dy) = (arg(1), arg(2));
                    let start = window.update(cx, |_, window, _| window.bounds().origin + last).unwrap_or_default();
                    let to = point(start.x + px(dx), start.y + px(dy));
                    for i in 1..=10 {
                        let t = i as f32 / 10.;
                        let at = point(start.x + (to.x - start.x) * t, start.y + (to.y - start.y) * t);
                        let _ = window.update(cx, |_, window, cx| {
                            let local = at - window.bounds().origin;
                            window.pointer_move(local, Some(MouseButton::Left), cx);
                        });
                        cx.background_executor().timer(Duration::from_millis(16)).await;
                    }
                    last = window.update(cx, |_, window, _| to - window.bounds().origin).unwrap_or(last);
                }
                "up" => {
                    let _ = window.update(cx, |_, window, cx| window.pointer_up(last, MouseButton::Left, cx));
                }
                "move" => {
                    last = point(px(arg(1)), px(arg(2)));
                    let p = last;
                    let _ = window.update(cx, |_, window, cx| window.pointer_move(p, None, cx));
                }
                "right" => {
                    let p = point(px(arg(1)), px(arg(2)));
                    last = p;
                    let _ = window.update(cx, |_, window, cx| {
                        window.pointer_move(p, None, cx);
                        window.pointer_down(p, MouseButton::Right, cx);
                        window.pointer_up(p, MouseButton::Right, cx);
                    });
                }
                "middle" => {
                    let p = point(px(arg(1)), px(arg(2)));
                    last = p;
                    let _ = window.update(cx, |_, window, cx| {
                        window.pointer_move(p, None, cx);
                        window.pointer_down(p, MouseButton::Middle, cx);
                        window.pointer_up(p, MouseButton::Middle, cx);
                    });
                }
                "esc" => {
                    let _ = window.update(cx, |_, window, cx| window.key_down("escape", false, cx));
                }
                "busy" => {
                    // Keeps the main thread busy, as a big render or a slow
                    // handler would: the check AGENTS.md asks for on animations.
                    std::thread::sleep(Duration::from_millis(arg(1) as u64));
                }
                "wait" => {
                    cx.background_executor().timer(Duration::from_millis(arg(1) as u64)).await;
                }
                "shot" => {
                    let path = parts.get(1).copied().unwrap_or("/tmp/beardock.png").to_string();
                    let _ = window.update(cx, |_, window, cx| {
                        window.render_frame(cx);
                        match window.render_to_image() {
                            Ok(image) => {
                                if let Err(e) = image.save(&path) {
                                    eprintln!("shot failed: {e}");
                                }
                            }
                            Err(e) => eprintln!("shot failed: {e}"),
                        }
                    });
                }
                "window" => {
                    let n = arg(1) as usize;
                    let windows = cx.update(|cx| cx.windows());
                    match windows.get(n).copied() {
                        Some(w) => window = w,
                        None => eprintln!("no window {n} (there are {})", windows.len()),
                    }
                }
                "others" => {
                    let prefix = parts.get(1).copied().unwrap_or("/tmp/beardock-window").to_string();
                    let windows = cx.update(|cx| cx.windows());
                    let mut n = 0;
                    for w in windows.into_iter().filter(|w| *w != window) {
                        n += 1;
                        let path = format!("{prefix}-{n}.png");
                        let _ = w.update(cx, |_, window, cx| {
                            window.render_frame(cx);
                            match window.render_to_image() {
                                Ok(image) => {
                                    if let Err(e) = image.save(&path) {
                                        eprintln!("shot failed: {e}");
                                    }
                                }
                                Err(e) => eprintln!("shot failed: {e}"),
                            }
                        });
                    }
                }
                "pos" => {
                    // The window's top-left on screen, for a test that drives the
                    // real pointer (desktop/dock/pointer-test.sh).
                    let path = parts.get(1).copied().unwrap_or("/tmp/beardock-pos.txt").to_string();
                    let origin = window.update(cx, |_, window, _| window.bounds().origin).unwrap_or_default();
                    let _ = std::fs::write(&path, format!("{} {}\n", origin.x.as_f32(), origin.y.as_f32()));
                }
                "quit" => {
                    let _ = cx.update(|cx| cx.quit());
                }
                other => eprintln!("unknown step {other}"),
            }
            cx.background_executor().timer(Duration::from_millis(30)).await;
        }
        if quit {
            let _ = cx.update(|cx| cx.quit());
        }
    })
    .detach();
}
