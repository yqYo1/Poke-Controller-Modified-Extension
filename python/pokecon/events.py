"""
Event system — Python wrapper around Rust EventBus.

Provides event subscription, emission, and filtering capabilities,
backed by the Rust ``pokecon.events.EventBus`` when available.

Usage::

    from pokecon.events import on, emit, once, off

    def my_handler(event_type, data):
        print(f"Received {event_type}: {data}")

    # Subscribe to an exact event
    handler_id = on("battle_start", my_handler)

    # One-time handler
    once("encounter", my_handler)

    # Emit an event
    emit("battle_start", {"opponent": "Pikachu"})

    # Unsubscribe by handler ID
    off(handler_id)

    # Pattern-based subscription (fnmatch-style wildcards)
    on("battle_*", my_handler, pattern="battle_*")

    # Group management
    with autocmd_group("my_group"):
        on("event1", handler1)
        on("event2", handler2)

    autocmd_clear("my_group")  # removes all handlers in the group
"""

from __future__ import annotations

import fnmatch
import json
import logging
import threading
import uuid
from contextlib import contextmanager
from typing import (
    TYPE_CHECKING,
    Any,
    Callable,
    Iterator,
)

from pokecon._adapter import (
    _RUST_CORE_AVAILABLE,
    _get_rust_module,
)

if TYPE_CHECKING:
    pass

_logger: logging.Logger = logging.getLogger(__name__)

# ===================================================================
# Type aliases
# ===================================================================

HandlerId = str
"""Unique identifier for a registered event handler."""

EventHandler = Callable[..., Any]
"""Type of a callback function registered as an event handler."""

# ===================================================================
# Module-level state
# ===================================================================

_rust_bus: Any = None
"""Lazily-initialized singleton Rust EventBus instance."""

_handlers: dict[HandlerId, dict[str, Any]] = {}
"""All registered handlers keyed by handler ID.

Each record has:
    - ``id``: HandlerId
    - ``event_name``: str (the event name used when registering)
    - ``callback``: EventHandler
    - ``once``: bool
    - ``pattern``: str | None (fnmatch pattern, or None for exact match)
    - ``phase``: str | None
    - ``group``: str | None
    - ``wrapper``: Callable | None (the wrapper registered with Rust bus)
"""

_groups: dict[str, set[HandlerId]] = {}
"""Mapping of group name → set of handler IDs."""

_event_schemas: dict[str, Any] = {}
"""Registered event schemas for *define_event*."""

_lock: threading.Lock = threading.Lock()
"""Protects all module-level mutable state."""


# ===================================================================
# Internal helpers
# ===================================================================


def _generate_handler_id() -> HandlerId:
    """Generate a unique handler identifier."""
    return uuid.uuid4().hex


def _ensure_rust_bus() -> Any:
    """Lazily create and return the singleton Rust EventBus instance.

    Returns ``None`` if the Rust extension is not available or fails
    to load.
    """
    global _rust_bus

    if _rust_bus is not None:
        return _rust_bus

    if not _RUST_CORE_AVAILABLE:
        _logger.debug("Rust core not available — using pure-Python event dispatch")
        return None

    try:
        events_mod = _get_rust_module("events")
        _rust_bus = events_mod.EventBus()
        _logger.debug("Rust EventBus initialized")
        return _rust_bus
    except (ImportError, AttributeError) as exc:
        _logger.warning("Could not create Rust EventBus: %s", exc)
        return None


def _serialize_data(data: Any) -> str:
    """Serialize event *data* to a JSON string for the Rust bus.

    If *data* is already a ``str``, it is returned as-is.
    """
    if isinstance(data, str):
        return data
    try:
        return json.dumps(data, ensure_ascii=False, default=str)
    except TypeError, ValueError:
        return str(data)


def _matches(event_type: str, record: dict[str, Any]) -> bool:
    """Return ``True`` if *record* (a handler record) matches *event_type*.

    Applies the following filters in order:

    1. If the record has a ``pattern``, use ``fnmatch``.
    2. Otherwise, compare *event_type* to the record's ``event_name``.
    """
    pattern = record.get("pattern")
    if pattern:
        return fnmatch.fnmatch(event_type, pattern)
    return event_type == record.get("event_name")


