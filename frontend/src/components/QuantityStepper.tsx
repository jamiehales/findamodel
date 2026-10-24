import { Minus, Plus } from 'lucide-react';
import styles from './QuantityStepper.module.css';

interface Props {
  value: number;
  onChange: (next: number) => void;
  /** Read out with the buttons: "Dwarf ranger". */
  label?: string;
  size?: 'sm' | 'md';
  min?: number;
}

/** Minus, count, plus; used for printing list quantities. */
export default function QuantityStepper({ value, onChange, label, size = 'md', min = 0 }: Props) {
  const stop = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };
  return (
    <div
      className={styles.stepper}
      data-size={size}
      role="group"
      aria-label={label ? `Quantity of ${label}` : 'Quantity'}
    >
      <button
        type="button"
        aria-label="Decrease quantity"
        disabled={value <= min}
        onClick={(e) => {
          stop(e);
          onChange(value - 1);
        }}
      >
        <Minus size={14} strokeWidth={2} />
      </button>
      <span aria-live="polite">{value}</span>
      <button
        type="button"
        aria-label="Increase quantity"
        onClick={(e) => {
          stop(e);
          onChange(value + 1);
        }}
      >
        <Plus size={14} strokeWidth={2} />
      </button>
    </div>
  );
}
