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

    pub fn single_instance() -> Option<Instance> {
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
        pub fn show_requested(&self) -> bool {
            !self.ev.is_null() && unsafe { WaitForSingleObject(self.ev, 0) } == WAIT_OBJECT_0
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

#[cfg(not(windows))]
mod imp {
    use super::Ev;
    pub struct Instance;
    pub fn single_instance() -> Option<Instance> { Some(Instance) }
    impl Instance { pub fn show_requested(&self) -> bool { false } }
    pub fn set_autostart(_on: bool) {}
    pub fn work_area() -> Option<(i32, i32, i32, i32)> { None }
    pub struct Shell { pub hotkey_label: String }
    pub fn shell(_a: bool) -> Result<Shell, String> { Err("bu platformda tray yok".into()) }
    impl Shell { pub fn poll(&self) -> Vec<Ev> { vec![] } }
}

pub use imp::*;
