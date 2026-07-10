#[cfg(windows)]
mod platform {
    use super::TrayMenuState;

    use std::mem::size_of;
    use std::ptr::null_mut;
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread::{self, JoinHandle};

    use eframe::egui;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostMessageW, RegisterClassW,
        SetForegroundWindow, TrackPopupMenu, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
        CW_USEDEFAULT, GWLP_USERDATA, HMENU, IDI_APPLICATION, MF_CHECKED, MF_SEPARATOR, MF_STRING,
        MF_UNCHECKED, MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CREATE, WM_DESTROY,
        WM_LBUTTONDBLCLK, WM_NCCREATE, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
    };

    const TRAY_ID: u32 = 1;
    const WM_CLEARLINE_TRAY: u32 = WM_APP + 77;
    const MENU_SHOW: u32 = 1001;
    const MENU_TOGGLE_DENOISE: u32 = 1002;
    const MENU_TOGGLE_STARTUP: u32 = 1003;
    const MENU_TOGGLE_WIND: u32 = 1004;
    const MENU_TOGGLE_ECHO: u32 = 1005;
    const MENU_EXIT: u32 = 1099;
    const APP_ICON_RESOURCE_ID: usize = 1;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TrayEvent {
        ShowWindow,
        ToggleNoiseSuppression,
        ToggleStartOnLogin,
        ToggleWindNoiseReduction,
        ToggleEchoCancellation,
        Exit,
    }

    pub struct TrayController {
        hwnd: isize,
        menu_state: Arc<Mutex<TrayMenuState>>,
        thread: Option<JoinHandle<()>>,
    }

    impl TrayController {
        pub fn install(ctx: egui::Context) -> Option<(Self, mpsc::Receiver<TrayEvent>)> {
            let (hwnd_sender, hwnd_receiver) = mpsc::sync_channel(1);
            let (event_sender, event_receiver) = mpsc::channel();
            let menu_state = Arc::new(Mutex::new(TrayMenuState::default()));
            let menu_state_for_thread = Arc::clone(&menu_state);
            let thread = thread::spawn(move || {
                let hwnd = unsafe { create_tray_window(ctx, event_sender, menu_state_for_thread) };
                let _ = hwnd_sender.send(hwnd as isize);
                if hwnd.is_null() {
                    return;
                }

                unsafe {
                    let mut message = MSG::default();
                    while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            });

            let hwnd = hwnd_receiver.recv().ok()?;
            if hwnd == 0 {
                let _ = thread.join();
                return None;
            }

            Some((
                Self {
                    hwnd,
                    menu_state,
                    thread: Some(thread),
                },
                event_receiver,
            ))
        }

        pub fn update_menu_state(&self, state: TrayMenuState) {
            if let Ok(mut menu_state) = self.menu_state.lock() {
                *menu_state = state;
            }
        }
    }

    impl Drop for TrayController {
        fn drop(&mut self) {
            unsafe {
                PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
            }
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    unsafe fn create_tray_window(
        ctx: egui::Context,
        event_sender: mpsc::Sender<TrayEvent>,
        menu_state: Arc<Mutex<TrayMenuState>>,
    ) -> HWND {
        let class_name = to_wide("ClearLineTrayWindow");
        let instance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&class);

        let boxed = Box::new(TrayState {
            ctx,
            event_sender,
            menu_state,
        });
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            to_wide("ClearLine").as_ptr(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            null_mut(),
            null_mut(),
            instance,
            Box::into_raw(boxed).cast(),
        );

        if hwnd.is_null() {
            return hwnd;
        }

        if !add_tray_icon(hwnd) {
            DestroyWindow(hwnd);
            return null_mut();
        }

        hwnd
    }

    struct TrayState {
        ctx: egui::Context,
        event_sender: mpsc::Sender<TrayEvent>,
        menu_state: Arc<Mutex<TrayMenuState>>,
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCCREATE => {
                let createstruct =
                    lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
                let state = (*createstruct).lpCreateParams as *mut TrayState;
                windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    state as isize,
                );
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_CLEARLINE_TRAY => {
                match lparam as u32 {
                    WM_LBUTTONDBLCLK => {
                        if let Some(state) = tray_state(hwnd) {
                            show_main_window(state);
                        }
                    }
                    WM_RBUTTONUP => {
                        if let Some(state) = tray_state(hwnd) {
                            show_tray_menu(hwnd, state);
                        }
                    }
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                remove_tray_icon(hwnd);
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                let state = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                ) as *mut TrayState;
                if !state.is_null() {
                    drop(Box::from_raw(state));
                    windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                        hwnd,
                        GWLP_USERDATA,
                        0,
                    );
                }
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                0
            }
            WM_CREATE => 0,
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn tray_state(hwnd: HWND) -> Option<&'static TrayState> {
        let state =
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
                as *const TrayState;
        state.as_ref()
    }

    unsafe fn show_main_window(state: &TrayState) {
        state
            .ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        state
            .ctx
            .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        state.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        let _ = state.event_sender.send(TrayEvent::ShowWindow);
        state.ctx.request_repaint();
    }

    unsafe fn show_tray_menu(hwnd: HWND, state: &TrayState) {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }

        let menu_state = state
            .menu_state
            .lock()
            .map(|state| *state)
            .unwrap_or_default();

        append_menu_item(menu, MENU_SHOW, "打开 ClearLine");
        append_separator(menu);
        append_checked_menu_item(
            menu,
            MENU_TOGGLE_DENOISE,
            "降噪",
            menu_state.noise_suppression_enabled,
        );
        append_checked_menu_item(
            menu,
            MENU_TOGGLE_STARTUP,
            "开机自启动",
            menu_state.start_on_login_enabled,
        );
        append_checked_menu_item(
            menu,
            MENU_TOGGLE_WIND,
            "抗风噪增强",
            menu_state.wind_noise_reduction_enabled,
        );
        append_checked_menu_item(
            menu,
            MENU_TOGGLE_ECHO,
            "回音消除",
            menu_state.echo_cancellation_enabled,
        );
        append_separator(menu);
        append_menu_item(menu, MENU_EXIT, "退出 ClearLine");

        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            DestroyMenu(menu);
            return;
        }

        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as u32;
        DestroyMenu(menu);

        match command {
            MENU_SHOW => show_main_window(state),
            MENU_TOGGLE_DENOISE => send_tray_event(state, TrayEvent::ToggleNoiseSuppression),
            MENU_TOGGLE_STARTUP => send_tray_event(state, TrayEvent::ToggleStartOnLogin),
            MENU_TOGGLE_WIND => send_tray_event(state, TrayEvent::ToggleWindNoiseReduction),
            MENU_TOGGLE_ECHO => send_tray_event(state, TrayEvent::ToggleEchoCancellation),
            MENU_EXIT => send_tray_event(state, TrayEvent::Exit),
            _ => {}
        }
    }

    unsafe fn append_menu_item(menu: HMENU, id: u32, label: &str) {
        let label = to_wide(label);
        AppendMenuW(menu, MF_STRING, id as usize, label.as_ptr());
    }

    unsafe fn append_checked_menu_item(menu: HMENU, id: u32, label: &str, checked: bool) {
        let label = to_wide(label);
        let check_flag = if checked { MF_CHECKED } else { MF_UNCHECKED };
        AppendMenuW(menu, MF_STRING | check_flag, id as usize, label.as_ptr());
    }

    unsafe fn append_separator(menu: HMENU) {
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    }

    fn send_tray_event(state: &TrayState, event: TrayEvent) {
        let _ = state.event_sender.send(event);
        state.ctx.request_repaint();
    }

    unsafe fn add_tray_icon(hwnd: HWND) -> bool {
        let mut data = tray_icon_data(hwnd);
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_CLEARLINE_TRAY;
        let instance = GetModuleHandleW(std::ptr::null());
        data.hIcon = LoadIconW(instance, APP_ICON_RESOURCE_ID as *const u16);
        if data.hIcon.is_null() {
            data.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        }
        write_wide_fixed(&mut data.szTip, "ClearLine");
        Shell_NotifyIconW(NIM_ADD, &data) != 0
    }

    unsafe fn remove_tray_icon(hwnd: HWND) {
        let data = tray_icon_data(hwnd);
        Shell_NotifyIconW(NIM_DELETE, &data);
    }

    fn tray_icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            ..Default::default()
        }
    }

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn write_wide_fixed(buffer: &mut [u16], value: &str) {
        buffer.fill(0);
        let max_len = buffer.len().saturating_sub(1);
        for (slot, ch) in buffer.iter_mut().take(max_len).zip(value.encode_utf16()) {
            *slot = ch;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TrayMenuState {
    pub noise_suppression_enabled: bool,
    pub start_on_login_enabled: bool,
    pub wind_noise_reduction_enabled: bool,
    pub echo_cancellation_enabled: bool,
}

#[cfg(windows)]
pub use platform::TrayController;
#[cfg(windows)]
pub use platform::TrayEvent;

#[cfg(not(windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TrayEvent {
    ShowWindow,
    ToggleNoiseSuppression,
    ToggleStartOnLogin,
    ToggleWindNoiseReduction,
    ToggleEchoCancellation,
    Exit,
}

#[cfg(not(windows))]
pub struct TrayController;

#[cfg(not(windows))]
impl TrayController {
    pub fn install(
        _ctx: eframe::egui::Context,
    ) -> Option<(Self, std::sync::mpsc::Receiver<TrayEvent>)> {
        None
    }

    pub fn update_menu_state(&self, _state: TrayMenuState) {}
}
