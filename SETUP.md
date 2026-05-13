# Setup Guide for Non-NixOS Systems

This guide covers the additional setup required to run `nix run .#check` and other FHS-dependent commands on non-NixOS Linux distributions (Ubuntu, Debian, etc.).

## Prerequisites

### 1. Install Nix

Follow the [official Nix installer](https://nixos.org/download.html):

```bash
curl -L https://nixos.org/nix/install | sh
```

Enable flakes:

```bash
mkdir -p ~/.config/nix
echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

### 2. Install uidmap (Required for bubblewrap)

```bash
sudo apt install uidmap
```

This provides `newuidmap` and `newgidmap` which are required by bubblewrap (used by `buildFHSEnv`).

## Ubuntu 24.04+ Additional Setup

Ubuntu 24.04 and later versions ship with AppArmor restrictions on unprivileged user namespaces. This blocks bubblewrap from creating the FHS environment needed by `nix run .#check`.

### Automatic Setup (Recommended)

Run the provided setup script:

```bash
sudo nix run .#setup-bwrap
```

This will:
1. Detect if AppArmor is active
2. Create an AppArmor profile for bubblewrap at `/etc/apparmor.d/bwrap`
3. Load the profile
4. Disable `kernel.apparmor_restrict_unprivileged_userns` (Ubuntu 24.04+)

To make the kernel setting persistent across reboots:

```bash
echo "kernel.apparmor_restrict_unprivileged_userns=0" | sudo tee -a /etc/sysctl.conf
```

### Manual Setup

If you prefer to set up manually:

```bash
# Create AppArmor profile
sudo tee /etc/apparmor.d/bwrap << 'EOF'
abi <abi/4.0>,
include <tunables/global>

profile bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,

  # Site-specific additions and overrides. See local/README for details.
  include if exists <local/bwrap>
}
EOF

# Load the profile
sudo apparmor_parser -r /etc/apparmor.d/bwrap
```

### Verification

After setup, verify bubblewrap works:

```bash
bwrap --dev-bind / / --unshare-user-try echo "bwrap works"
```

You should see `bwrap works` without any permission errors.

## Running Checks

Once setup is complete:

```bash
# Run all checks (clippy, ruff, pytest, typos, formatting)
nix run .#check

# Run individual checks
nix run .#clippy
nix run .#test
nix run .#web-check
```

## Troubleshooting

### "setting up uid map: Permission denied"

1. Ensure `uidmap` is installed: `sudo apt install uidmap`
2. Ensure `/etc/subuid` and `/etc/subgid` are configured for your user
3. On Ubuntu 24.04+, ensure AppArmor profile is created (see above)

### "Unable to find libclang"

This error occurs when `buildFHSEnv` is not used. The `check` app wraps the execution in `buildFHSEnv` to isolate libclang from the host glibc. If you see this error, ensure the `check` app is using `buildFHSEnv` (see `flake.nix`).

### "GLIBC_ABI_* not found"

This indicates a glibc version mismatch between nix and the host system. The `buildFHSEnv` wrapper is designed to prevent this by isolating the environment. Ensure `buildFHSEnv` is being used.

## NixOS Users

No additional setup is required on NixOS. All commands work out of the box:

```bash
nix run .#check
```
