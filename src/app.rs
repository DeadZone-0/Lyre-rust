use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use anyhow::{bail, Result};
use crossterm::event::KeyCode;
use rustfft::{num_complex::Complex, Fft, FftPlanner};

use crate::audio::Audio;

const FFT_SIZE: usize = 2048;
const NUM_BINS: usize = 64;

pub struct App {
    pub audio: Audio,
    pub tracks: Vec<PathBuf>,
    pub idx: usize,
    pub quit: bool,
    pub bins: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
}

impl App {
    pub fn new(target: Option<PathBuf>) -> Result<Self> {
        let mut audio = Audio::new()?;
        let mut tracks = Vec::new();

        if let Some(p) = target {
            if !p.exists() {
                bail!("path does not exist: {}", p.display());
            }
            tracks = collect_tracks(&p)?;
            if tracks.is_empty() {
                bail!("no audio files found (.mp3, .wav, .flac) in: {}", p.display());
            }
        }

        if !tracks.is_empty() {
            let _ = audio.load(&tracks[0]);
        }

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos()))
            .collect();
        let fft_buf = vec![Complex { re: 0.0, im: 0.0 }; FFT_SIZE];
        let bins = vec![0.0; NUM_BINS];

        Ok(Self {
            audio,
            tracks,
            idx: 0,
            quit: false,
            bins,
            fft,
            window,
            fft_buf,
        })
    }

    pub fn handle_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char(' ') => self.audio.toggle_pause(),
            KeyCode::Char('s') => self.skip()?,
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let v = self.audio.volume() + 0.05;
                self.audio.set_volume(v);
            }
            KeyCode::Char('-') => {
                let v = self.audio.volume() - 0.05;
                self.audio.set_volume(v);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn skip(&mut self) -> Result<()> {
        if self.tracks.is_empty() {
            return Ok(());
        }
        self.idx = (self.idx + 1) % self.tracks.len();
        self.audio.load(&self.tracks[self.idx])
    }

    pub fn tick(&mut self) -> Result<()> {
        if !self.tracks.is_empty() && self.audio.is_empty() && !self.audio.is_paused() {
            if self.idx + 1 < self.tracks.len() {
                self.idx += 1;
                self.audio.load(&self.tracks[self.idx])?;
            }
        }
        self.update_fft();
        Ok(())
    }

    fn update_fft(&mut self) {
        let decay = 0.035f32;

        if self.audio.is_paused() || self.audio.is_empty() || self.tracks.is_empty() {
            for b in &mut self.bins {
                *b = (*b - decay).max(0.0);
            }
            return;
        }

        let samples: Vec<f32> = match self.audio.samples.try_lock() {
            Ok(ring) if ring.len() >= FFT_SIZE => {
                let skip = ring.len() - FFT_SIZE;
                ring.iter().skip(skip).copied().collect()
            }
            _ => Vec::new(),
        };

        if samples.len() < FFT_SIZE {
            for b in &mut self.bins {
                *b = (*b - decay).max(0.0);
            }
            return;
        }

        for (i, (&s, &w)) in samples.iter().zip(self.window.iter()).enumerate() {
            self.fft_buf[i] = Complex { re: s * w, im: 0.0 };
        }
        self.fft.process(&mut self.fft_buf);

        let half = FFT_SIZE / 2;
        let mags: Vec<f32> = self.fft_buf[..half]
            .iter()
            .map(|c| (c.re * c.re + c.im * c.im).sqrt() / FFT_SIZE as f32)
            .collect();

        let f_min = 25.0f32;
        let f_max = 18000.0f32;
        let nyq = 22050.0f32;

        for b in 0..NUM_BINS {
            let p0 = b as f32 / NUM_BINS as f32;
            let p1 = (b + 1) as f32 / NUM_BINS as f32;
            let f0 = f_min * (f_max / f_min).powf(p0);
            let f1 = f_min * (f_max / f_min).powf(p1);

            let i0 = ((f0 / nyq) * half as f32).floor() as usize;
            let i1 = (((f1 / nyq) * half as f32).ceil() as usize).max(i0 + 1);

            let start = i0.clamp(1, half - 1);
            let end = i1.clamp(start + 1, half);

            let sum: f32 = mags[start..end].iter().sum();
            let avg = sum / (end - start) as f32;

            let db = 20.0 * (avg * 4.0 + 1e-6).log10();
            let norm = ((db + 48.0) / 48.0).clamp(0.0, 1.0);

            let prev = self.bins[b];
            self.bins[b] = if norm > prev {
                norm
            } else {
                (prev - decay).max(norm).max(0.0)
            };
        }
    }

    pub fn track_name(&self) -> &str {
        self.tracks
            .get(self.idx)
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("No track loaded")
    }

    pub fn state_str(&self) -> &'static str {
        if self.tracks.is_empty() {
            "Stopped"
        } else if self.audio.is_paused() {
            "Paused"
        } else if self.audio.is_empty() {
            "Finished"
        } else {
            "Playing"
        }
    }
}

fn collect_tracks(target: &Path) -> Result<Vec<PathBuf>> {
    let mut tracks = Vec::new();
    if target.is_dir() {
        for entry in std::fs::read_dir(target)? {
            let p = entry?.path();
            if is_audio(&p) {
                tracks.push(p);
            }
        }
        tracks.sort();
    } else if is_audio(target) {
        tracks.push(target.to_path_buf());
    }
    Ok(tracks)
}

fn is_audio(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("mp3" | "wav" | "flac")
    )
}
