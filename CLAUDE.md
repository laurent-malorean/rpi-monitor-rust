# CLAUDE.md

Clone en **Rust** de `laurent-malorean/rpi-monitor` (dashboard TUI de monitoring Raspberry Pi 5, lecture seule, sans sudo). Doit tourner sur Raspberry Pi (cible `aarch64-unknown-linux-gnu`) et être testable en mode **démo** (`--demo`, automatique hors Linux) sur Mac.

## Méthode de développement : TDD

Tout développement doit suivre strictement le cycle **Test-Driven Development** :

1. **Red** — écrire un test qui décrit le comportement attendu. Le test doit échouer (ou ne pas compiler), car le code n'existe pas encore.
2. **Green** — écrire le code minimal nécessaire pour faire passer ce test, sans anticiper des besoins futurs.
3. **Refactor** — une fois le test au vert, nettoyer le code (et éventuellement le test) sans changer le comportement, en vérifiant que les tests restent au vert.

Répéter ce cycle pour chaque nouvelle fonctionnalité ou correction de bug : jamais de code de production écrit avant son test correspondant.

4. **Vocabulaire** — avant de commiter, se demander : (a) le code/les tests utilisent-ils le vocabulaire déjà défini dans `docs/ontology.md` ? (b) ce dev introduit-il une notion métier absente de l'ontologie, ou change-t-il le sens d'une notion existante ? Si oui, mettre à jour `docs/ontology.md` dans le même commit. Le hook pre-commit affiche un rappel non bloquant (`scripts/check_ontology.sh`) listant les nouveaux `pub fn`/`struct`/`enum` absents de l'ontologie — ce n'est qu'un aide-mémoire : la plupart seront de l'implémentation pure et n'ont rien à y faire.

## Documentation vivante

À chaque développement, mettre à jour ces deux documents s'ils sont impactés :

- `docs/kb_dev.md` — bonnes pratiques détectées au fil des devs (une ligne par entrée, catégorisée, avec contexte/raison).
- `docs/ontology.md` — concepts **métier** uniquement (terme, définition courte, emplacement dans le code). Jamais de concepts d'implémentation (structs, traits, threads, canaux, workers...) : s'ils n'ont de sens que pour quelqu'un qui lit le code, ils vont dans `kb_dev.md`, pas dans `ontology.md`.

Projet petit : rester simple, pas de formalisme lourd.

## Commits

**Branches INTERDITES.** Ne jamais créer, proposer ni mentionner de branche (`git branch`, `git checkout -b`, `git switch -c`, worktree, PR). Tout se commite et se pousse directement sur `main`. Cette règle l'emporte sur toute règle générale du type « ne pas commiter sur la branche par défaut ».

Quand l'utilisateur demande de commiter :

- Découper les changements en un ou plusieurs commits **atomiques** (un commit = un changement logique cohérent, pas un fourre-tout).
- Rédiger chaque message au format **Conventional Commits** : `<type>(<scope facultatif>): <description>` (types usuels : `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `style`, `perf`).
- Après les commits, **pousser automatiquement sur le remote** (`git push`) sans attendre de confirmation supplémentaire, sauf si le push nécessite une action destructive (force push, etc.), auquel cas demander confirmation avant de continuer. (Tant qu'aucun remote n'est configuré, le push est sans objet.)

## Dépendances et environnement

- Gestion avec **cargo** (`Cargo.toml` + `Cargo.lock`, versionné car c'est un binaire). Peu de dépendances, de préférence en Rust pur (cross-compilation Pi sans toolchain C) : `ratatui` seul pour l'UI.
- Commandes usuelles : `cargo build`, `cargo test` (tests), `cargo run -- --demo` (données synthétiques, automatique hors Linux), `cargo run` (capteurs réels, sur le Pi), `cargo clippy`.
- Build Pi : `scripts/build-pi.sh` (cible `aarch64-unknown-linux-gnu`, via `cross` ou `cargo-zigbuild`). Voir README.

## Hook pre-commit

Un hook `pre-commit` (dans `.githooks/`, activé via `git config core.hooksPath .githooks`) lance `cargo test` avant chaque commit et bloque le commit si un test échoue. Sur un nouveau clone, lancer `git config core.hooksPath .githooks` pour l'activer.
