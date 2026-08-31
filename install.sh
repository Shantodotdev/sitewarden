#!/usr/bin/env bash
# ==============================================================================
# SiteWarden - 1-Line Autonomous Production Installer
# Installs SiteWarden on any Linux VPS with Docker in seconds.
# ==============================================================================

set -e

INSTALL_DIR="/opt/sitewarden"
COMPOSE_URL="https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/docker-compose.yml"
CONFIG_URL="https://raw.githubusercontent.com/Shantodotdev/sitewarden/master/config.example.yaml"

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
echo -e "${BLUE}[1/4]${NC} 📁 Creating ${BOLD}${INSTALL_DIR}${NC} and setting up storage permissions..."
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
echo -e "${BLUE}[2/4]${NC} 📥 Downloading production configurations..."
curl -fsSL "${COMPOSE_URL}" -o docker-compose.yml

if [ ! -f config.yaml ]; then
    curl -fsSL "${CONFIG_URL}" -o config.yaml
    echo -e "      ${GREEN}✓ Created default config.yaml${NC}"
else
    echo -e "      ${YELLOW}ℹ Existing config.yaml preserved${NC}"
fi

# 5. Pull latest container image
echo -e "${BLUE}[3/4]${NC} 🐳 Pulling latest SiteWarden image from GHCR..."
docker compose pull

# 6. Launch daemon
echo -e "${BLUE}[4/4]${NC} 🚀 Starting SiteWarden service..."
docker compose up -d

echo -e "\n${BOLD}${GREEN}======================================================${NC}"
echo -e "${BOLD}${GREEN}  🎉 SiteWarden is actively monitoring your sites!    ${NC}"
echo -e "${BOLD}${GREEN}======================================================${NC}\n"
echo -e "  • ${BOLD}Config file:${NC}    ${INSTALL_DIR}/config.yaml ${YELLOW}(Hot-reloads automatically on save)${NC}"
echo -e "  • ${BOLD}View live logs:${NC} cd ${INSTALL_DIR} && docker compose logs -f"
echo -e "  • ${BOLD}Run once now:${NC}   docker exec -it sitewarden sitewarden --run-once"
echo -e "  • ${BOLD}Screenshots:${NC}    ${INSTALL_DIR}/screenshots/\n"
