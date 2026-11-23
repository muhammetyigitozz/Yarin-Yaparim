#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod db;
mod parse;
mod platform;

use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use db::{Db, Task};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use slint::winit_030::{winit, WinitWindowAccessor, EventResult};
use std::rc::Rc;

slint::include_modules!();

const COLORS: [u32; 7] = [0xC98088, 0x8FA88A, 0x7F9BB5, 0xC2A878, 0xA08FB8, 0xC48A6A, 0x8A8B90];
const MONTHS: [&str; 12] = ["Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara"];
const DAYS: [&str; 7] = ["pazartesi", "salı", "çarşamba", "perşembe", "cuma", "cumartesi", "pazar"];

fn color(i: i32) -> Color {
    let c = COLORS[(i.rem_euclid(7)) as usize];
    Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn now() -> (NaiveDate, NaiveTime) {
    let n = Local::now().naive_local();
    (n.date(), n.time())
}

/// (etiket, durum, düzenleme metni)
fn due_info(t: &Task) -> (String, i32, String) {
    let Some(d) = t.due_date else { return (String::new(), 0, String::new()) };
    let (today, tnow) = now();
    let diff = (d - today).num_days();
    let tm = t.due_time.map(|x| format!(" {}", x.format("%H:%M"))).unwrap_or_default();
    let overdue = diff < 0 || (diff == 0 && t.due_time.map_or(false, |x| x < tnow));
    let label = match diff {
        n if n < -1 => format!("{} gün geçti", -n),
        -1 => "dün geçti".to_string(),
        0 => format!("bugün{tm}"),
        1 => format!("yarın{tm}"),
        2..=6 => format!("{}{tm}", DAYS[d.weekday().num_days_from_monday() as usize]),
        _ => {
            let y = if d.year() != today.year() { format!(" {}", d.year()) } else { String::new() };
            format!("{} {}{y}{tm}", d.day(), MONTHS[d.month0() as usize])
        }
    };
    let state = if overdue { 3 } else if diff <= 1 { 2 } else { 1 };
    let edit = format!("{}{}", d.format("%d.%m.%Y"), tm);
    (label, state, edit)
}

/// Tray panelindeki çizgi: 7 gün kala boş, son ana yaklaştıkça dolar.
fn progress(t: &Task) -> f32 {
    let Some(d) = t.due_date else { return 0.0 };
    let due = d.and_time(t.due_time.unwrap_or(NaiveTime::from_hms_opt(23, 59, 0).unwrap()));
    let hours = (due - Local::now().naive_local()).num_minutes() as f32 / 60.0;
    (1.0 - hours / (7.0 * 24.0)).clamp(0.04, 1.0)
}

struct App {
    db: Db,
    kind: Cell<i32>,
    project: Cell<i64>,
    projects: Rc<VecModel<ProjectRow>>,
    tasks: Rc<VecModel<TaskRow>>,
    done: Rc<VecModel<TaskRow>>,
    last_version: Cell<i64>,
    last_minute: Cell<u32>,
    names: RefCell<Vec<(i64, String, i32)>>,
    panel: TrayPanel,
    panel_target: Cell<i64>,
    groups: Rc<VecModel<PanelGroup>>,
}

fn sync<T: Clone + 'static>(m: &VecModel<T>, rows: Vec<T>, key: impl Fn(&T) -> i32) {
    let same = m.row_count() == rows.len() && rows.iter().enumerate().all(|(i, r)| m.row_data(i).map(|o| key(&o)) == Some(key(r)));
    if same {
        for (i, r) in rows.into_iter().enumerate() { m.set_row_data(i, r); }
    } else {
        m.set_vec(rows);
    }
}

