//! Phase 3/5 player: Slint view wired to the engine, with subtitles and thumbnails.

slint::include_modules!();

#[path = "../i18n.rs"]
mod i18n;

use std::error::Error;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use i18n::{Catalog, Locale};
use player_core::cli::Cli;
use player_core::error::CoreError;
use player_core::model::MediaKind;
use player_core::platform::SingleInstance;
use player_core::presets::{
    self, AudioPreset, PlaybackPreset, PresetPayload, SubtitlePreset, VideoPreset,
};
use player_core::session::Session;
use player_core::settings::SettingsStore;
use player_core::state::{PlaybackEvent, PlaybackState};
use player_core::store::Store;
use player_core::subtitles::{self, SubtitleDocument};
use player_core::thumbnails::{ThumbnailCache, ThumbnailRequests};
use player_core::time::{format_hms, progress};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode};
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
    audio_tracks: Vec<TrackItem>,
    subtitle_tracks: Vec<TrackItem>,
    time: Duration,
    preset_names: Vec<SharedString>,
    equalizer_presets: Vec<SharedString>,
    applied_preset: Option<PresetPayload>,
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
    SelectAudioTrack(i32),
    SelectSubtitleTrack(i32),
    SetSubtitleDelayMs(i64),
    LoadSubtitle(String),
    SetAudioDelayMs(i64),
    SetVideoAdjust(u32, f32),
    SetAspect(String),
    SetSpeed(f32),
    SetSleep(u32),
    SetEqualizerPreset(u32),
    SavePreset {
        name: String,
        payload: Box<PresetPayload>,
    },
    LoadPreset(String),
    DeletePreset(String),
    RenamePreset {
        from: String,
        to: String,
    },
    SetPresetAuto {
        name: String,
        target: String,
        enabled: bool,
    },
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
    for theme in [ui.global::<AppTheme>(), mini.global::<AppTheme>()] {
        theme.set_locale(effective_locale.code().into());
        theme.set_rtl(effective_locale.rtl());
        theme.set_font_family(family.into());
    }
    if cli.rtl {
        ui.global::<AppTheme>().set_rtl(true);
        mini.global::<AppTheme>().set_rtl(true);
    }
    if std::env::var_os("PLAYER_LIGHT").is_some() {
        ui.global::<AppTheme>().set_mode("light".into());
        mini.global::<AppTheme>().set_mode("light".into());
    }
    ui.set_media_title(cli.media.first().cloned().unwrap_or_default().into());
    if std::env::var_os("PLAYER_KEEP_UI").is_some() {
        ui.set_autohide(false);
    }
    if std::env::var_os("PLAYER_SETTINGS").is_some() {
        ui.set_sidebar_visible(true);
        ui.set_sidebar_mode(1);
    }

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (ui_tx, ui_rx) = mpsc::channel::<UiUpdate>();
    let (thumb_tx, thumb_rx) = mpsc::channel::<(u64, u64)>(); // (token, time_ms)
    let (thumb_result_tx, thumb_result_rx) = mpsc::channel::<(u64, u64, Vec<u8>)>(); // (token, time_ms, rgba)

    let current_time = Arc::new(Mutex::new(Duration::ZERO));
    let subtitle_delay = Arc::new(Mutex::new(0i64));
    let subtitle_doc: Arc<Mutex<Option<SubtitleDocument>>> = Arc::new(Mutex::new(None));
    if let Some(path) = cli.subtitle_files.first()
        && let Ok(bytes) = std::fs::read(path)
        && let Ok(doc) = subtitles::parse(&bytes)
    {
        *subtitle_doc.lock().unwrap() = Some(doc);
    }

    let thumbnail_requests = Arc::new(Mutex::new(ThumbnailRequests::default()));
    wire_callbacks(
        &ui,
        &mini,
        cmd_tx.clone(),
        &thumb_tx,
        &subtitle_delay,
        &thumbnail_requests,
    );

    // Video frames: UI thread only, libVLC untouched here.
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

    // UI state snapshots from the worker.
    let time_for_subtitles = current_time.clone();
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
            ui.set_audio_tracks(model_from(update.audio_tracks));
            ui.set_subtitle_tracks(model_from(update.subtitle_tracks));
            ui.set_preset_names(slint::ModelRc::new(slint::VecModel::from(
                update.preset_names,
            )));
            ui.set_equalizer_presets(slint::ModelRc::new(slint::VecModel::from(
                update.equalizer_presets,
            )));
            if let Some(p) = update.applied_preset {
                ui.set_subtitle_delay_ms(p.subtitle.delay_ms as i32);
                ui.set_subtitle_relative_size(p.subtitle.relative_size);
                ui.set_audio_delay_ms(p.audio.delay_ms as i32);
                ui.set_video_brightness(p.video.brightness);
                ui.set_video_contrast(p.video.contrast);
                ui.set_video_saturation(p.video.saturation);
                ui.set_video_hue(p.video.hue as f32);
                ui.set_video_gamma(p.video.gamma);
                ui.set_video_zoom(p.video.zoom);
                ui.set_video_aspect(p.video.aspect_ratio.into());
                ui.set_playback_speed(p.playback.speed);
                ui.set_sleep_minutes(p.playback.sleep_minutes as i32);
            }
            *time_for_subtitles.lock().unwrap() = update.time;
            if !update.title.is_empty() {
                ui.set_media_title(update.title.into());
            }
        }
    });

    // Own subtitle renderer: active cues recomputed on every tick.
    let doc_for_timer = subtitle_doc.clone();
    let delay_for_timer = subtitle_delay.clone();
    let time_for_render = current_time.clone();
    let ui_weak = ui.as_weak();
    let subtitle_timer = Timer::default();
    subtitle_timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let Some(doc) = doc_for_timer.lock().unwrap().as_ref().cloned() else {
            return;
        };
        let delay = Duration::from_millis(delay_for_timer.lock().unwrap().max(0) as u64);
        let text = doc
            .active_cues(*time_for_render.lock().unwrap(), delay)
            .into_iter()
            .map(|cue| cue.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        ui.set_subtitle_text(text.into());
        ui.set_subtitle_visible(!ui.get_subtitle_text().is_empty());
    });

    // Seek thumbnails: cache on UI thread, headless grabber on a worker.
    let mut cache = ThumbnailCache::new(48);
    let requests_for_timer = thumbnail_requests.clone();
    let ui_weak = ui.as_weak();
    let thumb_timer = Timer::default();
    thumb_timer.start(TimerMode::Repeated, Duration::from_millis(40), move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        while let Ok((token, ms, rgba)) = thumb_result_rx.try_recv() {
            if !requests_for_timer.lock().unwrap().is_latest(token) {
                continue;
            }
            cache.insert(ms, rgba);
        }
        if ui.get_seek_thumb_visible() {
            let ms = (ui.get_progress() * 30_000.0) as u64; // approximate; worker uses requested ms
            if let Some(bytes) = cache.get(ms) {
                ui.set_seek_thumb(bytes_to_image(bytes));
            }
        }
    });

    let grabber_media = cli.media.first().cloned();
    let grabber_lib = lib.clone();
    let grabber = std::thread::Builder::new()
        .name("thumb-grabber".into())
        .spawn(move || {
            thumbnail_worker(grabber_lib, grabber_media, thumb_rx, thumb_result_tx);
        })?;

    // Text subtitles are rendered by our own Slint renderer. Only unparsable /
    // bitmap formats are handed to libVLC via Command::LoadSubtitle.
    if subtitle_doc.lock().unwrap().is_none() {
        for path in &cli.subtitle_files {
            let _ = cmd_tx.send(Command::LoadSubtitle(path.clone()));
        }
    }
    let _ = cmd_tx.send(Command::SetSubtitleDelayMs(0));

    let initial_presets = presets::load_all(&session.store)?;
    let eq_names = player.equalizer_presets();
    let worker = std::thread::Builder::new()
        .name("vlc-worker".into())
        .spawn(move || {
            worker_loop(
                instance,
                player,
                events,
                session,
                cli.media,
                cmd_rx,
                ui_tx,
                initial_presets,
                eq_names,
            );
        })?;

    ui.run()?;
    drop(cmd_tx);
    let _ = worker.join();
    let _ = grabber.join();
    Ok(())
}

