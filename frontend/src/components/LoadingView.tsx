import CircularProgress from '@mui/material/CircularProgress';
import Stack from '@mui/material/Stack';

interface LoadingViewProps {
  minHeight?: string | number;
}

export default function LoadingView({ minHeight = '30vh' }: LoadingViewProps) {
  return (
    <Stack alignItems="center" justifyContent="center" sx={{ minHeight }}>
      <CircularProgress color="primary" />
    </Stack>
  );
}
