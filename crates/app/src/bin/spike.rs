#![deny(unsafe_code)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use player_core::{state::PlaybackState, telemetry::summarize, time::format_hms};
use serde::Serialize;
use slint::{ComponentHandle, RenderingState};
use vlc_wrapper::{Instance, LibVlc, Media, MediaPlayer, MediaStats, PlayerEvent, VideoFrames};

#[allow(unsafe_code)]
mod generated_ui {
    slint::include_modules!();
}
use generated_ui::SpikeWindow;

struct Options {
    media: PathBuf,
    width: u32,
    height: u32,
    bench: Option<Duration>,
    report: Option<PathBuf>,
    runtime: Option<PathBuf>,
    rtl: bool,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args_os().skip(1);
        let media = args.next().ok_or("Usage: spike MEDIA --size 1920x1080 [--bench 20] [--report file.json] [--rtl] [--runtime DIRECTORY]")?;
        let mut options = Self {
            media: media.into(),
            width: 1920,
            height: 1080,
            bench: None,
            report: None,
            runtime: None,
            rtl: false,
        };
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--size") => {
                    let size = args.next().ok_or("--size requires WIDTHxHEIGHT")?;
                    let (w, h) = size
                        .to_str()
                        .and_then(|v| v.split_once('x'))
                        .ok_or("invalid --size")?;
                    options.width = w.parse().map_err(|_| "invalid width")?;
                    options.height = h.parse().map_err(|_| "invalid height")?;
                }
                Some("--bench") => {
                    let seconds: u64 = args
                        .next()
                        .and_then(|v| v.to_str().and_then(|s| s.parse().ok()))
                        .ok_or("invalid --bench seconds")?;
                    if !(1..=3600).contains(&seconds) {
                        return Err("--bench must be 1..=3600".into());
                    }
                    options.bench = Some(Duration::from_secs(seconds));
                }
                Some("--report") => {
                    options.report = Some(args.next().ok_or("missing report path")?.into())
                }
                Some("--runtime") => {
                    options.runtime = Some(args.next().ok_or("missing runtime path")?.into())
                }
                Some("--rtl") => options.rtl = true,
                _ => return Err(format!("unknown option: {}", arg.to_string_lossy())),
            }
        }
        if !options.media.is_file() {
            return Err(format!("media not found: {}", options.media.display()));
        }
        Ok(options)
    }
}

enum Command {
    Toggle,
    Stop,
    Seek(f32),
    Relative(i32),
    Volume(f32),
    Mute,
    Shutdown,
}

#[derive(Default)]
struct Snapshot {
    time: Duration,
    length: Duration,
    playing: bool,
    muted: bool,
    status: PlaybackState,
    stats: MediaStats,
}

enum Update {
    Snapshot(Snapshot),
    Error(String),
}

