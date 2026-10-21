//! The borderless desktop window: its configuration, menu, icon, and
//! expanding/collapsing between the countdown square and the full page.

use dioxus::desktop::muda::{self, Menu, PredefinedMenuItem, Submenu};
use dioxus::desktop::tao::window::{BadIcon, Icon};
use dioxus::desktop::{Config, LogicalPosition, LogicalSize, WindowBuilder, window};
use dioxus::prelude::*;
use std::fmt;

/// Initial window size: a square showing just the countdown. The tracker
/// section sits below it and appears when the window is dragged taller.
const WINDOW_WIDTH: f64 = 250.0;
pub(crate) const WINDOW_HEIGHT: f64 = 250.0;

/// Why the window could not be set up.
#[derive(Debug)]
pub(crate) enum WindowError {
    /// The menu bar could not be built.
    Menu(muda::Error),
    /// The bundled icon is not a readable PNG.
    IconDecode(image::ImageError),
    /// The decoded icon has dimensions the window system refuses.
    IconInvalid(BadIcon),
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Menu(source) => write!(f, "cannot build the menu bar: {source}"),
            Self::IconDecode(source) => write!(f, "cannot decode the app icon: {source}"),
            Self::IconInvalid(source) => write!(f, "the app icon is invalid: {source}"),
        }
    }
}

pub(crate) fn config() -> Result<Config, WindowError> {
    // No title bar at all: the window is a borderless, transparent surface
    // and the page paints its own rounded panel. It is moved by dragging the
    // panel background (see `App`), resized at the edges, and quit with
    // Cmd+Q, which still flushes the running session. It floats above all
    // other windows, even while another app is active.
    let window = WindowBuilder::new()
        .with_title("Timer")
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))
        .with_min_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
    // Flush with the top-left corner of the screen. Without a screen to
    // measure, macOS centres the window instead.
    let window = match screen_top_left() {
        Some(top_left) => window.with_position(top_left),
        None => window,
    };
    // Dioxus drops the menu bar for undecorated windows, which would take
    // Cmd+Q with it, so the menu is set explicitly. Order matters:
    // `with_menu` silently ignores the menu once a window without
    // decorations has been set, so it has to come before `with_window`.
    Ok(Config::new()
        .with_menu(app_menu()?)
        .with_window(window)
        .with_icon(app_icon()?))
}

/// Top-left corner of the area windows may use on the main screen: below
/// the menu bar, and right of the Dock if it sits on the left. `None` off
/// the main thread or without a screen.
fn screen_top_left() -> Option<LogicalPosition<f64>> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;

    // The first screen is the one with the menu bar; macOS measures every
    // screen from its bottom-left corner, tao from its top-left corner.
    let screen = NSScreen::screens(MainThreadMarker::new()?).firstObject()?;
    let full = screen.frame();
    let usable = screen.visibleFrame();
    let usable_top = usable.origin.y + usable.size.height;
    Some(LogicalPosition::new(
        usable.origin.x,
        full.size.height - usable_top,
    ))
}

/// Menu bar with the app menu (Quit, Hide, Minimize) and an Edit menu so the
/// usual clipboard shortcuts work in the duration field.
fn app_menu() -> Result<Menu, WindowError> {
    let menu = Menu::new();
    let app = Submenu::new("Timer", true);
    app.append_items(&[
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::minimize(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ])
    .map_err(WindowError::Menu)?;
    let edit = Submenu::new("Edit", true);
    edit.append_items(&[
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ])
    .map_err(WindowError::Menu)?;
    menu.append_items(&[&app, &edit])
        .map_err(WindowError::Menu)?;
    Ok(menu)
}

pub(crate) fn app_icon() -> Result<Icon, WindowError> {
    let bytes = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(bytes)
        .map_err(WindowError::IconDecode)?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).map_err(WindowError::IconInvalid)
}

/// Height of the whole page (countdown plus tracker) in logical pixels,
/// measured in the webview once its stylesheet and fonts are in.
async fn measure_full_height() -> Result<f64, document::EvalError> {
    document::eval(
        r#"
        const links = Array.from(document.querySelectorAll('link[rel="stylesheet"]'));
        await Promise.all(links.map((link) => link.sheet ? Promise.resolve() : new Promise((resolve) => {
            link.addEventListener('load', resolve, { once: true });
            link.addEventListener('error', resolve, { once: true });
        })));
        await document.fonts.ready;
        const app = document.querySelector('.app');
        const tracker = document.querySelector('.tracker');
        const bottomPadding = parseFloat(getComputedStyle(app).paddingBottom) || 0;
        return tracker.getBoundingClientRect().bottom - app.getBoundingClientRect().top + bottomPadding;
        "#,
    )
    .join::<f64>()
    .await
}

/// Expands the window to show the tracker or collapses it back to the
/// square. Shared via context so both the double-click and Cmd+D use it.
#[derive(Clone, Copy)]
pub(crate) struct Expander {
    /// Full page height: caps the window so it cannot be dragged taller than
    /// its content, and is the expanded height.
    full_height: Signal<Option<f64>>,
}

impl Expander {
    pub(crate) fn new() -> Self {
        Self {
            full_height: Signal::new(None),
        }
    }

    fn apply(mut self, height: f64) {
        window().set_max_inner_size(Some(LogicalSize::new(WINDOW_WIDTH, height)));
        self.full_height.set(Some(height));
    }

    pub(crate) async fn measure(self) {
        match measure_full_height().await {
            Ok(height) if height >= WINDOW_HEIGHT => self.apply(height),
            // The script failed, or the page is not laid out yet. The last
            // measured height stays; the next toggle measures again.
            Ok(_) | Err(_) => {}
        }
    }

    pub(crate) fn toggle(self) {
        spawn(async move {
            // Measured each time: the page grows when a storage problem is
            // shown below the tracker.
            self.measure().await;
            let Some(full) = *self.full_height.peek() else {
                return;
            };
            let win = window();
            // `inner_size()` is stale on macOS: wry swaps the window's content
            // view for its own, so tao's view never learns about resizes.
            // The frame size is the content size for a borderless window.
            let current = win.outer_size().to_logical::<f64>(win.scale_factor());
            let target = if current.height + 1.0 < full {
                full
            } else {
                WINDOW_HEIGHT
            };
            // macOS keeps the bottom edge fixed when a window is resized, so
            // pin the top-left corner: expand downwards, collapse upwards.
            // Without a known position the window just keeps macOS' choice.
            let top_left = win.outer_position().ok();
            win.set_inner_size(LogicalSize::new(WINDOW_WIDTH, target));
            if let Some(top_left) = top_left {
                win.set_outer_position(top_left);
            }
        });
    }
}
