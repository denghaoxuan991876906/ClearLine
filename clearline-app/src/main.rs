#![cfg_attr(windows, windows_subsystem = "windows")]

mod settings;
mod tray;
mod windows_settings;

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::Duration,
};

#[cfg(test)]
use clearline_core::AudioOutputTarget;
use clearline_core::{
    AudioFrameFormat, AudioInputDevice, AudioOutputDevice, AudioPipeline, AudioPipelineConfig,
    ClearLineError, CpalDeviceEnumerator, DeepFilterNetModelBundle, DeviceEnumerator, DeviceId,
    EchoCancellerBackend, EchoReferenceDiagnostics, PipelineMetrics, PipelineRuntimeInfo,
    PipelineState, SuppressionStrength, SuppressorMode,
};
use eframe::egui;
use egui::{Color32, FontData, FontDefinitions, FontFamily, RichText, Stroke, Vec2};
use settings::{
    suppression_strength_from_setting, suppression_strength_to_setting,
    suppressor_mode_from_setting, suppressor_mode_to_setting, PersistedSettings, SettingsStore,
};

const INITIAL_WINDOW_SIZE: [f32; 2] = [1085.0, 580.0];
const MINIMUM_WINDOW_SIZE: [f32; 2] = [1085.0, 580.0];
const PACKAGED_DEEPFILTER_MODEL_DIR: &str = "models/deepfilternet";
const DEEPFILTER_MODEL_DIR_ENV: &str = "CLEARLINE_DF_MODEL_DIR";
const MINIMIZED_ARG: &str = "--minimized";

fn main() -> eframe::Result {
    let start_minimized = start_minimized_from_args(env::args().skip(1));
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(initial_window_size())
            .with_min_inner_size(minimum_window_size())
            .with_visible(!start_minimized),
        ..Default::default()
    };

    eframe::run_native(
        "ClearLine",
        native_options,
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            configure_style(&cc.egui_ctx);
            Ok(Box::new(ClearLineApp::new(&cc.egui_ctx)))
        }),
    )
}

fn start_minimized_from_args(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter()
        .any(|arg| matches!(arg.as_str(), MINIMIZED_ARG | "--hidden" | "/minimized"))
}

fn initial_window_size() -> [f32; 2] {
    INITIAL_WINDOW_SIZE
}

fn minimum_window_size() -> [f32; 2] {
    MINIMUM_WINDOW_SIZE
}

struct ClearLineApp {
    enumerator: CpalDeviceEnumerator,
    devices: Vec<AudioInputDevice>,
    output_devices: Vec<AudioOutputDevice>,
    selected_device_id: Option<DeviceId>,
    selected_output_device_id: Option<DeviceId>,
    suppressor_mode: SuppressorModeSelection,
    noise_suppression_enabled: bool,
    suppression_strength: SuppressionStrength,
    wind_noise_reduction_enabled: bool,
    echo_cancellation_enabled: bool,
    start_on_login_enabled: bool,
    selected_tab: AppTab,
    pipeline: AudioPipeline,
    input_level: f32,
    status_message: String,
    settings_store: Option<SettingsStore>,
    pending_settings: Option<PersistedSettings>,
    settings_loaded: bool,
    tray: Option<tray::TrayController>,
    tray_events: Option<mpsc::Receiver<tray::TrayEvent>>,
    start_on_login_task: Option<mpsc::Receiver<(bool, Result<(), String>)>>,
    pending_start_on_login_enabled: Option<bool>,
    exit_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    Device,
    Status,
}

impl AppTab {
    const ALL: [AppTab; 2] = [AppTab::Device, AppTab::Status];
}

fn default_app_tab() -> AppTab {
    AppTab::Device
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SuppressorModeSelection(SuppressorMode);

impl SuppressorModeSelection {
    fn value(self) -> SuppressorMode {
        self.0
    }
}

fn default_suppressor_mode() -> SuppressorMode {
    SuppressorMode::HighQuality
}

fn resolve_input_device_from_settings(
    saved_id: Option<&str>,
    saved_name: Option<&str>,
    devices: &[AudioInputDevice],
) -> Option<DeviceId> {
    resolve_device_id_from_settings(
        saved_id,
        saved_name,
        devices
            .iter()
            .map(|device| (device.id(), device.name(), device.is_default())),
    )
}

fn resolve_output_device_from_settings(
    _saved_id: Option<&str>,
    _saved_name: Option<&str>,
    devices: &[AudioOutputDevice],
) -> Option<DeviceId> {
    devices
        .iter()
        .find(|device| is_vb_cable_render_device_name(device.name()))
        .map(|device| device.id().clone())
}

fn resolve_device_id_from_settings<'a>(
    saved_id: Option<&str>,
    saved_name: Option<&str>,
    devices: impl Iterator<Item = (&'a DeviceId, &'a str, bool)>,
) -> Option<DeviceId> {
    let devices = devices.collect::<Vec<_>>();

    if let Some(saved_id) = saved_id.filter(|value| !value.trim().is_empty()) {
        if let Some(device) = devices.iter().find(|device| device.0.as_str() == saved_id) {
            return Some(device.0.clone());
        }
    }

    if let Some(saved_name) = saved_name.filter(|value| !value.trim().is_empty()) {
        if let Some(device) = devices.iter().find(|device| device.1 == saved_name) {
            return Some(device.0.clone());
        }
    }

    devices
        .iter()
        .find(|device| device.2)
        .or_else(|| devices.first())
        .map(|device| device.0.clone())
}

impl ClearLineApp {
    fn new(ctx: &egui::Context) -> Self {
        let (settings_store, pending_settings, status_message, settings_loaded) =
            match SettingsStore::new() {
                Ok(store) => match store.load() {
                    Ok(settings) => {
                        let loaded = settings.is_some();
                        let status = if loaded {
                            "已加载本地设置".to_owned()
                        } else {
                            "正在初始化设备列表".to_owned()
                        };
                        (Some(store), settings, status, loaded)
                    }
                    Err(error) => (
                        Some(store),
                        None,
                        format!("设置文件无效，已使用默认设置：{error}"),
                        false,
                    ),
                },
                Err(error) => (
                    None,
                    None,
                    format!("设置目录不可用，已使用默认设置：{error}"),
                    false,
                ),
            };

        let mut app = Self::new_with_settings_for_tests(
            settings_store,
            pending_settings,
            status_message,
            settings_loaded,
        );
        if let Some((tray, events)) = tray::TrayController::install(ctx.clone()) {
            app.tray = Some(tray);
            app.tray_events = Some(events);
        }
        app.refresh_devices();
        app.start_on_login_enabled = windows_settings::is_start_on_login_enabled();
        app.start_pipeline_if_ready();
        app
    }

    fn new_with_settings_for_tests(
        settings_store: Option<SettingsStore>,
        pending_settings: Option<PersistedSettings>,
        status_message: String,
        settings_loaded: bool,
    ) -> Self {
        Self {
            enumerator: CpalDeviceEnumerator,
            devices: Vec::new(),
            output_devices: Vec::new(),
            selected_device_id: None,
            selected_output_device_id: None,
            suppressor_mode: SuppressorModeSelection(default_suppressor_mode()),
            noise_suppression_enabled: true,
            suppression_strength: SuppressionStrength::default(),
            wind_noise_reduction_enabled: false,
            echo_cancellation_enabled: true,
            start_on_login_enabled: false,
            selected_tab: default_app_tab(),
            pipeline: AudioPipeline::new(),
            input_level: 0.0,
            status_message,
            settings_store,
            pending_settings,
            settings_loaded,
            tray: None,
            tray_events: None,
            start_on_login_task: None,
            pending_start_on_login_enabled: None,
            exit_requested: false,
        }
    }

    #[cfg(test)]
    fn new_without_loading_settings_for_tests() -> Self {
        Self::new_with_settings_for_tests(None, None, "正在初始化设备列表".to_owned(), false)
    }

    fn refresh_devices(&mut self) {
        let input_result = self.enumerator.input_devices();

        match input_result {
            Ok(input_devices) => {
                self.devices = input_devices;
                self.ensure_selected_input_device();
                if let Ok(output_devices) = self.enumerator.output_devices() {
                    self.output_devices = output_devices;
                    self.ensure_selected_output_device();
                }
                self.apply_pending_settings_after_refresh();
                if self.devices.is_empty() || self.status_message == "正在初始化设备列表" {
                    self.status_message = if self.devices.is_empty() {
                        "未找到输入设备。请检查 Windows 麦克风权限和录音设备。".to_owned()
                    } else {
                        format!("已找到 {} 个输入设备", self.devices.len())
                    };
                }
            }
            Err(error) => {
                self.devices.clear();
                self.selected_device_id = None;
                self.pipeline.fail(error.to_string());
                self.status_message = format!("输入设备枚举失败：{error}");
            }
        }
    }

    fn ensure_selected_input_device(&mut self) {
        let current_is_valid = self
            .selected_device_id
            .as_ref()
            .is_some_and(|selected| self.devices.iter().any(|device| device.id() == selected));

        if current_is_valid {
            return;
        }

        self.selected_device_id = self
            .devices
            .iter()
            .find(|device| device.is_default())
            .or_else(|| self.devices.first())
            .map(|device| device.id().clone());
    }

    fn ensure_selected_output_device(&mut self) {
        let current_is_valid_vb_cable = self
            .selected_output_device_id
            .as_ref()
            .and_then(|selected| {
                self.output_devices
                    .iter()
                    .find(|device| device.id() == selected)
            })
            .is_some_and(|device| is_vb_cable_render_device_name(device.name()));

        if current_is_valid_vb_cable {
            return;
        }

        self.selected_output_device_id = self
            .output_devices
            .iter()
            .find(|device| is_vb_cable_render_device_name(device.name()))
            .map(|device| device.id().clone());
    }

    fn selected_device_label(&self) -> String {
        match self.selected_device() {
            Some(device) => device_label(device),
            None => "选择麦克风".to_owned(),
        }
    }

    fn selected_device(&self) -> Option<&AudioInputDevice> {
        let selected = self.selected_device_id.as_ref()?;
        self.devices.iter().find(|device| device.id() == selected)
    }

    fn selected_output_device(&self) -> Option<&AudioOutputDevice> {
        let selected = self.selected_output_device_id.as_ref()?;
        self.output_devices
            .iter()
            .find(|device| device.id() == selected)
    }

    fn apply_pending_settings_after_refresh(&mut self) {
        let Some(settings) = self.pending_settings.take() else {
            return;
        };

        let is_legacy_settings = settings.version < settings::SETTINGS_VERSION;

        self.suppressor_mode =
            SuppressorModeSelection(suppressor_mode_from_setting(&settings.suppressor_mode));
        self.suppression_strength =
            suppression_strength_from_setting(&settings.suppression_strength);
        self.wind_noise_reduction_enabled = settings.wind_noise_reduction_enabled;
        self.noise_suppression_enabled = settings.noise_suppression_enabled;
        self.start_on_login_enabled = settings.start_on_login_enabled;
        self.echo_cancellation_enabled = if is_legacy_settings {
            true
        } else {
            settings.echo_cancellation_enabled
        };

        self.selected_device_id = resolve_input_device_from_settings(
            settings.input_device_id.as_deref(),
            settings.input_device_name.as_deref(),
            &self.devices,
        );
        self.selected_output_device_id = resolve_output_device_from_settings(
            settings.output_device_id.as_deref(),
            settings.output_device_name.as_deref(),
            &self.output_devices,
        );

        let input_fell_back = settings.input_device_id.as_deref().is_some_and(|saved| {
            self.selected_device_id.as_ref().map(DeviceId::as_str) != Some(saved)
        });
        let output_fell_back = settings.output_device_id.as_deref().is_some_and(|saved| {
            self.selected_output_device_id
                .as_ref()
                .map(DeviceId::as_str)
                != Some(saved)
        });

        if input_fell_back || output_fell_back {
            self.status_message = "已加载本地设置，部分设备不可用，已使用可用设备".to_owned();
        } else if self.settings_loaded {
            self.status_message = "已加载本地设置".to_owned();
        }
    }

