# Nabaztag MTL VM: ABI reference and rewrite campaign notes

Reference for writing our own embedded application (`bc.jsp`) for the
Nabaztag:tag running the nabgcc firmware (uplg/nabgcc, `wpa23` fixes).
Compiled 2026-07-30 from the ServerlessNabaztag (SN) sources, which serve
as the map of the VM's ABI. The VM itself lives in the C firmware
(`src/vm/vinterp.c` and friends) and is frozen: this ABI will not move.

## Why this campaign is safe

`bc.jsp` is downloaded by the flash boot bytecode at every boot, never
flashed. A broken application means a reboot loop or a wedged VM; recovery
is fixing the file served by clapier and power-cycling the rabbit. The
config mode (head button held at boot) lives in flash and stays untouched.

## Toolchain (validated)

Docker image `mtl-dev` (debian bookworm-slim amd64 + gcc-multilib,
g++-multilib, make, perl, python3, xxd, curl). All commands run from
`vendor/ServerlessNabaztag` with:

```
rtk proxy docker run --rm --platform linux/amd64 \
  -v /Users/leonard/Github/cat-monitor:/work \
  -w /work/vendor/ServerlessNabaztag mtl-dev bash -c '<cmd>'
```

- Build compiler + simulator (once): `make compiler`
- Preprocess: `bash scripts/make_nominal.sh [-D SIMU]` -> `nominal.mtl`
  (concatenates `firmware/main.mtl` includes via `scripts/preproc.pl`)
- Compile: `./compiler/mtl_comp/mtl_comp -s nominal.mtl <out.bin>`
- Simulate: `./compiler/mtl_simu/mtl_simu --mac 0123456789ab \
  --logs init,vm,http_server --source nominal.mtl \
  --http_server_path vl --http_server_port 8081`
  (renders LEDs as ANSI truecolor in the terminal, ears as numbers;
  simulated net/audio/motors; serves HTTP from the given path)

Validation: SN sources recompile to 95 904 bytes / 749 functions
(served bc.jsp is 95 898; delta is a `$Rev$`-style source drift, not a
toolchain defect). Our build boots in the simulator (green breathing LED,
ears 13/13).

## The Metal language (VLISP / Sylvain Huet, 2005-2006)

ML-flavored, compiled to VM bytecode. Learned from the SN sources:

```
proto main 0;;                 // forward declaration, arity
var _leds_net_activity = 0;;   // global
const LEDS_OSC = { 0 1 2 ... };;  // table constant, dot-indexed: LEDS_OSC.x
fun _leds_osc x =
    let (x>>6)&3 -> q in       // let expr -> name in body
    if q==0 then LEDS_OSC.x
    else ...;;
```

- Statements end with `;;`. Comments `//` and `/* */`.
- C-style preprocessor (perl): `#include`, `#define`, `#ifdef`.
- Strings, lists (`hd`/`tl`), tables; GC in the VM (`gc` opcode).
- Protos live in `firmware/protos/*_protos.mtl`, one per module.

## VM natives (the hardware ABI, from compiler/vbc_str.h, 152 opcodes)

Beyond the usual VM core (arith, strings, tables, control flow):

| Domain   | Natives |
|----------|---------|
| LEDs     | `led` |
| Ears     | `motorset`, `motorget` |
| Buttons  | `button2`, `button3` |
| Audio out| `playStart`, `playFeed`, `playStop`, `playTime`, `sndVol`, `sndRefresh`, `sndWrite`, `sndRead`, `sndFeed`, `sndAmpli` |
| Audio in | `recStart`, `recStop`, `recVol`, `adp2wav`, `wav2adp`, `alaw2wav`, `wav2alaw` |
| Radio    | `netCb`, `netSend`, `netState`, `netMac`, `netChk`, `netSetmode`, `netScan`, `netAuth`, `netSeqAdd`, `netRssi`, `netPmk` |
| Sockets  | NOT IMPLEMENTED ON DEVICE, see below |
| Storage  | `envget`, `envset` (config sector), `load`, `save`, `bytecode` |
| RFID     | `rfidGet`, `rfidGetList`, `rfidRead`, `rfidWrite` |
| I2C      | `i2cRead`, `i2cWrite` |
| System   | `time`, `time_ms`, `loopcb`, `reboot`, `gc`, `corePP`, `corePush`, `corePull`, `coreBit0`, `crypt`, `uncrypt` |
| DANGER   | `flashFirmware` — the bytecode can reflash the C firmware. Never expose it in our application. |

## The network reality (verified in our own C firmware, vendor/nabgcc)

`grep 'case OPtcp|case OPudp' src/vm/vinterp.c` returns **zero**. The device
implements only raw 802.11 data frames plus radio control:

