# Installation

<DownloadButton />

## Windows

Téléchargez **`atelier-setup-windows-x86_64.exe`** depuis la [dernière release](https://github.com/ThomasFarineau/atelier-ide/releases/latest) et lancez-le. Il installe Atelier pour votre utilisateur seulement, sans droits administrateur, dans `%LOCALAPPDATA%\Programs\Atelier`, avec :

- un raccourci dans le menu Démarrer (et sur le bureau si vous cochez l'option) ;
- la commande `atelier` dans le PATH (voir [Ligne de commande](./command-line)) ;
- un désinstalleur dans les paramètres de Windows.

L'IDE est déjà dans l'installateur : rien n'est téléchargé au premier démarrage.

::: tip SmartScreen
L'installateur n'est pas encore signé. Windows peut afficher « Windows a protégé votre PC » : cliquez sur **Informations complémentaires › Exécuter quand même**.
:::

## Linux et macOS

Téléchargez une fois le lanceur de votre plateforme depuis la [dernière release](https://github.com/ThomasFarineau/atelier-ide/releases/latest) (`atelier-linux-x86_64`, `atelier-macos-aarch64` ou `atelier-macos-x86_64`), mettez-le dans un dossier à part et lancez-le. Il télécharge l'IDE à côté de lui, puis ajoute la commande `atelier` dans `~/.local/bin`.

Sur macOS, un fichier téléchargé par le navigateur est mis en quarantaine :

```bash
xattr -d com.apple.quarantine atelier-macos-aarch64
chmod +x atelier-macos-aarch64
```

## Mises à jour

On lance toujours le **lanceur** (`atelier`), jamais l'IDE lui-même : les raccourcis et la commande le font pour vous. Comme Discord ou VS Code, il affiche une petite fenêtre avec le logo, l'étape en cours et une barre de progression, et se ferme une fois l'IDE démarré. À chaque démarrage, il :

1. demande à GitHub la dernière release (4 secondes au plus ; sans réseau, il passe) ;
2. si elle est plus récente que `app/VERSION` (ou que l'IDE n'est pas encore là), télécharge l'archive de la plateforme et la vérifie avec `SHA256SUMS` ;
3. remplace le dossier `app/` d'un bloc, puis lance l'IDE. L'IDE n'étant pas démarré à ce moment-là, rien n'est jamais verrouillé sous Windows. Une archive incorrecte ne change rien : l'ancienne version démarre.

Le lanceur se met lui-même à jour avec chaque release. En cas d'échec, la fenêtre reste ouverte avec la raison et les boutons **Réessayer** et **Fermer**. Ce qu'a fait le dernier démarrage est dans `bootstrap.log`, à côté du lanceur.

| Variable | Effet |
| --- | --- |
| `ATELIER_NO_UPDATE=1` | Saute la recherche de mise à jour |
| `ATELIER_REPO=propriétaire/dépôt` | Prend les releases d'un autre dépôt |
| `ATELIER_GITHUB_TOKEN` | Donne accès à un dépôt privé |

## Compiler depuis les sources

Il faut Rust stable et une chaîne de compilation native. Sous Windows, installez les Build Tools de Visual Studio avec la charge de travail C++.

```bash
cargo run                         # lancer l'IDE
cargo run -- chemin/du/projet     # ouvrir directement un projet
cargo run -- examples/demo        # le projet de démonstration multilangage
cargo build --release             # target/release/atelier-ide(.exe)
```

## Où Atelier range ses données

La langue, le thème, la taille de police, les projets récents et les plugins activés sont enregistrés dans `settings.json`, dans le dossier de données d'Atelier :

| Système | Dossier |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Atelier` |
| macOS | `~/Library/Application Support/Atelier` |
| Linux | `$XDG_DATA_HOME/Atelier` (par défaut `~/.local/share/Atelier`) |

Les réglages d'un projet (configurations d'exécution, source des traductions, sessions d'IA, graphe de contexte) sont dans son dossier `.atelier/`.
