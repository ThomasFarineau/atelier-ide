<script setup lang="ts">
// A download button that links straight to the file of the latest release for the visitor's
// system (the installer on Windows, the launcher elsewhere), with the other systems below it.
// `releases/latest/download/<name>` always serves the file of the latest release.
import { computed, onMounted, ref } from 'vue'
import { useData } from 'vitepress'

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
    ? { download: 'Télécharger pour', others: 'Autres systèmes :', all: 'Toutes les versions' }
    : { download: 'Download for', others: 'Other systems:', all: 'All releases' },
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
  <div class="download">
    <a class="button" :href="base + files[os]">
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14" />
      </svg>
      {{ t.download }} {{ names[os] }}
      <span v-if="version" class="version">{{ version }}</span>
    </a>
    <p class="others">
      {{ t.others }}
      <template v-for="(o, i) in others" :key="o">
        <a :href="base + files[o]">{{ names[o] }}</a><span v-if="i < others.length - 1"> · </span>
      </template>
      · <a :href="`https://github.com/${repo}/releases`">{{ t.all }}</a>
    </p>
  </div>
</template>

<style scoped>
.download {
  margin-top: 16px;
  width: 100%;
}

.button {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border-radius: 20px;
  padding: 0 22px;
  line-height: 44px;
  font-size: 16px;
  font-weight: 600;
  color: var(--vp-button-brand-text);
  background-color: var(--vp-button-brand-bg);
  border: 1px solid var(--vp-button-brand-border);
  text-decoration: none;
  transition: background-color 0.25s;
}

.button:hover {
  background-color: var(--vp-button-brand-hover-bg);
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

.version {
  font-weight: 500;
  font-size: 13px;
  opacity: 0.8;
}

.others {
  margin: 10px 0 0;
  font-size: 13px;
  color: var(--vp-c-text-2);
  line-height: 1.6;
}

.others a {
  color: var(--vp-c-brand-1);
  text-decoration: none;
}

.others a:hover {
  text-decoration: underline;
}

@media (max-width: 959px) {
  .download {
    text-align: center;
  }
}
</style>
