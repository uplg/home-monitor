.PHONY: help backend frontend frontend-build test build-pi deploy deploy-push deploy-start deploy-stop deploy-status deploy-logs cloudflared-upgrade

# SSH target for the Raspberry Pi. Override on the command line or set
# PI_HOST in .env (deploy.sh and every deploy-* target read it from here).
PI_HOST ?= $(shell grep -E '^PI_HOST=' .env 2>/dev/null | tail -n1 | cut -d'=' -f2)
export PI_HOST

LOG_TARGET ?= stack

help:
	@printf "Development (this machine):\n"
	@printf "  make backend             Run the Rust backend in foreground (cargo run)\n"
	@printf "  make frontend            Run the frontend dev server (vite)\n"
	@printf "  make frontend-build      Build the frontend bundle\n"
	@printf "  make test                Run backend tests and frontend lint\n"
	@printf "\nRaspberry Pi 1 (uses PI_HOST, default from .env):\n"
	@printf "  make build-pi            Cross-build the ARMv6 musl backend binary\n"
	@printf "  make deploy              Full deploy: build + push + upgrade + restart\n"
	@printf "  make deploy-push         Push already-built artifacts and configs\n"
	@printf "  make deploy-start        (Re)install services and restart the stack\n"
	@printf "  make deploy-stop         Stop the stack on the Pi\n"
	@printf "  make deploy-status       Show service status and URLs\n"
	@printf "  make deploy-logs         Follow logs (LOG_TARGET=stack|backend|mosquitto|cloudflared)\n"
	@printf "  make cloudflared-upgrade Build latest cloudflared (ARMv6) and swap it on the Pi\n"

backend:
	cargo run --manifest-path backend/Cargo.toml

frontend:
	bun --cwd frontend run dev

frontend-build:
	bun --cwd frontend run build

test:
	cargo test --manifest-path backend/Cargo.toml
	bun --cwd frontend run lint

build-pi:
	bash scripts/build-rpi1-backend.sh

deploy:
	./deploy.sh all

deploy-push:
	./deploy.sh push

deploy-start:
	./deploy.sh start

deploy-stop:
	./deploy.sh stop

deploy-status:
	./deploy.sh status

deploy-logs:
	./deploy.sh logs $(LOG_TARGET)

# Build the latest cloudflared from ../cloudflared (pulls upstream first) and
# replace it on the Pi with a clean swap: stop service, keep a .bak of the
# previous binary, atomically move the new one in place, restart, verify.
cloudflared-upgrade:
	bash scripts/build-cloudflared-armv6.sh
	@test -n "$(PI_HOST)" || { printf 'Set PI_HOST (in .env or the environment)\n' >&2; exit 1; }
	rsync -avz cloudflared-arm "$(PI_HOST):/usr/local/bin/cloudflared.new"
	ssh "$(PI_HOST)" 'set -e; \
		chmod +x /usr/local/bin/cloudflared.new; \
		rc-service cloudflared-maison stop 2>/dev/null || true; \
		[ -f /usr/local/bin/cloudflared ] && cp /usr/local/bin/cloudflared /usr/local/bin/cloudflared.bak || true; \
		mv /usr/local/bin/cloudflared.new /usr/local/bin/cloudflared; \
		rc-service cloudflared-maison start; \
		sleep 3; \
		cloudflared --version; \
		rc-service cloudflared-maison status'
