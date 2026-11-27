## CrispTrim 0.3.3 : Linux, macOS et correctif pour ffmpeg installé par WinGet

- **Linux et macOS** : CrispTrim existe maintenant pour les trois systèmes.
  - **Linux** (64 bits, distribution de 2022 ou plus récente) : `CrispTrim-x86_64.AppImage` pour toutes les
    distributions (`chmod +x`, puis double-clic), `crisptrim_0.3.3_amd64.deb` pour Debian, Ubuntu, Mint…,
    ou `CrispTrim-linux-x86_64`, le programme seul. Il faut ffmpeg (`sudo apt install ffmpeg`…).
  - **macOS** 11 ou plus récent, Apple Silicon et Intel : `CrispTrim-macOS.dmg`, glisse CrispTrim dans
    *Applications*. Il faut ffmpeg (`brew install ffmpeg`). L'appli n'est pas signée par Apple : au premier
    lancement, clic droit > *Ouvrir*.
  - La transcription se fait sur le processeur (Accelerate sur Mac).
- **Correctif Windows** : si ffmpeg a été installé avec WinGet, CrispTrim pouvait afficher « ffprobe : Le
  chemin d'accès ne peut pas être parcouru car il contient un point de montage non approuvé (os error 448) »
  à l'ouverture d'un rush. CrispTrim lance maintenant directement le vrai ffmpeg.
- La coupe des reprises est marquée **BETA**.

Sur Windows, prends **`CrispTrim-Setup.exe`** (installateur) ou **`CrispTrim.exe`** (portable).
`crisptrim-gpu.exe` est téléchargé automatiquement par CrispTrim sur les PC NVIDIA.
