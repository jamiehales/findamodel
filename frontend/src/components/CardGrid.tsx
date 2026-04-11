import styles from './CardGrid.module.css';

export const DEFAULT_CARD_MIN_WIDTH_PX = 240;

interface CardGridProps {
  children: React.ReactNode;
  minCardWidth?: number; // px; defaults to DEFAULT_CARD_MIN_WIDTH_PX
}

export default function CardGrid({
  children,
  minCardWidth = DEFAULT_CARD_MIN_WIDTH_PX,
}: CardGridProps) {
  return (
    <div
      className={styles.grid}
      style={{ '--card-min-width': `${minCardWidth}px` } as React.CSSProperties}
    >
      {children}
    </div>
  );
}
