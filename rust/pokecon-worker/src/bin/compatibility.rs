//! Hermetic managed-worker discovery used by the compatibility corpus gate.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use pokecon_camera::{CaptureResolution, FlipMode, ScreenshotFormat};
use pokecon_worker::WorkerKind;
use pokecon_worker::ipc::ResourceSafety;
use pokecon_worker::script::protocol::{
    HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
    HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
    HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest, HostNetworkResult,
    HostNotificationRequest, HostOutputRequest, HostOverlayRequest, HostPopupImageRequest,
    HostTkRequest, HostTkResult, PYTHON_SITE_PACKAGES_ENV, ScriptDialogState,
    ScriptInitializeRequest,
};
use pokecon_worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};
use pokecon_worker::supervisor::{StopPurpose, WorkerLaunch, WorkerSupervisor};

#[derive(Debug, Parser)]
#[command(about = "Discover an immutable compatibility corpus in a managed worker")]
struct Cli {
    /// The separately built managed worker executable.
    #[arg(long)]
    worker: PathBuf,
    /// Materialized SerialController/Commands directory.
    #[arg(long)]
    command_root: PathBuf,
    /// Empty writable data root used by the worker fixture.
    #[arg(long)]
    data_root: PathBuf,
    /// Fixed internal site-packages needed by immutable legacy imports.
    #[arg(long)]
    site_packages: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct CompatibilityHost;

impl ScriptHost for CompatibilityHost {
    fn controller_input(
        &self,
        _request: HostControllerInputRequest,
    ) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn controller_neutral(&self) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn serial_write(&self, _data: Vec<u8>) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn serial_write_row(&self, _row: String) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn serial_reload(&self) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn output(&self, _request: HostOutputRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn dialog_open(
        &self,
        _request: HostDialogOpenRequest,
    ) -> Result<HostDialogOpenResult, ScriptHostError> {
        Ok(HostDialogOpenResult { dialog_id: 1 })
    }

    fn dialog_status(
        &self,
        _request: HostDialogStatusRequest,
    ) -> Result<HostDialogStatusResult, ScriptHostError> {
        Ok(HostDialogStatusResult {
            state: ScriptDialogState::Confirmed { values: Vec::new() },
        })
    }

    fn dialog_close_all(&self) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn network(&self, _request: HostNetworkRequest) -> Result<HostNetworkResult, ScriptHostError> {
        Ok(HostNetworkResult { message: None })
    }

    fn notification(&self, _request: HostNotificationRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn camera_initialize(&self) -> Result<HostCameraInitializeResult, ScriptHostError> {
        Ok(HostCameraInitializeResult {
            mapping: None,
            state: camera_state(),
        })
    }

    fn camera_control(
        &self,
        _request: HostCameraControlRequest,
    ) -> Result<HostCameraState, ScriptHostError> {
        Ok(camera_state())
    }

    fn overlay(&self, _request: HostOverlayRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn popup_image(&self, _request: HostPopupImageRequest) -> Result<(), ScriptHostError> {
        Ok(())
    }

    fn tk(&self, _request: HostTkRequest) -> Result<HostTkResult, ScriptHostError> {
        Ok(HostTkResult::default())
    }
}

impl ResourceSafety for CompatibilityHost {
    fn force_release(&self) {}
}

const fn camera_state() -> HostCameraState {
    HostCameraState {
        opened: false,
        fps: 45,
        capture_resolution: CaptureResolution::R1280x720,
        flip_mode: FlipMode::None,
        screenshot_format: ScreenshotFormat::Png,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let host = Arc::new(CompatibilityHost);
    let mut launch = WorkerLaunch::managed(&cli.worker, WorkerKind::Script).clear_environment();
    if let Some(site_packages) = cli.site_packages {
        launch = launch.environment(PYTHON_SITE_PACKAGES_ENV, site_packages);
    }
    let supervisor = WorkerSupervisor::new();
    let worker = supervisor.spawn(launch, host.clone()).await?;
    let client = ScriptWorkerClient::attach(worker.clone(), host)?;
    client
        .initialize(&ScriptInitializeRequest {
            profile: "compatibility".to_owned(),
            command_root: cli.command_root,
            data_root: cli.data_root,
        })
        .await?;
    let discovery = client.discover().await?;
    println!("{}", serde_json::to_string_pretty(&discovery)?);
    worker
        .stop(StopPurpose::ApplicationShutdown, Duration::from_secs(5))
        .await?;
    Ok(())
}
