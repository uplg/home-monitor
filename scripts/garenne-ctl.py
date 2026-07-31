#!/usr/bin/env python3
"""Drive garenne's UDP control port (deployed from G3 on).

    python3 scripts/garenne-ctl.py ping
    python3 scripts/garenne-ctl.py reboot
    python3 scripts/garenne-ctl.py color 7c5cff
    python3 scripts/garenne-ctl.py log 0
    python3 scripts/garenne-ctl.py --ip 192.168.1.42 ping
"""

import socket
import sys

RABBIT = "192.168.1.155"
PORT = 9998
MAGIC = "grn1 "


def main() -> int:
    args = sys.argv[1:]
    ip = RABBIT
    if len(args) >= 2 and args[0] == "--ip":
        ip = args[1]
        args = args[2:]
    if not args:
        print(__doc__.strip(), file=sys.stderr)
        return 1
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sock.settimeout(2.0)
    sock.sendto((MAGIC + " ".join(args)).encode(), (ip, PORT))
    try:
        data, _peer = sock.recvfrom(2048)
    except TimeoutError:
        print("no reply (rabbit absent, rebooting, or command lost)", file=sys.stderr)
        return 1
    print(data.decode(errors="replace"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
