// presets.js — los presets del lab del Mac, idénticos (app/ui/js/ui.js).
// «saorín · revelado» ES el default de la casa.

import { DEFAULT_PREFS } from "./state.js";

export const PRESETS = [
  ["saorín · revelado", { ...DEFAULT_PREFS }],
  ["La Chimera · S16", { ...DEFAULT_PREFS,
    grain: 0.45, grainSize: 3.4, grainRough: 0.5, grainChroma: 0.35, grainDefocus: 0.3,
    grainShadows: 0.7, grainMids: 1.0, grainHighs: 0.6, grainBlue: 1.3, filmRes: 0.7,
    halation: 0.25, halHue: 1.0, halSat: 0.9, halThr: 0.8, halSpread: 0.6, halWhite: 0.1,
    bloom: 0.2, bloomThr: 0.8, bloomWarm: 0.3, softness: 0.1, vignette: 0.2, weave: 0.15, chroma: 0 }],
  ["La Chimera · Bolex", { ...DEFAULT_PREFS,
    grain: 0.6, grainSize: 4.5, grainRough: 0.55, grainChroma: 0.35, grainDefocus: 0.4,
    grainShadows: 0.8, grainMids: 1.1, grainHighs: 0.6, grainBlue: 1.3, filmRes: 0.9,
    halation: 0.3, halThr: 0.75, halSpread: 0.6, halWhite: 0.1, bloom: 0.25, bloomWarm: 0.35,
    softness: 0.4, vignette: 0.35, weave: 0.5, flicker: 0.3 }],
  ["CineStill 800T", { ...DEFAULT_PREFS,
    grain: 0.35, grainSize: 3.0, grainRough: 0.4, filmRes: 0.5,
    halation: 1.2, halHue: 1.0, halSat: 1.0, halThr: 0.4, halSpread: 1.0, halWhite: 0.0,
    bloom: 0.5, bloomThr: 0.65, bloomWarm: 0.4, softness: 0.2, vignette: 0.3 }],
  ["FX off", { ...DEFAULT_PREFS,
    grain: 0, halation: 0, bloom: 0, softness: 0, vignette: 0, weave: 0,
    flicker: 0, dust: 0, chroma: 0, shutter: 0 }],
];

// stocks: capas parciales de color sobre el preset activo
export const STOCKS = [
  ["Kodak 50D", { hueSkew: 1.2, crosstalk: 0.35, subtractive: 0.8, stockSat: 1.2, print: 0.7, compImpact: 0.3 }],
  ["Kodak 250D", { hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.7, stockSat: 1.0, print: 0.6, compImpact: 0.15 }],
  ["Kodak 500T", { hueSkew: 1.1, crosstalk: 0.35, subtractive: 0.65, stockSat: 0.95, print: 0.6, compImpact: 0.2, halation: 0.6 }],
  ["Fuji Eterna", { hueSkew: 0.8, crosstalk: 0.25, subtractive: 0.5, stockSat: 0.85, print: 0.4, compImpact: 0.4 }],
  ["CineStill 800", { hueSkew: 1.0, crosstalk: 0.3, subtractive: 0.65, stockSat: 0.95, print: 0.6, halation: 1.2, halSpread: 1.0, halThr: 0.4 }],
];
