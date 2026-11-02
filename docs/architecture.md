# Architecture

Un seul moteur de monitoring, deux interfaces : le terminal (TUI) et le Web.

```
 Capteurs (sensors/)  ->  Collecteur (collector.rs, thread "collector")
                                   |
                         History partagée (Arc<Mutex<History>>)
                          |                          |
        buffers 30 min (TUI)              archive 48 h horodatée (archive.rs)
                          |                          |
                    TUI autonome            API JSON (server/api.rs)  <->  snapshot.rs (history.json)
                    (app.rs, ui.rs)                  |
                                          serveur HTTP (server/mod.rs) -> interface Web (web/)
```

## Deux modes, un seul collecteur

- `rpi-monitor [tui] [--demo]` : TUI autonome (inchangée). Elle démarre son propre collecteur ; c'est le mode SSH ponctuel.
- `rpi-monitor serve [--demo]` : service permanent. Collecteur + serveur HTTP + sauvegarde, sans terminal, arrêt propre sur SIGINT/SIGTERM.

Le collecteur ignore ses clients : fermer un navigateur, arrêter le serveur ou n'avoir aucun client n'a aucun effet sur la collecte. En service, il n'existe qu'un seul collecteur ; la TUI autonome ne se connecte pas au service (lancer les deux en parallèle lit simplement deux fois les capteurs, en lecture seule).

## Données

- **Buffers TUI** (`history.rs`) : 30 min à 1 s, sans horodatage, vidables par `r`. Non modifiés.
- **Archive** (`archive.rs`) : 48 h glissantes, périodes de 10 s (moyenne, min, max), horodatées. Une période sans mesure est absente (donnée indisponible), jamais 0. Le ventilateur et le throttling y valent 1 (actif) ou 0 par mesure, donc la moyenne d'une période est la fraction d'activité. Le `reset` de la TUI ne la touche pas.
- **Sauvegarde** (`snapshot.rs`) : `history.json` (dans le dossier de l'exécutable par défaut), écrit de façon atomique (fichier temporaire + `rename`) toutes les 10 min et à l'arrêt, relu au démarrage. Une écriture qui échoue ou un fichier illisible n'arrête jamais la collecte ni le démarrage.

## API (lecture seule, JSON, `/api/v1`)

| Route | Contenu |
|---|---|
| `GET /health` | état, uptime, mode démo (public, sans données) |
| `GET /metrics/current` | dernière valeur, libellé, unité des 7 indicateurs ; `null` si indisponible ; détails ventilateur et throttling |
| `GET /metrics/history?period=&key=&max_points=` | points `[ts, moyenne, min, max]`, rééchantillonnés à `max_points` (600 par défaut) en gardant pics et trous |
| `GET /metrics/stats?period=&key=` | min / max / moyenne sur la période |
| `GET /metrics/events?period=` | intervalles d'activité du ventilateur et du throttling |
| `POST /login`, `POST /logout` | session |

Toute méthode autre que GET sur les données est refusée (405) : aucun client ne peut agir sur le matériel.

## Sécurité

Le service écoute sur 127.0.0.1 par défaut ; un tunnel le publie. Derrière un tunnel, toutes les requêtes arrivent depuis 127.0.0.1 : filtrer par adresse source est inutile, d'où l'authentification applicative (mot de passe en fichier, cookie de session aléatoire HttpOnly / SameSite=Strict, 5 échecs par minute, en-têtes CSP et nosniff). Le mot de passe est le fichier `password` à côté de l'exécutable (droits 600 exigés). Le démarrage est refusé s'il manque, sauf `--no-auth` explicite.

## Fiabilité

Un capteur en échec ou qui panique donne `None` ; le serveur répond depuis la mémoire ; les écritures disque sont dans un thread séparé. Les requêtes lentes ne bloquent que leur worker (4 threads).
