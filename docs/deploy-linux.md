# Déploiement du service sur Linux (Raspberry Pi)

`rpi-monitor serve` collecte en continu, garde 48 h d'historique, expose l'API et l'interface Web, et survit à un redémarrage (sauvegarde périodique de l'historique). Il est lancé par systemd, sans session SSH.

La compilation du binaire (`scripts/build-pi.sh`) depuis le Mac est décrite dans [install.md](install.md).

**Tout vit dans un seul dossier, `/opt/rpi-monitor/`** :

| Fichier | Rôle |
|---|---|
| `rpi-monitor` | le binaire |
| `password` | le mot de passe d'accès (droits 600) |
| `history.json` | la sauvegarde de l'historique (créée par le service) |

Par défaut, le service cherche `password` et écrit `history.json` dans le dossier de son exécutable : déplacer le dossier suffit à tout déplacer. Seule l'unité systemd est ailleurs (`/etc/systemd/system/rpi-monitor.service`).

### Installation

**1. Sur le Mac** : le binaire `dist/rpi-monitor` est recompilé automatiquement à chaque commit (hook `post-commit`) ; sinon `scripts/build-pi.sh`. Copier ces trois fichiers du Mac vers le dossier personnel du Pi (par exemple `/home/<utilisateur>/`), avec FileZilla (SFTP) :

- `dist/rpi-monitor` : le binaire
- `deploy/rpi-monitor.service` : l'unité systemd
- `scripts/install-service.sh` : le script d'installation

**2. Sur le Pi** (en SSH) : lancer l'installation.

```sh
sudo sh install-service.sh
```

Le script crée l'utilisateur système `rpi-monitor` (groupe `video`, pour lire les capteurs), installe le binaire dans `/opt/rpi-monitor` et l'unité dans `/etc/systemd/system`, demande le **mot de passe d'accès** (écrit dans `/opt/rpi-monitor/password`, droits 600) puis active et démarre le service. Au démarrage du Pi, le service se lance seul et redémarre en cas de panne (`Restart=always`).

Vérifier, sur le Pi : `systemctl status rpi-monitor`, `journalctl -u rpi-monitor -f`, puis `curl http://127.0.0.1:8765/api/v1/health` depuis le Pi.

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

**1. Sur le Mac** : le binaire est déjà recompilé par le hook `post-commit` (sinon `scripts/build-pi.sh`). Recopier ces trois fichiers du Mac vers le dossier personnel du Pi (par exemple `/home/<utilisateur>/`), avec FileZilla (SFTP) :

- `dist/rpi-monitor` : le binaire
- `deploy/rpi-monitor.service` : l'unité systemd
- `scripts/install-service.sh` : le script d'installation

**2. Sur le Pi** : relancer le script, qui arrête le service, remplace le binaire, conserve le mot de passe et l'historique, puis redémarre.

```sh
sudo sh install-service.sh
```

L'arrêt écrit une dernière sauvegarde : l'historique est conservé (le temps d'arrêt apparaît comme un trou dans les courbes).

### Désinstallation

Sur le Pi :

```sh
sudo systemctl disable --now rpi-monitor
sudo rm /etc/systemd/system/rpi-monitor.service
sudo rm -r /opt/rpi-monitor        # binaire, mot de passe et historique
sudo userdel rpi-monitor
sudo systemctl daemon-reload
```

### Ressources

Pas de base de données : l'historique de 48 h tient en mémoire (quelques Mo) et est sauvegardé dans un seul fichier JSON (`/opt/rpi-monitor/history.json`, environ 5 Mo) toutes les 10 minutes et à l'arrêt, soit une écriture disque par 10 minutes. Mesurer sur le Pi après installation : `systemctl status rpi-monitor` (mémoire), `top -p $(pidof rpi-monitor)` (CPU), `du -h /opt/rpi-monitor`.
