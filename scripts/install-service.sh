#!/bin/sh
# Installe rpi-monitor comme service systemd, à lancer SUR LE PI : sudo sh install-service.sh
# Prérequis : rpi-monitor et rpi-monitor.service dans le dossier courant.
# Tout est rangé dans /opt/rpi-monitor : binaire, mot de passe (« password », droits 600), sauvegarde.
# Le mot de passe est demandé ici (jamais passé en argument).
set -e
DIR=/opt/rpi-monitor
[ "$(id -u)" -eq 0 ] || { echo "À lancer avec sudo." >&2; exit 1; }
BIN=""
for f in ./rpi-monitor ./dist/rpi-monitor; do [ -f "$f" ] && BIN="$f" && break; done
UNIT=""
for f in ./rpi-monitor.service ./deploy/rpi-monitor.service; do [ -f "$f" ] && UNIT="$f" && break; done
[ -n "$BIN" ] && [ -n "$UNIT" ] || { echo "Binaire ou unité introuvable (voir docs/deploy-linux.md)." >&2; exit 1; }

# Un vieux binaire (sans la commande « serve ») ferait redémarrer le service en boucle.
"$BIN" --help 2>&1 | grep -q "serve" || { echo "Le binaire $BIN est trop ancien (pas de commande « serve ») : recopier le dist/rpi-monitor le plus récent." >&2; exit 1; }

id rpi-monitor >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin --groups video rpi-monitor
install -d -m 750 -o rpi-monitor -g rpi-monitor "$DIR"
systemctl stop rpi-monitor 2>/dev/null || true
install -m 755 -o root -g root "$BIN" "$DIR/rpi-monitor"
install -m 644 "$UNIT" /etc/systemd/system/rpi-monitor.service

if [ ! -f "$DIR/password" ]; then
  printf "Mot de passe d'accès (16 caractères ou plus recommandés) : "
  stty -echo; read -r PW; stty echo; echo
  [ "${#PW}" -ge 8 ] || { echo "Mot de passe trop court." >&2; exit 1; }
  umask 077
  printf '%s\n' "$PW" > "$DIR/password"
  chown rpi-monitor:rpi-monitor "$DIR/password"
  chmod 600 /opt/rpi-monitor/password
else
  echo "Mot de passe existant conservé ($DIR/password)."
fi

systemctl daemon-reload
systemctl enable --now rpi-monitor
echo "Service démarré : systemctl status rpi-monitor · journalctl -u rpi-monitor -f"
