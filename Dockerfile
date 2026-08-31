# ==========================================
# Multi-Stage Dockerfile for SiteWarden
# Conforms to IEEE Std 830-1998 (SRS Section 5.1)
# ==========================================

# ------------------------------------------
# Stage 1: Build & Compile Binary
# ------------------------------------------
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/sitewarden

# Cache dependencies layer
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release || true
RUN rm -rf src

# Copy source code and build actual release binary
COPY . .
RUN touch src/lib.rs src/main.rs && cargo build --release

# ------------------------------------------
# Stage 2: Hardened Runtime Container
# ------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install Chromium, core fonts, SSL certificates, and dumb-init for zombie process reaping
RUN apt-get update && apt-get install -y --no-install-recommends \
    chromium \
    fonts-liberation \
    fonts-noto-color-emoji \
    ca-certificates \
    dumb-init \
    && rm -rf /var/lib/apt/lists/*

# Set Chromium binary location environment variable
ENV CHROME_BIN=/usr/bin/chromium \
    RUST_LOG=sitewarden=info,chromiumoxide=error,info

WORKDIR /app

# Create non-root system user and prepare directories
RUN groupadd -r sitewarden && useradd -r -g sitewarden -u 1000 -d /app sitewarden \
    && mkdir -p /app/screenshots \
    && chown -R sitewarden:sitewarden /app

# Copy compiled binary from builder stage to system PATH
COPY --from=builder /usr/src/sitewarden/target/release/sitewarden /usr/local/bin/sitewarden
RUN chmod +x /usr/local/bin/sitewarden && ln -s /usr/local/bin/sitewarden /app/sitewarden

# Switch to non-root user
USER sitewarden

# Volume for failure screenshots
VOLUME ["/app/screenshots"]

# Use dumb-init as PID 1 to ensure proper process reaping of headless browser tabs
ENTRYPOINT ["/usr/bin/dumb-init", "--", "/app/sitewarden"]

# Default command points to /app/config.yaml
CMD ["--config", "/app/config.yaml"]
