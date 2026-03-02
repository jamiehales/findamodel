import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { useActivePrintingList, useUpsertPrintingListItem } from '../lib/queries'
import styles from './PrintingListControls.module.css'

interface Props {
  modelId: string
}

export default function PrintingListControls({ modelId }: Props) {
  const { data: activeList } = useActivePrintingList()
  const { mutate: upsert } = useUpsertPrintingListItem()

  const activeListId = activeList?.id ?? ''
  const quantity = activeList?.items.find(i => i.modelId === modelId)?.quantity

  return (
    <Box className={styles.container}>
      <Box
        component="button"
        onClick={e => { e.preventDefault(); e.stopPropagation(); upsert({ listId: activeListId, modelId, quantity: (quantity ?? 0) - 1 }) }}
        className={styles.btn}
      >
        −
      </Box>

      <Typography
        className={styles.count}
        style={{ visibility: quantity !== undefined ? 'visible' : 'hidden' }}
      >
        ×{quantity ?? 0}
      </Typography>

      <Box
        component="button"
        onClick={e => { e.preventDefault(); e.stopPropagation(); upsert({ listId: activeListId, modelId, quantity: (quantity ?? 0) + 1 }) }}
        className={styles.btn}
      >
        +
      </Box>
    </Box>
  )
}
