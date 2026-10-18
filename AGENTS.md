# AGENTS.md

Rust GUI app (iced 0.14, tokio) for viewing multiple RTSP streams. Single crate, no workspace, no CI.

## Commands

`just` is the command interface (see `justfile`):

- `just run` — debug build with `RUST_LOG=better_stream=debug` (the app logs every `Message` through `log::debug!`; without `RUST_LOG` you see nothing)
- `just build` — **release** build
- `just test` — depends on `build`, so it does a full release build before `cargo test`

For a fast check during iteration use `cargo check` / `cargo test` directly; `just test` is slow for no benefit. The only tests are the pure helpers in `stream::rtsp` — everything else needs a camera.

The first build compiles OpenH264 from source (`openh264/source`) and takes a couple of minutes; later builds reuse it.

## Architecture

- `src/main.rs` — boots `iced::application(Player::boot, Player::update, Player::view)`. Deliberately *not* `#[tokio::main]`: iced runs its own executor, and the one tokio runtime lives inside the worker.
- `src/config.rs` — `Config::load` reads the `[[sources]]` list from `dirs::config_dir()/better-stream/config.toml`. Missing or broken files degrade to an empty list instead of failing.
- `src/ui/app.rs` — `Player` (iced state) and the central `Message` enum.
- `src/ui/threading.rs` — `WorkerSubscription` (a hand-written `iced::advanced::subscription::Recipe`, which is why the `advanced` feature is enabled) plus the `Worker` that runs on a plain `std::thread`.
- `src/stream/rtsp.rs` — `Source` / `Client` state (URL list with `next`/`previous`/`current`) **and** `play`, the retina + OpenH264 pipeline.

Module files are `ui.rs` / `stream.rs`, not `mod.rs`.

### Playback pipeline

`rtsp::play` describes the session, picks the first `video`/`h264` stream, sets it up over TCP and decodes every access unit into an RGBA `rtsp::Frame`.

Two conversions are easy to forget:

- retina emits NAL units with a 4-byte length prefix, OpenH264 wants Annex B start codes — that is `annex_b`.
- cameras that only advertise `sprop-parameter-sets` in the SDP never repeat SPS/PPS in band, so the decoder is primed from `VideoParameters::extra_data` via `parameter_sets` (an AVCDecoderConfigurationRecord).

Decoding starts at the first `is_random_access_point()` frame; without that, joining mid group of pictures produces a burst of OpenH264 errors. Individual decode errors are logged and skipped rather than ending the session.

Frames go through a `tokio::sync::mpsc` channel of size 1 and are sent with `try_send`, so a busy UI drops pictures instead of stalling the connection.

### Threading model (easy to get wrong)

Two different channel types, one per direction:

- UI → worker: `std::sync::mpsc::Sender<Message>`, stored in `Arc<Mutex<Option<...>>>` and filled in only when the subscription starts. `Player::send_worker` silently drops messages before that.
- Worker → UI: `iced::futures::channel::mpsc::Sender<Message>`, written with `block_on` in `Worker::send`.

State (`rtsp::Client`) is shared as `Arc<Mutex<_>>` and mutated by the worker, so **the UI must not read it until the worker reports completion**. Every branch in `Worker::run` must end in a reply message (`WorkerInitialized` / `WorkerFinished` / `WorkerIgnored`); commit `0ec7b66` fixed stale UI caused by a missing reply.

`Player::update` handles its own messages explicitly and relays everything else to the worker via the `_ =>` arm. A new UI-only `Message` variant must be matched *before* that catch-all, otherwise it is forwarded and bounces back as `WorkerIgnored`.

The `Worker` owns a `tokio::runtime::Runtime` because iced does not provide one. `Worker::play` aborts the previous `JoinHandle` before spawning the next, which is what makes `Next` / `Previous` / `Reconnect` tear down the old session.

`Message::StreamFrame` is logged at `trace`, not `debug`, so that one line per picture does not bury every other message.

## Current state

Feature complete for the stated purpose: configured sources are connected, decoded and displayed, with status text for the connecting, failed and ended cases.

Known limitations, all deliberate:

- H.264 only, TCP only, one source connected at a time — the name says "multi-stream", but the UI steps through sources rather than tiling them.
- No automatic reconnect; a dead camera needs the **Reconnect** button.
- Frozen streams are found by eye. There is no staleness timer yet, which would be the natural next feature.

Gotcha: `crate::stream` (our module) collides by name with `iced::stream`; `threading.rs` imports both, so keep the paths qualified.

## Conventions

- Commits follow Conventional Commits (`feat:`, `fix:`, `refactor:`, `style:`, `build:`) and are signed off — use `git commit -s`.
- Edition 2024; dependencies are pinned with `>=` ranges in `Cargo.toml`.
