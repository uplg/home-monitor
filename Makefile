.PHONY: help backend backend-release backend-build-release backend-build-pi backend-build-pi-cross backend-start backend-start-release backend-stop frontend frontend-build compose-up compose-down tunnel-up tunnel-down start stop status

LOG_DIR := logs
BACKEND_LOG := $(LOG_DIR)/backend.log
BACKEND_PID := .backend.pid
BACKEND_RELEASE_BIN := backend/target/release/cat-monitor-rust-backend
BACKEND_BUILD_FLAGS ?=
API_PORT ?= $(shell grep -E '^(API_PORT|PORT)=' .env 2>/dev/null | tail -n1 | cut -d'=' -f2 || echo 3033)
PUBLIC_HOSTNAME ?= $(shell grep -E '^CLOUDFLARE_PUBLIC_HOSTNAME=' .env 2>/dev/null | cut -d'=' -f2)

help:
	@printf "Targets:\n"
	@printf "  make backend         Run the Rust backend on the host in foreground\n"
	@printf "  make backend-release Run the Rust release binary in foreground\n"
	@printf "  make backend-build-release Build the Rust release binary\n"
	@printf "  make backend-build-pi Build the Pi-oriented release binary (no BLE)\n"
	@printf "  make backend-build-pi-cross Cross-build the Alpine Pi binary with zigbuild\n"
	@printf "  make backend-start   Start the Rust backend on the host in background\n"
	@printf "  make backend-start-release Start the Rust release binary in background\n"
	@printf "  make backend-stop    Stop the host backend\n"
	@printf "  make frontend        Run the frontend dev server\n"
	@printf "  make frontend-build  Build the frontend\n"
	@printf "  make compose-up      Start frontend + mosquitto with Docker\n"
	@printf "  make compose-down    Stop Docker services\n"
	@printf "  make tunnel-up       Start the Cloudflare tunnel\n"
	@printf "  make tunnel-down     Stop the Cloudflare tunnel\n"
	@printf "  make start          Start backend + frontend + mosquitto + tunnel\n"
	@printf "  make stop           Stop backend + frontend + mosquitto + tunnel\n"
	@printf "  make status         Show current service status\n"

backend:
	cargo run --manifest-path backend/Cargo.toml

backend-release:
	@if [ ! -x $(BACKEND_RELEASE_BIN) ]; then \
		printf "Release binary not found: %s\n" "$(BACKEND_RELEASE_BIN)"; \
		printf '%s\n' 'Build it first with make backend-build-release'; \
		exit 1; \
	fi
	$(BACKEND_RELEASE_BIN)

backend-build-release:
	cargo build --release --manifest-path backend/Cargo.toml $(BACKEND_BUILD_FLAGS)

backend-build-pi:
	cargo build --release --manifest-path backend/Cargo.toml --no-default-features

backend-build-pi-cross:
	bash scripts/build-rpi1-backend.sh

backend-start:
	@mkdir -p $(LOG_DIR)
	@if [ -f $(BACKEND_PID) ] && kill -0 $$(cat $(BACKEND_PID)) 2>/dev/null; then \
		printf "Backend already running (PID %s)\n" "$$(cat $(BACKEND_PID))"; \
	else \
		nohup cargo run --manifest-path backend/Cargo.toml > $(BACKEND_LOG) 2>&1 & echo $$! > $(BACKEND_PID); \
		printf "Backend started on host (PID %s)\n" "$$(cat $(BACKEND_PID))"; \
		printf "Backend log: %s\n" "$(BACKEND_LOG)"; \
	fi

