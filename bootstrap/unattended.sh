#!/bin/sh
# maison Alpine headless bootstrap - unattended sys-disk installation
#
# Based on macmpi/alpine-linux-headless-bootstrap unattended_sysdisk.sh
# Installs Alpine in sys mode (real disk install, not diskless).
#
# After the Pi reboots, finish deployment from your dev machine:
#   PI_HOST=root@<ip> ./deploy.sh push
#   PI_HOST=root@<ip> ./deploy.sh upgrade
#   PI_HOST=root@<ip> ./deploy.sh start

# Redirect all output to log file (the service won't show messages)
exec 1>>/tmp/alhb.log 2>&1

# shellcheck disable=SC2142
alias _logger='logger -st "${0##*/}"'

########################################################
# Configuration
########################################################
MY_HOSTNAME="maison"
MY_IFACE="eth0"
MY_DISK="mmcblk0"
MY_BOOT="${MY_DISK}p1"
MY_ROOT="${MY_DISK}p2"
MY_ROOT_SIZE="$((6*1024))"

APP_DIR="/opt/maison"
SERVICE_USER="maison"
SERVICE_GROUP="maison"

########################################################
# Locate boot media
########################################################
ovl="$( dmesg | grep -o 'Loading user settings from .*:' | awk '{print $5}' | sed 's/:.*$//' )"
if [ -f "${ovl}" ]; then
	ovlpath="$( dirname "$ovl" )"
else
	ovl="$( basename "${ovl}" )"
	ovlpath=$( find /media -maxdepth 2 -type d -path '*/.*' -prune -o -type f -name "${ovl}" -exec dirname {} \; | head -1 )
	ovl="${ovlpath}/${ovl}"
fi

########################################################
# Save SSH keys before setup-alpine wipes the disk
########################################################
_logger "Saving SSH authorized_keys"
if [ -f /root/.ssh/authorized_keys ]; then
	cp /root/.ssh/authorized_keys /tmp/authorized_keys.bak
	_logger "Found authorized_keys, saved to /tmp"
else
	_logger "WARNING: /root/.ssh/authorized_keys not found"
fi
# Also grab from boot media in case headless bootstrap already cleaned up
if [ -f "${ovlpath}/authorized_keys" ]; then
	cp "${ovlpath}/authorized_keys" /tmp/authorized_keys.bak
	_logger "Found authorized_keys on boot media, saved to /tmp"
fi

########################################################
# Install Alpine to SD card (sys mode)
########################################################
_logger "Starting sys-disk installation"
cat <<-EOF > /tmp/ANSWERFILE
	KEYMAPOPTS=none
	HOSTNAMEOPTS="$MY_HOSTNAME"
	DEVDOPTS=mdev
	INTERFACESOPTS="auto lo
	iface lo inet loopback

	auto $MY_IFACE
	iface $MY_IFACE inet dhcp
	"
	DNSOPTS=""
	TIMEZONEOPTS="Europe/Paris"
	PROXYOPTS=none
	APKREPOSOPTS="-1 -c"
	USEROPTS=none
	SSHDOPTS=openssh
	NTPOPTS=chrony

	export ERASE_DISKS=/dev/$MY_DISK
	export ROOT_SIZE=$MY_ROOT_SIZE
	DISKOPTS="-m sys /dev/$MY_DISK"
	EOF

SSH_CONNECTION="FAKE" setup-alpine -ef /tmp/ANSWERFILE

