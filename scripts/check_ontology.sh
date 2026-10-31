#!/bin/sh
# Aide-mémoire non bloquant : nouveaux items publics (staged) absents de docs/ontology.md.
git diff --cached -U0 -- 'src/*.rs' \
  | grep -E '^\+\s*pub (fn|struct|enum|trait) ' \
  | sed -E 's/^\+\s*pub (fn|struct|enum|trait) ([A-Za-z0-9_]+).*/\2/' \
  | sort -u \
  | while read -r name; do
      grep -q "$name" docs/ontology.md || missing="$missing $name"
      [ -n "$missing" ] && last="$missing"
      echo "$last" > /tmp/.rpimon_missing.$$
    done
if [ -f /tmp/.rpimon_missing.$$ ]; then
  echo "Vocabulaire : absents de docs/ontology.md (probablement de l'implémentation pure) :$(cat /tmp/.rpimon_missing.$$)"
  rm -f /tmp/.rpimon_missing.$$
fi
exit 0
