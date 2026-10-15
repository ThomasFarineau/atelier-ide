import { defineConfig, type DefaultTheme } from 'vitepress'

// The documentation of Atelier IDE, in English (root) and French (`/fr/`). Each page exists in
// both languages, at the same path.

const repo = 'https://github.com/ThomasFarineau/atelier-ide'

function sidebar(prefix: string, t: Record<string, string>): DefaultTheme.SidebarItem[] {
  return [
    {
      text: t.guide,
      items: [
        { text: t.install, link: `${prefix}/guide/getting-started` },
        { text: t.interface, link: `${prefix}/guide/interface` },
        { text: t.run, link: `${prefix}/guide/running` },
        { text: 'Git', link: `${prefix}/guide/git` },
        { text: t.shortcuts, link: `${prefix}/guide/shortcuts` },
        { text: t.cli, link: `${prefix}/guide/command-line` },
      ],
    },
    {
      text: t.languages,
      items: [
        { text: 'JavaScript & TypeScript', link: `${prefix}/languages/javascript` },
        { text: 'Rust', link: `${prefix}/languages/rust` },
        { text: 'Java', link: `${prefix}/languages/java` },
        { text: t.format, link: `${prefix}/languages/formatting` },
      ],
    },
    {
      text: t.tools,
      items: [
        { text: 'Node.js', link: `${prefix}/tools/node` },
        { text: 'Runtimes & Jupyter', link: `${prefix}/tools/runtimes` },
        { text: 'Maven & Gradle', link: `${prefix}/tools/maven-gradle` },
        { text: t.translations, link: `${prefix}/tools/translations` },
        { text: t.viewers, link: `${prefix}/tools/viewers` },
        { text: t.media, link: `${prefix}/tools/media` },
        { text: 'Plugins', link: `${prefix}/tools/plugins` },
      ],
    },
    {
      text: t.ai,
      items: [
        { text: t.mcp, link: `${prefix}/ai/mcp` },
        { text: t.graph, link: `${prefix}/ai/context-graph` },
        { text: t.commits, link: `${prefix}/ai/commits` },
      ],
    },
    {
      text: t.contributing,
      items: [
        { text: t.development, link: `${prefix}/contributing/development` },
        { text: t.releases, link: `${prefix}/contributing/releases` },
      ],
    },
  ]
}

const en = {
  guide: 'Guide',
  install: 'Installation',
  interface: 'Interface',
  run: 'Running code',
  shortcuts: 'Keyboard shortcuts',
  cli: 'Command line',
  languages: 'Languages',
  format: 'Reformat code',
  tools: 'Tools',
  translations: 'Translations (i18n)',
  viewers: 'File viewers',
  media: 'Media player',
  ai: 'AI',
  mcp: 'MCP server & sessions',
  graph: 'Context graph',
  commits: 'Commits by AI',
  contributing: 'Contributing',
  development: 'Development',
  releases: 'Versions & releases',
}

const fr = {
  guide: 'Guide',
  install: 'Installation',
  interface: 'Interface',
  run: 'Exécuter du code',
  shortcuts: 'Raccourcis clavier',
  cli: 'Ligne de commande',
  languages: 'Langages',
  format: 'Reformater le code',
  tools: 'Outils',
  translations: 'Traductions (i18n)',
  viewers: 'Visionneuses',
  media: 'Lecteur multimédia',
  ai: 'IA',
  mcp: 'Serveur MCP et sessions',
  graph: 'Graphe de contexte',
  commits: 'Commits par l’IA',
  contributing: 'Contribuer',
  development: 'Développement',
  releases: 'Versions et releases',
}

export default defineConfig({
  title: 'Atelier IDE',
  base: '/atelier-ide/',
  cleanUrls: true,
  lastUpdated: true,
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: '/atelier-ide/logo.svg' }]],
  themeConfig: {
    logo: '/logo.svg',
    socialLinks: [{ icon: 'github', link: repo }],
    footer: {
      message: `Released under the <a href="${repo}/blob/main/LICENSE">MIT License</a>.`,
      copyright: 'Copyright © 2026 Thomas Farineau',
    },
    search: {
      provider: 'local',
      options: {
        locales: {
          fr: {
            translations: {
              button: { buttonText: 'Rechercher', buttonAriaLabel: 'Rechercher' },
              modal: {
                noResultsText: 'Aucun résultat pour',
                resetButtonTitle: 'Effacer la recherche',
                footer: { selectText: 'choisir', navigateText: 'naviguer', closeText: 'fermer' },
              },
            },
          },
        },
      },
    },
  },
  locales: {
    root: {
      label: 'English',
      lang: 'en',
      description: 'A fast, modern desktop IDE written in Rust.',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/guide/getting-started' },
          { text: 'Download', link: `${repo}/releases/latest` },
          { text: 'Changelog', link: `${repo}/blob/main/CHANGELOG.md` },
        ],
        sidebar: sidebar('', en),
        editLink: { pattern: `${repo}/edit/main/docs/:path`, text: 'Edit this page on GitHub' },
      },
    },
    fr: {
      label: 'Français',
      lang: 'fr',
      link: '/fr/',
      description: 'Un IDE de bureau moderne et rapide, écrit en Rust.',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/fr/guide/getting-started' },
          { text: 'Télécharger', link: `${repo}/releases/latest` },
          { text: 'Changelog', link: `${repo}/blob/main/CHANGELOG.md` },
        ],
        sidebar: sidebar('/fr', fr),
        editLink: { pattern: `${repo}/edit/main/docs/:path`, text: 'Modifier cette page sur GitHub' },
        outline: { label: 'Sur cette page' },
        footer: {
          message: `Publié sous <a href="${repo}/blob/main/LICENSE">licence MIT</a>.`,
          copyright: 'Copyright © 2026 Thomas Farineau',
        },
        docFooter: { prev: 'Page précédente', next: 'Page suivante' },
        lastUpdated: { text: 'Mis à jour le' },
        returnToTopLabel: 'Retour en haut',
        sidebarMenuLabel: 'Menu',
        darkModeSwitchLabel: 'Apparence',
        lightModeSwitchTitle: 'Passer au thème clair',
        darkModeSwitchTitle: 'Passer au thème sombre',
        langMenuLabel: 'Changer de langue',
      },
    },
  },
})
