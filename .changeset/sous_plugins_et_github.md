---
default: minor
---

# Sous-plugins, et GitHub sous Git

**Sous-plugins.** Un plugin peut avoir des sous-plugins, affichés sous lui dans la fenêtre Plugins : un sous-plugin ne fonctionne que quand son parent est activé, et se désactive à part.

**GitHub.** Le premier, sous Git. Sa fenêtre Pull Request montre la pull request de la branche courante, la décision et le commentaire de chaque revue, et les commentaires des revues sur le code, par conversation ; une conversation ouvre son fichier à sa ligne. Il passe par la CLI GitHub (`gh`). Il apporte aussi les schémas des workflows GitHub Actions et de `dependabot.yml`.

**Schémas YAML.** Les schémas valent aussi pour les fichiers YAML : ceux qu'apporte un plugin, ou celui que nomme un commentaire `# yaml-language-server: $schema=…`. Valeurs vérifiées, clés et valeurs complétées, description sous la souris.
