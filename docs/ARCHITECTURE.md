# Architecture (phase 1)

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
