use iced::futures::StreamExt;
use openh264::{decoder::Decoder, formats::YUVSource};
use retina::{client, codec};
use serde::Deserialize;
use tokio::sync::mpsc;
use url::Url;

/// Annex B NAL unit delimiter
const START_CODE: [u8; 4] = [0, 0, 0, 1];
/// Size of the length prefix retina puts in front of every NAL unit
const LENGTH_PREFIX: usize = 4;
const USER_AGENT: &str = concat!("better-stream/", env!("CARGO_PKG_VERSION"));

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

/// Provides a source to access a RTSP stream
#[derive(Default, Debug, Clone, Deserialize)]
pub struct Source {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
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

/// Play a source, decoding H.264 into [`Frame`]s until the stream ends or fails.
///
/// Frames are dropped instead of queued when the receiver cannot keep up, so a
/// slow consumer never delays the connection.
pub async fn play(source: Source, frames: mpsc::Sender<Frame>) -> Result<()> {
    let options = client::SessionOptions::default()
        .creds(source.credentials())
        .user_agent(USER_AGENT.to_string())
        .teardown(client::TeardownPolicy::Auto);

    let mut session = client::Session::describe(Url::parse(&source.url)?, options).await?;

    let video = session
        .streams()
        .iter()
        .position(|stream| stream.media() == "video" && stream.encoding_name() == "h264")
        .ok_or("Source has no H.264 video stream")?;

    // TCP keeps packets in order and traverses NAT, which matters more than latency here
    let transport = client::Transport::Tcp(client::TcpTransportOptions::default());
    session
        .setup(video, client::SetupOptions::default().transport(transport))
        .await?;

    let mut decoder = Decoder::new()?;
    let mut buffer: Vec<u8> = Vec::new();

    // Cameras that only advertise their parameter sets in the SDP never repeat
    // them in band, so prime the decoder before the first picture arrives.
    if let Some(codec::ParametersRef::Video(parameters)) = session.streams()[video].parameters() {
        parameter_sets(parameters.extra_data(), &mut buffer);
        let _ = decoder.decode(&buffer);
    }

    let mut playing = session
        .play(client::PlayOptions::default())
        .await?
        .demuxed()?;

    let mut synchronized = false;

    while let Some(item) = playing.next().await {
        let codec::CodecItem::VideoFrame(frame) = item? else {
            continue;
        };

        // A connection usually joins mid group of pictures, where every frame
        // references pictures that were never received
        if !synchronized {
            if !frame.is_random_access_point() {
                continue;
            }
            synchronized = true;
        }

        annex_b(frame.data(), &mut buffer);

        let picture = match decoder.decode(&buffer) {
            Ok(Some(picture)) => picture,
            Ok(None) => continue,
            // A corrupted access unit should not end the session; the next
            // random access point recovers the picture.
            Err(error) => {
                log::warn!("Failed to decode frame: {error}");
                continue;
            }
        };

        let (width, height) = picture.dimensions();
        let mut pixels = vec![0; width * height * 4];
        picture.write_rgba8(&mut pixels);

        let frame = Frame {
            width: width as u32,
            height: height as u32,
            pixels: pixels,
        };

        match frames.try_send(frame) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => log::trace!("Dropped frame"),
            Err(mpsc::error::TrySendError::Closed(_)) => break,
        }
    }

    Ok(())
}

/// Rewrite length prefixed NAL units into start code delimited ones
fn annex_b(data: &[u8], output: &mut Vec<u8>) {
    output.clear();

    let mut offset = 0;

    while let Some(length) = data.get(offset..offset + LENGTH_PREFIX) {
        let length = u32::from_be_bytes([length[0], length[1], length[2], length[3]]) as usize;
        offset += LENGTH_PREFIX;

        let Some(unit) = data.get(offset..offset + length) else {
            break;
        };
        offset += length;

        output.extend_from_slice(&START_CODE);
        output.extend_from_slice(unit);
    }
}

/// Extract the parameter sets of an AVCDecoderConfigurationRecord as Annex B.
///
/// The record starts with a five byte header, followed by a group of sequence
/// parameter sets and a group of picture parameter sets. Each group is a count
/// byte followed by that many two byte lengths with their NAL unit.
fn parameter_sets(extra_data: &[u8], output: &mut Vec<u8>) {
    const HEADER: usize = 5;
    const COUNT: u8 = 0b0001_1111;

    output.clear();

    let mut offset = HEADER;

    for _ in 0..2 {
        let Some(&count) = extra_data.get(offset) else {
            return;
        };
        offset += 1;

        for _ in 0..(count & COUNT) {
            let Some(length) = extra_data.get(offset..offset + 2) else {
                return;
            };
            let length = u16::from_be_bytes([length[0], length[1]]) as usize;
            offset += 2;

            let Some(unit) = extra_data.get(offset..offset + length) else {
                return;
            };
            offset += length;

            output.extend_from_slice(&START_CODE);
            output.extend_from_slice(unit);
        }
    }
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

    #[test]
    fn annex_b_replaces_length_prefixes() {
        let mut output = Vec::new();
        annex_b(&[0, 0, 0, 2, 0x67, 0xAA, 0, 0, 0, 1, 0x68], &mut output);

        assert_eq!(output, [0, 0, 0, 1, 0x67, 0xAA, 0, 0, 0, 1, 0x68]);
    }

    #[test]
    fn annex_b_ignores_truncated_units() {
        let mut output = Vec::new();
        annex_b(&[0, 0, 0, 8, 0x67], &mut output);

        assert!(output.is_empty());
    }

    #[test]
    fn parameter_sets_reads_both_groups() {
        let mut output = Vec::new();
        parameter_sets(
            &[1, 0x42, 0, 0x1E, 0xFF, 0xE1, 0, 1, 0x67, 1, 0, 1, 0x68],
            &mut output,
        );

        assert_eq!(output, [0, 0, 0, 1, 0x67, 0, 0, 0, 1, 0x68]);
    }
}