fn model_from(tracks: Vec<TrackItem>) -> slint::ModelRc<TrackItem> {
    slint::ModelRc::new(slint::VecModel::from(tracks))
}

fn bytes_to_image(bytes: &[u8]) -> Image {
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(160, 90);
    let len = pixels.make_mut_bytes().len().min(bytes.len());
    pixels.make_mut_bytes()[..len].copy_from_slice(&bytes[..len]);
    Image::from_rgba8(pixels)
}

#[allow(clippy::too_many_arguments)]
fn wire_callbacks(
    ui: &PlayerWindow,
    mini: &MiniPlayerWindow,
    tx: Sender<Command>,
    thumb_tx: &Sender<(u64, u64)>,
    subtitle_delay: &Arc<Mutex<i64>>,
    requests: &Arc<Mutex<ThumbnailRequests>>,
) {
    let requests = requests.clone();
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
    let t = tx.clone();
    ui.on_select_audio_track(move |id| {
        let _ = t.send(Command::SelectAudioTrack(id));
    });
    let t = tx.clone();
    ui.on_select_subtitle_track(move |id| {
        let _ = t.send(Command::SelectSubtitleTrack(id));
    });
    let t = tx.clone();
    let delay = subtitle_delay.clone();
    ui.on_adjust_subtitle_delay(move |ms| {
        let mut d = delay.lock().unwrap();
        *d = (*d + i64::from(ms)).clamp(-10_000, 10_000);
        let _ = t.send(Command::SetSubtitleDelayMs(*d));
    });
    let t = tx.clone();
    ui.on_load_external_subtitle(move || {
        if let Ok(path) = std::env::var("PLAYER_SUBTITLE") {
            let _ = t.send(Command::LoadSubtitle(path));
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_toggle_audio_popover(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let open = !ui.get_audio_popover();
            ui.set_audio_popover(open);
            ui.set_subtitle_popover(false);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_toggle_subtitle_popover(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let open = !ui.get_subtitle_popover();
            ui.set_subtitle_popover(open);
            ui.set_audio_popover(false);
        }
    });
    let tx_thumb = thumb_tx.clone();
    let ui_weak = ui.as_weak();
    ui.on_seek_hover(move |pos| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if pos < 0.0 {
            ui.set_seek_thumb_visible(false);
            return;
        }
        let token = requests.lock().unwrap().request();
        let ms = (pos * 30_000.0) as u64;
        let _ = tx_thumb.send((token, ms));
    });

    let ui_weak = ui.as_weak();
    ui.on_adjust_subtitle_size(move |delta| {
        if let Some(ui) = ui_weak.upgrade() {
            let size = (ui.get_subtitle_size() + delta as f32).clamp(10.0, 72.0);
            ui.set_subtitle_size(size);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_cycle_subtitle_position(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let y = ui.get_subtitle_y();
            ui.set_subtitle_y(if y > 0.7 {
                0.5
            } else if y > 0.3 {
                0.08
            } else {
                0.86
            });
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_toggle_fullscreen(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let window = ui.window();
            window.set_fullscreen(!window.is_fullscreen());
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
            let v = !ui.get_sidebar_visible();
            ui.set_sidebar_visible(v);
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

    let weak = ui.as_weak();
    ui.on_select_settings_tab(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_tab(index);
        }
    });
    let t = tx.clone();
    ui.on_settings_subtitle_delay(move |v| {
        let _ = t.send(Command::SetSubtitleDelayMs(v as i64));
    });
    let weak = ui.as_weak();
    ui.on_settings_subtitle_size(move |v| {
        if let Some(ui) = weak.upgrade() {
            ui.set_subtitle_size(v * 0.2 + 10.0);
        }
    });
    let weak = ui.as_weak();
    ui.on_settings_outline(move |v| {
        if let Some(ui) = weak.upgrade() {
            ui.set_subtitle_outline(v);
        }
    });
    let weak = ui.as_weak();
    ui.on_settings_relative(move |v| {
        if let Some(ui) = weak.upgrade() {
            ui.set_subtitle_relative_size(v);
        }
    });
    let weak = ui.as_weak();
    ui.on_settings_position(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_subtitle_position_index(index);
        }
    });
    let t = tx.clone();
    ui.on_settings_audio_delay(move |v| {
        let _ = t.send(Command::SetAudioDelayMs(v as i64));
    });
    let t = tx.clone();
    ui.on_settings_equalizer(move |index| {
        let _ = t.send(Command::SetEqualizerPreset(index as u32));
    });
    let t = tx.clone();
    ui.on_settings_video(move |kind, value| {
        let _ = t.send(Command::SetVideoAdjust(kind as u32, value));
    });
    let t = tx.clone();
    ui.on_settings_aspect(move |text| {
        let _ = t.send(Command::SetAspect(text.to_string()));
    });
    let t = tx.clone();
    ui.on_settings_speed(move |value| {
        let _ = t.send(Command::SetSpeed(value));
    });
    let t = tx.clone();
    ui.on_settings_sleep(move |minutes| {
        let _ = t.send(Command::SetSleep(minutes as u32));
    });
    let t = tx.clone();
    let weak = ui.as_weak();
    ui.on_preset_save(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        let payload = PresetPayload {
            subtitle: SubtitlePreset {
                delay_ms: i64::from(ui.get_subtitle_delay_ms()),
                size: ui.get_subtitle_size() / 20.0,
                relative_size: ui.get_subtitle_relative_size(),
                outline_px: ui.get_subtitle_outline_px() as u32,
                y: ui.get_subtitle_y(),
                ..SubtitlePreset::default()
            },
            audio: AudioPreset {
                delay_ms: i64::from(ui.get_audio_delay_ms()),
                ..AudioPreset::default()
            },
            video: VideoPreset {
                brightness: ui.get_video_brightness(),
                contrast: ui.get_video_contrast(),
                saturation: ui.get_video_saturation(),
                hue: ui.get_video_hue() as i32,
                gamma: ui.get_video_gamma(),
                zoom: ui.get_video_zoom(),
                aspect_ratio: ui.get_video_aspect().to_string(),
            },
            playback: PlaybackPreset {
                speed: ui.get_playback_speed(),
                sleep_minutes: ui.get_sleep_minutes() as u32,
                ..PlaybackPreset::default()
            },
            ..PresetPayload::default()
        };
        let _ = t.send(Command::SavePreset {
            name: name.to_string(),
            payload: Box::new(payload),
        });
    });
    let t = tx.clone();
    ui.on_preset_load(move |name| {
        let _ = t.send(Command::LoadPreset(name.to_string()));
    });
    let t = tx.clone();
    ui.on_preset_delete(move |name| {
        let _ = t.send(Command::DeletePreset(name.to_string()));
    });
    let t = tx.clone();
    ui.on_preset_rename(move |name| {
        let _ = t.send(Command::RenamePreset {
            from: name.to_string(),
            to: name.to_string(),
        });
    });
    let t = tx.clone();
    ui.on_preset_auto(move |name, enabled| {
        let _ = t.send(Command::SetPresetAuto {
            name: name.to_string(),
            target: String::new(),
            enabled,
        });
    });

    let t = tx;
    mini.on_toggle_play(move || {
        let _ = t.send(Command::TogglePause);
    });
}

#[allow(clippy::too_many_arguments)]
fn worker_loop(
    instance: Instance,
    mut player: MediaPlayer,
    events: Receiver<PlayerEvent>,
    mut session: Session,
    media: Vec<String>,
    commands: Receiver<Command>,
    updates: Sender<UiUpdate>,
    mut saved_presets: Vec<presets::Preset>,
    eq_names: Vec<String>,
) {
    let mut index = 0usize;
    let mut applied_preset: Option<PresetPayload> = None;
    let mut sleep_deadline: Option<Instant> = None;
    let mut audio_tracks = Vec::new();
    let mut subtitle_tracks = Vec::new();
    if let Some(uri) = media.first()
        && let Err(e) = start_item(
            &mut session,
            &instance,
            &mut player,
            uri,
            &mut applied_preset,
            &saved_presets,
            &eq_names,
        )
    {
        eprintln!("playback start failed: {e}");
    }

    loop {
        if sleep_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            player.stop();
            sleep_deadline = None;
        }
        match commands.recv_timeout(Duration::from_millis(50)) {
            Ok(Command::TogglePause) => player.toggle_pause(),
            Ok(Command::SeekSecs(secs)) => {
                let base = player.time().unwrap_or_default();
                let step = Duration::from_secs(secs.unsigned_abs());
                player.set_time(if secs < 0 {
                    base.saturating_sub(step)
                } else {
                    base.saturating_add(step)
                });
            }
            Ok(Command::SeekPos(p)) => player.set_position(p),
            Ok(Command::SetVolume(v)) => player.set_volume((v * 200.0) as i32),
            Ok(Command::ToggleMute) => player.set_muted(!player.is_muted()),
            Ok(Command::Next) => {
                if let Some(uri) = session.queue.next_uri().map(str::to_owned) {
                    index = media.iter().position(|u| u == &uri).unwrap_or(index + 1);
                    if let Err(e) = start_item(
                        &mut session,
                        &instance,
                        &mut player,
                        &uri,
                        &mut applied_preset,
                        &saved_presets,
                        &eq_names,
                    ) {
                        eprintln!("{e}");
                    }
                }
            }
            Ok(Command::Previous) => {
                if let Some(uri) = session.queue.previous().map(str::to_owned)
                    && let Err(e) = start_item(
                        &mut session,
                        &instance,
                        &mut player,
                        &uri,
                        &mut applied_preset,
                        &saved_presets,
                        &eq_names,
                    )
                {
                    eprintln!("{e}");
                }
            }
            Ok(Command::SelectAudioTrack(id)) => {
                let _ = player.set_audio_track(id);
            }
            Ok(Command::SelectSubtitleTrack(id)) => {
                let _ = player.set_subtitle_track(id);
            }
            Ok(Command::SetSubtitleDelayMs(ms)) => {
                let _ = player.set_subtitle_delay_us(ms * 1000);
            }
            Ok(Command::LoadSubtitle(path)) => {
                if let Err(e) = player.add_subtitle_file(Path::new(&path)) {
                    eprintln!("subtitle: {e}");
                }
            }
            Ok(Command::SetAudioDelayMs(ms)) => {
                let _ = player.set_audio_delay_us(ms * 1000);
            }
            Ok(Command::SetVideoAdjust(0, v)) => {
                player.set_video_adjust(vlc_wrapper::VideoAdjust::Brightness, v)
            }
            Ok(Command::SetVideoAdjust(1, v)) => {
                player.set_video_adjust(vlc_wrapper::VideoAdjust::Contrast, v)
            }
            Ok(Command::SetVideoAdjust(2, v)) => {
                player.set_video_adjust(vlc_wrapper::VideoAdjust::Saturation, v)
            }
            Ok(Command::SetVideoAdjust(3, v)) => {
                player.set_video_adjust(vlc_wrapper::VideoAdjust::Hue, v)
            }
            Ok(Command::SetVideoAdjust(4, v)) => {
                player.set_video_adjust(vlc_wrapper::VideoAdjust::Gamma, v)
            }
            Ok(Command::SetVideoAdjust(_, _)) => {}
            Ok(Command::SetAspect(text)) => {
                player.set_aspect_ratio(if text.is_empty() { None } else { Some(&text) })
            }
            Ok(Command::SetSpeed(speed)) => {
                let _ = player.set_rate(speed);
            }
            Ok(Command::SetSleep(minutes)) => {
                sleep_deadline = (minutes > 0)
                    .then(|| Instant::now() + Duration::from_secs(u64::from(minutes) * 60));
            }
            Ok(Command::SetEqualizerPreset(i)) => {
                if let Err(e) = player.set_equalizer_preset(i) {
                    eprintln!("equalizer: {e}");
                }
            }
            Ok(Command::SavePreset { name, mut payload }) => {
                if payload.target.is_empty() {
                    payload.target = current_folder(&media, index);
                }
                if presets::save(&session.store, &name, &payload).is_ok() {
                    saved_presets = presets::load_all(&session.store).unwrap_or(saved_presets);
                }
            }
            Ok(Command::LoadPreset(name)) => {
                if let Some(preset) = saved_presets.iter().find(|p| p.name == name).cloned() {
                    apply_payload(&mut player, &preset.payload, &eq_names);
                    applied_preset = Some(preset.payload.clone());
                }
            }
            Ok(Command::DeletePreset(name)) => {
                if let Some(preset) = saved_presets.iter().find(|p| p.name == name) {
                    let _ = session.store.delete_preset(preset.id);
                    saved_presets = presets::load_all(&session.store).unwrap_or(saved_presets);
                }
            }
            Ok(Command::RenamePreset { from, to }) => {
                if let Some(preset) = saved_presets.iter().find(|p| p.name == from) {
                    let _ = session.store.rename_preset(preset.id, &to);
                    saved_presets = presets::load_all(&session.store).unwrap_or(saved_presets);
                }
            }
            Ok(Command::SetPresetAuto {
                name,
                target,
                enabled,
            }) => {
                if let Some(preset) = saved_presets.iter().find(|p| p.name == name) {
                    let mut payload = preset.payload.clone();
                    payload.target = if enabled {
                        if target.is_empty() {
                            current_folder(&media, index)
                        } else {
                            target
                        }
                    } else {
                        String::new()
                    };
                    let _ = presets::save(&session.store, &name, &payload);
                    saved_presets = presets::load_all(&session.store).unwrap_or(saved_presets);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        while let Ok(event) = events.try_recv() {
            let mapped = map_event(&event);
            if matches!(mapped, PlaybackEvent::Playing) {
                audio_tracks = to_items(player.audio_tracks(), player.audio_track());
                subtitle_tracks = to_items(player.subtitle_tracks(), player.subtitle_track());
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
                if let Err(e) = start_item(
                    &mut session,
                    &instance,
                    &mut player,
                    &next,
                    &mut applied_preset,
                    &saved_presets,
                    &eq_names,
                ) {
                    eprintln!("{e}");
                }
            }
            let state = &session.state;
            let len = state.length;
            let _ = updates.send(UiUpdate {
                title: media.get(index).cloned().unwrap_or_default(),
                elapsed: format_hms(state.time),
                total: len.map_or_else(|| "--:--:--".into(), format_hms),
                progress: progress(state.time, len),
                playing: state.status == PlaybackState::Playing,
                volume: player.volume() as f32 / 200.0,
                muted: player.is_muted(),
                audio_tracks: audio_tracks.clone(),
                subtitle_tracks: subtitle_tracks.clone(),
                time: state.time,
                preset_names: saved_presets
                    .iter()
                    .map(|p| SharedString::from(p.name.as_str()))
                    .collect(),
                equalizer_presets: eq_names
                    .iter()
                    .map(|s| SharedString::from(s.as_str()))
                    .collect(),
                applied_preset: applied_preset.take(),
            });
        }
    }
    player.stop();
}

fn current_folder(media: &[String], index: usize) -> String {
    media
        .get(index)
        .and_then(|uri| Path::new(uri).parent())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn apply_payload(player: &mut MediaPlayer, payload: &PresetPayload, eq_names: &[String]) {
    let v = &payload.video;
    player.set_video_adjust(vlc_wrapper::VideoAdjust::Brightness, v.brightness);
    player.set_video_adjust(vlc_wrapper::VideoAdjust::Contrast, v.contrast);
    player.set_video_adjust(vlc_wrapper::VideoAdjust::Saturation, v.saturation);
    player.set_video_adjust(vlc_wrapper::VideoAdjust::Hue, v.hue as f32);
    player.set_video_adjust(vlc_wrapper::VideoAdjust::Gamma, v.gamma);
    player.set_aspect_ratio(if v.aspect_ratio.is_empty() {
        None
    } else {
        Some(&v.aspect_ratio)
    });
    let _ = player.set_rate(payload.playback.speed);
    let _ = player.set_audio_delay_us(payload.audio.delay_ms * 1000);
    let _ = player.set_subtitle_delay_us(payload.subtitle.delay_ms * 1000);
    if let Some(index) = eq_names
        .iter()
        .position(|n| n == &payload.audio.equalizer_preset)
    {
        let _ = player.set_equalizer_preset(index as u32);
    }
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

fn to_items(tracks: Vec<vlc_wrapper::Track>, current: i32) -> Vec<TrackItem> {
    tracks
        .into_iter()
        .map(|t| TrackItem {
            id: t.id,
            name: if t.name.is_empty() {
                format!("Track {}", t.id)
            } else {
                t.name
            }
            .into(),
            selected: t.id == current,
        })
        .collect()
}

fn start_item(
    session: &mut Session,
    instance: &Instance,
    player: &mut MediaPlayer,
    uri: &str,
    applied: &mut Option<PresetPayload>,
    saved_presets: &[presets::Preset],
    eq_names: &[String],
) -> Result<(), Box<dyn Error>> {
    // A preset whose target folder contains this episode applies automatically.
    if let Some(preset) = presets::auto_apply_for(uri, saved_presets) {
        apply_payload(player, &preset.payload, eq_names);
        *applied = Some(preset.payload.clone());
    }
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

/// Separate headless player: never touches the main player or its frames.
fn thumbnail_worker(
    lib: Arc<LibVlc>,
    media_path: Option<String>,
    requests: Receiver<(u64, u64)>,
    results: Sender<(u64, u64, Vec<u8>)>,
) {
    let Some(path) = media_path else { return };
    let Ok(instance) = Instance::new(lib, &["--intf", "dummy", "--no-osd", "--no-audio"]) else {
        return;
    };
    let Ok(mut player) = MediaPlayer::new(&instance) else {
        return;
    };
    let Ok(frames) = VideoFrames::new(160, 90) else {
        return;
    };
    if player.set_video_frames(frames.clone()).is_err() {
        return;
    }
    let Ok(media) = Media::from_path(&instance, Path::new(&path)) else {
        return;
    };
    player.set_media(&media);
    if player.play().is_err() {
        return;
    }

    while let Ok((token, time_ms)) = requests.recv() {
        // Coalesce queued requests: only the newest token matters.
        let mut latest = (token, time_ms);
        while let Ok((t, ms)) = requests.try_recv() {
            latest = (t, ms);
        }
        let (token, time_ms) = latest;
        player.set_time(Duration::from_millis(time_ms));
        let deadline = Instant::now() + Duration::from_millis(900);
        loop {
            if let Some(frame) = frames.latest() {
                let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(160, 90);
                let (src, _) = frame.bgra_bytes().as_chunks::<4>();
                let (dst, _) = pixels.make_mut_bytes().as_chunks_mut::<4>();
                for (s, d) in src.iter().zip(dst.iter_mut()) {
                    d.copy_from_slice(&[s[2], s[1], s[0], 255]);
                }
                drop(frame);
                let _ = results.send((token, time_ms, pixels.make_mut_bytes().to_vec()));
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    player.stop();
}
