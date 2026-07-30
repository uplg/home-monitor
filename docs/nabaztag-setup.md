# Nabaztag setup

This guide covers reviving a Nabaztag/tag (V2, NA-RTL-002) rabbit from scratch, installing the ServerlessNabaztag firmware, connecting it to a WPA2 or WPA2/WPA3 mixed-mode WiFi network, and integrating it into Maison.

The firmware source and all runtime resources (Forth files, MP3s, choreographies, web UI, OpenAPI spec) are vendored in this repository as a git submodule at `vendor/ServerlessNabaztag/`. After cloning, run:

```bash
git submodule update --init vendor/ServerlessNabaztag vendor/nabgcc vendor/mtl_linux
```

The firmware payload the rabbit downloads at boot lives in `vendor/ServerlessNabaztag/vl/`.

Three git submodules are used:

- `vendor/ServerlessNabaztag/` — ServerlessNabaztag Metal bytecode firmware (layer 2)
- `vendor/nabgcc/` — nabgcc native ARM firmware (layer 1), branch `wpa2` with WPA2/WPA3 mixed-mode support
- `vendor/mtl_linux/` — Metal language compiler (needed to build nabgcc)

## Prerequisites

- A Nabaztag/tag (V2, model NA-RTL-002) — the second-generation rabbit with moving ears and a microphone
- A computer with WiFi
- A WPA2-secured WiFi network (2.4 GHz only — the rabbit has no 5 GHz radio)
- The rabbit's USB power supply
- A LAN where the Maison backend can reach the rabbit over HTTP
- An HTTP server on the LAN to serve the firmware files (see step 5)

The V1 Nabaztag (NA-RTL-001) does **not** have a WiFi chip that supports WPA2 at the firmware level. This guide is specifically for the V2 (Nabaztag/tag).

## Step 1 — Enter configuration mode

The rabbit must be in configuration mode (blue LEDs) to change WiFi and firmware settings.

1. Unplug the rabbit from power
2. Press and hold the head button (the large button on top of the head)
3. While holding the button, plug the USB power cable back in — the LEDs
   turn blue immediately
4. **Release the button within ~2-5 seconds, while the LEDs are blue.** The
   blue stays on and the rabbit starts its config access point.

Timing matters (verified in the boot source, `boot.0.0.0.13.mtl`): the
button is sampled once at power-on, and holding it for **more than 7
seconds** turns the LEDs white and drops the rabbit into its hardware test
mode instead — where every subsequent button press cycles LED colors
(yellow, blue, orange, cyan, magenta) while moving ears and playing sounds.
This is easy to mistake for a bricked device. If you see white LEDs or
color-cycling on presses, just power-cycle and redo the sequence, releasing
earlier.

If the LEDs do not turn blue at all, hold the button firmly before and
during power-on and try again.

## Step 2 — Connect to the rabbit's access point

In config mode, the rabbit broadcasts a WiFi network named `NabaztagXX` where `XX` are the last two hex digits of its MAC address.

1. On your computer, open WiFi settings
2. Connect to the `NabaztagXX` network (no password)
3. Your computer will get an IP via DHCP from the rabbit

You will lose internet access while connected to the rabbit's AP. This is expected.

## Step 3 — Open the configuration page

Open a browser and go to:

```
http://192.168.0.1
```

This is the rabbit's built-in configuration page. It shows fields for:

- **ESSID** — your WiFi network name
- **WPA key** — your WiFi password (WPA2 is supported on the V2)
- **Encryption** — select WPA/WPA2
- **DHCP** — leave enabled unless you want to assign a static IP
- **Violet Platform** — the firmware server URL (this is the key field)

## Step 4 — Configure WiFi (WPA2 / WPA2+WPA3)

Fill in your WiFi credentials:

- **ESSID**: your 2.4 GHz network name (exact, case-sensitive)
- **WPA key**: your WPA2 password
- **Encryption**: WPA or WPA2 (select the one matching your router)
- **DHCP**: enabled (recommended)

