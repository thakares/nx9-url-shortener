#!/usr/bin/env bash
#
# BZOD Production Docker Deployment
#
# Privacy-First URL Shortener & Landing Page Platform
#
# Usage:
#   curl -fsSL https://bzo.in/deploy.sh | sudo bash
#
# Or:
#   sudo bash deploy.sh
#
# Environment overrides:
#   BZOD_VERSION=0.7.0
#   BZOD_IMAGE=ghcr.io/thakares/nx9-url-shortener
#   BZOD_ROOT=/DATA/AppData/bzod
#   BZOD_PORT=8654
#

set -euo pipefail

# ============================================================
# Configuration
# ============================================================

BZOD_VERSION="${BZOD_VERSION:-0.7.0}"
BZOD_IMAGE="${BZOD_IMAGE:-ghcr.io/thakares/nx9-url-shortener}"
BZOD_ROOT="${BZOD_ROOT:-/DATA/AppData/bzod}"
BZOD_PORT="${BZOD_PORT:-8654}"

CONTAINER_NAME="${CONTAINER_NAME:-bzod}"

DATA_DIR="${BZOD_ROOT}/data"
CONFIG_DIR="${BZOD_ROOT}/config"
IMAGES_DIR="${BZOD_ROOT}/images"
COMPOSE_DIR="${BZOD_ROOT}/compose"

COMPOSE_FILE="${COMPOSE_DIR}/docker-compose.yml"
ENV_FILE="${COMPOSE_DIR}/bzod.env"
BACKUP_ROOT="${BZOD_ROOT}/backups"

IMAGE="${BZOD_IMAGE}:${BZOD_VERSION}"

# ============================================================
# Output
# ============================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() {
    echo -e "${BLUE}$*${NC}"
}

success() {
    echo -e "${GREEN}$*${NC}"
}

warning() {
    echo -e "${YELLOW}$*${NC}"
}

error() {
    echo -e "${RED}$*${NC}" >&2
}

die() {
    error "$*"
    exit 1
}

# ============================================================
# Root check
# ============================================================

if [[ "${EUID}" -ne 0 ]]; then
    die "This script must be run as root. Use: sudo bash deploy.sh"
fi

echo
echo -e "${BLUE}============================================================${NC}"
echo -e "${BLUE} BZOD — Production Docker Deployment${NC}"
echo -e "${BLUE}============================================================${NC}"
echo
echo "Version:       ${BZOD_VERSION}"
echo "Image:         ${IMAGE}"
echo "Application:   ${BZOD_ROOT}"
echo "Data:          ${DATA_DIR}"
echo "Config:        ${CONFIG_DIR}"
echo "Images:        ${IMAGES_DIR}"
echo "Port:          ${BZOD_PORT}"
echo

# ============================================================
# 1. Install Docker
# ============================================================

info "[1/8] Checking Docker..."

if ! command -v docker >/dev/null 2>&1; then
    info "Docker is not installed. Installing Docker..."

    apt-get update -qq
    apt-get install -y \
        ca-certificates \
        curl

    install -m 0755 -d /etc/apt/keyrings

    if [[ ! -f /etc/apt/keyrings/docker.asc ]]; then
        curl -fsSL \
            https://download.docker.com/linux/debian/gpg \
            -o /etc/apt/keyrings/docker.asc

        chmod a+r /etc/apt/keyrings/docker.asc
    fi

    . /etc/os-release

    echo \
        "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] \
        https://download.docker.com/linux/debian \
        ${VERSION_CODENAME} stable" \
        > /etc/apt/sources.list.d/docker.list

    apt-get update -qq

    apt-get install -y \
        docker-ce \
        docker-ce-cli \
        containerd.io \
        docker-buildx-plugin \
        docker-compose-plugin
fi

if ! docker info >/dev/null 2>&1; then
    systemctl enable --now docker
fi

if ! docker compose version >/dev/null 2>&1; then
    die "Docker Compose plugin is unavailable."
fi

success "✓ Docker and Docker Compose available"

# ============================================================
# 2. Create persistent directories
# ============================================================

