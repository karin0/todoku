# Todoku

A Telegram bot keeping a task list for one chat. Every line of a message sent there
becomes a task, and the message is deleted once the tasks are stored. The bot keeps a
panel message listing the open tasks with the time each was added, and tapping a task
completes it. Each topic of a forum keeps its own list and panel.

Tasks live in SQLite. A completed task moves from the `Todo` table to `Done`, and the
`Panel` table records the panel message of each topic, so a restarted bot edits the
panels it left. A panel Telegram refuses to edit is sent again and the old one deleted.

The panel is a rich message showing each time as an absolute `tg-time` entity. A
relative one makes Telegram Desktop lay the panel out again at every tick, with the
work doubling each time, until the client freezes.

`/start` sends a fresh panel, or the usage when the list is empty, and `/help` sends
the usage. An update from any other chat is reported to the allowed chat. Updates that
arrived while the bot was down are discarded at startup.

## Configuration

The bot reads its environment:

- `TELEGRAM_BOT_TOKEN`, required.
- `ALLOWED_CHAT_ID`, required, the one chat the bot serves.
- `TELEGRAM_API_BASE_URL`, defaulting to `https://api.telegram.org`.
- `DB_PATH`, defaulting to `todo.db` in the working directory.

Task times in a panel's fallback text use the system time zone.

## Running

```sh
cargo build --release
TELEGRAM_BOT_TOKEN=... ALLOWED_CHAT_ID=... target/release/todoku
```

The default `tls` feature lets the bot reach an `https` API server. A bot talking to a
local Bot API server over plain HTTP builds with `--no-default-features`. SIGINT or
SIGTERM stops the bot, and a second one aborts it.

## Checks

`./check.sh` runs shellcheck, formatting, clippy with and without `tls`, and the tests.
The lint levels live in `Cargo.toml`, so a bare `cargo clippy` enforces the same set.