########################################################
# Prepare post-install script for the new system
########################################################
_logger "Preparing post-install script"
cat <<-SETUP > /tmp/sys-setup.sh
	#!/bin/sh
	set -x

	# Disable root password login (SSH key auth only)
	# Use '*' not '!' — BusyBox passwd -l sets '!' which makes OpenSSH
	# treat the account as locked and reject even pubkey auth.
	# '*' means "no password set" without locking the account.
	sed -i 's|^root:[^:]*:|root:*:|' /etc/shadow

	# Enable community repo
	if [ -f /etc/apk/repositories ]; then
		sed -i 's|^#\(.*/community\)|\1|' /etc/apk/repositories
	fi

	# Install packages
	apk update
	apk upgrade --available
	apk add --no-cache \
		bash \
		ca-certificates \
		curl \
		git \
		mosquitto \
		rsync \
		openssh \
		chrony \
		tzdata

	# Timezone
	if [ -f /usr/share/zoneinfo/Europe/Paris ]; then
		cp /usr/share/zoneinfo/Europe/Paris /etc/localtime
		echo "Europe/Paris" > /etc/timezone
	fi

	# Service user and group
	if ! getent group "${SERVICE_GROUP}" >/dev/null 2>&1; then
		addgroup -S "${SERVICE_GROUP}"
	fi
	if ! id -u "${SERVICE_USER}" >/dev/null 2>&1; then
		adduser -S -D -H -h "${APP_DIR}" -G "${SERVICE_GROUP}" -s /sbin/nologin "${SERVICE_USER}"
	fi
	# Serial port access (Zigbee USB adapter on /dev/ttyUSB0)
	addgroup "${SERVICE_USER}" dialout

	# Directory tree
	mkdir -p \
		"${APP_DIR}/backend/target/release" \
		"${APP_DIR}/frontend/dist" \
		"${APP_DIR}/deploy/systemd" \
		"${APP_DIR}/deploy/openrc" \
		"${APP_DIR}/deploy/mosquitto" \
		"${APP_DIR}/mosquitto/certs" \
		"${APP_DIR}/cache/tempo" \
		"${APP_DIR}/logs"

	chown -R root:root "${APP_DIR}"
	chown -R "${SERVICE_USER}:${SERVICE_GROUP}" "${APP_DIR}/cache"

	# Placeholder runtime JSON files
	for json_file in \
		"${APP_DIR}/device-cache.json" \
		"${APP_DIR}/hue-lamps.json" \
		"${APP_DIR}/hue-lamps-blacklist.json" \
		"${APP_DIR}/zigbee-lamps.json" \
		"${APP_DIR}/zigbee-lamps-blacklist.json"
	do
		if [ ! -e "\${json_file}" ]; then
			printf '%s\n' '[]' > "\${json_file}"
			chown "${SERVICE_USER}:${SERVICE_GROUP}" "\${json_file}"
		fi
	done

	if [ ! -e "${APP_DIR}/broadlink-codes.json" ]; then
		printf '%s\n' '{"codes":[]}' > "${APP_DIR}/broadlink-codes.json"
		chown "${SERVICE_USER}:${SERVICE_GROUP}" "${APP_DIR}/broadlink-codes.json"
	fi

	# Mosquitto config
	mkdir -p /etc/mosquitto/conf.d /etc/mosquitto/certs/maison /var/log/mosquitto
	chown -R mosquitto:mosquitto /var/log/mosquitto 2>/dev/null || true

	cat > /etc/mosquitto/conf.d/maison.conf <<'MQEOF'
# Installed by maison unattended bootstrap
listener 1883 0.0.0.0
allow_anonymous true
log_dest file /var/log/mosquitto/mosquitto.log
log_type warning
log_type error
MQEOF

	# Log files
	touch /var/log/maison.log /var/log/cloudflared-maison.log
	chown "${SERVICE_USER}:${SERVICE_GROUP}" /var/log/maison.log /var/log/cloudflared-maison.log
	chmod 644 /var/log/maison.log /var/log/cloudflared-maison.log

	# Enable services
	rc-update add mosquitto default 2>/dev/null || true
	rc-update add sshd default 2>/dev/null || true
	rc-update add chronyd default 2>/dev/null || true

	# Ensure sshd allows root login and key auth
	sed -i 's/^#*PermitRootLogin.*/PermitRootLogin yes/' /etc/ssh/sshd_config
	sed -i 's/^#*PubkeyAuthentication.*/PubkeyAuthentication yes/' /etc/ssh/sshd_config

	# motd
	cat > /etc/motd <<'MOTD'

       /\\_/\\
      ( o.o )    maison
       > ^ <

      Alpine Linux on Raspberry Pi 1
      --------------------------------
      app     /opt/maison
      logs    /var/log/maison.log
      mqtt    localhost:1883 / :8883 (tls, meross)
      tunnel  /var/log/cloudflared-maison.log

MOTD
SETUP
chmod +x /tmp/sys-setup.sh

########################################################
# Mount new system and run post-install
########################################################
_logger "Mounting new system for post-installation"
mkdir -p /mnt/boot /mnt/tmp /mnt/dev /mnt/proc /mnt/sys
mount /dev/$MY_ROOT /mnt
mount /dev/$MY_BOOT /mnt/boot
mount --bind /tmp /mnt/tmp
mount --bind /dev /mnt/dev
mount --bind /proc /mnt/proc
mount --bind /sys /mnt/sys

_logger "Running post-install script on disk-based system"
chroot /mnt /tmp/sys-setup.sh
sync

# Copy SSH authorized_keys from backup to installed system
_logger "Copying SSH keys to installed system"
if [ -f /tmp/authorized_keys.bak ]; then
	mkdir -p /mnt/root/.ssh
	cp /tmp/authorized_keys.bak /mnt/root/.ssh/authorized_keys
	chmod 700 /mnt/root/.ssh
	chmod 600 /mnt/root/.ssh/authorized_keys
	_logger "SSH keys installed"
else
	_logger "WARNING: no authorized_keys backup found, SSH key auth will not work"
fi

_logger "Cleaning up mounts"
umount /mnt/sys
umount /mnt/proc
umount /mnt/dev
umount /mnt/tmp
umount /mnt/boot
umount /mnt

_logger "maison bootstrap complete. Rebooting into installed system."
reboot
