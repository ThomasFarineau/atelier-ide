import { defineConfig, type DefaultTheme } from 'vitepress'
import { execFileSync } from 'node:child_process'
import { loadCatalog, repoRoot, type Lang, type Section } from './catalog'

// The documentation of Atelier IDE, in English (root) and French (`/fr/`). Each page exists in
// both languages, at the same path. The pages of the built-in plugins and languages live in the
// source code, in the `doc` folder of each one (see catalog.ts).

const repo = 'https://github.com/atelier-ide-org/app'
const catalog = loadCatalog()

/** The pages of the plugins (or languages) that have one, by name. */
function pages(section: Section, prefix: string): DefaultTheme.SidebarItem[] {
  const lang: Lang = prefix ? 'fr' : 'en'
  return catalog
    .filter((p) => p.section === section && p.pages[lang])
    .map((p) => ({ text: p.name[lang], link: `${prefix}/${section}/${p.slug}` }))
    .sort((a, b) => a.text.localeCompare(b.text, lang))
}

/** "Edit this page" opens the file of the page, in the plugin for the pages of plugins. The
 * pattern is sent to the browser on its own: it cannot use `repo`. */
function editLink(text: string): DefaultTheme.EditLink {
  return {
    text,
    pattern: ({ filePath, params }) =>
      `https://github.com/atelier-ide-org/app/edit/main/${params?.source ?? `docs/${filePath}`}`,
  }
}

function sidebar(prefix: string, t: Record<string, string>): DefaultTheme.SidebarItem[] {
  return [
    // The catalog first, as a large link (styled by `.sidebar-catalog` in theme/style.css).
    {
      text: `<span class="sidebar-catalog"><span class="title">${t.catalog}</span><span class="hint">${t.catalogHint}</span></span>`,
      link: `${prefix}/plugins/`,
    },
    {
      text: t.guide,
      items: [
        { text: t.install, link: `${prefix}/guide/getting-started` },
        { text: t.interface, link: `${prefix}/guide/interface` },
        { text: t.run, link: `${prefix}/guide/running` },
        { text: t.shortcuts, link: `${prefix}/guide/shortcuts` },
        { text: t.cli, link: `${prefix}/guide/command-line` },
      ],
    },
    {
      text: t.languages,
      items: [
        ...pages('languages', prefix),
        { text: t.schemas, link: `${prefix}/languages/json` },
        { text: t.format, link: `${prefix}/languages/formatting` },
      ],
    },
    {
      text: 'Plugins',
      collapsed: false,
      items: pages('plugins', prefix),
    },
    {
      text: t.ai,
      items: [
        { text: t.mcp, link: `${prefix}/plugins/mcp` },
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
  schemas: 'JSON & YAML schemas',
  catalog: 'Plugin catalog',
  catalogHint: 'Every built-in plugin and language',
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
  schemas: 'Schémas JSON et YAML',
  catalog: 'Catalogue des plugins',
  catalogHint: 'Tous les plugins et langages intégrés',
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
  base: '/app/',
  cleanUrls: true,
  lastUpdated: true,
  // The date of a page of a plugin is the one of its file in the plugin, not of the route file.
  transformPageData(page) {
    const source = page.params?.source
    if (!source) return
    try {
      const out = execFileSync('git', ['log', '-1', '--format=%at', '--', source], { cwd: repoRoot })
      const seconds = Number(out.toString().trim())
      if (seconds) page.lastUpdated = seconds * 1000
    } catch {}
  },
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: '/app/logo.svg' }]],
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
          { text: 'Guide', link: '/guide/getting-started', activeMatch: '^/guide/' },
          { text: 'Plugins', link: '/plugins/', activeMatch: '^/(plugins|languages)/' },
          { text: 'Download', link: `${repo}/releases/latest` },
          { text: 'Changelog', link: `${repo}/blob/main/CHANGELOG.md` },
        ],
        sidebar: sidebar('', en),
        editLink: editLink('Edit this page on GitHub'),
      },
    },
    fr: {
      label: 'Français',
      lang: 'fr',
      link: '/fr/',
      description: 'Un IDE de bureau moderne et rapide, écrit en Rust.',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/fr/guide/getting-started', activeMatch: '^/fr/guide/' },
          { text: 'Plugins', link: '/fr/plugins/', activeMatch: '^/fr/(plugins|languages)/' },
          { text: 'Télécharger', link: `${repo}/releases/latest` },
          { text: 'Changelog', link: `${repo}/blob/main/CHANGELOG.md` },
        ],
        sidebar: sidebar('/fr', fr),
        editLink: editLink('Modifier cette page sur GitHub'),
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
