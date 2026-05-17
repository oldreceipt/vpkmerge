import { reactive, watch, readonly } from 'vue';

const STORAGE_KEY = 'vpkmerge.settings.v1';

const THEMES = ['light', 'dark', 'system'];
const DOODLE_THEMES = ['arcane', 'celestial', 'botanical', 'nautical'];

const defaults = {
  theme: 'system',
  doodleTheme: 'arcane',
  candleEnabled: true,
};

function loadFromStorage() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...defaults };
    const parsed = JSON.parse(raw);
    return {
      theme: THEMES.includes(parsed.theme) ? parsed.theme : defaults.theme,
      doodleTheme: DOODLE_THEMES.includes(parsed.doodleTheme) ? parsed.doodleTheme : defaults.doodleTheme,
      candleEnabled: typeof parsed.candleEnabled === 'boolean' ? parsed.candleEnabled : defaults.candleEnabled,
    };
  } catch {
    return { ...defaults };
  }
}

const state = reactive(loadFromStorage());

const systemMql = typeof window !== 'undefined' && window.matchMedia
  ? window.matchMedia('(prefers-color-scheme: dark)')
  : null;

function resolvedDark() {
  if (state.theme === 'dark') return true;
  if (state.theme === 'light') return false;
  return systemMql ? systemMql.matches : true;
}

function applyToDocument() {
  const html = document.documentElement;
  html.classList.toggle('dark', resolvedDark());
  html.setAttribute('data-doodle', state.doodleTheme);
  html.setAttribute('data-candle', state.candleEnabled ? 'on' : 'off');
}

function persist() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({
      theme: state.theme,
      doodleTheme: state.doodleTheme,
      candleEnabled: state.candleEnabled,
    }));
  } catch { /* noop */ }
}

let initialized = false;
function init() {
  if (initialized) return;
  initialized = true;
  applyToDocument();
  watch(
    () => [state.theme, state.doodleTheme, state.candleEnabled],
    () => { applyToDocument(); persist(); },
  );
  if (systemMql) {
    const onChange = () => { if (state.theme === 'system') applyToDocument(); };
    if (systemMql.addEventListener) systemMql.addEventListener('change', onChange);
    else if (systemMql.addListener) systemMql.addListener(onChange);
  }
}

export function useSettings() {
  init();
  return {
    settings: readonly(state),
    setTheme: (t) => { if (THEMES.includes(t)) state.theme = t; },
    setDoodleTheme: (d) => { if (DOODLE_THEMES.includes(d)) state.doodleTheme = d; },
    setCandleEnabled: (b) => { state.candleEnabled = !!b; },
    THEMES,
    DOODLE_THEMES,
  };
}
