import { copyFile, cp, mkdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const rootDirectory = path.resolve(scriptDirectory, '..');
const modulesDirectory = path.join(rootDirectory, 'node_modules');
const assetsDirectory = path.join(rootDirectory, 'public', 'assets');
const fontAwesomeDirectory = path.join(modulesDirectory, '@fortawesome', 'fontawesome-free');

const files = [
  [path.join(modulesDirectory, 'alpinejs', 'dist', 'cdn.min.js'), path.join(assetsDirectory, 'vendor', 'alpine.min.js')],
  [path.join(modulesDirectory, 'js-yaml', 'dist', 'js-yaml.min.js'), path.join(assetsDirectory, 'vendor', 'js-yaml.min.js')],
  [path.join(fontAwesomeDirectory, 'css', 'all.min.css'), path.join(assetsDirectory, 'vendor', 'fontawesome', 'css', 'all.min.css')],
  [path.join(modulesDirectory, '@fontsource-variable', 'inter', 'files', 'inter-latin-wght-normal.woff2'), path.join(assetsDirectory, 'fonts', 'inter-latin-variable.woff2')],
];

await Promise.all(files.map(async ([source, destination]) => {
  await mkdir(path.dirname(destination), { recursive: true });
  await copyFile(source, destination);
}));

const webfontsDirectory = path.join(assetsDirectory, 'vendor', 'fontawesome', 'webfonts');
await rm(webfontsDirectory, { recursive: true, force: true });
await cp(path.join(fontAwesomeDirectory, 'webfonts'), webfontsDirectory, { recursive: true });
