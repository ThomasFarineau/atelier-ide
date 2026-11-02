import { h } from 'vue'
import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import DownloadButton from './DownloadButton.vue'
import PluginMarketplace from './PluginMarketplace.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  // The download button sits under the actions of the home page, and is `<DownloadButton />` in
  // any page.
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      'home-hero-actions-after': () => h(DownloadButton, { guide: true }),
    }),
  enhanceApp({ app }) {
    app.component('DownloadButton', DownloadButton)
    app.component('PluginMarketplace', PluginMarketplace)
  },
} satisfies Theme
