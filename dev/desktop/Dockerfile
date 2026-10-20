# A throwaway Linux desktop for trying uvview, viewed in a browser through noVNC.
FROM rust:latest AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        xvfb x11vnc novnc websockify fluxbox xterm \
        libx11-6 libxcursor1 libxrandr2 libxi6 libxkbcommon-x11-0 pulseaudio-utils \
        libgl1 libegl1 libgl1-mesa-dri fonts-noto-core \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/uvview /usr/local/bin/uvview
COPY dev/desktop/start.sh /usr/local/bin/start.sh
EXPOSE 6080
CMD ["start.sh"]
