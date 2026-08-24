#!/usr/bin/env python3
"""Provision a factory-mode Meross plug (MSS310-style) for maison — no cloud.

The historical provisioning was done with an arandall/meross-style client that
never made it into this repo; this script recreates it, stdlib only.

A plug blinking green/red broadcasts an open AP "Meross_XXXX" with itself at
10.10.10.1. Join that AP with this machine, then:

    # 1. (optional) look at what the plug sees
    ./scripts/meross-provision.py scan

    # 2. read uuid/mac (also a good connectivity check)
    ./scripts/meross-provision.py info

    # 3. point it at the local broker + set the signing key, then push WiFi
    ./scripts/meross-provision.py provision \
        --mqtt-host 192.168.1.103 \
        --key blabliblou \
        --ssid "MonSSID" --password "MonMotDePasse"

The plug reboots, joins the WiFi, connects to Mosquitto on the Pi (:8883,
TLS, cert NOT validated by the plug) and stops blinking. Then find its DHCP
IP, append {name, ip, key} to meross-devices.json and restart the backend.

Factory-mode messages are signed with the EMPTY key; after provisioning the
plug expects the key you set here (use the same one as the existing plugs,
see meross-devices.json).
"""

import argparse
import base64
import hashlib
import json
import sys
import time
import urllib.request
import uuid


def build_packet(method: str, namespace: str, payload: dict, key: str, host: str) -> dict:
    message_id = uuid.uuid4().hex
    timestamp = int(time.time())
    sign = hashlib.md5(f"{message_id}{key}{timestamp}".encode()).hexdigest()
    return {
        "header": {
            "from": f"http://{host}/config",
            "messageId": message_id,
            "method": method,
            "namespace": namespace,
            "payloadVersion": 1,
            "sign": sign,
            "timestamp": timestamp,
        },
        "payload": payload,
    }


def send(host: str, method: str, namespace: str, payload: dict, key: str = "", timeout: int = 10) -> dict:
    packet = build_packet(method, namespace, payload, key, host)
    request = urllib.request.Request(
        f"http://{host}/config",
        data=json.dumps(packet).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        answer = json.loads(response.read())
    if answer.get("header", {}).get("method") == "ERROR":
        raise SystemExit(f"plug returned an error for {namespace}: {json.dumps(answer['payload'])}")
    return answer


def cmd_info(args: argparse.Namespace) -> None:
    answer = send(args.host, "GET", "Appliance.System.All", {})
    system = answer["payload"]["all"]["system"]
    hardware, firmware = system["hardware"], system["firmware"]
    print(f"type:     {hardware.get('type')} {hardware.get('chipType', '')}")
    print(f"uuid:     {hardware.get('uuid')}")
    print(f"mac:      {hardware.get('macAddress')}")
    print(f"firmware: {firmware.get('version')}")
    print(f"server:   {firmware.get('server')}:{firmware.get('port')}")


def cmd_scan(args: argparse.Namespace) -> None:
    answer = send(args.host, "GET", "Appliance.Config.WifiList", {}, timeout=30)
    for network in answer["payload"].get("wifiList", []):
        ssid = base64.b64decode(network.get("ssid", "")).decode(errors="replace")
        print(
            f"{ssid:32s} bssid={network.get('bssid')} channel={network.get('channel')} "
            f"signal={network.get('signal')} encryption={network.get('encryption')} "
            f"cipher={network.get('cipher')}"
        )


def cmd_provision(args: argparse.Namespace) -> None:
    print("[*] Reading device identity...")
    cmd_info(args)

    print(f"[*] Setting key + MQTT broker {args.mqtt_host}:{args.mqtt_port} ...")
    send(
        args.host,
        "SET",
        "Appliance.Config.Key",
        {
            "key": {
                "gateway": {
                    "host": args.mqtt_host,
                    "port": args.mqtt_port,
                    "secondHost": args.mqtt_host,
                    "secondPort": args.mqtt_port,
                },
                "key": args.key,
                "userId": "",
            }
        },
    )

    wifi = {
        "ssid": base64.b64encode(args.ssid.encode()).decode(),
        "password": base64.b64encode(args.password.encode()).decode(),
    }
    # Newer firmwares want the BSS parameters echoed back from the scan.
    if args.bssid:
        wifi.update(
            {
                "bssid": args.bssid,
                "channel": args.channel,
                "encryption": args.encryption,
                "cipher": args.cipher,
            }
        )
    print(f"[*] Pushing WiFi config for SSID {args.ssid!r} ...")
    send(args.host, "SET", "Appliance.Config.Wifi", {"wifi": wifi})

    print()
    print("Done. The plug reboots and joins your WiFi (LED goes solid).")
    print("Next: find its DHCP IP on the router, then add to meross-devices.json:")
    print(json.dumps({"name": "Nouvelle prise", "ip": "<DHCP-IP>", "key": args.key}, indent=2))
    print("and restart the maison backend.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", default="10.10.10.1", help="plug address in AP mode (default 10.10.10.1)")
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("info", help="read device identity (Appliance.System.All)")
    sub.add_parser("scan", help="list WiFi networks the plug sees")

    provision = sub.add_parser("provision", help="set key + broker, then push WiFi credentials")
    provision.add_argument("--mqtt-host", required=True, help="Mosquitto host the plug will connect to (the Pi)")
    provision.add_argument("--mqtt-port", type=int, default=8883)
    provision.add_argument("--key", required=True, help="signing key (same as meross-devices.json entries)")
    provision.add_argument("--ssid", required=True)
    provision.add_argument("--password", required=True)
    provision.add_argument("--bssid", help="from `scan`; only needed if the plug ignores ssid+password alone")
    provision.add_argument("--channel", type=int, default=0)
    provision.add_argument("--encryption", type=int, default=0)
    provision.add_argument("--cipher", type=int, default=0)

    args = parser.parse_args()
    {"info": cmd_info, "scan": cmd_scan, "provision": cmd_provision}[args.command](args)


if __name__ == "__main__":
    try:
        main()
    except (urllib.error.URLError, TimeoutError) as error:
        print(f"!! cannot reach the plug: {error}", file=sys.stderr)
        print("   Are you connected to its Meross_XXXX access point?", file=sys.stderr)
        sys.exit(1)
