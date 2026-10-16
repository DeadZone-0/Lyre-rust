use std::f32::consts::PI;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use anyhow::{anyhow, Result};
use crossterm::event::KeyCode;
use rustfft::{num_complex::Complex, Fft, FftPlanner};

use crate::audio::Audio;
use crate::lyrics::Lyrics;
use crate::mpris_sync;
use crate::theme::{Theme, THEMES};

const FFT_SIZE: usize = 2048;
const NUM_BINS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Local,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VizMode {
    Bars,
    Spectrum,
    Oscilloscope,
    CenterWave,
}

impl VizMode {
    pub fn next(self) -> Self {
        if self == Self::Bars {
            Self::Spectrum
        } else if self == Self::Spectrum {
            Self::Oscilloscope
        } else if self == Self::Oscilloscope {
            Self::CenterWave
        } else {
            Self::Bars
        }
    }

    pub fn name(self) -> &'static str {
        if self == Self::Bars {
            "CAVA Bars"
        } else if self == Self::Spectrum {
            "Spectrum"
        } else if self == Self::Oscilloscope {
            "Oscilloscope"
        } else {
            "Stereo Wave"
        }
    }
}

pub struct App {
    pub audio: Audio,
    pub mode: AppMode,
    pub mpris_active: bool,
    pub mpris_title: String,
    pub mpris_artist: String,
    pub mpris_player: String,
    pub mpris_pos: Duration,
    pub mpris_len: Duration,
    pub mpris_playing: bool,
    pub tracks: Vec<PathBuf>,
    pub idx: usize,
    pub selected_idx: usize,
    pub show_sidebar: bool,
    pub show_help: bool,
    pub quit: bool,
    pub bins: Vec<f32>,
    pub peaks: Vec<f32>,
    pub bass_avg: f32,
    pub pulse: f32,
    pub lyrics: Lyrics,
    pub viz_mode: VizMode,
    pub theme_idx: usize,
    last_mpris_title: String,
    last_mpris_artist: String,
    mpris_tick: u8,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
    lyrics_tx: Sender<Lyrics>,
    lyrics_rx: Receiver<Lyrics>,
}

impl App {
    pub fn new(target: Option<&str>) -> Result<Self> {
        let audio = Audio::new()?;
        let (tracks, start_idx, initial_mode) = if let Some(arg) = target {
            if arg == "--system" || arg == "-s" {
                let list = collect_tracks(Path::new(".")).unwrap_or_default();
                (list, 0, AppMode::System)
            } else {
                let p = PathBuf::from(arg);
                if !p.exists() {
                    return Err(anyhow!("path does not exist: {}", p.display()));
                }
                if p.is_dir() {
                    let list = collect_tracks(&p)?;
                    if list.is_empty() {
                        return Err(anyhow!("no audio files found (.mp3, .wav, .flac) in: {}", p.display()));
                    }
                    (list, 0, AppMode::Local)
                } else {
                    let parent = p.parent().unwrap_or(Path::new("."));
                    let mut list = collect_tracks(parent).unwrap_or_default();
                    if list.is_empty() {
                        list.push(p.clone());
                    }
                    let mut pos = 0;
                    for (i, t) in list.iter().enumerate() {
                        if t == &p {
                            pos = i;
                            break;
                        }
                    }
                    (list, pos, AppMode::Local)
                }
            }
        } else {
            let list = collect_tracks(Path::new(".")).unwrap_or_default();
            (list, 0, AppMode::System)
        };

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let mut window = Vec::with_capacity(FFT_SIZE);
        for i in 0..FFT_SIZE {
            let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos());
            window.push(w);
        }

        let fft_buf = vec![Complex { re: 0.0, im: 0.0 }; FFT_SIZE];
        let bins = vec![0.0; NUM_BINS];
        let peaks = vec![0.0; NUM_BINS];
        let (lyrics_tx, lyrics_rx) = channel();

        let mut app = Self {
            audio,
            mode: initial_mode,
            mpris_active: false,
            mpris_title: String::new(),
            mpris_artist: String::new(),
            mpris_player: String::new(),
            mpris_pos: Duration::ZERO,
            mpris_len: Duration::ZERO,
            mpris_playing: false,
            tracks,
            idx: start_idx,
            selected_idx: start_idx,
            show_sidebar: false,
            show_help: false,
            quit: false,
            bins,
            peaks,
            bass_avg: 0.0,
            pulse: 0.0,
            lyrics: Lyrics::default(),
            viz_mode: VizMode::Bars,
            theme_idx: 0,
            last_mpris_title: String::new(),
            last_mpris_artist: String::new(),
            mpris_tick: 0,
            fft,
            window,
            fft_buf,
            lyrics_tx,
            lyrics_rx,
        };

