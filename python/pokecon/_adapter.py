"""Rust-core adapter — bridge between Python wrappers and Rust-backed implementations.

This module provides:

- ``_RustCoreAdapter`` (injected by **CommandMeta** into the MRO of every
  user-script subclass)
- Type-conversion helpers for normalizing Python wrapper types to Rust types
- Error-handling utilities for Rust-Python interop
- Module-level adapter functions that delegate to Rust PyO3 bindings when
  available, falling back to pure-Python implementations otherwise.

Architecture
~~~~~~~~~~~~
::

    UserScript.press()
        └── _RustCoreAdapter.press()      ← injected by CommandMeta
                ├── [Rust available] → Rust PyKeyManager / PythonCommand
                └── [Rust missing]  → super().press()  (Python impl)
"""

from __future__ import annotations

import importlib as _importlib
import importlib.util as _importlib_util
import logging
import threading
import time
from typing import (
    TYPE_CHECKING,
    Any,
)

if TYPE_CHECKING:
    pass

_logger: logging.Logger = logging.getLogger(__name__)

# ===================================================================
# Rust availability detection
# ===================================================================

# The Rust ``pokecon`` extension registers submodules including
# ``pokecon.command`` (no trailing 's').  The pure-Python package has
# ``pokecon.commands`` (with 's') and ``pokecon.keys`` (pure Python).
# We probe for ``pokecon.command`` to detect whether the Rust extension
# is actually installed.
_RUST_CORE_AVAILABLE: bool = _importlib_util.find_spec("pokecon.command") is not None

# ===================================================================
# Lazy import helpers
# ===================================================================

_RUST_MODULE_CACHE: dict[str, Any] = {}
_RUST_MODULE_LOCK: threading.Lock = threading.Lock()


def _get_rust_module(name: str) -> Any:
    """Lazy-import a Rust submodule of the ``pokecon`` extension.

    Results are cached so that repeated access is cheap.
    Raises ``ImportError`` if the Rust extension is not installed.
    """
    if name in _RUST_MODULE_CACHE:
        return _RUST_MODULE_CACHE[name]

    with _RUST_MODULE_LOCK:
        if name in _RUST_MODULE_CACHE:  # double-check
            return _RUST_MODULE_CACHE[name]
        try:
            mod = _importlib.import_module(f"pokecon.{name}")
            _RUST_MODULE_CACHE[name] = mod
            return mod
        except ImportError:
            raise


def _rust_available() -> bool:
    """Return ``True`` if the Rust ``pokecon`` extension is importable."""
    return _RUST_CORE_AVAILABLE


# ===================================================================
# Error handling
# ===================================================================


class RustBindingError(RuntimeError):
    """Raised when a Rust binding operation fails.

    This wraps Rust-side errors (``PyRuntimeError``, ``PyValueError``, etc.)
    into a consistent Python exception that carries context about which
    operation failed.
    """


def _wrap_rust_error(err: Exception, context: str = "") -> RustBindingError:
    """Convert a Rust ``PyErr`` (or any Exception) to a ``RustBindingError``.

    Parameters
    ----------
    err : Exception
        The original exception (typically a ``PyErr`` subclass raised by PyO3).
    context : str
        Optional human-readable description of the operation that failed.

    Returns
    -------
    RustBindingError
        A new exception with a combined message.
    """
    msg = str(err)
    if context:
        msg = f"{context}: {msg}"
    return RustBindingError(msg)


# ===================================================================
# Type conversion helpers
# ===================================================================


def _rust_to_python_button(rust_btn: Any) -> Any:
    """Convert a Rust ``PyButton`` to a Python ``Button``.

    When the Rust extension is installed, ``pokecon.keys.Button`` *is* the
    Rust ``PyButton`` class.  This function is a no-op identity pass-through
    for forward compatibility.
    """
    return rust_btn


def _rust_to_python_hat(rust_hat: Any) -> Any:
    """Convert a Rust ``PyHat`` to a Python ``Hat`` (identity passthrough)."""
    return rust_hat


