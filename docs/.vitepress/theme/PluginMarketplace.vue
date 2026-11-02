<script setup lang="ts">
// The built-in plugins as a marketplace, from their doc folders (see ../catalog.ts): a search
// bar, then the plugins (filtered by category) and the languages in two lists, a card per plugin
// with its logo and a link to its page. The search ignores case and accents, and every
// word typed must be found in the name, the description, the category or the tags.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useData, withBase } from 'vitepress'
import { data as plugins, type Plugin } from './catalog.data'
import { categories } from './categories'
import type { Category } from '../catalog'

const { lang } = useData()
const l = computed<'en' | 'fr'>(() => (lang.value.startsWith('fr') ? 'fr' : 'en'))
const prefix = computed(() => (l.value === 'fr' ? '/fr' : ''))

const t = computed(() =>
  l.value === 'fr'
    ? {
        search: 'Rechercher un plugin, une extension, un outil…',
        all: 'Tous',
        languages: 'Langages',
        plugins: 'Plugins',
        builtin: 'Intégré',
        enabled: 'Activé par défaut',
        subs: 'Sous-plugins',
        noneHere: 'Aucun plugin dans cette catégorie pour cette recherche.',
        docs: 'Documentation',
        empty: 'Aucun plugin ne correspond à',
        reset: 'Effacer la recherche',
        clear: 'Effacer',
      }
    : {
        search: 'Search a plugin, a file extension, a tool…',
        all: 'All',
        languages: 'Languages',
        plugins: 'Plugins',
        builtin: 'Built-in',
        enabled: 'Enabled by default',
        subs: 'Sub-plugins',
        noneHere: 'No plugin in this category for this search.',
        docs: 'Documentation',
        empty: 'No plugin matches',
        reset: 'Clear the search',
        clear: 'Clear',
      },
)

const query = ref('')
const category = ref<Category | null>(null)
const input = ref<HTMLInputElement | null>(null)

const fold = (text: string) =>
  text.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase()

// Sub-plugins are not cards of their own: they are listed in the card of their parent, and
// what they do is found by the search of their parent.
const subs = (p: Plugin) => plugins.filter((s) => s.parent === p.slug)
const top = plugins.filter((p) => !p.parent)

const text = (p: Plugin) =>
  [p.name.en, p.name.fr, p.summary[l.value], categories[p.category][l.value], ...p.tags, p.slug].join(' ')

const haystacks = computed(() =>
  top.map((p) => fold([text(p), ...subs(p).map(text)].join(' '))),
)

const matching = computed(() => {
  const words = fold(query.value).split(/\s+/).filter(Boolean)
  return top.filter((_, i) => words.every((w) => haystacks.value[i].includes(w)))
})

// Languages and plugins are two lists; the categories only filter the plugins.
const languages = computed(() => matching.value.filter((p) => p.category === 'languages'))
const others = computed(() => matching.value.filter((p) => p.category !== 'languages'))
const filtered = computed(() =>
  category.value ? others.value.filter((p) => p.category === category.value) : others.value,
)

const pluginCategories = (Object.keys(categories) as Category[]).filter((c) => c !== 'languages')

// The count of each category follows the search, so that a chip at 0 says where not to look.
const counts = computed(() => {
  const c = {} as Record<Category, number>
  for (const key of pluginCategories) c[key] = 0
  for (const p of others.value) c[p.category]++
  return c
})

const href = (link?: string) => (link ? withBase(`${prefix.value}/${link}`) : undefined)

const sections = computed(() => [
  { key: 'plugins', title: t.value.plugins, items: filtered.value, chips: true },
  { key: 'languages', title: t.value.languages, items: languages.value, chips: false },
])

const initials = (name: string) => {
  const words = name.replace(/[^\p{L}\p{N} ]/gu, ' ').split(/\s+/).filter(Boolean)
  return words.length > 1 ? (words[0][0] + words[1][0]).toUpperCase() : name.slice(0, 2)
}

const tile = (c: Category) => {
  const [a, b] = categories[c].color
  return { background: `linear-gradient(135deg, ${a}, ${b})` }
}

const reset = () => {
  query.value = ''
  category.value = null
  input.value?.focus()
}

