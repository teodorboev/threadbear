import { defineConfig, loadEnv, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { imageSourcePolicy, withImagePolicy } from './src/picture-src.ts';

// Pictures load only from the site itself and its own Convex deployment's
// file storage (Daybreak finding 133, SECURITY.md "Pictures in replies"):
// the page draws an agent's web address as a link, and this policy holds if
// one ever slips through as an <img>.
function imagePolicy(): Plugin {
  return {
    name: 'threadbear-image-policy',
    transformIndexHtml: {
      order: 'pre',
      handler(html, ctx) {
        const env = { ...loadEnv(ctx.server?.config.mode ?? 'production', new URL('..', import.meta.url).pathname, 'VITE_'), ...process.env };
        const policy = imageSourcePolicy(env.VITE_CONVEX_URL, env.VITE_CONVEX_SITE_URL);
        return withImagePolicy(html, policy);
      },
    },
  };
}

// The website (docs/website.md). Built into web/dist, which
// `npm run web:deploy` uploads to the Convex deployment's static hosting.
export default defineConfig(({ command }) => {
  // The Convex URL is baked in at build time. static-hosting's deploy and
  // `upload --build` set it; a plain build without it would ship a site
  // that can't load anything.
  if (command === 'build' && !process.env.VITE_CONVEX_URL) throw new Error('VITE_CONVEX_URL is not set: use npm run web:deploy');
  return {
    root: 'web',
    envDir: '..',
    plugins: [react(), imagePolicy()],
    build: { outDir: 'dist', emptyOutDir: true },
    server: { port: 5181 },
  };
});