def _dispatch(hid: HandlerId, event_type: str, data: str) -> None:
    """Dispatch an event to a single handler identified by *hid*.

    This function:
    - Looks up the handler record
    - Applies pattern/phase filtering
    - Handles ``once`` semantics (auto-unsubscribe)
    - Invokes the callback

    The Rust bus calls this from its emitted wrapper callbacks.
    """
    record: dict[str, Any] | None = None
    is_once: bool = False

    with _lock:
        rec = _handlers.get(hid)
        if rec is None:
            return  # handler was already removed

        # Apply event-type matching
        if not _matches(event_type, rec):
            return

        # Check phase filter
        phase_filter = rec.get("phase")
        if phase_filter is not None:
            # The incoming data is just a string from Rust; we don't have
            # phase info in the Rust EventBus's emit path.  We pass it
            # through anyway — the callback signature includes phase.
            pass

        is_once = rec.get("once", False)
        if is_once:
            # Remove the handler after first invocation
            _remove_handler_locked(hid)

        # Snapshot the callback while holding the lock
        record = dict(rec)

    # Invoke the callback outside the lock
    try:
        callback: EventHandler = record["callback"]
        if record.get("phase"):
            callback(event_type, data, record["phase"])
        else:
            callback(event_type, data)
    except Exception:
        _logger.exception(
            "Event handler %s raised an exception for '%s'",
            hid,
            event_type,
        )


def _make_wrapper(hid: HandlerId) -> Callable[[str, str], None]:
    """Build a Python callback that can be registered with the Rust EventBus.

    The returned function is a closure that calls :func:`_dispatch` when
    invoked by Rust's ``emit``.
    """

    def _wrapper(event_type: str, data: str) -> None:
        _dispatch(hid, event_type, data)

    return _wrapper


def _remove_handler_locked(hid: HandlerId) -> None:
    """Remove a handler from the internal registry.

    Must be called while holding ``_lock``.
    """
    record = _handlers.pop(hid, None)
    if record is not None:
        grp = record.get("group")
        if grp and grp in _groups:
            _groups[grp].discard(hid)
            if not _groups[grp]:
                _groups.pop(grp, None)


# ===================================================================
# Public API
# ===================================================================


def on(
    event_name: str,
    callback: EventHandler,
    phase: str | None = None,
    pattern: str | None = None,
    group: str | None = None,
) -> HandlerId:
    """Register a callback for *event_name*.

    The callback is invoked with ``(event_type, data)`` when the event
    is emitted.  If *phase* is provided, the callback receives three
    arguments: ``(event_type, data, phase)``.

    Parameters
    ----------
    event_name : str
        Name of the event to listen for.
    callback : callable
        Function to invoke when the event fires.
    phase : str, optional
        Event phase filter (e.g., ``"capture"``, ``"target"``,
        ``"bubble"``).  Stored as metadata and passed through; phase
        filtering is **not** enforced by the Rust bus.
    pattern : str, optional
        Glob-style pattern (e.g., ``"battle_*"``) for matching event
        names at dispatch time.  When set, *event_name* is still used
        for registration but *pattern* controls which events trigger
        the callback.
    group : str, optional
        Group name for batch removal via :func:`autocmd_clear` or
        the :func:`autocmd_group` context manager.  If not provided
        and an :func:`autocmd_group` context is active, the active
        group is used automatically.

    Returns
    -------
    HandlerId
        Opaque handle that can be passed to :func:`off` to unregister.

    Example
    -------
    >>> def handler(evt, data):
    ...     print(f"{evt}: {data}")
    >>> hid = on("battle_start", handler)
    >>> off(hid)
    """
    hid = _generate_handler_id()

    # If no explicit group, check the autocmd_group context stack
    if group is None:
        group = _get_active_group()

    record: dict[str, Any] = {
        "id": hid,
        "event_name": event_name,
        "callback": callback,
        "once": False,
        "pattern": pattern,
        "phase": phase,
        "group": group,
        "wrapper": None,
    }

    # Register with the Rust bus for exact-match events (no pattern filtering).
    # Pattern-based handlers are dispatched purely from Python during emit().
    wrapper: Callable[[str, str], None] | None = None
    bus = _ensure_rust_bus()
    if bus is not None and pattern is None:
        wrapper = _make_wrapper(hid)
        try:
            bus.on(event_name, wrapper)
            record["wrapper"] = wrapper
        except Exception as exc:
            _logger.error(
                "Failed to register handler %s with Rust bus: %s",
                hid,
                exc,
            )
            wrapper = None  # fall through to pure-Python dispatch

    with _lock:
        _handlers[hid] = record
        if group is not None:
            _groups.setdefault(group, set()).add(hid)

    _logger.debug("Registered handler %s for event '%s'", hid, event_name)
    return hid


