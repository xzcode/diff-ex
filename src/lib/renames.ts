import type { DirectoryEntry } from '../contracts'

export interface RenameCandidate { left: DirectoryEntry; right: DirectoryEntry }
// Match only unique, unmatched regular files with equal content hashes and
// lengths. No metadata-only guesses, symlinks, ambiguous duplicates or writes.
export function findRenameCandidates(entries: readonly DirectoryEntry[]): RenameCandidate[] {
  const left = new Map<string, DirectoryEntry[]>(), right = new Map<string, DirectoryEntry[]>()
  for (const entry of entries) {
    if (entry.error || (entry.status !== 'leftOnly' && entry.status !== 'rightOnly')) continue
    const info = entry.status === 'leftOnly' ? entry.left : entry.right
    if (info?.kind !== 'file' || !info.fingerprint) continue
    const key = info.size + ':' + info.fingerprint
    const groups = entry.status === 'leftOnly' ? left : right
    const group = groups.get(key) || []
    group.push(entry)
    groups.set(key, group)
  }
  const result: RenameCandidate[] = []
  for (const [key, group] of left) {
    const other = right.get(key)
    if (group.length === 1 && other?.length === 1 && group[0]!.relativePath !== other[0]!.relativePath) result.push({ left: group[0]!, right: other[0]! })
  }
  return result.sort((a,b) => a.left.relativePath.localeCompare(b.left.relativePath))
}
