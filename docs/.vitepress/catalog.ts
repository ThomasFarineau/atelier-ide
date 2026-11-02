// The built-in plugins of the IDE, read from the `doc` folder of each one, so that the docs
// follow the code:
//
//   src/plugins/builtins/<plugin>/doc/   (or src/languages/<language>/doc/)
//     en-US.md   the page of the plugin, at /plugins/<plugin> (or /languages/<language>)
//     fr-FR.md   its French page, at /fr/plugins/<plugin>
//
// The front matter of a page is the card of the plugin: `name` and `summary` in each language
// (the IDE shows them in its Plugins page too, see build.rs), and in en-US.md only `category`,
// `icon`, `tags` and `see`. A file with only a front matter is a card without a page; `see` then
// sends the card to the page of another plugin (`git#github`).
//
// A plugin folder inside another one is a sub-plugin (git/github): it has no card of its own and
// is listed in the card of its parent.
//
// Used by the config (sidebar), the routes of the pages and the data of the catalog. Any
// mistake in a card stops the build, with the file at fault.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { parse } from 'yaml'

export const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')

export type Section = 'plugins' | 'languages'
export type Lang = 'en' | 'fr'

export const categories = ['vcs', 'build', 'viewers', 'data', 'ai', 'tools'] as const
export type Category = (typeof categories)[number] | 'languages'

export interface CatalogPlugin {
  /** The name of its folder, and of its page. */
  slug: string
  section: Section
  /** The slug of the plugin it is part of. */
  parent?: string
  category: Category
  name: Record<Lang, string>
  summary: Record<Lang, string>
  tags: string[]
  /** The SVG of the icon as data URLs, `dark` when the plugin has a dark variant. */
  icon?: { light: string; dark?: string }
  /** Where its page is, from the root of the docs (`plugins/cargo`), with an anchor for `see`. */
  link?: string
  /** The Markdown files of its pages, from the root of the repository. */
  pages: Partial<Record<Lang, string>>
}

const roots: { dir: string; section: Section }[] = [
  { dir: 'src/plugins/builtins', section: 'plugins' },
  { dir: 'src/languages', section: 'languages' },
]

const files: Record<Lang, string> = { en: 'en-US.md', fr: 'fr-FR.md' }

/** Globs of every file the catalog is read from, for the watchers of `vitepress dev`. */
export const watched = roots.map((r) => `${repoRoot}/${r.dir}/**/doc/*.md`.replace(/\\/g, '/'))

/** Every `doc` folder under `dir`, with the plugin folders that hold it, outermost first. */
function docFolders(dir: string, chain: string[] = []): { folder: string; chain: string[] }[] {
  const out: { folder: string; chain: string[] }[] = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    const sub = path.join(dir, entry.name)
    if (entry.name === 'doc') {
      out.push({ folder: dir, chain })
    } else {
      const own = fs.existsSync(path.join(sub, 'doc')) ? [...chain, entry.name] : chain
      out.push(...docFolders(sub, own))
    }
  }
  return out
}

function svg(file: string) {
  return `data:image/svg+xml;base64,${fs.readFileSync(file).toString('base64')}`
}

function fail(file: string, message: string): never {
  throw new Error(`${path.relative(repoRoot, file)}: ${message}`)
}

/** The front matter (the card) and the body (the page, empty without one) of a file. */
function read(file: string): { card: Record<string, any>; body: string } {
  if (!fs.existsSync(file)) fail(file, 'missing')
  const text = fs.readFileSync(file, 'utf8').replace(/\r\n/g, '\n')
  const match = text.match(/^---\n([\s\S]*?)\n---(?:\n|$)/)
  if (!match) fail(file, 'no front matter')
  const card = parse(match[1])
  if (!card || typeof card !== 'object') fail(file, 'the front matter is not a YAML mapping')
  for (const key of ['name', 'summary']) {
    if (typeof card[key] !== 'string' || !card[key].trim()) fail(file, `"${key}" is required`)
  }
  return { card, body: text.slice(match[0].length).trim() }
}

const relative = (file: string) => path.relative(repoRoot, file).replace(/\\/g, '/')

export function loadCatalog(): CatalogPlugin[] {
  const plugins: CatalogPlugin[] = []
  for (const root of roots) {
    for (const { folder, chain } of docFolders(path.join(repoRoot, root.dir))) {
      const doc = path.join(folder, 'doc')
      const enFile = path.join(doc, files.en)
      const frFile = path.join(doc, files.fr)
      const { card: en, body: enBody } = read(enFile)
      const { card: fr, body: frBody } = read(frFile)
      const slug = chain[chain.length - 1]

      const category = (root.section === 'languages' ? 'languages' : en.category) as Category
      if (root.section === 'plugins' && !categories.includes(category as any)) {
        fail(enFile, `"category" must be one of ${categories.join(', ')}`)
      }

      let icon: CatalogPlugin['icon']
      const iconFile = path.resolve(folder, typeof en.icon === 'string' ? en.icon : 'icon.svg')
      if (fs.existsSync(iconFile)) {
        const dark = iconFile.replace(/\.svg$/, '.dark.svg')
        icon = { light: svg(iconFile), dark: fs.existsSync(dark) ? svg(dark) : undefined }
      } else if (en.icon) {
        fail(enFile, `the icon ${en.icon} does not exist`)
      }

      if (!enBody !== !frBody) fail(doc, 'a page in one language only')
      const pages: CatalogPlugin['pages'] = enBody ? { en: relative(enFile), fr: relative(frFile) } : {}

      const tags = (v: unknown) => (Array.isArray(v) ? v.map(String) : [])
      plugins.push({
        slug,
        section: root.section,
        parent: chain.length > 1 ? chain[chain.length - 2] : undefined,
        category,
        name: { en: en.name, fr: fr.name },
        summary: { en: en.summary.trim(), fr: fr.summary.trim() },
        tags: [...tags(en.tags), ...tags(fr.tags)],
        icon,
        link: enBody
          ? `${root.section}/${slug}`
          : typeof en.see === 'string'
            ? `${root.section}/${en.see}`
            : undefined,
        pages,
      })
    }
  }

  const slugs = new Set<string>()
  for (const p of plugins) {
    if (slugs.has(p.slug)) throw new Error(`two plugins have the folder name ${p.slug}`)
    slugs.add(p.slug)
  }
  return plugins.sort((a, b) => a.name.en.localeCompare(b.name.en))
}

/** The routes of the pages of one section in one language, for a `[name].paths.ts` file: the
 * body of the page, without its front matter (the card). */
export function pageRoutes(section: Section, lang: Lang, param: string) {
  return loadCatalog()
    .filter((p) => p.section === section && p.pages[lang])
    .map((p) => ({
      params: { [param]: p.slug, source: p.pages[lang] },
      content: read(path.join(repoRoot, p.pages[lang]!)).body,
    }))
}
