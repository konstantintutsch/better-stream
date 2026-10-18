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
name = "Front Door"
url = "rtsp://192.0.2.10:554/stream1"

[[sources]]
name = "Back Yard"
url = "rtsp://192.0.2.11:554/stream1"
username = "admin"
password = "hunter2"
```

A source can show just a part of its picture with `zoom`. `x` and `y` are the
centre of the visible part in percent of the picture width and height, measured
from the top left corner (both default to `50`, the middle). `factor` is the
magnification: `2` shows half of the width and half of the height. The visible
part never leaves the picture, so a centre near an edge is moved inwards, and a
factor below `1` shows the whole picture.

```toml
[[sources]]
name = "Garage Door"
url = "rtsp://192.0.2.12:554/stream1"
zoom = { x = 80, y = 30, factor = 3 }
```

By default the window has a title bar. Set `hide_title_bar` to remove it; the
window then shows a quit button instead. The option has to be placed above the
first `[[sources]]` table and is read at startup.

```toml
hide_title_bar = true

[[sources]]
name = "Front Door"
url = "rtsp://192.0.2.10:554/stream1"
```

The path to the configuration file is shown in the window while no sources are
configured.

## Usage

- **◀** and **▶** switch to another source
- **↻** retries the current source after a camera went away
- **✕** closes the application, only shown when `hide_title_bar` is set

H.264, H.265 and Motion JPEG video is supported. Streams are transported over
TCP.

## Building

Decoding is done by FFmpeg, so its development libraries (`libavcodec`,
`libavformat`, `libswscale`, `libavutil`) and `pkg-config` have to be installed,
for example with `brew install ffmpeg pkg-config`.

```sh
just build
```

See the [`justfile`](justfile) for the remaining commands.
