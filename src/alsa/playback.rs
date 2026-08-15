use alsa::pcm::{Access, Format, HwParams, State, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{bail, Context, Result};

pub struct PlaybackResult {
    pub frames_played: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_ms: u64,
}

struct WavInfo {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    data_offset: usize,
    data_len: usize,
}

fn parse_wav(raw: &[u8]) -> Result<WavInfo> {
    if raw.len() < 12 {
        bail!("data too short for WAV header");
    }
    if &raw[0..4] != b"RIFF" || &raw[8..12] != b"WAVE" {
        bail!("not a WAV file (missing RIFF/WAVE magic)");
    }

    let mut pos = 12;
    let mut audio_format = 0u16;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut fmt_found = false;

    while pos + 8 <= raw.len() {
        let chunk_id: [u8; 4] = raw[pos..pos + 4].try_into()?;
        let chunk_size = u32::from_le_bytes(raw[pos + 4..pos + 8].try_into()?) as usize;

        match &chunk_id {
            b"fmt " => {
                if chunk_size < 16 {
                    bail!("fmt chunk too small ({chunk_size} bytes)");
                }
                let f = &raw[pos + 8..];
                audio_format = u16::from_le_bytes(f[0..2].try_into()?);
                if audio_format != 1 && audio_format != 3 {
                    bail!(
                        "unsupported WAV audio format {audio_format} (need PCM=1 or IEEE float=3)"
                    );
                }
                channels = u16::from_le_bytes(f[2..4].try_into()?);
                sample_rate = u32::from_le_bytes(f[4..8].try_into()?);
                bits_per_sample = u16::from_le_bytes(f[14..16].try_into()?);
                fmt_found = true;
            }
            b"data" => {
                if !fmt_found {
                    bail!("data chunk before fmt chunk");
                }
                let data_end = (pos + 8 + chunk_size).min(raw.len());
                return Ok(WavInfo {
                    audio_format,
                    channels,
                    sample_rate,
                    bits_per_sample,
                    data_offset: pos + 8,
                    data_len: data_end - (pos + 8),
                });
            }
            _ => {}
        }

        pos += 8 + chunk_size;
        // RIFF chunks are word-aligned
        if !chunk_size.is_multiple_of(2) {
            pos += 1;
        }
    }

    if !fmt_found {
        bail!("WAV missing fmt chunk");
    }
    bail!("WAV missing data chunk");
}

fn wav_to_alsa_format(audio_format: u16, bits: u16) -> Result<Format> {
    match (audio_format, bits) {
        (1, 16) => Ok(Format::s16()),
        (1, 32) => Ok(Format::s32()),
        (3, 32) => Ok(Format::float()),
        (3, 64) => Ok(Format::float64()),
        _ => bail!("unsupported WAV: audio_format={audio_format} bits_per_sample={bits}"),
    }
}

pub fn parse_sample_format(s: &str) -> Result<(Format, u16)> {
    match s {
        "s16le" => Ok((Format::s16(), 16)),
        "s32le" => Ok((Format::s32(), 32)),
        "f32le" => Ok((Format::float(), 32)),
        "f64le" => Ok((Format::float64(), 64)),
        _ => bail!("unsupported sample format '{s}' (expected s16le, s32le, f32le, f64le)"),
    }
}

fn write_all_samples<S: Copy>(
    pcm: &PCM,
    io: &alsa::pcm::IO<S>,
    samples: &[S],
    channels: usize,
) -> Result<u64> {
    let mut total_frames = 0u64;
    let mut remaining = samples;

    while !remaining.is_empty() {
        match io.writei(remaining) {
            Ok(frames) => {
                total_frames += frames as u64;
                remaining = &remaining[frames * channels..];
            }
            Err(e) => {
                pcm.try_recover(e, false)?;
            }
        }
    }

    Ok(total_frames)
}

fn play_samples(
    device: &str,
    sample_rate: u32,
    channels: u16,
    format: Format,
    bits_per_sample: u16,
    data: &[u8],
) -> Result<PlaybackResult> {
    let pcm = PCM::new(device, Direction::Playback, false)
        .with_context(|| format!("failed to open ALSA device '{device}'"))?;

    let hwp = HwParams::any(&pcm)?;
    hwp.set_channels(channels as u32)?;
    hwp.set_rate(sample_rate, ValueOr::Nearest)?;
    hwp.set_format(format)?;
    hwp.set_access(Access::RWInterleaved)?;
    pcm.hw_params(&hwp)?;

    // Start playback when buffer is full rather than immediately
    let hwp = pcm.hw_params_current()?;
    let swp = pcm.sw_params_current()?;
    swp.set_start_threshold(hwp.get_buffer_size()?)?;
    pcm.sw_params(&swp)?;

    let ch = channels as usize;

    let frames_played = match (format, bits_per_sample) {
        (f, 16) if f == Format::s16() => {
            let io = pcm.io_i16()?;
            let samples: Vec<i16> = data
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            write_all_samples(&pcm, &io, &samples, ch)?
        }
        (f, 32) if f == Format::s32() => {
            let io = pcm.io_i32()?;
            let samples: Vec<i32> = data
                .chunks_exact(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            write_all_samples(&pcm, &io, &samples, ch)?
        }
        (f, 32) if f == Format::float() => {
            let io = pcm.io_f32()?;
            let samples: Vec<f32> = data
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            write_all_samples(&pcm, &io, &samples, ch)?
        }
        (f, 64) if f == Format::float64() => {
            let io = pcm.io_f64()?;
            let samples: Vec<f64> = data
                .chunks_exact(8)
                .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                .collect();
            write_all_samples(&pcm, &io, &samples, ch)?
        }
        _ => bail!("unsupported format/bits: {format:?} {bits_per_sample}-bit"),
    };

    if pcm.state() != State::Running {
        pcm.start()?;
    }
    pcm.drain()?;

    let duration_ms = if sample_rate > 0 {
        (frames_played * 1000) / sample_rate as u64
    } else {
        0
    };

    Ok(PlaybackResult {
        frames_played,
        sample_rate,
        channels,
        duration_ms,
    })
}

pub fn play_wav_file(path: &std::path::Path, device: &str) -> Result<PlaybackResult> {
    let raw = std::fs::read(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let wav = parse_wav(&raw)?;
    let format = wav_to_alsa_format(wav.audio_format, wav.bits_per_sample)?;
    play_samples(
        device,
        wav.sample_rate,
        wav.channels,
        format,
        wav.bits_per_sample,
        &raw[wav.data_offset..wav.data_offset + wav.data_len],
    )
}

pub fn play_pcm(
    data: &[u8],
    sample_rate: u32,
    channels: u16,
    sample_format: &str,
    device: &str,
) -> Result<PlaybackResult> {
    let (format, bits) = parse_sample_format(sample_format)?;
    play_samples(device, sample_rate, channels, format, bits, data)
}
