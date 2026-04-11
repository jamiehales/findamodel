import styles from './CardGrid.module.css';

interface CardGridProps {
  children: React.ReactNode;
  minCardWidth?: number; // px; defaults to 240
}

export default function CardGrid({ children, minCardWidth = 240 }: CardGridProps) {
  return (
    <div
      className={styles.grid}
      style={{ '--card-min-width': `${minCardWidth}px` } as React.CSSProperties}
    >
      {children}
    </div>
  );
}
