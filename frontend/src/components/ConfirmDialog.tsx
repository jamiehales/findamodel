import { ConfirmDialog as KitConfirmDialog } from '@jamiehales/ui-kit/mostlymaking';

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  message?: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  pending?: boolean;
  destructive?: boolean;
}

export default function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  pending = false,
  destructive = true,
}: ConfirmDialogProps) {
  return (
    <KitConfirmDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onCancel();
      }}
      title={title}
      description={message}
      confirmLabel={confirmLabel}
      destructive={destructive}
      confirmDisabled={pending}
      onConfirm={(event) => {
        event.preventDefault();
        onConfirm();
      }}
    />
  );
}
