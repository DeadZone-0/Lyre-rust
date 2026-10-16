use std::path::{Path, PathBuf};
use anyhow::{bail, Result};
use crossterm::event::KeyCode;

use crate::audio::Audio;

pub struct App {
    pub audio: Audio,
    pub tracks: Vec<PathBuf>,
    pub idx: usize,
    pub quit: bool,
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

        Ok(Self {
            audio,
            tracks,
            idx: 0,
            quit: false,
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
        Ok(())
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
