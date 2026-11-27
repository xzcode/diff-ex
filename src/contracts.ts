export type Side = 'left' | 'right'
export type CompareKind = 'text' | 'folder' | 'structured'
export type Theme = 'dark' | 'black' | 'gray' | 'light'
export type Locale = 'system' | 'en' | 'zh-CN'
export type LineEnding = 'lf' | 'crlf' | 'cr' | 'mixed' | 'none'
export interface TextDocument {
  path: string | null
  name: string
  content: string
  encoding: string
  bom: boolean
  lineEnding: LineEnding
  fingerprint: string | null
  byteLength: number
}
export interface SaveTextRequest {
  path: string
  content: string
  encoding: string
  bom: boolean
  lineEnding: LineEnding
  expectedFingerprint: string | null
}
export interface TextOptions {
  algorithm: 'myers' | 'patience' | 'histogram'
  ignoreCase: boolean
  whitespace: 'none' | 'trim' | 'all'
  ignoreBlankLines: boolean
  ignoreLineEndings: boolean
}
export interface TextHunk {
  leftStart: number
  leftEnd: number
  rightStart: number
  rightEnd: number
}
export interface TextDiffResult {
  hunks: TextHunk[]
  equal: boolean
  added: number
  removed: number
  modified: number
  unchanged: number
}
export type StructuredFormat = 'json' | 'yaml' | 'xml'
export interface StructuredOptions {
  format: StructuredFormat
  ignoreArrayOrder: boolean
  ignoreCase: boolean
}
export interface StructureChange {
  path: string
  kind: 'added' | 'removed' | 'changed' | 'typeChanged'
  left: string | null
  right: string | null
}
export interface StructuredResult {
  equal: boolean
  changes: StructureChange[]
  leftCanonical: string
  rightCanonical: string
}
export interface DirectoryOptions {
  method: 'content' | 'metadata'
  showHidden: boolean
  respectGitignore: boolean
  excludes: string[]
}
export type EntryStatus = 'same' | 'different' | 'leftOnly' | 'rightOnly' | 'conflict' | 'error'
export interface FileInfo {
  kind: 'file' | 'directory' | 'symlink'
  size: number
  modifiedMs: number | null
  fingerprint: string | null
}
export interface DirectoryEntry {
  relativePath: string
  status: EntryStatus
  left: FileInfo | null
  right: FileInfo | null
  error: string | null
}
export interface DirectoryResult {
  leftRoot: string
  rightRoot: string
  entries: DirectoryEntry[]
}
export type SyncMode = 'leftToRight' | 'rightToLeft' | 'mirrorLeft' | 'mirrorRight' | 'bidirectional'
export interface SyncOperation {
  relativePath: string
  action: 'copyLeft' | 'copyRight' | 'deleteLeft' | 'deleteRight' | 'createLeftDir' | 'createRightDir' | 'conflict'
  reason: string
  bytes: number
}
export interface SyncPlan {
  id: string
  leftRoot: string
  rightRoot: string
  mode: SyncMode
  operations: SyncOperation[]
}
export interface SyncResult {
  completed: number
  skipped: number
  backupRoot: string | null
  errors: string[]
}
export interface CompareSession {
  id: string
  title: string
  kind: CompareKind
  left: TextDocument
  right: TextDocument
  savedLeft: string
  savedRight: string
  savedLeftMetadata?: string
  savedRightMetadata?: string
  textOptions: TextOptions
  structuredOptions: StructuredOptions
  leftRoot: string
  rightRoot: string
  directoryOptions: DirectoryOptions
}
export interface Preferences {
  theme: Theme
  locale: Locale
  fontSize: number
  wordWrap: boolean
  showSidebar: boolean
}