WPA2 support comes from the nabgcc wpa2 branch (vendored at `vendor/nabgcc/`). The V2 hardware WiFi chip supports WPA2; it was only the original Violet firmware that was limited to WEP.

### WPA2/WPA3 mixed-mode routers

Many modern routers default to WPA2/WPA3 "transitional" or "compatibility" mode, which requires connecting clients to advertise MFP (Management Frame Protection) capability. The stock nabgcc firmware does not do this and the router rejects the association with status code 31 (ROBUST_MGMT_POLICY).

The `wpa2` branch in `vendor/nabgcc/` includes a fix that declares MFPC=1 (MFP Capable) in the RSN Capabilities field of the Association Request, without implementing full PMF. This works when the router has MFPR=0 (MFP not Required), which is the case for most "compatibility" or "transitional" WPA2/WPA3 modes.

If your router is set to **WPA3-only** (MFPR=1, requiring full PMF with IGTK/BIP-CMAC-128), the rabbit will not be able to connect. Switch your router to WPA2/WPA3 compatibility mode instead.

To use this fix, you need to flash the patched firmware — see "Building and flashing the nabgcc firmware" below.

## Step 5 — Serve the firmware locally and configure the rabbit

The rabbit fetches its firmware files over HTTP from whatever host is set in the **Violet Platform** field. The firmware files are vendored in this repository at `vendor/ServerlessNabaztag/vl/`.

### Serve the firmware from the Maison host

Start an HTTP server on port 80 serving the `vl/` directory. The simplest way:

```bash
# From the repository root
python3 -m http.server 80 --directory vendor/ServerlessNabaztag/
```

Or with a specific bind address (e.g. if the Maison host is `192.168.1.10`):

```bash
python3 -m http.server 80 --bind 192.168.1.10 --directory vendor/ServerlessNabaztag/
```

If port 80 requires root, either use `sudo` or redirect from a higher port:

```bash
# Serve on port 8080 and redirect 80 → 8080 (Linux)
python3 -m http.server 8080 --directory vendor/ServerlessNabaztag/ &
sudo iptables -t nat -I PREROUTING -p tcp --dport 80 -j REDIRECT --to-ports 8080
```

On the Raspberry Pi running Alpine, you can use the same approach or set up a permanent nginx/lighttpd vhost.

Verify the firmware is reachable:

```bash
curl -s http://192.168.1.10/vl/bc.jsp | head -5
```

You should see the bootcode JSP content.

### Set the Violet Platform

In the rabbit's configuration page (http://192.168.0.1), set the **Violet Platform** field to your server:

```
192.168.1.10/vl
```

Replace `192.168.1.10` with the actual IP of the machine serving the files.

Important:

- Do **not** include `http://` or `https://` — just the bare hostname/IP and path
- Do **not** add a trailing slash
- The rabbit will fetch its firmware from this URL on next boot
- The server must be reachable from the rabbit's WiFi network on port 80 (HTTP only — the rabbit does not support HTTPS for firmware downloads)

## Step 6 — Save and reboot

Click the save/apply button on the configuration page. The rabbit will:

1. Disconnect the config AP
2. Reboot
3. Connect to your WiFi network using the credentials you entered
4. Download the ServerlessNabaztag firmware from the configured platform URL
5. Boot into ServerlessNabaztag

The first boot after firmware change may take a minute or two while the rabbit downloads and installs the new firmware files.

After a successful boot, the rabbit should:

- Perform a brief ear movement and LED animation
- Settle into an idle state with ears in their default position
- Be reachable on your local network via its DHCP-assigned IP

## Step 7 — Find the rabbit's IP address

The rabbit gets its IP from your router's DHCP server. To find it:

**Option A — Check your router's DHCP lease table.** Look for a client named `nabaztag` or with a MAC address starting with `00:13:d4` (Violet/Nabaztag OUI).

**Option B — Use nmap to scan your network:**

```bash
nmap -sn 192.168.1.0/24 | grep -B2 "00:13:D4"
```

