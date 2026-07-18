use std::collections::BTreeMap;
use std::str::FromStr as _;

use pokecon_settings::service::{PatchClass, RuntimeSettingsApplier};
use serde_json::Value;

use crate::backend::{CameraConfig, CameraError};
use crate::frame::FlipMode;
use crate::manager::CameraManager;
use crate::screenshot::{ScreenshotFormat, ScreenshotRuntimeSettings};

#[derive(Clone, Debug)]
struct CameraSettingsValues {
    config: CameraConfig,
    flip: FlipMode,
    screenshot_format: ScreenshotFormat,
    jpeg_quality: u8,
}

impl CameraSettingsValues {
    fn overlay(&self, changes: &BTreeMap<String, Value>) -> Result<Self, CameraError> {
        let mut selector = self.config.selector().clone();
        let mut fps = self.config.requested_fps();
        let mut resolution = self.config.resolution();
        let mut flip = self.flip;
        let mut screenshot_format = self.screenshot_format;
        let mut jpeg_quality = self.jpeg_quality;
        if let Some(value) = changes.get("camera.device") {
            selector = serde_json::from_value(value.clone())
                .map_err(|_| CameraError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("camera.capture_fps") {
            fps = u32::try_from(value.as_u64().ok_or(CameraError::InvalidSettingsValue)?)
                .map_err(|_| CameraError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("camera.capture_resolution") {
            resolution = value
                .as_str()
                .ok_or(CameraError::InvalidSettingsValue)?
                .parse()
                .map_err(|_| CameraError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("camera.flip_mode") {
            flip = FlipMode::from_str(value.as_str().ok_or(CameraError::InvalidSettingsValue)?)
                .map_err(|_| CameraError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("camera.screenshot_format") {
            screenshot_format = ScreenshotFormat::from_str(
                value.as_str().ok_or(CameraError::InvalidSettingsValue)?,
            )
            .map_err(|_| CameraError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("jpeg_quality") {
            jpeg_quality = u8::try_from(value.as_u64().ok_or(CameraError::InvalidSettingsValue)?)
                .map_err(|_| CameraError::InvalidSettingsValue)?;
            if !(1..=100).contains(&jpeg_quality) {
                return Err(CameraError::InvalidSettingsValue);
            }
        }
        Ok(Self {
            config: CameraConfig::new(selector, fps, resolution)?,
            flip,
            screenshot_format,
            jpeg_quality,
        })
    }
}

/// Settings-service bridge for acquisition transactions and immediate visual
/// processing/encoding defaults.
#[derive(Clone, Debug)]
pub struct CameraSettingsApplier {
    manager: CameraManager,
    screenshot_settings: ScreenshotRuntimeSettings,
    current: CameraSettingsValues,
}

impl CameraSettingsApplier {
    /// Creates a bridge from the already resolved startup values.
    ///
    /// # Errors
    ///
    /// Rejects an invalid FPS or JPEG quality.
    pub fn new(
        manager: CameraManager,
        config: CameraConfig,
        flip: FlipMode,
        screenshot_format: ScreenshotFormat,
        jpeg_quality: u8,
        screenshot_settings: ScreenshotRuntimeSettings,
    ) -> Result<Self, CameraError> {
        if !(1..=100).contains(&jpeg_quality) {
            return Err(CameraError::InvalidSettingsValue);
        }
        manager.set_flip(flip);
        screenshot_settings.set_format(screenshot_format);
        screenshot_settings
            .set_jpeg_quality(jpeg_quality)
            .map_err(|_| CameraError::InvalidSettingsValue)?;
        Ok(Self {
            manager,
            screenshot_settings,
            current: CameraSettingsValues {
                config,
                flip,
                screenshot_format,
                jpeg_quality,
            },
        })
    }

    fn apply_values(
        &self,
        class: PatchClass,
        next: &CameraSettingsValues,
    ) -> Result<(), CameraError> {
        if class == PatchClass::Camera {
            self.manager.apply_config(next.config.clone())?;
        }
        self.manager.set_flip(next.flip);
        self.screenshot_settings.set_format(next.screenshot_format);
        self.screenshot_settings
            .set_jpeg_quality(next.jpeg_quality)
            .map_err(|_| CameraError::InvalidSettingsValue)
    }
}

impl RuntimeSettingsApplier for CameraSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        let relevant = changes.keys().any(|id| {
            matches!(
                id.as_str(),
                "camera.device"
                    | "camera.capture_fps"
                    | "camera.capture_resolution"
                    | "camera.flip_mode"
                    | "camera.screenshot_format"
                    | "jpeg_quality"
            )
        });
        if !relevant {
            return Ok(());
        }
        let next = self
            .current
            .overlay(changes)
            .map_err(|_| "camera runtime transaction failed".to_owned())?;
        self.apply_values(class, &next)
            .map_err(|_| "camera runtime transaction failed".to_owned())?;
        self.current = next;
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        let Ok(restored) = self.current.overlay(previous) else {
            tracing::error!(
                diagnostic_id = "CAMERA_SETTINGS_ROLLBACK_INVALID",
                "camera settings rollback values were invalid"
            );
            return;
        };
        if self.apply_values(class, &restored).is_ok() {
            self.current = restored;
        } else {
            tracing::error!(
                diagnostic_id = "CAMERA_SETTINGS_ROLLBACK_FAILED",
                "camera settings rollback could not restore the previous capture state"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Arc;
    use std::time::Duration;

    use pokecon_settings::service::{PatchClass, RuntimeSettingsApplier};
    use serde_json::json;

    use super::CameraSettingsApplier;
    use crate::backend::CameraConfig;
    use crate::frame::{CaptureResolution, FlipMode};
    use crate::manager::CameraManager;
    use crate::screenshot::{ScreenshotFormat, ScreenshotRuntimeSettings};
    use crate::selector::CameraSelector;
    use crate::virtual_camera::{
        RecordedFrame, RecordedFrameSource, VirtualCameraBackend, VirtualOpenPlan,
        VirtualReconfigurePlan, VirtualSessionPlan,
    };

    #[test]
    fn settings_bridge_applies_and_rolls_back_capture_and_encoding_values() {
        let backend = VirtualCameraBackend::default();
        backend.push_open(VirtualOpenPlan::Success(VirtualSessionPlan {
            effective_fps: 30,
            frames: RecordedFrameSource::new([RecordedFrame::Solid([1, 2, 3])]),
            reconfigurations: VecDeque::from([
                VirtualReconfigurePlan::Accept { effective_fps: 24 },
                VirtualReconfigurePlan::Accept { effective_fps: 30 },
            ]),
        }));
        let initial =
            CameraConfig::new(CameraSelector::Index(0), 30, CaptureResolution::R640x360).unwrap();
        let manager =
            CameraManager::start(Arc::new(backend), initial.clone(), FlipMode::None).unwrap();
        let screenshot = ScreenshotRuntimeSettings::default();
        let mut applier = CameraSettingsApplier::new(
            manager.clone(),
            initial,
            FlipMode::None,
            ScreenshotFormat::Png,
            85,
            screenshot.clone(),
        )
        .unwrap();
        let capture = BTreeMap::from([
            ("camera.capture_fps".to_owned(), json!(25)),
            ("camera.capture_resolution".to_owned(), json!("1280x720")),
        ]);
        applier.apply(PatchClass::Camera, &capture).unwrap();
        assert_eq!(manager.status().camera_fps, 24);
        assert_eq!(manager.status().camera_resolution, "1280x720");
        applier.rollback(
            PatchClass::Camera,
            &BTreeMap::from([
                ("camera.capture_fps".to_owned(), json!(30)),
                ("camera.capture_resolution".to_owned(), json!("640x360")),
            ]),
        );
        assert_eq!(manager.status().camera_resolution, "640x360");

        applier
            .apply(
                PatchClass::Ordinary,
                &BTreeMap::from([
                    ("camera.flip_mode".to_owned(), json!("both")),
                    ("camera.screenshot_format".to_owned(), json!("jpeg")),
                    ("jpeg_quality".to_owned(), json!(42)),
                ]),
            )
            .unwrap();
        assert_eq!(manager.flip(), FlipMode::Both);
        assert_eq!(screenshot.format(), ScreenshotFormat::Jpeg);
        assert_eq!(screenshot.jpeg_quality(), 42);
        manager.shutdown(Duration::from_secs(1)).unwrap();
    }
}
