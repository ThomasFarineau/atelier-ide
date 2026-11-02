// The names and colors of the categories of the catalog of plugins. A card gives its category in
// `category` (see ../catalog.ts); languages are their own list.
import type { Category } from '../catalog'

export const categories: Record<Category, { en: string; fr: string; color: [string, string] }> = {
  vcs: { en: 'Version control', fr: 'Gestion de versions', color: ['#f97316', '#ef4444'] },
  build: { en: 'Build & packages', fr: 'Build et paquets', color: ['#0ea5e9', '#2563eb'] },
  viewers: { en: 'Viewers', fr: 'Visionneuses', color: ['#ec4899', '#d946ef'] },
  data: { en: 'Data & infrastructure', fr: 'Données et infrastructure', color: ['#10b981', '#0d9488'] },
  ai: { en: 'AI', fr: 'IA', color: ['#f72be2', '#047cff'] },
  tools: { en: 'Tools', fr: 'Outils', color: ['#eab308', '#f59e0b'] },
  languages: { en: 'Languages', fr: 'Langages', color: ['#8b5cf6', '#6d3df5'] },
}
