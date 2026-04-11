import Stack from '@mui/material/Stack';
import type { StackProps } from '@mui/material/Stack';
import styles from './PageLayout.module.css';

type PageVariant = 'full' | 'narrow' | 'medium';

interface PageLayoutProps extends Omit<StackProps, 'className'> {
  variant?: PageVariant;
}

export default function PageLayout({ variant = 'full', children, ...rest }: PageLayoutProps) {
  const cls = [styles.page, styles[variant]].join(' ');
  return (
    <Stack direction="column" className={cls} {...rest}>
      {children}
    </Stack>
  );
}
