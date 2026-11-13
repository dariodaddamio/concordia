import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const siteRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const opifexRoot = process.env.OPIFEX_ROOT ?? 'C:/Users/dario/dev/opifex';
const packageJson = JSON.parse(await readFile(path.join(opifexRoot, 'packages/ui/package.json'), 'utf8'));
const destination = path.join(siteRoot, 'src/styles/opifex');
const files = [
  ['tokens.css', 'packages/ui/src/css/tokens.css'],
  ['base.css', 'packages/ui/src/css/base.css'],
  ['studio-shell.css', 'apps/studio/src/styles/studio-shell.css'],
  ['studio-page.css', 'apps/studio/src/styles/studio-page.css']
];

await mkdir(destination, { recursive: true });
const header = (source) => `/* Vendored from ${opifexRoot.replaceAll('\\', '/')} / ${source}
 * Opifex Studio source version: ${packageJson.version}
 * Refresh with: npm run sync:opifex
 */
`;
for (const [name, source] of files) {
  await writeFile(path.join(destination, name), header(source) + await readFile(path.join(opifexRoot, source), 'utf8'));
}

const bridge = `/* Vendored from Opifex Studio ${packageJson.version}. This bridge maps Opifex tokens to Starlight. */
@import './tokens.css';
@import './base.css';
:root {
  --sl-color-accent-low: color-mix(in srgb, var(--opx-accent) 12%, var(--opx-canvas));
  --sl-color-accent: var(--opx-accent);
  --sl-color-accent-high: var(--opx-text);
  --sl-color-white: var(--opx-canvas);
  --sl-color-gray-1: var(--opx-text);
  --sl-color-gray-2: var(--opx-text-muted);
  --sl-color-gray-3: var(--opx-border);
  --sl-color-gray-4: var(--opx-surface-muted);
  --sl-color-gray-5: var(--opx-surface);
  --sl-color-gray-6: var(--opx-canvas);
  --sl-color-black: var(--opx-text);
  --sl-font: var(--opx-font-ui);
  --sl-font-system: var(--opx-font-ui);
  --sl-font-mono: var(--opx-font-mono);
  --sl-text-body: 1rem;
  --sl-line-height: 1.65;
  --sl-content-width: 52rem;
}
body { font-family: var(--opx-font-ui); }
h1, h2, h3 { font-family: var(--opx-font-display); letter-spacing: var(--opx-tracking-display); }
code, pre { font-family: var(--opx-font-mono); }
.sl-markdown-content :is(table, th, td) { border-color: var(--opx-border); }
.sl-markdown-content :is(th, td) { padding: 0.7rem 0.85rem; }
.sl-markdown-content blockquote { border-inline-start-color: var(--opx-accent); }
`;
await writeFile(path.join(destination, 'theme.css'), header('site bridge based on packages/ui tokens and apps/studio styles') + bridge);
console.log(`Synced ${files.length} Opifex files at version ${packageJson.version}.`);
