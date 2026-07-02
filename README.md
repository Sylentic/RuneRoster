# RuneRoster

A native Windows launcher for managing multiple Jagex accounts and launching RuneLite per profile — no browser embedding, no webview, no extra installs.

Add several Jagex accounts, pick a character for each, and launch isolated RuneLite instances side-by-side, all from one small native app built with Rust and [iced](https://github.com/iced-rs/iced).

## Features

- **Multi-account profiles** — add as many Jagex accounts as you like; each gets its own isolated RuneLite settings profile.
- **Native login flow** — logs in through your system browser via Jagex's own OAuth2/OIDC flow; no embedded browser or webview required.
- **Character picker** — pick which character to launch per account.
- **Reauthentication** — if a stored session expires, the affected profile shows a "Log in again" action that refreshes it in place.
- **Copy settings from Default** — copy your main RuneLite profile's plugin configuration into a newly added account so every account starts with the same setup.
- **OSRS news feed** — shows the two latest official Old School RuneScape news posts (with thumbnails) right in the launcher.
- **Secure credential storage** — session credentials are stored in the OS credential store (Windows Credential Manager) via the `keyring` crate, never in plain text.

## Status

Windows-first v1. Linux support is planned as a fast-follow (the code is structured to make that straightforward) but not yet implemented. See [rs-launcher-plan.md](../rs-launcher-plan.md) for the full build plan, architecture notes, and known open items.

## Building

Requires the Rust GNU toolchain on Windows (`stable-x86_64-pc-windows-gnu`) plus a linker (MinGW-w64).

```powershell
cargo build --workspace
cargo test --workspace
cargo run -p runeroster
```

## License

MIT — see [LICENSE](LICENSE).
