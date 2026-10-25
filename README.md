# BeatMaker

DAW personnelle, 100 % locale (hors ligne, sans compte). Moteur audio en Rust, interface Tauri 2.
Voir `docs/ARCHITECTURE.md` (architecture) et `docs/ROADMAP.md` (phases).

**État : Phase 1 (MVP) + Phase 2a** : pistes multiples façon Reason, instrument par piste, rack de plug-ins, modulation, quantification 1/1 à 1/128, boucle, annuler/rétablir des prises, export WAV multipiste.

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
2. Dans « Audio et MIDI » : choisis la sortie, la fréquence et le tampon, puis **Appliquer**.
3. **Pistes** : chaque piste est une ligne horizontale avec son propre instrument (synthé soustractif, synthé FM, boîte à rythmes) et un preset au choix. « + Ajouter une piste » en crée une nouvelle.
4. **Armer** une piste (●) : le clavier MIDI joue son instrument et l'enregistrement y va. `1` = enregistrer (décompte), Échap = stop, Espace = lecture.
5. **Rack** (à droite) : réglages de la piste sélectionnée sous forme de potentiomètres (glisser, Maj = précis, double-clic = défaut), module de **Modulation (LFO)** (vibrato, trémolo, auto-pan, filtre qui respire) et **plug-ins** (filtre, distorsion, chorus, delay, reverb, compresseur) via « + Plug-in » ; ↑↓ pour l'ordre, ON/OFF pour contourner.
6. **Quantifier** : le curseur « Palier » va de 1/1 à 1/128 (mode Pro : triolets, force, swing). Le bouton aligne la piste ; **Annuler** (Cmd+Z) revient en arrière sans limite.
7. **Boucle** : joue en boucle N mesures ; enregistre plusieurs passages avec « Superposer » (mode Pro) pour construire un motif.
8. **Exporter en WAV** : mixe toutes les pistes (muet/solo respectés) dans `~/Music/BeatMaker/`.

Clavier d'ordinateur de secours : rangée du milieu = touches blanches, rangée du dessus = touches noires
(positions physiques QWERTY : sur un clavier AZERTY, `A S D F…` correspond aux touches `Q S D F…`), `Z`/`X` octave, `C`/`V` vélocité, `Alt` = sustain.

## Limites connues

- Le projet n'est pas encore sauvegardé sur disque (sauvegarde, autosave et récupération : prochaine phase). Seul l'export WAV persiste.
- Annuler/rétablir couvre les prises (enregistrement, quantification, effacement), pas encore l'ajout/suppression de pistes ni les réglages.
- Le « routage visuel » du rack est une chaîne linéaire (instrument → modulation → plug-ins → mixeur) ; pas de câblage libre façon Reason.
- Pas de bus, d'envois/retours ni d'automation (phase 4). Pas de plug-ins externes (VST/AU) : hors périmètre.
- Les sons sont synthétisés : bons pour synthés, basses et batterie ; pas de piano/cordes/guitares réalistes avant les banques d'échantillons (phase 11).
- Les notes MIDI en direct sont horodatées au début du tampon audio (± ~5 ms à 256 frames).
- Sortie audio stéréo uniquement. Pas d'entrée micro (phase 7).
- Non testé sur macOS ni avec du vrai matériel au moment de la livraison.

Licences des dépendances : `LICENSES.md`. Aucun échantillon sonore n'est inclus (tout est synthétisé).
