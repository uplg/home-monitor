# Maison — Architecture Brief

## 1. Project identity

- **Name**: "Maison" (crate `maison-backend`, service `maison`)
- **Purpose**: self-hosted home-automation dashboard — cat devices (feeder,
  fountain, litter box), lamps (Hue BLE, Zigbee), smart plugs (Meross), IR
  climate control (Broadlink → Mitsubishi AC), and French Tempo electricity
  tariff tracking/prediction.
- **Production target**: Raspberry Pi 1 (Alpine Linux, OpenRC, musl,
  `--no-default-features` build without Bluetooth). Development on macOS.

## 2. Repository layout

```
maison/
├── backend/                 # Rust backend (Axum 0.8, tokio)
│   ├── src/
│   │   ├── main.rs          # entrypoint, dotenvy, graceful shutdown
│   │   ├── lib.rs           # AppState, router assembly, layers (CORS,
│   │   │                    #   security headers, 180s TimeoutLayer, trace)
│   │   ├── config.rs        # env-driven Config (paths, auth, zigbee)
│   │   ├── error.rs         # AppError (thiserror + IntoResponse)
│   │   ├── auth.rs          # JWT + Argon2, HttpOnly cookie, rate limiting
│   │   ├── tuya.rs          # Tuya local TCP (feeder/fountain/litter box)
│   │   ├── meross.rs        # Meross plugs via LOCAL HTTP (reqwest) — the
│   │   │                    #   backend does NOT speak MQTT; the plugs'
│   │   │                    #   firmware needs a reachable TLS broker on
│   │   │                    #   :8883 to boot, hence the Mosquitto service
│   │   ├── hue.rs/hue_stub.rs # Philips Hue BLE (btleplug, feature "bluetooth")
│   │   ├── broadlink.rs     # Broadlink IR manager + persisted climate state
│   │   ├── mitsubishi_ir.rs # Mitsubishi AC IR frame encoder (see §5)
│   │   ├── zigbee.rs        # Zigbee lamp manager (native EZSP only)
│   │   ├── zigbee_native.rs # EZSP/EmberZNet driver (see §4)
│   │   ├── tempo.rs         # RTE Tempo tariffs, history, prediction model
│   │   └── routes/          # one module per domain, all JWT-authenticated
│   │                        #   except /health
│   └── tests/               # integration tests + fixtures
├── frontend/                # React 19 + Vite + TS + Tailwind 4 (Bun)
│   └── src/                 # pages/, components/devices/, components/ui/
│                            #   (shadcn), i18n (en+fr), ThemeContext
│                            #   (system/light/dark, .dark class strategy)
├── cache/tempo/             # Tempo history + calibration (persisted)
├── deploy/                  # OpenRC units, mosquitto conf
├── docs/                    # Pi setup, Tempo calibration, flashing
├── mosquitto/               # broker config + certs (Meross TLS :8883)
├── scripts/                 # Pi cross-build (zigbuild), IR capture, flash
├── Makefile                 # dev targets + Pi deploy wrappers + cloudflared upgrade
├── deploy.sh                # one-shot Pi deployment helper
└── *.json                   # runtime state (devices, lamps, users,
                             #   broadlink-codes, climate-state, …)
```

There is **no zigbee2mqtt integration** (removed 2026-08): no MQTT client in
the backend, no `rumqttc`, no Z2M config. Mosquitto stays solely because
Meross plug firmware requires a reachable TLS broker.

## 3. Zigbee stack (native EZSP)

Dependency chain (all pinned in `backend/Cargo.toml` + lockfile):

- `ashv2` — **uplg fork, branch `upstream-sync`** = upstream
  (PaulmannLighting) v12 + robustness fixes not yet upstream: transmitter
  self-requeue deadlock removed (local pending queue + housekeeping tick),
  frame-number reset after RST/RST-ACK, receiver exit on fatal serial errors,
  active retransmission of timed-out DATA frames, duplicate-retransmission
  payload dedupe. Transport-agnostic (AsyncRead/AsyncWrite); the backend
  opens the port with `tokio-serial`. The `ezsp` cargo feature provides the
  `Transmit`/`Receive` adapters.
- `ezsp` 15 — **uplg fork, branch `upstream-sync`**, wired through
  `[patch.crates-io]` so both the direct dep and ashv2's internal dep
  resolve to it. Single fork patch: `importTransientKey` uses the legacy
  EZSP ≤ v13 wire format (no SecManContext prefix) because the Sonoff
  Dongle Lite MG21 firmware line is EmberZNet 7.4.x = EZSP v13. If the
  dongle ever runs EZSP ≥ v14 firmware, drop this patch and the
  `[patch.crates-io]` entry to run vanilla crates.io ezsp.
