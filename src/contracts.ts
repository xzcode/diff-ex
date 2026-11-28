export type Side = 'left' | 'right'
export type CompareKind = 'text' | 'folder' | 'structured' | 'binary' | 'image' | 'table' | 'merge' | 'archive' | 'git' | 'remote' | 'document' | 'database' | 'batch'
export type Theme = 'dark' | 'black' | 'gray' | 'white' | 'light'
export type Language = 'en' | 'zh-CN' | 'zh-TW' | 'ja' | 'ko' | 'de' | 'fr' | 'es' | 'pt-BR' | 'ru' | 'ms' | 'id'
export type Locale = 'system' | Language
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
  ignoreLinePrefixes?: string[]
  ignoreLineRegex?: string
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
export interface AssetRef {
  path: string | null
  name: string
  size: number
  modified: string
}
export interface AssetSettings {
  binaryOffset: number
  binaryPageSize: number
  imageMode: 'side' | 'overlay' | 'split' | 'heat'
  imageTolerance: number
  imageMix: number
  tableDelimiter: 'auto' | ',' | '\t' | ';' | '|'
  tableEncoding: string
  tableHeader: boolean
  tableColumnMode: 'position' | 'header'
  tableRowMode: 'position' | 'key'
  tableKey: string
  tableIgnoreType: boolean
  leftSheet: string
  rightSheet: string
}
export interface BinaryStats {
  compared: number
  changed: number
  leftOnly: number
  rightOnly: number
  firstDifference: number | null
  lastDifference: number | null
}
export interface TableCell { value: string; kind: string }
export interface TableSheet { rowStart: number; colStart: number; rows: (TableCell | null)[][] }
export type MergeChoice = 'base' | 'left' | 'right' | 'manual'
export interface MergeResolution { choice: MergeChoice; text: string }
export interface MergeSegment { id: string; start: number; end: number; kind: 'same' | 'left' | 'right' | 'both' | 'conflict'; base: string; left: string; right: string; text: string }
export interface MergeDraft {
  base: TextDocument
  savedBase: string
  output: TextDocument
  savedOutput: string
  savedOutputMetadata: string
  prepared: boolean
  manual: boolean
  resolutions: Record<string, MergeResolution>
}
export interface ArchiveItem { path: string; kind: 'file' | 'directory' | 'link' | 'special'; size: number; fingerprint: string | null; error: string | null }
export interface ArchiveEntry { path: string; status: 'same' | 'different' | 'leftOnly' | 'rightOnly' | 'conflict' | 'error'; left: ArchiveItem | null; right: ArchiveItem | null }
export interface ArchiveResult { entries: ArchiveEntry[]; leftFormat: string; rightFormat: string }
export interface SnapshotItem { path:string;kind:string;size:number;fingerprint:string|null;mode:string|null;error:string|null }
export interface SnapshotEntry { path:string;status:EntryStatus;left:SnapshotItem|null;right:SnapshotItem|null }
export interface GitSelection { repository:string;revision:string }
export interface GitResult { left:GitSelection;right:GitSelection;entries:SnapshotEntry[] }
export interface SshProfile { name:string;host:string;port:number;username:string;fingerprint:string;auth:'agent'|'key'|'password';keyPath:string }
export interface RemoteSelection { kind:'local'|'sftp';path:string;profile:SshProfile }
export interface RemoteRequest { source:RemoteSelection;secret:string|null }
export interface RemoteResult { left:RemoteSelection;right:RemoteSelection;entries:SnapshotEntry[] }
export interface DocUnit { label:string;text:string;layout:string;layoutFingerprint:string|null;warning:string|null;page:number|null }
export interface DocumentData { format:string;units:DocUnit[];notes:string[];fingerprint:string }
export type DbEngine='sqlite'|'mysql'|'postgres'|'mongodb'
export interface DbProfile {name:string;engine:DbEngine;host:string;port:number;username:string;database:string;schema:string;path:string;tls:boolean;caPath:string;authDatabase:string}
export interface DbSource {profile:DbProfile;relation:string}
export interface DbOptions {mode:'schema'|'data'|'both';keys:string[];ignoreColumns:string[]}
export interface DbValue {kind:string;value:string;raw?:string}
export type DbRow=Record<string,DbValue>
export interface DbDifference {kind:string;name:string;status:'same'|'different'|'leftOnly'|'rightOnly';left:string|null;right:string|null}
export interface DbRowDifference {key:string;status:'different'|'leftOnly'|'rightOnly';columns:string[];left:DbRow|null;right:DbRow|null}
export interface DbResult {schema:DbDifference[];rows:DbRowDifference[];leftRows:number;rightRows:number;equalRows:number;keys:string[];dataCompared:boolean}
export interface BatchItem {id:string;kind:'text'|'structured'|'binary'|'folder'|'archive'|'document';left:string;right:string;format?:'json'|'yaml'|'xml'}
export interface CompareSession {
  dbSources?: Record<Side,DbSource>
  dbOptions?:DbOptions
  batchItems?:BatchItem[]
  gitSources?: Record<Side,GitSelection>
  remoteSources?: Record<Side,RemoteSelection>
  merge?: MergeDraft
  assetSources?: Record<Side, AssetRef | null>
  assetSettings?: AssetSettings
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
  dbProfiles?:DbProfile[]
  remoteProfiles?: SshProfile[]
  theme: Theme
  locale: Locale
  fontSize: number
  wordWrap: boolean
  showSidebar: boolean
}
