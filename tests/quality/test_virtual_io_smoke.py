from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
SMOKE_SCRIPT = REPOSITORY / "scripts/integration/virtual-io-smoke.sh"


def _script_text() -> str:
    return SMOKE_SCRIPT.read_text(encoding="utf-8")


def test_smoke_script_establishes_development_resource_provenance() -> None:
    text = _script_text()
    assert "export POKECON_RESOURCE_PROVENANCE=development" in text
    export_position = text.index("export POKECON_RESOURCE_PROVENANCE=development")
    first_cargo = text.index("cargo test")
    assert export_position < first_cargo


def test_smoke_script_waits_for_device_node_after_modprobe() -> None:
    text = _script_text()
    assert "did not appear after loading v4l2loopback" in text


def test_smoke_script_detects_writer_exit_before_claiming_readiness() -> None:
    text = _script_text()
    assert "writer_failed=true" in text
    assert "the virtual camera writer exited before" in text
    assert (
        "the virtual camera writer exited before the integration tests started" in text
    )


def test_smoke_script_still_cleans_up_writer_module_and_temp_dir() -> None:
    text = _script_text()
    assert 'kill "$writer_pid"' in text
    assert '"$modprobe_bin" -r v4l2loopback' in text
    assert '"$modprobe_bin" -r videodev' in text
    assert "v4l2loopback_loaded_by_script" in text
    assert "videodev_loaded_by_script" in text
    assert "could not unload v4l2loopback loaded by this script" in text
    assert "could not unload videodev loaded by this script" in text
    assert text.index('"$modprobe_bin" -r v4l2loopback') < text.index(
        '"$modprobe_bin" -r videodev'
    )
    assert 'rm -rf -- "$temp_dir"' in text


def test_smoke_script_preserves_preexisting_modules() -> None:
    text = _script_text()
    assert "already loaded but" in text
    assert "/sys/module/videodev" in text


def test_smoke_script_passes_shell_syntax_check() -> None:
    bash = shutil.which("bash")
    assert bash is not None
    result = subprocess.run(  # noqa: S603 - fixed bash syntax-check arguments
        [bash, "-n", str(SMOKE_SCRIPT)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
