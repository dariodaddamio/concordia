import starlight from '@astrojs/starlight';
import { defineConfig } from 'astro/config';

export default defineConfig({
  site: 'https://dariodaddamio.github.io',
  base: '/concordia',
  integrations: [starlight({
    title: 'Concordia',
    description: 'A local augmenter for provider CLIs, projects and memory.',
    logo: { text: 'Concordia' },
    customCss: ['./src/styles/opifex/theme.css'],
    social: { github: 'https://github.com/dariodaddamio/concordia' },
    sidebar: [
      { label: 'Start here', items: [{ label: 'Overview', slug: 'index' }, { label: 'Install', slug: 'install' }, { label: 'Quick start', slug: 'quick-start' }] },
      { label: 'Use Concordia', items: [{ label: 'Providers', slug: 'providers' }, { label: 'Harness and memory', slug: 'harness-memory' }, { label: 'Commands', slug: 'commands' }, { label: 'Configuration', slug: 'configuration' }, { label: 'Safety', slug: 'safety' }] }
    ]
  })]
});
