#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::env;

use pokecon::camera::{
    CameraBackend as _, CameraConfig, CameraSelector, CaptureResolution, NativeCameraBackend,
};

#[test]
#[ignore = "requires a v4l2loopback writer and POKECON_V4L2_INDEX"]
fn native_v4l2_backend_enumerates_opens_and_decodes_a_virtual_camera() {
    let index = env::var("POKECON_V4L2_INDEX")
        .expect("POKECON_V4L2_INDEX must identify the prepared loopback camera")
        .parse::<u32>()
        .expect("POKECON_V4L2_INDEX must be an unsigned integer");
    let selector = CameraSelector::Index(index);
    let backend = NativeCameraBackend;
    let devices = backend
        .enumerate(Some(&selector))
        .expect("native V4L2 enumeration must succeed");
    assert!(
        devices
            .iter()
            .any(|device| device.selector == selector && device.available),
        "the loopback camera must be enumerated as available"
    );

    let config = CameraConfig::new(selector, 30, CaptureResolution::R640x360)
        .expect("the virtual camera configuration must be valid");
    let mut session = backend
        .open(&config)
        .expect("native V4L2 backend must open the loopback camera");
    assert_eq!(session.effective_config().effective_fps(), 30);
    let frame = session
        .read_frame()
        .expect("native V4L2 backend must decode a complete frame");
    assert_eq!(frame.size(), CaptureResolution::R640x360.size());
    assert_eq!(
        frame.pixels().len(),
        CaptureResolution::R640x360.size().byte_len()
    );
    let sampled_colors = frame
        .pixels()
        .chunks_exact(3)
        .step_by(997)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect::<BTreeSet<_>>();
    assert!(
        sampled_colors.len() > 8,
        "the decoded test pattern must contain multiple colors"
    );
    assert!(
        sampled_colors
            .iter()
            .any(|[blue, green, red]| { blue != green || green != red || blue != red }),
        "the decoded test pattern must preserve color channels"
    );

    let effective = session
        .reconfigure(30, CaptureResolution::R640x360)
        .expect("same-device V4L2 reconfiguration must succeed");
    assert_eq!(effective.effective_fps(), 30);
    session
        .read_frame()
        .expect("capture must continue after V4L2 reconfiguration");
    session.close();
}
