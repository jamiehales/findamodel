import { Spinner } from '@jamiehales/ui-kit/mostlymaking';
import styles from './StatusView.module.css';

interface LoadingViewProps {
  minHeight?: string | number;
}

export default function LoadingView({ minHeight = '30vh' }: LoadingViewProps) {
  return (
    <div className={styles.center} style={{ minHeight }}>
      <Spinner size="lg" className={styles.spinner} />
    </div>
  );
}
