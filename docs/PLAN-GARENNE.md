# PLAN-GARENNE: the garenne + clapier campaign

Written 2026-07-30 (Fable), superseding the milestone list at the end of
`docs/nabaztag-mtl-abi.md`. Same goal, different order: the previous plan
built the whole IP stack before the first real-rabbit test. This one puts
the device feedback loop first, because the firmware campaign already
taught us what debugging blind costs.

Ground rules (standing, from Leonard): all code and comments in English;
no commit or push without his explicit go (commits prepared, he pushes);
ServerlessNabaztag (SN) and the Violet sources are datasheets, never
donors — zero imported lines.

## New facts that reshape the plan

Verified today against the official Violet documents (now archived in
`vendor/violet/`) and the toolchain sources:

1. **The frame boundary is LLC/SNAP, not Ethernet.** The Violet driver
   spec (`DT_violet4_lisp-wifi-driver_revE.pdf`, §3.4) states it
   explicitly: a frame handed to `netSend`/received via `netCb` starts at
   the LLC header — `AA AA 03 00 00 00` + 2-byte ethertype (0x0806 ARP,
   0x0800 IPv4) — followed directly by the ARP/IP packet. Destination MAC
   is a separate `netSend` argument; source MAC arrives as a separate
   callback argument. SN's `arp.mtl` concurs byte for byte. There is no
   Ethernet layer to write. RX parsing starts at offset 8.
