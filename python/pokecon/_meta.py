"""CommandMeta metaclass — dynamically selects API version and injects correct MRO.

When a user class inherits from ``PythonCommand`` or ``ImageProcPythonCommand``
(both defined with ``__is_interface__ = True`` and ``metaclass=CommandMeta``),
the metaclass:

1. Detects the interface base class.
2. Reads ``__api_version__`` from the user class (default: ``"v1"``).
3. Looks up the registered implementation class for that version.
4. Rebuilds the MRO as::

    [UserClass, _RustCoreAdapter, <ImplClass>, <InterfaceClass>, …]

The ``_RustCoreAdapter`` is inserted first (after the user class) so that
it can intercept calls and delegate to Rust PyO3 bindings when available.
"""

from __future__ import annotations

import os
from abc import ABCMeta


class CommandMeta(ABCMeta):
    """Metaclass for PythonCommand / ImageProcPythonCommand.

    Class attributes
    ----------------
    _registry : dict[str, type]
        Every user-script subclass that goes through the metaclass is stored
        here keyed by class name (useful for introspection / debugging).
    _interface_registry : dict[str, dict[str, type]]
        Nested dict: ``{base_name: {version: impl_class}}``.
        Register with ``CommandMeta.register_interface()``.
    """

    _registry: dict[str, type] = {}
    _interface_registry: dict[str, dict[str, type]] = {
        "PythonCommand": {},
        "ImageProcPythonCommand": {},
    }

    # ── Registry API ────────────────────────────────────────────────────────

    @classmethod
    def register_interface(
        mcs,
        base_name: str,
        version: str,
        interface_cls: type,
        override: bool = False,
    ) -> None:
        """Register an implementation class for *base_name* @ *version*.

        Parameters
        ----------
        base_name : str
            ``"PythonCommand"`` or ``"ImageProcPythonCommand"``.
        version : str
            Version string, e.g. ``"v1"``, ``"v2"``.
        interface_cls : type
            The class that provides the actual implementations.  Must have
            ``__is_interface__ = True``.
        override : bool
            If *True*, replace an existing registration for the same version.
        """
        if not getattr(interface_cls, "__is_interface__", False):
            raise ValueError(
                f"{interface_cls.__name__} must have __is_interface__ = True"
            )
        if base_name not in mcs._interface_registry:
            mcs._interface_registry[base_name] = {}
        if version in mcs._interface_registry[base_name] and not override:
            raise ValueError(
                f"Interface {base_name}@{version} already registered. "
                "Use override=True to replace."
            )
        mcs._interface_registry[base_name][version] = interface_cls

    @classmethod
    def get_interface(mcs, base_name: str, version: str) -> type:
        """Return the implementation class for *base_name* @ *version*."""
        versions = mcs._interface_registry.get(base_name, {})
        if version not in versions:
            available = (
                ", ".join(f"'{v}'" for v in versions)
                if versions
                else "(none registered)"
            )
            raise ValueError(
                f"Unknown API version '{version}' for {base_name}. "
                f"Available: {available}"
            )
        return versions[version]

    @classmethod
    def list_versions(mcs, base_name: str) -> list[str]:
        """List registered version strings for *base_name*."""
        return list(mcs._interface_registry.get(base_name, {}).keys())

    # ── Metaclass core ──────────────────────────────────────────────────────

    def __new__(
        mcs,
        name: str,
        bases: tuple[type, ...],
        namespace: dict[str, object],
        **kwargs: object,
    ) -> type:
        """Intercept class creation for user scripts.

        Behaviour
        ---------
        * If ``__is_interface__ = True`` is in *namespace*, the class is an
          **interface definition** (like ``PythonCommand`` itself).  It passes
          through to ``type.__new__`` unchanged.
        * Otherwise the metaclass looks for a direct base that is a **CommandMeta
          interface** (i.e. ``isinstance(base, CommandMeta) and
          base.__is_interface__ is True``).  If found, the metaclass:
            - Reads ``__api_version__`` (default ``"v1"``).
            - Retrieves the registered implementation for that version.
            - Injects ``_RustCoreAdapter`` *before* the implementation.
            - Builds the final class with the custom MRO.
        * If neither condition applies, the class passes through normally.
        """
        # ── 1. Interface definition → pass through ──────────────────────────
        if namespace.get("__is_interface__", False):
            return type.__new__(mcs, name, bases, namespace)

        # ── 2. Find a CommandMeta-interface base ────────────────────────────
        interface: type | None = None
        for base in bases:
            if isinstance(base, CommandMeta) and getattr(
                base, "__is_interface__", False
            ):
                interface = base
                break

        if interface is None:
            # Not a subclass of PythonCommand/ImageProcPythonCommand
            return type.__new__(mcs, name, bases, namespace)

        # ── 3. Resolve API version ──────────────────────────────────────────
        api_version: str = str(
            namespace.get("__api_version__")
            or os.environ.get("POKECON_API_VERSION", "v1")
        )
        interface_name: str = interface.__name__

        # ── 4. Get the implementation class for this version ────────────────
        impl_cls: type = mcs.get_interface(interface_name, api_version)

        # ── 5. Lazy-import the adapter (avoids circular deps at module level) ─
        from pokecon._adapter import _RustCoreAdapter  # noqa: PLC0415

        # ── 6. Build new namespace with metadata ────────────────────────────
        new_namespace: dict[str, object] = dict(namespace)
        new_namespace["__interface__"] = interface
        new_namespace["__api_version__"] = api_version
        new_namespace["__impl_adapter__"] = _RustCoreAdapter

        # ── 7. Create the class with custom MRO ─────────────────────────────
        # MRO: [UserClass, _RustCoreAdapter, impl_cls, interface, …]
        new_bases = (_RustCoreAdapter, impl_cls)
        cls = type.__new__(mcs, name, new_bases, new_namespace)
        mcs._registry[name] = cls
        return cls
