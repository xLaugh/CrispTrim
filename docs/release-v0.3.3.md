## CrispTrim 0.3.3 : correctif pour ffmpeg installé par WinGet

- Si ffmpeg a été installé avec WinGet, CrispTrim pouvait afficher « ffprobe : Le chemin d'accès ne peut
  pas être parcouru car il contient un point de montage non approuvé (os error 448) » à l'ouverture d'un
  rush. CrispTrim lance maintenant directement le vrai ffmpeg, et l'erreur disparaît.

Prends **`CrispTrim-Setup.exe`** (installateur) ou **`CrispTrim.exe`** (portable). Si tu as déjà
CrispTrim, « Nouvelle version disponible » > *Mettre à jour* suffit.
