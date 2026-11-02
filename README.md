# CrispTrim

Coupe automatiquement les **silences** et les **hésitations** (« euh », « hum »…) de tes rushes,
puis exporte une timeline pour **DaVinci Resolve**, **Final Cut Pro**, **Premiere Pro**, **Vegas Pro**, ou un MP4 monté.
Tout tourne en local, sur ta carte graphique. La transcription mot à mot utilise **CrisperWhisper 2.0**.

![CrispTrim : timeline avec les silences hachurés et les hésitations en orange](docs/screenshot.png)

## Ce que ça fait

- **Silences** : détectés sur l'énergie du son, avec un seuil automatique réglable.
- **Hésitations** : « euh », « heu », « hum », « mmh »… et bruits de bouche, repérés mot à mot par CrisperWhisper.
- **Aperçu monté** : la lecture saute les coupes. Chaque coupe s'écoute et se garde ou se recoupe d'un clic.
- **Exports**, créés à côté du rush :

  | Logiciel / format | Fichier | Import |
  |---|---|---|
  | DaVinci Resolve, Final Cut Pro | `.fcpxml` | *Fichier > Importer > Timeline* |
  | Premiere Pro | `.xml` | *Fichier > Importer* |
  | Vegas Pro | `.vegas.txt` | *Fichier > Importer > Fichier texte EDL* |
  | Vidéo montée | `.mp4` | NVENC si disponible |
  | Sous-titres / texte | `.srt` / `.txt` | recalés sur le montage, sans les hésitations |

## Installation (Windows)

