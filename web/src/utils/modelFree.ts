import type {ModelOption} from '../api'

// The catalog may omit "free" for model IDs ending in :free (OpenRouter)
// or -free (such as OpenCode Go).
// Respect an authoritative paid classification before falling back to the suffix.
export function isFreeModel(model: Pick<ModelOption, 'id' | 'free' | 'freeStatus'>): boolean {
  if (model.freeStatus === 'paid') return false
  if (model.freeStatus === 'free' || model.free === true) return true
  return /(?:[:\-]free)$/i.test(model.id.trim())
}