        if app.mode == AppMode::System {
            let _ = app.audio.start_capture();
            app.poll_mpris_immediate();
        } else if !app.tracks.is_empty() {
            let _ = app.load_current();
        }

        Ok(app)
    }

    pub fn theme(&self) -> &'static Theme {
        &THEMES[self.theme_idx]
    }

    pub fn toggle_mode(&mut self) {
        if self.mode == AppMode::Local {
            self.mode = AppMode::System;
            if !self.audio.is_paused() {
                self.audio.pause();
            }
            let _ = self.audio.start_capture();
            self.poll_mpris_immediate();
        } else {
            self.mode = AppMode::Local;
            self.audio.stop_capture();
            if !self.tracks.is_empty() && self.audio.is_paused() {
                self.audio.play();
            }
            self.lyrics = Lyrics::default();
            if !self.tracks.is_empty() {
                let p = self.tracks[self.idx].clone();
                let tx = self.lyrics_tx.clone();
                std::thread::spawn(move || {
                    let lrc = Lyrics::load(&p);
                    let _ = tx.send(lrc);
                });
            }
        }
    }

    fn poll_mpris_immediate(&mut self) {
        let active = mpris_sync::poll(
            &mut self.mpris_title,
            &mut self.mpris_artist,
            &mut self.mpris_player,
            &mut self.mpris_pos,
            &mut self.mpris_len,
            &mut self.mpris_playing,
        );
        self.mpris_active = active;

        if active {
            if self.mpris_title != self.last_mpris_title || self.mpris_artist != self.last_mpris_artist {
                self.last_mpris_title = self.mpris_title.clone();
                self.last_mpris_artist = self.mpris_artist.clone();
                self.lyrics = Lyrics::default();
                let title = self.mpris_title.clone();
                let artist = if self.mpris_artist.is_empty() {
                    None
                } else {
                    Some(self.mpris_artist.clone())
                };
                let tx = self.lyrics_tx.clone();
                std::thread::spawn(move || {
                    let lrc = Lyrics::load_cached(&title, artist.as_deref());
                    let _ = tx.send(lrc);
                });
            }
        } else {
            self.last_mpris_title.clear();
            self.last_mpris_artist.clear();
        }
    }

    pub fn current_pos(&self) -> Duration {
        if self.mode == AppMode::System {
            self.mpris_pos
        } else {
            self.audio.pos()
        }
    }

    pub fn current_duration(&self) -> Option<Duration> {
        if self.mode == AppMode::System {
            if self.mpris_active && !self.mpris_len.is_zero() {
                Some(self.mpris_len)
            } else {
                None
            }
        } else {
            self.audio.duration
        }
    }

    pub fn load_current(&mut self) -> Result<()> {
        if self.tracks.is_empty() {
            return Ok(());
        }
        let p = self.tracks[self.idx].clone();
        self.audio.load(&p)?;
        self.lyrics = Lyrics::default();
        let tx = self.lyrics_tx.clone();
        std::thread::spawn(move || {
            let lrc = Lyrics::load(&p);
            let _ = tx.send(lrc);
        });
        Ok(())
    }

    pub fn handle_key(&mut self, code: KeyCode) -> Result<()> {
        match code {
            KeyCode::Char('q') => {
                if self.show_help {
                    self.show_help = false;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else if self.show_sidebar {
                    self.show_sidebar = false;
                }
            }
            KeyCode::Char('?') => self.show_help = !self.show_help,
            KeyCode::Char('m') => self.toggle_mode(),
            KeyCode::Tab | KeyCode::Char('p') => {
                self.show_sidebar = !self.show_sidebar;
                if self.show_sidebar {
                    self.selected_idx = self.idx;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.show_sidebar && !self.tracks.is_empty() {
                    self.selected_idx = (self.selected_idx + 1).min(self.tracks.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.show_sidebar && !self.tracks.is_empty() {
                    self.selected_idx = self.selected_idx.saturating_sub(1);
                }
            }
            KeyCode::Enter => {
                if self.show_sidebar && !self.tracks.is_empty() {
                    if self.mode == AppMode::System {
                        self.toggle_mode();
                    }
                    self.idx = self.selected_idx;
                    self.load_current()?;
                }
            }
            KeyCode::Char(' ') => {
                if self.mode == AppMode::System {
                    mpris_sync::play_pause();
                } else {
                    self.audio.toggle_pause();
                }
            }
            KeyCode::Char('n') => {
                if self.mode == AppMode::System {
                    mpris_sync::next();
                } else {
                    self.skip()?;
                }
            }
            KeyCode::Char('b') => {
                if self.mode == AppMode::System {
                    mpris_sync::previous();
                } else {
                    self.prev_track()?;
                }
            }
            KeyCode::Char('s') => {
                if self.mode == AppMode::System {
                    mpris_sync::next();
                } else {
                    self.skip()?;
                }
            }
            KeyCode::Char('v') => self.viz_mode = self.viz_mode.next(),
            KeyCode::Char('t') => self.theme_idx = (self.theme_idx + 1) % THEMES.len(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let v = self.audio.volume() + 0.05;
                self.audio.set_volume(v);
            }
            KeyCode::Char('-') => {
                let v = self.audio.volume() - 0.05;
                self.audio.set_volume(v);
            }
            KeyCode::Char('[') => self.lyrics.offset_ms -= 100,
            KeyCode::Char(']') => self.lyrics.offset_ms += 100,
            _ => {}
        }
        Ok(())
    }

    pub fn skip(&mut self) -> Result<()> {
        if self.tracks.is_empty() {
            return Ok(());
        }
        self.idx = (self.idx + 1) % self.tracks.len();
        self.selected_idx = self.idx;
        self.load_current()
    }

    pub fn prev_track(&mut self) -> Result<()> {
        if self.tracks.is_empty() {
            return Ok(());
        }
        if self.idx == 0 {
            self.idx = self.tracks.len() - 1;
        } else {
            self.idx -= 1;
        }
        self.selected_idx = self.idx;
        self.load_current()
    }

    pub fn tick(&mut self) -> Result<()> {
        if self.mode == AppMode::Local {
            if !self.tracks.is_empty() && self.audio.is_empty() && !self.audio.is_paused() {
                if self.idx + 1 < self.tracks.len() {
                    self.idx += 1;
                    self.selected_idx = self.idx;
                    self.load_current()?;
                }
            }
        } else {
            self.mpris_tick = (self.mpris_tick + 1) % 15;
            if self.mpris_tick == 0 {
                self.poll_mpris_immediate();
            }
        }

        while let Ok(lrc) = self.lyrics_rx.try_recv() {
            self.lyrics = lrc;
        }

        self.update_fft();
        Ok(())
    }

    pub fn wave_samples(&self, count: usize) -> Vec<f32> {
        let mut out = vec![0.0; count];
        if let Ok(ring) = self.audio.samples.try_lock() {
            let len = ring.len();
            if len >= count {
                let offset = len - count;
                for i in 0..count {
                    out[i] = ring[offset + i];
                }
            }
        }
        out
    }

    pub fn stereo_samples(&self, count: usize) -> (Vec<f32>, Vec<f32>) {
        let mut left = vec![0.0; count];
        let mut right = vec![0.0; count];
        if let Ok(s) = self.audio.stereo.try_lock() {
            let len = s.0.len();
            if len >= count {
                let offset = len - count;
                for i in 0..count {
                    left[i] = s.0[offset + i];
                    right[i] = s.1[offset + i];
                }
            }
        }
        (left, right)
    }

    fn update_fft(&mut self) {
        if self.mode == AppMode::Local && (self.audio.is_paused() || self.audio.is_empty() || self.tracks.is_empty()) {
            for b in 0..self.bins.len() {
                self.bins[b] = (self.bins[b] * 0.94 - 0.002).max(0.0);
                self.peaks[b] = (self.peaks[b] - 0.0035).max(0.0);
            }
            self.pulse = (self.pulse - 0.04).max(0.0);
            return;
        }

        let lock_res = self.audio.samples.try_lock();
        if lock_res.is_err() {
            for b in 0..self.bins.len() {
                self.bins[b] = (self.bins[b] * 0.94 - 0.002).max(0.0);
                self.peaks[b] = (self.peaks[b] - 0.0035).max(0.0);
            }
            self.pulse = (self.pulse - 0.04).max(0.0);
            return;
        }
        let ring = lock_res.unwrap();

        if ring.len() < FFT_SIZE {
            for b in 0..self.bins.len() {
                self.bins[b] = (self.bins[b] * 0.94 - 0.002).max(0.0);
                self.peaks[b] = (self.peaks[b] - 0.0035).max(0.0);
            }
            self.pulse = (self.pulse - 0.04).max(0.0);
            return;
        }

        let offset = ring.len() - FFT_SIZE;
        for i in 0..FFT_SIZE {
            let s = ring[offset + i];
            let w = self.window[i];
            self.fft_buf[i] = Complex { re: s * w, im: 0.0 };
        }
        drop(ring);
        self.fft.process(&mut self.fft_buf);

        let half = FFT_SIZE / 2;
        let mut mags = vec![0.0f32; half];
        for i in 0..half {
            let c = self.fft_buf[i];
            mags[i] = (c.re * c.re + c.im * c.im).sqrt() / FFT_SIZE as f32;
        }

        let mut bass_sum = 0.0f32;
        for i in 1..=6 {
            bass_sum += mags[i];
        }
        let bass_energy = bass_sum / 6.0;
        if bass_energy > self.bass_avg * 1.35 && bass_energy > 0.012 {
            self.pulse = 1.0;
        }
        self.bass_avg = self.bass_avg * 0.94 + bass_energy * 0.06;
        self.pulse = (self.pulse - 0.04).max(0.0);

        let f_min = 42.0f32;
        let f_max = 16000.0f32;
        let nyq = 22050.0f32;
        let mut raw = [0.0f32; NUM_BINS];

        for b in 0..NUM_BINS {
            let p0 = b as f32 / NUM_BINS as f32;
            let p1 = (b + 1) as f32 / NUM_BINS as f32;
            let f0 = f_min * (f_max / f_min).powf(p0);
            let f1 = f_min * (f_max / f_min).powf(p1);

            let i0 = ((f0 / nyq) * half as f32).floor() as usize;
            let i1 = (((f1 / nyq) * half as f32).ceil() as usize).max(i0 + 1);

            let start = i0.clamp(1, half - 1);
            let end = i1.clamp(start + 1, half);

            let mut max_m = 0.0f32;
            let mut sum_m = 0.0f32;
            for i in start..end {
                let m = mags[i];
                if m > max_m {
                    max_m = m;
                }
                sum_m += m;
            }
            let avg_m = sum_m / (end - start) as f32;
            let band_mag = max_m * 0.75 + avg_m * 0.25;

            let t = b as f32 / (NUM_BINS - 1) as f32;
            let eq = 1.0 + 7.5 * t.powf(1.3);
            let bass_taper = if b == 0 { 0.75 } else if b == 1 { 0.88 } else { 1.0 };
            let val = band_mag * eq * bass_taper * 6.0;

            let db = 20.0 * (val + 1e-5).log10();
            let norm = ((db + 40.0) / 40.0).clamp(0.0, 1.0);
            raw[b] = norm;
        }

        let mut smooth = [0.0f32; NUM_BINS];
        for b in 0..NUM_BINS {
            if b == 0 {
                smooth[0] = raw[0] * 0.65 + raw[1] * 0.35;
            } else if b == NUM_BINS - 1 {
                smooth[b] = raw[b] * 0.65 + raw[b - 1] * 0.35;
            } else {
                smooth[b] = raw[b - 1] * 0.22 + raw[b] * 0.56 + raw[b + 1] * 0.22;
            }
        }

        for b in 0..NUM_BINS {
            let target = smooth[b];
            let prev = self.bins[b];

            let curr = if target > prev {
                prev + (target - prev) * 0.34
            } else {
                (prev * 0.935 - 0.002).max(0.0).max(target)
            };
            self.bins[b] = curr;

            let prev_peak = self.peaks[b];
            self.peaks[b] = if curr >= prev_peak {
                curr
            } else {
                (prev_peak - 0.0035).max(curr).max(0.0)
            };
        }
    }

    pub fn track_name(&self) -> &str {
        if let Some(p) = self.tracks.get(self.idx) {
            p.file_name().and_then(|n| n.to_str()).unwrap_or("No track loaded")
        } else {
            "No track loaded"
        }
    }

    pub fn state_str(&self) -> &'static str {
        if self.mode == AppMode::System {
            if !self.mpris_active {
                "Waiting"
            } else if self.mpris_playing {
                "Playing"
            } else {
                "Paused"
            }
        } else if self.tracks.is_empty() {
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
    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_ascii_lowercase();
        ext == "mp3" || ext == "wav" || ext == "flac"
    } else {
        false
    }
}
