# ==========================================
# Stage 1: Rust Builder
# ==========================================
FROM rust:slim AS rust-builder
WORKDIR /app

# Install dependencies required for building Rust on Debian
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev build-essential ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better layer caching
COPY backend/Cargo.toml backend/Cargo.lock* backend/
COPY backend/api/Cargo.toml backend/api/
COPY backend/shared/Cargo.toml backend/shared/
COPY backend/indexer/Cargo.toml backend/indexer/
COPY backend/verifier/Cargo.toml backend/verifier/
COPY backend/seeder/Cargo.toml backend/seeder/
COPY backend/contract_abi/Cargo.toml backend/contract_abi/
COPY soroban-registry/Cargo.toml soroban-registry/
COPY soroban-registry/crates/ soroban-registry/crates/

# Copy full source and build the release binary
COPY backend/ ./backend/
COPY database/ ./database/

WORKDIR /app/backend
RUN cargo build --release --bin api

# ==========================================
# Stage 2: Node Builder
# ==========================================
FROM node:20-slim AS node-builder
WORKDIR /app

# Enable pnpm as strictly required by their contributing guide
RUN corepack enable pnpm

# Copy frontend source and build
COPY frontend/ ./frontend/
WORKDIR /app/frontend
RUN pnpm install --frozen-lockfile
RUN pnpm build

# ==========================================
# Stage 3: Healthcheck Tooling (The Trap Fix)
# ==========================================
FROM busybox:1.36-uclibc AS healthcheck-builder
# We pull busybox solely to extract a standalone wget binary

# ==========================================
# Stage 4: Production Distroless Runtime
# ==========================================
# We use cc-debian12 because Rust binaries dynamically link to libc/libgcc
FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app

# Copy the compiled Rust API binary
COPY --from=rust-builder --chown=nonroot:nonroot /app/backend/target/release/api /app/api

# Copy the frontend build output
COPY --from=node-builder --chown=nonroot:nonroot /app/frontend/.next /app/frontend/.next
COPY --from=node-builder --chown=nonroot:nonroot /app/frontend/public /app/frontend/public

# Copy the standalone wget binary for our healthcheck
COPY --from=healthcheck-builder /bin/wget /bin/wget

# Enforce the non-root user for security (Acceptance Criteria)
USER nonroot:nonroot
EXPOSE 3001

# The required Healthcheck (using our injected wget)
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/bin/wget", "-q", "-O", "-", "http://localhost:3001/health"]

# Execute the API
CMD ["/app/api"]