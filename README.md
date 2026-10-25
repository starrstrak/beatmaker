# BeatMaker

DAW personnelle, 100 % locale (hors ligne, sans compte). Moteur audio en Rust, interface Tauri 2.
Voir `docs/ARCHITECTURE.md` (architecture) et `docs/ROADMAP.md` (phases).

**État : Phase 0 + Phase 1 (MVP)** : jouer au clavier MIDI, enregistrer avec métronome et décompte, réécouter, exporter en WAV.

## Démarrage rapide (macOS)

```sh
./scripts/mac-setup.sh --run   # installe ce qui manque, teste, génère diagnostic.txt, lance l'app
```

Colle le contenu de `diagnostic.txt` dans la conversation. Détail manuel ci-dessous.

## Installation manuelle (macOS, Apple Silicon)

```sh
xcode-select --install                                   # outils de compilation Apple
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust (rustup)
cargo install tauri-cli --version "^2" --locked          # commande `cargo tauri`
```

## Lancement

```sh
cargo tauri dev          # depuis la racine du dépôt : ouvre l'application
```

Application empaquetée (`.app`) : `cargo tauri icon src-tauri/icons/icon.png && cargo tauri build`.

## Test sans interface (outil `bmk`)

```sh
cargo test                                                    # tests du moteur (rendu offline, enregistrement, export, zéro allocation temps réel)
cargo run -p beatmaker-io --bin bmk -- devices                # liste sorties audio + entrées MIDI détectées
cargo run -p beatmaker-io --bin bmk -- demo demo.wav 96000 24 # rend une boucle d'accords en WAV 24 bits / 96 kHz
cargo run -p beatmaker-io --bin bmk -- live --buffer 256      # jouer au clavier MIDI dans le terminal (r=enregistrer, s=stop, p=lecture, e fichier.wav = export)
```

## Utilisation

1. Branche le M-Audio Oxygen 49 en USB (reconnu automatiquement, aucun pilote requis).
2. Dans « Son et clavier » : choisis la sortie (haut-parleurs du MacBook), la fréquence, le tampon, puis **Appliquer**.
3. Joue. Pitch bend, molette de modulation, pédale de sustain, aftertouch et vélocité sont pris en compte.
4. **Enregistrer** (touche `1`) : décompte puis enregistrement. **Stop** (Échap) : la prise apparaît. **Lecture** (Espace).
5. **Exporter en WAV** : fichier créé dans `~/Music/BeatMaker/`.

Clavier d'ordinateur de secours : rangée du milieu = touches blanches, rangée du dessus = touches noires
(positions physiques QWERTY : sur un clavier AZERTY, `A S D F…` correspond aux touches `Q S D F…`), `Z`/`X` octave, `C`/`V` vélocité, `Alt` = sustain.

## Limites connues de la phase 1

- Un seul synthé monophonique par voix (16 voix) et une seule prise MIDI (pas encore de pistes ni d'annuler/rétablir : phase 2).
- Les notes MIDI en direct sont horodatées au début du tampon audio (± une taille de tampon, soit ~5 ms à 256 frames).
- Sortie audio stéréo uniquement. Pas d'entrée micro (phase 7).
- Non testé sur macOS ni avec du vrai matériel au moment de la livraison : voir « Validation ».

Licences des dépendances : `LICENSES.md`. Aucun échantillon sonore n'est inclus (tout est synthétisé).
