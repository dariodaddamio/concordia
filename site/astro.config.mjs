import starlight from '@astrojs/starlight';
import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://dariodaddamio.github.io',
  base: '/concordia',
  integrations: [starlight({
    title: 'Concordia',
    description: 'Run Claude Code, Codex, Cursor, Grok Build and opencode chats in one Windows app.',
    customCss: ['./src/styles/opifex/theme.css'],
    social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/dariodaddamio/concordia' }],
    sidebar: [
      { label: 'Start here', items: [{ label: 'Overview', link: '/' }, { label: 'Install', slug: 'install' }, { label: 'Quick start', slug: 'quick-start' }] },
      { label: 'Use Concordia', items: [{ label: 'Features', slug: 'features' }, { label: 'Providers', slug: 'providers' }, { label: 'Harness and memory', slug: 'harness-memory' }, { label: 'Commands', slug: 'commands' }, { label: 'Configuration', slug: 'configuration' }, { label: 'Safety', slug: 'safety' }] }
    ]
  })]
});
