## CrispTrim 0.2.0 : réécriture complète en Rust

CrispTrim est maintenant un seul programme Rust (`CrispTrim.exe`) : plus besoin de Python ni de PyTorch.

- Même fenêtre : rushes, lecteur avec aperçu monté, timeline, onglets Réglages / Coupes / Texte.
- Mêmes règles de coupe et **exports identiques** à la version 0.1.1 (FCPXML, Premiere, Vegas, JSON, SRT, TXT).
- Ligne de commande intégrée au même programme (`CrispTrim.exe rush.mp4 --export fcpxml,premiere`).
- Logo CrispTrim dans la barre des tâches.

**Limite de cette version** : la transcription CrisperWhisper n'est pas encore refaite en Rust. Les « euh » sont
coupés quand la version Python a déjà transcrit le fichier (le cache est repris automatiquement) ; sinon,
seuls les silences sont coupés.

**FFmpeg** est nécessaire (`winget install ffmpeg`), ou celui installé par l'ancienne version de CrispTrim.

L'ancienne version Python reste sur la branche [`python-old`](https://github.com/xLaugh/CrispTrim/tree/python-old)
et dans la release [v0.1.1](https://github.com/xLaugh/CrispTrim/releases/tag/v0.1.1).
