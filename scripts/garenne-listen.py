#!/usr/bin/env python3
"""Listen for garenne's UDP log broadcasts (deployed from G1 on).

The rabbit broadcasts one-line logs to 255.255.255.255:9999. Run this on
the Mac and watch the rabbit talk:

    python3 scripts/garenne-listen.py
"""

import datetime
import socket

PORT = 9999


def main() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    sock.bind(("0.0.0.0", PORT))
    print(f"listening on UDP :{PORT} (Ctrl-C to quit)")
    while True:
        data, (host, _port) = sock.recvfrom(2048)
        now = datetime.datetime.now().strftime("%H:%M:%S.%f")[:-3]
        print(f"{now} {host} {data.decode(errors='replace').rstrip()}")


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
