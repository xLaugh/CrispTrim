## CrispTrim 0.3.1 : installateur et mise à jour en un clic

- **`CrispTrim-Setup.exe`** (conseillé) : installe CrispTrim pour ton compte, sans droits administrateur, avec
  raccourcis dans le menu Démarrer (et sur le Bureau si tu veux) et désinstallation dans
  *Paramètres > Applications*. À la désinstallation, tu choisis de garder ou non les modèles et le cache.
- **`CrispTrim.exe`** (portable) : un seul fichier, rien à installer.
- **Mise à jour en un clic** pour les deux : « Nouvelle version disponible » > *Mettre à jour*. CrispTrim
  télécharge la nouvelle version, se ferme et se relance tout seul.
- **Module carte graphique** (`crisptrim-gpu.exe`) : corrigé, il est de nouveau publié. Les PC NVIDIA
  (RTX 30 ou plus récent) transcrivent sur la carte graphique, environ 6 fois plus vite que le temps réel.

Prends **`CrispTrim-Setup.exe`** ou **`CrispTrim.exe`**. `crisptrim-gpu.exe` est téléchargé automatiquement
par CrispTrim quand il en a besoin.

L'ancienne version Python reste sur la branche [`python-old`](https://github.com/xLaugh/CrispTrim/tree/python-old).