1. Télécharge **`CrispTrim-Setup.exe`** depuis la [dernière release](https://github.com/xLaugh/CrispTrim/releases/latest).
2. Double-clique dessus. Si Windows affiche « Windows a protégé votre ordinateur », clique sur
   *Informations complémentaires > Exécuter quand même* (le programme n'est pas signé).
3. L'installateur récupère tout seul Python, PyTorch, FFmpeg et le modèle de transcription
   (~6 Go à télécharger, 10 à 20 minutes), puis crée les raccourcis.

Tout est installé dans `%LOCALAPPDATA%\CrispTrim`, sans droits administrateur. Les mises à jour se font en
un clic depuis l'appli (« Nouvelle version disponible » > *Mettre à jour*) et ne téléchargent que le code.
Pour désinstaller : *Paramètres > Applications > CrispTrim*.

Une carte graphique NVIDIA est conseillée ; sans elle, la transcription tourne sur le processeur, plus lentement.

## Installation depuis le code (développement)

Prérequis :
- [Git](https://git-scm.com/) et [uv](https://docs.astral.sh/uv/) ;
- [FFmpeg](https://ffmpeg.org/) dans le PATH (`winget install ffmpeg`) ;
- une carte graphique NVIDIA (conseillé ; marche aussi sur processeur, plus lentement).

```bash
git clone https://github.com/xLaugh/CrispTrim.git
cd CrispTrim
uv sync --extra asr
```

`--extra asr` installe PyTorch (CUDA 12.8, compatible RTX 50xx) et CrisperWhisper. Le modèle se télécharge
au premier fichier analysé. Garde le dossier dans un chemin court (ex. `Documents\CrispTrim`) : PyTorch plante
sur Windows quand un chemin dépasse 260 caractères.

## Utilisation

Double-clique sur **`CrispTrim.bat`** (ou `uv run crisptrim-app`) : une fenêtre s'ouvre.

1. **Glisse tes vidéos** dans la fenêtre (ou *Ajouter des rushes*) : l'analyse et la transcription démarrent.
2. La timeline montre la forme d'onde, les silences hachurés et les hésitations en orange.
3. Ajuste les réglages (seuil, silence minimum, marge) : la coupe se recalcule instantanément.
4. Onglet **Coupes** : écoute chaque coupe (▶) et décoche celles à garder. Onglet **Texte** : transcription cliquable.
5. Exporte vers ton logiciel de montage.

Raccourcis : Espace (lecture), P (aperçu monté), ← / → (coupe précédente / suivante), Ctrl + molette (zoom).

Les vidéos que l'aperçu ne sait pas lire (ProRes, HEVC 10 bits…) reçoivent un proxy 540p ; les exports
pointent toujours vers le fichier original.

## Mises à jour

Au démarrage, CrispTrim regarde s'il existe une version plus récente dans les
[Releases](https://github.com/xLaugh/CrispTrim/releases). Si oui, « Nouvelle version disponible » s'affiche
en haut à droite. Avec l'installateur, le bouton *Mettre à jour* fait tout. Depuis le code :

```bash
git pull
uv sync --extra asr
```

## Ligne de commande

```bash
# Silences + hésitations, exports FCPXML + XML Premiere + JSON à côté du fichier
uv run crisptrim rush.mp4

# Plusieurs fichiers, modèle plus rapide, exports au choix, dans un dossier de sortie
uv run crisptrim rush1.mp4 rush2.mov --model turbo --export fcpxml,premiere,vegas,mp4,srt -o montage/

# Silences seulement (sans transcription, très rapide)
uv run crisptrim rush.mp4 --no-fillers
```

Réglages utiles : `--min-silence 0.5` (s), `--margin 0.15` (s), `--threshold-db -40` (sinon automatique),
`--extra-filler ben` (couper aussi ce mot), `--keep-noises` (garder bâillements, toux, souffles).

## Développement

```bash
uv run --with pytest pytest
```

| Module | Rôle |
|---|---|
| `media.py` | ffprobe, décodage audio 16 kHz, proxy et vignettes |
| `silence.py` | zones sonores par énergie, seuil automatique |
| `transcribe.py` | CrisperWhisper 2.0, mots horodatés, cache |
| `fillers.py` | reconnaissance des hésitations françaises |
| `cutter.py` | segments gardés : parole + marges − hésitations, alignés sur les images |
| `export.py` | FCPXML 1.9, XML Premiere (xmeml v4), EDL texte Vegas Pro, MP4, JSON |
| `text.py` | texte et sous-titres recalés sur le montage |
| `updates.py` | vérification des nouvelles versions (Releases GitHub) |
| `cli.py` | ligne de commande |
| `app/server.py` | serveur local (FastAPI) + fenêtre native (pywebview / WebView2) |
| `app/static/` | interface : lecteur, timeline, coupes, transcription, export |
| `installer/launcher.py` | `CrispTrim-Setup.exe` : installation, lancement, mise à jour, désinstallation |
| `installer/build.py` | construit le `.exe` (PyInstaller) : `uv run --no-project --python 3.12 --with pyinstaller python installer/build.py` |

**Publier une version** : change `__version__` dans `crisptrim/__init__.py`, commit, puis
`git tag v0.2.0 && git push --tags`. L'action GitHub crée la release, y joint `CrispTrim-Setup.exe`,
et les applis installées la signalent.

## Feuille de route

- Export groupé de plusieurs rushes, `.exe` autonome
- Répétitions et faux départs (« je je je », phrases reprises)
- Modèle verbatim français maison, utilisable commercialement

## Crédits

Créé par **[xLaugh](https://github.com/xLaugh)**.

- [CrisperWhisper 2.0](https://huggingface.co/nyrahealth) par Nyra Health : transcription mot à mot
  (code MIT ; **poids du modèle sous licence non commerciale**, téléchargés au premier lancement).
- [Whisper](https://github.com/openai/whisper) par OpenAI (MIT)
- [FFmpeg](https://ffmpeg.org/), [FastAPI](https://fastapi.tiangolo.com/), [pywebview](https://pywebview.flowrl.com/)

## Licence

CrispTrim est distribué sous licence **GNU GPL v3** : voir [LICENSE](LICENSE).

Les poids de CrisperWhisper ne font pas partie de ce dépôt et ont leur propre licence, non commerciale :
une utilisation commerciale de CrispTrim demande un autre modèle de transcription.
