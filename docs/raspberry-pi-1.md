# Raspberry Pi 1 deployment

This repository can be prepared for a Raspberry Pi 1 Model B running Alpine Linux (`alpine-rpi-3.24.1-armhf`) by running the stack in host-native mode, without Docker and without Hue BLE.

For the SD card flashing and first headless SSH bootstrapping flow on macOS, see `docs/alpine-headless-flash-macos.md` and `scripts/flash-alpine-headless-macos.sh`.

Recommended constraints for this target:

- no Docker
- no on-device frontend build
- no Bluetooth build support
- Mosquitto, the Rust backend, and optionally Cloudflared should be managed by OpenRC

## Recommended deployment shape

On the Pi, run:

- Mosquitto on the host
- the Rust backend release binary (drives the Zigbee dongle natively)
- the prebuilt frontend files in `frontend/dist`

The backend now serves the frontend directly when `FRONTEND_DIST_DIR/index.html` exists.

That means there is no separate nginx or frontend container on the Pi.

## Build on a faster machine

Build the frontend:

```bash
bun --cwd frontend run build
```

Build a Pi-oriented backend binary without Bluetooth support:

```bash
cargo build --release --manifest-path backend/Cargo.toml --no-default-features
```

Copy these artifacts to the Pi:

- `backend/target/release/maison-backend`
- `frontend/dist/`
- `.env`
- `deploy/mosquitto/maison.conf`
- `deploy/openrc/maison`
- `deploy/openrc/cloudflared-maison`
- runtime JSON files from the repo root that your installation needs

## Cross-compiling from the dev machine

Your existing dev workflow stays unchanged. The Raspberry Pi flow is isolated behind a separate helper script and Make target.

Install prerequisites on the dev machine:

```bash
rustup toolchain install stable
cargo install cargo-zigbuild
brew install zig
rustup target add arm-unknown-linux-musleabihf
```

Then run:

```bash
make build-pi
```

Or use the one-shot deploy script:

```bash
PI_HOST=pi@192.168.1.50 ./deploy.sh all
```

Useful subcommands:

```bash
PI_HOST=pi@192.168.1.50 ./deploy.sh build
PI_HOST=pi@192.168.1.50 ./deploy.sh push
PI_HOST=pi@192.168.1.50 ./deploy.sh upgrade
PI_HOST=pi@192.168.1.50 ./deploy.sh start
PI_HOST=pi@192.168.1.50 ./deploy.sh stop
PI_HOST=pi@192.168.1.50 ./deploy.sh status
PI_HOST=pi@192.168.1.50 ./deploy.sh logs backend
PI_HOST=pi@192.168.1.50 PI_PASSWORD='secret' ./deploy.sh push
```

That runs `scripts/build-rpi1-backend.sh`, which:

- builds `backend/Cargo.toml` in `release`
- disables Bluetooth at compile time with `--no-default-features`
- targets a Pi 1 compatible ARMv6 hard-float musl binary
- builds only the main backend binary to keep the linker workload smaller on macOS
- leaves the normal host build and normal `cargo` workflow untouched

Default artifact path:

```bash
target/arm-unknown-linux-musleabihf/release/maison-backend
```

`make backend`, `make frontend` and `cargo check` keep working on the dev machine as before.

## Recommended `.env` values on the Pi

```bash
HOST=0.0.0.0
PORT=3033
JWT_SECRET=replace-this
FRONTEND_DIST_DIR=frontend/dist
DISABLE_BLUETOOTH=true
AUTH_COOKIE_SECURE=false
```

Notes:

- keep `AUTH_COOKIE_SECURE=true` only if the Pi is behind real HTTPS
- `DISABLE_BLUETOOTH=true` is still recommended even if you built with `--no-default-features`

## Mosquitto on Alpine

Install Mosquitto directly on the host:

```bash
sudo apk add --no-cache mosquitto
```

Install the repository config and certificates:

```bash
sudo mkdir -p /etc/mosquitto/conf.d /etc/mosquitto/certs/maison
sudo cp deploy/mosquitto/maison.conf /etc/mosquitto/conf.d/maison.conf
sudo cp mosquitto/certs/ca.pem /etc/mosquitto/certs/maison/ca.pem
sudo cp mosquitto/certs/server.pem /etc/mosquitto/certs/maison/server.pem
sudo cp mosquitto/certs/server-key.pem /etc/mosquitto/certs/maison/server-key.pem
sudo chown -R mosquitto:mosquitto /etc/mosquitto/certs/maison
sudo chmod 600 /etc/mosquitto/certs/maison/server-key.pem
sudo rc-update add mosquitto default
sudo rc-service mosquitto restart
```

This keeps:

- `1883` for local debugging
- `8883` for Meross devices that need TLS MQTT

## OpenRC services

The repository includes `deploy/openrc/maison` and `deploy/openrc/cloudflared-maison`.

The deploy script installs them automatically on Alpine. If you need to do it manually:

```bash
sudo cp deploy/openrc/maison /etc/init.d/maison
sudo chmod +x /etc/init.d/maison
sudo rc-update add maison default
sudo rc-service maison start
```

Check logs:

```bash
tail -f /var/log/maison.log
tail -f /var/log/mosquitto/mosquitto.log
```

## Optional Cloudflare Tunnel on Alpine

Install `cloudflared` directly on the host, keep `CLOUDFLARE_TUNNEL_TOKEN` in `/opt/maison/.env`, then install the repository service:

```bash
sudo cp deploy/openrc/cloudflared-maison /etc/init.d/cloudflared-maison
sudo chmod +x /etc/init.d/cloudflared-maison
sudo rc-update add cloudflared-maison default
sudo rc-service cloudflared-maison start
```

Check logs:

```bash
tail -f /var/log/cloudflared-maison.log
```

The service uses the same `.env` file as the backend and runs only when `CLOUDFLARE_TUNNEL_TOKEN` is set.

## Zigbee on Alpine

The Rust backend drives the Zigbee coordinator natively over EZSP; there is no Node or Zigbee2MQTT layer. Mosquitto stays in place because Meross devices need a reachable TLS broker.

Configure the dongle with:

```bash
ZIGBEE_ADAPTER=ember
ZIGBEE_SERIAL_PORT=/dev/ttyUSB0
ZIGBEE_EZSP_PROTOCOL_VERSION=13
```

## Important caveats

- Hue BLE on Raspberry Pi 1 is not a good default target; build without the Bluetooth feature unless you explicitly need it.
- The frontend is served by the Rust backend on the same port as the API, so the default access URL is `http://<pi-ip>:3033`.
- `cloudflared` itself is host-native now, but its availability still depends on Cloudflare providing a working ARMv6 binary for the OS you install on the Pi.