def _python_to_rust_key_type(py_obj: Any) -> Any:
    """Normalize a Python key-type object to the Rust equivalent.

    When both the pure-Python *and* Rust ``pokecon.keys`` modules define
    ``Button``, ``Hat``, etc., user code may hold references to either set
    of types.  This helper ensures that what gets passed to Rust bindings
    is a Rust-native type.

    If the object is already a Rust type (or if Rust is not available),
    it is returned unchanged.
    """
    if not _RUST_CORE_AVAILABLE:
        return py_obj
    # The Rust extension's types *are* what gets imported from
    # ``pokecon.keys`` when ``_RUST_KEYS_AVAILABLE`` is True; Python wrapper
    # code checks for this and defines pure-Python fallbacks only when Rust
    # is missing.  So under normal circumstances py_obj is already a Rust type.
    return py_obj


def _gamepad_inputs_from_args(
    *args: Any,
    **kwargs: Any,
) -> list[Any]:
    """Build a list of Rust ``GamepadInput`` values from positional/keyword args.

    This is a no-op placeholder for future use when the adapter needs to
    construct ``GamepadInput`` enums to pass to Rust ``PyKeyManager``.
    Currently, the Rust ``PyKeyManager.press()`` / ``hold()`` methods accept
    Python ``Button`` / ``Hat`` / ``Direction`` / ``Touchscreen`` objects
    directly and convert them internally via ``pyany_to_gamepad_inputs()``.
    """
    return list(args)


# ===================================================================
# Rust key-type inspection helpers
# ===================================================================


def _is_rust_button(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyButton`` instance."""
    if not _RUST_CORE_AVAILABLE:
        return False
    try:
        keys_mod = _get_rust_module("keys")
        return isinstance(obj, keys_mod.Button)
    except (ImportError, AttributeError):
        return False


def _is_rust_hat(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyHat`` instance."""
    if not _RUST_CORE_AVAILABLE:
        return False
    try:
        keys_mod = _get_rust_module("keys")
        return isinstance(obj, keys_mod.Hat)
    except (ImportError, AttributeError):
        return False


def _is_rust_direction(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyDirection`` instance."""
    if not _RUST_CORE_AVAILABLE:
        return False
    try:
        keys_mod = _get_rust_module("keys")
        return isinstance(obj, keys_mod.Direction)
    except (ImportError, AttributeError):
        return False


def _is_rust_stick(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyStick`` instance."""
    if not _RUST_CORE_AVAILABLE:
        return False
    try:
        keys_mod = _get_rust_module("keys")
        return isinstance(obj, keys_mod.Stick)
    except (ImportError, AttributeError):
        return False


def _is_rust_touchscreen(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyTouchscreen`` instance."""
    if not _RUST_CORE_AVAILABLE:
        return False
    try:
        keys_mod = _get_rust_module("keys")
        return isinstance(obj, keys_mod.Touchscreen)
    except (ImportError, AttributeError):
        return False


# ===================================================================
# Module adapter functions
# ===================================================================


# ── Sender adapter ────────────────────────────────────────────────


def create_sender(is_show_serial: bool = False) -> Any:
    """Create a serial ``Sender`` instance.

    When Rust is available, creates a Rust-backed ``Sender`` from
    ``pokecon.sender.Sender``.  Otherwise, falls back to the pure-Python
    ``Commands.Sender.Sender``.

    Parameters
    ----------
    is_show_serial : bool
        If ``True``, print sent data to stdout (default ``False``).

    Returns
    -------
    Sender
        A sender instance compatible with the original ``KeyPress`` API.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            sender_mod = _get_rust_module("sender")
            return sender_mod.Sender(is_show_serial)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust Sender not available, falling back: %s",
                exc,
            )
            raise

    # Pure-Python fallback
    from Commands.Sender import (
        Sender as _PySender,  # type: ignore[import-untyped]  # noqa: PLC0415
    )

    return _PySender(is_show_serial)


def create_key_manager() -> Any:
    """Create a ``KeyManager`` instance.

    When Rust is available, creates a Rust ``KeyManager`` from
    ``pokecon.keys.KeyManager`` (which wraps the Rust ``PyKeyManager``).
    Otherwise, falls back to a pure-Python ``SendFormat``-based key manager.

    Returns
    -------
    KeyManager
        A key manager instance compatible with the original ``KeyPress`` API.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            keys_mod = _get_rust_module("keys")
            return keys_mod.KeyManager()
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust KeyManager not available, falling back: %s",
                exc,
            )
            raise

    msg = (
        "KeyManager requires the Rust extension (pure-Python fallback not implemented)"
    )
    raise RustBindingError(msg)


# ── Command adapter ────────────────────────────────────────────────


def create_command(script_dir: str) -> Any:
    """Create a ``Command`` (CommandManager) instance.

    When Rust is available, creates a ``pokecon.command.Command``
    (the Rust-backed ``CommandManager``).  Otherwise raises an error.

    Parameters
    ----------
    script_dir : str
        Path to the scripts directory.

    Returns
    -------
    Command
        A command manager instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            cmd_mod = _get_rust_module("command")
            return cmd_mod.Command(script_dir)
        except (ImportError, AttributeError) as exc:
            raise RustBindingError(
                f"Failed to create Rust Command: {exc}",
            ) from exc

    msg = "Command requires the Rust extension"
    raise RustBindingError(msg)


# ── Event bus adapter ──────────────────────────────────────────────


def create_event_bus() -> Any:
    """Create an ``EventBus`` instance.

    When Rust is available, creates a ``pokecon.events.EventBus``
    (the Rust-backed synchronous event bus).  Otherwise falls back to a
    pure-Python implementation.

    Returns
    -------
    EventBus
        An event bus instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            events_mod = _get_rust_module("events")
            return events_mod.EventBus()
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust EventBus not available, falling back: %s",
                exc,
            )
            raise

    msg = "EventBus requires the Rust extension"
    raise RustBindingError(msg)


# ── Notification adapters ──────────────────────────────────────────


def create_notification(message: str) -> Any:
    """Create a ``Notification`` instance.

    When Rust is available, creates a ``pokecon.notify.Notification``.
    Otherwise falls back to a simple pure-Python notification object.

    Parameters
    ----------
    message : str
        Notification body text.

    Returns
    -------
    Notification
        A notification instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            notify_mod = _get_rust_module("notify")
            return notify_mod.Notification(message)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust Notification not available, falling back: %s",
                exc,
            )
            raise

    msg = "Notification requires the Rust extension"
    raise RustBindingError(msg)


def create_desktop_notifier(app_name: str) -> Any:
    """Create a ``DesktopNotifier`` instance.

    When Rust is available, creates a ``pokecon.notify.DesktopNotifier``.
    Otherwise falls back to ``plyer.notification`` if available.

    Parameters
    ----------
    app_name : str
        Application name for the notification source.

    Returns
    -------
    DesktopNotifier
        A desktop notifier instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            notify_mod = _get_rust_module("notify")
            return notify_mod.DesktopNotifier(app_name)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust DesktopNotifier not available, falling back: %s",
                exc,
            )
            raise

    msg = "DesktopNotifier requires the Rust extension"
    raise RustBindingError(msg)


def create_discord_notifier(webhook_url: str) -> Any:
    """Create a ``DiscordNotifier`` instance.

    Parameters
    ----------
    webhook_url : str
        Discord webhook URL.

    Returns
    -------
    DiscordNotifier
        A Discord notifier instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            notify_mod = _get_rust_module("notify")
            return notify_mod.DiscordNotifier(webhook_url)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust DiscordNotifier not available, falling back: %s",
                exc,
            )
            raise

    msg = "DiscordNotifier requires the Rust extension"
    raise RustBindingError(msg)


