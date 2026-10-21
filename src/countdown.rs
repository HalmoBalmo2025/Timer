//! The countdown card: the big number and the duration field, Start/Stop
//! and Reset, and the keys and shortcuts for them.

use dioxus::desktop::{HotKeyState, use_global_shortcut};
use dioxus::prelude::*;

use crate::duration::{format_countdown, sanitize_duration_input};
use crate::timer::{Event, Field, NumberView, TimerHandle};
use crate::window::Expander;

/// The classes of the status dot: `live` (pulsing) while running, `problem`
/// while tracked time is not being saved; the colours are in `main.css`.
pub(crate) const fn status_dot_class(view: NumberView, problem: Option<&str>) -> &'static str {
    match (view, problem) {
        (NumberView::Remaining(_), Some(_)) => "status-dot live problem",
        (NumberView::Remaining(_), None) => "status-dot live",
        (NumberView::Duration | NumberView::Editing, Some(_)) => "status-dot problem",
        (NumberView::Duration | NumberView::Editing, None) => "status-dot",
    }
}

/// The classes of the big number showing `text`: `warning` in the last five
/// seconds of a round (4 … 0), `editable` while the stopped duration can be
/// clicked, and `tenths` (smaller) with a third digit (`17.5`) so it fits
/// the square window.
pub(crate) fn number_class(view: NumberView, text: &str) -> &'static str {
    let with_tenths = text.chars().filter(char::is_ascii_digit).count() > 2;
    match (view, with_tenths) {
        (NumberView::Remaining(seconds), true) if seconds < 5 => "countdown-number tenths warning",
        (NumberView::Remaining(seconds), false) if seconds < 5 => "countdown-number warning",
        (NumberView::Remaining(_) | NumberView::Editing, true) => "countdown-number tenths",
        (NumberView::Remaining(_) | NumberView::Editing, false) => "countdown-number",
        (NumberView::Duration, true) => "countdown-number tenths editable",
        (NumberView::Duration, false) => "countdown-number editable",
    }
}

/// Whether a shortcut works: a system-wide one could be registered, or the
/// window's key listener is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shortcut {
    Active,
    /// Another app holds the combination, the system refused it, or the
    /// webview's key channel closed.
    Unavailable,
}

/// The tooltip of the Start/Stop button: which keys work for it.
pub(crate) const fn start_stop_tooltip(window_keys: Shortcut, global: Shortcut) -> &'static str {
    match (window_keys, global) {
        (Shortcut::Active, Shortcut::Active) => "Key S or Ctrl+#",
        (Shortcut::Active, Shortcut::Unavailable) => "Key S (Ctrl+# is not available)",
        (Shortcut::Unavailable, Shortcut::Active) => "Ctrl+# (the window's keys stopped working)",
        (Shortcut::Unavailable, Shortcut::Unavailable) => {
            "The shortcuts stopped working; click instead"
        }
    }
}

/// The tooltip of the Reset button: which keys work for it.
pub(crate) const fn reset_tooltip(window_keys: Shortcut, global: Shortcut) -> &'static str {
    match (window_keys, global) {
        (Shortcut::Active, Shortcut::Active) => "Key R or Ctrl++",
        (Shortcut::Active, Shortcut::Unavailable) => "Key R (Ctrl++ is not available)",
        (Shortcut::Unavailable, Shortcut::Active) => "Ctrl++ (the window's keys stopped working)",
        (Shortcut::Unavailable, Shortcut::Unavailable) => {
            "The shortcuts stopped working; click instead"
        }
    }
}

impl Shortcut {
    const fn of<T, E>(registration: &Result<T, E>) -> Self {
        match registration {
            Ok(_) => Self::Active,
            Err(_) => Self::Unavailable,
        }
    }
}

/// Keys pressed in the window, as sent by the listener in `Countdown`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WindowKey {
    StartStop,
    Reset,
    ExpandCollapse,
}

impl WindowKey {
    /// The key the listener sent: the key's name, or `cmd+d`. Other keys
    /// are `None`.
    pub(crate) fn parse(key: &str) -> Option<Self> {
        match key {
            "s" | "S" => Some(Self::StartStop),
            "r" | "R" => Some(Self::Reset),
            "cmd+d" => Some(Self::ExpandCollapse),
            _ => None,
        }
    }
}

