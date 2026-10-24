//! The mp3 presets: what Electron's ffmpeg did with `-b:a 256k`, without
//! ffmpeg. Symphonia decodes the downloaded AAC, LAME encodes it as a
//! constant-bitrate mp3.

use std::{fs::File, io::Write, path::Path};

use mp3lame_encoder::{Bitrate, Builder, Encoder, FlushNoGap, InterleavedPcm, MonoPcm, Quality};
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    errors::Error,
    formats::{probe::Hint, FormatOptions, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

/// LAME's constant bitrates, in kbps
const BITRATES: [(u32, Bitrate); 16] = [
    (8, Bitrate::Kbps8),
    (16, Bitrate::Kbps16),
    (24, Bitrate::Kbps24),
    (32, Bitrate::Kbps32),
    (40, Bitrate::Kbps40),
    (48, Bitrate::Kbps48),
    (64, Bitrate::Kbps64),
    (80, Bitrate::Kbps80),
    (96, Bitrate::Kbps96),
    (112, Bitrate::Kbps112),
    (128, Bitrate::Kbps128),
    (160, Bitrate::Kbps160),
    (192, Bitrate::Kbps192),
    (224, Bitrate::Kbps224),
    (256, Bitrate::Kbps256),
    (320, Bitrate::Kbps320),
];

/// The highest LAME bitrate not above `kbps` (8 at least).
pub fn bitrate(kbps: u32) -> u32 {
    BITRATES.iter().rev().find(|(rate, _)| *rate <= kbps).map_or(8, |(rate, _)| *rate)
}

fn new_encoder(kbps: u32, sample_rate: u32, channels: usize) -> Result<Encoder, String> {
    let error = |error: mp3lame_encoder::BuildError| format!("LAME: {error}");
    let mut builder = Builder::new().ok_or("LAME could not start")?;
    builder.set_num_channels(channels as u8).map_err(error)?;
    builder.set_sample_rate(sample_rate).map_err(error)?;
    let rate = BITRATES.iter().find(|(rate, _)| *rate == bitrate(kbps)).map(|(_, rate)| *rate);
    builder.set_brate(rate.unwrap_or(Bitrate::Kbps256)).map_err(error)?;
    // LAME's recommended `-q 2`
    builder.set_quality(Quality::NearBest).map_err(error)?;
    builder.build().map_err(error)
}

/// Encodes `input` (AAC in MP4) as an mp3 at `kbps` into `output`, calling
/// `on_progress` with the share done.
pub fn transcode(input: &Path, output: &Path, kbps: u32, mut on_progress: impl FnMut(f64)) -> Result<(), String> {
    let source = File::open(input).map_err(|error| error.to_string())?;
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|error| format!("reading the audio failed: {error}"))?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio track")?;
    let track_id = track.id;
    let total_frames = track.num_frames.filter(|&frames| frames > 0);
    let params = track.codec_params.as_ref().and_then(|params| params.audio()).ok_or("no audio parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|error| format!("no decoder: {error}"))?;

    let mut out = std::io::BufWriter::new(File::create(output).map_err(|error| error.to_string())?);
    let mut encoder: Option<(Encoder, usize)> = None;
    let mut samples: Vec<f32> = vec![];
    let mut buffer: Vec<u8> = vec![];
    let mut frames_done = 0u64;
    let mut last_reported = 0.0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(format!("reading the audio failed: {error}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet: skip it, like the example decoders do
            Err(Error::DecodeError(_)) => continue,
            Err(error) => return Err(format!("decoding failed: {error}")),
        };
        let spec = decoded.spec();
        let channels = spec.channels().count();
        if encoder.is_none() {
            if !(1..=2).contains(&channels) {
                return Err(format!("{channels} channels"));
            }
            encoder = Some((new_encoder(kbps, spec.rate(), channels)?, channels));
        }
        let (encoder, channels) = encoder.as_mut().unwrap();
        samples.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut samples);
        let frames = samples.len() / *channels;
        buffer.clear();
        buffer.reserve(mp3lame_encoder::max_required_buffer_size(frames));
        let written = if *channels == 2 {
            encoder.encode(InterleavedPcm(&samples), buffer.spare_capacity_mut())
        } else {
            encoder.encode(MonoPcm(&samples), buffer.spare_capacity_mut())
        }
        .map_err(|error| format!("LAME: {error}"))?;
        write_encoded(&mut out, &mut buffer, written)?;

        frames_done += frames as u64;
        if let Some(total) = total_frames {
            let ratio = (frames_done as f64 / total as f64).min(1.0);
            if ratio - last_reported >= 0.01 {
                last_reported = ratio;
                on_progress(ratio);
            }
        }
    }

    let (mut encoder, _) = encoder.ok_or("no audio decoded")?;
    buffer.clear();
    buffer.reserve(7200);
    let written = encoder
        .flush::<FlushNoGap>(buffer.spare_capacity_mut())
        .map_err(|error| format!("LAME: {error}"))?;
    write_encoded(&mut out, &mut buffer, written)?;
    out.flush().map_err(|error| error.to_string())?;
    on_progress(1.0);
    Ok(())
}

/// Writes the `written` bytes LAME just put in `buffer`'s spare capacity.
fn write_encoded(out: &mut impl Write, buffer: &mut Vec<u8>, written: usize) -> Result<(), String> {
    // SAFETY: LAME initialized `written` bytes of the (cleared) buffer's capacity
    unsafe { buffer.set_len(written) };
    out.write_all(buffer).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::bitrate;

    #[test]
    fn picks_a_lame_bitrate() {
        assert_eq!(bitrate(256), 256);
        assert_eq!(bitrate(250), 224);
        assert_eq!(bitrate(1000), 320);
        assert_eq!(bitrate(1), 8);
    }
}
