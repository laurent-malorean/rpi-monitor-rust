#!/bin/sh
# Charge chaque cœur différemment pour une capture d'écran du moniteur (à lancer SUR LE PI).
# Usage : sh stress-cpu.sh [durée en secondes, 180 par défaut]     Arrêt anticipé : Ctrl+C
# Chaque cœur alterne charge et repos avec son propre rythme, donc les courbes se distinguent :
# cœur 1 ≈ 20 % du temps, cœur 2 ≈ 40 %, cœur 3 ≈ 55 %, cœur 4 ≈ 70 %.
# La fréquence du Pi 5 est commune aux 4 cœurs : elle suit la charge totale, elle ne varie pas par cœur.
# Aucun privilège nécessaire ; rien n'est installé ni modifié.
DURATION=${1:-180}
N=$(nproc 2>/dev/null || getconf _NPROCESSORS_ONLN)
END=$(( $(date +%s) + DURATION ))
trap 'kill $(jobs -p) 2>/dev/null; exit 0' INT TERM

# secondes restantes, plafonnées à $1
cap() {
  left=$(( END - $(date +%s) ))
  [ "$left" -gt 0 ] || { echo 0; return; }
  [ "$1" -lt "$left" ] && echo "$1" || echo "$left"
}

load_core() {
  core=$1; busy=$((5 + 5 * (core % 4))); rest=$((20 - 4 * (core % 4)))
  while [ "$(cap 1)" -gt 0 ]; do
    if command -v taskset >/dev/null 2>&1; then taskset -c "$core" yes > /dev/null & else yes > /dev/null & fi
    pid=$!
    sleep "$(cap "$busy")"
    kill "$pid" 2>/dev/null; { wait "$pid"; } 2>/dev/null
    sleep "$(cap "$rest")"
  done
}

echo "Charge de $N cœur(s) pendant ${DURATION} s (Ctrl+C pour arrêter)."
i=0
while [ "$i" -lt "$N" ]; do
  load_core "$i" &
  i=$((i + 1))
done
wait
echo "Terminé."
