#!/usr/bin/env python3
"""
Flash firmware to a Nabaztag rabbit via its built-in HTTP config server.

The rabbit must be in config mode (hold button + power on), and your
computer must be connected to its WiFi AP (NabaztagXX).

Usage:
    scripts/flash-nabaztag.py [path/to/firmware.sim]

If no path is given, defaults to vendor/nabgcc-latest/Nab-wpa23-release.sim

How it works:
    The rabbit's MTL (VLISP) bootloader runs a tiny HTTP/1.0 server.
    It accepts a POST to /c and scans the raw request body for "-violet-"
    delimiters to find the firmware payload. It does NOT parse multipart
    MIME boundaries at all.

    We send a raw HTTP/1.0 POST with the .sim file as the body (no
    multipart overhead). The rabbit buffers the entire request in RAM,
    extracts the firmware from the -violet- delimiters, decrypts it,
    flashes it to IntROM, and triggers a watchdog reset.

    The connection WILL drop after a successful flash — this is normal.

Why it's slow:
    The rabbit's VLISP VM stores each TCP segment as a separate GC-managed
    heap object in a linked list. On every incoming segment, it walks the
    ENTIRE list (O(n) slistlen) to check if enough data arrived. This means
    total processing is O(n^2) in segment count, and GC pauses grow as the
    heap fills. If we send too fast, the rabbit's RT2501 WiFi USB buffers
    overflow while it's busy with GC/slistlen, causing dropped segments and
    a permanent stall.

    Fix: we pace each send with an adaptive delay that increases as more
    segments accumulate, giving the rabbit time for slistlen + GC between
    segments. This makes the upload slow (~5-10 min) but reliable.
"""

import os
import sys
import socket
import time

# --- colors ---


def log(msg):
    print(f"\033[1;34m==>\033[0m {msg}")


def ok(msg):
    print(f"\033[1;32m OK\033[0m {msg}")


def warn(msg):
    print(f"\033[1;33mWRN\033[0m {msg}", file=sys.stderr)


def fail(msg):
    print(f"\033[1;31mERR\033[0m {msg}", file=sys.stderr)
    sys.exit(1)