def send_line_notification(
    access_token: str,
    message: str,
    title: str | None = None,
) -> None:
    """Send a LINE notification via Rust bindings (deprecated, no-op stub).

    Parameters
    ----------
    access_token : str
        LINE Notify access token (unused).
    message : str
        Notification body text.
    title : str, optional
        Notification title (ignored).
    """
    if _RUST_CORE_AVAILABLE:
        try:
            notify_mod = _get_rust_module("notify")
            notify_mod.send_line(access_token, message, title)
            return
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust send_line not available: %s",
                exc,
            )
            return

    _logger.info("LINE notification skipped (no Rust extension)")


def send_desktop_notification(
    app_name: str,
    message: str,
    title: str | None = None,
) -> None:
    """Send a native desktop notification via Rust bindings.

    Parameters
    ----------
    app_name : str
        Application name shown as the notification source.
    message : str
        Main body text of the notification.
    title : str, optional
        Optional notification title.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            notify_mod = _get_rust_module("notify")
            notify_mod.send_desktop(app_name, message, title)
            return
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust send_desktop not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback via plyer
    try:
        from plyer import notification  # type: ignore[import-untyped]  # noqa: PLC0415

        kwargs: dict[str, Any] = {
            "title": title or app_name,
            "message": message,
            "timeout": 5,
        }
        notification.notify(**kwargs)
    except Exception as exc:
        _logger.warning("Desktop notification failed (plyer): %s", exc)


def send_discord_notification(
    webhook_url: str,
    message: str,
    title: str | None = None,
) -> None:
    """Send a Discord notification via Rust bindings.

    Parameters
    ----------
    webhook_url : str
        Discord webhook URL.
    message : str
        Message content.
    title : str, optional
        Optional embed title.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            notify_mod = _get_rust_module("notify")
            notify_mod.send_discord(webhook_url, message, title)
            return
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust send_discord not available: %s",
                exc,
            )
            raise

    msg = "send_discord requires the Rust extension"
    raise RustBindingError(msg)


