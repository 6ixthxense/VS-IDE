import React, { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { SqlBrowseFilter, SqlColumnMetadata, SqlFilterOperator, SqlRowLocator, SqlTableRow, SqlTableRowsResult, SqlTableSummary } from '@shared/types/sql'
import { useEditorStore } from '../../store/editorStore'
import { confirmAction, showErrorToast, showInfoToast, showSuccessToast } from '../../store/feedbackStore'
import { useSqlStore } from '../../store/sqlStore'
import { useWorkspaceStore } from '../../store/workspaceStore'

type RowDrawerMode = 'view' | 'edit' | 'insert'

interface RowDrawerState {
  mode: RowDrawerMode
  row: SqlTableRow | null
  draft: Record<string, string>
}

interface PinnedColumnMeta {
  left: number
  width: number
  isEdge: boolean
}

interface ColumnMenuState {
  columnName: string
  x: number
  y: number
}

const FILTER_OPERATORS: Array<{ value: SqlFilterOperator; label: string }> = [
  { value: 'contains', label: 'Contains' },
  { value: 'startsWith', label: 'Starts with' },
  { value: 'endsWith', label: 'Ends with' },
  { value: 'equals', label: '=' },
  { value: 'notEquals', label: '!=' },
  { value: 'greaterThan', label: '>' },
  { value: 'greaterThanOrEqual', label: '>=' },
  { value: 'lessThan', label: '<' },
  { value: 'lessThanOrEqual', label: '<=' },
  { value: 'isNull', label: 'Is null' },
  { value: 'isNotNull', label: 'Is not null' },
]

const DATABASE_COMPACT_MODE_KEY = 'vs-monitor-ide:database:compact-preview'
const DATABASE_SCHEMA_COLLAPSED_KEY = 'vs-monitor-ide:database:schema-collapsed'
const DATABASE_TABLE_LAYOUTS_KEY = 'vs-monitor-ide:database:table-layouts'
const DATABASE_TABLE_PRESETS_KEY = 'vs-monitor-ide:database:table-presets'
const DATABASE_WORKSPACE_PRESET_LIBRARY_KEY = 'vs-ide:database:workspace-preset-library'
const DATABASE_EXPORT_TEMPLATE_FORMAT_KEY = 'vs-ide:database:export-template-format'
const MIN_COLUMN_WIDTH = 84
const MAX_COLUMN_WIDTH = 720

interface DatabaseTableLayoutPreference {
  compactMode: boolean
  pinnedColumns: string[]
  hiddenColumns: string[]
  columnOrder: string[]
  columnWidths: Record<string, number>
}

interface DatabaseTablePresetBrowseState {
  pageSize: number
  sortColumn: string | null
  sortDirection: 'asc' | 'desc'
  filters: SqlBrowseFilter[]
}

interface DatabaseTablePreset {
  id: string
  name: string
  createdAt: string
  updatedAt: string
  layout: DatabaseTableLayoutPreference
  browse: DatabaseTablePresetBrowseState
}

interface DatabaseTablePresetState {
  activePresetId: string | null
  presets: DatabaseTablePreset[]
}

interface DatabaseWorkspaceLibraryPreset {
  id: string
  name: string
  createdAt: string
  updatedAt: string
  source: {
    connectionId: string
    tableId: string
  }
  layout: DatabaseTableLayoutPreference
  browse: DatabaseTablePresetBrowseState
}

interface DatabaseTablePresetExportPayload {
  kind: 'vs-ide-table-preset'
  version: 1
  exportedAt: string
  source: {
    connectionId: string
    tableId: string
  }
  preset: DatabaseTablePreset
}

interface ExportColumn {
  key: string
  label: string
}

type ExportColumnMode = 'visible' | 'all' | 'selected'
type ExportFormat = 'csv' | 'xlsx'
type ExportRowScope = 'page' | 'all'
type ColumnEditorKind = 'text' | 'textarea' | 'boolean' | 'integer' | 'number' | 'date' | 'time' | 'datetime'

interface ExportTemplateDefinition {
  id: string
  label: string
  columnMode: ExportColumnMode
  rowScope: ExportRowScope
}

const EXPORT_TEMPLATES: ExportTemplateDefinition[] = [
  {
    id: 'visible-all-rows',
    label: 'Visible + All Rows',
    columnMode: 'visible',
    rowScope: 'all',
  },
  {
    id: 'selected-current-page',
    label: 'Selected + Current Page',
    columnMode: 'selected',
    rowScope: 'page',
  },
  {
    id: 'all-columns-all-rows',
    label: 'All Columns + All Rows',
    columnMode: 'all',
    rowScope: 'all',
  },
]

function readCompactModePreference() {
  if (typeof window === 'undefined') return false

  try {
    return window.localStorage.getItem(DATABASE_COMPACT_MODE_KEY) === '1'
  } catch {
    return false
  }
}

function writeCompactModePreference(value: boolean) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_COMPACT_MODE_KEY, value ? '1' : '0')
  } catch {
    /* ignore */
  }
}

function readExportTemplateFormatPreference(): ExportFormat {
  if (typeof window === 'undefined') return 'xlsx'

  try {
    return window.localStorage.getItem(DATABASE_EXPORT_TEMPLATE_FORMAT_KEY) === 'csv' ? 'csv' : 'xlsx'
  } catch {
    return 'xlsx'
  }
}

function writeExportTemplateFormatPreference(value: ExportFormat) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_EXPORT_TEMPLATE_FORMAT_KEY, value)
  } catch {
    /* ignore */
  }
}

function createDatabaseTableLayoutKey(connectionId: string, tableId: string) {
  return `${connectionId}::${tableId}`
}

function readDatabaseTableLayouts() {
  if (typeof window === 'undefined') return {}

  try {
    const raw = window.localStorage.getItem(DATABASE_TABLE_LAYOUTS_KEY)
    if (!raw) return {}
    return JSON.parse(raw) as Record<string, DatabaseTableLayoutPreference>
  } catch {
    return {}
  }
}

function writeDatabaseTableLayouts(layouts: Record<string, DatabaseTableLayoutPreference>) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_TABLE_LAYOUTS_KEY, JSON.stringify(layouts))
  } catch {
    /* ignore */
  }
}

function getDefaultPinnedColumnNames(columns: SqlColumnMetadata[]) {
  return getPinnedColumns(columns).map((column) => column.name)
}

function sanitizePinnedColumnNames(pinnedColumns: string[], columns: SqlColumnMetadata[]) {
  const availableNames = new Set(columns.map((column) => column.name))
  return [...new Set(pinnedColumns.filter((name) => availableNames.has(name)))]
}

function sanitizeHiddenColumnNames(hiddenColumns: string[], columns: SqlColumnMetadata[]) {
  const availableNames = new Set(columns.map((column) => column.name))
  return [...new Set(hiddenColumns.filter((name) => availableNames.has(name)))]
}

function sanitizeColumnOrder(columnOrder: string[], columns: SqlColumnMetadata[]) {
  const availableNames = new Set(columns.map((column) => column.name))
  const sanitized = [...new Set(columnOrder.filter((name) => availableNames.has(name)))]

  columns.forEach((column) => {
    if (!sanitized.includes(column.name)) {
      sanitized.push(column.name)
    }
  })

  return sanitized
}

function clampColumnWidth(width: number) {
  return Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, Math.round(width)))
}

function sanitizeColumnWidths(columnWidths: Record<string, number>, columns: SqlColumnMetadata[]) {
  const availableNames = new Set(columns.map((column) => column.name))

  return Object.fromEntries(
    Object.entries(columnWidths)
      .filter(([name, value]) => availableNames.has(name) && Number.isFinite(value))
      .map(([name, value]) => [name, clampColumnWidth(value)])
  ) as Record<string, number>
}

function clonePresetBrowseFilters(filters: SqlBrowseFilter[]) {
  return filters.map((filter) => ({
    column: filter.column,
    operator: filter.operator,
    value: filter.value ?? '',
  }))
}

function sanitizePresetBrowseFilters(filters: SqlBrowseFilter[], columns: SqlColumnMetadata[]) {
  const availableNames = new Set(columns.map((column) => column.name))

  const sanitized = filters.flatMap((filter) => {
    const operator = FILTER_OPERATORS.some((option) => option.value === filter.operator) ? filter.operator : 'contains'
    const nextColumn = typeof filter.column === 'string' && availableNames.has(filter.column) ? filter.column : undefined
    const requiresColumn = filterOperatorRequiresColumn(operator)

    if (requiresColumn && !nextColumn) {
      return []
    }

    return [{
      column: nextColumn,
      operator,
      value: filterOperatorUsesValue(operator) ? (filter.value ?? '') : '',
    }]
  })

  return sanitized.length > 0 ? sanitized : [createEmptyFilterDraft()]
}

function sanitizePresetBrowseState(
  browse: Partial<DatabaseTablePresetBrowseState> | undefined,
  columns: SqlColumnMetadata[]
): DatabaseTablePresetBrowseState {
  const pageSizeCandidate = browse?.pageSize
  const pageSize = [10, 25, 50, 100].includes(pageSizeCandidate ?? 0) ? (pageSizeCandidate as number) : 25
  const sortColumn = typeof browse?.sortColumn === 'string' && columns.some((column) => column.name === browse.sortColumn)
    ? browse.sortColumn
    : null

  return {
    pageSize,
    sortColumn,
    sortDirection: browse?.sortDirection === 'desc' ? 'desc' : 'asc',
    filters: sanitizePresetBrowseFilters(browse?.filters ?? [createEmptyFilterDraft()], columns),
  }
}

function sanitizeDatabaseTableLayout(layout: Partial<DatabaseTableLayoutPreference> | undefined, columns: SqlColumnMetadata[]): DatabaseTableLayoutPreference {
  return {
    compactMode: Boolean(layout?.compactMode),
    pinnedColumns: sanitizePinnedColumnNames(layout?.pinnedColumns ?? [], columns),
    hiddenColumns: sanitizeHiddenColumnNames(layout?.hiddenColumns ?? [], columns),
    columnOrder: sanitizeColumnOrder(layout?.columnOrder ?? [], columns),
    columnWidths: sanitizeColumnWidths(layout?.columnWidths ?? {}, columns),
  }
}

function sanitizeStoredLayoutSnapshot(layout: Partial<DatabaseTableLayoutPreference> | undefined): DatabaseTableLayoutPreference {
  return {
    compactMode: Boolean(layout?.compactMode),
    pinnedColumns: [...new Set((Array.isArray(layout?.pinnedColumns) ? layout?.pinnedColumns : []).filter((name): name is string => typeof name === 'string' && name.trim().length > 0))],
    hiddenColumns: [...new Set((Array.isArray(layout?.hiddenColumns) ? layout?.hiddenColumns : []).filter((name): name is string => typeof name === 'string' && name.trim().length > 0))],
    columnOrder: [...new Set((Array.isArray(layout?.columnOrder) ? layout?.columnOrder : []).filter((name): name is string => typeof name === 'string' && name.trim().length > 0))],
    columnWidths: Object.fromEntries(
      Object.entries(layout?.columnWidths ?? {})
        .filter(([name, width]) => typeof name === 'string' && name.trim().length > 0 && Number.isFinite(width))
        .map(([name, width]) => [name, clampColumnWidth(Number(width))])
    ) as Record<string, number>,
  }
}

function sanitizeStoredPresetBrowseState(browse: Partial<DatabaseTablePresetBrowseState> | undefined): DatabaseTablePresetBrowseState {
  const pageSizeCandidate = browse?.pageSize
  const pageSize = [10, 25, 50, 100].includes(pageSizeCandidate ?? 0) ? (pageSizeCandidate as number) : 25
  const filters = Array.isArray(browse?.filters)
    ? browse.filters.map(sanitizeBrowseFilter).filter((filter): filter is SqlBrowseFilter => filter != null)
    : []

  return {
    pageSize,
    sortColumn: typeof browse?.sortColumn === 'string' && browse.sortColumn.trim().length > 0 ? browse.sortColumn : null,
    sortDirection: browse?.sortDirection === 'desc' ? 'desc' : 'asc',
    filters: filters.length > 0 ? filters : [createEmptyFilterDraft()],
  }
}

function readDatabaseTableLayout(connectionId: string, tableId: string, columns: SqlColumnMetadata[]): DatabaseTableLayoutPreference {
  const layoutKey = createDatabaseTableLayoutKey(connectionId, tableId)
  const savedLayouts = readDatabaseTableLayouts()
  const savedLayout = savedLayouts[layoutKey]
  const defaultPinnedColumns = getDefaultPinnedColumnNames(columns)

  return {
    compactMode: savedLayout?.compactMode ?? readCompactModePreference(),
    pinnedColumns: savedLayout
      ? sanitizePinnedColumnNames(savedLayout.pinnedColumns ?? [], columns)
      : defaultPinnedColumns,
    hiddenColumns: savedLayout
      ? sanitizeHiddenColumnNames(savedLayout.hiddenColumns ?? [], columns)
      : [],
    columnOrder: savedLayout
      ? sanitizeColumnOrder(savedLayout.columnOrder ?? [], columns)
      : columns.map((column) => column.name),
    columnWidths: savedLayout
      ? sanitizeColumnWidths(savedLayout.columnWidths ?? {}, columns)
      : {},
  }
}

function writeDatabaseTableLayout(connectionId: string, tableId: string, layout: DatabaseTableLayoutPreference, columns: SqlColumnMetadata[]) {
  const layoutKey = createDatabaseTableLayoutKey(connectionId, tableId)
  const layouts = readDatabaseTableLayouts()
  layouts[layoutKey] = {
    compactMode: layout.compactMode,
    pinnedColumns: [...new Set(layout.pinnedColumns)],
    hiddenColumns: [...new Set(layout.hiddenColumns)],
    columnOrder: [...new Set(layout.columnOrder)],
    columnWidths: sanitizeColumnWidths(layout.columnWidths, columns),
  }
  writeDatabaseTableLayouts(layouts)
}

function readDatabaseTablePresetStore() {
  if (typeof window === 'undefined') return {}

  try {
    const raw = window.localStorage.getItem(DATABASE_TABLE_PRESETS_KEY)
    if (!raw) return {}
    return JSON.parse(raw) as Record<string, DatabaseTablePresetState>
  } catch {
    return {}
  }
}

function writeDatabaseTablePresetStore(store: Record<string, DatabaseTablePresetState>) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_TABLE_PRESETS_KEY, JSON.stringify(store))
  } catch {
    /* ignore */
  }
}

