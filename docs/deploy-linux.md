# Déploiement du service sur Linux (Raspberry Pi)

`rpi-monitor serve` collecte en continu, garde 48 h d'historique, expose l'API et l'interface Web, et survit à un redémarrage (sauvegarde périodique de l'historique). Il est lancé par systemd, sans session SSH.

La compilation du binaire (`scripts/build-pi.sh`) depuis le poste de développement est décrite dans [install.md](install.md).

**Tout vit dans un seul dossier : celui où vous posez l'exécutable** (par exemple `~/rpi-monitor`) :

| Fichier | Rôle |
|---|---|
| `rpi-monitor` | le binaire |
| `password` | le mot de passe d'accès (droits 600) |
| `history.json` | la sauvegarde de l'historique (créée par le service) |

Par défaut, le service cherche `password` et écrit `history.json` dans le dossier de son exécutable : déplacer le dossier suffit à tout déplacer. Rien n'est copié ailleurs. Seule l'unité systemd est dans `/etc/systemd/system/rpi-monitor.service` (exigence de systemd) ; elle est générée par le script d'installation avec le chemin de ce dossier, aucun chemin n'est figé.

### Installation

**1. Sur votre ordinateur** : le dossier `dist/` est recompilé automatiquement à chaque commit (hook `post-commit`), sinon `scripts/build-pi.sh`. Il contient les trois fichiers à copier, rien d'autre à aller chercher :

- `rpi-monitor` : le binaire
- `rpi-monitor.service` : l'unité systemd
- `install-service.sh` : le script d'installation

Les copier ensemble dans **un même dossier du Pi**, au choix (par exemple `~/rpi-monitor`) avec l'outil de transfert de votre choix : client SFTP graphique (FileZilla, WinSCP, Cyberduck…) ou `scp` en ligne de commande.

**2. Sur le Pi** (en SSH ou au clavier) : se placer dans ce dossier et lancer l'installation.

```sh
cd ~/rpi-monitor
sudo sh install-service.sh
```

Le script détecte son propre dossier, vérifie que le binaire est assez récent (il refuse un binaire sans la commande `serve`), demande le **mot de passe d'accès** (écrit dans `password`, droits 600, à côté du binaire), génère l'unité systemd pour ce dossier, puis active et démarre le service. Le service tourne sous l'utilisateur propriétaire du dossier (groupe `video` ajouté pour lire les capteurs). Au démarrage du Pi, il se lance seul et redémarre en cas de panne (`Restart=always`).

**Ne pas déplacer le dossier ensuite** sans relancer `sudo sh install-service.sh` depuis le nouvel emplacement : l'unité mémorise le chemin.

Vérifier, sur le Pi : `systemctl status rpi-monitor`, `journalctl -u rpi-monitor -f`, puis `curl http://127.0.0.1:8765/api/v1/health` depuis le Pi.

Changer le mot de passe : éditer le fichier `password` du dossier (conserver les droits 600), puis `sudo systemctl restart rpi-monitor`.

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

**1. Sur votre ordinateur** : `dist/` est déjà recompilé par le hook `post-commit` (sinon `scripts/build-pi.sh`). Recopier ses trois fichiers vers le dossier personnel du Pi, avec le même outil de transfert.

**2. Sur le Pi** : relancer le script dans le dossier, qui arrête le service, régénère l'unité, conserve le mot de passe et l'historique, puis redémarre.

```sh
cd ~/rpi-monitor
sudo sh install-service.sh
```

L'arrêt écrit une dernière sauvegarde : l'historique est conservé (le temps d'arrêt apparaît comme un trou dans les courbes).

### Désinstallation

Sur le Pi :

```sh
sudo systemctl disable --now rpi-monitor
sudo rm /etc/systemd/system/rpi-monitor.service
rm -r ~/rpi-monitor                # le dossier : binaire, mot de passe et historique (facultatif)
sudo systemctl daemon-reload
```

### Ressources

Pas de base de données : l'historique de 48 h tient en mémoire (quelques Mo) et est sauvegardé dans un seul fichier JSON (`history.json` dans le dossier, environ 5 Mo) toutes les 10 minutes et à l'arrêt, soit une écriture disque par 10 minutes. Mesurer sur le Pi après installation : `systemctl status rpi-monitor` (mémoire), `top -p $(pidof rpi-monitor)` (CPU), `du -h ~/rpi-monitor`.
