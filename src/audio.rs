use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::{
    source::SeekError, ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink,
    Player as AudioSink, Sample, SampleRate, Source,
};

pub struct Audio {
    _device: MixerDeviceSink,
    sink: AudioSink,
    capture_stream: Option<cpal::Stream>,
    pub path: Option<PathBuf>,
    pub duration: Option<Duration>,
    pub samples: Arc<Mutex<VecDeque<f32>>>,
    pub stereo: Arc<Mutex<(VecDeque<f32>, VecDeque<f32>)>>,
}

impl Audio {
    pub fn new() -> Result<Self> {
        let mut device = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("failed to open audio device")?;
        device.log_on_drop(false);
        let sink = AudioSink::connect_new(&device.mixer());
        Ok(Self {
            _device: device,
            sink,
            capture_stream: None,
            path: None,
            duration: None,
            samples: Arc::new(Mutex::new(VecDeque::with_capacity(4096))),
            stereo: Arc::new(Mutex::new((
                VecDeque::with_capacity(2048),
                VecDeque::with_capacity(2048),
            ))),
        })
    }

    pub fn load<P: AsRef<Path>>(&mut self, p: P) -> Result<()> {
        let p = p.as_ref();
        let file = File::open(p).with_context(|| format!("failed to open {:?}", p))?;
        let source = Decoder::try_from(BufReader::new(file)).map_err(|e| anyhow::anyhow!("{e}"))?;
        let dur = source.total_duration();
        let channels = source.channels().get() as usize;

        let tap = SampleTap {
            input: source,
            channels,
            chan_idx: 0,
            frame_acc: 0.0,
            left_sample: 0.0,
            right_sample: 0.0,
            ring: Arc::clone(&self.samples),
            stereo_ring: Arc::clone(&self.stereo),
            buf: Vec::with_capacity(64),
            stereo_buf: (Vec::with_capacity(64), Vec::with_capacity(64)),
        };

        if let Ok(mut ring) = self.samples.lock() {
            ring.clear();
        }
        if let Ok(mut s_ring) = self.stereo.lock() {
            s_ring.0.clear();
            s_ring.1.clear();
        }

        self.sink.clear();
        self.sink.append(tap);
        self.sink.play();

        self.path = Some(p.to_path_buf());
        self.duration = dur;
        Ok(())
    }

    pub fn toggle_pause(&self) {
        if self.sink.is_paused() {
            self.sink.play();
        } else {
            self.sink.pause();
        }
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn play(&self) {
        self.sink.play();
    }

    pub fn is_paused(&self) -> bool {
        self.sink.is_paused()
    }

    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    pub fn pos(&self) -> Duration {
        self.sink.get_pos()
    }

    pub fn volume(&self) -> f32 {
        self.sink.volume()
    }

    pub fn set_volume(&self, vol: f32) {
        self.sink.set_volume(vol.clamp(0.0, 2.0));
    }

    pub fn start_capture(&mut self) -> Result<()> {
        self.stop_capture();
        let host = cpal::default_host();
        let mut target_device = None;

        if let Ok(devices) = host.input_devices() {
            for dev in devices {
                if let Ok(desc) = dev.description() {
                    let name = desc.name().to_ascii_lowercase();
                    if name.contains("monitor") {
                        target_device = Some(dev);
                        break;
                    }
                    let mut found_monitor = false;
                    for ext in desc.extended() {
                        if ext.to_ascii_lowercase().contains("monitor") {
                            found_monitor = true;
                            break;
                        }
                    }
                    if found_monitor {
                        target_device = Some(dev);
                        break;
                    }
                }
            }
        }

        let device = match target_device {
            Some(d) => d,
            None => host
                .default_input_device()
                .ok_or_else(|| anyhow::anyhow!("no audio input or monitor device found"))?,
        };

        let config = device.default_input_config()?;
        let channels = config.channels() as usize;
        let samples_ring = Arc::clone(&self.samples);
        let stereo_ring = Arc::clone(&self.stereo);
        let err_fn = |_| ();

        let stream_config = config.config();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config,
                move |data: &[f32], _| write_capture_f32(data, channels, &samples_ring, &stereo_ring),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config,
                move |data: &[i16], _| write_capture_i16(data, channels, &samples_ring, &stereo_ring),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                stream_config,
                move |data: &[u16], _| write_capture_u16(data, channels, &samples_ring, &stereo_ring),
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I32 => device.build_input_stream(
                stream_config,
                move |data: &[i32], _| write_capture_i32(data, channels, &samples_ring, &stereo_ring),
                err_fn,
                None,
            )?,
            fmt => return Err(anyhow::anyhow!("unsupported audio sample format: {fmt:?}")),
        };

        stream.play()?;
        self.capture_stream = Some(stream);
        Ok(())
    }

    pub fn stop_capture(&mut self) {
        if let Some(stream) = self.capture_stream.take() {
            let _ = stream.pause();
        }
    }
}

fn write_capture_f32(
    data: &[f32],
    channels: usize,
    samples: &Arc<Mutex<VecDeque<f32>>>,
    stereo: &Arc<Mutex<(VecDeque<f32>, VecDeque<f32>)>>,
) {
    if channels == 0 || data.is_empty() {
        return;
    }
    let frames = data.len() / channels;
    if frames == 0 {
        return;
    }

    if let Ok(mut ring) = samples.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let mut sum = 0.0f32;
            for c in 0..channels {
                sum += data[offset + c];
            }
            if ring.len() >= 4096 {
                ring.pop_front();
            }
            ring.push_back(sum / channels as f32);
        }
    }

    if let Ok(mut s_ring) = stereo.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let left = data[offset];
            let right = if channels > 1 { data[offset + 1] } else { left };
            if s_ring.0.len() >= 2048 {
                s_ring.0.pop_front();
                s_ring.1.pop_front();
            }
            s_ring.0.push_back(left);
            s_ring.1.push_back(right);
        }
    }
}

