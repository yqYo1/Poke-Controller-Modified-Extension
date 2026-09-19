//! Opt-in access to internal domain types for cross-target integration tests.
//!
//! This module is available only with the `integration-test-support` feature.
//! It is not a product extension surface; it prevents integration-test needs
//! from making the production module tree public by default.

/// Camera fixtures and domain values used across process-level tests.
pub mod camera {
    pub use crate::camera::{
        BgrFrame, CameraBackend, CameraConfig, CameraSelector, CaptureResolution, FlipMode,
        NativeCameraBackend, ScreenshotFormat, SharedFrameRing,
    };
}

/// Canonical contract models and generators used by drift tests.
pub mod contracts {
    /// Canonical setting model values used by contract drift tests.
    pub mod model {
        pub use crate::contracts::model::{Access, Mutability, Scope, Setting};
    }

    pub use crate::contracts::{PROTOCOL_REGISTRY_JSON, settings_registry};

    /// Explicit read-only generator checks used by contract drift tests.
    #[cfg(feature = "contract-generator")]
    pub mod generator {
        pub use crate::contracts::{check_generated_artifacts, check_openapi_artifact};
    }
}

/// Device, input-arbitration, and serial fixtures used by integration tests.
pub mod device {
    /// Canonical controller state used by arbitration and serial tests.
    pub mod controller {
        pub use crate::device::controller::{Button, ControllerState};
    }

    /// Input sources and arbitration values used by integration tests.
    pub mod input {
        pub use crate::device::input::{
            ApplyResult, InputArbiter, InputEvent, InputGeneration, InputPriority, InputSequence,
            InputSnapshot, InputSourceId, InputSourceKind, MouseButtons, PressState,
        };
    }

    /// Native and virtual serial implementations used by integration tests.
    pub mod serial {
        pub use crate::device::serial::{
            ControllerFormat, NativeSerialBackend, PortSelector, SerialBackend, SerialConfig,
            SerialManager, VirtualOpenPlan, VirtualSerialBackend, VirtualSerialEndpoint,
        };
    }
}

/// Dynamic-configuration domain and protocol values used by worker tests.
pub mod dynamic {
    /// Worker initialization and profile-switch protocol values.
    pub mod protocol {
        pub use crate::dynamic::protocol::{
            DynamicInitializeRequest, DynamicProfileSwitchResult, DynamicWorkerStatus,
            PYTHON_SITE_PACKAGES_ENV,
        };
    }

    pub use crate::dynamic::{
        CommandDisplayItem, CommandInfo, DynamicConfigControl, DynamicConfigLanguage, DynamicHost,
        InMemoryDynamicHost,
    };
}

/// Settings and managed-environment fixtures used by cross-process tests.
pub mod settings {
    /// HMAC key persistence used by cross-process locking tests.
    pub mod hmac_key {
        pub use crate::settings::hmac_key::HmacKey;
    }

    /// Filesystem lock manager used by cross-process tests.
    pub mod lock {
        pub use crate::settings::lock::LockManager;
    }

    /// Signed manifest output used by managed-environment tests.
    pub mod manifest {
        pub use crate::settings::manifest::ManifestOutput;
    }

    /// Constraint and Python-worker contracts used by package tests.
    pub mod package {
        pub use crate::settings::package::{ConstraintResolver, PythonWorker};
    }

    /// TOML persistence used by cross-process tests.
    pub mod persistence {
        pub use crate::settings::persistence::TomlStore;
    }

    /// XDG roots and safe path components used by integration tests.
    pub mod roots {
        pub use crate::settings::roots::{
            BaseDirectories, EffectiveRoots, RootEnvironment, SafeComponent,
        };
    }

    /// Managed uv execution values used by environment tests.
    pub mod uv {
        pub use crate::settings::uv::{ManagedUv, UvChildEnvironment};
    }

    /// Managed virtual-environment lifecycle values used by integration tests.
    pub mod venv {
        pub use crate::settings::venv::{
            PreparationDisposition, UvExecutionContext, UvExecutor, VenvError, VenvManager,
            VenvOwnership, VenvPreparationRequest,
        };
    }
}

/// Worker IPC, lifecycle, and supervision fixtures used by integration tests.
pub mod worker {
    /// Dynamic-worker client used by lifecycle tests.
    pub mod dynamic {
        pub use crate::worker::dynamic::DynamicWorkerClient;
    }

    /// Worker-generation errors used by lifecycle tests.
    pub mod generation {
        pub use crate::worker::generation::{GenerationError, GenerationPhase};
    }

    /// Typed IPC values and resource-safety hook used by worker tests.
    pub mod ipc {
        pub use crate::worker::ipc::{
            DisconnectReason, IpcValue, LogLevel, LogPayload, LogTarget, ResourceSafety,
        };
    }

    /// Script-worker protocol and host/client contracts used by runtime tests.
    pub mod script {
        pub use crate::worker::script::{ScriptHost, ScriptHostError, ScriptWorkerClient};

        /// Script IPC requests, responses, and state values.
        pub mod protocol {
            pub use crate::worker::script::protocol::{
                HostCameraControlRequest, HostCameraInitializeResult, HostCameraState,
                HostControllerInputRequest, HostDialogOpenRequest, HostDialogOpenResult,
                HostDialogStatusRequest, HostDialogStatusResult, HostNetworkRequest,
                HostNetworkResult, HostNotificationRequest, HostOutputRequest, HostOverlayRequest,
                HostPopupImageRequest, HostTkRequest, HostTkResult, MAX_NOTIFICATION_IMAGE_BYTES,
                MAX_POPUP_IMAGE_BYTES, PYTHON_SITE_PACKAGES_ENV, ScriptCommandKind, ScriptControl,
                ScriptDialogState, ScriptExecuteRequest, ScriptExecutionOutcome,
                ScriptInitializeRequest, ScriptInputAction, ScriptPointerButton,
                ScriptPointerEvent, ScriptPointerPhase, ScriptStick, ScriptTkEvent,
                ScriptWorkerStatus,
            };
        }
    }

    /// Process launch, supervision, and stop values used by worker tests.
    pub mod supervisor {
        pub use crate::worker::supervisor::{
            ManagedWorker, StopPurpose, SupervisorError, WorkerLaunch, WorkerSupervisor,
        };
    }

    pub use crate::worker::WorkerKind;
}
