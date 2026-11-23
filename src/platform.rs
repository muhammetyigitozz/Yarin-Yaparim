// Windows'a özgü parçalar: tek örnek, otomatik başlatma, tray, kısayol, çalışma alanı.
// Diğer platformlarda (geliştirme/test) boş uygulamalar.

pub enum Ev {
    PanelToggle,
    OpenMain,
    Hotkey,
    AutostartToggled(bool),
    Quit,
}

#[cfg(windows)]
mod imp {
    use super::Ev;
    use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, RECT, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};

    fn w(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

    const EVENT_NAME: &str = "Local\\YarinYaparim.Show";

    /// Tek örnek: zaten çalışıyorsa onu öne getirtip None döner.
    pub struct Instance { ev: HANDLE, _mutex: HANDLE }

    pub fn single_instance(_cmd: &str) -> Option<Instance> {
        unsafe {
            let m = CreateMutexW(std::ptr::null(), 0, w("Local\\YarinYaparim.Mutex").as_ptr());
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let e = OpenEventW(EVENT_MODIFY_STATE, 0, w(EVENT_NAME).as_ptr());
                if !e.is_null() { SetEvent(e); }
                return None;
            }
            let ev = CreateEventW(std::ptr::null(), 0, 0, w(EVENT_NAME).as_ptr());
            Some(Instance { ev, _mutex: m })
        }
    }

    impl Instance {
        pub fn poll(&self) -> Vec<Ev> {
            if !self.ev.is_null() && unsafe { WaitForSingleObject(self.ev, 0) } == WAIT_OBJECT_0 { vec![Ev::OpenMain] } else { vec![] }
        }
    }

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_NAME: &str = "YarinYaparim";

    pub fn set_autostart(on: bool) {
        use winreg::{enums::*, RegKey};
        let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE) else { return };
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let _ = k.set_value(RUN_NAME, &format!("\"{}\" --hidden", exe.display()));
            }
        } else {
            let _ = k.delete_value(RUN_NAME);
        }
    }

    /// (sol, üst, sağ, alt) fiziksel piksel
    pub fn work_area() -> Option<(i32, i32, i32, i32)> {
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut _, 0) };
        (ok != 0).then_some((r.left, r.top, r.right, r.bottom))
    }

    pub struct Shell {
        _tray: TrayIcon,
        _hk: Option<GlobalHotKeyManager>,
        open_id: tray_icon::menu::MenuId,
        auto: CheckMenuItem,
        quit_id: tray_icon::menu::MenuId,
        pub hotkey_label: String,
    }

    pub fn shell(autostart: bool) -> Result<Shell, String> {
        let rgba = include_bytes!("../ui/tray32.rgba").to_vec();
        let icon = Icon::from_rgba(rgba, 32, 32).map_err(|e| format!("ikon: {e}"))?;
        let menu = Menu::new();
        let open = MenuItem::new("Ana pencere", true, None);
        let auto = CheckMenuItem::new("Windows ile başlat", true, autostart, None);
        let quit = MenuItem::new("Çık", true, None);
        let _ = menu.append_items(&[&open, &auto, &PredefinedMenuItem::separator(), &quit]);
        let tray = TrayIconBuilder::new()
            .with_tooltip("Yarın Yaparım")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| format!("tray: {e}"))?;
        let mut hotkey_label = String::new();
        let hk = GlobalHotKeyManager::new().map_err(|e| crate::log(&format!("kısayol yöneticisi: {e}"))).ok().and_then(|m| {
            for (mods, label) in [
                (Modifiers::CONTROL, "Ctrl+Space"),
                (Modifiers::CONTROL | Modifiers::SHIFT, "Ctrl+Shift+Space"),
                (Modifiers::CONTROL | Modifiers::ALT, "Ctrl+Alt+Space"),
            ] {
                match m.register(HotKey::new(Some(mods), Code::Space)) {
                    Ok(()) => { hotkey_label = label.to_string(); return Some(m); }
                    Err(e) => crate::log(&format!("{label} alınamadı: {e}")),
                }
            }
            None
        });
        Ok(Shell { open_id: open.id().clone(), quit_id: quit.id().clone(), auto, _tray: tray, _hk: hk, hotkey_label })
    }

    impl Shell {
        pub fn poll(&self) -> Vec<Ev> {
            let mut out = vec![];
            while let Ok(e) = TrayIconEvent::receiver().try_recv() {
                match e {
                    TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => out.push(Ev::PanelToggle),
                    TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => out.push(Ev::OpenMain),
                    _ => {}
                }
            }
            while let Ok(e) = MenuEvent::receiver().try_recv() {
                if e.id == self.open_id { out.push(Ev::OpenMain); }
                else if e.id == self.quit_id { out.push(Ev::Quit); }
                else if &e.id == self.auto.id() { out.push(Ev::AutostartToggled(self.auto.is_checked())); }
            }
            while let Ok(e) = GlobalHotKeyEvent::receiver().try_recv() {
                if e.state == HotKeyState::Pressed { out.push(Ev::Hotkey); }
            }
            out
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Ev;
    use global_hotkey::{hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use ksni::blocking::TrayMethods;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::mpsc::{channel, Receiver, Sender};

    fn sock_path() -> PathBuf {
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(d) => PathBuf::from(d).join("yarinyaparim.sock"),
            None => PathBuf::from(format!("/tmp/yarinyaparim-{}.sock", std::env::var("USER").unwrap_or_default())),
        }
    }

    /// Tek örnek: Unix soketi. Zaten çalışıyorsa komutu (show/panel) ona iletip None döner.
    pub struct Instance { l: Option<UnixListener> }

    pub fn single_instance(cmd: &str) -> Option<Instance> {
        let p = sock_path();
        if let Ok(mut s) = UnixStream::connect(&p) {
            let _ = s.write_all(cmd.as_bytes());
            return None;
        }
        let _ = std::fs::remove_file(&p);
        let l = UnixListener::bind(&p).ok();
        if let Some(l) = &l { let _ = l.set_nonblocking(true); }
        Some(Instance { l })
    }

    impl Instance {
        pub fn poll(&self) -> Vec<Ev> {
            let mut out = vec![];
            if let Some(l) = &self.l {
                while let Ok((mut s, _)) = l.accept() {
                    let _ = s.set_read_timeout(Some(std::time::Duration::from_millis(200)));
                    let mut b = String::new();
                    let _ = s.read_to_string(&mut b);
                    out.push(if b.trim() == "panel" { Ev::Hotkey } else { Ev::OpenMain });
                }
            }
            out
        }
    }

    fn desktop_file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("autostart").join("yarinyaparim.desktop"))
    }

    pub fn set_autostart(on: bool) {
        let Some(f) = desktop_file() else { return };
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            let _ = std::fs::write(&f, format!(
                "[Desktop Entry]\nType=Application\nName=Yarın Yaparım\nExec=\"{}\" --hidden\nX-GNOME-Autostart-enabled=true\n",
                exe.display()));
        } else {
            let _ = std::fs::remove_file(f);
        }
    }

    pub fn work_area() -> Option<(i32, i32, i32, i32)> { None }

    struct Tray { tx: Sender<Ev>, auto: bool }

    impl ksni::Tray for Tray {
        fn id(&self) -> String { "yarinyaparim".into() }
        fn title(&self) -> String { "Yarın Yaparım".into() }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            // RGBA → ARGB
            let rgba = include_bytes!("../ui/tray32.rgba");
            let data = rgba.chunks(4).flat_map(|c| [c[3], c[0], c[1], c[2]]).collect();
            vec![ksni::Icon { width: 32, height: 32, data }]
        }
        fn activate(&mut self, _x: i32, _y: i32) { let _ = self.tx.send(Ev::PanelToggle); }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::*;
            vec![
                StandardItem { label: "Ana pencere".into(), activate: Box::new(|t: &mut Self| { let _ = t.tx.send(Ev::OpenMain); }), ..Default::default() }.into(),
                StandardItem { label: "Panel".into(), activate: Box::new(|t: &mut Self| { let _ = t.tx.send(Ev::Hotkey); }), ..Default::default() }.into(),
                CheckmarkItem { label: "Oturumla başlat".into(), checked: self.auto, activate: Box::new(|t: &mut Self| {
                    t.auto = !t.auto;
                    let _ = t.tx.send(Ev::AutostartToggled(t.auto));
                }), ..Default::default() }.into(),
                MenuItem::Separator,
                StandardItem { label: "Çık".into(), activate: Box::new(|t: &mut Self| { let _ = t.tx.send(Ev::Quit); }), ..Default::default() }.into(),
            ]
        }
    }

    pub struct Shell {
        _tray: ksni::blocking::Handle<Tray>,
        _hk: Option<GlobalHotKeyManager>,
        rx: Receiver<Ev>,
        pub hotkey_label: String,
    }

    pub fn shell(autostart: bool) -> Result<Shell, String> {
        let (tx, rx) = channel();
        let tray = Tray { tx, auto: autostart }.spawn().map_err(|e| format!("tray (StatusNotifier): {e}"))?;
        let mut hotkey_label = String::new();
        // Yalnız X11'de çalışır; Wayland'de sistem kısayoluna `YarinYaparim --panel` bağlanır.
        let hk = GlobalHotKeyManager::new().map_err(|e| crate::log(&format!("kısayol yöneticisi: {e}"))).ok().and_then(|m| {
            match m.register(HotKey::new(Some(Modifiers::CONTROL), Code::Space)) {
                Ok(()) => { hotkey_label = "Ctrl+Space".into(); Some(m) }
                Err(e) => { crate::log(&format!("Ctrl+Space alınamadı: {e}")); None }
            }
        });
        Ok(Shell { _tray: tray, _hk: hk, rx, hotkey_label })
    }

    impl Shell {
        pub fn poll(&self) -> Vec<Ev> {
            let mut out: Vec<Ev> = self.rx.try_iter().collect();
            while let Ok(e) = GlobalHotKeyEvent::receiver().try_recv() {
                if e.state == HotKeyState::Pressed { out.push(Ev::Hotkey); }
            }
            out
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::Ev;
    pub struct Instance;
    pub fn single_instance(_cmd: &str) -> Option<Instance> { Some(Instance) }
    impl Instance { pub fn poll(&self) -> Vec<Ev> { vec![] } }
    pub fn set_autostart(_on: bool) {}
    pub fn work_area() -> Option<(i32, i32, i32, i32)> { None }
    pub struct Shell { pub hotkey_label: String }
    pub fn shell(_a: bool) -> Result<Shell, String> { Err("bu platformda tray yok".into()) }
    impl Shell { pub fn poll(&self) -> Vec<Ev> { vec![] } }
}

pub use imp::*;
