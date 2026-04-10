import type { ReactNode } from 'react';
import Dialog, { type DialogProps } from '@mui/material/Dialog';
import DialogActions from '@mui/material/DialogActions';
import DialogContent from '@mui/material/DialogContent';
import DialogTitle from '@mui/material/DialogTitle';

interface AppDialogProps {
  open: boolean;
  onClose: DialogProps['onClose'];
  title: ReactNode;
  children: ReactNode;
  actions?: ReactNode;
  maxWidth?: DialogProps['maxWidth'];
  fullWidth?: boolean;
  scroll?: DialogProps['scroll'];
  contentDividers?: boolean;
}

export default function AppDialog({
  open,
  onClose,
  title,
  children,
  actions,
  maxWidth = 'sm',
  fullWidth = false,
  scroll,
  contentDividers = false,
}: AppDialogProps) {
  return (
    <Dialog open={open} onClose={onClose} maxWidth={maxWidth} fullWidth={fullWidth} scroll={scroll}>
      <DialogTitle>{title}</DialogTitle>
      <DialogContent dividers={contentDividers}>{children}</DialogContent>
      {actions ? <DialogActions>{actions}</DialogActions> : null}
    </Dialog>
  );
}