# ── Network adapters ────────────────────────────────────────────────


def create_http_client(timeout: float | None = None) -> Any:
    """Create an ``HttpClient`` instance.

    Parameters
    ----------
    timeout : float, optional
        Default request timeout in seconds.

    Returns
    -------
    HttpClient
        An HTTP client instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            net_mod = _get_rust_module("net")
            return net_mod.HttpClient(timeout)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust HttpClient not available: %s",
                exc,
            )
            raise

    msg = "HttpClient requires the Rust extension"
    raise RustBindingError(msg)


def create_socket_client(recv_timeout: float | None = None) -> Any:
    """Create a ``SocketClient`` instance.

    Parameters
    ----------
    recv_timeout : float, optional
        Receive timeout in seconds.

    Returns
    -------
    SocketClient
        A socket client instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            net_mod = _get_rust_module("net")
            return net_mod.SocketClient(recv_timeout)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust SocketClient not available: %s",
                exc,
            )
            raise

    msg = "SocketClient requires the Rust extension"
    raise RustBindingError(msg)


def create_websocket_client() -> Any:
    """Create a ``WebSocketClient`` instance.

    Returns
    -------
    WebSocketClient
        A WebSocket client instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            net_mod = _get_rust_module("net")
            return net_mod.WebSocketClient()
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust WebSocketClient not available: %s",
                exc,
            )
            raise

    msg = "WebSocketClient requires the Rust extension"
    raise RustBindingError(msg)


def create_mqtt_client(
    host: str,
    port: int,
    client_id: str | None = None,
    username: str | None = None,
    password: str | None = None,
    keep_alive: int = 60,
    clean_session: bool = True,
) -> Any:
    """Create an ``MqttClient`` instance.

    Parameters
    ----------
    host : str
        MQTT broker hostname.
    port : int
        MQTT broker port.
    client_id : str, optional
        Custom client ID.
    username : str, optional
        Username for authentication.
    password : str, optional
        Password for authentication.
    keep_alive : int
        Keep-alive interval in seconds (default: 60).
    clean_session : bool
        Clean session flag (default: True).

    Returns
    -------
    MqttClient
        An MQTT client instance.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            net_mod = _get_rust_module("net")
            return net_mod.MqttClient(
                host,
                port,
                client_id=client_id,
                username=username,
                password=password,
                keep_alive=keep_alive,
                clean_session=clean_session,
            )
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust MqttClient not available: %s",
                exc,
            )
            raise

    msg = "MqttClient requires the Rust extension"
    raise RustBindingError(msg)


# ── Image processing adapters ────────────────────────────────────────


def image_load(path: str) -> Any:
    """Load an image from file via Rust bindings.

    Parameters
    ----------
    path : str
        Path to the image file.

    Returns
    -------
    numpy.ndarray
        HxWx3 RGB numpy array.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.load(path)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust image load not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415

    img = cv2.imread(path)
    if img is None:
        raise RustBindingError(f"Failed to load image: {path}")
    return cv2.cvtColor(img, cv2.COLOR_BGR2RGB)


def image_save(path: str, image: Any) -> None:
    """Save an image to file via Rust bindings.

    Parameters
    ----------
    path : str
        Output file path.
    image : numpy.ndarray
        HxWx3 RGB numpy array.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.save(path, image)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust image save not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415

    bgr = cv2.cvtColor(image, cv2.COLOR_RGB2BGR)
    cv2.imwrite(path, bgr)


