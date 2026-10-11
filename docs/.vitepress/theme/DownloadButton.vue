<script setup lang="ts">
// A download button that links straight to the file of the latest release for the visitor's
// system (the installer on Windows, the launcher elsewhere), with the other systems below it.
// `releases/latest/download/<name>` always serves the file of the latest release.
// On the home page (`guide`), a second button leads to the guide, on the same row.
import { computed, onMounted, ref } from 'vue'
import { useData, withBase } from 'vitepress'

defineProps<{ guide?: boolean }>()

type Os = 'windows' | 'mac-arm' | 'mac-intel' | 'linux'

const repo = 'ThomasFarineau/atelier-ide'
const base = `https://github.com/${repo}/releases/latest/download/`
const files: Record<Os, string> = {
  windows: 'atelier-setup-windows-x86_64.exe',
  'mac-arm': 'atelier-macos-aarch64',
  'mac-intel': 'atelier-macos-x86_64',
  linux: 'atelier-linux-x86_64',
}
const names: Record<Os, string> = {
  windows: 'Windows',
  'mac-arm': 'macOS (Apple Silicon)',
  'mac-intel': 'macOS (Intel)',
  linux: 'Linux',
}

const { lang } = useData()
const fr = computed(() => lang.value.startsWith('fr'))
const t = computed(() =>
  fr.value
    ? {
        download: 'Télécharger pour',
        guide: 'Commencer',
        guideLink: '/fr/guide/getting-started',
        others: 'Aussi pour',
        all: 'toutes les versions',
      }
    : {
        download: 'Download for',
        guide: 'Get started',
        guideLink: '/guide/getting-started',
        others: 'Also for',
        all: 'all releases',
      },
)

// Windows until the page knows better: the page is rendered ahead of time, without a browser.
const os = ref<Os>('windows')
const version = ref('')

onMounted(async () => {
  const agent = navigator.userAgent
  if (/Mac/i.test(agent)) {
    os.value = 'mac-arm'
    // Only Chromium tells the architecture; Safari and Firefox on a Mac get Apple Silicon.
    const data = (navigator as any).userAgentData
    if (data?.getHighEntropyValues) {
      try {
        const { architecture } = await data.getHighEntropyValues(['architecture'])
        if (architecture === 'x86') os.value = 'mac-intel'
      } catch {}
    }
  } else if (/Linux|X11/i.test(agent) && !/Android/i.test(agent)) {
    os.value = 'linux'
  }
  try {
    const response = await fetch(`https://api.github.com/repos/${repo}/releases/latest`)
    if (response.ok) version.value = (await response.json()).tag_name ?? ''
  } catch {}
})

const others = computed(() => (Object.keys(files) as Os[]).filter((o) => o !== os.value))
</script>

<template>
  <div class="download" :class="{ home: guide }">
    <div class="buttons">
      <a class="button brand" :href="base + files[os]">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14" />
        </svg>
        {{ t.download }} {{ names[os] }}
      </a>
      <a v-if="guide" class="button alt" :href="withBase(t.guideLink)">{{ t.guide }}</a>
    </div>
    <p class="others">
      <span v-if="version" class="version">{{ version }} · </span>
      {{ t.others }}
      <template v-for="o in others" :key="o">
        <a :href="base + files[o]">{{ names[o] }}</a>,
      </template>
      <a :href="`https://github.com/${repo}/releases`">{{ t.all }}</a>
    </p>
  </div>
</template>

<style scoped>
.download {
  margin: 8px 0 24px;
}

.download.home {
  margin: 32px 0 0;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

/* The size of the large buttons of the default theme. */
.vp-doc .button,
.button {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  border-radius: 24px;
  padding: 0 24px;
  line-height: 46px;
  font-size: 16px;
  font-weight: 600;
  text-decoration: none;
  white-space: nowrap;
  transition: color 0.25s, border-color 0.25s, background-color 0.25s;
}

.vp-doc .button.brand,
.button.brand {
  color: var(--vp-button-brand-text);
  background-color: var(--vp-button-brand-bg);
  border-color: var(--vp-button-brand-border);
}

.button.brand:hover {
  background-color: var(--vp-button-brand-hover-bg);
}

.button.alt {
  color: var(--vp-button-alt-text);
  background-color: var(--vp-button-alt-bg);
  border-color: var(--vp-button-alt-border);
}

.button.alt:hover {
  background-color: var(--vp-button-alt-hover-bg);
}

.button svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2.2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.others {
  margin: 12px 0 0;
  font-size: 14px;
  color: var(--vp-c-text-2);
  line-height: 1.6;
}

.vp-doc .others a,
.others a {
  color: var(--vp-c-brand-1);
  font-weight: 500;
  text-decoration: none;
}

.others a:hover {
  text-decoration: underline;
}

.version {
  color: var(--vp-c-text-1);
  font-weight: 600;
}

@media (max-width: 959px) {
  .download.home {
    text-align: center;
  }

  .download.home .buttons {
    justify-content: center;
  }
}
</style>