**Option C — Use arp-scan (macOS with Homebrew):**

```bash
sudo arp-scan --localnet | grep -i "00:13:d4"
```

**Option D — Try mDNS.** Some firmware versions respond to mDNS, but this is not guaranteed.

Once you have the IP (e.g. `192.168.1.42`), verify by opening the web interface:

```
http://192.168.1.42/
```

You should see the ServerlessNabaztag web UI with status information, ear controls, LED controls, and a Forth console.

## Step 8 — First-use checks

Open the web UI at `http://<rabbit-ip>/` and verify:

1. **Status page**: shows firmware version, uptime, free memory, WiFi signal strength
2. **Ears**: try moving left and right ears from the web UI
3. **LEDs**: set a color on the nose LED to confirm LED control works
4. **Time**: the rabbit syncs time via NTP automatically; check the displayed time is correct
5. **Tai Chi**: trigger the Tai Chi animation to test ears + LEDs + sound together

If the web UI loads but some features do not respond, try rebooting the rabbit from the web UI (or power-cycle it) and wait 30 seconds.

## Step 9 — Configure Maison

### Environment variable

Set the rabbit's IP in your `.env` file:

```bash
NABAZTAG_HOST=192.168.1.42
```

This is optional. If not set, the backend reads the host from `nabaztag.json`. You can also configure the host at runtime through the Maison API or frontend.

The config file path can be overridden with `NABAZTAG_JSON_PATH` (defaults to `nabaztag.json` in the project root).

### Runtime configuration via API

With the backend running, configure the rabbit's host and enable Tempo integration:

```bash
curl -X POST http://localhost:3033/api/nabaztag/config \
  -H 'Content-Type: application/json' \
  -H 'Cookie: auth=<your-session-cookie>' \
  -d '{
    "host": "192.168.1.42",
    "name": "Nabaztag",
    "tempoEnabled": true
  }'
```

This persists the configuration to `nabaztag.json`.

### Verify connectivity

```bash
curl http://localhost:3033/api/nabaztag/status \
  -H 'Cookie: auth=<your-session-cookie>'
```

A successful response means the Maison backend can reach the rabbit.

## Step 10 — DHCP reservation

The rabbit has no way to set a static IP from its own UI once running ServerlessNabaztag. To keep its IP stable:

1. Log into your router
2. Find the rabbit's DHCP lease (MAC starting with `00:13:d4`)
3. Create a DHCP reservation / static lease for that MAC address
4. Reboot the rabbit or wait for the lease to renew

This ensures `NABAZTAG_HOST` stays valid across reboots.

## Maison API reference

All endpoints are under `/api/nabaztag` and require authentication.

### Configuration

| Method | Path | Description |
|--------|------|-------------|
| GET | `/config` | Get current config (host, name, tempoEnabled) |
| POST | `/config` | Update config |

### Status

| Method | Path | Description |
|--------|------|-------------|
| GET | `/status` | Full status from the rabbit (firmware, uptime, memory, WiFi, etc.) |

### Sleep / Wake

| Method | Path | Description |
|--------|------|-------------|
| POST | `/sleep` | Put the rabbit to sleep |
| POST | `/wakeup` | Wake the rabbit up |

### Ears

| Method | Path | Description |
|--------|------|-------------|
| GET | `/ears` | Get current ear positions (left, right: 0-16) |
| POST | `/ears` | Move an ear: `{"ear": 0, "position": 8, "direction": 0}` |

Ear values:
- `ear`: 0 = left, 1 = right
- `position`: 0 (down) to 16 (fully up)
- `direction`: 0 = forward, 1 = backward

### LEDs

| Method | Path | Description |
|--------|------|-------------|
| POST | `/leds` | Set LED colors: `{"nose": "#ff0000", "left": "#00ff00", ...}` |
| POST | `/leds/clear` | Clear all LED overrides (pass `-1` to each LED) |

