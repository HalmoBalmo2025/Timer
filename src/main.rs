//! Timer: a small always-on-top macOS window with a repeating countdown and
//! time tracking per day, week and month.

mod activity;
mod calendar;
mod clock;
mod countdown;
mod crash;
mod duration;
mod storage;
mod storage_problem;
mod timer;
mod tracker;
mod window;

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::use_wry_event_handler;
use dioxus::html::geometry::ClientPoint;
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use std::io::{self, Write as _};
use std::process::ExitCode;

use countdown::Countdown;
use timer::{Event as TimerEvent, use_timer};
use tracker::TimeTracker;
use window::Expander;

/// Mouse travel (logical px) after which a press on the panel becomes a
/// window drag. Plain clicks and double-clicks stay below it, so they are
/// never swallowed by a drag session.
const DRAG_THRESHOLD: f64 = 4.0;

fn main() -> ExitCode {
    crash::install();
    let config = match window::config() {
        Ok(config) => config,
        Err(error) => {
            match writeln!(io::stderr(), "Timer cannot start: {error}") {
                // A failing stderr must not panic: the exit code still says it.
                Ok(()) | Err(_) => {}
            }
            return ExitCode::FAILURE;
        }
    };
    // Held until the process ends; see `activity`.
    let _precise_timers = activity::keep_timers_precise();
    LaunchBuilder::new()
        .with_cfg(desktop! { config })
        .launch(App);
    // Not reached: on quit, tao ends the process from inside its event
    // loop. The line satisfies the signature.
    ExitCode::SUCCESS
}

#[component]
fn App() -> Element {
    let timer = use_timer();
    let expander = use_context_provider(Expander::new);
    use_future(move || expander.measure());

    // Where the primary button went down on the panel; the press turns into
    // a window drag once the pointer has moved DRAG_THRESHOLD px.
    let mut press_origin = use_signal(|| None::<ClientPoint>);

    // Closing the window or quitting (Cmd+Q) flushes the running session, so
    // a normal exit never loses tracked time.
    use_wry_event_handler(move |event, _| {
        let closing = matches!(
            event,
            Event::LoopDestroyed
                | Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    window_id: _,
                    ..
                }
        );
        if closing {
            timer.dispatch(TimerEvent::Checkpoint);
        }
    });

    rsx! {
        Stylesheet { href: asset!("/assets/main.css") }
        div {
            class: "app",
            // Dragging the panel background moves the window; a double-click
            // expands or collapses it. Buttons and the input stop these
            // events from bubbling up to here.
            onmousedown: move |e| press_origin.set(Some(e.client_coordinates())),
            onmousemove: move |e| {
                let Some(origin) = press_origin() else { return };
                if !e.held_buttons().contains(MouseButton::Primary) {
                    press_origin.set(None);
                    return;
                }
                let point = e.client_coordinates();
                if (point.x - origin.x).abs() >= DRAG_THRESHOLD
                    || (point.y - origin.y).abs() >= DRAG_THRESHOLD
                {
                    press_origin.set(None);
                    dioxus::desktop::window().drag();
                }
            },
            onmouseup: move |_| press_origin.set(None),
            ondoubleclick: move |_| expander.toggle(),
            Countdown {}
            TimeTracker {}
        }
    }
}
