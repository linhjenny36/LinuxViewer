#!/bin/sh
# Xvfb display -> x11vnc -> noVNC on port 6080, running LinuxViewer (reopened if closed).
set -e
Xvfb :1 -screen 0 1600x900x24 &
export DISPLAY=:1
sleep 1
fluxbox >/dev/null 2>&1 &
x11vnc -display :1 -forever -shared -nopw -quiet -rfbport 5900 &
websockify --web /usr/share/novnc 6080 localhost:5900 >/dev/null 2>&1 &
echo "Open http://localhost:6080/vnc.html?autoconnect=1&resize=remote"
while true; do uvview || true; sleep 1; done
