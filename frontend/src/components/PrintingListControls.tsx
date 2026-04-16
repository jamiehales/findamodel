import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import Chip from '@mui/material/Chip';
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
  const quantity = listItem?.quantity ?? 0;

  function handleUpdate(e: React.MouseEvent, nextQuantity: number) {
    e.preventDefault();
    e.stopPropagation();
    upsert({ listId: activeListId, modelId, quantity: nextQuantity });
  }

  return (
    <Box className={`${styles.container}${quantity === 0 ? ` ${styles.addOnly}` : ''}`}>
      {quantity > 0 ? (
        <>
          <Button
            variant="outlined"
            size="small"
            onClick={(e) => handleUpdate(e, quantity - 1)}
            className={`${styles.btn}${!showButtons ? ` ${styles.btnHidden}` : ''}`}
            aria-label="Decrease quantity"
          >
            −
          </Button>

          <Chip label={`×${quantity}`} size="small" variant="outlined" className={styles.chip} />

          <Button
            variant="outlined"
            size="small"
            onClick={(e) => handleUpdate(e, quantity + 1)}
            className={`${styles.btn}${!showButtons ? ` ${styles.btnHidden}` : ''}`}
            aria-label="Increase quantity"
          >
            +
          </Button>
        </>
      ) : (
        <Button
          variant="outlined"
          size="small"
          onClick={(e) => handleUpdate(e, 1)}
          className={`${styles.addBtn}${!showButtons ? ` ${styles.btnHidden}` : ''}`}
        >
          Add to printing list
        </Button>
      )}
    </Box>
  );
}