function readDatabaseTablePresets(connectionId: string, tableId: string, columns: SqlColumnMetadata[]): DatabaseTablePresetState {
  const tableKey = createDatabaseTableLayoutKey(connectionId, tableId)
  const store = readDatabaseTablePresetStore()
  const entry = store[tableKey]

  if (!entry) {
    return {
      activePresetId: null,
      presets: [],
    }
  }

  const presets = (Array.isArray(entry.presets) ? entry.presets : []).flatMap((preset) => {
    if (!preset || typeof preset !== 'object') return []

    const candidate = preset as Partial<DatabaseTablePreset>
    const id = typeof candidate.id === 'string' && candidate.id.trim().length > 0 ? candidate.id : null
    const name = typeof candidate.name === 'string' && candidate.name.trim().length > 0 ? candidate.name.trim() : null
    if (!id || !name) return []

    return [{
      id,
      name,
      createdAt: typeof candidate.createdAt === 'string' ? candidate.createdAt : new Date().toISOString(),
      updatedAt: typeof candidate.updatedAt === 'string' ? candidate.updatedAt : new Date().toISOString(),
      layout: sanitizeDatabaseTableLayout(candidate.layout, columns),
      browse: sanitizePresetBrowseState(candidate.browse, columns),
    }]
  })

  const activePresetId = typeof entry.activePresetId === 'string' && presets.some((preset) => preset.id === entry.activePresetId)
    ? entry.activePresetId
    : null

  return {
    activePresetId,
    presets,
  }
}

function writeDatabaseTablePresets(
  connectionId: string,
  tableId: string,
  presetState: DatabaseTablePresetState,
  columns: SqlColumnMetadata[]
) {
  const tableKey = createDatabaseTableLayoutKey(connectionId, tableId)
  const store = readDatabaseTablePresetStore()
  const presets = presetState.presets.map((preset) => ({
    ...preset,
    layout: sanitizeDatabaseTableLayout(preset.layout, columns),
    browse: sanitizePresetBrowseState(preset.browse, columns),
  }))

  store[tableKey] = {
    activePresetId: presetState.activePresetId && presets.some((preset) => preset.id === presetState.activePresetId)
      ? presetState.activePresetId
      : null,
    presets,
  }

  writeDatabaseTablePresetStore(store)
}

function createWorkspacePresetLibraryKey(rootPath: string) {
  return rootPath.replace(/\\/g, '/').toLowerCase()
}

function readWorkspacePresetLibraryStore() {
  if (typeof window === 'undefined') return {}

  try {
    const raw = window.localStorage.getItem(DATABASE_WORKSPACE_PRESET_LIBRARY_KEY)
    if (!raw) return {}
    return JSON.parse(raw) as Record<string, DatabaseWorkspaceLibraryPreset[]>
  } catch {
    return {}
  }
}

function writeWorkspacePresetLibraryStore(store: Record<string, DatabaseWorkspaceLibraryPreset[]>) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_WORKSPACE_PRESET_LIBRARY_KEY, JSON.stringify(store))
  } catch {
    /* ignore */
  }
}

function sanitizeWorkspaceLibraryPreset(candidate: unknown) {
  if (!candidate || typeof candidate !== 'object') {
    return null
  }

  const preset = candidate as Partial<DatabaseWorkspaceLibraryPreset>
  const id = typeof preset.id === 'string' && preset.id.trim().length > 0 ? preset.id : null
  const name = typeof preset.name === 'string' && preset.name.trim().length > 0 ? preset.name.trim() : null
  const connectionId = typeof preset.source?.connectionId === 'string' && preset.source.connectionId.trim().length > 0
    ? preset.source.connectionId
    : null
  const tableId = typeof preset.source?.tableId === 'string' && preset.source.tableId.trim().length > 0
    ? preset.source.tableId
    : null

  if (!id || !name || !connectionId || !tableId) {
    return null
  }

  const now = new Date().toISOString()

  return {
    id,
    name,
    createdAt: typeof preset.createdAt === 'string' ? preset.createdAt : now,
    updatedAt: typeof preset.updatedAt === 'string' ? preset.updatedAt : now,
    source: {
      connectionId,
      tableId,
    },
    layout: sanitizeStoredLayoutSnapshot(preset.layout),
    browse: sanitizeStoredPresetBrowseState(preset.browse),
  } satisfies DatabaseWorkspaceLibraryPreset
}

function readWorkspaceLibraryPresets(rootPath: string | null) {
  if (!rootPath) return []

  const workspaceKey = createWorkspacePresetLibraryKey(rootPath)
  const store = readWorkspacePresetLibraryStore()
  const entries = store[workspaceKey]

  if (!Array.isArray(entries)) {
    return []
  }

  return entries
    .map(sanitizeWorkspaceLibraryPreset)
    .filter((preset): preset is DatabaseWorkspaceLibraryPreset => preset != null)
}

function writeWorkspaceLibraryPresets(rootPath: string | null, presets: DatabaseWorkspaceLibraryPreset[]) {
  if (!rootPath) return

  const workspaceKey = createWorkspacePresetLibraryKey(rootPath)
  const store = readWorkspacePresetLibraryStore()
  store[workspaceKey] = presets.map((preset) => ({
    ...preset,
    layout: sanitizeStoredLayoutSnapshot(preset.layout),
    browse: sanitizeStoredPresetBrowseState(preset.browse),
  }))
  writeWorkspacePresetLibraryStore(store)
}

function createDatabaseTablePresetId() {
  return `preset-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

function sanitizeImportedPreset(candidate: unknown, columns: SqlColumnMetadata[]) {
  if (!candidate || typeof candidate !== 'object') {
    return null
  }

  const preset = candidate as Partial<DatabaseTablePreset>
  const name = typeof preset.name === 'string' && preset.name.trim().length > 0 ? preset.name.trim() : null
  if (!name) {
    return null
  }

  const now = new Date().toISOString()

  return {
    id: typeof preset.id === 'string' && preset.id.trim().length > 0 ? preset.id : createDatabaseTablePresetId(),
    name,
    createdAt: typeof preset.createdAt === 'string' ? preset.createdAt : now,
    updatedAt: typeof preset.updatedAt === 'string' ? preset.updatedAt : now,
    layout: sanitizeDatabaseTableLayout(preset.layout, columns),
    browse: sanitizePresetBrowseState(preset.browse, columns),
  } satisfies DatabaseTablePreset
}

function parseImportedPresetPayload(raw: string, columns: SqlColumnMetadata[]) {
  const parsed = JSON.parse(raw) as unknown
  const directPreset = sanitizeImportedPreset(parsed, columns)
  if (directPreset) {
    return directPreset
  }

  if (!parsed || typeof parsed !== 'object') {
    throw new Error('The file does not contain a valid preset.')
  }

  const payload = parsed as Partial<DatabaseTablePresetExportPayload>
  if (payload.kind !== 'vs-ide-table-preset' || !payload.preset) {
    throw new Error('Unsupported preset file format.')
  }

  const preset = sanitizeImportedPreset(payload.preset, columns)
  if (!preset) {
    throw new Error('The preset payload is missing a valid name or data.')
  }

  return preset
}

function buildPresetExportPayload(connectionId: string, tableId: string, preset: DatabaseTablePreset): DatabaseTablePresetExportPayload {
  return {
    kind: 'vs-ide-table-preset',
    version: 1,
    exportedAt: new Date().toISOString(),
    source: {
      connectionId,
      tableId,
    },
    preset,
  }
}

function downloadJsonFile(filename: string, text: string) {
  const blob = new Blob([text], { type: 'application/json;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  anchor.click()
  URL.revokeObjectURL(url)
}

async function pickJsonFile() {
  return new Promise<File | null>((resolve) => {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.json,application/json,text/json,.txt,text/plain'
    input.onchange = () => resolve(input.files?.[0] ?? null)
    input.oncancel = () => resolve(null)
    input.click()
  })
}

function stringifyExportValue(value: unknown) {
  if (value == null) return ''
  if (typeof value === 'object') {
    try {
      return JSON.stringify(value)
    } catch {
      return String(value)
    }
  }

  return String(value)
}

function createCsvContent(columns: ExportColumn[], rows: Array<Record<string, unknown>>) {
  const escapeCsv = (value: string) => `"${value.replace(/"/g, '""')}"`
  const header = columns.map((column) => escapeCsv(column.label)).join(',')
  const body = rows.map((row) => (
    columns
      .map((column) => escapeCsv(stringifyExportValue(row[column.key])))
      .join(',')
  ))

  return [header, ...body].join('\r\n')
}

function buildExportFileName(baseName: string, extension: 'csv' | 'xlsx') {
  const timestamp = new Date().toISOString().replace(/[:]/g, '-').replace(/\.\d+Z$/, '')
  const safeBaseName = baseName.replace(/[^a-z0-9-_]+/gi, '-').replace(/^-+|-+$/g, '') || 'database-export'
  return `${safeBaseName}-${timestamp}.${extension}`
}

function downloadTextFile(filename: string, text: string, mimeType: string) {
  const blob = new Blob([text], { type: mimeType })
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = filename
  anchor.click()
  URL.revokeObjectURL(url)
}

async function exportRowsAsCsv(baseName: string, columns: ExportColumn[], rows: Array<Record<string, unknown>>) {
  const csv = createCsvContent(columns, rows)
  downloadTextFile(buildExportFileName(baseName, 'csv'), csv, 'text/csv;charset=utf-8')
}

async function exportRowsAsXlsx(baseName: string, columns: ExportColumn[], rows: Array<Record<string, unknown>>) {
  const XLSX = await import('xlsx')
  const worksheetRows = rows.map((row) => Object.fromEntries(
    columns.map((column) => [column.label, stringifyExportValue(row[column.key])])
  ))
  const worksheet = XLSX.utils.json_to_sheet(worksheetRows)
  const workbook = XLSX.utils.book_new()
  XLSX.utils.book_append_sheet(workbook, worksheet, 'Export')
  XLSX.writeFileXLSX(workbook, buildExportFileName(baseName, 'xlsx'))
}

function getAppliedBrowseFilters(filters: SqlBrowseFilter[]) {
  return filters.filter((filter) => {
    if (filter.operator === 'isNull' || filter.operator === 'isNotNull') {
      return Boolean(filter.column)
    }

    if (!filter.value || filter.value.trim().length === 0) {
      return false
    }

    return filter.operator === 'contains' || Boolean(filter.column)
  })
}

function orderColumns(columns: SqlColumnMetadata[], columnOrder: string[]) {
  const order = sanitizeColumnOrder(columnOrder, columns)
  return order
    .map((name) => columns.find((column) => column.name === name))
    .filter((column): column is SqlColumnMetadata => column != null)
}

function readSchemaCollapsedPreference() {
  if (typeof window === 'undefined') return false

  try {
    return window.localStorage.getItem(DATABASE_SCHEMA_COLLAPSED_KEY) === '1'
  } catch {
    return false
  }
}

function writeSchemaCollapsedPreference(value: boolean) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_SCHEMA_COLLAPSED_KEY, value ? '1' : '0')
  } catch {
    /* ignore */
  }
}

function filterOperatorRequiresColumn(operator: SqlFilterOperator) {
  return operator !== 'contains'
}

function filterOperatorUsesValue(operator: SqlFilterOperator) {
  return operator !== 'isNull' && operator !== 'isNotNull'
}

function createEmptyFilterDraft(operator: SqlFilterOperator = 'contains'): SqlBrowseFilter {
  return { operator, value: '' }
}

function getFilterDrafts(filters?: SqlBrowseFilter[]) {
  if (!filters || filters.length === 0) {
    return [createEmptyFilterDraft()]
  }

  return filters.map((filter) => ({
    column: filter.column,
    operator: filter.operator,
    value: filter.value ?? '',
  }))
}

function getPinnedColumnWidth(column: SqlColumnMetadata) {
  const name = column.name.toLowerCase()
  const dataType = column.dataType.toUpperCase()

  if (name === 'id') {
    return dataType.includes('INT') ? 88 : 210
  }

  if (name.includes('name') || name.includes('title')) {
    return 180
  }

  if (name.includes('status') || name.includes('state') || name.includes('type')) {
    return 140
  }

  if (name.endsWith('id')) {
    return dataType.includes('INT') ? 110 : 170
  }

  return 150
}

function getPinnedColumns(columns: SqlColumnMetadata[]) {
  const selected: SqlColumnMetadata[] = []
  const seen = new Set<string>()
  const priorities = ['id', 'name', 'title', 'status']

  priorities.forEach((priority) => {
    const match = columns.find((column) => column.name.toLowerCase() === priority)
    if (match && !seen.has(match.name)) {
      selected.push(match)
      seen.add(match.name)
    }
  })

  if (selected.length < 2) {
    columns.forEach((column) => {
      if (selected.length >= 2 || seen.has(column.name)) return

      const lowerName = column.name.toLowerCase()
      if (lowerName.endsWith('id') || lowerName.includes('name') || lowerName.includes('status')) {
        selected.push(column)
        seen.add(column.name)
      }
    })
  }

  return selected.slice(0, 2)
}

function DatabaseEmptyState({
  title,
  description,
  action,
}: {
  title: string
  description: string
  action?: React.ReactNode
}) {
  return (
    <div className="sidebar-empty">
      <div className="panel-empty-card">
        <span className="panel-empty-eyebrow">Database</span>
        <strong>{title}</strong>
        <p>{description}</p>
        {action}
      </div>
    </div>
  )
}

function stringifyCellValue(value: unknown) {
  if (value == null) return ''
  return String(value)
}

function getColumnDataType(column: SqlColumnMetadata) {
  return (column.dataType || 'TEXT').toUpperCase()
}

function getColumnEditorKind(column: SqlColumnMetadata, rawValue: string): ColumnEditorKind {
  const dataType = getColumnDataType(column)
  const normalizedName = column.name.toLowerCase()

  if (dataType.includes('BOOL')) {
    return 'boolean'
  }

  if (dataType.includes('DATETIME') || dataType.includes('TIMESTAMP')) {
    return 'datetime'
  }

  if (dataType.includes('DATE') && !dataType.includes('TIME')) {
    return 'date'
  }

  if (dataType.includes('TIME') && !dataType.includes('DATE')) {
    return 'time'
  }

  if (dataType.includes('INT')) {
    return 'integer'
  }

  if (
    dataType.includes('REAL')
    || dataType.includes('FLOA')
    || dataType.includes('DOUB')
    || dataType.includes('NUM')
    || dataType.includes('DEC')
  ) {
    return 'number'
  }

  const prefersTextarea = rawValue.includes('\n')
    || rawValue.length > 96
    || /(content|description|message|detail|reason|body|notes?|comment|json|sql)/i.test(normalizedName)
    || dataType.includes('CLOB')
    || dataType.includes('JSON')

  return prefersTextarea ? 'textarea' : 'text'
}

