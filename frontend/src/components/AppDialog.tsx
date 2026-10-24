import type { ReactNode } from 'react';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
  Sheet,
  SheetContent,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from '@jamiehales/ui-kit/mostlymaking';
import styles from './AppDialog.module.css';

interface AppDialogProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  actions?: ReactNode;
  /** Panel width; `sm` is the kit's confirm size. */
  size?: 'sm' | 'md' | 'lg';
}

/** A centred dialog for previews and help text. */
export default function AppDialog({
  open,
  onClose,
  title,
  children,
  actions,
  size = 'md',
}: AppDialogProps) {
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className={styles[size]} aria-describedby={undefined}>
        <DialogTitle>{title}</DialogTitle>
        <div className={styles.body}>{children}</div>
        {actions ? <DialogFooter>{actions}</DialogFooter> : null}
      </DialogContent>
    </Dialog>
  );
}

interface AppSheetProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  side?: 'left' | 'right';
}

/** A panel docked to an edge: the right for editing forms, the left for phone filters. */
export function AppSheet({
  open,
  onClose,
  title,
  children,
  footer,
  side = 'right',
}: AppSheetProps) {
  return (
    <Sheet open={open} onOpenChange={(next) => !next && onClose()}>
      <SheetContent side={side} className={styles.sheet} aria-describedby={undefined}>
        <SheetHeader>
          <SheetTitle>{title}</SheetTitle>
        </SheetHeader>
        <div className={styles.sheetBody}>{children}</div>
        {footer ? <SheetFooter>{footer}</SheetFooter> : null}
      </SheetContent>
    </Sheet>
  );
}