#[component]
pub(crate) fn Countdown() -> Element {
    let timer = use_context::<TimerHandle>();
    let expander = use_context::<Expander>();

    // System-wide shortcuts that also work while another app is in front:
    // Ctrl+# starts and stops, Ctrl++ resets. global-hotkey matches physical
    // keys, so they are named by their US position: on the German keyboard
    // `#` sits on the Backslash key (left of Return) and `+` on the
    // BracketRight key (right of Ü). Each fires on press and on release, so
    // only the press counts. If another app already holds a combination,
    // registering it fails, and the button's tooltip says so.
    let start_stop_shortcut = Shortcut::of(&use_global_shortcut("ctrl+backslash", move |state| {
        if state == HotKeyState::Pressed {
            timer.start_stop();
        }
    }));
    let reset_shortcut = Shortcut::of(&use_global_shortcut("ctrl+bracketright", move |state| {
        if state == HotKeyState::Pressed {
            timer.dispatch(Event::Reset);
        }
    }));

    // Keyboard shortcuts, independent of which element has focus: the
    // listener lives on the document and never resolves, so the channel
    // back to Rust stays open for the lifetime of the app. Should it close
    // (the script failed, the webview went away), the tooltips say so.
    let mut window_keys = use_signal(|| Shortcut::Active);
    use_future(move || async move {
        let mut keys = document::eval(
            r"
            document.addEventListener('keydown', (e) => {
                if (e.repeat) return;
                if (e.metaKey && !e.ctrlKey && !e.altKey && e.key.toLowerCase() === 'd') {
                    e.preventDefault();
                    dioxus.send('cmd+d');
                    return;
                }
                if (e.metaKey || e.ctrlKey || e.altKey) return;
                dioxus.send(e.key);
            });
            await new Promise(() => {});
            ",
        );
        while let Ok(key) = keys.recv::<String>().await {
            match WindowKey::parse(&key) {
                Some(WindowKey::StartStop) => timer.start_stop(),
                Some(WindowKey::Reset) => timer.dispatch(Event::Reset),
                Some(WindowKey::ExpandCollapse) => expander.toggle(),
                None => {}
            }
        }
        window_keys.set(Shortcut::Unavailable);
    });

    let view = timer.number();
    let storage_problem = timer.storage_problem();
    let dot_class = status_dot_class(view, storage_problem.as_deref());
    let running = match view {
        NumberView::Remaining(_) => Running::Yes,
        NumberView::Duration | NumberView::Editing => Running::No,
    };

    rsx! {
        section { class: "countdown",
            span { class: dot_class, title: storage_problem }
            div { class: "countdown-display",
                CountdownNumber {
                    view,
                    duration_input: timer.duration_input(),
                    on_field: move |field| timer.dispatch(Event::Field(field)),
                }
            }
            div { class: "btn-row",
                button {
                    class: match running {
                        Running::Yes => "btn stop",
                        Running::No => "btn start",
                    },
                    title: start_stop_tooltip(window_keys(), start_stop_shortcut),
                    onmousedown: move |e| e.stop_propagation(),
                    ondoubleclick: move |e| e.stop_propagation(),
                    onclick: move |_| timer.start_stop(),
                    match running {
                        Running::Yes => "Stop",
                        Running::No => "Start",
                    }
                }
                button {
                    class: "btn reset",
                    title: reset_tooltip(window_keys(), reset_shortcut),
                    disabled: match running {
                        Running::Yes => false,
                        Running::No => true,
                    },
                    onmousedown: move |e| e.stop_propagation(),
                    ondoubleclick: move |e| e.stop_propagation(),
                    onclick: move |_| timer.dispatch(Event::Reset),
                    "Reset"
                }
            }
        }
    }
}

/// Whether the countdown runs, for the buttons.
#[derive(Clone, Copy)]
enum Running {
    Yes,
    No,
}

/// The big number: the seconds left while running, otherwise the duration,
/// which turns into a text field when clicked.
#[component]
fn CountdownNumber(
    view: NumberView,
    duration_input: Signal<String>,
    on_field: EventHandler<Field>,
) -> Element {
    let text = match view {
        NumberView::Remaining(seconds) => format_countdown(seconds),
        NumberView::Duration | NumberView::Editing => duration_input.cloned(),
    };
    let class = number_class(view, &text);

    match view {
        // A plain element while running, so pressing on the number drags
        // the window like the rest of the panel.
        NumberView::Remaining(_) => rsx! {
            div { class, NumberText { text } }
        },
        NumberView::Editing => rsx! {
            input {
                r#type: "text",
                inputmode: "decimal",
                class,
                value: "{text}",
                onmounted: move |_| {
                    // Should focusing fail, the field just shows without
                    // the caret; one click puts it there. The `Eval` is
                    // dropped on purpose.
                    document::eval(
                        r"
                        const el = document.querySelector('input.countdown-number');
                        if (el) {
                            el.focus();
                            el.setSelectionRange(el.value.length, el.value.length);
                        }
                        ",
                    );
                },
                onblur: move |_| on_field.call(Field::Showing),
                onmousedown: move |e| e.stop_propagation(),
                ondoubleclick: move |e| e.stop_propagation(),
                oninput: move |e| duration_input.set(sanitize_duration_input(&e.value())),
            }
        },
        NumberView::Duration => rsx! {
            div {
                class,
                onmousedown: move |e| e.stop_propagation(),
                ondoubleclick: move |e| e.stop_propagation(),
                onclick: move |_| on_field.call(Field::Editing),
                NumberText { text }
            }
        },
    }
}

/// A countdown number with its decimal point set in a slimmer span.
#[component]
fn NumberText(text: String) -> Element {
    // One inline wrapper, so the point sits on the digits' baseline instead
    // of being centred as a flex item of its own.
    match text.split_once('.') {
        Some((whole, tenths)) => rsx! {
            span {
                "{whole}"
                span { class: "point", "." }
                "{tenths}"
            }
        },
        None => rsx! {
            span { "{text}" }
        },
    }
}
