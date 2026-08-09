#!/usr/bin/env bash
# BZOD Production Deployment Script
# curl -fsSL https://bzo.in/deploy.sh | sudo bash

set -euo pipefail

BZOD_VERSION="0.6.0"

SERVICE_USER="bzod"
INSTALL_PATH="/usr/local/bin/bzod"
CONFIG_DIR="/etc/bzod"
DATA_DIR="/var/lib/bzod/data"
ENV_FILE="${CONFIG_DIR}/bzod.env"
SYSTEMD_UNIT="/etc/systemd/system/bzod.service"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

# Temporary file cleanup
TMP_BINARY=""
cleanup() {
    rm -f "${TMP_BINARY:-}" "${TMP_GHCR:-}"
}
trap cleanup EXIT

echo -e "${BLUE}=== BZOD - Privacy-First URL Shortener & Landing Page Platform ===${NC}"
echo -e "Production deployment started...\n"

if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root (use sudo).${NC}"
    exit 1
fi

# 1. Install Base Dependencies
echo -e "${BLUE}[1/8] Installing base system dependencies...${NC}"
apt-get update -qq
apt-get install -y openssl sqlite3 ca-certificates curl tar gzip

# 2. Install Binary (safe atomic download)
echo -e "\n${BLUE}[2/8] Installing BZOD binary...${NC}"

ARCH="$(uname -m)"
case $ARCH in
    x86_64)  BINARY_NAME="bzod-x86_64-unknown-linux-gnu" ;;
    aarch64|arm64) BINARY_NAME="bzod-aarch64-unknown-linux-gnu" ;;
    armv7l)  BINARY_NAME="bzod-armv7-unknown-linux-gnueabihf" ;;
    *) echo -e "${RED}Unsupported architecture: $ARCH${NC}"; exit 1 ;;
esac

REPO="thakares/nx9-url-shortener"
RELEASE_URL="https://github.com/${REPO}/releases/download/v${BZOD_VERSION}/${BINARY_NAME}"

TMP_BINARY=$(mktemp)

echo "Trying GitHub Releases..."
if curl --retry 5 --retry-delay 2 --retry-connrefused \
    -L -f -o "${TMP_BINARY}" "${RELEASE_URL}" 2>/dev/null; then
    echo -e "${GREEN}✓ Downloaded from GitHub Releases${NC}"
else
    echo -e "${BLUE}GitHub Releases not available. Trying GHCR...${NC}"
    if command -v docker >/dev/null 2>&1; then
        TMP_GHCR=$(mktemp)
        docker pull ghcr.io/${REPO}:latest >/dev/null 2>&1 || true
        if docker run --rm --entrypoint cat ghcr.io/${REPO}:latest /usr/local/bin/bzod > "${TMP_GHCR}" 2>/dev/null && [ -s "${TMP_GHCR}" ]; then
            mv "${TMP_GHCR}" "${TMP_BINARY}"
            echo -e "${GREEN}✓ Extracted from GHCR${NC}"
        fi
    fi

    if [ ! -s "${TMP_BINARY}" ]; then
        echo -e "${BLUE}Falling back to local build...${NC}"
        if ! command -v cargo >/dev/null 2>&1; then
            echo -e "${RED}Neither pre-built binary nor cargo available.${NC}"
            exit 1
        fi
        apt-get install -y pkg-config build-essential
        cargo build --release
        cp target/release/bzod "${TMP_BINARY}"
        echo -e "${GREEN}✓ Built from source${NC}"
    fi
fi

# Atomic replace with backup
if [ -f "${INSTALL_PATH}" ]; then
    cp "${INSTALL_PATH}" "${INSTALL_PATH}.bak" 2>/dev/null || true
fi

install -m 755 "${TMP_BINARY}" "${INSTALL_PATH}"

# Verify
if [ ! -x "${INSTALL_PATH}" ]; then
    echo -e "${RED}Binary installation failed${NC}"
    exit 1
fi

"${INSTALL_PATH}" --version >/dev/null && echo -e "${GREEN}✓ Binary verified (--version)${NC}" || {
    echo -e "${RED}Binary verification failed${NC}"
    exit 1
}

# Verify -V also works
"${INSTALL_PATH}" -V >/dev/null && echo -e "${GREEN}✓ Binary verified (-V)${NC}" || {
    echo -e "${RED}Binary -V verification failed${NC}"
    exit 1
}

# Show installed version and verify it matches requested version
VERSION=$("${INSTALL_PATH}" --version 2>/dev/null | head -n1 || echo "unknown")
EXPECTED_VERSION="bzod ${BZOD_VERSION}"
if [ "${VERSION}" != "${EXPECTED_VERSION}" ]; then
    echo -e "${RED}Version mismatch: expected '${EXPECTED_VERSION}', got '${VERSION}'${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Installed ${VERSION} (${ARCH})${NC}"