- `silizium` 3 — crates.io (security manager types).

Driver design (`zigbee_native.rs`):

- One driver task owns the pipeline: tokio-serial stream → `ashv2::start`
  (2 spawned actor futures) → `ezsp::Client::run` (2 more futures) →
  `Connection` + bounded callback channel. `PipelineTasks` tracks the four
  `JoinHandle`s for liveness (`is_alive`) and bounded teardown (abort+join).
- EZSP protocol version: desired from `ZIGBEE_EZSP_PROTOCOL_VERSION`
  (default 13); on `ProtocolVersionMismatch` the pipeline is rebuilt once
  with the version the NCP announced (firmware upgrades need no config).
- HTTP handlers reach the driver via a bounded command channel; both the
  enqueue and the reply wait are bounded (`COMMAND_REPLY_TIMEOUT`) so no
  request can hang. Touchlink scans get a longer per-command timeout.
- **The driver never dies permanently**: reconnection retries forever with
  capped exponential backoff; past `MAX_RECONNECT_ATTEMPTS` the lifecycle
  reports `Failed` (API fails fast with the reason) while retries continue
  in the background. Watchdog (`WATCHDOG_TIMEOUT`) plus per-command,
  per-callback, and network-bring-up timeouts guard the event loop; any
  breach tears down and rebuilds the pipeline.
- `zigbee.rs` layers lamp persistence (`zigbee-lamps.json`, blacklist) and
  the HTTP-facing views on top of the driver snapshots.

## 4. Climate (Broadlink → Mitsubishi AC)

- `mitsubishi_ir.rs` encodes 18-byte Mitsubishi frames from a command
  grammar: `state-<mode>-<temp>-fan-<fan>-vane-<vane>[-wide-…][-econo-…]
  [-stop-HH-MM][-stopin-<minutes>][-timer-off]` or `state-off`.
- The unit's clock byte is set from the backend's local time on every send
  (the frame resets the AC clock, so absolute timers only work when the
  clock is transmitted). `stopin-<minutes>` is the relative sleep timer the
  UI uses ("turn off in 1h/3h", 10-minute granularity, wraps midnight).
- The last commanded state is persisted (`climate-state.json`, config
  `CLIMATE_STATE_JSON_PATH`) and exposed at `GET /api/broadlink/mitsubishi/
  state`; the frontend form restores it once on mount (`lastOnCommand`
  parsed back into form state, one-shot hydration).

## 5. Other integrations

| Domain | Transport | Notes |
|---|---|---|
| Tuya (feeder/fountain/litter) | local TCP (rust-async-tuyapi fork) | device cache persisted |
| Meross plugs | local HTTP | broker on :8883 only for plug firmware boot |
| Hue lamps | BLE (btleplug), feature-gated | stub on Pi builds |
| Broadlink | UDP discovery + IR send/learn (rbroadlink) | codes in broadlink-codes.json |
| Tempo | RTE + data.gouv + Open-Meteo HTTP APIs | calibrated prediction model, seasons cached |

## 6. Frontend notes

- Theme: `.dark` class on `<html>`, `system/light/dark` in localStorage,
  pre-paint script in `index.html`; `ThemeProvider` sits above the router so
  the login page is themed; `ThemeSwitcher` is available on login and in the
  app layout. Design tokens in `src/index.css` (`@theme` + `.dark` override);
  `tw-animate-css` supplies the shadcn animate-in/out utilities.
- Data fetching: TanStack Query; toasts via sonner wrapper (`use-toast`).
- i18n: i18next, `en` + `fr` locales.
- Build: `bun run build` (tsc + vite), lint `oxlint`, format `oxfmt`.

## 7. Operational notes

- Production = Raspberry Pi 1 only (Alpine 3.24, sys mode, OpenRC — no
  Docker anywhere). Dev machine runs `make backend` / `make frontend`;
  deployment goes through `deploy.sh` (wrapped by `make deploy*`), and
  `make cloudflared-upgrade` rebuilds/swaps the ARMv6 tunnel binary.
- Runtime JSON state lives at the repo root (gitignored where mutable).
- `users.json` requires Argon2 password hashes; the backend refuses the
  default JWT secret.
- Every route sits behind a global 180s timeout layer; auth endpoints are
  rate limited.
