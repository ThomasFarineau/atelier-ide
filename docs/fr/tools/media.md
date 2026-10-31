# Lecteur multimédia

Le plugin intégré **Lecteur multimédia** ouvre la musique et les vidéos dans un lecteur, à la place de l'éditeur de texte.

| Type | Fichiers | Décodés par |
| --- | --- | --- |
| Musique | `.mp3`, `.wav`, `.flac`, `.ogg`, `.m4a` | l'IDE lui-même |
| Autres sons | `.opus`, `.aac`, `.oga` (et toute musique que l'IDE ne sait pas lire) | ffmpeg |
| Vidéos | `.mp4`, `.mkv`, `.avi`, `.mov`, `.webm`, `.m4v`, `.wmv`, `.flv`, `.mpg` | ffmpeg |

## Lecture

- Un fichier s'ouvre en pause. **▶** ou un clic sur l'image lance ou met en pause la lecture, et la barre en dessous affiche le temps et permet de se déplacer dans le fichier.
- Quand le pointeur est sur le lecteur : **Espace** lance ou met en pause, **←** et **→** reculent ou avancent de 5 secondes.
- Le volume et le bouton pour couper le son sont communs à tous les fichiers.
- Un fichier se met en pause quand son onglet n'est plus affiché, et s'arrête quand l'onglet est fermé.
- Les vidéos sont affichées au plus en 1280 × 720 et à 60 images par seconde, mises à l'échelle de l'éditeur. Une vidéo filmée à la verticale avec un téléphone s'affiche droite. Sans périphérique audio, une vidéo est lue sans le son.

## ffmpeg

Les vidéos ont besoin de `ffmpeg` et `ffprobe`. L'IDE utilise ceux du `PATH`. S'il n'y en a pas, le lecteur propose de **télécharger ffmpeg** : le build statique LGPL de [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (environ 170 Mo), vérifié par sa somme SHA-256 et installé dans `~/.atelier/tools/ffmpeg`, où seuls `ffmpeg` et `ffprobe` sont gardés. Le téléchargement est proposé sous Windows (x64) et Linux (x64, ARM64). Sous macOS, installez-le avec `brew install ffmpeg`, puis cliquez sur **Chercher à nouveau**.
