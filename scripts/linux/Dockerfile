# Linux release build for Shift, run from macOS through Docker.
#
#   docker build -f scripts/linux/Dockerfile -t shift-linux-build .
#
# The base image is whatever Debian bookworm image is local; Tauri's Linux
# bundlers (deb, AppImage) need WebKitGTK, the indicator library and pkg-config
# in addition to a Rust and Node toolchain, none of which exist on macOS.
ARG BASE_IMAGE=debian:bookworm-slim
FROM ${BASE_IMAGE}

ENV DEBIAN_FRONTEND=noninteractive
# libxdo-dev is required by tauri's global-shortcut plugin; the indicator and
# rsvg libraries back the tray icon and generated icons.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl xz-utils git pkg-config file \
      build-essential libssl-dev \
      libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev \
      libayatana-appindicator3-dev librsvg2-dev patchelf \
    && rm -rf /var/lib/apt/lists/*

# pnpm is installed through npm rather than Corepack: Corepack re-resolves pnpm over
# the network in every fresh container, and this project's lockfile wants pnpm 8/9.
ARG NODE_VERSION=22.14.0
RUN arch="$(dpkg --print-architecture)" \
    && case "$arch" in arm64) node_arch=arm64 ;; amd64) node_arch=x64 ;; *) node_arch=$arch ;; esac \
    && curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
         | tar -xJ -C /usr/local --strip-components=1 \
    && npm install --global pnpm@9

ENV CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable --component rustfmt

WORKDIR /work
COPY . /work

# The Linux build has no Swift sidecars, and the frontend must exist before
# Tauri embeds it.
RUN pnpm install --frozen-lockfile && pnpm prepare:ocr && pnpm check && pnpm build

CMD ["bash", "-c", "cd src-tauri && cargo test --lib && cd .. && pnpm tauri build --bundles deb,appimage"]