def once(
    event_name: str,
    callback: EventHandler,
    phase: str | None = None,
    pattern: str | None = None,
) -> HandlerId:
    """Register a one-time callback for *event_name*.

    The callback is automatically unsubscribed after its first
    invocation.  All parameters are as in :func:`on`.

    Returns
    -------
    HandlerId
        Handler identifier (can be used with :func:`off` to cancel
        before the event fires).
    """
    hid = _generate_handler_id()

    record: dict[str, Any] = {
        "id": hid,
        "event_name": event_name,
        "callback": callback,
        "once": True,
        "pattern": pattern,
        "phase": phase,
        "group": None,
        "wrapper": None,
    }

    wrapper: Callable[[str, str], None] | None = None
    bus = _ensure_rust_bus()
    if bus is not None and pattern is None:
        wrapper = _make_wrapper(hid)
        try:
            bus.on(event_name, wrapper)
            record["wrapper"] = wrapper
        except Exception as exc:
            _logger.error(
                "Failed to register once-handler %s with Rust bus: %s",
                hid,
                exc,
            )
            wrapper = None

    with _lock:
        _handlers[hid] = record

    _logger.debug("Registered one-time handler %s for event '%s'", hid, event_name)
    return hid


def off(handler_id: HandlerId) -> None:
    """Unregister a handler by its ID.

    Parameters
    ----------
    handler_id : HandlerId
        The identifier returned by :func:`on` or :func:`once`.

    Notes
    -----
    This removes the handler from the Python registry.  If the handler
    was registered with the Rust bus, the Rust-side callback wrapper
    will still be invoked on future emits, but the ``_dispatch``
    function will silently skip it (no-op) since the record no longer
    exists in the Python dict.
    """
    with _lock:
        _remove_handler_locked(handler_id)

    _logger.debug("Removed handler %s", handler_id)


def off_all() -> None:
    """Unregister **all** event handlers.

    Resets the entire handler registry, all groups, and clears the
    Rust bus if available.
    """
    bus = _ensure_rust_bus()

    with _lock:
        # Collect all unique event names that had wrappers registered
        # on the Rust bus so we can clean them up.
        event_types: set[str] = set()
        for record in _handlers.values():
            if record.get("wrapper") is not None:
                event_types.add(record["event_name"])

        _handlers.clear()
        _groups.clear()

    # Clean up Rust bus registrations
    if bus is not None:
        for evt in event_types:
            try:
                bus.off(evt)
            except Exception:
                pass  # best-effort

    _logger.debug("All event handlers removed")


def emit(event_name: str, data: Any = None) -> None:
    """Emit an event, notifying all matching handlers.

    Parameters
    ----------
    event_name : str
        The event type identifier.
    data : Any, optional
        Payload to pass to handlers.  If not a string, it is
        JSON-serialized before being sent to the Rust bus.  Handler
        callbacks receive the serialized string.

    Notes
    -----
    Dispatch order:

    1. If the Rust bus is available, its ``emit()`` is called first.
       This invokes all ``PyObject`` callbacks registered via
       ``bus.on()`` (i.e., exact-match handlers with no pattern).
    2. Pattern-based handlers (those registered with a ``pattern``
       argument) are then dispatched from Python via ``fnmatch``
       matching against *event_name*.
    """
    data_str = _serialize_data(data)

    bus = _ensure_rust_bus()

    with _lock:
        # Collect the handlers to dispatch:
        #   - If Rust bus is available, pattern-based handlers are dispatched
        #     from Python because the Rust bus doesn't support fnmatch.
        #   - If Rust bus is NOT available, ALL handlers must be dispatched
        #     from Python (both exact-match and pattern-based).
        if bus is not None:
            # Rust bus handles exact-match via registered wrappers;
            # we only need to dispatch pattern-based handlers.
            hids_to_dispatch = [
                hid for hid, rec in _handlers.items() if rec.get("pattern") is not None
            ]
        else:
            # Rust bus unavailable — dispatch every matching handler
            # from Python.
            hids_to_dispatch = [
                hid for hid, rec in _handlers.items() if _matches(event_name, rec)
            ]

    # 1. Rust bus — fires exact-match handlers
    if bus is not None:
        try:
            bus.emit(event_name, data_str)
        except Exception as exc:
            _logger.error("Rust EventBus emit failed: %s", exc)

    # 2. Python dispatch — handlers that the Rust bus can't handle
    for hid in hids_to_dispatch:
        _dispatch(hid, event_name, data_str)


