use std::process::Command;
use std::thread;
use std::time::Duration;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
#[allow(non_snake_case)]
struct LrcResponse {
    syncedLyrics: Option<String>,
}

struct LyricLine {
    time_ms: u64,
    text: String,
}

// pull current track tags from cmus
fn get_cmus_status() -> Option<(String, String)> {
    let output = Command::new("cmus-remote").arg("-Q").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut artist = None;
    let mut title = None;

    for line in stdout.lines() {
        if line.starts_with("tag artist ") {
            artist = Some(line.trim_start_matches("tag artist ").to_string());
        } else if line.starts_with("tag title ") {
            title = Some(line.trim_start_matches("tag title ").to_string());
        }
    }
    Some((artist?, title?))
}

// get current playback position in ms
fn get_cmus_position() -> u64 {
    if let Ok(output) = Command::new("cmus-remote").arg("-Q").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with("position ") {
                if let Ok(sec) = line.trim_start_matches("position ").parse::<u64>() {
                    return sec * 1000;
                }
            }
        }
    }
    0
}

// quick url encoder for API requests
fn urlencode(s: &str) -> String {
    s.chars().map(|c| match c {
        ' ' => "+".to_string(),
        _ if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' => c.to_string(),
        _ => format!("%{:02X}", c as u32),
    }).collect()
}

// parse an LRC line into a struct with a timestamp
fn parse_line(line: &str) -> Option<LyricLine> {
    if !line.starts_with('[') || line.len() < 10 { return None; }
    let m: u64 = line[1..3].parse().ok()?;
    let s: u64 = line[4..6].parse().ok()?;
    let ms: u64 = line[7..9].parse().unwrap_or(0) * 10;
    let text = line[10..].trim().to_string();
    if text.is_empty() { return None; }
    Some(LyricLine { time_ms: m * 60000 + s * 1000 + ms, text })
}

// grab lyrics from lrclib
fn fetch_lyrics(artist: &str, title: &str) -> Option<Vec<LyricLine>> {
    let url = format!("https://lrclib.net/api/get?artist_name={}&track_name={}", urlencode(artist), urlencode(title));
    let resp = ureq::get(&url).call().ok()?;
    let data: LrcResponse = resp.into_json().ok()?;
    let synced = data.syncedLyrics?;
    let lines: Vec<LyricLine> = synced.lines().filter_map(parse_line).collect();
    if lines.is_empty() { None } else { Some(lines) }
}

fn main() {
    // hide cursor so it doesn't clutter the terminal view
    print!("\x1b[?25l");
    
    const LATENCY_OFFSET: u64 = 350;

    let mut current_artist = String::new();
    let mut current_title = String::new();
    let mut lyrics: Vec<LyricLine> = Vec::new();

    loop {
        if let Some((artist, title)) = get_cmus_status() {
            // track changed — fetch new lyrics
            if artist != current_artist || title != current_title {
                current_artist = artist;
                current_title = title;
                
                lyrics = fetch_lyrics(&current_artist, &current_title).unwrap_or_else(|| {
                    vec![LyricLine { time_ms: 0, text: "(Synced lyrics not found)".to_string() }]
                });
            }

            let pos = get_cmus_position().saturating_add(LATENCY_OFFSET);
            let mut active = 0;
            for (i, l) in lyrics.iter().enumerate() {
                if pos >= l.time_ms { active = i; } else { break; }
            }

            // clear screen before drawing the frame
            print!("\x1b[H\x1b[2J");
            
            let indent = "      ";

            // player header
            println!("\n{} \x1b[1;36m♫ Now Playing:\x1b[0m \x1b[1m{}\x1b[0m \x1b[2m-\x1b[0m \x1b[1m{}\x1b[0m\n\n", indent, current_title, current_artist);

            let window_size = 7;
            let start = active.saturating_sub(window_size / 2);
            let end = std::cmp::min(start + window_size, lyrics.len());

            for i in start..end {
                if i == active {
                    let cur = lyrics[i].time_ms;
                    let next = if i + 1 < lyrics.len() { lyrics[i + 1].time_ms } else { cur + 3500 };
                    let dur = next.saturating_sub(cur).max(300);
                    let prog = (pos.saturating_sub(cur) as f32 / dur as f32).clamp(0.0, 1.0);

                    let line_chars: Vec<char> = lyrics[i].text.chars().collect();
                    let total_chars = line_chars.len();
                    let elapsed = ((total_chars as f32 * prog) * 1.1) as usize;

                    // render active line 
                    print!("{} \x1b[1;32m❯\x1b[0m   ", indent);
                    for (idx, &ch) in line_chars.iter().enumerate() {
                        if idx < elapsed {
                            print!("\x1b[1;97m{}\x1b[0m", ch); // already sung (white)
                        } else if idx == elapsed {
                            print!("\x1b[1;32m{}\x1b[0m", ch); // right now (green)
                        } else {
                            print!("\x1b[2m{}\x1b[0m", ch);     // upcoming (dim)
                        }
                    }
                    println!("\n");
                } else if i < active {
                    // already past
                    println!("{}     \x1b[2m{}\x1b[0m", indent, lyrics[i].text);
                } else {
                    // upcoming lines
                    println!("{}     \x1b[90m{}\x1b[0m", indent, lyrics[i].text);
                }
            }
        } else {
            print!("\x1b[H\x1b[2J");
            println!("\n      Nothing is playing in cmus.");
        }

        // frame rate / poll interval
        thread::sleep(Duration::from_millis(30));
    }
}