impl App {
    fn row(&self, t: &Task) -> TaskRow {
        let (label, state, edit) = due_info(t);
        let subs = if t.parent_id.is_none() { self.db.subtasks(t.id) } else { vec![] };
        let names = self.names.borrow();
        let (pname, pcol) = names.iter().find(|p| p.0 == t.project_id).map(|p| (p.1.clone(), p.2)).unwrap_or_default();
        TaskRow {
            id: t.id as i32,
            title: t.title.clone().into(),
            done: t.done,
            high: t.high,
            due_label: label.into(),
            due_state: state,
            due_edit: edit.into(),
            sub_total: subs.len() as i32,
            sub_done: subs.iter().filter(|s| s.done).count() as i32,
            project_name: pname.into(),
            project_color: color(pcol),
            progress: progress(t),
            subtasks: ModelRc::new(VecModel::from(
                subs.iter().map(|s| SubRow { id: s.id as i32, title: s.title.clone().into(), done: s.done }).collect::<Vec<_>>(),
            )),
        }
    }

    fn reload(&self, ui: &AppWindow) {
        let (today, _) = now();
        let projs = self.db.projects();
        *self.names.borrow_mut() = projs.iter().map(|p| (p.id, p.name.clone(), p.color)).collect();
        if self.kind.get() == 1 && !projs.iter().any(|p| p.id == self.project.get()) {
            self.kind.set(0);
        }
        sync(&self.projects, projs.iter().map(|p| ProjectRow {
            id: p.id as i32, name: p.name.clone().into(), color: color(p.color), color_idx: p.color, open: p.open,
        }).collect(), |r| r.id);

        let dated = self.db.dated_open();
        let soon = dated.iter().filter(|t| (t.due_date.unwrap() - today).num_days() <= 7).count();
        ui.set_soon_count(if soon > 0 { soon.to_string().into() } else { SharedString::new() });

        if self.kind.get() == 0 {
            let late = dated.iter().filter(|t| due_info(t).1 == 3).count();
            let week = dated.len() - late;
            ui.set_view_title("Yakında".into());
            ui.set_view_summary(match (late, week) {
                (0, 0) => String::new(),
                (0, w) => format!("{w} tarihli"),
                (l, w) => format!("{l} gecikmiş · {w} yaklaşan"),
            }.into());
            sync(&self.tasks, dated.iter().map(|t| self.row(t)).collect(), |r| r.id);
            self.done.set_vec(vec![]);
            ui.set_done_count(0);
        } else {
            let pid = self.project.get();
            let p = projs.iter().find(|p| p.id == pid).unwrap();
            let all = self.db.tasks_of(pid, None);
            let (done, open): (Vec<_>, Vec<_>) = all.into_iter().partition(|t| t.done);
            let dated_n = open.iter().filter(|t| t.due_date.is_some()).count();
            ui.set_view_title(p.name.clone().into());
            ui.set_view_summary(if open.is_empty() { String::new() } else if dated_n > 0 {
                format!("{} açık · {} tarihli", open.len(), dated_n)
            } else { format!("{} açık", open.len()) }.into());
            sync(&self.tasks, open.iter().map(|t| self.row(t)).collect(), |r| r.id);
            ui.set_done_count(done.len() as i32);
            let show = ui.get_show_done();
            sync(&self.done, if show { done.iter().take(200).map(|t| self.row(t)).collect() } else { vec![] }, |r| r.id);
        }
        ui.set_view_kind(self.kind.get());
        ui.set_view_project(self.project.get() as i32);

        // Tray paneli: geciken + 7 gün içindekiler, proje sırasıyla
        let names = self.names.borrow().clone();
        let mut groups = vec![];
        let mut n = 0;
        for (pid, name, col) in names.iter() {
            let items: Vec<TaskRow> = dated.iter()
                .filter(|t| t.project_id == *pid && (t.due_date.unwrap() - today).num_days() <= 7)
                .map(|t| self.row(t)).collect();
            if !items.is_empty() {
                n += items.len();
                groups.push(PanelGroup { name: name.clone().into(), color: color(*col), items: ModelRc::new(VecModel::from(items)) });
            }
        }
        self.groups.set_vec(groups);
        self.panel.set_summary(if n > 0 { format!("{n} görev").into() } else { SharedString::new() });
        let target = self.panel_project();
        let tp = names.iter().find(|p| Some(p.0) == target);
        self.panel.set_target_id(target.unwrap_or(-1) as i32);
        self.panel.set_target_name(tp.map(|p| p.1.clone()).unwrap_or_default().into());
        self.panel.set_target_color(color(tp.map(|p| p.2).unwrap_or(6)));
    }

