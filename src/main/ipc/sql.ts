import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants'
import type { SqlBrowseTableRequest, SqlDeleteRowRequest, SqlInsertRowRequest, SqlQueryRequest, SqlUpdateRowRequest } from '@shared/types/sql'
import { sqlService } from '../services/sql'

export function registerSqlHandlers() {
  ipcMain.handle(IPC.SQL_GET_STATUS, () => sqlService.getStatus())
  ipcMain.handle(IPC.SQL_EXECUTE_QUERY, (_event, request: SqlQueryRequest) => sqlService.executeQuery(request))
  ipcMain.handle(IPC.SQL_GET_SCHEMA, (_event, connectionId: string) => sqlService.getSchema(connectionId))
  ipcMain.handle(IPC.SQL_GET_TABLE_ROWS, (_event, request: SqlBrowseTableRequest) => sqlService.getTableRows(request))
  ipcMain.handle(IPC.SQL_UPDATE_ROW, (_event, request: SqlUpdateRowRequest) => sqlService.updateRow(request))
  ipcMain.handle(IPC.SQL_INSERT_ROW, (_event, request: SqlInsertRowRequest) => sqlService.insertRow(request))
  ipcMain.handle(IPC.SQL_DELETE_ROW, (_event, request: SqlDeleteRowRequest) => sqlService.deleteRow(request))
}
