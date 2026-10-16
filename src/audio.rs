use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Duration;
use anyhow::{Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player as AudioSink, Source};

pub struct Audio {
    _device: MixerDeviceSink,
    sink: AudioSink,
    pub path: Option<PathBuf>,
    pub duration: Option<Duration>,
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
        })
    }

    pub fn load<P: AsRef<Path>>(&mut self, p: P) -> Result<()> {
        let p = p.as_ref();
        let file = File::open(p).with_context(|| format!("failed to open {:?}", p))?;
        let source = Decoder::try_from(BufReader::new(file)).map_err(|e| anyhow::anyhow!("{e}"))?;
        let dur = source.total_duration();

        self.sink.clear();
        self.sink.append(source);
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
