//! Headless Phase 1 engine harness.
//!
//! Wires libVLC events through an async channel into the core state machine,
//! drives consecutive playback, applies A–B loops, saves resume positions and
//! reports every state transition. No UI, no video window: it exercises the
//! acceptance criteria that can be verified from a terminal.

use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
use std::{env, process};

use player_core::brand;
use player_core::cli::Cli;
use player_core::error::{CoreError, Result as CoreResult};
use player_core::model::{MediaKind, TrackInfo};
use player_core::session::{Session, SessionEffects};
use player_core::settings::SettingsStore;
use player_core::state::{PlaybackEvent, PlaybackState};
use player_core::store::Store;
use vlc_wrapper::{Instance, LibVlc, Media, MediaPlayer, PlayerEvent, VideoFrames, VlcError};

fn main() {
    if let Err(e) = run() {
        eprintln!("engine: {e}");
        process::exit(1);
    }
}

fn run() -> CoreResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let cli = Cli::parse(&args).map_err(|e| CoreError::Invalid(e.to_string()))?;
    if cli.media.is_empty() {
        eprintln!(
            "usage: engine <file-or-url>... [--runtime DIR] [--max-seconds N] \
             [--start-time SECS] [--rtl] [--no-video]"
        );
        process::exit(2);
    }

    let lib = match &cli.runtime {
        Some(dir) => LibVlc::load_from(dir).map_err(wrapper_error)?,
        None => LibVlc::discover().map_err(wrapper_error)?,
    };
    // `intf=dummy` plus no video keeps this harness usable in CI and on a
    // headless desktop session. Audio output is left alone so libVLC's clock
    // advances at real speed and produces meaningful time events.
    let instance =
        Instance::new(lib.clone(), &["--intf", "dummy", "--no-osd"]).map_err(wrapper_error)?;
    let mut player = MediaPlayer::new(&instance).map_err(wrapper_error)?;
    // A tiny frame sink gives libVLC a real presentation clock; without it the
    // synthetic video-only clips finish instantly and no resume timing is testable.
    let frames = VideoFrames::new(320, 180).map_err(wrapper_error)?;
    player
        .set_video_frames(frames.clone())
        .map_err(wrapper_error)?;
    let events = player.take_events().expect("event receiver is taken once");

    let config = brand::config_dir();
    let store = Store::open(&config.join("library.sqlite3"))?;
    let settings = SettingsStore::open(config.join("settings.toml"))?;
    let mut session = Session::new(store, settings)?;
    session.set_queue(cli.media.clone(), 0)?;

    println!("APP={} ID={}", brand::APP_NAME, brand::APP_ID);
    println!("VLC={}", lib.version());
    println!("QUEUE={:?}", cli.media);
    println!(
        "RESUME={:?}",
        session.resume_position(&cli.media[0])?.map(|d| d.as_secs())
    );

    let queue = cli.media.clone();
    let mut index = 0usize;
    let mut played = 0usize;
    let global_deadline = cli
        .max_seconds
        .map(|s| Instant::now() + Duration::from_secs(s * queue.len() as u64 + 10));
    loop {
        if played >= queue.len() || global_deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        let uri = queue[index].clone();
        match play_item(
            &mut session,
            &instance,
            &player,
            &frames,
            &events,
            &uri,
            cli.start_time,
            cli.max_seconds,
        )? {
            Outcome::Next(next_uri) => {
                played += 1;
                index = queue
                    .iter()
                    .position(|u| u == &next_uri)
                    .unwrap_or(index + 1);
                if index >= queue.len() {
                    break;
                }
            }
            Outcome::Stop => break,
        }
    }
    println!("ENGINE_DONE items={}", queue.len());
    Ok(())
}

enum Outcome {
    Next(String),
    Stop,
}

