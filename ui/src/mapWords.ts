// What the map of where the space goes says about each box, in words, and
// how a box is known: shared by the list, the map and its tooltip, so they
// always say the same.

import type { MapNode } from './backend'
import { count, size } from './format'

/** The online storage a person here would know by name. */
export const SERVICE = typeof navigator !== 'undefined' && /Mac/.test(navigator.platform) ? 'iCloud' : 'OneDrive'

/**
 * How each box in a map is known, the same way everywhere: a folder by its
 * path, anything else by the folder it is in and what it is.
 */
export function keysOf(top: MapNode) {
  const keys = new Map<MapNode, string>()
  const visit = (node: MapNode, parent: string) => {
    const key = node.path ?? `${parent}|${node.node}|${node.kind ?? ''}|${node.name}`
    keys.set(node, key)
    node.children?.forEach((child) => visit(child, node.path ?? key))
  }
  visit(top, '')
  return keys
}

/** The box known by `key`, with the boxes above it, outermost first. */
export function find(top: MapNode, keys: Map<MapNode, string>, key: string | null): MapNode[] {
  if (key === null) return []
  const path: MapNode[] = []
  const visit = (node: MapNode): boolean => {
    path.push(node)
    if (keys.get(node) === key || node.children?.some(visit)) return true
    path.pop()
    return false
  }
  visit(top)
  return path
}

/** The kind that takes the most room in a box. */
export function mainKind(node: MapNode): 'photo' | 'video' | 'other' {
  if (node.kind) return node.kind
  const most = Math.max(node.photoBytes, node.videoBytes, node.otherBytes)
  return most === node.photoBytes && most > 0 ? 'photo' : most === node.videoBytes && most > 0 ? 'video' : 'other'
}

/** What a box holds, in words: "1,204 photos, 37 videos and 12 other files". */
export function holds(node: MapNode) {
  const parts = [
    node.photos > 0 ? count(node.photos, 'photo') : '',
    node.videos > 0 ? count(node.videos, 'video') : '',
    node.others > 0 ? count(node.others, 'other file') : '',
  ].filter(Boolean)
  if (parts.length === 0) return 'nothing'
  return parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`
}

/** What a row is called: its name, or for several together, how many. */
export function called(node: MapNode) {
  if (node.node === 'smaller') return count(node.folders ?? 0, 'smaller folder')
  if (node.node === 'files') return holds(node)
  return node.name
}

/** The notes said beside a box, in words. */
export function notesOn(node: MapNode) {
  const notes: string[] = []
  if (node.skipped) notes.push(`${node.skipped}: Keevoro never looks through it`)
  else if (node.leftOut) notes.push(`Not backed up: ${node.leftOut}`)
  if (node.onlineOnlyBytes) notes.push(`${size(node.onlineOnlyBytes)} in ${SERVICE} only, not on this computer`)
  if (node.unreadable) notes.push('Couldn’t be read')
  if (node.notSized) notes.push('Not all measured')
  if (node.moreFolders) notes.push(`${count(node.moreFolders, 'more folder')} counted in`)
  return notes
}


/** A kind of file picked in the list, or one ending of it (its place in the map's `kinds`). */
export type Picked = { kind: 'photo' | 'video' | 'other'; ending?: number }

/** Does a box hold any of the kind, or the ending, picked? */
export function holdsPicked(node: MapNode, picked: Picked) {
  if (picked.ending !== undefined) return node.endings?.includes(picked.ending) ?? false
  const files = picked.kind === 'photo' ? node.photos : picked.kind === 'video' ? node.videos : node.others
  return files > 0
}

/** What a kind of file is called, as `keevoro map --kinds` says it. */
export const KIND_NAMES = { photo: 'Photos', video: 'Videos', other: 'Everything else' } as const

/** An ending as it is said: "jpg", "(no ending)" or "(other endings)", as `keevoro map --kinds` says it. */
export function endingName(ending: string) {
  return ending === '' ? '(no ending)' : ending === '…' ? '(other endings)' : ending
}