function normalizeBooleanDraftValue(rawValue: string) {
  const normalized = rawValue.trim().toLowerCase()
  if (normalized === 'true' || normalized === '1') return 'true'
  if (normalized === 'false' || normalized === '0') return 'false'
  return normalized.length === 0 ? '' : rawValue
}

function normalizeTemporalDraftValue(rawValue: string, kind: Extract<ColumnEditorKind, 'date' | 'time' | 'datetime'>) {
  const trimmed = rawValue.trim()
  if (trimmed.length === 0) return ''

  if (kind === 'date') {
    const dateMatch = trimmed.match(/^(\d{4}-\d{2}-\d{2})/)
    return dateMatch?.[1] ?? null
  }

  if (kind === 'time') {
    const timeMatch = trimmed.match(/(\d{2}:\d{2})(?::(\d{2}))?/)
    if (!timeMatch) return null
    return timeMatch[2] ? `${timeMatch[1]}:${timeMatch[2]}` : timeMatch[1]
  }

  const dateTimeMatch = trimmed.match(/^(\d{4}-\d{2}-\d{2})[T\s](\d{2}:\d{2})(?::(\d{2}))?/)
  if (!dateTimeMatch) return null
  return dateTimeMatch[3]
    ? `${dateTimeMatch[1]}T${dateTimeMatch[2]}:${dateTimeMatch[3]}`
    : `${dateTimeMatch[1]}T${dateTimeMatch[2]}`
}

function getColumnEditorLabel(kind: ColumnEditorKind) {
  switch (kind) {
    case 'boolean':
      return 'Boolean'
    case 'integer':
      return 'Integer'
    case 'number':
      return 'Number'
    case 'date':
      return 'Date'
    case 'time':
      return 'Time'
    case 'datetime':
      return 'DateTime'
    case 'textarea':
      return 'Long text'
    default:
      return 'Text'
  }
}

function getQueryPreview(sql: string, maxLength = 72) {
  const normalized = sql.replace(/\s+/g, ' ').trim()
  if (normalized.length <= maxLength) {
    return normalized
  }

  return `${normalized.slice(0, maxLength - 1)}…`
}

function getSelectedConsoleSql(textarea: HTMLTextAreaElement | null) {
  if (!textarea) return ''

  const selectionStart = Math.max(0, textarea.selectionStart ?? 0)
  const selectionEnd = Math.max(selectionStart, textarea.selectionEnd ?? selectionStart)
  if (selectionStart === selectionEnd) {
    return ''
  }

  return textarea.value.slice(selectionStart, selectionEnd).trim()
}

function parseDraftValue(rawValue: string, column: SqlColumnMetadata) {
  const trimmed = rawValue.trim()
  if (trimmed.length === 0) {
    return column.nullable ? null : rawValue
  }

  const dataType = getColumnDataType(column)

  if (dataType.includes('BOOL')) {
    if (trimmed.toLowerCase() === 'true') return true
    if (trimmed.toLowerCase() === 'false') return false
    if (trimmed === '1') return true
    if (trimmed === '0') return false
    return rawValue
  }

  if (dataType.includes('INT') && /^-?\d+$/.test(trimmed)) {
    return Number.parseInt(trimmed, 10)
  }

  if (
    (dataType.includes('REAL') || dataType.includes('FLOA') || dataType.includes('DOUB') || dataType.includes('NUM') || dataType.includes('DEC')) &&
    !Number.isNaN(Number(trimmed))
  ) {
    return Number(trimmed)
  }

  return rawValue
}

function getColumnValidationMessage(column: SqlColumnMetadata, rawValue: string) {
  const trimmed = rawValue.trim()
  if (trimmed.length === 0) {
    return column.nullable || column.isPrimaryKey ? null : 'This column is required.'
  }

  const dataType = getColumnDataType(column)
  if (dataType.includes('BOOL') && !['true', 'false', '1', '0'].includes(trimmed.toLowerCase())) {
    return 'Use true, false, 1, or 0.'
  }
  if (dataType.includes('INT') && !/^-?\d+$/.test(trimmed)) {
    return 'Use a whole number.'
  }
  if (
    (dataType.includes('REAL') || dataType.includes('FLOA') || dataType.includes('DOUB') || dataType.includes('NUM') || dataType.includes('DEC')) &&
    Number.isNaN(Number(trimmed))
  ) {
    return 'Use a numeric value.'
  }
  if ((dataType.includes('DATETIME') || dataType.includes('TIMESTAMP')) && normalizeTemporalDraftValue(trimmed, 'datetime') == null) {
    return 'Use a valid date and time.'
  }
  if (dataType.includes('DATE') && !dataType.includes('TIME') && normalizeTemporalDraftValue(trimmed, 'date') == null) {
    return 'Use a valid date.'
  }
  if (dataType.includes('TIME') && !dataType.includes('DATE') && normalizeTemporalDraftValue(trimmed, 'time') == null) {
    return 'Use a valid time.'
  }

  return null
}

function buildDraftFromRow(row: SqlTableRow | null, columns: SqlColumnMetadata[]) {
  return Object.fromEntries(
    columns.map((column) => [column.name, stringifyCellValue(row?.values[column.name])])
  )
}

function collectUpdatedValues(row: SqlTableRow, columns: SqlColumnMetadata[], draft: Record<string, string>) {
  return Object.fromEntries(
    columns
      .filter((column) => !column.isPrimaryKey)
      .flatMap((column) => {
        const nextRawValue = draft[column.name] ?? stringifyCellValue(row.values[column.name])
        const currentRawValue = stringifyCellValue(row.values[column.name])
        if (nextRawValue === currentRawValue) {
          return []
        }

        return [[column.name, parseDraftValue(nextRawValue, column)]]
      })
  )
}

function collectInsertValues(columns: SqlColumnMetadata[], draft: Record<string, string>) {
  return Object.fromEntries(
    columns.flatMap((column) => {
      const rawValue = draft[column.name]
      if (rawValue == null || rawValue.trim().length === 0) {
        return []
      }

      return [[column.name, parseDraftValue(rawValue, column)]]
    })
  )
}

function renderRowDetailInput(
  column: SqlColumnMetadata,
  value: string,
  disabled: boolean,
  validationMessage: string | null,
  onDraftChange: (columnName: string, value: string) => void
) {
  const editorKind = getColumnEditorKind(column, value)
  const inputClassName = `database-detail-input ${validationMessage ? 'invalid' : ''}`

  if (editorKind === 'boolean') {
    const normalizedBooleanValue = normalizeBooleanDraftValue(value)
    return (
      <select
        className={`${inputClassName} database-detail-select`}
        value={normalizedBooleanValue === 'true' || normalizedBooleanValue === 'false' || normalizedBooleanValue === '' ? normalizedBooleanValue : ''}
        disabled={disabled}
        onChange={(event) => onDraftChange(column.name, event.target.value)}
      >
        {column.nullable && <option value="">NULL</option>}
        {!column.nullable && <option value="">Select true/false</option>}
        <option value="true">True</option>
        <option value="false">False</option>
      </select>
    )
  }

  if (editorKind === 'integer' || editorKind === 'number') {
    return (
      <input
        className={inputClassName}
        type="number"
        step={editorKind === 'integer' ? '1' : 'any'}
        value={value}
        disabled={disabled}
        onChange={(event) => onDraftChange(column.name, event.target.value)}
      />
    )
  }

  if (editorKind === 'date' || editorKind === 'time' || editorKind === 'datetime') {
    const normalizedTemporalValue = normalizeTemporalDraftValue(value, editorKind)
    if (normalizedTemporalValue != null) {
      return (
        <input
          className={inputClassName}
          type={editorKind === 'datetime' ? 'datetime-local' : editorKind}
          step={editorKind === 'datetime' ? 1 : undefined}
          value={normalizedTemporalValue}
          disabled={disabled}
          onChange={(event) => onDraftChange(column.name, event.target.value)}
        />
      )
    }
  }

  if (editorKind === 'textarea') {
    return (
      <textarea
        className={`${inputClassName} multiline`}
        value={value}
        disabled={disabled}
        rows={4}
        onChange={(event) => onDraftChange(column.name, event.target.value)}
      />
    )
  }

  return (
    <input
      className={inputClassName}
      type="text"
      value={value}
      disabled={disabled}
      onChange={(event) => onDraftChange(column.name, event.target.value)}
    />
  )
}

