use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::{Context, Result};
use rodio::{
    source::SeekError, ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink,
    Player as AudioSink, Sample, SampleRate, Source,
};

pub struct Audio {
    _device: MixerDeviceSink,
    sink: AudioSink,
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
