import { pageRoutes, watched } from '../../.vitepress/catalog'

export default {
  watch: watched,
  paths: () => pageRoutes('plugins', 'fr', 'plugin'),
}