    fn persisted_settings_snapshot(&self) -> PersistedSettings {
        let input = self.selected_device();
        let output = self.selected_output_device();

        PersistedSettings {
            version: settings::SETTINGS_VERSION,
            input_device_id: self
                .selected_device_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            input_device_name: input.map(|device| device.name().to_owned()),
            output_device_id: self
                .selected_output_device_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            output_device_name: output.map(|device| device.name().to_owned()),
            output_target: settings::OUTPUT_TARGET_VB_CABLE.to_owned(),
            suppressor_mode: suppressor_mode_to_setting(self.suppressor_mode.value()).to_owned(),
            suppression_strength: suppression_strength_to_setting(self.suppression_strength)
                .to_owned(),
            wind_noise_reduction_enabled: self.wind_noise_reduction_enabled,
            echo_cancellation_enabled: self.echo_cancellation_enabled,
            noise_suppression_enabled: self.noise_suppression_enabled,
            start_on_login_enabled: self.start_on_login_enabled,
            deepfilter_model_dir: String::new(),
        }
    }

    fn effective_suppressor_mode(&self) -> SuppressorMode {
        if self.noise_suppression_enabled {
            self.suppressor_mode.value()
        } else {
            SuppressorMode::Bypass
        }
    }

    fn effective_wind_noise_reduction_enabled(&self) -> bool {
        self.noise_suppression_enabled && self.wind_noise_reduction_enabled
    }

    fn effective_echo_cancellation_enabled(&self) -> bool {
        self.noise_suppression_enabled && self.echo_cancellation_enabled
    }

    fn start_pipeline_if_ready(&mut self) {
        if self.pipeline.state().is_running() {
            return;
        }
        if self.selected_device_id.is_some() && self.selected_output_device_id.is_some() {
            self.start_pipeline();
        }
    }

    fn apply_runtime_change_if_running(&mut self) {
        if self.pipeline.state().is_running() {
            self.pipeline.stop();
            self.start_pipeline();
        } else {
            self.start_pipeline_if_ready();
        }
    }

    fn set_noise_suppression_enabled(&mut self, enabled: bool) {
        if self.noise_suppression_enabled == enabled {
            return;
        }
        self.noise_suppression_enabled = enabled;
        self.save_settings_after_user_change();
        self.apply_runtime_change_if_running();
    }

    fn set_wind_noise_reduction_enabled(&mut self, enabled: bool) {
        if self.wind_noise_reduction_enabled == enabled {
            return;
        }
        self.wind_noise_reduction_enabled = enabled;
        self.save_settings_after_user_change();
        self.apply_runtime_change_if_running();
    }

    fn set_echo_cancellation_enabled(&mut self, enabled: bool) {
        if self.echo_cancellation_enabled == enabled {
            return;
        }
        self.echo_cancellation_enabled = enabled;
        self.save_settings_after_user_change();
        self.apply_runtime_change_if_running();
    }

    fn begin_start_on_login_change(&mut self, enabled: bool) {
        if self.pending_start_on_login_enabled.is_some() {
            return;
        }

        let (sender, receiver) = mpsc::channel();
        self.pending_start_on_login_enabled = Some(enabled);
        self.start_on_login_task = Some(receiver);
        self.status_message = if enabled {
            "正在开启开机自启动".to_owned()
        } else {
            "正在关闭开机自启动".to_owned()
        };

        std::thread::spawn(move || {
            let result = windows_settings::set_start_on_login_enabled(enabled)
                .map_err(|error| error.to_string());
            let _ = sender.send((enabled, result));
        });
    }

    fn request_exit(&mut self, ctx: &egui::Context) {
        self.exit_requested = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn save_settings_after_user_change(&mut self) {
        let Some(store) = self.settings_store.clone() else {
            return;
        };
        let settings = self.persisted_settings_snapshot();

        match store.save(&settings) {
            Ok(()) => {
                if !self.pipeline.state().is_running() {
                    self.status_message = "设置已保存".to_owned();
                }
            }
            Err(error) => {
                self.status_message = format!("设置保存失败：{error}");
            }
        }
    }

    fn start_pipeline(&mut self) {
        let Some(device_id) = self.selected_device_id.clone() else {
            self.pipeline.fail("未选择输入设备");
            self.status_message = "启动前请先选择麦克风".to_owned();
            return;
        };
        let Some(output_device) = self
            .selected_output_device()
            .filter(|device| is_vb_cable_render_device_name(device.name()))
        else {
            self.pipeline.fail("未找到 VB-CABLE 输出端点");
            self.status_message =
                "未找到 VB-CABLE 输出端点（CABLE Input 或 CABLE In 16 Ch）。请先安装 VB-CABLE，或重新运行 ClearLine 安装器。".to_owned();
            return;
        };
        let output_device_id = output_device.id().clone();
        let output_label = output_device.name().to_owned();
        let mode = self.effective_suppressor_mode();
        let model_status = packaged_deepfilter_model_status();
        let mut config = AudioPipelineConfig::new(device_id, output_device_id, mode)
            .with_suppression_strength(self.suppression_strength)
            .with_wind_noise_reduction(self.effective_wind_noise_reduction_enabled())
            .with_echo_cancellation(self.effective_echo_cancellation_enabled());
        if mode == SuppressorMode::HighQuality {
            if model_status != DeepFilterModelUiStatus::Valid {
                let message = deepfilter_startup_warning(mode, &model_status)
                    .unwrap_or_else(|| "DeepFilterNet 模型不可用，无法启动高质量降噪".to_owned());
                self.pipeline.fail(&message);
                self.status_message = message;
                return;
            }

            let Some(bundle) = deepfilter_model_bundle_for_pipeline() else {
                let message = "DeepFilterNet 模型不可用，无法启动高质量降噪".to_owned();
                self.pipeline.fail(&message);
                self.status_message = message;
                return;
            };
            config = config.with_deepfilternet_model_bundle(bundle);
        }
        let uses_deepfilter_model = config.deepfilternet_model_bundle().is_some();
        let selected_mode_label = if uses_deepfilter_model {
            "高质量降噪（DeepFilterNet 模型）".to_owned()
        } else if mode == SuppressorMode::HighQuality {
            "高质量降噪".to_owned()
        } else {
            mode_label(mode).to_owned()
        };
        match self.pipeline.start(config) {
            Ok(()) => {
                self.input_level = self.pipeline.input_level();
                self.status_message = format!("正在使用{selected_mode_label}输出到 {output_label}");
            }
            Err(error) => {
                self.input_level = 0.0;
                self.pipeline.fail(error.to_string());
                self.status_message = format!("启动失败：{error}");
            }
        }
    }

    fn sync_pipeline_level(&mut self) {
        self.input_level = self.pipeline.input_level();
    }
}

impl eframe::App for ClearLineApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_tray_events(ui.ctx());
        self.poll_start_on_login_task();
        self.update_tray_menu_state();
        self.handle_close_to_tray(ui.ctx());
        self.sync_pipeline_level();
        if self.pipeline.state().is_running() || self.start_on_login_task.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }

        egui::Frame::NONE
            .fill(ios_background())
            .inner_margin(egui::Margin::symmetric(20, 16))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                self.header(ui);
                ui.add_space(12.0);
                self.tab_bar(ui);
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.desktop_content(ui);
                    });
            });
    }
}

impl ClearLineApp {
    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        let mut events = Vec::new();
        if let Some(receiver) = self.tray_events.as_ref() {
            while let Ok(event) = receiver.try_recv() {
                events.push(event);
            }
        }