    fn default_project(&self) -> Option<i64> {
        let names = self.names.borrow();
        if names.iter().any(|p| p.0 == self.project.get()) { return Some(self.project.get()); }
        if names.len() == 1 { Some(names[0].0) } else { None }
    }

    /// Panelde seçili proje (silindiyse ana penceredeki son proje)
    fn panel_project(&self) -> Option<i64> {
        let t = self.panel_target.get();
        if self.names.borrow().iter().any(|p| p.0 == t) { Some(t) } else { self.default_project() }
    }

    fn quick_add(&self, text: &str) -> Result<(), String> {
        self.quick_add_to(text, self.default_project())
    }

    fn quick_add_to(&self, text: &str, default: Option<i64>) -> Result<(), String> {
        let (today, tnow) = now();
        let p = parse::parse(text, today, tnow);
        let pid = match &p.project {
            Some(name) => self.db.project_by_name(name),
            None => default,
        };
        if p.title.trim().is_empty() { return Err("Görev metni boş kaldı".into()); }
        match pid {
            Some(pid) => { self.db.add_task(pid, None, p.title.trim(), p.date, p.time, p.high); Ok(()) }
            None => Err(match &p.project {
                Some(n) => format!("“{n}” adında proje yok"),
                None => "Hangi projeye? Sonuna #proje yaz".to_string(),
            }),
        }
    }
}

/// Exe'nin yanındaki yarinyaparim.log dosyasına yazar.
pub fn log(msg: &str) {
    use std::io::Write;
    let path = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("yarinyaparim.log")));
    if let Some(p) = path {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{} {}", Local::now().format("%Y-%m-%d %H:%M:%S"), msg);
        }
    }
}

fn data_path() -> std::path::PathBuf {
    let base = std::env::var_os("APPDATA").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("."));
    let dir = base.join("YarinYaparim");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("veri.db")
}

fn show_main(ui: &AppWindow) {
    let _ = ui.show();
    ui.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
}

fn show_panel(ui: &AppWindow, panel: &TrayPanel) {
    let sf = ui.window().scale_factor();
    if let Some((_l, _t, r, b)) = platform::work_area() {
        let (pw, ph, m) = ((360.0 * sf) as i32, (480.0 * sf) as i32, (12.0 * sf) as i32);
        panel.window().set_position(slint::PhysicalPosition::new(r - pw - m, b - ph - m));
    }
    let _ = panel.show();
    panel.window().with_winit_window(|w| {
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowExtWindows;
            w.set_skip_taskbar(true);
        }
        w.focus_window();
    });
    panel.invoke_focus_quick();
}

