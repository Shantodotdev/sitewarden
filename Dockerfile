# ------------------------------------------
# Stage 1: Build & Compile Static Musl Binary
# ------------------------------------------
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /usr/src/sitewarden

# Cache dependencies layer
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release || true
RUN rm -rf src

# Copy source code and build optimized release binary
COPY . .
RUN touch src/lib.rs src/main.rs && cargo build --release

# ------------------------------------------
# Stage 2: Hardened Lightweight Runtime Container (<350 MB)
# ------------------------------------------
FROM alpine:3.21 AS runtime

# Install headless Chromium, standard fonts, SSL certificates, and dumb-init
RUN apk add --no-cache \
    chromium \
    font-liberation \
    font-noto-emoji \
    ca-certificates \
    dumb-init

# Set Chromium executable location environment variable
ENV CHROME_BIN=/usr/bin/chromium-browser \
    RUST_LOG=sitewarden=info,chromiumoxide=error,info

WORKDIR /app

# Create non-root system user and prepare storage
RUN addgroup -S sitewarden && adduser -S sitewarden -G sitewarden -u 1000 \
    && mkdir -p /app/screenshots \
    && chown -R sitewarden:sitewarden /app

# Copy compiled binary and default template from builder stage
COPY --from=builder /usr/src/sitewarden/target/release/sitewarden /usr/local/bin/sitewarden
COPY --from=builder /usr/src/sitewarden/config.example.yaml /app/config.example.yaml
RUN chmod +x /usr/local/bin/sitewarden && ln -s /usr/local/bin/sitewarden /app/sitewarden

# Switch to non-root user
USER sitewarden

# Volume for failure screenshots
VOLUME ["/app/screenshots"]

# Use dumb-init as PID 1 to ensure proper process reaping of headless browser tabs
ENTRYPOINT ["/usr/bin/dumb-init", "--", "/app/sitewarden"]

# Default command points to /app/config.yaml
CMD ["--config", "/app/config.yaml"]
