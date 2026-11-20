//! Phase 4 library + queue window.

slint::include_modules!();

#[path = "../i18n.rs"]
mod i18n;

use std::cell::RefCell;
use std::error::Error;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use i18n::{Catalog, Locale};
use player_core::brand;
use player_core::error::CoreError;
use player_core::model::MediaItem;
use player_core::playlist::{SortMode, filter_items, sort_items};
use player_core::scanner::{ScanHandle, scan_folder};
use player_core::session::Session;
use player_core::settings::SettingsStore;
use player_core::store::Store;
use slint::{ModelRc, SharedString, VecModel};

type SharedSession = Rc<RefCell<Session>>;

fn main() -> Result<(), Box<dyn Error>> {
    let cli: Vec<String> = std::env::args().skip(1).collect();
    let folder = cli.first().cloned();
    let locale = Locale::parse(std::env::var("PLAYER_LOCALE").as_deref().unwrap_or("en"));

    let config = brand::config_dir();
    let db_path = config.join("library.sqlite3");
    let store = Store::open(&db_path)?;
    let settings = SettingsStore::open(config.join("settings.toml"))?;
    let session = Rc::new(RefCell::new(Session::new(store, settings)?));

    let ui = LibraryWindow::new()?;
    let catalog = Catalog::load(locale);
    i18n::apply(&ui.global::<Strings>(), &catalog);
    let theme = ui.global::<AppTheme>();
    theme.set_locale(catalog.locale().code().into());
    theme.set_rtl(catalog.locale().rtl());
    theme.set_font_family(
        if catalog.locale().rtl() {
            "Noto Sans Arabic"
        } else {
            "Inter"
        }
        .into(),
    );

    let (scan_tx, scan_rx) = mpsc::channel::<String>();
    let cancel = ScanHandle::default();
    refresh(&ui, &session, "", SortMode::Name)?;

    if let Some(path) = folder {
        let db_path = db_path.clone();
        let path = std::path::PathBuf::from(path);
        let cancel = cancel.clone();
        let tx = scan_tx.clone();
        std::thread::spawn(move || {
            let Ok(store) = Store::open(&db_path) else {
                let _ = tx.send("database unavailable".into());
                return;
            };
            match scan_folder(&store, &path, true, &cancel, |n| {
                let _ = tx.send(format!("scanned {n} entries"));
            }) {
                Ok(stats) => {
                    let _ = tx.send(format!("done: +{} files", stats.added));
                }
                Err(e) => {
                    let _ = tx.send(e.to_string());
                }
            }
        });
    }

    wire_callbacks(&ui, &session, &scan_tx, &cancel, &catalog);

    let ui_weak = ui.as_weak();
    let scan_timer = slint::Timer::default();
    scan_timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(120),
        move || {
            let Some(ui) = ui_weak.upgrade() else { return };
            while let Ok(status) = scan_rx.try_recv() {
                ui.set_status(status.into());
            }
        },
    );

    ui.run()?;
    cancel.cancel();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn wire_callbacks(
    ui: &LibraryWindow,
    session: &SharedSession,
    scan_tx: &mpsc::Sender<String>,
    cancel: &ScanHandle,
    catalog: &Catalog,
) {
    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_select_section(move |section| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_section(section);
            let _ = refresh(&ui, &s, &ui.get_search_text(), SortMode::Name);
        }
    });
    let ui_weak = ui.as_weak();
    ui.on_set_view(move |mode| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_view_mode(mode);
        }
    });
    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_set_sort(move |_| {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = refresh(&ui, &s, &ui.get_search_text(), SortMode::Date);
        }
    });
    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_search_changed(move |query| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_search_text(query.clone());
            let _ = refresh(&ui, &s, &query, SortMode::Name);
        }
    });
    let tx = scan_tx.clone();
    let c = cancel.clone();
    let ui_weak = ui.as_weak();
    ui.on_scan_folder(move || {
        let _ = tx.send("pass a folder path as the first argument to scan".into());
        c.cancel();
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_status("scan cancelled".into());
        }
    });

    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_add_to_queue(move |id| {
        let Some(ui) = ui_weak.upgrade() else { return };
        let borrowed = s.borrow();
        if let Ok(Some(item)) = borrowed.store.media_by_id(i64::from(id)) {
            drop(borrowed);
            if let Ok(()) = s.borrow_mut().append(item.uri) {
                let _ = refresh(&ui, &s, &ui.get_search_text(), SortMode::Name);
            }
        }
    });
    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_remove_from_queue(move |index| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if s.borrow_mut().remove_at(index.max(0) as usize).is_ok() {
            let _ = refresh(&ui, &s, &ui.get_search_text(), SortMode::Name);
        }
    });
    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_move_queue(move |from, to| {
        let Some(ui) = ui_weak.upgrade() else { return };
        if s.borrow_mut()
            .move_item(from.max(0) as usize, to.max(0) as usize)
            .is_ok()
        {
            let _ = refresh(&ui, &s, &ui.get_search_text(), SortMode::Name);
        }
    });
    let s = session.clone();
    let ui_weak = ui.as_weak();
    ui.on_clear_queue(move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        if s.borrow_mut().clear_queue().is_ok() {
            let _ = refresh(&ui, &s, &ui.get_search_text(), SortMode::Name);
        }
    });
    let s = session.clone();
    ui.on_open_media(move |id| {
        let Ok(Some(item)) = s.borrow().store.media_by_id(i64::from(id)) else {
            return;
        };
        let _ = std::process::Command::new(brand::EXE_NAME)
            .arg(item.uri)
            .spawn();
    });
    let ui_weak = ui.as_weak();
    ui.on_toggle_play(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_playing(!ui.get_playing());
        }
    });
    let text = catalog.get("library-queue").to_owned();
    ui.on_close_library(move || {
        let _ = text;
        let _ = slint::quit_event_loop();
    });
}

fn refresh(
    ui: &LibraryWindow,
    session: &SharedSession,
    query: &str,
    sort: SortMode,
) -> Result<(), CoreError> {
    let borrowed = session.borrow();
    let section = ui.get_section();
    let mut items: Vec<MediaItem> = borrowed.store.list_media(10_000, 0)?;
    if section.contains("Music") || section.contains("الموسيقى") {
        items.retain(|m| m.kind == player_core::model::MediaKind::Audio);
    } else if section.contains("Videos") || section.contains("الفيديو") {
        items.retain(|m| m.kind == player_core::model::MediaKind::Video);
    } else if section.contains("Recent") || section.contains("الأخيرة") {
        items = borrowed.store.recent(200)?;
    }
    items = filter_items(&items, query);
    sort_items(&mut items, sort, |id| {
        borrowed
            .store
            .progress(id)
            .is_ok_and(|p| p.is_some_and(|p| p.completed))
    });
    let cards: Vec<MediaCard> = items
        .iter()
        .map(|m| MediaCard {
            id: i32::try_from(m.id).unwrap_or(i32::MAX),
            title: m.title.clone().into(),
            subtitle: format!("{} · {}s", m.kind.as_str(), m.duration_ms / 1000).into(),
            kind: m.kind.as_str().into(),
            watched: borrowed
                .store
                .progress(m.id)
                .ok()
                .flatten()
                .is_some_and(|p| p.completed),
        })
        .collect();
    ui.set_items(ModelRc::new(VecModel::from(cards)));
    let uris: Vec<SharedString> = borrowed
        .queue
        .items()
        .iter()
        .map(|u| SharedString::from(u.as_str()))
        .collect();
    ui.set_queue(ModelRc::new(VecModel::from(uris)));
    ui.set_status(format!("{} items", items.len()).into());
    Ok(())
}