2. **The simulator can test the stack.** `compiler/mtl_simu/vnet.c`
   really implements `netChk` (one's-complement checksum) and `netSeqAdd`
   (32-bit big-endian add); only `netSend` is a stub. So if the stack is
   written as pure functions (frame string in, frame strings out, natives
   only at the edges), the simulator becomes our unit-test runner. The
   old claim "the simulator cannot test it" only holds for the
   `netSend`/`netCb` edges, which are ~20 lines.
3. **Association is inherited, not established.** `bc.jsp` arrives over
   the WiFi seconds before garenne starts; the rt2501 association lives
   in the C firmware and survives the bytecode handoff. At t=0 garenne
   has a working link (`netState` == 4, RT2501_S_CONNECTED per the spec).
   `netScan`/`netAuth` is *reconnection* logic — robustness, not
   bring-up. The previous plan had it first; it belongs near the end.
4. **DHCP is off the critical path.** The boot bytecode already ran DHCP
   for our MAC; the Freebox holds the lease (192.168.1.155). Garenne can
   use that IP statically for the whole bring-up (hardcoded first, then
   read from the config sector — the Violet boot source shows static-IP
   fields and a `CONF_NETDHCP` flag at offset 41). A full DHCP client is
   a late-robustness item, not a prerequisite for TCP.
5. **`netState` values** (spec §3.3): 0 broken, 1 idle, 2 scanning,
   3 connecting, 4 connected, 5 master. `netSend` has a `lowrate` flag
   meant for short critical frames (ARP, DHCP) — use it there.

## Architecture decisions (proposed, Leonard arbitrates)

- **The rabbit fetches; clapier digests.** Anything the rabbit must parse
  is served by clapier in a fixed binary or line-oriented format designed
  for MTL (no JSON parser in the VM — that fragility was SN's mistake).
  Weather, time, playlists: pre-chewed server-side.
- **HTTP dialect mirrors the rabbit's own:** one request per connection,
  `Connection: close`, full `Content-Length`, `User-Agent: garenne/x.y`.
  Clapier was built for exactly this dialect; garenne's client and server
  both speak it. No keep-alive, no pipelining, no chunked encoding.
- **The UI lives on clapier, not on the rabbit.** The rabbit exposes a
  small HTTP API (`/status`, later actions) with
  `Access-Control-Allow-Origin: *`; the browser loads the page from
  clapier and calls the rabbit cross-origin. A 33 MHz ARM7 running a VM
  should not serve web assets. (SN had the rabbit involved in its UI;
  we deliberately do not.)
- **Clapier serves a tribe, not a rabbit** (Leonard, 2026-07-30). The
  identity is already on the wire: the boot request is
  `bc.jsp?v=0.0.0.13&m=00:19:db:9c:28:15&l=…&p=…&h=4` (verified in the
  prod log) — the `m` param is the rabbit's MAC, sent before any of our
  code runs. So content resolution is keyed on it from day one:
  `overlay/rabbits/<mac>/…` → `overlay/common/…` → SN tree, falling back
  to the identity-less chain when a request carries no `m` (SN's *.forth
  fetches don't; every garenne request will). The MAC is untrusted input:
  validate strictly (hex and colons only, fixed length) before it goes
  anywhere near a path. What this buys: **canary deploys** — one rabbit
  runs a new garenne while the others stay put, which is also how the
  campaign itself will test against a live tribe member; later, per-rabbit
  config (name, identity color, behaviors) served as a small binary blob.
- **Out of scope, permanently until proven needed:** DNS (the config
  sector carries the server as a dotted-quad IP on our LAN), IP
  fragmentation (drop), IPv6, TLS, congestion control beyond a fixed
  small window, XMPP, Forth, telnet. `flashFirmware` is never called —
  greppable invariant.

## Track C — clapier

**C1. Overlay + one-command deploy (prerequisite for G0).**
Add `--overlay <dir>` to clapier-vl with the tribe-aware resolution
chain: `overlay/rabbits/<mac>/…` (when the request carries a valid `m`)
→ `overlay/common/…` → SN tree. Keeps `vendor/ServerlessNabaztag`
pristine and makes rollback = delete one file. Small + tests (including
one asserting a hostile `m` never reaches the filesystem), plist edit,
service restart. Then `scripts/deploy-garenne.sh` in cat-monitor: build
garenne, copy `build/garenne.bin` to `<overlay>/…/vl/bc.jsp` (atomic
rename), targeting `common` or a specific rabbit (`--rabbit <mac>` for
canary); `deploy-garenne.sh rollback` removes it. Power-cycle the rabbit
to take effect (until G3 removes the need to stand up).
*Fallback if we prefer not to touch prod code: the script swaps the file
inside the SN tree with a `.sn` backup — works, dirties vendor, no
per-rabbit routing.*

**C2. Tribe-aware observability (alongside G4).**
Journal tags requests with `User-Agent: garenne/x.y` and the `m` param;
`/_clapier` grows a fleet table: one row per MAC ever seen — last seen,
source IP, boot version or garenne version, uptime (from a heartbeat
query string). Cheap, and turns clapier logs into the server-side eyes
of every test, per rabbit.

**C3. House content + services (after G7).**
Content root moves out of `vendor/` into a small owned tree (bc.jsp,
sounds, UI page). Binary endpoints for the rabbit: `/garenne/time`,
`/garenne/weather`, `/garenne/config` (per-rabbit blob: name, identity
color, behaviors — resolved through the same tribe chain), curated MP3
library. TTS fully local: macOS `say` → MP3 (afconvert/ffmpeg) → served
by clapier, played by the rabbit. The SN tree is retired the day garenne
covers what Leonard actually uses.

**Standing:** commit 9f4c02b awaits `git push -u origin main` by Leonard;
every new clapier change is prepared, shown, and committed only on go.

## Track G — garenne (each milestone has a visible proof)

**G0. v0.1 on the real rabbit.**
Add ~15 lines first: base LED color driven by `netState` (violet
breathing if 4, amber otherwise). Deploy via C1, power-cycle.
*Proof: the physical rabbit breathes violet.*
This single test settles four unknowns at once: mtl_comp output is a
valid bc.jsp, the boot handoff runs our bytecode, the association
survives into the VM, and `loopcb`/`led`/`time_ms` behave on hardware
like in the simulator. Rollback: `deploy-garenne.sh rollback` + replug.

**G1. The rabbit speaks — UDP log channel.**
`net/llc.mtl` (8-byte header build/parse), `net/ipv4.mtl` (header build,
`netChk` checksum), `net/udp.mtl` (datagram build), `log.mtl`: broadcast
a log line to 255.255.255.255:9999 (lowrate, no ARP needed), IP
hardcoded for now. `scripts/garenne-listen.py` on the Mac (same pattern
as `diag-listen.py`).
*Proof: garenne loglines scroll on the Mac. printf exists on device.*

**G2. The rabbit answers — RX path.**
`netCb` dispatcher (ethertype switch, cheap early drops), `net/arp.mtl`
(reply, request, small cache, gratuitous announce at init),
`net/icmp.mtl` (echo reply).
*Proof: `ping 192.168.1.155` answers.* The full TX+RX round trip is
proven with ~150 lines of stack, before TCP exists.

**G3. Remote control — the iteration unlock.**
UDP listener on a control port (magic cookie prefix, LAN only):
`reboot` (native), `led`, `loglevel`, `stats`. `scripts/garenne-ctl.py`.
*Proof: `garenne-ctl reboot` → rabbit reboots → boot fetches the fresh
bc.jsp.* From this point new builds deploy without touching the rabbit;
the edit-deploy-test loop drops to ~30 seconds. This is why G3 comes
before the hard work, not after.

**G4. TCP client + HTTP GET — the mountain.**
`net/tcp.mtl`: minimal-correct — 4-slot connection table, 3-way
handshake, in-order-only reassembly (drop out-of-order, let retransmit
heal), retransmit on timeout, `netSeqAdd` for all sequence math (VM ints
are not 32-bit safe; every 32-bit wire quantity stays a 4-byte string),
MSS 536 to start (raise to 1460 once stable), proper FIN, skip TIME_WAIT
niceties. `net/env.mtl`: config-sector read via `envget` (server IP at
offset 0x00; map the full layout from the Violet boot source while
there). `http/client.mtl`: GET, Host, Connection: close, read to close; every
request carries `?m=<netMac>` so the tribe routing and the fleet table
see garenne exactly like they see the boot.
*Proof: garenne fetches `/vl/motd.txt`; the request shows in clapier's
journal with `User-Agent: garenne/0.4` and its MAC; body echoed on the
G1 log.*
Client before server because clapier's journal gives free server-side
observability, and fetching is the load-bearing product path (audio,
config). Unicast needs G2's ARP resolve — order holds.

**G5. HTTP server — `/status`.**
Listen on :80, one connection at a time is acceptable, tiny router:
`/status` (version, uptime, netState, RSSI via `netRssi`, counters,
task list) + `Access-Control-Allow-Origin: *`.
*Proof: `curl http://192.168.1.155/status` from the Mac.*

**G6. Network robustness.**
Link watchdog task on `netState`; reassociation via `netScan`/`netAuth`
with PMK and SSID from the config sector; full DHCP client (retire the
hardcoded IP); ARP cache aging; retransmit tuning; error counters
surfaced in `/status`.
*Proof: toggle the Freebox WiFi → the rabbit re-associates and re-leases
alone; a scapy malformed-frame fuzz doesn't kill it; ping flood holds.*

**G7. Organs, one driver at a time.**
Ears (`motorset`/`motorget`, position calibration, small choreography
primitives); button (`button2`/`button3`: single/double/long); audio out
(HTTP-stream MP3 from clapier through `playStart`/`playFeed`/`sndVol` —
streaming while downloading is the one real-time-sensitive piece);
record and RFID optional, on demand. Each organ: its own module, a
`/status` field, a `garenne-ctl` demo verb.

**G8. Product.**
App behaviors composed from the above: heartbeat to clapier, daily
chimes, weather color on the nose LED, TTS messages, quiet hours. UI on
clapier as a tribe dashboard — one card per rabbit, driving each
rabbit's API directly (IPs learned from the fleet table). Shaped with
Leonard, not speculated here.

## Track T — tests (transversal, starts at G1)

- **Golden-frame unit tests in the simulator.** `garenne/tests/*.mtl`:
  reference frames generated once with scapy on the Mac, embedded as hex
  strings; pure stack functions asserted against them (parse → fields,
  build → exact bytes, checksum round-trips, TCP state transitions
  driven by synthetic segments). `build.sh test` runs them in the
  `mtl-dev` container; one PASS/FAIL line per module. CI-able.
- **The sock seam stays, demoted to a convenience.** `net/sock.mtl`
  (device: our stack) vs `net/sock_sim.mtl` (simulator: `tcpOpen`/co
  BSD natives), single `#ifdef SIMU` seam, identical app above. Its job
  is interactive whole-app runs in the terminal — the sim build can even
  talk to the real clapier. Correctness of the stack itself is the
  golden-frame suite's job, on the pure functions, sim and device alike.
- **On device:** G1 logs are printf, G3 is the remote handle, the LED is
  coarse state. Nothing is ever debugged blind again.

## Proposed layout

```
garenne/
  main.mtl  tasks.mtl  leds.mtl  app.mtl  log.mtl
  env.mtl                      // config sector
  net/  llc.mtl arp.mtl ipv4.mtl icmp.mtl udp.mtl tcp.mtl
        dhcp.mtl link.mtl sock.mtl sock_sim.mtl
  http/ client.mtl server.mtl
  tests/ vectors.mtl t_ipv4.mtl t_arp.mtl t_udp.mtl t_tcp.mtl ...
  build.sh                     // + `test` mode
scripts/ deploy-garenne.sh garenne-listen.py garenne-ctl.py
```

## Effort guess (sessions, honest)

C1+G0 ≈ 1 · G1+G2 ≈ 1 · G3 ≈ ½ · G4 ≈ 2–3 (the mountain) · G5 ≈ 1 ·
G6 ≈ 1–2 · G7 ≈ 1–2 per organ · tests amortized throughout. Bytecode
size is a non-issue (SN proves ≥95 KB loads; garenne through G6 stays
under ~20 KB).

## Open questions, each pinned to the milestone that answers it

- ANSWERED (G0, 2026-07-30 21:44, on hardware): bc.jsp container format
  IS the raw mtl_comp output — the boot loaded and ran it.
- ANSWERED (G0, same run): the association survives the boot→VM handoff
  (`link=4` in the very first pulse). netScan/netAuth is G6 material,
  as planned.
- ANSWERED (G1, same run): `netSend` with nil length sends the whole
  string; our LLC/SNAP + IPv4 + UDP framing and checksums are accepted
  by the Mac's kernel — the golden vectors told the truth.
- Does the boot leave a `netCb`/`loopcb` handler registered that garenne
  must overwrite → **G1** init order (register ours first thing).
- Exact config-sector layout (static IP fields, server as dotted quad) →
  mapped from the Violet boot source during **G4**'s env.mtl.
- VM integer width (31-bit tagged suspected — the existence of
  `netSeqAdd` is the tell) → grammar PDF + one sim test before **G4**;
  until then the 4-byte-string rule stands everywhere.

## Datasheet shelf (read, never copy)

Primary — official Violet/Huet, in `vendor/violet/`: the WiFi driver
spec (the `netSend`/`netCb`/`netState` contract), the Metal grammar PDF,
the natives XLS, `boot.0.0.0.11.txt` (DHCP + config sector + handoff,
what runs before us), `nominal.010115.txt` (Violet's own app).
Secondary — SN `firmware/` (how someone else drove the same ABI),
`docs/nabaztag-mtl-abi.md` (our notes). Metal doc online:
sylvain-huet.com/rsc/metal/doc.

## Session 4, concretely

1. C1: overlay in clapier-vl + tests, deploy script — commit prepared,
   Leonard validates and pushes.
2. G0: netState LED color, build, deploy, power-cycle, photograph the
   violet breath.
3. G1: llc/ipv4/udp build path + log.mtl + garenne-listen.py — first
   loglines from inside the VM.
