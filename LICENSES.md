# Licences

Le code de BeatMaker est publié sous licence MIT (usage personnel, aucune restriction).

## Dépendances Rust (toutes permissives, usage local sans condition)

| Crate | Rôle | Licence |
|---|---|---|
| rtrb | file lock-free audio ↔ contrôle | MIT / Apache-2.0 |
| hound | écriture/lecture WAV | Apache-2.0 |
| cpal | sortie audio (CoreAudio sur macOS) | Apache-2.0 |
| midir | entrée MIDI (CoreMIDI sur macOS) | MIT |
| serde, serde_json | sérialisation | MIT / Apache-2.0 |
| tauri 2 (+ wry, tao) | fenêtre et WebView | MIT / Apache-2.0 |

Vérifié à l'intégration avec `cargo tree`; à re-vérifier à chaque nouvelle dépendance.

## Sons

Phase 1 : aucun échantillon. Les sons sont synthétisés from scratch par `crates/engine/src/synth.rs` (domaine public / MIT).
Les banques d'échantillons CC0/CC-BY arriveront en phase 11, avec la source et la licence de chacune listées ici.
