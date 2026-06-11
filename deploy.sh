#!/usr/bin/env bash

# BZOD Deployment Script (Debian Native Deployment)
# This script sets up a secure, production-ready systemd service for BZOD.

set -euo pipefail

# Configurations
SERVICE_USER="bzod"
INSTALL_PATH="/usr/local/bin/bzod"
CONFIG_DIR="/etc/bzod"
DATA_DIR="/var/lib/bzod/data"
ENV_FILE="${CONFIG_DIR}/bzod.env"
SYSTEMD_UNIT="/etc/systemd/system/bzod.service"

# Color outputs
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== BZOD Debian Deployment Script ===${NC}"

# 1. Check Root Privileges
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root (or via sudo).${NC}"
    exit 1
fi

# 2. Install Package Dependencies
echo -e "\n${BLUE}[1/8] Installing system dependencies (SQLite, OpenSSL, Tar)...${NC}"
apt-get update
apt-get install -y openssl sqlite3 ca-certificates curl tar gzip

# 3. Compile Production Build Locally
echo -e "\n${BLUE}[2/8] Compiling release binary...${NC}"
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: cargo not found. Please install Rust or copy a compiled 'bzod' binary to the current directory.${NC}"
    exit 1
fi

cargo build --release
echo -e "${GREEN}Release build completed.${NC}"

# 4. Install Binary
echo -e "\n${BLUE}[3/8] Installing binary to ${INSTALL_PATH}...${NC}"
cp target/release/bzod "${INSTALL_PATH}"
chmod 755 "${INSTALL_PATH}"
chown root:root "${INSTALL_PATH}"
echo -e "${GREEN}Binary installed successfully.${NC}"

# 5. Create Dedicated locked-down System User
echo -e "\n${BLUE}[4/8] Creating dedicated system user '${SERVICE_USER}'...${NC}"
if ! id -u "${SERVICE_USER}" &>/dev/null; then
    useradd -r -s /usr/sbin/nologin -m -d /var/lib/bzod "${SERVICE_USER}"
    echo -e "${GREEN}System user '${SERVICE_USER}' created.${NC}"
else
    echo "User '${SERVICE_USER}' already exists."
fi

# 6. Configure Directory Trees and Permissions
echo -e "\n${BLUE}[5/8] Setting up configuration and data directories...${NC}"
mkdir -p "${CONFIG_DIR}"
mkdir -p "${DATA_DIR}"

# Copy .env file if it exists, otherwise prompt/generate
if [ -f .env ] && [ ! -f "${ENV_FILE}" ]; then
    echo "Copying local .env file to ${ENV_FILE}..."
    cp .env "${ENV_FILE}"
elif [ ! -f "${ENV_FILE}" ]; then
    echo "Generating default configuration file at ${ENV_FILE}..."
    cat <<EOF > "${ENV_FILE}"
HOST=0.0.0.0
PORT=8080
DATA_DIR=${DATA_DIR}
COOKIE_SECURE=true
SESSION_SECRET=$(openssl rand -hex 32)
ADMIN_USERNAME=admin
# SHA-256 for bootstrap (Default: admin)
ADMIN_PASSWORD_SHA256=8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918
LINK_CHECK_INTERVAL_MINS=60
AGGREGATION_INTERVAL_MINS=60
DATA_RETENTION_DAYS=365
EOF
fi

chmod 600 "${ENV_FILE}"
chown -R root:"${SERVICE_USER}" "${CONFIG_DIR}"
chown -R "${SERVICE_USER}":"${SERVICE_USER}" /var/lib/bzod
echo -e "${GREEN}Directories and permission parameters configured.${NC}"

# 7. Initialise DB as the service user (avoids file permission conflicts)
echo -e "\n${BLUE}[6/8] Initialising databases...${NC}"
sudo -u "${SERVICE_USER}" "${INSTALL_PATH}" init-db --data-dir "${DATA_DIR}"
echo -e "${GREEN}Databases initialised.${NC}"

# 8. Set Up Systemd Service
echo -e "\n${BLUE}[7/8] Installing systemd service unit...${NC}"
cat <<EOF > "${SYSTEMD_UNIT}"
[Unit]
Description=BZOD - Personal URL Shortener & Landing Page Platform
After=network.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
WorkingDirectory=/var/lib/bzod
EnvironmentFile=${ENV_FILE}
ExecStart=${INSTALL_PATH} serve --host 0.0.0.0 --port 8080 --data-dir ${DATA_DIR}
Restart=on-failure
RestartSec=5s

# Hardening / Sandboxing options for security
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
ReadWritePaths=/var/lib/bzod

[Install]
WantedBy=multi-user.target
EOF

chmod 644 "${SYSTEMD_UNIT}"
systemctl daemon-reload
echo -e "${GREEN}Systemd service registered.${NC}"

# 9. Enable and Start the Service
echo -e "\n${BLUE}[8/8] Starting BZOD service...${NC}"
systemctl enable bzod
systemctl restart bzod

sleep 2
if systemctl is-active --quiet bzod; then
    echo -e "${GREEN}BZOD service is running successfully!${NC}"
    echo -e "\n${BLUE}=== Deployment Completed Successfully ===${NC}"
    echo -e "You can access BZOD at http://localhost:8080"
    echo -e "Admin Login Dashboard is at http://localhost:8080/admin"
    echo -e "System service logs: journalctl -u bzod -f"
    echo -e "To change the default admin password, run: bzod create-admin --data-dir ${DATA_DIR}"
else
    echo -e "${RED}Error: BZOD service failed to start. Check logs using: journalctl -u bzod -n 50${NC}"
fi
