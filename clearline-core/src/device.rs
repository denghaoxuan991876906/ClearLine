use crate::ClearLineResult;

#[cfg(windows)]
use crate::ClearLineError;
#[cfg(windows)]
use cpal::traits::{DeviceTrait, HostTrait};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for DeviceId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for DeviceId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInputDevice {
    id: DeviceId,
    name: String,
    is_default: bool,
    sample_rate_hz: Option<u32>,
    channels: Option<u16>,
}

impl AudioInputDevice {
    pub fn new(
        id: impl Into<DeviceId>,
        name: impl Into<String>,
        is_default: bool,
    ) -> AudioInputDevice {
        AudioInputDevice {
            id: id.into(),
            name: clean_endpoint_device_name(&name.into()),
            is_default,
            sample_rate_hz: None,
            channels: None,
        }
    }

    pub fn with_default_format(
        mut self,
        sample_rate_hz: Option<u32>,
        channels: Option<u16>,
    ) -> Self {
        self.sample_rate_hz = sample_rate_hz;
        self.channels = channels;
        self
    }

    pub fn id(&self) -> &DeviceId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn sample_rate_hz(&self) -> Option<u32> {
        self.sample_rate_hz
    }

    pub fn channels(&self) -> Option<u16> {
        self.channels
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioOutputDevice {
    id: DeviceId,
    name: String,
    is_default: bool,
    sample_rate_hz: Option<u32>,
    channels: Option<u16>,
}

impl AudioOutputDevice {
    pub fn new(
        id: impl Into<DeviceId>,
        name: impl Into<String>,
        is_default: bool,
    ) -> AudioOutputDevice {
        AudioOutputDevice {
            id: id.into(),
            name: clean_endpoint_device_name(&name.into()),
            is_default,
            sample_rate_hz: None,
            channels: None,
        }
    }

    pub fn with_default_format(
        mut self,
        sample_rate_hz: Option<u32>,
        channels: Option<u16>,
    ) -> Self {
        self.sample_rate_hz = sample_rate_hz;
        self.channels = channels;
        self
    }

    pub fn id(&self) -> &DeviceId {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn sample_rate_hz(&self) -> Option<u32> {
        self.sample_rate_hz
    }

    pub fn channels(&self) -> Option<u16> {
        self.channels
    }
}

fn clean_endpoint_device_name(raw_name: &str) -> String {
    let trimmed = raw_name.trim();

    trailing_hardware_name(trimmed)
        .filter(|_| has_generic_endpoint_prefix(trimmed))
        .unwrap_or(trimmed)
        .to_owned()
}

fn trailing_hardware_name(name: &str) -> Option<&str> {
    let name = name.trim();
    if !name.ends_with(')') {
        return None;
    }

    let open_paren = name.rfind('(')?;
    let prefix = name[..open_paren].trim();
    let hardware_name = name[open_paren + '('.len_utf8()..name.len() - ')'.len_utf8()].trim();

    if prefix.is_empty() || hardware_name.is_empty() {
        return None;
    }

    Some(hardware_name)
}

fn has_generic_endpoint_prefix(name: &str) -> bool {
    let Some(open_paren) = name.rfind('(') else {
        return false;
    };
    let prefix = name[..open_paren].trim();
    let prefix_ascii = prefix.to_ascii_lowercase();

    matches!(
        prefix_ascii.as_str(),
        "microphone"
            | "microphone array"
            | "headset microphone"
            | "line in"
            | "line out"
            | "stereo mix"
            | "recording device"
            | "speaker"
            | "speakers"
            | "headphone"
            | "headphones"
            | "headset"
            | "playback device"
    ) || !prefix
        .chars()
        .any(|character| character.is_ascii_alphanumeric())
}

pub trait DeviceEnumerator {
    fn input_devices(&self) -> ClearLineResult<Vec<AudioInputDevice>>;

    fn output_devices(&self) -> ClearLineResult<Vec<AudioOutputDevice>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CpalDeviceEnumerator;

impl DeviceEnumerator for CpalDeviceEnumerator {
    #[cfg(windows)]
    fn input_devices(&self) -> ClearLineResult<Vec<AudioInputDevice>> {
        let host = cpal::default_host();
        let default_id = host
            .default_input_device()
            .and_then(|device| device.id().ok())
            .map(|id| id.to_string());
        let devices = host
            .input_devices()
            .map_err(|error| ClearLineError::DeviceEnumeration(error.to_string()))?;

        let mut inputs = Vec::new();
        for (index, device) in devices.enumerate() {
            let name = device.to_string();
            let cpal_id = device
                .id()
                .map(|id| id.to_string())
                .unwrap_or_else(|error| format!("cpal-input-{index}:{name}:{error}"));
            let is_default = default_id.as_deref() == Some(cpal_id.as_str());
            let default_config = device.default_input_config().ok();
            let sample_rate_hz = default_config.as_ref().map(|config| config.sample_rate());
            let channels = default_config.as_ref().map(|config| config.channels());
            let id = DeviceId::new(cpal_id);

            inputs.push(
                AudioInputDevice::new(id, name, is_default)
                    .with_default_format(sample_rate_hz, channels),
            );
        }

        Ok(inputs)
    }

    #[cfg(not(windows))]
    fn input_devices(&self) -> ClearLineResult<Vec<AudioInputDevice>> {
        Ok(Vec::new())
    }

    #[cfg(windows)]
    fn output_devices(&self) -> ClearLineResult<Vec<AudioOutputDevice>> {
        let host = cpal::default_host();
        let default_id = host
            .default_output_device()
            .and_then(|device| device.id().ok())
            .map(|id| id.to_string());
        let devices = host
            .output_devices()
            .map_err(|error| ClearLineError::DeviceEnumeration(error.to_string()))?;

        let mut outputs = Vec::new();
        for (index, device) in devices.enumerate() {
            let name = device.to_string();
            let cpal_id = device
                .id()
                .map(|id| id.to_string())
                .unwrap_or_else(|error| format!("cpal-output-{index}:{name}:{error}"));
            let is_default = default_id.as_deref() == Some(cpal_id.as_str());
            let default_config = device.default_output_config().ok();
            let sample_rate_hz = default_config.as_ref().map(|config| config.sample_rate());
            let channels = default_config.as_ref().map(|config| config.channels());
            let id = DeviceId::new(cpal_id);

            outputs.push(
                AudioOutputDevice::new(id, name, is_default)
                    .with_default_format(sample_rate_hz, channels),
            );
        }

        Ok(outputs)
    }

    #[cfg(not(windows))]
    fn output_devices(&self) -> ClearLineResult<Vec<AudioOutputDevice>> {
        Ok(Vec::new())
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InputDeviceSelector {
    selected: Option<DeviceId>,
}

impl InputDeviceSelector {
    pub fn select(&mut self, device_id: DeviceId) {
        self.selected = Some(device_id);
    }

    pub fn clear(&mut self) {
        self.selected = None;
    }

    pub fn selected(&self) -> Option<&DeviceId> {
        self.selected.as_ref()
    }

    pub fn resolve<'a>(&self, devices: &'a [AudioInputDevice]) -> Option<&'a AudioInputDevice> {
        let selected = self.selected.as_ref()?;
        devices.iter().find(|device| device.id() == selected)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OutputDeviceSelector {
    selected: Option<DeviceId>,
}

impl OutputDeviceSelector {
    pub fn select(&mut self, device_id: DeviceId) {
        self.selected = Some(device_id);
    }

    pub fn clear(&mut self) {
        self.selected = None;
    }

    pub fn selected(&self) -> Option<&DeviceId> {
        self.selected.as_ref()
    }

    pub fn resolve<'a>(&self, devices: &'a [AudioOutputDevice]) -> Option<&'a AudioOutputDevice> {
        let selected = self.selected.as_ref()?;
        devices.iter().find(|device| device.id() == selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_resolves_selected_input_device() {
        let devices = vec![
            AudioInputDevice::new("mic-1", "Built-in Microphone", true),
            AudioInputDevice::new("mic-2", "USB Microphone", false),
        ];

        let mut selector = InputDeviceSelector::default();
        selector.select(DeviceId::new("mic-2"));

        let selected = selector.resolve(&devices).expect("selected device exists");
        assert_eq!(selected.name(), "USB Microphone");
        assert_eq!(selected.id().as_str(), "mic-2");
    }

    #[test]
    fn selector_returns_none_when_device_disappears() {
        let devices = vec![AudioInputDevice::new("mic-1", "Built-in Microphone", true)];

        let mut selector = InputDeviceSelector::default();
        selector.select(DeviceId::new("mic-2"));

        assert!(selector.resolve(&devices).is_none());
    }

    #[test]
    fn input_device_name_strips_localized_microphone_prefix() {
        let device = AudioInputDevice::new("mic-1", "麦克风 (MCHOSE V9 Turbo+)", true);

        assert_eq!(device.name(), "MCHOSE V9 Turbo+");
    }

    #[test]
    fn input_device_name_strips_unrenderable_prefix_before_hardware_name() {
        let device = AudioInputDevice::new("mic-1", "□□□ (MCHOSE V9 Turbo+)", true);

        assert_eq!(device.name(), "MCHOSE V9 Turbo+");
    }

    #[test]
    fn input_device_name_preserves_plain_hardware_name() {
        let device = AudioInputDevice::new("mic-1", "MCHOSE V9 Turbo+", true);

        assert_eq!(device.name(), "MCHOSE V9 Turbo+");
    }

    #[test]
    fn output_selector_resolves_selected_output_device() {
        let devices = vec![
            AudioOutputDevice::new("out-1", "Speakers", true),
            AudioOutputDevice::new("out-2", "VB-CABLE Input", false),
        ];

        let mut selector = OutputDeviceSelector::default();
        selector.select(DeviceId::new("out-2"));

        let selected = selector.resolve(&devices).expect("selected device exists");
        assert_eq!(selected.name(), "VB-CABLE Input");
        assert_eq!(selected.id().as_str(), "out-2");
    }

    #[test]
    fn output_device_name_strips_localized_speaker_prefix() {
        let device = AudioOutputDevice::new("out-1", "扬声器 (VB-CABLE Input)", false);

        assert_eq!(device.name(), "VB-CABLE Input");
    }

    #[test]
    fn output_device_name_strips_english_speakers_prefix() {
        let device = AudioOutputDevice::new("out-1", "Speakers (VB-CABLE Input)", false);

        assert_eq!(device.name(), "VB-CABLE Input");
    }
}
