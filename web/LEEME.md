# La web del proyecto

El sitio de <https://laboratorios.joseluissaorin.com>. Estático puro: un
`index.html` con todo dentro (sin build, sin framework, sin dependencias),
las cuatro capturas y las tipografías.

Las capturas son **de la aplicación de verdad**, hechas con
`screencapture -l <ventana>` sobre la app corriendo. Cuando cambie la
interfaz, se rehacen y se vuelve a desplegar.

## Desplegar

```bash
npx wrangler pages deploy web --project-name laboratorios-saorin --branch main
```

El dominio (`laboratorios.joseluissaorin.com`) ya está atado al proyecto de
Pages con su CNAME; solo hace falta volver a subir los ficheros.
