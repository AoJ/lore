// Build the Milkdown editor bundle for the lore desktop app.
// Source: ./index.js  →  Output: ../assets/milkdown.js (IIFE, minified).
//
// Usage:   npm run build
//          npm run watch   (rebuild on change)

import { build, context } from 'esbuild';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const watch = process.argv.includes('--watch');

const config = {
  entryPoints: [resolve(here, 'index.js')],
  bundle: true,
  format: 'iife',
  outfile: resolve(here, '..', 'assets', 'milkdown.js'),
  minify: true,
  target: ['safari14', 'chrome100'],
  legalComments: 'inline',
  logLevel: 'info',
};

if (watch) {
  const ctx = await context(config);
  await ctx.watch();
  console.log('[lore-editor] watching for changes…');
} else {
  await build(config);
  console.log(`[lore-editor] built ${config.outfile}`);
}
