use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use lofty::file::TaggedFileExt;
use lofty::tag::Accessor;

#[derive(Clone, Debug)]
pub struct LyricLine {
    pub time: Duration,
    pub text: String,
}

#[derive(Default, Clone, Debug)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    pub offset_ms: i64,
}

impl Lyrics {
    pub fn load(audio_path: &Path) -> Self {
        let lrc_path = audio_path.with_extension("lrc");
        if lrc_path.exists() {
            if let Ok(content) = fs::read_to_string(&lrc_path) {
                return Self {
                    lines: parse_lrc(&content),
                    offset_ms: 0,
                };
            }
        }

        let (artist, title) = extract_tags(audio_path);
        if let Some(t) = title {
            if let Some(content) = fetch_lrclib_query(artist.as_deref(), &t) {
                let _ = fs::write(&lrc_path, &content);
                return Self {
                    lines: parse_lrc(&content),
                    offset_ms: 0,
                };
            }
        }

        Self::default()
    }

    pub fn load_cached(title: &str, artist: Option<&str>) -> Self {
        let clean_t = clean_string(title);
        let clean_a = artist.map(clean_string);

        let cache_dir = get_cache_dir();
        let safe_artist = clean_a.as_deref().unwrap_or("unknown");
        let safe_filename = format!("{safe_artist}_{clean_t}.lrc")
            .replace(['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|', ' '], "_");
        let lrc_path = cache_dir.join(safe_filename);

        if lrc_path.exists() {
            if let Ok(content) = fs::read_to_string(&lrc_path) {
                return Self {
                    lines: parse_lrc(&content),
                    offset_ms: 0,
                };
            }
        }

        if let Some(content) = fetch_lrclib_query(clean_a.as_deref(), &clean_t) {
            let _ = fs::create_dir_all(&cache_dir);
            let _ = fs::write(&lrc_path, &content);
            return Self {
                lines: parse_lrc(&content),
                offset_ms: 0,
            };
        }

        Self::default()
    }

    pub fn current_index(&self, pos: Duration) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let effective_ms = pos.as_millis() as i64 + self.offset_ms;
        if effective_ms < 0 {
            return None;
        }
        let t = Duration::from_millis(effective_ms as u64);
        let idx = self.lines.partition_point(|l| l.time <= t);
        if idx == 0 {
            if self.lines[0].time <= t {
                Some(0)
            } else {
                None
            }
        } else {
            Some(idx - 1)
        }
    }
}

pub fn parse_lrc(content: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if !line.starts_with('[') {
            continue;
        }
        if let Some(close) = line.find(']') {
            let tag = &line[1..close];
            let text = line[close + 1..].trim();
            if let Some((m_str, s_str)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (m_str.parse::<u64>(), s_str.parse::<f64>()) {
                    let ms = (m as f64 * 60.0 + s) * 1000.0;
                    lines.push(LyricLine {
                        time: Duration::from_millis(ms.max(0.0) as u64),
                        text: text.to_string(),
                    });
                }
            }
        }
    }
    lines.sort_by_key(|l| l.time);
    lines
}

fn fetch_lrclib_query(artist: Option<&str>, title: &str) -> Option<String> {
    if let Some(art) = artist {
        let resp = ureq::get("https://lrclib.net/api/get")
            .set("User-Agent", "Lyre/0.1.0")
            .query("artist_name", art)
            .query("track_name", title)
            .call();
        if let Ok(res) = resp {
            if let Ok(text) = res.into_string() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(synced) = val.get("syncedLyrics").and_then(|v| v.as_str()) {
                        if !synced.is_empty() {
                            return Some(synced.to_string());
                        }
                    }
                }
            }
        }
    }

    let q = match artist {
        Some(art) => format!("{art} {title}"),
        None => title.to_string(),
    };
    let resp = ureq::get("https://lrclib.net/api/search")
        .set("User-Agent", "Lyre/0.1.0")
        .query("q", &q)
        .call();
    if let Ok(res) = resp {
        if let Ok(text) = res.into_string() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(arr) = val.as_array() {
                    for item in arr {
                        if let Some(synced) = item.get("syncedLyrics").and_then(|v| v.as_str()) {
                            if !synced.is_empty() {
                                return Some(synced.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn get_cache_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache").join("lyra")
    } else {
        PathBuf::from("/tmp/lyra")
    }
}

fn extract_tags(audio_path: &Path) -> (Option<String>, Option<String>) {
    if let Ok(tagged) = lofty::read_from_path(audio_path) {
        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
        let artist = tag.and_then(|t| t.artist().map(|s| s.trim().to_string()));
        let title = tag.and_then(|t| t.title().map(|s| s.trim().to_string()));
        if title.is_some() {
            let clean_title = title.map(|t| clean_string(&t));
            let clean_artist = artist.map(|a| clean_string(&a));
            return (clean_artist, clean_title);
        }
    }
    parse_filename(audio_path)
}

fn parse_filename(audio_path: &Path) -> (Option<String>, Option<String>) {
    let Some(stem) = audio_path.file_stem().and_then(|s| s.to_str()) else {
        return (None, None);
    };
    let clean = stem.replace('_', " ");
    if let Some((a, t)) = clean.split_once(" - ") {
        (Some(clean_string(a)), Some(clean_string(t)))
    } else {
        (None, Some(clean_string(&clean)))
    }
}

fn clean_string(s: &str) -> String {
    let mut out = s.to_string();
    for delim in [" || ", " (", " ["] {
        if let Some(idx) = out.find(delim) {
            out.truncate(idx);
        }
    }
    out.trim().to_string()
}
