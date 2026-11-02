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

## 4. Déploiement de la TUI autonome

```sh
scp dist/rpi-monitor <utilisateur>@<pi>:~/
ssh <utilisateur>@<pi> ./rpi-monitor
```

Aucun Python ni `sudo` nécessaire sur le Pi.

## 5. Service permanent (collecte 24 h/24 + interface Web)

`rpi-monitor serve` collecte en continu, garde 48 h d'historique, expose l'API et l'interface Web, et survit à un redémarrage (sauvegarde périodique de l'historique). Il est lancé par systemd, sans session SSH.

**Tout vit dans un seul dossier, `/opt/rpi-monitor/`** :

| Fichier | Rôle |
|---|---|
| `rpi-monitor` | le binaire |
| `password` | le mot de passe d'accès (droits 600) |
| `history.json` | la sauvegarde de l'historique (créée par le service) |

Par défaut, le service cherche `password` et écrit `history.json` dans le dossier de son exécutable : déplacer le dossier suffit à tout déplacer. Seule l'unité systemd est ailleurs (`/etc/systemd/system/rpi-monitor.service`).

### Installation

```sh
scripts/build-pi.sh
scp dist/rpi-monitor deploy/rpi-monitor.service scripts/install-service.sh <utilisateur>@<pi>:~/
ssh -t <utilisateur>@<pi> sudo sh install-service.sh
```

Le script crée l'utilisateur système `rpi-monitor` (groupe `video`, pour lire les capteurs), installe le binaire et l'unité, demande le **mot de passe d'accès** (écrit dans `/opt/rpi-monitor/password`, droits 600) puis active et démarre le service. Au démarrage du Pi, le service se lance seul et redémarre en cas de panne (`Restart=always`).

Vérifier : `systemctl status rpi-monitor`, `journalctl -u rpi-monitor -f`, puis `curl http://127.0.0.1:8765/api/v1/health` depuis le Pi.

Changer le mot de passe : éditer `/opt/rpi-monitor/password` (conserver les droits 600, propriétaire `rpi-monitor`), puis `sudo systemctl restart rpi-monitor`.

### Configuration

Ordre de priorité : défaut < variable d'environnement < option de la ligne de commande.

| Réglage | Option | Variable | Défaut |
|---|---|---|---|
| Adresse d'écoute | `--host` | `RPI_MONITOR_HOST` | `127.0.0.1` |
| Port | `--port` | `RPI_MONITOR_PORT` | `8765` |
| Fichier du mot de passe | `--password-file` | `RPI_MONITOR_PASSWORD_FILE` | `password` dans le dossier de l'exécutable |
| Dossier de sauvegarde | `--data-dir` | `RPI_MONITOR_DATA_DIR` | le dossier de l'exécutable |

Changer le port sans toucher au code : `sudo systemctl edit rpi-monitor`, puis

```ini
[Service]
Environment=RPI_MONITOR_PORT=9090
```

et `sudo systemctl restart rpi-monitor`. Un port invalide ou déjà occupé donne un message explicite dans `journalctl`.

### Accès hors du réseau local

Le service n'écoute que sur `127.0.0.1` : rien n'est ouvert sur le réseau. Pour le consulter à distance, publier ce port local avec un **tunnel de votre choix** (Tailscale Funnel, Cloudflare Tunnel, etc.). Le service est indépendant du tunnel choisi.

Un tunnel rend le service public : c'est pourquoi **un mot de passe est obligatoire** (`--no-auth` existe pour les tests, à ne jamais utiliser derrière un tunnel). Le mot de passe donne un cookie de session valable 30 jours ; 5 échecs par minute bloquent les tentatives. Les sessions sont en mémoire : un redémarrage du service oblige à se reconnecter. Choisir un mot de passe long (16 caractères ou plus).

Derrière un tunnel HTTPS, le cookie reçoit l'attribut `Secure` si le tunnel transmet `X-Forwarded-Proto: https`.

### Mise à jour

Relancer simplement le script d'installation avec le nouveau binaire : il arrête le service, remplace le binaire, conserve le mot de passe et l'historique, puis redémarre.

```sh
scripts/build-pi.sh
scp dist/rpi-monitor deploy/rpi-monitor.service scripts/install-service.sh <utilisateur>@<pi>:~/
ssh -t <utilisateur>@<pi> sudo sh install-service.sh
```

L'arrêt écrit une dernière sauvegarde : l'historique est conservé (le temps d'arrêt apparaît comme un trou dans les courbes).

### Désinstallation

```sh
sudo systemctl disable --now rpi-monitor
sudo rm /etc/systemd/system/rpi-monitor.service
sudo rm -r /opt/rpi-monitor        # binaire, mot de passe et historique
sudo userdel rpi-monitor
sudo systemctl daemon-reload
```

### Ressources

Pas de base de données : l'historique de 48 h tient en mémoire (quelques Mo) et est sauvegardé dans un seul fichier JSON (`/opt/rpi-monitor/history.json`, environ 5 Mo) toutes les 10 minutes et à l'arrêt, soit une écriture disque par 10 minutes. Mesurer sur le Pi après installation : `systemctl status rpi-monitor` (mémoire), `top -p $(pidof rpi-monitor)` (CPU), `du -h /opt/rpi-monitor`.
