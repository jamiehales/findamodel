import React, { createContext, useContext, useState, useCallback } from 'react'

export type PrintingList = Record<string, number> // modelId → quantity

interface PrintingListContextValue {
  items: PrintingList
  addItem: (id: string) => void
  removeItem: (id: string) => void
  setQuantity: (id: string, qty: number) => void
  clearList: () => void
  totalCount: number
}

const STORAGE_KEY = 'findamodel.printingList'

function load(): PrintingList {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? (JSON.parse(raw) as PrintingList) : {}
  } catch {
    return {}
  }
}

function save(items: PrintingList) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(items))
}

const PrintingListContext = createContext<PrintingListContextValue | null>(null)

export function PrintingListProvider({ children }: { children: React.ReactNode }) {
  const [items, setItems] = useState<PrintingList>(load)

  const addItem = useCallback((id: string) => {
    setItems(prev => {
      const next = { ...prev, [id]: 1 }
      save(next)
      return next
    })
  }, [])

  const removeItem = useCallback((id: string) => {
    setItems(prev => {
      const next = { ...prev }
      delete next[id]
      save(next)
      return next
    })
  }, [])

  const setQuantity = useCallback((id: string, qty: number) => {
    setItems(prev => {
      const next = { ...prev }
      if (qty <= 0) {
        delete next[id]
      } else {
        next[id] = qty
      }
      save(next)
      return next
    })
  }, [])

  const clearList = useCallback(() => {
    setItems({})
    save({})
  }, [])

  const totalCount = Object.values(items).reduce((a, b) => a + b, 0)

  return (
    <PrintingListContext.Provider value={{ items, addItem, removeItem, setQuantity, clearList, totalCount }}>
      {children}
    </PrintingListContext.Provider>
  )
}

export function usePrintingList() {
  const ctx = useContext(PrintingListContext)
  if (!ctx) throw new Error('usePrintingList must be used within PrintingListProvider')
  return ctx
}
