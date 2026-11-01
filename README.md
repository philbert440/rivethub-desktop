# RivetHub desktop

The native RivetHub client for Linux, written in Rust on [GPUI](https://github.com/zed-industries/zed).
It talks to an `rh` node (the runtime lives in
[philbert440/rivethub-runtime](https://github.com/philbert440/rivethub-runtime)) and works with the
sessions, topics, memory and plug-ins of the nodes on that node's mesh.

It runs on Wayland or X11. Plug-ins' web panels also need WebKitGTK 4.1 (see [Install](#install)).

## Install

There are no prebuilt packages: `packaging/linux/install.sh` builds from this checkout and installs
for the current user.

1. Install a Rust toolchain with [rustup](https://rustup.rs). `rust-toolchain.toml` pins
   1.99.1 with clippy and rustfmt, and rustup fetches it on first build.
2. Install the build dependencies. On Debian/Ubuntu these are the packages CI installs, plus
   `pkg-config`, which `install.sh` uses:

   ```sh
   sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
     libfontconfig-dev libfreetype-dev libxcb1-dev libasound2-dev libssl-dev \
     libwebkit2gtk-4.1-dev clang pkg-config
   ```

   On Arch the script asks for `pacman -S webkit2gtk-4.1`.
3. Run the script from the checkout:

   ```sh
   packaging/linux/install.sh
   ```

What `install.sh` does, in order:

- Exits with an install hint unless `pkg-config --exists webkit2gtk-4.1` succeeds.
- Runs `cargo build --release -p ui -p webview-gtk`.
- Installs, under `PREFIX` (default `$HOME/.local`):
  - `bin/rivethub-desktop` (the app),
  - `lib/rivethub-desktop/librivethub_webview.so` (the web panel engine),
  - `share/icons/hicolor/512x512/apps/rivethub-desktop.png`,
  - `share/applications/rivethub-desktop.desktop`.
- Runs `update-desktop-database` on that `applications` directory if it is installed.

It reads two environment variables: `PREFIX` for the install root and `CARGO_TARGET_DIR` for where
cargo put the build (default `./target`). Make sure `$PREFIX/bin` is on your `PATH`.

**WebKitGTK at runtime.** The app itself never links WebKitGTK. The first time a web panel opens, it
loads `librivethub_webview.so` from beside its binary or from `../lib/rivethub-desktop/`. If the
library or WebKitGTK 4.1 is missing, everything else works and the panel says what is missing.

## Connecting to a node

The app does not pair itself. It connects over TLS to an `rh` gateway, by default the one on this
machine, and reaches the other nodes through it:

| Setting | Default | Override |
| --- | --- | --- |
| Gateway URL | `https://127.0.0.1:5174` | `RIVETHUB_URL` |
| CA bundle (PEM) the gateway's certificate is checked against | `~/.rivethub/step-root.crt` | `RIVETHUB_CA` |
| Device certificate, `device.crt` + `device.key` | `~/.config/RivetHub/mtls/` | `RIVETHUB_DEVICE_DIR` |

- The CA bundle is required: if it can't be read or holds no certificates, the app logs the error
  and exits.
- The device certificate is optional. When both files are there the app presents them to every node;
  otherwise it connects without one, and nodes that need one show as "didn't accept this device".
- Other nodes are found from the local node's `/mesh.json` and reached at the URLs it lists, with
  the same CA and device certificate.
- **Settings → Nodes** shows the gateway in use and whether `RIVETHUB_URL` set it, plus each node's
  status, session count and rh release. **Settings → Devices** shows the device certificate and the CA.

Example for a gateway on another machine:

```sh
RIVETHUB_URL=https://node.example:5174 RIVETHUB_CA=/path/to/root.crt rivethub-desktop
```

### Pairing a phone

**Settings → Devices → Pair a phone** connects RivetHub on Android to this node. Type a name for the
phone (1–64 letters, digits, `.`, `_` or `-`) and click **Show pairing code**. Scan the QR code with
the phone's camera, or tap **Scan pairing QR** in the phone app. The code works once and expires;
the card shows the time left and says when the phone has paired. The phone gets its own certificate,
and nothing is typed on it. A node can refuse (pairing off, you aren't its owner, or its rh is too
old), and the card then says why.

## Using it

The left sidebar holds **Workspace**, **Memory**, the pages plug-ins add, and **Settings**. Click the wordmark to fold the sidebar.

**Workspace** has three levels: *Everything* (a TOPICS lane, most recent first, and an
UNCATEGORIZED lane), a *topic*, and a *session*. The bar at the top shows where you are.

- **Side panel** (Ctrl+B): Agents, Past topics and History, with a box that filters as you type
  and searches transcripts on Enter.
- **New session** (Ctrl+T): pick an agent or a harness on a node, a model, and optionally a topic.
- **Topics**: a topic screen lists its subtopics, its History and a New session button. Its rail
  shows the topic's Decisions (add with Enter), a catch-up of what its newest sessions last did,
  what new sessions started here are given, and plug-ins' cards. Drag a session onto a topic to
  file it, or a topic card onto another topic to nest it. Right-click a topic or session for its
  menu: rename, move, merge, close or reopen, mark as history, share, view in Memory, delete, plus
  plug-ins' session actions.
- **Sessions** open in **Chat** or **Terminal** (Ctrl+Shift+T switches; Settings → General picks
  the default).
  - *Chat* streams the transcript with tool calls. It has Allow once / Allow for session / Reject
    for approvals and cards for an agent's questions. You can pick the model and effort, attach
    PNG, JPEG, GIF or WebP images (button, paste or drop), and use "Fork from here" on a turn.
    Sending during a turn queues the message; Ctrl+Shift+Enter steers the turn instead, if the
    harness takes steering. Stop, or Esc twice, stops it.
  - *Terminal* resumes the session's own TUI in a terminal on its node. rh runs one copy at a
    time, so Chat hands over when its turn ends.
- A session that needs you (waiting on an approval or an answer) or finishes a reply can raise a
  desktop notification, optionally with a chime. Ctrl+J jumps to the next one that needs you.

**Memory** has four tabs:

- **Ask**: search what your sessions said, over any time, today, yesterday, or the last 7 or 14 days.
- **Topics**: each topic's wiki page, with editable Decisions and its sessions.
- **Entities**: entities by type, with their pages, aliases and "Mentioned in".
- **Stats**: memory's counts and upkeep, then **To tidy** (proposed topic, entity and type changes,
  obvious-trash sessions, tag proposals) and **Flagged** (jobs memory stopped retrying).

**Settings** tabs: General (new-session defaults, open sessions in Chat or Terminal, jump to
newest, notifications), Agents, Appearance (Dark, Light, Match desktop, Omarchy), Nodes, Devices,
Memory (health and how filing works), Privacy (default sharing for new sessions and topics),
Plug-ins, About (version, preferences and log paths, and **Copy** diagnostics for a bug report).

**Plug-ins** run on a node and are reached through it. Turn one on, and approve what it asks for,
in Settings → Plug-ins, where its own settings form also lives. A running plug-in can add:

- sidebar pages, cards on a topic's rail, items in a session's menu, dialogs and toasts, all drawn
  with the app's own widgets;
- canvases (pan, zoom, select, drag and connect items);
- web panels: the plug-in's own page in WebKitGTK, served from `rhpanel://` and given the app's
  theme. It can only reach what the app passes on from the plug-in.

Files a plug-in hands you are saved to `$XDG_DOWNLOAD_DIR`, else `~/Downloads`.

## Keyboard shortcuts

All bindings are in `bind_keys` in `crates/ui/src/main.rs`. Every one uses Ctrl; there are no
platform-specific variants.

| Keys | Where | Action |
| --- | --- | --- |
| Ctrl+, | anywhere | Open Settings |
| Tab / Shift+Tab | anywhere | Next / previous field or panel |
| Ctrl+T | Workspace | New session |
| Ctrl+B | Workspace | Show or hide the side panel |
| Ctrl+J | Workspace | Open the next session that needs you |
| Ctrl+Up | Workspace | Up one level: session → its topic → Everything |
| Ctrl+Shift+T | session | Switch between Chat and Terminal |
| Ctrl+Shift+Enter | composer | Steer the running turn with the draft |
| Esc, Esc (within 600 ms) | composer | Stop the running turn |
| Esc | topic picker | Close it |
| Esc | renaming a topic, editing a Memory page | Cancel |
| Esc | Decisions input | Cancel |
| Delete / Backspace | plug-in canvas | Delete the selection |
| Enter | plug-in canvas | Activate the one selected item |
| Esc | plug-in canvas | Clear the selection |
| Ctrl+= / Ctrl+- / Ctrl+0 | plug-in canvas | Zoom in / zoom out / fit |
| Enter | text field | Submit (send, save, search) |
| Shift+Enter | text field | New line (multi-line fields) |
| Left / Right / Up / Down | text field | Move (Shift+arrow selects) |
| Home / End | text field | Start / end of the line |
| Backspace / Delete | text field | Delete back / forward |
| Ctrl+A | text field | Select all |
| Ctrl+C or Ctrl+Insert | text field | Copy |
| Ctrl+X | text field | Cut |
| Ctrl+V or Shift+Insert | text field | Paste |

In a **terminal**, Ctrl+J, Tab and Shift+Tab go to the terminal instead of the app. Ctrl+Insert
copies the selection and Shift+Insert pastes. Every other key the app doesn't bind goes to the
terminal. Ctrl+click opens a link, and right-click offers Copy and Paste. In a **web panel**, Tab and
Shift+Tab move through the page, Ctrl+V, Ctrl+Shift+V and Shift+Insert paste the app's clipboard into
it, and other keys go to the page. On a **plug-in canvas**, Shift+drag on empty space box-selects,
Shift+click or Super+click toggles an item, the wheel pans, and Ctrl+wheel or Super+wheel zooms.

## Files

| Path | What |
| --- | --- |
| `$XDG_CONFIG_HOME/rivethub-desktop/preferences.json` | Preferences |
| `$XDG_CONFIG_HOME/rivethub-desktop/forks.json` | Where each fork made here came from |
| `$XDG_STATE_HOME/rivethub-desktop/rivethub-desktop.log` | Log, also on stderr; `RUST_LOG` filters it (default `info`) |
| `$XDG_STATE_HOME/rivethub-desktop/window` | Window size and position |
| `$XDG_RUNTIME_DIR/rivethub-desktop.{lock,sock}` | One app per login: a second launch raises the running window and exits |

`XDG_CONFIG_HOME` falls back to `~/.config` and `XDG_STATE_HOME` to `~/.local/state`.

## Building from source

```sh
cargo build --release -p ui -p webview-gtk   # the app and the web panel engine
./target/release/rivethub-desktop            # or under $CARGO_TARGET_DIR
```

The binary crate is `ui` and its binary is `rivethub-desktop`. `webview-gtk` builds
`librivethub_webview.so` beside it, where the app looks first. The `wire` crate (rh's API types)
comes from rivethub-runtime at the revision pinned in `Cargo.toml`, and GPUI comes from the Zed
repository at tag `v1.25.3`. Both are git dependencies, so the first build needs access to GitHub.

The workspace crates are `model` (domain types), `gateway` (the only crate that speaks rh's
contract), `app` (state and background work), `term` (terminal emulation on `alacritty_terminal`),
`webview` and `webview-gtk` (web panels), and `ui` (the GPUI app).

CI (`.github/workflows/ci.yml`) runs these with `RUSTFLAGS=-D warnings`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

`crates/ui/tests/boundaries.rs` fails if any crate but `gateway` uses `wire`, or if any
`.rs` file passes 600 lines. The smoke tests in `crates/app/tests/smoke.rs` are ignored by default
because they need a running gateway: `cargo test -p app --test smoke -- --ignored --test-threads=1`.

## Licence

Apache-2.0. See [LICENSE](LICENSE).
