# AGENTS.md

Rust GUI app (iced 0.14, tokio) for viewing multiple RTSP streams. Single crate, no workspace, no CI.

## Commands

`just` is the command interface (see `justfile`):

- `just run` — debug build with `RUST_LOG=better_stream=debug` (the app logs every `Message` through `log::debug!`; without `RUST_LOG` you see nothing)
- `just build` — **release** build
- `just test` — depends on `build`, so it does a full release build before `cargo test`

For a fast check during iteration use `cargo check` / `cargo test` directly; `just test` is slow for no benefit. There are currently no tests in `src/`.

## Architecture

- `src/main.rs` — boots `iced::application(Player::boot, Player::update, Player::view)` inside `#[tokio::main]`.
- `src/ui/app.rs` — `Player` (iced state) and the central `Message` enum.
- `src/ui/threading.rs` — `WorkerSubscription` (a hand-written `iced::advanced::subscription::Recipe`, which is why the `advanced` feature is enabled) plus the `Worker` that runs on a plain `std::thread`.
- `src/stream/rtsp.rs` — `Source` / `Client` state (URL list with `next`/`previous`/`current`). Module files are `ui.rs` / `stream.rs`, not `mod.rs`.

### Threading model (easy to get wrong)

Two different channel types, one per direction:

- UI → worker: `std::sync::mpsc::Sender<Message>`, stored in `Arc<Mutex<Option<...>>>` and filled in only when the subscription starts. `Player::send_worker` silently drops messages before that.
- Worker → UI: `iced::futures::channel::mpsc::Sender<Message>`, written with `block_on` in `Worker::send`.

State (`rtsp::Client`) is shared as `Arc<Mutex<_>>` and mutated by the worker, so **the UI must not read it until the worker reports completion**. Every branch in `Worker::run` must end in a reply message (`WorkerInitialized` / `WorkerFinished` / `WorkerIgnored`); commit `0ec7b66` fixed stale UI caused by a missing reply.

`Player::update` handles its own messages explicitly and relays everything else to the worker via the `_ =>` arm. A new UI-only `Message` variant must be matched *before* that catch-all, otherwise it is forwarded and bounces back as `WorkerIgnored`.

## Current state

- The `retina` dependency is declared but **not used anywhere**; `stream::rtsp` is custom state, not a retina wrapper. No real RTSP I/O exists yet.
- `Player::boot` contains placeholder sources (`"first"`, `"middle"`, `"last"`) marked with a `TODO` — there is no config or user input path yet.
- `crate::stream` (our module) collides by name with `iced::stream`; `threading.rs` imports both, so keep the paths qualified.

## Conventions

- Commits follow Conventional Commits (`feat:`, `fix:`, `refactor:`, `style:`, `build:`) and are signed off — use `git commit -s`.
- Edition 2024; dependencies are pinned with `>=` ranges in `Cargo.toml`.