def image_resize(
    image: Any,
    width: int,
    height: int,
) -> Any:
    """Resize an image via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWxC numpy array.
    width : int
        Target width.
    height : int
        Target height.

    Returns
    -------
    numpy.ndarray
        Resized image.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.resize(image, width, height)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust image resize not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415

    return cv2.resize(image, (width, height), interpolation=cv2.INTER_NEAREST)


def image_crop(
    image: Any,
    x: int,
    y: int,
    width: int,
    height: int,
) -> Any:
    """Crop a region from an image via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWxC numpy array.
    x, y : int
        Top-left corner of the crop region.
    width, height : int
        Dimensions of the crop region.

    Returns
    -------
    numpy.ndarray
        Cropped image.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.crop(image, x, y, width, height)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust image crop not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV or numpy slicing
    return image[y : y + height, x : x + width]


def image_grayscale(image: Any) -> Any:
    """Convert an image to grayscale via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWx3 RGB numpy array.

    Returns
    -------
    numpy.ndarray
        HxWx1 grayscale array.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.grayscale(image)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust image grayscale not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415

    gray = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY)
    return gray[..., None]  # add channel dimension


def image_template_match(
    image: Any,
    template: Any,
    threshold: float = 0.7,
) -> list[tuple[int, int, float]]:
    """Find template matches in an image via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWx3 RGB image.
    template : numpy.ndarray
        HxWx3 RGB template.
    threshold : float
        Minimum confidence threshold (default: 0.7).

    Returns
    -------
    list[tuple[int, int, float]]
        List of ``(x, y, confidence)`` tuples for each match.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.template_match(image, template, threshold)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust template_match not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415
    import numpy as np  # noqa: PLC0415

    gray_img = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY)
    gray_tpl = cv2.cvtColor(template, cv2.COLOR_RGB2GRAY)
    result = cv2.matchTemplate(gray_img, gray_tpl, cv2.TM_CCOEFF_NORMED)
    locations = np.where(result >= threshold)
    matches: list[tuple[int, int, float]] = []
    for pt in zip(*locations[::-1]):
        conf = float(result[pt[1], pt[0]])
        matches.append((int(pt[0]), int(pt[1]), conf))
    return matches


def image_template_match_best(
    image: Any,
    template: Any,
    threshold: float = 0.7,
) -> tuple[int, int, float] | None:
    """Find the single best template match via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWx3 RGB image.
    template : numpy.ndarray
        HxWx3 RGB template.
    threshold : float
        Minimum confidence threshold (default: 0.7).

    Returns
    -------
    tuple[int, int, float] or None
        ``(x, y, confidence)`` of the best match, or ``None``.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.template_match_best(image, template, threshold)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust template_match_best not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415

    gray_img = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY)
    gray_tpl = cv2.cvtColor(template, cv2.COLOR_RGB2GRAY)
    result = cv2.matchTemplate(gray_img, gray_tpl, cv2.TM_CCOEFF_NORMED)
    _, max_val, _, max_loc = cv2.minMaxLoc(result)
    if max_val >= threshold:
        return (int(max_loc[0]), int(max_loc[1]), float(max_val))
    return None


def image_in_range(
    image: Any,
    lower: tuple[int, int, int],
    upper: tuple[int, int, int],
) -> Any:
    """Detect pixels within a BGR color range via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWx3 image.
    lower : tuple[int, int, int]
        Lower BGR bounds.
    upper : tuple[int, int, int]
        Upper BGR bounds.

    Returns
    -------
    numpy.ndarray
        Binary mask (HxWx1, values 0 or 255).
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.in_range(image, list(lower), list(upper))
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust in_range not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415
    import numpy as np  # noqa: PLC0415

    lower_arr = np.array(lower, dtype=np.uint8)
    upper_arr = np.array(upper, dtype=np.uint8)
    mask = cv2.inRange(image, lower_arr, upper_arr)
    return mask[..., None]


