# CrispTrim — fiche des règles (réécriture Rust)

Source : le code de l'auteur (github.com/xLaugh/CrispTrim, version 0.1.1, identique à la version installée
dans `%LOCALAPPDATA%\CrispTrim\app`), et la décompilation de `CrispTrim.exe` ci-dessous.

## Décompilation de CrispTrim.exe (Ghidra)
- **0x140004c60** (`FUN_140004c60`, ev_2c62e931…) : démarrage du chargeur PyInstaller. Ouvre l'archive PKG
  embarquée, lit les options `pyi-contents-directory`, `pyi-runtime-tmpdir`, `pyi-disable-windowed-traceback`,
  gère les variables `_PYI_ARCHIVE_FILE`, `_PYI_PARENT_PROCESS_LEVEL`, `_PYI_APPLICATION_HOME_DIR`,
  `PYINSTALLER_RESET_ENVIRONMENT`, précharge VCRUNTIME140.dll et VCRUNTIME140_1.dll, règle le dossier des DLL.
  Aucune règle de coupe.
- **0x140004470** (`FUN_140004470`, ev_bac6ede4…) : pour chaque entrée « script » de l'archive, construit
  `<home>\<nom>.py`, désérialise le code et l'exécute dans `__main__`. Le seul script est `launcher`
  (Python 3.12, ev_e70404ae…).
- Recherche de chaînes « silence|fcpxml|premiere|vegas|threshold|seuil|whisper|euh » dans l'exe : **0 résultat**
  (ev_c7e9b35c…). Le code natif ne contient donc rien sur les coupes : toutes les règles ci-dessous viennent
  du code Python que le lanceur exécute.
- Script `launcher` extrait (ev_da5d40da…) et lu (ev_d53ddc33…, 82 objets de code) : mêmes noms et constantes
  que `installer/launcher.py` du dépôt.

## Ressources Windows de CrispTrim.exe (ev_f8bf7aea…)
| Valeur | Original | Rust |
|---|---|---|
| Icône (RT_GROUP_ICON 1) | 9 tailles : 16, 20, 24, 32, 40, 48, 64, 128, 256 px ; l'image 256 px (RT_ICON 9, 17 163 octets) a le SHA-256 6d681b5e… | `assets/crisptrim.ico` (mêmes 9 tailles) et `assets/crisptrim.png` : **même SHA-256** que RT_ICON 9 |
| Icône de la fenêtre / barre des tâches | `server.py set_icon` (crisptrim.ico) + `SetCurrentProcessExplicitAppUserModelID("xLaugh.CrispTrim")` | `ui::run` (IconData du PNG) + `set_app_id` |
| Version (RT_VERSION 1) | CompanyName xLaugh, LegalCopyright « xLaugh, GNU GPL v3 », ProductName CrispTrim | `build.rs` (mêmes champs, version du paquet) |
| Manifeste (RT_MANIFEST 1) | asInvoker, longPathAware, Common-Controls 6.0 | `build.rs` (copie du manifeste) |
| Chaînes d'API d'icône dans le code natif | aucune (LoadIcon, AppUserModel… : 0 résultat, ev_c53c6dfe…) : l'icône vient des ressources, pas du code | — |

## Lanceur — script `launcher` (dans CrispTrim.exe)
| Valeur | Nombre | Source | Utilisé en Rust |
|---|---|---|---|
| Dossier de données | `CRISPTRIM_HOME`, sinon dossier de l'exe s'il contient `install.json`, sinon `%LOCALAPPDATA%\CrispTrim` | `Paths.__init__` (ligne 54) | `data_dir()` |
| FFmpeg de CrispTrim | `<dossier>\bin` en tête du PATH | `tool_env` (ligne 124) | `media::tool` |
| Modèle | `nyralabs/CrisperWhisper2.0_large` dans `<dossier>\models` (`HF_HOME`) | `MODEL_REPO`, `Job.get_model` | pour la future transcription Rust |
| Pas de console | `CREATE_NO_WINDOW` (0x08000000) | `NO_WINDOW` | `media::tool` |
| Port du serveur | 8765 | `PORT` | sans objet (plus de serveur web en Rust) |
| Installation | uv 0.13.5, FFmpeg 9.0.2, Python 3.12, 12 Go libres | `UV_VERSION`, `FFMPEG_VERSION`, `NEED_FREE` | pas encore refait en Rust |
| Tags de release | `^v\d+\.\d+(\.\d+)?$` ; un autre tag est refusé par `--update` | `TAG_RE`, `main` (ligne 917) | `updates::is_release_tag` + contrôle dans `release.yml` |
| Mise à jour en un clic | télécharge `github.com/xLaugh/CrispTrim/archive/refs/tags/<tag>.zip`, exige `pyproject.toml` dans l'archive sinon « Archive du code invalide. », garde l'ancienne version en cas d'échec | `Job.put_code` (ligne 370), `Update.work` (ligne 792), ev_d2d3c75a… | la release Rust n'a pas de `pyproject.toml` : les installations Python refusent proprement la v0.2.0 et restent en 0.1.1 |
| Couleurs du lanceur | fond #0b0d12, accent #7c6cff, ok #3ddc97, danger #ff5d6c | constantes `BG`, `ACCENT`, `OK`, `DANGER` | `theme.rs` (mêmes valeurs) |