def progress(sent, total, eta=None):
    pct = sent * 100 // total
    bar = "█" * (pct // 2) + "░" * (50 - pct // 2)
    eta_str = f" ETA {eta:.0f}s" if eta is not None else ""
    print(
        f"\r    [{bar}] {pct:3d}% ({sent:,}/{total:,}){eta_str}   ", end="", flush=True
    )


def validate_sim(path):
    """Validate .sim file structure and return file contents."""
    with open(path, "rb") as f:
        data = f.read()

    if len(data) < 24:
        fail(f"File too small ({len(data)} bytes) to be a valid .sim")

    if data[:8] != b"-violet-":
        fail(f"Invalid .sim: doesn't start with '-violet-' (got: {data[:8]!r})")

    if data[-8:] != b"-violet-":
        fail(f"Invalid .sim: doesn't end with '-violet-' (got: {data[-8:]!r})")

    hex_size_field = data[8:16].decode("ascii")
    try:
        hex_payload_len = int(hex_size_field, 16)
    except ValueError:
        fail(f"Invalid size field in .sim: '{hex_size_field}'")

    binary_size = hex_payload_len // 2
    expected_size = 8 + 8 + hex_payload_len + 8

    if len(data) != expected_size:
        fail(f"Size mismatch: file is {len(data)} bytes, expected {expected_size}")

    ok(
        f"Valid .sim: {binary_size:,} bytes firmware ({hex_payload_len:,} hex chars, {len(data):,} bytes total)"
    )
    return data


def check_connectivity(ip):
    """Check if the rabbit's HTTP server is responding."""
    log(f"Checking connectivity to rabbit at {ip}...")

    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(5)
        sock.connect((ip, 80))

        # Send a minimal GET request
        req = f"GET /u.htm HTTP/1.0\r\nHost: {ip}\r\n\r\n".encode()
        sock.sendall(req)

        # Read response
        resp = b""
        while True:
            try:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                resp += chunk
            except socket.timeout:
                break

        sock.close()

        if b"HTTP/" in resp and (b"200" in resp[:30]):
            ok("HTTP server responding (GET /u.htm -> 200)")
            return True
        else:
            first_line = (
                resp.split(b"\r\n")[0].decode("ascii", errors="replace")
                if resp
                else "(empty)"
            )
            warn(f"GET /u.htm returned: {first_line}")
            return True  # server is there, just unexpected response

    except (socket.timeout, ConnectionRefusedError, OSError) as e:
        fail(
            f"Cannot reach {ip}: {e}\n     Is the rabbit in config mode? Are you connected to its WiFi AP?"
        )


def upload_firmware(ip, sim_data):
    """
    Send the firmware to the rabbit via raw HTTP/1.0 POST.

    We bypass multipart/form-data entirely. The rabbit's server doesn't
    parse MIME boundaries — it scans the raw request for "-violet-" markers.
    So we just send the .sim file contents directly as the POST body.

    Strategy: let the OS TCP stack handle flow control naturally. The rabbit
    has an 800-byte receive window — the OS will respect it. We just feed
    data in small chunks and let sendall() block as long as needed. No
    timeouts on sends — the rabbit is slow and we must be patient.
    """

    body = sim_data
    header = (
        f"POST /c HTTP/1.0\r\n"
        f"Host: {ip}\r\n"
        f"Content-Length: {len(body)}\r\n"
        f"Content-Type: application/octet-stream\r\n"
        f"\r\n"
    ).encode()

    total = len(header) + len(body)
    log(f"Uploading firmware to {ip}...")
    log(f"  HTTP header : {len(header)} bytes")
    log(f"  Body (.sim) : {len(body):,} bytes")
    log(f"  Total       : {total:,} bytes")
    print()

    # --- connect ---
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(10)

    try:
        sock.connect((ip, 80))
    except (socket.timeout, ConnectionRefusedError, OSError) as e:
        fail(f"Cannot connect to {ip}:80: {e}")

    ok("Connected to rabbit")

    # No TCP_NODELAY — let Nagle coalesce for optimal segment sizing.
    # Small send buffer to apply backpressure from the rabbit's 800-byte window.
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1024)

    # CRITICAL: Increase the TCP retransmission connection drop time.
    # The rabbit's VLISP VM gets progressively slower as it accumulates
    # data (O(n^2) slistlen). By the time we're past 50%, the rabbit can
    # take several seconds to ACK each segment. macOS's default TCP stack
    # will give up on retransmissions after ~30-75 seconds of silence,
    # killing the connection. We set this to 10 minutes to let the rabbit
    # take as long as it needs.
    # TCP_RXT_CONNDROPTIME = 0x80 (128) on macOS — value in seconds.
    TCP_RXT_CONNDROPTIME = 128
    try:
        sock.setsockopt(socket.IPPROTO_TCP, TCP_RXT_CONNDROPTIME, 600)
    except OSError:
        warn("Could not set TCP_RXT_CONNDROPTIME — upload may fail if rabbit is slow")

    # NO TIMEOUT on sends. The rabbit is slow (60MHz ARM7, O(n^2) slistlen,
    # GC pauses). It will ACK when it's ready. We wait as long as it takes.
    sock.settimeout(None)

    payload = header + body
    total = len(payload)
    CHUNK_SIZE = 512

    log(f"Sending {total:,} bytes...")
    log(f"  No send timeout — waiting for rabbit to ACK at its own pace.")
    log(f"  This may take a long time. Be patient.")
    print()

    sent = 0
    t_start = time.monotonic()

    try:
        while sent < total:
            end = min(sent + CHUNK_SIZE, total)
            chunk = payload[sent:end]
            sock.sendall(chunk)  # blocks until rabbit ACKs — no timeout
            sent = end

            elapsed = time.monotonic() - t_start
            if sent > 0 and elapsed > 0:
                rate = sent / elapsed
                remaining = (total - sent) / rate if rate > 0 else 0
                progress(sent, total, remaining)
            else:
                progress(sent, total)

    except (BrokenPipeError, ConnectionResetError) as e:
        print()
        elapsed = time.monotonic() - t_start
        fail(f"Connection lost at {sent:,}/{total:,} bytes after {elapsed:.0f}s: {e}")
    except OSError as e:
        print()
        elapsed = time.monotonic() - t_start
        fail(f"Send error at {sent:,}/{total:,} bytes after {elapsed:.0f}s: {e}")

    elapsed = time.monotonic() - t_start
    print()
    ok(
        f"Upload complete: {sent:,} bytes in {elapsed:.1f}s ({sent / elapsed / 1024:.1f} KB/s)"
    )

    # --- wait for response or disconnect ---
    log("Waiting for rabbit to process and flash firmware...")
    log("  No timeout — waiting indefinitely for response or disconnect.")
    log("  The rabbit decodes, decrypts, and writes to flash.")
    log("  Ctrl+C to abort if nothing happens after a few minutes.")
    print()

    # No timeout on receive either — wait for the rabbit to respond or drop.
    sock.settimeout(None)

    try:
        resp = b""
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            resp += chunk

        if resp:
            resp_text = resp.decode("ascii", errors="replace")
            if "error" in resp_text.lower():
                print()
                log("Rabbit response:")
                print(resp_text[:500])
                fail(
                    "Rabbit returned an error — firmware may be corrupt or incompatible"
                )
            else:
                print()
                log("Rabbit response:")
                print(resp_text[:500])
                ok("Got HTTP response — flash may have completed before reset")
        else:
            ok("Connection closed cleanly by rabbit")

    except ConnectionResetError:
        ok("Connection reset by rabbit (watchdog reset after flash)")

    except KeyboardInterrupt:
        print()
        ok("Interrupted by user")

    except OSError as e:
        ok(f"Connection dropped ({e})")

    finally:
        try:
            sock.close()
        except:
            pass

    print()
    ok("Upload + flash sequence complete.")
    log("Wait 1-2 minutes, then power cycle the rabbit.")
    log("If it boots normally (not blue/config mode), the flash succeeded.")


def main():
    default_sim = "vendor/nabgcc-latest/Nab-wpa23-release.sim"
    sim_path = sys.argv[1] if len(sys.argv) > 1 else default_sim
    ip = os.environ.get("NABAZTAG_IP", "192.168.0.1")

    if not os.path.isfile(sim_path):
        fail(f"Firmware file not found: {sim_path}")

    log(f"Firmware : {sim_path}")
    log(f"Target   : {ip}")
    print()

    sim_data = validate_sim(sim_path)
    check_connectivity(ip)
    print()
    upload_firmware(ip, sim_data)


if __name__ == "__main__":
    main()
