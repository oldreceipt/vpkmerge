import { ref } from 'vue';

const MAX_ENTRIES = 500;
const APP_VERSION = '0.2.0';

const entries = ref([]);

function nowIso() {
  return new Date().toISOString();
}

function push(level, message) {
  const next = entries.value.slice(-(MAX_ENTRIES - 1));
  next.push({ ts: nowIso(), level, message });
  entries.value = next;
  if (level === 'error') console.error('[vpkmerge]', message);
  else if (level === 'warn') console.warn('[vpkmerge]', message);
  else console.log('[vpkmerge]', message);
}

function clear() {
  entries.value = [];
}

function formatExport() {
  const header = [
    `vpkmerge ${APP_VERSION} session log`,
    `Exported: ${nowIso()}`,
    `User agent: ${typeof navigator !== 'undefined' ? navigator.userAgent : 'n/a'}`,
    `Entries: ${entries.value.length}${entries.value.length === MAX_ENTRIES ? ' (ring buffer full; older entries dropped)' : ''}`,
    '',
  ].join('\n');
  const body = entries.value
    .map((e) => `${e.ts}  ${e.level.toUpperCase().padEnd(5)}  ${e.message}`)
    .join('\n');
  return `${header}${body}\n`;
}

export function useLogs() {
  return {
    entries,
    log: (msg) => push('info', msg),
    warn: (msg) => push('warn', msg),
    error: (msg) => push('error', msg),
    clear,
    formatExport,
  };
}
