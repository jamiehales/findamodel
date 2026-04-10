import Button from '@mui/material/Button';
import Typography from '@mui/material/Typography';
import AppDialog from './AppDialog';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  pending?: boolean;
}

export default function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  pending = false,
}: ConfirmDialogProps) {
  return (
    <AppDialog
      open={open}
      onClose={onCancel}
      title={title}
      actions={[
        <Button key="confirm" onClick={onConfirm} variant="warning" disabled={pending}>
          {confirmLabel}
        </Button>,
        <Button key="cancel" onClick={onCancel} variant="contained" disabled={pending}>
          Cancel
        </Button>,
      ]}
    >
      <Typography>{message}</Typography>
    </AppDialog>
  );
}
