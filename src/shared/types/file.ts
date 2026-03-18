export interface FileEntry {
  name: string
  path: string
  isDirectory: boolean
  children?: FileEntry[]
}

export interface Tab {
  id: string
  path: string
  name: string
  content: string
  savedContent: string
  language: string
}
