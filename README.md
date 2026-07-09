# cmux-sidebar-agents

`cmux-sidebar-agents` is a live cmux sidebar monitor for AI agents and notifications. It shows each reported agent with its state, name, workspace/screen breadcrumb, and age, followed by recent notifications. Press Enter to jump to the row's surface.

## Sidebar Plugin Contract

This is an ordinary terminal TUI implementing the same sidebar contract documented by the [cmux-sidebar-fzf reference plugin](https://github.com/manaflow-ai/cmux-sidebar-fzf#sidebar-plugin-contract). It reads `CMUX_TUI_SOCKET` first and accepts `CMUX_MUX_SOCKET` as a legacy fallback. `Esc` never exits the plugin; use `Ctrl-C` to exit.

The plugin polls `list-agents` and `list-workspaces` every two seconds; it does not open a streaming subscription. The server's `list-workspaces` tree carries the current per-surface notification id, level, and unread state, but not its title, body, timestamp, or history. The notification section therefore uses honest level-derived labels such as `Warning notification` with the target breadcrumb. Notifications for the already-active surface and older replaced notifications are not available through the polling schema.

## Features

- Agent states mapped to `●` running, `✔` done, and `⚠` needs attention.
- Needs-attention agents first, then running, then done; newest first within each group.
- Workspace and screen breadcrumbs resolved from each agent's surface.
- Current unread notifications, shown in bold with their level and target breadcrumb.
- Enter jumps through the typed workspace, screen, pane, and tab selection methods.
- `r` forces a refresh; idle refresh runs every two seconds.
- Reconnect screen with exponential backoff and narrow-width middle truncation.

Keys: `Up`/`Down` or `Ctrl-K`/`Ctrl-J` move, `Enter` jumps, `r` refreshes, and `Ctrl-C` exits. `Esc` intentionally does nothing because cmux owns the focus escape.

Agent display names use the reported `--session` value when present, then the surface name/title, then the numeric surface id.

## Standalone Development

Run cmux, find the mux socket path, and pass it to the plugin:

```sh
CMUX_TUI_SOCKET=/path/to/cmux-tui.sock cargo run
```

For a local headless development session:

```sh
cmux-tui --headless --session agents-dev
CMUX_TUI_SOCKET="${TMPDIR:-/tmp}/cmux-tui-$(id -u)/agents-dev.sock" cargo run
```

Running without either socket environment variable is supported and renders a helpful reconnect screen instead of panicking.

Agents appear after a client reports one, for example:

```sh
cmux-tui --session agents-dev report-agent \
  --surface 1 --state working --source socket --session reviewer
```

## Install With cmux

```sh
cmux-tui plugin install https://github.com/manaflow-ai/cmux-sidebar-agents
```

## Build and Test

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```
