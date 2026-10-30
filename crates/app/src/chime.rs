use std::f32::consts::TAU;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::Hub;

/// The old app's chime: two soft rising notes, each starting at its offset in seconds.
const NOTES: [(f32, f32); 2] = [(660., 0.), (880., 0.12)];
const NOTE: f32 = 0.2;
const PEAK: f32 = 0.15;
const QUIET: f32 = 0.0001;
/// How long the stream stays open: the last note and a little room for the device to drain.
const PLAYS_FOR: Duration = Duration::from_millis(450);

/// The chime's level `t` seconds in: each note rises to its peak in 20 ms and fades by 180 ms.
fn sample(t: f32) -> f32 {
    NOTES
        .iter()
        .map(|(frequency, start)| {
            let local = t - start;
            if !(0. ..NOTE).contains(&local) {
                return 0.;
            }
            let level = if local < 0.02 {
                QUIET * (PEAK / QUIET).powf(local / 0.02)
            } else {
                PEAK * (QUIET / PEAK).powf(((local - 0.02) / 0.16).min(1.))
            };
            level * (TAU * frequency * t).sin()
        })
        .sum()
}

fn play() -> Result<(), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no audio output".to_owned())?;
    let supported = device.default_output_config().map_err(|err| err.to_string())?;
    let config = supported.config();
    match supported.sample_format() {
        SampleFormat::F32 => sound::<f32>(&device, &config),
        SampleFormat::I16 => sound::<i16>(&device, &config),
        SampleFormat::U16 => sound::<u16>(&device, &config),
        SampleFormat::I32 => sound::<i32>(&device, &config),
        other => Err(format!("the audio output takes {other} samples")),
    }
}

fn sound<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &StreamConfig,
) -> Result<(), String> {
    #[expect(clippy::cast_precision_loss, reason = "sample rates fit an f32 exactly")]
    let rate = config.sample_rate.0 as f32;
    let channels = usize::from(config.channels.max(1));
    let mut frame: u32 = 0;
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                for out in data.chunks_mut(channels) {
                    #[expect(clippy::cast_precision_loss, reason = "a frame count under a second")]
                    let value = T::from_sample(sample(frame as f32 / rate));
                    frame = frame.saturating_add(1);
                    out.fill(value);
                }
            },
            |err| tracing::warn!(%err, "the notification chime's audio stream failed"),
            None,
        )
        .map_err(|err| err.to_string())?;
    stream.play().map_err(|err| err.to_string())?;
    std::thread::sleep(PLAYS_FOR);
    Ok(())
}

impl Hub {
    /// Plays the notification chime on the default audio output, in the background; a desktop
    /// without audio just stays quiet.
    pub fn chime(&self) {
        self.inner.runtime.spawn_blocking(|| match play() {
            Ok(()) => tracing::info!("played the notification chime"),
            Err(err) => tracing::warn!(%err, "playing the notification chime failed"),
        });
    }
}