        for event in events {
            match event {
                tray::TrayEvent::ShowWindow => self.show_window(ctx),
                tray::TrayEvent::ToggleNoiseSuppression => {
                    self.set_noise_suppression_enabled(!self.noise_suppression_enabled);
                }
                tray::TrayEvent::ToggleStartOnLogin => {
                    if self.pending_start_on_login_enabled.is_none() {
                        self.begin_start_on_login_change(!self.start_on_login_enabled);
                    }
                }
                tray::TrayEvent::ToggleWindNoiseReduction => {
                    self.set_wind_noise_reduction_enabled(!self.wind_noise_reduction_enabled);
                }
                tray::TrayEvent::ToggleEchoCancellation => {
                    self.set_echo_cancellation_enabled(!self.echo_cancellation_enabled);
                }
                tray::TrayEvent::Exit => self.request_exit(ctx),
            }
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn update_tray_menu_state(&self) {
        let Some(tray) = self.tray.as_ref() else {
            return;
        };
        tray.update_menu_state(tray::TrayMenuState {
            noise_suppression_enabled: self.noise_suppression_enabled,
            start_on_login_enabled: self
                .pending_start_on_login_enabled
                .unwrap_or(self.start_on_login_enabled),
            wind_noise_reduction_enabled: self.wind_noise_reduction_enabled,
            echo_cancellation_enabled: self.echo_cancellation_enabled,
        });
    }

    fn poll_start_on_login_task(&mut self) {
        let Some(receiver) = self.start_on_login_task.as_ref() else {
            return;
        };

        match receiver.try_recv() {
            Ok((enabled, result)) => {
                self.start_on_login_task = None;
                self.pending_start_on_login_enabled = None;
                match result {
                    Ok(()) => {
                        self.start_on_login_enabled = enabled;
                        self.save_settings_after_user_change();
                        self.status_message = if enabled {
                            "已开启开机自启动".to_owned()
                        } else {
                            "已关闭开机自启动".to_owned()
                        };
                    }
                    Err(error) => {
                        self.status_message = format!("开机自启动设置失败：{error}");
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.start_on_login_task = None;
                self.pending_start_on_login_enabled = None;
                self.status_message = "开机自启动设置失败：后台任务已断开".to_owned();
            }
        }
    }

    fn handle_close_to_tray(&mut self, ctx: &egui::Context) {
        if self.exit_requested {
            return;
        }
        if self.tray.is_some() && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.status_message = "已最小化到系统托盘，双击托盘图标可恢复窗口".to_owned();
        }
    }

    fn desktop_content(&mut self, ui: &mut egui::Ui) {
        match self.selected_tab {
            AppTab::Device => self.device_tab(ui),
            AppTab::Status => {
                self.status_tab(ui);
            }
        }
    }

    fn device_tab(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                self.device_card(ui);
                ui.add_space(12.0);
                self.mode_card(ui);
            });

            columns[1].vertical(|ui| {
                self.sound_settings_card(ui);
                ui.add_space(12.0);
                self.level_card(ui);
            });
        });
    }

    fn status_tab(&self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                self.level_card(ui);
                ui.add_space(12.0);
                self.processing_card(ui);
            });

            columns[1].vertical(|ui| {
                self.buffer_card(ui);
                ui.add_space(12.0);
                self.status_card(ui);
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .fill(Color32::from_rgb(232, 232, 237))
            .corner_radius(18)
            .inner_margin(egui::Margin::same(4))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in AppTab::ALL {
                        let selected = self.selected_tab == tab;
                        let button = egui::Button::new(
                            RichText::new(tab_label(tab))
                                .size(14.0)
                                .strong()
                                .color(if selected { Color32::WHITE } else { ios_text() }),
                        )
                        .fill(if selected {
                            ios_blue()
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(Stroke::NONE)
                        .corner_radius(15)
                        .min_size(Vec2::new(116.0, 34.0));

                        if ui.add(button).clicked() {
                            self.selected_tab = tab;
                        }
                    }
                });
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("ClearLine")
                        .size(28.0)
                        .strong()
                        .color(ios_text()),
                );
                ui.add_space(1.0);
                ui.label(
                    RichText::new("本地实时麦克风降噪")
                        .size(14.0)
                        .color(ios_secondary_text()),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(build_info_label(BuildInfo::current()))
                        .size(12.0)
                        .color(ios_secondary_text()),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.noise_suppression_toggle(ui);
            });
        });
    }

    fn device_card(&mut self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_title(ui, "输入麦克风", "选择需要降噪的真实录音设备");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let devices = self.devices.clone();
                egui::ComboBox::from_id_salt("input-device-selector")
                    .width(ui.available_width() - 92.0)
                    .selected_text(self.selected_device_label())
                    .show_ui(ui, |ui| {
                        for device in devices {
                            let is_selected = self.selected_device_id.as_ref() == Some(device.id());
                            if ui
                                .selectable_label(is_selected, device_label(&device))
                                .clicked()
                            {
                                self.selected_device_id = Some(device.id().clone());
                                self.save_settings_after_user_change();
                                self.apply_runtime_change_if_running();
                            }
                        }
                    });

                let refresh_button = egui::Button::new(RichText::new("刷新").color(ios_blue()))
                    .fill(ios_control_fill())
                    .stroke(Stroke::NONE)
                    .corner_radius(12)
                    .min_size(Vec2::new(72.0, 30.0));
                if ui.add(refresh_button).clicked() {
                    self.refresh_devices();
                    self.apply_runtime_change_if_running();
                }
            });
        });
    }

    fn sound_settings_card(&mut self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_title(ui, "Windows 录音设备", "语音软件请选择 CABLE Output");
            ui.add_space(8.0);
            self.default_recording_device_prompt(ui);
        });
    }

    fn default_recording_device_prompt(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(default_recording_device_title())
                    .size(14.0)
                    .strong()
                    .color(ios_text()),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(default_recording_device_message())
                    .size(13.0)
                    .color(ios_secondary_text()),
            );
            ui.add_space(10.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(open_sound_settings_button_label()).color(ios_blue()),
                    )
                    .fill(ios_control_fill())
                    .stroke(Stroke::NONE)
                    .corner_radius(12)
                    .min_size(Vec2::new(132.0, 32.0)),
                )
                .clicked()
            {
                match windows_settings::open_sound_settings() {
                    Ok(()) => self.status_message = sound_settings_opened_status().to_owned(),
                    Err(error) => {
                        self.status_message = sound_settings_open_failed_status(error);
                    }
                }
            }
        });
    }

    fn mode_card(&mut self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            let is_running = self.pipeline.state().is_running();
            ui.set_min_width(ui.available_width());
            section_title(ui, "降噪强度", mode_card_subtitle());
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                for strength in SuppressionStrength::ALL {
                    let selected = self.suppression_strength == strength;
                    let button = egui::Button::new(
                        RichText::new(strength_button_label(strength))
                            .size(14.0)
                            .color(if selected { Color32::WHITE } else { ios_text() }),
                    )
                    .fill(if selected {
                        ios_blue()
                    } else {
                        ios_control_fill()
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(18)
                    .min_size(Vec2::new(82.0, 34.0));

                    if ui.add(button).clicked() {
                        self.suppression_strength = strength;
                        self.save_settings_after_user_change();
                        self.apply_runtime_change_if_running();
                    }
                }
            });

            ui.add_space(12.0);
            self.auxiliary_toggles_row(ui, is_running);
        });
    }

    fn noise_suppression_toggle(&mut self, ui: &mut egui::Ui) {
        let selected = self.noise_suppression_enabled;
        let button = egui::Button::new(
            RichText::new(noise_suppression_config_label(selected))
                .size(14.0)
                .color(if selected { Color32::WHITE } else { ios_text() }),
        )
        .fill(if selected {
            Color32::from_rgb(52, 199, 89)
        } else {
            ios_control_fill()
        })
        .stroke(Stroke::NONE)
        .corner_radius(18)
        .min_size(Vec2::new(132.0, 36.0));

        if ui.add(button).clicked() {
            self.set_noise_suppression_enabled(!self.noise_suppression_enabled);
        }
    }

    fn auxiliary_toggles_row(&mut self, ui: &mut egui::Ui, is_running: bool) {
        let _ = is_running;
        ui.horizontal(|ui| {
            {
                let selected = self.wind_noise_reduction_enabled;
                let button = egui::Button::new(
                    RichText::new(wind_reduction_config_label(selected))
                        .size(14.0)
                        .color(if selected { Color32::WHITE } else { ios_text() }),
                )
                .fill(if selected {
                    Color32::from_rgb(52, 199, 89)
                } else {
                    ios_control_fill()
                })
                .stroke(Stroke::NONE)
                .corner_radius(18)
                .min_size(Vec2::new(150.0, 36.0));

                if ui
                    .add_enabled(self.noise_suppression_enabled, button)
                    .clicked()
                {
                    self.set_wind_noise_reduction_enabled(!self.wind_noise_reduction_enabled);
                }
            }

            ui.add_space(8.0);

            {
                let selected = self.echo_cancellation_enabled;
                let button = egui::Button::new(
                    RichText::new(echo_cancellation_config_label(selected))
                        .size(14.0)
                        .color(if selected { Color32::WHITE } else { ios_text() }),
                )
                .fill(if selected {
                    Color32::from_rgb(52, 199, 89)
                } else {
                    ios_control_fill()
                })
                .stroke(Stroke::NONE)
                .corner_radius(18)
                .min_size(Vec2::new(132.0, 36.0));

                if ui
                    .add_enabled(self.noise_suppression_enabled, button)
                    .clicked()
                {
                    self.set_echo_cancellation_enabled(!self.echo_cancellation_enabled);
                }
            }
        });
    }

    fn level_card(&self, ui: &mut egui::Ui) {
        card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_title(ui, "输入电平", "音频流接入后这里会显示实时电平");
            ui.add_space(10.0);

            ios_progress_bar(ui, self.input_level);
        });
    }

    fn buffer_card(&self, ui: &mut egui::Ui) {
        let metrics = self.pipeline.metrics();
        let runtime_info = self.pipeline.runtime_info();

        card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_title(
                ui,
                "延迟与缓冲状态",
                "用于观察输出是否稳定，定位断续、溢出和延迟问题",
            );
            ui.add_space(10.0);
            ios_progress_bar(ui, metrics.fill_ratio());
            ui.add_space(10.0);
            info_row(ui, "水位", buffer_fill_text(metrics));
            ui.add_space(6.0);
            info_row(ui, "缓冲延迟", buffer_latency_label(metrics, runtime_info));
            ui.add_space(6.0);
            info_row(ui, "算法帧", algorithm_latency_label(runtime_info));
            ui.add_space(6.0);
            info_row(
                ui,
                "健康",
                buffer_health_label(
                    metrics.underrun_sample_count(),
                    metrics.dropped_sample_count(),
                ),
            );
            ui.add_space(6.0);
            info_row(ui, "建议", buffer_diagnostic_label(metrics, runtime_info));
            ui.add_space(6.0);
            info_row(ui, "欠载样本", metrics.underrun_sample_count().to_string());
            ui.add_space(6.0);
            info_row(ui, "溢出丢弃", metrics.dropped_sample_count().to_string());
        });
    }

    fn processing_card(&self, ui: &mut egui::Ui) {
        let runtime_info = self.pipeline.runtime_info();
        let echo_reference = self.pipeline.echo_reference_diagnostics();

        card_frame().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            section_title(
                ui,
                "处理链路",
                "确认当前算法后端、格式和 AEC 参考是否真正生效",
            );
            ui.add_space(10.0);
            info_row(ui, "后端", backend_name_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "降噪", noise_suppression_status_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "强度", suppression_strength_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "抗风噪", wind_reduction_runtime_label(runtime_info));
            ui.add_space(6.0);
            info_row(
                ui,
                echo_cancellation_title(),
                echo_cancellation_runtime_label(runtime_info),
            );
            ui.add_space(6.0);
            info_row(
                ui,
                "参考音频",
                echo_reference_level_label(runtime_info, echo_reference),
            );
            ui.add_space(6.0);
            info_row(
                ui,
                "参考状态",
                echo_reference_health_label(runtime_info, echo_reference),
            );
            ui.add_space(6.0);
            info_row(
                ui,
                "DeepFilterNet",
                packaged_deepfilter_model_status_label(),
            );
            ui.add_space(6.0);
            info_row(ui, "版本", build_info_label(BuildInfo::current()));
            ui.add_space(6.0);
            info_row(ui, "输入格式", input_format_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "输出格式", output_format_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "帧大小", frame_size_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "推理状态", inference_health_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "推理延迟", inference_latency_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "推理队列", inference_queue_label(runtime_info));
            ui.add_space(6.0);
            info_row(ui, "推理丢帧", inference_drop_label(runtime_info));
        });
    }

    fn status_card(&self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .fill(Color32::from_rgb(248, 248, 250))
            .corner_radius(16)
            .stroke(Stroke::new(1.0, ios_separator()))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("状态")
                            .size(13.0)
                            .strong()
                            .color(ios_secondary_text()),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            pipeline_state_label(self.pipeline.state()),
                            self.status_message
                        ))
                        .size(14.0)
                        .color(ios_text()),
                    );
                });
            });
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = load_first_existing_font(chinese_font_candidates()) {
        fonts.font_data.insert(
            "clearline-cjk".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );

        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "clearline-cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

fn load_first_existing_font(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|path| fs::read(path).ok())
}

fn chinese_font_candidates() -> &'static [&'static str] {
    &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        "/mnt/c/Windows/Fonts/msyh.ttc",
        "/mnt/c/Windows/Fonts/simhei.ttf",
        "/mnt/c/Windows/Fonts/simsun.ttc",
    ]
}

fn configure_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut style = (*ctx.style_of(egui::Theme::Light)).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = ios_background();
    style.visuals.window_fill = ios_card();
    style.visuals.faint_bg_color = ios_control_fill();
    style.visuals.extreme_bg_color = Color32::WHITE;
    style.visuals.hyperlink_color = ios_blue();
    style.visuals.selection.bg_fill = Color32::from_rgb(210, 232, 255);
    style.visuals.selection.stroke = Stroke::new(1.0, ios_blue());
    style.visuals.widgets.inactive.corner_radius = 12.into();
    style.visuals.widgets.hovered.corner_radius = 12.into();
    style.visuals.widgets.active.corner_radius = 12.into();
    style.visuals.widgets.inactive.bg_fill = ios_control_fill();
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(234, 234, 238);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(224, 224, 230);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, ios_separator());
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(210, 210, 216));
    style.spacing.item_spacing = Vec2::new(9.0, 6.0);
    style.spacing.button_padding = Vec2::new(13.0, 7.0);
    style.spacing.combo_width = 280.0;
    ctx.set_style_of(egui::Theme::Light, style);
}

fn card_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(ios_card())
        .corner_radius(22)
        .stroke(Stroke::new(1.0, ios_separator()))
        .inner_margin(egui::Margin::same(14))
}

fn section_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(16.0).strong().color(ios_text()));
    if !subtitle.is_empty() {
        ui.add_space(2.0);
        ui.label(
            RichText::new(subtitle)
                .size(13.0)
                .color(ios_secondary_text()),
        );
    }
}

fn info_row(ui: &mut egui::Ui, label: &str, value: impl AsRef<str>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(13.0)
                .strong()
                .color(ios_secondary_text()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value.as_ref()).size(14.0).color(ios_text()));
        });
    });
}

fn buffer_fill_text(metrics: PipelineMetrics) -> String {
    if metrics.capacity_samples() == 0 {
        "未运行".to_owned()
    } else {
        format!(
            "{} / {}（{:.0}%）",
            metrics.buffered_samples(),
            metrics.capacity_samples(),
            metrics.fill_ratio() * 100.0
        )
    }
}

