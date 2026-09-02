# Build environment for Handy's Linux bundles: the toolchain only, never the
# app. Driven by ./shell.
#
# Ubuntu 24.04, the release CI builder for Handy's AppImage and rpm: nothing
# bundles glibc, so building against an older one than the host's is what keeps
# the bundles runnable on more than just this machine.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive

# build.yml's "install dependencies (ubuntu 24.04 x64)" list, plus the toolchains
# crates shell out to and what the bundlers need: tauri copies xdg-open into the
# AppDir, linuxdeploy's plugins shell out to patchelf and file, appimagetool
# validates the .desktop entry, and WebKit's media playback rides on the
# GStreamer plugins that `bundleMediaFramework` pulls in.
RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf \
      libasound2-dev libopenblas-dev libx11-dev libxtst-dev libxrandr-dev \
      libgtk-layer-shell-dev libevdev-dev libssl-dev \
      build-essential clang libclang-dev cmake pkg-config \
      ca-certificates curl git unzip xdg-utils desktop-file-utils file \
      gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    && rm -rf /var/lib/apt/lists/*

# ggml's Vulkan backend needs the loader, headers and glslc. LunarG's repo is
# where CI gets them.
RUN curl -fsSL -o /etc/apt/trusted.gpg.d/lunarg.asc https://packages.lunarg.com/lunarg-signing-key-pub.asc \
 && curl -fsSL -o /etc/apt/sources.list.d/lunarg-vulkan.list https://packages.lunarg.com/vulkan/1.3.290/lunarg-vulkan-1.3.290-noble.list \
 && apt-get update && apt-get install -y --no-install-recommends vulkan-sdk \
 && rm -rf /var/lib/apt/lists/*

# The distro's apt rust is far too old for tauri 2. shell repoints CARGO_HOME at the
# build tree so the crate cache survives between runs. The node_modules/.bin shims
# ask for `node`; bun answers to that name.
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path \
 && curl -fsSL https://bun.sh/install | BUN_INSTALL=/usr/local bash \
 && ln -s bun /usr/local/bin/node

# With no /dev/fuse in a container, the linuxdeploy AppImages have to unpack
# themselves rather than mount.
ENV APPIMAGE_EXTRACT_AND_RUN=1

# The Vulkan loader and the Wayland client library have to come from the host's
# own graphics stack. A bundled copy shadows it and can stop the host's Mesa or
# Vulkan ICD from loading.
ENV LINUXDEPLOY_EXCLUDED_LIBRARIES='libvulkan.so*;libwayland-client.so*'

# Build as the invoking user, so what lands in the bind-mounted repo stays owned
# by them.
# noble ships its own uid-1000 user, which would collide.
ARG USER_ID=1000
RUN userdel -r ubuntu 2>/dev/null; useradd -u $USER_ID -m build
USER build
ENV HOME=/home/build

# Fill Tauri's AppImage tool cache, which it otherwise re-downloads on every
# build. linuxdeploy comes from upstream rather than Tauri's 2024 mirror, which
# predates the LINUXDEPLOY_EXCLUDED_LIBRARIES above.
RUN arch=$(uname -m) \
 && mkdir -p $HOME/.cache/tauri && cd $HOME/.cache/tauri \
 && curl -fsSLO "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage" \
 && curl -fsSL -o linuxdeploy-plugin-appimage.AppImage "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-$arch.AppImage" \
 && curl -fsSLO "https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-$arch" \
 && curl -fsSLO https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh \
 && curl -fsSLO https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gstreamer/master/linuxdeploy-plugin-gstreamer.sh \
 && chmod +x ./*
