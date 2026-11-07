## CrispTrim 0.3.0 : la transcription revient, en Rust

CrispTrim retrouve la détection des « euh », sans Python : CrisperWhisper 2.0 tourne directement dans le
programme (candle). Les mots et horodatages sont identiques à ceux de la version Python 0.1.1.

- **Avancement affiché** pendant la transcription : téléchargement du modèle, chargement, « Transcription… 42 % ».
- **Carte graphique NVIDIA** (RTX 30 ou plus récente) : au premier fichier, CrispTrim télécharge une fois son module
  carte graphique (`crisptrim-gpu.exe` et les bibliothèques NVIDIA, environ 700 Mo). Environ 6 fois plus rapide
  que le temps réel avec le modèle large.
- **Sans carte NVIDIA** : transcription sur le processeur avec le modèle turbo (environ 2 fois plus rapide que le
  temps réel sur un processeur récent).
- Le modèle se télécharge au premier fichier (turbo 1,6 Go, large 3,1 Go). Les transcriptions déjà faites par la
  version Python sont reprises.

Télécharge **`CrispTrim.exe`** ci-dessous : c'est le seul fichier à prendre. `crisptrim-gpu.exe` est téléchargé
automatiquement par CrispTrim quand il en a besoin.

FFmpeg reste nécessaire (`winget install ffmpeg`), ou celui installé par l'ancienne version de CrispTrim.
L'ancienne version Python reste sur la branche [`python-old`](https://github.com/xLaugh/CrispTrim/tree/python-old).
