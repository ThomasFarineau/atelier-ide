// The catalog of the built-in plugins for the pages, read at build time from the doc folders of
// the plugins (see ../catalog.ts). The paths of their Markdown files stay out of the page.
import { loadCatalog, watched, type CatalogPlugin } from '../catalog'

export type Plugin = Omit<CatalogPlugin, 'pages'>

declare const data: Plugin[]
export { data }

export default {
  watch: watched,
  load: (): Plugin[] => loadCatalog().map(({ pages, ...plugin }) => plugin),
}
