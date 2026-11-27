import * as monaco from 'monaco-editor'
import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import TypeScriptWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import type { Theme } from '../contracts'

self.MonacoEnvironment = {
  getWorker(_moduleId: string, label: string) {
    if (label === 'json') return new JsonWorker()
    if (label === 'css' || label === 'scss' || label === 'less') return new CssWorker()
    if (label === 'html' || label === 'handlebars' || label === 'razor') return new HtmlWorker()
    if (label === 'typescript' || label === 'javascript') return new TypeScriptWorker()
    return new EditorWorker()
  },
}

const palette: Record<Theme, { base: 'vs' | 'vs-dark'; background: string; foreground: string; gutter: string; border: string; selection: string }> = {
  dark: { base: 'vs-dark', background: '#151b25', foreground: '#d7dfec', gutter: '#647186', border: '#263140', selection: '#334d72' },
  black: { base: 'vs-dark', background: '#050505', foreground: '#e4e4e7', gutter: '#71717a', border: '#242428', selection: '#34344c' },
  gray: { base: 'vs-dark', background: '#2a2c30', foreground: '#e2e4e9', gutter: '#90959f', border: '#42454d', selection: '#455065' },
  light: { base: 'vs', background: '#f6f5f1', foreground: '#293343', gutter: '#87909c', border: '#dedfdc', selection: '#d9e5f5' },
}

for (const [name, colors] of Object.entries(palette)) {
  monaco.editor.defineTheme(`diff-ex-${name}`, {
    base: colors.base,
    inherit: true,
    rules: [],
    colors: {
      'editor.background': colors.background,
      'editor.foreground': colors.foreground,
      'editorLineNumber.foreground': colors.gutter,
      'editorLineNumber.activeForeground': colors.foreground,
      'editor.selectionBackground': colors.selection,
      'editor.inactiveSelectionBackground': `${colors.selection}88`,
      'editor.lineHighlightBackground': name === 'light' ? '#1f293705' : '#ffffff04',
      'editorGutter.background': colors.background,
      'editorWidget.background': colors.background,
      'editorWidget.border': colors.border,
      'editorOverviewRuler.border': colors.border,
      'diffEditor.border': colors.border,
      'diffEditor.insertedTextBackground': name === 'light' ? '#168a5633' : '#29b8762a',
      'diffEditor.removedTextBackground': name === 'light' ? '#d94b6033' : '#ed667c28',
      'diffEditor.insertedLineBackground': name === 'light' ? '#168a5612' : '#29b87610',
      'diffEditor.removedLineBackground': name === 'light' ? '#d94b6012' : '#ed667c10',
    },
  })
}

export function setMonacoTheme(theme: Theme) {
  monaco.editor.setTheme(`diff-ex-${theme}`)
}

const extensions: Record<string, string> = {
  json: 'json', jsonc: 'json', js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript',
  ts: 'typescript', tsx: 'typescript', vue: 'html', html: 'html', htm: 'html', xml: 'xml', xsd: 'xml', svg: 'xml',
  css: 'css', scss: 'scss', less: 'less', md: 'markdown', markdown: 'markdown', yaml: 'yaml', yml: 'yaml',
  toml: 'ini', ini: 'ini', properties: 'ini', cfg: 'ini', py: 'python', rs: 'rust', java: 'java',
  go: 'go', c: 'c', h: 'c', cpp: 'cpp', hpp: 'cpp', cs: 'csharp', sql: 'sql', sh: 'shell',
  bash: 'shell', zsh: 'shell', ps1: 'powershell', rb: 'ruby', php: 'php', kt: 'kotlin', swift: 'swift',
  dart: 'dart', lua: 'lua', r: 'r', bat: 'bat', cmd: 'bat', dockerfile: 'dockerfile', diff: 'diff', patch: 'diff',
}

export function languageForFile(name: string): string {
  const basename = name.replaceAll('\\', '/').split('/').at(-1)?.toLowerCase() || ''
  if (basename === 'dockerfile') return 'dockerfile'
  return extensions[basename.split('.').at(-1) || ''] || 'plaintext'
}

export const codeLanguages = [
  ['auto', 'Auto'], ['plaintext', 'Text'], ['json', 'JSON'], ['yaml', 'YAML'], ['xml', 'XML'],
  ['typescript', 'TypeScript'], ['javascript', 'JavaScript'], ['java', 'Java'], ['rust', 'Rust'],
  ['python', 'Python'], ['go', 'Go'], ['cpp', 'C++'], ['csharp', 'C#'], ['sql', 'SQL'],
  ['html', 'HTML'], ['css', 'CSS'], ['markdown', 'Markdown'], ['shell', 'Shell'], ['ini', 'INI'],
] as const

export { monaco }