LED fields: `nose`, `left`, `center`, `right`, `base` (all `#RRGGBB` or `"-1"` to clear). Optional `breathing: true` enables the breathing effect.

The 5 physical LEDs on the Nabaztag/tag are:
- 0 = nose (top, forward-facing)
- 1 = left body
- 2 = center body
- 3 = right body
- 4 = bottom/base

### Sound

| Method | Path | Description |
|--------|------|-------------|
| POST | `/play` | Play audio from URL: `{"url": "http://..."}` |
| POST | `/say` | Text-to-speech: `{"text": "Hello"}` |
| POST | `/sound/communication` | Play the MIDI communication jingle |
| POST | `/sound/ack` | Play the MIDI acknowledgment sound |
| POST | `/sound/abort` | Play the MIDI abort sound |
| POST | `/sound/ministop` | Play the MIDI mini-stop sound |
| POST | `/stop` | Stop all current playback |

### Info services (LED animation channels)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/info` | Set an info service: `{"service": "weather", "value": 5}` |
| POST | `/info/clear` | Clear all info service animations |

Valid services: `weather`, `pollution`, `traffic`, `stock`, `mail`, `service4`, `service5`, `nose`. Value range is typically 0-10; set to -1 to clear.

### Utility

| Method | Path | Description |
|--------|------|-------------|
| POST | `/taichi` | Play the Tai Chi animation (ears + LEDs + sound) |
| POST | `/surprise` | Play a surprise animation |
| POST | `/reboot` | Reboot the rabbit |
| POST | `/update-time` | Force NTP time sync |
| GET | `/animations` | List current running animations |
| GET | `/tasks` | List scheduled tasks |

### Setup

| Method | Path | Description |
|--------|------|-------------|
| POST | `/setup` | Update rabbit settings (location, language, sleep schedule, etc.) |

Setup fields (all optional):
- `latitude`, `longitude` — location for weather
- `language` — TTS language
- `taichi` — Tai Chi interval in minutes (0 = disabled)
- `cityCode` — city code for weather
- `dst` — daylight saving time offset
- `wakeUp` — hour to wake up (0-23)
- `goToBed` — hour to go to sleep (0-23)

### Forth interpreter

| Method | Path | Description |
|--------|------|-------------|
| POST | `/forth` | Execute Forth code: `{"code": "1 2 + ."}` |

The response includes the Forth output as a string.

### Tempo integration

| Method | Path | Description |
|--------|------|-------------|
| POST | `/tempo/push` | Fetch Tempo data and push color mapping to LEDs + ears |

Optional body: `{"forceRefresh": true}` to bypass the Tempo cache.

Tempo color mapping:
- **Blue day**: all LEDs blue, ears down (position 0)
- **White day**: all LEDs white, ears mid (position 8)
- **Red day**: all LEDs red with breathing effect, ears fully up (position 16)

Left side LEDs (nose, left, center) show today's color. Right side LEDs (right, base) show tomorrow's color when known.

## ServerlessNabaztag direct API

The rabbit itself exposes an HTTP API on port 80. The Maison backend proxies these, but for debugging you can hit them directly:

```bash
# Status
curl http://192.168.1.42/status

# Move left ear to position 8
curl "http://192.168.1.42/left?p=8&d=0"

# Set nose LED to red
curl -X POST "http://192.168.1.42/leds?n=%23ff0000"

# Text-to-speech
curl "http://192.168.1.42/say?t=hello%20world"

# Execute Forth
curl -X POST http://192.168.1.42/forth -d 'c=1 2 + .'

# Reboot
curl http://192.168.1.42/reboot
```

The OpenAPI spec is available at `http://<rabbit-ip>/openapi.json`. A Swagger UI is also vendored at `vendor/ServerlessNabaztag/vl/api/index.html` (serve it with the same HTTP server used for the firmware).

## Forth examples

The Nabaztag runs a Forth interpreter. You can execute code through the web UI, Telnet (port 22), or the Maison API. The full list of available Forth words is in `vendor/ServerlessNabaztag/vl/words.txt`.

