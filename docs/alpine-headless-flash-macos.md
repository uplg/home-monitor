# Flash Alpine Headless From macOS

Use `scripts/flash-alpine-headless-macos.sh` to flash an Alpine `.img.xz` file to an SD card from macOS and inject the headless bootstrap overlay used for first-boot SSH access.

What it does:

- flashes your Alpine image to the target SD card
- downloads or reuses `headless.apkovl.tar.gz` from `macmpi/alpine-linux-headless-bootstrap`
- copies the overlay to the boot partition root
- auto-detects the `bootstrap/` directory and uses its files when available
- generates a default `interfaces` file for DHCP on `eth0` when you do not provide one
- generates a default `unattended.sh` that sets the hostname to `maison` when you do not provide one
- optionally injects `authorized_keys`, `interfaces`, `wpa_supplicant.conf`, `unattended.sh`, and `ssh_host_*_key*`

## Quick start (recommended)

The repository ships a `bootstrap/unattended.sh` that fully provisions the Pi at first boot (packages, service user, Mosquitto, directory tree). The flash script auto-detects it:

```bash
scripts/flash-alpine-headless-macos.sh \
  --image ~/Downloads/alpine-rpi-3.24.1-armhf.img.xz \
  --disk disk4
```

After the Pi boots and the unattended script runs, finish the deployment:

```bash
PI_HOST=pi@<ip> ./deploy.sh build
PI_HOST=pi@<ip> ./deploy.sh push
PI_HOST=pi@<ip> ./deploy.sh start
```

## Bootstrap directory auto-detection

When the `bootstrap/` directory exists at the repository root, the flash script automatically picks up any files it contains:

| File | Purpose |
|------|---------|
| `bootstrap/unattended.sh` | First-boot provisioning script |
| `bootstrap/interfaces` | Network interface config |
| `bootstrap/wpa_supplicant.conf` | WiFi config |
| `bootstrap/authorized_keys` | SSH public keys |
| `bootstrap/ssh-host-keys/` | Pre-generated SSH host keys |

Command-line flags always take precedence over auto-detected files.

## Basic usage without bootstrap/

```bash
scripts/flash-alpine-headless-macos.sh \
  --image ~/Downloads/alpine-rpi-3.24.1-armhf.img.xz \
  --disk disk4
```

The script auto-detects a local SSH public key from:

- `~/.ssh/id_ed25519.pub`
- `~/.ssh/id_ecdsa.pub`
- `~/.ssh/id_rsa.pub`

If you want to force a specific key:

```bash
scripts/flash-alpine-headless-macos.sh \
  --image ~/Downloads/alpine-rpi-3.24.1-armhf.img.xz \
  --disk disk4 \
  --authorized-keys ~/.ssh/id_ed25519.pub
```

## Optional extra files via flags

```bash
scripts/flash-alpine-headless-macos.sh \
  --image ~/Downloads/alpine-rpi-3.24.1-armhf.img.xz \
  --disk disk4 \
  --hostname maison \
  --interfaces ./bootstrap/interfaces \
  --wpa-supplicant ./bootstrap/wpa_supplicant.conf \
  --unattended ./bootstrap/unattended.sh \
  --ssh-host-keys ./bootstrap/ssh-host-keys
```

## Default headless behavior

If you do not provide custom files and the `bootstrap/` directory is empty or missing, the script prepares a minimal headless bootstrap setup for you:

- `interfaces`: DHCP on `eth0`
- `unattended.sh`: sets the hostname to `maison`
- `authorized_keys`: copied from your first detected local SSH public key, when available

You can override the hostname with:

```bash
scripts/flash-alpine-headless-macos.sh \
  --image ~/Downloads/alpine-rpi-3.24.1-armhf.img.xz \
  --disk disk4 \
  --hostname maison
```

## Notes

- the script is destructive for the selected SD card; it asks for an explicit disk confirmation unless you pass `--no-confirm`
- on success, it ejects the SD card by default; use `--keep-mounted` if you want to inspect the boot partition afterward
- if you do not inject `authorized_keys`, Alpine headless bootstrap falls back to its default first-boot SSH behavior
