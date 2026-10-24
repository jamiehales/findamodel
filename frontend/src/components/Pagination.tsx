import { ChevronLeft, ChevronRight } from 'lucide-react';
import { Button, IconButton } from '@jamiehales/ui-kit/mostlymaking';
import styles from './Pagination.module.css';

interface Props {
  page: number;
  totalPages: number;
  onPageChange: (page: number) => void;
  /** Mono summary on the left, e.g. "1–24 of 2,418". */
  summary?: string;
}

function pageWindow(page: number, total: number): (number | 'gap')[] {
  const pages = new Set([1, total, page - 1, page, page + 1]);
  const sorted = [...pages].filter((p) => p >= 1 && p <= total).sort((a, b) => a - b);
  const out: (number | 'gap')[] = [];
  for (const p of sorted) {
    const prev = out[out.length - 1];
    if (typeof prev === 'number' && p - prev > 1) out.push('gap');
    out.push(p);
  }
  return out;
}

export default function Pagination({ page, totalPages, onPageChange, summary }: Props) {
  if (totalPages <= 1 && !summary) return null;
  return (
    <nav className={styles.pager} aria-label="Pages">
      {summary ? <span className={styles.summary}>{summary}</span> : <span />}
      {totalPages > 1 && (
        <div className={styles.pages}>
          <IconButton
            variant="ghost"
            size="sm"
            aria-label="Previous page"
            disabled={page <= 1}
            onClick={() => onPageChange(page - 1)}
          >
            <ChevronLeft size={16} strokeWidth={1.5} />
          </IconButton>
          {pageWindow(page, totalPages).map((p, i) =>
            p === 'gap' ? (
              <span key={`gap-${i}`} className={styles.gap} aria-hidden="true">
                …
              </span>
            ) : (
              <Button
                key={p}
                size="sm"
                variant={p === page ? 'secondary' : 'ghost'}
                aria-current={p === page ? 'page' : undefined}
                onClick={() => onPageChange(p)}
                className={styles.page}
              >
                {p}
              </Button>
            ),
          )}
          <IconButton
            variant="ghost"
            size="sm"
            aria-label="Next page"
            disabled={page >= totalPages}
            onClick={() => onPageChange(page + 1)}
          >
            <ChevronRight size={16} strokeWidth={1.5} />
          </IconButton>
        </div>
      )}
    </nav>
  );
}
