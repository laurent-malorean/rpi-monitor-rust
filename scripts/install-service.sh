#!/bin/sh
# Installe rpi-monitor comme service systemd, à lancer SUR LE PI : sudo sh install-service.sh
# Prérequis : dist/rpi-monitor (ou ./rpi-monitor) et rpi-monitor.service dans le dossier courant.
# Le mot de passe est demandé ici (jamais passé en argument) puis rangé dans un fichier 600.
set -e
[ "$(id -u)" -eq 0 ] || { echo "À lancer avec sudo." >&2; exit 1; }
BIN=""
for f in ./rpi-monitor ./dist/rpi-monitor; do [ -f "$f" ] && BIN="$f" && break; done
UNIT=""
for f in ./rpi-monitor.service ./deploy/rpi-monitor.service; do [ -f "$f" ] && UNIT="$f" && break; done
[ -n "$BIN" ] && [ -n "$UNIT" ] || { echo "Binaire ou unité introuvable (voir docs/install.md)." >&2; exit 1; }

install -m 755 "$BIN" /usr/local/bin/rpi-monitor
install -m 644 "$UNIT" /etc/systemd/system/rpi-monitor.service

install -d -m 700 /etc/rpi-monitor
if [ ! -f /etc/rpi-monitor/password ]; then
  printf "Mot de passe d'accès (16 caractères ou plus recommandés) : "
  stty -echo; read -r PW; stty echo; echo
  [ "${#PW}" -ge 8 ] || { echo "Mot de passe trop court." >&2; exit 1; }
  umask 077
  printf '%s\n' "$PW" > /etc/rpi-monitor/password
  chmod 600 /etc/rpi-monitor/password
else
  echo "Mot de passe existant conservé (/etc/rpi-monitor/password)."
fi

systemctl daemon-reload
systemctl enable --now rpi-monitor
echo "Service démarré : systemctl status rpi-monitor · journalctl -u rpi-monitor -f"