fn write_capture_i16(
    data: &[i16],
    channels: usize,
    samples: &Arc<Mutex<VecDeque<f32>>>,
    stereo: &Arc<Mutex<(VecDeque<f32>, VecDeque<f32>)>>,
) {
    if channels == 0 || data.is_empty() {
        return;
    }
    let frames = data.len() / channels;
    if frames == 0 {
        return;
    }

    if let Ok(mut ring) = samples.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let mut sum = 0.0f32;
            for c in 0..channels {
                sum += data[offset + c] as f32 / 32768.0;
            }
            if ring.len() >= 4096 {
                ring.pop_front();
            }
            ring.push_back(sum / channels as f32);
        }
    }

    if let Ok(mut s_ring) = stereo.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let left = data[offset] as f32 / 32768.0;
            let right = if channels > 1 {
                data[offset + 1] as f32 / 32768.0
            } else {
                left
            };
            if s_ring.0.len() >= 2048 {
                s_ring.0.pop_front();
                s_ring.1.pop_front();
            }
            s_ring.0.push_back(left);
            s_ring.1.push_back(right);
        }
    }
}

fn write_capture_u16(
    data: &[u16],
    channels: usize,
    samples: &Arc<Mutex<VecDeque<f32>>>,
    stereo: &Arc<Mutex<(VecDeque<f32>, VecDeque<f32>)>>,
) {
    if channels == 0 || data.is_empty() {
        return;
    }
    let frames = data.len() / channels;
    if frames == 0 {
        return;
    }

    if let Ok(mut ring) = samples.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let mut sum = 0.0f32;
            for c in 0..channels {
                sum += (data[offset + c] as f32 - 32768.0) / 32768.0;
            }
            if ring.len() >= 4096 {
                ring.pop_front();
            }
            ring.push_back(sum / channels as f32);
        }
    }

    if let Ok(mut s_ring) = stereo.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let left = (data[offset] as f32 - 32768.0) / 32768.0;
            let right = if channels > 1 {
                (data[offset + 1] as f32 - 32768.0) / 32768.0
            } else {
                left
            };
            if s_ring.0.len() >= 2048 {
                s_ring.0.pop_front();
                s_ring.1.pop_front();
            }
            s_ring.0.push_back(left);
            s_ring.1.push_back(right);
        }
    }
}

fn write_capture_i32(
    data: &[i32],
    channels: usize,
    samples: &Arc<Mutex<VecDeque<f32>>>,
    stereo: &Arc<Mutex<(VecDeque<f32>, VecDeque<f32>)>>,
) {
    if channels == 0 || data.is_empty() {
        return;
    }
    let frames = data.len() / channels;
    if frames == 0 {
        return;
    }

    if let Ok(mut ring) = samples.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let mut sum = 0.0f32;
            for c in 0..channels {
                sum += data[offset + c] as f32 / 2147483648.0;
            }
            if ring.len() >= 4096 {
                ring.pop_front();
            }
            ring.push_back(sum / channels as f32);
        }
    }

    if let Ok(mut s_ring) = stereo.try_lock() {
        for f in 0..frames {
            let offset = f * channels;
            let left = data[offset] as f32 / 2147483648.0;
            let right = if channels > 1 {
                data[offset + 1] as f32 / 2147483648.0
            } else {
                left
            };
            if s_ring.0.len() >= 2048 {
                s_ring.0.pop_front();
                s_ring.1.pop_front();
            }
            s_ring.0.push_back(left);
            s_ring.1.push_back(right);
        }
    }
}

struct SampleTap<S> {
    input: S,
    channels: usize,
    chan_idx: usize,
    frame_acc: f32,
    left_sample: f32,
    right_sample: f32,
    ring: Arc<Mutex<VecDeque<f32>>>,
    stereo_ring: Arc<Mutex<(VecDeque<f32>, VecDeque<f32>)>>,
    buf: Vec<f32>,
    stereo_buf: (Vec<f32>, Vec<f32>),
}

impl<S: Source> Iterator for SampleTap<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.input.next()?;
        if self.chan_idx == 0 {
            self.left_sample = sample;
            if self.channels == 1 {
                self.right_sample = sample;
            }
        } else if self.chan_idx == 1 {
            self.right_sample = sample;
        }

        self.frame_acc += sample;
        self.chan_idx += 1;
        if self.chan_idx >= self.channels {
            let mono = self.frame_acc / self.channels as f32;
            self.chan_idx = 0;
            self.frame_acc = 0.0;
            self.buf.push(mono);
            self.stereo_buf.0.push(self.left_sample);
            self.stereo_buf.1.push(self.right_sample);

            if self.buf.len() >= 64 {
                if let Ok(mut ring) = self.ring.try_lock() {
                    for &s in &self.buf {
                        if ring.len() >= 4096 {
                            ring.pop_front();
                        }
                        ring.push_back(s);
                    }
                }
                if let Ok(mut s_ring) = self.stereo_ring.try_lock() {
                    for i in 0..self.stereo_buf.0.len() {
                        if s_ring.0.len() >= 2048 {
                            s_ring.0.pop_front();
                            s_ring.1.pop_front();
                        }
                        s_ring.0.push_back(self.stereo_buf.0[i]);
                        s_ring.1.push_back(self.stereo_buf.1[i]);
                    }
                }
                self.buf.clear();
                self.stereo_buf.0.clear();
                self.stereo_buf.1.clear();
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.input.size_hint()
    }
}

impl<S: Source> Source for SampleTap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.input.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.input.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.input.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.input.try_seek(pos)
    }
}
