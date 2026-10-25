# Architecture (phase 2a)

```
UI (ui/, WebView)  --invoke-->  src-tauri (commandes)  -->  Session (control thread)
                                                              | SPSC lock-free (rtrb)
MIDI USB -> midir callback --SharedSender-------------------->|  Command
                                                              v
                    cpal callback -> AudioProcessor::process_device  (audio thread)
                                                              | Event (status, notes enregistrées, clip rendu)
                                                              v
                                                      Session::poll (30 Hz)
```

## Règles du thread audio (testées)
- Aucune allocation ni libération : `tests/rt_alloc.rs` installe un allocateur compteur et échoue si `process_*` alloue.
  Un clip remplacé est renvoyé au thread de contrôle (`Event::ClipRetired`) pour y être libéré.
- Aucun verrou, aucune E/S. Paramètres et commandes passent par une file SPSC (4096 commandes, 8192 évènements).
- Traitement en f32. Le rendu est découpé en blocs de 32 échantillons alignés sur le flux absolu : le son ne dépend pas de la taille du tampon
  (`block_size_does_not_change_the_sound`).
- Limiteur de sécurité en sortie (transparent sous 0,8, jamais > 1,0).

## Temps musical
Positions en ticks (960 par noire), indépendantes de la fréquence d'échantillonnage : un clip enregistré se rend à 44,1 comme à 192 kHz.
Le transport avance en ticks : un changement de tempo en cours de lecture ne produit aucun saut.

## Export
`render_clip` utilise exactement le même `AudioProcessor` que le temps réel (hors ligne, plus vite que le temps réel).
WAV 16/24 bits (dither TPDF) ou 32 bits flottant, 8 kHz à 192 kHz.

## Modèle de pistes (phase 2a)
```
Session (control thread)                      AudioProcessor (audio thread)
  Project { TrackState[] }  --Command-->       tracks[ Track ]
   instrument kind + params                      Instrument (enum: Subtractive | Fm | Drums)
   effects[ kind + params + bypass ]             -> effects[ Slot{Effect, bypass} ]  (max 8)
   volume/pan/mute/solo, LFO, quantise           -> LFO modulation -> fader/pan -> master mix
   clip (MIDI ticks)                             clip playback / live input (armed track)
```
- Le projet vit côté contrôle (`TrackState`). Chaque instrument/effet est construit côté contrôle (allocations) puis envoyé en `Box` ;
  l'ancien objet revient par `Event::Retired` pour être libéré côté contrôle (jamais sur le thread audio).
- Changer de carte son ou de fréquence reconstruit tout le graphe depuis le projet (`Session::attach`).
- Les périphériques (instrument, effets) sont décrits par des tables `ParamInfo` : l'UI génère les potentiomètres depuis ce catalogue.
- Quantiseur : `quantize.rs`, grille de 1/1 à 1/128 (+ triolets), force 0..1, swing 50..75 %, durées de notes préservées.
- Boucle : le thread audio découpe le tampon à la fin de la boucle (position exacte à l'échantillon) ; les notes tenues sont coupées au bouclage.
