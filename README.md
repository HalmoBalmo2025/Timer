# Timer

A lean macOS desktop timer, built with [Rust](https://www.rust-lang.org/) and [Dioxus 0.7](https://dioxuslabs.com/learn/0.7/).

Two features in one window:

- **Countdown** – an editable display in seconds (18 s by default, also with one decimal such as `17.5` – then `17` shows for half a second at the start, after that it counts down in whole seconds; the last second shows `00` for a full second before the round starts again). When a round ends, the next one starts on its own. The field takes up to eight digits before the decimal point; the duration used last is remembered and also used when the field is empty or `0` at Start. **Start** starts the countdown and the time tracking together, **Stop** ends both. **Reset** rewinds the running countdown to the beginning without interrupting the time tracking.
- **Time tracking** – runs while the countdown runs: shows the current session, adds up the days, and sums up the week (Mon–Sun) and the month. A month is made of whole weeks: a week belongs to the month its Sunday falls in. If a week ends in the next month, even with only its Sunday, it already belongs to that month – so a month has four or five weeks. Hours are shown with two digits, with three only from 100 hours on.

The display changes to within a few milliseconds of the full second – also while another app is in front. For this the app registers a "latency-critical" activity with macOS (`src/activity.rs`); without it, macOS throttles the timers of background apps (App Nap) after a minute or two, and the seconds arrive up to 0.1 s late – one second too long, the next too short.

## Usage

Keys in the window: `S` for Start/Stop, `R` for Reset, `Cmd+D` expands and collapses the time tracking.

The app menu adds the usual macOS shortcuts: `Cmd+Q` quits the app, `Cmd+H` hides it, `Cmd+M` minimises it; `Cmd+X`/`Cmd+C`/`Cmd+V`/`Cmd+A` work in the duration field.

System-wide shortcuts that also work while another app is in front:

- `Ctrl+#` – Start/Stop
- `Ctrl++` – Reset (only while the timer runs)

These are the `#` key (left of Return) and the `+` key (right of Ü) on the German keyboard. Since the shortcuts are bound to the physical key, the code names them after their position on the US keyboard: `ctrl+backslash` and `ctrl+bracketright`.

They are registered at start-up through Dioxus' `use_global_shortcut`; macOS asks for no accessibility permission for this. If another app already holds one of the combinations, only that shortcut is lost, and the button's tooltip says it is not available. If the keys in the window stop working, the tooltip says that too.

The window has no title bar, is semi-transparent, stays in front of all other windows (also while another app is active) and opens as a square (250×250 px) flush with the top-left corner of the screen – right below the menu bar and, if the Dock sits on the left, right of it – so that only the countdown is visible. The time tracking sits below it: a double-click on the background or `Cmd+D` expands and collapses the window; it can also be dragged taller at its bottom edge (no taller than its content). The window is moved by grabbing it anywhere on the background (not on the buttons). Quit with `Cmd+Q`; the running session is saved.

## Building it yourself

### Requirements

- macOS
- Xcode Command Line Tools (compiler and linker):

  ```sh
  xcode-select --install
  ```

- Rust through [rustup](https://rustup.rs/). The Rust version is pinned in `rust-toolchain.toml`; rustup installs it on the first build.
- The Dioxus CLI (`dx`) **in the version that matches the `dioxus` crate** (currently 0.7.10, see `Cargo.lock`):

  ```sh
  cargo install dioxus-cli --version 0.7.10 --locked
  ```

  A mismatch between `dx --version` and the `dioxus` crate causes cryptic errors such as `expected (), found String` during hot reload – reinstall `dx` in the matching version if that happens.

Then get the repository:

```sh
git clone https://github.com/HalmoBalmo2025/Timer.git
cd Timer
```

### With the script (recommended)

```sh
./install.sh
```

The script does everything in one go:

1. It removes old asset files from dx's folder, so only the current ones go into the bundle.
2. It builds the release bundle (`dx bundle --package-types macos --release`). The build uses the optimised `[profile.release]` from `Cargo.toml` (`opt-level = 3`, fat LTO, one codegen unit) and takes a few minutes the first time, about a minute after that.
3. It quits a running Timer and waits until it has really gone – the app saves its session while quitting. If a Timer is still running after ten seconds (`dx serve`, for example), the script stops with a message.
4. It replaces `/Applications/Timer.app` with the new version and starts it.
5. It deletes the `target` folder (`cargo clean`), which grows to several GB. The next build therefore starts from scratch.

If the build fails, the script stops before it touches the installed app.

### By hand

The same steps as a single command:

```sh
rm -rf target/dx/Timer/release/macos/Timer.app target/dx/Timer/bundle && \
dx bundle --package-types macos --release && \
(osascript -e 'quit app "Timer"' 2>/dev/null || true) && \
while pgrep -xq Timer; do sleep 0.2; done && \
rm -rf /Applications/Timer.app && \
cp -R target/dx/Timer/bundle/macos/macos/Timer.app /Applications/Timer.app && \
open /Applications/Timer.app && \
cargo clean
```

Unlike the script, the `while` loop waits without a time limit until no Timer runs any more; if `dx serve` is running, quit it first.

To build the app without installing it, only the first two lines are needed; the bundle is then at:

```
target/dx/Timer/bundle/macos/macos/Timer.app
```

> Only the `--release` bundle build creates the `.icns` and with it a visible app icon in the Dock and the Finder.

### Development

```sh
dx serve
```

Starts the app with live reload; changes under `src/` or to `assets/main.css` rebuild it. The Dock icon stays the default Dioxus one here – macOS windows have no icon of their own in development mode. If the installed app is already running, only the one started first saves (see below).

## Data

All tracked times are stored as JSON in the user's home folder:

```
~/Library/Application Support/timer/data.json
```

The `Library` folder is hidden in the Finder. The quickest way there: press `Cmd+Shift+G` in the Finder and enter `~/Library/Application Support/timer`, or in the terminal:

```sh
open ~/Library/Application\ Support/timer
```

Format: `{"version": 1, "days": {"2026-08-25": 3600, ...}, "countdown_secs": 18}` (the format version; seconds per day, keyed by date; and the countdown duration used last, e.g. `18` or `17.5`). Files written by earlier builds have no `version` field yet and count as version 1.

The file is written every 10 seconds while tracking, on Stop, and when the window is closed or the app is quit with `Cmd+Q`. It is first written to a temporary file, which is flushed to the disk and then put in place of the old one – a crash or power loss in the middle of writing never leaves half a file behind. If saving fails, the app tries again on Start, on Stop and when the window closes.

Whole seconds are booked: Reset does not interrupt the tracking; only the started last second of a session (at most 0.99 s per Start/Stop) is dropped on Stop. A session past midnight is split between both days, also on days with a clock change. While the Mac sleeps, neither the countdown nor the tracking runs on: time asleep is not working time. Old days are never deleted – although the app only shows the current week and month, the history stays.

If Timer runs twice (the installed app and `dx serve`, for example), only the one started first saves; the second shows with the yellow dot that it saves nothing. So the two cannot overwrite each other's times. For this, a `data.json.lock` sits next to the file.

If the file cannot be read (edited into a broken state by hand, say, or written by a newer Timer), the app starts empty but **does not overwrite the file**: the dot in the top-right corner turns yellow, and the reason is shown below the time tracking. Restart the app after repairing the file. A failed save is reported the same way.

## Crashes

If the app crashes, it first writes one line with the place in the code to `~/Library/Logs/Timer/panic-<date-time>.log` (listed in the Console app under "Log Reports"). At most the seconds since the last save are lost, that is up to 10 seconds.

## Project structure

```
assets/
  icon.png     App icon (source of the .icns when bundling)
  main.css     Styling of the app
src/
  main.rs      Entry point: starts the window; App component (moving, expanding)
  timer/       Countdown and time tracking as one state machine (mod.rs) and its Dioxus side (shell.rs)
  countdown.rs Countdown component, shortcuts and tooltips
  duration.rs  The duration: input, display, file format, and which second shows when
  tracker.rs   TimeTracker component
  storage.rs   Loading, saving and locking the JSON file; booking time to days
  storage_problem.rs  The messages shown when nothing is saved
  calendar.rs  Which days make up the week and the month
  clock.rs     The one place that reads the clock or waits on it
  crash.rs     Writes the place of a crash to the log
  activity.rs  Keeps macOS from throttling the app's timers
  window.rs    Window configuration (start position, always in front), menu, icon, expanding/collapsing
Cargo.toml     Dependencies and build profiles
Dioxus.toml    Bundle configuration (identifier, publisher, icon)
rust-toolchain.toml  Pinned Rust version
install.sh     Builds the release and installs it in /Applications
LICENSE        The MIT License
```

## License

Timer is released under the [MIT License](LICENSE).
