#!/usr/bin/env bash
# ==============================================================================
# SiteWarden - 1-Line Autonomous Production Installer
# Installs SiteWarden on any Linux VPS with Docker in seconds.
# ==============================================================================

set -e

REPO="Shantodotdev/sitewarden"
INSTALL_DIR="/opt/sitewarden"
BIN_DIR="/usr/local/bin"
COMPOSE_URL="https://raw.githubusercontent.com/${REPO}/master/docker-compose.yml"
CONFIG_URL="https://raw.githubusercontent.com/${REPO}/master/config.example.yaml"

# Colors for terminal output
BOLD="\033[1m"
GREEN="\033[0;32m"
BLUE="\033[0;34m"
YELLOW="\033[1;33m"
RED="\033[0;31m"
NC="\033[0m"

echo -e "\n${BOLD}${BLUE}======================================================${NC}"
echo -e "${BOLD}${GREEN}           SiteWarden Production Installer           ${NC}"
echo -e "${BOLD}${BLUE}======================================================${NC}\n"

# 1. Check for Docker
if ! command -v docker &> /dev/null; then
    echo -e "${RED}[Error] Docker is not installed on this system.${NC}"
    echo -e "Please install Docker first (https://docs.docker.com/engine/install/) and re-run this installer.\n"
    exit 1
fi

# 2. Check for Docker Compose
if ! docker compose version &> /dev/null; then
    echo -e "${RED}[Error] Docker Compose plugin is required.${NC}"
    echo -e "Please install Docker Compose and re-run.\n"
    exit 1
fi

# 3. Create target directory and fix permissions
echo -e "${BLUE}[1/5]${NC} 📁 Creating ${BOLD}${INSTALL_DIR}${NC} and setting up storage permissions..."
if [ "$EUID" -ne 0 ]; then
    sudo mkdir -p "${INSTALL_DIR}/screenshots"
    sudo chown -R "$USER:$USER" "${INSTALL_DIR}"
    sudo chown -R 1000:1000 "${INSTALL_DIR}/screenshots"
else
    mkdir -p "${INSTALL_DIR}/screenshots"
    chown -R 1000:1000 "${INSTALL_DIR}/screenshots"
fi

cd "${INSTALL_DIR}"

# 4. Download compose and config templates
echo -e "${BLUE}[2/5]${NC} 📥 Downloading production configurations..."
curl -fsSL "${COMPOSE_URL}" -o docker-compose.yml

if [ ! -f config.yaml ]; then
    curl -fsSL "${CONFIG_URL}" -o config.yaml
    chmod 600 config.yaml
    echo -e "      ${GREEN}✓ Created default config.yaml (permissions: 600)${NC}"
else
    chmod 600 config.yaml 2>/dev/null || true
    echo -e "      ${YELLOW}ℹ Existing config.yaml preserved${NC}"
fi

# 5. Install global host CLI wrapper
echo -e "${BLUE}[3/5]${NC} ⚡ Installing global ${BOLD}${BIN_DIR}/sitewarden${NC} CLI tool..."
WRAPPER_FILE="${BIN_DIR}/sitewarden"

cat << 'EOF' > /tmp/sitewarden_wrapper.tmp
#!/usr/bin/env bash
# ==============================================================================
# SiteWarden Host CLI Wrapper
# Proxies commands to the isolated SiteWarden Docker daemon container.
# ==============================================================================

COMPOSE_DIR="/opt/sitewarden"
COMPOSE_FILE="${COMPOSE_DIR}/docker-compose.yml"
CONTAINER_NAME="sitewarden"

RED="\033[0;31m"
GREEN="\033[0;32m"
YELLOW="\033[1;33m"
NC="\033[0m"

if [ ! -f "$COMPOSE_FILE" ]; then
    echo -e "${RED}[Error] SiteWarden installation not found at ${COMPOSE_DIR}${NC}"
    echo -e "Please run the installer: curl -fsSL https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/install.sh | bash"
    exit 1
fi

case "$1" in
  start)
    echo -e "${GREEN}Starting SiteWarden background daemon...${NC}"
    docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" up -d
    ;;
  stop)
    echo -e "${YELLOW}Stopping SiteWarden daemon...${NC}"
    docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" down
    ;;
  restart)
    echo -e "${GREEN}Restarting SiteWarden daemon...${NC}"
    docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" restart
    ;;
  logs)
    shift
    docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" logs -f "$@"
    ;;
  update|upgrade)
    echo -e "${GREEN}Pulling latest SiteWarden release...${NC}"
    docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" pull
    docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" up -d
    ;;
  *)
    TTY_FLAG=""
    if [ -t 0 ] && [ -t 1 ]; then
        TTY_FLAG="-it"
    elif [ -t 0 ]; then
        TTY_FLAG="-i"
    fi

    RUNNING_ID=$(docker ps -q -f "name=^/${CONTAINER_NAME}$")
    if [ -n "$RUNNING_ID" ]; then
        docker exec $TTY_FLAG "$CONTAINER_NAME" sitewarden "$@"
    else
        docker compose --project-directory "$COMPOSE_DIR" -f "$COMPOSE_FILE" run --rm --no-deps sitewarden "$@"
    fi
    ;;
esac
EOF

if [ "$EUID" -ne 0 ] && [ ! -w "$BIN_DIR" ]; then
    sudo install -m 755 /tmp/sitewarden_wrapper.tmp "$WRAPPER_FILE"
else
    install -m 755 /tmp/sitewarden_wrapper.tmp "$WRAPPER_FILE"
fi
rm -f /tmp/sitewarden_wrapper.tmp
echo -e "      ${GREEN}✓ Installed ${WRAPPER_FILE}${NC}"

# 6. Pull latest container image
echo -e "${BLUE}[4/5]${NC} 🐳 Pulling latest SiteWarden image from GHCR..."
docker compose pull

# 7. Launch daemon
echo -e "${BLUE}[5/5]${NC} 🚀 Starting SiteWarden service..."
docker compose up -d

echo -e "\n${BOLD}${GREEN}======================================================${NC}"
echo -e "${BOLD}${GREEN}  🎉 SiteWarden is actively monitoring your sites!    ${NC}"
echo -e "${BOLD}${GREEN}======================================================${NC}\n"
echo -e "  • ${BOLD}View dashboard:${NC} sitewarden status"
echo -e "  • ${BOLD}Run tests now:${NC}  sitewarden test"
echo -e "  • ${BOLD}Stream logs:${NC}    sitewarden logs"
echo -e "  • ${BOLD}Diagnostics:${NC}    sitewarden doctor"
echo -e "  • ${BOLD}Config file:${NC}    ${INSTALL_DIR}/config.yaml ${YELLOW}(Hot-reloads automatically on save)${NC}"
echo -e "  • ${BOLD}Screenshots:${NC}    ${INSTALL_DIR}/screenshots/\n"