fn main() -> Result<(), slint::PlatformError> {
    let hidden = std::env::args().any(|a| a == "--hidden");
    std::panic::set_hook(Box::new(|info| log(&format!("PANİK: {info}"))));
    log(&format!("başladı (gizli: {hidden})"));
    let Some(instance) = platform::single_instance() else { return Ok(()) };

    let ui = AppWindow::new()?;
    let panel = TrayPanel::new()?;
    let db = Db::open(&data_path()).expect("veritabanı açılamadı");

    // Windows ile başlat: ilk çalıştırmada açık; her açılışta exe yolunu tazele
    if db.setting("autostart").is_none() { db.set_setting("autostart", "1"); }
    let autostart = db.setting("autostart").as_deref() == Some("1");
    platform::set_autostart(autostart);

    let panel_target: i64 = db.setting("panel_project").and_then(|v| v.parse().ok()).unwrap_or(-1);
    let groups = Rc::new(VecModel::default());
    panel.set_groups(ModelRc::from(groups.clone()));
    let app = Rc::new(App {
        last_version: Cell::new(db.data_version()),
        db,
        kind: Cell::new(0),
        project: Cell::new(-1),
        projects: Rc::new(VecModel::default()),
        tasks: Rc::new(VecModel::default()),
        done: Rc::new(VecModel::default()),
        last_minute: Cell::new(Local::now().minute()),
        names: RefCell::new(vec![]),
        panel_target: Cell::new(panel_target),
        panel: panel.clone_strong(),
        groups,
    });
    if let Some(p) = app.db.projects().first() { app.kind.set(1); app.project.set(p.id); }
    ui.set_projects(ModelRc::from(app.projects.clone()));
    panel.set_projects(ModelRc::from(app.projects.clone()));
    ui.set_tasks(ModelRc::from(app.tasks.clone()));
    ui.set_done_tasks(ModelRc::from(app.done.clone()));
    app.reload(&ui);

    macro_rules! cb {
        ($method:ident, |$a:ident, $u:ident $(, $arg:ident : $t:ty)*| $body:block) => {{
            let w = ui.as_weak();
            let app = app.clone();
            ui.$method(move |$($arg: $t),*| {
                let $u = w.unwrap();
                let $a = &app;
                let r = $body;
                $a.reload(&$u);
                r
            });
        }};
    }

    cb!(on_select_view, |a, _u, kind: i32, pid: i32| {
        a.kind.set(kind);
        if kind == 1 { a.project.set(pid as i64); }
    });
    cb!(on_add_task, |a, u, text: SharedString| {
        match a.quick_add(&text) {
            Ok(()) => true,
            Err(m) => { u.set_quick_msg(m.into()); false }
        }
    });
    cb!(on_add_sub, |a, _u, id: i32, text: SharedString| {
        if let Some(t) = a.db.task(id as i64) {
            a.db.add_task(t.project_id, Some(t.id), text.trim(), None, None, false);
        }
    });
    cb!(on_toggle_task, |a, _u, id: i32| {
        if let Some(t) = a.db.task(id as i64) { a.db.set_done(t.id, !t.done); }
    });
    cb!(on_delete_task, |a, _u, id: i32| { a.db.delete_task(id as i64); });
    cb!(on_rename_task, |a, _u, id: i32, text: SharedString| {
        if !text.trim().is_empty() { a.db.rename(id as i64, text.trim()); }
    });
    cb!(on_set_due, |a, _u, id: i32, text: SharedString| {
        let (today, tnow) = now();
        match parse::parse_due_only(&text, today, tnow) {
            Some((d, t)) => { a.db.set_due(id as i64, d, t); true }
            None => false,
        }
    });
    cb!(on_toggle_high, |a, _u, id: i32| {
        if let Some(t) = a.db.task(id as i64) { a.db.set_high(t.id, !t.high); }
    });
    cb!(on_move_task, |a, _u, id: i32, n: i32| { a.db.move_task(id as i64, n as i64); });
    cb!(on_move_project, |a, _u, id: i32, n: i32| { a.db.move_project(id as i64, n as i64); });
    cb!(on_save_project, |a, _u, id: i32, name: SharedString, c: i32| {
        let name = name.trim();
        if !name.is_empty() {
            if id < 0 {
                let nid = a.db.add_project(name, c);
                a.kind.set(1);
                a.project.set(nid);
            } else {
                a.db.update_project(id as i64, name, c);
            }
        }
    });
    cb!(on_delete_project, |a, _u, id: i32| { a.db.delete_project(id as i64); });
    cb!(on_show_done_changed, |_a, _u| {});

    // Tray paneli geri çağrıları
    let last_hide = Rc::new(Cell::new(std::time::Instant::now() - std::time::Duration::from_secs(5)));
    {
        let (w, a) = (ui.as_weak(), app.clone());
        panel.on_toggle_task(move |id| {
            if let Some(t) = a.db.task(id as i64) { a.db.set_done(t.id, !t.done); }
            a.reload(&w.unwrap());
        });
        let (w, a) = (ui.as_weak(), app.clone());
        panel.on_add_task(move |text| {
            let r = a.quick_add_to(&text, a.panel_project());
            if let Err(m) = &r { a.panel.set_quick_msg(m.clone().into()); }
            a.reload(&w.unwrap());
            r.is_ok()
        });
        let (w, a) = (ui.as_weak(), app.clone());
        panel.on_pick_project(move |id| {
            a.panel_target.set(id as i64);
            a.db.set_setting("panel_project", &id.to_string());
            a.reload(&w.unwrap());
        });
        let (w, a) = (ui.as_weak(), app.clone());
        panel.on_open_task(move |id| {
            let u = w.unwrap();
            if let Some(t) = a.db.task(id as i64) {
                a.kind.set(1);
                a.project.set(t.project_id);
                u.set_expanded_id(id);
            }
            a.reload(&u);
            let _ = a.panel.hide();
            show_main(&u);
        });
        let (w, a) = (ui.as_weak(), app.clone());
        panel.on_open_main(move || {
            let _ = a.panel.hide();
            show_main(&w.unwrap());
        });
        // Odak kaybedince panel kapanır
        let lh = last_hide.clone();
        panel.window().on_winit_window_event(move |win, ev| {
            if let winit::event::WindowEvent::Focused(false) = ev {
                if win.is_visible() && std::env::var_os("YY_PANEL").is_none() {
                    let _ = win.hide();
                    lh.set(std::time::Instant::now());
                }
            }
            EventResult::Propagate
        });
    }

    // Tray olay döngüsü başladıktan sonra kurulur
    let shell: Rc<RefCell<Option<platform::Shell>>> = Rc::new(RefCell::new(None));
    {
        let (shell, p) = (shell.clone(), panel.as_weak());
        slint::Timer::single_shot(std::time::Duration::from_millis(0), move || {
            match platform::shell(autostart) {
                Ok(s) => {
                    log(&format!("tray hazır, kısayol: {}", if s.hotkey_label.is_empty() { "yok" } else { &s.hotkey_label }));
                    if let Some(p) = p.upgrade() { p.set_hotkey_label(s.hotkey_label.clone().into()); }
                    *shell.borrow_mut() = Some(s);
                }
                Err(e) => log(&format!("tray kurulamadı: {e}")),
            }
        });
    }
    {
        let shell = shell.clone();
        ui.window().on_close_requested(move || {
            let has_tray = shell.borrow().is_some();
            log(&format!("kapat'a basıldı, tray var: {has_tray}"));
            if !has_tray { slint::quit_event_loop().ok(); }
            slint::CloseRequestResponse::HideWindow
        });
    }

    // Olay yoklaması: tray, kısayol, ikinci örnek, dış DB değişikliği, dakika
    let timer = slint::Timer::default();
    {
        let w = ui.as_weak();
        let app = app.clone();
        let tick = Cell::new(0u32);
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(80), move || {
            let Some(u) = w.upgrade() else { return };
            let panel = &app.panel;
            if instance.show_requested() { show_main(&u); }
            let evs = shell.borrow().as_ref().map(|s| s.poll()).unwrap_or_default();
            {
                for ev in evs {
                    match ev {
                        platform::Ev::PanelToggle => {
                            if panel.window().is_visible() { let _ = panel.hide(); }
                            else if last_hide.get().elapsed().as_millis() > 400 { app.reload(&u); show_panel(&u, panel); }
                        }
                        platform::Ev::Hotkey => {
                            if panel.window().is_visible() { let _ = panel.hide(); } else { app.reload(&u); show_panel(&u, panel); }
                        }
                        platform::Ev::OpenMain => { let _ = panel.hide(); show_main(&u); }
                        platform::Ev::AutostartToggled(on) => {
                            app.db.set_setting("autostart", if on { "1" } else { "0" });
                            platform::set_autostart(on);
                        }
                        platform::Ev::Quit => { log("tray'den çıkıldı"); slint::quit_event_loop().ok(); }
                    }
                }
            }
            let t = tick.get().wrapping_add(1);
            tick.set(t);
            if t % 12 == 0 {
                let v = app.db.data_version();
                let m = Local::now().minute();
                if v != app.last_version.get() || m != app.last_minute.get() {
                    app.last_version.set(v);
                    app.last_minute.set(m);
                    app.reload(&u);
                }
            }
        });
    }

    if !hidden { let _ = ui.show(); }
    if std::env::var_os("YY_PANEL").is_some() { show_panel(&ui, &panel); }
    let r = slint::run_event_loop_until_quit();
    log(&format!("olay döngüsü bitti: {:?}", r.as_ref().err()));
    r
}
