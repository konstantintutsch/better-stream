use iced::futures::StreamExt;
use retina::{client, codec};
use serde::Deserialize;
use std::{fmt, time::Duration};
use tokio::{
    sync::mpsc,
    time::{Instant, timeout, timeout_at},
};
use url::Url;

use super::decoder::{Codec, Decoder};

const USER_AGENT: &str = concat!("better-stream/", env!("CARGO_PKG_VERSION"));

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

/// How long a connection may take to start playing before it is given up on
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a session may go without a new picture before it counts as frozen
const STALL_TIMEOUT: Duration = Duration::from_secs(10);

/// The connection is alive, but no new picture arrived in time.
///
/// Returned by [`play`] so that callers can tell a frozen stream, which is
/// worth reconnecting, from one that failed.
#[derive(Debug)]
pub struct Stalled;

impl fmt::Display for Stalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "No picture for {} seconds", STALL_TIMEOUT.as_secs())
    }
}

impl std::error::Error for Stalled {}

/// Provides a source to access a RTSP stream
#[derive(Default, Debug, Clone, Deserialize)]
pub struct Source {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub name: Option<String>,
    pub zoom: Option<Zoom>,
}

/// Part of the picture to show, relative to the size of the picture
///
/// `x` and `y` are the centre of the visible part in percent, measured from the
/// top left corner. `factor` is the magnification: 2 shows half the width and
/// half the height. The visible part never leaves the picture, so a centre close
/// to an edge is moved inwards.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Zoom {
    #[serde(default = "Zoom::centre")]
    pub x: f32,
    #[serde(default = "Zoom::centre")]
    pub y: f32,
    pub factor: f32,
}

impl Zoom {
    fn centre() -> f32 {
        50.0
    }

    /// Visible part of a picture of `width` x `height` as `(left, top, width, height)` in pixels
    pub fn region(&self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        // Anything below 1 would show more than there is
        let share = 1.0 / self.factor.max(1.0);

        let (left, visible_width) = Self::axis(self.x, share, width);
        let (top, visible_height) = Self::axis(self.y, share, height);

        (left, top, visible_width, visible_height)
    }

    /// Start and length of the visible part along one axis of `size` pixels
    fn axis(centre: f32, share: f32, size: u32) -> (u32, u32) {
        let length = ((size as f32 * share).round() as u32).clamp(1, size.max(1));
        let middle = size as f32 * centre.clamp(0.0, 100.0) / 100.0;
        let start = (middle - length as f32 / 2.0).round().max(0.0) as u32;

        (start.min(size.saturating_sub(length)), length)
    }
}

impl Source {
    /// Authentication to send to the server, if any was configured
    pub fn credentials(&self) -> Option<client::Credentials> {
        match (&self.username, &self.password) {
            (Some(username), Some(password)) => Some(client::Credentials {
                username: username.clone(),
                password: password.clone(),
            }),
            _ => None,
        }
    }
}

/// A single decoded picture in RGBA8
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Manages a multiple RTSP streams from multiple sources
#[derive(Default, Debug, Clone)]
pub struct Client {
    pub sources: Vec<Source>,
    pub source: usize,
}

impl Client {
    pub fn new(sources: Vec<Source>) -> Self {
        Client {
            sources: sources,
            source: usize::default(),
        }
    }

    pub fn next(&mut self) {
        if self.sources.is_empty() {
            return;
        }

        let last: usize = self.sources.len() - 1;
        let first: usize = 0;

        self.source = if self.source == last {
            first
        } else {
            self.source + 1
        };
    }

    pub fn previous(&mut self) {
        if self.sources.is_empty() {
            return;
        }

        let last: usize = self.sources.len() - 1;
        let first: usize = 0;

        self.source = if self.source == first {
            last
        } else {
            self.source - 1
        };
    }

    pub fn current(&self) -> Option<&Source> {
        return self.sources.get(self.source);
    }
}