#[allow(clippy::too_many_arguments)]
fn play_item(
    session: &mut Session,
    instance: &Instance,
    player: &MediaPlayer,
    frames: &VideoFrames,
    events: &Receiver<PlayerEvent>,
    uri: &str,
    start_time: Option<Duration>,
    max_seconds: Option<u64>,
) -> CoreResult<Outcome> {
    let kind = classify(uri);
    let title = Path::new(uri)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| uri.to_owned());
    let media_id = session.begin_playback(uri, &title, kind)?;
    println!("ITEM_START media={media_id} uri={uri}");
    let media = if uri.contains("://") {
        Media::from_location(instance, uri)
    } else {
        Media::from_path(instance, Path::new(uri))
    }
    .map_err(wrapper_error)?;

    player.set_media(&media);
    let resume = match start_time {
        Some(t) => Some(t),
        None => session.resume_position(uri)?,
    };
    if let Some(position) = resume {
        println!(
            "RESUME_APPLIED media={media_id} secs={}",
            position.as_secs()
        );
        player.set_time(position);
    }
    player.play().map_err(wrapper_error)?;

    let deadline = max_seconds.map(|s| Instant::now() + Duration::from_secs(s));
    let mut last_print = Instant::now();
    let mut last_status = PlaybackState::Stopped;

    loop {
        // Keep the double buffer draining; dropping the lease frees the slot.
        let _ = frames.latest();
        let event = match events.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => {
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    // A capped run saves an *incomplete* position so the next
                    // run can demonstrate resume, then advances the queue.
                    let pos = player.time().unwrap_or_default();
                    session.store.save_progress(
                        media_id,
                        pos.as_millis().min(i64::MAX as u128) as i64,
                        false,
                    )?;
                    println!("ITEM_CAPPED media={media_id} ms={}", pos.as_millis());
                    player.stop();
                    return Ok(session
                        .queue
                        .next_uri()
                        .map(str::to_owned)
                        .map_or(Outcome::Stop, Outcome::Next));
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(Outcome::Stop),
        };

        let mapper: PlaybackEvent = map_event(&event);
        if matches!(mapper, PlaybackEvent::Playing) {
            publish_metadata(session, player);
        }
        let effects = session.apply_event(mapper);
        session.commit_effects(&effects)?;
        apply_effects(player, &effects);

        let state = session.state.status;
        if state != last_status {
            last_status = state;
            println!("STATE={state:?}");
        }
        if last_print.elapsed() >= Duration::from_secs(1) {
            last_print = Instant::now();
            println!(
                "TICK media={media_id} state={state:?} time={:?} len={:?}",
                session.state.time, session.state.length
            );
        }

        if effects.stop || matches!(event, PlayerEvent::EncounteredError) {
            return Ok(Outcome::Stop);
        }
        if let Some(next) = effects.play {
            player.stop();
            return Ok(Outcome::Next(next));
        }
    }
}

fn publish_metadata(session: &mut Session, player: &MediaPlayer) {
    // Called from the worker thread, never from a libVLC callback.
    let tracks = PlaybackEvent::Tracks {
        audio: to_tracks(player.audio_tracks()),
        video: to_tracks(player.video_tracks()),
        subtitle: to_tracks(player.subtitle_tracks()),
    };
    session.apply_event(tracks);
    session.apply_event(PlaybackEvent::Chapters {
        current: player.chapter(),
        count: player.chapter_count(),
    });
    session.apply_event(PlaybackEvent::AspectRatioChanged(player.aspect_ratio()));
    session.apply_event(PlaybackEvent::VolumeChanged(player.volume()));
    session.apply_event(PlaybackEvent::MuteChanged(player.is_muted()));
    session.apply_event(PlaybackEvent::AudioDelayChanged(player.audio_delay_us()));
    session.apply_event(PlaybackEvent::SubtitleDelayChanged(
        player.subtitle_delay_us(),
    ));
}

fn to_tracks(tracks: Vec<vlc_wrapper::Track>) -> Vec<TrackInfo> {
    tracks
        .into_iter()
        .map(|t| TrackInfo {
            id: t.id,
            name: t.name,
        })
        .collect()
}

fn map_event(event: &PlayerEvent) -> PlaybackEvent {
    match *event {
        PlayerEvent::Opening => PlaybackEvent::Opening,
        PlayerEvent::Buffering(v) => PlaybackEvent::Buffering(v),
        PlayerEvent::Playing => PlaybackEvent::Playing,
        PlayerEvent::Paused => PlaybackEvent::Paused,
        PlayerEvent::Stopped => PlaybackEvent::Stopped,
        PlayerEvent::EndReached => PlaybackEvent::EndReached,
        PlayerEvent::EncounteredError => PlaybackEvent::EncounteredError,
        PlayerEvent::TimeChanged(t) => PlaybackEvent::TimeChanged(t),
        PlayerEvent::PositionChanged(p) => PlaybackEvent::PositionChanged(p),
        PlayerEvent::LengthChanged(l) => PlaybackEvent::LengthChanged(l),
        PlayerEvent::VoutCount(n) => PlaybackEvent::VoutCount(n),
    }
}

fn apply_effects(player: &MediaPlayer, effects: &SessionEffects) {
    if let Some(seek) = effects.seek {
        player.set_time(seek);
    }
}

fn classify(uri: &str) -> MediaKind {
    if uri.contains("://") {
        return MediaKind::Stream;
    }
    match Path::new(uri)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3" | "m4a" | "aac" | "flac" | "wav" | "ogg" | "opus" | "wma") => MediaKind::Audio,
        _ => MediaKind::Video,
    }
}

fn wrapper_error(e: VlcError) -> CoreError {
    CoreError::Invalid(e.to_string())
}