fn buffer_health_label(underrun_sample_count: u64, dropped_sample_count: u64) -> &'static str {
    match (underrun_sample_count > 0, dropped_sample_count > 0) {
        (true, _) => "曾发生欠载",
        (false, true) => "曾发生溢出",
        (false, false) => "稳定",
    }
}

fn buffer_latency_label(
    metrics: PipelineMetrics,
    runtime_info: Option<&PipelineRuntimeInfo>,
) -> String {
    let Some(info) = runtime_info else {
        return "未运行".to_owned();
    };
    if metrics.capacity_samples() == 0 {
        return "未运行".to_owned();
    }

    let output_format = info.output_format();
    let current_ms = metrics.buffered_latency_ms(output_format).unwrap_or(0);
    let Some(capacity_ms) = metrics.capacity_latency_ms(output_format) else {
        return "未运行".to_owned();
    };

    format!("约 {current_ms}ms / 上限 {capacity_ms}ms")
}

fn algorithm_latency_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(info) = runtime_info else {
        return "未连接音频流".to_owned();
    };

    let suppressor = info.suppressor();
    let frame_size_samples = suppressor.frame_size_samples();
    if frame_size_samples == 0 {
        return "无固定算法帧".to_owned();
    }

    let frame_metrics = PipelineMetrics::new(frame_size_samples, frame_size_samples, 0, 0);
    let Some(latency_ms) = frame_metrics.buffered_latency_ms(info.input_format()) else {
        return "无固定算法帧".to_owned();
    };

    format!("约 {latency_ms}ms（{}）", mode_label(suppressor.mode()))
}

fn buffer_diagnostic_label(
    metrics: PipelineMetrics,
    runtime_info: Option<&PipelineRuntimeInfo>,
) -> String {
    let Some(info) = runtime_info else {
        return "启动后显示诊断".to_owned();
    };
    if metrics.capacity_samples() == 0 {
        return "启动后显示诊断".to_owned();
    }
    if metrics.underrun_sample_count() > 0 {
        return "曾发生欠载：可能听到断续".to_owned();
    }
    if metrics.dropped_sample_count() > 0 {
        return "曾发生溢出：输入一度快于输出".to_owned();
    }

    let current_ms = metrics
        .buffered_latency_ms(info.output_format())
        .unwrap_or(0);
    let algorithm_ms = algorithm_latency_ms_value(info).unwrap_or(0);

    if current_ms >= 250 {
        "缓冲偏高：延迟可能明显".to_owned()
    } else if algorithm_ms > 0 && current_ms < algorithm_ms {
        "缓冲较低：延迟低，注意观察欠载".to_owned()
    } else {
        "稳定：延迟和缓冲正常".to_owned()
    }
}

fn algorithm_latency_ms_value(info: &PipelineRuntimeInfo) -> Option<u32> {
    let frame_size_samples = info.suppressor().frame_size_samples();
    if frame_size_samples == 0 {
        return None;
    }

    PipelineMetrics::new(frame_size_samples, frame_size_samples, 0, 0)
        .buffered_latency_ms(info.input_format())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuildInfo {
    version: &'static str,
    profile: &'static str,
    commit: &'static str,
}

impl BuildInfo {
    fn current() -> Self {
        Self::new(
            env!("CARGO_PKG_VERSION"),
            build_profile(),
            option_env!("CLEARLINE_GIT_COMMIT").unwrap_or("unknown"),
        )
    }

    fn new(version: &'static str, profile: &'static str, commit: &'static str) -> Self {
        Self {
            version,
            profile,
            commit,
        }
    }

    fn normalized_commit(self) -> &'static str {
        if self.commit.trim().is_empty() {
            "unknown"
        } else {
            self.commit
        }
    }
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn build_info_label(info: BuildInfo) -> String {
    format!(
        "v{} · {} · {}",
        info.version,
        info.profile,
        info.normalized_commit()
    )
}

fn wind_reduction_config_label(enabled: bool) -> &'static str {
    if enabled {
        "抗风噪增强：开启"
    } else {
        "抗风噪增强：关闭"
    }
}

fn wind_reduction_runtime_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    runtime_info
        .map(|info| {
            if info.wind_noise_reduction_enabled() {
                "已启用".to_owned()
            } else {
                "未启用".to_owned()
            }
        })
        .unwrap_or_else(|| "未连接音频流".to_owned())
}

fn echo_cancellation_title() -> &'static str {
    "回音消除"
}

fn echo_cancellation_config_label(enabled: bool) -> &'static str {
    if enabled {
        "回音消除：开启"
    } else {
        "回音消除：关闭"
    }
}

fn echo_cancellation_runtime_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(info) = runtime_info else {
        return "未连接音频流".to_owned();
    };

    match info.echo_cancellation().backend() {
        EchoCancellerBackend::Disabled => "未启用".to_owned(),
        EchoCancellerBackend::Aec3 => "已启用（AEC3）".to_owned(),
    }
}

fn echo_cancellation_is_active(runtime_info: Option<&PipelineRuntimeInfo>) -> Option<bool> {
    runtime_info.map(|info| info.echo_cancellation().backend() != EchoCancellerBackend::Disabled)
}

fn echo_reference_level_label(
    runtime_info: Option<&PipelineRuntimeInfo>,
    diagnostics: Option<EchoReferenceDiagnostics>,
) -> String {
    match echo_cancellation_is_active(runtime_info) {
        None => "未连接音频流".to_owned(),
        Some(false) => "未启用".to_owned(),
        Some(true) => diagnostics
            .map(|diagnostics| {
                format!(
                    "{:.0}%（缓冲 {} samples）",
                    diagnostics.level() * 100.0,
                    diagnostics.buffered_samples()
                )
            })
            .unwrap_or_else(|| "未连接参考音频".to_owned()),
    }
}

fn echo_reference_health_label(
    runtime_info: Option<&PipelineRuntimeInfo>,
    diagnostics: Option<EchoReferenceDiagnostics>,
) -> String {
    match echo_cancellation_is_active(runtime_info) {
        None => "未连接音频流".to_owned(),
        Some(false) => "未启用".to_owned(),
        Some(true) => {
            let Some(diagnostics) = diagnostics else {
                return "未连接参考音频".to_owned();
            };
            if diagnostics.missing_frames() == 0 && diagnostics.dropped_samples() == 0 {
                "稳定".to_owned()
            } else {
                format!(
                    "缺帧 {} · 丢弃 {}",
                    diagnostics.missing_frames(),
                    diagnostics.dropped_samples()
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeepFilterModelUiStatus {
    NotConfigured,
    Valid,
    MissingAsset(String),
    Invalid(String),
}

fn packaged_deepfilter_model_dir() -> Option<PathBuf> {
    let env_value = env::var_os(DEEPFILTER_MODEL_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());
    if env_value.is_some() {
        return env_value;
    }

    env::current_exe()
        .ok()
        .map(|exe_path| packaged_deepfilter_model_dir_for_exe(&exe_path))
}

fn packaged_deepfilter_model_dir_for_exe(exe_path: &Path) -> PathBuf {
    exe_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(PACKAGED_DEEPFILTER_MODEL_DIR)
}

fn packaged_deepfilter_model_status() -> DeepFilterModelUiStatus {
    packaged_deepfilter_model_dir()
        .map(|path| deepfilter_model_status(&path))
        .unwrap_or(DeepFilterModelUiStatus::NotConfigured)
}

fn packaged_deepfilter_model_status_label() -> String {
    match packaged_deepfilter_model_status() {
        DeepFilterModelUiStatus::NotConfigured => {
            "未找到随程序打包的 DeepFilterNet 模型".to_owned()
        }
        status => deepfilter_model_status_label(status),
    }
}

fn deepfilter_model_status(path: impl AsRef<Path>) -> DeepFilterModelUiStatus {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return DeepFilterModelUiStatus::NotConfigured;
    }

    match DeepFilterNetModelBundle::from_dir(path) {
        Ok(_) => DeepFilterModelUiStatus::Valid,
        Err(ClearLineError::ModelAssetMissing { path }) => {
            DeepFilterModelUiStatus::MissingAsset(model_asset_filename(Path::new(&path)))
        }
        Err(error) => DeepFilterModelUiStatus::Invalid(error.to_string()),
    }
}

fn deepfilter_model_status_label(status: DeepFilterModelUiStatus) -> String {
    match status {
        DeepFilterModelUiStatus::NotConfigured => "未找到模型目录".to_owned(),
        DeepFilterModelUiStatus::Valid => "打包模型可用：DeepFilterNet".to_owned(),
        DeepFilterModelUiStatus::MissingAsset(filename) => {
            format!("打包模型不完整：缺少 {filename}")
        }
        DeepFilterModelUiStatus::Invalid(message) => format!("打包模型无效：{message}"),
    }
}

fn deepfilter_model_bundle_for_pipeline() -> Option<DeepFilterNetModelBundle> {
    packaged_deepfilter_model_dir().and_then(|path| DeepFilterNetModelBundle::from_dir(path).ok())
}

fn deepfilter_startup_warning(
    mode: SuppressorMode,
    status: &DeepFilterModelUiStatus,
) -> Option<String> {
    if mode != SuppressorMode::HighQuality {
        return None;
    }

    match status {
        DeepFilterModelUiStatus::Valid => None,
        DeepFilterModelUiStatus::NotConfigured => {
            Some("DeepFilterNet 模型不可用：未找到随程序打包的模型".to_owned())
        }
        DeepFilterModelUiStatus::MissingAsset(filename) => {
            Some(format!("DeepFilterNet 模型不可用：缺少 {filename}"))
        }
        DeepFilterModelUiStatus::Invalid(message) => {
            Some(format!("DeepFilterNet 模型不可用：{message}"))
        }
    }
}

fn model_asset_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|filename| filename.to_str())
        .unwrap_or_else(|| path.to_str().unwrap_or("unknown"))
        .to_owned()
}

fn backend_name_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    runtime_info
        .map(|info| info.suppressor().backend_name().to_owned())
        .unwrap_or_else(|| "未连接音频流".to_owned())
}

fn noise_suppression_status_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(info) = runtime_info else {
        return "未连接音频流".to_owned();
    };

    let suppressor = info.suppressor();
    if suppressor.is_real_noise_suppression() {
        return match suppressor.backend_name() {
            "nnnoiseless-rnnoise" => "低延迟降噪（RNNoise）已启用".to_owned(),
            "adaptive-quality-v1" => "实验降噪后端已启用".to_owned(),
            "deepfilternet" | "deepfilternet-tract" | "deepfilternet-tract-worker" => {
                "高质量降噪（DeepFilterNet 模型）已启用".to_owned()
            }
            backend => format!("{backend} 已启用"),
        };
    }

    match suppressor.mode() {
        SuppressorMode::Bypass => "未启用降噪".to_owned(),
        SuppressorMode::LowLatency => "RNNoise 不支持当前格式，已使用安全回退".to_owned(),
        SuppressorMode::HighQuality => "高质量后端不可用，未启用降噪".to_owned(),
    }
}

fn input_format_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    runtime_info
        .map(|info| runtime_format_label(info.input_format()))
        .unwrap_or_else(|| "未连接音频流".to_owned())
}

fn output_format_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    runtime_info
        .map(|info| runtime_format_label(info.output_format()))
        .unwrap_or_else(|| "未连接音频流".to_owned())
}

fn runtime_format_label(format: AudioFrameFormat) -> String {
    format!(
        "{} Hz / {}",
        format.sample_rate_hz(),
        channel_count_label(format.channels())
    )
}

fn channel_count_label(channels: u16) -> String {
    format!("{} 声道", channels.max(1))
}

