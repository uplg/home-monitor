# Maison

## What it can do

- Monitor and control local home devices from a single interface.
- Manage Tuya-based devices such as feeders, fountains, and litter boxes.
- Track energy and status data for Meross plugs.
- Control Philips Hue lamps over Bluetooth and Zigbee.
- Handle Hue dimmer switch (v1 at least), global handling, On/off change power state for every connected zigbee device, dim up/down same.
- Query Tempo data, predictions, history, and calibration helpers.
- Mirror the daily Tempo colors on a Nabaztag running the garenne firmware (belly LED = today, ears = tomorrow).
- Keep access private with local authentication and secure session cookies.

![Maison](/screenshots/maison.jpg?v=1786751396)

Exposes only two app components:

- `frontend/`: the current Vite/React frontend
- `backend/`: the Rust backend

## Runtime files kept in place

The Rust backend reads these files directly from the repo root:

- `devices.json`
- `device-cache.json`
- `users.json`
- `meross-devices.json`
- `hue-lamps.json`
- `hue-lamps-blacklist.json`
- `zigbee-lamps.json`
- `zigbee-lamps-blacklist.json`
- `climate-state.json`
- `mosquitto/`

Tempo cache and calibration files now live in `cache/tempo/`.

Tempo recalibration workflow is documented in `docs/tempo-calibration.md`.

## Prerequisites

- `bun` for the frontend
- Rust and `cargo` for the backend (`cargo-zigbuild` for the Pi cross-build)
- a compatible Zigbee USB dongle for Zigbee support, for example a Sonoff Dongle Lite MG21 (`adapter: ember`)

## Raspberry Pi 1

For Raspberry Pi 1 deployments, the intended setup is fully host-native:

- run Mosquitto directly on the Pi
- plug in the Zigbee USB coordinator (Sonoff MG21-based dongle); the backend drives it natively over EZSP
- build the frontend once, then let the Rust backend serve `frontend/dist`
- run the cross-built musl release binary (no Bluetooth: `--no-default-features`)
- set `DISABLE_BLUETOOTH=true`
- set `AUTH_COOKIE_SECURE=false` if the Pi is exposed only over plain HTTP on the LAN

Deployment notes and host-native service files are in `docs/raspberry-pi-1.md`, `deploy/openrc/maison`, `deploy/openrc/cloudflared-maison`, and `deploy/mosquitto/maison.conf`.

There is also a one-shot deployment helper for the Pi: `deploy.sh`.
It supports `all`, `build`, `push`, `upgrade`, `start`, `stop`, `status`, and `logs`.
It also accepts `PI_PASSWORD` for password-based SSH when `sshpass` is installed locally.

The Raspberry Pi 1 target now assumes Alpine Linux with OpenRC and a musl backend build.

For first boot without screen or keyboard, use `scripts/flash-alpine-headless-macos.sh` and `docs/alpine-headless-flash-macos.md`.

Zigbee is driven natively by the Rust backend over EZSP (serial dongle); there is no Zigbee2MQTT or Node.js layer.

## Environment

```bash
cp .env.example .env
```

Main settings:

- `PORT` / `API_PORT`: Rust backend port, default `3033`
- `JWT_SECRET`: auth signing secret
- `FRONTEND_DIST_DIR`: built frontend directory served directly by the backend when `index.html` exists
- `DISABLE_BLUETOOTH`: set `true` to disable Hue BLE support
- `ZIGBEE_SERIAL_PORT`: serial path of the Zigbee USB dongle
- `ZIGBEE_ADAPTER`: adapter type, `ember` for MG21/EZSP dongles such as the Sonoff Dongle Lite MG21
- `AUTH_COOKIE_NAME`: session cookie name
- `AUTH_COOKIE_SECURE`: keep `true` when the app is exposed through HTTPS/Cloudflare
- `AUTH_RATE_LIMIT_ATTEMPTS`: max failed login attempts per IP+username window
- `AUTH_RATE_LIMIT_WINDOW_SECONDS`: backend login throttling window
- `CLOUDFLARE_TUNNEL_TOKEN`: optional token for the Cloudflare tunnel profile
- `CLOUDFLARED_PROTOCOL`: Cloudflare transport protocol, default `http2` for better compatibility behind NAT
- `CLOUDFLARE_PUBLIC_HOSTNAME`: optional stable public hostname, for example `home.example.com`

## Security notes

- `JWT_SECRET` must be set to a strong unique value; the backend now refuses to start with the default secret.
- `users.json` must exist and contain at least one account with `password_hash`; plaintext passwords are refused.
- Browser access is expected through the frontend only.
- Auth uses an `HttpOnly` cookie.
- Login throttling.
- Simple audit logs are emitted for login success, failure, and rate-limit hits.

To generate a password hash for `users.json`:

```bash
cargo run --manifest-path backend/Cargo.toml --bin hash_password -- 'your-password'
```

Then :
```bash
cp users.json.template users.json
# copy previous argon2i hashes into this file.
```

## Development (this machine)

```bash
make backend         # cargo run, backend on :3033
make frontend        # vite dev server, proxies /api to :3033
make test            # backend tests + frontend lint
```

## Deployment (Raspberry Pi 1)

Everything goes through `deploy.sh`, wrapped by Make targets. `PI_HOST` is
read from `.env`.

```bash
make deploy              # build (frontend + ARMv6 musl backend) + push + restart services
make deploy-status       # service states, versions, URLs
make deploy-logs         # follow logs (LOG_TARGET=stack|backend|mosquitto|cloudflared)
make cloudflared-upgrade # rebuild latest cloudflared for ARMv6 and swap it on the Pi
```

The Pi runs three OpenRC services: `mosquitto` (TLS :8883 for the Meross
plugs), `maison` (the backend, which also serves the frontend), and
`cloudflared-maison` (the tunnel, when `CLOUDFLARE_TUNNEL_TOKEN` is set in
`.env`; set `CLOUDFLARE_PUBLIC_HOSTNAME` for the public URL).

Full instructions are in `docs/raspberry-pi-1.md`.

## Validation

- Frontend build: `bun --cwd frontend run build`
- Backend tests: `cargo test --manifest-path backend/Cargo.toml`
- Minimal Pi-oriented backend check: `cargo check --manifest-path backend/Cargo.toml --no-default-features`

### Planned

- Matter bridge (but will not handle cats-related devices such as litter as it's not yet in the specification.)
