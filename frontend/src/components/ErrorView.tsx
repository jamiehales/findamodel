import styles from './StatusView.module.css';

interface ErrorViewProps {
  message?: string;
  minHeight?: string | number;
}

export default function ErrorView({
  message = 'Something went wrong.',
  minHeight = '30vh',
}: ErrorViewProps) {
  return (
    <div className={styles.center} style={{ minHeight }}>
      <p role="alert" className={styles.error}>
        {message}
      </p>
    </div>
  );
}