```forth
\ Print stack contents
1 2 3 .s

\ Simple arithmetic
10 20 + .

\ Set nose LED to green
0 255 0 0 leds.set

\ Move left ear to position 12
12 0 0 ear.move

\ Print free memory
mem .

\ List all defined words
words
```

Key Forth files on the rabbit (editable via web UI, source in `vendor/ServerlessNabaztag/vl/`):
- `init.forth` — runs at boot
- `config.forth` — configuration variables
- `consts.forth` — constant definitions
- `crontab.forth` — scheduled tasks
- `hooks.forth` — event hooks
- `telnet.forth` — Telnet session setup
- `weather.forth` — weather display logic

## Telnet access

ServerlessNabaztag exposes an interactive Forth console on TCP port 22 (not the standard Telnet port 23):

```bash
telnet 192.168.1.42 22
```

This gives you a live Forth REPL. Type `bye` to disconnect.

If you set a password in the ServerlessNabaztag web UI, the same password is required for both Web and Telnet access.

## Building and flashing the nabgcc firmware

The rabbit runs two firmware layers:

1. **Layer 1 (native ARM, `.sim` file)** — nabgcc: C firmware with WiFi/WPA2 stack, USB driver, HTTP bootloader, and a virtual machine. This is flashed directly to the rabbit's flash memory.
2. **Layer 2 (bytecode, `bc.jsp`)** — ServerlessNabaztag: Metal language bytecode compiled by `mtl_compiler`, downloaded by the rabbit over HTTP on each boot.

The WPA2/WPA3 fix lives in layer 1 (nabgcc). You only need to rebuild and reflash nabgcc if you are modifying the native firmware (WiFi stack, association, EAPOL handshake, etc.). Layer 2 (ServerlessNabaztag) is served from `vendor/ServerlessNabaztag/vl/` and does not need recompilation.

### Prerequisites

- Docker (with `linux/amd64` emulation if on Apple Silicon)
- Rust toolchain (for building `tools/mkfirmware`)
- Git submodules initialized:
  ```bash
  git submodule update --init vendor/nabgcc vendor/mtl_linux
  ```

### Build with the script

```bash
scripts/build-nabgcc.sh
```

This will:

1. Build the `mkfirmware` Rust tool on the host
2. Start a Docker container (`debian:bullseye-slim`, `linux/amd64`) with `gcc-arm-none-eabi`, `g++-multilib`, and `libnewlib-arm-none-eabi`
3. Compile the Metal boot bytecode with `mtl_compiler` (32-bit)
4. Cross-compile nabgcc with `arm-none-eabi-gcc`
5. Generate the `.sim` firmware file at `vendor/nabgcc-latest/Nab-wpa23.sim`

For a smaller production firmware without debug logging:

```bash
scripts/build-nabgcc.sh --release
```

This disables `DEBUG_VM`, `DEBUG_AUDIO`, and `DEBUG_MAIN`, producing `Nab-wpa23-release.sim`.

### Build output

| File | Location | Description |
|------|----------|-------------|
| `Nab.bin` | `vendor/nabgcc/bin/` | Raw ARM binary (~115 KB debug, ~105 KB release) |
| `Nab.elf` | `vendor/nabgcc/bin/` | ELF with debug symbols |
| `Nab-wpa23.sim` | `vendor/nabgcc-latest/` | Encrypted firmware ready to flash (~230 KB) |

Build artifacts in `vendor/nabgcc/bin/` and `vendor/nabgcc/obj/` are gitignored.

### The .sim firmware format

The `.sim` file is the format the rabbit's bootloader expects for firmware uploads. It consists of:

```
-violet- <hex-encoded-size> <hex-encoded-encrypted-bytes> -violet-
```

- The size field is 8 hex characters representing `2 * binary_size`
- Encryption is a simple byte cipher using a lookup table, with initial key `0x47`
- The `tools/mkfirmware` Rust binary handles this encoding (a faithful port of the original `vendor/nabgcc/utils/mkfirmware.php`)