// `/` focuses the search, as on most marketplaces, unless the visitor is typing elsewhere.
const onKey = (e: KeyboardEvent) => {
  const target = e.target as HTMLElement
  if (e.key !== '/' || target.closest('input, textarea, [contenteditable="true"]')) return
  e.preventDefault()
  input.value?.focus()
}
onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <section class="marketplace">
    <div class="search">
      <svg class="search-icon" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="11" cy="11" r="7" />
        <path d="m20 20-3.5-3.5" />
      </svg>
      <input
        ref="input"
        v-model="query"
        type="search"
        :placeholder="t.search"
        :aria-label="t.search"
        autocomplete="off"
        spellcheck="false"
      />
      <button v-if="query" class="clear" type="button" :aria-label="t.clear" @click="reset">×</button>
      <kbd v-else class="hint">/</kbd>
    </div>

    <template v-for="s in sections" :key="s.key">
      <div v-if="s.chips ? others.length : s.items.length" class="section">
        <div class="section-head">
          <span class="section-title">{{ s.title }}</span>
          <span class="section-count" aria-live="polite">{{ s.items.length }}</span>
        </div>

        <div v-if="s.chips" class="chips" role="toolbar">
          <button type="button" class="chip" :class="{ active: !category }" @click="category = null">
            {{ t.all }} <span class="count">{{ others.length }}</span>
          </button>
          <button
            v-for="key in pluginCategories"
            :key="key"
            type="button"
            class="chip"
            :class="{ active: category === key, empty: !counts[key] }"
            @click="category = category === key ? null : key"
          >
            <span class="dot" :style="tile(key)" />
            {{ categories[key][l] }} <span class="count">{{ counts[key] }}</span>
          </button>
        </div>

        <div v-if="s.items.length" class="grid">
          <div v-for="p in s.items" :key="p.slug" class="card" :class="{ linked: p.link }">
            <div class="head">
              <div v-if="p.icon" class="icon logo" :class="{ themed: p.icon.dark }">
                <img class="light" :src="p.icon.light" alt="" />
                <img v-if="p.icon.dark" class="dark" :src="p.icon.dark" alt="" />
              </div>
              <div v-else class="icon" :style="tile(p.category)">{{ initials(p.name[l]) }}</div>
              <div class="title">
                <a v-if="p.link" class="name stretched" :href="href(p.link)">{{ p.name[l] }}</a>
                <span v-else class="name">{{ p.name[l] }}</span>
                <span class="category">{{ categories[p.category][l] }}</span>
              </div>
            </div>
            <p class="description">{{ p.summary[l] }}</p>
            <div v-if="subs(p).length" class="subs">
              <span class="subs-label">{{ t.subs }}</span>
              <component
                :is="sub.link ? 'a' : 'span'"
                v-for="sub in subs(p)"
                :key="sub.slug"
                class="sub"
                :href="href(sub.link)"
                :title="sub.summary[l]"
              >
                ↳ {{ sub.name[l] }}
              </component>
            </div>
            <div class="foot">
              <span class="badge">{{ t.builtin }}</span>
              <span class="badge on"><span class="led" />{{ t.enabled }}</span>
              <span v-if="p.link" class="more">{{ t.docs }} →</span>
            </div>
          </div>
        </div>
        <p v-else class="none-here">{{ t.noneHere }}</p>
      </div>
    </template>

    <div v-if="!matching.length" class="none">
      <p>{{ t.empty }} « {{ query }} ».</p>
      <button type="button" class="chip active" @click="reset">{{ t.reset }}</button>
    </div>
  </section>
</template>

<style scoped>
.marketplace {
  margin: 24px 0 40px;
}

/* Search bar */
.search {
  position: relative;
  display: flex;
  align-items: center;
}

.search input {
  width: 100%;
  height: 52px;
  padding: 0 52px 0 48px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 14px;
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  font-size: 16px;
  transition: border-color 0.2s, box-shadow 0.2s, background-color 0.2s;
}

.search input::placeholder {
  color: var(--vp-c-text-3);
}

.search input:focus {
  outline: none;
  border-color: var(--vp-c-brand-1);
  background: var(--vp-c-bg);
  box-shadow: 0 0 0 4px var(--vp-c-brand-soft);
}

.search input::-webkit-search-cancel-button {
  display: none;
}

.search-icon {
  position: absolute;
  left: 16px;
  width: 20px;
  height: 20px;
  fill: none;
  stroke: var(--vp-c-text-3);
  stroke-width: 2;
  stroke-linecap: round;
  pointer-events: none;
}

.hint,
.clear {
  position: absolute;
  right: 14px;
}

.hint {
  padding: 1px 8px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  color: var(--vp-c-text-3);
}

.clear {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  font-size: 18px;
  line-height: 1;
  color: var(--vp-c-text-2);
  background: var(--vp-c-default-soft);
}

