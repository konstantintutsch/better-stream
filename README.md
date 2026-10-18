# Better Stream

A multi-stream RTSP viewer for quick glances detecting frozen streams.

Better Stream plays one source at a time and lets you step through the list, so
a single window is enough to check whether every camera is still moving.

## Configuration

Sources are read from a TOML file:

| Platform | Path                                                     |
| -------- | -------------------------------------------------------- |
| Linux    | `~/.config/better-stream/config.toml`                    |
| macOS    | `~/Library/Application Support/better-stream/config.toml` |
| Windows  | `%APPDATA%\better-stream\config.toml`                    |

Every source needs a `url`; `username` and `password` are only required when the
camera asks for authentication.

```toml
[[sources]]
url = "rtsp://192.0.2.10:554/stream1"

[[sources]]
url = "rtsp://192.0.2.11:554/stream1"
username = "admin"
password = "hunter2"
```

The path to the configuration file is shown in the window while no sources are
configured.

## Usage

- **Previous** and **Next** switch to another source
- **Reconnect** retries the current source after a camera went away

Only H.264 video is supported, which is what RTSP cameras overwhelmingly use.
Streams are transported over TCP.

## Building

```sh
just build
```

See the [`justfile`](justfile) for the remaining commands.