backend-start-release:
	@mkdir -p $(LOG_DIR)
	@if [ ! -x $(BACKEND_RELEASE_BIN) ]; then \
		printf "Release binary not found: %s\n" "$(BACKEND_RELEASE_BIN)"; \
		printf '%s\n' 'Build it first with make backend-build-release'; \
		exit 1; \
	fi
	@if [ -f $(BACKEND_PID) ] && kill -0 $$(cat $(BACKEND_PID)) 2>/dev/null; then \
		printf "Backend already running (PID %s)\n" "$$(cat $(BACKEND_PID))"; \
	else \
		nohup $(BACKEND_RELEASE_BIN) > $(BACKEND_LOG) 2>&1 & echo $$! > $(BACKEND_PID); \
		printf "Backend release started on host (PID %s)\n" "$$(cat $(BACKEND_PID))"; \
		printf "Backend log: %s\n" "$(BACKEND_LOG)"; \
	fi

backend-stop:
	@if [ -f $(BACKEND_PID) ] && kill -0 $$(cat $(BACKEND_PID)) 2>/dev/null; then \
		kill $$(cat $(BACKEND_PID)) 2>/dev/null || true; \
		printf "Stopped backend PID %s\n" "$$(cat $(BACKEND_PID))"; \
	else \
		printf "Backend is not running\n"; \
	fi
	@rm -f $(BACKEND_PID)
	@pkill -f "cargo run --manifest-path backend/Cargo.toml" 2>/dev/null || true
	@pkill -f "$(BACKEND_RELEASE_BIN)" 2>/dev/null || true

frontend:
	bun --cwd frontend run dev

frontend-build:
	bun --cwd frontend run build

compose-up:
	docker compose up -d --build frontend mqtt

compose-down:
	docker compose down

tunnel-up:
	@if [ -z "$${CLOUDFLARE_TUNNEL_TOKEN:-}" ] && ! grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' .env 2>/dev/null; then \
		printf "Set CLOUDFLARE_TUNNEL_TOKEN in .env before starting the tunnel\n"; \
		exit 1; \
	fi
	@docker compose --profile tunnel up -d cloudflared || { \
		printf '%s\n' 'Cloudflare tunnel failed to start. Check that CLOUDFLARE_TUNNEL_TOKEN is the tunnel connector token from Cloudflare Zero Trust.'; \
		exit 1; \
	}

tunnel-down:
	docker compose --profile tunnel stop cloudflared

start: backend-start
	@docker compose up -d --build frontend mqtt
	@if [ -n "$${CLOUDFLARE_TUNNEL_TOKEN:-}" ] || grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' .env 2>/dev/null; then \
		docker compose --profile tunnel up -d cloudflared || printf '%s\n' 'Cloudflare tunnel failed to start. The token is likely invalid; keep local services running and replace CLOUDFLARE_TUNNEL_TOKEN with the connector token from your named tunnel.'; \
	else \
		printf '%s\n' 'Cloudflare tunnel skipped: set CLOUDFLARE_TUNNEL_TOKEN in .env to enable it'; \
	fi
	@printf '\nStarted services:\n'
	@printf '%s\n' "- Backend: http://localhost:$(API_PORT)"
	@printf '%s\n' '- Frontend: http://localhost'
	@printf '%s\n' '- MQTT: localhost:1883'
	@printf '%s\n' '- MQTT: localhost:8883'
	@if [ -n "$(PUBLIC_HOSTNAME)" ]; then \
		printf '%s\n' "- Public URL: https://$(PUBLIC_HOSTNAME)"; \
	else \
		printf '%s\n' '- Public URL: configure a Cloudflare public hostname and set CLOUDFLARE_PUBLIC_HOSTNAME in .env'; \
	fi

stop: backend-stop
	@docker compose --profile tunnel down --remove-orphans

status:
	@printf "Backend: "
	@if [ -f $(BACKEND_PID) ] && kill -0 $$(cat $(BACKEND_PID)) 2>/dev/null; then \
		printf "running (PID %s)\n" "$$(cat $(BACKEND_PID))"; \
	else \
		printf "stopped\n"; \
	fi
	@if [ -n "$${CLOUDFLARE_TUNNEL_TOKEN:-}" ] || grep -q '^CLOUDFLARE_TUNNEL_TOKEN=.' .env 2>/dev/null; then \
		docker compose ps; \
	else \
		docker compose ps frontend mqtt; \
		printf "Cloudflare tunnel: not configured\n"; \
	fi
