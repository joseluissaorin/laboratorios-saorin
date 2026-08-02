// @ts-check
import { defineConfig } from 'astro/config';
import react from '@astrojs/react';

// El sitio es ESTÁTICO y va a Cloudflare Pages: no hay servidor que mantener
// ni nada que se caiga a las tres de la mañana. React entra sólo como isla
// donde hace falta interacción (las salas), y el resto se sirve como HTML.
export default defineConfig({
  site: 'https://laboratorios.joseluissaorin.com',
  integrations: [react()],
  build: { inlineStylesheets: 'always' },
});