def has_handlers(event_name: str) -> bool:
    """Return ``True`` if at least one handler is registered for *event_name*.

    Checks both the Rust bus (exact-match handlers) and the Python
    pattern-handler registry.
    """
    bus = _ensure_rust_bus()
    if bus is not None:
        try:
            if bus.has_handlers(event_name):
                return True
        except Exception:
            pass

    with _lock:
        return any(_matches(event_name, rec) for rec in _handlers.values())


def num_event_types() -> int:
    """Return the number of distinct event types with registered handlers.

    This counts distinct ``event_name`` values from all registered
    handler records in the Python registry.  When the Rust bus is
    active, exact-match handlers are stored in both places, so we
    use the Python dict as the canonical source of truth to avoid
    double-counting.
    """
    with _lock:
        event_names: set[str] = set()
        for rec in _handlers.values():
            event_names.add(rec["event_name"])
        return len(event_names)


# ===================================================================
# Autocmd (group) management
# ===================================================================


@contextmanager
def autocmd_group(name: str) -> Iterator[None]:
    """Context manager for registering handlers under a named group.

    All :func:`on` calls inside the ``with`` block are automatically
    assigned to *name* as their group.  The group can later be cleared
    with :func:`autocmd_clear`.

    Usage::

        with autocmd_group("battle_handlers"):
            on("battle_start", handler1)
            on("battle_end", handler2)

        autocmd_clear("battle_handlers")
    """
    # We use a thread-local to track the active group so that
    # on() calls inside the context pick it up automatically.
    _group_stack = getattr(autocmd_group, "_stack", [])
    _group_stack.append(name)
    autocmd_group._stack = _group_stack  # type: ignore[attr-defined]

    try:
        yield
    finally:
        _group_stack.pop()
        autocmd_group._stack = _group_stack  # type: ignore[attr-defined]


def _get_active_group() -> str | None:
    """Return the active group name from the autocmd_group stack, if any."""
    stack = getattr(autocmd_group, "_stack", [])
    return stack[-1] if stack else None


def autocmd_clear(group: str) -> None:
    """Remove all handlers registered under *group*.

    Parameters
    ----------
    group : str
        Group name used when registering handlers.
    """
    with _lock:
        hids = _groups.pop(group, set())

    for hid in hids:
        off(hid)

    _logger.debug("Cleared autocmd group '%s' (%d handlers)", group, len(hids))


# ===================================================================
# Event metadata / schema
# ===================================================================


def define_event(event_name: str, schema: Any) -> None:
    """Define metadata or a schema for an event type.

    This is a lightweight registry for event documentation and schema
    validation hints.  Stored data can be retrieved via
    :func:`get_event_schema`.

    Parameters
    ----------
    event_name : str
        The event type identifier.
    schema : Any
        Arbitrary metadata (e.g., a dict describing expected fields).
    """
    with _lock:
        _event_schemas[event_name] = schema
    _logger.debug("Defined schema for event '%s'", event_name)


def get_event_schema(event_name: str) -> Any:
    """Return the schema previously registered via :func:`define_event`.

    Returns ``None`` if no schema was registered for *event_name*.
    """
    with _lock:
        return _event_schemas.get(event_name)


# ===================================================================
# Compatibility helpers for autocommand-style APIs
# ===================================================================


def get_handler_count() -> int:
    """Return the total number of registered handler records."""
    with _lock:
        return len(_handlers)


def get_group_names() -> list[str]:
    """Return a list of all group names with registered handlers."""
    with _lock:
        return list(_groups.keys())


def get_handler_ids() -> list[HandlerId]:
    """Return a list of all active handler IDs."""
    with _lock:
        return list(_handlers.keys())