```
netCb #handler            handler is called as: fun handler frame macsrc
                          (both MTL strings; frame is the received payload)
netSend buf index len macdst indmac speed    -> int, sends buf[index..index+len]
netChk buf index len seed -> int             one's-complement checksum (IP/TCP/UDP)
netMac                    -> 6-byte string   our MAC
netState                  -> int             rt2501 link state
netRssi                   -> int             average RSSI
netScan ssid              -> list of tables  each: [ssid mac bssid rssi chn rateset encryption]
netAuth scan mac authmode encrypt key        associates (key = 32-byte PMK)
netPmk ssid passphrase    -> 32-byte string  PBKDF2, done in C (slow in MTL)
netSetmode mode ssid chn                     station / master (AP) mode
netSeqAdd seq n           -> 4-byte string   32-bit big-endian add, for TCP seq math
```

Consequences for garenne:

1. **We write the whole IP stack in MTL**: Ethernet-ish framing over the
   rt2501, ARP, IPv4, ICMP, UDP, DHCP, DNS, TCP. This is not optional and
   it is the bulk of the remaining work.
2. **The simulator cannot test it.** `mtl_simu` (linux_simunet.c) offers
   BSD-socket natives `tcpOpen/tcpListen/tcpSend/udpSend` and calls back on
   `SYS_CBTCP`; it does **not** emulate raw frames. So a simulator build
   must bind garenne's socket API to those natives, while the device build
   binds it to our own stack. One narrow seam, two backends: keep the
   application above `sock_*` identical, swap the layer below with `#ifdef
   SIMU`. That seam is the only place the two builds may differ.
3. `netChk` and `netSeqAdd` exist precisely because checksums and 32-bit
   sequence arithmetic are painful in the VM. Use them.

## SN firmware module map (reference only, no code is imported)

```
firmware/
  main.mtl          entry point + feature flags
  hw/               leds, ears, button, rfid          (thin, sane: KEEP as base)
  ipv4/             arp, icmp, tcp, udp, trame        (TCP/IP in MTL: KEEP)
  net/              wifi, dhcp, dns, http, ntp, sock  (KEEP, audit)
  utils/            json, task, md5, b64, url, time…  (KEEP, prune)
  audio/            audiolib, midi, record, reclib    (KEEP, audit)
  chor/             choreographic, palette, streaming (KEEP, audit)
  srv/              http_server, telnet_server        (REWRITE: our API)
  run/              run.mtl, ping.mtl, xmpp.mtl 118K  (REWRITE: our app; drop XMPP/PING)
  forth/            forth interpreter + words         (DECIDE: nice feature, big)
  protos/           declarations per module
```

Feature flags in main.mtl: `SERVERLESS` vs `PING` vs `XMPP` (Violet legacy),
`WEBSERVER`, `TELNETSERVER`, many `*_DEBUG`.

Broken-by-design in SN (do not port): `say` via a long-dead Google
Translate TTS endpoint; hooks POSTing to `/hooks/*.php` nobody serves;
weather JSON parsed inside the VM (fragile), UI (vl/index.html) fetched
from the platform (we replace it with our own page, no bytecode change
needed).

## Metal, learned the hard way

- The compiler is a real ML type checker with unification. `S cannot be
  unified with fun` means an argument order mismatch, not a syntax error.
- A `proto` is an arity declaration only, and every proto that reaches the
  compiler needs a body: pulling in one module drags its whole dependency
  cone (their LED module knows about audio, their scheduler formats JSON,
  their JSON depends on Forth). That is why garenne imports nothing.
  `preproc_remove_extra_protos.py` only strips duplicate protos.
- Records: `[field:value field2:value2]`, read `x.field`, write
  `set x.field = v`. Indirect call `call f [arg1 arg2]`, function
  reference `#name`.
- `match x with (Ctor -> e) | (Ctor arg -> e) | (_ -> e)`; sum types via
  `type T = A | B _ | C;;`.
- Loops: `for l = list; l != nil; tl l do body`. Lists: `hd`, `tl`, `::`.
- Statements end with `;;`. `let value -> name in body` binds.
- Strings are byte buffers (`strnew`, `strget`, `strset`, `strsub`,
  `strcat`, `strcatlist`): the natural packet buffer type.

## Campaign plan

1. DONE toolchain + validation (2026-07-30)
2. DONE ABI inventory (this document)
3. DONE garenne v0.1: our scheduler + our LED driver, 780 bytes, breathes
   violet in the simulator
4. v0.2 network, entirely ours: config sector via `envget`, association via
   `netScan`/`netAuth`, then Ethernet/ARP/IPv4/ICMP/UDP/DHCP, then TCP, then
   an HTTP server. Simulator seam as described above.
5. Real-rabbit test served by clapier (rollback: restore the old bc.jsp)
6. Organs one by one: ears (`motorset`/`motorget`), audio, button, RFID
7. Our web UI as `vl/index.html` (same-origin JS onto the rabbit's API)
8. Services rethought: speech synthesized server-side into MP3s the rabbit
   plays, weather pre-digested by clapier, chimes and surprises curated
