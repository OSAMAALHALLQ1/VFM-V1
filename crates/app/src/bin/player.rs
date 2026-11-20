//! Phase 3 player: Slint view wired to the Phase 1 engine on a worker thread.

slint::include_modules!();

#[path = "../i18n.rs"]
mod i18n;

use std::error::Error;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use i18n::{Catalog, Locale};
use player_core::cli::Cli;
use player_core::error::CoreError;
use player_core::model::MediaKind;
use player_core::platform::SingleInstance;
use player_core::session::Session;
use player_core::settings::SettingsStore;
use player_core::state::{PlaybackEvent, PlaybackState};
use player_core::store::Store;
use player_core::time::{format_hms, progress};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode};
use vlc_wrapper::{Instance, LibVlc, Media, MediaPlayer, PlayerEvent, VideoFrames};

#[derive(Debug)]
struct UiUpdate {
    title: String,
    elapsed: String,
    total: String,
    progress: f32,
    playing: bool,
    volume: f32,
    muted: bool,
}

#[derive(Debug)]
enum Command {
    TogglePause,
    SeekSecs(i64),
    SeekPos(f32),
    SetVolume(f32),
    ToggleMute,
    Next,
    Previous,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = Cli::parse(&args).map_err(|e| CoreError::Invalid(e.to_string()))?;
    let locale = Locale::parse(std::env::var("PLAYER_LOCALE").as_deref().unwrap_or("en"));

    if SingleInstance::is_second_instance() {
        SingleInstance::notify(&cli.media)?;
        return Ok(());
    }
    let _instance_guard = SingleInstance::try_acquire()?;

    let lib = match &cli.runtime {
        Some(dir) => LibVlc::load_from(dir)?,
        None => LibVlc::discover()?,
    };
    let instance = Instance::new(lib.clone(), &["--intf", "dummy", "--no-osd"])?;
    let mut player = MediaPlayer::new(&instance)?;
    let frames = VideoFrames::new(1280, 720)?;
    player.set_video_frames(frames.clone())?;
    let events = player.take_events().expect("event receiver taken once");

    let config = player_core::brand::config_dir();
    let store = Store::open(&config.join("library.sqlite3"))?;
    let settings = SettingsStore::open(config.join("settings.toml"))?;
    let mut session = Session::new(store, settings)?;
    session.set_queue(cli.media.clone(), 0)?;

    let ui = PlayerWindow::new()?;
    let mini = MiniPlayerWindow::new()?;
    let catalog = Catalog::load(locale);
    i18n::apply(&ui.global::<Strings>(), &catalog);
    i18n::apply(&mini.global::<Strings>(), &catalog);
    let effective_locale = catalog.locale();
    let family = if effective_locale.rtl() {
        "Noto Sans Arabic"
    } else {
        "Inter"
    };
    let theme = ui.global::<AppTheme>();
    theme.set_locale(effective_locale.code().into());
    theme.set_rtl(effective_locale.rtl());
    theme.set_font_family(family.into());
    let mini_theme = mini.global::<AppTheme>();
    mini_theme.set_locale(effective_locale.code().into());
    mini_theme.set_rtl(effective_locale.rtl());
    mini_theme.set_font_family(family.into());
    if cli.rtl {
        ui.global::<AppTheme>().set_rtl(true);
        mini.global::<AppTheme>().set_rtl(true);
    }
    ui.set_media_title(cli.media.first().cloned().unwrap_or_default().into());
    if std::env::var_os("PLAYER_KEEP_UI").is_some() {
        ui.set_autohide(false);
    }
    if std::env::var_os("PLAYER_LIGHT").is_some() {
        ui.global::<AppTheme>().set_mode("light".into());
        mini.global::<AppTheme>().set_mode("light".into());
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (ui_tx, ui_rx) = mpsc::channel::<UiUpdate>();

    wire_callbacks(&ui, &mini, cmd_tx.clone());

    // Video frames are consumed on the UI thread with a short lease, then
    // uploaded to Slint; libVLC is never touched from this thread.
    let video_frames = frames.clone();
    let ui_weak = ui.as_weak();
    let mini_weak = mini.as_weak();
    let video_timer = Timer::default();
    video_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(frame) = video_frames.latest() else {
            return;
        };
        let mut pixels =
            SharedPixelBuffer::<Rgba8Pixel>::new(video_frames.width(), video_frames.height());
        let (src, _) = frame.bgra_bytes().as_chunks::<4>();
        let (dst, _) = pixels.make_mut_bytes().as_chunks_mut::<4>();
        for (s, d) in src.iter().zip(dst.iter_mut()) {
            d.copy_from_slice(&[s[2], s[1], s[0], 255]);
        }
        drop(frame);
        let image = Image::from_rgba8(pixels);
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_video_frame(image.clone());
        }
        if let Some(mini) = mini_weak.upgrade() {
            mini.set_video_frame(image);
        }
    });

    let ui_weak = ui.as_weak();
    let state_timer = Timer::default();
    state_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        while let Ok(update) = ui_rx.try_recv() {
            ui.set_elapsed(update.elapsed.into());
            ui.set_total(update.total.into());
            ui.set_progress(update.progress);
            ui.set_playing(update.playing);
            ui.set_volume(update.volume);
            ui.set_muted(update.muted);
            if !update.title.is_empty() {
                ui.set_media_title(update.title.into());
            }
        }
    });

    let worker = std::thread::Builder::new()
        .name("vlc-worker".into())
        .spawn(move || {
            worker_loop(
                instance, player, events, session, frames, cli.media, cmd_rx, ui_tx,
            );
        })?;

    ui.run()?;
    drop(cmd_tx);
    let _ = worker.join();
    Ok(())
}