### Flash the firmware

1. Put the rabbit in configuration mode (hold head button, plug in power, wait for blue LEDs)
2. Connect to the `NabaztagXX` WiFi access point
3. Open `http://192.168.0.1/u.htm` in a browser
4. Upload the `.sim` file (`vendor/nabgcc-latest/Nab-wpa23.sim`)
5. Wait for the upload to complete — the rabbit will reboot automatically
6. Reconnect to the `NabaztagXX` AP and configure WiFi settings at `http://192.168.0.1` (see Step 3-4 above)

The `/u.htm` page is the rabbit's built-in firmware upload form. It is separate from the main configuration page.

After flashing, you must reconfigure WiFi settings — the firmware update does not preserve them.

### Pre-built firmware

A pre-built `.sim` with the WPA2/WPA3 fix is committed at:

```
vendor/nabgcc-latest/Nab-wpa23.sim
```

If you do not need to modify the nabgcc source, you can flash this file directly without building.

The original unmodified firmware is also available for comparison:

```
vendor/nabgcc-latest/Nab0013.01ca89.sim
```

## Troubleshooting

### Rabbit stays in config mode (blue LEDs) after saving WiFi settings

- Double-check the ESSID is exactly right (case-sensitive, no trailing spaces)
- Verify your network is 2.4 GHz — the rabbit does not support 5 GHz
- If your router uses WPA2/WPA3 mixed mode, you need the patched nabgcc firmware (see "Building and flashing the nabgcc firmware" above)
- Try WPA instead of WPA2 if your router supports both
- Power-cycle the rabbit (unplug, wait 5 seconds, replug without holding the button)

### Rabbit boots but is not reachable on the network

- Check your router's DHCP lease table for new entries
- The rabbit may take 30-60 seconds to fully boot and get an IP
- Make sure your computer is on the same subnet as the rabbit
- Try a network scan with nmap

### Maison backend returns "Nabaztag host not configured"

- Set `NABAZTAG_HOST` in `.env` or POST to `/api/nabaztag/config` with the rabbit's IP
- Restart the backend after changing `.env`

### Maison backend returns "Bad Gateway" or timeout

- Verify the rabbit is powered on and connected to WiFi
- Ping the rabbit: `ping 192.168.1.42`
- Check that no firewall is blocking HTTP traffic to the rabbit
- The rabbit's HTTP server runs on port 80; ensure nothing is intercepting that

### LEDs or ears do not respond but status works

- The rabbit may be in sleep mode; try POST to `/api/nabaztag/wakeup`
- Reboot the rabbit from the API or web UI
- Check that the Forth `init.forth` has not overridden LED/ear behavior

### Firmware update fails (rabbit reverts to old firmware)

- The Violet Platform URL must not include `http://` — just the bare hostname and path (e.g. `192.168.1.10/vl`)
- The rabbit must be able to reach the firmware server on port 80 (HTTP, not HTTPS)
- Verify the firmware server is running: `curl http://192.168.1.10/vl/bc.jsp`
- The firmware files are in `vendor/ServerlessNabaztag/vl/` — make sure the submodule is initialized

### Re-entering configuration mode

If you need to change WiFi settings or the firmware URL later, repeat the config mode entry from Step 1 (unplug, hold button, replug, wait for blue LEDs).

### WiFi association fails with status 31 (ROBUST_MGMT_POLICY)

This means the router requires MFP (Protected Management Frames) capability and the rabbit's firmware is not advertising it. This happens with WPA2/WPA3 mixed-mode routers.

Fix: flash the patched nabgcc firmware from `vendor/nabgcc-latest/Nab-wpa23.sim` (see "Building and flashing the nabgcc firmware" above).

If the problem persists after flashing, check your router settings:
- Switch to "WPA2/WPA3 Compatibility" or "Transitional" mode (not WPA3-only)
- Ensure MFPR (MFP Required) is disabled — the rabbit declares MFP Capable but does not implement full PMF