function QueryResultView() {
  const queryResult = useSqlStore((s) => s.queryResult)

  if (!queryResult) {
    return (
      <div className="database-results-empty">
        <i className="fa-solid fa-database"></i>
        <strong>Run a query to inspect rows</strong>
        <span>SELECT results will render here, and write queries will report affected rows.</span>
      </div>
    )
  }

  if (!queryResult.success) {
    return (
      <div className="database-results-empty database-results-error">
        <i className="fa-solid fa-triangle-exclamation"></i>
        <strong>Query failed</strong>
        <span>{queryResult.error}</span>
      </div>
    )
  }

  if (queryResult.columns.length === 0) {
    return (
      <div className="database-results-empty">
        <i className="fa-solid fa-check"></i>
        <strong>{queryResult.message ?? 'Statement executed successfully.'}</strong>
        <span>{queryResult.rowCount === 1 ? '1 row affected.' : `${queryResult.rowCount} rows affected.`}</span>
      </div>
    )
  }

  const exportColumns = queryResult.columns.map<ExportColumn>((column) => ({
    key: column,
    label: column,
  }))
  const baseName = 'query-results'

  const handleExportCsv = async () => {
    try {
      await exportRowsAsCsv(baseName, exportColumns, queryResult.rows)
      showSuccessToast('Exported the current query result to CSV.', 'Export complete')
    } catch (error) {
      showErrorToast((error as Error).message || 'Unable to export the query result as CSV.', 'CSV export failed')
    }
  }

  const handleExportXlsx = async () => {
    try {
      await exportRowsAsXlsx(baseName, exportColumns, queryResult.rows)
      showSuccessToast('Exported the current query result to XLSX.', 'Export complete')
    } catch (error) {
      showErrorToast((error as Error).message || 'Unable to export the query result as XLSX.', 'XLSX export failed')
    }
  }

  return (
    <div className="database-result-surface">
      <div className="database-browser-toolbar">
        <span className="database-meta-text">
          {queryResult.rowCount} rows • {queryResult.columns.length} columns • {queryResult.durationMs} ms
        </span>
        <div className="database-preset-controls">
          <button
            type="button"
            className="database-secondary-btn"
            onClick={() => void handleExportCsv()}
            title="Export the current query result rows as CSV"
          >
            <i className="fa-solid fa-file-csv"></i>
            Export CSV
          </button>
          <button
            type="button"
            className="database-secondary-btn"
            onClick={() => void handleExportXlsx()}
            title="Export the current query result rows as XLSX"
          >
            <i className="fa-solid fa-file-excel"></i>
            Export XLSX
          </button>
        </div>
      </div>

      <div className="database-table-wrap">
        <table className="database-table">
          <thead>
            <tr>
              {queryResult.columns.map((column) => (
                <th key={column}>{column}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {queryResult.rows.map((row, index) => (
              <tr key={`${index}-${queryResult.columns.join('|')}`}>
                {queryResult.columns.map((column) => (
                  <td key={`${index}-${column}`}>{String(row[column] ?? '')}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function RowDetailDrawer({
  open,
  mode,
  row,
  draft,
  columns,
  writable,
  onClose,
  onDraftChange,
  onSave,
  onDelete,
  isBusy,
}: {
  open: boolean
  mode: RowDrawerMode
  row: SqlTableRow | null
  draft: Record<string, string>
  columns: SqlColumnMetadata[]
  writable: boolean
  onClose: () => void
  onDraftChange: (columnName: string, value: string) => void
  onSave: () => void
  onDelete: () => void
  isBusy: boolean
}) {
  if (!open) return null

  return (
    <aside className="database-detail-drawer">
      <div className="database-detail-head">
        <div className="database-detail-copy">
          <span className="panel-empty-eyebrow">Row Detail</span>
          <strong>
            {mode === 'insert' ? 'New Row' : mode === 'edit' ? 'Edit Row' : 'Inspect Row'}
          </strong>
          <span className="database-meta-text">
            {writable ? 'Validation follows column metadata.' : 'This result is read only.'}
          </span>
        </div>
        <button type="button" className="database-secondary-btn" onClick={onClose}>
          Close
        </button>
      </div>

      <div className="database-detail-fields">
        {columns.map((column) => {
          const value = draft[column.name] ?? ''
          const validationMessage = getColumnValidationMessage(column, value)
          const disabled = !writable || (mode !== 'insert' && column.isPrimaryKey)
          const editorKind = getColumnEditorKind(column, value)

          return (
            <label key={column.name} className="database-detail-field">
              <span className="database-detail-label">
                {column.name}
                <em>{column.dataType || 'TEXT'}</em>
              </span>
              {renderRowDetailInput(column, value, disabled, validationMessage, onDraftChange)}
              <span className="database-detail-meta">
                {column.isPrimaryKey ? 'Primary key' : column.nullable ? 'Nullable' : 'Required'} • {getColumnEditorLabel(editorKind)}
              </span>
              {validationMessage && <span className="database-detail-error">{validationMessage}</span>}
            </label>
          )
        })}
      </div>

      <div className="database-detail-actions">
        {writable && mode !== 'view' && (
          <button type="button" className="database-primary-btn" onClick={onSave} disabled={isBusy}>
            {mode === 'insert' ? 'Insert Row' : 'Save Changes'}
          </button>
        )}
        {writable && mode === 'edit' && row?.locator && (
          <button type="button" className="database-danger-btn" onClick={onDelete} disabled={isBusy}>
            Delete Row
          </button>
        )}
      </div>
    </aside>
  )
}

interface ColumnContextMenuProps {
  x: number
  y: number
  columnName: string
  canMoveLeft: boolean
  canMoveRight: boolean
  isPinned: boolean
  onClose: () => void
  onMoveLeft: () => void
  onMoveRight: () => void
  onTogglePin: () => void
  onHide: () => void
}

function ColumnContextMenu({
  x,
  y,
  columnName,
  canMoveLeft,
  canMoveRight,
  isPinned,
  onClose,
  onMoveLeft,
  onMoveRight,
  onTogglePin,
  onHide,
}: ColumnContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handlePointerDown = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onClose()
      }
    }

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose()
      }
    }

    window.addEventListener('mousedown', handlePointerDown)
    window.addEventListener('keydown', handleEscape)

    return () => {
      window.removeEventListener('mousedown', handlePointerDown)
      window.removeEventListener('keydown', handleEscape)
    }
  }, [onClose])

  const maxX = typeof window === 'undefined' ? x : Math.max(12, window.innerWidth - 210)
  const maxY = typeof window === 'undefined' ? y : Math.max(12, window.innerHeight - 220)
  const left = Math.min(Math.max(12, x), maxX)
  const top = Math.min(Math.max(12, y), maxY)

  return createPortal(
    <div
      ref={menuRef}
      className="context-menu database-column-context-menu"
      style={{ left, top }}
      onClick={(event) => event.stopPropagation()}
    >
      <button
        type="button"
        className="context-menu-item database-column-context-item"
        onClick={() => {
          onMoveLeft()
          onClose()
        }}
        disabled={!canMoveLeft}
      >
        <i className="fa-solid fa-arrow-left"></i>
        Move {columnName} Left
      </button>
      <button
        type="button"
        className="context-menu-item database-column-context-item"
        onClick={() => {
          onMoveRight()
          onClose()
        }}
        disabled={!canMoveRight}
      >
        <i className="fa-solid fa-arrow-right"></i>
        Move {columnName} Right
      </button>
      <button
        type="button"
        className="context-menu-item database-column-context-item"
        onClick={() => {
          onTogglePin()
          onClose()
        }}
      >
        <i className="fa-solid fa-thumbtack"></i>
        {isPinned ? `Unpin ${columnName}` : `Pin ${columnName}`}
      </button>
      <div className="context-menu-separator"></div>
      <button
        type="button"
        className="context-menu-item database-column-context-item"
        onClick={() => {
          onHide()
          onClose()
        }}
      >
        <i className="fa-solid fa-eye-slash"></i>
        Hide {columnName}
      </button>
    </div>,
    document.body
  )
}

function ToolbarMenu({
  icon,
  label,
  children,
}: {
  icon: string
  label: string
  children: React.ReactNode
}) {
  return (
    <details className="database-toolbar-menu">
      <summary className="database-secondary-btn">
        <i className={`fa-solid ${icon}`}></i>
        {label}
      </summary>
      <div className="database-toolbar-popover">
        {children}
      </div>
    </details>
  )
}

function BrowserGrid({ tableRows }: { tableRows: SqlTableRowsResult }) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const browse = useSqlStore((s) => s.browse)
  const setBrowsePage = useSqlStore((s) => s.setBrowsePage)
  const setBrowsePageSize = useSqlStore((s) => s.setBrowsePageSize)
  const setBrowseSort = useSqlStore((s) => s.setBrowseSort)
  const setBrowseFilters = useSqlStore((s) => s.setBrowseFilters)
  const applyBrowseState = useSqlStore((s) => s.applyBrowseState)
  const updateTableRow = useSqlStore((s) => s.updateTableRow)
  const insertTableRow = useSqlStore((s) => s.insertTableRow)
  const deleteTableRow = useSqlStore((s) => s.deleteTableRow)
  const isMutatingRows = useSqlStore((s) => s.isMutatingRows)
  const [filterDrafts, setFilterDrafts] = useState<SqlBrowseFilter[]>(getFilterDrafts(tableRows.filters))
  const [drawerState, setDrawerState] = useState<RowDrawerState | null>(null)
  const [compactMode, setCompactMode] = useState(false)
  const [pinnedColumnNames, setPinnedColumnNames] = useState<string[]>([])
  const [hiddenColumnNames, setHiddenColumnNames] = useState<string[]>([])
  const [columnOrder, setColumnOrder] = useState<string[]>([])
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({})
  const [presets, setPresets] = useState<DatabaseTablePreset[]>([])
  const [activePresetId, setActivePresetId] = useState<string | null>(null)
  const [workspaceLibraryPresets, setWorkspaceLibraryPresets] = useState<DatabaseWorkspaceLibraryPreset[]>([])
  const [selectedWorkspacePresetId, setSelectedWorkspacePresetId] = useState<string | null>(null)
  const [exportingMode, setExportingMode] = useState<string | null>(null)
  const [exportColumnMode, setExportColumnMode] = useState<ExportColumnMode>('visible')
  const [selectedExportColumnNames, setSelectedExportColumnNames] = useState<string[]>([])
  const [showExportColumnPicker, setShowExportColumnPicker] = useState(false)
  const [quickExportFormat, setQuickExportFormat] = useState<ExportFormat>(readExportTemplateFormatPreference)
  const [openColumnMenu, setOpenColumnMenu] = useState<ColumnMenuState | null>(null)
  const resizeStateRef = useRef<{ columnName: string; startX: number; startWidth: number } | null>(null)
  const defaultPinnedColumnNames = useMemo(() => getDefaultPinnedColumnNames(tableRows.columns), [tableRows.columns])

  useEffect(() => {
    writeCompactModePreference(compactMode)
  }, [compactMode])

  useEffect(() => {
    writeExportTemplateFormatPreference(quickExportFormat)
  }, [quickExportFormat])

  useEffect(() => {
    setFilterDrafts(getFilterDrafts(tableRows.filters))
    setDrawerState(null)
  }, [tableRows.tableId, tableRows.filters])

  useEffect(() => {
    const layout = readDatabaseTableLayout(tableRows.connectionId, tableRows.tableId, tableRows.columns)
    const presetState = readDatabaseTablePresets(tableRows.connectionId, tableRows.tableId, tableRows.columns)
    setCompactMode(layout.compactMode)
    setPinnedColumnNames(layout.pinnedColumns)
    setHiddenColumnNames(layout.hiddenColumns)
    setColumnOrder(layout.columnOrder)
    setColumnWidths(layout.columnWidths)
    setPresets(presetState.presets)
    setActivePresetId(presetState.activePresetId)
  }, [defaultPinnedColumnNames, tableRows.columns, tableRows.connectionId, tableRows.tableId])

  useEffect(() => {
    setShowExportColumnPicker(false)
  }, [tableRows.tableId])

  useEffect(() => {
    setOpenColumnMenu(null)
  }, [tableRows.tableId])

  useEffect(() => {
    const nextWorkspaceLibraryPresets = readWorkspaceLibraryPresets(rootPath)
    setWorkspaceLibraryPresets(nextWorkspaceLibraryPresets)
    setSelectedWorkspacePresetId((current) => (
      current && nextWorkspaceLibraryPresets.some((preset) => preset.id === current)
        ? current
        : null
    ))
  }, [rootPath])

  useEffect(() => {
    writeDatabaseTableLayout(tableRows.connectionId, tableRows.tableId, {
      compactMode,
      pinnedColumns: sanitizePinnedColumnNames(pinnedColumnNames, tableRows.columns),
      hiddenColumns: sanitizeHiddenColumnNames(hiddenColumnNames, tableRows.columns),
      columnOrder: sanitizeColumnOrder(columnOrder, tableRows.columns),
      columnWidths: sanitizeColumnWidths(columnWidths, tableRows.columns),
    }, tableRows.columns)
  }, [columnOrder, columnWidths, compactMode, hiddenColumnNames, pinnedColumnNames, tableRows.columns, tableRows.connectionId, tableRows.tableId])

  useEffect(() => {
    const handleMouseMove = (event: MouseEvent) => {
      const resizeState = resizeStateRef.current
      if (!resizeState) return

      const nextWidth = clampColumnWidth(resizeState.startWidth + (event.clientX - resizeState.startX))
      setColumnWidths((current) => ({
        ...current,
        [resizeState.columnName]: nextWidth,
      }))
    }

    const handleMouseUp = () => {
      if (!resizeStateRef.current) return
      resizeStateRef.current = null
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }

    window.addEventListener('mousemove', handleMouseMove)
    window.addEventListener('mouseup', handleMouseUp)

    return () => {
      window.removeEventListener('mousemove', handleMouseMove)
      window.removeEventListener('mouseup', handleMouseUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
  }, [])

  if (tableRows.error) {
    return (
      <div className="database-results-empty database-results-error">
        <i className="fa-solid fa-triangle-exclamation"></i>
        <strong>Table browser failed</strong>
        <span>{tableRows.error}</span>
      </div>
    )
  }

  const safeTotalRows = Number.isFinite(tableRows.totalRows) ? tableRows.totalRows : 0
  const safeLimit = Math.max(1, tableRows.limit ?? browse.pageSize)
  const safeOffset = Math.max(0, tableRows.offset ?? 0)
  const totalPages = Math.max(1, Math.ceil(safeTotalRows / safeLimit))
  const currentPage = Math.floor(safeOffset / safeLimit) + 1
  const normalizedColumnWidths = useMemo(() => sanitizeColumnWidths(columnWidths, tableRows.columns), [columnWidths, tableRows.columns])
  const orderedColumns = useMemo(() => orderColumns(tableRows.columns, columnOrder), [columnOrder, tableRows.columns])
  const visibleColumns = useMemo(() => {
    const hidden = new Set(sanitizeHiddenColumnNames(hiddenColumnNames, orderedColumns))
    return orderedColumns.filter((column) => !hidden.has(column.name))
  }, [hiddenColumnNames, orderedColumns])
  const pinnedColumns = useMemo(() => {
    const sanitizedPinnedColumns = sanitizePinnedColumnNames(pinnedColumnNames, visibleColumns)
    const manualPinned = sanitizedPinnedColumns
      .map((name) => visibleColumns.find((column) => column.name === name))
      .filter((column): column is SqlColumnMetadata => column != null)

    return manualPinned.length > 0 ? manualPinned : []
  }, [pinnedColumnNames, visibleColumns])
  const pinnedColumnMeta = useMemo(() => {
    let left = 0

    return Object.fromEntries(
      pinnedColumns.map((column, index) => {
        const width = normalizedColumnWidths[column.name] ?? getPinnedColumnWidth(column)
        const meta: PinnedColumnMeta = {
          left,
          width,
          isEdge: index === pinnedColumns.length - 1,
        }
        left += width
        return [column.name, meta]
      })
    ) as Record<string, PinnedColumnMeta>
  }, [normalizedColumnWidths, pinnedColumns])
  const hiddenColumnsCount = tableRows.columns.length - visibleColumns.length
  const menuColumnName = openColumnMenu?.columnName ?? null
  const menuColumnIndex = menuColumnName ? visibleColumns.findIndex((column) => column.name === menuColumnName) : -1
  const menuColumn = menuColumnIndex >= 0 ? visibleColumns[menuColumnIndex] : null
  const menuPinnedMeta = menuColumn ? pinnedColumnMeta[menuColumn.name] : undefined
  const menuCanMoveLeft = menuColumnIndex > 0
  const menuCanMoveRight = menuColumnIndex >= 0 && menuColumnIndex < visibleColumns.length - 1
  const hasCustomColumnOrder = sanitizeColumnOrder(columnOrder, tableRows.columns).join('|') !== tableRows.columns.map((column) => column.name).join('|')
  const hasCustomColumnWidths = Object.keys(normalizedColumnWidths).length > 0
  const savedLayoutActive = hasCustomColumnWidths || hasCustomColumnOrder || hiddenColumnNames.length > 0 || pinnedColumnNames.length > 0 || compactMode || browse.pageSize !== 25 || browse.sortColumn != null || browse.filters.length > 1 || (browse.filters[0] && (browse.filters[0].column != null || (browse.filters[0].value ?? '').trim().length > 0 || browse.filters[0].operator !== 'contains'))
  const tableExportRows = tableRows.rows.map((row) => row.values)
  const appliedBrowseFilters = useMemo(() => getAppliedBrowseFilters(tableRows.filters ?? []), [tableRows.filters])

  useEffect(() => {
    setSelectedExportColumnNames((current) => {
      const availableNames = new Set(orderedColumns.map((column) => column.name))
      const nextSelection = current.filter((name) => availableNames.has(name))
      if (nextSelection.length > 0) {
        return nextSelection
      }

      return visibleColumns.map((column) => column.name)
    })
  }, [orderedColumns, visibleColumns])

  const getExportSourceColumns = (mode: ExportColumnMode) => {
    if (mode === 'all') {
      return orderedColumns
    }

    if (mode === 'selected') {
      const selectedNames = new Set(selectedExportColumnNames)
      return orderedColumns.filter((column) => selectedNames.has(column.name))
    }

    return visibleColumns
  }

  const getExportDescriptor = (mode: ExportColumnMode) => {
    const sourceColumns = getExportSourceColumns(mode)
    const columns = sourceColumns.map<ExportColumn>((column) => ({
      key: column.name,
      label: column.name,
    }))

    return {
      columns,
      scopeSuffix: mode === 'all'
        ? 'all-columns'
        : mode === 'selected'
          ? 'selected-columns'
          : 'visible-columns',
      modeLabel: mode === 'all'
        ? 'all columns'
        : mode === 'selected'
          ? 'selected columns'
          : 'visible columns',
    }
  }

  const exportDescriptor = getExportDescriptor(exportColumnMode)
  const exportColumns = exportDescriptor.columns

  const getColumnCellStyle = (column: SqlColumnMetadata, pinnedMeta?: PinnedColumnMeta) => {
    const width = normalizedColumnWidths[column.name] ?? (pinnedMeta ? getPinnedColumnWidth(column) : null)

    if (width == null && !pinnedMeta) {
      return undefined
    }

    return {
      ...(pinnedMeta ? { left: `${pinnedMeta.left}px` } : {}),
      ...(width != null
        ? {
            minWidth: `${width}px`,
            width: `${width}px`,
          }
        : {}),
    }
  }

  const getCurrentLayoutSnapshot = (): DatabaseTableLayoutPreference => ({
    compactMode,
    pinnedColumns: sanitizePinnedColumnNames(pinnedColumnNames, tableRows.columns),
    hiddenColumns: sanitizeHiddenColumnNames(hiddenColumnNames, tableRows.columns),
    columnOrder: sanitizeColumnOrder(columnOrder, tableRows.columns),
    columnWidths: sanitizeColumnWidths(columnWidths, tableRows.columns),
  })

  const getCurrentBrowseSnapshot = (): DatabaseTablePresetBrowseState => ({
    pageSize: browse.pageSize,
    sortColumn: browse.sortColumn,
    sortDirection: browse.sortDirection,
    filters: clonePresetBrowseFilters(browse.filters),
  })

  const runTableExport = async (
    rowScope: ExportRowScope,
    columnMode: ExportColumnMode,
    format: ExportFormat,
    triggerId: string
  ) => {
    const descriptor = getExportDescriptor(columnMode)
    const baseName = rowScope === 'page'
      ? `${tableRows.tableId}-${descriptor.scopeSuffix}-page-${currentPage}`
      : `${tableRows.tableId}-${descriptor.scopeSuffix}-filtered-all`

    if (descriptor.columns.length === 0) {
      showInfoToast('Choose at least one export column before exporting this table.', 'No columns selected')
      return
    }

    try {
      setExportingMode(triggerId)
      const rows = rowScope === 'page' ? tableExportRows : await fetchAllFilteredRows()
      const scopeLabel = rowScope === 'page' ? 'the current page' : `${rows.length} filtered rows`

      if (format === 'csv') {
        await exportRowsAsCsv(baseName, descriptor.columns, rows)
      } else {
        await exportRowsAsXlsx(baseName, descriptor.columns, rows)
      }

      showSuccessToast(`Exported ${scopeLabel} using ${descriptor.modeLabel} to ${format.toUpperCase()}.`, 'Export complete')
    } catch (error) {
      const rowScopeLabel = rowScope === 'page' ? 'the current table page' : 'all filtered rows'
      showErrorToast(
        (error as Error).message || `Unable to export ${rowScopeLabel} as ${format.toUpperCase()}.`,
        `${format.toUpperCase()} export failed`
      )
    } finally {
      setExportingMode(null)
    }
  }

  const handleExportCurrentPageCsv = async () => {
    await runTableExport('page', exportColumnMode, 'csv', 'page-csv')
  }

  const handleExportCurrentPageXlsx = async () => {
    await runTableExport('page', exportColumnMode, 'xlsx', 'page-xlsx')
  }

  const fetchAllFilteredRows = async () => {
    const chunkSize = 1000
    const rows: Array<Record<string, unknown>> = []
    const totalRowsToFetch = Math.max(0, tableRows.totalRows)

    if (totalRowsToFetch === 0) {
      return rows
    }

    for (let offset = 0; offset < totalRowsToFetch; offset += chunkSize) {
      const chunk = await window.electronAPI.getSqlTableRows({
        connectionId: tableRows.connectionId,
        tableId: tableRows.tableId,
        limit: Math.min(chunkSize, totalRowsToFetch - offset),
        offset,
        sortColumn: tableRows.sortColumn,
        sortDirection: tableRows.sortDirection,
        filters: appliedBrowseFilters,
      })

      if (chunk.error) {
        throw new Error(chunk.error)
      }

      rows.push(...chunk.rows.map((row) => row.values))
    }

    return rows
  }

  const handleExportAllFilteredCsv = async () => {
    await runTableExport('all', exportColumnMode, 'csv', 'all-csv')
  }

  const handleExportAllFilteredXlsx = async () => {
    await runTableExport('all', exportColumnMode, 'xlsx', 'all-xlsx')
  }

  const handleExportTemplate = async (template: ExportTemplateDefinition) => {
    await runTableExport(
      template.rowScope,
      template.columnMode,
      quickExportFormat,
      `template-${template.id}-${quickExportFormat}`
    )
  }

  const applyStateSnapshot = async (layout: DatabaseTableLayoutPreference, browseSnapshot: DatabaseTablePresetBrowseState) => {
    const sanitizedLayout = sanitizeDatabaseTableLayout(layout, tableRows.columns)
    const sanitizedBrowse = sanitizePresetBrowseState(browseSnapshot, tableRows.columns)

    setCompactMode(sanitizedLayout.compactMode)
    setPinnedColumnNames(sanitizedLayout.pinnedColumns)
    setHiddenColumnNames(sanitizedLayout.hiddenColumns)
    setColumnOrder(sanitizedLayout.columnOrder)
    setColumnWidths(sanitizedLayout.columnWidths)
    setFilterDrafts(getFilterDrafts(sanitizedBrowse.filters))

    await applyBrowseState({
      pageSize: sanitizedBrowse.pageSize,
      sortColumn: sanitizedBrowse.sortColumn,
      sortDirection: sanitizedBrowse.sortDirection,
      filters: sanitizedBrowse.filters,
      pageIndex: 0,
    })
  }

  const togglePinnedColumn = (columnName: string) => {
    setPinnedColumnNames((current) => {
      if (current.includes(columnName)) {
        return current.filter((name) => name !== columnName)
      }

      return [...current, columnName]
    })
  }

  const restoreAutoPins = () => {
    setPinnedColumnNames(defaultPinnedColumnNames)
  }

  const toggleHiddenColumn = (columnName: string) => {
    setHiddenColumnNames((current) => {
      if (current.includes(columnName)) {
        return current.filter((name) => name !== columnName)
      }

      return [...current, columnName]
    })
  }

  const showAllColumns = () => {
    setHiddenColumnNames([])
  }

  const moveColumn = (columnName: string, direction: 'left' | 'right') => {
    setColumnOrder((current) => {
      const next = sanitizeColumnOrder(current, tableRows.columns)
      const hiddenSet = new Set(sanitizeHiddenColumnNames(hiddenColumnNames, tableRows.columns))
      const visibleOrder = next.filter((name) => !hiddenSet.has(name))
      const index = visibleOrder.indexOf(columnName)
      if (index < 0) return next

      const targetIndex = direction === 'left' ? index - 1 : index + 1
      if (targetIndex < 0 || targetIndex >= visibleOrder.length) return next

      const reorderedVisible = [...visibleOrder]
      const [item] = reorderedVisible.splice(index, 1)
      reorderedVisible.splice(targetIndex, 0, item)

      let visiblePointer = 0
      return next.map((name) => {
        if (hiddenSet.has(name)) {
          return name
        }

        const replacement = reorderedVisible[visiblePointer]
        visiblePointer += 1
        return replacement
      })
    })
  }

  const resetColumnOrder = () => {
    setColumnOrder(tableRows.columns.map((column) => column.name))
  }

  const beginColumnResize = (event: React.MouseEvent<HTMLButtonElement>, column: SqlColumnMetadata) => {
    event.preventDefault()
    event.stopPropagation()

    const headerCell = event.currentTarget.closest('th')
    const startWidth = headerCell
      ? clampColumnWidth(headerCell.getBoundingClientRect().width)
      : normalizedColumnWidths[column.name] ?? getPinnedColumnWidth(column)

    resizeStateRef.current = {
      columnName: column.name,
      startX: event.clientX,
      startWidth,
    }

    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
  }

  const resetColumnWidth = (columnName: string) => {
    setColumnWidths((current) => {
      if (!(columnName in current)) {
        return current
      }

      const next = { ...current }
      delete next[columnName]
      return next
    })
  }

  const resetAllColumnWidths = () => {
    setColumnWidths({})
  }

  const persistPresetState = (nextPresets: DatabaseTablePreset[], nextActivePresetId: string | null) => {
    setPresets(nextPresets)
    setActivePresetId(nextActivePresetId)
    writeDatabaseTablePresets(
      tableRows.connectionId,
      tableRows.tableId,
      {
        activePresetId: nextActivePresetId,
        presets: nextPresets,
      },
      tableRows.columns
    )
  }

  const persistWorkspaceLibraryState = (nextPresets: DatabaseWorkspaceLibraryPreset[], nextSelectedPresetId: string | null) => {
    setWorkspaceLibraryPresets(nextPresets)
    setSelectedWorkspacePresetId(nextSelectedPresetId)
    writeWorkspaceLibraryPresets(rootPath, nextPresets)
  }

  const applyPreset = async (preset: DatabaseTablePreset, nextPresets: DatabaseTablePreset[] = presets) => {
    persistPresetState(nextPresets, preset.id)
    await applyStateSnapshot(preset.layout, preset.browse)
  }

  const handleSavePreset = async () => {
    const initialValue = activePresetId
      ? presets.find((preset) => preset.id === activePresetId)?.name ?? ''
      : `Preset ${presets.length + 1}`
    const nextName = window.prompt('Preset name', initialValue)?.trim()

    if (!nextName) {
      return
    }

    const existingPreset = presets.find((preset) => preset.name.toLowerCase() === nextName.toLowerCase())
    if (existingPreset && existingPreset.id !== activePresetId) {
      const shouldOverwrite = await confirmAction({
        title: 'Overwrite preset?',
        message: `A preset named "${existingPreset.name}" already exists for this table.`,
        confirmLabel: 'Overwrite',
        cancelLabel: 'Cancel',
        tone: 'warning',
      })

      if (!shouldOverwrite) {
        return
      }
    }

    const now = new Date().toISOString()
    const currentLayout = getCurrentLayoutSnapshot()
    const currentBrowse = getCurrentBrowseSnapshot()
    const presetId = existingPreset?.id ?? createDatabaseTablePresetId()
    const nextPreset: DatabaseTablePreset = {
      id: presetId,
      name: nextName,
      createdAt: existingPreset?.createdAt ?? now,
      updatedAt: now,
      layout: currentLayout,
      browse: currentBrowse,
    }

    const nextPresets = existingPreset
      ? presets.map((preset) => (preset.id === existingPreset.id ? nextPreset : preset))
      : [...presets, nextPreset]

    persistPresetState(nextPresets, nextPreset.id)
    showSuccessToast(`Saved preset "${nextName}" for this table.`, 'Preset saved')
  }

  const handleUpdatePreset = () => {
    if (!activePresetId) return

    const activePreset = presets.find((preset) => preset.id === activePresetId)
    if (!activePreset) return

    const now = new Date().toISOString()
    const nextPreset: DatabaseTablePreset = {
      ...activePreset,
      updatedAt: now,
      layout: getCurrentLayoutSnapshot(),
      browse: getCurrentBrowseSnapshot(),
    }

    const nextPresets = presets.map((preset) => (preset.id === activePreset.id ? nextPreset : preset))
    persistPresetState(nextPresets, nextPreset.id)
    showSuccessToast(`Updated preset "${nextPreset.name}".`, 'Preset updated')
  }

  const handleDeletePreset = async () => {
    if (!activePresetId) return

    const activePreset = presets.find((preset) => preset.id === activePresetId)
    if (!activePreset) return

    const shouldDelete = await confirmAction({
      title: 'Delete preset?',
      message: `Remove the preset "${activePreset.name}" from this table?`,
      confirmLabel: 'Delete preset',
      cancelLabel: 'Keep preset',
      tone: 'warning',
    })

    if (!shouldDelete) {
      return
    }

    const nextPresets = presets.filter((preset) => preset.id !== activePreset.id)
    persistPresetState(nextPresets, null)
    showSuccessToast(`Deleted preset "${activePreset.name}".`, 'Preset removed')
  }

  const selectedPreset = activePresetId ? presets.find((preset) => preset.id === activePresetId) ?? null : null
  const selectedWorkspacePreset = selectedWorkspacePresetId
    ? workspaceLibraryPresets.find((preset) => preset.id === selectedWorkspacePresetId) ?? null
    : null

  const handleExportPreset = async () => {
    const fallbackName = `${tableRows.tableId}-layout`
    const presetToExport: DatabaseTablePreset = selectedPreset ?? {
      id: createDatabaseTablePresetId(),
      name: fallbackName,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      layout: getCurrentLayoutSnapshot(),
      browse: getCurrentBrowseSnapshot(),
    }

    const payload = buildPresetExportPayload(tableRows.connectionId, tableRows.tableId, presetToExport)
    const json = JSON.stringify(payload, null, 2)
    const safeFileName = `${presetToExport.name.replace(/[^a-z0-9-_]+/gi, '-').replace(/^-+|-+$/g, '') || fallbackName}.json`

    downloadJsonFile(safeFileName, json)

    let copiedToClipboard = false
    if (navigator.clipboard?.writeText) {
      try {
        await navigator.clipboard.writeText(json)
        copiedToClipboard = true
      } catch {
        copiedToClipboard = false
      }
    }

    showSuccessToast(
      copiedToClipboard
        ? `Exported "${presetToExport.name}" to JSON and copied it to the clipboard.`
        : `Exported "${presetToExport.name}" to a JSON file.`,
      'Preset exported'
    )
  }

  const handleImportPreset = async () => {
    const file = await pickJsonFile()
    if (!file) return

    try {
      const importedPreset = parseImportedPresetPayload(await file.text(), tableRows.columns)
      const duplicateByName = presets.find((preset) => preset.name.toLowerCase() === importedPreset.name.toLowerCase())

      let nextPreset = {
        ...importedPreset,
        id: createDatabaseTablePresetId(),
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      }
      let nextPresets = presets

      if (duplicateByName) {
        const shouldOverwrite = await confirmAction({
          title: 'Overwrite preset?',
          message: `A preset named "${duplicateByName.name}" already exists for this table.`,
          confirmLabel: 'Overwrite',
          cancelLabel: 'Keep both',
          tone: 'warning',
        })

        if (shouldOverwrite) {
          nextPreset = {
            ...nextPreset,
            id: duplicateByName.id,
            createdAt: duplicateByName.createdAt,
          }
          nextPresets = presets.map((preset) => (preset.id === duplicateByName.id ? nextPreset : preset))
        } else {
          const suffixBase = `${importedPreset.name} (Imported)`
          let suffixIndex = 1
          let uniqueName = suffixBase
          while (presets.some((preset) => preset.name.toLowerCase() === uniqueName.toLowerCase())) {
            suffixIndex += 1
            uniqueName = `${suffixBase} ${suffixIndex}`
          }
          nextPreset = {
            ...nextPreset,
            name: uniqueName,
          }
          nextPresets = [...presets, nextPreset]
        }
      } else {
        nextPresets = [...presets, nextPreset]
      }

      await applyPreset(nextPreset, nextPresets)
      showSuccessToast(`Imported preset "${nextPreset.name}" for this table.`, 'Preset imported')
    } catch (error) {
      showErrorToast((error as Error).message || 'Unable to import the preset JSON.', 'Preset import failed')
    }
  }

  const handleSaveToWorkspaceLibrary = async () => {
    if (!rootPath) {
      showInfoToast('Open a workspace first before saving to the shared preset library.', 'No workspace')
      return
    }

    const defaultName = selectedPreset?.name ?? `${tableRows.tableId} view`
    const nextName = window.prompt('Workspace preset name', defaultName)?.trim()

    if (!nextName) {
      return
    }

    const existingPreset = workspaceLibraryPresets.find((preset) => preset.name.toLowerCase() === nextName.toLowerCase())
    if (existingPreset) {
      const shouldOverwrite = await confirmAction({
        title: 'Overwrite workspace preset?',
        message: `A workspace preset named "${existingPreset.name}" already exists.`,
        confirmLabel: 'Overwrite',
        cancelLabel: 'Cancel',
        tone: 'warning',
      })

      if (!shouldOverwrite) {
        return
      }
    }

    const now = new Date().toISOString()
    const nextPreset: DatabaseWorkspaceLibraryPreset = {
      id: existingPreset?.id ?? createDatabaseTablePresetId(),
      name: nextName,
      createdAt: existingPreset?.createdAt ?? now,
      updatedAt: now,
      source: {
        connectionId: tableRows.connectionId,
        tableId: tableRows.tableId,
      },
      layout: getCurrentLayoutSnapshot(),
      browse: getCurrentBrowseSnapshot(),
    }

    const nextPresets = existingPreset
      ? workspaceLibraryPresets.map((preset) => (preset.id === existingPreset.id ? nextPreset : preset))
      : [...workspaceLibraryPresets, nextPreset]

    persistWorkspaceLibraryState(nextPresets, nextPreset.id)
    showSuccessToast(`Saved "${nextName}" to the workspace preset library.`, 'Workspace preset saved')
  }

  const handleApplyWorkspacePreset = async () => {
    if (!selectedWorkspacePresetId) return

    const preset = workspaceLibraryPresets.find((entry) => entry.id === selectedWorkspacePresetId)
    if (!preset) return

    persistPresetState(presets, null)
    persistWorkspaceLibraryState(workspaceLibraryPresets, preset.id)
    await applyStateSnapshot(preset.layout, preset.browse)
    showSuccessToast(`Applied workspace preset "${preset.name}".`, 'Workspace preset applied')
  }

  const handleDeleteWorkspacePreset = async () => {
    if (!selectedWorkspacePresetId) return

    const preset = workspaceLibraryPresets.find((entry) => entry.id === selectedWorkspacePresetId)
    if (!preset) return

    const shouldDelete = await confirmAction({
      title: 'Delete workspace preset?',
      message: `Remove "${preset.name}" from the shared workspace library?`,
      confirmLabel: 'Delete preset',
      cancelLabel: 'Keep preset',
      tone: 'warning',
    })

    if (!shouldDelete) {
      return
    }

    const nextPresets = workspaceLibraryPresets.filter((entry) => entry.id !== preset.id)
    persistWorkspaceLibraryState(nextPresets, null)
    showSuccessToast(`Deleted workspace preset "${preset.name}".`, 'Workspace preset removed')
  }

  const handleApplyFilter = () => {
    const nextFilters = filterDrafts.map((filter) => ({
      column: filter.column,
      operator: filter.operator,
      value: filterOperatorUsesValue(filter.operator) ? (filter.value ?? '') : '',
    }))
    void setBrowseFilters(nextFilters)
  }

  const openDrawer = (mode: RowDrawerMode, row: SqlTableRow | null) => {
    setDrawerState({
      mode,
      row,
      draft: buildDraftFromRow(row, tableRows.columns),
    })
  }

  const handleSaveDrawer = async () => {
    if (!drawerState) return

    if (drawerState.mode === 'insert') {
      const values = collectInsertValues(tableRows.columns, drawerState.draft)
      const result = await insertTableRow(values)
      if (result?.success) {
        setDrawerState(null)
      }
      return
    }

    if (!drawerState.row?.locator) return
    const values = collectUpdatedValues(drawerState.row, tableRows.columns, drawerState.draft)
    const result = await updateTableRow(drawerState.row.locator, values)
    if (result?.success) {
      setDrawerState(null)
    }
  }

  const handleDeleteFromDrawer = async () => {
    if (!drawerState?.row?.locator) return
    const confirmed = await confirmAction({
      title: 'Delete this row?',
      message: 'This will permanently remove the selected row from the SQLite table.',
      confirmLabel: 'Delete Row',
      cancelLabel: 'Keep Row',
      tone: 'warning',
    })

    if (!confirmed) return
    const result = await deleteTableRow(drawerState.row.locator)
    if (result?.success) {
      setDrawerState(null)
    }
  }

  return (
    <div className={`database-browser-shell ${drawerState ? 'drawer-open' : ''}`}>
      <div className={`database-browser-main ${compactMode ? 'compact' : ''}`}>
        <div className="database-browser-toolbar">
          <div className="database-toolbar-summary">
            <span className={`database-status-pill ${tableRows.writable ? 'database-status-live' : ''}`}>
              {tableRows.writable ? 'Editable' : 'Read only'}
            </span>
            <span className="database-meta-text">
              {safeTotalRows} rows • page {currentPage}/{totalPages}
            </span>
            <span className="database-meta-text">
              {visibleColumns.length}/{tableRows.columns.length} visible • {pinnedColumns.length} pinned
            </span>
          </div>

          <div className="database-toolbar-actions">
            <ToolbarMenu icon="fa-sliders" label="Layout">
              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Layout</span>
                <span className="database-meta-text">
                  order {hasCustomColumnOrder ? 'custom' : 'default'} • widths {hasCustomColumnWidths ? 'custom' : 'auto'} • layout {savedLayoutActive ? 'saved' : 'default'}
                </span>
              </div>

              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Presets</span>
                <select
                  className="database-browser-select"
                  value={activePresetId ?? ''}
                  onChange={(event) => {
                    const nextPresetId = event.target.value || null

                    if (!nextPresetId) {
                      persistPresetState(presets, null)
                      return
                    }

                    const preset = presets.find((entry) => entry.id === nextPresetId)
                    if (!preset) return
                    void applyPreset(preset)
                  }}
                >
                  <option value="">Current layout</option>
                  {presets.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.name}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleSavePreset()}
                  title="Save the current browser and layout state as a preset"
                >
                  <i className="fa-solid fa-bookmark"></i>
                  Save
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={handleUpdatePreset}
                  disabled={!activePresetId}
                  title="Update the selected preset with the current state"
                >
                  <i className="fa-solid fa-floppy-disk"></i>
                  Update
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleDeletePreset()}
                  disabled={!activePresetId}
                  title="Delete the selected preset"
                >
                  <i className="fa-solid fa-trash"></i>
                  Delete
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleExportPreset()}
                  title="Export the selected preset, or the current layout, as JSON"
                >
                  <i className="fa-solid fa-file-export"></i>
                  Export JSON
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleImportPreset()}
                  title="Import a preset JSON file into this table"
                >
                  <i className="fa-solid fa-file-import"></i>
                  Import JSON
                </button>
              </div>

              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Workspace library</span>
                <select
                  className="database-browser-select"
                  value={selectedWorkspacePresetId ?? ''}
                  onChange={(event) => setSelectedWorkspacePresetId(event.target.value || null)}
                  title="Choose a shared workspace preset"
                >
                  <option value="">Workspace library</option>
                  {workspaceLibraryPresets.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.name} · {preset.source.tableId}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleSaveToWorkspaceLibrary()}
                  disabled={!rootPath}
                  title="Save the current table state into the shared workspace preset library"
                >
                  <i className="fa-solid fa-layer-group"></i>
                  Save
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleApplyWorkspacePreset()}
                  disabled={!selectedWorkspacePreset}
                  title="Apply the selected shared workspace preset to this table"
                >
                  <i className="fa-solid fa-wand-magic-sparkles"></i>
                  Apply
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleDeleteWorkspacePreset()}
                  disabled={!selectedWorkspacePreset}
                  title="Delete the selected shared workspace preset"
                >
                  <i className="fa-solid fa-folder-minus"></i>
                  Delete
                </button>
              </div>

              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Columns</span>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={resetAllColumnWidths}
                  disabled={!hasCustomColumnWidths}
                  title="Reset all resized column widths for this table"
                >
                  <i className="fa-solid fa-text-width"></i>
                  Reset Widths
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={resetColumnOrder}
                  disabled={!hasCustomColumnOrder}
                  title="Reset the column order for this table"
                >
                  <i className="fa-solid fa-arrow-rotate-left"></i>
                  Reset Order
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={restoreAutoPins}
                  title="Restore the default pinned columns for this table"
                >
                  <i className="fa-solid fa-thumbtack"></i>
                  Auto Pin
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={showAllColumns}
                  disabled={hiddenColumnsCount === 0}
                  title="Show every column in this table again"
                >
                  <i className="fa-solid fa-eye"></i>
                  Show All
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => setCompactMode((current) => !current)}
                  title="Toggle compact table density"
                >
                  <i className={`fa-solid ${compactMode ? 'fa-expand' : 'fa-compress'}`}></i>
                  {compactMode ? 'Comfort' : 'Compact'}
                </button>
              </div>
            </ToolbarMenu>

            <ToolbarMenu icon="fa-file-export" label="Export">
              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Columns</span>
                <select
                  className="database-browser-select"
                  value={exportColumnMode}
                  onChange={(event) => {
                    const nextMode = event.target.value as ExportColumnMode
                    setExportColumnMode(nextMode)
                    if (nextMode !== 'selected') {
                      setShowExportColumnPicker(false)
                      return
                    }

                    setShowExportColumnPicker(true)
                    setSelectedExportColumnNames((current) => current.length > 0 ? current : visibleColumns.map((column) => column.name))
                  }}
                  title="Choose which columns should be included in exports"
                >
                  <option value="visible">Visible columns</option>
                  <option value="all">All columns</option>
                  <option value="selected">Selected columns</option>
                </select>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => setShowExportColumnPicker((current) => !current)}
                  disabled={exportColumnMode !== 'selected'}
                  title="Choose exactly which columns should be exported"
                >
                  <i className="fa-solid fa-list-check"></i>
                  {showExportColumnPicker ? 'Hide Picker' : 'Pick Columns'}
                </button>
                <span className="database-meta-text">Exporting {exportColumns.length} cols</span>
              </div>

              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Quick export</span>
                <select
                  className="database-browser-select"
                  value={quickExportFormat}
                  onChange={(event) => setQuickExportFormat(event.target.value as ExportFormat)}
                  title="Choose the format used by one-click export templates"
                >
                  <option value="xlsx">Quick XLSX</option>
                  <option value="csv">Quick CSV</option>
                </select>
                {EXPORT_TEMPLATES.map((template) => {
                  const templateColumns = getExportDescriptor(template.columnMode).columns.length
                  const isDisabled = exportingMode !== null
                    || templateColumns === 0
                    || (template.rowScope === 'page' ? tableExportRows.length === 0 : tableRows.totalRows === 0)
                  const isActive = exportingMode === `template-${template.id}-${quickExportFormat}`

                  return (
                    <button
                      key={template.id}
                      type="button"
                      className="database-secondary-btn"
                      onClick={() => void handleExportTemplate(template)}
                      disabled={isDisabled}
                      title={`One-click export ${template.label} as ${quickExportFormat.toUpperCase()}`}
                    >
                      <i className="fa-solid fa-bolt"></i>
                      {isActive ? 'Exporting...' : template.label}
                    </button>
                  )
                })}
              </div>

              <div className="database-toolbar-group">
                <span className="database-toolbar-heading">Manual export</span>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleExportCurrentPageCsv()}
                  disabled={exportColumns.length === 0 || tableExportRows.length === 0 || exportingMode !== null}
                  title="Export only the current visible table page as CSV"
                >
                  <i className="fa-solid fa-file-csv"></i>
                  Page CSV
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleExportCurrentPageXlsx()}
                  disabled={exportColumns.length === 0 || tableExportRows.length === 0 || exportingMode !== null}
                  title="Export only the current visible table page as XLSX"
                >
                  <i className="fa-solid fa-file-excel"></i>
                  Page XLSX
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleExportAllFilteredCsv()}
                  disabled={exportColumns.length === 0 || tableRows.totalRows === 0 || exportingMode !== null}
                  title="Export every row that matches the current filters and sort as CSV"
                >
                  <i className="fa-solid fa-filter"></i>
                  All CSV
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => void handleExportAllFilteredXlsx()}
                  disabled={exportColumns.length === 0 || tableRows.totalRows === 0 || exportingMode !== null}
                  title="Export every row that matches the current filters and sort as XLSX"
                >
                  <i className="fa-solid fa-filter"></i>
                  All XLSX
                </button>
              </div>
            </ToolbarMenu>

            {tableRows.writable && (
              <button type="button" className="database-primary-btn" onClick={() => openDrawer('insert', null)} disabled={isMutatingRows}>
                New Row
              </button>
            )}
          </div>
        </div>

        {exportColumnMode === 'selected' && showExportColumnPicker && (
          <div className="database-export-config">
            <div className="database-export-config-head">
              <span className="database-preview-title">Export Selected Columns</span>
              <div className="database-preset-controls">
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => setSelectedExportColumnNames(visibleColumns.map((column) => column.name))}
                >
                  <i className="fa-solid fa-table-columns"></i>
                  Use Visible
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => setSelectedExportColumnNames(orderedColumns.map((column) => column.name))}
                >
                  <i className="fa-solid fa-layer-group"></i>
                  Select All
                </button>
                <button
                  type="button"
                  className="database-secondary-btn"
                  onClick={() => setSelectedExportColumnNames([])}
                >
                  <i className="fa-solid fa-eraser"></i>
                  Clear
                </button>
              </div>
            </div>
            <div className="database-export-column-list">
              {orderedColumns.map((column) => {
                const isHidden = !visibleColumns.some((entry) => entry.name === column.name)
                const isChecked = selectedExportColumnNames.includes(column.name)

                return (
                  <label key={column.name} className={`database-export-column-option ${isChecked ? 'active' : ''}`}>
                    <input
                      type="checkbox"
                      checked={isChecked}
                      onChange={(event) => {
                        const checked = event.target.checked
                        setSelectedExportColumnNames((current) => (
                          checked
                            ? [...new Set([...current, column.name])]
                            : current.filter((name) => name !== column.name)
                        ))
                      }}
                    />
                    <span>{column.name}</span>
                    {isHidden ? <em>Hidden</em> : null}
                  </label>
                )
              })}
            </div>
          </div>
        )}

        <div className="database-browser-controls">
          <div className="database-filter-stack">
            {filterDrafts.map((filter, index) => {
              const filterNeedsColumn = filterOperatorRequiresColumn(filter.operator)
              const filterNeedsValue = filterOperatorUsesValue(filter.operator)
              const filterColumnValue = filter.column ?? '__all'

              return (
                <div key={`${index}-${filter.operator}-${filter.column ?? 'all'}`} className="database-filter-row">
                  <span className="database-filter-joiner">{index === 0 ? 'WHERE' : 'AND'}</span>
                  <select
                    className="database-browser-select"
                    value={filterColumnValue}
                    onChange={(event) => {
                      const nextColumn = event.target.value === '__all' ? undefined : event.target.value
                      setFilterDrafts((current) => current.map((entry, entryIndex) => (
                        entryIndex === index
                          ? {
                              ...entry,
                              column: nextColumn,
                              operator: nextColumn == null && filterOperatorRequiresColumn(entry.operator) ? 'contains' : entry.operator,
                            }
                          : entry
                      )))
                    }}
                  >
                    <option value="__all" disabled={filterNeedsColumn}>All columns</option>
                    {tableRows.columns.map((column) => (
                      <option key={column.name} value={column.name}>{column.name}</option>
                    ))}
                  </select>
                  <select
                    className="database-browser-select"
                    value={filter.operator}
                    onChange={(event) => {
                      const nextOperator = event.target.value as SqlFilterOperator
                      setFilterDrafts((current) => current.map((entry, entryIndex) => {
                        if (entryIndex !== index) return entry

                        return {
                          ...entry,
                          operator: nextOperator,
                          column: filterOperatorRequiresColumn(nextOperator) && !entry.column
                            ? tableRows.columns[0]?.name
                            : entry.column,
                          value: filterOperatorUsesValue(nextOperator) ? (entry.value ?? '') : '',
                        }
                      }))
                    }}
                  >
                    {FILTER_OPERATORS.map((operator) => (
                      <option key={operator.value} value={operator.value}>{operator.label}</option>
                    ))}
                  </select>
                  <input
                    className="database-browser-search"
                    value={filter.value ?? ''}
                    placeholder={filterNeedsValue ? 'Filter rows...' : 'No value needed'}
                    disabled={!filterNeedsValue}
                    onChange={(event) => {
                      const nextValue = event.target.value
                      setFilterDrafts((current) => current.map((entry, entryIndex) => (
                        entryIndex === index
                          ? {
                              ...entry,
                              value: nextValue,
                            }
                          : entry
                      )))
                    }}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        handleApplyFilter()
                      }
                    }}
                  />
                  <button
                    type="button"
                    className="database-secondary-btn"
                    onClick={() => {
                      setFilterDrafts((current) => (
                        current.length === 1
                          ? [createEmptyFilterDraft()]
                          : current.filter((_, entryIndex) => entryIndex !== index)
                      ))
                    }}
                    disabled={filterDrafts.length === 1}
                    title="Remove filter"
                  >
                    <i className="fa-solid fa-minus"></i>
                  </button>
                </div>
              )
            })}
          </div>
          <button
            type="button"
            className="database-secondary-btn"
            onClick={() => {
              setFilterDrafts((current) => [...current, createEmptyFilterDraft(tableRows.columns.length === 0 ? 'contains' : 'contains')])
            }}
          >
            <i className="fa-solid fa-plus"></i>
            Add Filter
          </button>
          <button type="button" className="database-secondary-btn" onClick={handleApplyFilter}>
            Apply Filters
          </button>
          <button type="button" className="database-secondary-btn" onClick={() => {
            setFilterDrafts([createEmptyFilterDraft()])
            void setBrowseFilters([])
          }}>
            Clear
          </button>
          <select
            className="database-browser-select"
            value={String(browse.pageSize)}
            onChange={(event) => void setBrowsePageSize(Number(event.target.value))}
          >
            {[10, 25, 50, 100].map((size) => (
              <option key={size} value={size}>{size} rows</option>
            ))}
          </select>
        </div>

        {visibleColumns.length === 0 ? (
          <div className="database-results-empty database-table-empty">
            <i className="fa-solid fa-eye-slash"></i>
            <strong>All columns are hidden</strong>
            <span>Use Show All Columns to bring the table grid back, or unhide individual columns from a saved layout reset.</span>
          </div>
        ) : safeTotalRows === 0 ? (
          <div className="database-results-empty database-table-empty">
            <i className="fa-solid fa-table-list"></i>
            <strong>No rows in this table yet</strong>
            <span>
              {tableRows.writable
                ? 'Use New Row to insert the first record, or relax the filters if this table should already contain data.'
                : 'This table is empty, or the current filters are hiding every row.'}
            </span>
          </div>
        ) : (
          <>
            <div className="database-table-wrap">
              <table className="database-table">
                <thead>
                  <tr>
                    {visibleColumns.map((column) => {
                      const isSorted = browse.sortColumn === column.name
                      const pinnedMeta = pinnedColumnMeta[column.name]
                      return (
                        <th
                          key={column.name}
                          className={pinnedMeta ? `database-pinned-cell ${pinnedMeta.isEdge ? 'edge' : ''}` : undefined}
                          style={getColumnCellStyle(column, pinnedMeta)}
                          onContextMenu={(event) => {
                            event.preventDefault()
                            event.stopPropagation()
                            setOpenColumnMenu({
                              columnName: column.name,
                              x: event.clientX,
                              y: event.clientY,
                            })
                          }}
                          title={`Right-click for ${column.name} actions`}
                        >
                          <div className="database-column-head">
                            <button
                              type="button"
                              className={`database-sort-btn ${isSorted ? 'active' : ''}`}
                              onClick={() => void setBrowseSort(
                                column.name,
                                isSorted && browse.sortDirection === 'asc' ? 'desc' : 'asc'
                              )}
                            >
                              {column.name}
                              <i className={`fa-solid ${isSorted && browse.sortDirection === 'desc' ? 'fa-sort-down' : 'fa-sort-up'}`}></i>
                            </button>
                          </div>
                          <button
                            type="button"
                            className="database-column-resize-handle"
                            onMouseDown={(event) => beginColumnResize(event, column)}
                            onDoubleClick={(event) => {
                              event.preventDefault()
                              event.stopPropagation()
                              resetColumnWidth(column.name)
                            }}
                            title={`Resize ${column.name}`}
                            aria-label={`Resize ${column.name}`}
                          />
                        </th>
                      )
                    })}
                    <th>Actions</th>
                  </tr>
                </thead>
                <tbody>
                  {tableRows.rows.map((row, rowIndex) => (
                    <tr key={`${rowIndex}-${tableRows.tableId}`} onClick={() => openDrawer('view', row)}>
                      {visibleColumns.map((column) => {
                        const pinnedMeta = pinnedColumnMeta[column.name]
                        const cellValue = String(row.values[column.name] ?? '')

                        return (
                          <td
                            key={`${rowIndex}-${column.name}`}
                            className={pinnedMeta ? `database-pinned-cell ${pinnedMeta.isEdge ? 'edge' : ''}` : undefined}
                            style={getColumnCellStyle(column, pinnedMeta)}
                            title={cellValue}
                          >
                            {cellValue}
                          </td>
                        )
                      })}
                      <td>
                        <div className="database-row-actions">
                          <button
                            type="button"
                            className="database-secondary-btn"
                            onClick={(event) => {
                              event.stopPropagation()
                              openDrawer(tableRows.writable && row.locator ? 'edit' : 'view', row)
                            }}
                          >
                            {tableRows.writable && row.locator ? 'Edit' : 'View'}
                          </button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            {totalPages > 1 && (
              <div className="database-pagination">
                <button type="button" className="database-secondary-btn" onClick={() => void setBrowsePage(0)} disabled={currentPage <= 1}>
                  First
                </button>
                <button type="button" className="database-secondary-btn" onClick={() => void setBrowsePage(browse.pageIndex - 1)} disabled={currentPage <= 1}>
                  Prev
                </button>
                <span className="database-meta-text">Page {currentPage} of {totalPages}</span>
                <button type="button" className="database-secondary-btn" onClick={() => void setBrowsePage(browse.pageIndex + 1)} disabled={currentPage >= totalPages}>
                  Next
                </button>
                <button type="button" className="database-secondary-btn" onClick={() => void setBrowsePage(totalPages - 1)} disabled={currentPage >= totalPages}>
                  Last
                </button>
              </div>
            )}
          </>
        )}
      </div>

      {openColumnMenu && menuColumn && (
        <ColumnContextMenu
          x={openColumnMenu.x}
          y={openColumnMenu.y}
          columnName={menuColumn.name}
          canMoveLeft={menuCanMoveLeft}
          canMoveRight={menuCanMoveRight}
          isPinned={Boolean(menuPinnedMeta)}
          onClose={() => setOpenColumnMenu(null)}
          onMoveLeft={() => moveColumn(menuColumn.name, 'left')}
          onMoveRight={() => moveColumn(menuColumn.name, 'right')}
          onTogglePin={() => togglePinnedColumn(menuColumn.name)}
          onHide={() => toggleHiddenColumn(menuColumn.name)}
        />
      )}

      <RowDetailDrawer
        open={drawerState != null}
        mode={drawerState?.mode ?? 'view'}
        row={drawerState?.row ?? null}
        draft={drawerState?.draft ?? {}}
        columns={tableRows.columns}
        writable={tableRows.writable}
        onClose={() => setDrawerState(null)}
        onDraftChange={(columnName, value) => {
          setDrawerState((current) => current ? {
            ...current,
            draft: {
              ...current.draft,
              [columnName]: value,
            },
          } : current)
        }}
        onSave={() => void handleSaveDrawer()}
        onDelete={() => void handleDeleteFromDrawer()}
        isBusy={isMutatingRows}
      />
    </div>
  )
}

function ResultSurface() {
  const resultMode = useSqlStore((s) => s.resultMode)
  const tableRows = useSqlStore((s) => s.tableRows)
  const isLoadingTableRows = useSqlStore((s) => s.isLoadingTableRows)

  if (resultMode === 'browser') {
    if (isLoadingTableRows) {
      return <div className="loading">Loading table rows...</div>
    }

    if (!tableRows) {
      return (
        <div className="database-results-empty">
          <i className="fa-solid fa-table"></i>
          <strong>Select a table to browse its rows</strong>
          <span>The browser will let you inspect, sort, filter, paginate, and edit writable tables.</span>
        </div>
      )
    }

    return <BrowserGrid tableRows={tableRows} />
  }

  return <QueryResultView />
}

function TableList({
  tables,
  activeTableId,
  onBrowse,
}: {
  tables: SqlTableSummary[]
  activeTableId: string | null
  onBrowse: (table: SqlTableSummary) => void
}) {
  if (tables.length === 0) {
    return (
      <div className="database-empty-inline">
        <span>No tables or views found in this SQLite file yet.</span>
      </div>
    )
  }

  return (
    <div className="database-table-list">
      {tables.map((table) => (
        <button
          key={table.id}
          type="button"
          className={`database-table-item ${table.id === activeTableId ? 'active' : ''}`}
          onClick={() => onBrowse(table)}
        >
          <span className="database-table-name">
            <i className={`fa-solid ${table.type === 'view' ? 'fa-table-cells-large' : 'fa-table'}`}></i>
            {table.name}
          </span>
        </button>
      ))}
    </div>
  )
}

function useDatabaseViewState(enableSync = false) {
  const { rootPath, revision, openFolder } = useWorkspaceStore()
  const getActiveTab = useEditorStore((s) => s.getActiveTab)
  const activeTab = getActiveTab()
  const {
    serviceStatus,
    databaseFiles,
    activeConnectionId,
    activeTableId,
    schema,
    queryResult,
    tableRows,
    resultMode,
    queryText,
    queryHistory,
    savedSnippets,
    isLoadingFiles,
    isLoadingSchema,
    isRunningQuery,
    refreshStatus,
    refreshWorkspaceDatabases,
    selectConnection,
    refreshSchema,
    setQueryText,
    saveSnippet,
    updateSnippet,
    deleteSnippet,
    clearQueryHistory,
    runQuery,
    browseTable,
  } = useSqlStore()

  useEffect(() => {
    if (!enableSync) return
    void refreshStatus()
  }, [enableSync, refreshStatus])

  useEffect(() => {
    if (!enableSync) return
    void refreshWorkspaceDatabases(rootPath)
  }, [enableSync, refreshWorkspaceDatabases, revision, rootPath])

  const activeTableColumns = activeTableId ? schema?.columnsByTable[activeTableId] ?? [] : []
  const activeTable = useMemo(
    () => schema?.tables.find((table) => table.id === activeTableId) ?? null,
    [activeTableId, schema?.tables]
  )
  const activeConnection = useMemo(
    () => databaseFiles.find((file) => file.id === activeConnectionId) ?? null,
    [activeConnectionId, databaseFiles]
  )

  const handleCreateWorkspaceDatabase = async () => {
    if (!rootPath) return

    const result = await window.electronAPI.createFile(rootPath, 'app.sqlite')
    if (!result.success || !result.path) {
      showErrorToast(result.error || 'Unable to create the SQLite file.', 'Create database failed', result.details)
      return
    }

    await refreshWorkspaceDatabases(rootPath)
    await selectConnection(`sqlite:${result.path}`)
    showSuccessToast('Created app.sqlite in the current workspace.', 'Database created')
  }

  const handleLoadActiveSqlTab = () => {
    if (!activeTab || activeTab.language !== 'sql') {
      showInfoToast('Open a .sql file first if you want to import editor content into the SQL console.', 'No active SQL tab')
      return
    }

    setQueryText(activeTab.content)
  }

  return {
    rootPath,
    openFolder,
    serviceStatus,
    databaseFiles,
    activeConnectionId,
    activeConnection,
    activeTableId,
    activeTable,
    activeTableColumns,
    schema,
    queryResult,
    tableRows,
    resultMode,
    queryText,
    queryHistory,
    savedSnippets,
    isLoadingFiles,
    isLoadingSchema,
    isRunningQuery,
    refreshWorkspaceDatabases,
    selectConnection,
    refreshSchema,
    setQueryText,
    saveSnippet,
    updateSnippet,
    deleteSnippet,
    clearQueryHistory,
    runQuery,
    browseTable,
    handleCreateWorkspaceDatabase,
    handleLoadActiveSqlTab,
  }
}

export function DatabasePanel() {
  const {
    rootPath,
    openFolder,
    serviceStatus,
    databaseFiles,
    activeConnectionId,
    activeTableId,
    activeTableColumns,
    schema,
    isLoadingFiles,
    isLoadingSchema,
    refreshWorkspaceDatabases,
    selectConnection,
    refreshSchema,
    browseTable,
    handleCreateWorkspaceDatabase,
  } = useDatabaseViewState()
  const [isSchemaCollapsed, setIsSchemaCollapsed] = useState(readSchemaCollapsedPreference)

  useEffect(() => {
    writeSchemaCollapsedPreference(isSchemaCollapsed)
  }, [isSchemaCollapsed])

  if (!rootPath) {
    return (
      <div className="database-panel database-sidebar-panel">
        <div className="sidebar-header">
          <span className="sidebar-title">DATABASE</span>
        </div>
        <DatabaseEmptyState
          title="Open a workspace to explore SQLite files"
          description="The database panel scans your current workspace for .db, .sqlite, and .sqlite3 files, then lets you query them and browse their tables."
          action={(
            <button className="open-folder-btn" onClick={openFolder}>
              <i className="fa-solid fa-folder-open"></i> Open Folder
            </button>
          )}
        />
      </div>
    )
  }

  return (
    <div className="database-panel database-sidebar-panel">
      <div className="sidebar-header">
        <span className="sidebar-title">DATABASE</span>
        <div className="sidebar-actions">
          <button type="button" onClick={() => void refreshWorkspaceDatabases(rootPath)} title="Refresh SQLite files">
            <i className={`fa-solid fa-arrow-rotate-right ${isLoadingFiles ? 'fa-spin' : ''}`}></i>
          </button>
          <button type="button" onClick={() => void refreshSchema()} title="Refresh schema" disabled={!activeConnectionId}>
            <i className={`fa-solid fa-table-list ${isLoadingSchema ? 'fa-spin' : ''}`}></i>
          </button>
        </div>
      </div>

      {databaseFiles.length === 0 && !isLoadingFiles ? (
        <DatabaseEmptyState
          title="No SQLite files found yet"
          description="Drop a database file into the workspace or create a starter one here. The console and data browser will connect as soon as the file appears."
          action={(
            <button className="open-folder-btn" onClick={() => void handleCreateWorkspaceDatabase()}>
              <i className="fa-solid fa-database"></i> Create app.sqlite
            </button>
          )}
        />
      ) : (
        <div className="database-sidebar-layout">
          <section className="database-section">
            <div className="database-section-head">
              <span>SQLite files</span>
              {serviceStatus && (
                <span className="database-status-pill">
                  {serviceStatus.features.rowEditing ? 'SQLite editable' : serviceStatus.phase}
                </span>
              )}
            </div>
            <div className="database-connection-list">
              {databaseFiles.map((file) => (
                <button
                  key={file.id}
                  type="button"
                  className={`database-connection-btn ${file.id === activeConnectionId ? 'active' : ''}`}
                  onClick={() => void selectConnection(file.id)}
                >
                  <strong>{file.name}</strong>
                  <span>{file.path.replace(rootPath, '.')}</span>
                </button>
              ))}
            </div>
          </section>

          <section className={`database-section database-schema-section ${isSchemaCollapsed ? 'is-collapsed' : ''}`}>
            <div className="database-section-head">
              <span>Schema</span>
              <div className="database-section-head-actions">
                <span className="database-meta-text">
                  {schema?.tables.length ?? 0} items
                </span>
                <button
                  type="button"
                  className="database-section-toggle"
                  aria-expanded={!isSchemaCollapsed}
                  aria-label={isSchemaCollapsed ? 'Expand schema' : 'Collapse schema'}
                  title={isSchemaCollapsed ? 'Expand schema' : 'Collapse schema'}
                  onClick={() => setIsSchemaCollapsed((current) => !current)}
                >
                  <i className={`fa-solid ${isSchemaCollapsed ? 'fa-chevron-down' : 'fa-chevron-up'}`}></i>
                </button>
              </div>
            </div>
            {!isSchemaCollapsed ? (
              <TableList
                tables={schema?.tables ?? []}
                activeTableId={activeTableId}
                onBrowse={(table) => void browseTable(table)}
              />
            ) : null}
          </section>
        </div>
      )}
    </div>
  )
}

export function DatabaseWorkspace() {
  const {
    rootPath,
    openFolder,
    activeConnectionId,
    activeConnection,
    activeTable,
    activeTableColumns,
    databaseFiles,
    queryResult,
    tableRows,
    resultMode,
    queryText,
    queryHistory,
    savedSnippets,
    isLoadingFiles,
    isRunningQuery,
    refreshSchema,
    setQueryText,
    saveSnippet,
    updateSnippet,
    deleteSnippet,
    clearQueryHistory,
    runQuery,
    handleCreateWorkspaceDatabase,
    handleLoadActiveSqlTab,
  } = useDatabaseViewState(true)
  const [selectedHistoryId, setSelectedHistoryId] = useState('')
  const [selectedSnippetId, setSelectedSnippetId] = useState('')
  const [selectedConsoleSql, setSelectedConsoleSql] = useState('')
  const consoleInputRef = useRef<HTMLTextAreaElement | null>(null)

  useEffect(() => {
    setSelectedHistoryId((current) => (
      current && queryHistory.some((entry) => entry.id === current) ? current : ''
    ))
  }, [queryHistory])

  useEffect(() => {
    setSelectedSnippetId((current) => (
      current && savedSnippets.some((entry) => entry.id === current) ? current : ''
    ))
  }, [savedSnippets])

  useEffect(() => {
    syncSelectedConsoleSql(consoleInputRef.current)
  }, [queryText])

  const selectedSnippet = useMemo(
    () => savedSnippets.find((entry) => entry.id === selectedSnippetId) ?? null,
    [savedSnippets, selectedSnippetId]
  )

  const historyOptions = useMemo(
    () => queryHistory.slice(0, 20),
    [queryHistory]
  )
  const hasSelectedConsoleSql = selectedConsoleSql.length > 0

  const syncSelectedConsoleSql = (textarea: HTMLTextAreaElement | null) => {
    setSelectedConsoleSql(getSelectedConsoleSql(textarea))
  }

  const handleRunConsoleQuery = (preferSelection = false) => {
    const nextSelectedSql = getSelectedConsoleSql(consoleInputRef.current)
    setSelectedConsoleSql(nextSelectedSql)

    if (preferSelection && nextSelectedSql.length > 0) {
      void runQuery(nextSelectedSql)
      return
    }

    void runQuery()
  }

  const handleSaveSnippet = () => {
    const suggestedName = selectedSnippet?.name ?? `Snippet ${savedSnippets.length + 1}`
    const snippetName = window.prompt('Save SQL snippet as:', suggestedName)?.trim()
    if (!snippetName) return

    const snippet = saveSnippet(snippetName, queryText)
    if (!snippet) {
      showInfoToast('Type a SQL query before saving a snippet.', 'Nothing to save')
      return
    }

    setSelectedSnippetId(snippet.id)
    showSuccessToast(`Saved snippet "${snippet.name}".`, 'Snippet saved')
  }

  const handleUpdateSnippet = () => {
    if (!selectedSnippet) return

    const nextName = window.prompt('Update snippet name:', selectedSnippet.name)?.trim()
    if (!nextName) return

    const snippet = updateSnippet(selectedSnippet.id, nextName, queryText)
    if (!snippet) {
      showInfoToast('Type a SQL query before updating this snippet.', 'Nothing to save')
      return
    }

    setSelectedSnippetId(snippet.id)
    showSuccessToast(`Updated snippet "${snippet.name}".`, 'Snippet updated')
  }

  const handleDeleteSnippet = async () => {
    if (!selectedSnippet) return

    const confirmed = await confirmAction({
      title: 'Delete this snippet?',
      message: `Remove "${selectedSnippet.name}" from saved SQL snippets?`,
      confirmLabel: 'Delete snippet',
      cancelLabel: 'Keep snippet',
      tone: 'warning',
    })

    if (!confirmed) return

    deleteSnippet(selectedSnippet.id)
    setSelectedSnippetId('')
    showSuccessToast(`Deleted snippet "${selectedSnippet.name}".`, 'Snippet deleted')
  }

  const handleClearHistory = async () => {
    if (queryHistory.length === 0) return

    const confirmed = await confirmAction({
      title: 'Clear SQL query history?',
      message: 'This removes the saved run history from the SQL console on this machine.',
      confirmLabel: 'Clear history',
      cancelLabel: 'Keep history',
      tone: 'warning',
    })

    if (!confirmed) return

    clearQueryHistory()
    setSelectedHistoryId('')
    showSuccessToast('Cleared the SQL query history.', 'History cleared')
  }

  if (!rootPath) {
    return (
      <div className="database-workspace">
        <DatabaseEmptyState
          title="Open a workspace to inspect databases"
          description="Use the activity bar to enter database mode, then browse tables and run SQL in the main editor area."
          action={(
            <button className="open-folder-btn" onClick={openFolder}>
              <i className="fa-solid fa-folder-open"></i> Open Folder
            </button>
          )}
        />
      </div>
    )
  }

  if (databaseFiles.length === 0 && !isLoadingFiles) {
    return (
      <div className="database-workspace">
        <DatabaseEmptyState
          title="No SQLite files found yet"
          description="Create a workspace database first, then the preview surface here will show live table data and query results."
          action={(
            <button className="open-folder-btn" onClick={() => void handleCreateWorkspaceDatabase()}>
              <i className="fa-solid fa-database"></i> Create app.sqlite
            </button>
          )}
        />
      </div>
    )
  }

  if (!activeConnectionId) {
    return (
      <div className="database-workspace">
        <div className="database-results-empty">
          <i className="fa-solid fa-database"></i>
          <strong>Select a database from the left sidebar</strong>
          <span>The editor area will turn into a table preview and SQL surface as soon as you choose one.</span>
        </div>
      </div>
    )
  }

  return (
    <div className="database-workspace">
      <div className="database-workspace-header">
        <div className="database-workspace-copy">
          <span className="panel-empty-eyebrow">Database Workspace</span>
          <div className="database-workspace-title-row">
            <strong>{activeTable ? activeTable.name : activeConnection?.name ?? 'SQLite'}</strong>
            {activeTable && <span className="database-status-pill">{activeTable.type}</span>}
            <span
              className="database-preview-meta"
              title={
                resultMode === 'browser'
                  ? activeTable
                    ? `${activeTable.name} • ${activeTable.type} • ${tableRows?.totalRows ?? 0} rows`
                    : 'Ready'
                  : queryResult
                    ? `${queryResult.rowCount} rows • ${queryResult.durationMs} ms`
                    : 'Ready'
              }
            >
              {resultMode === 'browser'
                ? activeTable
                  ? `${activeTable.name} • ${activeTable.type} • ${tableRows?.totalRows ?? 0} rows`
                  : 'Ready'
                : queryResult
                  ? `${queryResult.rowCount} rows • ${queryResult.durationMs} ms`
                  : 'Ready'}
            </span>
          </div>
          <span className="database-meta-text">
            {activeTable
              ? `${activeConnection?.name ?? 'SQLite'} • ${activeTable.type}`
              : `${activeConnection?.name ?? 'SQLite'} • Ready to query`}
          </span>
        </div>
        <div className="database-console-actions">
          <button type="button" className="database-primary-btn" onClick={() => handleRunConsoleQuery(false)} disabled={isRunningQuery || !activeConnectionId}>
            <i className="fa-solid fa-play"></i>
            {isRunningQuery ? 'Running...' : 'Run Query'}
          </button>
          {hasSelectedConsoleSql && (
            <button
              type="button"
              className="database-secondary-btn"
              onClick={() => handleRunConsoleQuery(true)}
              disabled={isRunningQuery || !activeConnectionId}
              title="Run only the SQL text currently highlighted in the console"
            >
              <i className="fa-solid fa-i-cursor"></i>
              Run Selected
            </button>
          )}
          <ToolbarMenu icon="fa-ellipsis" label="Tools">
            <div className="database-toolbar-group">
              <span className="database-toolbar-heading">SQL console</span>
              <button type="button" className="database-secondary-btn" onClick={handleLoadActiveSqlTab}>
                <i className="fa-solid fa-file-lines"></i>
                Load SQL Tab
              </button>
              <button type="button" className="database-secondary-btn" onClick={() => void refreshSchema()} disabled={!activeConnectionId}>
                <i className="fa-solid fa-arrows-rotate"></i>
                Refresh Schema
              </button>
            </div>
          </ToolbarMenu>
        </div>
      </div>

      <div className="database-workspace-main">
        <section className="database-results-panel">
          <div className="database-section-head database-preview-head">
            <span className="database-preview-title">{resultMode === 'browser' ? 'Table Preview' : 'Query Results'}</span>
          </div>
          <ResultSurface />
        </section>

        <section className="database-console-panel">
          <div className="database-section-head database-console-head">
            <div className="database-console-head-copy">
              <span>SQL console</span>
              <span className="database-meta-text">
                {hasSelectedConsoleSql
                  ? `Ctrl+Enter runs the ${selectedConsoleSql.length}-char selection`
                  : 'Ctrl+Enter runs the full query'}
              </span>
            </div>
            <div className="database-console-head-actions">
              <span className="database-meta-text">
                {savedSnippets.length} snippets • {queryHistory.length} history items
              </span>
              <ToolbarMenu icon="fa-clock-rotate-left" label="History">
                <div className="database-toolbar-group">
                  <span className="database-toolbar-heading">Recent queries</span>
                  <select
                    className="database-browser-select database-console-select"
                    value={selectedHistoryId}
                    onChange={(event) => {
                      const nextId = event.target.value
                      setSelectedHistoryId(nextId)
                      const entry = queryHistory.find((historyEntry) => historyEntry.id === nextId)
                      if (entry) {
                        setQueryText(entry.sql)
                      }
                    }}
                  >
                    <option value="">Recent queries</option>
                    {historyOptions.map((entry) => (
                      <option key={entry.id} value={entry.id}>
                        {`${entry.success ? 'OK' : 'ERR'} • ${getQueryPreview(entry.sql, 56)}`}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="database-secondary-btn"
                    onClick={() => void handleClearHistory()}
                    disabled={queryHistory.length === 0}
                  >
                    <i className="fa-solid fa-trash-can"></i>
                    Clear History
                  </button>
                </div>
              </ToolbarMenu>
              <ToolbarMenu icon="fa-bookmark" label="Snippets">
                <div className="database-toolbar-group">
                  <span className="database-toolbar-heading">Saved snippets</span>
                  <select
                    className="database-browser-select database-console-select"
                    value={selectedSnippetId}
                    onChange={(event) => {
                      const nextId = event.target.value
                      setSelectedSnippetId(nextId)
                      const snippet = savedSnippets.find((entry) => entry.id === nextId)
                      if (snippet) {
                        setQueryText(snippet.sql)
                      }
                    }}
                  >
                    <option value="">Saved snippets</option>
                    {savedSnippets.map((snippet) => (
                      <option key={snippet.id} value={snippet.id}>
                        {snippet.name}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="database-secondary-btn"
                    onClick={handleSaveSnippet}
                    disabled={queryText.trim().length === 0}
                  >
                    <i className="fa-solid fa-bookmark"></i>
                    Save
                  </button>
                  <button
                    type="button"
                    className="database-secondary-btn"
                    onClick={handleUpdateSnippet}
                    disabled={!selectedSnippet || queryText.trim().length === 0}
                  >
                    <i className="fa-solid fa-pen-to-square"></i>
                    Update
                  </button>
                  <button
                    type="button"
                    className="database-secondary-btn"
                    onClick={() => void handleDeleteSnippet()}
                    disabled={!selectedSnippet}
                  >
                    <i className="fa-solid fa-trash"></i>
                    Delete
                  </button>
                </div>
              </ToolbarMenu>
            </div>
          </div>
          <textarea
            ref={consoleInputRef}
            className="database-console-input"
            value={queryText}
            onChange={(event) => {
              setQueryText(event.target.value)
              syncSelectedConsoleSql(event.target)
            }}
            onSelect={(event) => syncSelectedConsoleSql(event.currentTarget)}
            onClick={(event) => syncSelectedConsoleSql(event.currentTarget)}
            onKeyUp={(event) => syncSelectedConsoleSql(event.currentTarget)}
            onKeyDown={(event) => {
              if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
                event.preventDefault()
                handleRunConsoleQuery(true)
              }
            }}
            spellCheck={false}
            placeholder="SELECT * FROM users LIMIT 100;"
          />
        </section>
      </div>
    </div>
  )
}
