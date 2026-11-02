# Ontologie (concepts métier)

| Terme | Définition | Où dans le code |
|---|---|---|
| Indicateur | Grandeur surveillée affichée sur une ligne : valeur actuelle, min/max/moyenne, courbe. Les 7 actuels : CPU %, fréquence CPU, consommation, RAM %, température, ventilateur, throttling. | `src/app.rs` (`SPECS`) |
| Historique | 30 minutes de mesures conservées par indicateur. | `src/collector.rs` (`HISTORY_S`) |
| Historique long | 48 h glissantes de mesures horodatées, agrégées par périodes de 10 s (moyenne, min, max), consultables par l’interface Web. Une période sans mesure est absente, jamais 0. | `src/archive.rs` (`Series`) |
| Service permanent | Mode `serve` : la collecte tourne sans interface terminal et se consulte par navigateur. Configuré par défaut < variable d’environnement < ligne de commande ; écoute par défaut sur 127.0.0.1:8765 ; mot de passe obligatoire sauf `--no-auth` explicite. | `src/server/config.rs` (`Config`, `resolve`), `src/cli.rs` (`ServeOptions`) |
| Événement | Intervalle continu pendant lequel le ventilateur est actif ou le throttling en cours, reconstitué depuis l’historique long (périodes de 10 s consécutives actives). | `src/server/api.rs` (`intervals`) |
| Accès protégé | Toute l’API et l’interface exigent une session : un mot de passe unique (fichier lisible par le seul propriétaire) donne un cookie valable 30 jours ; 5 échecs par minute bloquent les tentatives. Nécessaire car un tunnel rend le service public, et tout y arrive depuis 127.0.0.1. | `src/server/auth.rs` (`Auth`, `load_password`) |
| Fenêtre d'affichage | Durée visible sur la courbe (1, 5 ou 30 min), indépendante de l'historique conservé. | `src/app.rs` (`WINDOWS`) |
| Ventilateur actif | Le ventilateur est demandé ou tourne : état de refroidissement, PWM ou vitesse mesurée > 0. | `src/sensors/mod.rs` (`FanReading::active`) |
| Période de 10 s | Intervalle d'agrégation du ventilateur : une seule activation suffit à la marquer. | `src/history.rs` (`BucketSeries`) |
| Mesure indisponible | Capteur absent ou illisible : affiché `--`, jamais compté comme 0. | `src/history.rs` (NaN) |
| Throttling | Le firmware du Pi bride ou signale un problème : sous-tension, fréquence bridée, throttle, limite de température. « En cours » ≠ « survenu depuis le boot ». Un benchmark pendant un throttling n'est pas fiable. | `src/sensors/mod.rs` (`parse_throttled`, `Throttle`) |
| Consommation | Puissance électrique estimée du Pi (W) : somme tension × courant des rails du PMIC. Estimation, hors entrée 5 V et pertes. | `src/sensors/linux.rs` (`parse_pmic_power`) |
| Mode démo | Exécution avec des données synthétiques plutôt que de vrais capteurs (développement hors Pi, ex. sur Mac). Automatique hors Linux. | `src/sensors/demo.rs` (`DemoSensors`), `src/main.rs` |
