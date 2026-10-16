use std::time::Duration;
use mpris::{PlaybackStatus, PlayerFinder};

pub fn poll(
    title: &mut String,
    artist: &mut String,
    player: &mut String,
    pos: &mut Duration,
    len: &mut Duration,
    playing: &mut bool,
) -> bool {
    read_state(title, artist, player, pos, len, playing).is_some()
}

fn read_state(
    title: &mut String,
    artist: &mut String,
    player: &mut String,
    pos: &mut Duration,
    len: &mut Duration,
    playing: &mut bool,
) -> Option<()> {
    let p = get_player()?;
    let meta = p.get_metadata().ok()?;
    let t = meta.title()?;

    title.clear();
    title.push_str(t);

    artist.clear();
    meta.artists().unwrap_or_default().first().map(|a| artist.push_str(a));

    player.clear();
    player.push_str(p.identity());

    *pos = p.get_position().unwrap_or_default();
    *len = meta.length().unwrap_or_default();
    *playing = p.get_playback_status().unwrap_or(PlaybackStatus::Stopped) == PlaybackStatus::Playing;

    ().into()
}

pub fn play_pause() {
    get_player().map(|p| p.play_pause());
}

pub fn next() {
    get_player().map(|p| p.next());
}

pub fn previous() {
    get_player().map(|p| p.previous());
}

fn get_player() -> Option<mpris::Player> {
    let finder = PlayerFinder::new().ok()?;
    finder
        .find_active()
        .ok()
        .or_else(|| finder.iter_players().ok()?.filter_map(|r| r.ok()).next())
}
