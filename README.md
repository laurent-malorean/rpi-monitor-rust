# rpi-monitor-rust

Dashboard de monitoring pour **Raspberry Pi 5**, dans le terminal (TUI), en **lecture seule**, **sans sudo** et **sans Python**. C'est un portage en Rust de [rpi-monitor](https://github.com/laurent-malorean/rpi-monitor), l'application Python d'origine.

## Une expérience : ma première application en Rust

C'est ma première application en Rust, et je ne connais pas le langage. Le but du projet n'est pas seulement le moniteur : c'est de **tester le développement d'une application Rust entièrement menée avec une IA (Claude Code), sans connaître Rust**.

Le cadre est le même que pour le projet Python : un `CLAUDE.md` qui impose la méthode, et une documentation vivante dans `docs/`.

- **TDD strict** : chaque fonctionnalité commence par un test qui échoue, puis le code minimal, puis le nettoyage.
- **Commits atomiques** en Conventional Commits, avec un hook pre-commit qui lance `cargo test` et refuse le commit si un test échoue.
- **Vocabulaire métier** tenu à jour dans `docs/ontology.md`, **bonnes pratiques** dans `docs/kb_dev.md`.

Le code produit par l'IA est donc contrôlé par ses tests et par ces règles, pas par ma lecture du Rust. Les limites de ce mode de travail (ce que les tests ne couvrent pas, ce que je ne peux pas relire) font partie de ce qu'on cherche à mesurer.

## Ce que fait l'application

Sept indicateurs empilés. Pour chacun : la valeur actuelle en gros à gauche (avec min / max / moyenne), et la courbe dans le temps à droite.

1. **CPU (%)** : charge globale (`/proc/stat`).
2. **Fréquence (MHz)** : fréquence courante du processeur (`cpufreq`).
3. **Consommation (W)** : puissance estimée, somme tension × courant des rails du PMIC (`vcgencmd pmic_read_adc`). C'est une estimation, pas la mesure à l'entrée USB-C.
4. **RAM (%)** : mémoire utilisée (`/proc/meminfo`).
5. **Température (°C)** : température du processeur.
6. **Ventilateur** : histogramme par périodes de 10 s. Une activation d'une seule mesure reste visible.
7. **Throttling** : sous-tension, fréquence bridée, throttle, limite de température. « En cours » est distingué de « survenu depuis le démarrage ».

Une mesure absente est affichée `--` et n'est jamais comptée comme zéro.

Collecte : ventilateur toutes les 500 ms, les autres mesures toutes les secondes. Historique conservé : 30 minutes. Affichage rafraîchi toutes les secondes.

### Touches

| Touche | Action |
|---|---|
| `q` | quitter (`Ctrl+C` aussi) |
| `p` | pause de l'affichage (la collecte continue) |
| `r` | remise à zéro de l'historique |
| `w` | fenêtre d'affichage 1 / 5 / 30 min (5 min au départ) |

## Deux modes

- **Capteurs réels** : sur le Raspberry Pi (Linux).
- **Mode démo** : données synthétiques cohérentes (charge qui varie, température progressive, déclenchement du ventilateur, événements de throttling). Il permet de tester toute l'interface sans Raspberry Pi, par exemple sur Mac. Il est **activé automatiquement hors Linux**.

## Lancement

```sh
cargo run -- --demo     # mode démo (automatique hors Linux)
cargo run               # capteurs réels, à lancer sur le Pi
cargo test              # tests
```

## Installation et compilation pour le Pi

Le développement se fait sur Mac (Apple Silicon) et le binaire est compilé pour le Pi depuis le Mac, avec `cargo-zigbuild`. La procédure complète est dans [docs/install.md](docs/install.md). En bref :

```sh
scripts/build-pi.sh
scp target/aarch64-unknown-linux-gnu/release/rpi-monitor <utilisateur>@<pi>:~/
```

Le résultat est un binaire Linux ARM64 autonome d'environ 700 Ko (glibc 2.36, Raspberry Pi OS Bookworm). Ni Python ni `sudo` ne sont nécessaires sur le Pi.

## Architecture

![Architecture de rpi-monitor-rust](docs/architecture.svg)

- `src/sensors/` : le trait `Sensors`, les capteurs Linux (`linux.rs`, avec les racines `/sys` et `/proc` injectables pour les tests) et les capteurs de démo (`demo.rs`).
- `src/history.rs` : buffers circulaires (30 min) et agrégation par périodes de 10 s.
- `src/collector.rs` : thread de collecte, indépendant de l'interface.
- `src/render.rs` : fonctions de tracé pures (courbes braille, blocs, échelle, rééchantillonnage, gros chiffres).
- `src/app.rs` : état (pause, fenêtre, reset, touches) et mise en forme des indicateurs.
- `src/ui.rs` : dessin avec ratatui. `src/main.rs` et `src/cli.rs` : point d'entrée.

Pour ajouter une métrique : une méthode dans `Sensors`, une série dans `History`, un `Spec` dans `app.rs`.

## État du projet

- 68 tests automatiques, qui ne nécessitent aucun Raspberry Pi (les lectures `/sys` et `/proc` sont testées sur des arbres de fichiers simulés).
- Le mode démo a été vérifié dans un vrai terminal sur Mac, et la compilation croisée pour le Pi fonctionne.
- **À valider** : l'exécution sur un vrai Raspberry Pi 5 (chemins sysfs, `vcgencmd`), et la comparaison de performances avec la version Python (mémoire, CPU, démarrage).

## Documentation

- [CLAUDE.md](CLAUDE.md) : méthode de développement et conventions.
- [docs/ontology.md](docs/ontology.md) : le vocabulaire métier.
- [docs/kb_dev.md](docs/kb_dev.md) : les bonnes pratiques relevées au fil du développement.
- [docs/install.md](docs/install.md) : installation et déploiement.
