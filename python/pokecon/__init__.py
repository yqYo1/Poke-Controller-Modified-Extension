"""Typed Python compatibility package backed by isolated Rust workers."""

from importlib.metadata import PackageNotFoundError, version

try:
    __version__ = version("poke-controller-modified-extension")
except PackageNotFoundError:
    __version__ = "0+uninstalled"

__all__ = ["__version__"]