fn wire_callbacks(ui: &PlayerWindow, mini: &MiniPlayerWindow, tx: Sender<Command>) {
    let t = tx.clone();
    ui.on_toggle_play(move || {
        let _ = t.send(Command::TogglePause);
    });
    let t = tx.clone();
    ui.on_seek(move |p| {
        let _ = t.send(Command::SeekPos(p));
    });
    let t = tx.clone();
    ui.on_relative_seek(move |s| {
        let _ = t.send(Command::SeekSecs(i64::from(s)));
    });
    let t = tx.clone();
    ui.on_set_volume(move |v| {
        let _ = t.send(Command::SetVolume(v));
    });
    let t = tx.clone();
    ui.on_toggle_mute(move || {
        let _ = t.send(Command::ToggleMute);
    });
    let t = tx.clone();
    ui.on_next(move || {
        let _ = t.send(Command::Next);
    });
    let t = tx.clone();
    ui.on_previous(move || {
        let _ = t.send(Command::Previous);
    });

    let ui_weak = ui.as_weak();
    ui.on_toggle_fullscreen(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let window = ui.window();
            let next = !window.is_fullscreen();
            window.set_fullscreen(next);
            ui.invoke_show_ui("F".into());
        }
    });
    let ui_weak = ui.as_weak();
    let mini_weak = mini.as_weak();
    ui.on_toggle_pip(move || {
        if let Some(mini) = mini_weak.upgrade() {
            mini.show().ok();
        }
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_show_ui("P".into());
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_toggle_sidebar(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let visible = !ui.get_sidebar_visible();
            ui.set_sidebar_visible(visible);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_open_file(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let text = ui.global::<Strings>().get_osd_open_later();
            ui.invoke_show_ui(text);
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_minimize_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.window().set_minimized(true);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_maximize_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let window = ui.window();
            window.set_maximized(!window.is_maximized());
        }
    });
    ui.on_close_window(move || {
        let _ = slint::quit_event_loop();
    });

    let t = tx;
    mini.on_toggle_play(move || {
        let _ = t.send(Command::TogglePause);
    });
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    instance: Instance,
    player: MediaPlayer,
    events: Receiver<PlayerEvent>,
    mut session: Session,
    _frames: VideoFrames,
    media: Vec<String>,
    commands: Receiver<Command>,
    updates: Sender<UiUpdate>,
) {
    let mut index = 0usize;
    if let Some(uri) = media.first()
        && let Err(e) = start_item(&mut session, &instance, &player, uri)
    {
        eprintln!("playback start failed: {e}");
    }

    loop {
        match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(Command::TogglePause) => player.toggle_pause(),
            Ok(Command::SeekSecs(secs)) => {
                let base = player.time().unwrap_or_default();
                let step = Duration::from_secs(secs.unsigned_abs());
                let target = if secs < 0 {
                    base.saturating_sub(step)
                } else {
                    base.saturating_add(step)
                };
                player.set_time(target);
            }
            Ok(Command::SeekPos(p)) => {
                if let Some(len) = player.length() {
                    player.set_position(p);
                    let _ = len;
                }
            }
            Ok(Command::SetVolume(v)) => player.set_volume((v * 200.0) as i32),
            Ok(Command::ToggleMute) => {
                let muted = !player.is_muted();
                player.set_muted(muted);
            }
            Ok(Command::Next) => {
                if let Some(uri) = session.queue.next_uri().map(str::to_owned) {
                    index = media.iter().position(|u| u == &uri).unwrap_or(index + 1);
                    if let Err(e) = start_item(&mut session, &instance, &player, &uri) {
                        eprintln!("{e}");
                    }
                }
            }
            Ok(Command::Previous) => {
                if let Some(uri) = session.queue.previous().map(str::to_owned)
                    && let Err(e) = start_item(&mut session, &instance, &player, &uri)
                {
                    eprintln!("{e}");
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        while let Ok(event) = events.try_recv() {
            let mapped = match event {
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
            };
            if matches!(mapped, PlaybackEvent::Playing) {
                publish_metadata(&mut session, &player);
            }
            let effects = session.apply_event(mapped);
            if let Err(e) = session.commit_effects(&effects) {
                eprintln!("{e}");
            }
            if let Some(seek) = effects.seek {
                player.set_time(seek);
            }
            if let Some(next) = effects.play {
                index = media.iter().position(|u| u == &next).unwrap_or(index + 1);
                if let Err(e) = start_item(&mut session, &instance, &player, &next) {
                    eprintln!("{e}");
                }
            }

            let state = &session.state;
            let len = state.length;
            let update = UiUpdate {
                title: media.get(index).cloned().unwrap_or_default(),
                elapsed: format_hms(state.time),
                total: len.map_or_else(|| "--:--:--".into(), format_hms),
                progress: progress(state.time, len),
                playing: state.status == PlaybackState::Playing,
                volume: player.volume() as f32 / 200.0,
                muted: player.is_muted(),
            };
            let _ = updates.send(update);
        }
    }
    player.stop();
}

fn start_item(
    session: &mut Session,
    instance: &Instance,
    player: &MediaPlayer,
    uri: &str,
) -> Result<(), Box<dyn Error>> {
    let title = Path::new(uri)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| uri.to_owned());
    let kind = if uri.contains("://") {
        MediaKind::Stream
    } else {
        MediaKind::Video
    };
    session.begin_playback(uri, &title, kind)?;
    let media = if uri.contains("://") {
        Media::from_location(instance, uri)?
    } else {
        Media::from_path(instance, Path::new(uri))?
    };
    player.set_media(&media);
    if let Some(pos) = session.resume_position(uri)? {
        player.set_time(pos);
    }
    player.play()?;
    Ok(())
}

fn publish_metadata(session: &mut Session, player: &MediaPlayer) {
    let to_info = |tracks: Vec<vlc_wrapper::Track>| -> Vec<player_core::model::TrackInfo> {
        tracks
            .into_iter()
            .map(|t| player_core::model::TrackInfo {
                id: t.id,
                name: t.name,
            })
            .collect()
    };
    session.apply_event(PlaybackEvent::Tracks {
        audio: to_info(player.audio_tracks()),
        video: to_info(player.video_tracks()),
        subtitle: to_info(player.subtitle_tracks()),
    });
    session.apply_event(PlaybackEvent::Chapters {
        current: player.chapter(),
        count: player.chapter_count(),
    });
    session.apply_event(PlaybackEvent::AudioDelayChanged(player.audio_delay_us()));
    session.apply_event(PlaybackEvent::SubtitleDelayChanged(
        player.subtitle_delay_us(),
    ));
}
