import { isLocale } from './i18n'
import type { CompareKind, CompareSession, Preferences, Side, TextDocument } from '../contracts'

export interface SavedSession { id: string; name: string; savedAt: string; session: CompareSession }
export interface Workspace { version: 1; sessions: CompareSession[]; activeId: string; preferences: Preferences; savedSessions: SavedSession[] }
export const defaultPreferences: Preferences = { theme: 'dark', locale: 'system', fontSize: 13, wordWrap: false, showSidebar: true }
export const newDocument = (): TextDocument => ({ path: null, name: 'Untitled', content: '', encoding: 'UTF-8', bom: false, lineEnding: 'lf', fingerprint: null, byteLength: 0 })
export const metadataSignature = (doc: TextDocument): string => JSON.stringify([doc.encoding.toLowerCase(), doc.bom, doc.lineEnding])
export function newSession(kind: CompareKind = 'text'): CompareSession {
  return { id: crypto.randomUUID(), title: '', kind, left: newDocument(), right: newDocument(), savedLeft: '', savedRight: '', savedLeftMetadata: metadataSignature(newDocument()), savedRightMetadata: metadataSignature(newDocument()),
    textOptions: { algorithm: 'myers', ignoreCase: false, whitespace: 'none', ignoreBlankLines: false, ignoreLineEndings: true, ignoreLinePrefixes: [], ignoreLineRegex: '' },
    structuredOptions: { format: 'json', ignoreArrayOrder: false, ignoreCase: false },
    leftRoot: '', rightRoot: '', directoryOptions: { method: 'content', showHidden: true, respectGitignore: true, excludes: [] } }
}
export const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
export const isSideDirty = (session: CompareSession, side: Side): boolean => session[side].content !== (side === 'left' ? session.savedLeft : session.savedRight) || metadataSignature(session[side]) !== ((side === 'left' ? session.savedLeftMetadata : session.savedRightMetadata) ?? metadataSignature(session[side]))
export const isDirty = (session: CompareSession): boolean => isSideDirty(session, 'left') || isSideDirty(session, 'right')
function validDocument(v: unknown): v is TextDocument {
  if (!v || typeof v !== 'object') return false
  const x = v as TextDocument
  return typeof x.name === 'string' && typeof x.content === 'string' && typeof x.encoding === 'string' &&
    typeof x.bom === 'boolean' && (x.path === null || typeof x.path === 'string') &&
    (x.fingerprint === null || typeof x.fingerprint === 'string') && ['lf', 'crlf', 'cr', 'mixed', 'none'].includes(x.lineEnding) && typeof x.byteLength === 'number'
}
function validSession(v: unknown): v is CompareSession {
  if (!v || typeof v !== 'object') return false
  const s = v as CompareSession
  return typeof s.id === 'string' && typeof s.title === 'string' && ['text', 'folder', 'structured'].includes(s.kind) &&
    validDocument(s.left) && validDocument(s.right) && typeof s.savedLeft === 'string' && typeof s.savedRight === 'string' &&
    typeof s.leftRoot === 'string' && typeof s.rightRoot === 'string' && !!s.textOptions && !!s.structuredOptions && !!s.directoryOptions &&
    (s.textOptions.ignoreLinePrefixes === undefined || (Array.isArray(s.textOptions.ignoreLinePrefixes) && s.textOptions.ignoreLinePrefixes.every(p => typeof p === 'string'))) &&
    (s.textOptions.ignoreLineRegex === undefined || typeof s.textOptions.ignoreLineRegex === 'string') &&
    ['myers', 'patience', 'histogram'].includes(s.textOptions.algorithm) && ['none', 'trim', 'all'].includes(s.textOptions.whitespace) &&
    ['ignoreCase', 'ignoreBlankLines', 'ignoreLineEndings'].every(k => typeof (s.textOptions as unknown as Record<string, unknown>)[k] === 'boolean') &&
    ['json', 'yaml', 'xml'].includes(s.structuredOptions.format) && typeof s.structuredOptions.ignoreCase === 'boolean' && typeof s.structuredOptions.ignoreArrayOrder === 'boolean' &&
    ['content', 'metadata'].includes(s.directoryOptions.method) && typeof s.directoryOptions.showHidden === 'boolean' && typeof s.directoryOptions.respectGitignore === 'boolean' &&
    Array.isArray(s.directoryOptions.excludes) && s.directoryOptions.excludes.every(x => typeof x === 'string')
}
export function decodeWorkspace(text: string): Workspace {
  const x: unknown = JSON.parse(text)
  if (!x || typeof x !== 'object') throw new Error('Invalid workspace file')
  const w = x as Workspace
  if (w.version !== 1 || !Array.isArray(w.sessions) || !w.sessions.every(validSession) ||
      !Array.isArray(w.savedSessions) || !w.savedSessions.every(s => s && typeof s.id === 'string' && typeof s.name === 'string' && typeof s.savedAt === 'string' && validSession(s.session))) {
    throw new Error('The workspace has an unsupported version or malformed session data.')
  }
  const p = w.preferences
  if (!p || !['dark', 'black', 'gray', 'white', 'light'].includes(p.theme) || !isLocale(p.locale) ||
    !Number.isFinite(p.fontSize) || p.fontSize < 8 || p.fontSize > 40 || typeof p.wordWrap !== 'boolean' || typeof p.showSidebar !== 'boolean') throw new Error('Invalid workspace preferences')
  if (new Set(w.sessions.map(s => s.id)).size !== w.sessions.length) throw new Error('Duplicate session identifiers')
  w.activeId = w.sessions.some(s => s.id === w.activeId) ? w.activeId : w.sessions[0]?.id || ''
  return w
}
export function sampleSession(): CompareSession {
  const s = newSession('text')
  s.title = 'app.config.json'
  s.left.name = 'app.config.json'
  s.right.name = 'app.config.json'
  s.left.content = '{\n  "name": "Diff EX",\n  "version": "0.1.0",\n  "workspace": {\n    "theme": "dark",\n    "language": "en",\n    "restoreSessions": false\n  },\n  "compare": {\n    "algorithm": "myers",\n    "ignoreWhitespace": false\n  },\n  "features": [\n    "text",\n    "folders"\n  ]\n}\n'
  s.right.content = '{\n  "name": "Diff EX",\n  "version": "0.2.0",\n  "workspace": {\n    "theme": "dark",\n    "language": "system",\n    "restoreSessions": true\n  },\n  "compare": {\n    "algorithm": "patience",\n    "ignoreWhitespace": false\n  },\n  "features": [\n    "text",\n    "folders",\n    "structured"\n  ]\n}\n'
  s.savedLeft = s.left.content
  s.savedRight = s.right.content
  return s
}
