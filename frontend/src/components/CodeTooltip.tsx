import Tooltip from '@mui/material/Tooltip';
import type { TooltipProps } from '@mui/material/Tooltip';
import styles from './CodeTooltip.module.css';

interface CodeTooltipProps {
  code: string;
  placement?: TooltipProps['placement'];
  children: React.ReactElement;
}

export default function CodeTooltip({ code, placement = 'top', children }: CodeTooltipProps) {
  return (
    <Tooltip title={<pre className={styles.pre}>{code}</pre>} placement={placement} arrow>
      {children}
    </Tooltip>
  );
}
