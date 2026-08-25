# Maison

## What it can do

- Monitor and control local home devices from a single interface.
- Manage Tuya-based devices such as feeders, fountains, and litter boxes.
- Track energy and status data for Meross plugs.
- Control Philips Hue lamps over Bluetooth and Zigbee.
- Handle Hue dimmer switch (v1 at least), global handling, On/off change power state for every connected zigbee device, dim up/down same.
- Query Tempo data, predictions, history, and calibration helpers.
- Mirror the daily Tempo colors on a Nabaztag running the garenne firmware (belly LED = today, ears = tomorrow).
- Turn a set-top-box remote control into a house remote: an AirTies AIR 7310T (custom firmware) decodes its Ruwido IR remote and forwards every key press to maison, which maps buttons to actions. See "IR remote" below.
- Drive the living-room Philips TV (55PUS6753, Saphi) over its JointSPACE API: power, volume, Ambilight and remote keys, plus Wake-on-LAN for deep standby. See "Television" below.
- Drive the Android TV box (MECOOL LEAP-S1) from a proper on-screen remote — D-pad, media, volume, app shortcuts and APK sideloading — over a native ADB client. See "Android TV box" below.
- Keep access private with local authentication and secure session cookies.

![Maison](/screenshots/maison.jpg?v=1787691685)

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
- `ir-keymap.json`
- `tv.json`
- `androidtv.json`
- `adb-key`
- `mosquitto/`

Tempo cache and calibration files now live in `cache/tempo/`.

Tempo recalibration workflow is documented in `docs/tempo-calibration.md`.

## Television

The 55PUS6753 runs Saphi, not Android TV, which is what makes it controllable
at all: the JointSPACE API answers plain HTTP on port 1925 with
`pairing_type: "none"` — no pairing, no auth, no certificate. Copy
`tv.json.template` to `tv.json` and fill in the set's address, its MAC (for
Wake-on-LAN) and, optionally, the Android box address.

Two things are worth knowing before poking at it:

- **The endpoint whitelist is not advisory.** Saphi answers `Forbidden` or
  `Not Found` on what it does not implement (`/6/sources`, `/6/applications`,
  `/6/activities/*`, anything under `/5/`), and hitting those repeatedly kills
  the embedded server *persistently* — neither a standby cycle nor the API's
  own `Standby` brings it back, only unplugging the set from the mains for
  ~30 s. `tv.rs` therefore models the reachable surface as an enum and spaces
  every request through a single gate. Do not widen it without verifying.
- **There are two sleep depths.** Light standby still answers on 1925
  (`powerstate: "Standby"`); deep standby drops the network stack entirely and
  needs a Wake-on-LAN magic packet first, which takes ~20 s and only revives
  the network — the panel stays off until the following `powerstate: On`.

Source switching is the one thing JointSPACE cannot do on Saphi. Powering on
with `switchToBox` instead nudges the Android box awake over DIAL, which makes
it assert CEC One Touch Play — that both powers the set and routes it to the
box's HDMI input. This is the fix for "the TV came up on the wrong input".

## IR remote debounce

Presses of the same key closer than 1.2 s are treated as phantom doubles:
marginal IR reception splits one hold into several presses, which is fatal for
toggles — the action cancels itself. That default is wrong for navigation,
where the second press of a D-pad is deliberate and arrives well inside the
window. Bindings therefore take an optional `debounce_ms`; the shipped keymap
uses 150 ms for the D-pad, media and volume keys and leaves toggles (climate,
plugs, lamps) on the 1.2 s default. The value is clamped at 50 ms so a typo
cannot disable the filter for the bindings that depend on it.

Worth knowing when a key feels slow: `input keyevent` starts a JVM on the box,
which costs ~150 ms awake but around **10 s when the box is asleep** — the same
delay the official `adb` binary shows, so it is the box, not this code.

## Android TV box

The box runs plain Android 14 with network debugging enabled, so Maison talks
to it over **ADB, implemented natively in `adb.rs`** — no `adb` binary is
shipped to the Pi. The existing Rust crates all drive the local `adb` *server*
(a second daemon on :5037), which is exactly the dependency worth avoiding on
an ARMv6 Alpine box.

Authentication is the part worth knowing about. On connect the box sends a
20-byte token; the client signs it with RSA-2048 and answers. **The token is
already a SHA-1 digest, so it must be signed pre-hashed** — hashing it again
yields a signature the box silently rejects, after which every connection
falls back to re-sending the public key and re-prompting on screen.

The signing key is generated on first use and stored in `adb-key` (0600).
Generating 2048 bits takes tens of seconds on a Pi 1, so it happens off the
async runtime, and the television will show one "Allow USB debugging?" prompt
the first time the backend connects. Accept it once — with *always allow* — and
it never comes back.

Beyond the remote, the box is also how the television gets powered and routed:
it runs with `power_control_mode=broadcast` and `tv_wake_on_one_touch_play=1`,
so waking it asserts CEC One Touch Play (TV on, input switched) and sleeping it
broadcasts a CEC standby.

APKs can be sideloaded from the dashboard: the file goes over ADB's `sync:`
service to the box's temp directory, then through `pm install -r`. Uploads are
capped at 96 MB — the Pi has 512 MB and holds the payload in memory.

## IR remote (AirTies STB)

An AirTies AIR 7310T set-top box running its custom firmware
([uplg/BCM7231B2](https://github.com/uplg/BCM7231B2)) decodes the Ruwido IR
remote in hardware; its
`kird` daemon POSTs every key event to `POST /api/ir/key`, authenticated with
the `IR_API_TOKEN` machine token (constant-time compare, independent from the
JWT session auth; set the same token in the STB's `device/kird.conf`).

Buttons are configured from the frontend: Dashboard → Télécommande. The page
draws the physical remote (mapped keys highlighted), captures a key by asking
you to press it (it polls `GET /api/ir/recent`), and each binding holds an
ordered list of actions. One button can drive several devices, and a Test
button dry-runs the actions without saving.

Available actions, all "reversible" where it makes sense (`on` / `off` /
`toggle`, toggle reads the device's current state and flips it):

- Nabaztag command (garenne grammar: `dance`, `chor /vl/config/chor/taichi.chor`, `ears 8 8`, …)
- Zigbee lamp power (on/off/toggle) and brightness (0-254, `repeat` fires while the button is held)
- Meross plug power (on/off/toggle)
- Broadlink saved IR code
- Mitsubishi AC toggle: turns the AC on with structured settings
  (mode/temperature/fan/vane pickers) or off if the last commanded state was
  on. The blast is delayed ~1.2 s so the remote's own IR repeats cannot
  collide with the RM4 transmission at the AC's receiver.

The keymap persists in `ir-keymap.json` (server-side, survives reboots and
deployments). Phantom double-presses from marginal IR reception are debounced
server-side (1.2 s per key); unmapped keys return 200 and are logged
(`unmapped IR key`) so new buttons are easy to discover.

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
- `IR_API_TOKEN`: machine token for the STB IR bridge (empty/unset disables `/api/ir/key`)
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
