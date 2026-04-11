import Box from '@mui/material/Box';
import Chip from '@mui/material/Chip';
import Typography from '@mui/material/Typography';
import { useActivePrintingList, useUpsertPrintingListItem } from '../lib/queries';
import styles from './PrintingListControls.module.css';

interface Props {
  modelId: string;
  showButtons?: boolean;
}

export default function PrintingListControls({ modelId, showButtons = true }: Props) {
  const { data: activeList } = useActivePrintingList();
  const { mutate: upsert } = useUpsertPrintingListItem();

  const activeListId = activeList?.id ?? '';
  const listItem = activeList?.items.find((i) => i.modelId === modelId);
  const quantity = listItem?.quantity;

  return (
    <Box className={styles.container}>
      <Box
        component="button"
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          upsert({ listId: activeListId, modelId, quantity: (quantity ?? 0) - 1 });
        }}
        className={`${styles.btn}${!showButtons ? ` ${styles.btnHidden}` : ''}`}
      >
        −
      </Box>

      {quantity == null ? null : quantity > 0 ? (
        <Chip label={`×${quantity}`} size="small" className={styles.chip} />
      ) : (
        <Typography className={styles.count}>×0</Typography>
      )}

      <Box
        component="button"
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          upsert({ listId: activeListId, modelId, quantity: (quantity ?? 0) + 1 });
        }}
        className={`${styles.btn}${!showButtons ? ` ${styles.btnHidden}` : ''}`}
      >
        +
      </Box>
    </Box>
  );
}
