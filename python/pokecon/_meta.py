class CommandMeta(type):
    _registry: dict[str, type] = {}
    _interface_registry: dict[str, dict[str, type]] = {
        "PythonCommand": {},
        "ImageProcPythonCommand": {},
    }

    @classmethod
    def register_interface(
        mcs, base_name: str, version: str, interface_cls: type, override: bool = False
    ) -> None:
        if not getattr(interface_cls, "__is_interface__", False):
            raise ValueError(
                f"{interface_cls.__name__} must have __is_interface__ = True"
            )
        if base_name not in mcs._interface_registry:
            mcs._interface_registry[base_name] = {}
        if version in mcs._interface_registry[base_name] and not override:
            raise ValueError(f"Interface {base_name}@{version} already registered")
        mcs._interface_registry[base_name][version] = interface_cls

    @classmethod
    def get_interface(mcs, base_name: str, version: str) -> type:
        versions = mcs._interface_registry.get(base_name, {})
        if version not in versions:
            available = ", ".join(versions.keys())
            raise ValueError(
                f"Unknown API version '{version}' for {base_name}. Available: {available}"
            )
        return versions[version]

    @classmethod
    def list_versions(mcs, base_name: str) -> list[str]:
        return list(mcs._interface_registry.get(base_name, {}).keys())
