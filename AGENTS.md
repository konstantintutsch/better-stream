# AGENTS.md

Rust GUI app (iced 0.14, tokio) for viewing multiple RTSP streams. Single crate, no workspace, no CI.

## Commands

`just` is the command interface (see `justfile`):

- `just run` — debug build with `RUST_LOG=better_stream=debug` (the app logs every `Message` through `log::debug!`; without `RUST_LOG` you see nothing)
- `just build` — **release** build
- `just test` — depends on `build`, so it does a full release build before `cargo test`

For a fast check during iteration use `cargo check` / `cargo test` directly; `just test` is slow for no benefit. The only tests are the pure helpers in `stream::rtsp` — everything else needs a camera.

Decoding links the **system FFmpeg** through `ffmpeg-next` (found via `pkg-config`, bindings via bindgen/libclang), so the FFmpeg development libraries must be installed (`brew install ffmpeg pkg-config`). `ffmpeg-next` needs its `format` feature even though only codec and scaling are used; it does not compile without it.

## Architecture

- `src/main.rs` — boots `iced::application(Player::boot, Player::update, Player::view)`. Deliberately *not* `#[tokio::main]`: iced runs its own executor, and the one tokio runtime lives inside the worker.
- `src/config.rs` — `Config::load` reads the `[[sources]]` list from `dirs::config_dir()/better-stream/config.toml`. Missing or broken files degrade to an empty list instead of failing.
- `src/ui/app.rs` — `Player` (iced state) and the central `Message` enum.
- `src/ui/threading.rs` — `WorkerSubscription` (a hand-written `iced::advanced::subscription::Recipe`, which is why the `advanced` feature is enabled) plus the `Worker` that runs on a plain `std::thread`.
- `src/stream/rtsp.rs` — `Source` / `Client` state (URL list with `next`/`previous`/`current`) **and** `play`, the retina + FFmpeg pipeline.
- `src/stream/decoder.rs` — `Codec` (retina encoding name → FFmpeg codec) and `Decoder`, an FFmpeg decoder plus swscale conversion to RGBA.

Module files are `ui.rs` / `stream.rs`, not `mod.rs`.

### Playback pipeline

`rtsp::play` describes the session, picks the first video stream whose encoding `Codec::from_encoding_name` knows (H.264, H.265, JPEG — exactly what retina can depacketize), sets it up over TCP and decodes every access unit into an RGBA `rtsp::Frame`.

Things that are easy to get wrong:

- retina emits H.264/H.265 access units with 4-byte length prefixes and reports the AVCDecoderConfigurationRecord / HEVCDecoderConfigurationRecord as `VideoParameters::extra_data`. That is exactly the "avcC/hvcC" layout FFmpeg understands, so `Decoder::new` hands it over as `extradata` and packets are passed through unmodified — no Annex B conversion. Empty extra data is correct for JPEG.
- the decoder is opened lazily at the first random access point, because retina only knows the parameters after the first frame, and reopened when `VideoFrame::has_new_parameters()` is set.
- `avcodec_receive_frame` clears the target frame even when it returns `EAGAIN`, so `Decoder::decode` receives into a scratch frame and swaps on success.

Decoding starts at the first `is_random_access_point()` frame; without that, joining mid group of pictures produces a burst of decode errors. Individual decode errors are logged and skipped rather than ending the session.

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

- H.264, H.265 and MJPEG only (what retina can depacketize; adding a codec means a new `Codec` variant and a retina depacketizer), TCP only, one source connected at a time — the name says "multi-stream", but the UI steps through sources rather than tiling them.
- No automatic reconnect; a dead camera needs the **Reconnect** button.
- Frozen streams are found by eye. There is no staleness timer yet, which would be the natural next feature.

Gotcha: `crate::stream` (our module) collides by name with `iced::stream`; `threading.rs` imports both, so keep the paths qualified.

## Conventions

- Commits follow Conventional Commits (`feat:`, `fix:`, `refactor:`, `style:`, `build:`) and are signed off — use `git commit -s`.
- Edition 2024; dependencies are pinned with `>=` ranges in `Cargo.toml`.
