import React, { createContext, useContext, useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { useActivePrintingList, queryKeys } from './queries'
import { clearPrintingListItems, upsertPrintingListItem } from './api'

export type PrintingList = Record<string, number> // modelId → quantity

interface PrintingListContextValue {
  activeListId: string | null
  items: PrintingList
  addItem: (id: string) => void
  removeItem: (id: string) => void
  setQuantity: (id: string, qty: number) => void
  clearList: () => void
  totalCount: number
}

const PrintingListContext = createContext<PrintingListContextValue | null>(null)

export function PrintingListProvider({ children }: { children: React.ReactNode }) {
  const { data: activeList } = useActivePrintingList()
  const queryClient = useQueryClient()

  const activeListId = activeList?.id ?? null

  const items: PrintingList = React.useMemo(() => {
    if (!activeList) return {}
    return Object.fromEntries(activeList.items.map(i => [i.modelId, i.quantity]))
  }, [activeList])

  const setQuantity = useCallback((modelId: string, qty: number) => {
    if (!activeListId) return
    upsertPrintingListItem(activeListId, modelId, qty).then(updated => {
      queryClient.setQueryData(queryKeys.activePrintingList, updated)
      queryClient.setQueryData(queryKeys.printingList(updated.id), updated)
    })
  }, [activeListId, queryClient])

  const addItem = useCallback((id: string) => {
    setQuantity(id, (items[id] ?? 0) + 1)
  }, [setQuantity, items])

  const removeItem = useCallback((id: string) => {
    setQuantity(id, 0)
  }, [setQuantity])

  const clearList = useCallback(() => {
    if (!activeListId) return
    clearPrintingListItems(activeListId).then(updated => {
      queryClient.setQueryData(queryKeys.activePrintingList, updated)
      queryClient.setQueryData(queryKeys.printingList(updated.id), updated)
    })
  }, [activeListId, queryClient])

  const totalCount = Object.values(items).reduce((a, b) => a + b, 0)

  return (
    <PrintingListContext.Provider value={{ activeListId, items, addItem, removeItem, setQuantity, clearList, totalCount }}>
      {children}
    </PrintingListContext.Provider>
  )
}

export function usePrintingList() {
  const ctx = useContext(PrintingListContext)
  if (!ctx) throw new Error('usePrintingList must be used within PrintingListProvider')
  return ctx
}