# 3. Create System User
echo -e "\n${BLUE}[3/8] Creating system user '${SERVICE_USER}'...${NC}"
if ! id -u "${SERVICE_USER}" &>/dev/null; then
    useradd -r -s /usr/sbin/nologin -m -d /var/lib/bzod "${SERVICE_USER}"
fi

# 4. Setup Directories
echo -e "\n${BLUE}[4/8] Setting up directories...${NC}"
mkdir -p "${CONFIG_DIR}" "${DATA_DIR}"
chown -R "${SERVICE_USER}:${SERVICE_USER}" "/var/lib/bzod"
chmod 700 "${CONFIG_DIR}"

# 5. Configuration (preserve on upgrades)
echo -e "\n${BLUE}[5/8] Configuration...${NC}"
if [ ! -f "${ENV_FILE}" ]; then
    echo -e "${BLUE}Generating new secure configuration...${NC}"
    cat <<EOF > "${ENV_FILE}"
HOST=0.0.0.0
PORT=8654
DATA_DIR=${DATA_DIR}
COOKIE_SECURE=true
RUST_LOG=info
SESSION_SECRET=$(openssl rand -hex 32)
EOF
    chmod 600 "${ENV_FILE}"
    chown root:"${SERVICE_USER}" "${ENV_FILE}"
else
    echo -e "${GREEN}Existing configuration preserved${NC}"
fi

# 6. Systemd Service
echo -e "\n${BLUE}[6/8] Installing hardened systemd service...${NC}"
cat <<EOF > "${SYSTEMD_UNIT}"
[Unit]
Description=BZOD - Privacy-First URL Shortener & Landing Page Platform
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
WorkingDirectory=/var/lib/bzod
EnvironmentFile=${ENV_FILE}
ExecStart=${INSTALL_PATH} serve

Restart=on-failure
RestartSec=5s

# Security Hardening
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ProtectHostname=yes
RestrictSUIDSGID=yes
LockPersonality=yes
NoNewPrivileges=yes
ReadWritePaths=/var/lib/bzod

[Install]
WantedBy=multi-user.target
EOF

chmod 644 "${SYSTEMD_UNIT}"
systemctl daemon-reload

# 7. Initialize & Start
echo -e "\n${BLUE}[7/8] Initializing and starting service...${NC}"

# Database creation and migration is handled automatically by 'bzod serve'
if [ -f "${DATA_DIR}/admin/admin.db" ] || [ -f "${DATA_DIR}/admin.db" ]; then
    echo -e "${GREEN}✓ Existing database detected (upgrade mode)${NC}"

    # Pre-upgrade: stop service and backup databases
    if systemctl is-active --quiet bzod 2>/dev/null; then
        echo -e "${BLUE}  Stopping BZOD for safe database backup...${NC}"
        systemctl stop bzod
    fi

    BACKUP_DIR="/var/lib/bzod/pre-upgrade-backup-v${BZOD_VERSION}"
    mkdir -p "${BACKUP_DIR}"
    cp -a "${DATA_DIR}" "${BACKUP_DIR}/data" 2>/dev/null || true
    cp "${ENV_FILE}" "${BACKUP_DIR}/bzod.env" 2>/dev/null || true
    echo -e "${GREEN}  ✓ Pre-upgrade backup created at ${BACKUP_DIR}${NC}"
else
    echo -e "${GREEN}✓ Fresh installation (databases will be created on first start)${NC}"
fi

systemctl enable --now bzod

# 8. Validation + Rollback
sleep 3

if ! systemctl is-active --quiet bzod; then
    echo -e "${RED}Service failed to start! Rolling back...${NC}"
    if [ -f "${INSTALL_PATH}.bak" ]; then
        install -m 755 "${INSTALL_PATH}.bak" "${INSTALL_PATH}"
        systemctl restart bzod || true
    fi
    journalctl -u bzod -n 50 --no-pager
    exit 1
fi

# Clean up backup on success
rm -f "${INSTALL_PATH}.bak" 2>/dev/null || true

# Soft health check
if command -v curl >/dev/null 2>&1; then
    if curl -fsS http://127.0.0.1:8654/status >/dev/null 2>&1; then
        echo -e "${GREEN}✓ HTTP health check passed${NC}"
    else
        echo -e "${BLUE}✓ Service is running (systemd healthy)${NC}"
    fi
fi

# Final Message
IP=$(hostname -I | awk '{print $1}' | head -n1)
echo -e "\n${GREEN}=== BZOD Deployed Successfully! ===${NC}"
echo -e "🌐 Web UI:     http://${IP}:8654"
echo -e "🔑 Admin:      http://${IP}:8654/admin"
echo -e "🖥 Architecture: ${ARCH}"
echo -e "📦 Version: ${VERSION}"
echo -e "\nNext step (first install):"
echo -e "   sudo -u bzod bzod create-admin"
echo -e "\nCommands:"
echo -e "   journalctl -u bzod -f"
echo -e "   bzod doctor"
echo -e "   systemctl status bzod"

echo -e "\n${GREEN}Enjoy your lightweight, privacy-first, self-hosted URL shortener!${NC}"