.clear:hover {
  color: var(--vp-c-text-1);
}

/* Categories */
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 16px;
}

.chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 999px;
  font-size: 13px;
  font-weight: 500;
  color: var(--vp-c-text-2);
  background: var(--vp-c-bg);
  transition: color 0.2s, border-color 0.2s, background-color 0.2s, opacity 0.2s;
}

.chip:hover {
  color: var(--vp-c-text-1);
  border-color: var(--vp-c-brand-1);
}

.chip.active {
  color: var(--vp-c-brand-1);
  border-color: var(--vp-c-brand-1);
  background: var(--vp-c-brand-soft);
}

.chip.empty:not(.active) {
  opacity: 0.45;
}

.chip .count {
  font-size: 11px;
  color: var(--vp-c-text-3);
}

.chip .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.section {
  margin-top: 36px;
}

.section-head {
  display: flex;
  align-items: baseline;
  gap: 10px;
  padding-bottom: 10px;
  border-bottom: 1px solid var(--vp-c-divider);
}

.section-title {
  font-size: 20px;
  font-weight: 600;
  color: var(--vp-c-text-1);
}

.section-count {
  padding: 1px 8px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 600;
  color: var(--vp-c-text-2);
  background: var(--vp-c-default-soft);
}

.section .grid {
  margin-top: 16px;
}

.none-here {
  margin: 16px 0 0 !important;
  font-size: 14px;
  color: var(--vp-c-text-3);
}

/* Cards */
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 14px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 14px;
  position: relative;
  background: var(--vp-c-bg-soft);
  transition: border-color 0.2s, transform 0.2s, box-shadow 0.2s;
}

.card.linked:hover {
  border-color: var(--vp-c-brand-1);
  transform: translateY(-2px);
  box-shadow: 0 8px 24px -12px var(--vp-c-brand-1);
}

.head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.icon {
  flex: none;
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: 11px;
  color: #fff;
  font-size: 15px;
  font-weight: 700;
  letter-spacing: 0.02em;
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.18);
}

.title {
  min-width: 0;
}

.title .name {
  display: block;
  font-size: 15px;
  line-height: 1.3;
  font-weight: 600;
  color: var(--vp-c-text-1);
  text-decoration: none;
}

/* The logo of the plugin, the one the IDE shows; without one, its initials on the color of its
   category. */
.icon.logo {
  background: var(--vp-c-bg);
  box-shadow: inset 0 0 0 1px var(--vp-c-divider);
}

.icon.logo img {
  width: 26px;
  height: 26px;
}

.icon.logo .dark,
:global(.dark) .icon.logo.themed .light {
  display: none;
}

:global(.dark) .icon.logo.themed .dark {
  display: block;
}

/* The link of the name covers the whole card; the links of the sub-plugins sit above it. */
.stretched::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
}

.subs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding-top: 10px;
  border-top: 1px dashed var(--vp-c-divider);
}

.subs-label {
  font-size: 11.5px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--vp-c-text-3);
}

.sub {
  position: relative;
  z-index: 1;
  padding: 2px 8px;
  border-radius: 6px;
  font-size: 12.5px;
  font-weight: 500;
  color: var(--vp-c-brand-1);
  background: var(--vp-c-brand-soft);
  text-decoration: none;
}

a.sub:hover {
  text-decoration: underline;
}

.category {
  font-size: 12px;
  color: var(--vp-c-text-3);
}

.description {
  flex: 1;
  margin: 0 !important;
  font-size: 13.5px;
  line-height: 1.55;
  color: var(--vp-c-text-2);
}

.foot {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.badge {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 8px;
  border-radius: 6px;
  font-size: 11.5px;
  font-weight: 500;
  color: var(--vp-c-text-2);
  background: var(--vp-c-default-soft);
}

.badge.on {
  color: var(--vp-c-green-1);
  background: var(--vp-c-green-soft);
}

.led {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
}

.more {
  margin-left: auto;
  font-size: 12.5px;
  font-weight: 500;
  color: var(--vp-c-brand-1);
  opacity: 0;
  transition: opacity 0.2s;
}

.card.linked:hover .more,
.card.linked:focus-within .more {
  opacity: 1;
}

@media (hover: none) {
  .more {
    opacity: 1;
  }
}

.none {
  margin-top: 24px;
  padding: 40px 16px;
  border: 1px dashed var(--vp-c-divider);
  border-radius: 14px;
  text-align: center;
  color: var(--vp-c-text-2);
}

.none p {
  margin: 0 0 12px !important;
}
</style>
