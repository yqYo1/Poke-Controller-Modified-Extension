use std::collections::BTreeMap;
use std::future::Future;

use serde_json::Value;
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::device::{ControllerFormat, SerialConfig, SerialError, SerialManager};
use crate::settings::service::{PatchClass, RuntimeSettingsApplier};

#[derive(Clone, Debug)]
struct SerialSettingsValues {
    port: String,
    baud_rate: u32,
    data_format: ControllerFormat,
}

impl SerialSettingsValues {
    fn config(&self) -> Result<Option<SerialConfig>, SerialError> {
        if self.port.is_empty() {
            return Ok(None);
        }
        SerialConfig::new(self.port.clone(), self.baud_rate, self.data_format).map(Some)
    }

    fn overlay(&self, changes: &BTreeMap<String, Value>) -> Result<Self, SerialError> {
        let mut next = self.clone();
        if let Some(value) = changes.get("serial.port") {
            value
                .as_str()
                .ok_or(SerialError::InvalidSettingsValue)?
                .clone_into(&mut next.port);
        }
        if let Some(value) = changes.get("serial.baud_rate") {
            next.baud_rate =
                u32::try_from(value.as_u64().ok_or(SerialError::InvalidSettingsValue)?)
                    .map_err(|_| SerialError::InvalidSettingsValue)?;
        }
        if let Some(value) = changes.get("serial.data_format") {
            next.data_format = value
                .as_str()
                .and_then(ControllerFormat::parse)
                .ok_or(SerialError::InvalidSettingsValue)?;
        }
        next.config()?;
        Ok(next)
    }
}

/// Synchronous settings-service bridge backed by the application's existing
/// multithread Tokio runtime. It never creates a second runtime.
#[derive(Debug)]
pub(crate) struct SerialSettingsApplier {
    manager: SerialManager,
    runtime: Handle,
    current: SerialSettingsValues,
}

impl SerialSettingsApplier {
    /// Creates a bridge for the already resolved startup serial values.
    ///
    /// # Errors
    ///
    /// Rejects invalid startup settings.
    pub(crate) fn new(
        manager: SerialManager,
        runtime: Handle,
        port: String,
        baud_rate: u32,
        data_format: ControllerFormat,
    ) -> Result<Self, SerialError> {
        let current = SerialSettingsValues {
            port,
            baud_rate,
            data_format,
        };
        current.config()?;
        Ok(Self {
            manager,
            runtime,
            current,
        })
    }

    fn run<F>(&self, future: F) -> Result<(), SerialError>
    where
        F: Future<Output = Result<(), SerialError>>,
    {
        if Handle::try_current().is_ok() {
            if self.runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
                return Err(SerialError::SynchronousBridgeUnavailable);
            }
            tokio::task::block_in_place(|| self.runtime.block_on(future))
        } else {
            self.runtime.block_on(future)
        }
    }

    fn apply_values(&self, values: &SerialSettingsValues) -> Result<(), SerialError> {
        match values.config()? {
            Some(config) => self.run(self.manager.update_config(config)),
            None => self.run(self.manager.clear_config()),
        }
    }
}

impl RuntimeSettingsApplier for SerialSettingsApplier {
    fn apply(
        &mut self,
        class: PatchClass,
        changes: &BTreeMap<String, Value>,
    ) -> Result<(), String> {
        if class != PatchClass::Serial {
            return Ok(());
        }
        let next = self
            .current
            .overlay(changes)
            .map_err(|_| "serial runtime transaction failed".to_owned())?;
        self.apply_values(&next)
            .map_err(|_| "serial runtime transaction failed".to_owned())?;
        self.current = next;
        Ok(())
    }

    fn rollback(&mut self, class: PatchClass, previous: &BTreeMap<String, Value>) {
        if class != PatchClass::Serial {
            return;
        }
        let Ok(restored) = self.current.overlay(previous) else {
            tracing::error!(
                diagnostic_id = "SERIAL_SETTINGS_ROLLBACK_INVALID",
                "serial settings rollback values were invalid"
            );
            return;
        };
        if self.apply_values(&restored).is_ok() {
            self.current = restored;
        } else {
            tracing::error!(
                diagnostic_id = "SERIAL_SETTINGS_ROLLBACK_FAILED",
                "serial settings rollback could not restore the previous connection"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io;
    use std::sync::Arc;

    use serde_json::json;

    use crate::device::{
        ControllerFormat, SerialConfig, SerialManager, VirtualOpenPlan, VirtualSerialBackend,
        VirtualSerialEndpoint,
    };
    use crate::settings::service::{PatchClass, RuntimeSettingsApplier};

    use super::SerialSettingsApplier;

    fn config(port: &str) -> SerialConfig {
        SerialConfig::new(port, 9600, ControllerFormat::Default).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn settings_bridge_applies_before_commit_and_keeps_old_values_on_failure() {
        let backend = VirtualSerialBackend::default();
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Fail(io::ErrorKind::NotFound))
            .await;
        backend
            .push_plan(VirtualOpenPlan::Success(VirtualSerialEndpoint::new()))
            .await;
        let manager = SerialManager::new(Arc::new(backend));
        manager.apply_config(config("old")).await.unwrap();
        let mut applier = SerialSettingsApplier::new(
            manager.clone(),
            tokio::runtime::Handle::current(),
            "old".to_owned(),
            9600,
            ControllerFormat::Default,
        )
        .unwrap();
        assert!(
            applier
                .apply(
                    PatchClass::Serial,
                    &BTreeMap::from([("serial.port".to_owned(), json!("new"))]),
                )
                .is_err()
        );
        assert_eq!(
            manager.current_config().await.unwrap().selector.as_str(),
            "old"
        );
    }
}