## Silences — `silence.py`
| Valeur | Nombre | Source |
|---|---|---|
| Fréquence d'analyse | 16 000 Hz mono | `media.SAMPLE_RATE` |
| Tranche d'énergie | 10 ms (160 échantillons) | `silence.FRAME`, `frame_db` |
| dB d'une tranche | 20·log10(rms + 1e-10) | `frame_db` |
| Seuil auto | plancher = max(P10, −80) ; pic = P95 ; seuil = plancher + 0,3·(pic − plancher), puis min(max(seuil, −60), pic − 6) | `auto_threshold` |
| Seuil si aucune tranche | −45 dB | `auto_threshold` |
| Silence minimum (pause gardée si plus courte, sauf au début/fin) | 0,5 s | `SilenceParams.min_silence` |
| Son minimum (clics ignorés) | 0,08 s | `SilenceParams.min_sound` |

## Coupes — `cutter.py`, `intervals.py`
| Valeur | Nombre | Source |
|---|---|---|
| Marge autour de la parole | 0,15 s | `CutParams.margin` |
| Segment gardé minimal | 0,25 s | `CutParams.min_keep` |
| Marge autour d'un « euh » collé | 0,03 s | `CutParams.filler_pad` |
| Débordement max d'une coupe d'hésitation | 1,0 s | `CutParams.max_filler_extend` |
| Coupe d'hésitation | gauche = fin du mot précédent + marge si < début du « euh », sinon max(fin préc., début − 0,03) ; droite symétrique ; bornée à ±1 s | `filler_cuts` |
| Gardé | (parole + marge) − coupes d'hésitation, fusion, puis segments ≥ 0,25 s | `compute_cuts` |
| Région « hésitation » | trou qui chevauche une coupe d'hésitation, sinon « silence » | `compute_cuts` |
| Alignement images | a = round(s·fps), b = round(e·fps), fusion si chevauchement (arrondi « pair » de Python) | `to_frames` |
| « Couper les silences » décoché | parole = toute la durée | `server.recut` |

## Hésitations — `fillers.py`
- Marqueurs : `[uh] [um] [uhm] [hm] [mm]`.
- Expression : `e+u+h+ | h+e+u+h* | e+u+h*m+ | h+u+m+ | h+m+ | m+h+ | m{2,} | u+h+ | u+m+` (mot entier, minuscules, ponctuation retirée).
- Bruits : tout `[xxx]` sauf marqueurs et `[laughter] [laugh] [laughs] [music] [applause] [singing]`.

## Transcription — `transcribe.py`
- Cache : `<cache>/<nom>.<sha1("chemin|taille|mtime_ns|modèle|langue")[:16]>.words.json`, cache = `%LOCALAPPDATA%\CrispTrim\cache`.
- Modèles : large (défaut), turbo, medium, small ; langue fr.
- **Rust V1** : relit ce cache (et le `_crisptrim.json` du CLI à côté du rush). Pas encore de modèle en Rust (inconnu unk_20802f02986104cb).

## Lecture / timeline — `static/app.js`
| Valeur | Nombre | Source |
|---|---|---|
| Hauteur de la règle | 20 px | `RULER` |
| Zoom max | 600 px/s | `clampView` |
| Pas de zoom boutons | ×1,6 | `#btn-zoom-in` |
| Pas de zoom molette | ×1,25 / ×0,8 | `wheel` |
| Plancher forme d'onde | 70 dB | `draw` (`FLOOR`) |
| Pas de graduation | 1er de [0,1 … 1800] tel que pas·pps ≥ 80 px | `draw` |
| Écoute d'une coupe | de début − 0,7 s à fin + 0,7 s | `listen` |
| Coupe suivante/précédente | coupe à plus de 0,6 s, se place 0,5 s avant | `jumpCut` |
| Aperçu monté | saute une coupe active si t < fin − 0,02 s | `activeCutAt` |
| Suivi de lecture | recadre si t > début + 92 % de la vue, à 10 % | `tick` |
| Pause affichée dans le texte | écart > 1 s | `renderTranscript` |
| Délai de recalcul des réglages | 180 ms | `onSettings` |
| Curseurs | seuil −70…−15 (pas 1), silence 0,2…2 (0,05), marge 0…0,5 (0,01) | `index.html` |

## Texte et sous-titres — `text.py`
- Paragraphe : écart > 1,5 s. Sous-titre : 42 car./ligne, 2 lignes, ≤ 5 s, coupure sur pause > 0,8 s (dans le montage) ; coupure naturelle après pause > 0,3 s ou virgule ; durée min 0,7 s ; 40 ms avant le suivant.

## Exports — `export.py`
- FCPXML 1.9, xmeml v4 (Premiere), EDL texte Vegas (ms, CRLF, UTF-8 BOM), MP4 (NVENC p5 cq 19 sinon x264 medium crf 18, AAC 192k), JSON.
- Cadences standard : ramenées à la plus proche si écart < 1 %.

## Mises à jour — `updates.py`
- API GitHub `releases/latest`, cache 6 h (`TTL`), délai 5 s.

## Fenêtre — `server.py`
- 1440×900, minimum 1000×640, fond #0e1014.