fn frame_size_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(info) = runtime_info else {
        return "未连接音频流".to_owned();
    };

    let frame_size_samples = info.suppressor().frame_size_samples();
    if frame_size_samples == 0 {
        "无固定帧".to_owned()
    } else {
        format!("{frame_size_samples} samples")
    }
}

fn inference_health_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics())
    else {
        return "未使用后台推理".to_owned();
    };

    if diagnostics.is_degraded() || diagnostics.inference_errors() > 0 {
        "已降级".to_owned()
    } else if diagnostics.late_output_frames() > 0
        || diagnostics.dropped_input_frames() > 0
        || diagnostics.dropped_output_frames() > 0
    {
        "推理偏慢".to_owned()
    } else {
        "稳定".to_owned()
    }
}

fn inference_latency_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics())
    else {
        return "未使用后台推理".to_owned();
    };

    match (
        diagnostics.last_inference_time_ms(),
        diagnostics.max_inference_time_ms(),
    ) {
        (Some(last), Some(max)) => format!("最近 {last}ms / 最大 {max}ms"),
        (Some(last), None) => format!("最近 {last}ms / 最大 --"),
        (None, Some(max)) => format!("最近 -- / 最大 {max}ms"),
        (None, None) => "等待首帧".to_owned(),
    }
}

fn inference_queue_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics())
    else {
        return "未使用后台推理".to_owned();
    };

    format!(
        "输入 {}/{} · 输出 {}/{}",
        diagnostics.pending_input_frames(),
        diagnostics.input_queue_capacity(),
        diagnostics.pending_output_frames(),
        diagnostics.output_queue_capacity()
    )
}

fn inference_drop_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(diagnostics) = runtime_info.and_then(|info| info.suppressor().worker_diagnostics())
    else {
        return "未使用后台推理".to_owned();
    };

    format!(
        "输入丢弃 {} · 输出丢弃 {} · 迟到 {}",
        diagnostics.dropped_input_frames(),
        diagnostics.dropped_output_frames(),
        diagnostics.late_output_frames()
    )
}

fn ios_progress_bar(ui: &mut egui::Ui, value: f32) {
    let desired_size = Vec2::new(ui.available_width(), 12.0);
    let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let radius = 6.0;
    let fill_width =
        (rect.width() * value.clamp(0.0, 1.0)).max(if value > 0.0 { 8.0 } else { 0.0 });
    let fill_rect = egui::Rect::from_min_size(rect.min, Vec2::new(fill_width, rect.height()));

    ui.painter()
        .rect_filled(rect, radius, Color32::from_rgb(229, 229, 234));
    if fill_width > 0.0 {
        ui.painter().rect_filled(fill_rect, radius, ios_blue());
    }
}

fn device_label(device: &AudioInputDevice) -> String {
    if device.is_default() {
        format!("{}（默认）", device.name())
    } else {
        device.name().to_owned()
    }
}

#[cfg(test)]
fn output_device_label(device: &AudioOutputDevice) -> String {
    if device.is_default() {
        format!("{}（默认）", device.name())
    } else {
        device.name().to_owned()
    }
}

fn is_vb_cable_render_device_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    (normalized.contains("cable input") || normalized.contains("cable in"))
        && !normalized.contains("cable-a")
        && !normalized.contains("cable-b")
        && !normalized.contains("cable-c")
        && !normalized.contains("cable-d")
}

fn default_recording_device_title() -> &'static str {
    "Windows 默认录音设备"
}

fn default_recording_device_message() -> &'static str {
    "如需让 Discord、微信、QQ、浏览器会议等应用使用降噪后的声音，请在 Windows 中把 CABLE Output 设为默认录音设备。"
}

fn open_sound_settings_button_label() -> &'static str {
    "打开声音设置"
}

fn sound_settings_opened_status() -> &'static str {
    "已打开 Windows 声音设置"
}

fn sound_settings_open_failed_status(error: impl std::fmt::Display) -> String {
    format!("打开 Windows 声音设置失败：{error}")
}

fn mode_card_subtitle() -> &'static str {
    ""
}

fn mode_label(mode: SuppressorMode) -> &'static str {
    match mode {
        SuppressorMode::Bypass => "无降噪",
        SuppressorMode::LowLatency => "低延迟降噪",
        SuppressorMode::HighQuality => "高质量降噪",
    }
}

fn noise_suppression_config_label(enabled: bool) -> &'static str {
    if enabled {
        "降噪：开启"
    } else {
        "降噪：关闭"
    }
}

fn selected_strength_label(strength: SuppressionStrength) -> &'static str {
    strength_button_label(strength)
}

fn strength_button_label(strength: SuppressionStrength) -> &'static str {
    match strength {
        SuppressionStrength::Gentle => "柔和",
        SuppressionStrength::Balanced => "标准",
        SuppressionStrength::Strong => "强力",
    }
}

fn suppression_strength_label(runtime_info: Option<&PipelineRuntimeInfo>) -> String {
    let Some(info) = runtime_info else {
        return "未连接音频流".to_owned();
    };

    if info.suppressor().mode() == SuppressorMode::Bypass {
        return "不适用".to_owned();
    }

    info.suppressor()
        .strength()
        .map(selected_strength_label)
        .unwrap_or("标准")
        .to_owned()
}

fn tab_label(tab: AppTab) -> &'static str {
    match tab {
        AppTab::Device => "设备",
        AppTab::Status => "状态",
    }
}

fn pipeline_state_label(state: &PipelineState) -> String {
    match state {
        PipelineState::Stopped => "已停止".to_owned(),
        PipelineState::Starting => "启动中".to_owned(),
        PipelineState::Running => "运行中".to_owned(),
        PipelineState::Error(message) => format!("错误：{message}"),
    }
}

fn ios_background() -> Color32 {
    Color32::from_rgb(242, 242, 247)
}

fn ios_card() -> Color32 {
    Color32::WHITE
}

fn ios_control_fill() -> Color32 {
    Color32::from_rgb(242, 242, 247)
}

fn ios_separator() -> Color32 {
    Color32::from_rgb(224, 224, 230)
}

fn ios_text() -> Color32 {
    Color32::from_rgb(28, 28, 30)
}

fn ios_secondary_text() -> Color32 {
    Color32::from_rgb(99, 99, 102)
}

