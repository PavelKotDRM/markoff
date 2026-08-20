FROM rust:1-bookworm

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        libasound2-dev \
        libegl1-mesa-dev \
        libgtk-3-dev \
        libwayland-dev \
        libx11-dev \
        libxcursor-dev \
        libxinerama-dev \
        libxkbcommon-dev \
        libxrandr-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*