info "[2/8] Creating persistent application directories..."

mkdir -p \
    "${DATA_DIR}" \
    "${CONFIG_DIR}" \
    "${IMAGES_DIR}" \
    "${COMPOSE_DIR}" \
    "${BACKUP_ROOT}"

chmod 700 "${CONFIG_DIR}"
chmod 755 "${IMAGES_DIR}"

success "✓ Persistent directories ready"

# ============================================================
# 3. Configuration
# ============================================================

info "[3/8] Preparing configuration..."

if [[ ! -f "${ENV_FILE}" ]]; then

    cat > "${ENV_FILE}" <<EOF
BZOD_VERSION=${BZOD_VERSION}
BZOD_IMAGE=${BZOD_IMAGE}

HOST=0.0.0.0
PORT=8654

DATA_DIR=/app/data
CONFIG_DIR=/app/config
IMAGES_DIR=/app/images

COOKIE_SECURE=true
RUST_LOG=info
EOF

    chmod 600 "${ENV_FILE}"

    success "✓ New Docker configuration created"

else

    warning "Existing Docker configuration preserved"

    # Update image/version while preserving all other settings.
    sed -i \
        -E "s#^BZOD_VERSION=.*#BZOD_VERSION=${BZOD_VERSION}#" \
        "${ENV_FILE}" || true

    sed -i \
        -E "s#^BZOD_IMAGE=.*#BZOD_IMAGE=${BZOD_IMAGE}#" \
        "${ENV_FILE}" || true
fi

# ============================================================
# 4. Create Compose definition
# ============================================================

info "[4/8] Writing Docker Compose configuration..."

cat > "${COMPOSE_FILE}" <<'EOF'
services:

  bzod:
    image: ${BZOD_IMAGE}:${BZOD_VERSION}
    container_name: bzod

    restart: unless-stopped

    ports:
      - "${PORT:-8654}:8654"

    environment:
      HOST: "${HOST:-0.0.0.0}"
      PORT: "${PORT:-8654}"

      DATA_DIR: "/app/data"
      CONFIG_DIR: "/app/config"
      IMAGES_DIR: "/app/images"

      COOKIE_SECURE: "${COOKIE_SECURE:-true}"
      RUST_LOG: "${RUST_LOG:-info}"

    volumes:

      # Persistent application databases.
      - ${BZOD_ROOT}/data:/app/data

      # Persistent application configuration.
      - ${BZOD_ROOT}/config:/app/config

      # User-uploaded / application images.
      #
      # IMPORTANT:
      # /app/images is required by the image router.
      - ${BZOD_ROOT}/images:/app/images

    healthcheck:
      test:
        [
          "CMD",
          "curl",
          "-fsS",
          "http://127.0.0.1:8654/status"
        ]
      interval: 30s
      timeout: 5s
      start_period: 10s
      retries: 3

    security_opt:
      - no-new-privileges:true
EOF

# Append BZOD_ROOT because compose needs it.
if ! grep -q '^BZOD_ROOT=' "${ENV_FILE}"; then
    echo "BZOD_ROOT=${BZOD_ROOT}" >> "${ENV_FILE}"
fi

# Port variable expected by compose.
if ! grep -q '^PORT=' "${ENV_FILE}"; then
    echo "PORT=${BZOD_PORT}" >> "${ENV_FILE}"
fi

success "✓ Docker Compose configuration written"

# ============================================================
# 5. Backup existing installation
# ============================================================

info "[5/8] Creating pre-upgrade backup..."

TIMESTAMP="$(date '+%Y%m%d-%H%M%S')"
BACKUP_DIR="${BACKUP_ROOT}/pre-upgrade-${TIMESTAMP}-v${BZOD_VERSION}"

mkdir -p "${BACKUP_DIR}"

if [[ -d "${DATA_DIR}" ]]; then
    cp -a "${DATA_DIR}" "${BACKUP_DIR}/data"
fi

if [[ -d "${CONFIG_DIR}" ]]; then
    cp -a "${CONFIG_DIR}" "${BACKUP_DIR}/config"
fi