fn worker(
    options: &Options,
    frames: VideoFrames,
    commands: Receiver<Command>,
    updates: mpsc::SyncSender<Update>,
) -> Result<(), String> {
    let lib = match &options.runtime {
        Some(dir) => LibVlc::load_from(dir),
        None => LibVlc::discover(),
    }
    .map_err(|e| e.to_string())?;
    println!(
        "libVLC: {} | runtime: {}",
        lib.version(),
        lib.dir().display()
    );
    println!("startup: creating playback instance");
    let instance = Instance::new(
        lib,
        &[
            "--no-video-title-show",
            "--no-osd",
            "--no-sub-autodetect-file",
            "--avcodec-hw=none",
            "--quiet",
        ],
    )
    .map_err(|e| e.to_string())?;
    println!("startup: playback instance ready");
    let media = Media::from_path(&instance, &options.media).map_err(|e| e.to_string())?;
    let mut player = MediaPlayer::new(&instance).map_err(|e| e.to_string())?;
    player.set_video_frames(frames).map_err(|e| e.to_string())?;
    player.set_media(&media);
    let events = player.take_events().expect("first receiver");
    player.play().map_err(|e| e.to_string())?;
    let mut status = PlaybackState::Opening;
    let mut last_update = Instant::now() - Duration::from_secs(1);
    loop {
        match commands.recv_timeout(Duration::from_millis(15)) {
            Ok(Command::Toggle) => {
                if status.needs_restart() {
                    player.play().map_err(|e| e.to_string())?;
                } else {
                    player.toggle_pause();
                }
            }
            Ok(Command::Stop) => player.stop(),
            Ok(Command::Seek(pos)) => player.set_position(pos),
            Ok(Command::Relative(seconds)) => {
                let current = player.time().unwrap_or_default().as_secs_f64();
                player.set_time(Duration::from_secs_f64(
                    (current + f64::from(seconds)).max(0.0),
                ));
            }
            Ok(Command::Volume(v)) => player.set_volume((v.clamp(0.0, 1.0) * 100.0).round() as i32),
            Ok(Command::Mute) => player.set_muted(!player.is_muted()),
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        for event in events.try_iter() {
            status = match event {
                PlayerEvent::Opening => PlaybackState::Opening,
                PlayerEvent::Playing => PlaybackState::Playing,
                PlayerEvent::Paused => PlaybackState::Paused,
                PlayerEvent::Stopped => PlaybackState::Stopped,
                PlayerEvent::EndReached => PlaybackState::Ended,
                PlayerEvent::EncounteredError => {
                    return Err("libVLC could not decode this media".into());
                }
                _ => continue,
            };
            println!("event={status:?}");
        }
        if last_update.elapsed() >= Duration::from_millis(200) {
            let snapshot = Snapshot {
                time: player.time().unwrap_or_default(),
                length: player.length().unwrap_or_default(),
                playing: player.is_playing(),
                muted: player.is_muted(),
                status,
                stats: media.stats().unwrap_or_default(),
            };
            let _ = updates.try_send(Update::Snapshot(snapshot));
            last_update = Instant::now();
        }
    }
    player.stop();
    Ok(())
}

#[derive(Default)]
struct Measurement {
    measuring: bool,
    started: Option<Instant>,
    base_delivered: u64,
    base_overwritten: u64,
    base_starved: u64,
    base_stats: MediaStats,
    stats: MediaStats,
    supplied_sequence: u64,
    rendered_sequence: u64,
    supplied: u64,
    rendered: u64,
    copy_ms: Vec<f64>,
    render_gap_ms: Vec<f64>,
    last_render: Option<Instant>,
}

#[derive(Serialize)]
struct Report {
    media: String,
    width: u32,
    height: u32,
    chroma: &'static str,
    decoder: &'static str,
    renderer: &'static str,
    warmup_seconds: u32,
    measured_seconds: f64,
    callbacks: u64,
    ui_supplied: u64,
    rendered_unique: u64,
    callback_fps: f64,
    rendered_fps: f64,
    overwritten_unread: u64,
    starved: u64,
    supplied_not_rendered: u64,
    libvlc_decoded: u64,
    libvlc_displayed: u64,
    libvlc_lost: u64,
    copy_swizzle_ms: Option<player_core::telemetry::Summary>,
    render_gap_ms: Option<player_core::telemetry::Summary>,
}

fn command_handler(tx: &Sender<Command>, command: Command) {
    let _ = tx.send(command);
}

fn run(options: Options) -> Result<(), String> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("femtovg".into())
        .select()
        .map_err(|e| e.to_string())?;
    let window = SpikeWindow::new().map_err(|e| e.to_string())?;
    window.set_rtl(options.rtl);
    window.set_media_title(
        options
            .media
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref()
            .into(),
    );
    println!("startup: showing renderer before playback initialization");
    window.show().map_err(|e| e.to_string())?;
    println!("startup: renderer ready");
    let frames = VideoFrames::new(options.width, options.height).map_err(|e| e.to_string())?;
    let (tx, commands) = mpsc::channel();
    let (updates_tx, updates) = mpsc::sync_channel(8);
    let worker_frames = frames.clone();
    let worker_options = Options {
        media: options.media.clone(),
        width: options.width,
        height: options.height,
        bench: None,
        report: None,
        runtime: options.runtime.clone(),
        rtl: options.rtl,
    };
    let thread = std::thread::spawn(move || {
        let result = worker(&worker_options, worker_frames, commands, updates_tx.clone());
        if let Err(error) = &result {
            let _ = updates_tx.try_send(Update::Error(error.clone()));
        }
        result
    });
    let send = tx.clone();
    window.on_toggle_play(move || command_handler(&send, Command::Toggle));
    let send = tx.clone();
    window.on_stop_play(move || command_handler(&send, Command::Stop));
    let send = tx.clone();
    window.on_seek(move |pos| command_handler(&send, Command::Seek(pos)));
    let send = tx.clone();
    window.on_relative_seek(move |seconds| command_handler(&send, Command::Relative(seconds)));
    let send = tx.clone();
    window.on_toggle_mute(move || command_handler(&send, Command::Mute));
    let send = tx.clone();
    let weak = window.as_weak();
    window.on_change_volume(move |v| {
        if let Some(ui) = weak.upgrade() {
            ui.set_volume(v.clamp(0.0, 1.0));
        }
        command_handler(&send, Command::Volume(v));
    });
    let weak = window.as_weak();
    window.on_toggle_fullscreen(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().set_fullscreen(!ui.window().is_fullscreen());
        }
    });

    let measurement = Rc::new(RefCell::new(Measurement::default()));
    let render_measurement = measurement.clone();
    println!("startup: installing rendering notifier");
    window
        .window()
        .set_rendering_notifier(move |state, _| {
            if matches!(state, RenderingState::AfterRendering) {
                let mut m = render_measurement.borrow_mut();
                if m.supplied_sequence != m.rendered_sequence {
                    m.rendered_sequence = m.supplied_sequence;
                    if m.measuring {
                        m.rendered += 1;
                        let now = Instant::now();
                        if let Some(previous) = m.last_render {
                            m.render_gap_ms
                                .push(now.duration_since(previous).as_secs_f64() * 1000.0);
                        }
                        m.last_render = Some(now);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let launched = Instant::now();
    let last_log = Rc::new(RefCell::new(Instant::now()));
    let result_report = Rc::new(RefCell::new(None));
    let report_result = result_report.clone();
    let weak = window.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(4),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            for update in updates.try_iter() {
                match update {
                    Update::Snapshot(s) => {
                        ui.set_elapsed(format_hms(s.time).into());
                        ui.set_total(format_hms(s.length).into());
                        ui.set_progress(player_core::time::progress(s.time, Some(s.length)));
                        ui.set_playing(s.playing);
                        ui.set_muted(s.muted);
                        let status = match (s.status, ui.get_rtl()) {
                            (PlaybackState::Playing, true) => "قيد التشغيل",
                            (PlaybackState::Paused, true) => "متوقف مؤقتًا",
                            (PlaybackState::Stopped, true) => "متوقف",
                            (PlaybackState::Ended, true) => "انتهى",
                            (PlaybackState::Opening, true) => "جارٍ الفتح",
                            (PlaybackState::Playing, false) => "Playing",
                            (PlaybackState::Paused, false) => "Paused",
                            (PlaybackState::Stopped, false) => "Stopped",
                            (PlaybackState::Ended, false) => "Ended",
                            (PlaybackState::Opening, false) => "Opening",
                        };
                        ui.set_status(status.into());
                        measurement.borrow_mut().stats = s.stats;
                    }
                    Update::Error(error) => {
                        ui.set_status(
                            if ui.get_rtl() {
                                "خطأ في التشغيل — راجع سجل التشخيص"
                            } else {
                                "Playback error — see diagnostic log"
                            }
                            .into(),
                        );
                        eprintln!("{error}");
                    }
                }
            }
            let mut m = measurement.borrow_mut();
            if !m.measuring && launched.elapsed() >= Duration::from_secs(3) {
                let counters = frames.counters();
                m.measuring = true;
                m.started = Some(Instant::now());
                m.base_delivered = counters.delivered;
                m.base_overwritten = counters.overwritten;
                m.base_starved = counters.starved;
                m.base_stats = m.stats;
            }
            if let Some(frame) = frames.latest() {
                let copy_start = Instant::now();
                let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(
                    frames.width(),
                    frames.height(),
                );
                let (src_chunks, _) = frame.bgra_bytes().as_chunks::<4>();
                let (dst_chunks, _) = pixels.make_mut_bytes().as_chunks_mut::<4>();
                for (src, dst) in src_chunks.iter().zip(dst_chunks.iter_mut()) {
                    dst.copy_from_slice(&[src[2], src[1], src[0], 255]);
                }
                m.supplied_sequence = frame.sequence();
                drop(frame);
                ui.set_video_frame(slint::Image::from_rgba8(pixels));
                if m.measuring {
                    m.supplied += 1;
                    m.copy_ms.push(copy_start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            if last_log.borrow().elapsed() >= Duration::from_secs(1) {
                let c = frames.counters();
                let elapsed = m.started.map_or(0.0, |t| t.elapsed().as_secs_f64());
                let fps = if elapsed > 0.0 {
                    m.rendered as f64 / elapsed
                } else {
                    0.0
                };
                let hud = format!(
                    "{} × {} · RV32 → RGBA · {fps:.1} rendered fps · {} unread drops",
                    frames.width(),
                    frames.height(),
                    c.overwritten
                );
                ui.set_telemetry(hud.as_str().into());
                println!(
                    "t={:.2} callbacks={} rendered={} overwritten={} decoded={} lost={}",
                    launched.elapsed().as_secs_f64(),
                    c.delivered,
                    m.rendered,
                    c.overwritten,
                    m.stats.decoded_video,
                    m.stats.lost_pictures
                );
                *last_log.borrow_mut() = Instant::now();
            }
            if let (Some(duration), Some(start)) = (options.bench, m.started)
                && start.elapsed() >= duration
            {
                let c = frames.counters();
                let elapsed = start.elapsed().as_secs_f64();
                let callbacks = c.delivered.saturating_sub(m.base_delivered);
                *report_result.borrow_mut() = Some(Report {
                    media: options.media.display().to_string(),
                    width: frames.width(),
                    height: frames.height(),
                    chroma: "RV32/BGRX -> RGBA",
                    decoder: "software (avcodec-hw=none)",
                    renderer: "winit-femtovg/OpenGL",
                    warmup_seconds: 3,
                    measured_seconds: elapsed,
                    callbacks,
                    ui_supplied: m.supplied,
                    rendered_unique: m.rendered,
                    callback_fps: callbacks as f64 / elapsed,
                    rendered_fps: m.rendered as f64 / elapsed,
                    overwritten_unread: c.overwritten.saturating_sub(m.base_overwritten),
                    starved: c.starved.saturating_sub(m.base_starved),
                    supplied_not_rendered: m.supplied.saturating_sub(m.rendered),
                    libvlc_decoded: m
                        .stats
                        .decoded_video
                        .saturating_sub(m.base_stats.decoded_video),
                    libvlc_displayed: m
                        .stats
                        .displayed_pictures
                        .saturating_sub(m.base_stats.displayed_pictures),
                    libvlc_lost: m
                        .stats
                        .lost_pictures
                        .saturating_sub(m.base_stats.lost_pictures),
                    copy_swizzle_ms: summarize(&m.copy_ms),
                    render_gap_ms: summarize(&m.render_gap_ms),
                });
                let _ = slint::quit_event_loop();
            }
        },
    );
    println!("startup: entering UI event loop");
    let ui_result = window.run().map_err(|e| e.to_string());
    timer.stop();
    let _ = tx.send(Command::Shutdown);
    let worker_result = thread.join().map_err(|_| "playback worker panicked")?;
    if let Some(report) = result_report.borrow().as_ref() {
        let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
        println!("{json}");
        if let Some(path) = options.report {
            std::fs::write(path, json).map_err(|e| e.to_string())?;
        }
    }
    ui_result?;
    worker_result
}

fn main() {
    if let Err(error) = Options::parse().and_then(run) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