/// Play a source, decoding its video into [`Frame`]s until the stream ends or fails.
///
/// Frames are dropped instead of queued when the receiver cannot keep up, so a
/// slow consumer never delays the connection.
///
/// Fails with [`Stalled`] when the session stops producing pictures.
pub async fn play(source: Source, frames: mpsc::Sender<Frame>) -> Result<()> {
    let options = client::SessionOptions::default()
        .creds(source.credentials())
        .user_agent(USER_AGENT.to_string())
        .teardown(client::TeardownPolicy::Auto);

    // A camera that froze may accept the connection and never answer
    let (mut playing, video, codec) = timeout(CONNECT_TIMEOUT, async {
        let mut session = client::Session::describe(Url::parse(&source.url)?, options).await?;

        let (video, codec) = session
            .streams()
            .iter()
            .enumerate()
            .filter(|(_, stream)| stream.media() == "video")
            .find_map(|(index, stream)| {
                Codec::from_encoding_name(stream.encoding_name()).map(|codec| (index, codec))
            })
            .ok_or("Source has no supported video stream")?;

        log::debug!("Playing {codec:?} stream");

        // TCP keeps packets in order and traverses NAT, which matters more than latency here
        let transport = client::Transport::Tcp(client::TcpTransportOptions::default());
        session
            .setup(video, client::SetupOptions::default().transport(transport))
            .await?;

        let playing = session
            .play(client::PlayOptions::default())
            .await?
            .demuxed()?;

        Ok::<_, Error>((playing, video, codec))
    })
    .await
    .map_err(|_| "Connection timed out")??;

    // Opened once the stream is at a random access point, because the codec
    // parameters are only complete after the first frame
    let mut decoder: Option<Decoder> = None;

    // Packets can keep arriving while no picture comes out of them, so only
    // decoded pictures count as a sign of life
    let mut last_picture = Instant::now();

    loop {
        let item = match timeout_at(last_picture + STALL_TIMEOUT, playing.next()).await {
            Ok(Some(item)) => item?,
            Ok(None) => break,
            Err(_) => return Err(Stalled.into()),
        };

        let codec::CodecItem::VideoFrame(frame) = item else {
            continue;
        };

        // The parameters changed, so the decoder has to be opened again
        if frame.has_new_parameters() {
            decoder = None;
        }

        // A connection usually joins mid group of pictures, where every frame
        // references pictures that were never received
        if decoder.is_none() {
            if !frame.is_random_access_point() {
                continue;
            }

            let extra_data = match playing.streams()[video].parameters() {
                Some(codec::ParametersRef::Video(parameters)) => parameters.extra_data().to_vec(),
                _ => Vec::new(),
            };

            decoder = Some(Decoder::new(codec, &extra_data, source.zoom)?);
        }

        let Some(decoder) = decoder.as_mut() else {
            continue;
        };

        let frame = match decoder.decode(frame.data()) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            // A corrupted access unit should not end the session; the next
            // random access point recovers the picture.
            Err(error) => {
                log::warn!("Failed to decode frame: {error}");
                continue;
            }
        };

        last_picture = Instant::now();

        match frames.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => log::trace!("Dropped frame"),
            Err(mpsc::error::TrySendError::Closed(_)) => break,
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zoom(x: f32, y: f32, factor: f32) -> Zoom {
        Zoom { x, y, factor }
    }

    #[test]
    fn zoom_centres_the_region() {
        assert_eq!(zoom(50.0, 50.0, 2.0).region(1000, 600), (250, 150, 500, 300));
        assert_eq!(zoom(25.0, 50.0, 4.0).region(1000, 600), (125, 225, 250, 150));
    }

    #[test]
    fn zoom_stays_inside_the_picture() {
        assert_eq!(zoom(0.0, 0.0, 2.0).region(1000, 600), (0, 0, 500, 300));
        assert_eq!(zoom(100.0, 100.0, 2.0).region(1000, 600), (500, 300, 500, 300));
        assert_eq!(zoom(500.0, -30.0, 2.0).region(1000, 600), (500, 0, 500, 300));
    }

    #[test]
    fn zoom_below_one_shows_everything() {
        assert_eq!(zoom(10.0, 90.0, 1.0).region(1000, 600), (0, 0, 1000, 600));
        assert_eq!(zoom(10.0, 90.0, 0.5).region(1000, 600), (0, 0, 1000, 600));
        assert_eq!(zoom(10.0, 90.0, f32::NAN).region(1000, 600), (0, 0, 1000, 600));
        assert_eq!(zoom(10.0, 90.0, 0.0).region(1000, 600), (0, 0, 1000, 600));
    }

    #[test]
    fn zoom_keeps_at_least_one_pixel() {
        assert_eq!(zoom(50.0, 50.0, f32::INFINITY).region(1000, 600).2, 1);
        assert_eq!(zoom(50.0, 50.0, 1e9).region(1000, 600).3, 1);
    }

    #[test]
    fn zoom_is_read_from_toml() {
        let source: Source = toml::from_str(
            "url = \"rtsp://camera\"\nzoom = { x = 80, y = 20.5, factor = 3 }",
        )
        .unwrap();
        let zoom = source.zoom.unwrap();
        assert_eq!((zoom.x, zoom.y, zoom.factor), (80.0, 20.5, 3.0));

        let source: Source =
            toml::from_str("url = \"rtsp://camera\"\nzoom = { factor = 2 }").unwrap();
        let zoom = source.zoom.unwrap();
        assert_eq!((zoom.x, zoom.y), (50.0, 50.0));
    }

    #[test]
    fn client_wraps_around() {
        let source = Source::default();
        let mut client = Client::new(vec![source.clone(), source.clone(), source]);

        client.previous();
        assert_eq!(client.source, 2);
        client.next();
        assert_eq!(client.source, 0);
    }

    #[test]
    fn client_without_sources_has_no_current() {
        let mut client = Client::new(Vec::new());

        client.next();
        client.previous();
        assert!(client.current().is_none());
    }
}
