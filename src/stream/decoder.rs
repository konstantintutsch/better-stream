use std::{ptr, sync::Once};

use ffmpeg_next::{
    self as ffmpeg, Packet,
    codec::{self, Id},
    decoder, frame,
    software::scaling,
    util::format::Pixel,
};

use super::rtsp::{Frame, Result, Zoom};

static INIT: Once = Once::new();

/// Video codecs retina can depacketize and FFmpeg can decode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
    Jpeg,
}

impl Codec {
    /// Look up a codec by the encoding name of its RTSP stream, as found in the SDP
    pub fn from_encoding_name(name: &str) -> Option<Self> {
        match name {
            "h264" => Some(Codec::H264),
            "h265" => Some(Codec::H265),
            "jpeg" => Some(Codec::Jpeg),
            _ => None,
        }
    }

    fn id(self) -> Id {
        match self {
            Codec::H264 => Id::H264,
            Codec::H265 => Id::HEVC,
            Codec::Jpeg => Id::MJPEG,
        }
    }
}

/// Decodes the frames retina demuxes into RGBA [`Frame`]s using FFmpeg
pub struct Decoder {
    video: decoder::Video,
    scaler: Option<scaling::Context>,
    decoded: frame::Video,
    received: frame::Video,
    rgba: frame::Video,
    zoom: Option<Zoom>,
}

// SAFETY: the FFmpeg contexts are only ever used through `&mut self`, so they
// are never accessed from two threads at once. They hold no thread-local state,
// which makes moving them between threads sound.
unsafe impl Send for Decoder {}

impl Decoder {
    /// Open a decoder for `codec`.
    ///
    /// `extra_data` is what retina reports as [`retina::codec::VideoParameters::extra_data`]:
    /// the AVCDecoderConfigurationRecord of H.264, the HEVCDecoderConfigurationRecord of H.265
    /// and nothing for JPEG. FFmpeg then expects the length prefixed NAL units
    /// retina produces, instead of Annex B.
    ///
    /// Pictures are cut down to `zoom` when one is given.
    pub fn new(codec: Codec, extra_data: &[u8], zoom: Option<Zoom>) -> Result<Self> {
        INIT.call_once(|| {
            if let Err(error) = ffmpeg::init() {
                log::warn!("Failed to initialize FFmpeg: {error}");
            }
            // Lost packets make FFmpeg complain about every broken picture,
            // which is already reported through the decode result
            ffmpeg::log::set_level(ffmpeg::log::Level::Error);
        });

        let found = decoder::find(codec.id()).ok_or("FFmpeg has no decoder for this codec")?;

        let mut context = codec::Context::new_with_codec(found);
        context.set_flags(codec::Flags::LOW_DELAY);
        set_extra_data(&mut context, extra_data)?;

        let video = context.decoder().open()?.video()?;

        Ok(Decoder {
            video,
            scaler: None,
            decoded: frame::Video::empty(),
            received: frame::Video::empty(),
            rgba: frame::Video::empty(),
            zoom,
        })
    }

    /// Decode one access unit, returning the picture it completes, if any
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Frame>> {
        self.video.send_packet(&Packet::copy(data))?;

        let mut received = false;

        loop {
            // A failed receive clears the frame, so the last picture is kept aside
            match self.video.receive_frame(&mut self.received) {
                Ok(()) => {
                    std::mem::swap(&mut self.decoded, &mut self.received);
                    received = true;
                }
                Err(ffmpeg::Error::Other {
                    errno: ffmpeg::error::EAGAIN,
                })
                | Err(ffmpeg::Error::Eof) => break,
                Err(error) => return Err(error.into()),
            }
        }

        if !received {
            return Ok(None);
        }

        self.convert().map(Some)
    }

    fn convert(&mut self) -> Result<Frame> {
        let format = self.decoded.format();
        let width = self.decoded.width();
        let height = self.decoded.height();

        let current = self.scaler.as_ref().is_some_and(|scaler| {
            let input = scaler.input();
            input.format == format && input.width == width && input.height == height
        });

        if !current {
            self.scaler = Some(scaling::Context::get(
                format,
                width,
                height,
                Pixel::RGBA,
                width,
                height,
                scaling::Flags::BILINEAR,
            )?);
            // The output picture is allocated for the size of the first conversion
            self.rgba = frame::Video::empty();
        }

        let scaler = self.scaler.as_mut().ok_or("Scaler is missing")?;
        scaler.run(&self.decoded, &mut self.rgba)?;

        let (left, top, visible_width, visible_height) = match &self.zoom {
            Some(zoom) => zoom.region(width, height),
            None => (0, 0, width, height),
        };

        // Rows are padded to the stride, the frame is tightly packed
        let row = visible_width as usize * 4;
        let offset = left as usize * 4;
        let stride = self.rgba.stride(0);
        let data = self.rgba.data(0);

        let mut pixels = Vec::with_capacity(row * visible_height as usize);
        for line in data
            .chunks(stride)
            .skip(top as usize)
            .take(visible_height as usize)
        {
            pixels.extend_from_slice(&line[offset..offset + row]);
        }

        Ok(Frame {
            width: visible_width,
            height: visible_height,
            pixels,
        })
    }
}

/// Hand `extra_data` to FFmpeg, which takes ownership of a padded copy
fn set_extra_data(context: &mut codec::Context, extra_data: &[u8]) -> Result<()> {
    if extra_data.is_empty() {
        return Ok(());
    }

    // SAFETY: the buffer is allocated with av_mallocz and the padding FFmpeg
    // requires, and the context frees it with av_free when it is dropped.
    unsafe {
        let size = extra_data.len();
        let buffer =
            ffmpeg::ffi::av_mallocz(size + ffmpeg::ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize)
                as *mut u8;

        if buffer.is_null() {
            return Err("Failed to allocate codec extra data".into());
        }

        ptr::copy_nonoverlapping(extra_data.as_ptr(), buffer, size);

        let raw = context.as_mut_ptr();
        (*raw).extradata = buffer;
        (*raw).extradata_size = size as i32;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_is_found_by_encoding_name() {
        assert_eq!(Codec::from_encoding_name("h264"), Some(Codec::H264));
        assert_eq!(Codec::from_encoding_name("h265"), Some(Codec::H265));
        assert_eq!(Codec::from_encoding_name("jpeg"), Some(Codec::Jpeg));
        assert_eq!(Codec::from_encoding_name("pcma"), None);
    }

    #[test]
    fn every_codec_has_an_ffmpeg_decoder() {
        for codec in [Codec::H264, Codec::H265, Codec::Jpeg] {
            assert!(decoder::find(codec.id()).is_some(), "{codec:?}");
        }
    }
}
