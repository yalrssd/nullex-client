/**
 * Extracts a leading regional-indicator flag emoji (or leading pictograph)
 * from a proxy node name so server rows can display it in a circular badge.
 */
const FLAG_REGEX = /^([\u{1F1E6}-\u{1F1FF}]{2})\s*/u

export function extractLeadingFlagAndTitle(rawName: string): {
  flag: string | null
  title: string
} {
  const trimmed = rawName.trim()
  const match = trimmed.match(FLAG_REGEX)
  if (match) {
    const flag = match[1]
    const rest = trimmed.slice(match[0].length).trim()
    return {
      flag,
      title: rest || trimmed
    }
  }
  return { flag: null, title: trimmed }
}