if [[ -d "${IMAGES_DIR}" ]]; then
    cp -a "${IMAGES_DIR}" "${BACKUP_DIR}/images"
fi

cp -a "${COMPOSE_FILE}" "${BACKUP_DIR}/docker-compose.yml"
cp -a "${ENV_FILE}" "${BACKUP_DIR}/bzod.env"

success "✓ Backup created:"
echo "  ${BACKUP_DIR}"

# ============================================================
# 6. Pull new image
# ============================================================

info "[6/8] Pulling BZOD ${BZOD_VERSION} image..."

if ! docker pull "${IMAGE}"; then
    die "Unable to pull ${IMAGE}"
fi

success "✓ Docker image downloaded"

# ============================================================
# 7. Deploy
# ============================================================

info "[7/8] Deploying BZOD..."

cd "${COMPOSE_DIR}"

# Stop/remove the existing container through Compose.
docker compose \
    --env-file "${ENV_FILE}" \
    -f "${COMPOSE_FILE}" \
    down \
    --remove-orphans

# Start the requested image.
docker compose \
    --env-file "${ENV_FILE}" \
    -f "${COMPOSE_FILE}" \
    up -d

success "✓ BZOD container started"

# ============================================================
# 8. Validation
# ============================================================

info "[8/8] Validating deployment..."

sleep 5

if ! docker inspect \
    --format '{{.State.Running}}' \
    "${CONTAINER_NAME}" 2>/dev/null | grep -q '^true$'; then

    error "BZOD container failed to start."
    echo

    docker compose \
        --env-file "${ENV_FILE}" \
        -f "${COMPOSE_FILE}" \
        logs --tail=100

    error
    error "Deployment failed. Existing data was not removed."
    error "Backup: ${BACKUP_DIR}"

    exit 1
fi

success "✓ Container is running"

# ------------------------------------------------------------
# Health check
# ------------------------------------------------------------

HEALTH_OK=0

for _ in {1..12}; do
    if curl -fsS \
        "http://127.0.0.1:${BZOD_PORT}/status" \
        >/dev/null 2>&1; then

        HEALTH_OK=1
        break
    fi

    sleep 2
done

if [[ "${HEALTH_OK}" -eq 1 ]]; then
    success "✓ HTTP health check passed"
else
    warning "⚠ HTTP health check did not respond yet"
    warning "The container is running; inspect logs if necessary:"
    echo
    echo "  docker compose -f ${COMPOSE_FILE} logs --tail=100"
fi

# ============================================================
# Verify image and binary
# ============================================================

echo
info "Installed image:"
docker image inspect "${IMAGE}" \
    --format '  {{.RepoTags}}  ({{.Id}})' \
    2>/dev/null || true

echo
info "Container:"
docker inspect "${CONTAINER_NAME}" \
    --format '  {{.Name}}  {{.Config.Image}}' \
    2>/dev/null || true

echo
info "Persistent mounts:"
docker inspect "${CONTAINER_NAME}" \
    --format '{{range .Mounts}}  {{.Source}} -> {{.Destination}}{{"\n"}}{{end}}' \
    2>/dev/null || true

# ============================================================
# Final status
# ============================================================

echo
echo -e "${GREEN}============================================================${NC}"
echo -e "${GREEN} BZOD ${BZOD_VERSION} deployed successfully${NC}"
echo -e "${GREEN}============================================================${NC}"
echo

echo "Web UI:"
echo "  http://<server-ip>:${BZOD_PORT}"

echo
echo "Persistent data:"
echo "  ${DATA_DIR}"

echo
echo "Persistent images:"
echo "  ${IMAGES_DIR}"

echo
echo "Docker Compose:"
echo "  ${COMPOSE_FILE}"

echo
echo "Backup:"
echo "  ${BACKUP_DIR}"

echo
echo "Useful commands:"
echo "  docker compose -f ${COMPOSE_FILE} ps"
echo "  docker compose -f ${COMPOSE_FILE} logs -f bzod"
echo "  docker compose -f ${COMPOSE_FILE} restart bzod"

echo
success "Deployment complete."
