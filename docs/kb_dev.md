# Bonnes pratiques détectées

- **Capteurs Pi** : chemins hwmon numérotés de façon instable, les chercher par le fichier `name` (`pwmfan`, `cpu_thermal`) et redétecter après un échec de lecture.
- **Ventilateur** : `pwm1` et `fan1_input` peuvent valoir 0 alors qu'il tourne ; croiser avec `cooling_device*/cur_state`.
- **Collecte** : thread séparé de l'UI ; un capteur en échec donne `None` (et un panic est rattrapé par `catch_unwind`), jamais un arrêt de la collecte. Donc pas de `panic = "abort"` en release.
- **Fichiers non versionnés** : `target/`, captures d'écran et `.DS_Store` ignorés par git.
- **Throttling Pi** : lire `get_throttled` en sysfs (`devices/platform/*/get_throttled`) avant de lancer `vcgencmd` ; le fallback `vcgencmd` n'est tenté que sur le vrai `/sys` pour que les tests ne dépendent pas de la machine.
- **Consommation Pi 5** : `vcgencmd pmic_read_adc` (lecture seule, sans sudo) ; appariement des rails `_A`/`_V` par nom, `EXT5V_V` exclu car sans courant. Comme le throttling, appelé seulement sur le vrai `/sys`.
- **Graphiques / échelle auto** : une courbe pleine (aires) sur une échelle automatique exagère les valeurs (47 °C paraît « presque plein ») ; préférer une ligne.
- **Lignes compactes** : une ligne binaire (throttling) n'a pas besoin du gros chiffre ; `Spec.compact` la ramène à 5 lignes pour que 6 indicateurs tiennent dans ~35 lignes de terminal.
- **Hauteur de terminal** : 7 indicateurs demandent ~40 lignes ; en dessous, les lignes à gros chiffre se compriment. Ne pas combiner hauteur proportionnelle et minimum sur les lignes empilées (constaté sur le Pi avec Textual) : utiliser des `Constraint::Min`/`Length` explicites dans ratatui.
- **Tests UI** : tester le rendu avec `ratatui::backend::TestBackend` à plusieurs hauteurs de terminal, pas seulement une grande.
- **Rust / portabilité** : remplacer `psutil` par la lecture directe de `/proc/stat` (CPU, deltas), `/proc/meminfo` (`MemAvailable`) et `cpufreq/scaling_cur_freq` ; racines `/sys` et `/proc` injectables pour tester sur Mac avec un faux arbre dans un répertoire temporaire.
- **Rust / cross-compilation Pi** : dépendances en Rust pur, aucune lib C ; cible `aarch64-unknown-linux-gnu` (Pi 5, OS 64 bits) via `cross` ou `cargo-zigbuild`.
- **Démo** : le mode démo implémente le même trait `Sensors` que les capteurs réels ; générateur pseudo-aléatoire maison (xorshift) pour éviter la dépendance `rand`.