def image_threshold(
    image: Any,
    value: int,
) -> Any:
    """Apply a binary threshold via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWxC image (converted to grayscale if needed).
    value : int
        Threshold value (pixels >= value become 255).

    Returns
    -------
    numpy.ndarray
        Binary mask (HxWx1).
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.threshold(image, value)
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust threshold not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: use OpenCV
    import cv2  # type: ignore[import-untyped]  # noqa: PLC0415

    gray = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY) if image.shape[2] == 3 else image
    _, binary = cv2.threshold(gray, value, 255, cv2.THRESH_BINARY)
    return binary[..., None]


def image_preprocess(
    image: Any,
    crop_region: tuple[int, int, int, int] | None = None,
    grayscale: bool = False,
    lower: tuple[int, int, int] | None = None,
    upper: tuple[int, int, int] | None = None,
    threshold: int | None = None,
) -> Any:
    """Apply the full preprocessing pipeline via Rust bindings.

    Parameters
    ----------
    image : numpy.ndarray
        HxWxC input image.
    crop_region : tuple, optional
        ``(x, y, w, h)`` crop rectangle.
    grayscale : bool
        Convert to grayscale (default: False).
    lower, upper : tuple, optional
        BGR in-range bounds for binarization (overrides grayscale).
    threshold : int, optional
        Binary threshold value.

    Returns
    -------
    numpy.ndarray
        Processed image.
    """
    if _RUST_CORE_AVAILABLE:
        try:
            ip_mod = _get_rust_module("image_proc")
            return ip_mod.preprocess(
                image,
                crop_region=crop_region,
                grayscale=grayscale,
                lower=list(lower) if lower else None,
                upper=list(upper) if upper else None,
                threshold_val=threshold,
            )
        except (ImportError, AttributeError) as exc:
            _logger.warning(
                "Rust preprocess not available: %s",
                exc,
            )
            raise

    # Pure-Python fallback: chain the operations step by step
    img = image

    # 1. Crop
    if crop_region is not None:
        x, y, w, h = crop_region
        img = img[y : y + h, x : x + w]

    # 2. Grayscale or in-range
    if lower is not None and upper is not None:
        img = image_in_range(img, lower, upper)
    elif grayscale:
        img = image_grayscale(img)

    # 3. Threshold
    if threshold is not None:
        img = image_threshold(img, threshold)

    return img


# ===================================================================
# _RustCoreAdapter — mixin class injected by CommandMeta
# ===================================================================


class _RustCoreAdapter:
    """Transparent mixin injected into the MRO by ``CommandMeta``.

    This class sits between the user's script class and the registered
    implementation.  When a method is called on ``self``, the MRO lookup
    reaches this class before the implementation.

    For every method, the adapter:
    1. Checks if ``_use_rust`` is ``True`` (Rust extension available).
    2. If yes, delegates to the Rust-backed implementation.
    3. If no, passes through via ``super()`` to the Python implementation.

    NOTE
    ----
    This class is **never** referenced by user code directly.  It is
    injected automatically by ``CommandMeta.__new__``.
    """

    # NOTE: No ``__slots__`` — this class participates in cooperative MRO
    # and may be combined with classes that have arbitrary instance dicts.

    def __init__(self, *args: object, **kwargs: object) -> None:
        self._use_rust: bool = _RUST_CORE_AVAILABLE
        super().__init__(*args, **kwargs)

    # ── Public status properties ──────────────────────────────────────────

    @property
    def available(self) -> bool:
        """``True`` when the Rust ``pokecon`` extension is importable."""
        return self._use_rust

    @property
    def rust_available(self) -> bool:
        """Alias for :attr:`available`."""
        return self._use_rust

    # ── Key methods pass-through (for future Rust integration) ─────────────

    # ── Key methods transparent passthrough ───────────────────────────────

    def press(self, *args: object, **kwargs: object) -> None:
        """Send a button press.

        When Rust is available, delegates to Rust ``PyKeyManager`` via
        :func:`create_key_manager`.  Otherwise, falls back to the Python
        implementation (via ``super()`` or ``self.keys``).

        Accepts any arguments matching the original ``KeyPress.press()`` API
        or the Rust ``PyKeyManager.press()`` API, depending on availability.
        """
        if self._use_rust:
            try:
                import pokecon.keys as _keys  # noqa: PLC0415

                km = _keys.KeyManager()
                km.press(*args, **kwargs)
                return
            except (ImportError, AttributeError) as exc:
                _logger.warning("Rust press not available: %s", exc)
                pass

        # Try super() first (cooperative MRO)
        super_result = getattr(super(), "press", None)
        if super_result is not None:
            super_result(*args, **kwargs)
            return
        # Fall back to self.keys (traditional delegation)
        if hasattr(self, "keys") and self.keys is not None:
            self.keys.press(*args, **kwargs)
            return
        _logger.debug("press(%s, %s) — no implementation", args, kwargs)

    def hold(self, *args: object, **kwargs: object) -> None:
        """Hold a button or combination of inputs.

        Accepts the same arguments as the original ``KeyPress.hold()``
        (e.g. ``hold(button, wait=0.01)``) or Rust ``PyKeyManager.hold()``
        (e.g. ``hold(inputs, duration=0.1)``).
        """
        if self._use_rust:
            try:
                import pokecon.keys as _keys  # noqa: PLC0415

                km = _keys.KeyManager()
                km.hold(*args, **kwargs)
                return
            except (ImportError, AttributeError) as exc:
                _logger.warning("Rust hold not available: %s", exc)
                pass

        super_result = getattr(super(), "hold", None)
        if super_result is not None:
            super_result(*args, **kwargs)
            return
        if hasattr(self, "keys") and self.keys is not None:
            self.keys.hold(*args, **kwargs)
            return
        _logger.debug("hold(%s, %s) — no implementation", args, kwargs)

    def hold_end(self, *args: object, **kwargs: object) -> None:
        """Release all held buttons.

        Accepts the same arguments as the original ``KeyPress.holdEnd()``
        (e.g. ``hold_end(duration=0.1)``).
        """
        if self._use_rust:
            try:
                import pokecon.keys as _keys  # noqa: PLC0415

                km = _keys.KeyManager()
                km.release_all(*args, **kwargs)
                return
            except (ImportError, AttributeError) as exc:
                _logger.warning("Rust hold_end not available: %s", exc)
                pass

        super_result = getattr(super(), "hold_end", None)
        if super_result is not None:
            super_result(*args, **kwargs)
            return
        if hasattr(self, "keys") and self.keys is not None:
            self.keys.holdEnd(*args, **kwargs)
            return
        _logger.debug("hold_end(%s, %s) — no implementation", args, kwargs)

    def wait(self, wait_time: float) -> None:
        """Sleep for the given duration."""
        time.sleep(wait_time)

    def short_wait(self) -> None:
        """Sleep for 0.1 seconds."""
        time.sleep(0.1)

    def finish(self) -> None:
        """Finish the command: release all buttons."""
        if self._use_rust:
            try:
                import pokecon.keys as _keys  # noqa: PLC0415

                km = _keys.KeyManager()
                km.release_all()
                return
            except (ImportError, AttributeError) as exc:
                _logger.warning("Rust finish not available: %s", exc)
                pass

        super_result = getattr(super(), "finish", None)
        if super_result is not None:
            super_result()
            return
        _logger.debug("finish() — no implementation")

    # ── Serial bridge methods ──────────────────────────────────────────────

    def press_button(self, button_name: str, duration_ms: int) -> None:
        """Send a button-press via Rust serial bindings.

        Parameters
        ----------
        button_name : str
            Name of the button (e.g. ``\"A\"``, ``\"B\"``).
        duration_ms : int
            Hold duration in milliseconds.

        Raises
        ------
        NotImplementedError
            If Rust bindings are not yet wired.
        """
        if self._use_rust:
            try:
                import pokecon.command as _cmd  # noqa: PLC0415

                _cmd.press_button(button_name, duration_ms)
                return
            except (ImportError, AttributeError):
                msg = "Rust serial bindings not yet wired"
                raise NotImplementedError(msg) from None

        msg = "Rust serial bindings not available"
        raise NotImplementedError(msg)

    # ── Notification bridge methods ────────────────────────────────────────

    def line_text(self, text: str, token: str | None = None) -> None:
        """Send a LINE notification via Rust bindings.

        Parameters
        ----------
        text : str
            Message text to send.
        token : str, optional
            LINE channel access token.
        """
        if self._use_rust:
            try:
                import pokecon.notify as _notify  # noqa: PLC0415

                _notify.send_line(
                    access_token=token or "",
                    message=text,
                )
                return
            except (ImportError, AttributeError) as exc:
                _logger.warning("Rust line_text not available: %s", exc)
                pass

        super_result = getattr(super(), "line_text", None)
        if super_result is not None:
            super_result(text, token)
            return
        _logger.debug("line_text(%s) — no implementation", text)

    def discord_text(self, text: str, webhook_url: str | None = None) -> None:
        """Send a Discord notification via Rust bindings.

        Parameters
        ----------
        text : str
            Message content.
        webhook_url : str, optional
            Discord webhook URL.
        """
        if self._use_rust:
            try:
                import pokecon.notify as _notify  # noqa: PLC0415

                _notify.send_discord(
                    webhook_url=webhook_url or "",
                    message=text,
                )
                return
            except (ImportError, AttributeError) as exc:
                _logger.warning("Rust discord_text not available: %s", exc)
                pass

        super_result = getattr(super(), "discord_text", None)
        if super_result is not None:
            super_result(text, webhook_url)
            return
        _logger.debug("discord_text(%s) — no implementation", text)

    # ── Image processing bridge methods ────────────────────────────────────

    def template_match(
        self,
        template_path: str,
        threshold: float = 0.7,
    ) -> tuple[bool, tuple[int, int], float]:
        """Run template matching via Rust CV bindings.

        Parameters
        ----------
        template_path : str
            Path to the template image file.
        threshold : float
            Minimum confidence (default: 0.7).

        Returns
        -------
        tuple[bool, tuple[int, int], float]
            ``(found, (x, y), confidence)``.

        Raises
        ------
        NotImplementedError
            If Rust CV bindings are not yet wired.
        """
        if self._use_rust:
            try:
                import pokecon.image_proc as _ip  # noqa: PLC0415

                return _ip.template_match(template_path, threshold)
            except (ImportError, AttributeError):
                msg = "Rust CV bindings not yet wired"
                raise NotImplementedError(msg) from None

        msg = "Rust CV bindings not available"
        raise NotImplementedError(msg)


# ===================================================================
# Public aliases for internal helpers
# ===================================================================

#: ``True`` when the Rust extension is available.
RUST_CORE_AVAILABLE: bool = _RUST_CORE_AVAILABLE


def rust_available() -> bool:
    """Return ``True`` if the Rust ``pokecon`` extension is importable."""
    return _rust_available()


def rust_to_python_button(rust_btn: Any) -> Any:
    """Convert a Rust ``PyButton`` to a Python ``Button`` (identity pass)."""
    return _rust_to_python_button(rust_btn)


def rust_to_python_hat(rust_hat: Any) -> Any:
    """Convert a Rust ``PyHat`` to a Python ``Hat`` (identity pass)."""
    return _rust_to_python_hat(rust_hat)


def python_to_rust_key_type(py_obj: Any) -> Any:
    """Normalize a Python key-type object to the Rust equivalent."""
    return _python_to_rust_key_type(py_obj)


def gamepad_inputs_from_args(*args: Any, **kwargs: Any) -> list[Any]:
    """Build Rust ``GamepadInput`` values from positional/keyword args."""
    return _gamepad_inputs_from_args(*args, **kwargs)


def is_rust_button(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyButton`` instance."""
    return _is_rust_button(obj)


def is_rust_hat(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyHat`` instance."""
    return _is_rust_hat(obj)


def is_rust_direction(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyDirection`` instance."""
    return _is_rust_direction(obj)


def is_rust_stick(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyStick`` instance."""
    return _is_rust_stick(obj)


def is_rust_touchscreen(obj: Any) -> bool:
    """Return ``True`` if *obj* is a Rust ``PyTouchscreen`` instance."""
    return _is_rust_touchscreen(obj)


__all__ = [
    # Core adapter class (used by CommandMeta)
    "_RustCoreAdapter",
    # Status flag
    "RUST_CORE_AVAILABLE",
    "rust_available",
    # Error types
    "RustBindingError",
    # Type conversion helpers
    "rust_to_python_button",
    "rust_to_python_hat",
    "python_to_rust_key_type",
    "gamepad_inputs_from_args",
    # Type inspection helpers
    "is_rust_button",
    "is_rust_hat",
    "is_rust_direction",
    "is_rust_stick",
    "is_rust_touchscreen",
    # Module adapter functions — Sender
    "create_sender",
    "create_key_manager",
    # Module adapter functions — Command
    "create_command",
    # Module adapter functions — Events
    "create_event_bus",
    # Module adapter functions — Notifications
    "create_notification",
    "create_desktop_notifier",
    "create_discord_notifier",
    "send_line_notification",
    "send_desktop_notification",
    "send_discord_notification",
    # Module adapter functions — Networking
    "create_http_client",
    "create_socket_client",
    "create_websocket_client",
    "create_mqtt_client",
    # Module adapter functions — Image processing
    "image_load",
    "image_save",
    "image_resize",
    "image_crop",
    "image_grayscale",
    "image_template_match",
    "image_template_match_best",
    "image_in_range",
    "image_threshold",
    "image_preprocess",
]