fn ios_blue() -> Color32 {
    Color32::from_rgb(0, 122, 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localized_mode_labels_are_chinese() {
        assert_eq!(mode_label(SuppressorMode::Bypass), "无降噪");
        assert_eq!(mode_label(SuppressorMode::LowLatency), "低延迟降噪");
        assert_eq!(mode_label(SuppressorMode::HighQuality), "高质量降噪");
    }

    #[test]
    fn default_suppressor_mode_is_high_quality() {
        assert_eq!(default_suppressor_mode(), SuppressorMode::HighQuality);
    }

    #[test]
    fn restore_input_device_prefers_id_then_name_then_default() {
        let devices = vec![
            AudioInputDevice::new("default-id", "Default Mic", true),
            AudioInputDevice::new("saved-id", "Saved Mic", false),
            AudioInputDevice::new("name-id", "Name Mic", false),
        ];

        assert_eq!(
            resolve_input_device_from_settings(Some("saved-id"), Some("Name Mic"), &devices)
                .unwrap()
                .as_str(),
            "saved-id"
        );
        assert_eq!(
            resolve_input_device_from_settings(Some("missing-id"), Some("Name Mic"), &devices)
                .unwrap()
                .as_str(),
            "name-id"
        );
        assert_eq!(
            resolve_input_device_from_settings(Some("missing-id"), Some("Missing Mic"), &devices)
                .unwrap()
                .as_str(),
            "default-id"
        );
    }

    #[test]
    fn restore_output_device_ignores_non_cable_saved_device_when_vb_cable_exists() {
        let devices = vec![
            AudioOutputDevice::new("default-out", "Default Speaker", true),
            AudioOutputDevice::new("saved-out", "Saved Output", false),
            AudioOutputDevice::new("cable-out", "CABLE Input", false),
        ];

        assert_eq!(
            resolve_output_device_from_settings(Some("saved-out"), Some("Saved Output"), &devices)
                .unwrap()
                .as_str(),
            "cable-out"
        );
    }

    #[test]
    fn restore_output_device_returns_none_when_vb_cable_is_missing() {
        let devices = vec![
            AudioOutputDevice::new("default-out", "Default Speaker", true),
            AudioOutputDevice::new("saved-out", "Saved Output", false),
        ];

        assert_eq!(
            resolve_output_device_from_settings(Some("saved-out"), Some("Saved Output"), &devices),
            None
        );
    }

    #[test]
    fn restore_output_device_prefers_cable_input_when_no_saved_device() {
        let devices = vec![
            AudioOutputDevice::new("default-out", "Default Speakers", true),
            AudioOutputDevice::new("cable-out", "CABLE Input", false),
        ];

        assert_eq!(
            resolve_output_device_from_settings(None, None, &devices)
                .unwrap()
                .as_str(),
            "cable-out"
        );

        let devices = vec![
            AudioOutputDevice::new("default-out", "Default Speakers", true),
            AudioOutputDevice::new(
                "cable-out",
                "CABLE In 16 Ch (VB-Audio Virtual Cable)",
                false,
            ),
        ];

        assert_eq!(
            resolve_output_device_from_settings(None, None, &devices)
                .unwrap()
                .as_str(),
            "cable-out"
        );
    }

    #[test]
    fn fresh_output_selection_prefers_vb_cable_over_default_speakers() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.output_devices = vec![
            AudioOutputDevice::new("default-out", "Default Speakers", true),
            AudioOutputDevice::new("cable-out", "CABLE Input", false),
        ];

        app.ensure_selected_output_device();

        assert_eq!(
            app.selected_output_device_id.as_ref().unwrap().as_str(),
            "cable-out"
        );
    }

    #[test]
    fn output_selection_replaces_existing_non_cable_device_with_vb_cable() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.output_devices = vec![
            AudioOutputDevice::new("default-out", "Default Speakers", true),
            AudioOutputDevice::new(
                "cable-out",
                "CABLE In 16 Ch (VB-Audio Virtual Cable)",
                false,
            ),
        ];
        app.selected_output_device_id = Some(DeviceId::new("default-out"));

        app.ensure_selected_output_device();

        assert_eq!(
            app.selected_output_device_id.as_ref().unwrap().as_str(),
            "cable-out"
        );
    }

    #[test]
    fn output_selection_is_empty_when_vb_cable_is_missing() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.output_devices = vec![AudioOutputDevice::new(
            "default-out",
            "Default Speakers",
            true,
        )];

        app.ensure_selected_output_device();

        assert_eq!(app.selected_output_device_id, None);
    }

    #[test]
    fn app_settings_snapshot_includes_current_choices() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.devices = vec![AudioInputDevice::new("mic-id", "Saved Mic", true)];
        app.output_devices = vec![AudioOutputDevice::new("out-id", "Saved Output", true)];
        app.selected_device_id = Some(DeviceId::new("mic-id"));
        app.selected_output_device_id = Some(DeviceId::new("out-id"));
        app.suppressor_mode = SuppressorModeSelection(SuppressorMode::HighQuality);
        app.suppression_strength = clearline_core::SuppressionStrength::Strong;
        app.wind_noise_reduction_enabled = true;
        app.echo_cancellation_enabled = true;
        app.noise_suppression_enabled = false;
        app.start_on_login_enabled = true;

        let settings = app.persisted_settings_snapshot();

        assert_eq!(settings.input_device_id.as_deref(), Some("mic-id"));
        assert_eq!(settings.input_device_name.as_deref(), Some("Saved Mic"));
        assert_eq!(settings.output_device_id.as_deref(), Some("out-id"));
        assert_eq!(settings.output_device_name.as_deref(), Some("Saved Output"));
        assert_eq!(settings.suppressor_mode, settings::MODE_HIGH_QUALITY);
        assert_eq!(settings.suppression_strength, settings::STRENGTH_STRONG);
        assert!(settings.wind_noise_reduction_enabled);
        assert!(settings.echo_cancellation_enabled);
        assert!(!settings.noise_suppression_enabled);
        assert!(settings.start_on_login_enabled);
        assert!(
            settings.deepfilter_model_dir.is_empty(),
            "DeepFilterNet 模型目录由安装包提供，不再保存 UI 手填路径"
        );
    }

    #[test]
    fn disabling_noise_suppression_uses_bypass_pipeline_mode() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();

        assert_eq!(app.effective_suppressor_mode(), SuppressorMode::HighQuality);
        app.noise_suppression_enabled = false;
        assert_eq!(app.effective_suppressor_mode(), SuppressorMode::Bypass);
    }

    #[test]
    fn header_only_shows_noise_suppression_control() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let header_source = production_source
            .split("fn header")
            .nth(1)
            .and_then(|rest| rest.split("fn device_card").next())
            .expect("header section exists");

        assert!(production_source.contains("start_pipeline_if_ready"));
        assert!(production_source.contains("app.start_pipeline_if_ready();"));
        assert!(!header_source.contains("top_control_switch"));
        assert!(header_source.contains("self.noise_suppression_toggle(ui);"));
        assert!(!header_source.contains("self.startup_toggle(ui);"));
        assert!(!header_source.contains("self.exit_button(ui);"));
    }

    #[test]
    fn tray_startup_setting_uses_background_task_to_avoid_ui_stall() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let startup_change = production_source
            .split("fn begin_start_on_login_change")
            .nth(1)
            .and_then(|rest| rest.split("fn request_exit").next())
            .expect("startup change function exists");

        assert!(startup_change.contains("std::thread::spawn"));
        assert!(startup_change.contains("pending_start_on_login_enabled"));
        assert!(startup_change.contains("start_on_login_task"));
        assert!(production_source.contains("poll_start_on_login_task"));
    }

    #[test]
    fn wind_and_echo_effects_are_gated_by_noise_suppression() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");

        assert!(production_source.contains("fn effective_wind_noise_reduction_enabled"));
        assert!(production_source.contains("fn effective_echo_cancellation_enabled"));
        assert!(production_source
            .contains(".with_wind_noise_reduction(self.effective_wind_noise_reduction_enabled())"));
        assert!(production_source
            .contains(".with_echo_cancellation(self.effective_echo_cancellation_enabled())"));
    }

    #[test]
    fn tray_sources_include_context_menu_quick_settings_and_exit() {
        let tray_source = include_str!("tray.rs");

        for marker in [
            "TrayEvent",
            "TrayMenuState",
            "CreatePopupMenu",
            "TrackPopupMenu",
            "MF_CHECKED",
            "append_checked_menu_item",
            "ToggleNoiseSuppression",
            "ToggleStartOnLogin",
            "ToggleWindNoiseReduction",
            "ToggleEchoCancellation",
            "Exit",
            "退出 ClearLine",
        ] {
            assert!(tray_source.contains(marker), "tray.rs missing {marker}");
        }
    }

    #[test]
    fn app_updates_tray_menu_state_from_current_settings() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let update_source = production_source
            .split("fn update_tray_menu_state")
            .nth(1)
            .and_then(|rest| rest.split("fn poll_start_on_login_task").next())
            .expect("update_tray_menu_state exists");

        assert!(production_source.contains("self.update_tray_menu_state();"));
        assert!(update_source.contains("noise_suppression_enabled: self.noise_suppression_enabled"));
        assert!(update_source.contains("pending_start_on_login_enabled"));
        assert!(update_source
            .contains("wind_noise_reduction_enabled: self.wind_noise_reduction_enabled"));
        assert!(update_source.contains("echo_cancellation_enabled: self.echo_cancellation_enabled"));
    }

    #[test]
    fn runtime_control_changes_request_pipeline_restart_when_running() {
        let source = include_str!("main.rs");
        let device_card = source
            .split("fn device_card")
            .nth(1)
            .and_then(|rest| rest.split("fn sound_settings_card").next())
            .expect("device_card section exists");
        let mode_card = source
            .split("fn mode_card")
            .nth(1)
            .and_then(|rest| rest.split("fn level_card").next())
            .expect("mode_card section exists");

        assert!(!device_card.contains("add_enabled_ui(!is_running"));
        assert!(!device_card.contains("add_enabled(!is_running"));
        assert!(device_card.contains("self.apply_runtime_change_if_running();"));
        assert!(mode_card.contains("self.apply_runtime_change_if_running();"));
    }

    #[test]
    fn app_sources_route_startup_and_exit_through_tray_not_main_window() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");

        assert!(source.contains("mod tray;"));
        assert!(source.contains("handle_tray_events"));
        assert!(source.contains("close_requested()"));
        assert!(source.contains("ViewportCommand::CancelClose"));
        assert!(source.contains("ViewportCommand::Visible(false)"));
        assert!(source.contains("start_on_login_enabled"));
        assert!(!production_source.contains("fn startup_toggle"));
        assert!(!production_source.contains("fn exit_button"));
        assert!(!production_source.contains("exit_app_button_label"));
    }

    #[test]
    fn app_applies_loaded_settings_to_fields_and_devices() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.devices = vec![
            AudioInputDevice::new("default-mic", "Default Mic", true),
            AudioInputDevice::new("saved-mic", "Saved Mic", false),
        ];
        app.output_devices = vec![
            AudioOutputDevice::new("default-out", "Default Output", true),
            AudioOutputDevice::new("saved-out", "Saved Output", false),
            AudioOutputDevice::new("cable-out", "CABLE Input", false),
        ];
        app.pending_settings = Some(PersistedSettings {
            input_device_id: Some("saved-mic".to_owned()),
            input_device_name: Some("Saved Mic".to_owned()),
            output_device_id: Some("saved-out".to_owned()),
            output_device_name: Some("Saved Output".to_owned()),
            suppressor_mode: settings::MODE_HIGH_QUALITY.to_owned(),
            suppression_strength: settings::STRENGTH_GENTLE.to_owned(),
            wind_noise_reduction_enabled: true,
            echo_cancellation_enabled: true,
            deepfilter_model_dir: r"E:\Dev\模型onnx".to_owned(),
            ..PersistedSettings::default()
        });

        app.apply_pending_settings_after_refresh();

        assert_eq!(
            app.selected_device_id.as_ref().unwrap().as_str(),
            "saved-mic"
        );
        assert_eq!(
            app.selected_output_device_id.as_ref().unwrap().as_str(),
            "cable-out"
        );
        assert_eq!(app.suppressor_mode.value(), SuppressorMode::HighQuality);
        assert_eq!(
            app.suppression_strength,
            clearline_core::SuppressionStrength::Gentle
        );
        assert!(app.wind_noise_reduction_enabled);
        assert!(app.echo_cancellation_enabled);
        assert!(app.pending_settings.is_none());
    }

    #[test]
    fn app_migrates_legacy_settings_to_high_quality_and_aec() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.devices = vec![AudioInputDevice::new("mic-1", "Mic One", true)];
        app.output_devices = vec![AudioOutputDevice::new("out-1", "Out One", true)];
        app.pending_settings = Some(PersistedSettings {
            version: settings::SETTINGS_VERSION - 1,
            input_device_id: Some("mic-1".to_owned()),
            input_device_name: Some("Mic One".to_owned()),
            output_device_id: Some("out-1".to_owned()),
            output_device_name: Some("Out One".to_owned()),
            output_target: settings::OUTPUT_TARGET_VIRTUAL_MIC.to_owned(),
            suppressor_mode: settings::MODE_LOW_LATENCY.to_owned(),
            suppression_strength: settings::STRENGTH_BALANCED.to_owned(),
            wind_noise_reduction_enabled: false,
            echo_cancellation_enabled: false,
            noise_suppression_enabled: true,
            start_on_login_enabled: false,
            deepfilter_model_dir: String::new(),
        });

        app.apply_pending_settings_after_refresh();

        assert_eq!(app.suppressor_mode.value(), SuppressorMode::HighQuality);
        assert!(app.echo_cancellation_enabled);
    }

    #[test]
    fn save_settings_writes_current_snapshot() {
        let path = unique_temp_settings_path("app-save");
        let store = SettingsStore::from_path(path.clone());
        let mut app = ClearLineApp::new_with_settings_for_tests(
            Some(store),
            None,
            "正在初始化设备列表".to_owned(),
            false,
        );
        app.devices = vec![AudioInputDevice::new("mic-id", "Saved Mic", true)];
        app.output_devices = vec![AudioOutputDevice::new("out-id", "Saved Output", true)];
        app.selected_device_id = Some(DeviceId::new("mic-id"));
        app.selected_output_device_id = Some(DeviceId::new("out-id"));

        app.save_settings_after_user_change();

        let loaded = SettingsStore::from_path(path).load().unwrap().unwrap();
        assert_eq!(loaded.input_device_id.as_deref(), Some("mic-id"));
        assert_eq!(loaded.output_device_id.as_deref(), Some("out-id"));
        assert!(loaded.deepfilter_model_dir.is_empty());
    }

    #[test]
    fn output_device_label_marks_default_device_in_chinese() {
        let device = AudioOutputDevice::new("out-1", "VB-CABLE Input", true);

        assert_eq!(output_device_label(&device), "VB-CABLE Input（默认）");
    }

    #[test]
    fn default_recording_device_prompt_only_opens_sound_settings() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");

        assert_eq!(default_recording_device_title(), "Windows 默认录音设备");
        assert!(default_recording_device_message().contains("默认录音设备"));
        assert!(default_recording_device_message().contains("CABLE Output"));
        assert_eq!(open_sound_settings_button_label(), "打开声音设置");
        assert!(!production_source.contains("set_vb_cable_output_as_default_microphone"));
        assert!(!production_source.contains("set_default_microphone_button_label"));
    }

    #[test]
    fn sound_settings_status_messages_are_chinese() {
        assert_eq!(sound_settings_opened_status(), "已打开 Windows 声音设置");
        assert_eq!(
            sound_settings_open_failed_status("仅 Windows 可用"),
            "打开 Windows 声音设置失败：仅 Windows 可用"
        );
    }

    #[test]
    fn production_ui_omits_extra_runtime_lock_copy() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let removed_helper = ["running", "_lock", "_hint()"].concat();

        assert!(!production_source.contains(&removed_helper));
    }

    #[test]
    fn runtime_sensitive_controls_apply_live_in_source() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");

        assert!(!production_source.contains("ui.add_enabled_ui(!is_running, |ui|"));
        assert!(!production_source.contains("ui.add_enabled(!is_running, button)"));
        assert!(production_source.contains("apply_runtime_change_if_running"));
        assert!(!production_source.contains("TextEdit::singleline"));
    }

    #[test]
    fn source_omits_removed_runtime_hint_copy() {
        let source = include_str!("main.rs");
        let removed_hint = String::from_utf8(vec![
            232, 191, 144, 232, 161, 140, 228, 184, 173, 233, 156, 128, 229, 129, 156, 230, 173,
            162, 229, 144, 142, 228, 191, 174, 230, 148, 185,
        ])
        .expect("valid UTF-8 test fixture");

        assert!(!source.contains(&removed_hint));
    }

    #[test]
    fn mode_card_omits_extra_toggle_descriptions() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");

        for removed_copy in [
            "强风噪、喷麦时可开启",
            "使用系统默认播放设备作为参考",
            "低延迟和高质量均生效",
        ] {
            assert!(
                !production_source.contains(removed_copy),
                "设备页不应再显示额外说明文案：{removed_copy}"
            );
        }
    }

    #[test]
    fn wind_and_echo_toggles_share_one_compact_row() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let row_source = production_source
            .split("fn auxiliary_toggles_row")
            .nth(1)
            .and_then(|source| source.split("fn level_card").next())
            .expect("auxiliary toggle row should exist");

        assert!(row_source.contains("wind_reduction_config_label(selected)"));
        assert!(row_source.contains("echo_cancellation_config_label(selected)"));
        assert_eq!(row_source.matches("ui.horizontal(|ui|").count(), 1);
    }

    #[test]
    fn localized_pipeline_state_labels_are_chinese() {
        assert_eq!(pipeline_state_label(&PipelineState::Stopped), "已停止");
        assert_eq!(pipeline_state_label(&PipelineState::Starting), "启动中");
        assert_eq!(pipeline_state_label(&PipelineState::Running), "运行中");
        assert_eq!(
            pipeline_state_label(&PipelineState::Error("没有输入设备".to_owned())),
            "错误：没有输入设备"
        );
    }

    #[test]
    fn app_tab_labels_are_chinese() {
        assert_eq!(tab_label(AppTab::Device), "设备");
        assert_eq!(tab_label(AppTab::Status), "状态");
    }

    #[test]
    fn visible_tabs_are_device_and_status_only() {
        assert_eq!(AppTab::ALL.as_slice(), &[AppTab::Device, AppTab::Status]);
    }

    #[test]
    fn default_tab_is_device_page() {
        assert_eq!(default_app_tab(), AppTab::Device);
    }

    #[test]
    fn window_sizes_match_desktop_layout_defaults() {
        assert_eq!(initial_window_size(), [1085.0, 580.0]);
        assert_eq!(minimum_window_size(), [1085.0, 580.0]);
    }

    #[test]
    fn app_auto_start_requires_input_and_vb_cable_output() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let auto_start = production_source
            .split("fn start_pipeline_if_ready")
            .nth(1)
            .and_then(|rest| rest.split("fn apply_runtime_change_if_running").next())
            .expect("start_pipeline_if_ready exists");

        assert!(auto_start.contains("selected_device_id.is_some()"));
        assert!(auto_start.contains("selected_output_device_id.is_some()"));
        assert!(auto_start.contains("self.start_pipeline();"));
    }

    #[test]
    fn device_page_does_not_expose_output_target_choice() {
        let source = include_str!("main.rs");
        let output_target_all = ["OutputTargetSelection", "::ALL"].concat();
        let legacy_output_label = ["播放设备", " / 虚拟音频线"].concat();
        let legacy_output_selector = ["output-device", "-selector"].concat();

        assert!(!source.contains(&output_target_all));
        assert!(!source.contains(&legacy_output_label));
        assert!(!source.contains(&legacy_output_selector));
    }

    #[test]
    fn app_start_source_uses_vb_cable_output_device() {
        let source = include_str!("main.rs");
        let expected_config_builder = [
            "AudioPipelineConfig::new",
            "(device_id, output_device_id, mode)",
        ]
        .concat();
        let legacy_virtual_builder = ["AudioPipelineConfig::for_", "virtual_microphone"].concat();
        let legacy_device_name = ["ClearLine", " Virtual Microphone"].concat();

        assert!(source.contains(&expected_config_builder));
        assert!(!source.contains(&legacy_virtual_builder));
        assert!(!source.contains(&legacy_device_name));
    }

    #[test]
    fn app_start_rejects_non_vb_cable_output_even_if_selected_id_exists() {
        let mut app = ClearLineApp::new_without_loading_settings_for_tests();
        app.devices = vec![AudioInputDevice::new("mic-id", "Mic", true)];
        app.output_devices = vec![AudioOutputDevice::new("speaker-id", "Speakers", true)];
        app.selected_device_id = Some(DeviceId::new("mic-id"));
        app.selected_output_device_id = Some(DeviceId::new("speaker-id"));
        app.noise_suppression_enabled = false;

        app.start_pipeline();

        assert!(matches!(app.pipeline.state(), PipelineState::Error(_)));
        assert!(app.status_message.contains("未找到 VB-CABLE 输出端点"));
    }

    #[test]
    fn app_start_source_passes_echo_cancellation_to_pipeline() {
        let source = include_str!("main.rs");

        assert!(
            source.contains(".with_echo_cancellation(self.effective_echo_cancellation_enabled())")
        );
    }

    #[test]
    fn top_header_and_tabs_are_outside_scroll_area() {
        let source = include_str!("main.rs");
        let header_pos = source.find("self.header(ui);").expect("header call exists");
        let tab_pos = source.find("self.tab_bar(ui);").expect("tab call exists");
        let scroll_pos = source
            .find("egui::ScrollArea::vertical()")
            .expect("scroll area exists");

        assert!(header_pos < scroll_pos);
        assert!(tab_pos < scroll_pos);
    }

    #[test]
    fn device_tab_keeps_virtual_microphone_guidance_compact() {
        let source = include_str!("main.rs");
        let device_tab = source
            .split("fn device_tab")
            .nth(1)
            .and_then(|rest| rest.split("fn status_tab").next())
            .expect("device_tab section exists");

        assert!(device_tab.contains("self.sound_settings_card(ui);"));
        assert!(!device_tab.contains("self.virtual_microphone_card(ui);"));
        assert!(!device_tab.contains("self.status_card(ui);"));
    }

    #[test]
    fn device_tab_does_not_expose_manual_model_path_controls() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");
        let manual_model_label = ["DeepFilterNet", " 模型目录"].concat();
        let text_edit_symbol =
            ["TextEdit", "::singleline(&mut self.deepfilter_model_dir)"].concat();

        assert!(!production_source.contains(&manual_model_label));
        assert!(!production_source.contains(&text_edit_symbol));
        assert!(!production_source.contains("C:\\\\Models\\\\DeepFilterNet"));
    }

    #[test]
    fn mode_card_title_is_compact() {
        assert_eq!(mode_card_subtitle(), "");
    }

    #[test]
    fn mode_card_only_shows_strength_not_mode_choice() {
        let source = include_str!("main.rs");
        let mode_card = source
            .split("fn mode_card")
            .nth(1)
            .and_then(|rest| rest.split("fn auxiliary_toggles_row").next())
            .expect("mode_card section exists");

        assert!(mode_card.contains("降噪强度"));
        assert!(mode_card.contains("for strength in SuppressionStrength::ALL"));
        assert!(!mode_card.contains("降噪模式"));
        assert!(!mode_card.contains("user_selectable_suppressor_modes"));
        assert!(!mode_card.contains("mode_label(mode)"));
        assert!(!mode_card.contains("self.suppressor_mode ="));
    }

    #[test]
    fn sound_settings_guidance_uses_stacked_button_layout() {
        let source = include_str!("main.rs");
        let prompt = source
            .split("fn default_recording_device_prompt")
            .nth(1)
            .and_then(|rest| rest.split("fn mode_card").next())
            .expect("default_recording_device_prompt section exists");

        assert!(prompt.contains("ui.vertical(|ui|"));
        assert!(!prompt.contains("right_to_left"));
    }

    #[test]
    fn buffer_health_labels_are_chinese() {
        assert_eq!(buffer_health_label(0, 0), "稳定");
        assert_eq!(buffer_health_label(1, 0), "曾发生欠载");
        assert_eq!(buffer_health_label(0, 1), "曾发生溢出");
    }

    #[test]
    fn latency_diagnostic_labels_are_chinese() {
        let runtime_info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "nnnoiseless-rnnoise",
                480,
                true,
            ),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );
        let metrics = PipelineMetrics::new(960, 96_000, 0, 0);

        assert_eq!(
            buffer_latency_label(metrics, Some(&runtime_info)),
            "约 10ms / 上限 1000ms"
        );
        assert_eq!(
            algorithm_latency_label(Some(&runtime_info)),
            "约 10ms（低延迟降噪）"
        );
        assert_eq!(
            buffer_diagnostic_label(metrics, Some(&runtime_info)),
            "稳定：延迟和缓冲正常"
        );
        assert_eq!(
            buffer_latency_label(PipelineMetrics::default(), None),
            "未运行"
        );
    }

    #[test]
    fn processing_status_labels_are_chinese() {
        let info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "nnnoiseless-rnnoise",
                480,
                true,
            )
            .with_strength(clearline_core::SuppressionStrength::Balanced),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );

        assert_eq!(
            noise_suppression_status_label(Some(&info)),
            "低延迟降噪（RNNoise）已启用"
        );
        assert_eq!(suppression_strength_label(Some(&info)), "标准");
        assert_eq!(
            runtime_format_label(info.input_format()),
            "48000 Hz / 1 声道"
        );
        assert_eq!(frame_size_label(Some(&info)), "480 samples");
        assert_eq!(noise_suppression_status_label(None), "未连接音频流");
    }

    #[test]
    fn echo_cancellation_labels_are_chinese() {
        let input_format = clearline_core::AudioFrameFormat::new(48_000, 1);
        let disabled = PipelineRuntimeInfo::new(
            input_format,
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "nnnoiseless-rnnoise",
                480,
                true,
            ),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );
        let enabled =
            disabled
                .clone()
                .with_echo_cancellation(clearline_core::EchoCancellerRuntimeInfo::new(
                    clearline_core::EchoCancellerBackend::Aec3,
                    input_format,
                ));

        assert_eq!(echo_cancellation_title(), "回音消除");
        assert_eq!(echo_cancellation_config_label(true), "回音消除：开启");
        assert_eq!(echo_cancellation_config_label(false), "回音消除：关闭");
        assert_eq!(echo_cancellation_runtime_label(None), "未连接音频流");
        assert_eq!(echo_cancellation_runtime_label(Some(&disabled)), "未启用");
        assert_eq!(
            echo_cancellation_runtime_label(Some(&enabled)),
            "已启用（AEC3）"
        );
    }

    #[test]
    fn echo_reference_diagnostic_labels_are_chinese() {
        let input_format = clearline_core::AudioFrameFormat::new(48_000, 1);
        let disabled = PipelineRuntimeInfo::new(
            input_format,
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "nnnoiseless-rnnoise",
                480,
                true,
            ),
            AudioOutputTarget::ClearLineVirtualMicrophone,
        );
        let enabled =
            disabled
                .clone()
                .with_echo_cancellation(clearline_core::EchoCancellerRuntimeInfo::new(
                    clearline_core::EchoCancellerBackend::Aec3,
                    input_format,
                ));
        let diagnostics = clearline_core::EchoReferenceDiagnostics::new(0.235, 480, 2, 3);

        assert_eq!(echo_reference_level_label(None, None), "未连接音频流");
        assert_eq!(echo_reference_level_label(Some(&disabled), None), "未启用");
        assert_eq!(
            echo_reference_level_label(Some(&enabled), None),
            "未连接参考音频"
        );
        assert_eq!(
            echo_reference_level_label(Some(&enabled), Some(diagnostics)),
            "24%（缓冲 480 samples）"
        );
        assert_eq!(
            echo_reference_health_label(Some(&enabled), Some(diagnostics)),
            "缺帧 2 · 丢弃 3"
        );
        assert_eq!(
            echo_reference_health_label(
                Some(&enabled),
                Some(clearline_core::EchoReferenceDiagnostics::new(
                    0.1, 480, 0, 0
                )),
            ),
            "稳定"
        );
    }

    #[test]
    fn fallback_status_labels_do_not_show_passthrough_word() {
        for mode in [
            SuppressorMode::Bypass,
            SuppressorMode::LowLatency,
            SuppressorMode::HighQuality,
        ] {
            let info = PipelineRuntimeInfo::new(
                clearline_core::AudioFrameFormat::new(44_100, 1),
                clearline_core::AudioFrameFormat::new(44_100, 2),
                clearline_core::SuppressorRuntimeInfo::new(mode, "bypass-placeholder", 0, false),
                AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
            );
            let label = noise_suppression_status_label(Some(&info));

            assert!(
                !label.contains("直通"),
                "fallback label should not expose passthrough wording: {label}"
            );
        }
    }

    #[test]
    fn rnnoise_format_fallback_status_is_explicit() {
        let info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(44_100, 1),
            clearline_core::AudioFrameFormat::new(44_100, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::LowLatency,
                "bypass-placeholder",
                441,
                false,
            ),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );

        assert_eq!(
            noise_suppression_status_label(Some(&info)),
            "RNNoise 不支持当前格式，已使用安全回退"
        );
    }

    #[test]
    fn high_quality_processing_status_label_is_not_placeholder() {
        let info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::HighQuality,
                "adaptive-quality-v1",
                960,
                true,
            )
            .with_strength(clearline_core::SuppressionStrength::Strong),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );

        assert_eq!(
            noise_suppression_status_label(Some(&info)),
            "实验降噪后端已启用"
        );
        assert_eq!(suppression_strength_label(Some(&info)), "强力");
        assert_eq!(
            strength_button_label(clearline_core::SuppressionStrength::Gentle),
            "柔和"
        );
        assert_eq!(
            strength_button_label(clearline_core::SuppressionStrength::Balanced),
            "标准"
        );
        assert_eq!(
            strength_button_label(clearline_core::SuppressionStrength::Strong),
            "强力"
        );
    }

    #[test]
    fn wind_reduction_status_labels_are_chinese() {
        assert_eq!(wind_reduction_config_label(true), "抗风噪增强：开启");
        assert_eq!(wind_reduction_config_label(false), "抗风噪增强：关闭");
        assert_eq!(wind_reduction_runtime_label(None), "未连接音频流");
    }

    #[test]
    fn top_header_control_labels_are_chinese() {
        assert_eq!(noise_suppression_config_label(true), "降噪：开启");
        assert_eq!(noise_suppression_config_label(false), "降噪：关闭");
    }

    #[test]
    fn old_start_stop_control_is_not_in_production_ui() {
        let source = include_str!("main.rs");
        let production_source = source
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source section exists");

        assert!(!production_source.contains("开始降噪"));
        assert!(!production_source.contains("停止降噪"));
        assert!(!production_source.contains("top_control_switch"));
    }

    #[test]
    fn deepfilter_startup_warning_reports_packaged_model_unavailable() {
        assert_eq!(
            deepfilter_startup_warning(
                SuppressorMode::LowLatency,
                &DeepFilterModelUiStatus::MissingAsset("enc.onnx".to_owned())
            ),
            None
        );
        assert_eq!(
            deepfilter_startup_warning(
                SuppressorMode::HighQuality,
                &DeepFilterModelUiStatus::Valid
            ),
            None
        );
        assert_eq!(
            deepfilter_startup_warning(
                SuppressorMode::HighQuality,
                &DeepFilterModelUiStatus::NotConfigured
            ),
            Some("DeepFilterNet 模型不可用：未找到随程序打包的模型".to_owned())
        );

        let warning = deepfilter_startup_warning(
            SuppressorMode::HighQuality,
            &DeepFilterModelUiStatus::MissingAsset("enc.onnx".to_owned()),
        )
        .expect("incomplete packaged model should produce a warning");

        assert!(warning.contains("DeepFilterNet"));
        assert!(warning.contains("不可用"));
        assert!(warning.contains("缺少 enc.onnx"));
        assert!(!warning.contains("RNNoise"));
        assert!(!warning.contains("回退"));
        assert!(!warning.contains("内置高质量"));
    }

    #[test]
    fn deepfilter_startup_warning_reports_model_unavailable_without_rnnoise_fallback() {
        let warning = deepfilter_startup_warning(
            SuppressorMode::HighQuality,
            &DeepFilterModelUiStatus::MissingAsset("enc.onnx".to_owned()),
        )
        .expect("incomplete packaged model should produce a warning");

        assert!(warning.contains("DeepFilterNet"));
        assert!(warning.contains("不可用"));
        assert!(!warning.contains("RNNoise"));
        assert!(!warning.contains("回退"));
    }

    #[test]
    fn header_does_not_duplicate_state_badge_next_to_controls() {
        let source = include_str!("main.rs");
        let duplicated_badge_call = ["state_", "badge(ui, &state);"].concat();

        assert!(
            !source.contains(&duplicated_badge_call),
            "顶部控件已经表达常用状态，不应再并排显示运行/停止状态徽标"
        );
    }

    #[test]
    fn build_info_label_contains_version_profile_and_commit() {
        let info = BuildInfo::new("0.1.0", "release", "d527159");

        assert_eq!(build_info_label(info), "v0.1.0 · release · d527159");
    }

    #[test]
    fn build_info_uses_unknown_commit_when_git_metadata_is_missing() {
        let info = BuildInfo::new("0.1.0", "debug", "");

        assert_eq!(build_info_label(info), "v0.1.0 · debug · unknown");
    }

    #[test]
    fn deepfilter_model_status_is_not_configured_for_empty_path() {
        assert_eq!(
            deepfilter_model_status(Path::new("")),
            DeepFilterModelUiStatus::NotConfigured
        );
        assert_eq!(
            deepfilter_model_status_label(DeepFilterModelUiStatus::NotConfigured),
            "未找到模型目录"
        );
    }

    #[test]
    fn packaged_deepfilter_model_dir_is_next_to_exe() {
        assert_eq!(
            packaged_deepfilter_model_dir_for_exe(Path::new("/opt/ClearLine/ClearLine.exe")),
            PathBuf::from("/opt/ClearLine").join(PACKAGED_DEEPFILTER_MODEL_DIR)
        );
    }

    #[test]
    fn deepfilter_model_status_accepts_required_onnx_bundle() {
        let model_dir = unique_temp_model_dir("app-deepfilter-valid");
        std::fs::write(model_dir.join("enc.onnx"), []).unwrap();
        std::fs::write(model_dir.join("erb_dec.onnx"), []).unwrap();
        std::fs::write(model_dir.join("df_dec.onnx"), []).unwrap();
        std::fs::write(model_dir.join("config.ini"), []).unwrap();

        assert_eq!(
            deepfilter_model_status(&model_dir),
            DeepFilterModelUiStatus::Valid
        );
        assert_eq!(
            deepfilter_model_status_label(DeepFilterModelUiStatus::Valid),
            "打包模型可用：DeepFilterNet"
        );
    }

    #[test]
    fn deepfilter_model_status_reports_missing_asset_filename() {
        let model_dir = unique_temp_model_dir("app-deepfilter-missing");
        std::fs::write(model_dir.join("enc.onnx"), []).unwrap();
        std::fs::write(model_dir.join("erb_dec.onnx"), []).unwrap();

        assert_eq!(
            deepfilter_model_status_label(deepfilter_model_status(&model_dir)),
            "打包模型不完整：缺少 df_dec.onnx"
        );
    }

    #[test]
    fn deepfilter_model_bundle_accepts_valid_bundle_dir() {
        let model_dir = unique_temp_model_dir("app-deepfilter-pipeline-bundle");
        std::fs::write(model_dir.join("enc.onnx"), []).unwrap();
        std::fs::write(model_dir.join("erb_dec.onnx"), []).unwrap();
        std::fs::write(model_dir.join("df_dec.onnx"), []).unwrap();
        std::fs::write(model_dir.join("config.ini"), []).unwrap();

        let bundle = DeepFilterNetModelBundle::from_dir(&model_dir).unwrap();

        assert_eq!(bundle.root_dir(), model_dir.as_path());
        assert!(DeepFilterNetModelBundle::from_dir(model_dir.join("missing")).is_err());
    }

    #[test]
    fn deepfilternet_status_reports_real_backend() {
        let info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::HighQuality,
                "deepfilternet-tract",
                480,
                true,
            )
            .with_strength(clearline_core::SuppressionStrength::Balanced),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );

        assert_eq!(
            noise_suppression_status_label(Some(&info)),
            "高质量降噪（DeepFilterNet 模型）已启用"
        );
    }

    #[test]
    fn deepfilternet_worker_status_reports_real_backend() {
        let info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::HighQuality,
                "deepfilternet-tract-worker",
                480,
                true,
            )
            .with_strength(clearline_core::SuppressionStrength::Balanced)
            .with_worker_diagnostics(
                clearline_core::SuppressorWorkerDiagnostics::new(3, 1, 3, 0)
                    .with_last_inference_time_ms(7)
                    .with_max_inference_time_ms(11),
            ),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );

        assert_eq!(
            noise_suppression_status_label(Some(&info)),
            "高质量降噪（DeepFilterNet 模型）已启用"
        );
        assert_eq!(inference_health_label(Some(&info)), "稳定");
        assert_eq!(inference_latency_label(Some(&info)), "最近 7ms / 最大 11ms");
        assert_eq!(inference_queue_label(Some(&info)), "输入 1/3 · 输出 0/3");
        assert_eq!(
            inference_drop_label(Some(&info)),
            "输入丢弃 0 · 输出丢弃 0 · 迟到 0"
        );
    }

    #[test]
    fn deepfilternet_worker_status_reports_slow_or_degraded_state() {
        let slow_info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::HighQuality,
                "deepfilternet-tract-worker",
                480,
                true,
            )
            .with_worker_diagnostics(
                clearline_core::SuppressorWorkerDiagnostics::new(3, 3, 3, 2)
                    .with_late_output_frames(4)
                    .with_last_inference_time_ms(35)
                    .with_max_inference_time_ms(42),
            ),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );
        assert_eq!(inference_health_label(Some(&slow_info)), "推理偏慢");

        let degraded_info = PipelineRuntimeInfo::new(
            clearline_core::AudioFrameFormat::new(48_000, 1),
            clearline_core::AudioFrameFormat::new(48_000, 2),
            clearline_core::SuppressorRuntimeInfo::new(
                SuppressorMode::HighQuality,
                "deepfilternet-tract-worker",
                480,
                true,
            )
            .with_worker_diagnostics(
                clearline_core::SuppressorWorkerDiagnostics::new(3, 0, 3, 0)
                    .with_inference_errors(1)
                    .with_degraded(true),
            ),
            AudioOutputTarget::AudioDevice(DeviceId::new("out-1")),
        );
        assert_eq!(inference_health_label(Some(&degraded_info)), "已降级");
    }

    #[test]
    fn app_default_features_enable_deepfilternet_backend() {
        let manifest = include_str!("../Cargo.toml");

        assert!(
            manifest.contains("default = [\"deepfilternet\", \"aec\"]"),
            "默认 exe 应包含 DeepFilterNet 后端，避免 UI 配置后仍只能使用 adaptive 后端"
        );
    }

    #[test]
    fn app_default_features_enable_aec_backend() {
        let manifest = include_str!("../Cargo.toml");

        assert!(manifest.contains("aec = [\"clearline-core/aec\"]"));
        assert!(
            manifest.contains("default = [\"deepfilternet\", \"aec\"]"),
            "默认 exe 应包含 AEC3 后端，避免开启回音消除后仍回退为未启用"
        );
    }

    #[test]
    fn app_default_features_do_not_enable_rnnoise_backend() {
        let manifest = include_str!("../Cargo.toml");

        assert!(
            !manifest.contains("features = [\"rnnoise\"]"),
            "默认 App 不应启用 RNNoise，避免用户路径回退到会产生杂音的低延迟后端"
        );
    }

    fn unique_temp_model_dir(prefix: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "clearline-{prefix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unique_temp_settings_path(prefix: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "clearline-app-settings-{prefix}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }
}
