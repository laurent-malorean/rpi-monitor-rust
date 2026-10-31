# Installation (poste de dev macOS Apple Silicon)

Prérequis pour développer, tester en mode démo et compiler pour le Raspberry Pi 5 (`aarch64-unknown-linux-gnu`).

## 1. Outils

```sh
brew install rustup zig
```

Ajouter `rustup` au PATH, **avant** `/opt/homebrew/bin` (sinon le `cargo` de Homebrew reste prioritaire). Un simple `export` dans le terminal ne vaut que pour la session en cours : l'écrire aussi dans `~/.zshrc` pour le rendre permanent.

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH"                          # session en cours
echo 'export PATH="$(brew --prefix rustup)/bin:$PATH"' >> ~/.zshrc       # permanent
```

Puis installer la toolchain et la cible Pi (`brew install rustup` n'installe aucune toolchain par défaut) :

```sh
rustup default stable
rustup target add aarch64-unknown-linux-gnu
cargo install --locked cargo-zigbuild
```

Vérification : `which cargo` doit pointer vers le répertoire de `rustup`, et `rustup target list --installed` doit lister `aarch64-unknown-linux-gnu`.

## 2. Développement et tests (Mac)

```sh
cargo test
cargo run -- --demo     # données synthétiques (automatique hors Linux)
```

## 3. Compilation pour le Pi

```sh
scripts/build-pi.sh
```

Le script vise la glibc 2.36 (Raspberry Pi OS Bookworm), compile dans un cache hors du projet (`~/.cache/rpi-monitor-target`) et ne laisse que `dist/rpi-monitor`, le seul fichier à copier sur le Pi.

## 4. Déploiement

```sh
scp dist/rpi-monitor <utilisateur>@<pi>:~/
ssh <utilisateur>@<pi> ./rpi-monitor
```

Aucun Python ni `sudo` nécessaire sur le Pi.
