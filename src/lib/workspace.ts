import { isLocale } from './i18n'
import type { AssetRef, AssetSettings, MergeDraft, GitSelection, RemoteSelection, SshProfile, CompareKind, CompareSession, Preferences, Side, TextDocument } from '../contracts'

export interface SavedSession { id: string; name: string; savedAt: string; session: CompareSession }
export interface Workspace { version: 1; sessions: CompareSession[]; activeId: string; preferences: Preferences; savedSessions: SavedSession[] }
export const defaultPreferences: Preferences = { theme: 'dark', locale: 'system', fontSize: 13, wordWrap: false, showSidebar: true }
export const defaultGitSource = (revision='HEAD'):GitSelection => ({repository:'',revision})
export const defaultSshProfile = ():SshProfile => ({name:'',host:'',port:22,username:'',fingerprint:'',auth:'agent',keyPath:''})
export const defaultRemoteSource = (kind:RemoteSelection['kind']='sftp'):RemoteSelection => ({kind,path:'',profile:defaultSshProfile()})
function validSshProfile(value:unknown):value is SshProfile{
  if(!value||typeof value!=='object')return false
  const p=value as SshProfile
  return ['name','host','username','fingerprint','keyPath'].every(k=>typeof (p as unknown as Record<string,unknown>)[k]==='string') &&
    Number.isInteger(p.port)&&p.port>=1&&p.port<=65535&&['agent','key','password'].includes(p.auth)
}
function validGitSource(v:unknown):v is GitSelection{
  if(!v||typeof v!=='object')return false
  const s=v as GitSelection
  return typeof s.repository==='string'&&typeof s.revision==='string'
}
function validRemoteSource(v:unknown):v is RemoteSelection{
  if(!v||typeof v!=='object')return false
  const s=v as RemoteSelection
  return ['local','sftp'].includes(s.kind)&&typeof s.path==='string'&&validSshProfile(s.profile)
}
export const defaultAssetSettings = (): AssetSettings => ({
  binaryOffset: 0, binaryPageSize: 512, imageMode: 'side', imageTolerance: 0, imageMix: 50,
  tableDelimiter: 'auto', tableEncoding: 'auto', tableHeader: true, tableColumnMode: 'position',
  tableRowMode: 'position', tableKey: '', tableIgnoreType: false, leftSheet: '', rightSheet: '',
})
function validAsset(value: unknown): value is AssetRef | null {
  if (value === null) return true
  if (!value || typeof value !== 'object') return false
  const a = value as AssetRef
  return (a.path === null || (typeof a.path === 'string' && !!a.path)) && typeof a.name === 'string' &&
    Number.isSafeInteger(a.size) && a.size >= 0 && typeof a.modified === 'string'
}
function validAssetSettings(value: unknown): boolean {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false
  const p = { ...defaultAssetSettings(), ...value }
  return Number.isSafeInteger(p.binaryOffset) && p.binaryOffset >= 0 && [256,512,1024,4096].includes(p.binaryPageSize) &&
    ['side','overlay','split','heat'].includes(p.imageMode) &&
    Number.isInteger(p.imageTolerance) && p.imageTolerance >= 0 && p.imageTolerance <= 255 &&
    Number.isFinite(p.imageMix) && p.imageMix >= 0 && p.imageMix <= 100 &&
    ['auto',',','\t',';','|'].includes(p.tableDelimiter) && ['auto','utf-8','utf-16le','utf-16be','gb18030','shift_jis','windows-1252'].includes(p.tableEncoding) &&
    typeof p.tableHeader === 'boolean' && ['position','header'].includes(p.tableColumnMode) && ['position','key'].includes(p.tableRowMode) &&
    typeof p.tableKey === 'string' && typeof p.tableIgnoreType === 'boolean' && typeof p.leftSheet === 'string' && typeof p.rightSheet === 'string'
}
export const newDocument = (): TextDocument => ({ path: null, name: 'Untitled', content: '', encoding: 'UTF-8', bom: false, lineEnding: 'lf', fingerprint: null, byteLength: 0 })
export const metadataSignature = (doc: TextDocument): string => JSON.stringify([doc.encoding.toLowerCase(), doc.bom, doc.lineEnding])
export function newMergeDraft(): MergeDraft {
  const output = { ...newDocument(), name: 'merged.txt' }
  return { base: newDocument(), savedBase: '', output, savedOutput: '', savedOutputMetadata: metadataSignature(output), prepared: false, manual: false, resolutions: {} }
}
export function newSession(kind: CompareKind = 'text'): CompareSession {
  return { id: crypto.randomUUID(), title: '', kind, ...(kind === 'merge' ? { merge: newMergeDraft() } : {}), ...(kind === 'git' ? {gitSources:{left:defaultGitSource(),right:defaultGitSource('WORKTREE')}} : {}), ...(kind === 'remote' ? {remoteSources:{left:defaultRemoteSource('local'),right:defaultRemoteSource()}} : {}), assetSources: { left: null, right: null }, assetSettings: defaultAssetSettings(), left: newDocument(), right: newDocument(), savedLeft: '', savedRight: '', savedLeftMetadata: metadataSignature(newDocument()), savedRightMetadata: metadataSignature(newDocument()),
    textOptions: { algorithm: 'myers', ignoreCase: false, whitespace: 'none', ignoreBlankLines: false, ignoreLineEndings: true, ignoreLinePrefixes: [], ignoreLineRegex: '' },
    structuredOptions: { format: 'json', ignoreArrayOrder: false, ignoreCase: false },
    leftRoot: '', rightRoot: '', directoryOptions: { method: 'content', showHidden: true, respectGitignore: true, excludes: [] } }
}
export const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
export const isSideDirty = (session: CompareSession, side: Side): boolean => session[side].content !== (side === 'left' ? session.savedLeft : session.savedRight) || metadataSignature(session[side]) !== ((side === 'left' ? session.savedLeftMetadata : session.savedRightMetadata) ?? metadataSignature(session[side]))
export const isDirty = (session: CompareSession): boolean => isSideDirty(session, 'left') || isSideDirty(session, 'right') || !!(session.merge && (session.merge.base.content !== session.merge.savedBase || session.merge.output.content !== session.merge.savedOutput || metadataSignature(session.merge.output) !== session.merge.savedOutputMetadata))
function validDocument(v: unknown): v is TextDocument {
  if (!v || typeof v !== 'object') return false
  const x = v as TextDocument
  return typeof x.name === 'string' && typeof x.content === 'string' && typeof x.encoding === 'string' &&
    typeof x.bom === 'boolean' && (x.path === null || typeof x.path === 'string') &&
    (x.fingerprint === null || typeof x.fingerprint === 'string') && ['lf', 'crlf', 'cr', 'mixed', 'none'].includes(x.lineEnding) && typeof x.byteLength === 'number'
}
function validMerge(value: unknown): boolean {
  if (!value || typeof value !== 'object') return false
  const m = value as MergeDraft
  return validDocument(m.base) && validDocument(m.output) && typeof m.savedBase === 'string' && typeof m.savedOutput === 'string' &&
    typeof m.savedOutputMetadata === 'string' && typeof m.prepared === 'boolean' && typeof m.manual === 'boolean' &&
    !!m.resolutions && typeof m.resolutions === 'object' && !Array.isArray(m.resolutions) &&
    Object.entries(m.resolutions).every(([k,v]) => /^edit:\d+:\d+$/.test(k) && !!v && ['base','left','right','manual'].includes(v.choice) && typeof v.text === 'string')
}
function validSession(v: unknown): v is CompareSession {
  if (!v || typeof v !== 'object') return false
  const s = v as CompareSession
  return typeof s.id === 'string' && typeof s.title === 'string' && ['text', 'folder', 'structured', 'binary', 'image', 'table','merge','archive','git','remote','document'].includes(s.kind) &&
    (s.kind !== 'git' || (!!s.gitSources && validGitSource(s.gitSources.left) && validGitSource(s.gitSources.right))) &&
    (s.kind !== 'remote' || (!!s.remoteSources && validRemoteSource(s.remoteSources.left) && validRemoteSource(s.remoteSources.right))) &&
    (s.kind !== 'merge' ? s.merge === undefined : validMerge(s.merge)) &&
    (s.assetSources === undefined || (!!s.assetSources && validAsset(s.assetSources.left) && validAsset(s.assetSources.right))) &&
    (s.assetSettings === undefined || validAssetSettings(s.assetSettings)) &&
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
  if (!p || (p.remoteProfiles!==undefined && (!Array.isArray(p.remoteProfiles)||!p.remoteProfiles.every(validSshProfile)||new Set(p.remoteProfiles.map(v=>v.name)).size!==p.remoteProfiles.length)) || !['dark', 'black', 'gray', 'white', 'light'].includes(p.theme) || !isLocale(p.locale) ||
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
