export interface TextSpanEdit {
  start: number
  length: number
  newText: string
}

export function applyTextSpanEdits(content: string, edits: readonly TextSpanEdit[]) {
  return [...edits]
    .sort((left, right) => right.start - left.start || right.length - left.length)
    .reduce((nextContent, edit) => {
      const start = Math.max(0, edit.start)
      const end = Math.max(start, start + edit.length)

      return `${nextContent.slice(0, start)}${edit.newText}${nextContent.slice(end)}`
    }, content)
}
