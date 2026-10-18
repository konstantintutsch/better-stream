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

            decoder = Some(Decoder::new(codec, &extra_data)?);
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
