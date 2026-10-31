# rpi-monitor-rust

Portage en Rust de [rpi-monitor](https://github.com/laurent-malorean/rpi-monitor) : dashboard TUI de monitoring Raspberry Pi 5, en lecture seule, sans sudo, sans Python.

7 lignes empilées : valeur actuelle en gros à gauche (+ min/max/moyenne), courbe temporelle à droite.

1. **CPU (%)** : charge globale (`/proc/stat`).
2. **Fréquence (MHz)** : `cpufreq`.
3. **Consommation (W)** : somme tension × courant des rails du PMIC (`vcgencmd pmic_read_adc`).
4. **RAM (%)** : mémoire utilisée (`/proc/meminfo`).
5. **Température (°C)** : température CPU.
6. **Ventilateur** : histogramme par périodes de 10 s (une activation d'une seule mesure est conservée).
7. **Throttling** : sous-tension, fréquence bridée, throttle, limite de température (en cours ≠ survenu depuis le boot).

## Lancement

```
cargo run -- --demo     # données synthétiques (automatique hors Linux)
cargo run               # capteurs réels, sur le Pi
cargo test              # tests
```

Touches : `q` quitter · `p` pause de l'affichage (la collecte continue) · `r` reset historique · `w` fenêtre 1 / 5 / 30 min.

## Installation, compilation pour le Pi

Voir [docs/install.md](docs/install.md). En bref : `scripts/build-pi.sh`, puis `scp` du binaire `target/aarch64-unknown-linux-gnu/release/rpi-monitor` sur le Pi.

## Architecture

- `src/sensors/` : interface `Sensors`, capteurs Linux (`linux.rs`, racines `/sys` et `/proc` injectables) et capteurs synthétiques (`demo.rs`).
- `src/history.rs` : buffers circulaires (30 min) et agrégation par périodes de 10 s.
- `src/collector.rs` : thread de collecte (ventilateur 500 ms, reste 1 s), indépendant de l'UI.
- `src/render.rs` : fonctions pures de tracé (braille, blocs, échelle, rééchantillonnage, gros chiffres).
- `src/app.rs` : état (pause, fenêtre, reset, touches) et mise en forme des indicateurs.
- `src/ui.rs` : dessin ratatui. `src/main.rs`, `src/cli.rs` : point d'entrée.

Ajouter une métrique : une méthode de `Sensors`, une série dans `History`, un `Spec` dans `app.rs`.

Voir `CLAUDE.md` (méthode TDD, conventions) et `docs/` (ontologie, bonnes pratiques).
