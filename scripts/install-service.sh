#!/bin/sh
# Installe rpi-monitor comme service systemd, là où il se trouve : sudo sh install-service.sh
# Prérequis : dans le même dossier que ce script, rpi-monitor (binaire) et rpi-monitor.service.
# Rien n'est copié ailleurs : le binaire, le mot de passe (« password », droits 600) et la sauvegarde
# de l'historique restent dans ce dossier. Seule l'unité va dans /etc/systemd/system (exigence de systemd).
# Le mot de passe est demandé ici (jamais passé en argument).
set -e
DIR=$(cd "$(dirname "$0")" && pwd)
[ "$(id -u)" -eq 0 ] || { echo "À lancer avec sudo." >&2; exit 1; }
BIN="$DIR/rpi-monitor"
UNIT="$DIR/rpi-monitor.service"
[ -f "$BIN" ] && [ -f "$UNIT" ] || { echo "rpi-monitor et rpi-monitor.service doivent être dans $DIR (voir docs/deploy-linux.md)." >&2; exit 1; }
chmod 755 "$BIN"

# Un vieux binaire (sans la commande « serve ») ferait redémarrer le service en boucle.
"$BIN" --help 2>&1 | grep -q "serve" || { echo "Le binaire $BIN est trop ancien (pas de commande « serve ») : recopier le dist/rpi-monitor le plus récent." >&2; exit 1; }

# Le service tourne sous l'utilisateur propriétaire du dossier (à défaut, celui qui a lancé sudo).
OWNER=$(stat -c %U "$DIR")
[ "$OWNER" != "root" ] || OWNER="${SUDO_USER:-}"
{ [ -n "$OWNER" ] && [ "$OWNER" != "root" ]; } || { echo "Lancer le script avec sudo depuis un utilisateur normal (pas root)." >&2; exit 1; }

if [ ! -f "$DIR/password" ]; then
  printf "Mot de passe d'accès (16 caractères ou plus recommandés) : "
  stty -echo; read -r PW; stty echo; echo
  [ "${#PW}" -ge 8 ] || { echo "Mot de passe trop court." >&2; exit 1; }
  umask 077
  printf '%s\n' "$PW" > "$DIR/password"
else
  echo "Mot de passe existant conservé ($DIR/password)."
fi
chown "$OWNER" "$DIR/password"
chmod 600 "$DIR/password"

systemctl stop rpi-monitor 2>/dev/null || true
sed -e "s#@DIR@#$DIR#g" -e "s#@USER@#$OWNER#g" "$UNIT" > /etc/systemd/system/rpi-monitor.service
systemctl daemon-reload
systemctl enable --now rpi-monitor
echo "Service démarré depuis $DIR (utilisateur $OWNER) : systemctl status rpi-monitor · journalctl -u rpi-monitor -f"
