import React from 'react';

type Style = {
  color?: string;
  bold: boolean;
};

const ANSI_COLORS: Record<number, string> = {
  30: '#000000',
  31: '#aa0000',
  32: '#00aa00',
  33: '#ffaa00',
  34: '#5555ff',
  35: '#aa00aa',
  36: '#55ffff',
  37: '#aaaaaa',
  90: '#555555',
  91: '#ff5555',
  92: '#55ff55',
  93: '#ffff55',
  94: '#5555ff',
  95: '#ff55ff',
  96: '#55ffff',
  97: '#ffffff',
};

const SECTION_COLORS: Record<string, string> = {
  '0': '#000000',
  '1': '#0000aa',
  '2': '#00aa00',
  '3': '#00aaaa',
  '4': '#aa0000',
  '5': '#aa00aa',
  '6': '#ffaa00',
  '7': '#aaaaaa',
  '8': '#555555',
  '9': '#5555ff',
  a: '#55ff55',
  b: '#55ffff',
  c: '#ff5555',
  d: '#ff55ff',
  e: '#ffff55',
  f: '#ffffff',
};

function applySgr(params: string, style: Style) {
  const codes =
    params.length === 0
      ? [0]
      : params.split(';').map((part) => Number.parseInt(part, 10) || 0);
  for (const code of codes) {
    if (code === 0) {
      style.color = undefined;
      style.bold = false;
    } else if (code === 1) {
      style.bold = true;
    } else if (code === 22) {
      style.bold = false;
    } else if (code === 39) {
      style.color = undefined;
    } else if (ANSI_COLORS[code]) {
      style.color = ANSI_COLORS[code];
    }
  }
}

function applySection(code: string, style: Style) {
  const color = SECTION_COLORS[code.toLowerCase()];
  if (color) {
    style.color = color;
    return;
  }
  if (code.toLowerCase() === 'r') {
    style.color = undefined;
    style.bold = false;
  } else if (code.toLowerCase() === 'l') {
    style.bold = true;
  }
}

/**
 * Turn ANSI SGR and Minecraft section codes into colored spans.
 * Unknown escape sequences are removed.
 */
export function renderConsoleText(raw: string): React.ReactNode {
  const text = raw.replace(/\^\[/g, '\u001b');
  if (!/[\u001b§]/.test(text)) return text;

  const parts: React.ReactNode[] = [];
  const style: Style = { bold: false };
  let plain = '';
  let key = 0;

  const flush = () => {
    if (!plain) return;
    parts.push(
      <span
        key={key}
        style={{
          color: style.color,
          fontWeight: style.bold ? 700 : undefined,
        }}
      >
        {plain}
      </span>
    );
    key += 1;
    plain = '';
  };

  for (let i = 0; i < text.length; i += 1) {
    const char = text[i];
    if (char === '\u001b' && text[i + 1] === '[') {
      const end = text.slice(i + 2).search(/[A-Za-z]/);
      if (end === -1) break;
      const command = text[i + 2 + end];
      const params = text.slice(i + 2, i + 2 + end);
      flush();
      if (command === 'm') applySgr(params, style);
      i += 2 + end;
      continue;
    }
    if (char === '§' && i + 1 < text.length) {
      flush();
      applySection(text[i + 1], style);
      i += 1;
      continue;
    }
    plain += char;
  }
  flush();
  return parts;
}
