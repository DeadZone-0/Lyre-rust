use std::io::{Read, Write};
use std::path::Path;
use lofty::file::TaggedFileExt;

#[derive(Clone, Default)]
pub struct CoverArt {
    pub rgb: Option<image::RgbImage>,
    pub png: Option<Vec<u8>>,
}

pub fn is_kitty() -> bool {
    if std::env::var("KITTY_PID").is_ok() || std::env::var("KITTY_WINDOW_ID").is_ok() {
        return true;
    }
    let term = std::env::var("TERM").unwrap_or_default();
    if term.contains("kitty") {
        return true;
    }
    let prog = std::env::var("TERM_PROGRAM").unwrap_or_default();
    prog == "ghostty" || prog == "WezTerm"
}

pub fn extract_cover(audio_path: Option<&Path>, art_url: Option<&str>) -> Option<Vec<u8>> {
    if art_url.is_some() {
        let u = art_url.unwrap();
        if !u.is_empty() {
            let res = fetch_mpris_art(u);
            if res.is_some() {
                return res;
            }
        }
    }

    if audio_path.is_some() {
        let p = audio_path.unwrap();
        let res = extract_local_art(p);
        if res.is_some() {
            return res;
        }
    }

    None
}

fn extract_local_art(p: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(p).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    for pic in tag.pictures() {
        let d = pic.data();
        if !d.is_empty() {
            return d.to_vec().into();
        }
    }

    let parent = p.parent()?;
    for name in ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "album.jpg", "album.png", "front.jpg", "front.png"] {
        let candidate = parent.join(name);
        if candidate.exists() {
            let bytes = std::fs::read(&candidate).ok()?;
            return bytes.into();
        }
    }

    None
}

fn fetch_mpris_art(url: &str) -> Option<Vec<u8>> {
    if url.starts_with("file://") {
        let path = &url[7..];
        let bytes = std::fs::read(path).ok()?;
        return bytes.into();
    }
    if url.starts_with('/') {
        let bytes = std::fs::read(url).ok()?;
        return bytes.into();
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let resp = ureq::get(url).timeout(std::time::Duration::from_secs(4)).call().ok()?;
        let mut bytes = Vec::new();
        resp.into_reader().read_to_end(&mut bytes).ok()?;
        return bytes.into();
    }
    None
}

pub fn load_cover(bytes: &[u8]) -> Option<CoverArt> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgb = img.to_rgb8();

    let mut png = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png);
    img.write_to(&mut cursor, image::ImageFormat::Png).ok()?;

    CoverArt {
        rgb: rgb.into(),
        png: png.into(),
    }.into()
}

pub fn send_kitty_image(png_bytes: &[u8], col: u16, row: u16, cols: u16, rows: u16) {
    clear_kitty_image();
    let b64 = base64_encode(png_bytes);
    let mut stdout = std::io::stdout();
    let _ = crossterm::queue!(stdout, crossterm::cursor::MoveTo(col, row));

    let chunk_size = 4096;
    let bytes = b64.as_bytes();
    let mut offset = 0;
    let mut is_first = true;

    while offset < bytes.len() {
        let end = (offset + chunk_size).min(bytes.len());
        let chunk = std::str::from_utf8(&bytes[offset..end]).unwrap_or("");
        let is_last = end == bytes.len();
        let m = if is_last { 0 } else { 1 };

        if is_first {
            let _ = stdout.write_all(
                format!("\x1b_Ga=T,f=100,t=d,i=1,c={cols},r={rows},C=1,q=2,m={m};{chunk}\x1b\\").as_bytes(),
            );
            is_first = false;
        } else {
            let _ = stdout.write_all(
                format!("\x1b_Gm={m};{chunk}\x1b\\").as_bytes(),
            );
        }
        offset = end;
    }
    let _ = stdout.flush();
}

pub fn clear_kitty_image() {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\x1b_Ga=d,d=i,i=1\x1b\\");
    let _ = stdout.flush();
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(triple >> 18) & 0x3F] as char);
        out.push(TABLE[(triple >> 12) & 0x3F] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(triple >> 6) & 0x3F] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[triple & 0x3F] as char);
        } else {
            out.push('=');
        }
    }
    out
}
